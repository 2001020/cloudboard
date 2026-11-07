#include "Common.h"
#include <cstdio>
#include <cstdlib>

// Minimal JSON read/write — no external library needed.
// We only store a flat object with known keys.

static std::wstring GetSettingsPath(){
    wchar_t exePath[MAX_PATH];
    GetModuleFileNameW(nullptr, exePath, MAX_PATH);
    std::wstring dir(exePath);
    size_t slash = dir.find_last_of(L"\\/");
    if (slash != std::wstring::npos) dir = dir.substr(0, slash + 1);
    return dir + L"settings.json";
}

// Simple JSON integer/bool reader
static int ReadJsonInt(const std::wstring& json, const wchar_t* key, int defVal){
    std::wstring search = std::wstring(L"\"") + key + L"\"";
    size_t pos = json.find(search);
    if (pos == std::wstring::npos) return defVal;
    pos = json.find(L':', pos + search.size());
    if (pos == std::wstring::npos) return defVal;
    pos++;
    while (pos < json.size() && (json[pos] == ' ' || json[pos] == '\t' || json[pos] == '\r' || json[pos] == '\n'))
        pos++;
    if (pos >= json.size()) return defVal;
    if (json[pos] == 't') return 1;  // true
    if (json[pos] == 'f') return 0;  // false
    return _wtoi(json.c_str() + pos);
}

static float ReadJsonFloat(const std::wstring& json, const wchar_t* key, float defVal){
    std::wstring search = std::wstring(L"\"") + key + L"\"";
    size_t pos = json.find(search);
    if (pos == std::wstring::npos) return defVal;
    pos = json.find(L':', pos + search.size());
    if (pos == std::wstring::npos) return defVal;
    pos++;
    while (pos < json.size() && (json[pos] == ' ' || json[pos] == '\t' || json[pos] == '\r' || json[pos] == '\n'))
        pos++;
    if (pos >= json.size()) return defVal;
    return (float)_wtof(json.c_str() + pos);
}

static void ApplyAssistSettings(const std::wstring& json){
    // Do not restore visibility. Teaching aids should be hidden after every app restart.
    g.assist.kind = AssistKind::None;
    int tk = ReadJsonInt(json, L"triangleKind", (int)TriangleKind::RightIsosceles);
    if (tk < (int)TriangleKind::RightIsosceles || tk > (int)TriangleKind::ThirtySixty) tk = (int)TriangleKind::RightIsosceles;
    g.assist.triangleKind = (TriangleKind)tk;
    g.assist.trianglePos.x = ReadJsonFloat(json, L"triangleX", g.assist.trianglePos.x);
    g.assist.trianglePos.y = ReadJsonFloat(json, L"triangleY", g.assist.trianglePos.y);
    g.assist.triangleAngle = ReadJsonFloat(json, L"triangleAngle", g.assist.triangleAngle);
    g.assist.triangleScale = ReadJsonFloat(json, L"triangleScale", g.assist.triangleScale);
    if (g.assist.triangleScale < 0.45f || g.assist.triangleScale > 2.4f) g.assist.triangleScale = 1.0f;
    g.assist.compassCenter.x = ReadJsonFloat(json, L"compassX", g.assist.compassCenter.x);
    g.assist.compassCenter.y = ReadJsonFloat(json, L"compassY", g.assist.compassCenter.y);
    g.assist.compassRadius = ReadJsonFloat(json, L"compassRadius", g.assist.compassRadius);
    if (g.assist.compassRadius < 30.0f || g.assist.compassRadius > 2000.0f) g.assist.compassRadius = 150.0f;
    g.assist.compassAngle = ReadJsonFloat(json, L"compassAngle", g.assist.compassAngle);
    g.assist.dragMode = AssistDragMode::None;
}

