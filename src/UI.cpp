#include "UI.h"
#include "Board.h"
#include "PdfDoc.h"
#include "GLRenderer.h"
#include "ProjectIO.h"

namespace UI {

// ====== helper: int -> REAL ======
template<typename T> static inline REAL R(T v) { return (REAL)v; }

// ====== Mixed font rendering (CJK -> Microsoft YaHei, Latin/digits -> Anthropic Sans Text) ======
static bool IsCJK(wchar_t ch){
    return (ch >= 0x4E00 && ch <= 0x9FFF) ||   // CJK Unified Ideographs
           (ch >= 0x3400 && ch <= 0x4DBF) ||   // CJK Extension A
           (ch >= 0x3000 && ch <= 0x303F) ||   // CJK Symbols and Punctuation
           (ch >= 0xFE30 && ch <= 0xFE4F) ||   // CJK Compatibility Forms
           (ch >= 0x2E80 && ch <= 0x2EFF) ||   // CJK Radicals Supplement
           (ch >= 0x2F00 && ch <= 0x2FDF) ||   // Kangxi Radicals
           (ch >= 0x3040 && ch <= 0x309F) ||   // Hiragana
           (ch >= 0x30A0 && ch <= 0x30FF) ||   // Katakana
           (ch >= 0xAC00 && ch <= 0xD7AF) ||   // Hangul Syllables
           (ch >= 0xFF01 && ch <= 0xFF60) ||   // Fullwidth punctuation/forms
           (ch >= 0xFFE0 && ch <= 0xFFE6);     // Fullwidth signs
}

// Draw string with mixed font: CJK uses Microsoft YaHei, Latin/digits/symbols use Anthropic Sans Text
static void DrawMixedString(Graphics& gr, const wchar_t* text, int len,
                            const PointF& origin, float fontSize, const Color& color){
    if (!text || (len < 0 ? text[0] == 0 : len == 0)) return;
    if (len < 0) len = (int)wcslen(text);
    FontFamily ffCJK(L"Microsoft YaHei");
    FontFamily ffLatin(L"Anthropic Sans Text");
    SolidBrush br(color);
    StringFormat sf;
    sf.SetFormatFlags(StringFormatFlagsMeasureTrailingSpaces);
    sf.SetTrimming(StringTrimmingNone);
    float curX = origin.X;
    int runStart = 0;
    bool runCJK = IsCJK(text[0]);
    for (int i = 1; i <= len; ++i){
        bool endOfRun = (i == len);
        if (!endOfRun && IsCJK(text[i]) != runCJK) endOfRun = true;
        if (endOfRun){
            int runLen = i - runStart;
            FontFamily& ff = runCJK ? ffCJK : ffLatin;
            Font font(&ff, fontSize, FontStyleRegular, UnitPixel);
            gr.DrawString(text + runStart, runLen, &font, PointF(curX, origin.Y), &sf, &br);
            RectF bound;
            gr.MeasureString(text + runStart, runLen, &font, PointF(curX, origin.Y), &sf, &bound);
            curX += bound.Width;
            if (i < len){ runStart = i; runCJK = IsCJK(text[i]); }
        }
    }
}
static void DrawMixedString(Graphics& gr, const wchar_t* text, int len,
                            const RectF& layoutRect, float fontSize, const Color& color,
                            const StringFormat* sf){
    if (!text || (len < 0 ? text[0] == 0 : len == 0)) return;
    if (len < 0) len = (int)wcslen(text);
    FontFamily ffCJK(L"Microsoft YaHei");
    FontFamily ffLatin(L"Anthropic Sans Text");
    SolidBrush br(color);
    StringFormat sfMeasure;
    sfMeasure.SetFormatFlags(StringFormatFlagsMeasureTrailingSpaces);
    sfMeasure.SetTrimming(StringTrimmingNone);
    float totalW = 0;
    { int runStart = 0; bool runCJK = IsCJK(text[0]);
        for (int i = 1; i <= len; ++i){
            bool endOfRun = (i == len);
            if (!endOfRun && IsCJK(text[i]) != runCJK) endOfRun = true;
            if (endOfRun){
                int runLen = i - runStart;
                FontFamily& ff = runCJK ? ffCJK : ffLatin;
                Font font(&ff, fontSize, FontStyleRegular, UnitPixel);
                RectF bound;
                gr.MeasureString(text + runStart, runLen, &font, PointF(0,0), &sfMeasure, &bound);
                totalW += bound.Width;
                if (i < len){ runStart = i; runCJK = IsCJK(text[i]); }
            }
        }
    }
    float startX = layoutRect.X, startY = layoutRect.Y, layoutH = layoutRect.Height;
    if (sf){
        StringAlignment hAlign = sf->GetAlignment();
        StringAlignment vAlign = sf->GetLineAlignment();
        if (hAlign == StringAlignmentCenter) startX = layoutRect.X + (layoutRect.Width - totalW) / 2.0f;
        else if (hAlign == StringAlignmentFar) startX = layoutRect.X + layoutRect.Width - totalW;
        if (vAlign == StringAlignmentCenter) startY = layoutRect.Y + (layoutH - fontSize) / 2.0f;
        else if (vAlign == StringAlignmentFar) startY = layoutRect.Y + layoutH - fontSize;
    }
    DrawMixedString(gr, text, len, PointF(startX, startY), fontSize, color);
}

// Determine if a string has any CJK characters
static bool HasCJK(const wchar_t* text, int len = -1){
    if (!text) return false;
    if (len < 0) len = (int)wcslen(text);
    for (int i = 0; i < len; ++i)
        if (IsCJK(text[i])) return true;
    return false;
}

// ====== Rounded rectangle helper ======
static void FillRoundRect(Graphics& gr, const Color& color, float x, float y, float w, float h, float r){
    GraphicsPath path;
    path.AddArc(x, y, r*2, r*2, 180, 90);
    path.AddArc(x+w-r*2, y, r*2, r*2, 270, 90);
    path.AddArc(x+w-r*2, y+h-r*2, r*2, r*2, 0, 90);
    path.AddArc(x, y+h-r*2, r*2, r*2, 90, 90);
    path.CloseFigure();
    SolidBrush br(color);
    gr.FillPath(&br, &path);
}
static void DrawRoundRect(Graphics& gr, const Color& color, float x, float y, float w, float h, float r, float penW){
    GraphicsPath path;
    path.AddArc(x, y, r*2, r*2, 180, 90);
    path.AddArc(x+w-r*2, y, r*2, r*2, 270, 90);
    path.AddArc(x+w-r*2, y+h-r*2, r*2, r*2, 0, 90);
    path.AddArc(x, y+h-r*2, r*2, r*2, 90, 90);
    path.CloseFigure();
    Pen pen(color, penW);
    gr.DrawPath(&pen, &path);
}
static void FillDrawRoundRect(Graphics& gr, const Color& fill, const Color& border,
                               float x, float y, float w, float h, float r, float bw = 1.0f){
    FillRoundRect(gr, fill, x, y, w, h, r);
    DrawRoundRect(gr, border, x, y, w, h, r, bw);
}

// ====== color palette ======
static const Color kColorPalette[] = {
    Color(255, 30, 30, 30),   // black
    Color(255,231, 76, 60),   // red
    Color(255,243,156, 18),   // orange
    Color(255,241,196, 15),   // yellow
    Color(255, 39,174, 96),   // green
    Color(255, 41,128,185),   // blue
    Color(255,142, 68,173),   // purple
    Color(255,240,240,240),   // white
};
static const int kColorCount = 8;

static const float kSizeOptions[] = { 2.0f, 4.0f, 8.0f, 14.0f, 24.0f };
static const int kSizeCount = 5;

// ====== Refined theme colors ======
// Theme color palette (must be before colAccent cache)
static const Color kThemeColors[] = {
    Color(255, 91, 141, 239),   // 0: blue (default)
    Color(255, 156, 106, 222),  // 1: purple
    Color(255, 82, 196, 138),   // 2: green
    Color(255, 243, 156, 18),   // 3: orange
    Color(255, 231, 76, 60),    // 4: red
    Color(255, 26, 188, 156),   // 5: cyan
};
static const int kThemeColorCount = 6;

static Color GetAccentColor(){
    int idx = g.themeColor;
    if (idx < 0 || idx >= kThemeColorCount) idx = 0;
    return kThemeColors[idx];
}
static Color GetAccentDarkColor(){
    Color c = GetAccentColor();
    return Color(255, (BYTE)(c.GetR()*0.75f), (BYTE)(c.GetG()*0.75f), (BYTE)(c.GetB()*0.75f));
}

// ====== Dynamic theme colors (dark/light mode) ======
// Dark mode palette
static const Color kDarkText(255, 232, 236, 241);        // #E8ECF1
static const Color kDarkTextDim(255, 123, 135, 148);     // #7B8794
static const Color kDarkDivider(35, 255, 255, 255);
static const Color kDarkBarBg(255, 26, 31, 46);          // #1A1F2E
static const Color kDarkTopA(255, 30, 36, 54);           // #1E2436
static const Color kDarkTopB(255, 24, 28, 40);           // #181C28
static const Color kDarkPopupBg(255, 37, 43, 59);        // #252B3B
static const Color kDarkPopupBorder(255, 61, 68, 85);    // #3D4455
static const Color kDarkBtnBg(30, 255, 255, 255);
static const Color kDarkBtnHover(50, 255, 255, 255);

// Light mode palette
static const Color kLightText(255, 30, 30, 30);
static const Color kLightTextDim(255, 120, 120, 130);
static const Color kLightDivider(30, 0, 0, 0);
static const Color kLightBarBg(255, 245, 247, 250);      // #F5F7FA
static const Color kLightTopA(255, 235, 238, 242);       // #EBEEF2
static const Color kLightTopB(255, 225, 228, 232);       // #E1E4E8
static const Color kLightPopupBg(255, 255, 255, 255);
static const Color kLightPopupBorder(255, 210, 215, 220);
static const Color kLightBtnBg(25, 0, 0, 0);
static const Color kLightBtnHover(45, 0, 0, 0);

// Cached theme colors - refreshed each frame via UpdateThemeColors()
static Color colTextCache(255, 232, 236, 241);
static Color colTextDimCache(255, 123, 135, 148);
static Color colDividerCache(35, 255, 255, 255);
static Color colBarBgCache(255, 26, 31, 46);
static Color colTopACache(255, 30, 36, 54);
static Color colTopBCache(255, 24, 28, 40);
static Color colPopupBgCache(255, 37, 43, 59);
static Color colPopupBorderCache(255, 61, 68, 85);
static Color colBtnBgCache(30, 255, 255, 255);
static Color colBtnHoverCache(50, 255, 255, 255);
static const Color colDanger(255, 231, 76, 60);

static void UpdateThemeColors(){
    if (g.isDarkMode){
        colTextCache = kDarkText;        colTextDimCache = kDarkTextDim;
        colDividerCache = kDarkDivider;  colBarBgCache = kDarkBarBg;
        colTopACache = kDarkTopA;        colTopBCache = kDarkTopB;
        colPopupBgCache = kDarkPopupBg;  colPopupBorderCache = kDarkPopupBorder;
        colBtnBgCache = kDarkBtnBg;      colBtnHoverCache = kDarkBtnHover;
    } else {
        colTextCache = kLightText;        colTextDimCache = kLightTextDim;
        colDividerCache = kLightDivider;  colBarBgCache = kLightBarBg;
        colTopACache = kLightTopA;        colTopBCache = kLightTopB;
        colPopupBgCache = kLightPopupBg;  colPopupBorderCache = kLightPopupBorder;
        colBtnBgCache = kLightBtnBg;      colBtnHoverCache = kLightBtnHover;
    }
}

// Cached accent colors - refreshed each frame via UpdateAccentCache()
static Color colAccentCache(255, 91, 141, 239);
static Color colAccentDarkCache(255, 61, 111, 217);
// Theme-tinted toolbar glass background colors
static Color colBarTopCache(255, 32, 38, 56);
static Color colBarBotCache(255, 22, 26, 40);

static BYTE MixByte(BYTE base, BYTE accent, float amount){
    float v = (float)base * (1.0f - amount) + (float)accent * amount;
    if (v < 0.0f) v = 0.0f;
    if (v > 255.0f) v = 255.0f;
    return (BYTE)(v + 0.5f);
}

static Color BlendForToolbar(Color base, Color accent, float amount, int minLuma, int maxLuma){
    BYTE r = MixByte(base.GetR(), accent.GetR(), amount);
    BYTE g2 = MixByte(base.GetG(), accent.GetG(), amount);
    BYTE b = MixByte(base.GetB(), accent.GetB(), amount);
    int luma = (int)(0.2126f * r + 0.7152f * g2 + 0.0722f * b + 0.5f);
    if (luma < minLuma || luma > maxLuma){
        float target = (float)(luma < minLuma ? minLuma : maxLuma);
        float factor = target / std::max(1.0f, (float)luma);
        r = (BYTE)std::max(0.0f, std::min(255.0f, r * factor));
        g2 = (BYTE)std::max(0.0f, std::min(255.0f, g2 * factor));
        b = (BYTE)std::max(0.0f, std::min(255.0f, b * factor));
    }
    return Color(255, r, g2, b);
}

static void UpdateAccentCache(){
    colAccentCache = GetAccentColor();
    colAccentDarkCache = GetAccentDarkColor();

    Color a = GetAccentColor();
    if (g.isDarkMode){
        colBarTopCache = BlendForToolbar(kDarkTopA, a, 0.26f, 34, 92);
        colBarBotCache = BlendForToolbar(kDarkTopB, a, 0.20f, 26, 78);
    } else {
        colBarTopCache = BlendForToolbar(kLightTopA, a, 0.18f, 178, 240);
        colBarBotCache = BlendForToolbar(kLightTopB, a, 0.14f, 166, 232);
    }

    UpdateThemeColors();
}

// Aliases so existing code just works
#define colAccent colAccentCache
#define colAccentDark colAccentDarkCache
#define colAccentGlow colAccentCache

// Aliases for theme colors
#define colText colTextCache
#define colTextDim colTextDimCache
#define colDivider colDividerCache
#define colBarBg colBarBgCache
#define colTopA colTopACache
#define colTopB colTopBCache
#define colPopupBg colPopupBgCache
#define colPopupBorder colPopupBorderCache
#define colBtnBg colBtnBgCache
#define colBtnHover colBtnHoverCache

// ====== hit regions ======
struct Hit { RECT rc; int id; };
static std::vector<Hit> g_hits;
static std::vector<Hit> g_popupHits;

static void AddHit(std::vector<Hit>& v, int x, int y, int w, int h, int id){
    RECT r2; r2.left=x; r2.top=y; r2.right=x+w; r2.bottom=y+h;
    v.push_back({r2, id});
}

// ====== vector icon drawing (Lucide-inspired, 24脳24 grid) ======
enum IconId {
    ICO_PAN, ICO_PEN, ICO_ERASER, ICO_SELECT,
    ICO_UNDO, ICO_REDO, ICO_EXIT,
    ICO_CHEVRON_LEFT, ICO_CHEVRON_RIGHT, ICO_PLUS, ICO_MINUS,
    ICO_DROPDOWN, ICO_CLOUD, ICO_CENTER,
    ICO_FOLDER_OPEN, ICO_SAVE, ICO_INFO, ICO_ZOOM,
    ICO_COPY, ICO_DELETE, ICO_TRIANGLE_TOOL, ICO_COMPASS_TOOL
};

static void DrawVectorIcon(Graphics& gr, IconId id, float cx, float cy, float sz, Color col) {
    // Lucide style: 24脳24 grid, stroke-width 2, round caps/joins
    float sc = sz / 24.0f;
    float sw = std::max(1.2f, sz / 12.0f);

    Pen pen(col, sw);
    pen.SetStartCap(LineCapRound);
    pen.SetEndCap(LineCapRound);
    pen.SetLineJoin(LineJoinRound);
    SolidBrush br(col);

    // Map SVG 24脳24 coordinate to screen pixel
    auto P = [=](float x, float y) -> PointF {
        return PointF(cx + (x - 12.0f) * sc, cy + (y - 12.0f) * sc);
    };
    (void)P; // suppress unused warning

    switch (id) {

    // ---- Pan: four-way move arrows ----
    case ICO_PAN: {
        float aw = 3.0f; // arrowhead size in grid units
        // Vertical shaft + arrowheads
        gr.DrawLine(&pen, P(12, 4), P(12, 20));
        gr.DrawLine(&pen, P(12, 4),  P(12-aw, 7));
        gr.DrawLine(&pen, P(12, 4),  P(12+aw, 7));
        gr.DrawLine(&pen, P(12, 20), P(12-aw, 17));
        gr.DrawLine(&pen, P(12, 20), P(12+aw, 17));
        // Horizontal shaft + arrowheads
        gr.DrawLine(&pen, P(4, 12), P(20, 12));
        gr.DrawLine(&pen, P(4, 12),  P(7, 12-aw));
        gr.DrawLine(&pen, P(4, 12),  P(7, 12+aw));
        gr.DrawLine(&pen, P(20, 12), P(17, 12-aw));
        gr.DrawLine(&pen, P(20, 12), P(17, 12+aw));
        break;
    }

    // ---- Pen: pencil-line.svg ----
    case ICO_PEN: {
        gr.DrawLine(&pen, P(13, 21), P(21, 21));
        gr.DrawLine(&pen, P(15, 5), P(19, 9));

        GraphicsPath p;
        p.StartFigure();
        p.AddLine(P(21.174f, 6.812f), P(7.827f, 20.161f));
        p.AddBezier(P(7.827f, 20.161f), P(7.596f, 20.392f), P(7.314f, 20.561f), P(6.997f, 20.658f));
        p.AddLine(P(6.997f, 20.658f), P(2.644f, 21.978f));
        p.AddBezier(P(2.644f, 21.978f), P(2.247f, 22.098f), P(1.901f, 21.752f), P(2.021f, 21.356f));
        p.AddLine(P(2.021f, 21.356f), P(3.342f, 17.004f));
        p.AddBezier(P(3.342f, 17.004f), P(3.439f, 16.687f), P(3.608f, 16.405f), P(3.842f, 16.174f));
        p.AddLine(P(3.842f, 16.174f), P(17.188f, 2.825f));
        p.AddBezier(P(17.188f, 2.825f), P(18.289f, 1.724f), P(20.073f, 1.724f), P(21.174f, 2.825f));
        p.AddBezier(P(21.174f, 2.825f), P(22.275f, 3.926f), P(22.275f, 5.711f), P(21.174f, 6.812f));
        gr.DrawPath(&pen, &p);
        break;
    }

    // ---- Eraser: Lucide eraser inspired outline ----
    case ICO_ERASER: {
        GraphicsPath p;
        p.StartFigure();
        p.AddLine(P(7, 21), P(2.7f, 16.7f));
        p.AddBezier(P(2.7f, 16.7f), P(1.7f, 15.7f), P(1.7f, 14.3f), P(2.7f, 13.3f));
        p.AddLine(P(2.7f, 13.3f), P(12.3f, 3.7f));
        p.AddBezier(P(12.3f, 3.7f), P(13.3f, 2.7f), P(14.7f, 2.7f), P(15.7f, 3.7f));
        p.AddLine(P(15.7f, 3.7f), P(21.3f, 9.3f));
        p.AddBezier(P(21.3f, 9.3f), P(22.3f, 10.3f), P(22.3f, 11.7f), P(21.3f, 12.7f));
        p.AddLine(P(21.3f, 12.7f), P(13, 21));
        gr.DrawPath(&pen, &p);
        gr.DrawLine(&pen, P(22, 21), P(7, 21));
        gr.DrawLine(&pen, P(5, 11), P(14, 20));
        break;
    }

    // ---- Select: mouse pointer cursor (filled) ----
    case ICO_SELECT: {
        PointF pts[] = {
            P(4,    4),
            P(4,    20),
            P(9.5f,  15.5f),
            P(13,   21),
            P(16,   19.5),
            P(12.5f, 14.0f),
            P(18.5f, 12.0f),
        };
        GraphicsPath p;
        p.AddLines(pts, 7);
        p.CloseFigure();
        gr.FillPath(&br, &p);
        break;
    }

    // ---- Undo: curved left arrow (Lucide undo-2) ----
    case ICO_UNDO: {
        // Arrowhead pointing left
        gr.DrawLine(&pen, P(9, 4), P(4, 9));
        gr.DrawLine(&pen, P(9, 14), P(4, 9));
        // Shaft to arc start
        gr.DrawLine(&pen, P(4, 9), P(14.5f, 9.0f));
        // Semicircular arc (centre 14.5, 14.5  radius 5.5)
        {
            float acx = cx + (14.5f - 12.0f) * sc;
            float acy = cy + (14.5f - 12.0f) * sc;
            float ar  = 5.5f * sc;
            RectF arcR(acx - ar, acy - ar, ar*2, ar*2);
            gr.DrawArc(&pen, arcR, 270.0f, 180.0f);
        }
        // Tail line
        gr.DrawLine(&pen, P(14.5f, 20.0f), P(11, 20));
        break;
    }

    // ---- Redo: curved right arrow (Lucide redo-2) ----
    case ICO_REDO: {
        gr.DrawLine(&pen, P(15, 4), P(20, 9));
        gr.DrawLine(&pen, P(15, 14), P(20, 9));
        gr.DrawLine(&pen, P(20, 9), P(9.5f, 9.0f));
        {
            float acx = cx + (9.5f - 12.0f) * sc;
            float acy = cy + (14.5f - 12.0f) * sc;
            float ar  = 5.5f * sc;
            RectF arcR(acx - ar, acy - ar, ar*2, ar*2);
            gr.DrawArc(&pen, arcR, 270.0f, -180.0f);
        }
        gr.DrawLine(&pen, P(9.5f, 20.0f), P(13, 20));
        break;
    }

    // ---- Exit: X mark ----
    case ICO_EXIT: {
        gr.DrawLine(&pen, P(6, 6), P(18, 18));
        gr.DrawLine(&pen, P(18, 6), P(6, 18));
        break;
    }

    // ---- Chevron left ----
    case ICO_CHEVRON_LEFT: {
        gr.DrawLine(&pen, P(15, 6), P(9, 12));
        gr.DrawLine(&pen, P(9, 12), P(15, 18));
        break;
    }

    // ---- Chevron right ----
    case ICO_CHEVRON_RIGHT: {
        gr.DrawLine(&pen, P(9, 6), P(15, 12));
        gr.DrawLine(&pen, P(15, 12), P(9, 18));
        break;
    }

    // ---- Plus ----
    case ICO_PLUS: {
        gr.DrawLine(&pen, P(5, 12), P(19, 12));
        gr.DrawLine(&pen, P(12, 5), P(12, 19));
        break;
    }

    // ---- Minus ----
    case ICO_MINUS: {
        gr.DrawLine(&pen, P(5, 12), P(19, 12));
        break;
    }

    // ---- Dropdown triangle ----
    case ICO_DROPDOWN: {
        float s = 3.5f * sc;
        PointF pts[] = {
            PointF(cx - s, cy - s * 0.35f),
            PointF(cx + s, cy - s * 0.35f),
            PointF(cx,     cy + s * 0.65f)
        };
        GraphicsPath p;
        p.AddLines(pts, 3);
        p.CloseFigure();
        gr.FillPath(&br, &p);
        break;
    }

    // ---- Cloud logo (Lucide-inspired arc cloud) ----
    case ICO_CLOUD: {
        GraphicsPath path;
        path.StartFigure();
        // Bottom line: right to left
        path.AddLine(P(17.5f, 19.0f), P(9.0f, 19.0f));
        // Main body arc: center(9,12) r=7, from bottom(90 degrees) clockwise 253.4 degrees
        path.AddArc(P(2, 5).X, P(2, 5).Y, 14.0f*sc, 14.0f*sc, 90.0f, 253.4f);
        // Connector to right bump
        path.AddLine(P(15.71f, 10.0f), P(17.5f, 10.0f));
        // Right bump arc: center(17.5,14.5) r=4.5, from top(270 degrees) clockwise 180 degrees
        path.AddArc(P(13, 10).X, P(13, 10).Y, 9.0f*sc, 9.0f*sc, 270.0f, 180.0f);
        path.CloseFigure();
        gr.FillPath(&br, &path);
        break;
    }

    // ---- Center: crosshair / target (Lucide crosshair) ----
    case ICO_CENTER: {
        // Center dot
        float r = 2.0f * sc;
        PointF c = P(12, 12);
        gr.FillEllipse(&br, c.X - r, c.Y - r, r*2, r*2);
        // Outer circle
        float cr = 7.0f * sc;
        gr.DrawEllipse(&pen, c.X - cr, c.Y - cr, cr*2, cr*2);
        // Crosshair lines (4 segments with gap for circle)
        gr.DrawLine(&pen, P(12, 3), P(12, 7.5f));
        gr.DrawLine(&pen, P(12, 16.5f), P(12, 21));
        gr.DrawLine(&pen, P(3, 12), P(7.5f, 12));
        gr.DrawLine(&pen, P(16.5f, 12), P(21, 12));
        break;
    }
    case ICO_FOLDER_OPEN: {
        // Open folder icon: back panel + tab + front panel with opening
        // Back panel
        {
            PointF tl = P(3, 6), br = P(21, 19);
            gr.DrawRectangle(&pen, tl.X, tl.Y, br.X - tl.X, br.Y - tl.Y);
        }
        // Tab on top-left
        PointF tabPts[] = { P(3, 6), P(3, 4), P(9, 4), P(10, 6) };
        gr.DrawLines(&pen, tabPts, 4);
        // Front panel (lower, slightly forward)
        PointF frontPts[] = { P(2, 10), P(5, 19), P(22, 19), P(22, 10) };
        gr.DrawLines(&pen, frontPts, 4);
        break;
    }
    case ICO_SAVE: {
        gr.DrawRectangle(&pen, P(4, 4).X, P(4, 4).Y, 16.0f*sc, 16.0f*sc);
        gr.DrawLine(&pen, P(8, 4), P(8, 10));
        gr.DrawLine(&pen, P(8, 10), P(16, 10));
        gr.DrawLine(&pen, P(16, 10), P(16, 4));
        gr.DrawLine(&pen, P(8, 20), P(8, 14));
        gr.DrawLine(&pen, P(8, 14), P(16, 14));
        gr.DrawLine(&pen, P(16, 14), P(16, 20));
        break;
    }
    case ICO_INFO: {
        // Circle with "i" inside
        float cr = 9.0f * sc;
        PointF cc = P(12, 12);
        gr.DrawEllipse(&pen, cc.X - cr, cc.Y - cr, cr * 2, cr * 2);
        // Dot on top of i
        {
            PointF dot = P(12, 7);
            float dr = 1.5f * sc;
            gr.FillEllipse(&br, dot.X - dr, dot.Y - dr, dr * 2, dr * 2);
        }
        // Stem of i
        gr.DrawLine(&pen, P(12, 10), P(12, 17));
        break;
    }

    case ICO_ZOOM: {
        // Magnifying glass (Lucide "search" style)
        // Circle: center (11,11), radius 8
        float zcr = 8.0f * sc;
        PointF zcc = P(11, 11);
        gr.DrawEllipse(&pen, zcc.X - zcr, zcc.Y - zcr, zcr * 2, zcr * 2);
        // Handle: line from (16.65, 16.65) to (21, 21)
        gr.DrawLine(&pen, P(16.65f, 16.65f), P(21, 21));
        // Plus sign inside (zoom-in indicator)
        gr.DrawLine(&pen, P(11, 8), P(11, 14));
        gr.DrawLine(&pen, P(8, 11), P(14, 11));
        break;
    }

    case ICO_COPY: {
        // Two overlapping rectangles (Lucide "copy" style)
        // Back rectangle
        gr.DrawRectangle(&pen, P(8, 3).X, P(8, 3).Y, 13 * sc, 13 * sc);
        // Front rectangle
        gr.DrawRectangle(&pen, P(3, 8).X, P(3, 8).Y, 13 * sc, 13 * sc);
        break;
    }

    case ICO_DELETE: {
        // Trash can (Lucide "trash-2" style)
        // Lid
        gr.DrawLine(&pen, P(3, 6), P(21, 6));
        gr.DrawLine(&pen, P(8, 6), P(8, 4));
        gr.DrawLine(&pen, P(16, 6), P(16, 4));
        gr.DrawLine(&pen, P(8, 4), P(16, 4));
        // Can body
        PointF tl = P(5, 6), br2 = P(19, 20);
        gr.DrawRectangle(&pen, tl.X, tl.Y, br2.X - tl.X, br2.Y - tl.Y);
        // Inner lines
        gr.DrawLine(&pen, P(10, 10), P(10, 17));
        gr.DrawLine(&pen, P(14, 10), P(14, 17));
        break;
    }

    case ICO_TRIANGLE_TOOL: {
        PointF tri[] = { P(5, 19), P(19, 19), P(5, 5), P(5, 19) };
        gr.DrawLines(&pen, tri, 4);
        gr.DrawLine(&pen, P(8, 16), P(12, 16));
        gr.DrawLine(&pen, P(8, 13), P(15, 13));
        gr.DrawLine(&pen, P(8, 10), P(11, 10));
        break;
    }

    case ICO_COMPASS_TOOL: {
        gr.DrawEllipse(&pen, P(10, 5).X - 2*sc, P(10, 5).Y - 2*sc, 4*sc, 4*sc);
        gr.DrawLine(&pen, P(10, 7), P(5, 20));
        gr.DrawLine(&pen, P(10, 7), P(18, 20));
        gr.DrawLine(&pen, P(7, 14), P(15, 14));
        break;
    }
    }
}

// ====== tool button definitions ======
struct ToolBtn { int id; ToolKind tool; IconId icon; const wchar_t* lbl; bool hasPopup; };
static constexpr bool kAssistToolsEnabled = false; // Teaching aids are hidden until the feature is re-enabled.
static const ToolBtn kToolBtns[] = {
    { ID_TOOL_PAN,    ToolKind::Pan,    ICO_PAN,    L"拖动", false },
    { ID_TOOL_PEN,    ToolKind::Pen,    ICO_PEN,    L"画笔", true  },
    { ID_TOOL_ERASER, ToolKind::Eraser, ICO_ERASER, L"橡皮", true  },
    { ID_TOOL_SELECT, ToolKind::Select, ICO_SELECT, L"选择", false },
    // { ID_ASSIST_MENU, ToolKind::Select, ICO_TRIANGLE_TOOL, L"教具", true },
    { ID_TOOL_UNDO,   ToolKind::Select, ICO_UNDO,   L"撤销", false },
    { ID_TOOL_REDO,   ToolKind::Select, ICO_REDO,   L"重做", false },
};
static const int kToolBtnCount = (int)(sizeof(kToolBtns) / sizeof(kToolBtns[0]));

// ====== draw bottom bar (floating frosted glass, centered buttons) ======

// ====== draw bottom bar (floating frosted glass, centered buttons) ======
static void DrawBottomBar(Graphics& gr, HWND hWnd){
    RECT r = BottomBarRect(hWnd);
    int W = r.right - r.left, H = r.bottom - r.top;

    // ===== Exit button at bottom-left corner =====
    int exitBtnW = S(44), exitBtnH = H - S(10);
    int exitBtnX = r.left + S(8);
    int exitBtnY = r.top + S(5);

    Color exitBg, exitBorder, exitIco, exitLbl;
    if (g.isDarkMode) {
        exitBg = Color(180, 60, 40, 44);  exitBorder = Color(50, 200, 80, 80);
        exitIco = Color(255, 255, 100, 100); exitLbl = Color(255, 255, 130, 130);
    } else {
        exitBg = Color(180, 250, 180, 180); exitBorder = Color(60, 220, 140, 140);
        exitIco = Color(255, 220, 50, 50);   exitLbl = Color(255, 200, 40, 40);
    }

    // Exit button shadow
    FillRoundRect(gr, Color(18, 0, 0, 0),
        R(exitBtnX + 2), R(exitBtnY + 3), R(exitBtnW), R(exitBtnH), R(S(8)));
    FillDrawRoundRect(gr, exitBg, exitBorder,
        R(exitBtnX), R(exitBtnY), R(exitBtnW), R(exitBtnH), R(S(8)), 0.6f);
    DrawVectorIcon(gr, ICO_EXIT, (float)(exitBtnX + exitBtnW/2),
                   (float)(exitBtnY + exitBtnH * 0.35f), Sf(14.0f), exitIco);
    {
        StringFormat sfExit; sfExit.SetAlignment(StringAlignmentCenter);
        DrawMixedString(gr, L"退出", -1,
            RectF(R(exitBtnX), R(exitBtnY + exitBtnH * 0.55f), R(exitBtnW), R(exitBtnH * 0.4f)),
            Sf(8.0f), exitLbl, &sfExit);
    }
    AddHit(g_hits, exitBtnX, exitBtnY, exitBtnW, exitBtnH, ID_TOP_EXIT);

    // ===== Center button (return to origin) =====
    int centerBtnW = S(44), centerBtnH = exitBtnH;
    int centerBtnX = exitBtnX + exitBtnW + S(6);
    int centerBtnY = exitBtnY;

    Color cenBg, cenBorder, cenIco, cenLbl;
    if (g.isDarkMode) {
        cenBg = Color(180, 50, 60, 75); cenBorder = Color(50, 100, 140, 180);
        cenIco = Color(255, 180, 210, 245); cenLbl = Color(255, 170, 200, 235);
    } else {
        cenBg = Color(180, 235, 240, 245); cenBorder = Color(60, 180, 190, 200);
        cenIco = Color(255, 70, 100, 150); cenLbl = Color(255, 60, 90, 140);
    }

    FillRoundRect(gr, Color(18, 0, 0, 0),
        R(centerBtnX + 2), R(centerBtnY + 3), R(centerBtnW), R(centerBtnH), R(S(8)));
    FillDrawRoundRect(gr, cenBg, cenBorder,
        R(centerBtnX), R(centerBtnY), R(centerBtnW), R(centerBtnH), R(S(8)), 0.6f);
    DrawVectorIcon(gr, ICO_CENTER, (float)(centerBtnX + centerBtnW/2),
                   (float)(centerBtnY + centerBtnH * 0.35f), Sf(14.0f), cenIco);
    {
        StringFormat sfCenter; sfCenter.SetAlignment(StringAlignmentCenter);
        DrawMixedString(gr, L"回中", -1,
            RectF(R(centerBtnX), R(centerBtnY + centerBtnH * 0.55f), R(centerBtnW), R(centerBtnH * 0.4f)),
            Sf(8.0f), cenLbl, &sfCenter);
    }
    AddHit(g_hits, centerBtnX, centerBtnY, centerBtnW, centerBtnH, ID_VIEW_CENTER);

    // ===== Minimize button =====
    int minBtnW = S(44), minBtnH = exitBtnH;
    int minBtnX = centerBtnX + centerBtnW + S(6);
    int minBtnY = exitBtnY;

    FillRoundRect(gr, Color(18, 0, 0, 0),
        R(minBtnX + 2), R(minBtnY + 3), R(minBtnW), R(minBtnH), R(S(8)));
    FillDrawRoundRect(gr, cenBg, cenBorder,
        R(minBtnX), R(minBtnY), R(minBtnW), R(minBtnH), R(S(8)), 0.6f);
    {
        float rcx = (float)(minBtnX + minBtnW / 2);
        float rcy = (float)(minBtnY + minBtnH * 0.35f);
        float sz = Sf(14.0f);
        float sc = sz / 24.0f;
        float sw2 = std::max(1.2f, sz / 12.0f);
        Pen minPen(cenIco, sw2);
        minPen.SetStartCap(LineCapRound);
        minPen.SetEndCap(LineCapRound);
        minPen.SetLineJoin(LineJoinRound);
        float rx = rcx - 8.0f * sc, ry = rcy - 6.0f * sc;
        float rw = 16.0f * sc, rh = 12.0f * sc;
        gr.DrawRectangle(&minPen, rx, ry, rw, rh);
        float bh2 = 3.0f * sc;
        SolidBrush minBr(cenIco);
        gr.FillRectangle(&minBr, rx + sw2, ry + rh - bh2 - sw2, rw - sw2 * 2, bh2);
    }
    {
        StringFormat sfMin; sfMin.SetAlignment(StringAlignmentCenter);
        DrawMixedString(gr, L"最小", -1,
            RectF(R(minBtnX), R(minBtnY + minBtnH * 0.55f), R(minBtnW), R(minBtnH * 0.4f)),
            Sf(8.0f), cenLbl, &sfMin);
    }
    AddHit(g_hits, minBtnX, minBtnY, minBtnW, minBtnH, ID_WINDOW_MIN);

    // ===== Settings (About) button =====
    int aboutBtnW = S(44), aboutBtnH = exitBtnH;
    int aboutBtnX = minBtnX + minBtnW + S(6);
    int aboutBtnY = exitBtnY;

    FillRoundRect(gr, Color(18, 0, 0, 0),
        R(aboutBtnX + 2), R(aboutBtnY + 3), R(aboutBtnW), R(aboutBtnH), R(S(8)));
    FillDrawRoundRect(gr, cenBg, cenBorder,
        R(aboutBtnX), R(aboutBtnY), R(aboutBtnW), R(aboutBtnH), R(S(8)), 0.6f);
    DrawVectorIcon(gr, ICO_INFO, (float)(aboutBtnX + aboutBtnW/2),
                   (float)(aboutBtnY + aboutBtnH * 0.35f), Sf(14.0f), cenIco);
    {
        StringFormat sfAbout; sfAbout.SetAlignment(StringAlignmentCenter);
        DrawMixedString(gr, L"设置", -1,
            RectF(R(aboutBtnX), R(aboutBtnY + aboutBtnH * 0.55f), R(aboutBtnW), R(aboutBtnH * 0.4f)),
            Sf(8.0f), cenLbl, &sfAbout);
    }
    AddHit(g_hits, aboutBtnX, aboutBtnY, aboutBtnW, aboutBtnH, ID_ABOUT);

    // ===== Frosted glass floating panel =====
    int margin = S(5);
    int barX = aboutBtnX + aboutBtnW + S(8);
    int barY = r.top + margin;
    int barW = r.right - margin - barX;
    int barH = H - margin * 2;
    float radius = (float)S(14);

    // Drop shadow (soft, offset)
    FillRoundRect(gr, Color(18, 0, 0, 0),
        R(barX + 3), R(barY + 4), R(barW), R(barH), R(radius));
    FillRoundRect(gr, Color(12, 0, 0, 0),
        R(barX + 5), R(barY + 6), R(barW), R(barH), R(radius));

    // Glass background - gradient from lighter to darker (simulates depth)
    {
        GraphicsPath path;
        path.AddArc(R(barX), R(barY), R(radius*2), R(radius*2), 180, 90);
        path.AddArc(R(barX+barW-radius*2), R(barY), R(radius*2), R(radius*2), 270, 90);
        path.AddArc(R(barX+barW-radius*2), R(barY+barH-radius*2), R(radius*2), R(radius*2), 0, 90);
        path.AddArc(R(barX), R(barY+barH-radius*2), R(radius*2), R(radius*2), 90, 90);
        path.CloseFigure();

        Color glassTop = g.isDarkMode
            ? Color(215, colBarTopCache.GetR(), colBarTopCache.GetG(), colBarTopCache.GetB())
            : Color(230, colBarTopCache.GetR(), colBarTopCache.GetG(), colBarTopCache.GetB());
        Color glassBot = g.isDarkMode
            ? Color(235, colBarBotCache.GetR(), colBarBotCache.GetG(), colBarBotCache.GetB())
            : Color(245, colBarBotCache.GetR(), colBarBotCache.GetG(), colBarBotCache.GetB());
        LinearGradientBrush glassBg(
            PointF(R(barX), R(barY)),
            PointF(R(barX), R(barY + barH)),
            glassTop, glassBot);
        gr.FillPath(&glassBg, &path);

        // Top highlight - thin bright line for glass reflection
        Pen highlight(g.isDarkMode ? Color(50, 255, 255, 255) : Color(60, 255, 255, 255), 0.6f);
        gr.DrawPath(&highlight, &path);

        // Subtle bottom border
        Pen bottomLine(g.isDarkMode ? Color(25, 255, 255, 255) : Color(30, 0, 0, 0), 0.4f);
        gr.DrawArc(&bottomLine,
            R(barX), R(barY+barH-radius*2), R(radius*2), R(radius*2), 90, 90);
        gr.DrawLine(&bottomLine,
            R(barX + radius), R(barY + barH),
            R(barX + barW - radius), R(barY + barH));
        gr.DrawArc(&bottomLine,
            R(barX+barW-radius*2), R(barY+barH-radius*2), R(radius*2), R(radius*2), 0, 90);
    }

    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter);
    SolidBrush brTextW(colText);
    SolidBrush brDim(colTextDim);

