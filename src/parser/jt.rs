//! Apertura dei file JT: conversione in VRML 2.0 con il programma esterno `jt2wrl.exe`
//! (`tools-src/jt2wrl`, basato su JTReader), poi caricamento del WRL come un file qualsiasi.
//!
//! Il convertitore è un processo separato: se un JT danneggiato lo fa terminare, il Gestore resta
//! aperto e mostra l'errore. Il WRL prodotto va nella cartella temporanea e resta finché il file
//! è aperto (la scena lo tiene mappato in memoria per l'export).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use super::loader::{load_blocking, LoadEvent};

/// Quota della barra di avanzamento dedicata alla conversione (il resto è la lettura del WRL).
const CONVERSION_SHARE: f32 = 0.5;

/// Vero per i file `.jt`.
pub fn is_jt(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jt"))
}

/// Il convertitore: `tools\jt2wrl.exe` accanto al Gestore, oppure `jt2wrl.exe` nella stessa
/// cartella. `GESTORE_JT2WRL` indica un percorso diverso (prove).
pub fn converter_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GESTORE_JT2WRL").map(PathBuf::from) {
        return p.exists().then_some(p);
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [dir.join("tools").join("jt2wrl.exe"), dir.join("jt2wrl.exe")].into_iter().find(|p| p.exists())
}

/// Cartella dei WRL convertiti.
fn work_dir() -> PathBuf {
    std::env::temp_dir().join("gestore-wrl-jt")
}

