#pragma once
#include "Common.h"

namespace Board {
    void Init();
    void BeginStroke(float x, float y);
    bool AppendStroke(float x, float y);
    void EndStroke();
    void AddStroke(const Stroke& s);
    void BeginShape(float x, float y);
    void UpdateShape(float x, float y);
    void EndShape();
    void BeginErase(float x, float y);
    void MoveErase(float x, float y);
    void EndErase();
    void AddText(float x, float y);
    void CommitTextEdit();
    void AddImageFromFile(const std::wstring& path, float cx, float cy);
    void Undo(); void Redo(); void ClearCurrent();
    int HitTextAt(float x, float y);
    int HitImageAt(float x, float y);
    void ClearSelection();
    void SelectItemsInRect(float x1, float y1, float x2, float y2);
    bool HitSelected(float x, float y);
    void MoveSelectedBy(float dx, float dy);
    void DeleteSelected();
    void CopySelected();
    void PasteClipboard();
    void NextPage(); void PrevPage(); void GotoPage(int idx);
    void AddPage(); void DelPage();
    void ClampPdfScroll();
    bool HasAnyInk();
    void LoadPdf(const std::wstring& path);
    void RenderPdfPages();
    void PrefetchVisiblePdfPages();
    int  PdfPageAtY(float boardY);
    void DrawAll(Graphics& g, const RECT& boardRc);
    void DrawStroke(Graphics& g, const Stroke& s);
    void BoardDrawGL(const RECT& boardRc);
    void DrawTextsGDI(Graphics& gr, const RECT& boardRc);
    void DrawPageToBitmap(int pageIdx, Bitmap& bmp);
    void AutoSave();
    void ExportCurrentPNG(const std::wstring& path);
    PointF2 ScreenToBoard(int x, int y, const RECT& boardRc);
    PointF2 BoardToScreen(float bx, float by, const RECT& boardRc);
}