    // ===== Calculate total width for centering =====
    int btnW = S(50), btnH = barH - S(10);
    int btnGap = S(3);
    int divGap = S(14);

    int navBtnS = S(30);
    int navPageW = S(48);
    int navGap = S(5);

    int openBtnW = btnW + btnGap; // open file button + gap
    int saveBtnW = btnW + btnGap; // save project button + gap
    int zoomBtnW = btnW + btnGap; // zoom button + gap
    int toolW = kToolBtnCount * btnW + (kToolBtnCount - 1) * btnGap;
    int navW  = navBtnS + navGap + navPageW + navGap + navBtnS;
    int navPad = S(8);
    int availW = barW - navW - navPad;
    int totalToolsW = openBtnW + saveBtnW + zoomBtnW + toolW;
    int startX = barX + (availW - totalToolsW) / 2;

    // ===== Draw open file button =====
    // Shared colors for inactive button backgrounds (dark/light adaptive)
    Color btnBg2 = g.isDarkMode ? Color(22, 255, 255, 255) : Color(22, 0, 0, 0);
    Color btnBd2 = g.isDarkMode ? Color(18, 255, 255, 255) : Color(14, 0, 0, 0);

    int bx = startX;
    int by = barY + S(5);
    {
        FillDrawRoundRect(gr, btnBg2, btnBd2,
            R(bx), R(by), R(btnW), R(btnH), R(S(8)), 0.4f);
        DrawVectorIcon(gr, ICO_FOLDER_OPEN, (float)(bx + btnW/2),
                       (float)(by + btnH * 0.32f), Sf(16.0f), colTextDim);
        StringFormat sfCOpen; sfCOpen.SetAlignment(StringAlignmentCenter);
        DrawMixedString(gr, L"打开", -1,
            RectF(R(bx), R(by + btnH * 0.55f), R(btnW), R(btnH * 0.4f)),
            Sf(9.0f), colTextDim, &sfCOpen);
        AddHit(g_hits, bx, by, btnW, btnH, ID_TOP_OPEN);
    }
    bx += openBtnW;

    // ===== Draw save project button =====
    {
        FillDrawRoundRect(gr, btnBg2, btnBd2,
            R(bx), R(by), R(btnW), R(btnH), R(S(8)), 0.4f);
        DrawVectorIcon(gr, ICO_SAVE, (float)(bx + btnW/2),
                       (float)(by + btnH * 0.32f), Sf(16.0f), colTextDim);
        StringFormat sfCSave; sfCSave.SetAlignment(StringAlignmentCenter);
        DrawMixedString(gr, L"保存", -1,
            RectF(R(bx), R(by + btnH * 0.55f), R(btnW), R(btnH * 0.4f)),
            Sf(9.0f), colTextDim, &sfCSave);
        AddHit(g_hits, bx, by, btnW, btnH, ID_TOP_SAVE);
    }
    bx += saveBtnW;

    // ===== Draw zoom button =====
    {
        bool zoomActive = g.zoomPopupOpen;
        if (zoomActive){
            FillDrawRoundRect(gr, colAccent, colAccentDark,
                R(bx), R(by), R(btnW), R(btnH), R(S(8)), 1.0f);
        } else {
            FillDrawRoundRect(gr, btnBg2, btnBd2,
                R(bx), R(by), R(btnW), R(btnH), R(S(8)), 0.4f);
        }
        Color zoomIcoCol = zoomActive ? Color(255,255,255,255) : colTextDim;
        DrawVectorIcon(gr, ICO_ZOOM, (float)(bx + btnW/2),
                       (float)(by + btnH * 0.32f), Sf(16.0f), zoomIcoCol);
        // Show zoom percentage as label
        FontFamily yhZoom(L"Anthropic Sans Text");
        Font fZoomLbl(&yhZoom, Sf(8.0f), FontStyleRegular, UnitPixel);
        StringFormat sfC2; sfC2.SetAlignment(StringAlignmentCenter);
        SolidBrush brZoomLbl(zoomActive ? Color(255,255,255,255) : colTextDim);
        wchar_t zoomBuf[8];
        int zoomPct = (int)(g.zoomLevel / kZoomDisplayBase * 100.0f + 0.5f);
        swprintf(zoomBuf, 8, L"%d%%", zoomPct);
        gr.DrawString(zoomBuf, -1, &fZoomLbl,
            RectF(R(bx), R(by + btnH * 0.58f), R(btnW), R(btnH * 0.35f)),
            &sfC2, &brZoomLbl);
        AddHit(g_hits, bx, by, btnW, btnH, ID_ZOOM_TOGGLE);
    }
    bx += zoomBtnW;

    // ===== Draw tool buttons =====

    for (int i = 0; i < kToolBtnCount; ++i){
        const ToolBtn& tb = kToolBtns[i];
        bool active = false;
        if (tb.id == ID_ASSIST_MENU)
            active = (g.assist.kind != AssistKind::None);
        else if (tb.id != ID_TOOL_UNDO && tb.id != ID_TOOL_REDO)
            active = (g.tool == tb.tool);

        bool popupActive = false;
        if (tb.hasPopup && tb.tool == ToolKind::Pen && g.penPopupOpen) popupActive = true;
        if (tb.hasPopup && tb.tool == ToolKind::Eraser && g.eraserPopupOpen) popupActive = true;
        if (tb.id == ID_ASSIST_MENU && g.assistPopupOpen) popupActive = true;

        if (active || popupActive){
            FillDrawRoundRect(gr, colAccent, colAccentDark,
                R(bx), R(by), R(btnW), R(btnH), R(S(8)), 1.0f);
        } else {
            FillDrawRoundRect(gr, btnBg2, btnBd2,
                R(bx), R(by), R(btnW), R(btnH), R(S(8)), 0.4f);
        }

        Color icoCol = active ? Color(255,255,255,255) : colTextDim;
        SolidBrush brIco(icoCol);

        // Vector icon
        float iconSz = Sf(16.0f);
        DrawVectorIcon(gr, tb.icon, (float)(bx + btnW / 2), (float)(by + btnH * 0.32f), iconSz, icoCol);

        // Label
        RectF lblR(R(bx), R(by + btnH * 0.55f), R(btnW), R(btnH * 0.4f));
        DrawMixedString(gr, tb.lbl, -1, lblR, Sf(9.0f), icoCol, &sfC);

        // Popup arrow (vector triangle)
        if (tb.hasPopup){
            float arrSz = Sf(5.0f);
            float arrX = (float)(bx + btnW - S(10));
            float arrY = (float)(by + S(4));
            DrawVectorIcon(gr, ICO_DROPDOWN, arrX, arrY, arrSz, icoCol);
        }

        // Pen color indicator dot
        if (tb.tool == ToolKind::Pen){
            float dotR = Sf(4.0f);
            float dotX = (float)(bx + btnW - S(10));
            float dotY = (float)(by + btnH - S(10));
            SolidBrush dotBr(g.color);
            gr.FillEllipse(&dotBr, dotX - dotR, dotY - dotR, dotR * 2, dotR * 2);
        }

        // Eraser size display
        if (tb.tool == ToolKind::Eraser){
            wchar_t szBuf[8];
            swprintf(szBuf, 8, L"%.0f", g.eraserSize);
            FontFamily astEr(L"Anthropic Sans Text");
            Font fErSize(&astEr, Sf(7.0f), FontStyleRegular, UnitPixel);
            StringFormat sfEr; sfEr.SetAlignment(StringAlignmentFar);
            sfEr.SetLineAlignment(StringAlignmentFar);
            SolidBrush brErSz(icoCol);
            gr.DrawString(szBuf, -1, &fErSize,
                RectF(R(bx + S(2)), R(by + btnH * 0.55f), R(btnW - S(4)), R(btnH * 0.38f)),
                &sfEr, &brErSz);
        }

        AddHit(g_hits, bx, by, btnW, btnH, tb.id);
        bx += btnW + btnGap;
    }

