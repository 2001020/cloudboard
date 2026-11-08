#include "Common.h"
#include "Board.h"
#include "UI.h"
#include "PdfDoc.h"
#include "RenderQueue.h"
#include "GLRenderer.h"
#include "ProjectIO.h"
#include <ShellScalingApi.h>
#include <wincrypt.h>

// App version - keep in sync with release.bat VERSION
#define APP_MIN_SUPPORTED L"20.0.0"

// OTA update check URL - Gitee raw file service (small files, no login required)
#define UPDATE_CHECK_URL  L"https://raw.giteeusercontent.com/jellybean123/cloudboard-updates/raw/master/update.json"

// ====== OTA Update Check ======

// Compare two dot-separated version strings. Returns: <0 if a<b, 0 if equal, >0 if a>b
static int CompareVersion(const wchar_t* a, const wchar_t* b){
    std::wstring wa(a), wb(b);
    size_t pa = 0, pb = 0;
    while (pa < wa.size() || pb < wb.size()){
        int na = 0, nb = 0;
        while (pa < wa.size() && wa[pa] != L'.'){ na = na * 10 + (wa[pa] - L'0'); pa++; }
        while (pb < wb.size() && wb[pb] != L'.'){ nb = nb * 10 + (wb[pb] - L'0'); pb++; }
        if (na != nb) return na - nb;
        pa++; pb++; // skip dot
    }
    return 0;
}

// Extract a JSON string value for a given key. Returns empty string if not found.
static std::wstring ExtractJsonString(const std::wstring& json, const wchar_t* key){
    std::wstring search = std::wstring(L"\"") + key + L"\"";
    size_t pos = json.find(search);
    if (pos == std::wstring::npos) return L"";
    pos = json.find(L'"', pos + search.size() + 1); // find opening quote of value
    if (pos == std::wstring::npos) return L"";
    pos++; // skip opening quote
    size_t end = json.find(L'"', pos);
    if (end == std::wstring::npos) return L"";
    return json.substr(pos, end - pos);
}

// HTTP GET - returns raw body as wide string. Runs synchronously (used on background thread).
// Sets *cancelled to true if the operation was aborted.
static std::wstring WinHttpGet(const wchar_t* url, std::atomic<bool>* cancelled){
    URL_COMPONENTS uc{}; uc.dwStructSize = sizeof(uc);
    wchar_t host[256]={0}, path[2048]={0};
    uc.lpszHostName=host; uc.dwHostNameLength=256;
    uc.lpszUrlPath=path; uc.dwUrlPathLength=2048;
    if (!WinHttpCrackUrl(url, 0, 0, &uc)) return L"";

    HINTERNET hS=WinHttpOpen(L"CloudBoard/26.2.0",
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,WINHTTP_NO_PROXY_NAME,WINHTTP_NO_PROXY_BYPASS,0);
    if (!hS) return L"";
    HINTERNET hC=WinHttpConnect(hS, host, uc.nPort, 0);
    HINTERNET hR=nullptr;
    if (hC) hR=WinHttpOpenRequest(hC, L"GET", path, nullptr,
        WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES,
        (uc.nScheme==INTERNET_SCHEME_HTTPS)?WINHTTP_FLAG_SECURE:0);

    std::string raw;
    if (hR && WinHttpSendRequest(hR,nullptr,0,nullptr,0,0,0) && WinHttpReceiveResponse(hR,nullptr)){
        DWORD avail=0;
        while (WinHttpQueryDataAvailable(hR, &avail) && avail>0){
            if (cancelled && cancelled->load()) break;
            size_t off=raw.size(); raw.resize(off+avail);
            DWORD read=0;
            if (!WinHttpReadData(hR, &raw[off], avail, &read)) break;
            raw.resize(off+read);
        }
    }
    if (hR) WinHttpCloseHandle(hR);
    if (hC) WinHttpCloseHandle(hC);
    WinHttpCloseHandle(hS);

    if (cancelled && cancelled->load()) return L"";
    if (raw.empty()) return L"";
    int wlen=MultiByteToWideChar(CP_UTF8,0,raw.c_str(),(int)raw.size(),nullptr,0);
    if (wlen<=0) return L"";
    std::wstring json(wlen, L'\0');
    MultiByteToWideChar(CP_UTF8,0,raw.c_str(),(int)raw.size(),&json[0],wlen);
    return json;
}

