//! Aggiornamento dalle Release di GitHub.
//!
//! All'avvio (se attivo nelle preferenze) o dal pulsante in Info si chiede a GitHub l'ultima
//! versione pubblicata. Se è più nuova si mostra cosa cambia e si chiede se aggiornare. Lo zip
//! scaricato si estrae in una cartella temporanea; poi l'exe in uso viene rinominato
//! (`dima3d.exe.old`: Windows non lascia sovrascrivere un programma aperto, ma lo lascia
//! rinominare), i file nuovi prendono il suo posto e il programma si riavvia. Il vecchio exe si
//! cancella all'avvio successivo.
//!
//! La rete passa da PowerShell di Windows: usa il proxy di sistema con le credenziali di Windows
//! (le reti aziendali lo richiedono spesso) e sa estrarre gli zip, senza librerie in più.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

/// Repository delle Release.
const REPO: &str = "matteo80/dima3d";
/// Solo gli zip delle Release del progetto si possono scaricare.
const DOWNLOAD_PREFIX: &str = "https://github.com/matteo80/dima3d/releases/download/";
/// Nome dell'exe nel pacchetto.
const EXE: &str = "dima3d.exe";

/// Versione pubblicata su GitHub.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// Senza la `v` davanti (es. `0.2.3`)
    pub version: String,
    pub zip_url: String,
    pub page_url: String,
    /// Note della versione (testo della Release)
    pub notes: String,
}

/// Stato della verifica e del download.
#[derive(Default)]
pub struct UpdateState {
    check: Option<(Receiver<Result<Release, String>>, bool)>,
    download: Option<Download>,
    /// Versione nuova da proporre (finestra aperta)
    pub offer: Option<Release>,
    /// Esito di una verifica chiesta a mano (finestra aperta)
    pub message: Option<String>,
}

impl UpdateState {
    pub fn busy(&self) -> bool {
        self.check.is_some() || self.download.is_some()
    }
}

/// `true` se `remote` (es. `v0.3.0` o `0.3.0`) è più nuova di `current`.
pub fn is_newer(remote: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let (r, c) = (parts(remote), parts(current));
    for i in 0..r.len().max(c.len()) {
        let (a, b) = (r.get(i).copied().unwrap_or(0), c.get(i).copied().unwrap_or(0));
        if a != b {
            return a > b;
        }
    }
    false
}

/// Release dall'uscita dello script di verifica: versione, zip, pagina, poi le note.
fn parse_release(text: &str) -> Result<Release, String> {
    let mut lines = text.lines();
    let tag = lines.next().unwrap_or("").trim();
    let zip_url = lines.next().unwrap_or("").trim().to_string();
    let page_url = lines.next().unwrap_or("").trim().to_string();
    if tag.is_empty() {
        return Err("risposta di GitHub non valida".into());
    }
    if !zip_url.starts_with(DOWNLOAD_PREFIX) || !zip_url.ends_with(".zip") {
        return Err("la versione pubblicata non ha uno zip da scaricare".into());
    }
    let notes = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Ok(Release { version: tag.trim_start_matches(['v', 'V']).to_string(), zip_url, page_url, notes })
}

