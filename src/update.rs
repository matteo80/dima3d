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
    download: Option<(Receiver<Result<PathBuf, String>>, Release)>,
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

/// Esegue uno script PowerShell senza finestra; i dati arrivano dalle variabili d'ambiente.
fn powershell(script: &str, env: &[(&str, &str)]) -> Result<String, String> {
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
    let out = command.output().map_err(|e| format!("PowerShell non disponibile: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        // PowerShell va a capo a metà del messaggio: si riunisce
        let err: String = String::from_utf8_lossy(&out.stderr).split_whitespace().collect::<Vec<_>>().join(" ");
        if err.contains("Not Found") || err.contains("404") {
            return Err("su GitHub non c'è ancora una versione pubblicata".into());
        }
        let short: String = err.chars().take(160).collect();
        Err(format!("connessione a GitHub non riuscita ({})", if short.is_empty() { "errore sconosciuto" } else { &short }))
    }
}

fn fetch_latest() -> Result<Release, String> {
    let script = "$r = Invoke-RestMethod -UseBasicParsing -Uri $env:DIMA3D_API -Headers @{ 'User-Agent' = 'Dima3D' }; \
        $zip = ($r.assets | Where-Object { $_.name -like '*.zip' } | Select-Object -First 1).browser_download_url; \
        Write-Output $r.tag_name; Write-Output $zip; Write-Output $r.html_url; Write-Output $r.body";
    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    parse_release(&powershell(script, &[("DIMA3D_API", &api)])?)
}

/// Scarica ed estrae lo zip; ritorna la cartella con i file del pacchetto (quella dell'exe).
fn download(release: &Release) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("Dima3D-aggiornamento");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = "Invoke-WebRequest -UseBasicParsing -Uri $env:DIMA3D_ZIP -OutFile (Join-Path $env:DIMA3D_DIR 'pacchetto.zip') \
        -Headers @{ 'User-Agent' = 'Dima3D' }; \
        Expand-Archive -Path (Join-Path $env:DIMA3D_DIR 'pacchetto.zip') -DestinationPath (Join-Path $env:DIMA3D_DIR 'file') -Force";
    let dir_text = dir.to_string_lossy().into_owned();
    powershell(script, &[("DIMA3D_ZIP", &release.zip_url), ("DIMA3D_DIR", &dir_text)])
        .map_err(|e| e.replace("connessione a GitHub non riuscita", "download non riuscito"))?;
    find_package(&dir.join("file")).ok_or_else(|| format!("nello zip non c'è {EXE}"))
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

/// Installa il pacchetto scaricato e riavvia il programma. Ritorna solo in caso di errore.
pub fn install_and_restart(package: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let install = current.parent().ok_or("cartella del programma non trovata")?;
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
        if let Some((rx, _)) = &self.update.download {
            match rx.try_recv() {
                Ok(result) => {
                    let (_, release) = self.update.download.take().unwrap();
                    match result {
                        Ok(package) => self.request_install(ctx, package),
                        Err(e) => {
                            self.update.message = Some(format!("Aggiornamento a {} non riuscito: {e}.", release.version));
                        }
                    }
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
    }

    fn start_download(&mut self, release: Release) {
        let (tx, rx) = channel();
        let r = release.clone();
        std::thread::spawn(move || {
            let _ = tx.send(download(&r));
        });
        self.update.message = Some(format!("Scaricamento di {} {}…", crate::APP_NAME, release.version));
        self.update.download = Some((rx, release));
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
            println!("pacchetto: {:?}", download(r));
        }
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
