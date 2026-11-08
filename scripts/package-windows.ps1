# Build and package CloudBoard for Windows.
#   pwsh scripts/package-windows.ps1 [-Target x86_64-pc-windows-msvc] [-Arch x64] [-PdfiumVersion latest] [-NoInstaller]
# Output (dist/):
#   cloudboard-<ver>-windows-<arch>.zip        portable package, also used by the in-app updater
#   cloudboard-<ver>-windows-<arch>-setup.exe  Inno Setup installer (if iscc is available)
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Arch = "x64",
    [string]$PdfiumVersion = "latest",
    [switch]$NoInstaller
)
$ErrorActionPreference = "Stop"
$Root = Resolve-Path "$PSScriptRoot\.."
Set-Location $Root

$Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "CloudBoard $Version ($Target)"

cargo build --release --locked --target $Target -p cloudboard-windows -p cloudboard-updater
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$Dist = Join-Path $Root "dist"
$Stage = Join-Path $Dist "windows-$Arch\app"
Remove-Item -Recurse -Force (Join-Path $Dist "windows-$Arch") -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Stage | Out-Null

Copy-Item "target\$Target\release\cloudboard.exe" $Stage
Copy-Item "target\$Target\release\cloudboard-updater.exe" $Stage

# PDFium (BSD) from bblanchon/pdfium-binaries.
$PdfiumAsset = if ($Arch -eq "arm64") { "pdfium-win-arm64.tgz" } else { "pdfium-win-x64.tgz" }
$PdfiumUrl = if ($PdfiumVersion -eq "latest") {
    "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/$PdfiumAsset"
} else {
    "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$PdfiumVersion/$PdfiumAsset"
}
$Tmp = Join-Path $Dist "pdfium-$Arch"
New-Item -ItemType Directory -Force $Tmp | Out-Null
Invoke-WebRequest -Uri $PdfiumUrl -OutFile "$Tmp\pdfium.tgz"
tar -xzf "$Tmp\pdfium.tgz" -C $Tmp
Copy-Item "$Tmp\bin\pdfium.dll" $Stage
Copy-Item "$Tmp\LICENSE" (Join-Path $Stage "PDFIUM-LICENSE.txt") -ErrorAction SilentlyContinue

Copy-Item "README.md" $Stage -ErrorAction SilentlyContinue

$Zip = Join-Path $Dist "cloudboard-$Version-windows-$Arch.zip"
Remove-Item $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$Stage\*" -DestinationPath $Zip
Write-Host "Portable package: $Zip"

if (-not $NoInstaller) {
    $Iscc = Get-Command iscc -ErrorAction SilentlyContinue
    if (-not $Iscc) { $Iscc = Get-Item "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" -ErrorAction SilentlyContinue }
    if ($Iscc) {
        & $Iscc.FullName /Q "/DAppVersion=$Version" "/DSourceDir=$Stage" "/DOutputDir=$Dist" "/DArchSuffix=$Arch" "packaging\windows\cloudboard.iss"
        if ($LASTEXITCODE -ne 0) { throw "iscc failed" }
        Write-Host "Installer: $Dist\cloudboard-$Version-windows-$Arch-setup.exe"
    } else {
        Write-Warning "Inno Setup (iscc) not found; skipping installer."
    }
}