/// Comando PowerShell senza finestra per lo script; i dati arrivano dalle variabili d'ambiente.
fn powershell_command(script: &str, env: &[(&str, &str)]) -> std::process::Command {
    let preamble = "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; \
        [Console]::OutputEncoding = [Text.Encoding]::UTF8; \
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
        [Net.WebRequest]::DefaultWebProxy.Credentials = [Net.CredentialCache]::DefaultNetworkCredentials; ";
    let mut command = std::process::Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"]);
    command.arg(format!("{preamble}{script}"));
    for (k, v) in env {
        command.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Errore di PowerShell in una riga leggibile (PowerShell va a capo a metà del messaggio).
fn powershell_error(stderr: &[u8], what: &str) -> String {
    let err: String = String::from_utf8_lossy(stderr).split_whitespace().collect::<Vec<_>>().join(" ");
    if err.contains("Not Found") || err.contains("404") {
        return "su GitHub non c'è ancora una versione pubblicata".into();
    }
    let short: String = err.chars().take(160).collect();
    format!("{what} ({})", if short.is_empty() { "errore sconosciuto" } else { &short })
}

/// Esegue uno script PowerShell e ne ritorna l'uscita.
fn powershell(script: &str, env: &[(&str, &str)]) -> Result<String, String> {
    let out = powershell_command(script, env).output().map_err(|e| format!("PowerShell non disponibile: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(powershell_error(&out.stderr, "connessione a GitHub non riuscita"))
    }
}

fn fetch_latest() -> Result<Release, String> {
    let script = "$r = Invoke-RestMethod -UseBasicParsing -Uri $env:DIMA3D_API -Headers @{ 'User-Agent' = 'Dima3D' }; \
        $zip = ($r.assets | Where-Object { $_.name -like '*.zip' } | Select-Object -First 1).browser_download_url; \
        Write-Output $r.tag_name; Write-Output $zip; Write-Output $r.html_url; Write-Output $r.body";
    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    parse_release(&powershell(script, &[("DIMA3D_API", &api)])?)
}

/// Avanzamento del download, dal thread che lo segue.
#[derive(Debug)]
enum DownloadEvent {
    /// Byte ricevuti e totali (0 se il server non lo dice)
    Progress(u64, u64),
    /// Download finito, estrazione dello zip in corso
    Extracting,
    /// Cartella con i file del pacchetto, o l'errore
    Finished(Result<PathBuf, String>),
}

/// Script di download: scrive `P ricevuti totali` ogni 150 ms, `E` quando estrae.
const DOWNLOAD_SCRIPT: &str = "$zip = Join-Path $env:DIMA3D_DIR 'pacchetto.zip'; \
    $req = [Net.HttpWebRequest]::Create($env:DIMA3D_ZIP); $req.UserAgent = 'Dima3D'; \
    $resp = $req.GetResponse(); $total = $resp.ContentLength; $in = $resp.GetResponseStream(); \
    $out = [IO.File]::Create($zip); $buf = New-Object byte[] 262144; $done = 0; \
    $sw = [Diagnostics.Stopwatch]::StartNew(); $next = 0; \
    try { while (($n = $in.Read($buf, 0, $buf.Length)) -gt 0) { $out.Write($buf, 0, $n); $done += $n; \
        if ($sw.ElapsedMilliseconds -ge $next) { $next = $sw.ElapsedMilliseconds + 150; \
        [Console]::Out.WriteLine(\"P $done $total\"); [Console]::Out.Flush() } } } \
    finally { $out.Close(); $in.Close(); $resp.Close() }; \
    [Console]::Out.WriteLine(\"P $done $total\"); [Console]::Out.WriteLine('E'); [Console]::Out.Flush(); \
    Expand-Archive -Path $zip -DestinationPath (Join-Path $env:DIMA3D_DIR 'file') -Force";

/// Scarica ed estrae lo zip, mandando l'avanzamento a `tx`. `child` tiene il processo, per
/// poterlo fermare con «Annulla».
fn download(release: &Release, tx: &std::sync::mpsc::Sender<DownloadEvent>, child: &ChildSlot) -> Result<PathBuf, String> {
    use std::io::BufRead;
    let dir = std::env::temp_dir().join("Dima3D-aggiornamento");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dir_text = dir.to_string_lossy().into_owned();
    let mut command = powershell_command(DOWNLOAD_SCRIPT, &[("DIMA3D_ZIP", &release.zip_url), ("DIMA3D_DIR", &dir_text)]);
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut process = command.spawn().map_err(|e| format!("PowerShell non disponibile: {e}"))?;
    let stdout = process.stdout.take().ok_or("uscita di PowerShell non leggibile")?;
    let mut stderr = process.stderr.take();
    *child.lock().unwrap() = Some(process);
    for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("P") => {
                let mut n = || words.next().and_then(|w| w.parse::<i64>().ok()).unwrap_or(0).max(0) as u64;
                let (done, total) = (n(), n());
                let _ = tx.send(DownloadEvent::Progress(done, total));
            }
            Some("E") => {
                let _ = tx.send(DownloadEvent::Extracting);
            }
            _ => {}
        }
    }
    let Some(mut process) = child.lock().unwrap().take() else {
        return Err("annullato".into());
    };
    let status = process.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        let mut err = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = std::io::Read::read_to_end(s, &mut err);
        }
        return Err(powershell_error(&err, "download non riuscito"));
    }
    find_package(&dir.join("file")).ok_or_else(|| format!("nello zip non c'è {EXE}"))
}

