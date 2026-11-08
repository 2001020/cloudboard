//! Background PDF renderer. PDFium is not thread-safe, so a single dedicated
//! thread owns the library and the document, executing small tile jobs in
//! priority order. The UI thread never waits for a render.

use crate::cache::{TileKey, TileReq, TILE};
use crate::ffi::{self, Api, FS_MATRIX, FS_RECTF, FS_SIZEF};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use tiny_skia::Pixmap;

#[derive(Debug, thiserror::Error, Clone)]
pub enum PdfError {
    #[error("{0}")]
    Library(String),
    #[error("无法打开 PDF（错误代码 {0}）")]
    Open(u64),
    #[error("PDF 需要密码")]
    Password,
    #[error("PDF 渲染线程已停止")]
    Stopped,
}

#[derive(Debug, Clone)]
pub struct DocInfo {
    pub doc_id: u64,
    /// Page sizes in PDF points.
    pub pages: Vec<(f32, f32)>,
}

pub struct TileResult {
    pub doc_id: u64,
    pub key: TileKey,
    pub pixmap: Pixmap,
    pub render_ms: f32,
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

enum Cmd {
    Open { bytes: Arc<Vec<u8>>, reply: Sender<Result<DocInfo, PdfError>> },
    Close,
}

#[derive(Default)]
struct Queue {
    cmds: VecDeque<Cmd>,
    jobs: Vec<TileReq>,
    doc_id: u64,
    shutdown: bool,
}

struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

pub struct PdfEngine {
    shared: Arc<Shared>,
    results: Receiver<TileResult>,
    thread: Option<JoinHandle<()>>,
    doc_id: u64,
}

/// Find the PDFium library: next to the executable, in a `lib` subfolder or
/// (macOS) the app bundle's Frameworks folder; `CLOUDBOARD_PDFIUM` overrides.
pub fn locate_library() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLOUDBOARD_PDFIUM") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = ffi::library_file_name();
    [dir.join(name), dir.join("lib").join(name), dir.join("../Frameworks").join(name)].into_iter().find(|p| p.exists())
}

impl PdfEngine {
    pub fn start(lib: &Path, waker: Waker) -> Result<PdfEngine, PdfError> {
        let shared = Arc::new(Shared { q: Mutex::new(Queue::default()), cv: Condvar::new() });
        let (rtx, rrx) = channel();
        let (init_tx, init_rx) = channel();
        let s2 = shared.clone();
        let lib = lib.to_path_buf();
        let thread = std::thread::Builder::new()
            .name("cb-pdf".into())
            .spawn(move || {
                let api = match unsafe { Api::load(&lib) } {
                    Ok(a) => {
                        let _ = init_tx.send(Ok(()));
                        a
                    }
                    Err(e) => {
                        let _ = init_tx.send(Err(PdfError::Library(e)));
                        return;
                    }
                };
                worker(api, s2, rtx, waker);
            })
            .map_err(|e| PdfError::Library(e.to_string()))?;
        init_rx.recv().map_err(|_| PdfError::Stopped)??;
        Ok(PdfEngine { shared, results: rrx, thread: Some(thread), doc_id: 0 })
    }

    /// Open a document (replacing the current one). Blocks briefly while
    /// PDFium parses the cross-reference table and page sizes.
    pub fn open(&mut self, bytes: Arc<Vec<u8>>) -> Result<DocInfo, PdfError> {
        let (tx, rx) = channel();
        {
            let mut q = self.shared.q.lock().unwrap();
            q.jobs.clear();
            q.cmds.push_back(Cmd::Open { bytes, reply: tx });
        }
        self.shared.cv.notify_all();
        let info = rx.recv().map_err(|_| PdfError::Stopped)??;
        self.doc_id = info.doc_id;
        Ok(info)
    }

    pub fn close(&mut self) {
        let mut q = self.shared.q.lock().unwrap();
        q.jobs.clear();
        q.cmds.push_back(Cmd::Close);
        drop(q);
        self.shared.cv.notify_all();
        self.doc_id = 0;
        while self.results.try_recv().is_ok() {}
    }

    /// Replace the pending job list (anything not listed is cancelled).
    pub fn set_wanted(&self, mut jobs: Vec<TileReq>) {
        // Highest priority (lowest value) at the back for O(1) pop.
        jobs.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal));
        let mut q = self.shared.q.lock().unwrap();
        q.jobs = jobs;
        drop(q);
        self.shared.cv.notify_all();
    }

    pub fn pending(&self) -> usize {
        self.shared.q.lock().unwrap().jobs.len()
    }

    pub fn try_recv(&self) -> Option<TileResult> {
        loop {
            let r = self.results.try_recv().ok()?;
            if r.doc_id == self.doc_id {
                return Some(r);
            }
        }
    }

    pub fn doc_id(&self) -> u64 {
        self.doc_id
    }
}

