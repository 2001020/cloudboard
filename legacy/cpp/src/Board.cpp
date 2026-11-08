#include "Board.h"
#include "UI.h"
#include "PdfDoc.h"
#include "RenderQueue.h"
#include "GLRenderer.h"
#include <fstream>
#include <unordered_map>

namespace Board {

static constexpr float kPdfRenderScale = 2.0f;
static constexpr int kPdfKeepRadius = 8;
static std::unordered_map<void*, GLuint> s_texCache;

static void ReleaseCachedTexture(Gdiplus::Bitmap* bmp){
    if (!bmp) return;
    auto it = s_texCache.find((void*)bmp);
    if (it == s_texCache.end()) return;
    if (glc.hDC && glc.hRC){
        wglMakeCurrent(glc.hDC, glc.hRC);
        glDeleteTextures(1, &it->second);
        wglMakeCurrent(nullptr, nullptr);
    }
    s_texCache.erase(it);
}

static bool IsCJKChar(wchar_t ch){
    return (ch >= 0x4E00 && ch <= 0x9FFF) || (ch >= 0x3400 && ch <= 0x4DBF) ||
           (ch >= 0x3000 && ch <= 0x303F) || (ch >= 0xFE30 && ch <= 0xFE4F) ||
           (ch >= 0xFF01 && ch <= 0xFF60) || (ch >= 0xFFE0 && ch <= 0xFFE6);
}

static void DrawMixedBoardText(Graphics& gr, const std::wstring& text,
    const PointF& pos, float fontSize, const Color& color){
    if (text.empty()) return;
    FontFamily ffCJK(L"Microsoft YaHei");
    FontFamily ffLatin(L"Anthropic Sans Text");
    StringFormat sf; sf.SetAlignment(StringAlignmentNear);
    SolidBrush br(color);
    size_t start = 0;
    float x = pos.X;
    while (start < text.size()){
        bool cjk = IsCJKChar(text[start]);
        size_t end = start + 1;
        while (end < text.size() && IsCJKChar(text[end]) == cjk) end++;
        std::wstring seg = text.substr(start, end - start);
        FontFamily* ff = cjk ? &ffCJK : &ffLatin;
        Font f(ff, fontSize, FontStyleRegular, UnitPixel);
        PointF segPos(x, pos.Y);
        gr.DrawString(seg.c_str(), -1, &f, segPos, &sf, &br);
        RectF bound; gr.MeasureString(seg.c_str(), -1, &f, segPos, &sf, &bound);
        x += bound.Width;
        start = end;
    }
}

PointF2 ScreenToBoard(int x, int y, const RECT& boardRc){
    float sx = (float)(x - boardRc.left) + g.scrollX;
    float sy = (float)(y - boardRc.top)  + g.scrollY;
    return { sx / g.zoomLevel, sy / g.zoomLevel };
}
PointF2 BoardToScreen(float bx, float by, const RECT& boardRc){
    float sx = bx * g.zoomLevel - g.scrollX + (float)boardRc.left;
    float sy = by * g.zoomLevel - g.scrollY + (float)boardRc.top;
    return { sx, sy };
}

void Init(){
    g.pages.clear();
    g.pages.emplace_back();
    g.thumbCache.assign(1, nullptr);
    g.thumbDirty.assign(1, true);
    g.currentPage = 0;
    g.nextObjectUid = 1;
    g.scrollX = 0;
    g.scrollY = 0;
    g.currentProjectPath.clear();
}

static Page& cur(){ return g.pages[g.currentPage]; }

static int ClampPageIndex(int pageIdx){
    if (g.pages.empty()) return 0;
    if (pageIdx < 0) return 0;
    int last = (int)g.pages.size() - 1;
    return pageIdx > last ? last : pageIdx;
}

static Page& pageAt(int pageIdx){
    return g.pages[ClampPageIndex(pageIdx)];
}

// ====== Pen strokes ======

void BeginStroke(float x, float y){
    if (g.drawing) EndStroke();
    g.drawing = true;
    g.activeDrawPage = ClampPageIndex(g.currentPage);
    g.current = Stroke();
    g.current.uid = g.nextObjectUid++;
    g.current.color = g.color;
    g.current.width = g.penSize;
    g.current.style = g.lineStyle;
    g.current.pts.push_back({x,y});
    g.lastStrokeInputTick = GetTickCount();
}

bool AppendStroke(float x, float y){
    if (!g.drawing) return false;
    auto& pts = g.current.pts;
    if (!pts.empty()){
        auto& last = pts.back();
        float dx = x - last.x, dy = y - last.y;
        float minScreenDist = 1.1f;
        float minBoardDist = minScreenDist / std::max(0.25f, g.zoomLevel);
        float minDist2 = minBoardDist * minBoardDist;
        if (dx*dx + dy*dy < minDist2) return false;

        if (pts.size() >= 2){
            auto& prev = pts[pts.size() - 2];
            float ax = last.x - prev.x, ay = last.y - prev.y;
            float bx = x - last.x, by = y - last.y;
            float al2 = ax*ax + ay*ay, bl2 = bx*bx + by*by;
            if (al2 > 0.01f && bl2 > 0.01f){
                float dot = (ax*bx + ay*by) / std::sqrt(al2 * bl2);
                if (dot > 0.985f && dx*dx + dy*dy < minDist2 * 6.0f){
                    last = {x, y};
                    g.lastStrokeInputTick = GetTickCount();
                    return true;
                }
            }
        }
    }
    pts.push_back({x,y});
    g.lastStrokeInputTick = GetTickCount();
    return true;
}

void EndStroke(){
    if (!g.drawing) return;
    g.drawing = false;
    g.lastStrokeEndTick = GetTickCount();
    if (g.current.pts.size() < 2){
        g.current = Stroke();
        g.activeDrawPage = -1;
        return;
    }

    int pageIdx = ClampPageIndex(g.activeDrawPage >= 0 ? g.activeDrawPage : g.currentPage);
    auto& p = pageAt(pageIdx);
    DWORD now = GetTickCount();
    bool newUndoGroup =
        !g.penUndoGroupActive ||
        g.penUndoGroupPage != pageIdx ||
        now - g.lastPenUndoTick > 450;
    if (newUndoGroup){
        p.undoStack.push_back({p.strokes, p.texts, p.images});
        g.penUndoGroupActive = true;
        g.penUndoGroupPage = pageIdx;
    }
    g.lastPenUndoTick = now;
    p.redoStack.clear();
    p.strokes.push_back(g.current);

    g.current = Stroke();
    g.activeDrawPage = -1;
    g.thumbDirty[pageIdx] = true;
}

void AddStroke(const Stroke& s){
    if (s.pts.size() < 2 && !s.isShape) return;
    Stroke ns = s;
    if (ns.uid == 0) ns.uid = g.nextObjectUid++;
    auto& p = cur();
    p.undoStack.push_back({p.strokes, p.texts, p.images});
    p.redoStack.clear();
    p.strokes.push_back(std::move(ns));
    g.penUndoGroupActive = false;
    g.thumbDirty[g.currentPage] = true;
    g.stableW = 0;
}

// ====== Shape drag ======

void BeginShape(float x, float y){
    g.shaping = true;
    g.activeDrawPage = ClampPageIndex(g.currentPage);
    g.shapeStart = {x,y};
    g.shapeEnd = {x,y};
}
void UpdateShape(float x, float y){
    if (!g.shaping) return;
    g.shapeEnd = {x,y};
}
void EndShape(){
    if (!g.shaping) return;
    g.shaping = false;
    float w = std::abs(g.shapeEnd.x - g.shapeStart.x);
    float h = std::abs(g.shapeEnd.y - g.shapeStart.y);
    if (w < 5 && h < 5){
        g.activeDrawPage = -1;
        return;
    }
    Stroke s;
    s.uid = g.nextObjectUid++;
    s.color = g.color;
    s.width = g.penSize;
    s.style = g.lineStyle;
    s.isShape = true;
    s.shape = g.currentShape;
    s.a = { std::min(g.shapeStart.x, g.shapeEnd.x), std::min(g.shapeStart.y, g.shapeEnd.y) };
    s.b = { std::max(g.shapeStart.x, g.shapeEnd.x), std::max(g.shapeStart.y, g.shapeEnd.y) };
    if (g.currentShape == ShapeKind::Line || g.currentShape == ShapeKind::Arrow){
        s.a = g.shapeStart; s.b = g.shapeEnd;
    }
    int pageIdx = ClampPageIndex(g.activeDrawPage >= 0 ? g.activeDrawPage : g.currentPage);
    auto& p = pageAt(pageIdx);
    p.undoStack.push_back({p.strokes, p.texts, p.images});
    p.redoStack.clear();
    p.strokes.push_back(s);
    g.activeDrawPage = -1;
    g.thumbDirty[pageIdx] = true;
    g.stableW = 0;
}

// ====== Real-time eraser ======

static float EraserRadius(){
    return std::max(1.0f, g.eraserSize * 0.5f);
}

static float DistPointSegSq(float px, float py, PointF2 a, PointF2 b){
    float dx = b.x - a.x, dy = b.y - a.y;
    float len2 = dx * dx + dy * dy;
    if (len2 <= 1e-6f){
        float ex = px - a.x, ey = py - a.y;
        return ex * ex + ey * ey;
    }
    float t = ((px - a.x) * dx + (py - a.y) * dy) / len2;
    t = std::max(0.0f, std::min(1.0f, t));
    float qx = a.x + dx * t, qy = a.y + dy * t;
    float ex = px - qx, ey = py - qy;
    return ex * ex + ey * ey;
}

static bool ShapeHitByEraser(const Stroke& s, float x, float y, float er){
    float hitR = er + std::max(1.0f, s.width * 0.5f);
    float hit2 = hitR * hitR;

    auto segHit = [&](PointF2 a, PointF2 b) {
        return DistPointSegSq(x, y, a, b) <= hit2;
    };

    switch (s.shape){
    case ShapeKind::Line:
    case ShapeKind::Arrow:
        return segHit(s.a, s.b);
    case ShapeKind::Rect: {
        float l = std::min(s.a.x, s.b.x), r = std::max(s.a.x, s.b.x);
        float t = std::min(s.a.y, s.b.y), b = std::max(s.a.y, s.b.y);
        if (x < l - hitR || x > r + hitR || y < t - hitR || y > b + hitR) return false;
        return segHit({l,t}, {r,t}) || segHit({r,t}, {r,b}) ||
               segHit({r,b}, {l,b}) || segHit({l,b}, {l,t});
    }
    case ShapeKind::Triangle: {
        PointF2 p1{ (s.a.x + s.b.x) * 0.5f, s.a.y };
        PointF2 p2{ s.a.x, s.b.y };
        PointF2 p3{ s.b.x, s.b.y };
        return segHit(p1, p2) || segHit(p2, p3) || segHit(p3, p1);
    }
    case ShapeKind::Ellipse: {
        float l = std::min(s.a.x, s.b.x), r = std::max(s.a.x, s.b.x);
        float t = std::min(s.a.y, s.b.y), b = std::max(s.a.y, s.b.y);
        float rx = (r - l) * 0.5f, ry = (b - t) * 0.5f;
        if (rx <= 0.0f || ry <= 0.0f) return false;
        float cx = (l + r) * 0.5f, cy = (t + b) * 0.5f;
        if (x < l - hitR || x > r + hitR || y < t - hitR || y > b + hitR) return false;
        constexpr int kSamples = 72;
        PointF2 prev{ cx + rx, cy };
        for (int i = 1; i <= kSamples; ++i){
            float a = (float)i * 6.28318530718f / (float)kSamples;
            PointF2 curPt{ cx + std::cos(a) * rx, cy + std::sin(a) * ry };
            if (segHit(prev, curPt)) return true;
            prev = curPt;
        }
        return false;
    }
    default:
        return false;
    }
}

// Apply one eraser circle to strokes with centerline precision.
static void ApplyEraseAt(std::vector<Stroke>& strokes, float x, float y, float er){
    float circleR2 = er * er;

    auto segCrossCircle = [&](float ax, float ay, float bx, float by,
                              float& t1, float& t2) -> int {
        float dx = bx - ax, dy = by - ay;
        float fx = ax - x, fy = ay - y;
        float a = dx*dx + dy*dy;
        if (a < 1e-10f) return 0;
        float b = 2.0f*(fx*dx + fy*dy);
        float c = fx*fx + fy*fy - circleR2;
        float disc = b*b - 4.0f*a*c;
        if (disc < 0) return 0;
        float sq = std::sqrt(disc);
        float s1 = (-b - sq) / (2.0f*a);
        float s2 = (-b + sq) / (2.0f*a);
        int n = 0;
        float r[2];
        if (s1 > 1e-4f && s1 < 1.0f-1e-4f) r[n++] = s1;
        if (s2 > 1e-4f && s2 < 1.0f-1e-4f && std::fabs(s2-s1) > 1e-4f) r[n++] = s2;
        if (n >= 1) t1 = r[0];
        if (n >= 2) t2 = r[1];
        return n;
    };

    auto lerpPt = [](PointF2 a, PointF2 b, float t) -> PointF2 {
        return { a.x + t*(b.x - a.x), a.y + t*(b.y - a.y) };
    };

    std::vector<Stroke> out;
    out.reserve(strokes.size());

    for (auto& s : strokes) {
        if (s.isShape) {
            if (ShapeHitByEraser(s, x, y, er)) continue;
            out.push_back(std::move(s));
            continue;
        }

        float strokeEr = er + std::max(0.5f, s.width * 0.35f);
        float strokeEr2 = strokeEr * strokeEr;
        circleR2 = strokeEr2;

        float minX = 1e9f, minY = 1e9f, maxX = -1e9f, maxY = -1e9f;
        for (const auto& pt : s.pts){
            minX = std::min(minX, pt.x);
            minY = std::min(minY, pt.y);
            maxX = std::max(maxX, pt.x);
            maxY = std::max(maxY, pt.y);
        }
        if (x < minX - strokeEr || x > maxX + strokeEr ||
            y < minY - strokeEr || y > maxY + strokeEr){
            out.push_back(std::move(s));
            continue;
        }

        // Stroke erasing with circle-segment intersection.
        std::vector<PointF2> seg;
        bool anyModified = false;
        for (size_t i = 0; i < s.pts.size(); i++) {
            float ddx = s.pts[i].x - x, ddy = s.pts[i].y - y;
            bool inside = (ddx*ddx + ddy*ddy < strokeEr2);

            if (i == 0) {
                if (!inside) seg.push_back(s.pts[i]);
                else anyModified = true;
                continue;
            }

            float ddxp = s.pts[i-1].x - x, ddyp = s.pts[i-1].y - y;
            bool prevInside = (ddxp*ddxp + ddyp*ddyp < strokeEr2);

            if (!prevInside && !inside) {
                float ct1, ct2;
                int cn = segCrossCircle(s.pts[i-1].x, s.pts[i-1].y,
                                         s.pts[i].x, s.pts[i].y, ct1, ct2);
                if (cn == 2) {
                    anyModified = true;
                    seg.push_back(lerpPt(s.pts[i-1], s.pts[i], ct1));
                    if (seg.size() >= 2) {
                        Stroke ns = s; ns.pts = std::move(seg); out.push_back(std::move(ns));
                    }
                    seg.clear();
                    seg.push_back(lerpPt(s.pts[i-1], s.pts[i], ct2));
                    seg.push_back(s.pts[i]);
                } else {
                    seg.push_back(s.pts[i]);
                }
            } else if (prevInside && !inside) {
                anyModified = true;
                float ct1, ct2;
                segCrossCircle(s.pts[i-1].x, s.pts[i-1].y,
                               s.pts[i].x, s.pts[i].y, ct1, ct2);
                seg.push_back(lerpPt(s.pts[i-1], s.pts[i], ct1));
                seg.push_back(s.pts[i]);
            } else if (!prevInside && inside) {
                anyModified = true;
                float ct1, ct2;
                int cn = segCrossCircle(s.pts[i-1].x, s.pts[i-1].y,
                                         s.pts[i].x, s.pts[i].y, ct1, ct2);
                if (cn >= 1)
                    seg.push_back(lerpPt(s.pts[i-1], s.pts[i], ct1));
                if (seg.size() >= 2) {
                    Stroke ns = s; ns.pts = std::move(seg); out.push_back(std::move(ns));
                }
                seg.clear();
            } else {
                anyModified = true;
            }
        }
        if (!anyModified) {
            out.push_back(std::move(s));
        } else if (seg.size() >= 2) {
            Stroke ns = s; ns.pts = std::move(seg); out.push_back(std::move(ns));
        }
    }
    strokes = std::move(out);
}

void BeginErase(float x, float y){
    g.erasing = true;
    g.eraserHasErased = false;
    g.eraserTravel = 0.0f;
    g.preEraseStrokes = cur().strokes;
    g.eraserPath.clear();
    g.eraserPath.push_back({x, y});
}

void MoveErase(float x, float y){
    if (!g.erasing) return;
    if (g.eraserPath.empty()){
        g.eraserPath.push_back({x, y});
        return;
    }
    PointF2 last = g.eraserPath.back();
    float dx = x - last.x, dy = y - last.y;
    float dist = std::sqrt(dx*dx + dy*dy);
    if (dist < 0.5f) return;

    g.eraserPath.push_back({x, y});
    g.eraserTravel += dist;

    float er = EraserRadius();
    float armDistance = std::max(3.0f, er * 0.35f);
    if (!g.eraserHasErased){
        if (g.eraserTravel < armDistance) return;
        g.eraserHasErased = true;
    }

    float step = std::max(er * 0.45f, 1.5f);
    int n = std::max(1, (int)std::ceil(dist / step));
    for (int i = 1; i <= n; ++i){
        float t = (float)i / (float)n;
        PointF2 pt = {last.x + dx*t, last.y + dy*t};
        ApplyEraseAt(cur().strokes, pt.x, pt.y, er);
    }
}

void EndErase(){
    if (!g.erasing) return;
    g.erasing = false;

    if (!g.eraserHasErased){
        g.preEraseStrokes.clear();
        g.eraserPath.clear();
        g.eraserTravel = 0.0f;
        return;
    }

    // Strokes have already been modified in-place during MoveErase.
    // Push undo with the original snapshot (preEraseStrokes) as the undo state.
    auto& p = cur();
    // The undo should restore the pre-erase state
    UndoSnapshot undoSnap;
    undoSnap.strokes = std::move(g.preEraseStrokes);
    undoSnap.texts = p.texts;
    undoSnap.images = p.images;
    p.undoStack.push_back(std::move(undoSnap));
    p.redoStack.clear();
    g.eraserHasErased = false;
    g.eraserTravel = 0.0f;
    g.preEraseStrokes.clear();
    g.eraserPath.clear();
    g.thumbDirty[g.currentPage] = true;

    // Refresh stable bitmap since strokes changed
    g.stableW = 0; // force stable bitmap rebuild
}

// ====== Text ======

void AddText(float x, float y){
    TextItem t;
    t.uid = g.nextObjectUid++;
    t.pos = {x, y};
    t.color = g.color;
    t.fontSize = g.fontSize;
    t.family = g.fontFamily;
    cur().texts.push_back(t);
    g.editingTextIdx = (int)cur().texts.size() - 1;
    g.thumbDirty[g.currentPage] = true;

    if (g.hEdit){ DestroyWindow(g.hEdit); g.hEdit = nullptr; }
    RECT br = BoardRect(g.hWnd);
    int sx = (int)(x - g.scrollX + br.left);
    int sy = (int)(y - g.scrollY + br.top);
    int w = 400, h = (int)(g.fontSize * 1.6f) + 8;
    g.hEdit = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", L"",
        WS_CHILD|WS_VISIBLE|ES_AUTOHSCROLL,
        sx, sy, w, h,
        g.hWnd, (HMENU)0, g.hInst, nullptr);
    HFONT hFont = CreateFontW((int)(-g.fontSize), 0,0,0,
        FW_NORMAL, FALSE, FALSE, FALSE,
        DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, g.fontFamily.c_str());
    SendMessageW(g.hEdit, WM_SETFONT, (WPARAM)hFont, TRUE);
    SetFocus(g.hEdit);
}

