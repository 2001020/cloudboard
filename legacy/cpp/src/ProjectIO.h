#pragma once
#include "Common.h"

namespace ProjectIO {
    bool IsCloudboardFile(const std::wstring& path);
    bool BeginSaveCloudboard(HWND hWnd);
    bool SaveCloudboardToFile(const std::wstring& path, std::wstring* error);
    bool LoadCloudboardFromFile(const std::wstring& path, std::wstring* error);
}
