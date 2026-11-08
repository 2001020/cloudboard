// gen_icon.cpp — Generate cloudboard.ico at build time
// Compile: cl /EHsc /std:c++17 gen_icon.cpp /link gdiplus.lib ole32.lib
#include <windows.h>
#include <gdiplus.h>
#include <cstdio>
#pragma comment(lib, "gdiplus.lib")
using namespace Gdiplus;

int main(){
    ULONG_PTR token;
    GdiplusStartupInput si;
    GdiplusStartup(&token, &si, nullptr);

    // Draw a simple cloud icon
    int sz = 256;
    Bitmap bmp(sz, sz, PixelFormat32bppARGB);
    Graphics g(&bmp);
    g.SetSmoothingMode(SmoothingModeAntiAlias);
    g.Clear(Color(0, 0, 0, 0));

    // Blue circle background
    SolidBrush bg(Color(255, 79, 130, 210));
    g.FillEllipse(&bg, 8, 8, sz - 16, sz - 16);

    // White cloud
    SolidBrush white(Color(255, 255, 255, 255));
    int cx = sz / 2, cy = sz / 2;
    g.FillEllipse(&white, cx - 70, cy - 30, 80, 60);
    g.FillEllipse(&white, cx - 30, cy - 50, 80, 60);
    g.FillEllipse(&white, cx + 10, cy - 30, 80, 60);
    g.FillEllipse(&white, cx - 50, cy - 10, 140, 50);

    // Save as PNG to memory
    CLSID pngClsid;
    UINT num = 0, size2 = 0;
    GetImageEncodersSize(&num, &size2);
    ImageCodecInfo* info = (ImageCodecInfo*)malloc(size2);
    GetImageEncoders(num, size2, info);
    for (UINT i = 0; i < num; i++){
        if (wcscmp(info[i].MimeType, L"image/png") == 0){
            pngClsid = info[i].Clsid;
            break;
        }
    }
    free(info);

    // Save PNG to stream
    IStream* stream = nullptr;
    CreateStreamOnHGlobal(nullptr, TRUE, &stream);
    bmp.Save(stream, &pngClsid, nullptr);

    STATSTG stat;
    stream->Stat(&stat, STATFLAG_DEFAULT);
    DWORD pngSize = (DWORD)stat.cbSize.QuadPart;
    BYTE* pngData = new BYTE[pngSize];
    LARGE_INTEGER li; li.QuadPart = 0;
    stream->Seek(li, STREAM_SEEK_SET, nullptr);
    stream->Read(pngData, pngSize, nullptr);
    stream->Release();

    // Write ICO file (header + 1 PNG entry)
    FILE* f = fopen("cloudboard.ico", "wb");
    // ICO header
    WORD reserved = 0, type = 1, count = 1;
    fwrite(&reserved, 2, 1, f);
    fwrite(&type, 2, 1, f);
    fwrite(&count, 2, 1, f);
    // Entry: 0 width/height = 256
    BYTE w = 0, h = 0, colors = 0, rsv = 0;
    WORD planes = 1, bpp = 32;
    DWORD dataSize = pngSize, offset = 22;
    fwrite(&w, 1, 1, f);
    fwrite(&h, 1, 1, f);
    fwrite(&colors, 1, 1, f);
    fwrite(&rsv, 1, 1, f);
    fwrite(&planes, 2, 1, f);
    fwrite(&bpp, 2, 1, f);
    fwrite(&dataSize, 4, 1, f);
    fwrite(&offset, 4, 1, f);
    // PNG data
    fwrite(pngData, 1, pngSize, f);
    fclose(f);

    delete[] pngData;
    GdiplusShutdown(token);
    return 0;
}