void CommitTextEdit(){
    if (!g.hEdit || g.editingTextIdx < 0) return;
    int n = GetWindowTextLengthW(g.hEdit);
    std::wstring buf(n+1, L'\0');
    GetWindowTextW(g.hEdit, &buf[0], n+1);
    buf.resize(n);
    if (!buf.empty()){
        auto& p = cur();
        p.undoStack.push_back({p.strokes, p.texts, p.images});
        p.redoStack.clear();
        cur().texts[g.editingTextIdx].text = buf;
    } else {
        cur().texts.erase(cur().texts.begin() + g.editingTextIdx);
    }
    HFONT hFont = (HFONT)SendMessageW(g.hEdit, WM_GETFONT, 0, 0);
    DestroyWindow(g.hEdit);
    if (hFont) DeleteObject(hFont);
    g.hEdit = nullptr;
    g.editingTextIdx = -1;
    g.thumbDirty[g.currentPage] = true;
    InvalidateRect(g.hWnd, nullptr, FALSE);
}

// ====== Image ======

void AddImageFromFile(const std::wstring& path, float cx, float cy){
    Bitmap* raw = Bitmap::FromFile(path.c_str());
    if (!raw) return;
    if (raw->GetLastStatus() != Ok){ delete raw; return; }
    ImageItem it;
    it.uid = g.nextObjectUid++;
    it.bmp.reset(raw);
    float w = (float)raw->GetWidth();
    float h = (float)raw->GetHeight();
    float maxSide = 600;
    float scale = std::min(1.0f, maxSide / std::max(w,h));
    it.w = w * scale;
    it.h = h * scale;
    it.pos = { cx - it.w/2, cy - it.h/2 };
    auto& p = cur();
    p.undoStack.push_back({p.strokes, p.texts, p.images});
    p.redoStack.clear();
    cur().images.push_back(std::move(it));
    g.thumbDirty[g.currentPage] = true;
}

// ====== Undo / Redo / Clear ======

void Undo(){
    auto& p = cur();
    if (p.undoStack.empty()) return;
    g.penUndoGroupActive = false;
    p.redoStack.push_back({p.strokes, p.texts, p.images});
    auto snap = std::move(p.undoStack.back());
    p.undoStack.pop_back();
    p.strokes = std::move(snap.strokes);
    p.texts   = std::move(snap.texts);
    p.images  = std::move(snap.images);
    g.selectedItems.clear();
    g.thumbDirty[g.currentPage] = true;
    g.stableW = 0;
}

