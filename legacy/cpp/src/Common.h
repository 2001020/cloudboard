#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <windows.h>
#include <windowsx.h>
#include <commctrl.h>
#include <commdlg.h>
#include <shellapi.h>
#include <shlwapi.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <winhttp.h>
#pragma comment(lib, "winhttp.lib")

// GDI+
#include <objidl.h>
#include <gdiplus.h>
using namespace Gdiplus;

// OpenGL
#include <gl/GL.h>
#pragma comment(lib, "opengl32.lib")

#include <vector>
#include <string>
#include <memory>
#include <algorithm>
#include <cmath>
#include <chrono>
#include <ctime>
#include <cstdint>
#include <cwchar>
#include <atomic>
#include <cstdio>
#include <functional>

#define APP_VERSION       L"26.2.0"

constexpr int kTopBarH_base     = 0;
constexpr int kBottomBarH_base  = 60;
constexpr int kMaxPages         = 56;
constexpr float kZoomDisplayBase = 2.0f;
constexpr float kZoomMin         = 0.5f;
constexpr float kZoomMax         = 7.0f;

constexpr UINT_PTR TIMER_COUNTDOWN    = 1004;
constexpr UINT_PTR TIMER_STOPWATCH    = 1005;
constexpr UINT_PTR TIMER_HINT_HIDE    = 1006;
constexpr UINT_PTR TIMER_SCROLL_IDLE  = 1007;
constexpr UINT_PTR TIMER_UPDATE_PROG  = 1008;
constexpr UINT_PTR TIMER_INK_REPAINT  = 1009;

enum CmdID : int {
    ID_TOOL_PEN = 100, ID_TOOL_ERASER, ID_TOOL_PAN, ID_TOOL_SELECT,
    ID_TOOL_UNDO, ID_TOOL_REDO, ID_TOOL_CLEAR,
    ID_TOP_EXIT = 202, ID_TOP_OPEN = 203, ID_TOP_SAVE = 204,
    ID_PAGE_PREV = 300, ID_PAGE_NEXT, ID_PAGE_ADD, ID_PAGE_DEL,
    ID_TIMER_START = 500, ID_SW_START, ID_SW_PAUSE, ID_SW_RESET, ID_TIMER_CLOSE,
    ID_VIEW_CENTER = 600, ID_WINDOW_MIN, ID_ABOUT = 650,
    ID_PAGE_NUM_TOGGLE = 700,
    ID_CONFIRM_YES = 800, ID_CONFIRM_NO, ID_EXIT_CONFIRMED, ID_PAGE_DEL_CONFIRMED, ID_SET_RESET_CONFIRMED, ID_SET_CHECK_UPDATE,
    ID_EXIT_SAVE_CONFIRMED, ID_EXIT_DISCARD_CONFIRMED,
    ID_NEWWIN_CLOSE = 810, ID_NEWWIN_MINIMIZE, ID_NEWWIN_VISIBLE,
    ID_ZOOM_TOGGLE = 860, ID_ZOOM_50, ID_ZOOM_75, ID_ZOOM_100, ID_ZOOM_150, ID_ZOOM_200, ID_ZOOM_300, ID_ZOOM_350,
    ID_ASSIST_MENU = 880, ID_ASSIST_TRIANGLE45, ID_ASSIST_TRIANGLE30, ID_ASSIST_COMPASS, ID_ASSIST_HIDE,
    ID_SEL_DELETE = 900, ID_SEL_COPY, ID_SEL_COLOR = 920,
    ID_FILE_CLOSE = 950, ID_FILE_BACK, ID_FILE_FORWARD, ID_FILE_PARENT,
    ID_FILE_NAV_DESKTOP, ID_FILE_NAV_DOCUMENTS, ID_FILE_NAV_DOWNLOADS, ID_FILE_NAV_PICTURES, ID_FILE_NAV_THISPC,
    ID_FILE_ITEM = 1000,
};

enum class ToolKind { Pen, Eraser, Pan, Select };
enum class LineStyle { Solid, Dashed, Dotted };
enum class ShapeKind { Rect, Ellipse, Line, Arrow, Triangle, Free };
enum class AssistKind { None, Triangle, Compass };
enum class TriangleKind { RightIsosceles, ThirtySixty };
enum class AssistDragMode { None, MoveTriangle, RotateTriangle, DrawTriangleLine, MoveCompass, AdjustCompassRadius, DrawCompassArc };

struct PointF2 { float x, y; float pressure{ 1.0f }; uint16_t dtMs{ 0 }; };

struct SelectedItem { enum Type { Stroke, Text, Image }; Type type; int index; int pageIndex; };

