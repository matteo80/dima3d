# Prepara la cartella e lo zip da distribuire: dist\Gestore-WRL-<versione>.zip
# Contiene gestore-wrl.exe, tools\jt2wrl.exe (se compilato), le licenze e un LEGGIMI.
# Uso:  powershell -ExecutionPolicy Bypass -File pacchetto.ps1  [-SenzaCompilare]
param([switch]$SenzaCompilare)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (-not $SenzaCompilare) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'Compilazione non riuscita' }
}

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$name = "Gestore-WRL-$version"
$dir = Join-Path 'dist' $name
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force (Join-Path $dir 'licenses') | Out-Null

Copy-Item 'target\release\gestore-wrl.exe' $dir
Copy-Item 'LICENSE' (Join-Path $dir 'LICENSE.txt')
Copy-Item 'licenses\*.txt' (Join-Path $dir 'licenses')

$jt = 'target\release\tools\jt2wrl.exe'
if (Test-Path $jt) {
    New-Item -ItemType Directory -Force (Join-Path $dir 'tools') | Out-Null
    Copy-Item $jt (Join-Path $dir 'tools')
} else {
    Write-Warning 'tools\jt2wrl.exe non trovato: il pacchetto non aprirà i file JT (vedi tools-src\jt2wrl\README.md)'
}

$readme = @"
Gestore WRL $version
====================

Avvio: doppio clic su gestore-wrl.exe. Non serve installare niente: si può copiare la cartella
intera su un altro PC (Windows 10 o 11, 64 bit) o su una chiavetta.

Formati
- Apertura: WRL (VRML 2.0 e 1.0), STL, JT (con tools\jt2wrl.exe, che deve restare nella
  cartella tools accanto all'eseguibile).
- Export: WRL, STL, Open Inventor (.iv), scegliendo il tipo nella finestra Salva.

Le preferenze sono salvate in %APPDATA%\Gestore-WRL\impostazioni.txt.

Licenze: LICENSE.txt (Gestore WRL) e la cartella licenses (librerie incluse).
"@
# Fine riga di Windows, così il Blocco note lo mostra bene
Set-Content -Path (Join-Path $dir 'LEGGIMI.txt') -Value ($readme -replace "`r?`n", "`r`n") -Encoding UTF8 -NoNewline

$zip = Join-Path 'dist' "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $dir -DestinationPath $zip
$mb = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "Pronto: $zip ($mb MB)"