// Background thread: performs the full update check + download + install
// Result codes posted via WM_APP_UPDATE_DONE: 0=up-to-date, 1=error, 2=cancelled
static DWORD WINAPI CheckUpdateThread(LPVOID){
    // --- Step 1: Fetch update.json ---
    g.loadingText = L"正在访问服务器...";
    std::wstring json = WinHttpGet(UPDATE_CHECK_URL, &g.loadingCancelled);
    if (g.loadingCancelled.load()){ PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 2, 0); return 0; }
    if (json.empty()){ PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 1, 0); return 0; }

    // --- Step 2: Parse & compare ---
    g.loadingText = L"正在比对版本号...";
    std::wstring remoteVer = ExtractJsonString(json, L"version");
    std::wstring downloadUrl = ExtractJsonString(json, L"download_url");
    if (remoteVer.empty()){ PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 1, 0); return 0; }

    if (CompareVersion(remoteVer.c_str(), APP_VERSION) <= 0){
        PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 0, 0);  // up-to-date
        return 0;
    }

    // Parse expected checksum from update.json
    std::wstring expectedSha256 = ExtractJsonString(json, L"checksum");

    // --- Step 3: Download update to app directory's updates folder ---
    g.loadingText = L"正在下载更新...";

    wchar_t exePath[MAX_PATH]; GetModuleFileNameW(nullptr,exePath,MAX_PATH);
    std::wstring appDir(exePath);
    size_t sl=appDir.find_last_of(L"\\/");
    if (sl!=std::wstring::npos) appDir=appDir.substr(0,sl+1);

    std::wstring updDir = appDir + L"updates\\";
    CreateDirectoryW(updDir.c_str(), nullptr);
    std::wstring zipPath = updDir + L"update.zip";

    bool dlOk = false;
    {
        URL_COMPONENTS duc{}; duc.dwStructSize=sizeof(duc);
        wchar_t dhost[256]={0}, dpath[2048]={0};
        duc.lpszHostName=dhost; duc.dwHostNameLength=256;
        duc.lpszUrlPath=dpath; duc.dwUrlPathLength=2048;
        if (WinHttpCrackUrl(downloadUrl.c_str(),0,0,&duc)){
            HINTERNET s=WinHttpOpen(L"CloudBoard/26.2.0",
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,WINHTTP_NO_PROXY_NAME,WINHTTP_NO_PROXY_BYPASS,0);
            HINTERNET c=WinHttpConnect(s,dhost,duc.nPort,0);
            HINTERNET r=nullptr;
            if (c) r=WinHttpOpenRequest(c,L"GET",dpath,nullptr,
                WINHTTP_NO_REFERER,WINHTTP_DEFAULT_ACCEPT_TYPES,
                (duc.nScheme==INTERNET_SCHEME_HTTPS)?WINHTTP_FLAG_SECURE:0);
            if (r && WinHttpSendRequest(r,nullptr,0,nullptr,0,0,0) && WinHttpReceiveResponse(r,nullptr)){
                DWORD total=0, sz=sizeof(total);
                WinHttpQueryHeaders(r,WINHTTP_QUERY_CONTENT_LENGTH|WINHTTP_QUERY_FLAG_NUMBER,
                    WINHTTP_HEADER_NAME_BY_INDEX,&total,&sz,WINHTTP_NO_HEADER_INDEX);
                HANDLE hf=CreateFileW(zipPath.c_str(),GENERIC_WRITE,0,nullptr,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
                if (hf!=INVALID_HANDLE_VALUE){
                    DWORD avail=0, got=0; std::vector<BYTE> buf(65536);
                    while (WinHttpQueryDataAvailable(r,&avail)&&avail>0){
                        if (g.loadingCancelled.load()) break;
                        DWORD toRead=std::min(avail,(DWORD)buf.size()), read=0;
                        if (!WinHttpReadData(r,buf.data(),toRead,&read)) break;
                        DWORD wr=0; WriteFile(hf,buf.data(),read,&wr,nullptr);
                        got+=read;
                        if (total>0){
                            int p = 30 + (int)(60.0f * (float)got / (float)total);
                            if (p > g.loadingProgress.load()) g.loadingProgress.store(p);
                        }
                    }
                    CloseHandle(hf); dlOk=(got>0&&!g.loadingCancelled.load());
                }
            }
            if (r) WinHttpCloseHandle(r);
            if (c) WinHttpCloseHandle(c);
            WinHttpCloseHandle(s);
        }
    }

    if (g.loadingCancelled.load()){ DeleteFileW(zipPath.c_str()); PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 2, 0); return 0; }
    if (!dlOk){ PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 1, 0); return 0; }

    // --- Step 3.5: Verify SHA256 ---
    g.loadingText = L"正在验证完整性...";
    g.loadingProgress.store(90);
    if (!expectedSha256.empty()){
        // Compute SHA256 of downloaded zip
        std::wstring actualSha256;
        HANDLE hf2=CreateFileW(zipPath.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,0,nullptr);
        if (hf2!=INVALID_HANDLE_VALUE){
            HCRYPTPROV hProv=0; HCRYPTHASH hHash=0;
            if (CryptAcquireContextW(&hProv,nullptr,nullptr,PROV_RSA_AES,CRYPT_VERIFYCONTEXT)){
                if (CryptCreateHash(hProv,CALG_SHA_256,0,0,&hHash)){
                    BYTE rbuf[65536]; DWORD rread;
                    while (ReadFile(hf2,rbuf,sizeof(rbuf),&rread,nullptr)&&rread>0)
                        CryptHashData(hHash,rbuf,rread,0);
                    DWORD hashLen=32; BYTE hash[32];
                    if (CryptGetHashParam(hHash,HP_HASHVAL,hash,&hashLen,0)){
                        wchar_t hex[65];
                        for (int i=0;i<32;i++) swprintf(hex+i*2,3,L"%02x",hash[i]);
                        hex[64]=0; actualSha256=hex;
                    }
                    CryptDestroyHash(hHash);
                }
                CryptReleaseContext(hProv,0);
            }
            CloseHandle(hf2);
        }
        if (actualSha256.empty() || _wcsicmp(actualSha256.c_str(),expectedSha256.c_str())!=0){
            DeleteFileW(zipPath.c_str());
            PostMessageW(g.hWnd, WM_APP_UPDATE_DONE, 1, 0); return 0;
        }
    }

    // --- Step 4: Install ---
    g.loadingText = L"正在准备安装...";
    g.loadingProgress.store(97);

    {
        // Batch script: wait for old exe to close, extract the package into app root, start it.
        // The batch lives in updates\ and is launched via ShellExecute.
        std::wstring batPath=updDir+L"update.bat";
        std::wstring bat;
        bat+=L"@chcp 65001 >nul\r\n";
        bat+=L"@echo off\r\n";
        bat+=L"echo [CloudBoard Updater]\r\n";
        bat+=L"echo Waiting for old app to close...\r\n";
        bat+=L":wait\r\n";
        bat+=L"ping -n 2 127.0.0.1 >nul\r\n";
        bat+=L"tasklist /fi \"IMAGENAME eq cloudboard.exe\" 2>nul | find /i \"cloudboard.exe\" >nul\r\n";
        bat+=L"if not errorlevel 1 goto wait\r\n";
        bat+=L"echo Extracting update package...\r\n";
        bat+=L"powershell -NoProfile -ExecutionPolicy Bypass -Command \"Expand-Archive -Force -Path '"+zipPath+L"' -DestinationPath '"+appDir+L"'\"\r\n";
        bat+=L"if errorlevel 1 exit /b 1\r\n";
        bat+=L"echo Starting CloudBoard...\r\n";
        bat+=L"start \"\" \""+appDir+L"cloudboard.exe\"\r\n";
        bat+=L"echo Cleaning up...\r\n";
        bat+=L"del /f /q \""+zipPath+L"\" 2>nul\r\n";
        bat+=L"del /f /q \""+batPath+L"\"\r\n";
        bat+=L"exit\r\n";

        HANDLE hb=CreateFileW(batPath.c_str(),GENERIC_WRITE,0,nullptr,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
        if (hb!=INVALID_HANDLE_VALUE){
            DWORD wr=0;
            // Write UTF-8 BOM first so cmd.exe on CJK Windows interprets the file correctly
            unsigned char bom[]={0xEF,0xBB,0xBF};
            WriteFile(hb,bom,3,&wr,nullptr);
            int u8len=WideCharToMultiByte(CP_UTF8,0,bat.c_str(),(int)bat.size(),nullptr,0,nullptr,nullptr);
            std::string nb(u8len,'\0');
            WideCharToMultiByte(CP_UTF8,0,bat.c_str(),(int)bat.size(),&nb[0],u8len,nullptr,nullptr);
            WriteFile(hb,nb.c_str(),(DWORD)nb.size(),&wr,nullptr);
            CloseHandle(hb);
        }
        g.loadingProgress.store(100);
        // Launch updater batch first (it waits for old process), then close app
        ShellExecuteW(nullptr, L"open", batPath.c_str(), nullptr, nullptr, SW_HIDE);
        Sleep(300);
        g.silentExit = true;
        PostMessageW(g.hWnd, WM_CLOSE, 0, 0);
    }
    return 0;
}

void CheckUpdate(){
    g.loadingActive = true;
    g.loadingCancelled.store(false);
    g.loadingProgress = 0;
    g.loadingTotal = 100;
    g.loadingText = L"正在访问服务器...";

    // Start progress animation timer (10%/sec to 97%)
    SetTimer(g.hWnd, TIMER_UPDATE_PROG, 100, nullptr);

    // Spawn background thread for HTTP + download + install
    CloseHandle(CreateThread(nullptr, 0, CheckUpdateThread, nullptr, 0, nullptr));

    InvalidateRect(g.hWnd, nullptr, FALSE);
}

// Global instance
AppState g;

static const wchar_t* kClassName = L"CloudBoard.Main";