struct Stroke {
    uint64_t uid{ 0 };
    std::vector<PointF2> pts; Color color{ 30, 30, 30 }; float width{ 4.0f };
    LineStyle style{ LineStyle::Solid }; bool isHighlighter{ false }; bool isShape{ false };
    ShapeKind shape{ ShapeKind::Free }; PointF2 a{}, b{};
};

struct TextItem {
    uint64_t uid{ 0 };
    std::wstring text; PointF2 pos{}; Color color{ 30, 30, 30 };
    float fontSize{ 36.f }; std::wstring family{ L"Anthropic Sans Text" }; bool bold{ false };
};

struct ImageItem { uint64_t uid{ 0 }; std::shared_ptr<Bitmap> bmp; PointF2 pos{}; float w{}, h{}; float rotateDeg{ 0 }; };

struct ConfirmDialog {
    bool active = false; std::wstring message; int confirmCmdId = 0;
    int altCmdId = 0; std::wstring altLabel; int thirdCmdId = 0; std::wstring thirdLabel;
    std::wstring cancelLabel;
};

struct FileBrowser {
    bool active = false; std::wstring currentDir; std::vector<std::wstring> entries;
    std::vector<std::wstring> fullPath; std::vector<bool> isDir; int scrollY = 0;
    bool dragging = false; int dragLastY = 0; int downX = 0, downY = 0; bool captureDown{ false };
    std::vector<std::wstring> history; int historyIdx = -1;
};

struct AssistState {
    AssistKind kind{ AssistKind::None };
    TriangleKind triangleKind{ TriangleKind::RightIsosceles };
    PointF2 trianglePos{ 360.0f, 260.0f };
    float triangleAngle{ 0.0f };
    float triangleScale{ 1.0f };
    PointF2 compassCenter{ 460.0f, 300.0f };
    float compassRadius{ 150.0f };
    float compassAngle{ -0.65f };
    AssistDragMode dragMode{ AssistDragMode::None };
    PointF2 dragLastBoard{};
    PointF2 dragStartBoard{};
    int dragEdge{ -1 };
    float dragStartAngle{ 0.0f };
    float compassDrawLastAngle{ 0.0f };
    float compassDrawSweep{ 0.0f };
    float dragStartRadius{ 0.0f };
    float gestureStartDist{ 0.0f };
    float gestureStartAngle{ 0.0f };
    PointF2 gestureStartMid{};
    PointF2 gestureStartTrianglePos{};
    float gestureStartTriangleAngle{ 0.0f };
    float gestureStartTriangleScale{ 1.0f };
    PointF2 gestureStartCompassCenter{};
    float gestureStartCompassRadius{ 0.0f };
    float gestureStartCompassAngle{ 0.0f };
};

struct UndoSnapshot { std::vector<Stroke> strokes; std::vector<TextItem> texts; std::vector<ImageItem> images; };

struct Page {
    std::vector<Stroke> strokes; std::vector<TextItem> texts; std::vector<ImageItem> images;
    std::vector<UndoSnapshot> undoStack; std::vector<UndoSnapshot> redoStack;
    std::shared_ptr<Bitmap> bgImage;
};

