//! Apertura dei formati letti da un convertitore esterno: conversione in VRML 2.0, poi caricamento
//! del WRL come un file qualsiasi.
//! - JT: `tools\jt2wrl.exe` (`tools-src/jt2wrl`, basato su JTReader);
//! - STEP: `tools\step2wrl\step2wrl.exe` (`tools-src/step2wrl`, basato su OpenCASCADE), con le
//!   tolleranze di tassellazione scelte dall'utente.
//!
//! Il convertitore è un processo separato: se un file danneggiato lo fa terminare, il Gestore
//! resta aperto e mostra l'errore. Il WRL prodotto va nella cartella temporanea e resta finché il
//! file è aperto (la scena lo tiene mappato in memoria per l'export).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use super::loader::{load_blocking, LoadEvent};

/// Quota della barra di avanzamento dedicata alla conversione (il resto è la lettura del WRL).
const CONVERSION_SHARE: f32 = 0.5;

/// Formato da convertire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Jt,
    Step(crate::settings::StepMesh),
}

impl Kind {
    /// Formato del file dall'estensione (`None`: lo legge direttamente il Gestore).
    pub fn of(path: &Path, step: crate::settings::StepMesh) -> Option<Kind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "jt" => Some(Kind::Jt),
            "stp" | "step" => Some(Kind::Step(step)),
            _ => None,
        }
    }

    /// Nome del formato nei messaggi.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Jt => "JT",
            Kind::Step(_) => "STEP",
        }
    }

    /// Variabile d'ambiente con un convertitore diverso (prove) e posizioni accanto al Gestore.
    fn locations(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Kind::Jt => ("GESTORE_JT2WRL", &["tools\\jt2wrl.exe", "jt2wrl.exe"]),
            // Le DLL di OpenCASCADE stanno nella cartella del convertitore
            Kind::Step(_) => ("GESTORE_STEP2WRL", &["tools\\step2wrl\\step2wrl.exe", "step2wrl\\step2wrl.exe"]),
        }
    }

    /// Argomenti dopo `<entrata> <uscita>`.
    fn extra_args(self) -> Vec<String> {
        match self {
            Kind::Jt => Vec::new(),
            Kind::Step(m) => {
                let mut a = vec!["--deviazione".into(), m.deviation.to_string(), "--angolo".into(), m.angle.to_string()];
                if m.relative {
                    a.push("--relativa".into());
                }
                a
            }
        }
    }
}

/// Vero per i file `.stp`/`.step`.
pub fn is_step(path: &Path) -> bool {
    matches!(Kind::of(path, Default::default()), Some(Kind::Step(_)))
}

