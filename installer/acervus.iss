; Acervus Finance installer (Inno Setup 6), by VZ Labs.
; Build with scripts\build-installer.ps1, which passes AppVersion. The
; artwork in installer\branding comes from scripts\make-installer-art.ps1.
;
; Data safety: the app keeps its database in %LOCALAPPDATA%\VZ Labs\Acervus,
; which this installer never writes to or removes. Upgrades replace only
; program files; uninstalling leaves the data folder in place and says where
; it is.
;
; Before 0.9 the program was vz-finance.exe and its folder
; %LOCALAPPDATA%\VZFinance; the app moves that folder itself on first start.
; The AppId is unchanged so upgrades find the existing installation.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
; Never change AppId: it is how upgrades find the existing installation.
AppId={{0ECA0F62-FCA8-4844-A6F4-8E1E43FAC220}
AppName=Acervus Finance
AppVersion={#AppVersion}
AppVerName=Acervus Finance {#AppVersion}
AppPublisher=VZ Labs
AppContact=officialvzlabs@gmail.com
AppSupportURL=mailto:officialvzlabs@gmail.com
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
; The setup program carries the VZ Labs logo; the installed app keeps its own icon.
SetupIconFile=branding\vz-labs.ico
WizardImageFile=branding\wizard-164.bmp,branding\wizard-246.bmp,branding\wizard-328.bmp
WizardSmallImageFile=branding\wizard-small-55.bmp,branding\wizard-small-83.bmp,branding\wizard-small-110.bmp
WizardImageStretch=no
WizardImageBackColor=$000000
UninstallDisplayIcon={app}\acervus.exe
UninstallDisplayName=Acervus Finance
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Acervus Finance
VersionInfoCompany=VZ Labs
VersionInfoDescription=Acervus Finance setup

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[InstallDelete]
; The program before 0.9, and shortcuts from before the 0.4 rename.
Type: files; Name: "{app}\vz-finance.exe"
Type: files; Name: "{autoprograms}\VZ Finance.lnk"
Type: files; Name: "{autodesktop}\VZ Finance.lnk"

[Files]
Source: "..\target\release\acervus.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\fonts\Inter-LICENSE.txt"; DestDir: "{app}\licenses"; DestName: "Inter-OFL.txt"; Flags: ignoreversion
Source: "..\docs\ACCOUNTING.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Acervus Finance"; Filename: "{app}\acervus.exe"; Comment: "Private lending portfolio manager, by VZ Labs"
Name: "{autodesktop}\Acervus Finance"; Filename: "{app}\acervus.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\acervus.exe"; Description: "Launch Acervus Finance"; Flags: nowait postinstall skipifsilent

[Code]
// An open pre-0.9 copy (vz-finance.exe) would keep its files in use: ask the
// person to close it first, so the upgrade can replace it and the app can
// move its data folder on the next start.
function InitializeSetup(): Boolean;
var
  Code: Integer;
begin
  Result := True;
  while Exec(ExpandConstant('{sys}\cmd.exe'), '/C tasklist /FI "IMAGENAME eq vz-finance.exe" | find /I "vz-finance.exe" >NUL',
      '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0) do
  begin
    if MsgBox('Acervus Finance is open. Close it, then click OK to continue.', mbInformation, MB_OKCANCEL) = IDCANCEL then
    begin
      Result := False;
      Exit;
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{localappdata}\VZ Labs\Acervus');
    if not DirExists(DataDir) then
      DataDir := ExpandConstant('{localappdata}\VZFinance');
    if DirExists(DataDir) and not UninstallSilent then
      MsgBox('Acervus Finance was removed. Your financial data was kept in:' + #13#10 + #13#10 +
        DataDir + #13#10 + #13#10 +
        'Reinstalling will pick it up again. To erase it permanently, delete that folder yourself.',
        mbInformation, MB_OK);
  end;
end;