    // ===== Divider removed (tools and nav are now separate) =====

    // ===== Page navigation (right side of panel) =====
    int navY = by + (btnH - (barH - S(20))) / 2;
    int navH = barH - S(20);
    FontFamily astNav(L"Anthropic Sans Text");
    Font fNavBtn(&astNav, Sf(11.0f), FontStyleBold, UnitPixel);
    StringFormat sfNav; sfNav.SetAlignment(StringAlignmentCenter);
    float navIconSz = Sf(14.0f);

    // Position navigation at far right of the panel
    // [+][鈭抅 buttons added to the right of page nav
    int addDelBtnW = S(30);
    int addDelGap = S(3);
    int navTotalW = addDelBtnW + addDelGap + addDelBtnW + addDelGap + navBtnS + navGap + navPageW + navGap + navBtnS;
    int px = barX + barW - navTotalW - S(8);

    auto drawNavIcon = [&](IconId iid, int bw, int bid) {
        Color navBtnBg = g.isDarkMode ? Color(22, 255, 255, 255) : Color(22, 0, 0, 0);
        Color navBtnBd = g.isDarkMode ? Color(18, 255, 255, 255) : Color(14, 0, 0, 0);
        FillDrawRoundRect(gr, navBtnBg, navBtnBd,
            R(px), R(navY), R(bw), R(navH), R(S(6)), 0.4f);
        DrawVectorIcon(gr, iid, (float)(px + bw/2), (float)(navY + navH/2), navIconSz, colText);
        AddHit(g_hits, px, navY, bw, navH, bid);
        px += bw + navGap;
    };

    drawNavIcon(ICO_CHEVRON_LEFT, navBtnS, ID_PAGE_PREV);
    {
        wchar_t buf[32];
        int displayPage = g.currentPage + 1;
        if (g.pdfMode && !g.pdfPageYOffset.empty()){
            RECT brPage = BoardRect(hWnd);
            float centerY = (g.scrollY + (float)(brPage.bottom - brPage.top) * 0.5f) / std::max(0.25f, g.zoomLevel);
            displayPage = Board::PdfPageAtY(centerY) + 1;
        }
        swprintf(buf, 32, L"%d/%d", displayPage, (int)g.pages.size());
        Color pageBgCol = g.isDarkMode ? Color(22, 255, 255, 255) : Color(22, 0, 0, 0);
        Color pageBdCol = g.isDarkMode ? Color(18, 255, 255, 255) : Color(14, 0, 0, 0);
        FillDrawRoundRect(gr, pageBgCol, pageBdCol,
            R(px), R(navY), R(navPageW), R(navH), R(S(6)), 0.4f);
        gr.DrawString(buf, -1, &fNavBtn,
            RectF(R(px), R(navY + navH/2 - 9), R(navPageW), R(navH)),
            &sfNav, &brTextW);
        AddHit(g_hits, px, navY, navPageW, navH, ID_PAGE_NUM_TOGGLE);
        px += navPageW + navGap;
    }
    drawNavIcon(ICO_CHEVRON_RIGHT, navBtnS, ID_PAGE_NEXT);

    // ==== Add/Del page buttons (non-PDF only) ====
    if (!g.pdfMode){
        Font fPlusMinus2(&astNav, Sf(13.0f), FontStyleBold, UnitPixel);
        StringFormat sfPM2; sfPM2.SetAlignment(StringAlignmentCenter);
        sfPM2.SetLineAlignment(StringAlignmentCenter);

        // [+] button
        FillDrawRoundRect(gr, Color(80, 60, 60, 60), colAccent,
            R(px), R(navY + S(2)), R(addDelBtnW), R(navH - S(4)), R(S(4)), 1.0f);
        DrawVectorIcon(gr, ICO_PLUS, (float)(px + addDelBtnW/2), (float)(navY + navH/2), Sf(12.0f), colAccent);
        AddHit(g_hits, px, navY, addDelBtnW, navH, ID_PAGE_ADD);
        px += addDelBtnW + addDelGap;

        // [鈭抅 button
        FillDrawRoundRect(gr, Color(80, 60, 60, 60), Color(200, 200, 60, 60),
            R(px), R(navY + S(2)), R(addDelBtnW), R(navH - S(4)), R(S(4)), 1.0f);
        DrawVectorIcon(gr, ICO_MINUS, (float)(px + addDelBtnW/2), (float)(navY + navH/2), Sf(12.0f), Color(200, 200, 60, 60));
        AddHit(g_hits, px, navY, addDelBtnW, navH, ID_PAGE_DEL);
    }
}

// ====== Thumbnail sidebar (right edge overlay) ======
// ====== Pen popup (3-row layout) ======
static void DrawPenPopup(Graphics& gr, HWND hWnd){
    if (!g.penPopupOpen) return;
    RECT br = BottomBarRect(hWnd);

    int popW = S(330), popH = S(130);
    int popX = br.left + (br.right - br.left - popW) / 2;
    int popY = br.top - popH - S(8);

    // Shadow + background
    FillRoundRect(gr, Color(40, 0, 0, 0),
        R(popX + 3), R(popY + 3), R(popW), R(popH), R(S(10)));
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(popX), R(popY), R(popW), R(popH), R(S(10)), 1.5f);

    g_popupHits.clear();

    FontFamily yh(L"Anthropic Sans Text");
    SolidBrush brDim(colTextDim);
    StringFormat sfc; sfc.SetAlignment(StringAlignmentCenter);

    // ---- Row 1: Colors ----
    int x = popX + S(14);
    int row1Y = popY + S(22);
    DrawMixedString(gr, L"颜色", -1, PointF(R(x), R(row1Y - S(7))), Sf(10.0f), colTextDim);
    x += S(36);
    for (int i = 0; i < kColorCount; ++i){
        int sz2 = S(24);
        int yy = row1Y - sz2 / 2;
        FillRoundRect(gr, kColorPalette[i], R(x), R(yy), R(sz2), R(sz2), R(S(5)));
        bool active = (g.color.GetR()==kColorPalette[i].GetR() &&
                       g.color.GetG()==kColorPalette[i].GetG() &&
                       g.color.GetB()==kColorPalette[i].GetB());
        if (active)
            DrawRoundRect(gr, colAccent, R(x-2), R(yy-2), R(sz2+4), R(sz2+4), R(S(6)), 2.0f);
        AddHit(g_popupHits, x, yy, sz2, sz2, 1000+i);
        x += sz2 + S(4);
    }

    // ---- Row 2: Pen sizes ----
    x = popX + S(14);
    int row2Y = popY + S(58);
    DrawMixedString(gr, L"粗细", -1, PointF(R(x), R(row2Y - S(7))), Sf(10.0f), colTextDim);
    x += S(36);
    for (int i = 0; i < kSizeCount; ++i){
        int box = S(28);
        int yy = row2Y - box / 2;
        bool active = std::abs(g.penSize - kSizeOptions[i]) < 0.01f;
        FillDrawRoundRect(gr, active ? Color(80, 91, 141, 239) : colBtnBg,
            active ? colAccent : Color(25, 255, 255, 255),
            R(x), R(yy), R(box), R(box), R(S(5)), 0.5f);
        float dotSz = kSizeOptions[i] + 2.0f;
        if (dotSz < 4) dotSz = 4; if (dotSz > 20) dotSz = 20;
        SolidBrush dot(active ? colAccent : colTextDim);
        gr.FillEllipse(&dot, R(x+(box-dotSz)/2), R(yy+(box-dotSz)/2), R(dotSz), R(dotSz));
        AddHit(g_popupHits, x, yy, box, box, 1100+i);
        x += box + S(4);
    }

    // ---- Row 3: Line styles (drawn as actual lines) ----
    x = popX + S(14);
    int row3Y = popY + S(96);
    DrawMixedString(gr, L"线型", -1, PointF(R(x), R(row3Y - S(7))), Sf(10.0f), colTextDim);
    x += S(36);
    LineStyle lsArr[] = {LineStyle::Solid, LineStyle::Dashed, LineStyle::Dotted};
    for (int j = 0; j < 3; ++j){
        int wbtn = S(44), hbtn = S(24), yy = row3Y - hbtn / 2;
        bool active = (g.lineStyle == lsArr[j]);
        FillDrawRoundRect(gr, active ? colAccent : colBtnBg,
            active ? colAccentDark : Color(25, 255, 255, 255),
            R(x), R(yy), R(wbtn), R(hbtn), R(S(5)), 0.5f);
        // Draw sample line (fixed preview size, accurate dash pattern matching rendering)
        {
            float previewWidth = Sf(3.0f); // always fixed size for preview
            Pen linePen(active ? Color(255,255,255,255) : colTextDim, previewWidth);
            linePen.SetStartCap(LineCapRound);
            linePen.SetEndCap(LineCapRound);
            if (lsArr[j] == LineStyle::Dashed){
                linePen.SetDashStyle(DashStyleCustom);
                REAL dash[] = {6.0f, 4.0f};
                linePen.SetDashPattern(dash, 2);
            } else if (lsArr[j] == LineStyle::Dotted){
                linePen.SetDashStyle(DashStyleCustom);
                REAL dot[] = {1.0f, 3.0f};
                linePen.SetDashPattern(dot, 2);
            }
            gr.DrawLine(&linePen, R(x + S(7)), R(row3Y), R(x + wbtn - S(7)), R(row3Y));
        }
        AddHit(g_popupHits, x, yy, wbtn, hbtn, 1200+(int)lsArr[j]);
        x += wbtn + S(4);
    }
}

// ====== Eraser popup with slider ======
static const int ID_ERASER_SLIDER = 2000;
static const int ID_ERASER_CLEAR  = 2100;

static void DrawEraserPopup(Graphics& gr, HWND hWnd){
    if (!g.eraserPopupOpen) return;
    RECT br = BottomBarRect(hWnd);

    int popW = S(330), popH = S(70);
    int popX = br.left + (br.right - br.left - popW) / 2;
    int popY = br.top - popH - S(8);

    // Shadow + background
    FillRoundRect(gr, Color(40, 0, 0, 0),
        R(popX + 3), R(popY + 3), R(popW), R(popH), R(S(10)));
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(popX), R(popY), R(popW), R(popH), R(S(10)), 1.5f);

    g_popupHits.clear();

    StringFormat sfc; sfc.SetAlignment(StringAlignmentCenter);
    sfc.SetLineAlignment(StringAlignmentCenter);

    int x = popX + S(14);
    int cy = popY + popH / 2;

    // "大小" label
    DrawMixedString(gr, L"大小", -1, PointF(R(x), R(cy - S(7))), Sf(10.0f), colTextDim);
    x += S(36);

    // Slider track
    int sliderLeft = x;
    int sliderRight = popX + popW - S(90); // leave room for clear button
    int sliderW = sliderRight - sliderLeft;
    int trackY = cy;
    int trackH = S(4);
    int trackR = trackH / 2;

    // Background track
    FillRoundRect(gr, Color(60, 255, 255, 255),
        (float)sliderLeft, (float)(trackY - trackH/2), (float)sliderW, (float)trackH, (float)trackR);

    // Filled portion (from left to thumb position)
    float t = (g.eraserSize - AppState::kEraserMin) / (AppState::kEraserMax - AppState::kEraserMin);
    if (t < 0) t = 0; if (t > 1) t = 1;
    int fillW = (int)(t * sliderW);
    if (fillW > 0){
        FillRoundRect(gr, colAccent,
            (float)sliderLeft, (float)(trackY - trackH/2), (float)fillW, (float)trackH, (float)trackR);
    }

    // Thumb circle
    int thumbR = S(10);
    int thumbX = sliderLeft + fillW;
    SolidBrush thumbBr(colAccent);
    gr.FillEllipse(&thumbBr, (float)(thumbX - thumbR), (float)(trackY - thumbR), (float)(thumbR*2), (float)(thumbR*2));
    // White inner circle
    int innerR = S(4);
    SolidBrush innerBr(Color(255, 255, 255, 255));
    gr.FillEllipse(&innerBr, (float)(thumbX - innerR), (float)(trackY - innerR), (float)(innerR*2), (float)(innerR*2));

    // Size label next to slider
    {
        wchar_t buf[16];
        swprintf(buf, 16, L"%.0f", g.eraserSize);
        FontFamily ast(L"Anthropic Sans Text");
        Font fSz(&ast, Sf(10.0f), FontStyleRegular, UnitPixel);
        SolidBrush brSz(colTextDim);
        StringFormat sfR; sfR.SetAlignment(StringAlignmentNear);
        sfR.SetLineAlignment(StringAlignmentCenter);
        gr.DrawString(buf, -1, &fSz,
            RectF((float)(sliderLeft - S(2)), (float)(trackY + thumbR + S(2)), (float)sliderW, Sf(14.0f)),
            &sfR, &brSz);
    }

    // Slider hit area (tall enough for easy touch)
    AddHit(g_popupHits, sliderLeft, cy - S(20), sliderW, S(40), ID_ERASER_SLIDER);

    // Clear screen button
    {
        int bw = S(64), bh = S(30), yy = cy - bh / 2;
        int btnX = sliderRight + S(14);
        FillDrawRoundRect(gr, colDanger, Color(255, 192, 57, 43),
            R(btnX), R(yy), R(bw), R(bh), R(S(5)), 1.0f);
        DrawMixedString(gr, L"清屏", -1, RectF(R(btnX),R(yy),R(bw),R(bh)), Sf(12.0f), colText, &sfc);
        AddHit(g_popupHits, btnX, yy, bw, bh, ID_ERASER_CLEAR);
    }
}

// ====== Zoom popup ======
static void DrawZoomPopup(Graphics& gr, HWND hWnd){
    if (!g.zoomPopupOpen) return;
    RECT br = BottomBarRect(hWnd);

    int popW = S(460), popH = S(50);
    int popX = br.left + (br.right - br.left - popW) / 2;
    int popY = br.top - popH - S(8);

    // Shadow + background
    FillRoundRect(gr, Color(40, 0, 0, 0),
        R(popX + 3), R(popY + 3), R(popW), R(popH), R(S(10)));
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(popX), R(popY), R(popW), R(popH), R(S(10)), 1.5f);

    g_popupHits.clear();

    FontFamily yh(L"Anthropic Sans Text");
    Font fBtn(&yh, Sf(12.0f), FontStyleBold, UnitPixel);
    StringFormat sfc; sfc.SetAlignment(StringAlignmentCenter);
    sfc.SetLineAlignment(StringAlignmentCenter);

    int x = popX + S(14);
    int cy = popY + popH / 2;

    // Zoom preset buttons use display percentages; internally 100% maps to 2.0x.
    static const int zoomPresets[] = { 50, 75, 100, 150, 200, 300, 350 };
    static const int zoomCmdIds[] = { ID_ZOOM_50, ID_ZOOM_75, ID_ZOOM_100, ID_ZOOM_150, ID_ZOOM_200,
                                      ID_ZOOM_300, ID_ZOOM_350 };
    int bw = S(48), bh = S(32), yy = cy - bh / 2;
    int presetCount = sizeof(zoomPresets) / sizeof(zoomPresets[0]);

    for (int i = 0; i < presetCount; ++i){
        bool active = std::abs(g.zoomLevel / kZoomDisplayBase * 100.0f - (float)zoomPresets[i]) < 1.0f;
        FillDrawRoundRect(gr, active ? colAccent : colBtnBg,
            active ? colAccentDark : Color(25, 255, 255, 255),
            R(x), R(yy), R(bw), R(bh), R(S(5)), 0.5f);
        SolidBrush br2(active ? Color(255,255,255,255) : colTextDim);
        wchar_t buf[8]; swprintf(buf, 8, L"%d%%", zoomPresets[i]);
        gr.DrawString(buf, -1, &fBtn, RectF(R(x), R(yy), R(bw), R(bh)), &sfc, &br2);
        AddHit(g_popupHits, x, yy, bw, bh, zoomCmdIds[i]);
        x += bw + S(6);
    }
}

// ====== Teaching tools popup ======
static void DrawAssistPopup(Graphics& gr, HWND hWnd){
    if (!kAssistToolsEnabled) return;
    if (!g.assistPopupOpen) return;
    RECT br = BottomBarRect(hWnd);
    int popW = S(390), popH = S(64);
    int popX = br.left + (br.right - br.left - popW) / 2;
    int popY = br.top - popH - S(8);

    FillRoundRect(gr, Color(40, 0, 0, 0),
        R(popX + 3), R(popY + 3), R(popW), R(popH), R(S(10)));
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(popX), R(popY), R(popW), R(popH), R(S(10)), 1.5f);

    g_popupHits.clear();
    StringFormat sfc; sfc.SetAlignment(StringAlignmentCenter);
    sfc.SetLineAlignment(StringAlignmentCenter);
    struct Item { int id; const wchar_t* label; };
    Item items[] = {
        { ID_ASSIST_TRIANGLE45, L"45°三角板" },
        { ID_ASSIST_TRIANGLE30, L"30°三角板" },
        { ID_ASSIST_COMPASS, L"圆规" },
        { ID_ASSIST_HIDE, L"隐藏" }
    };
    int bw = S(88), bh = S(38), gap = S(6);
    int x = popX + S(14), y = popY + (popH - bh) / 2;
    for (auto& it : items){
        bool active =
            (it.id == ID_ASSIST_TRIANGLE45 && g.assist.kind == AssistKind::Triangle && g.assist.triangleKind == TriangleKind::RightIsosceles) ||
            (it.id == ID_ASSIST_TRIANGLE30 && g.assist.kind == AssistKind::Triangle && g.assist.triangleKind == TriangleKind::ThirtySixty) ||
            (it.id == ID_ASSIST_COMPASS && g.assist.kind == AssistKind::Compass);
        FillDrawRoundRect(gr, active ? colAccent : colBtnBg,
            active ? colAccentDark : Color(25, 255, 255, 255),
            R(x), R(y), R(bw), R(bh), R(S(6)), 0.5f);
        DrawMixedString(gr, it.label, -1, RectF(R(x), R(y), R(bw), R(bh)),
            Sf(11.0f), active ? Color(255,255,255,255) : colText, &sfc);
        AddHit(g_popupHits, x, y, bw, bh, it.id);
        x += bw + gap;
    }
}

// ====== Hint toast ======
static void DrawHint(Graphics& gr, HWND hWnd){
    if (g.hintText.empty()) return;
    if (GetTickCount() > g.hintHideAt){ g.hintText.clear(); return; }
    RECT br2 = BoardRect(hWnd);
    // Measure text width using mixed font
    FontFamily ffCJK(L"Microsoft YaHei");
    FontFamily ffLatin(L"Anthropic Sans Text");
    StringFormat sfm; sfm.SetFormatFlags(StringFormatFlagsMeasureTrailingSpaces);
    // Estimate width by measuring with CJK font
    Font fM(&ffCJK, Sf(13.0f), FontStyleRegular, UnitPixel);
    RectF bound;
    gr.MeasureString(g.hintText.c_str(), -1, &fM, PointF(0,0), &sfm, &bound);
    int w = (int)bound.Width + S(32), h = S(34);
    int hx = (br2.left + br2.right) / 2 - w / 2;
    int hy = br2.top + S(14);

    // Shadow + rounded bg
    FillRoundRect(gr, Color(30, 0, 0, 0),
        R(hx + 2), R(hy + 2), R(w), R(h), R(S(8)));
    FillDrawRoundRect(gr, Color(200, 40, 46, 56), Color(40, 255, 255, 255),
        R(hx), R(hy), R(w), R(h), R(S(8)), 0.5f);

    StringFormat sfc; sfc.SetAlignment(StringAlignmentCenter);
    sfc.SetLineAlignment(StringAlignmentCenter);
    DrawMixedString(gr, g.hintText.c_str(), -1,
        RectF(R(hx + S(16)), R(hy), R(w - S(32)), R(h)),
        Sf(13.0f), Color(255,255,255,255), &sfc);
}

// ====== Ensure stable bitmap ======
static void EnsureStable(int W, int H){
    if (g.stableW == W && g.stableH == H && g.hStableBmp) return;
    if (g.hStableBmp) DeleteObject(g.hStableBmp);
    if (g.hStableDC) DeleteDC(g.hStableDC);
    HDC hScr = GetDC(g.hWnd);
    g.hStableDC = CreateCompatibleDC(hScr);
    g.hStableBmp = CreateCompatibleBitmap(hScr, W, H);
    ReleaseDC(g.hWnd, hScr);
    g.stableW = W; g.stableH = H;
}

static bool StableBitmapMatches(int W, int H){
    if (!g.hStableBmp || !g.hStableDC) return false;
    BITMAP bm{};
    if (GetObjectW(g.hStableBmp, sizeof(bm), &bm) == 0) return false;
    return bm.bmWidth == W && bm.bmHeight == H;
}

// ====== Settings dialog overlay ======
#define ID_SET_THEME_DARK   1301
#define ID_SET_THEME_LIGHT  1302
#define ID_SET_THEME_SYS    1303
#define ID_SET_CLOSE        1304
#define ID_SET_THEME_COLOR  1310  // + index
#define ID_SET_SHOW_SPLASH  1322
#define ID_SET_RESET        1323

