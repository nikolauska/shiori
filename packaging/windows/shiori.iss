; Inno Setup script for Shiori's Windows installer. The release workflow
; builds it with:
;
;   iscc /DAppVersion=0.2.0 /DSourceDir=<folder with shiori.exe> packaging\windows\shiori.iss
;
; The installer is per user: it needs no administrator rights, installs to
; %LOCALAPPDATA%\Programs\Shiori and can be updated by running a newer
; installer over it. User data lives in %APPDATA%\shiori and
; %LOCALAPPDATA%\shiori\cache, outside the program folder, so installing,
; updating and uninstalling never touch the list or settings.

#ifndef AppVersion
  #error Pass the version with /DAppVersion=x.y.z
#endif
#ifndef SourceDir
  #error Pass the folder holding shiori.exe with /DSourceDir=<folder>
#endif

#define RepoRoot AddBackslash(SourcePath) + "..\.."

[Setup]
; The AppId ties every later installer to this installation, so an update
; replaces it instead of adding a second entry to "Installed apps". Never
; change it.
AppId={{64925326-9B9C-41DF-AEBC-974262C30126}
AppName=Shiori
AppVersion={#AppVersion}
AppVerName=Shiori {#AppVersion}
AppPublisher=Shiori
AppPublisherURL=https://github.com/nikolauska/shiori
AppSupportURL=https://github.com/nikolauska/shiori/issues
AppUpdatesURL=https://github.com/nikolauska/shiori/releases
VersionInfoVersion={#AppVersion}
; Per user only. An all-users install would need administrator rights for
; every update, which the in-app update notice cannot ask for.
PrivilegesRequired=lowest
; With lowest privileges {autopf} is %LOCALAPPDATA%\Programs.
DefaultDirName={autopf}\Shiori
DisableDirPage=auto
DefaultGroupName=Shiori
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 and later.
MinVersion=10.0
WizardStyle=modern
SetupIconFile={#RepoRoot}\assets\icon.ico
UninstallDisplayIcon={app}\shiori.exe
UninstallDisplayName=Shiori
OutputDir={#RepoRoot}
OutputBaseFilename=shiori-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
; Shiori keeps running in the tray after its window is closed, so the
; installer has to close it before replacing the exe. Restart Manager asks it
; to close and, with "force", ends it if it does not. Its data is in SQLite
; and written atomically, so ending it loses at most an unsaved countdown.
CloseApplications=force
RestartApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\shiori.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#RepoRoot}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Shiori"; Filename: "{app}\shiori.exe"
Name: "{autodesktop}\Shiori"; Filename: "{app}\shiori.exe"; Tasks: desktopicon

[Run]
; skipifsilent: a silent update (a later in-app updater) should not open a
; window by itself; RestartApplications brings back a copy that was running.
Filename: "{app}\shiori.exe"; Description: "{cm:LaunchProgram,Shiori}"; Flags: nowait postinstall skipifsilent
