; Inno Setup script for CloudBoard (per-user install, no admin rights needed).
; Built by scripts/package-windows.ps1:  iscc /DAppVersion=27.0.0 /DSourceDir=dist\windows-x64\app cloudboard.iss

#ifndef AppVersion
  #define AppVersion "27.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\windows-x64\app"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
#ifndef ArchSuffix
  #define ArchSuffix "x64"
#endif

[Setup]
AppId={{6B3D2C1E-27C0-4B0A-9C2B-0CB27C10DB00}
AppName=CloudBoard
AppVersion={#AppVersion}
AppVerName=CloudBoard {#AppVersion}
AppPublisher=CloudBoard
AppPublisherURL=https://github.com/2001020/cloudboard
AppSupportURL=https://github.com/2001020/cloudboard/issues
DefaultDirName={localappdata}\Programs\CloudBoard
DefaultGroupName=CloudBoard
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=cloudboard-{#AppVersion}-windows-{#ArchSuffix}-setup
SetupIconFile=..\..\assets\icons\cloudboard.ico
UninstallDisplayIcon={app}\cloudboard.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible arm64
ChangesAssociations=yes
CloseApplications=yes

[Languages]
Name: "chinesesimp"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\CloudBoard"; Filename: "{app}\cloudboard.exe"
Name: "{group}\卸载 CloudBoard"; Filename: "{uninstallexe}"
Name: "{userdesktop}\CloudBoard"; Filename: "{app}\cloudboard.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\cloudboard.exe"; Description: "立即启动 CloudBoard"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\cloudboard.exe"; Parameters: "--unregister"; Flags: runhidden; RunOnceId: "UnregisterAssoc"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\.update-staging"
Type: filesandordirs; Name: "{app}\.update-backup"
