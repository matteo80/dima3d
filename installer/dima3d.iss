; Installer di Dima 3D (Inno Setup 6). Lo compila pacchetto.ps1:
;   ISCC.exe /DVersione=0.2.6 installer\dima3d.iss
; Prende i file da dist\Dima3D-<versione> (preparati da pacchetto.ps1) e scrive
; dist\Dima3D-<versione>-setup.exe.
;
; Installa nella cartella dell'utente (%LOCALAPPDATA%\Programs\Dima3D): non servono permessi
; da amministratore, e l'aggiornamento dal programma può sostituire i file senza chiedere niente.

#ifndef Versione
  #error Manca la versione: ISCC /DVersione=x.y.z installer\dima3d.iss
#endif

[Setup]
AppId={{D6649788-0CB3-4941-A297-5C1005F07E6E}
AppName=Dima 3D
AppVersion={#Versione}
AppVerName=Dima 3D {#Versione}
AppPublisher=matteo80
AppPublisherURL=https://github.com/matteo80/dima3d
AppSupportURL=https://github.com/matteo80/dima3d
AppUpdatesURL=https://github.com/matteo80/dima3d/releases
DefaultDirName={localappdata}\Programs\Dima3D
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
SetupIconFile=..\assets\icona.ico
UninstallDisplayIcon={app}\dima3d.exe
UninstallDisplayName=Dima 3D
OutputDir=..\dist
OutputBaseFilename=Dima3D-{#Versione}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Chiude Dima 3D se è aperto durante l'installazione o la disinstallazione
CloseApplications=yes
ChangesAssociations=yes

[Languages]
Name: "italiano"; MessagesFile: "compiler:Languages\Italian.isl"

[Tasks]
Name: "desktop"; Description: "Crea un collegamento sul desktop"; GroupDescription: "Collegamenti:"
Name: "progetti"; Description: "Apri i progetti .dima3d con Dima 3D (doppio clic)"; GroupDescription: "File:"

[Files]
Source: "..\dist\Dima3D-{#Versione}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Dima 3D"; Filename: "{app}\dima3d.exe"
Name: "{userdesktop}\Dima 3D"; Filename: "{app}\dima3d.exe"; Tasks: desktop

[Registry]
; Associazione dei progetti, solo per l'utente
Root: HKCU; Subkey: "Software\Classes\.dima3d"; ValueType: string; ValueName: ""; ValueData: "Dima3D.Progetto"; Flags: uninsdeletevalue; Tasks: progetti
Root: HKCU; Subkey: "Software\Classes\Dima3D.Progetto"; ValueType: string; ValueName: ""; ValueData: "Progetto Dima 3D"; Flags: uninsdeletekey; Tasks: progetti
Root: HKCU; Subkey: "Software\Classes\Dima3D.Progetto\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\dima3d.exe,0"; Tasks: progetti
Root: HKCU; Subkey: "Software\Classes\Dima3D.Progetto\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\dima3d.exe"" ""%1"""; Tasks: progetti

[Run]
Filename: "{app}\dima3d.exe"; Description: "Avvia Dima 3D"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Lasciato da un aggiornamento dal programma
Type: files; Name: "{app}\dima3d.exe.old"
