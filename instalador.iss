; Script Inno Setup para VIO com Automação de IA nativa via PowerShell

#define MyAppName "VIO"
#define MyAppVersion "0.1"
#define MyAppPublisher "SpinOff"
#define MyAppURL "http://arum.tec.br/SpinOff/index.htm"
#define MyAppExeName "vio.exe"

[Setup]
AppId={{AD4861DB-1935-4F81-B5A9-7A55663ED741}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
DefaultGroupName={#MyAppName}
OutputBaseFilename=Instalador_VIO
SolidCompression=yes
WizardStyle=modern dynamic

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
brazilianportuguese.ReadyLabel2a=O assistente está pronto para instalar o VIO.%n%nATENÇÃO: Caso sua máquina não possua o motor de IA (Ollama) ou o modelo Phi-3, eles serão baixados e configurados automaticamente agora. Certifique-se de estar conectado à internet.
english.ReadyLabel2a=Setup is ready to install VIO.%n%nNOTE: If your system lacks Ollama or the Phi-3 model, they will be downloaded and configured automatically. Make sure you are connected to the internet.

[Files]
Source: "C:\Users\gabri\OneDrive\Documentos\GitHub\VIO - VerInOff - Verificação In Off\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "C:\Users\gabri\OneDrive\Documentos\GitHub\VIO - VerInOff - Verificação In Off\config_rules\*"; DestDir: "{app}\config_rules"; Flags: ignoreversion recursesubdirs createallsubdirs

[Registry]
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
; 1. Se o Ollama não existir, o PowerShell baixa da web e roda a instalação silenciosa
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""& {{ Write-Host 'Baixando instalador oficial do Ollama...'; [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri 'https://ollama.com/download/OllamaSetup.exe' -OutFile '$env:TEMP\OllamaSetup.exe'; Write-Host 'Instalando motor Ollama...'; Start-Process -FilePath '$env:TEMP\OllamaSetup.exe' -ArgumentList '/silent' -Wait; Remove-Item '$env:TEMP\OllamaSetup.exe' -ErrorAction SilentlyContinue }}"""; StatusMsg: "Baixando e instalando o motor Ollama (IA)..."; Flags: waituntilterminated; Check: not OllamaExiste

; 2. Baixa o modelo Phi-3 exibindo o progresso no terminal
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""& {{ Write-Host 'Configurando modelo de IA Phi-3 para o VIO...'; ollama pull phi3; if ($LASTEXITCODE -ne 0) {{ & $env:LOCALAPPDATA\Programs\Ollama\ollama.exe pull phi3 }} }}"""; StatusMsg: "Baixando o modelo Phi-3 (necessário acesso à internet)..."; Flags: waituntilterminated

[Code]
function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Param + ';', ';' + OrigPath + ';') = 0;
end;

function OllamaExiste(): Boolean;
begin
  Result := FileExists(FileSearch('ollama.exe', GetEnv('PATH'))) or 
            FileExists(ExpandConstant('{localappdata}\Programs\Ollama\ollama.exe'));
end;