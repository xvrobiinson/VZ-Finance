; VZ Finance installer (Inno Setup 6).
; Build with scripts\build-installer.ps1, which passes AppVersion.
;
; Data safety: the app stores its database in %LOCALAPPDATA%\VZFinance, which
; this installer never writes to or removes. Upgrades replace only program
; files; uninstalling leaves the data folder in place and says where it is.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
; Never change AppId: it is how upgrades find the existing installation.
AppId={{0ECA0F62-FCA8-4844-A6F4-8E1E43FAC220}
AppName=VZ Finance
AppVersion={#AppVersion}
AppVerName=VZ Finance {#AppVersion}
AppPublisher=VZ Finance
DefaultDirName={autopf}\VZ Finance
DefaultGroupName=VZ Finance
DisableProgramGroupPage=yes
; Per-user install by default (no admin prompt); admins may choose all users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=VZFinance-Setup-{#AppVersion}
SetupIconFile=..\assets\icons\vz-finance.ico
UninstallDisplayIcon={app}\vz-finance.exe
UninstallDisplayName=VZ Finance
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName=VZ Finance

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\vz-finance.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\fonts\Inter-LICENSE.txt"; DestDir: "{app}\licenses"; DestName: "Inter-OFL.txt"; Flags: ignoreversion
Source: "..\docs\ACCOUNTING.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\VZ Finance"; Filename: "{app}\vz-finance.exe"; Comment: "Private lending portfolio manager"
Name: "{autodesktop}\VZ Finance"; Filename: "{app}\vz-finance.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\vz-finance.exe"; Description: "Launch VZ Finance"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{localappdata}\VZFinance');
    if DirExists(DataDir) and not UninstallSilent then
      MsgBox('VZ Finance was removed. Your financial data was kept in:' + #13#10 + #13#10 +
        DataDir + #13#10 + #13#10 +
        'Reinstalling will pick it up again. To erase it permanently, delete that folder yourself.',
        mbInformation, MB_OK);
  end;
end;