struct AppState {
    HINSTANCE hInst{}; HWND hWnd{}; ULONG_PTR gdiplusToken{ 0 }; HICON hAppIcon{ nullptr };
    float dpiScale{ 1.0f };
    ToolKind tool{ ToolKind::Pen }; Color color{ 30, 30, 30 }; float penSize{ 4.0f };
    LineStyle lineStyle{ LineStyle::Solid }; ShapeKind currentShape{ ShapeKind::Rect };
    float fontSize{ 36.f }; std::wstring fontFamily{ L"Anthropic Sans Text" };
    float eraserSize{ 20.0f }; static constexpr float kEraserMin = 5.0f; static constexpr float kEraserMax = 80.0f;
    bool eraserDragging{ false };
    bool penPopupOpen{ false }; bool eraserPopupOpen{ false }; bool assistPopupOpen{ false };
    bool selPopupOpen{ false }; int selPopupX{ 0 }, selPopupY{ 0 };
    ConfirmDialog confirm; FileBrowser fileBrowser; AssistState assist;
    std::vector<Page> pages; int currentPage{ 0 };
    uint64_t nextObjectUid{ 1 };
    bool drawing{ false }; Stroke current; bool shaping{ false }; PointF2 shapeStart{}, shapeEnd{}; int activeDrawPage{ -1 };
    std::vector<SelectedItem> selectedItems; bool rubberBand{ false }; PointF2 rubberStart{}, rubberEnd{};
    bool draggingSelection{ false }; PointF2 dragStart{}; HWND hEdit{ nullptr }; int editingTextIdx{ -1 };
    std::vector<Stroke> clipboardStrokes; std::vector<TextItem> clipboardTexts; std::vector<ImageItem> clipboardImages;
    bool panning{ false }; bool wheelPanning{ false }; bool silentExit{ false }; bool exitAfterProjectSave{ false };
    int panLastX{ 0 }, panLastY{ 0 }; float scrollX{ 0 }, scrollY{ 0 }; float prevScrollX{ 0 }, prevScrollY{ 0 };
    bool erasing{ false }; bool eraserHasErased{ false }; float eraserTravel{ 0.0f };
    std::vector<Stroke> preEraseStrokes; std::vector<PointF2> eraserPath;
    DWORD lastStrokeInputTick{ 0 }; DWORD lastStrokeEndTick{ 0 };
    DWORD lastPenUndoTick{ 0 }; bool penUndoGroupActive{ false }; int penUndoGroupPage{ -1 };
    DWORD lastInkRepaintTick{ 0 }; bool inkRepaintPending{ false };
    DWORD activeTouchId{ 0 }; bool touchContactActive{ false };
    std::wstring hintText; DWORD hintHideAt{ 0 }; bool settingsOpen{ false };
    std::wstring currentProjectPath;
    int colorTheme{ 0 }; int themeColor{ 0 }; bool fullscreenMode{ true }; bool showSplash{ true }; bool isDarkMode{ true };
    HWND hTimerPanel{ nullptr }; int countdownSec{ 0 }; bool countdownRunning{ false };
    int stopwatchSec{ 0 }; bool stopwatchRunning{ false };
    HBITMAP hMemBmp{ nullptr }; HDC hMemDC{ nullptr }; int memW{ 0 }, memH{ 0 };
    HBITMAP hStableBmp{ nullptr }; HDC hStableDC{ nullptr }; int stableW{ 0 }, stableH{ 0 };
    std::vector<std::shared_ptr<Bitmap>> thumbCache; std::vector<bool> thumbDirty;
    bool loadingActive{ false }; std::wstring loadingText; std::atomic<int> loadingProgress{ 0 };
    int loadingTotal{ 0 }; std::atomic<bool> loadingCancelled{ false };
    float zoomLevel{ kZoomDisplayBase }; bool zoomPopupOpen{ false }; float lastPinchDist{ 0.0f }; bool pinching{ false };
    void* pdfDoc{ nullptr }; bool pdfMode{ false }; std::wstring pdfPath;
    std::vector<float> pdfPageYOffset; float pdfTotalHeight{ 0.0f }; float pdfPageWidth{ 0.0f }; int pdfViewCenter{ 0 };
    DWORD lastTouchTick{ 0 }; void* renderQueue{ nullptr };
    bool glInitialized{ false };
};

extern AppState g;

inline int S(int base) { return (int)(base * g.dpiScale); }
inline float Sf(float base) { return base * g.dpiScale; }

inline RECT BoardRect(HWND hWnd) { RECT rc; GetClientRect(hWnd, &rc); return { 0, S(kTopBarH_base), rc.right, rc.bottom - S(kBottomBarH_base) }; }
inline RECT TopBarRect(HWND hWnd) { RECT rc; GetClientRect(hWnd, &rc); return { 0, 0, rc.right, S(kTopBarH_base) }; }
inline RECT BottomBarRect(HWND hWnd) { RECT rc; GetClientRect(hWnd, &rc); int bh = S(kBottomBarH_base); return { 0, rc.bottom - bh, rc.right, rc.bottom }; }
inline bool PtInRect2(const RECT& r, int x, int y){ return x>=r.left && x<r.right && y>=r.top && y<r.bottom; }

void ShowToast(const std::wstring& s);
inline void PumpMessages(){ MSG m; while (PeekMessageW(&m, nullptr, 0, 0, PM_REMOVE)){ TranslateMessage(&m); DispatchMessageW(&m); } }

constexpr UINT WM_APP_RENDER_DONE  = WM_APP + 0x100;
constexpr UINT WM_APP_UPDATE_DONE  = WM_APP + 0x200;
constexpr UINT WM_APP_PROJECT_SAVE_DONE = WM_APP + 0x300;

struct RenderResult { int pageIdx; std::shared_ptr<Bitmap> bitmap; bool isThumbnail; };

void SaveSettings(); void LoadSettings(); void ResolveDarkMode(); void CheckUpdate();
