#include "ProjectIO.h"
#include "Board.h"
#include "PdfDoc.h"
#include <fstream>
#include <vector>
#include <algorithm>

namespace ProjectIO {

static constexpr uint32_t kMagic = 0x42444C43; // CLDB
static constexpr uint32_t kVersion = 1;
static constexpr uint32_t kEndian = 0x01020304;
static constexpr int32_t kCoordScale = 100;

enum BlockType : uint32_t {
    BLK_META = 1,
    BLK_PDF_DATA = 2,
    BLK_PAGES = 3,
    BLK_ASSIST = 4
};

struct Writer {
    std::vector<uint8_t> b;
    template<class T> void pod(const T& v){
        const uint8_t* p = reinterpret_cast<const uint8_t*>(&v);
        b.insert(b.end(), p, p + sizeof(T));
    }
    void bytes(const void* p, size_t n){
        const uint8_t* q = reinterpret_cast<const uint8_t*>(p);
        b.insert(b.end(), q, q + n);
    }
    void str(const std::wstring& s){
        uint32_t n = (uint32_t)s.size();
        pod(n);
        if (n) bytes(s.data(), (size_t)n * sizeof(wchar_t));
    }
    void blob(const std::vector<uint8_t>& v){
        uint64_t n = (uint64_t)v.size();
        pod(n);
        if (!v.empty()) bytes(v.data(), v.size());
    }
};

struct Reader {
    const uint8_t* p{};
    const uint8_t* e{};
    bool ok{ true };
    Reader() = default;
    Reader(const std::vector<uint8_t>& v) : p(v.data()), e(v.data() + v.size()) {}
    Reader(const uint8_t* data, size_t len) : p(data), e(data + len) {}
    template<class T> T pod(){
        T v{};
        if (!ok || p + sizeof(T) > e){ ok = false; return v; }
        memcpy(&v, p, sizeof(T));
        p += sizeof(T);
        return v;
    }
    std::wstring str(){
        uint32_t n = pod<uint32_t>();
        if (!ok || p + (size_t)n * sizeof(wchar_t) > e){ ok = false; return L""; }
        std::wstring s((const wchar_t*)p, (size_t)n);
        p += (size_t)n * sizeof(wchar_t);
        return s;
    }
    std::vector<uint8_t> blob(){
        uint64_t n = pod<uint64_t>();
        if (!ok || n > (uint64_t)(e - p)){ ok = false; return {}; }
        std::vector<uint8_t> v(p, p + (size_t)n);
        p += (size_t)n;
        return v;
    }
};

static uint32_t Argb(Color c){
    return ((uint32_t)c.GetA() << 24) | ((uint32_t)c.GetR() << 16) |
           ((uint32_t)c.GetG() << 8) | (uint32_t)c.GetB();
}

static Color ColorFromArgb(uint32_t v){
    return Color((BYTE)(v >> 24), (BYTE)(v >> 16), (BYTE)(v >> 8), (BYTE)v);
}

static int32_t Q(float v){ return (int32_t)std::lround(v * (float)kCoordScale); }
static float UQ(int32_t v){ return (float)v / (float)kCoordScale; }

static void EnsureObjectUids(){
    uint64_t maxUid = 0;
    for (auto& pg : g.pages){
        for (auto& s : pg.strokes){ if (s.uid == 0) s.uid = g.nextObjectUid++; maxUid = std::max(maxUid, s.uid); }
        for (auto& t : pg.texts){ if (t.uid == 0) t.uid = g.nextObjectUid++; maxUid = std::max(maxUid, t.uid); }
        for (auto& im : pg.images){ if (im.uid == 0) im.uid = g.nextObjectUid++; maxUid = std::max(maxUid, im.uid); }
    }
    g.nextObjectUid = std::max(g.nextObjectUid, maxUid + 1);
}

static int GetEncoderClsidLocal(const WCHAR* format, CLSID* pClsid){
    UINT num = 0, size = 0;
    GetImageEncodersSize(&num, &size);
    if (size == 0) return -1;
    std::vector<uint8_t> buf(size);
    auto* info = (ImageCodecInfo*)buf.data();
    GetImageEncoders(num, size, info);
    for (UINT i = 0; i < num; ++i){
        if (wcscmp(info[i].MimeType, format) == 0){
            *pClsid = info[i].Clsid;
            return (int)i;
        }
    }
    return -1;
}

static std::vector<uint8_t> BitmapToPng(Bitmap* bmp){
    std::vector<uint8_t> out;
    if (!bmp) return out;
    IStream* stream = nullptr;
    if (FAILED(CreateStreamOnHGlobal(nullptr, TRUE, &stream))) return out;
    CLSID clsid{};
    if (GetEncoderClsidLocal(L"image/png", &clsid) >= 0 && bmp->Save(stream, &clsid, nullptr) == Ok){
        HGLOBAL hg = nullptr;
        if (SUCCEEDED(GetHGlobalFromStream(stream, &hg))){
            SIZE_T sz = GlobalSize(hg);
            void* p = GlobalLock(hg);
            if (p && sz > 0) out.assign((uint8_t*)p, (uint8_t*)p + sz);
            if (p) GlobalUnlock(hg);
        }
    }
    stream->Release();
    return out;
}

static std::shared_ptr<Bitmap> PngToBitmap(const std::vector<uint8_t>& data){
    if (data.empty()) return nullptr;
    HGLOBAL hg = GlobalAlloc(GMEM_MOVEABLE, data.size());
    if (!hg) return nullptr;
    void* p = GlobalLock(hg);
    memcpy(p, data.data(), data.size());
    GlobalUnlock(hg);
    IStream* stream = nullptr;
    if (FAILED(CreateStreamOnHGlobal(hg, TRUE, &stream))){
        GlobalFree(hg);
        return nullptr;
    }
    Bitmap* src = Bitmap::FromStream(stream);
    std::shared_ptr<Bitmap> result;
    if (src && src->GetLastStatus() == Ok){
        Rect rc(0, 0, src->GetWidth(), src->GetHeight());
        Bitmap* clone = src->Clone(rc, PixelFormat32bppARGB);
        if (clone) result.reset(clone);
    }
    delete src;
    stream->Release();
    return result;
}

static bool ReadFileBytes(const std::wstring& path, std::vector<uint8_t>& out){
    std::ifstream f(path, std::ios::binary);
    if (!f) return false;
    f.seekg(0, std::ios::end);
    std::streamoff n = f.tellg();
    if (n < 0) return false;
    f.seekg(0, std::ios::beg);
    out.resize((size_t)n);
    if (n > 0) f.read((char*)out.data(), n);
    return (bool)f || n == 0;
}

static bool WriteFileBytes(const std::wstring& path, const std::vector<uint8_t>& data){
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (!f) return false;
    if (!data.empty()) f.write((const char*)data.data(), data.size());
    return (bool)f;
}

static std::wstring DefaultProjectName(){
    SYSTEMTIME st; GetLocalTime(&st);
    wchar_t buf[64];
    swprintf(buf, 64, L"%04d%02d%02d-%02d%02d%02d.cloudboard",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
    return buf;
}

static std::wstring EnsureExt(std::wstring path){
    size_t slash = path.find_last_of(L"\\/");
    size_t dot = path.find_last_of(L'.');
    if (dot == std::wstring::npos || (slash != std::wstring::npos && dot < slash)){
        path += L".cloudboard";
    } else {
        std::wstring ext = path.substr(dot + 1);
        for (auto& c : ext) c = (wchar_t)towlower(c);
        if (ext != L"cloudboard")
            path = path.substr(0, dot) + L".cloudboard";
    }
    return path;
}

bool IsCloudboardFile(const std::wstring& path){
    size_t dot = path.find_last_of(L'.');
    if (dot == std::wstring::npos) return false;
    std::wstring ext = path.substr(dot + 1);
    for (auto& c : ext) c = (wchar_t)towlower(c);
    return ext == L"cloudboard";
}

static void AddBlock(Writer& file, uint32_t type, const Writer& block){
    file.pod(type);
    uint64_t len = (uint64_t)block.b.size();
    file.pod(len);
    if (len) file.bytes(block.b.data(), block.b.size());
}

static void WriteStroke(Writer& w, const Stroke& s){
    w.pod(s.uid);
    w.pod(Argb(s.color));
    w.pod(s.width);
    int32_t style = (int32_t)s.style, shape = (int32_t)s.shape;
    uint32_t flags = (s.isHighlighter ? 1u : 0u) | (s.isShape ? 2u : 0u);
    w.pod(style); w.pod(flags); w.pod(shape);
    w.pod(s.a.x); w.pod(s.a.y); w.pod(s.b.x); w.pod(s.b.y);
    uint32_t n = (uint32_t)s.pts.size();
    w.pod(n);
    int32_t px = 0, py = 0;
    for (uint32_t i = 0; i < n; ++i){
        int32_t x = Q(s.pts[i].x), y = Q(s.pts[i].y);
        int32_t dx = (i == 0) ? x : x - px;
        int32_t dy = (i == 0) ? y : y - py;
        uint16_t pressure = (uint16_t)std::max(0, std::min(4095, (int)std::lround(s.pts[i].pressure * 1024.0f)));
        uint16_t dt = s.pts[i].dtMs;
        w.pod(dx); w.pod(dy); w.pod(pressure); w.pod(dt);
        px = x; py = y;
    }
}

static Stroke ReadStroke(Reader& r){
    Stroke s;
    s.uid = r.pod<uint64_t>();
    s.color = ColorFromArgb(r.pod<uint32_t>());
    s.width = r.pod<float>();
    s.style = (LineStyle)r.pod<int32_t>();
    uint32_t flags = r.pod<uint32_t>();
    s.isHighlighter = (flags & 1) != 0;
    s.isShape = (flags & 2) != 0;
    s.shape = (ShapeKind)r.pod<int32_t>();
    s.a.x = r.pod<float>(); s.a.y = r.pod<float>();
    s.b.x = r.pod<float>(); s.b.y = r.pod<float>();
    uint32_t n = r.pod<uint32_t>();
    s.pts.reserve(n);
    int32_t x = 0, y = 0;
    for (uint32_t i = 0; i < n; ++i){
        x += r.pod<int32_t>();
        y += r.pod<int32_t>();
        uint16_t pressure = r.pod<uint16_t>();
        uint16_t dt = r.pod<uint16_t>();
        s.pts.push_back({ UQ(x), UQ(y), (float)pressure / 1024.0f, dt });
    }
    return s;
}

static void WritePages(Writer& w){
    uint32_t pageCount = (uint32_t)g.pages.size();
    w.pod(pageCount);
    for (uint32_t pi = 0; pi < pageCount; ++pi){
        const auto& p = g.pages[pi];
        uint32_t strokeCount = (uint32_t)p.strokes.size();
        w.pod(strokeCount);
        for (auto& s : p.strokes) WriteStroke(w, s);

        uint32_t textCount = (uint32_t)p.texts.size();
        w.pod(textCount);
        for (auto& t : p.texts){
            w.pod(t.uid); w.str(t.text);
            w.pod(t.pos.x); w.pod(t.pos.y);
            w.pod(Argb(t.color)); w.pod(t.fontSize); w.str(t.family);
            uint8_t bold = t.bold ? 1 : 0; w.pod(bold);
        }

        uint32_t imageCount = 0;
        for (int i = 0; i < (int)p.images.size(); ++i)
            if (!(g.pdfMode && i == 0)) imageCount++;
        w.pod(imageCount);
        for (int i = 0; i < (int)p.images.size(); ++i){
            if (g.pdfMode && i == 0) continue;
            const auto& im = p.images[i];
            w.pod(im.uid); w.pod(im.pos.x); w.pod(im.pos.y);
            w.pod(im.w); w.pod(im.h); w.pod(im.rotateDeg);
            w.blob(BitmapToPng(im.bmp.get()));
        }
    }
}

static std::vector<Page> ReadPages(Reader& r, uint64_t* maxUid){
    uint32_t pageCount = r.pod<uint32_t>();
    std::vector<Page> pages;
    pages.resize(std::max<uint32_t>(1, pageCount));
    for (uint32_t pi = 0; pi < pageCount && r.ok; ++pi){
        auto& p = pages[pi];
        uint32_t strokeCount = r.pod<uint32_t>();
        for (uint32_t i = 0; i < strokeCount && r.ok; ++i){
            Stroke s = ReadStroke(r);
            *maxUid = std::max(*maxUid, s.uid);
            p.strokes.push_back(std::move(s));
        }
        uint32_t textCount = r.pod<uint32_t>();
        for (uint32_t i = 0; i < textCount && r.ok; ++i){
            TextItem t;
            t.uid = r.pod<uint64_t>(); t.text = r.str();
            t.pos.x = r.pod<float>(); t.pos.y = r.pod<float>();
            t.color = ColorFromArgb(r.pod<uint32_t>());
            t.fontSize = r.pod<float>(); t.family = r.str();
            t.bold = r.pod<uint8_t>() != 0;
            *maxUid = std::max(*maxUid, t.uid);
            p.texts.push_back(std::move(t));
        }
        uint32_t imageCount = r.pod<uint32_t>();
        for (uint32_t i = 0; i < imageCount && r.ok; ++i){
            ImageItem im;
            im.uid = r.pod<uint64_t>();
            im.pos.x = r.pod<float>(); im.pos.y = r.pod<float>();
            im.w = r.pod<float>(); im.h = r.pod<float>(); im.rotateDeg = r.pod<float>();
            im.bmp = PngToBitmap(r.blob());
            *maxUid = std::max(*maxUid, im.uid);
            p.images.push_back(std::move(im));
        }
    }
    return pages;
}

bool SaveCloudboardToFile(const std::wstring& pathIn, std::wstring* error){
    std::wstring path = EnsureExt(pathIn);
    EnsureObjectUids();

    Writer file;
    file.pod(kMagic); file.pod(kVersion); file.pod(kEndian);

    Writer meta;
    meta.str(APP_VERSION);
    meta.str(g.pdfPath);
    uint8_t pdfMode = g.pdfMode ? 1 : 0;
    meta.pod(pdfMode);
    meta.pod(g.currentPage); meta.pod(g.zoomLevel); meta.pod(g.scrollX); meta.pod(g.scrollY);
    meta.pod(g.nextObjectUid); meta.pod(g.pdfPageWidth); meta.pod(g.pdfTotalHeight);
    wchar_t user[128] = {};
    DWORD userLen = 128;
    if (GetUserNameW(user, &userLen)) meta.str(user); else meta.str(L"");
    AddBlock(file, BLK_META, meta);

    if (g.pdfMode && !g.pdfPath.empty()){
        std::vector<uint8_t> pdfBytes;
        if (ReadFileBytes(g.pdfPath, pdfBytes)){
            Writer pdf; pdf.str(g.pdfPath); pdf.blob(pdfBytes);
            AddBlock(file, BLK_PDF_DATA, pdf);
        }
    }

    Writer pages;
    WritePages(pages);
    AddBlock(file, BLK_PAGES, pages);

    Writer assist;
    int32_t ak = (int32_t)g.assist.kind, tk = (int32_t)g.assist.triangleKind;
    assist.pod(ak); assist.pod(tk);
    assist.pod(g.assist.trianglePos.x); assist.pod(g.assist.trianglePos.y);
    assist.pod(g.assist.triangleAngle); assist.pod(g.assist.triangleScale);
    assist.pod(g.assist.compassCenter.x); assist.pod(g.assist.compassCenter.y);
    assist.pod(g.assist.compassRadius); assist.pod(g.assist.compassAngle);
    AddBlock(file, BLK_ASSIST, assist);

    std::wstring tmp = path + L".tmp";
    if (!WriteFileBytes(tmp, file.b)){
        if (error) *error = L"无法写入临时文件";
        return false;
    }
    if (!MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)){
        DeleteFileW(tmp.c_str());
        if (error) *error = L"无法替换目标文件";
        return false;
    }
    return true;
}

static std::wstring WriteEmbeddedPdfTemp(const std::vector<uint8_t>& data){
    wchar_t tempDir[MAX_PATH]; GetTempPathW(MAX_PATH, tempDir);
    wchar_t tempFile[MAX_PATH]; GetTempFileNameW(tempDir, L"cld", 0, tempFile);
    std::wstring path = tempFile;
    DeleteFileW(path.c_str());
    path += L".pdf";
    return WriteFileBytes(path, data) ? path : L"";
}

bool LoadCloudboardFromFile(const std::wstring& path, std::wstring* error){
    std::vector<uint8_t> bytes;
    if (!ReadFileBytes(path, bytes)){
        if (error) *error = L"无法读取文件";
        return false;
    }
    Reader r(bytes);
    if (r.pod<uint32_t>() != kMagic || r.pod<uint32_t>() > kVersion || r.pod<uint32_t>() != kEndian || !r.ok){
        if (error) *error = L"不是有效的 .cloudboard 文件";
        return false;
    }

    bool pdfMode = false;
    int currentPage = 0;
    float zoom = kZoomDisplayBase, sx = 0, sy = 0;
    uint64_t nextUid = 1, maxUid = 0;
    std::vector<uint8_t> pdfBytes;
    std::vector<Page> loadedPages;
    bool gotPages = false;
    AssistState assist;

    while (r.ok && r.p < r.e){
        uint32_t type = r.pod<uint32_t>();
        uint64_t len = r.pod<uint64_t>();
        if (!r.ok || len > (uint64_t)(r.e - r.p)){ r.ok = false; break; }
        Reader br(r.p, (size_t)len);
        r.p += (size_t)len;

        if (type == BLK_META){
            (void)br.str();
            (void)br.str();
            pdfMode = br.pod<uint8_t>() != 0;
            currentPage = br.pod<int>();
            zoom = br.pod<float>(); sx = br.pod<float>(); sy = br.pod<float>();
            nextUid = br.pod<uint64_t>();
        } else if (type == BLK_PDF_DATA){
            (void)br.str();
            pdfBytes = br.blob();
        } else if (type == BLK_PAGES){
            loadedPages = ReadPages(br, &maxUid);
            gotPages = br.ok;
        } else if (type == BLK_ASSIST){
            assist.kind = (AssistKind)br.pod<int32_t>();
            assist.triangleKind = (TriangleKind)br.pod<int32_t>();
            assist.trianglePos.x = br.pod<float>(); assist.trianglePos.y = br.pod<float>();
            assist.triangleAngle = br.pod<float>(); assist.triangleScale = br.pod<float>();
            assist.compassCenter.x = br.pod<float>(); assist.compassCenter.y = br.pod<float>();
            assist.compassRadius = br.pod<float>(); assist.compassAngle = br.pod<float>();
        }
    }
    if (!r.ok || !gotPages){
        if (error) *error = L".cloudboard 文件数据不完整";
        return false;
    }

    if (pdfMode && !pdfBytes.empty()){
        std::wstring pdfPath = WriteEmbeddedPdfTemp(pdfBytes);
        if (pdfPath.empty()){
            if (error) *error = L"无法恢复内嵌 PDF";
            return false;
        }
        Board::LoadPdf(pdfPath);
        int n = std::min((int)g.pages.size(), (int)loadedPages.size());
        for (int i = 0; i < n; ++i){
            auto pdfImage = g.pages[i].images.empty() ? ImageItem{} : g.pages[i].images[0];
            g.pages[i].strokes = std::move(loadedPages[i].strokes);
            g.pages[i].texts = std::move(loadedPages[i].texts);
            g.pages[i].images.clear();
            g.pages[i].images.push_back(std::move(pdfImage));
            for (auto& im : loadedPages[i].images) g.pages[i].images.push_back(std::move(im));
            g.thumbDirty[i] = true;
        }
    } else {
        g.pdfMode = false;
        if (g.pdfDoc){ delete (PdfDoc*)g.pdfDoc; g.pdfDoc = nullptr; }
        g.pdfPath.clear();
        g.pdfPageYOffset.clear(); g.pdfTotalHeight = 0; g.pdfPageWidth = 0;
        g.pages = std::move(loadedPages);
        if (g.pages.empty()) g.pages.emplace_back();
        g.thumbCache.assign(g.pages.size(), nullptr);
        g.thumbDirty.assign(g.pages.size(), true);
    }

    g.currentPage = std::max(0, std::min(currentPage, (int)g.pages.size() - 1));
    g.zoomLevel = std::max(kZoomMin, std::min(kZoomMax, zoom));
    g.scrollX = sx; g.scrollY = sy;
    if (g.pdfMode) Board::ClampPdfScroll();
    g.nextObjectUid = std::max(nextUid, maxUid + 1);
    g.assist = assist;
    g.assist.kind = AssistKind::None;
    g.selectedItems.clear();
    g.stableW = 0;
    g.currentProjectPath = path;
    InvalidateRect(g.hWnd, nullptr, FALSE);
    return true;
}

struct SaveThreadData { std::wstring path; bool rememberPath{ true }; };

static DWORD WINAPI SaveThreadProc(LPVOID p){
    std::unique_ptr<SaveThreadData> data((SaveThreadData*)p);
    std::wstring* msg = new std::wstring;
    bool ok = SaveCloudboardToFile(data->path, msg);
    if (ok && data->rememberPath) g.currentProjectPath = data->path;
    if (ok) *msg = L"已保存 .cloudboard";
    PostMessageW(g.hWnd, WM_APP_PROJECT_SAVE_DONE, ok ? 1 : 0, (LPARAM)msg);
    return 0;
}

bool BeginSaveCloudboard(HWND hWnd){
    if (g.hEdit) Board::CommitTextEdit();
    ReleaseCapture();
    bool started = false;

    if (!g.currentProjectPath.empty()){
        auto* data = new SaveThreadData;
        data->path = g.currentProjectPath;
        data->rememberPath = true;
        ShowToast(L"正在保存 .cloudboard...");
        HANDLE th = CreateThread(nullptr, 0, SaveThreadProc, data, 0, nullptr);
        if (th){
            CloseHandle(th);
            return true;
        }
        delete data;
        ShowToast(L"保存失败：无法启动保存线程");
        return false;
    }

    HRESULT coHr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    bool didCoInit = SUCCEEDED(coHr);
    IFileSaveDialog* dialog = nullptr;
    HRESULT hr = CoCreateInstance(CLSID_FileSaveDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dialog));
    if (SUCCEEDED(hr) && dialog){
        COMDLG_FILTERSPEC filters[] = {
            { L"CloudBoard project", L"*.cloudboard" }
        };
        dialog->SetFileTypes(1, filters);
        dialog->SetDefaultExtension(L"cloudboard");
        std::wstring name = DefaultProjectName();
        dialog->SetFileName(name.c_str());
        DWORD options = 0;
        if (SUCCEEDED(dialog->GetOptions(&options))){
            dialog->SetOptions(options | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_NOCHANGEDIR);
        }
        hr = dialog->Show(hWnd);
        if (SUCCEEDED(hr)){
            IShellItem* item = nullptr;
            if (SUCCEEDED(dialog->GetResult(&item)) && item){
                PWSTR raw = nullptr;
                if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &raw)) && raw){
                    auto* data = new SaveThreadData;
                    data->path = EnsureExt(raw);
                    CoTaskMemFree(raw);
                    ShowToast(L"正在保存 .cloudboard...");
                    HANDLE th = CreateThread(nullptr, 0, SaveThreadProc, data, 0, nullptr);
                    if (th){
                        CloseHandle(th);
                        started = true;
                    } else {
                        delete data;
                        ShowToast(L"保存失败：无法启动保存线程");
                    }
                }
                item->Release();
            }
        }
        dialog->Release();
    }
    if (didCoInit) CoUninitialize();
    return started;
}

} // namespace ProjectIO