void Redo(){
    auto& p = cur();
    if (p.redoStack.empty()) return;
    g.penUndoGroupActive = false;
    p.undoStack.push_back({p.strokes, p.texts, p.images});
    auto snap = std::move(p.redoStack.back());
    p.redoStack.pop_back();
    p.strokes = std::move(snap.strokes);
    p.texts   = std::move(snap.texts);
    p.images  = std::move(snap.images);
    g.selectedItems.clear();
    g.thumbDirty[g.currentPage] = true;
    g.stableW = 0;
}

void ClearCurrent(){
    auto& p = cur();
    size_t firstUserImage = (g.pdfMode && !p.images.empty()) ? 1u : 0u;
    bool hasUserImages = p.images.size() > firstUserImage;
    if (p.strokes.empty() && p.texts.empty() && !hasUserImages) return;
    g.penUndoGroupActive = false;
    p.undoStack.push_back({p.strokes, p.texts, p.images});
    p.redoStack.clear();
    p.strokes.clear();
    p.texts.clear();
    if (firstUserImage == 0u){
        p.images.clear();
    } else if (p.images.size() > firstUserImage){
        p.images.erase(p.images.begin() + (ptrdiff_t)firstUserImage, p.images.end());
    }
    g.thumbDirty[g.currentPage] = true;
    g.selectedItems.clear();
    g.current = Stroke();
    g.activeDrawPage = -1;
    g.stableW = 0;
}

// ====== Selection / move ======

static void StrokeBounds(const Stroke& s, float& minX, float& minY, float& maxX, float& maxY) {
    if (s.isShape) {
        minX = std::min(s.a.x, s.b.x);
        minY = std::min(s.a.y, s.b.y);
        maxX = std::max(s.a.x, s.b.x);
        maxY = std::max(s.a.y, s.b.y);
        return;
    }
    minX = minY = 1e9f;
    maxX = maxY = -1e9f;
    for (auto& p : s.pts) {
        if (p.x < minX) minX = p.x;
        if (p.y < minY) minY = p.y;
        if (p.x > maxX) maxX = p.x;
        if (p.y > maxY) maxY = p.y;
    }
}

int HitTextAt(float x, float y){
    auto& p = cur();
    Graphics g_(g.hWnd);
    for (int i = (int)p.texts.size()-1; i>=0; --i){
        const auto& t = p.texts[i];
        FontFamily ff(t.family.c_str());
        Font font(&ff, t.fontSize, FontStyleRegular, UnitPixel);
        RectF bound;
        StringFormat sf;
        g_.MeasureString(t.text.c_str(), -1, &font, PointF(t.pos.x, t.pos.y), &sf, &bound);
        if (x>=bound.X && x<bound.X+bound.Width && y>=bound.Y && y<bound.Y+bound.Height)
            return i;
    }
    return -1;
}

static bool IsPdfPageImage(int pageIndex, int imageIndex){
    return g.pdfMode && imageIndex == 0 &&
           pageIndex >= 0 && pageIndex < (int)g.pages.size() &&
           !g.pdfPageYOffset.empty();
}

int HitImageAt(float x, float y){
    auto& p = cur();
    for (int i = (int)p.images.size()-1; i>=0; --i){
        if (IsPdfPageImage(g.currentPage, i)) continue;
        const auto& it = p.images[i];
        if (x>=it.pos.x && x<it.pos.x+it.w && y>=it.pos.y && y<it.pos.y+it.h)
            return i;
    }
    return -1;
}

void ClearSelection(){
    g.selectedItems.clear();
}

void SelectItemsInRect(float x1, float y1, float x2, float y2){
    float rx1 = std::min(x1, x2), ry1 = std::min(y1, y2);
    float rx2 = std::max(x1, x2), ry2 = std::max(y1, y2);
    g.selectedItems.clear();

    if (g.pdfMode){
        // Cross-page selection: search all pages whose Y range overlaps the selection rect
        for (int pi = 0; pi < (int)g.pages.size(); ++pi){
            float yOff = g.pdfPageYOffset[pi];
            float yEnd = (pi + 1 < (int)g.pdfPageYOffset.size()) ? g.pdfPageYOffset[pi + 1] : g.pdfTotalHeight;
            // Skip pages entirely outside the selection rect
            if (yEnd < ry1 || yOff > ry2) continue;

            auto& p = g.pages[pi];
            // Strokes
            for (int i = 0; i < (int)p.strokes.size(); ++i){
                float minX, minY, maxX, maxY;
                StrokeBounds(p.strokes[i], minX, minY, maxX, maxY);
                if (maxX >= rx1 && minX <= rx2 && maxY >= ry1 && minY <= ry2)
                    g.selectedItems.push_back({SelectedItem::Stroke, i, pi});
            }
            // Texts
            {
                Graphics g_(g.hWnd);
                for (int i = 0; i < (int)p.texts.size(); ++i){
                    const auto& t = p.texts[i];
                    FontFamily ff(t.family.c_str());
                    Font font(&ff, t.fontSize, FontStyleRegular, UnitPixel);
                    RectF bound;
                    StringFormat sf;
                    g_.MeasureString(t.text.c_str(), -1, &font, PointF(t.pos.x, t.pos.y), &sf, &bound);
                    if (bound.X + bound.Width >= rx1 && bound.X <= rx2 &&
                        bound.Y + bound.Height >= ry1 && bound.Y <= ry2)
                        g.selectedItems.push_back({SelectedItem::Text, i, pi});
                }
            }
            // Images
            for (int i = 0; i < (int)p.images.size(); ++i){
                if (IsPdfPageImage(pi, i)) continue;
                const auto& it = p.images[i];
                if (it.pos.x + it.w >= rx1 && it.pos.x <= rx2 &&
                    it.pos.y + it.h >= ry1 && it.pos.y <= ry2)
                    g.selectedItems.push_back({SelectedItem::Image, i, pi});
            }
        }
    } else {
        // Normal mode: single page
        auto& p = cur();
        int pi = g.currentPage;

        // Strokes
        for (int i = 0; i < (int)p.strokes.size(); ++i){
            float minX, minY, maxX, maxY;
            StrokeBounds(p.strokes[i], minX, minY, maxX, maxY);
            if (maxX >= rx1 && minX <= rx2 && maxY >= ry1 && minY <= ry2)
                g.selectedItems.push_back({SelectedItem::Stroke, i, pi});
        }
        // Texts
        {
            Graphics g_(g.hWnd);
            for (int i = 0; i < (int)p.texts.size(); ++i){
                const auto& t = p.texts[i];
                FontFamily ff(t.family.c_str());
                Font font(&ff, t.fontSize, FontStyleRegular, UnitPixel);
                RectF bound;
                StringFormat sf;
                g_.MeasureString(t.text.c_str(), -1, &font, PointF(t.pos.x, t.pos.y), &sf, &bound);
                if (bound.X + bound.Width >= rx1 && bound.X <= rx2 &&
                    bound.Y + bound.Height >= ry1 && bound.Y <= ry2)
                    g.selectedItems.push_back({SelectedItem::Text, i, pi});
            }
        }
        // Images
        for (int i = 0; i < (int)p.images.size(); ++i){
            if (IsPdfPageImage(pi, i)) continue;
            const auto& it = p.images[i];
            if (it.pos.x + it.w >= rx1 && it.pos.x <= rx2 &&
                it.pos.y + it.h >= ry1 && it.pos.y <= ry2)
                g.selectedItems.push_back({SelectedItem::Image, i, pi});
        }
    }
}

bool HitSelected(float x, float y){
    for (auto& sel : g.selectedItems){
        if (sel.pageIndex < 0 || sel.pageIndex >= (int)g.pages.size()) continue;
        auto& p = g.pages[sel.pageIndex];
        switch (sel.type){
        case SelectedItem::Stroke: {
            if (sel.index < 0 || sel.index >= (int)p.strokes.size()) break;
            float minX, minY, maxX, maxY;
            StrokeBounds(p.strokes[sel.index], minX, minY, maxX, maxY);
            if (x >= minX - 5 && x <= maxX + 5 && y >= minY - 5 && y <= maxY + 5)
                return true;
            break;
        }
        case SelectedItem::Text: {
            if (sel.index < 0 || sel.index >= (int)p.texts.size()) break;
            auto& t = p.texts[sel.index];
            Graphics g_(g.hWnd);
            FontFamily ff(t.family.c_str());
            Font font(&ff, t.fontSize, FontStyleRegular, UnitPixel);
            RectF bound;
            StringFormat sf;
            g_.MeasureString(t.text.c_str(), -1, &font, PointF(t.pos.x, t.pos.y), &sf, &bound);
            if (x >= bound.X && x <= bound.X + bound.Width &&
                y >= bound.Y && y <= bound.Y + bound.Height)
                return true;
            break;
        }
        case SelectedItem::Image: {
            if (sel.index < 0 || sel.index >= (int)p.images.size()) break;
            if (IsPdfPageImage(sel.pageIndex, sel.index)) break;
            auto& it = p.images[sel.index];
            if (x >= it.pos.x && x <= it.pos.x + it.w &&
                y >= it.pos.y && y <= it.pos.y + it.h)
                return true;
            break;
        }
        }
    }
    return false;
}

void MoveSelectedBy(float dx, float dy){
    for (auto& sel : g.selectedItems){
        if (sel.pageIndex < 0 || sel.pageIndex >= (int)g.pages.size()) continue;
        auto& p = g.pages[sel.pageIndex];
        switch (sel.type){
        case SelectedItem::Stroke:
            if (sel.index >= 0 && sel.index < (int)p.strokes.size()){
                auto& s = p.strokes[sel.index];
                for (auto& pt : s.pts){ pt.x += dx; pt.y += dy; }
                s.a.x += dx; s.a.y += dy;
                s.b.x += dx; s.b.y += dy;
            }
            break;
        case SelectedItem::Text:
            if (sel.index >= 0 && sel.index < (int)p.texts.size()){
                p.texts[sel.index].pos.x += dx;
                p.texts[sel.index].pos.y += dy;
            }
            break;
        case SelectedItem::Image:
            if (sel.index >= 0 && sel.index < (int)p.images.size()){
                if (IsPdfPageImage(sel.pageIndex, sel.index)) break;
                p.images[sel.index].pos.x += dx;
                p.images[sel.index].pos.y += dy;
            }
            break;
        }
        g.thumbDirty[sel.pageIndex] = true;
    }
}

void DeleteSelected(){
    if (g.selectedItems.empty()) return;
    // Group by page for undo/redo
    for (int pi = 0; pi < (int)g.pages.size(); ++pi){
        auto& p = g.pages[pi];
        std::vector<int> strokeIdx, textIdx, imageIdx;
        for (auto& sel : g.selectedItems){
            if (sel.pageIndex != pi) continue;
            switch (sel.type){
            case SelectedItem::Stroke: strokeIdx.push_back(sel.index); break;
            case SelectedItem::Text:   textIdx.push_back(sel.index); break;
            case SelectedItem::Image:
                if (!IsPdfPageImage(pi, sel.index)) imageIdx.push_back(sel.index);
                break;
            }
        }
        if (strokeIdx.empty() && textIdx.empty() && imageIdx.empty()) continue;
        p.undoStack.push_back({p.strokes, p.texts, p.images});
        p.redoStack.clear();
        std::sort(strokeIdx.rbegin(), strokeIdx.rend());
        std::sort(textIdx.rbegin(), textIdx.rend());
        std::sort(imageIdx.rbegin(), imageIdx.rend());
        for (int i : strokeIdx) if (i >= 0 && i < (int)p.strokes.size()) p.strokes.erase(p.strokes.begin()+i);
        for (int i : textIdx)   if (i >= 0 && i < (int)p.texts.size())   p.texts.erase(p.texts.begin()+i);
        for (int i : imageIdx)  if (i >= 0 && i < (int)p.images.size())  p.images.erase(p.images.begin()+i);
        g.thumbDirty[pi] = true;
    }
    g.selectedItems.clear();
}