static void DrawSettingsDialog(Graphics& gr, HWND hWnd){
    if (!g.settingsOpen) return;
    RECT br2 = BoardRect(hWnd);
    int boardW = br2.right - br2.left;
    int boardH = br2.bottom - br2.top;
    int cx = br2.left + boardW / 2;
    int cy = br2.top + boardH / 2;

    // Dim overlay
    SolidBrush dimBg(Color(140, 0, 0, 0));
    gr.FillRectangle(&dimBg, R(br2.left), R(br2.top), R(boardW), R(boardH));

    // Dialog box
    int dlgW = S(440), dlgH = S(480);
    int dlgX = cx - dlgW/2, dlgY = cy - dlgH/2;

    FillRoundRect(gr, Color(40, 0, 0, 0),
        R(dlgX + 3), R(dlgY + 3), R(dlgW), R(dlgH), R(S(12)));
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(dlgX), R(dlgY), R(dlgW), R(dlgH), R(S(12)), 1.5f);

    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter);
    sfC.SetLineAlignment(StringAlignmentCenter);

    Color accent = GetAccentColor();

    // Title
    DrawMixedString(gr, L"设置", -1,
        RectF(R(dlgX), R(dlgY + S(18)), R(dlgW), R(S(24))),
        Sf(16.0f), colText, &sfC);

    // Separator
    Pen sepPen(colPopupBorder, 1.0f);
    gr.DrawLine(&sepPen, (REAL)(dlgX + S(20)), (REAL)(dlgY + S(50)),
                (REAL)(dlgX + dlgW - S(20)), (REAL)(dlgY + S(50)));

    int itemX = dlgX + S(28);
    int itemW = dlgW - S(56);

    // === Section 1: Color mode (dark/light/system) ===
    int y = dlgY + S(65);
    DrawMixedString(gr, L"界面颜色模式", -1,
        PointF(R(itemX), R(y)), Sf(12.0f), colText);
    {
        int radioX = itemX + S(8);
        int radioY = y + S(26);
        int radioH = S(28);
        int radioGap = S(4);
        struct TO { const wchar_t* label; int id; };
        TO opts[] = {
            { L"深色（默认）", ID_SET_THEME_DARK },
            { L"浅色",         ID_SET_THEME_LIGHT },
            { L"跟随系统",     ID_SET_THEME_SYS },
        };
        for (int i = 0; i < 3; ++i){
            int ry = radioY + i * (radioH + radioGap);
            bool active = (g.colorTheme == i);
            float circR = S(7);
            float circX = radioX + circR;
            float circY = ry + radioH / 2.0f;
            if (active){
                SolidBrush brOut(accent);
                gr.FillEllipse(&brOut, circX - circR, circY - circR, circR * 2, circR * 2);
                SolidBrush brIn(colPopupBg);
                gr.FillEllipse(&brIn, circX - circR*0.5f, circY - circR*0.5f, circR, circR);
            } else {
                Pen penR(colTextDim, 1.5f);
                gr.DrawEllipse(&penR, circX - circR, circY - circR, circR * 2, circR * 2);
            }
            DrawMixedString(gr, opts[i].label, -1,
                PointF(circX + circR + S(8), ry + S(3)), Sf(12.0f),
                active ? accent : colText);
            AddHit(g_hits, radioX, ry, itemW, radioH, opts[i].id);
        }
    }

    // === Section 2: Theme color (accent) ===
    y += S(130);
    DrawMixedString(gr, L"主题色", -1,
        PointF(R(itemX), R(y)), Sf(12.0f), colText);
    {
        int colorSz = S(28);
        int colorGap = S(8);
        int totalW2 = kThemeColorCount * colorSz + (kThemeColorCount - 1) * colorGap;
        int startX = itemX + S(8);
        for (int i = 0; i < kThemeColorCount; ++i){
            int px = startX + i * (colorSz + colorGap);
            int py = y + S(24);
            FillRoundRect(gr, kThemeColors[i], (float)px, (float)py, (float)colorSz, (float)colorSz, (float)S(5));
            if (g.themeColor == i){
                DrawRoundRect(gr, Color(255,255,255,255), (float)(px-2), (float)(py-2),
                    (float)(colorSz+4), (float)(colorSz+4), (float)S(6), 2.5f);
            }
            AddHit(g_hits, px, py, colorSz, colorSz, ID_SET_THEME_COLOR + i);
        }
    }

    // === Section 3: Show splash screen ===
    y += S(70);
    {
        int cbSz = S(22);
        int cbX = dlgX + dlgW - S(28) - cbSz;
        int cbY = y + S(2);
        DrawMixedString(gr, L"启动时显示加载画面", -1,
            PointF(R(itemX), R(cbY + 2)), Sf(12.0f), colText);
        float bx2 = (float)cbX, by2 = (float)cbY, bs2 = (float)cbSz;
        if (g.showSplash){
            FillRoundRect(gr, accent, bx2, by2, bs2, bs2, (float)S(4));
            Pen penW2(Color(255,255,255,255), 2.0f);
            gr.DrawLine(&penW2, bx2+bs2*0.25f, by2+bs2*0.5f, bx2+bs2*0.42f, by2+bs2*0.68f);
            gr.DrawLine(&penW2, bx2+bs2*0.42f, by2+bs2*0.68f, bx2+bs2*0.75f, by2+bs2*0.3f);
        } else {
            Pen penB2(colPopupBorder, 1.5f);
            gr.DrawRectangle(&penB2, bx2, by2, bs2, bs2);
        }
        AddHit(g_hits, itemX, cbY - S(4), itemW, cbSz + S(8), ID_SET_SHOW_SPLASH);
    }

    // === Section 4: Check for updates ===
    y += S(50);
    {
        int upBtnW = S(160), upBtnH = S(34);
        int upBtnX = cx - upBtnW / 2;
        FillDrawRoundRect(gr, colBtnBg, colPopupBorder,
            R(upBtnX), R(y), R(upBtnW), R(upBtnH), R(S(6)), 0.8f);
        DrawMixedString(gr, L"检查更新", -1,
            RectF(R(upBtnX), R(y), R(upBtnW), R(upBtnH)),
            Sf(11.0f), colText, &sfC);
        AddHit(g_hits, upBtnX, y, upBtnW, upBtnH, ID_SET_CHECK_UPDATE);
    }

    y += S(36);
    {
        std::wstring ver = L"CloudBoard ";
        ver += APP_VERSION;
        DrawMixedString(gr, ver.c_str(), -1,
            RectF(R(itemX), R(y), R(itemW), R(S(22))),
            Sf(10.0f), colTextDim, &sfC);
    }

    // === Section 5: Reset + Close buttons ===
    y += S(38);
    int btnW = S(120), btnH = S(38);
    int btnGap = S(12);
    int totalBtnW = btnW * 2 + btnGap;
    int btnStartX = cx - totalBtnW / 2;

    // Reset button (red warning style)
    FillDrawRoundRect(gr, Color(255, 180, 60, 60), Color(255, 140, 40, 40),
        R(btnStartX), R(y), R(btnW), R(btnH), R(S(6)), 1.0f);
    DrawMixedString(gr, L"重置所有设置", -1,
        RectF(R(btnStartX), R(y), R(btnW), R(btnH)),
        Sf(11.0f), Color(255,255,255,255), &sfC);
    AddHit(g_hits, btnStartX, y, btnW, btnH, ID_SET_RESET);

    // Close button
    FillDrawRoundRect(gr, accent, GetAccentDarkColor(),
        R(btnStartX + btnW + btnGap), R(y), R(btnW), R(btnH), R(S(6)), 1.0f);
    DrawMixedString(gr, L"关闭", -1,
        RectF(R(btnStartX + btnW + btnGap), R(y), R(btnW), R(btnH)),
        Sf(13.0f), Color(255,255,255,255), &sfC);
    AddHit(g_hits, btnStartX + btnW + btnGap, y, btnW, btnH, ID_SET_CLOSE);
}

// ====== Selection right-click popup ======
static void DrawSelPopup(Graphics& gr, HWND hWnd){
    if (!g.selPopupOpen) return;

    // Layout: top row = 2 action buttons (Copy, Delete), bottom = color palette row
    int cellH = S(48);
    int gap = S(2);
    int pad = S(8);
    int colorSz = S(24);
    int colorGap = S(4);
    int colorRowH = colorSz + S(8);
    // Calculate width to fit 8 color swatches with gaps
    int totalColorW = kColorCount * colorSz + (kColorCount - 1) * colorGap;
    int popW = totalColorW + pad * 2;
    // Make sure popup is at least wide enough for buttons
    int minBtnW = S(160);
    if (popW < minBtnW) popW = minBtnW;
    int popH = cellH + gap + colorRowH + pad * 2;
    int popX = g.selPopupX, popY = g.selPopupY;

    // Clamp to board area
    RECT br2 = BoardRect(hWnd);
    if (popX + popW > br2.right - S(4)) popX = br2.right - popW - S(4);
    if (popY + popH > br2.bottom - S(4)) popY = br2.bottom - popH - S(4);
    if (popX < br2.left + S(4)) popX = br2.left + S(4);
    if (popY < br2.top + S(4)) popY = br2.top + S(4);

    // Shadow
    FillRoundRect(gr, Color(30, 0, 0, 0),
        R(popX + 2), R(popY + 3), R(popW), R(popH), R(S(8)));
    // Background
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(popX), R(popY), R(popW), R(popH), R(S(8)), 1.2f);

    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter);
    sfC.SetLineAlignment(StringAlignmentCenter);

    int btnW2 = (popW - pad * 2 - gap) / 2;

    // Copy button (left)
    {
        int bx = popX + pad, by = popY + pad;
        FillDrawRoundRect(gr, colAccent, colAccentDark,
            R(bx), R(by), R(btnW2), R(cellH), R(S(5)), 1.0f);
        DrawVectorIcon(gr, ICO_COPY, (float)(bx + btnW2 / 2),
                       (float)(by + cellH * 0.4f), Sf(14.0f), colText);
        DrawMixedString(gr, L"复制", -1,
            RectF(R(bx), R(by + cellH * 0.62f), R(btnW2), R(cellH * 0.35f)),
            Sf(9.0f), colText, &sfC);
        AddHit(g_popupHits, bx, by, btnW2, cellH, ID_SEL_COPY);
    }
    // Delete button (right)
    {
        int bx = popX + pad + btnW2 + gap, by = popY + pad;
        FillDrawRoundRect(gr, Color(200, 192, 57, 43), Color(255, 192, 57, 43),
            R(bx), R(by), R(btnW2), R(cellH), R(S(5)), 1.0f);
        DrawVectorIcon(gr, ICO_DELETE, (float)(bx + btnW2 / 2),
                       (float)(by + cellH * 0.4f), Sf(14.0f), colText);
        DrawMixedString(gr, L"删除", -1,
            RectF(R(bx), R(by + cellH * 0.62f), R(btnW2), R(cellH * 0.35f)),
            Sf(9.0f), colText, &sfC);
        AddHit(g_popupHits, bx, by, btnW2, cellH, ID_SEL_DELETE);
    }

    // Color palette row
    int colorY = popY + pad + cellH + gap;
    int colorStartX = popX + (popW - totalColorW) / 2;
    for (int i = 0; i < kColorCount; ++i){
        int cx = colorStartX + i * (colorSz + colorGap);
        int cy = colorY + (colorRowH - colorSz) / 2;
        FillRoundRect(gr, kColorPalette[i], R(cx), R(cy), R(colorSz), R(colorSz), R(S(4)));
        // Border
        Pen bPen(Color(60, 0, 0, 0), 0.8f);
        gr.DrawRectangle(&bPen, (REAL)cx, (REAL)cy, (REAL)colorSz, (REAL)colorSz);
        AddHit(g_popupHits, cx, cy, colorSz, colorSz, ID_SEL_COLOR + i);
    }
}

// ====== Custom confirm dialog overlay ======
static void DrawConfirmDialog(Graphics& gr, HWND hWnd){
    if (!g.confirm.active) return;
    RECT br2 = BoardRect(hWnd);
    int boardW = br2.right - br2.left;
    int boardH = br2.bottom - br2.top;
    int cx = br2.left + boardW / 2;
    int cy = br2.top + boardH / 2;

    // Dim overlay
    SolidBrush dimBg(Color(140, 0, 0, 0));
    gr.FillRectangle(&dimBg, R(br2.left), R(br2.top), R(boardW), R(boardH));

    bool hasThird = !g.confirm.thirdLabel.empty();

    // Dialog box - taller for 3 vertical buttons
    int dlgW = S(340);
    int dlgH = hasThird ? S(300) : S(180);
    int dlgX = cx - dlgW/2, dlgY = cy - dlgH/2;

    FillRoundRect(gr, Color(40, 0, 0, 0),
        R(dlgX + 3), R(dlgY + 3), R(dlgW), R(dlgH), R(S(12)));
    FillDrawRoundRect(gr, colPopupBg, colPopupBorder,
        R(dlgX), R(dlgY), R(dlgW), R(dlgH), R(S(12)), 1.5f);

    FontFamily yh(L"Anthropic Sans Text");
    Font fBtn(&yh, Sf(12.0f), FontStyleBold, UnitPixel);
    SolidBrush brTextW(colText);
    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter);
    sfC.SetLineAlignment(StringAlignmentCenter);

    if (hasThird){
        // ===== 3-button vertical layout =====
        int msgH = S(60);
        // Message
        DrawMixedString(gr, g.confirm.message.c_str(), -1,
            RectF(R(dlgX + S(20)), R(dlgY + S(14)), R(dlgW - S(40)), R(msgH)),
            Sf(14.0f), colText, &sfC);

        // 3 buttons stacked vertically, full width
        int btnW2 = dlgW - S(48);
        int btnH2 = S(42);
        int btnGap2 = S(10);
        int btnX2 = dlgX + S(24);
        int btnYStart = dlgY + S(14) + msgH + S(10);

        // Button 1: confirm (accent)
        {
            int by = btnYStart;
            FillDrawRoundRect(gr, colAccent, colAccentDark,
                R(btnX2), R(by), R(btnW2), R(btnH2), R(S(6)), 1.0f);
            DrawMixedString(gr, g.confirm.altLabel.empty() ? L"确定" : g.confirm.altLabel.c_str(),
                -1, RectF(R(btnX2), R(by), R(btnW2), R(btnH2)),
                Sf(12.0f), colText, &sfC);
            AddHit(g_hits, btnX2, by, btnW2, btnH2, ID_CONFIRM_NO + 1);
        }
        // Button 2: alt (danger / secondary)
        {
            int by = btnYStart + (btnH2 + btnGap2);
            FillDrawRoundRect(gr, Color(200, 192, 57, 43), Color(255, 192, 57, 43),
                R(btnX2), R(by), R(btnW2), R(btnH2), R(S(6)), 1.0f);
            DrawMixedString(gr, g.confirm.thirdLabel.c_str(), -1,
                RectF(R(btnX2), R(by), R(btnW2), R(btnH2)),
                Sf(12.0f), colText, &sfC);
            AddHit(g_hits, btnX2, by, btnW2, btnH2, ID_CONFIRM_NO + 2);
        }
        // Button 3: cancel (ghost)
        {
            int by = btnYStart + 2 * (btnH2 + btnGap2);
            FillDrawRoundRect(gr, colBtnBg, Color(50, 255, 255, 255),
                R(btnX2), R(by), R(btnW2), R(btnH2), R(S(6)), 1.0f);
            DrawMixedString(gr, g.confirm.cancelLabel.empty() ? L"取消" : g.confirm.cancelLabel.c_str(),
                -1, RectF(R(btnX2), R(by), R(btnW2), R(btnH2)),
                Sf(12.0f), colText, &sfC);
            AddHit(g_hits, btnX2, by, btnW2, btnH2, ID_CONFIRM_NO);
        }
    } else {
        // ===== Original 2-button horizontal layout =====
        // Message
        DrawMixedString(gr, g.confirm.message.c_str(), -1,
            RectF(R(dlgX + S(20)), R(dlgY + S(10)), R(dlgW - S(40)), R(dlgH - S(70))),
            Sf(14.0f), colText, &sfC);

        int btnH = S(38);
        int btnY2 = dlgY + dlgH - btnH - S(16);
        int btnW = S(90);
        int gap = S(12);
        int btnCount = g.confirm.altLabel.empty() ? 2 : 3;
        int totalBtnW = btnCount * btnW + (btnCount - 1) * gap;
        int startX = cx - totalBtnW / 2;

        // "确定" button
        {
            int bx = startX;
            FillDrawRoundRect(gr, colAccent, colAccentDark,
                R(bx), R(btnY2), R(btnW), R(btnH), R(S(6)), 1.0f);
            DrawMixedString(gr, L"确定", -1,
                RectF(R(bx), R(btnY2), R(btnW), R(btnH)),
                Sf(12.0f), colText, &sfC);
            AddHit(g_hits, bx, btnY2, btnW, btnH, ID_CONFIRM_YES);
        }
        // Optional alt button
        if (!g.confirm.altLabel.empty()){
            int bx = startX + btnW + gap;
            FillDrawRoundRect(gr, Color(200, 192, 57, 43), Color(255, 192, 57, 43),
                R(bx), R(btnY2), R(btnW), R(btnH), R(S(6)), 1.0f);
            DrawMixedString(gr, g.confirm.altLabel.c_str(), -1,
                RectF(R(bx), R(btnY2), R(btnW), R(btnH)),
                Sf(12.0f), colText, &sfC);
            AddHit(g_hits, bx, btnY2, btnW, btnH, ID_CONFIRM_NO + 1);
        }
        // "取消" button
        {
            int bx = startX + (btnCount - 1) * (btnW + gap);
            FillDrawRoundRect(gr, colBtnBg, Color(50, 255, 255, 255),
                R(bx), R(btnY2), R(btnW), R(btnH), R(S(6)), 1.0f);
            DrawMixedString(gr, L"取消", -1,
                RectF(R(bx), R(btnY2), R(btnW), R(btnH)),
                Sf(12.0f), colText, &sfC);
            AddHit(g_hits, bx, btnY2, btnW, btnH, ID_CONFIRM_NO);
        }
    }
}

// ====== Loading overlay ======
static void DrawLoadingOverlay(Graphics& gr, HWND hWnd){
    if (!g.loadingActive) return;
    RECT rc2; GetClientRect(hWnd, &rc2);
    int cw = rc2.right, ch = rc2.bottom;

    // Semi-transparent dark overlay over entire window
    SolidBrush dimBg(Color(160, 0, 0, 0));
    gr.FillRectangle(&dimBg, 0, 0, cw, ch);

    // Centered progress panel
    int panelW = S(360), panelH = S(140);
    int px = (cw - panelW) / 2;
    int py = (ch - panelH) / 2;

    FillDrawRoundRect(gr, Color(240, 30, 36, 54), Color(100, 91, 141, 239),
        R(px), R(py), R(panelW), R(panelH), R(S(14)), 1.5f);

    FontFamily yh(L"Anthropic Sans Text");
    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter);
    sfC.SetLineAlignment(StringAlignmentCenter);

    // Title text - always light on dark overlay
    Color colTitle(255, 232, 236, 241);
    DrawMixedString(gr, g.loadingText.c_str(), -1,
        RectF(R(px), R(py + S(14)), R(panelW), R(S(24))),
        Sf(14.0f), colTitle, &sfC);

    // Progress bar
    int lp = g.loadingProgress.load();
    float progress = g.loadingTotal > 0 ? (float)lp / (float)g.loadingTotal : 0.0f;
    int barMargin = S(30);
    int barX = px + barMargin;
    int barY = py + panelH / 2 + S(4);
    int barW = panelW - barMargin * 2;
    int barH = S(12);
    FillRoundRect(gr, Color(80, 255, 255, 255), R(barX), R(barY), R(barW), R(barH), R(S(6)));
    if (progress > 0.001f){
        int fillW = std::max(S(12), (int)(barW * progress));
        FillRoundRect(gr, colAccent, R(barX), R(barY), R(fillW), R(barH), R(S(6)));
    }

    // Progress text below bar - always light
    wchar_t buf[64];
    swprintf(buf, 64, L"%d / %d  (%.0f%%)", lp, g.loadingTotal, progress * 100.0f);
    SolidBrush brProgText(Color(255, 200, 210, 220));
    Font fProg(&yh, Sf(11.0f), FontStyleRegular, UnitPixel);
    gr.DrawString(buf, -1, &fProg,
        RectF(R(barX), R(barY + barH + S(6)), R(barW), R(S(20))), &sfC, &brProgText);

}

// ====== File system helpers ======
static void FileBrowserLoadDir(const std::wstring& dir, bool pushHistory = true){
    // MUST copy dir to local - caller may pass g.fileBrowser.fullPath[idx]
    // which gets destroyed when we clear the vectors below.
    std::wstring path = dir;

    g.fileBrowser.currentDir = path;
    g.fileBrowser.entries.clear();
    g.fileBrowser.fullPath.clear();
    g.fileBrowser.isDir.clear();
    g.fileBrowser.scrollY = 0;

    // Push to navigation history
    if (pushHistory){
        // Trim forward history
        if (g.fileBrowser.historyIdx >= 0 && g.fileBrowser.historyIdx < (int)g.fileBrowser.history.size() - 1)
            g.fileBrowser.history.resize(g.fileBrowser.historyIdx + 1);
        g.fileBrowser.history.push_back(path);
        g.fileBrowser.historyIdx = (int)g.fileBrowser.history.size() - 1;
    }

    WIN32_FIND_DATAW fd;
    std::wstring pattern = path + L"\\*";
    HANDLE hFind = FindFirstFileW(pattern.c_str(), &fd);
    if (hFind == INVALID_HANDLE_VALUE) return;

    // Collect items (keep ".." separate)
    std::vector<std::wstring> dirs, dirsP, files, filesP;
    bool hasParent = false;
    std::wstring parentName, parentPath;
    do {
        std::wstring name = fd.cFileName;
        if (name == L".") continue;
        bool isDir = (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        std::wstring full = path + L"\\" + name;

        if (isDir){
            if (name == L".."){
                hasParent = true;
                parentName = name;
                // Resolve ".." to canonical parent path
                wchar_t resolved[MAX_PATH] = {};
                if (GetFullPathNameW(full.c_str(), MAX_PATH, resolved, nullptr))
                    parentPath = resolved;
                else
                    parentPath = full;
            } else {
                dirs.push_back(name);
                dirsP.push_back(full);
            }
        } else {
            size_t dot = name.find_last_of(L'.');
            if (dot != std::wstring::npos){
                std::wstring ext = name.substr(dot+1);
                for (auto& c : ext) c = (wchar_t)towlower(c);
                if (ext == L"pdf" || ext == L"png" || ext == L"jpg" || ext == L"jpeg" ||
                    ext == L"bmp" || ext == L"gif"){
                    files.push_back(name);
                    filesP.push_back(full);
                }
            }
        }
    } while (FindNextFileW(hFind, &fd));
    FindClose(hFind);

    // Sort helper
    auto sortAlpha = [](std::vector<std::wstring>& names, std::vector<std::wstring>& paths){
        std::vector<int> idx(names.size());
        for (int i = 0; i < (int)idx.size(); ++i) idx[i] = i;
        std::sort(idx.begin(), idx.end(), [&](int a, int b){ return _wcsicmp(names[a].c_str(), names[b].c_str()) < 0; });
        std::vector<std::wstring> nn = names, pp = paths;
        for (int i = 0; i < (int)idx.size(); ++i){ names[i] = nn[idx[i]]; paths[i] = pp[idx[i]]; }
    };

    sortAlpha(dirs, dirsP);
    sortAlpha(files, filesP);

    // Merge: ".." first, then dirs, then files
    if (hasParent){
        g.fileBrowser.entries.push_back(parentName);
        g.fileBrowser.fullPath.push_back(parentPath);
        g.fileBrowser.isDir.push_back(true);
    }
    for (int i = 0; i < (int)dirs.size(); ++i){
        g.fileBrowser.entries.push_back(dirs[i]);
        g.fileBrowser.fullPath.push_back(dirsP[i]);
        g.fileBrowser.isDir.push_back(true);
    }
    for (int i = 0; i < (int)files.size(); ++i){
        g.fileBrowser.entries.push_back(files[i]);
        g.fileBrowser.fullPath.push_back(filesP[i]);
        g.fileBrowser.isDir.push_back(false);
    }
}

// Navigate back/forward in history
static void FileBrowserGoBack(){
    if (g.fileBrowser.historyIdx > 0){
        g.fileBrowser.historyIdx--;
        FileBrowserLoadDir(g.fileBrowser.history[g.fileBrowser.historyIdx], false);
    }
}
static void FileBrowserGoForward(){
    if (g.fileBrowser.historyIdx < (int)g.fileBrowser.history.size() - 1){
        g.fileBrowser.historyIdx++;
        FileBrowserLoadDir(g.fileBrowser.history[g.fileBrowser.historyIdx], false);
    }
}

// ====== File browser overlay ======
// ====== Explorer-style file browser overlay ======

// Helper: draw a folder icon (GDI+ vector)
static void DrawFolderIcon(Graphics& gr, float x, float y, float sz, bool isOpen){
    float sw = std::max(1.0f, sz / 12.0f);
    Color folderCol(255, 245, 185, 59);      // golden yellow
    Color folderBack(255, 215, 155, 30);
    Pen penOut(Color(255, 190, 140, 20), sw);

    // Back panel
    SolidBrush brBack(folderBack);
    float backW = sz * 0.85f, backH = sz * 0.65f;
    float backX = x + sz * 0.08f, backY = y + sz * 0.25f;
    gr.FillRectangle(&brBack, backX, backY, backW, backH);
    gr.DrawRectangle(&penOut, backX, backY, backW, backH);

    // Front tab
    SolidBrush brFront(folderCol);
    float tabW = sz * 0.35f, tabH = sz * 0.18f;
    gr.FillRectangle(&brFront, backX, backY - tabH + sw, tabW, tabH);
    gr.DrawRectangle(&penOut, backX, backY - tabH + sw, tabW, tabH);

    // Front panel (slightly offset to show 3D)
    float frontY = backY + sz * 0.12f;
    float frontH = backH - sz * 0.12f;
    gr.FillRectangle(&brFront, backX, frontY, backW, frontH);
    gr.DrawRectangle(&penOut, backX, frontY, backW, frontH);
}

// Helper: draw file type icon
static void DrawFileIcon(Graphics& gr, float x, float y, float sz, const std::wstring& ext){
    float sw = std::max(1.0f, sz / 12.0f);
    float w = sz * 0.7f, h = sz * 0.88f;
    float fx = x + (sz - w) * 0.5f, fy = y + sz * 0.06f;

    // Dog-ear corner
    float ear = sz * 0.2f;
    GraphicsPath path;
    path.AddLine(fx, fy, fx + w - ear, fy);
    path.AddLine(fx + w - ear, fy, fx + w, fy + ear);
    path.AddLine(fx + w, fy + ear, fx + w, fy + h);
    path.AddLine(fx + w, fy + h, fx, fy + h);
    path.CloseFigure();

    SolidBrush brDoc(Color(255, 255, 255, 255));
    Pen penDoc(Color(255, 160, 165, 175), sw);
    gr.FillPath(&brDoc, &path);
    gr.DrawPath(&penDoc, &path);

    // Dog-ear fold
    Pen penFold(Color(255, 140, 145, 155), sw * 0.7f);
    gr.DrawLine(&penFold, fx + w - ear, fy, fx + w - ear, fy + ear);
    gr.DrawLine(&penFold, fx + w - ear, fy + ear, fx + w, fy + ear);

    // Extension label
    FontFamily yh(L"Anthropic Sans Text");
    Font fExt(&yh, Sf(7.0f), FontStyleBold, UnitPixel);
    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter);
    sfC.SetLineAlignment(StringAlignmentCenter);

    std::wstring label = ext;
    for (auto& c : label) c = (wchar_t)towlower(c);
    Color labelCol(255, 255, 255, 255);
    if (label == L"pdf") labelCol = Color(255, 220, 50, 50);
    else if (label == L"png" || label == L"jpg" || label == L"jpeg" || label == L"bmp" || label == L"gif")
        labelCol = Color(255, 50, 140, 220);

    SolidBrush brLabel(labelCol);
    RectF labelRect(fx, fy + h * 0.45f, w, h * 0.45f);
    gr.DrawString(label.c_str(), -1, &fExt, labelRect, &sfC, &brLabel);
}