/// Processo del download, condiviso con la finestra per «Annulla».
type ChildSlot = std::sync::Arc<std::sync::Mutex<Option<std::process::Child>>>;

/// Download in corso.
struct Download {
    rx: Receiver<DownloadEvent>,
    release: Release,
    child: ChildSlot,
    done: u64,
    total: u64,
    extracting: bool,
    /// Ultimi campioni (istante, byte) per la velocità media degli ultimi secondi
    samples: std::collections::VecDeque<(std::time::Instant, u64)>,
}

impl Download {
    /// Byte al secondo negli ultimi 3 s.
    fn speed(&self) -> Option<f64> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        let seconds = last.0.duration_since(first.0).as_secs_f64();
        (seconds > 0.3).then(|| (last.1 - first.1) as f64 / seconds)
    }
}

/// `12,3 MB`
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0).replace('.', ",")
}

/// `1 min 20 s`, `45 s`
fn duration_text(seconds: f64) -> String {
    let s = seconds.round().max(1.0) as u64;
    if s >= 60 { format!("{} min {} s", s / 60, s % 60) } else { format!("{s} s") }
}

/// Cartella che contiene l'exe (lo zip ha una cartella `Dima3D-x.y.z` con dentro i file).
fn find_package(root: &Path) -> Option<PathBuf> {
    if root.join(EXE).is_file() {
        return Some(root.to_path_buf());
    }
    std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).find_map(|p| find_package(&p))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Mette i file nuovi al posto di quelli della cartella `install` (dove sta l'exe in uso).
/// L'exe in uso viene rinominato, non sovrascritto; se qualcosa va storto torna al suo posto.
fn install_into(package: &Path, install: &Path) -> Result<(), String> {
    let exe = install.join(EXE);
    let old = install.join(format!("{EXE}.old"));
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("la cartella del programma non si può modificare ({e})"))?;
    if let Err(e) = copy_dir(package, install) {
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("copia dei file nuovi non riuscita ({e})"));
    }
    Ok(())
}

/// Si può scrivere nella cartella (es. no in «Programmi» senza permessi da amministratore).
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".dima3d-prova-scrittura");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(e) => e.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

/// Testo PowerShell tra apici singoli (l'apice si raddoppia).
fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// Script che installa da amministratore: aspetta che il programma `pid` si chiuda, mette i
/// file nuovi al posto dei vecchi (se non riesce rimette l'exe com'era) e, con `launch`, riapre
/// il programma come utente normale (tramite Esplora risorse, non da amministratore).
fn elevated_script(package: &Path, install: &Path, pid: u32, launch: bool) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         $pkg = {pkg}\r\n$dest = {dest}\r\n\
         try {{ Wait-Process -Id {pid} -Timeout 60 -ErrorAction SilentlyContinue }} catch {{ }}\r\n\
         $exe = Join-Path $dest '{EXE}'\r\n$old = \"$exe.old\"\r\n\
         try {{\r\n\
           Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue\r\n\
           Move-Item -LiteralPath $exe -Destination $old -Force\r\n\
           Get-ChildItem -LiteralPath $pkg | Copy-Item -Destination $dest -Recurse -Force\r\n\
           Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue\r\n\
         }} catch {{\r\n\
           if (-not (Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $old)) {{ Move-Item -LiteralPath $old -Destination $exe -Force }}\r\n\
         }}\r\n\
         {start}",
        pkg = quoted(package),
        dest = quoted(install),
        start = if launch { "Start-Process -FilePath 'explorer.exe' -ArgumentList ('\"' + $exe + '\"')\r\n" } else { "" },
    )
}