void CopySelected(){
    g.clipboardStrokes.clear();
    g.clipboardTexts.clear();
    g.clipboardImages.clear();
    for (auto& sel : g.selectedItems){
        if (sel.pageIndex < 0 || sel.pageIndex >= (int)g.pages.size()) continue;
        auto& p = g.pages[sel.pageIndex];
        switch (sel.type){
        case SelectedItem::Stroke:
            if (sel.index >= 0 && sel.index < (int)p.strokes.size())
                g.clipboardStrokes.push_back(p.strokes[sel.index]);
            break;
        case SelectedItem::Text:
            if (sel.index >= 0 && sel.index < (int)p.texts.size())
                g.clipboardTexts.push_back(p.texts[sel.index]);
            break;
        case SelectedItem::Image:
            if (sel.index >= 0 && sel.index < (int)p.images.size() &&
                !IsPdfPageImage(sel.pageIndex, sel.index))
                g.clipboardImages.push_back(p.images[sel.index]);
            break;
        }
    }
}

void PasteClipboard(){
    if (g.clipboardStrokes.empty() && g.clipboardTexts.empty() && g.clipboardImages.empty()) return;
    auto& p = cur();
    p.undoStack.push_back({p.strokes, p.texts, p.images});
    p.redoStack.clear();

    g.selectedItems.clear();
    float offX = 20.0f, offY = 20.0f;

    for (auto& s : g.clipboardStrokes){
        Stroke ns = s;
        ns.uid = g.nextObjectUid++;
        for (auto& pt : ns.pts){ pt.x += offX; pt.y += offY; }
        ns.a.x += offX; ns.a.y += offY;
        ns.b.x += offX; ns.b.y += offY;
        int idx = (int)p.strokes.size();
        p.strokes.push_back(std::move(ns));
        g.selectedItems.push_back({SelectedItem::Stroke, idx, g.currentPage});
    }
    for (auto& t : g.clipboardTexts){
        TextItem nt = t;
        nt.uid = g.nextObjectUid++;
        nt.pos.x += offX; nt.pos.y += offY;
        int idx = (int)p.texts.size();
        p.texts.push_back(std::move(nt));
        g.selectedItems.push_back({SelectedItem::Text, idx, g.currentPage});
    }
    for (auto& im : g.clipboardImages){
        ImageItem ni = im;
        ni.uid = g.nextObjectUid++;
        ni.pos.x += offX; ni.pos.y += offY;
        int idx = (int)p.images.size();
        p.images.push_back(std::move(ni));
        g.selectedItems.push_back({SelectedItem::Image, idx, g.currentPage});
    }
    g.thumbDirty[g.currentPage] = true;
}

// ====== Pages ======

void ClampPdfScroll(){
    if (!g.pdfMode) return;
    RECT br2 = BoardRect(g.hWnd);
    float viewW = (float)(br2.right - br2.left);
    float viewH = (float)(br2.bottom - br2.top);
    float z = g.zoomLevel;
    float pageScreenWidth = g.pdfPageWidth * z;

    // Horizontal clamping
    if (pageScreenWidth <= viewW){
        // Page fits in viewport - center it
        float idealCenter = (pageScreenWidth - viewW) / 2.0f;
        g.scrollX = idealCenter;
    } else {
        // Page wider than viewport - clamp to page edges
        float maxSX = pageScreenWidth - viewW;
        if (g.scrollX < 0) g.scrollX = 0;
        if (g.scrollX > maxSX) g.scrollX = maxSX;
    }

    // Vertical clamping
    float contentScreenH = g.pdfTotalHeight * z;
    if (contentScreenH <= viewH){
        // Content fits in viewport - center it
        float idealCenter = (contentScreenH - viewH) / 2.0f;
        g.scrollY = idealCenter;
    } else {
        // Content taller than viewport - clamp to page edges
        float maxSY = contentScreenH - viewH;
        if (g.scrollY < 0) g.scrollY = 0;
        if (g.scrollY > maxSY) g.scrollY = maxSY;
    }
}

static int PdfViewportCenterPage(){
    if (!g.pdfMode || g.pdfPageYOffset.empty() || g.zoomLevel <= 0.0f) return ClampPageIndex(g.currentPage);
    RECT br2 = BoardRect(g.hWnd);
    float viewH = (float)(br2.bottom - br2.top);
    float boardCenterY = (g.scrollY + viewH * 0.5f) / g.zoomLevel;
    return ClampPageIndex(PdfPageAtY(boardCenterY));
}

static void ScrollToPdfPageCentered(int pageIdx){
    pageIdx = ClampPageIndex(pageIdx);
    if (!g.pdfMode || g.pdfPageYOffset.empty()){
        g.currentPage = pageIdx;
        return;
    }
    RECT br2 = BoardRect(g.hWnd);
    float viewHBoard = (float)(br2.bottom - br2.top) / std::max(0.25f, g.zoomLevel);
    float pageTop = g.pdfPageYOffset[pageIdx];
    float pageBottom = (pageIdx + 1 < (int)g.pdfPageYOffset.size())
        ? g.pdfPageYOffset[pageIdx + 1]
        : g.pdfTotalHeight;
    float pageH = std::max(1.0f, pageBottom - pageTop);
    float targetBoardY = pageTop + pageH * 0.5f - viewHBoard * 0.5f;
    g.scrollY = targetBoardY * g.zoomLevel;
    g.currentPage = pageIdx;
    ClampPdfScroll();
    g.currentPage = PdfViewportCenterPage();
    g.prevScrollY = g.scrollY;
}

void NextPage(){
    g.selectedItems.clear();
    if (g.pdfMode){
        int basePage = PdfViewportCenterPage();
        if (basePage + 1 < (int)g.pages.size()) ScrollToPdfPageCentered(basePage + 1);
        else ScrollToPdfPageCentered(basePage);
        return;
    }
    if (g.currentPage + 1 < (int)g.pages.size()){
        g.currentPage++;
    } else if ((int)g.pages.size() < kMaxPages){
        AddPage();
    }
}
void PrevPage(){
    g.selectedItems.clear();
    if (g.pdfMode){
        int basePage = PdfViewportCenterPage();
        if (basePage > 0) ScrollToPdfPageCentered(basePage - 1);
        else ScrollToPdfPageCentered(basePage);
        return;
    }
    if (g.currentPage > 0) g.currentPage--;
}
void GotoPage(int idx){
    g.selectedItems.clear();
    if (idx>=0 && idx<(int)g.pages.size()){
        g.currentPage = idx;
        if (g.pdfMode){
            ScrollToPdfPageCentered(idx);
        }
    }
}
void AddPage(){
    if (g.pdfMode) return;
    if ((int)g.pages.size() >= kMaxPages){
        ShowToast(L"Reached max 56 pages");
        return;
    }
    g.pages.emplace_back();
    g.thumbCache.push_back(nullptr);
    g.thumbDirty.push_back(true);
    g.currentPage = (int)g.pages.size() - 1;
}
void DelPage(){
    if (g.pdfMode) return;
    if (g.pages.size() <= 1){
        ShowToast(L"Keep at least one page");
        return;
    }
    g.pages.erase(g.pages.begin() + g.currentPage);
    g.thumbCache.erase(g.thumbCache.begin() + g.currentPage);
    g.thumbDirty.erase(g.thumbDirty.begin() + g.currentPage);
    if (g.currentPage >= (int)g.pages.size()) g.currentPage = (int)g.pages.size() - 1;
}

// ====== File opening helpers ======

bool HasAnyInk(){
    for (auto& pg : g.pages){
        if (!pg.strokes.empty() || !pg.texts.empty() || !pg.images.empty())
            return true;
    }
    return false;
}

// ====== PDF lazy page rendering ======
// Pages are rendered at actual physical size (PDF points × DPI/72).
// No viewport-dependent scaling - pages maintain true dimensions across window sizes.

static void RenderSinglePdfPage(int pageIdx){
    if (pageIdx < 0 || pageIdx >= (int)g.pages.size()) return;
    if (!g.pages[pageIdx].images.empty() && g.pages[pageIdx].images[0].bmp) return;
    if (!g.pdfDoc) return;

    PdfDoc* pdf = (PdfDoc*)g.pdfDoc;
    RECT br2 = BoardRect(g.hWnd);
    int viewW = br2.right - br2.left;
    float dpi = g.dpiScale * 96.0f;
    float pxPerPt = dpi / 72.0f;

    float pageW, pageH;
    int pw, ph;
    if (pdf->GetPageSize(pageIdx, &pageW, &pageH) && pageW > 0 && pageH > 0){
        pw = (int)(pageW * pxPerPt);
        ph = (int)(pageH * pxPerPt);
    } else {
        pw = viewW;
        ph = (int)(br2.bottom - br2.top);
    }
    int rw = std::max(1, (int)(pw * kPdfRenderScale));
    int rh = std::max(1, (int)(ph * kPdfRenderScale));
    Bitmap* pageBmp = pdf->RenderPage(pageIdx, rw, rh);
    if (pageBmp){
        if (!g.pages[pageIdx].images.empty())
            g.pages[pageIdx].images[0].bmp.reset(pageBmp);
        else
            g.pages[pageIdx].bgImage.reset(pageBmp);
        g.thumbDirty[pageIdx] = true;
    }
}

static void EnsurePdfPageRendered(int pageIdx){
    if (pageIdx < 0 || pageIdx >= (int)g.pages.size()) return;
    if (!g.pdfDoc) return;

    if (!g.renderQueue){
        // Fallback: synchronous rendering (no thread pool)
        if (g.pages[pageIdx].images.empty() || !g.pages[pageIdx].images[0].bmp) RenderSinglePdfPage(pageIdx);
        if (pageIdx > 0 && (g.pages[pageIdx-1].images.empty() || !g.pages[pageIdx-1].images[0].bmp)) RenderSinglePdfPage(pageIdx - 1);
        if (pageIdx + 1 < (int)g.pages.size() && (g.pages[pageIdx+1].images.empty() || !g.pages[pageIdx+1].images[0].bmp)) RenderSinglePdfPage(pageIdx + 1);
        return;
    }

    PdfDoc* pdf = (PdfDoc*)g.pdfDoc;
    int pageCount = (int)g.pages.size();

    auto makeTask = [&](int idx, int priority) -> RenderTask {
        int pw = 1;
        int ph = 1;
        if (!g.pages[idx].images.empty()){
            const auto& img = g.pages[idx].images[0];
            pw = std::max(1, (int)(img.w * kPdfRenderScale));
            ph = std::max(1, (int)(img.h * kPdfRenderScale));
        }
        return {idx, pw, ph, false, priority};
    };

    // ONLY submit visible page + immediate neighbors as urgent.
    // Do NOT submit all pages - that causes RAM explosion.
    std::vector<RenderTask> batch;
    for (int offset = -2; offset <= 2; ++offset){
        int idx = pageIdx + offset;
        if (idx < 0 || idx >= pageCount) continue;
        if (!g.pages[idx].images.empty() && g.pages[idx].images[0].bmp) continue;
        batch.push_back(makeTask(idx, -3000 + idx));
    }

    // Also submit the next few pages beyond buffer at medium priority
    for (int offset = 3; offset <= 6; ++offset){
        int idx = pageIdx + offset;
        if (idx < 0 || idx >= pageCount) continue;
        if (!g.pages[idx].images.empty() && g.pages[idx].images[0].bmp) continue;
        batch.push_back(makeTask(idx, idx));
    }

    if (!batch.empty())
        RenderQueue_SubmitUrgent((RenderQueue*)g.renderQueue, pdf, batch.data(), (int)batch.size());
}