void SaveSettings(){
    std::wstring path = GetSettingsPath();
    FILE* f = nullptr;
    // Binary mode to avoid CRT \n→\r\n translation breaking ftell/fread on reload
    if (_wfopen_s(&f, path.c_str(), L"wb") != 0 || !f) return;

    // Write UTF-16 BOM so LoadSettings can detect encoding
    unsigned short bom = 0xFEFF;
    fwrite(&bom, sizeof(bom), 1, f);

    fwprintf(f, L"{\n");
    fwprintf(f, L"  \"colorTheme\": %d,\n", g.colorTheme);
    fwprintf(f, L"  \"themeColor\": %d,\n", g.themeColor);
    fwprintf(f, L"  \"showSplash\": %s,\n", g.showSplash ? L"true" : L"false");
    fwprintf(f, L"  \"assistKind\": %d,\n", (int)AssistKind::None);
    fwprintf(f, L"  \"triangleKind\": %d,\n", (int)g.assist.triangleKind);
    fwprintf(f, L"  \"triangleX\": %.3f,\n", g.assist.trianglePos.x);
    fwprintf(f, L"  \"triangleY\": %.3f,\n", g.assist.trianglePos.y);
    fwprintf(f, L"  \"triangleAngle\": %.6f,\n", g.assist.triangleAngle);
    fwprintf(f, L"  \"triangleScale\": %.4f,\n", g.assist.triangleScale);
    fwprintf(f, L"  \"compassX\": %.3f,\n", g.assist.compassCenter.x);
    fwprintf(f, L"  \"compassY\": %.3f,\n", g.assist.compassCenter.y);
    fwprintf(f, L"  \"compassRadius\": %.3f,\n", g.assist.compassRadius);
    fwprintf(f, L"  \"compassAngle\": %.6f\n", g.assist.compassAngle);
    fwprintf(f, L"}\n");
    fclose(f);
}

void LoadSettings(){
    std::wstring path = GetSettingsPath();
    FILE* f = nullptr;
    // Binary mode — ftell returns reliable byte offset
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return;

    // Check for UTF-16 BOM (0xFEFF)
    unsigned short bom = 0;
    size_t bomRead = fread(&bom, sizeof(bom), 1, f);
    bool hasBom = (bomRead == 1 && bom == 0xFEFF);

    // If no BOM, rewind and read raw bytes as UTF-8 / narrow text
    if (!hasBom){
        fseek(f, 0, SEEK_END);
        long len = ftell(f);
        fseek(f, 0, SEEK_SET);
        if (len <= 0 || len > 10000){ fclose(f); return; }
        std::string raw(len, '\0');
        fread(&raw[0], 1, len, f);
        fclose(f);

        // Convert narrow string to wide for our parser
        int wlen = MultiByteToWideChar(CP_UTF8, 0, raw.c_str(), (int)raw.size(), nullptr, 0);
        if (wlen <= 0){ return; }
        std::wstring json(wlen, L'\0');
        MultiByteToWideChar(CP_UTF8, 0, raw.c_str(), (int)raw.size(), &json[0], wlen);

        g.colorTheme = ReadJsonInt(json, L"colorTheme", 0);
        if (g.colorTheme < 0 || g.colorTheme > 2) g.colorTheme = 0;
        g.themeColor = ReadJsonInt(json, L"themeColor", 0);
        if (g.themeColor < 0 || g.themeColor >= 6) g.themeColor = 0;
        g.showSplash = ReadJsonInt(json, L"showSplash", 1) != 0;
        ApplyAssistSettings(json);
    } else {
        // BOM present — file was written by us in UTF-16
        fseek(f, 0, SEEK_END);
        long len = ftell(f);
        // File size minus BOM, in bytes
        long dataLen = len - (long)sizeof(bom);
        fseek(f, (long)sizeof(bom), SEEK_SET);
        if (dataLen <= 0 || dataLen > 10000){ fclose(f); return; }
        // Read as wide chars
        std::wstring json(dataLen / sizeof(wchar_t) + 1, L'\0');
        size_t read = fread(&json[0], sizeof(wchar_t), dataLen / sizeof(wchar_t), f);
        json.resize(read);
        fclose(f);

        g.colorTheme = ReadJsonInt(json, L"colorTheme", 0);
        if (g.colorTheme < 0 || g.colorTheme > 2) g.colorTheme = 0;
        g.themeColor = ReadJsonInt(json, L"themeColor", 0);
        if (g.themeColor < 0 || g.themeColor >= 6) g.themeColor = 0;
        g.showSplash = ReadJsonInt(json, L"showSplash", 1) != 0;
        ApplyAssistSettings(json);
    }

    ResolveDarkMode();
}

void ResolveDarkMode(){
    if (g.colorTheme == 2){
        // Follow system
        HKEY hKey;
        DWORD val = 0, sz = sizeof(DWORD);
        if (RegOpenKeyExW(HKEY_CURRENT_USER,
                L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                0, KEY_READ, &hKey) == ERROR_SUCCESS){
            RegQueryValueExW(hKey, L"AppsUseLightTheme", nullptr, nullptr, (LPBYTE)&val, &sz);
            RegCloseKey(hKey);
        }
        g.isDarkMode = (val == 0);
    } else {
        g.isDarkMode = (g.colorTheme == 0);
    }
}