// Helper: draw an up arrow icon
static void DrawUpArrow(Graphics& gr, float cx, float cy, float sz, Color col){
    float sw = std::max(1.2f, sz / 10.0f);
    Pen pen(col, sw);
    pen.SetStartCap(LineCapRound);
    pen.SetEndCap(LineCapRound);
    pen.SetLineJoin(LineJoinRound);
    float half = sz * 0.35f;
    gr.DrawLine(&pen, cx, cy + half, cx, cy - half);
    gr.DrawLine(&pen, cx - half * 0.6f, cy - half * 0.2f, cx, cy - half);
    gr.DrawLine(&pen, cx + half * 0.6f, cy - half * 0.2f, cx, cy - half);
}

static void DrawFileBrowser(Graphics& gr, HWND hWnd){
    if (!g.fileBrowser.active) return;
    RECT br2 = BoardRect(hWnd);
    int boardW = br2.right - br2.left;
    int boardH = br2.bottom - br2.top;

    // Semi-transparent overlay
    SolidBrush dimBg(Color(120, 0, 0, 0));
    gr.FillRectangle(&dimBg, R(br2.left), R(br2.top), R(boardW), R(boardH));

    // Panel - match Windows Explorer proportions, nearly full-width
    int panelW = std::min(S(680), boardW - S(20));
    int panelH = std::min(S(560), boardH - S(20));
    int panelX = br2.left + (boardW - panelW) / 2;
    int panelY = br2.top + (boardH - panelH) / 2;

    // ====== Explorer-style colors (light theme) ======
    Color exWhite(255, 255, 255, 255);
    Color exTitleBar(255, 240, 240, 240);          // light gray title bar
    Color exToolbar(255, 243, 243, 243);            // toolbar bg
    Color exToolbarBorder(255, 218, 218, 218);
    Color exNavSidebar(255, 248, 248, 248);         // left nav panel
    Color exNavSidebarHover(255, 230, 238, 250);    // hover for nav items
    Color exNavSidebarActive(255, 215, 228, 248);   // active nav item
    Color exPathBg(255, 255, 255, 255);
    Color exPathBorder(255, 204, 204, 204);
    Color exItemHover(255, 235, 240, 250);
    Color exItemFolder(255, 245, 185, 59);
    Color exText(255, 30, 30, 30);
    Color exTextDim(255, 120, 120, 120);
    Color exAccent(255, 0, 102, 204);               // Win11 accent
    Color exCloseBtn(255, 232, 17, 35);             // red close on hover
    Color exStatusBg(255, 240, 240, 240);
    Color exDivider(255, 228, 228, 228);

    // Shadow
    FillRoundRect(gr, Color(50, 0, 0, 0),
        R(panelX + S(4)), R(panelY + S(4)), R(panelW), R(panelH), R(S(8)));

    // Panel background (white)
    FillDrawRoundRect(gr, exWhite, Color(255, 200, 200, 200),
        R(panelX), R(panelY), R(panelW), R(panelH), R(S(8)), 1.0f);

    FontFamily yh(L"Anthropic Sans Text");
    FontFamily ast(L"Anthropic Sans Text");
    Font fTitle(&yh, Sf(12.0f), FontStyleRegular, UnitPixel);
    Font fNav(&ast, Sf(9.0f), FontStyleRegular, UnitPixel);
    Font fPath(&ast, Sf(10.0f), FontStyleRegular, UnitPixel);
    Font fItem(&yh, Sf(12.0f), FontStyleRegular, UnitPixel);
    Font fStatus(&yh, Sf(9.0f), FontStyleRegular, UnitPixel);
    SolidBrush brText(exText);
    SolidBrush brTextDim(exTextDim);
    SolidBrush brAccent(exAccent);
    SolidBrush brWhite(exWhite);
    StringFormat sfL; sfL.SetAlignment(StringAlignmentNear); sfL.SetLineAlignment(StringAlignmentCenter);
    StringFormat sfC; sfC.SetAlignment(StringAlignmentCenter); sfC.SetLineAlignment(StringAlignmentCenter);

    int titleH = S(36);
    int toolbarH = S(38);
    int sidebarW = S(160);
    int statusH = S(26);

    // ====== 1. Title bar ======
    {
        // Title bar background (light gray)
        FillRoundRect(gr, exTitleBar,
            R(panelX), R(panelY), R(panelW), R(titleH), R(S(8)));
        // Fill bottom corners to square
        SolidBrush tbBg(exTitleBar);
        gr.FillRectangle(&tbBg, R(panelX), R(panelY + titleH - S(8)), R(panelW), R(S(8)));

        // Title text: folder name
        std::wstring folderName = g.fileBrowser.currentDir;
        size_t lastSlash = folderName.find_last_of(L"\\/");
        if (lastSlash != std::wstring::npos && lastSlash > 0)
            folderName = folderName.substr(lastSlash + 1);
        if (folderName.empty()) folderName = g.fileBrowser.currentDir;

        DrawMixedString(gr, folderName.c_str(), -1,
            PointF(R(panelX + S(14)), R(panelY + titleH / 2 - S(7))), Sf(12.0f), exText);

        // Close button (X) - Windows 11 style: top-right, subtle rounded bg
        int closeW = S(46), closeH = titleH;
        int closeX = panelX + panelW - closeW;
        int closeY = panelY;
        FillRoundRect(gr, Color(255, 240, 240, 240),
            R(closeX + S(3)), R(closeY + S(3)), R(closeW - S(6)), R(closeH - S(6)), R(S(4)));
        float cx = (float)(closeX + closeW / 2);
        float cy = (float)(closeY + closeH / 2);
        float half = Sf(5.5f);
        Pen closePen(Color(255, 80, 80, 80), 1.4f);
        closePen.SetStartCap(LineCapRound);
        closePen.SetEndCap(LineCapRound);
        gr.DrawLine(&closePen, cx - half, cy - half, cx + half, cy + half);
        gr.DrawLine(&closePen, cx + half, cy - half, cx - half, cy + half);
        AddHit(g_hits, closeX, closeY, closeW, closeH, ID_FILE_CLOSE);
    }

    // ====== 2. Toolbar (back, forward, up + address bar) ======
    {
        int tbY = panelY + titleH;
        SolidBrush tbBg(exToolbar);
        gr.FillRectangle(&tbBg, R(panelX), R(tbY), R(panelW), R(toolbarH));
        // Bottom border
        SolidBrush divLine(exToolbarBorder);
        gr.FillRectangle(&divLine, R(panelX), R(tbY + toolbarH - 1), R(panelW), R(1));

        int btnSz = S(28);
        int btnY = tbY + (toolbarH - btnSz) / 2;
        int curX = panelX + S(8);

        // Back button
        {
            bool canBack = g.fileBrowser.historyIdx > 0;
            Color arrowCol = canBack ? exText : exTextDim;
            FillRoundRect(gr, Color(15, 0, 0, 0),
                R(curX), R(btnY), R(btnSz), R(btnSz), R(S(4)));
            float cx = (float)(curX + btnSz / 2);
            float cy = (float)(btnY + btnSz / 2);
            float sw = std::max(1.2f, Sf(8.0f) / 10.0f);
            Pen pen(arrowCol, sw);
            pen.SetStartCap(LineCapRound); pen.SetEndCap(LineCapRound);
            float half = Sf(8.0f);
            gr.DrawLine(&pen, cx + half * 0.5f, cy - half, cx - half * 0.6f, cy);
            gr.DrawLine(&pen, cx - half * 0.6f, cy, cx + half * 0.5f, cy + half);
            if (canBack) AddHit(g_hits, curX, btnY, btnSz, btnSz, ID_FILE_BACK);
            curX += btnSz + S(4);
        }

        // Forward button
        {
            bool canFwd = g.fileBrowser.historyIdx < (int)g.fileBrowser.history.size() - 1;
            Color arrowCol = canFwd ? exText : exTextDim;
            FillRoundRect(gr, Color(15, 0, 0, 0),
                R(curX), R(btnY), R(btnSz), R(btnSz), R(S(4)));
            float cx = (float)(curX + btnSz / 2);
            float cy = (float)(btnY + btnSz / 2);
            float sw = std::max(1.2f, Sf(8.0f) / 10.0f);
            Pen pen(arrowCol, sw);
            pen.SetStartCap(LineCapRound); pen.SetEndCap(LineCapRound);
            float half = Sf(8.0f);
            gr.DrawLine(&pen, cx - half * 0.5f, cy - half, cx + half * 0.6f, cy);
            gr.DrawLine(&pen, cx + half * 0.6f, cy, cx - half * 0.5f, cy + half);
            if (canFwd) AddHit(g_hits, curX, btnY, btnSz, btnSz, ID_FILE_FORWARD);
            curX += btnSz + S(4);
        }

        // Up button
        {
            FillRoundRect(gr, Color(15, 0, 0, 0),
                R(curX), R(btnY), R(btnSz), R(btnSz), R(S(4)));
            float cx = (float)(curX + btnSz / 2);
            float cy = (float)(btnY + btnSz / 2);
            DrawUpArrow(gr, cx, cy, Sf(18.0f), exText);
            AddHit(g_hits, curX, btnY, btnSz, btnSz, ID_FILE_PARENT);
            curX += btnSz + S(8);
        }

        // Address bar (breadcrumb path)
        {
            int addrX = curX;
            int addrW = panelX + panelW - addrX - S(10);
            int addrH = S(26);
            int addrY = tbY + (toolbarH - addrH) / 2;

            // White rounded rect with thin border
            FillDrawRoundRect(gr, exPathBg, exPathBorder,
                R(addrX), R(addrY), R(addrW), R(addrH), R(S(4)), 0.8f);

            // Draw path with ">" separators
            std::wstring path = g.fileBrowser.currentDir;
            // Show abbreviated path if too long
            if ((int)path.length() > 50){
                // Keep first 3 chars (drive) and last part
                size_t lastS = path.find_last_of(L"\\/");
                if (lastS != std::wstring::npos)
                    path = path.substr(0, 4) + L"... > " + path.substr(lastS + 1);
            }
            DrawMixedString(gr, path.c_str(), -1,
                RectF(R(addrX + S(8)), R(addrY), R(addrW - S(16)), R(addrH)), Sf(10.0f), exText, &sfL);
        }
    }

    // ====== 3. Left navigation sidebar ======
    {
        int sideX = panelX;
        int sideY = panelY + titleH + toolbarH;
        int sideH = panelH - titleH - toolbarH - statusH;

        // Background
        SolidBrush sideBg(exNavSidebar);
        gr.FillRectangle(&sideBg, R(sideX), R(sideY), R(sidebarW), R(sideH));

        // Right border
        SolidBrush divLine(exDivider);
        gr.FillRectangle(&divLine, R(sideX + sidebarW - 1), R(sideY), R(1), R(sideH));

        // Quick Access header
        int itemY = sideY + S(6);
        DrawMixedString(gr, L"快速访问", -1,
            PointF(R(sideX + S(12)), R(itemY)), Sf(9.0f), exTextDim);
        itemY += S(20);

        // Quick access items
        struct NavItem { int id; const wchar_t* label; const wchar_t* icon; };
        NavItem navItems[] = {
            { ID_FILE_NAV_DESKTOP, L"桌面", L"PC" },
            { ID_FILE_NAV_DOWNLOADS, L"下载", L"DL" },
            { ID_FILE_NAV_DOCUMENTS, L"文档", L"DOC" },
            { ID_FILE_NAV_PICTURES, L"图片", L"IMG" },
            { ID_FILE_NAV_THISPC, L"此电脑", L"PC" },
        };
        int navCount = sizeof(navItems) / sizeof(navItems[0]);

        Font fSideItem(&yh, Sf(10.0f), FontStyleRegular, UnitPixel);
        int navItemH = S(28);

        for (int i = 0; i < navCount; ++i){
            int iy = itemY + i * navItemH;

            // Get the actual path for active-state comparison
            wchar_t actualPath[MAX_PATH] = {};
            int csidl = -1;
            if (navItems[i].id == ID_FILE_NAV_DESKTOP) csidl = CSIDL_DESKTOP;
            else if (navItems[i].id == ID_FILE_NAV_DOWNLOADS) csidl = -3; // special handling
            else if (navItems[i].id == ID_FILE_NAV_DOCUMENTS) csidl = CSIDL_PERSONAL;
            else if (navItems[i].id == ID_FILE_NAV_PICTURES) csidl = CSIDL_MYPICTURES;
            else if (navItems[i].id == ID_FILE_NAV_THISPC) csidl = -2;

            bool isActive = false;
            if (csidl >= 0 && SUCCEEDED(SHGetFolderPathW(nullptr, csidl, nullptr, 0, actualPath))){
                isActive = (_wcsicmp(g.fileBrowser.currentDir.c_str(), actualPath) == 0);
            } else if (csidl == -2){
                isActive = (g.fileBrowser.currentDir == L"C:\\" || g.fileBrowser.currentDir.length() <= 3);
            } else if (csidl == -3){
                // Downloads: %USERPROFILE%\Downloads
                if (SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_PROFILE, nullptr, 0, actualPath))){
                    wcscat_s(actualPath, L"\\Downloads");
                    DWORD attr = GetFileAttributesW(actualPath);
                    if (attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY))
                        isActive = (_wcsicmp(g.fileBrowser.currentDir.c_str(), actualPath) == 0);
                }
            }

            // Active/hover background
            if (isActive){
                SolidBrush activeBg(exNavSidebarActive);
                gr.FillRectangle(&activeBg, R(sideX + S(4)), R(iy), R(sidebarW - S(8)), R(navItemH));
            }

            // Icon + label
            std::wstring label = std::wstring(navItems[i].icon) + L" " + navItems[i].label;
            Color itemCol = isActive ? exAccent : exText;
            DrawMixedString(gr, label.c_str(), -1,
                RectF(R(sideX + S(12)), R(iy), R(sidebarW - S(24)), R(navItemH)),
                Sf(10.0f), itemCol, &sfL);

            AddHit(g_hits, sideX + S(4), iy, sidebarW - S(8), navItemH, navItems[i].id);
        }
    }

    // ====== 4. Main content area (file list) ======
    {
        int listX = panelX + sidebarW;
        int listY = panelY + titleH + toolbarH;
        int listW = panelW - sidebarW;
        int listH = panelH - titleH - toolbarH - statusH;
        int itemH = S(36);

        // White background
        SolidBrush listBg(exWhite);
        gr.FillRectangle(&listBg, R(listX), R(listY), R(listW), R(listH));

        // Clip to content area
        RectF clipRect(R(listX), R(listY), R(listW), R(listH));
        Region clip(clipRect);
        gr.SetClip(&clip);

        float scrollOff = (float)g.fileBrowser.scrollY;
        for (int i = 0; i < (int)g.fileBrowser.entries.size(); ++i){
            int iy = listY + i * itemH - (int)scrollOff;
            if (iy + itemH < listY) continue;
            if (iy > listY + listH) break;

            // Alternating row background (very subtle)
            if (i % 2 == 1){
                SolidBrush rowBg(Color(255, 248, 249, 250));
                gr.FillRectangle(&rowBg, R(listX), R(iy), R(listW), R(itemH));
            }

            // Icon
            float iconSz = Sf(20.0f);
            float iconX = (float)(listX + S(10));
            float iconY = (float)(iy + (itemH - (int)iconSz) / 2);

            if (g.fileBrowser.isDir[i]){
                DrawFolderIcon(gr, iconX, iconY, iconSz, true);
            } else {
                // Get extension
                std::wstring ext;
                size_t dot = g.fileBrowser.entries[i].find_last_of(L'.');
                if (dot != std::wstring::npos) ext = g.fileBrowser.entries[i].substr(dot + 1);
                DrawFileIcon(gr, iconX, iconY, iconSz, ext);
            }

            // File/folder name
            std::wstring displayName = g.fileBrowser.entries[i];
            if (displayName == L"..") displayName = L".. (上级目录)";
            DrawMixedString(gr, displayName.c_str(), -1,
                PointF(R(listX + S(34)), R(iy + itemH / 2 - S(7))), Sf(12.0f), exText);

            AddHit(g_hits, listX, iy, listW, itemH, ID_FILE_ITEM + i);
        }

        gr.ResetClip();

        // Scrollbar (Windows style - thin, appears on right edge)
        int totalH2 = (int)g.fileBrowser.entries.size() * itemH;
        if (totalH2 > listH){
            int sbW = S(8);
            int sbX2 = listX + listW - sbW - S(2);
            float sbRatio = (float)listH / (float)totalH2;
            int sbH2 = std::max(S(30), (int)(listH * sbRatio));
            int sbY2 = listY + (int)(g.fileBrowser.scrollY * sbRatio);
            if (sbY2 + sbH2 > listY + listH) sbY2 = listY + listH - sbH2;
            if (sbY2 < listY) sbY2 = listY;
            FillRoundRect(gr, Color(180, 180, 180, 180), R(sbX2), R(sbY2), R(sbW), R(sbH2), R(S(4)));
        }

        // Empty state
        if (g.fileBrowser.entries.empty()){
            DrawMixedString(gr, L"此文件夹为空", -1,
                RectF(R(listX), R(listY), R(listW), R(listH)), Sf(12.0f), exTextDim, &sfC);
        }
    }

    // ====== 5. Status bar ======
    {
        int stY = panelY + panelH - statusH;
        SolidBrush stBg(exStatusBg);
        gr.FillRectangle(&stBg, R(panelX), R(stY), R(panelW), R(statusH));
        // Top border
        SolidBrush divLine(exDivider);
        gr.FillRectangle(&divLine, R(panelX), R(stY), R(panelW), R(1));

        int totalItems = (int)g.fileBrowser.entries.size();
        int dirs = 0, files = 0;
        for (int i = 0; i < totalItems; ++i){
            if (g.fileBrowser.isDir[i]) dirs++; else files++;
        }
        wchar_t statText[128];
        swprintf(statText, 128, L"%d 个项目（%d 个文件夹，%d 个文件）", totalItems, dirs, files);
        DrawMixedString(gr, statText, -1,
            PointF(R(panelX + S(14)), R(stY + statusH / 2 - S(6))), Sf(9.0f), exTextDim);
    }
}

static void FileBrowserOpenFile(const std::wstring& path){
    g.fileBrowser.active = false;
    g.stableW = 0; // force full redraw to rebuild stable bitmap (clear stale file browser UI)

    size_t dot = path.find_last_of(L'.');
    if (dot == std::wstring::npos){
        ShowToast(L"不支持的文件类型");
        return;
    }
    std::wstring ext = path.substr(dot+1);
    for (auto& c : ext) c = (wchar_t)towlower(c);

    bool isPdf = (ext == L"pdf");
    bool isProject = ProjectIO::IsCloudboardFile(path);
    bool isImage = (ext == L"png" || ext == L"jpg" || ext == L"jpeg" ||
                    ext == L"bmp" || ext == L"gif");

    if (!isPdf && !isImage && !isProject){
        ShowToast(L"不支持的文件类型：" + ext);
        return;
    }

    // Warn if replacing existing content, then open directly in current window
    if (Board::HasAnyInk()){
        ShowToast(L"当前内容将被替换");
    }

    if (isProject){
        std::wstring err;
        if (ProjectIO::LoadCloudboardFromFile(path, &err))
            ShowToast(L"已打开 .cloudboard");
        else
            ShowToast(L"打开失败：" + err);
    } else if (isPdf){
        Board::LoadPdf(path);
    } else {
        g.currentProjectPath.clear();
        RECT br2 = BoardRect(g.hWnd);
        Board::AddImageFromFile(path,
            (br2.right-br2.left)*0.5f + g.scrollX,
            (br2.bottom-br2.top)*0.5f + g.scrollY);
        g.tool = ToolKind::Select;
    }
}