/// Avvia l'installazione da amministratore: Windows chiede il permesso (UAC). Lo script parte
/// e aspetta che il programma si chiuda; se l'utente rifiuta, errore.
fn install_elevated(package: &Path, install: &Path) -> Result<(), String> {
    let script = std::env::temp_dir().join("Dima3D-aggiornamento").join("installa.ps1");
    std::fs::write(&script, elevated_script(package, install, std::process::id(), true)).map_err(|e| e.to_string())?;
    let launcher = "Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden \
        -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"' + $env:DIMA3D_SCRIPT + '\"')";
    let script_text = script.to_string_lossy().into_owned();
    let out = powershell_command(launcher, &[("DIMA3D_SCRIPT", &script_text)]).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err("serve il permesso di amministratore per scrivere nella cartella del programma, e non è stato dato".into())
    }
}

/// Installa il pacchetto scaricato e riavvia il programma. Ritorna solo in caso di errore.
/// Se la cartella del programma non è scrivibile (es. «Programmi») chiede il permesso a Windows:
/// in quel caso i file li copia lo script da amministratore, dopo la chiusura del programma.
pub fn install_and_restart(package: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let install = current.parent().ok_or("cartella del programma non trovata")?;
    if !writable(install) {
        return install_elevated(package, install);
    }
    install_into(package, install)?;
    std::process::Command::new(install.join(EXE)).spawn().map_err(|e| format!("riavvio non riuscito ({e})"))?;
    Ok(())
}

/// Cancella l'exe della versione precedente lasciato da un aggiornamento.
pub fn cleanup_previous() {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        let _ = std::fs::remove_file(dir.join(format!("{EXE}.old")));
    }
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("Dima3D-aggiornamento"));
}