// ====== PDF file association ======
static void RegisterPdfAssociation(){
    wchar_t exePath[MAX_PATH];
    GetModuleFileNameW(nullptr, exePath, MAX_PATH);

    HKEY hKey;
    const wchar_t* progId = L"CloudBoard.pdf";

    // Register ProgID
    std::wstring keyPath = L"Software\\Classes\\";
    keyPath += progId;
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        const wchar_t* desc = L"CloudBoard PDF Document";
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)desc, (DWORD)((wcslen(desc) + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    // DefaultIcon
    keyPath = L"Software\\Classes\\";
    keyPath += progId;
    keyPath += L"\\DefaultIcon";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        std::wstring iconVal = std::wstring(exePath) + L",0";
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)iconVal.c_str(), (DWORD)((iconVal.size() + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    // shell\open\command
    keyPath = L"Software\\Classes\\";
    keyPath += progId;
    keyPath += L"\\shell\\open\\command";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        std::wstring cmdVal = std::wstring(L"\"") + exePath + L"\" \"%1\"";
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)cmdVal.c_str(), (DWORD)((cmdVal.size() + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    // Register as OpenWith option for .pdf
    keyPath = L"Software\\Classes\\.pdf\\OpenWithProgids";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        RegSetValueExW(hKey, progId, 0, REG_NONE, nullptr, 0);
        RegCloseKey(hKey);
    }
}

static void RegisterCloudboardAssociation(){
    wchar_t exePath[MAX_PATH];
    GetModuleFileNameW(nullptr, exePath, MAX_PATH);

    HKEY hKey;
    const wchar_t* progId = L"CloudBoard.cloudboard";

    std::wstring keyPath = L"Software\\Classes\\";
    keyPath += progId;
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        const wchar_t* desc = L"CloudBoard Project";
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)desc, (DWORD)((wcslen(desc) + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    keyPath = L"Software\\Classes\\";
    keyPath += progId;
    keyPath += L"\\DefaultIcon";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        std::wstring iconVal = std::wstring(exePath) + L",0";
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)iconVal.c_str(), (DWORD)((iconVal.size() + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    keyPath = L"Software\\Classes\\";
    keyPath += progId;
    keyPath += L"\\shell\\open\\command";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        std::wstring cmdVal = std::wstring(L"\"") + exePath + L"\" \"%1\"";
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)cmdVal.c_str(), (DWORD)((cmdVal.size() + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    keyPath = L"Software\\Classes\\.cloudboard";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        RegSetValueExW(hKey, nullptr, 0, REG_SZ,
            (const BYTE*)progId, (DWORD)((wcslen(progId) + 1) * sizeof(wchar_t)));
        RegCloseKey(hKey);
    }

    keyPath = L"Software\\Classes\\.cloudboard\\OpenWithProgids";
    if (RegCreateKeyExW(HKEY_CURRENT_USER, keyPath.c_str(), 0, nullptr,
            0, KEY_WRITE, nullptr, &hKey, nullptr) == ERROR_SUCCESS){
        RegSetValueExW(hKey, progId, 0, REG_NONE, nullptr, 0);
        RegCloseKey(hKey);
    }
    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, nullptr, nullptr);
}

// ====== Splash screen (GDI+) ======
static HWND g_hSplash = nullptr;
static const wchar_t* kSplashClass = L"CloudBoard.Splash";
static std::wstring g_splashFontPath;
static DWORD g_splashAnimStart = 0;
static const wchar_t* g_splashText = L"";
static constexpr UINT_PTR SPLASH_TIMER = 1;
static int g_splashW = 0, g_splashH = 0;
static HBITMAP g_splashBgBmp = nullptr;
static HDC g_splashBgDC = nullptr;
static int g_splashBgW = 0, g_splashBgH = 0;

static LRESULT CALLBACK SplashWndProc(HWND, UINT, WPARAM, LPARAM);
static void SplashUpdateText(const wchar_t* text);

static void SplashReleaseBg(){
    if (g_splashBgBmp){ DeleteObject(g_splashBgBmp); g_splashBgBmp = nullptr; }
    if (g_splashBgDC){ DeleteDC(g_splashBgDC); g_splashBgDC = nullptr; }
    g_splashBgW = 0;
    g_splashBgH = 0;
}

static void SplashCreate(HINSTANCE hInst){
    WNDCLASSW wc{};
    wc.lpfnWndProc = SplashWndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(NULL_BRUSH);
    wc.lpszClassName = kSplashClass;
    RegisterClassW(&wc);

    HMONITOR hMon = MonitorFromPoint({0,0}, MONITOR_DEFAULTTOPRIMARY);
    UINT dpiX=96, dpiY=96;
    GetDpiForMonitor(hMon, MDT_EFFECTIVE_DPI, &dpiX, &dpiY);
    float splashScale = dpiX / 96.0f;

    int sw = GetSystemMetrics(SM_CXSCREEN), sh = GetSystemMetrics(SM_CYSCREEN);
    g_splashW = (int)(420 * splashScale); g_splashH = (int)(220 * splashScale);

    g_hSplash = CreateWindowExW(WS_EX_TOPMOST, kSplashClass, L"",
        WS_POPUP | WS_VISIBLE, (sw-g_splashW)/2, (sh-g_splashH)/2,
        g_splashW, g_splashH, nullptr, nullptr, hInst, nullptr);

    if (g_hSplash){
        int cornerR = (int)(16*splashScale);
        HRGN hRgn = CreateRoundRectRgn(0,0,g_splashW,g_splashH,cornerR*2,cornerR*2);
        SetWindowRgn(g_hSplash, hRgn, TRUE);
        g_splashAnimStart = GetTickCount();
        SetTimer(g_hSplash, SPLASH_TIMER, 16, nullptr);
        ShowWindow(g_hSplash, SW_SHOW);
        UpdateWindow(g_hSplash);
    }
}

static float EaseOutCubic(float t){ float t1 = t-1.f; return t1*t1*t1+1.f; }

static LRESULT CALLBACK SplashWndProc(HWND hWnd, UINT msg, WPARAM wp, LPARAM lp){
    switch(msg){
    case WM_TIMER:
        if (wp == SPLASH_TIMER){
            if (GetTickCount() - g_splashAnimStart < 500)
                InvalidateRect(hWnd, nullptr, FALSE);
            else KillTimer(hWnd, SPLASH_TIMER);
        } return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps; HDC hdc = BeginPaint(hWnd, &ps);
        RECT rc; GetClientRect(hWnd, &rc);
        int W = rc.right, H = rc.bottom;
        if (W <= 0 || H <= 0){ EndPaint(hWnd, &ps); return 0; }

        // Double buffer
        HDC memDC = CreateCompatibleDC(hdc);
        HBITMAP memBmp = CreateCompatibleBitmap(hdc, W, H);
        HGDIOBJ oldBmp = SelectObject(memDC, memBmp);

        float scale = (float)W / 420.0f;

        if (!g_splashBgBmp || g_splashBgW != W || g_splashBgH != H){
            SplashReleaseBg();
            g_splashBgDC = CreateCompatibleDC(hdc);
            g_splashBgBmp = CreateCompatibleBitmap(hdc, W, H);
            HGDIOBJ oldBg = SelectObject(g_splashBgDC, g_splashBgBmp);
            Graphics bgGr(g_splashBgDC);
            bgGr.SetSmoothingMode(SmoothingModeAntiAlias);

            SolidBrush bgBrush(Color(255, 33, 87, 173));
            bgGr.FillRectangle(&bgBrush, 0, 0, W, H);

            // Geometric decorations (cached because startup can be CPU busy).
            Color pal[] = {
                Color(64,  60, 130, 220), Color(76,  20,  60, 150),
                Color(51,  80, 160, 240), Color(38,  40,  90, 180),
                Color(89, 100, 180, 255), Color(51,  70, 140, 200),
            };
            struct Shape { float x,y,w,h,angle; int type,pal; };
            Shape shapes[] = {
                {0.10f,0.15f,0.12f,0.12f,15.f,0,0},{0.85f,0.10f,0.08f,0.08f,0.f,1,1},
                {0.75f,0.70f,0.10f,0.10f,30.f,2,2},{0.20f,0.80f,0.07f,0.07f,45.f,0,3},
                {0.50f,0.05f,0.06f,0.06f,0.f,1,4},{0.90f,0.45f,0.09f,0.09f,20.f,0,5},
                {0.05f,0.50f,0.08f,0.08f,60.f,2,0},{0.60f,0.85f,0.06f,0.06f,0.f,1,1},
                {0.35f,0.25f,0.05f,0.05f,10.f,0,2},{0.15f,0.55f,0.04f,0.04f,0.f,1,3},
                {0.80f,0.30f,0.07f,0.07f,40.f,2,4},{0.45f,0.60f,0.05f,0.05f,0.f,0,5},
                {0.70f,0.15f,0.04f,0.04f,25.f,1,0},{0.30f,0.90f,0.08f,0.08f,35.f,2,1},
                {0.92f,0.85f,0.05f,0.05f,0.f,0,2},
            };
            for (auto& s : shapes){
                float cx = s.x*W, cy = s.y*H, sw2 = s.w*W, sh2 = s.h*H;
                SolidBrush sb(pal[s.pal]);
                if (s.type == 0){
                    bgGr.FillEllipse(&sb, cx - sw2/2, cy - sh2/2, sw2, sh2);
                } else if (s.type == 1){
                    float rr = std::min(sw2,sh2)*0.2f;
                    GraphicsPath path; path.AddArc(cx-sw2/2, cy-sh2/2, rr*2, rr*2, 180, 90);
                    path.AddArc(cx+sw2/2-rr*2, cy-sh2/2, rr*2, rr*2, 270, 90);
                    path.AddArc(cx+sw2/2-rr*2, cy+sh2/2-rr*2, rr*2, rr*2, 0, 90);
                    path.AddArc(cx-sw2/2, cy+sh2/2-rr*2, rr*2, rr*2, 90, 90);
                    path.CloseFigure();
                    bgGr.FillPath(&sb, &path);
                } else {
                    float a = s.angle*3.14159f/180.f, ca=cosf(a), sa=sinf(a);
                    PointF pts[3] = {
                        {cx - sw2/2*ca - (-sh2/2)*sa, cy - sw2/2*sa + (-sh2/2)*ca},
                        {cx + (-sw2/2)*ca - (sh2/2)*sa, cy + (-sw2/2)*sa + (sh2/2)*ca},
                        {cx + (sw2/2)*ca - (sh2/2)*sa, cy + (sw2/2)*sa + (sh2/2)*ca},
                    };
                    bgGr.FillPolygon(&sb, pts, 3);
                }
            }
            bgGr.Flush();
            SelectObject(g_splashBgDC, oldBg);
            g_splashBgW = W;
            g_splashBgH = H;
        }
        BitBlt(memDC, 0, 0, W, H, g_splashBgDC, 0, 0, SRCCOPY);

        Graphics gr(memDC);
        gr.SetSmoothingMode(SmoothingModeAntiAlias);
        gr.SetTextRenderingHint(TextRenderingHintAntiAlias);

        // Title animation
        float animT = std::min(1.0f, (GetTickCount()-g_splashAnimStart)/500.f);
        float ease = EaseOutCubic(animT);
        float titleTargetX = 36*scale, titleTargetY = 65*scale;
        float titleX = -120*scale + (titleTargetX + 120*scale) * ease;
        int textAlpha = (int)(255 * ease);
        if (textAlpha < 0) textAlpha = 0;

        {
            FontFamily ff(L"Anthropic Sans Text");
            Font titleFont(&ff, 24*scale, FontStyleBold, UnitPixel);
            SolidBrush titleBr(Color(textAlpha, 255, 255, 255));
            gr.DrawString(L"Cloudboard 26", -1, &titleFont,
                PointF(titleX, titleTargetY), &titleBr);
        }

        // Version
        {
            FontFamily ff(L"Anthropic Sans Text");
            Font verFont(&ff, 9*scale, FontStyleRegular, UnitPixel);
            SolidBrush verBr(Color(140, 200, 220, 240));
            gr.DrawString(L"v" APP_VERSION, -1, &verFont,
                PointF(36*scale, (float)(H - 32*scale)), &verBr);
        }

        // Progress text - right-aligned at bottom
        if (g_splashText && g_splashText[0]){
            FontFamily ff(L"Anthropic Sans Text");
            Font progFont(&ff, 10*scale, FontStyleRegular, UnitPixel);
            SolidBrush progBr(Color(textAlpha, 255, 255, 255));
            StringFormat sf; sf.SetAlignment(StringAlignmentFar);
            RectF layoutRc((float)W - 210*scale, (float)H - 40*scale, 200*scale, 15*scale);
            gr.DrawString(g_splashText, -1, &progFont, layoutRc, &sf, &progBr);
        }

        BitBlt(hdc, 0, 0, W, H, memDC, 0, 0, SRCCOPY);
        SelectObject(memDC, oldBmp);
        DeleteObject(memBmp);
        DeleteDC(memDC);
        EndPaint(hWnd, &ps);
        return 0;
    }
    case WM_DESTROY: KillTimer(hWnd, SPLASH_TIMER); break;
    } return DefWindowProcW(hWnd, msg, wp, lp);
}

static void SplashUpdateText(const wchar_t* text){
    g_splashText = text;
    if (g_hSplash) InvalidateRect(g_hSplash, nullptr, FALSE);
}

static void SplashDestroy(){
    if (g_hSplash){
        DestroyWindow(g_hSplash);
        g_hSplash = nullptr;
    }
    SplashReleaseBg();
    UnregisterClassW(kSplashClass, g.hInst);
}

// Touch input handler
static void HandleTouch(HWND hWnd, WPARAM wp, LPARAM lp){
    UINT cTouch = LOWORD(wp);
    if (cTouch == 0) return;
    std::vector<TOUCHINPUT> ti(cTouch);
    if (!GetTouchInputInfo((HTOUCHINPUT)lp, cTouch, ti.data(), sizeof(TOUCHINPUT)))
        return;

    // Three fingers: switch to eraser
    if (cTouch >= 3){
        g.tool = ToolKind::Eraser;
        g.penPopupOpen = false;
        g.eraserPopupOpen = false;
        g.zoomPopupOpen = false;
        g.assistPopupOpen = false;
        g.pinching = false;
        g.lastPinchDist = 0;
        ShowToast(L"三指 -> 橡皮擦");
        InvalidateRect(hWnd, nullptr, FALSE);
        CloseTouchInputHandle((HTOUCHINPUT)lp);
        return;
    }

    // Two fingers: pinch-to-zoom
    if (cTouch == 2){
        POINT pt0 = { ti[0].x / 100, ti[0].y / 100 };
        POINT pt1 = { ti[1].x / 100, ti[1].y / 100 };
        float dx = (float)(pt1.x - pt0.x);
        float dy = (float)(pt1.y - pt0.y);
        float dist = std::sqrt(dx*dx + dy*dy);

        // Teaching aids are currently disabled/hidden; keep the gesture code dormant for later re-enable.
        if (false && g.assist.kind != AssistKind::None){
            POINT c0 = pt0, c1 = pt1;
            ScreenToClient(hWnd, &c0);
            ScreenToClient(hWnd, &c1);
            RECT br2 = BoardRect(hWnd);
            PointF2 b0 = Board::ScreenToBoard(c0.x, c0.y, br2);
            PointF2 b1 = Board::ScreenToBoard(c1.x, c1.y, br2);
            PointF2 mid{ (b0.x + b1.x) * 0.5f, (b0.y + b1.y) * 0.5f };
            float boardDx = b1.x - b0.x, boardDy = b1.y - b0.y;
            float boardDist = std::sqrt(boardDx*boardDx + boardDy*boardDy);
            float boardAngle = std::atan2(boardDy, boardDx);

            if (ti[0].dwFlags & TOUCHEVENTF_DOWN || ti[1].dwFlags & TOUCHEVENTF_DOWN){
                g.pinching = true;
                g.touchContactActive = false;
                g.activeTouchId = 0;
                g.assist.gestureStartDist = std::max(1.0f, boardDist);
                g.assist.gestureStartAngle = boardAngle;
                g.assist.gestureStartMid = mid;
                g.assist.gestureStartTrianglePos = g.assist.trianglePos;
                g.assist.gestureStartTriangleAngle = g.assist.triangleAngle;
                g.assist.gestureStartTriangleScale = g.assist.triangleScale;
                g.assist.gestureStartCompassCenter = g.assist.compassCenter;
                g.assist.gestureStartCompassRadius = g.assist.compassRadius;
                g.assist.gestureStartCompassAngle = g.assist.compassAngle;
                if (g.drawing){ Board::EndStroke(); g.drawing = false; }
                if (g.erasing){ Board::EndErase(); g.erasing = false; g.current.pts.clear(); }
            } else if (g.pinching && (ti[0].dwFlags & TOUCHEVENTF_MOVE || ti[1].dwFlags & TOUCHEVENTF_MOVE)){
                float scale = boardDist / std::max(1.0f, g.assist.gestureStartDist);
                float da = boardAngle - g.assist.gestureStartAngle;
                PointF2 dm{ mid.x - g.assist.gestureStartMid.x, mid.y - g.assist.gestureStartMid.y };
                if (g.assist.kind == AssistKind::Triangle){
                    g.assist.trianglePos = { g.assist.gestureStartTrianglePos.x + dm.x, g.assist.gestureStartTrianglePos.y + dm.y };
                    g.assist.triangleAngle = g.assist.gestureStartTriangleAngle + da;
                    g.assist.triangleScale = std::max(0.45f, std::min(2.4f, g.assist.gestureStartTriangleScale * scale));
                } else if (g.assist.kind == AssistKind::Compass){
                    g.assist.compassCenter = { g.assist.gestureStartCompassCenter.x + dm.x, g.assist.gestureStartCompassCenter.y + dm.y };
                    g.assist.compassRadius = std::max(30.0f, g.assist.gestureStartCompassRadius * scale);
                    g.assist.compassAngle = g.assist.gestureStartCompassAngle + da;
                }
                InvalidateRect(hWnd, nullptr, FALSE);
            }

            if (ti[0].dwFlags & TOUCHEVENTF_UP || ti[1].dwFlags & TOUCHEVENTF_UP){
                g.pinching = false;
                SaveSettings();
            }
            CloseTouchInputHandle((HTOUCHINPUT)lp);
            return;
        }

        if (ti[0].dwFlags & TOUCHEVENTF_DOWN || ti[1].dwFlags & TOUCHEVENTF_DOWN){
            g.pinching = true;
            g.touchContactActive = false;
            g.activeTouchId = 0;
            g.lastPinchDist = dist;
            // Cancel any active drawing
            if (g.drawing){ Board::EndStroke(); g.drawing = false; }
            if (g.erasing){ Board::EndErase(); g.erasing = false; g.current.pts.clear(); }
            if (g.panning) g.panning = false;
        } else if (g.pinching && (ti[0].dwFlags & TOUCHEVENTF_MOVE || ti[1].dwFlags & TOUCHEVENTF_MOVE)){
            if (g.lastPinchDist > 1.0f){
                float ratio = dist / g.lastPinchDist;
                float oldZoom = g.zoomLevel;
                float newZoom = g.zoomLevel * ratio;
                // Clamp zoom range
                if (newZoom < kZoomMin) newZoom = kZoomMin;
                if (newZoom > kZoomMax) newZoom = kZoomMax;

                // Zoom centered on midpoint between two fingers
                ScreenToClient(hWnd, &pt0);
                ScreenToClient(hWnd, &pt1);
                int mx = (pt0.x + pt1.x) / 2;
                int my = (pt0.y + pt1.y) / 2;
                RECT br2 = BoardRect(hWnd);
                // Screen point relative to board rect
                float srx = (float)(mx - br2.left);
                float sry = (float)(my - br2.top);
                // Board coord under the midpoint
                float boardX = (srx + g.scrollX) / oldZoom;
                float boardY = (sry + g.scrollY) / oldZoom;
                // After zoom, the board coord should still map to the same screen point
                g.scrollX = boardX * newZoom - srx;
                g.scrollY = boardY * newZoom - sry;
                g.zoomLevel = newZoom;

                // PDF mode: clamp scroll
                if (g.pdfMode) Board::ClampPdfScroll();

                g.stableW = 0; // force stable bitmap rebuild
                InvalidateRect(hWnd, nullptr, FALSE);
            }
            g.lastPinchDist = dist;
        }

        if (ti[0].dwFlags & TOUCHEVENTF_UP || ti[1].dwFlags & TOUCHEVENTF_UP){
            g.pinching = false;
            g.lastPinchDist = 0;
        }

        CloseTouchInputHandle((HTOUCHINPUT)lp);
        return;
    }

    // Single finger: simulate mouse
    if (g.pinching){
        // If pinching was active and we're back to 1 finger, ignore
        CloseTouchInputHandle((HTOUCHINPUT)lp);
        return;
    }
    POINT pt = { ti[0].x / 100, ti[0].y / 100 };
    DWORD touchFlags = ti[0].dwFlags;
    DWORD touchId = ti[0].dwID;
    ScreenToClient(hWnd, &pt);
    CloseTouchInputHandle((HTOUCHINPUT)lp);

    if (touchFlags & TOUCHEVENTF_DOWN){
        if (g.touchContactActive && g.activeTouchId != touchId){
            if (g.drawing) Board::EndStroke();
            if (g.erasing){ Board::EndErase(); g.current.pts.clear(); }
        }
        g.touchContactActive = true;
        g.activeTouchId = touchId;
        g.lastTouchTick = GetTickCount();
        UI::HandleMouseDown(hWnd, pt.x, pt.y);
    } else if (touchFlags & TOUCHEVENTF_MOVE){
        if (g.touchContactActive && g.activeTouchId != touchId) return;
        g.lastTouchTick = GetTickCount();
        UI::HandleMouseMove(hWnd, pt.x, pt.y, true);
    } else if (touchFlags & TOUCHEVENTF_UP){
        if (g.touchContactActive && g.activeTouchId != touchId) return;
        g.lastTouchTick = GetTickCount();
        UI::HandleMouseUp(hWnd, pt.x, pt.y);
        g.touchContactActive = false;
        g.activeTouchId = 0;
    }
}

// File drop
static void HandleDrop(HWND hWnd, HDROP hDrop){
    UINT n = DragQueryFileW(hDrop, 0xFFFFFFFF, nullptr, 0);
    POINT pt;
    DragQueryPoint(hDrop, &pt);
    RECT br = BoardRect(hWnd);
    PointF2 bp = Board::ScreenToBoard(pt.x, pt.y, br);
    for (UINT i = 0; i < n; ++i){
        wchar_t buf[MAX_PATH];
        DragQueryFileW(hDrop, i, buf, MAX_PATH);
        std::wstring path = buf;
        size_t dot = path.find_last_of(L'.');
        if (dot != std::wstring::npos){
            std::wstring ext = path.substr(dot+1);
            for (auto& c : ext) c = (wchar_t)towlower(c);
            if (ext == L"png" || ext == L"jpg" || ext == L"jpeg" ||
                ext == L"bmp" || ext == L"gif"){
                Board::AddImageFromFile(path, bp.x + i*30, bp.y + i*30);
            }
        }
    }
    DragFinish(hDrop);
    InvalidateRect(hWnd, nullptr, FALSE);
}

LRESULT CALLBACK WndProc(HWND hWnd, UINT msg, WPARAM wp, LPARAM lp){
    switch (msg){
    case WM_CREATE: {
        RegisterTouchWindow(hWnd, 0);
        DragAcceptFiles(hWnd, TRUE);
        // DPI
        g.dpiScale = GetDpiForWindow(hWnd) / 96.0f;

        // Font already loaded in wWinMain (before splash screen)

        // Load application icon from resources
        {
            HICON hResIcon = LoadIcon(g.hInst, MAKEINTRESOURCE(101));
            if (hResIcon){
                g.hAppIcon = hResIcon;
                SendMessage(hWnd, WM_SETICON, ICON_BIG, (LPARAM)hResIcon);
                SendMessage(hWnd, WM_SETICON, ICON_SMALL, (LPARAM)hResIcon);
            }
        }
        // Fallback: generate application icon if resource not available
        if (!g.hAppIcon)
        {
            int icoSz = 64;
            Bitmap bmp(icoSz, icoSz, PixelFormat32bppARGB);
            Graphics gr(&bmp);
            gr.SetSmoothingMode(SmoothingModeAntiAlias);

            // Background: rounded dark rect
            GraphicsPath bgPath;
            float fr = 14.0f;
            float fsz = (float)icoSz;
            bgPath.AddArc(0.0f, 0.0f, fr*2, fr*2, 180.0f, 90.0f);
            bgPath.AddArc(fsz - fr*2, 0.0f, fr*2, fr*2, 270.0f, 90.0f);
            bgPath.AddArc(fsz - fr*2, fsz - fr*2, fr*2, fr*2, 0.0f, 90.0f);
            bgPath.AddArc(0.0f, fsz - fr*2, fr*2, fr*2, 90.0f, 90.0f);
            bgPath.CloseFigure();
            LinearGradientBrush bgBr(PointF(0.0f,0.0f), PointF(fsz, fsz),
                Color(255, 30, 36, 54), Color(255, 24, 28, 40));
            gr.FillPath(&bgBr, &bgPath);

            // White cloud (Lucide-inspired arc design, scaled to 64px icon)
            {
                float cx = icoSz / 2.0f, cy = icoSz / 2.0f;
                // Map 24x24 grid to 64px icon with padding
                float s = 2.4f;  // scale factor: 64/24 鈮?2.67, use 2.4 for slight padding
                auto PX = [&](float gx) -> float { return cx + (gx - 12.0f) * s; };
                auto PY = [&](float gy) -> float { return cy + (gy - 12.0f) * s; };

                GraphicsPath cloud;
                cloud.StartFigure();
                cloud.AddLine(PointF(PX(17.5f), PY(19.0f)), PointF(PX(9.0f), PY(19.0f)));
                cloud.AddArc(PX(2), PY(5), 14.0f*s, 14.0f*s, 90.0f, 253.4f);
                cloud.AddLine(PointF(PX(15.71f), PY(10.0f)), PointF(PX(17.5f), PY(10.0f)));
                cloud.AddArc(PX(13), PY(10), 9.0f*s, 9.0f*s, 270.0f, 180.0f);
                cloud.CloseFigure();
                SolidBrush whiteBr(Color(255, 255, 255, 255));
                gr.FillPath(&whiteBr, &cloud);
            }

            // Convert Bitmap -> HICON
            BITMAP bm; bmp.GetHBITMAP(Color(0,0,0,0), (HBITMAP*)&bm.bmBits);
            HBITMAP hBmp = nullptr;
            bmp.GetHBITMAP(Color(0,0,0,0), &hBmp);

            ICONINFO ii{};
            ii.fIcon = TRUE;
            ii.hbmMask = CreateBitmap(icoSz, icoSz, 1, 32, nullptr);
            ii.hbmColor = hBmp;
            g.hAppIcon = CreateIconIndirect(&ii);
            if (g.hAppIcon){
                SendMessage(hWnd, WM_SETICON, ICON_BIG, (LPARAM)g.hAppIcon);
                SendMessage(hWnd, WM_SETICON, ICON_SMALL, (LPARAM)g.hAppIcon);
            }
            DeleteObject(hBmp);
            DeleteObject(ii.hbmMask);
        }

        // Create parallel render thread pool
        if (!g.renderQueue)
            g.renderQueue = RenderQueue_Create(hWnd);

        // Initialize OpenGL for board rendering
        g.glInitialized = GLInit();

        return 0;
    }

    case WM_DPICHANGED: {
        g.dpiScale = HIWORD(wp) / 96.0f;
        RECT* rc = (RECT*)lp;
        SetWindowPos(hWnd, nullptr, rc->left, rc->top,
            rc->right - rc->left, rc->bottom - rc->top,
            SWP_NOZORDER | SWP_NOACTIVATE);
        InvalidateRect(hWnd, nullptr, FALSE);
        return 0;
    }

    case WM_SIZE:
        if (g.pdfMode) Board::RenderPdfPages();
        InvalidateRect(hWnd, nullptr, FALSE);
        return 0;

    case WM_SETTINGCHANGE:
        if (lp && wcscmp((const wchar_t*)lp, L"ImmersiveColorSet") == 0) {
            ResolveDarkMode();
            InvalidateRect(hWnd, nullptr, FALSE);
        }
        return 0;

    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC hdc = BeginPaint(hWnd, &ps);
        UI::DrawAll(hdc, hWnd);
        EndPaint(hWnd, &ps);
        return 0;
    }

    case WM_ERASEBKGND:
        return 1;

    case WM_LBUTTONDOWN: {
        // Skip if a touch event just fired (dedup for touch screens)
        if (GetTickCount() - g.lastTouchTick < 200) return 0;
        SetCapture(hWnd);
        int x = GET_X_LPARAM(lp), y = GET_Y_LPARAM(lp);
        UI::HandleMouseDown(hWnd, x, y);
        return 0;
    }
    case WM_MOUSEMOVE: {
        if (GetTickCount() - g.lastTouchTick < 200) return 0;
        int x = GET_X_LPARAM(lp), y = GET_Y_LPARAM(lp);
        UI::HandleMouseMove(hWnd, x, y, (wp & MK_LBUTTON) != 0);
        return 0;
    }
    case WM_LBUTTONUP: {
        if (GetTickCount() - g.lastTouchTick < 200){ ReleaseCapture(); return 0; }
        int x = GET_X_LPARAM(lp), y = GET_Y_LPARAM(lp);
        UI::HandleMouseUp(hWnd, x, y);
        ReleaseCapture();
        return 0;
    }
    case WM_RBUTTONDOWN: {
        int rx = GET_X_LPARAM(lp), ry = GET_Y_LPARAM(lp);
        if (g.tool == ToolKind::Select){
            RECT br = BoardRect(hWnd);
            if (PtInRect2(br, rx, ry)){
                PointF2 bp = Board::ScreenToBoard(rx, ry, br);
                if (g.selectedItems.empty() || !Board::HitSelected(bp.x, bp.y)){
                    int ti = Board::HitTextAt(bp.x, bp.y);
                    int ii = Board::HitImageAt(bp.x, bp.y);
                    g.selectedItems.clear();
                    if (ti >= 0){
                        g.selectedItems.push_back({SelectedItem::Text, ti, g.currentPage});
                    } else if (ii >= 0){
                        g.selectedItems.push_back({SelectedItem::Image, ii, g.currentPage});
                    } else {
                        float tol = S(8) / std::max(0.25f, g.zoomLevel);
                        Board::SelectItemsInRect(bp.x - tol, bp.y - tol, bp.x + tol, bp.y + tol);
                    }
                }
            }
        }
        if (!g.selectedItems.empty()){
            // Show selection context menu at right-click position
            g.selPopupOpen = true;
            g.selPopupX = rx;
            g.selPopupY = ry;
            g.penPopupOpen = false;
            g.eraserPopupOpen = false;
            g.zoomPopupOpen = false;
            g.assistPopupOpen = false;
            InvalidateRect(hWnd, nullptr, FALSE);
            return 0;
        }
        return 0;
    }

    case WM_CAPTURECHANGED:
    case WM_CANCELMODE:
        if (g.drawing){
            Board::EndStroke();
            InvalidateRect(hWnd, nullptr, FALSE);
        }
        if (g.erasing){
            Board::EndErase();
            g.current.pts.clear();
            InvalidateRect(hWnd, nullptr, FALSE);
        }
        g.touchContactActive = false;
        g.activeTouchId = 0;
        return 0;

    case WM_MOUSEWHEEL: {
        int delta = GET_WHEEL_DELTA_WPARAM(wp);
        POINT pt = { GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
        ScreenToClient(hWnd, &pt);
        RECT pb = BottomBarRect(hWnd);
        BOOL shift = GetKeyState(VK_SHIFT) & 0x8000;

        // File browser scroll
        if (g.fileBrowser.active){
            g.fileBrowser.scrollY -= delta;
            int itemH = S(36);
            int totalH = (int)g.fileBrowser.entries.size() * itemH;
            RECT br2 = BoardRect(hWnd);
            int boardH = br2.bottom - br2.top;
            int panelH = std::min(S(560), boardH - S(20));
            int listH = panelH - S(36) - S(38) - S(26);
            int maxScroll = std::max(0, totalH - listH);
            if (g.fileBrowser.scrollY < 0) g.fileBrowser.scrollY = 0;
            if (g.fileBrowser.scrollY > maxScroll) g.fileBrowser.scrollY = maxScroll;
            InvalidateRect(hWnd, nullptr, FALSE);
            return 0;
        }

        if (PtInRect2(pb, pt.x, pt.y)){
            // Bottom bar: ignored (no sidebar to scroll)
        } else if (g.tool == ToolKind::Pan || g.pdfMode) {
            if (shift && !g.pdfMode) {
                // Shift+wheel: horizontal scroll (non-PDF only)
                g.scrollX -= (float)delta;
            } else {
                // Normal: vertical scroll
                g.scrollY -= (float)delta;
            }
            // PDF mode: treat wheel scroll like panning for performance
            if (g.pdfMode){
                g.wheelPanning = true;
                Board::ClampPdfScroll();
                // Restart scroll-idle timer - reset wheelPanning after 300ms of inactivity
                SetTimer(hWnd, TIMER_SCROLL_IDLE, 300, nullptr);
            }
        } else {
            if (delta > 0) Board::PrevPage(); else Board::NextPage();
        }
        InvalidateRect(hWnd, nullptr, FALSE);
        return 0;
    }

    case WM_KEYDOWN: {
        BOOL ctrl = GetKeyState(VK_CONTROL) & 0x8000;
        switch (wp){
        case 'Z': if (ctrl){ Board::Undo(); InvalidateRect(hWnd,nullptr,FALSE);} break;
        case 'Y': if (ctrl){ Board::Redo(); InvalidateRect(hWnd,nullptr,FALSE);} break;
        case 'S': if (ctrl){ g.exitAfterProjectSave = false; ProjectIO::BeginSaveCloudboard(hWnd); } break;
        case 'C': if (ctrl && !g.selectedItems.empty()){ Board::CopySelected(); ShowToast(L"已复制"); } break;
        case VK_LEFT: case VK_PRIOR:
            if (g.pdfMode){ g.scrollY -= 300; Board::ClampPdfScroll(); }
            else Board::PrevPage();
            InvalidateRect(hWnd,nullptr,FALSE); break;
        case VK_RIGHT: case VK_NEXT:
            if (g.pdfMode){ g.scrollY += 300; Board::ClampPdfScroll(); }
            else Board::NextPage();
            InvalidateRect(hWnd,nullptr,FALSE); break;
        case VK_ESCAPE:
            if (g.confirm.active){
                g.confirm.active = false;
            }
            if (g.settingsOpen){
                g.settingsOpen = false;
                InvalidateRect(hWnd, nullptr, FALSE);
                break;
            }
            if (g.fileBrowser.active){
                g.fileBrowser.active = false;
                InvalidateRect(hWnd, nullptr, FALSE);
                break;
            }
            if (g.penPopupOpen || g.eraserPopupOpen || g.zoomPopupOpen || g.assistPopupOpen){
                g.penPopupOpen = false;
                g.eraserPopupOpen = false;
                g.zoomPopupOpen = false;
                g.assistPopupOpen = false;
            }
            if (g.hEdit) Board::CommitTextEdit();
            InvalidateRect(hWnd, nullptr, FALSE);
            break;
        case VK_DELETE:
            if (!g.selectedItems.empty()){
                Board::DeleteSelected();
                InvalidateRect(hWnd,nullptr,FALSE);
            }
            break;
        case '1': g.tool = ToolKind::Pan;    InvalidateRect(hWnd,nullptr,FALSE); break;
        case '2': g.tool = ToolKind::Pen;    InvalidateRect(hWnd,nullptr,FALSE); break;
        case '3': g.tool = ToolKind::Eraser; InvalidateRect(hWnd,nullptr,FALSE); break;
        case '4': g.tool = ToolKind::Select; InvalidateRect(hWnd,nullptr,FALSE); break;
        case 'T': {
            // Quick add text
            RECT br2 = BoardRect(hWnd);
            Board::AddText((br2.right-br2.left)*0.5f + g.scrollX,
                           (br2.bottom-br2.top)*0.5f + g.scrollY);
            InvalidateRect(hWnd,nullptr,FALSE);
            break;
        }
        }
        return 0;
    }

    case WM_TIMER: {
        if (wp == TIMER_COUNTDOWN){
            if (g.countdownSec > 0){
                g.countdownSec--;
                UI::UpdateTimerDisplay();
                if (g.countdownSec == 0){
                    KillTimer(hWnd, TIMER_COUNTDOWN);
                    g.countdownRunning = false;
                    MessageBeep(MB_ICONEXCLAMATION);
                    ShowToast(L"倒计时结束");
                }
            }
            if (g.stopwatchRunning){
                g.stopwatchSec++;
                UI::UpdateTimerDisplay();
            }
        } else if (wp == TIMER_HINT_HIDE){
            KillTimer(hWnd, TIMER_HINT_HIDE);
            InvalidateRect(hWnd, nullptr, FALSE);
        } else if (wp == TIMER_SCROLL_IDLE){
            KillTimer(hWnd, TIMER_SCROLL_IDLE);
            if (g.wheelPanning){
                g.wheelPanning = false;
                InvalidateRect(hWnd, nullptr, FALSE); // trigger full redraw with annotations
            }
        } else if (wp == TIMER_UPDATE_PROG){
            // Smooth progress animation for update check: advance 10%/sec to 97%
            if (g.loadingProgress < 97){
                g.loadingProgress++;
                InvalidateRect(hWnd, nullptr, FALSE);
            }
        } else if (wp == TIMER_INK_REPAINT){
            KillTimer(hWnd, TIMER_INK_REPAINT);
            g.inkRepaintPending = false;
            g.lastInkRepaintTick = GetTickCount();
            InvalidateRect(hWnd, nullptr, FALSE);
        }
        return 0;
    }

    case WM_COMMAND: {
        int id = LOWORD(wp);
        if (HIWORD(wp) == EN_CHANGE && (HWND)lp == g.hEdit)
            return 0;
        if (id == 0 && (HWND)lp == g.hEdit){
            Board::CommitTextEdit();
            return 0;
        }
        UI::OnCommand(hWnd, id);
        return 0;
    }

    case WM_TOUCH:
        HandleTouch(hWnd, wp, lp);
        return 0;

    case WM_DROPFILES:
        HandleDrop(hWnd, (HDROP)wp);
        return 0;

    case WM_APP_RENDER_DONE: {
        auto* result = reinterpret_cast<RenderResult*>(lp);
        if (result){
            if (result->pageIdx >= 0 && result->pageIdx < (int)g.pages.size() && result->bitmap){
                // Reject full-res results for pages far from viewport center
                // (these were evicted to save memory and should stay evicted)
                int dist = abs(result->pageIdx - g.pdfViewCenter);
                if (!result->isThumbnail && dist > 20){
                    // Discard - page is too far from current viewport
                } else if (result->isThumbnail){
                    if (g.thumbDirty[result->pageIdx] || !g.thumbCache[result->pageIdx]){
                    // Discard - page is too far from current viewport
                        g.thumbDirty[result->pageIdx] = false;
                    }
                } else if (g.pdfMode){
                    // Full-res PDF page: write back into the page image used by the board renderer.
                    if (!g.pages[result->pageIdx].images.empty()){
                        g.pages[result->pageIdx].images[0].bmp = result->bitmap;
                        g.thumbDirty[result->pageIdx] = true;
                    }
                } else {
                    // Full-res background: only set if not already rendered (non-PDF only)
                    if (!g.pages[result->pageIdx].bgImage){
                        g.pages[result->pageIdx].bgImage = result->bitmap;
                        g.thumbDirty[result->pageIdx] = true;

                        // Update loading progress
                        if (g.loadingActive && g.loadingProgress < g.loadingTotal){
                            g.loadingProgress++;
                            if (g.loadingProgress >= g.loadingTotal){
                                g.loadingActive = false;
                            }
                        }
                    }
                }
            }
            delete result;
            // Avoid render-done repaint storms while input is active; the next idle/full frame will pick it up.
            if (!g.wheelPanning && !g.panning && !g.drawing && !g.shaping && !g.erasing){
                InvalidateRect(hWnd, nullptr, FALSE);
            }
        }
        return 0;
    }

    case WM_APP_UPDATE_DONE: {
        // Update check completed (called from background thread)
        KillTimer(hWnd, TIMER_UPDATE_PROG);

        // If overlay already dismissed by cancel button, just clean up
        if (!g.loadingActive) return 0;

        g.loadingProgress = g.loadingTotal;  // 100%
        InvalidateRect(hWnd, nullptr, FALSE);

        int result = (int)wp;
        if (result == 0){
            ShowToast(L"已是最新版本 (" APP_VERSION L")");
        } else if (result == 1){
            ShowToast(L"检查更新失败：无法连接服务器");
        }
        // result==2: cancelled (toast already shown by cancel button handler)

        // Clean up loading state after a short delay so user sees 100%
        Sleep(600);
        g.loadingActive = false;
        InvalidateRect(hWnd, nullptr, FALSE);
        return 0;
    }

    case WM_APP_PROJECT_SAVE_DONE: {
        std::unique_ptr<std::wstring> msg((std::wstring*)lp);
        if (msg && !msg->empty()) ShowToast(*msg);
        if (g.exitAfterProjectSave){
            bool ok = (wp != 0);
            g.exitAfterProjectSave = false;
            if (ok){
                g.silentExit = true;
                PostMessageW(hWnd, WM_CLOSE, 0, 0);
            }
        }
        InvalidateRect(hWnd, nullptr, FALSE);
        return 0;
    }

    case WM_SETCURSOR: {
        POINT pt; GetCursorPos(&pt); ScreenToClient(hWnd, &pt);
        RECT br = BoardRect(hWnd);
        if (PtInRect2(br, pt.x, pt.y)){
            if (g.tool == ToolKind::Select){
                SetCursor(LoadCursor(nullptr, IDC_ARROW));
                return TRUE;
            }
            if (g.tool == ToolKind::Pan){
                SetCursor(LoadCursor(nullptr, IDC_SIZEALL));
                return TRUE;
            }
            SetCursor(LoadCursor(nullptr, IDC_CROSS));
            return TRUE;
        }
        return DefWindowProcW(hWnd, msg, wp, lp);
    }

    case WM_CLOSE: {
        if (g.silentExit){
            DestroyWindow(hWnd);
            return 0;
        }
        g.confirm = {};
        g.confirm.active = true;
        g.confirm.message = L"退出前是否保存当前白板？";
        g.confirm.altLabel = L"保存并退出";
        g.confirm.altCmdId = ID_EXIT_SAVE_CONFIRMED;
        g.confirm.thirdLabel = L"不保存退出";
        g.confirm.thirdCmdId = ID_EXIT_DISCARD_CONFIRMED;
        g.confirm.cancelLabel = L"取消";
        InvalidateRect(hWnd, nullptr, FALSE);
        return 0;
    }

    case WM_DESTROY:
        // Save settings before exit
        SaveSettings();
        // Shut down render thread pool FIRST (before deleting PdfDoc)
        if (g.renderQueue){
            RenderQueue_Destroy((RenderQueue*)g.renderQueue);
            g.renderQueue = nullptr;
        }
        // Purge any remaining render-done messages from the queue
        {
            MSG purgeMsg;
            while (PeekMessageW(&purgeMsg, nullptr, WM_APP_RENDER_DONE, WM_APP_RENDER_DONE, PM_REMOVE)){
                auto* rr = reinterpret_cast<RenderResult*>(purgeMsg.lParam);
                delete rr;
            }
        }
        if (g.pdfDoc){
            PdfDoc* pdf = (PdfDoc*)g.pdfDoc;
            delete pdf;
            g.pdfDoc = nullptr;
        }
        if (g.hAppIcon) DestroyIcon(g.hAppIcon);
        // Release OpenGL resources
        GLShutdown();
        if (g.hMemBmp) DeleteObject(g.hMemBmp);
        if (g.hMemDC) DeleteDC(g.hMemDC);
        if (g.hStableBmp) DeleteObject(g.hStableBmp);
        if (g.hStableDC) DeleteDC(g.hStableDC);
        // Unload custom font
        {
            wchar_t fontPath[MAX_PATH];
            GetModuleFileNameW(nullptr, fontPath, MAX_PATH);
            std::wstring fp(fontPath);
            size_t lastSlash = fp.find_last_of(L"\\/");
            if (lastSlash != std::wstring::npos)
                fp = fp.substr(0, lastSlash + 1);
            fp += L"AnthropicSans-Romans-Variable-25x258.ttf";
            RemoveFontResourceExW(fp.c_str(), FR_PRIVATE, 0);
        }
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hWnd, msg, wp, lp);
}

// EDIT subclass: Enter to commit, focus loss to commit
static LRESULT CALLBACK EditSubProc(HWND h, UINT m, WPARAM w, LPARAM l, UINT_PTR, DWORD_PTR){
    if (m == WM_CHAR && w == VK_RETURN){
        if (GetKeyState(VK_SHIFT) & 0x8000){
            // Shift+Enter: allow newline
        } else {
            Board::CommitTextEdit();
            return 0;
        }
    }
    if (m == WM_KILLFOCUS){
        Board::CommitTextEdit();
        return 0;
    }
    return DefSubclassProc(h, m, w, l);
}

// ====== Uninstall support ======
static void DoUninstall() {
    // Find install path from exe location
    wchar_t exePath[MAX_PATH];
    GetModuleFileNameW(nullptr, exePath, MAX_PATH);
    std::wstring dir(exePath);
    size_t slash = dir.find_last_of(L"\\/");
    if (slash != std::wstring::npos) dir = dir.substr(0, slash);

    // MessageBox confirmation (standard, since our GDI+ isn't set up yet)
    int result = MessageBoxW(nullptr,
        L"确定要卸载 CloudBoard 吗？\n\n将删除程序文件、快捷方式和注册表项。",
        L"卸载 CloudBoard",
        MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2);

    // Remove font registration
    std::wstring fontPath = dir + L"\\Fonts\\AnthropicSans-Romans-Variable-25x258.ttf";
    RemoveFontResourceW(fontPath.c_str());
    // Remove from registry
    HKEY hKey;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts",
                      0, KEY_SET_VALUE, &hKey) == ERROR_SUCCESS) {
        RegDeleteValueW(hKey, L"Anthropic Sans Text (TrueType)");
        RegCloseKey(hKey);
    }
    if (RegOpenKeyExW(HKEY_CURRENT_USER, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts",
                      0, KEY_SET_VALUE, &hKey) == ERROR_SUCCESS) {
        RegDeleteValueW(hKey, L"Anthropic Sans Text (TrueType)");
        RegCloseKey(hKey);
    }
    SendMessageW(HWND_BROADCAST, WM_FONTCHANGE, 0, 0);

    // Remove Start Menu shortcuts
    wchar_t startMenu[MAX_PATH];
    if (SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_COMMON_PROGRAMS, nullptr, 0, startMenu))) {
        std::wstring smDir = std::wstring(startMenu) + L"\\CloudBoard";
        SHFILEOPSTRUCTW fos = {};
        fos.wFunc = FO_DELETE;
        wchar_t from[MAX_PATH * 2] = {};
        wcsncpy_s(from, smDir.c_str(), MAX_PATH);
        fos.pFrom = from;
        fos.fFlags = FOF_NOCONFIRMATION | FOF_SILENT;
        SHFileOperationW(&fos);
    }

    // Remove desktop shortcut
    wchar_t desktop[MAX_PATH];
    if (SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_DESKTOP, nullptr, 0, desktop))) {
        std::wstring lnk = std::wstring(desktop) + L"\\CloudBoard.lnk";
        DeleteFileW(lnk.c_str());
    }

    // Remove uninstall registry
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                      0, KEY_SET_VALUE, &hKey) == ERROR_SUCCESS) {
        RegDeleteKeyW(hKey, L"CloudBoard");
        RegCloseKey(hKey);
    }
    if (RegOpenKeyExW(HKEY_CURRENT_USER, L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                      0, KEY_SET_VALUE, &hKey) == ERROR_SUCCESS) {
        RegDeleteKeyW(hKey, L"CloudBoard");
        RegCloseKey(hKey);
    }

    RegDeleteTreeW(HKEY_CURRENT_USER, L"Software\\Classes\\CloudBoard.cloudboard");
    RegDeleteTreeW(HKEY_CURRENT_USER, L"Software\\Classes\\.cloudboard");
    SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, nullptr, nullptr);

    // Schedule deletion of install directory (via batch file, since we're running from it)
    wchar_t tempDir[MAX_PATH];
    GetTempPathW(MAX_PATH, tempDir);
    std::wstring batPath = std::wstring(tempDir) + L"cb_uninstall.bat";
    FILE* f = nullptr;
    if (_wfopen_s(&f, batPath.c_str(), L"w") == 0 && f) {
        fwprintf(f, L"@echo off\n");
        fwprintf(f, L"timeout /t 2 /nobreak >nul\n");
        fwprintf(f, L"rd /s /q \"%s\"\n", dir.c_str());
        fwprintf(f, L"del \"%%~f0\"\n");
        fclose(f);

        // Run the batch file hidden
        ShellExecuteW(nullptr, L"open", batPath.c_str(), nullptr, tempDir, SW_HIDE);
    }

    MessageBoxW(nullptr, L"CloudBoard 已成功卸载。", L"卸载完成", MB_OK | MB_ICONINFORMATION);
}