/// Il convertitore del formato (vedi `Kind::locations`).
pub fn converter_path(kind: Kind) -> Option<PathBuf> {
    let (var, names) = kind.locations();
    if let Some(p) = std::env::var_os(var).map(PathBuf::from) {
        return p.exists().then_some(p);
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    names.iter().map(|n| dir.join(n)).find(|p| p.exists())
}

/// Cartella dei WRL convertiti.
fn work_dir() -> PathBuf {
    std::env::temp_dir().join("dima3d-conversioni")
}

/// Elimina le conversioni rimaste da sessioni precedenti (quelle ancora aperte da un altro
/// Dima 3D sono mappate in memoria: Windows non le lascia cancellare, e restano).
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
fn error_message(label: &str, code: Option<i32>, stderr: &[String]) -> String {
    let detail: Vec<&str> =
        stderr.iter().map(String::as_str).filter(|l| !l.starts_with("Avviso:") && !l.trim().is_empty()).collect();
    match (code, detail.last()) {
        (_, Some(last)) => format!("Conversione {label} non riuscita: {last}"),
        // 0xC0000135: manca una DLL accanto al convertitore
        (Some(-1073741515), None) => format!("Convertitore {label} incompleto: manca una DLL nella sua cartella"),
        (Some(c), None) => format!("Conversione {label} non riuscita (codice {c})"),
        (None, None) => format!("Il convertitore {label} si è chiuso in modo inatteso"),
    }
}

/// Converte il file in un thread e poi lo carica, inviando gli eventi come per un WRL.
pub fn convert_and_load(source: PathBuf, kind: Kind, cluster: crate::clusterer::ClusterConfig, sender: Sender<LoadEvent>) {
    std::thread::spawn(move || {
        let size = std::fs::metadata(&source).map(|m| m.len()).unwrap_or(0);
        let _ = sender.send(LoadEvent::Started { path: source.to_string_lossy().into_owned(), file_size_bytes: size });
        match convert(&source, kind, &sender) {
            Ok((wrl, warnings)) => load_blocking(&wrl, &source, cluster, &sender, CONVERSION_SHARE, warnings),
            Err(e) => {
                let _ = sender.send(LoadEvent::Error(e));
            }
        }
    });
}

/// Esegue il convertitore; ritorna il WRL prodotto e gli avvisi del convertitore.
fn convert(source: &Path, kind: Kind, sender: &Sender<LoadEvent>) -> Result<(PathBuf, Vec<String>), String> {
    let label = kind.label();
    let converter = converter_path(kind).ok_or_else(|| {
        format!("Convertitore {label} non trovato: serve {} nella cartella di Dima 3D", kind.locations().1[0])
    })?;
    let dir = work_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cartella temporanea non disponibile: {e}"))?;
    // Nome unico: un file con lo stesso nome può essere ancora aperto (e mappato) nella scena
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let out = dir.join(format!("{stem}-{stamp}.wrl"));

    let mut command = std::process::Command::new(&converter);
    command.arg(source).arg(&out).args(kind.extra_args()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| format!("Avvio del convertitore {label} non riuscito: {e}"))?;

    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        stderr.map(|s| BufReader::new(s).lines().map_while(Result::ok).collect::<Vec<_>>()).unwrap_or_default()
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some((p, text)) = parse_progress(&line) {
                let _ = sender.send(LoadEvent::Progress { percent: p * CONVERSION_SHARE, message: format!("{label}: {text}") });
            }
        }
    }
    let status = child.wait().map_err(|e| format!("Convertitore {label}: {e}"))?;
    let stderr = errors.join().unwrap_or_default();
    if !status.success() || !out.exists() {
        let _ = std::fs::remove_file(&out);
        return Err(error_message(label, status.code(), &stderr));
    }
    let warnings = stderr
        .iter()
        .filter_map(|l| l.strip_prefix("Avviso:").map(|w| format!("{label}:{w}")))
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
        assert_eq!(error_message("JT", Some(2), &lines), "Conversione JT non riuscita: File JT salvato in formato big-endian: non supportato.");
        assert_eq!(error_message("STEP", Some(5), &[]), "Conversione STEP non riuscita (codice 5)");
        assert!(error_message("STEP", Some(-1073741515), &[]).contains("DLL"));
        let step = crate::settings::StepMesh { deviation: 0.05, angle: 15.0, relative: true };
        assert_eq!(Kind::of(Path::new("C:\\a\\pezzo.JT"), step), Some(Kind::Jt));
        assert_eq!(Kind::of(Path::new("a.Stp"), step), Some(Kind::Step(step)));
        assert!(is_step(Path::new("a.STEP")) && !is_step(Path::new("pezzo.wrl")) && Kind::of(Path::new("x"), step).is_none());
        assert_eq!(Kind::Step(step).extra_args(), ["--deviazione", "0.05", "--angolo", "15", "--relativa"]);
        assert!(Kind::Jt.extra_args().is_empty());
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
            convert_and_load(jt.clone(), Kind::Jt, Default::default(), tx);
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

    /// STEP reali con il convertitore vero, con due tolleranze: `GESTORE_STEP="a.stp|b.step"`,
    /// `GESTORE_STEP2WRL` = `target\release\tools\step2wrl\step2wrl.exe`.
    #[test]
    #[ignore]
    fn step_reali() {
        let list = std::env::var("GESTORE_STEP").expect("GESTORE_STEP");
        for src in list.split('|') {
            for mesh in [crate::settings::StepMesh::default(), crate::settings::StepMesh { deviation: 0.01, angle: 10.0, relative: false }] {
                let start = std::time::Instant::now();
                let (tx, rx) = std::sync::mpsc::channel();
                convert_and_load(PathBuf::from(src), Kind::Step(mesh), Default::default(), tx);
                let scene = loop {
                    match rx.recv().unwrap() {
                        LoadEvent::Finished(scene) => break scene,
                        LoadEvent::Error(e) => panic!("{src}: {e}"),
                        _ => {}
                    }
                };
                let tris: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
                let bb = scene.compute_bounding_box();
                println!(
                    "{}: {:?} → {} pezzi, {tris} triangoli, ingombro {:?}..{:?}, {:?}, avvisi {:?}",
                    Path::new(src).file_name().unwrap().to_string_lossy(),
                    (mesh.deviation, mesh.angle),
                    scene.shapes_data.len(),
                    bb.min,
                    bb.max,
                    start.elapsed(),
                    scene.load_warnings
                );
                assert!(tris > 0);
            }
        }
    }
}
