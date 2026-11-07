@echo off
setlocal enabledelayedexpansion

set SCRIPT_DIR=%~dp0
for %%I in ("%SCRIPT_DIR%..") do set ROOT_DIR=%%~fI
pushd "%SCRIPT_DIR%"

set GITEE_USER=jellybean123
set GITEE_REPO=cloudboard-updates
set VERSION=26.2.0
set MSBUILD=C:\Progra~2\Microsoft Visual Studio\2019\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe
set VCRT_DIR=C:\Progra~2\Microsoft Visual Studio\2019\BuildTools\VC\Redist\MSVC\14.29.30133\x64\Microsoft.VC142.CRT
set PACKAGE_DIR=%SCRIPT_DIR%packages
set STAGE=%PACKAGE_DIR%\tmp
set ZIP=%PACKAGE_DIR%\cloudboard-v%VERSION%.zip
set UPDATE_JSON=%SCRIPT_DIR%update.json
set ROOT_UPDATE_JSON=%ROOT_DIR%\update.json

echo ============================================
echo  CloudBoard OTA Release Script
echo  Version: %VERSION%
echo ============================================

echo.
echo [1/4] Building Release x64...
if not exist "%MSBUILD%" (
    echo ERROR: MSBuild not found:
    echo   %MSBUILD%
    goto fail
)
"%MSBUILD%" "%ROOT_DIR%\cloudboard.vcxproj" /p:Configuration=Release /p:Platform=x64 /p:TrackFileAccess=false /t:Build /nologo
if errorlevel 1 (
    echo ERROR: Build failed!
    goto fail
)
echo Build OK.

echo.
echo [2/4] Packaging...
if not exist "%PACKAGE_DIR%" mkdir "%PACKAGE_DIR%"
if exist "%ZIP%" del /f /q "%ZIP%"
if exist "%STAGE%" rmdir /s /q "%STAGE%"
mkdir "%STAGE%"

if not exist "%ROOT_DIR%\x64\Release\cloudboard.exe" (
    echo ERROR: cloudboard.exe not found after build.
    goto fail
)
copy /y "%ROOT_DIR%\x64\Release\cloudboard.exe" "%STAGE%\" >nul

REM MuPDF is statically linked; do not package pdfium.dll.

REM Release builds use /MD, so package the VC++ 2019 runtime DLLs.
if exist "%VCRT_DIR%\vcruntime140.dll" (
    copy /y "%VCRT_DIR%\*.dll" "%STAGE%\" >nul
) else (
    echo WARNING: VC++ runtime DLLs were not found:
    echo   %VCRT_DIR%
    echo The package may not run on machines without the VC++ runtime installed.
)

if exist "%ROOT_DIR%\x64\Release\Updater.exe" copy /y "%ROOT_DIR%\x64\Release\Updater.exe" "%STAGE%\" >nul
if exist "%ROOT_DIR%\AnthropicSans-Romans-Variable-25x258.ttf" copy /y "%ROOT_DIR%\AnthropicSans-Romans-Variable-25x258.ttf" "%STAGE%\" >nul

powershell -NoProfile -ExecutionPolicy Bypass -Command "Compress-Archive -Force -Path '%STAGE%\*' -DestinationPath '%ZIP%'"
if errorlevel 1 (
    echo ERROR: Package creation failed!
    goto fail
)
rmdir /s /q "%STAGE%"

if not exist "%ZIP%" (
    echo ERROR: Package creation failed!
    goto fail
)

for %%A in ("%ZIP%") do set ZIP_SIZE=%%~zA
echo Package: %ZIP%  (%ZIP_SIZE% bytes)

echo.
echo [3/4] Computing SHA256...
for /f %%H in ('powershell -NoProfile -ExecutionPolicy Bypass -Command "(Get-FileHash '%ZIP%' -Algorithm SHA256).Hash.ToLower()"') do set SHA256=%%H
if "%SHA256%"=="" (
    echo ERROR: Failed to compute SHA256.
    goto fail
)
echo SHA256: %SHA256%

for /f %%D in ('powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-Date -Format yyyy-MM-dd"') do set RD=%%D

echo.
echo [4/4] Generating update.json...
(
echo {
echo   "version": "%VERSION%",
echo   "min_supported_version": "20.0.0",
echo   "download_url": "https://gitee.com/%GITEE_USER%/%GITEE_REPO%/releases/download/v%VERSION%/cloudboard-v%VERSION%.zip",
echo   "checksum_algorithm": "sha256",
echo   "checksum": "%SHA256%",
echo   "release_date": "%RD%",
echo   "release_notes": [
echo     "Improved PDF scrolling smoothness and reduced occasional stalls.",
echo     "Improved pen, eraser, toolbar and file-open experience."
echo   ],
echo   "mandatory": false,
echo   "size_bytes": %ZIP_SIZE%,
echo   "platform": "windows",
echo   "architecture": "x86_64"
echo }
) > "%UPDATE_JSON%"
copy /y "%UPDATE_JSON%" "%ROOT_UPDATE_JSON%" >nul

echo.
echo ============================================
echo  Release package ready!
echo ============================================
echo.
echo Package:
echo   %ZIP%
echo Update metadata:
echo   %UPDATE_JSON%
echo   %ROOT_UPDATE_JSON%
echo.
echo Manual steps:
echo   1. Go to: https://gitee.com/%GITEE_USER%/%GITEE_REPO%/releases
echo   2. Create release with tag: v%VERSION%
echo   3. Upload: %ZIP%
echo   4. Upload update.json to repo root on Gitee
echo   5. Edit release_notes in update.json before uploading if needed
echo.
echo Verify:
echo   https://gitee.com/%GITEE_USER%/%GITEE_REPO%/raw/master/update.json
echo.
popd
pause
exit /b 0

:fail
if exist "%STAGE%" rmdir /s /q "%STAGE%"
popd
pause
exit /b 1