impl Drop for PdfEngine {
    fn drop(&mut self) {
        {
            let mut q = self.shared.q.lock().unwrap();
            q.shutdown = true;
            q.jobs.clear();
        }
        self.shared.cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct OpenDoc {
    handle: ffi::FPDF_DOCUMENT,
    _bytes: Arc<Vec<u8>>,
    id: u64,
    sizes: Vec<(f32, f32)>,
    /// Small LRU of loaded pages (page parse is the expensive part).
    pages: Vec<(u32, ffi::FPDF_PAGE)>,
}

fn close_doc(api: &Api, d: OpenDoc) {
    unsafe {
        for (_, p) in d.pages {
            (api.FPDF_ClosePage)(p);
        }
        (api.FPDF_CloseDocument)(d.handle);
    }
}

fn worker(api: Api, shared: Arc<Shared>, results: Sender<TileResult>, waker: Waker) {
    let mut doc: Option<OpenDoc> = None;
    let mut next_id = 1u64;
    loop {
        let (cmd, job) = {
            let mut q = shared.q.lock().unwrap();
            loop {
                if q.shutdown {
                    if let Some(d) = doc.take() {
                        close_doc(&api, d);
                    }
                    return;
                }
                if let Some(c) = q.cmds.pop_front() {
                    break (Some(c), None);
                }
                if let Some(j) = q.jobs.pop() {
                    break (None, Some(j));
                }
                q = shared.cv.wait(q).unwrap();
            }
        };
        if let Some(c) = cmd {
            match c {
                Cmd::Open { bytes, reply } => {
                    if let Some(d) = doc.take() {
                        close_doc(&api, d);
                    }
                    let h = unsafe { (api.FPDF_LoadMemDocument64)(bytes.as_ptr() as *const _, bytes.len(), std::ptr::null()) };
                    if h.is_null() {
                        let e = unsafe { (api.FPDF_GetLastError)() } as u64;
                        let _ = reply.send(Err(if e == 4 { PdfError::Password } else { PdfError::Open(e) }));
                        continue;
                    }
                    let n = unsafe { (api.FPDF_GetPageCount)(h) }.max(0);
                    let mut sizes = Vec::with_capacity(n as usize);
                    for i in 0..n {
                        let mut s = FS_SIZEF::default();
                        unsafe { (api.FPDF_GetPageSizeByIndexF)(h, i, &mut s) };
                        sizes.push((s.width.max(1.0), s.height.max(1.0)));
                    }
                    let id = next_id;
                    next_id += 1;
                    shared.q.lock().unwrap().doc_id = id;
                    let _ = reply.send(Ok(DocInfo { doc_id: id, pages: sizes.clone() }));
                    doc = Some(OpenDoc { handle: h, _bytes: bytes, id, sizes, pages: Vec::new() });
                }
                Cmd::Close => {
                    if let Some(d) = doc.take() {
                        close_doc(&api, d);
                    }
                }
            }
            continue;
        }
        let Some(job) = job else { continue };
        let Some(d) = doc.as_mut() else { continue };
        let t0 = std::time::Instant::now();
        if let Some(pixmap) = render_tile(&api, d, &job) {
            let _ = results.send(TileResult { doc_id: d.id, key: job.key, pixmap, render_ms: t0.elapsed().as_secs_f32() * 1000.0 });
            waker();
        }
    }
}

fn page_handle(api: &Api, d: &mut OpenDoc, idx: u32) -> Option<ffi::FPDF_PAGE> {
    if let Some(i) = d.pages.iter().position(|(p, _)| *p == idx) {
        let e = d.pages.remove(i);
        d.pages.push(e);
        return Some(e.1);
    }
    let p = unsafe { (api.FPDF_LoadPage)(d.handle, idx as i32) };
    if p.is_null() {
        return None;
    }
    if d.pages.len() >= 6 {
        let (_, old) = d.pages.remove(0);
        unsafe { (api.FPDF_ClosePage)(old) };
    }
    d.pages.push((idx, p));
    Some(p)
}

fn render_tile(api: &Api, d: &mut OpenDoc, job: &TileReq) -> Option<Pixmap> {
    let &(pw, ph) = d.sizes.get(job.key.page as usize)?;
    let scale = job.scale;
    let (full_w, full_h) = ((pw * scale).ceil() as u32, (ph * scale).ceil() as u32);
    let (x0, y0) = (job.key.tx * TILE, job.key.ty * TILE);
    if x0 >= full_w || y0 >= full_h {
        return None;
    }
    let (w, h) = ((full_w - x0).min(TILE).max(1), (full_h - y0).min(TILE).max(1));
    let page = page_handle(api, d, job.key.page)?;
    let mut pm = Pixmap::new(w, h)?;
    unsafe {
        let bmp = (api.FPDFBitmap_CreateEx)(w as i32, h as i32, ffi::FPDFBITMAP_BGRA, pm.data_mut().as_mut_ptr() as *mut _, (w * 4) as i32);
        if bmp.is_null() {
            return None;
        }
        (api.FPDFBitmap_FillRect)(bmp, 0, 0, w as i32, h as i32, 0xFFFF_FFFF);
        let m = FS_MATRIX { a: scale, b: 0.0, c: 0.0, d: scale, e: -(x0 as f32), f: -(y0 as f32) };
        let clip = FS_RECTF { left: 0.0, top: 0.0, right: w as f32, bottom: h as f32 };
        (api.FPDF_RenderPageBitmapWithMatrix)(bmp, page, &m, &clip, ffi::FPDF_ANNOT | ffi::FPDF_REVERSE_BYTE_ORDER);
        (api.FPDFBitmap_Destroy)(bmp);
    }
    Some(pm)
}