int APIENTRY wWinMain(_In_ HINSTANCE hInst, _In_opt_ HINSTANCE, _In_ LPWSTR lpCmdLine, _In_ int nShow){
    // Uninstall mode
    if (lpCmdLine && wcsstr(lpCmdLine, L"/uninstall")) {
        DoUninstall();
        return 0;
    }

    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    g.hInst = hInst;

    // Initialize GDI+ first (needed for splash screen rendering)
    GdiplusStartupInput gsi;
    GdiplusStartup(&g.gdiplusToken, &gsi, nullptr);

    // Initialize PDF backend (MuPDF)
    PdfDoc::InitLibrary();

    // Init OpenGL early (needed for board rendering)
    GLInit();

    INITCOMMONCONTROLSEX icc{ sizeof(icc), ICC_STANDARD_CLASSES | ICC_BAR_CLASSES };
    InitCommonControlsEx(&icc);

    // Load persisted settings (before splash/UI so theme is correct)
    LoadSettings();

    // Show splash screen
    DWORD splashStart = GetTickCount();
    if (g.showSplash){
        SplashCreate(hInst);
        SplashUpdateText(L"正在初始化...");
        PumpMessages();
    }

    // Load custom font (needed regardless of splash)
    {
        wchar_t fontPath[MAX_PATH];
        GetModuleFileNameW(nullptr, fontPath, MAX_PATH);
        std::wstring fp(fontPath);
        size_t lastSlash = fp.find_last_of(L"\\/");
        if (lastSlash != std::wstring::npos)
            fp = fp.substr(0, lastSlash + 1);
        fp += L"AnthropicSans-Romans-Variable-25x258.ttf";
        g_splashFontPath = fp;
        AddFontResourceExW(fp.c_str(), FR_PRIVATE, 0);
    }

    // Register PDF association
    if (g.showSplash){ SplashUpdateText(L"正在注册文件关联..."); PumpMessages(); }
    RegisterPdfAssociation();
    RegisterCloudboardAssociation();

    WNDCLASSW wc{};
    wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
    wc.lpfnWndProc = WndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.hIcon = LoadIcon(hInst, MAKEINTRESOURCE(101));
    wc.hbrBackground = nullptr;
    wc.lpszClassName = kClassName;
    RegisterClassW(&wc);

    if (g.showSplash){ SplashUpdateText(L"正在加载界面..."); PumpMessages(); }

    Board::Init();
    int sw = GetSystemMetrics(SM_CXSCREEN);
    int sh = GetSystemMetrics(SM_CYSCREEN);

    g.hWnd = CreateWindowExW(
        WS_EX_ACCEPTFILES,
        kClassName,
        L"Cloudboard 26",
        WS_POPUP | WS_CLIPCHILDREN,
        0, 0, sw, sh,
        nullptr, nullptr, hInst, nullptr);
    if (!g.hWnd){
        if (g.showSplash) SplashDestroy();
        GLShutdown();
        GdiplusShutdown(g.gdiplusToken);
        return 0;
    }

    if (g.showSplash){
        // Keep splash visible for at least 1.6 seconds total.
        wchar_t exeDir[MAX_PATH];
        GetModuleFileNameW(nullptr, exeDir, MAX_PATH);
        std::wstring dirStr(exeDir);
        size_t lastS = dirStr.find_last_of(L"\\/");
        if (lastS != std::wstring::npos) dirStr = dirStr.substr(0, lastS + 1);

        std::vector<std::wstring> files;
        WIN32_FIND_DATAW fd;
        std::wstring pattern = dirStr + L"*";
        HANDLE hFind = FindFirstFileW(pattern.c_str(), &fd);
        if (hFind != INVALID_HANDLE_VALUE){
            do {
                if (fd.cFileName[0] == L'.') continue;
                files.push_back(fd.cFileName);
            } while (FindNextFileW(hFind, &fd));
            FindClose(hFind);
        }

        const DWORD minMs = 1600;
        for (int i = 0; i < (int)files.size(); ++i){
            DWORD elapsed = GetTickCount() - splashStart;
            if (elapsed >= minMs) break;
            std::wstring label = L"加载 " + files[i];
            SplashUpdateText(label.c_str());
            PumpMessages();
            Sleep(100);
        }

        SplashUpdateText(L"已完成预加载");
        PumpMessages();
        DWORD elapsed = GetTickCount() - splashStart;
        if (elapsed < minMs) Sleep(minMs - elapsed);
        SplashDestroy();
    } else {
        Sleep(1000);
    }

    // Show main window AFTER splash is destroyed
    ShowWindow(g.hWnd, SW_SHOW);
    UpdateWindow(g.hWnd);

    // Handle command-line file argument
    if (lpCmdLine && lpCmdLine[0]){
        std::wstring args = lpCmdLine;
        // Strip surrounding quotes
        if (args.size() >= 2 && args.front() == L'"' && args.back() == L'"')
            args = args.substr(1, args.size() - 2);
        if (!args.empty()){
            size_t dot = args.find_last_of(L'.');
            if (dot != std::wstring::npos){
                std::wstring ext = args.substr(dot+1);
                for (auto& c : ext) c = (wchar_t)towlower(c);
                if (ext == L"cloudboard"){
                    std::wstring err;
                    if (ProjectIO::LoadCloudboardFromFile(args, &err))
                        ShowToast(L"已打开 .cloudboard");
                    else
                        ShowToast(L"打开失败：" + err);
                } else if (ext == L"pdf"){
                    Board::LoadPdf(args);
                } else if (ext == L"png" || ext == L"jpg" || ext == L"jpeg" ||
                           ext == L"bmp" || ext == L"gif"){
                    g.currentProjectPath.clear();
                    RECT br2 = BoardRect(g.hWnd);
                    Board::AddImageFromFile(args,
                        (br2.right-br2.left)*0.5f + g.scrollX,
                        (br2.bottom-br2.top)*0.5f + g.scrollY);
                    g.tool = ToolKind::Select;
                }
            }
        }
    }

    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0)){
        if (g.hEdit && !GetProp(g.hEdit, L"_subclassed")){
            SetWindowSubclass(g.hEdit, EditSubProc, 0, 0);
            SetProp(g.hEdit, L"_subclassed", (HANDLE)1);
        }
        if (g.hEdit && IsDialogMessageW(g.hWnd, &msg)){
        } else {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    PdfDoc::DestroyLibrary();
    GLShutdown();
    GdiplusShutdown(g.gdiplusToken);
    return (int)msg.wParam;
}