// Evict rendered pages far from the current viewport to free memory
static void EvictDistantPages(int centerPage){
    int pageCount = (int)g.pages.size();
    int keepRadius = 15;  // keep pages within ±15 of current
    keepRadius = kPdfKeepRadius;
    int evicted = 0;
    for (int i = 0; i < pageCount; ++i){
        int dist = (i > centerPage) ? (i - centerPage) : (centerPage - i);
        if (dist > keepRadius && !g.pages[i].images.empty() && g.pages[i].images[0].bmp){
            ReleaseCachedTexture(g.pages[i].images[0].bmp.get());
            g.pages[i].images[0].bmp.reset();
            evicted++;
        }
    }
    g.pdfViewCenter = centerPage;
    if (evicted > 0 && g.renderQueue){
        // Also cancel far-away render tasks to free CPU
        RenderQueue_CancelLowPriority((RenderQueue*)g.renderQueue, centerPage + keepRadius + 100);
    }
}

void LoadPdf(const std::wstring& path){
    PdfDoc* pdf = new PdfDoc();
    if (!pdf->Load(path)){
        ShowToast(L"无法打开 PDF 文件");
        delete pdf;
        return;
    }

    int pageCount = pdf->GetPageCount();

    // Reject PDFs with more than 50 pages
    if (pageCount > 50){
        ShowToast(L"此文件页数过大（超过50页），无法加载");
        delete pdf;
        return;
    }

    // Clean up previous PDF document if any
    if (g.pdfDoc){
        PdfDoc* old = (PdfDoc*)g.pdfDoc;
        delete old;
        g.pdfDoc = nullptr;
    }
    g.pdfDoc = pdf;

    g.pdfMode = true;
    g.pdfPath = path;
    g.currentProjectPath.clear();

    // Reset state
    g.pages.clear();
    g.thumbCache.clear();
    g.thumbDirty.clear();
    g.selectedItems.clear();
    g.scrollX = 0;
    g.scrollY = 0;

    RECT br2 = BoardRect(g.hWnd);
    int viewW = br2.right - br2.left;
    float dpi = g.dpiScale * 96.0f;
    float pxPerPt = dpi / 72.0f;

    g.pdfPageYOffset.clear();
    float yOffset = 0.0f;
    bool firstPage = true;

    // Phase 1: Read sizes, render first 8 pages synchronously
    int visibleBatch = std::min(pageCount, 8);
    g.loadingActive = true;
    g.loadingText = L"正在渲染 PDF...";
    g.loadingTotal = visibleBatch;
    g.loadingProgress = 0;

    std::vector<float> pageSizesW, pageSizesH;
    for (int i = 0; i < pageCount; ++i){
        Page pg;
        float pageW, pageH;
        int pw, ph;
        if (pdf->GetPageSize(i, &pageW, &pageH) && pageW > 0 && pageH > 0){
            pw = (int)(pageW * pxPerPt);
            ph = (int)(pageH * pxPerPt);
            if (firstPage){ g.pdfPageWidth = (float)pw; firstPage = false; }
        } else {
            pw = viewW; ph = br2.bottom - br2.top;
            if (firstPage){ g.pdfPageWidth = (float)viewW; firstPage = false; }
        }
        pageSizesW.push_back((float)pw);
        pageSizesH.push_back((float)ph);
        g.pdfPageYOffset.push_back(yOffset);
        yOffset += (float)ph;

        ImageItem img;
        img.w = (float)pw;
        img.h = (float)ph;
        img.pos = { 0, g.pdfPageYOffset[i] };
        // Phase 1: render first batch synchronously at reduced scale
        if (i < visibleBatch){
            int rw = std::max(1, (int)(pw * kPdfRenderScale));
            int rh = std::max(1, (int)(ph * kPdfRenderScale));
            img.bmp.reset(pdf->RenderPage(i, rw, rh));
            g.loadingProgress = i + 1;
            PumpMessages();
        }
        // Phase 2: remaining pages will be rendered lazily on scroll
        pg.images.push_back(std::move(img));
        g.pages.push_back(std::move(pg));
    }

    g.pdfTotalHeight = yOffset;
    g.currentPage = 0;
    g.scrollX = (g.pdfPageWidth - (float)viewW) / 2.0f;

    g.thumbCache.assign(g.pages.size(), nullptr);
    g.thumbDirty.assign(g.pages.size(), true);
    ClampPdfScroll();

    g.loadingActive = false;
    g.stableW = 0;
    InvalidateRect(g.hWnd, nullptr, FALSE);
    ShowToast(L"已加载 PDF (" + std::to_wstring(pageCount) + L" 页)");
}

void RenderPdfPages(){
    if (!g.pdfMode || !g.pdfDoc) return;

    PdfDoc* pdf = (PdfDoc*)g.pdfDoc;
    RECT br2 = BoardRect(g.hWnd);
    int viewW = br2.right - br2.left;
    float dpi = g.dpiScale * 96.0f;
    float pxPerPt = dpi / 72.0f;
    g.pdfPageWidth = 0.0f;

    g.pdfPageYOffset.clear();
    float yOffset = 0.0f;
    bool firstPage = true;
    int pageCount = pdf->GetPageCount();

    g.loadingActive = true;
    g.loadingText = L"正在重新渲染 PDF...";
    g.loadingTotal = pageCount;
    g.loadingProgress = 0;

    for (int i = 0; i < pageCount && i < (int)g.pages.size(); ++i){
        float pageW, pageH;
        int pw, ph;
        if (pdf->GetPageSize(i, &pageW, &pageH) && pageW > 0 && pageH > 0){
            pw = (int)(pageW * pxPerPt);
            ph = (int)(pageH * pxPerPt);
            if (firstPage){ g.pdfPageWidth = (float)pw; firstPage = false; }
        } else {
            pw = viewW; ph = br2.bottom - br2.top;
            if (firstPage){ g.pdfPageWidth = (float)viewW; firstPage = false; }
        }
        g.pdfPageYOffset.push_back(yOffset);
        yOffset += (float)ph;

        // Re-render page via MuPDF at high resolution
        auto& pg = g.pages[i];
        pg.images.clear();
        ImageItem img;
        int rw = std::max(1, (int)(pw * kPdfRenderScale));
        int rh = std::max(1, (int)(ph * kPdfRenderScale));
        img.bmp.reset(pdf->RenderPage(i, rw, rh));
        img.w = (float)pw; // layout size stays at full res
        img.h = (float)ph;
        img.pos = { 0, g.pdfPageYOffset[i] };
        pg.images.push_back(std::move(img));
        g.thumbDirty[i] = true;

        g.loadingProgress = i + 1;
        PumpMessages();
    }
    g.pdfTotalHeight = yOffset;
    g.scrollX = (g.pdfPageWidth - (float)viewW) / 2.0f;
    ClampPdfScroll();
    g.loadingActive = false;
    g.stableW = 0; // force full redraw
    g.prevScrollX = g.scrollX;
    g.prevScrollY = g.scrollY;
    InvalidateRect(g.hWnd, nullptr, FALSE);
}

int PdfPageAtY(float boardY){
    for (int i = (int)g.pdfPageYOffset.size() - 1; i >= 0; --i){
        if (g.pdfPageYOffset[i] <= boardY) return i;
    }
    return 0;
}

void PrefetchVisiblePdfPages(){
    if (!g.pdfMode || !g.pdfDoc || g.pdfPageYOffset.empty()) return;
    RECT br2 = BoardRect(g.hWnd);
    int bh = br2.bottom - br2.top;
    if (bh <= 0 || g.zoomLevel <= 0.0f) return;

    float bTop = g.scrollY / g.zoomLevel;
    float bH = (float)bh / g.zoomLevel;
    int centerPage = PdfPageAtY(bTop + bH * 0.5f);
    if (centerPage < 0) centerPage = 0;
    if (centerPage >= (int)g.pages.size()) centerPage = (int)g.pages.size() - 1;
    if (centerPage != g.currentPage && !g.drawing && !g.shaping && !g.erasing && !g.draggingSelection){
        g.currentPage = centerPage;
    }
    EnsurePdfPageRendered(centerPage);
    EvictDistantPages(centerPage);
}

// ====== Rendering ======

static void DrawDashedPattern(Pen& pen, LineStyle ls){
    if (ls == LineStyle::Solid){ pen.SetDashStyle(DashStyleSolid); return; }
    if (ls == LineStyle::Dashed){
        REAL dash[] = { 6.0f, 4.0f };
        pen.SetDashPattern(dash, 2);
    } else {
        REAL dash[] = { 1.0f, 3.0f };
        pen.SetDashPattern(dash, 2);
    }
}

static float Len(PointF2 a, PointF2 b){
    float dx = b.x - a.x, dy = b.y - a.y;
    return std::sqrt(dx*dx + dy*dy);
}

static float DirDot(PointF2 a, PointF2 b, PointF2 c){
    float ux = b.x - a.x, uy = b.y - a.y;
    float vx = c.x - b.x, vy = c.y - b.y;
    float ul = std::sqrt(ux*ux + uy*uy);
    float vl = std::sqrt(vx*vx + vy*vy);
    if (ul < 0.001f || vl < 0.001f) return 1.0f;
    return (ux*vx + uy*vy) / (ul * vl);
}

static PointF2 Lerp(PointF2 a, PointF2 b, float t){
    return { a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t };
}

static float DistPointSeg(PointF2 p, PointF2 a, PointF2 b){
    float dx = b.x - a.x, dy = b.y - a.y;
    float len2 = dx*dx + dy*dy;
    if (len2 < 0.0001f) return Len(p, a);
    float t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
    t = std::max(0.0f, std::min(1.0f, t));
    return Len(p, Lerp(a, b, t));
}

static PointF2 CatmullRom(PointF2 p0, PointF2 p1, PointF2 p2, PointF2 p3, float t){
    float t2 = t * t, t3 = t2 * t;
    return {
        0.5f * ((2.0f * p1.x) + (-p0.x + p2.x) * t +
                (2.0f*p0.x - 5.0f*p1.x + 4.0f*p2.x - p3.x) * t2 +
                (-p0.x + 3.0f*p1.x - 3.0f*p2.x + p3.x) * t3),
        0.5f * ((2.0f * p1.y) + (-p0.y + p2.y) * t +
                (2.0f*p0.y - 5.0f*p1.y + 4.0f*p2.y - p3.y) * t2 +
                (-p0.y + 3.0f*p1.y - 3.0f*p2.y + p3.y) * t3)
    };
}

