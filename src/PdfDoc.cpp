#include "PdfDoc.h"
#include "mupdf/fitz.h"

PdfDoc::PdfDoc() = default;
PdfDoc::~PdfDoc() { Close(); }

void PdfDoc::InitLibrary(){
    // MuPDF has no global init — contexts are created per-document
}
void PdfDoc::DestroyLibrary(){
    // MuPDF has no global shutdown
}

bool PdfDoc::Load(const std::wstring& path){
    Close(); m_path = path;
    std::lock_guard<std::mutex> lock(m_mutex);

    // Convert path to UTF-8
    int u8len = WideCharToMultiByte(CP_UTF8, 0, path.c_str(), -1, nullptr, 0, nullptr, nullptr);
    if (u8len <= 0) return false;
    std::string u8path(u8len, '\0');
    WideCharToMultiByte(CP_UTF8, 0, path.c_str(), -1, &u8path[0], u8len, nullptr, nullptr);

    // Create MuPDF context
    m_ctx = fz_new_context(nullptr, nullptr, 256 << 20); // 256MB store
    if (!m_ctx) return false;

    // Register default document handlers
    fz_register_document_handlers(m_ctx);

    // Open the document
    fz_try(m_ctx)
        m_doc = fz_open_document(m_ctx, u8path.c_str());
    fz_catch(m_ctx){
        fz_drop_context(m_ctx);
        m_ctx = nullptr;
        return false;
    }

    if (!m_doc){
        fz_drop_context(m_ctx);
        m_ctx = nullptr;
        return false;
    }

    m_pageCount = fz_count_pages(m_ctx, m_doc);
    if (m_pageCount <= 0){
        Close();
        return false;
    }
    return true;
}

void PdfDoc::Close(){
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_doc){ fz_drop_document(m_ctx, m_doc); m_doc = nullptr; }
    if (m_ctx){ fz_drop_context(m_ctx); m_ctx = nullptr; }
    m_pageCount = 0;
}

bool PdfDoc::GetPageSize(int idx, float* outW, float* outH){
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_doc || !m_ctx || idx < 0 || idx >= m_pageCount) return false;

    fz_page* page = fz_load_page(m_ctx, m_doc, idx);
    if (!page) return false;

    fz_rect bounds = fz_bound_page(m_ctx, page);
    *outW = bounds.x1 - bounds.x0;
    *outH = bounds.y1 - bounds.y0;

    fz_drop_page(m_ctx, page);
    return (*outW > 0 && *outH > 0);
}

Bitmap* PdfDoc::RenderPage(int idx, int w, int h){
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_doc || !m_ctx || idx < 0 || idx >= m_pageCount || w <= 0 || h <= 0) return nullptr;

    fz_page* page = fz_load_page(m_ctx, m_doc, idx);
    if (!page) return nullptr;

    // Get page bounds and compute scale matrix
    fz_rect bounds = fz_bound_page(m_ctx, page);
    float pageW = bounds.x1 - bounds.x0;
    float pageH = bounds.y1 - bounds.y0;
    if (pageW <= 0 || pageH <= 0){ fz_drop_page(m_ctx, page); return nullptr; }

    float scaleX = (float)w / pageW;
    float scaleY = (float)h / pageH;
    float scale = std::min(scaleX, scaleY);

    // Center the page in the output bitmap
    float ox = ((float)w - pageW * scale) * 0.5f;
    float oy = ((float)h - pageH * scale) * 0.5f;

    fz_matrix ctm;
    ctm = fz_scale(scale, scale);
    ctm = fz_concat(ctm, fz_translate(ox / scale, oy / scale));

    // Render page to pixmap (RGB, no alpha)
    fz_colorspace* cs = fz_device_rgb(m_ctx);
    fz_pixmap* pix = nullptr;

    fz_try(m_ctx)
        pix = fz_new_pixmap_from_page(m_ctx, page, ctm, cs, 0);
    fz_catch(m_ctx){
        fz_drop_page(m_ctx, page);
        return nullptr;
    }

    if (!pix){ fz_drop_page(m_ctx, page); return nullptr; }

    int pw = fz_pixmap_width(m_ctx, pix);
    int ph = fz_pixmap_height(m_ctx, pix);
    int n = fz_pixmap_components(m_ctx, pix);
    int stride = fz_pixmap_stride(m_ctx, pix);
    unsigned char* samples = fz_pixmap_samples(m_ctx, pix);

    // Create GDI+ Bitmap (BGRA = PixelFormat32bppARGB in little-endian)
    Bitmap* bmp = new Bitmap(pw, ph, PixelFormat32bppARGB);
    if (bmp->GetLastStatus() == Ok){
        BitmapData bd;
        Rect rc(0, 0, pw, ph);
        if (bmp->LockBits(&rc, ImageLockModeWrite, PixelFormat32bppARGB, &bd) == Ok){
            BYTE* dst = (BYTE*)bd.Scan0;
            for (int y = 0; y < ph; ++y){
                unsigned char* src = samples + y * stride;
                BYTE* drow = dst + y * bd.Stride;
                for (int x = 0; x < pw; ++x){
                    if (n >= 3){
                        drow[x * 4 + 0] = src[x * n + 2]; // B ← R
                        drow[x * 4 + 1] = src[x * n + 1]; // G ← G
                        drow[x * 4 + 2] = src[x * n + 0]; // R ← B
                        drow[x * 4 + 3] = (n >= 4) ? src[x * n + 3] : 255; // A
                    } else {
                        drow[x * 4 + 0] = src[x * n];
                        drow[x * 4 + 1] = src[x * n];
                        drow[x * 4 + 2] = src[x * n];
                        drow[x * 4 + 3] = 255;
                    }
                }
            }
            bmp->UnlockBits(&bd);
        }
    }

    fz_drop_pixmap(m_ctx, pix);
    fz_drop_page(m_ctx, page);
    return bmp;
}