static void OpenFileWithWindowsDialog(HWND hWnd){
    ReleaseCapture();

    HRESULT coHr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    bool didCoInit = SUCCEEDED(coHr);

    IFileOpenDialog* dialog = nullptr;
    HRESULT hr = CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&dialog));
    if (SUCCEEDED(hr) && dialog){
        COMDLG_FILTERSPEC filters[] = {
            { L"CloudBoard/PDF/image files", L"*.cloudboard;*.pdf;*.png;*.jpg;*.jpeg;*.bmp;*.gif" },
            { L"CloudBoard projects", L"*.cloudboard" },
            { L"PDF files", L"*.pdf" },
            { L"Image files", L"*.png;*.jpg;*.jpeg;*.bmp;*.gif" },
            { L"All files", L"*.*" }
        };
        dialog->SetFileTypes((UINT)(sizeof(filters) / sizeof(filters[0])), filters);
        dialog->SetFileTypeIndex(1);

        DWORD options = 0;
        if (SUCCEEDED(dialog->GetOptions(&options))){
            dialog->SetOptions(options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST |
                               FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR);
        }

        hr = dialog->Show(hWnd);
        if (SUCCEEDED(hr)){
            IShellItem* item = nullptr;
            if (SUCCEEDED(dialog->GetResult(&item)) && item){
                PWSTR path = nullptr;
                if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &path)) && path){
                    FileBrowserOpenFile(path);
                    CoTaskMemFree(path);
                    InvalidateRect(hWnd, nullptr, FALSE);
                }
                item->Release();
            }
        }
        dialog->Release();
        if (didCoInit) CoUninitialize();
        return;
    }

    if (dialog) dialog->Release();
    if (didCoInit) CoUninitialize();

    wchar_t fileName[MAX_PATH] = {};
    OPENFILENAMEW ofn{};
    ofn.lStructSize = sizeof(ofn);
    ofn.hwndOwner = hWnd;
    ofn.lpstrFile = fileName;
    ofn.nMaxFile = MAX_PATH;
    ofn.lpstrFilter =
        L"CloudBoard / PDF / 图片文件\0*.cloudboard;*.pdf;*.png;*.jpg;*.jpeg;*.bmp;*.gif\0"
        L"CloudBoard 文件 (*.cloudboard)\0*.cloudboard\0"
        L"PDF 文件 (*.pdf)\0*.pdf\0"
        L"图片文件 (*.png;*.jpg;*.jpeg;*.bmp;*.gif)\0*.png;*.jpg;*.jpeg;*.bmp;*.gif\0"
        L"所有文件\0*.*\0";
    ofn.nFilterIndex = 1;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_EXPLORER;

    if (GetOpenFileNameW(&ofn)){
        FileBrowserOpenFile(fileName);
        InvalidateRect(hWnd, nullptr, FALSE);
    }
}

// ====== Floating geometry tools ======
static float Dist2(PointF2 a, PointF2 b){
    float dx = a.x - b.x, dy = a.y - b.y;
    return dx*dx + dy*dy;
}

static float Dist(PointF2 a, PointF2 b){ return std::sqrt(Dist2(a, b)); }
static PointF2 AddPt(PointF2 a, PointF2 b){ return { a.x + b.x, a.y + b.y }; }
static PointF2 SubPt(PointF2 a, PointF2 b){ return { a.x - b.x, a.y - b.y }; }
static PointF2 MulPt(PointF2 a, float k){ return { a.x * k, a.y * k }; }

static PointF2 RotatePt(PointF2 p, float a){
    float c = std::cos(a), s = std::sin(a);
    return { p.x * c - p.y * s, p.x * s + p.y * c };
}

static PointF2 AssistToScreen(PointF2 p, HWND hWnd){
    RECT br = BoardRect(hWnd);
    return Board::BoardToScreen(p.x, p.y, br);
}

static std::vector<PointF2> TriangleLocalPoints(){
    if (g.assist.triangleKind == TriangleKind::ThirtySixty)
        return { {-160, 96}, {160, 96}, {-160, -88.8f} };
    return { {-150, 100}, {150, 100}, {-150, -200} };
}

static std::vector<PointF2> TriangleInnerLocalPoints(){
    if (g.assist.triangleKind == TriangleKind::ThirtySixty)
        return { {-96, 62}, {66, 62}, {-96, -31.5f} };
    return { {-88, 58}, {52, 58}, {-88, -82} };
}

static PointF2 TriangleToBoard(PointF2 local){
    return AddPt(g.assist.trianglePos,
        RotatePt(MulPt(local, g.assist.triangleScale), g.assist.triangleAngle));
}

static std::vector<PointF2> TriangleBoardPoints(){
    auto local = TriangleLocalPoints();
    std::vector<PointF2> out;
    out.reserve(local.size());
    for (auto p : local) out.push_back(TriangleToBoard(p));
    return out;
}

static PointF2 ProjectPointToSegment(PointF2 p, PointF2 a, PointF2 b){
    PointF2 ab = SubPt(b, a);
    float len2 = ab.x*ab.x + ab.y*ab.y;
    if (len2 < 0.001f) return a;
    float t = ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2;
    t = std::max(0.0f, std::min(1.0f, t));
    return AddPt(a, MulPt(ab, t));
}

static bool PointInTriangle(PointF2 p, const std::vector<PointF2>& v){
    auto sign = [](PointF2 p1, PointF2 p2, PointF2 p3){
        return (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y);
    };
    float d1 = sign(p, v[0], v[1]);
    float d2 = sign(p, v[1], v[2]);
    float d3 = sign(p, v[2], v[0]);
    bool hasNeg = (d1 < 0) || (d2 < 0) || (d3 < 0);
    bool hasPos = (d1 > 0) || (d2 > 0) || (d3 > 0);
    return !(hasNeg && hasPos);
}

static int NearestTriangleEdge(PointF2 p, PointF2* projected, float hitDist){
    auto v = TriangleBoardPoints();
    int best = -1;
    float bestD = hitDist;
    for (int i = 0; i < 3; ++i){
        PointF2 q = ProjectPointToSegment(p, v[i], v[(i + 1) % 3]);
        float d = Dist(p, q);
        if (d < bestD){
            bestD = d;
            best = i;
            if (projected) *projected = q;
        }
    }
    return best;
}

static PointF2 CompassPenPoint(){
    return {
        g.assist.compassCenter.x + std::cos(g.assist.compassAngle) * g.assist.compassRadius,
        g.assist.compassCenter.y + std::sin(g.assist.compassAngle) * g.assist.compassRadius
    };
}

static void BuildCompassArc(float fromA, float sweep){
    g.current.pts.clear();
    int steps = std::max(6, (int)(std::fabs(sweep) * g.assist.compassRadius / 7.0f));
    for (int i = 0; i <= steps; ++i){
        float t = (float)i / (float)steps;
        float a = fromA + sweep * t;
        g.current.pts.push_back({
            g.assist.compassCenter.x + std::cos(a) * g.assist.compassRadius,
            g.assist.compassCenter.y + std::sin(a) * g.assist.compassRadius
        });
    }
}

static float NormalizeAngleDelta(float delta){
    while (delta > 3.14159265f) delta -= 6.2831853f;
    while (delta < -3.14159265f) delta += 6.2831853f;
    return delta;
}

static void DrawAssistTools(Graphics& gr, HWND hWnd){
    if (!kAssistToolsEnabled) return;
    if (g.assist.kind == AssistKind::None) return;
    gr.SetSmoothingMode(SmoothingModeAntiAlias);

    if (g.assist.kind == AssistKind::Triangle){
        auto bv = TriangleBoardPoints();
        PointF pts[3];
        for (int i = 0; i < 3; ++i){
            PointF2 sp = AssistToScreen(bv[i], hWnd);
            pts[i] = PointF(sp.x, sp.y);
        }
        auto innerLocal = TriangleInnerLocalPoints();
        PointF inner[3];
        for (int i = 0; i < 3; ++i){
            PointF2 sp = AssistToScreen(TriangleToBoard(innerLocal[i]), hWnd);
            inner[i] = PointF(sp.x, sp.y);
        }

        GraphicsPath body(FillModeAlternate);
        body.AddPolygon(pts, 3);
        body.AddPolygon(inner, 3);
        SolidBrush fill(Color(76, 105, 196, 255));
        Pen edge(Color(210, 42, 104, 202), Sf(2.0f));
        edge.SetLineJoin(LineJoinRound);
        gr.FillPath(&fill, &body);
        gr.DrawPolygon(&edge, pts, 3);
        Pen innerPen(Color(150, 255, 255, 255), Sf(1.5f));
        innerPen.SetLineJoin(LineJoinRound);
        gr.DrawPolygon(&innerPen, inner, 3);

        Pen bevel(Color(90, 255, 255, 255), Sf(5.0f));
        bevel.SetLineJoin(LineJoinRound);
        gr.DrawPolygon(&bevel, pts, 3);

        FontFamily ff(L"Anthropic Sans Text");
        Font numFont(&ff, Sf(9.0f), FontStyleRegular, UnitPixel);
        Color markTextColor(185, 255, 255, 255);
        Pen tickPen(Color(170, 255, 255, 255), Sf(1.0f));
        for (int e = 0; e < 3; ++e){
            PointF2 a = bv[e], b = bv[(e + 1) % 3];
            float len = Dist(a, b);
            int ticks = std::max(1, (int)(len / 12.0f));
            PointF2 dir = MulPt(SubPt(b, a), 1.0f / std::max(1.0f, len));
            PointF2 n{ -dir.y, dir.x };
            PointF2 mid = MulPt(AddPt(a, b), 0.5f);
            if (Dist(AddPt(a, MulPt(n, 12.0f)), g.assist.trianglePos) >
                Dist(AddPt(a, MulPt(n, -12.0f)), g.assist.trianglePos))
                n = MulPt(n, -1.0f);
            for (int t = 1; t < ticks; ++t){
                PointF2 p = AddPt(a, MulPt(SubPt(b, a), (float)t / (float)ticks));
                float tl = (t % 10 == 0) ? 13.0f : ((t % 5 == 0) ? 9.0f : 5.0f);
                PointF2 s1 = AssistToScreen(p, hWnd);
                PointF2 s2 = AssistToScreen(AddPt(p, MulPt(n, tl)), hWnd);
                gr.DrawLine(&tickPen, s1.x, s1.y, s2.x, s2.y);
                if (t % 10 == 0){
                    wchar_t nb[8];
                    swprintf(nb, 8, L"%d", t / 2);
                    PointF2 ts = AssistToScreen(AddPt(p, MulPt(n, tl + 9.0f)), hWnd);
                    DrawMixedString(gr, nb, -1,
                        RectF(ts.x - Sf(10), ts.y - Sf(6), Sf(20), Sf(12)),
                        Sf(9.0f), markTextColor, nullptr);
                }
            }
            PointF2 ms = AssistToScreen(AddPt(mid, MulPt(n, 18.0f)), hWnd);
            const wchar_t* label = (e == 0) ? L"cm" : L"";
            if (label[0])
                DrawMixedString(gr, label, -1,
                    RectF(ms.x - Sf(12), ms.y - Sf(7), Sf(24), Sf(14)),
                    Sf(9.0f), markTextColor, nullptr);
        }

        PointF2 corner = TriangleToBoard(TriangleLocalPoints()[0]);
        PointF2 x1 = TriangleToBoard(AddPt(TriangleLocalPoints()[0], { 28, 0 }));
        PointF2 y1 = TriangleToBoard(AddPt(TriangleLocalPoints()[0], { 0, -28 }));
        PointF2 xy = TriangleToBoard(AddPt(TriangleLocalPoints()[0], { 28, -28 }));
        PointF2 sc = AssistToScreen(corner, hWnd);
        PointF2 sx = AssistToScreen(x1, hWnd);
        PointF2 sy = AssistToScreen(y1, hWnd);
        PointF2 sxy = AssistToScreen(xy, hWnd);
        Pen anglePen(Color(180, 255, 255, 255), Sf(1.3f));
        gr.DrawLine(&anglePen, sx.x, sx.y, sxy.x, sxy.y);
        gr.DrawLine(&anglePen, sy.x, sy.y, sxy.x, sxy.y);

        const wchar_t* angles = (g.assist.triangleKind == TriangleKind::ThirtySixty) ? L"30° 60° 90°" : L"45° 45° 90°";
        PointF2 labelPos = AssistToScreen(TriangleToBoard({ -110, 78 }), hWnd);
        Font labelFont(&ff, Sf(12.0f), FontStyleRegular, UnitPixel);
        DrawMixedString(gr, angles, -1,
            RectF(labelPos.x, labelPos.y - Sf(22), Sf(110), Sf(20)),
            Sf(12.0f), markTextColor, nullptr);

        PointF2 handle = AssistToScreen(TriangleToBoard({ 0, -235 }), hWnd);
        SolidBrush hb(Color(210, 255, 255, 255));
        Pen hp(Color(230, 52, 108, 210), Sf(1.5f));
        float hr = Sf(12.0f);
        gr.FillEllipse(&hb, handle.x - hr, handle.y - hr, hr*2, hr*2);
        gr.DrawEllipse(&hp, handle.x - hr, handle.y - hr, hr*2, hr*2);
        return;
    }

    PointF2 c = AssistToScreen(g.assist.compassCenter, hWnd);
    PointF2 p = AssistToScreen(CompassPenPoint(), hWnd);
    float sr = g.assist.compassRadius * g.zoomLevel;
    Pen ghost(Color(80, 80, 180, 255), Sf(1.0f));
    REAL dash[] = { 6, 5 };
    ghost.SetDashPattern(dash, 2);
    gr.DrawEllipse(&ghost, c.x - sr, c.y - sr, sr*2, sr*2);

    PointF2 hinge{ (c.x + p.x) * 0.5f, std::min(c.y, p.y) - Sf(45.0f) };
    Pen shadow(Color(70, 0, 0, 0), Sf(7.0f));
    shadow.SetStartCap(LineCapRound); shadow.SetEndCap(LineCapRound);
    gr.DrawLine(&shadow, hinge.x + Sf(2), hinge.y + Sf(2), c.x + Sf(2), c.y + Sf(2));
    gr.DrawLine(&shadow, hinge.x + Sf(2), hinge.y + Sf(2), p.x + Sf(2), p.y + Sf(2));

    Pen leg(Color(235, 82, 92, 110), Sf(4.0f));
    leg.SetStartCap(LineCapRound); leg.SetEndCap(LineCapRound);
    gr.DrawLine(&leg, hinge.x, hinge.y, c.x, c.y);
    gr.DrawLine(&leg, hinge.x, hinge.y, p.x, p.y);
    Pen shine(Color(140, 255, 255, 255), Sf(1.2f));
    gr.DrawLine(&shine, hinge.x - Sf(2), hinge.y, c.x - Sf(2), c.y);
    gr.DrawLine(&shine, hinge.x - Sf(2), hinge.y, p.x - Sf(2), p.y);

    Pen cross(Color(220, 52, 108, 210), Sf(2.0f));
    gr.DrawLine(&cross, hinge.x - Sf(22), hinge.y + Sf(20), hinge.x + Sf(22), hinge.y + Sf(20));

    PointF needlePts[3] = {
        PointF(c.x, c.y + Sf(16)),
        PointF(c.x - Sf(5), c.y + Sf(4)),
        PointF(c.x + Sf(5), c.y + Sf(4))
    };
    SolidBrush needle(Color(235, 55, 65, 78));
    gr.FillPolygon(&needle, needlePts, 3);

    PointF pencilPts[3] = {
        PointF(p.x, p.y + Sf(17)),
        PointF(p.x - Sf(6), p.y + Sf(4)),
        PointF(p.x + Sf(6), p.y + Sf(4))
    };
    SolidBrush pencil(Color(235, 35, 120, 210));
    gr.FillPolygon(&pencil, pencilPts, 3);

    SolidBrush foot(Color(230, 255, 255, 255));
    Pen footPen(Color(230, 52, 108, 210), Sf(1.5f));
    float rr = Sf(13.0f);
    gr.FillEllipse(&foot, c.x - rr, c.y - rr, rr*2, rr*2);
    gr.DrawEllipse(&footPen, c.x - rr, c.y - rr, rr*2, rr*2);
    gr.FillEllipse(&foot, p.x - rr, p.y - rr, rr*2, rr*2);
    gr.DrawEllipse(&footPen, p.x - rr, p.y - rr, rr*2, rr*2);
    gr.FillEllipse(&foot, hinge.x - Sf(12), hinge.y - Sf(12), Sf(24), Sf(24));
    gr.DrawEllipse(&footPen, hinge.x - Sf(12), hinge.y - Sf(12), Sf(24), Sf(24));
    SolidBrush knob(Color(240, 52, 108, 210));
    gr.FillEllipse(&knob, hinge.x - Sf(5), hinge.y - Sf(5), Sf(10), Sf(10));

    wchar_t buf[48];
    swprintf(buf, 48, L"r %.0f", g.assist.compassRadius);
    FontFamily ff(L"Anthropic Sans Text");
    Font font(&ff, Sf(12.0f), FontStyleRegular, UnitPixel);
    SolidBrush text(Color(230, 40, 60, 90));
    gr.DrawString(buf, -1, &font, PointF(c.x + Sf(16), c.y + Sf(12)), &text);
}

// ====== Main draw ======
void DrawAll(HDC hdc, HWND hWnd){
    UpdateAccentCache();
    RECT cr; GetClientRect(hWnd, &cr);
    int W = cr.right, H = cr.bottom;

    // Ensure double buffer
    bool recreatedMem = false;
    if (g.memW != W || g.memH != H || !g.hMemBmp){
        if (g.hMemBmp) DeleteObject(g.hMemBmp);
        if (g.hMemDC) DeleteDC(g.hMemDC);
        HDC hScr = GetDC(hWnd);
        g.hMemDC = CreateCompatibleDC(hScr);
        g.hMemBmp = CreateCompatibleBitmap(hScr, W, H);
        ReleaseDC(hWnd, hScr);
        g.memW = W; g.memH = H;
        recreatedMem = true;
    }
    HGDIOBJ oldBmp = SelectObject(g.hMemDC, g.hMemBmp);

    if ((g.drawing || g.shaping) && !StableBitmapMatches(W, H) &&
        !recreatedMem && g.hMemBmp && g.memW == W && g.memH == H){
        EnsureStable(W, H);
        HGDIOBJ oldS = SelectObject(g.hStableDC, g.hStableBmp);
        BitBlt(g.hStableDC, 0, 0, W, H, g.hMemDC, 0, 0, SRCCOPY);
        SelectObject(g.hStableDC, oldS);
    }

    bool quickDraw = (g.drawing || g.shaping) && StableBitmapMatches(W, H) &&
                     !g.rubberBand && !g.draggingSelection;

    // Panning fast-path: active only while motion is in progress.
    // Idle frames use the full redraw path so the stable bitmap stays current.
    bool panFast = (g.panning || g.wheelPanning)
        && g.hStableBmp && g.stableW == W && g.stableH == H;

    if (quickDraw){
        // === Fast path: incremental draw ===
        // Copy stable bitmap
        HGDIOBJ oldS = SelectObject(g.hStableDC, g.hStableBmp);
        BitBlt(g.hMemDC, 0, 0, W, H, g.hStableDC, 0, 0, SRCCOPY);
        SelectObject(g.hStableDC, oldS);

        // Draw current stroke / shape preview / eraser on top
        Graphics gr(g.hMemDC);
        gr.SetSmoothingMode(SmoothingModeAntiAlias);
        RECT br2 = BoardRect(hWnd);
        Region clip(RectF(R(br2.left), R(br2.top),
            R(br2.right - br2.left), R(br2.bottom - br2.top)));
        gr.SetClip(&clip);
        Matrix mat;
        mat.Scale(g.zoomLevel, g.zoomLevel);
        mat.Translate(R(br2.left) - g.scrollX, R(br2.top) - g.scrollY, MatrixOrderAppend);
        gr.SetTransform(&mat);
        if (g.drawing && g.current.pts.size() >= 2)
            Board::DrawStroke(gr, g.current);
        if (g.shaping){
            Stroke s;
            s.color = g.color; s.width = g.penSize; s.style = g.lineStyle;
            s.isShape = true; s.shape = g.currentShape;
            s.a = { std::min(g.shapeStart.x, g.shapeEnd.x),
                    std::min(g.shapeStart.y, g.shapeEnd.y) };
            s.b = { std::max(g.shapeStart.x, g.shapeEnd.x),
                    std::max(g.shapeStart.y, g.shapeEnd.y) };
            if (g.currentShape == ShapeKind::Line || g.currentShape == ShapeKind::Arrow){
                s.a = g.shapeStart; s.b = g.shapeEnd;
            }
            Board::DrawStroke(gr, s);
        }
        // Eraser: draw cursor outline only (strokes are already modified in-place)
        if (g.erasing && !g.eraserPath.empty()){
            float erSz = std::max(1.0f, g.eraserSize * 0.5f);
            // Draw eraser cursor outline
            Pen erasePen(Color(120, 100, 100, 100), 1.0f);
            auto& last = g.eraserPath.back();
            gr.DrawEllipse(&erasePen, last.x - erSz, last.y - erSz, erSz*2, erSz*2);
        }
        gr.ResetTransform();
        gr.ResetClip();

        // Draw popups and hints on top
        gr.SetTextRenderingHint(TextRenderingHintClearTypeGridFit);
        g_hits.clear();
        g_popupHits.clear();
        DrawBottomBar(gr, hWnd);
        DrawAssistTools(gr, hWnd);
        DrawPenPopup(gr, hWnd);
        DrawEraserPopup(gr, hWnd);
        DrawZoomPopup(gr, hWnd);
        DrawAssistPopup(gr, hWnd);
        DrawSelPopup(gr, hWnd);
        DrawHint(gr, hWnd);
        // (sidebar removed)
        DrawSettingsDialog(gr, hWnd);
        DrawConfirmDialog(gr, hWnd);
        DrawFileBrowser(gr, hWnd);
        DrawLoadingOverlay(gr, hWnd);
    } else if (panFast && g.pdfMode) {
        // === PDF scroll fast path: shift the last stable frame, prefetch async ===
        RECT br2 = BoardRect(hWnd);
        int bw = br2.right - br2.left, bh = br2.bottom - br2.top;
        Board::PrefetchVisiblePdfPages();

        {
            Graphics gr(g.hMemDC);
            SolidBrush bgAll(g.isDarkMode ? Color(255, 24, 28, 40) : Color(255, 235, 238, 242));
            gr.FillRectangle(&bgAll, 0, 0, W, H);
        }

        HGDIOBJ oldS = SelectObject(g.hStableDC, g.hStableBmp);
        HBRUSH grayBrush = CreateSolidBrush(RGB(128, 128, 128));
        FillRect(g.hMemDC, &br2, grayBrush);
        DeleteObject(grayBrush);

        int dx = (int)std::round(g.prevScrollX - g.scrollX);
        int dy = (int)std::round(g.prevScrollY - g.scrollY);
        int copyW = bw - std::abs(dx);
        int copyH = bh - std::abs(dy);
        if (copyW > 0 && copyH > 0){
            int dstX = br2.left + std::max(dx, 0);
            int dstY = br2.top + std::max(dy, 0);
            int srcX = br2.left + std::max(-dx, 0);
            int srcY = br2.top + std::max(-dy, 0);
            BitBlt(g.hMemDC, dstX, dstY, copyW, copyH, g.hStableDC, srcX, srcY, SRCCOPY);
        }
        SelectObject(g.hStableDC, oldS);

        std::vector<RECT> exposed;
        if (std::abs(dx) >= bw || std::abs(dy) >= bh){
            exposed.push_back(br2);
        } else {
            if (dy > 0) exposed.push_back({ br2.left, br2.top, br2.right, br2.top + dy });
            if (dy < 0) exposed.push_back({ br2.left, br2.bottom + dy, br2.right, br2.bottom });
            if (dx > 0) exposed.push_back({ br2.left, br2.top, br2.left + dx, br2.bottom });
            if (dx < 0) exposed.push_back({ br2.right + dx, br2.top, br2.right, br2.bottom });
        }

        if (!exposed.empty()){
            Graphics gr(g.hMemDC);
            gr.SetSmoothingMode(SmoothingModeNone);
            gr.SetInterpolationMode(InterpolationModeNearestNeighbor);
            Matrix mat;
            mat.Scale(g.zoomLevel, g.zoomLevel);
            mat.Translate(R(br2.left) - g.scrollX, R(br2.top) - g.scrollY, MatrixOrderAppend);

            for (const RECT& er : exposed){
                RECT clipped{
                    std::max(er.left, br2.left),
                    std::max(er.top, br2.top),
                    std::min(er.right, br2.right),
                    std::min(er.bottom, br2.bottom)
                };
                if (clipped.left >= clipped.right || clipped.top >= clipped.bottom) continue;
                Region clip(RectF(R(clipped.left), R(clipped.top),
                    R(clipped.right - clipped.left), R(clipped.bottom - clipped.top)));
                gr.SetClip(&clip, CombineModeReplace);
                gr.SetTransform(&mat);
                Board::DrawAll(gr, br2);
                gr.ResetTransform();
            }
            gr.ResetClip();
        }

        oldS = SelectObject(g.hStableDC, g.hStableBmp);
        BitBlt(g.hStableDC, 0, 0, W, H, g.hMemDC, 0, 0, SRCCOPY);
        SelectObject(g.hStableDC, oldS);
        g.prevScrollX = g.scrollX;
        g.prevScrollY = g.scrollY;

        // Minimal UI
        {
            Graphics gr(g.hMemDC);
            gr.SetTextRenderingHint(TextRenderingHintClearTypeGridFit);
            g_hits.clear();
            g_popupHits.clear();
            DrawBottomBar(gr, hWnd);
            DrawLoadingOverlay(gr, hWnd);
        }
    } else if (panFast) {
        // === Panning fast path: redraw board, then draw clean UI once ===
        {
            Graphics gr(g.hMemDC);
            SolidBrush bgAll(g.isDarkMode ? Color(255, 24, 28, 40) : Color(255, 235, 238, 242));
            gr.FillRectangle(&bgAll, 0, 0, W, H);
        }

        RECT br2 = BoardRect(hWnd);
        int bw = br2.right - br2.left, bh = br2.bottom - br2.top;

        // Board rendering - try OpenGL first, fall back to GDI+
        bool glOk = false;
        if (g.glInitialized && bw > 0 && bh > 0){
            Board::BoardDrawGL(br2);
            if (!glc.readBuf.empty()){
                Graphics gr(g.hMemDC);
                Gdiplus::Bitmap glBmp(bw, bh, bw * 4, PixelFormat32bppARGB, glc.readBuf.data());
                if (glBmp.GetLastStatus() == Gdiplus::Ok){
                    gr.DrawImage(&glBmp, br2.left, br2.top, bw, bh);
                    glOk = true;
                }
            }
        }

        if (!glOk){
            // GDI+ fallback
            Graphics gr(g.hMemDC);
            Region clip(RectF(R(br2.left), R(br2.top), R(bw), R(bh)));
            gr.SetClip(&clip);
            Matrix mat;
            mat.Scale(g.zoomLevel, g.zoomLevel);
            mat.Translate(R(br2.left) - g.scrollX, R(br2.top) - g.scrollY, MatrixOrderAppend);
            gr.SetTransform(&mat);
            Board::DrawAll(gr, br2);
            gr.ResetTransform();
            gr.ResetClip();
        }

        // Minimal UI
        {
            Graphics gr(g.hMemDC);
            gr.SetTextRenderingHint(TextRenderingHintClearTypeGridFit);
            g_hits.clear();
            g_popupHits.clear();
            DrawBottomBar(gr, hWnd);
            DrawLoadingOverlay(gr, hWnd);
        }
    } else {
        // === Full redraw path ===
        Graphics gr(g.hMemDC);
        gr.SetSmoothingMode(SmoothingModeAntiAlias);
        gr.SetTextRenderingHint(TextRenderingHintClearTypeGridFit);

        // Overall background
        SolidBrush bgAll(g.isDarkMode ? Color(255, 24, 28, 40) : Color(255, 235, 238, 242));
        gr.FillRectangle(&bgAll, 0, 0, W, H);

        g_hits.clear();
        g_popupHits.clear();

        DrawBottomBar(gr, hWnd);

        // Board area with OpenGL (falls back to GDI+ if GL fails)
        RECT br2 = BoardRect(hWnd);
        int bw = br2.right - br2.left, bh = br2.bottom - br2.top;
        bool glOk = false;

        if (g.glInitialized && bw > 0 && bh > 0){
            Board::BoardDrawGL(br2);
            if (!glc.readBuf.empty()){
                Gdiplus::Bitmap glBmp(bw, bh, bw * 4, PixelFormat32bppARGB, glc.readBuf.data());
                if (glBmp.GetLastStatus() == Gdiplus::Ok){
                    gr.DrawImage(&glBmp, br2.left, br2.top, bw, bh);
                    glOk = true;
                }
            }
        }

        if (!glOk){
            // Fallback: render board with GDI+
            Region clip(RectF(R(br2.left), R(br2.top), R(bw), R(bh)));
            gr.SetClip(&clip);
            Matrix mat;
            mat.Scale(g.zoomLevel, g.zoomLevel);
            mat.Translate(R(br2.left) - g.scrollX, R(br2.top) - g.scrollY, MatrixOrderAppend);
            gr.SetTransform(&mat);
            Board::DrawAll(gr, br2);
            gr.ResetTransform();
            gr.ResetClip();
        } else {
            // Draw board text with GDI+ (CJK/Latin mixed text)
            Region clip(RectF(R(br2.left), R(br2.top), R(bw), R(bh)));
            gr.SetClip(&clip);
            Matrix mat;
            mat.Scale(g.zoomLevel, g.zoomLevel);
            mat.Translate(R(br2.left) - g.scrollX, R(br2.top) - g.scrollY, MatrixOrderAppend);
            gr.SetTransform(&mat);
            Board::DrawTextsGDI(gr, br2);
            gr.ResetTransform();
            gr.ResetClip();
        }

        // Popups
        DrawAssistTools(gr, hWnd);
        DrawPenPopup(gr, hWnd);
        DrawEraserPopup(gr, hWnd);
        DrawZoomPopup(gr, hWnd);
        DrawAssistPopup(gr, hWnd);
        DrawSelPopup(gr, hWnd);
        DrawHint(gr, hWnd);
        // (sidebar removed)
        DrawSettingsDialog(gr, hWnd);
        DrawConfirmDialog(gr, hWnd);
        DrawFileBrowser(gr, hWnd);
        DrawLoadingOverlay(gr, hWnd);

        // Update stable bitmap when not actively drawing
        if (!g.drawing && !g.shaping && !g.erasing){
            EnsureStable(W, H);
            HGDIOBJ oldS = SelectObject(g.hStableDC, g.hStableBmp);
            BitBlt(g.hStableDC, 0, 0, W, H, g.hMemDC, 0, 0, SRCCOPY);
            SelectObject(g.hStableDC, oldS);
            g.prevScrollX = g.scrollX; // sync scroll-blit baseline
            g.prevScrollY = g.scrollY;
        }
    }

    BitBlt(hdc, 0, 0, W, H, g.hMemDC, 0, 0, SRCCOPY);
    SelectObject(g.hMemDC, oldBmp);
}