static std::vector<PointF2> BuildDisplayStrokePoints(const Stroke& s){
    const auto& src = s.pts;
    if (src.size() < 3) return src;

    std::vector<PointF2> out;
    out.reserve(std::min<size_t>(src.size() * 3, src.size() + 512));
    out.push_back(src.front());

    const float sharpDot = 0.62f; // about 52 degrees: keep corners crisp
    const float longGap = std::max(18.0f, s.width * 7.0f);
    const float maxBulge = std::max(1.2f, s.width * 0.35f);

    for (size_t i = 0; i + 1 < src.size(); ++i){
        PointF2 p0 = (i > 0) ? src[i - 1] : src[i];
        PointF2 p1 = src[i];
        PointF2 p2 = src[i + 1];
        PointF2 p3 = (i + 2 < src.size()) ? src[i + 2] : src[i + 1];

        float segLen = Len(p1, p2);
        bool sharpStart = (i > 0) && DirDot(p0, p1, p2) < sharpDot;
        bool sharpEnd = (i + 2 < src.size()) && DirDot(p1, p2, p3) < sharpDot;
        bool smooth = segLen <= longGap && !sharpStart && !sharpEnd;

        if (!smooth){
            out.push_back(p2);
            continue;
        }

        int steps = std::max(2, std::min(5, (int)(segLen / 4.0f) + 1));
        for (int step = 1; step <= steps; ++step){
            float t = (float)step / (float)steps;
            PointF2 q = CatmullRom(p0, p1, p2, p3, t);
            if (DistPointSeg(q, p1, p2) > maxBulge)
                q = Lerp(p1, p2, t);
            out.push_back(q);
        }
    }
    return out;
}

void DrawStroke(Graphics& g_, const Stroke& s){
    if (s.isShape){
        Pen pen(s.color, s.width);
        DrawDashedPattern(pen, s.style);
        pen.SetStartCap(LineCapRound); pen.SetEndCap(LineCapRound);
        pen.SetLineJoin(LineJoinRound);
        switch (s.shape){
        case ShapeKind::Rect:
            g_.DrawRectangle(&pen, s.a.x, s.a.y, s.b.x - s.a.x, s.b.y - s.a.y); break;
        case ShapeKind::Ellipse:
            g_.DrawEllipse(&pen, s.a.x, s.a.y, s.b.x - s.a.x, s.b.y - s.a.y); break;
        case ShapeKind::Line:
            g_.DrawLine(&pen, s.a.x, s.a.y, s.b.x, s.b.y); break;
        case ShapeKind::Arrow: {
            g_.DrawLine(&pen, s.a.x, s.a.y, s.b.x, s.b.y);
            float ang = std::atan2(s.b.y - s.a.y, s.b.x - s.a.x);
            float len = 18.0f + s.width * 1.5f, a1 = ang + 2.6f, a2 = ang - 2.6f;
            g_.DrawLine(&pen, s.b.x, s.b.y, s.b.x+std::cos(a1)*len, s.b.y+std::sin(a1)*len);
            g_.DrawLine(&pen, s.b.x, s.b.y, s.b.x+std::cos(a2)*len, s.b.y+std::sin(a2)*len);
        } break;
        case ShapeKind::Triangle: {
            PointF triPts[] = { PointF((s.a.x+s.b.x)*0.5f,s.a.y), PointF(s.a.x,s.b.y),
                PointF(s.b.x,s.b.y), PointF((s.a.x+s.b.x)*0.5f,s.a.y) };
            g_.DrawLines(&pen, triPts, 4);
        } break;
        default: break;
        }
        return;
    }
    if (s.pts.size() < 2) return;
    Pen pen(s.color, s.width);
    pen.SetStartCap(LineCapRound); pen.SetEndCap(LineCapRound);
    pen.SetLineJoin(LineJoinRound);
    DrawDashedPattern(pen, s.style);
    std::vector<PointF2> displayPts = BuildDisplayStrokePoints(s);
    std::vector<PointF> pts; pts.reserve(displayPts.size());
    for (auto& p : displayPts) pts.push_back(PointF(p.x, p.y));
    if (pts.size() >= 2) g_.DrawLines(&pen, pts.data(), (INT)pts.size());
}

void DrawAll(Graphics& g_, const RECT& boardRc){
    int W = boardRc.right - boardRc.left;
    int H = boardRc.bottom - boardRc.top;

    // Visible area in board coordinates (accounts for zoom)
    float bW = (float)W / g.zoomLevel;  // visible board width
    float bH = (float)H / g.zoomLevel;  // visible board height
    float bLeft = g.scrollX / g.zoomLevel;
    float bTop = g.scrollY / g.zoomLevel;

    // ===== PDF continuous scroll mode =====
    if (g.pdfMode){
        if (g.loadingActive) return;

        // Gray background
        SolidBrush bgGray(Color(255, 128, 128, 128));
        g_.FillRectangle(&bgGray, bLeft, bTop, bW, bH);

        float visTop = bTop;
        float visBot = bTop + bH;
        float layoutW = g.pdfPageWidth;
        PrefetchVisiblePdfPages();

        g_.SetInterpolationMode(InterpolationModeBilinear);
        for (int i = 0; i < (int)g.pages.size(); ++i){
            auto& pg = g.pages[i];
            float yOff = g.pdfPageYOffset[i];
            float yEnd = (i + 1 < (int)g.pdfPageYOffset.size()) ? g.pdfPageYOffset[i + 1] : g.pdfTotalHeight;
            if (yEnd < visTop || yOff > visBot) continue;

            float ph = yEnd - yOff;
            SolidBrush bgWhite(Color(255, 255, 255, 255));
            g_.FillRectangle(&bgWhite, 0.0f, yOff, layoutW, ph);

            auto& img = pg.images[0];
            if (img.bmp){
                float ox = (layoutW - img.w) / 2.0f;
                g_.DrawImage(img.bmp.get(), ox, yOff, img.w, img.h);
            } else {
                EnsurePdfPageRendered(i);
            }
        }

        g_.SetSmoothingMode(SmoothingModeAntiAlias);
        for (int i = 0; i < (int)g.pages.size(); ++i){
            auto& pg = g.pages[i];
            float yOff = g.pdfPageYOffset[i];
            float yEnd = (i + 1 < (int)g.pdfPageYOffset.size()) ? g.pdfPageYOffset[i + 1] : g.pdfTotalHeight;
            if (yEnd < visTop || yOff > visBot) continue;

            for (auto& s : pg.strokes) DrawStroke(g_, s);
            for (size_t ii = 1; ii < pg.images.size(); ++ii){
                auto& it = pg.images[ii];
                if (it.bmp) g_.DrawImage(it.bmp.get(), it.pos.x, it.pos.y, it.w, it.h);
            }
            for (size_t ti = 0; ti < pg.texts.size(); ++ti){
                const auto& t = pg.texts[ti];
                DrawMixedBoardText(g_, t.text, PointF(t.pos.x, t.pos.y), t.fontSize, t.color);
            }
        }

        if (g.drawing && g.tool == ToolKind::Pen && !g.current.pts.empty()){
            g_.SetSmoothingMode(SmoothingModeAntiAlias);
            DrawStroke(g_, g.current);
        }

        if (g.erasing){
            SolidBrush eb(Color(40, 200, 200, 200));
            float ex = g.current.pts.empty() ? 0 : g.current.pts.back().x;
            float ey = g.current.pts.empty() ? 0 : g.current.pts.back().y;
            float er = EraserRadius();
            g_.FillEllipse(&eb, ex - er, ey - er, er*2, er*2);
        }

        if (g.shaping){
            Stroke s;
            s.color = g.color; s.width = g.penSize; s.style = g.lineStyle;
            s.isShape = true; s.shape = g.currentShape;
            s.a = { std::min(g.shapeStart.x, g.shapeEnd.x), std::min(g.shapeStart.y, g.shapeEnd.y) };
            s.b = { std::max(g.shapeStart.x, g.shapeEnd.x), std::max(g.shapeStart.y, g.shapeEnd.y) };
            if (g.currentShape == ShapeKind::Line || g.currentShape == ShapeKind::Arrow){
                s.a = g.shapeStart; s.b = g.shapeEnd;
            }
            g_.SetSmoothingMode(SmoothingModeAntiAlias);
            DrawStroke(g_, s);
        }

        // Selection outlines (coordinates are already global)
        Pen selPen(Color(220, 79, 195, 247), 1.5f);
        REAL selDash[] = {4,3}; selPen.SetDashPattern(selDash, 2);
        for (auto& sel : g.selectedItems){
            if (sel.pageIndex < 0 || sel.pageIndex >= (int)g.pages.size()) continue;
            auto& pg = g.pages[sel.pageIndex];
            switch (sel.type){
            case SelectedItem::Stroke: {
                if (sel.index < 0 || sel.index >= (int)pg.strokes.size()) break;
                float minX, minY, maxX, maxY;
                StrokeBounds(pg.strokes[sel.index], minX, minY, maxX, maxY);
                g_.DrawRectangle(&selPen,
                    minX-3, minY-3,
                    maxX-minX+6, maxY-minY+6);
                break;
            }
            case SelectedItem::Text: {
                if (sel.index < 0 || sel.index >= (int)pg.texts.size()) break;
                const auto& t = pg.texts[sel.index];
                FontFamily ff(t.family.c_str());
                Font font(&ff, t.fontSize, FontStyleRegular, UnitPixel);
                RectF bound;
                StringFormat sf;
                g_.MeasureString(t.text.c_str(), -1, &font,
                    PointF(t.pos.x, t.pos.y), &sf, &bound);
                g_.DrawRectangle(&selPen, bound.X-2, bound.Y-2, bound.Width+4, bound.Height+4);
                break;
            }
            case SelectedItem::Image: {
                if (sel.index < 0 || sel.index >= (int)pg.images.size()) break;
                if (IsPdfPageImage(sel.pageIndex, sel.index)) break;
                const auto& it = pg.images[sel.index];
                g_.DrawRectangle(&selPen, it.pos.x, it.pos.y, it.w, it.h);
                break;
            }
            }
        }

        // Rubber band
        if (g.rubberBand){
            float rx1 = std::min(g.rubberStart.x, g.rubberEnd.x);
            float ry1 = std::min(g.rubberStart.y, g.rubberEnd.y);
            float rx2 = std::max(g.rubberStart.x, g.rubberEnd.x);
            float ry2 = std::max(g.rubberStart.y, g.rubberEnd.y);
            SolidBrush rbFill(Color(30, 91, 141, 239));
            g_.FillRectangle(&rbFill, rx1, ry1, rx2-rx1, ry2-ry1);
            Pen rbPen(Color(180, 91, 141, 239), 1.0f);
            g_.DrawRectangle(&rbPen, rx1, ry1, rx2-rx1, ry2-ry1);
        }
        return;
    }

    // ===== Normal (non-PDF) mode =====
    SolidBrush bg(Color(255, 255, 255, 255));
    g_.FillRectangle(&bg, bLeft, bTop, bW, bH);

    auto& p = cur();
    if (p.bgImage){
        g_.DrawImage(p.bgImage.get(), RectF(0, 0, (REAL)p.bgImage->GetWidth(), (REAL)p.bgImage->GetHeight()));
    }
    for (auto& it : p.images){
        if (it.bmp) g_.DrawImage(it.bmp.get(), it.pos.x, it.pos.y, it.w, it.h);
    }
    g_.SetSmoothingMode(SmoothingModeAntiAlias);
    for (auto& s : p.strokes) DrawStroke(g_, s);

    // Current stroke preview (D2D)
    if (g.drawing && g.tool == ToolKind::Pen){
        DrawStroke(g_, g.current);
    }
    // Eraser cursor preview
    if (g.erasing){
        SolidBrush eb(Color(40, 200, 200, 200));
        float ex = g.current.pts.empty() ? 0 : g.current.pts.back().x;
        float ey = g.current.pts.empty() ? 0 : g.current.pts.back().y;
        float er = EraserRadius();
        g_.FillEllipse(&eb, ex - er, ey - er, er*2, er*2);
    }

    // Shape preview
    if (g.shaping){
        Stroke s;
        s.color = g.color; s.width = g.penSize; s.style = g.lineStyle;
        s.isShape = true; s.shape = g.currentShape;
        s.a = { std::min(g.shapeStart.x, g.shapeEnd.x), std::min(g.shapeStart.y, g.shapeEnd.y) };
        s.b = { std::max(g.shapeStart.x, g.shapeEnd.x), std::max(g.shapeStart.y, g.shapeEnd.y) };
        if (g.currentShape == ShapeKind::Line || g.currentShape == ShapeKind::Arrow){
            s.a = g.shapeStart; s.b = g.shapeEnd;
        }
        DrawStroke(g_, s);
    }

    // Texts
    for (size_t i = 0; i < p.texts.size(); ++i){
        const auto& t = p.texts[i];
        DrawMixedBoardText(g_, t.text, PointF(t.pos.x, t.pos.y), t.fontSize, t.color);
    }

    // Selection outlines
    Pen selPen3(Color(220, 79, 195, 247), 1.5f);
    REAL selDash3[] = {4,3}; selPen3.SetDashPattern(selDash3, 2);
    for (auto& sel : g.selectedItems){
        if (sel.pageIndex != g.currentPage && g.pdfMode) continue;
        switch (sel.type){
        case SelectedItem::Stroke: {
            if (sel.index < 0 || sel.index >= (int)p.strokes.size()) break;
            float minX, minY, maxX, maxY;
            StrokeBounds(p.strokes[sel.index], minX, minY, maxX, maxY);
            g_.DrawRectangle(&selPen3, minX-3, minY-3, maxX-minX+6, maxY-minY+6);
            break;
        }
        case SelectedItem::Text: {
            if (sel.index < 0 || sel.index >= (int)p.texts.size()) break;
            const auto& t = p.texts[sel.index];
            // DirectWrite doesn't easily give us bounds; estimate from fontSize
            float estW = (float)t.text.size() * t.fontSize * 0.6f;
            g_.DrawRectangle(&selPen3, t.pos.x-2, t.pos.y-2, t.fontSize*0.6f+4, t.fontSize+8);
            break;
        }
        case SelectedItem::Image: {
            if (sel.index < 0 || sel.index >= (int)p.images.size()) break;
            const auto& it = p.images[sel.index];
            g_.DrawRectangle(&selPen3, it.pos.x, it.pos.y, it.w, it.h);
            break;
        }
        }
    }

    // Rubber band rectangle (D2D)
    if (g.rubberBand){
        float rx1 = std::min(g.rubberStart.x, g.rubberEnd.x);
        float ry1 = std::min(g.rubberStart.y, g.rubberEnd.y);
        float rx2 = std::max(g.rubberStart.x, g.rubberEnd.x);
        float ry2 = std::max(g.rubberStart.y, g.rubberEnd.y);
        SolidBrush rbFill(Color(30, 91, 141, 239));
        g_.FillRectangle(&rbFill, rx1, ry1, rx2-rx1, ry2-ry1);
        Pen rbPen(Color(180, 91, 141, 239), 1.0f);
        g_.DrawRectangle(&rbPen, rx1, ry1, rx2-rx1, ry2-ry1);
    }
}

