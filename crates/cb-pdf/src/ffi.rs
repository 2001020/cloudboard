//! Minimal dynamic bindings to the PDFium C API (only what CloudBoard uses).

#![allow(non_snake_case, non_camel_case_types)]

use libloading::Library;
use std::ffi::{c_char, c_int, c_ulong, c_void};
use std::path::Path;

pub type FPDF_DOCUMENT = *mut c_void;
pub type FPDF_PAGE = *mut c_void;
pub type FPDF_BITMAP = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct FS_MATRIX {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct FS_RECTF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

pub const FPDF_ANNOT: c_int = 0x01;
pub const FPDF_LCD_TEXT: c_int = 0x02;
pub const FPDF_REVERSE_BYTE_ORDER: c_int = 0x10;
pub const FPDFBITMAP_BGRA: c_int = 4;

pub struct Api {
    _lib: Library,
    pub FPDF_InitLibrary: unsafe extern "C" fn(),
    pub FPDF_DestroyLibrary: unsafe extern "C" fn(),
    pub FPDF_GetLastError: unsafe extern "C" fn() -> c_ulong,
    pub FPDF_LoadMemDocument64: unsafe extern "C" fn(*const c_void, usize, *const c_char) -> FPDF_DOCUMENT,
    pub FPDF_CloseDocument: unsafe extern "C" fn(FPDF_DOCUMENT),
    pub FPDF_GetPageCount: unsafe extern "C" fn(FPDF_DOCUMENT) -> c_int,
    pub FPDF_GetPageSizeByIndexF: unsafe extern "C" fn(FPDF_DOCUMENT, c_int, *mut FS_SIZEF) -> c_int,
    pub FPDF_LoadPage: unsafe extern "C" fn(FPDF_DOCUMENT, c_int) -> FPDF_PAGE,
    pub FPDF_ClosePage: unsafe extern "C" fn(FPDF_PAGE),
    pub FPDFBitmap_CreateEx: unsafe extern "C" fn(c_int, c_int, c_int, *mut c_void, c_int) -> FPDF_BITMAP,
    pub FPDFBitmap_FillRect: unsafe extern "C" fn(FPDF_BITMAP, c_int, c_int, c_int, c_int, c_ulong),
    pub FPDFBitmap_Destroy: unsafe extern "C" fn(FPDF_BITMAP),
    pub FPDF_RenderPageBitmapWithMatrix: unsafe extern "C" fn(FPDF_BITMAP, FPDF_PAGE, *const FS_MATRIX, *const FS_RECTF, c_int),
}

macro_rules! sym {
    ($lib:expr, $name:ident) => {
        *$lib.get(concat!(stringify!($name), "\0").as_bytes()).map_err(|e| format!("{}: {e}", stringify!($name)))?
    };
}

impl Api {
    /// # Safety
    /// Loads a native library; the library must be a genuine PDFium build.
    pub unsafe fn load(path: &Path) -> Result<Api, String> {
        let lib = Library::new(path).map_err(|e| format!("无法加载 PDFium（{}）：{e}", path.display()))?;
        let api = Api {
            FPDF_InitLibrary: sym!(lib, FPDF_InitLibrary),
            FPDF_DestroyLibrary: sym!(lib, FPDF_DestroyLibrary),
            FPDF_GetLastError: sym!(lib, FPDF_GetLastError),
            FPDF_LoadMemDocument64: sym!(lib, FPDF_LoadMemDocument64),
            FPDF_CloseDocument: sym!(lib, FPDF_CloseDocument),
            FPDF_GetPageCount: sym!(lib, FPDF_GetPageCount),
            FPDF_GetPageSizeByIndexF: sym!(lib, FPDF_GetPageSizeByIndexF),
            FPDF_LoadPage: sym!(lib, FPDF_LoadPage),
            FPDF_ClosePage: sym!(lib, FPDF_ClosePage),
            FPDFBitmap_CreateEx: sym!(lib, FPDFBitmap_CreateEx),
            FPDFBitmap_FillRect: sym!(lib, FPDFBitmap_FillRect),
            FPDFBitmap_Destroy: sym!(lib, FPDFBitmap_Destroy),
            FPDF_RenderPageBitmapWithMatrix: sym!(lib, FPDF_RenderPageBitmapWithMatrix),
            _lib: lib,
        };
        (api.FPDF_InitLibrary)();
        Ok(api)
    }
}

impl Drop for Api {
    fn drop(&mut self) {
        unsafe { (self.FPDF_DestroyLibrary)() }
    }
}

/// Platform file name of the PDFium library.
pub fn library_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    }
}
