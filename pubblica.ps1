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

# Controllo della privacy prima del caricamento: il commit della Release deve essere già su
# GitHub (l'hook pre-push ha controllato codice, messaggi e autori) e note, LEGGIMI e file di
# testo del pacchetto non devono contenere le parole vietate (.git/info/parole-vietate.txt)
git fetch --quiet origin
$head = git rev-parse HEAD
git merge-base --is-ancestor $head origin/main
if ($LASTEXITCODE -ne 0) { throw 'Il commit corrente non è su GitHub: fai prima il push (con il controllo delle parole vietate)' }
$list = Join-Path (git rev-parse --git-dir) 'info\parole-vietate.txt'
if (-not (Test-Path $list)) { throw "Manca ${list}: senza l'elenco delle parole vietate non si pubblica" }
$patterns = @(Get-Content -Encoding UTF8 $list | Where-Object { $_.Trim() -and -not $_.Trim().StartsWith('#') })
$folder = Join-Path 'dist' "Dima3D-$version"
$texts = @(Get-Item $Note) + @(Get-ChildItem -Recurse -File $folder -Include *.txt, *.md, *.html, *.ini, *.json, *.toml)
foreach ($file in $texts) {
    $content = Get-Content -Raw -Encoding UTF8 $file.FullName
    foreach ($pattern in $patterns) {
        if ($content -match $pattern) {
            throw "PUBBLICAZIONE BLOCCATA: $($file.Name) contiene un riferimento non anonimo ($($Matches[0]))"
        }
    }
}
Write-Host "Controllo privacy: note e $($texts.Count - 1) file di testo del pacchetto puliti"

$assets = @($zip)
$setup = Join-Path 'dist' "Dima3D-$version-setup.exe"
if (Test-Path $setup) { $assets += $setup } else { Write-Warning "Manca ${setup}: pubblico solo lo zip" }

# Prima la bozza, poi i file uno alla volta con qualche ripetizione (il caricamento su
# uploads.github.com a volte si interrompe), infine la pubblicazione: mai una Release a metà,
# che il programma proporrebbe come aggiornamento senza lo zip.
$tag = "v$version"
& $gh release create $tag --draft --target main --title "Dima 3D $version" --notes-file $Note
if ($LASTEXITCODE -ne 0) { throw 'Bozza della Release non creata' }
foreach ($asset in $assets) {
    $ok = $false
    for ($i = 1; $i -le 5 -and -not $ok; $i++) {
        & $gh release upload $tag $asset --clobber
        $ok = $LASTEXITCODE -eq 0
        if (-not $ok) { Write-Warning "Caricamento di $asset non riuscito (tentativo $i)"; Start-Sleep 5 }
    }
    if (-not $ok) { throw "Caricamento di $asset non riuscito: la Release resta in bozza su GitHub, riprova pubblica.ps1 dopo averla eliminata (gh release delete $tag)" }
}
& $gh release edit $tag --draft=false
if ($LASTEXITCODE -ne 0) { throw 'Pubblicazione non riuscita: la Release resta in bozza' }
Write-Host "Pubblicata: $tag"
