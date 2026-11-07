#pragma once
#include "Common.h"
#include <mutex>

// MuPDF C API (context/document types are opaque handles)
extern "C" {
    typedef struct fz_context fz_context;
    typedef struct fz_document fz_document;
}

class PdfDoc {
public:
    PdfDoc();
    ~PdfDoc();
    bool Load(const std::wstring& path);
    void Close();
    int  GetPageCount() const { return m_pageCount; }
    bool GetPageSize(int idx, float* outW, float* outH);
    Bitmap* RenderPage(int idx, int pixelW, int pixelH);
    static void InitLibrary();
    static void DestroyLibrary();
private:
    fz_context*   m_ctx{ nullptr };
    fz_document*  m_doc{ nullptr };
    mutable std::mutex m_mutex;
    int m_pageCount{ 0 };
    std::wstring m_path;
};