void DrawPageToBitmap(int pageIdx, Bitmap& bmp){
    Graphics g_(&bmp);
    g_.SetSmoothingMode(SmoothingModeAntiAlias);

    if (g.pdfMode && pageIdx < (int)g.pages.size()){
        REAL bw = (REAL)bmp.GetWidth(), bh = (REAL)bmp.GetHeight();

        // For thumbnails (small bitmaps), render directly at thumbnail resolution
        // instead of full-res - downscale for much better performance
        if (bw <= 200 && g.pdfDoc){
            PdfDoc* pdf = (PdfDoc*)g.pdfDoc;
            // Calculate thumbnail aspect ratio
            float pageW, pageH;
            int tw = (int)bw, th = (int)bh;
            if (pdf->GetPageSize(pageIdx, &pageW, &pageH) && pageW > 0 && pageH > 0){
                th = (int)(tw / (pageW / pageH));
                if (th > (int)bh) { th = (int)bh; tw = (int)(th * (pageH / pageW)); }
            }
            Bitmap* pageBmp = pdf->RenderPage(pageIdx, tw, th);
            if (pageBmp){
                g_.SetInterpolationMode(InterpolationModeHighQualityBicubic);
                SolidBrush bg(Color(255, 255, 255, 255));
                g_.FillRectangle(&bg, 0.0f, 0.0f, bw, bh);
                // Center the thumbnail
                float ox = (bw - tw) / 2.0f, oy = (bh - th) / 2.0f;
                g_.DrawImage(pageBmp, ox, oy, (REAL)tw, (REAL)th);
                delete pageBmp;
                return;
            }
        }

        // For full-resolution rendering, use the page image or bgImage
        auto& pg = g.pages[pageIdx];
        if (g.pdfMode && !pg.images.empty() && pg.images[0].bmp){
            g_.SetInterpolationMode(InterpolationModeHighQualityBicubic);
            SolidBrush bg(Color(255, 255, 255, 255));
            g_.FillRectangle(&bg, 0.0f, 0.0f, bw, bh);
            g_.DrawImage(pg.images[0].bmp.get(), 0.0f, 0.0f, bw, bh);
            return;
        }
        if (!pg.bgImage) EnsurePdfPageRendered(pageIdx);
        if (pg.bgImage){
            g_.SetInterpolationMode(InterpolationModeHighQualityBicubic);
            SolidBrush bg(Color(255, 255, 255, 255));
            g_.FillRectangle(&bg, 0.0f, 0.0f, bw, bh);
            g_.DrawImage(pg.bgImage.get(), 0.0f, 0.0f, bw, bh);
            return;
        }
    }

    int saveIdx = g.currentPage;
    float saveSX = g.scrollX, saveSY = g.scrollY;
    g.currentPage = pageIdx;
    g.scrollX = 0; g.scrollY = 0;
    RECT rc = { 0, 0, (LONG)bmp.GetWidth(), (LONG)bmp.GetHeight() };
    DrawAll(g_, rc);
    g.currentPage = saveIdx;
    g.scrollX = saveSX; g.scrollY = saveSY;
}

// ====== Save / Export ======

static int GetEncoderClsid(const WCHAR* format, CLSID* pClsid){
    UINT num = 0, size = 0;
    GetImageEncodersSize(&num, &size);
    if (size == 0) return -1;
    ImageCodecInfo* info = (ImageCodecInfo*)malloc(size);
    if (!info) return -1;
    GetImageEncoders(num, size, info);
    int ret = -1;
    for (UINT i = 0; i < num; ++i){
        if (wcscmp(info[i].MimeType, format) == 0){
            *pClsid = info[i].Clsid;
            ret = (int)i; break;
        }
    }
    free(info);
    return ret;
}

void ExportCurrentPNG(const std::wstring& path){
    RECT br = BoardRect(g.hWnd);
    int W = br.right - br.left, H = br.bottom - br.top;
    if (W <= 0 || H <= 0) return;
    Bitmap bmp(W, H, PixelFormat32bppARGB);
    DrawPageToBitmap(g.currentPage, bmp);
    CLSID clsid;
    if (GetEncoderClsid(L"image/png", &clsid) >= 0){
        bmp.Save(path.c_str(), &clsid, nullptr);
    }
}

void AutoSave(){
    wchar_t buf[MAX_PATH];
    if (FAILED(SHGetFolderPathW(nullptr, CSIDL_LOCAL_APPDATA, nullptr, 0, buf))) return;
    std::wstring dir = buf;
    dir += L"\\CloudBoard";
    CreateDirectoryW(dir.c_str(), nullptr);
    dir += L"\\autosave";
    CreateDirectoryW(dir.c_str(), nullptr);
    SYSTEMTIME st; GetLocalTime(&st);
    wchar_t name[64];
    swprintf(name, 64, L"\\page_%d_%04d%02d%02d_%02d%02d%02d.png",
        g.currentPage+1, st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
    std::wstring path = dir + name;
    ExportCurrentPNG(path);
}

// ====== OpenGL board rendering ======

static GLuint GetOrCreateTexture(Gdiplus::Bitmap* bmp){
    if (!bmp) return 0;
    void* key = (void*)bmp;
    auto it = s_texCache.find(key);
    if (it != s_texCache.end()) return it->second;

    int w = bmp->GetWidth(), h = bmp->GetHeight();
    Gdiplus::PixelFormat fmt = bmp->GetPixelFormat();
    // Use the bitmap's native format for LockBits to avoid conversion issues
    Gdiplus::BitmapData bd;
    Gdiplus::Rect rc(0, 0, w, h);
    if (bmp->LockBits(&rc, Gdiplus::ImageLockModeRead, fmt, &bd) != Gdiplus::Ok)
        return 0;

    // Determine GL format based on pixel format
    GLenum glFmt = GL_BGRA; // default: ARGB little-endian = BGRA
    if (fmt == PixelFormat32bppRGB){
        // PDF bitmaps: BGRx (no alpha). Upload as RGB, set alpha=1 in shader won't work,
        // but for BGRA upload the X channel becomes A=255 which is correct for opaque.
        glFmt = GL_BGRA;
    }

    GLuint tex;
    glGenTextures(1, &tex);
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, glFmt, GL_UNSIGNED_BYTE, bd.Scan0);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);

    bmp->UnlockBits(&bd);
    s_texCache[key] = tex;
    return tex;
}

static void ClearTexCache(){
    wglMakeCurrent(glc.hDC, glc.hRC);
    for (auto& kv : s_texCache) glDeleteTextures(1, &kv.second);
    s_texCache.clear();
    wglMakeCurrent(nullptr, nullptr);
}

static void DrawStrokeGL(const Stroke& s){
    if (s.isShape){
        float sw = s.width;
        bool ds = (s.style != LineStyle::Solid);
        switch (s.shape){
        case ShapeKind::Rect:
            GLDrawShapeRect(s.a.x, s.a.y, s.b.x - s.a.x, s.b.y - s.a.y, s.color, sw, ds); break;
        case ShapeKind::Ellipse:
            GLDrawShapeEllipse(s.a.x, s.a.y, s.b.x - s.a.x, s.b.y - s.a.y, s.color, sw, ds); break;
        case ShapeKind::Line:
            GLDrawShapeLine(s.a.x, s.a.y, s.b.x, s.b.y, s.color, sw); break;
        case ShapeKind::Arrow:
            GLDrawShapeArrow(s.a.x, s.a.y, s.b.x, s.b.y, s.color, sw); break;
        case ShapeKind::Triangle:
            GLDrawShapeTriangle(s.a.x, s.a.y, s.b.x, s.b.y, s.color, sw, ds); break;
        default: break;
        }
        return;
    }
    if (s.pts.size() < 2) return;
    std::vector<PointF2> displayPts = BuildDisplayStrokePoints(s);
    GLDrawStroke(displayPts, s.color, s.width, s.style != LineStyle::Solid);
}