/// Elimina le conversioni rimaste da sessioni precedenti (quelle ancora aperte da un altro
/// Gestore sono mappate in memoria: Windows non le lascia cancellare, e restano).
pub fn cleanup_old_conversions() {
    if let Ok(entries) = std::fs::read_dir(work_dir()) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Riga di avanzamento del convertitore: "PROGRESS 0.420 testo".
fn parse_progress(line: &str) -> Option<(f32, String)> {
    let rest = line.strip_prefix("PROGRESS ")?;
    let (value, text) = rest.split_once(' ').unwrap_or((rest, ""));
    Some((value.parse::<f32>().ok()?.clamp(0.0, 1.0), text.to_string()))
}

/// Messaggio d'errore per l'utente dal codice di uscita e da quanto scritto su stderr.
fn error_message(code: Option<i32>, stderr: &[String]) -> String {
    let detail: Vec<&str> =
        stderr.iter().map(String::as_str).filter(|l| !l.starts_with("Avviso:") && !l.trim().is_empty()).collect();
    match (code, detail.last()) {
        (_, Some(last)) => format!("Conversione JT non riuscita: {last}"),
        (Some(c), None) => format!("Conversione JT non riuscita (codice {c})"),
        (None, None) => "Il convertitore JT si è chiuso in modo inatteso".to_string(),
    }
}

/// Converte il JT in un thread e poi lo carica, inviando gli eventi come per un WRL.
pub fn convert_and_load(jt: PathBuf, cluster: crate::clusterer::ClusterConfig, sender: Sender<LoadEvent>) {
    std::thread::spawn(move || {
        let size = std::fs::metadata(&jt).map(|m| m.len()).unwrap_or(0);
        let _ = sender.send(LoadEvent::Started { path: jt.to_string_lossy().into_owned(), file_size_bytes: size });
        match convert(&jt, &sender) {
            Ok((wrl, warnings)) => load_blocking(&wrl, &jt, cluster, &sender, CONVERSION_SHARE, warnings),
            Err(e) => {
                let _ = sender.send(LoadEvent::Error(e));
            }
        }
    });
}

/// Esegue `jt2wrl`; ritorna il WRL prodotto e gli avvisi del convertitore.
fn convert(jt: &Path, sender: &Sender<LoadEvent>) -> Result<(PathBuf, Vec<String>), String> {
    let converter = converter_path().ok_or_else(|| {
        "Convertitore JT non trovato: serve tools\\jt2wrl.exe nella cartella del Gestore WRL".to_string()
    })?;
    let dir = work_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cartella temporanea non disponibile: {e}"))?;
    // Nome unico: un file con lo stesso nome può essere ancora aperto (e mappato) nella scena
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let stem = jt.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "jt".into());
    let out = dir.join(format!("{stem}-{stamp}.wrl"));

    let mut command = std::process::Command::new(&converter);
    command.arg(jt).arg(&out).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| format!("Avvio del convertitore JT non riuscito: {e}"))?;

    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        stderr.map(|s| BufReader::new(s).lines().map_while(Result::ok).collect::<Vec<_>>()).unwrap_or_default()
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((p, text)) = parse_progress(&line) {
                let _ = sender.send(LoadEvent::Progress { percent: p * CONVERSION_SHARE, message: format!("JT: {text}") });
            }
        }
    }
    let status = child.wait().map_err(|e| format!("Convertitore JT: {e}"))?;
    let stderr = errors.join().unwrap_or_default();
    if !status.success() || !out.exists() {
        let _ = std::fs::remove_file(&out);
        return Err(error_message(status.code(), &stderr));
    }
    let warnings = stderr
        .iter()
        .filter_map(|l| l.strip_prefix("Avviso:").map(|w| format!("JT:{w}")))
        .collect();
    Ok((out, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_and_errors_are_understood() {
        assert_eq!(parse_progress("PROGRESS 0.420 Conversione: 3 pezzi"), Some((0.42, "Conversione: 3 pezzi".into())));
        assert_eq!(parse_progress("PROGRESS 1.000"), Some((1.0, String::new())));
        assert_eq!(parse_progress("DONE 3 120 0"), None);
        let lines = vec!["Avviso: pezzo X non decodificato".to_string(), "File JT salvato in formato big-endian: non supportato.".to_string()];
        assert_eq!(error_message(Some(2), &lines), "Conversione JT non riuscita: File JT salvato in formato big-endian: non supportato.");
        assert_eq!(error_message(Some(5), &[]), "Conversione JT non riuscita (codice 5)");
        assert!(is_jt(Path::new("C:\\a\\pezzo.JT")) && !is_jt(Path::new("pezzo.wrl")));
    }

    /// Percorso completo con un convertitore finto (script .cmd che scrive un WRL di prova):
    /// avanzamento, avvisi, nome del file JT nella scena; poi un convertitore che fallisce.
    #[test]
    fn a_jt_is_converted_and_loaded() {
        let dir = std::env::temp_dir().join("gestore-prova-jt");
        std::fs::create_dir_all(&dir).unwrap();
        let wrl = dir.join("sorgente.wrl");
        std::fs::write(&wrl, include_bytes!("../../esempi/test_colori.wrl")).unwrap();
        let jt = dir.join("pezzo.jt");
        std::fs::write(&jt, b"finto").unwrap();
        let fake = dir.join("finto_jt2wrl.cmd");
        std::fs::write(
            &fake,
            format!(
                "@echo off\r\necho PROGRESS 0.500 meta\r\necho Avviso: un pezzo saltato 1>&2\r\ncopy /y \"{}\" %2 >nul\r\necho DONE 1 1 1\r\n",
                wrl.display()
            ),
        )
        .unwrap();
        let failing = dir.join("rotto_jt2wrl.cmd");
        std::fs::write(&failing, "@echo off\r\necho File JT salvato in formato big-endian: non supportato. 1>&2\r\nexit /b 2\r\n").unwrap();

        let run = |converter: &Path| {
            // Variabile letta solo da converter_path: i due casi girano in sequenza nello stesso test
            std::env::set_var("GESTORE_JT2WRL", converter);
            let (tx, rx) = std::sync::mpsc::channel();
            convert_and_load(jt.clone(), Default::default(), tx);
            let mut progress = Vec::new();
            loop {
                match rx.recv().unwrap() {
                    LoadEvent::Progress { percent, message } => progress.push((percent, message)),
                    LoadEvent::Finished(scene) => return Ok((scene, progress)),
                    LoadEvent::Error(e) => return Err(e),
                    LoadEvent::Started { .. } => {}
                }
            }
        };

        let (scene, progress) = run(&fake).expect("conversione riuscita");
        assert_eq!(scene.display_name(scene.roots[0]), "pezzo.jt");
        assert_eq!(scene.sources[0].path.as_deref(), Some(jt.as_path()));
        assert!(!scene.shapes_data.is_empty());
        assert!(scene.load_warnings.iter().any(|w| w.contains("un pezzo saltato")), "{:?}", scene.load_warnings);
        assert!(progress.iter().any(|(p, m)| (*p - 0.25).abs() < 1e-6 && m == "JT: meta"), "{progress:?}");
        assert!(progress.iter().all(|(p, _)| (0.0..=1.0).contains(p)));

        let err = run(&failing).err().expect("conversione fallita");
        assert!(err.contains("big-endian"), "{err}");
        std::env::remove_var("GESTORE_JT2WRL");
    }
}