// ====== hit testing ======
static int HitGeneric(int x, int y){
    for (auto& h : g_hits)
        if (PtInRect2(h.rc, x, y)) return h.id;
    return 0;
}

bool HitPopup(int x, int y){
    for (auto& h : g_popupHits){
        if (PtInRect2(h.rc, x, y)){
            int id2 = h.id;
            if (id2>=1000 && id2<1100){
                int idx = id2-1000;
                if (idx>=0 && idx<kColorCount){
                    g.color = Color(255, kColorPalette[idx].GetR(),
                                        kColorPalette[idx].GetG(),
                                        kColorPalette[idx].GetB());
                }
                return true;
            }
            if (id2>=1100 && id2<1200){
                int idx = id2-1100;
                if (idx>=0 && idx<kSizeCount) g.penSize = kSizeOptions[idx];
                return true;
            }
            if (id2>=1200 && id2<1300){
                g.lineStyle = (LineStyle)(id2-1200);
                return true;
            }
            if (id2>=2000 && id2<2100){
                // Eraser slider - calculate new size from click X position
                g.eraserDragging = true;
                RECT br = BottomBarRect(g.hWnd);
                int popW = S(330);
                int popX = br.left + (br.right - br.left - popW) / 2;
                int sliderLeft = popX + S(14) + S(36);
                int sliderRight = popX + popW - S(90);
                int sliderW = sliderRight - sliderLeft;
                if (sliderW > 0){
                    float t2 = (float)(x - sliderLeft) / (float)sliderW;
                    if (t2 < 0) t2 = 0; if (t2 > 1) t2 = 1;
                    g.eraserSize = AppState::kEraserMin + t2 * (AppState::kEraserMax - AppState::kEraserMin);
                }
                return true;
            }
            if (id2==ID_ERASER_CLEAR){
                Board::ClearCurrent();
                InvalidateRect(g.hWnd, nullptr, FALSE);
                UpdateWindow(g.hWnd);
                ShowToast(L"已清屏（可撤销）");
                return true;
            }
            // Zoom presets (50% to 350%); internally 100% maps to 2.0x.
            if (id2 >= ID_ZOOM_50 && id2 <= ID_ZOOM_350){
                static const float zoomVals[] = {
                    0.5f * kZoomDisplayBase,
                    0.75f * kZoomDisplayBase,
                    1.0f * kZoomDisplayBase,
                    1.5f * kZoomDisplayBase,
                    2.0f * kZoomDisplayBase,
                    3.0f * kZoomDisplayBase,
                    3.5f * kZoomDisplayBase
                };
                int idx = id2 - ID_ZOOM_50;
                if (idx >= 0 && idx < (int)(sizeof(zoomVals) / sizeof(zoomVals[0]))){
                    // Zoom centered on board center
                    RECT br2 = BoardRect(g.hWnd);
                    float boardCX = g.scrollX + (br2.right - br2.left) * 0.5f;
                    float boardCY = g.scrollY + (br2.bottom - br2.top) * 0.5f;
                    float newZoom = zoomVals[idx];
                    float oldZoom = g.zoomLevel;
                    g.zoomLevel = newZoom;
                    // Adjust scroll to keep center point
                    g.scrollX = boardCX * (newZoom / oldZoom) - (br2.right - br2.left) * 0.5f;
                    g.scrollY = boardCY * (newZoom / oldZoom) - (br2.bottom - br2.top) * 0.5f;
                    if (g.pdfMode) Board::ClampPdfScroll();
                    g.stableW = 0; // force stable bitmap rebuild
                    ShowToast(std::to_wstring((int)(newZoom / kZoomDisplayBase * 100 + 0.5f)) + L"%");
                }
                return true;
            }
            // Selection popup: delete / copy / color change
            if (id2 == ID_SEL_DELETE || id2 == ID_SEL_COPY ||
                (id2 >= ID_SEL_COLOR && id2 < ID_SEL_COLOR + kColorCount)){
                OnCommand(g.hWnd, id2);
                return true;
            }
            if (kAssistToolsEnabled &&
                (id2 == ID_ASSIST_TRIANGLE45 || id2 == ID_ASSIST_TRIANGLE30 ||
                id2 == ID_ASSIST_COMPASS || id2 == ID_ASSIST_HIDE)){
                OnCommand(g.hWnd, id2);
                return true;
            }
            return true;
        }
    }
    return false;
}

void InvalidateThumb(int pageIdx){
    if (pageIdx>=0 && pageIdx<(int)g.thumbDirty.size())
        g.thumbDirty[pageIdx] = true;
}

static void RequestInkRepaint(HWND hWnd, bool force = false){
    DWORD now = GetTickCount();
    DWORD elapsed = now - g.lastInkRepaintTick;
    if (force || elapsed >= 8){
        KillTimer(hWnd, TIMER_INK_REPAINT);
        g.inkRepaintPending = false;
        g.lastInkRepaintTick = now;
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    }
    if (!g.inkRepaintPending){
        g.inkRepaintPending = true;
        SetTimer(hWnd, TIMER_INK_REPAINT, std::max<DWORD>(1, 8 - elapsed), nullptr);
    }
}

