#pragma once
#include "Common.h"

namespace UI {
    void DrawAll(HDC hdc, HWND hWnd);

    // Mouse routing
    bool HandleMouseDown(HWND hWnd, int x, int y);
    bool HandleMouseMove(HWND hWnd, int x, int y, bool buttonDown);
    bool HandleMouseUp(HWND hWnd, int x, int y);

    // Command dispatch
    void OnCommand(HWND hWnd, int cmdId);

    // Hit testing for option bars
    bool HitPopup(int x, int y);

    // Thumbnails
    void InvalidateThumb(int pageIdx);

    // Timer panel
    void ShowTimerPanel(HWND hParent);
    void UpdateTimerDisplay();
}
