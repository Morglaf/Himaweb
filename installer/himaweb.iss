; HimaWeb — installateur Windows (Inno Setup 6)
; Build local :
;   cargo build --release
;   iscc /DMyAppVersion=0.1.4 /DMyAppExeSource="..\target\release\himaweb.exe" himaweb.iss
; CI : voir .github/workflows/release.yml

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-dev"
#endif

#ifndef MyAppExeSource
  #define MyAppExeSource "..\target\release\himaweb.exe"
#endif

#define MyAppName "HimaWeb"
#define MyAppPublisher "Morglaf"
#define MyAppURL "https://github.com/Morglaf/Himaweb"
#define MyAppExeName "himaweb.exe"
#define MyAppId "{{A7C3E9F1-2B4D-4E8A-9C1F-6D5E8A0B3C2D}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\HimaWeb
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=HimaWeb-Setup-x64
SetupIconFile=
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UninstallDisplayIcon={app}\bin\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
VersionInfoVersion={#MyAppVersion}.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoProductName={#MyAppName}
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addpath"; Description: "Ajouter le dossier bin au PATH utilisateur"; GroupDescription: "Options :"; Flags: checkedonce

[Components]
Name: "core"; Description: "HimaWeb (obligatoire)"; Types: full compact custom; Flags: fixed
Name: "deps"; Description: "Dépendances Pimalaya"; Types: full
Name: "deps\himalaya"; Description: "Himalaya (mail) — obligatoire"; Types: full compact custom; Flags: fixed
Name: "deps\cardamum"; Description: "Cardamum (contacts)"; Types: full
Name: "deps\calendula"; Description: "Calendula (calendrier)"; Types: full
Name: "deps\ortie"; Description: "Ortie (OAuth)"; Types: full
Name: "deps\neverest"; Description: "Neverest (sync mail)"; Types: full
Name: "deps\mirador"; Description: "Mirador (watch boîtes)"; Types: full
Name: "deps\ollama"; Description: "Ollama (IA locale, via winget si dispo)"; Types: full

[Types]
Name: "full"; Description: "Installation complète (recommandée)"
Name: "compact"; Description: "HimaWeb + Himalaya seulement"
Name: "custom"; Description: "Personnalisée"; Flags: iscustom

[Files]
Source: "{#MyAppExeSource}"; DestDir: "{app}\bin"; DestName: "{#MyAppExeName}"; Flags: ignoreversion; Components: core
Source: "install-deps.ps1"; DestDir: "{app}\installer"; Flags: ignoreversion; Components: core

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\bin\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\bin\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; Corrélation Winget / Apps & fonctionnalités
Root: HKCU; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}"; ValueType: string; ValueName: "InstallPath"; ValueData: "{app}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}"; ValueType: string; ValueName: "Version"; ValueData: "{#MyAppVersion}"

[Code]
function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  { Ne pas doubler une entrée déjà présente }
  Result := Pos(';' + Param + ';', ';' + OrigPath + ';') = 0;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  BinDir: string;
  OrigPath: string;
begin
  if CurStep = ssPostInstall then
  begin
    if WizardIsTaskSelected('addpath') then
    begin
      BinDir := ExpandConstant('{app}\bin');
      if NeedsAddPath(BinDir) then
      begin
        if RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
        begin
          if OrigPath <> '' then
            OrigPath := OrigPath + ';' + BinDir
          else
            OrigPath := BinDir;
          RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath);
        end
        else
          RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', BinDir);
      end;
    end;
  end;
end;

function GetDepsTools(Param: string): string;
var
  Tools: string;
begin
  Tools := '';
  if WizardIsComponentSelected('deps\himalaya') then Tools := Tools + 'himalaya,';
  if WizardIsComponentSelected('deps\cardamum') then Tools := Tools + 'cardamum,';
  if WizardIsComponentSelected('deps\calendula') then Tools := Tools + 'calendula,';
  if WizardIsComponentSelected('deps\ortie') then Tools := Tools + 'ortie,';
  if WizardIsComponentSelected('deps\neverest') then Tools := Tools + 'neverest,';
  if WizardIsComponentSelected('deps\mirador') then Tools := Tools + 'mirador,';
  if (Length(Tools) > 0) and (Tools[Length(Tools)] = ',') then
    SetLength(Tools, Length(Tools) - 1);
  Result := Tools;
end;

function HasPimalayaDeps: Boolean;
begin
  Result := GetDepsTools('') <> '';
end;

[Run]
; Dépendances Pimalaya (téléchargements parallèles)
Filename: "powershell.exe"; \
  Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\installer\install-deps.ps1"" -Prefix ""{app}"" -Tools ""{code:GetDepsTools}"" -SkipIfPresent"; \
  StatusMsg: "Installation des dépendances Pimalaya…"; \
  Flags: waituntilterminated runhidden; \
  Check: HasPimalayaDeps
; Ollama séparé (winget)
Filename: "powershell.exe"; \
  Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\installer\install-deps.ps1"" -Prefix ""{app}"" -Ollama -SkipIfPresent"; \
  StatusMsg: "Installation d’Ollama…"; \
  Flags: waituntilterminated; \
  Components: deps\ollama
Filename: "{app}\bin\{#MyAppExeName}"; Description: "Lancer HimaWeb"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}\tools"
Type: filesandordirs; Name: "{app}\installer"
