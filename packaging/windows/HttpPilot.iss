#define MyAppName "HttpPilot"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "HttpPilot contributors"
#define MyAppExeName "httppilot-desktop.exe"

[Setup]
AppId=HttpPilot.Native.Desktop
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL=https://github.com/tiagosouzaf/httppilot
AppSupportURL=https://github.com/tiagosouzaf/httppilot/issues
AppUpdatesURL=https://github.com/tiagosouzaf/httppilot/releases
DefaultDirName={localappdata}\Programs\HttpPilot
DefaultGroupName=HttpPilot
DisableProgramGroupPage=yes
OutputBaseFilename=HttpPilot-0.1.0-Windows-x64-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na área de trabalho"; GroupDescription: "Atalhos:"; Flags: unchecked

[Files]
Source: "..\..\target\release\httppilot-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\httppilot-cli.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\HttpPilot"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\HttpPilot"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Abrir HttpPilot"; Flags: nowait postinstall skipifsilent