// ====== mouse events ======
bool HandleMouseDown(HWND hWnd, int x, int y){
    // Confirm dialog: intercept all input
    if (g.confirm.active){
        int id2 = HitGeneric(x, y);
        if (id2 == ID_CONFIRM_YES){
            g.confirm.active = false;
            OnCommand(hWnd, g.confirm.confirmCmdId);
        } else if (id2 == ID_CONFIRM_NO){
            g.confirm.active = false;
        } else if (id2 == ID_CONFIRM_NO + 1 && !g.confirm.altLabel.empty()){
            g.confirm.active = false;
            OnCommand(hWnd, g.confirm.altCmdId);
        } else if (id2 == ID_CONFIRM_NO + 2 && !g.confirm.thirdLabel.empty()){
            g.confirm.active = false;
            OnCommand(hWnd, g.confirm.thirdCmdId);
        }
        g.stableW = 0; // clear stale confirm dialog from stable bitmap
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    // Settings dialog: intercept all input
    if (g.settingsOpen){
        int id2 = HitGeneric(x, y);
        if (id2 == ID_ABOUT){
            g.settingsOpen = false;
            g.stableW = 0; // clear stale settings UI from stable bitmap
        }
        OnCommand(hWnd, id2);
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    // File browser: intercept all input
    if (g.fileBrowser.active){
        g.fileBrowser.downX = x;
        g.fileBrowser.downY = y;
        g.fileBrowser.captureDown = true;
        g.fileBrowser.dragging = false;
        // Record last Y for potential drag
        g.fileBrowser.dragLastY = y;
        // Defer ALL hit processing to HandleMouseUp to avoid double-action
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    // Loading overlay: absorb all clicks while active
    if (g.loadingActive){
        return true;
    }

    if (g.penPopupOpen || g.eraserPopupOpen || g.zoomPopupOpen || g.assistPopupOpen || g.selPopupOpen){
        if (HitPopup(x, y)){
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        g.penPopupOpen = false;
        g.eraserPopupOpen = false;
        g.eraserDragging = false;
        g.zoomPopupOpen = false;
        g.assistPopupOpen = false;
        g.selPopupOpen = false;
        // Force full redraw so stable bitmap drops the popup ghost
        g.stableW = 0;
        InvalidateRect(hWnd, nullptr, FALSE);
    }

    int id2 = HitGeneric(x, y);
    if (id2 != 0){
        if (id2 == ID_ZOOM_TOGGLE){
            g.zoomPopupOpen = !g.zoomPopupOpen;
            g.penPopupOpen = false;
            g.eraserPopupOpen = false;
            g.assistPopupOpen = false;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        if (id2 == ID_TOOL_PEN){
            g.tool = ToolKind::Pen;
            g.penUndoGroupActive = false;
            g.eraserPopupOpen = false;
            g.zoomPopupOpen = false;
            g.assistPopupOpen = false;
        g.selPopupOpen = false;
            g.penPopupOpen = !g.penPopupOpen;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        if (id2 == ID_TOOL_ERASER){
            g.tool = ToolKind::Eraser;
            g.penUndoGroupActive = false;
            g.penPopupOpen = false;
            g.zoomPopupOpen = false;
            g.assistPopupOpen = false;
        g.selPopupOpen = false;
            g.eraserPopupOpen = !g.eraserPopupOpen;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        OnCommand(hWnd, id2);
        return true;
    }

    RECT br2 = BoardRect(hWnd);
    if (!PtInRect2(br2, x, y)) return false;
    PointF2 bp = Board::ScreenToBoard(x, y, br2);

    if (g.pdfMode && g.wheelPanning){
        g.wheelPanning = false;
        KillTimer(hWnd, TIMER_SCROLL_IDLE);
        g.stableW = 0;
    }

    if (g.hEdit) Board::CommitTextEdit();

    g.penPopupOpen = false;
    g.eraserPopupOpen = false;
    g.zoomPopupOpen = false;
    g.assistPopupOpen = false;

    // PDF mode: update current page based on click position
    if (g.pdfMode && !g.pdfPageYOffset.empty()){
        int pageIdx = Board::PdfPageAtY(bp.y);
        if (pageIdx != g.currentPage){
            g.selectedItems.clear();
            g.currentPage = pageIdx;
        }
    }

    if (kAssistToolsEnabled && g.assist.kind == AssistKind::Triangle){
        PointF2 projected{};
        float hitDist = Sf(24.0f) / std::max(0.25f, g.zoomLevel);
        int edge = NearestTriangleEdge(bp, &projected, hitDist);
        PointF2 rotHandle = TriangleToBoard({ 0, -235 });
        if (g.tool == ToolKind::Pen && edge >= 0){
            g.assist.dragMode = AssistDragMode::DrawTriangleLine;
            g.assist.dragEdge = edge;
            Board::BeginStroke(projected.x, projected.y);
            g.current.pts.clear();
            g.current.pts.push_back(projected);
            g.current.pts.push_back(projected);
            RequestInkRepaint(hWnd, true);
            return true;
        }
        if (Dist(bp, rotHandle) <= hitDist * 1.3f){
            g.assist.dragMode = AssistDragMode::RotateTriangle;
            g.assist.dragStartAngle = std::atan2(bp.y - g.assist.trianglePos.y, bp.x - g.assist.trianglePos.x) - g.assist.triangleAngle;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        auto tri = TriangleBoardPoints();
        if (PointInTriangle(bp, tri)){
            g.assist.dragMode = AssistDragMode::MoveTriangle;
            g.assist.dragLastBoard = bp;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
    } else if (kAssistToolsEnabled && g.assist.kind == AssistKind::Compass){
        float hitDist = Sf(28.0f) / std::max(0.25f, g.zoomLevel);
        PointF2 penPt = CompassPenPoint();
        if (g.tool == ToolKind::Pen && Dist(bp, penPt) <= hitDist * 1.4f){
            g.assist.dragMode = AssistDragMode::DrawCompassArc;
            g.assist.dragStartAngle = g.assist.compassAngle;
            g.assist.compassDrawLastAngle = g.assist.compassAngle;
            g.assist.compassDrawSweep = 0.0f;
            Board::BeginStroke(penPt.x, penPt.y);
            BuildCompassArc(g.assist.dragStartAngle, g.assist.compassDrawSweep);
            RequestInkRepaint(hWnd, true);
            return true;
        }
        if (Dist(bp, penPt) <= hitDist * 1.4f){
            g.assist.dragMode = AssistDragMode::AdjustCompassRadius;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        if (Dist(bp, g.assist.compassCenter) <= hitDist * 1.4f){
            g.assist.dragMode = AssistDragMode::MoveCompass;
            g.assist.dragLastBoard = bp;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
    }

    if (g.tool == ToolKind::Pan){
        g.panning = true;
        g.panLastX = x;
        g.panLastY = y;
        return true;
    }
    if (g.tool == ToolKind::Eraser){
        Board::BeginErase(bp.x, bp.y);
        g.current.pts.push_back(bp);
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }
    if (g.tool == ToolKind::Select){
        // Check if clicking on an already-selected item -> start drag
        if (!g.selectedItems.empty() && Board::HitSelected(bp.x, bp.y)){
            // Push undo snapshot before drag
            auto& pg = g.pages[g.currentPage];
            pg.undoStack.push_back({pg.strokes, pg.texts, pg.images});
            pg.redoStack.clear();
            g.draggingSelection = true;
            g.dragStart = bp;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        // Check single-item click (text/image)
        int ti = Board::HitTextAt(bp.x, bp.y);
        int ii = Board::HitImageAt(bp.x, bp.y);
        if (ti >= 0){
            g.selectedItems.clear();
            g.selectedItems.push_back({SelectedItem::Text, ti, g.currentPage});
            g.draggingSelection = true;
            g.dragStart = bp;
            auto& pg = g.pages[g.currentPage];
            pg.undoStack.push_back({pg.strokes, pg.texts, pg.images});
            pg.redoStack.clear();
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        if (ii >= 0){
            g.selectedItems.clear();
            g.selectedItems.push_back({SelectedItem::Image, ii, g.currentPage});
            g.draggingSelection = true;
            g.dragStart = bp;
            auto& pg = g.pages[g.currentPage];
            pg.undoStack.push_back({pg.strokes, pg.texts, pg.images});
            pg.redoStack.clear();
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        // Nothing hit -> start rubber band selection
        g.selectedItems.clear();
        g.rubberBand = true;
        g.rubberStart = bp;
        g.rubberEnd = bp;
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    Board::BeginStroke(bp.x, bp.y);
    return true;
}

bool HandleMouseMove(HWND hWnd, int x, int y, bool buttonDown){
    if (!buttonDown){
        if (g.drawing){
            Board::EndStroke();
            RequestInkRepaint(hWnd, true);
            return true;
        }
        return false;
    }

    // Eraser slider drag
    if (g.eraserDragging && g.eraserPopupOpen){
        RECT br = BottomBarRect(hWnd);
        int popW = S(330);
        int popX = br.left + (br.right - br.left - popW) / 2;
        int sliderLeft = popX + S(14) + S(36);
        int sliderRight = popX + popW - S(90);
        int sliderW = sliderRight - sliderLeft;
        if (sliderW > 0){
            float t2 = (float)(x - sliderLeft) / (float)sliderW;
            if (t2 < 0) t2 = 0; if (t2 > 1) t2 = 1;
            g.eraserSize = AppState::kEraserMin + t2 * (AppState::kEraserMax - AppState::kEraserMin);
        }
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    // File browser scroll drag
    if (g.fileBrowser.active){
        int dy = y - g.fileBrowser.dragLastY;
        if (std::abs(y - g.fileBrowser.downY) > S(6)){
            g.fileBrowser.dragging = true;
        }
        if (g.fileBrowser.dragging){
            g.fileBrowser.scrollY -= dy;
            // Clamp scroll
            int itemH = S(36);
            int totalH = (int)g.fileBrowser.entries.size() * itemH;
            RECT tmpBr = BoardRect(hWnd);
            int boardW = tmpBr.right - tmpBr.left;
            int boardH = tmpBr.bottom - tmpBr.top;
            int panelH = std::min(S(560), boardH - S(20));
            int listH = panelH - S(36) - S(38) - S(26);
            int maxScroll = std::max(0, totalH - listH);
            if (g.fileBrowser.scrollY < 0) g.fileBrowser.scrollY = 0;
            if (g.fileBrowser.scrollY > maxScroll) g.fileBrowser.scrollY = maxScroll;
            g.fileBrowser.dragLastY = y;
            InvalidateRect(hWnd, nullptr, FALSE);
        }
        return true;
    }

    if (g.panning){
        float fdx = (float)(x - g.panLastX);
        float fdy = (float)(y - g.panLastY);
        g.scrollX -= fdx;
        g.scrollY -= fdy;
        if (g.pdfMode) Board::ClampPdfScroll();
        g.panLastX = x;
        g.panLastY = y;

        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    RECT br2 = BoardRect(hWnd);
    if (!PtInRect2(br2, x, y)) return false;
    PointF2 bp = Board::ScreenToBoard(x, y, br2);

    if (g.assist.dragMode != AssistDragMode::None){
        switch (g.assist.dragMode){
        case AssistDragMode::MoveTriangle: {
            if (!kAssistToolsEnabled) break;
            PointF2 d = SubPt(bp, g.assist.dragLastBoard);
            g.assist.trianglePos = AddPt(g.assist.trianglePos, d);
            g.assist.dragLastBoard = bp;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        case AssistDragMode::RotateTriangle:
            if (!kAssistToolsEnabled) break;
            g.assist.triangleAngle = std::atan2(bp.y - g.assist.trianglePos.y, bp.x - g.assist.trianglePos.x) - g.assist.dragStartAngle;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        case AssistDragMode::DrawTriangleLine: {
            if (!kAssistToolsEnabled) break;
            auto tri = TriangleBoardPoints();
            int edge = std::max(0, std::min(2, g.assist.dragEdge));
            PointF2 q = ProjectPointToSegment(bp, tri[edge], tri[(edge + 1) % 3]);
            if (g.drawing && g.current.pts.size() >= 2){
                g.current.pts.back() = q;
                RequestInkRepaint(hWnd);
            }
            return true;
        }
        case AssistDragMode::MoveCompass: {
            if (!kAssistToolsEnabled) break;
            PointF2 d = SubPt(bp, g.assist.dragLastBoard);
            g.assist.compassCenter = AddPt(g.assist.compassCenter, d);
            g.assist.dragLastBoard = bp;
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        case AssistDragMode::AdjustCompassRadius: {
            if (!kAssistToolsEnabled) break;
            PointF2 d = SubPt(bp, g.assist.compassCenter);
            g.assist.compassRadius = std::max(20.0f, std::sqrt(d.x*d.x + d.y*d.y));
            g.assist.compassAngle = std::atan2(d.y, d.x);
            InvalidateRect(hWnd, nullptr, FALSE);
            return true;
        }
        case AssistDragMode::DrawCompassArc: {
            if (!kAssistToolsEnabled) break;
            PointF2 d = SubPt(bp, g.assist.compassCenter);
            if (d.x*d.x + d.y*d.y > 1.0f){
                float nextAngle = std::atan2(d.y, d.x);
                g.assist.compassDrawSweep += NormalizeAngleDelta(nextAngle - g.assist.compassDrawLastAngle);
                g.assist.compassDrawLastAngle = nextAngle;
                g.assist.compassAngle = nextAngle;
                BuildCompassArc(g.assist.dragStartAngle, g.assist.compassDrawSweep);
                RequestInkRepaint(hWnd);
            }
            return true;
        }
        default:
            break;
        }
    }

    if (g.erasing){
        Board::MoveErase(bp.x, bp.y);
        if (!g.current.pts.empty()){
            g.current.pts.back() = bp;
        } else {
            g.current.pts.push_back(bp);
        }
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    if (g.rubberBand){
        g.rubberEnd = bp;
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    if (g.draggingSelection){
        float dx = bp.x - g.dragStart.x, dy = bp.y - g.dragStart.y;
        Board::MoveSelectedBy(dx, dy);
        g.dragStart = bp;
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    if (g.drawing){
        if (!g.current.pts.empty()){
            DWORD now = GetTickCount();
            auto last = g.current.pts.back();
            float dx = (bp.x - last.x) * g.zoomLevel;
            float dy = (bp.y - last.y) * g.zoomLevel;
            float jump = std::sqrt(dx*dx + dy*dy);
            float splitDist = std::max(10.0f, g.penSize * g.zoomLevel * 1.8f);
            if (g.lastStrokeInputTick && now - g.lastStrokeInputTick > 80 && jump > splitDist){
                Board::EndStroke();
                Board::BeginStroke(bp.x, bp.y);
                RequestInkRepaint(hWnd, true);
                return true;
            }
        }
        if (Board::AppendStroke(bp.x, bp.y))
            RequestInkRepaint(hWnd);
        return true;
    }

    return false;
}

bool HandleMouseUp(HWND hWnd, int x, int y){
    g.eraserDragging = false;

    // File browser: if was dragging, just stop; if was a tap, resolve click
    if (g.fileBrowser.active){
        if (!g.fileBrowser.dragging && g.fileBrowser.captureDown){
            // Tap captured while browser was open: resolve the click at down position
            int id2 = HitGeneric(g.fileBrowser.downX, g.fileBrowser.downY);
            g.fileBrowser.captureDown = false;
            if (id2 == ID_FILE_CLOSE){
                g.fileBrowser.active = false;
                g.stableW = 0; // force full redraw to clear stale file browser from stable
            } else if (id2 == ID_FILE_BACK){
                FileBrowserGoBack();
            } else if (id2 == ID_FILE_FORWARD){
                FileBrowserGoForward();
            } else if (id2 == ID_FILE_PARENT){
                std::wstring parent = g.fileBrowser.currentDir;
                size_t lastSlash = parent.find_last_of(L"\\/");
                if (lastSlash != std::wstring::npos && lastSlash > 2){
                    parent = parent.substr(0, lastSlash);
                    FileBrowserLoadDir(parent);
                }
            } else if (id2 == ID_FILE_NAV_DESKTOP || id2 == ID_FILE_NAV_DOCUMENTS ||
                       id2 == ID_FILE_NAV_DOWNLOADS || id2 == ID_FILE_NAV_PICTURES || id2 == ID_FILE_NAV_THISPC){
                wchar_t navPath[MAX_PATH] = {};
                bool ok = false;
                if (id2 == ID_FILE_NAV_DESKTOP)
                    ok = SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_DESKTOP, nullptr, 0, navPath));
                else if (id2 == ID_FILE_NAV_DOCUMENTS)
                    ok = SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_PERSONAL, nullptr, 0, navPath));
                else if (id2 == ID_FILE_NAV_DOWNLOADS){
                    if (SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_PROFILE, nullptr, 0, navPath))){
                        wcscat_s(navPath, L"\\Downloads");
                        DWORD attr = GetFileAttributesW(navPath);
                        ok = (attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY));
                    }
                }
                else if (id2 == ID_FILE_NAV_PICTURES)
                    ok = SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_MYPICTURES, nullptr, 0, navPath));
                else if (id2 == ID_FILE_NAV_THISPC){
                    FileBrowserLoadDir(L"C:\\");
                    InvalidateRect(hWnd, nullptr, FALSE);
                    return true;
                }
                if (ok) FileBrowserLoadDir(navPath);
            } else if (id2 >= ID_FILE_ITEM && id2 < ID_FILE_ITEM + (int)g.fileBrowser.entries.size()){
                int idx = id2 - ID_FILE_ITEM;
                // Double-check: verify the entry is actually a dir/file
                // by checking filesystem attributes (guards against stale hit data)
                DWORD attr = GetFileAttributesW(g.fileBrowser.fullPath[idx].c_str());
                bool actuallyDir = (attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY));
                if (actuallyDir || g.fileBrowser.isDir[idx]){
                    FileBrowserLoadDir(g.fileBrowser.fullPath[idx]);
                } else {
                    FileBrowserOpenFile(g.fileBrowser.fullPath[idx]);
                }
            }
        }
        g.fileBrowser.dragging = false;
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }

    if (g.assist.dragMode != AssistDragMode::None){
        AssistDragMode mode = g.assist.dragMode;
        g.assist.dragMode = AssistDragMode::None;
        g.assist.dragEdge = -1;
        if (mode == AssistDragMode::DrawTriangleLine || mode == AssistDragMode::DrawCompassArc){
            if (g.drawing) Board::EndStroke();
            RequestInkRepaint(hWnd, true);
        } else {
            SaveSettings();
            InvalidateRect(hWnd, nullptr, FALSE);
        }
        return true;
    }


    if (g.panning){
        g.panning = false;
        // Force full redraw to fill in any edge artifacts from BitBlt pan
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }
    if (g.erasing){
        Board::EndErase();
        g.current.pts.clear();
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }
    if (g.rubberBand){
        g.rubberBand = false;
        Board::SelectItemsInRect(g.rubberStart.x, g.rubberStart.y,
                                  g.rubberEnd.x, g.rubberEnd.y);
        InvalidateRect(hWnd, nullptr, FALSE);
        return true;
    }
    if (g.draggingSelection){
        g.draggingSelection = false;
        return true;
    }
    if (g.drawing){
        Board::EndStroke();
        RequestInkRepaint(hWnd, true);
        return true;
    }
    return false;
}

// ====== command dispatch ======
void OnCommand(HWND hWnd, int cmdId){
    switch (cmdId){
    case ID_TOOL_PAN:    g.tool = ToolKind::Pan;    g.penUndoGroupActive=false; g.penPopupOpen=false; g.eraserPopupOpen=false; g.assistPopupOpen=false; g.zoomPopupOpen=false; g.selPopupOpen=false; g.selectedItems.clear(); break;
    case ID_TOOL_PEN:    g.tool = ToolKind::Pen;    g.penUndoGroupActive=false; g.penPopupOpen=false; g.eraserPopupOpen=false; g.assistPopupOpen=false; g.zoomPopupOpen=false; g.selPopupOpen=false; g.selectedItems.clear(); break;
    case ID_TOOL_ERASER: g.tool = ToolKind::Eraser; g.penUndoGroupActive=false; g.penPopupOpen=false; g.eraserPopupOpen=false; g.assistPopupOpen=false; g.zoomPopupOpen=false; g.selPopupOpen=false; g.selectedItems.clear(); break;
    case ID_TOOL_SELECT: g.tool = ToolKind::Select; g.penUndoGroupActive=false; g.penPopupOpen=false; g.eraserPopupOpen=false; g.assistPopupOpen=false; g.zoomPopupOpen=false; g.selPopupOpen=false; g.selectedItems.clear(); break;
    case ID_ASSIST_MENU:
        if (!kAssistToolsEnabled) { g.assist.kind = AssistKind::None; g.assistPopupOpen = false; InvalidateRect(hWnd, nullptr, FALSE); return; }
        g.assistPopupOpen = !g.assistPopupOpen;
        g.penPopupOpen = false; g.eraserPopupOpen = false; g.zoomPopupOpen = false; g.selPopupOpen = false;
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_ASSIST_TRIANGLE45:
        if (!kAssistToolsEnabled) { g.assist.kind = AssistKind::None; g.assistPopupOpen = false; InvalidateRect(hWnd, nullptr, FALSE); return; }
        g.assist.kind = AssistKind::Triangle;
        g.assist.triangleKind = TriangleKind::RightIsosceles;
        g.assistPopupOpen = false;
        SaveSettings();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_ASSIST_TRIANGLE30:
        if (!kAssistToolsEnabled) { g.assist.kind = AssistKind::None; g.assistPopupOpen = false; InvalidateRect(hWnd, nullptr, FALSE); return; }
        g.assist.kind = AssistKind::Triangle;
        g.assist.triangleKind = TriangleKind::ThirtySixty;
        g.assistPopupOpen = false;
        SaveSettings();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_ASSIST_COMPASS:
        if (!kAssistToolsEnabled) { g.assist.kind = AssistKind::None; g.assistPopupOpen = false; InvalidateRect(hWnd, nullptr, FALSE); return; }
        g.assist.kind = AssistKind::Compass;
        g.assistPopupOpen = false;
        SaveSettings();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_ASSIST_HIDE:
        g.assist.kind = AssistKind::None;
        g.assistPopupOpen = false;
        SaveSettings();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_TOOL_UNDO:   g.selPopupOpen=false; Board::Undo(); break;
    case ID_TOOL_REDO:   g.selPopupOpen=false; Board::Redo(); break;
    case ID_TOP_EXIT:    SendMessageW(hWnd, WM_CLOSE, 0, 0); return;
    case ID_VIEW_CENTER:
        g.scrollX = 0; g.scrollY = 0;
        Board::ClampPdfScroll();  // re-center PDF pages if in PDF mode
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_WINDOW_MIN:
        ShowWindow(hWnd, SW_MINIMIZE);
        return;
    case ID_ABOUT:
        g.settingsOpen = true;
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_SET_CLOSE:
        g.settingsOpen = false;
        g.stableW = 0; // clear stale settings UI from stable bitmap
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_SET_THEME_DARK:
    case ID_SET_THEME_LIGHT:
    case ID_SET_THEME_SYS: {
        g.colorTheme = (cmdId == ID_SET_THEME_DARK) ? 0 : (cmdId == ID_SET_THEME_LIGHT) ? 1 : 2;
        ResolveDarkMode();
        SaveSettings();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    }
    case ID_SET_SHOW_SPLASH:
        g.showSplash = !g.showSplash;
        SaveSettings();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_SET_RESET:
        g.confirm = {};
        g.confirm.active = true;
        g.confirm.message = L"确定重置所有设置至默认值？";
        g.confirm.confirmCmdId = ID_SET_RESET_CONFIRMED;
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_SET_RESET_CONFIRMED:
        g.colorTheme = 0;
        g.themeColor = 0;
        g.showSplash = true;
        ResolveDarkMode();
        SaveSettings();
        ShowToast(L"设置已重置");
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_SET_CHECK_UPDATE:
        ShowToast(L"正在检查更新...");
        CheckUpdate();  // implemented in main.cpp
        return;
    case ID_TOP_OPEN:{
        OpenFileWithWindowsDialog(hWnd);
        InvalidateRect(hWnd, nullptr, FALSE);
        break;
    }
    case ID_TOP_SAVE:
        g.exitAfterProjectSave = false;
        ProjectIO::BeginSaveCloudboard(hWnd);
        break;
    case ID_PAGE_PREV: Board::PrevPage(); break;
    case ID_PAGE_NEXT: Board::NextPage(); break;
    case ID_PAGE_ADD:  Board::AddPage(); break;
    case ID_PAGE_DEL:{
        g.confirm = {};
        g.confirm.active = true;
        g.confirm.message = L"确定删除当前页？";
        g.confirm.confirmCmdId = ID_PAGE_DEL_CONFIRMED;
        InvalidateRect(hWnd, nullptr, FALSE);
        break;
    }
    case ID_PAGE_DEL_CONFIRMED:
        Board::DelPage();
        break;

    case ID_EXIT_SAVE_CONFIRMED:
        g.exitAfterProjectSave = ProjectIO::BeginSaveCloudboard(hWnd);
        return;
    case ID_EXIT_CONFIRMED:
    case ID_EXIT_DISCARD_CONFIRMED:
        DestroyWindow(hWnd);
        return;

    case ID_NEWWIN_CLOSE:
        DestroyWindow(hWnd);
        return;
    case ID_NEWWIN_MINIMIZE:
        ShowWindow(hWnd, SW_MINIMIZE);
        return;
    case ID_NEWWIN_VISIBLE:
        // Just close the dialog; window stays visible (dialog already closed by caller)
        return;

    case ID_SEL_DELETE:
        Board::DeleteSelected();
        g.selPopupOpen = false;
        g.selectedItems.clear();
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    case ID_SEL_COPY: {
        // Duplicate selected items (clone with offset) - per-page
        float offX = 20.0f, offY = 20.0f;
        // Group undo by page
        for (int pi = 0; pi < (int)g.pages.size(); ++pi){
            bool hasItems = false;
            for (auto& sel : g.selectedItems) {
                if (sel.pageIndex != pi) continue;
                if (g.pdfMode && sel.type == SelectedItem::Image && sel.index == 0) continue;
                hasItems = true;
                break;
            }
            if (!hasItems) continue;
            auto& p = g.pages[pi];
            p.undoStack.push_back({p.strokes, p.texts, p.images});
            p.redoStack.clear();
            for (auto& sel : g.selectedItems){
                if (sel.pageIndex != pi) continue;
                switch (sel.type){
                case SelectedItem::Stroke:
                    if (sel.index >= 0 && sel.index < (int)p.strokes.size()){
                        Stroke ns = p.strokes[sel.index];
                        ns.uid = g.nextObjectUid++;
                        for (auto& pt : ns.pts){ pt.x += offX; pt.y += offY; }
                        ns.a.x += offX; ns.a.y += offY;
                        ns.b.x += offX; ns.b.y += offY;
                        p.strokes.push_back(std::move(ns));
                    }
                    break;
                case SelectedItem::Text:
                    if (sel.index >= 0 && sel.index < (int)p.texts.size()){
                        TextItem nt = p.texts[sel.index];
                        nt.uid = g.nextObjectUid++;
                        nt.pos.x += offX; nt.pos.y += offY;
                        p.texts.push_back(std::move(nt));
                    }
                    break;
                case SelectedItem::Image:
                    if (sel.index >= 0 && sel.index < (int)p.images.size() &&
                        !(g.pdfMode && sel.index == 0)){
                        ImageItem ni = p.images[sel.index];
                        ni.uid = g.nextObjectUid++;
                        ni.pos.x += offX; ni.pos.y += offY;
                        p.images.push_back(std::move(ni));
                    }
                    break;
                }
            }
            g.thumbDirty[pi] = true;
        }
        g.selPopupOpen = false;
        InvalidateRect(hWnd, nullptr, FALSE);
        return;
    }
    default:
        // Check for theme color change
        if (cmdId >= ID_SET_THEME_COLOR && cmdId < ID_SET_THEME_COLOR + kThemeColorCount){
            g.themeColor = cmdId - ID_SET_THEME_COLOR;
            SaveSettings();
            InvalidateRect(hWnd, nullptr, FALSE);
            return;
        }
        // Check for ID_SEL_COLOR + index
        if (cmdId >= ID_SEL_COLOR && cmdId < ID_SEL_COLOR + kColorCount){
            int idx = cmdId - ID_SEL_COLOR;
            Color newColor = kColorPalette[idx];
            // Per-page undo
            for (int pi = 0; pi < (int)g.pages.size(); ++pi){
                bool hasItems = false;
                for (auto& sel : g.selectedItems) if (sel.pageIndex == pi) { hasItems = true; break; }
                if (!hasItems) continue;
                auto& p = g.pages[pi];
                p.undoStack.push_back({p.strokes, p.texts, p.images});
                p.redoStack.clear();
                for (auto& sel : g.selectedItems){
                    if (sel.pageIndex != pi) continue;
                    if (sel.type == SelectedItem::Stroke && sel.index >= 0 && sel.index < (int)p.strokes.size()){
                        p.strokes[sel.index].color = newColor;
                    } else if (sel.type == SelectedItem::Text && sel.index >= 0 && sel.index < (int)p.texts.size()){
                        p.texts[sel.index].color = newColor;
                    }
                }
                g.thumbDirty[pi] = true;
            }
            g.selPopupOpen = false;
            InvalidateRect(hWnd, nullptr, FALSE);
            return;
        }
        break;

    case ID_TIMER_START:{
        HWND hMin = GetDlgItem(g.hTimerPanel, 600);
        wchar_t buf2[16]; GetWindowTextW(hMin, buf2, 16);
        int m2 = _wtoi(buf2); if (m2<=0) m2=5;
        g.countdownSec = m2*60;
        g.countdownRunning = true;
        SetTimer(hWnd, TIMER_COUNTDOWN, 1000, nullptr);
        UpdateTimerDisplay();
        break;
    }
    case ID_SW_START:
        g.stopwatchRunning = true;
        SetTimer(hWnd, TIMER_STOPWATCH, 1000, nullptr);
        break;
    case ID_SW_PAUSE:
        g.stopwatchRunning = false;
        KillTimer(hWnd, TIMER_STOPWATCH);
        break;
    case ID_SW_RESET:
        g.stopwatchRunning = false;
        g.stopwatchSec = 0;
        KillTimer(hWnd, TIMER_STOPWATCH);
        UpdateTimerDisplay();
        break;
    case ID_TIMER_CLOSE:
        if (g.hTimerPanel){ DestroyWindow(g.hTimerPanel); g.hTimerPanel = nullptr; }
        break;
    }
    InvalidateRect(hWnd, nullptr, FALSE);
}

// ====== Timer panel ======
static LRESULT CALLBACK TimerPanelProc(HWND h, UINT m, WPARAM w, LPARAM l){
    switch (m){
    case WM_COMMAND:
        UI::OnCommand(g.hWnd, LOWORD(w));
        return 0;
    case WM_CLOSE:
        UI::OnCommand(g.hWnd, ID_TIMER_CLOSE);
        return 0;
    case WM_CTLCOLORSTATIC:
    case WM_CTLCOLORBTN:{
        HDC hdc2 = (HDC)w;
        SetTextColor(hdc2, RGB(232,236,241));
        SetBkColor(hdc2, RGB(37,43,59));
        static HBRUSH hb2 = CreateSolidBrush(RGB(37,43,59));
        return (LRESULT)hb2;
    }
    case WM_ERASEBKGND:{
        RECT rc2; GetClientRect(h, &rc2);
        HBRUSH hb3 = CreateSolidBrush(RGB(37,43,59));
        FillRect((HDC)w, &rc2, hb3);
        DeleteObject(hb3);
        return 1;
    }
    }
    return DefWindowProcW(h, m, w, l);
}

void ShowTimerPanel(HWND hParent){
    if (g.hTimerPanel){ SetForegroundWindow(g.hTimerPanel); return; }
    static bool sReg = false;
    if (!sReg){
        WNDCLASSW wc2{};
        wc2.lpfnWndProc = TimerPanelProc;
        wc2.hInstance = g.hInst;
        wc2.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc2.lpszClassName = L"CBTimerPanel";
        RegisterClassW(&wc2);
        sReg = true;
    }
    g.hTimerPanel = CreateWindowExW(WS_EX_TOOLWINDOW,
        L"CBTimerPanel", L"定时 / 计时",
        WS_POPUP|WS_CAPTION|WS_SYSMENU,
        300,200,380,280, hParent, nullptr, g.hInst, nullptr);

    HFONT hFn = CreateFontW(-14,0,0,0,FW_NORMAL,FALSE,FALSE,FALSE,
        DEFAULT_CHARSET,OUT_DEFAULT_PRECIS,CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,DEFAULT_PITCH|FF_DONTCARE,L"Anthropic Sans Text");
    HFONT hFnB = CreateFontW(-44,0,0,0,FW_BOLD,FALSE,FALSE,FALSE,
        DEFAULT_CHARSET,OUT_DEFAULT_PRECIS,CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,DEFAULT_PITCH|FF_DONTCARE,L"Anthropic Sans Text");

    CreateWindowExW(0,L"STATIC",L"00:00",
        WS_CHILD|WS_VISIBLE|SS_CENTER, 16,12,348,60,
        g.hTimerPanel,(HMENU)601,g.hInst,nullptr);
    SendMessageW(GetDlgItem(g.hTimerPanel,601), WM_SETFONT, (WPARAM)hFnB, TRUE);

    CreateWindowExW(0,L"STATIC",L"倒计时（分钟）",
        WS_CHILD|WS_VISIBLE, 16,90,110,22,
        g.hTimerPanel,(HMENU)602,g.hInst,nullptr);
    CreateWindowExW(WS_EX_CLIENTEDGE,L"EDIT",L"5",
        WS_CHILD|WS_VISIBLE|ES_NUMBER, 130,88,60,24,
        g.hTimerPanel,(HMENU)600,g.hInst,nullptr);
    CreateWindowExW(0,L"BUTTON",L"开始倒计时",
        WS_CHILD|WS_VISIBLE, 200,86,160,28,
        g.hTimerPanel,(HMENU)ID_TIMER_START,g.hInst,nullptr);

    CreateWindowExW(0,L"BUTTON",L"开始计时",
        WS_CHILD|WS_VISIBLE, 16,130,100,28,
        g.hTimerPanel,(HMENU)ID_SW_START,g.hInst,nullptr);
    CreateWindowExW(0,L"BUTTON",L"鏆傚仠",
        WS_CHILD|WS_VISIBLE, 130,130,80,28,
        g.hTimerPanel,(HMENU)ID_SW_PAUSE,g.hInst,nullptr);
    CreateWindowExW(0,L"BUTTON",L"重置",
        WS_CHILD|WS_VISIBLE, 220,130,80,28,
        g.hTimerPanel,(HMENU)ID_SW_RESET,g.hInst,nullptr);

    EnumChildWindows(g.hTimerPanel, [](HWND ch, LPARAM lp)->BOOL{
        if (GetDlgCtrlID(ch)==601) return TRUE;
        SendMessageW(ch, WM_SETFONT, (WPARAM)lp, TRUE);
        return TRUE;
    }, (LPARAM)hFn);

    ShowWindow(g.hTimerPanel, SW_SHOW);
}

void UpdateTimerDisplay(){
    if (!g.hTimerPanel) return;
    HWND h2 = GetDlgItem(g.hTimerPanel, 601);
    if (!h2) return;
    int sec2 = (g.countdownRunning || g.countdownSec>0) ? g.countdownSec : g.stopwatchSec;
    int mm = sec2/60, ss = sec2%60;
    wchar_t buf3[16]; swprintf(buf3, 16, L"%02d:%02d", mm, ss);
    SetWindowTextW(h2, buf3);
}

} // namespace UI

// ====== Global toast ======
void ShowToast(const std::wstring& s){
    g.hintText = s;
    g.hintHideAt = GetTickCount() + 3000;
    InvalidateRect(g.hWnd, nullptr, FALSE);
    SetTimer(g.hWnd, TIMER_HINT_HIDE, 3100, nullptr);
}