void BoardDrawGL(const RECT& boardRc){
    int bw = boardRc.right - boardRc.left;
    int bh = boardRc.bottom - boardRc.top;
    if (bw <= 0 || bh <= 0) return;

    GLBeginFrame(bw, bh);

    // Apply same transform as GDI+: Scale(zoom) * Translate(boardLeft - scrollX, boardTop - scrollY)
    GLPushTransform(g.zoomLevel, g.zoomLevel, (float)boardRc.left - g.scrollX, (float)boardRc.top - g.scrollY);

    // Visible area in board coordinates
    float bW = (float)bw / g.zoomLevel;
    float bH = (float)bh / g.zoomLevel;
    float bLeft = g.scrollX / g.zoomLevel;
    float bTop = g.scrollY / g.zoomLevel;

    if (g.pdfMode){
        // PDF continuous scroll mode
        Color grayBg(255, 128, 128, 128);
        GLFillRect(bLeft, bTop, bW, bH, grayBg);

        float visTop = bTop, visBot = bTop + bH;
        float layoutW = g.pdfPageWidth;
        PrefetchVisiblePdfPages();

        for (int i = 0; i < (int)g.pages.size(); ++i){
            auto& pg = g.pages[i];
            float yOff = g.pdfPageYOffset[i];
            float yEnd = (i + 1 < (int)g.pdfPageYOffset.size()) ? g.pdfPageYOffset[i + 1] : g.pdfTotalHeight;
            if (yEnd < visTop || yOff > visBot) continue;

            float ph = yEnd - yOff;

            // White page background
            Color white(255, 255, 255, 255);
            GLFillRect(0.0f, yOff, layoutW, ph, white);

            // PDF page image
            if (!pg.images.empty()){
                auto& img = pg.images[0];
                if (img.bmp){
                    GLuint tex = GetOrCreateTexture(img.bmp.get());
                    if (tex){
                        float ox = (layoutW - img.w) / 2.0f;
                        GLDrawTexture(tex, ox, yOff, img.w, img.h, true);
                    }
                } else {
                    EnsurePdfPageRendered(i);
                }
            }
        }

        // Annotations (strokes + user images)
        for (int i = 0; i < (int)g.pages.size(); ++i){
                auto& pg = g.pages[i];
                float yOff = g.pdfPageYOffset[i];
                float yEnd = (i + 1 < (int)g.pdfPageYOffset.size()) ? g.pdfPageYOffset[i + 1] : g.pdfTotalHeight;
                if (yEnd < visTop || yOff > visBot) continue;

                for (auto& s : pg.strokes) DrawStrokeGL(s);
                for (size_t ii = 1; ii < pg.images.size(); ++ii){
                    auto& it = pg.images[ii];
                    if (it.bmp){
                        GLuint tex = GetOrCreateTexture(it.bmp.get());
                        if (tex) GLDrawTexture(tex, it.pos.x, it.pos.y, it.w, it.h, true);
                    }
                }
            // Text items are handled by GDI+ overlay after GL compositing
        }

        // Current stroke preview
        if (g.drawing && g.tool == ToolKind::Pen && !g.current.pts.empty()){
            DrawStrokeGL(g.current);
        }

        // Eraser cursor
        if (g.erasing){
            float ex = g.current.pts.empty() ? 0 : g.current.pts.back().x;
            float ey = g.current.pts.empty() ? 0 : g.current.pts.back().y;
            Color eCol(40, 200, 200, 200);
            float er = EraserRadius();
            GLFillEllipse(ex, ey, er, er, eCol);
        }

        // Shape preview
        if (g.shaping){
            Stroke s;
            s.color = g.color; s.width = g.penSize; s.style = g.lineStyle;
            s.isShape = true; s.shape = g.currentShape;
            s.a = { std::min(g.shapeStart.x, g.shapeEnd.x), std::min(g.shapeStart.y, g.shapeEnd.y) };
            s.b = { std::max(g.shapeStart.x, g.shapeEnd.x), std::max(g.shapeStart.y, g.shapeEnd.y) };
            if (g.currentShape == ShapeKind::Line || g.currentShape == ShapeKind::Arrow){
                s.a = g.shapeStart; s.b = g.shapeEnd;
            }
            DrawStrokeGL(s);
        }

        // Selection outlines
        Color selCol(220, 79, 195, 247);
        for (auto& sel : g.selectedItems){
            if (sel.pageIndex < 0 || sel.pageIndex >= (int)g.pages.size()) continue;
            auto& pg = g.pages[sel.pageIndex];
            switch (sel.type){
            case SelectedItem::Stroke: {
                if (sel.index < 0 || sel.index >= (int)pg.strokes.size()) break;
                float minX, minY, maxX, maxY;
                StrokeBounds(pg.strokes[sel.index], minX, minY, maxX, maxY);
                GLDrawDashedRect(minX-3, minY-3, maxX-minX+6, maxY-minY+6, selCol, 1.5f);
                break;
            }
            case SelectedItem::Text: {
                if (sel.index < 0 || sel.index >= (int)pg.texts.size()) break;
                const auto& t = pg.texts[sel.index];
                GLDrawDashedRect(t.pos.x-2, t.pos.y-2, t.fontSize*0.6f+4, t.fontSize+8, selCol, 1.5f);
                break;
            }
            case SelectedItem::Image: {
                if (sel.index < 0 || sel.index >= (int)pg.images.size()) break;
                if (IsPdfPageImage(sel.pageIndex, sel.index)) break;
                const auto& it = pg.images[sel.index];
                GLDrawDashedRect(it.pos.x, it.pos.y, it.w, it.h, selCol, 1.5f);
                break;
            }
            }
        }

        // Rubber band
        if (g.rubberBand){
            float rx1 = std::min(g.rubberStart.x, g.rubberEnd.x);
            float ry1 = std::min(g.rubberStart.y, g.rubberEnd.y);
            float rx2 = std::max(g.rubberStart.x, g.rubberEnd.x);
            float ry2 = std::max(g.rubberStart.y, g.rubberEnd.y);
            Color rbFill(30, 91, 141, 239);
            Color rbBorder(180, 91, 141, 239);
            GLDrawRubberBand(rx1, ry1, rx2-rx1, ry2-ry1, rbFill, rbBorder, 1.0f);
        }
    } else {
        // Normal (non-PDF) mode
        Color white(255, 255, 255, 255);
        GLFillRect(bLeft, bTop, bW, bH, white);

        auto& p = cur();
        if (p.bgImage){
            GLuint tex = GetOrCreateTexture(p.bgImage.get());
            if (tex) GLDrawTexture(tex, 0, 0,
                (float)p.bgImage->GetWidth(), (float)p.bgImage->GetHeight(), true);
        }
        for (auto& it : p.images){
            if (it.bmp){
                GLuint tex = GetOrCreateTexture(it.bmp.get());
                if (tex) GLDrawTexture(tex, it.pos.x, it.pos.y, it.w, it.h, true);
            }
        }
        for (auto& s : p.strokes) DrawStrokeGL(s);

        // Current stroke preview
        if (g.drawing && g.tool == ToolKind::Pen){
            DrawStrokeGL(g.current);
        }

        // Eraser cursor
        if (g.erasing){
            float ex = g.current.pts.empty() ? 0 : g.current.pts.back().x;
            float ey = g.current.pts.empty() ? 0 : g.current.pts.back().y;
            Color eCol(40, 200, 200, 200);
            float er = EraserRadius();
            GLFillEllipse(ex, ey, er, er, eCol);
        }

        // Shape preview
        if (g.shaping){
            Stroke s;
            s.color = g.color; s.width = g.penSize; s.style = g.lineStyle;
            s.isShape = true; s.shape = g.currentShape;
            s.a = { std::min(g.shapeStart.x, g.shapeEnd.x), std::min(g.shapeStart.y, g.shapeEnd.y) };
            s.b = { std::max(g.shapeStart.x, g.shapeEnd.x), std::max(g.shapeStart.y, g.shapeEnd.y) };
            if (g.currentShape == ShapeKind::Line || g.currentShape == ShapeKind::Arrow){
                s.a = g.shapeStart; s.b = g.shapeEnd;
            }
            DrawStrokeGL(s);
        }

        // Selection outlines
        Color selCol3(220, 79, 195, 247);
        for (auto& sel : g.selectedItems){
            if (sel.pageIndex != g.currentPage && g.pdfMode) continue;
            switch (sel.type){
            case SelectedItem::Stroke: {
                if (sel.index < 0 || sel.index >= (int)p.strokes.size()) break;
                float minX, minY, maxX, maxY;
                StrokeBounds(p.strokes[sel.index], minX, minY, maxX, maxY);
                GLDrawDashedRect(minX-3, minY-3, maxX-minX+6, maxY-minY+6, selCol3, 1.5f);
                break;
            }
            case SelectedItem::Text: {
                if (sel.index < 0 || sel.index >= (int)p.texts.size()) break;
                const auto& t = p.texts[sel.index];
                GLDrawDashedRect(t.pos.x-2, t.pos.y-2, t.fontSize*0.6f+4, t.fontSize+8, selCol3, 1.5f);
                break;
            }
            case SelectedItem::Image: {
                if (sel.index < 0 || sel.index >= (int)p.images.size()) break;
                const auto& it = p.images[sel.index];
                GLDrawDashedRect(it.pos.x, it.pos.y, it.w, it.h, selCol3, 1.5f);
                break;
            }
            }
        }

        // Rubber band
        if (g.rubberBand){
            float rx1 = std::min(g.rubberStart.x, g.rubberEnd.x);
            float ry1 = std::min(g.rubberStart.y, g.rubberEnd.y);
            float rx2 = std::max(g.rubberStart.x, g.rubberEnd.x);
            float ry2 = std::max(g.rubberStart.y, g.rubberEnd.y);
            Color rbFill(30, 91, 141, 239);
            Color rbBorder(180, 91, 141, 239);
            GLDrawRubberBand(rx1, ry1, rx2-rx1, ry2-ry1, rbFill, rbBorder, 1.0f);
        }
    }

    GLPopTransform();
    GLEndFrame();
}

void DrawTextsGDI(Graphics& gr, const RECT& boardRc){
    int bw = boardRc.right - boardRc.left;
    int bh = boardRc.bottom - boardRc.top;
    float bTop = g.scrollY / g.zoomLevel;
    float bBot = (g.scrollY + bh) / g.zoomLevel;

    if (g.pdfMode){
        for (int i = 0; i < (int)g.pages.size(); ++i){
            auto& pg = g.pages[i];
            float yOff = g.pdfPageYOffset[i];
            float yEnd = (i + 1 < (int)g.pdfPageYOffset.size()) ? g.pdfPageYOffset[i + 1] : g.pdfTotalHeight;
            if (yEnd < bTop || yOff > bBot) continue;
            for (size_t ti = 0; ti < pg.texts.size(); ++ti){
                const auto& t = pg.texts[ti];
                DrawMixedBoardText(gr, t.text, Gdiplus::PointF(t.pos.x, t.pos.y),
                    t.fontSize, t.color);
            }
        }
    } else {
        auto& p = g.pages[g.currentPage];
        for (size_t ti = 0; ti < p.texts.size(); ++ti){
            const auto& t = p.texts[ti];
            DrawMixedBoardText(gr, t.text, Gdiplus::PointF(t.pos.x, t.pos.y),
                t.fontSize, t.color);
        }
    }
}

} // ns