impl crate::app::GestoreApp {
    /// Chiede a GitHub l'ultima versione; `manual` mostra anche l'esito se non c'è niente.
    pub fn check_for_updates(&mut self, manual: bool) {
        if self.update.busy() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch_latest());
        });
        self.update.check = Some((rx, manual));
        if manual {
            self.update.message = Some("Verifica in corso…".into());
        }
    }

    /// Controlla verifica e download in corso.
    pub fn poll_update(&mut self, ctx: &egui::Context) {
        if let Some((rx, manual)) = &self.update.check {
            let manual = *manual;
            match rx.try_recv() {
                Ok(result) => {
                    self.update.check = None;
                    let current = env!("CARGO_PKG_VERSION");
                    match result {
                        Ok(r) if is_newer(&r.version, current) && (manual || r.version != self.settings.skipped_version) => {
                            self.update.message = None;
                            self.update.offer = Some(r);
                        }
                        Ok(_) if manual => {
                            self.update.message = Some(format!("Hai già l'ultima versione ({current})."));
                        }
                        Err(e) if manual => self.update.message = Some(format!("Verifica non riuscita: {e}.")),
                        Err(e) => tracing::info!("Verifica aggiornamenti: {e}"),
                        Ok(_) => {}
                    }
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        let mut finished = None;
        if let Some(d) = &mut self.update.download {
            while let Ok(event) = d.rx.try_recv() {
                match event {
                    DownloadEvent::Progress(done, total) => {
                        let now = std::time::Instant::now();
                        d.done = done;
                        d.total = total;
                        d.samples.push_back((now, done));
                        while d.samples.front().is_some_and(|s| now.duration_since(s.0).as_secs_f64() > 3.0) {
                            d.samples.pop_front();
                        }
                    }
                    DownloadEvent::Extracting => d.extracting = true,
                    DownloadEvent::Finished(result) => finished = Some(result),
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(result) = finished {
            let release = self.update.download.take().map(|d| d.release);
            match result {
                Ok(package) => self.request_install(ctx, package),
                Err(e) if e == "annullato" => self.update.message = Some("Aggiornamento annullato.".into()),
                Err(e) => {
                    let version = release.map(|r| r.version).unwrap_or_default();
                    self.update.message = Some(format!("Aggiornamento a {version} non riuscito: {e}."));
                }
            }
        }
    }

    fn start_download(&mut self, release: Release) {
        let (tx, rx) = channel();
        let child: ChildSlot = Default::default();
        let (r, slot) = (release.clone(), child.clone());
        std::thread::spawn(move || {
            let result = download(&r, &tx, &slot);
            let _ = tx.send(DownloadEvent::Finished(result));
        });
        self.update.message = None;
        self.update.download = Some(Download {
            rx,
            release,
            child,
            done: 0,
            total: 0,
            extracting: false,
            samples: Default::default(),
        });
    }

    /// Ferma il download in corso.
    fn cancel_download(&mut self) {
        if let Some(d) = &self.update.download {
            if let Some(mut process) = d.child.lock().unwrap().take() {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
    }

    /// Installa subito, o dopo la conferma se ci sono modifiche non salvate.
    fn request_install(&mut self, ctx: &egui::Context, package: PathBuf) {
        self.update.message = None;
        if self.is_dirty() {
            self.project.pending = Some(crate::project::Pending::Update(package));
        } else if self.install_update(&package) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Installa il pacchetto e chiude il programma (la versione nuova si apre da sola).
    pub fn install_update(&mut self, package: &Path) -> bool {
        match install_and_restart(package) {
            Ok(()) => {
                self.project.allow_exit = true;
                self.update.message = None;
                true
            }
            Err(e) => {
                self.update.message = Some(format!("Aggiornamento non riuscito: {e}. Puoi scaricare lo zip dalla pagina del progetto."));
                false
            }
        }
    }
}

/// Finestra della versione nuova e degli esiti.
pub fn draw(ctx: &egui::Context, app: &mut crate::app::GestoreApp) {
    app.poll_update(ctx);
    if let Some(release) = app.update.offer.clone() {
        let mut close = false;
        egui::Window::new("Aggiornamento disponibile")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(format!(
                    "È disponibile {} {} (questa è la {}).",
                    crate::APP_NAME,
                    release.version,
                    env!("CARGO_PKG_VERSION")
                ));
                if !release.notes.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Novità").strong());
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.label(&release.notes);
                    });
                }
                if !release.page_url.is_empty() {
                    ui.hyperlink_to("Pagina della versione", &release.page_url);
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Il programma scarica il pacchetto, sostituisce i suoi file e si riapre.")
                        .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("Aggiorna adesso").clicked() {
                        app.start_download(release.clone());
                        close = true;
                    }
                    if ui.button("Più tardi").clicked() {
                        close = true;
                    }
                    if ui.button("Salta questa versione").on_hover_text("Non avvisare più per questa versione all'avvio").clicked() {
                        let mut settings = app.settings.clone();
                        settings.skipped_version = release.version.clone();
                        app.apply_settings(settings);
                        close = true;
                    }
                });
            });
        if close {
            app.update.offer = None;
        }
    }
    if let Some(d) = &app.update.download {
        let mut cancel = false;
        let title = format!("Aggiornamento a {} {}", crate::APP_NAME, d.release.version);
        egui::Window::new(title)
            .id(egui::Id::new("finestra_download"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                if d.extracting {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Estrazione dei file…");
                    });
                    return;
                }
                let fraction = if d.total > 0 { d.done as f32 / d.total as f32 } else { 0.0 };
                let amount = if d.total > 0 {
                    format!("{} di {}", megabytes(d.done), megabytes(d.total))
                } else {
                    megabytes(d.done)
                };
                ui.add(egui::ProgressBar::new(fraction).text(amount).animate(d.total == 0));
                let speed = d.speed();
                let detail = match speed {
                    Some(s) if s > 0.0 => {
                        let rate = format!("{}/s", megabytes(s as u64));
                        if d.total > d.done {
                            format!("{rate}  ·  mancano circa {}", duration_text((d.total - d.done) as f64 / s))
                        } else {
                            rate
                        }
                    }
                    _ if d.done == 0 => "Collegamento a GitHub…".to_string(),
                    _ => "In attesa di dati…".to_string(),
                };
                ui.label(egui::RichText::new(detail).weak());
                if ui.button("Annulla").clicked() {
                    cancel = true;
                }
            });
        if cancel {
            app.cancel_download();
        }
    }
    if let Some(message) = app.update.message.clone() {
        let busy = app.update.busy();
        let mut open = true;
        egui::Window::new("Aggiornamenti")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if busy {
                        ui.spinner();
                    }
                    ui.label(&message);
                });
            });
        if !open && !busy {
            app.update.message = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("v0.2.3", "0.2.2"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.1"));
        assert!(!is_newer("v0.2.2", "0.2.2"));
        assert!(!is_newer("0.2.1", "0.2.2"));
    }

    #[test]
    fn release_text_is_checked() {
        let ok = "v0.3.0\nhttps://github.com/matteo80/dima3d/releases/download/v0.3.0/Dima3D-0.3.0.zip\nhttps://github.com/matteo80/dima3d/releases/tag/v0.3.0\n- Novità uno\n- Novità due\n";
        let r = parse_release(ok).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.notes, "- Novità uno\n- Novità due");
        // Solo zip delle Release del progetto
        assert!(parse_release("v0.3.0\nhttps://example.com/x.zip\n\n").is_err());
        assert!(parse_release("v0.3.0\n\n\n").is_err());
        assert!(parse_release("").is_err());
    }

    /// Verifica vera su GitHub (rete): `cargo test verifica_github -- --ignored --nocapture`;
    /// con `DIMA3D_SCARICA=1` scarica ed estrae anche lo zip.
    #[test]
    #[ignore]
    fn verifica_github() {
        let release = fetch_latest();
        println!("{release:?}");
        if let (Ok(r), true) = (&release, std::env::var("DIMA3D_SCARICA").is_ok()) {
            let (tx, rx) = channel();
            let result = download(r, &tx, &Default::default());
            let events: Vec<DownloadEvent> = rx.try_iter().collect();
            println!("{} eventi, ultimi {:?}", events.len(), &events[events.len().saturating_sub(2)..]);
            println!("pacchetto: {result:?}");
        }
    }

    /// Lo script da amministratore (qui eseguito senza permessi speciali e senza riavvio) mette i
    /// file nuovi al posto dei vecchi, unisce le sottocartelle e toglie l'exe vecchio; percorsi con
    /// spazi e apici.
    #[test]
    fn elevated_script_replaces_the_files() {
        let base = std::env::temp_dir().join("dima3d prova l'amministratore");
        let _ = std::fs::remove_dir_all(&base);
        let (package, install) = (base.join("pacchetto"), base.join("Programmi").join("Dima 3D"));
        std::fs::create_dir_all(package.join("licenses")).unwrap();
        std::fs::create_dir_all(install.join("licenses")).unwrap();
        std::fs::write(package.join(EXE), "nuovo").unwrap();
        std::fs::write(package.join("licenses").join("a.txt"), "nuova").unwrap();
        std::fs::write(install.join(EXE), "vecchio").unwrap();
        std::fs::write(install.join("licenses").join("b.txt"), "resta").unwrap();
        let script = base.join("installa.ps1");
        // Processo inesistente: non c'è da aspettare
        std::fs::write(&script, elevated_script(&package, &install, 999_999, false)).unwrap();
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(install.join(EXE)).unwrap(), "nuovo");
        assert_eq!(std::fs::read_to_string(install.join("licenses").join("a.txt")).unwrap(), "nuova");
        assert!(install.join("licenses").join("b.txt").exists());
        assert!(!install.join("licenses").join("licenses").exists(), "niente cartelle annidate");
        assert!(!install.join(format!("{EXE}.old")).exists(), "exe vecchio tolto");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// La finestra del download mostra quanto manca, la velocità e «Annulla».
    #[test]
    fn download_window_shows_progress_and_speed() {
        let mut app = crate::app::GestoreApp::new_headless();
        let (tx, rx) = channel();
        let release = parse_release(
            "v9.0.0\nhttps://github.com/matteo80/dima3d/releases/download/v9.0.0/Dima3D-9.0.0.zip\n\n",
        )
        .unwrap();
        let start = std::time::Instant::now() - std::time::Duration::from_secs(2);
        app.update.download = Some(Download {
            rx,
            release,
            child: Default::default(),
            done: 0,
            total: 0,
            extracting: false,
            samples: [(start, 0)].into_iter().collect(),
        });
        tx.send(DownloadEvent::Progress(12 * 1_048_576, 36 * 1_048_576)).unwrap();
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(Default::default(), |ctx| draw(ctx, &mut app));
            texts.clear();
            for c in &out.shapes {
                if let egui::epaint::Shape::Text(t) = &c.shape {
                    texts.push(t.galley.text().to_string());
                }
            }
        }
        assert!(texts.iter().any(|t| t == "12,0 MB di 36,0 MB"), "{texts:?}");
        // 12 MB in 2 s: 6 MB/s, mancano 24 MB → 4 s
        assert!(texts.iter().any(|t| t.starts_with("6,0 MB/s") && t.ends_with("mancano circa 4 s")), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Annulla"), "{texts:?}");
        drop(tx);
    }

    #[test]
    fn sizes_and_times_are_readable() {
        assert_eq!(megabytes(36_290_000), "34,6 MB");
        assert_eq!(duration_text(45.2), "45 s");
        assert_eq!(duration_text(80.0), "1 min 20 s");
        assert!(writable(&std::env::temp_dir()));
    }

    /// I file nuovi prendono il posto dei vecchi, l'exe in uso resta come `.old`; se la copia
    /// non riesce l'exe torna al suo posto.
    #[test]
    fn install_replaces_files_and_keeps_the_old_exe() {
        let base = std::env::temp_dir().join("dima3d_test_aggiornamento");
        let _ = std::fs::remove_dir_all(&base);
        let (package, install) = (base.join("pacchetto").join("Dima3D-9.9.9"), base.join("installato"));
        std::fs::create_dir_all(package.join("licenses")).unwrap();
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(package.join(EXE), "nuovo").unwrap();
        std::fs::write(package.join("licenses").join("a.txt"), "licenza").unwrap();
        std::fs::write(install.join(EXE), "vecchio").unwrap();
        std::fs::write(install.join("preferenza.txt"), "resta").unwrap();

        assert_eq!(find_package(&base.join("pacchetto")), Some(package.clone()));
        install_into(&package, &install).unwrap();
        assert_eq!(std::fs::read_to_string(install.join(EXE)).unwrap(), "nuovo");
        assert_eq!(std::fs::read_to_string(install.join(format!("{EXE}.old"))).unwrap(), "vecchio");
        assert_eq!(std::fs::read_to_string(install.join("licenses").join("a.txt")).unwrap(), "licenza");
        assert!(install.join("preferenza.txt").exists(), "gli altri file restano");

        // Pacchetto sparito a metà: l'exe in uso torna com'era
        std::fs::write(install.join(EXE), "in uso").unwrap();
        assert!(install_into(&base.join("non-c-e"), &install).is_err());
        assert_eq!(std::fs::read_to_string(install.join(EXE)).unwrap(), "in uso");
        let _ = std::fs::remove_dir_all(&base);
    }
}
