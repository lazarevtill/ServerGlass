; The Windows installer.
;
;   .\scripts\package-windows.ps1             build it into dist\
;   .\scripts\package-windows.ps1 -Install    and install it on this machine
;
; Per-user, deliberately. PrivilegesRequired=lowest puts the app under %LOCALAPPDATA%\Programs and
; the shortcut in the user's own Start Menu, so installing asks for no administrator and raises no
; UAC prompt - the same bargain scripts/build-linux.sh --install makes with ~/.local. Nothing here
; wants a machine-wide install: everything the app stores is per-user already, including DPAPI
; blobs that no other account on the machine can decrypt.
;
; The build is unsigned, so SmartScreen shows "Windows protected your PC" the first time and needs
; More info > Run anyway. Signing means buying a code-signing certificate, exactly as notarising
; the macOS build means an Apple Developer identity. The README says so rather than pretending.
;
; AppVersion and SourceDir are passed in by scripts/package-windows.ps1 rather than written here,
; so the version has one home - Cargo.toml - and cannot be half-updated.

#define AppName "ServerGlass"
#define AppPublisher "lazarev.cloud"
#define AppUrl "https://gitlab.lazarev.cloud/lazarevtill/serverglass"
#define AppExe "ServerGlass.exe"

#ifndef AppVersion
  #error AppVersion was not passed in. Run scripts\package-windows.ps1, or ISCC /DAppVersion=0.3.0
#endif
#ifndef SourceDir
  #error SourceDir was not passed in. It is the publish folder: ...\win-x64\publish
#endif
#ifndef OutputDir
  #define OutputDir "..\..\..\dist"
#endif

[Setup]
; Generated once, and never regenerated. This is the identity Windows uses to recognise an existing
; install and upgrade it in place. A fresh GUID per build would install alongside the last one and
; leave an orphaned entry in Apps & Features behind every time.
AppId={{87F5A056-0986-4A32-BFB0-5A9E1BD1BD93}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/-/issues
AppUpdatesURL={#AppUrl}/-/releases
VersionInfoVersion={#AppVersion}

; {autopf} resolves to %LOCALAPPDATA%\Programs under PrivilegesRequired=lowest, and to Program
; Files only if this is ever run elevated on purpose.
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; No license page. The project is dual-licensed MIT and Apache-2.0, and a wizard page can show one
; file; showing either alone would state the terms wrongly. LICENSE-MIT and LICENSE-APACHE ship in
; the repository, where both are visible.
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
SetupIconFile=..\ServerGlass\Assets\ServerGlass.ico

; x64compatible rather than x64: the app is win-x64, which Windows on ARM runs under emulation.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Matches TargetPlatformMinVersion in ServerGlass.csproj. Windows App SDK 2.4 will not run below it,
; and failing here with a sentence is better than failing at launch with a missing entry point.
MinVersion=10.0.17763

OutputDir={#OutputDir}
OutputBaseFilename={#AppName}-{#AppVersion}-windows-x64-setup
; The payload is a self-contained .NET publish - the runtime and the Windows App SDK included -
; which is a little over 200 MB of mostly compressible DLLs. lzma2/max takes a couple of minutes
; and roughly thirds it; solid compression helps because the runtime files resemble each other.
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

; No [UninstallDelete]. The host inventory, the pinned host keys and the encrypted credentials live
; in %LOCALAPPDATA%\ServerGlass, outside {app}, and uninstalling deliberately leaves them: someone
; upgrading should not have to re-add every server and re-approve every host key, and someone
; leaving can delete one folder. docs/WINDOWS.md says where it is.
