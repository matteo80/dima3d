# Pubblica su GitHub la Release della versione corrente, con lo zip di pacchetto.ps1.
# Il programma la trova con «Verifica aggiornamenti» e propone di installarla.
# Uso:  powershell -ExecutionPolicy Bypass -File pubblica.ps1 -Note note.md
#       (note.md: le novità, in testo semplice; vengono mostrate nel programma)
param([Parameter(Mandatory = $true)][string]$Note)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$zip = Join-Path 'dist' "Dima3D-$version.zip"
if (-not (Test-Path $zip)) { throw "Manca ${zip}: esegui prima pacchetto.ps1" }
if (-not (Test-Path $Note)) { throw "File delle note non trovato: $Note" }

$gh = Get-Command gh -ErrorAction SilentlyContinue
$gh = if ($gh) { $gh.Source } else { 'C:\Program Files\GitHub CLI\gh.exe' }

# La Release punta al commit pubblicato: niente modifiche in sospeso
$dirty = git status --porcelain
if ($dirty) { throw 'Ci sono modifiche non salvate in git: fai prima commit e push' }

$assets = @($zip)
$setup = Join-Path 'dist' "Dima3D-$version-setup.exe"
if (Test-Path $setup) { $assets += $setup } else { Write-Warning "Manca ${setup}: pubblico solo lo zip" }
& $gh release create "v$version" @assets --title "Dima 3D $version" --notes-file $Note
if ($LASTEXITCODE -ne 0) { throw 'Pubblicazione non riuscita' }
Write-Host "Pubblicata: v$version"
