; Acervus Finance installer (Inno Setup 6).
; Build with scripts\build-installer.ps1, which passes AppVersion.
;
; Data safety: the app stores its database in %LOCALAPPDATA%\VZFinance, which
; this installer never writes to or removes. Upgrades replace only program
; files; uninstalling leaves the data folder in place and says where it is.
;
; The app was called VZ Finance before 0.4. The AppId, the data folder and the
; exe name are unchanged so upgrades find the existing installation and data.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
; Never change AppId: it is how upgrades find the existing installation.
AppId={{0ECA0F62-FCA8-4844-A6F4-8E1E43FAC220}
AppName=Acervus Finance
AppVersion={#AppVersion}
AppVerName=Acervus Finance {#AppVersion}
AppPublisher=VZ Finance
; Upgrades keep the folder of the existing installation.
DefaultDirName={autopf}\Acervus Finance
DefaultGroupName=Acervus Finance
DisableProgramGroupPage=yes
; Per-user install by default (no admin prompt); admins may choose all users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 version 1809 (build 17763) or later.
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=AcervusFinance-Setup-{#AppVersion}
SetupIconFile=..\assets\icons\vz-finance.ico
UninstallDisplayIcon={app}\vz-finance.exe
UninstallDisplayName=Acervus Finance
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Acervus Finance
VersionInfoCompany=VZ Finance

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[InstallDelete]
; Shortcuts from before the rename.
Type: files; Name: "{autoprograms}\VZ Finance.lnk"
Type: files; Name: "{autodesktop}\VZ Finance.lnk"

[Files]
Source: "..\target\release\vz-finance.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\fonts\Inter-LICENSE.txt"; DestDir: "{app}\licenses"; DestName: "Inter-OFL.txt"; Flags: ignoreversion
Source: "..\docs\ACCOUNTING.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Acervus Finance"; Filename: "{app}\vz-finance.exe"; Comment: "Private lending portfolio manager"
Name: "{autodesktop}\Acervus Finance"; Filename: "{app}\vz-finance.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\vz-finance.exe"; Description: "Launch Acervus Finance"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{localappdata}\VZFinance');
    if DirExists(DataDir) and not UninstallSilent then
      MsgBox('Acervus Finance was removed. Your financial data was kept in:' + #13#10 + #13#10 +
        DataDir + #13#10 + #13#10 +
        'Reinstalling will pick it up again. To erase it permanently, delete that folder yourself.',
        mbInformation, MB_OK);
  end;
end;
