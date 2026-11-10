use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use crate::scene_graph::{SceneGraph, SourceData};

/// Evento di avanzamento durante il caricamento e parsing del file WRL.
#[derive(Debug)]
pub enum LoadEvent {
    Started { path: String, file_size_bytes: u64 },
    Progress { percent: f32, message: String },
    Finished(SceneGraph),
    Error(String),
}

/// Avvia il caricamento asincrono di un file WRL in un thread di background.
/// Utilizza `memmap2` per la mappatura diretta O(1) della memoria, senza copiare l'intero file.
pub fn load_wrl_async(path: PathBuf, sender: Sender<LoadEvent>) {
    load_wrl_async_with(path, crate::clusterer::ClusterConfig::default(), sender);
}

/// Come `load_wrl_async`, con i parametri del raggruppamento automatico scelti dall'utente.
pub fn load_wrl_async_with(path: PathBuf, cluster: crate::clusterer::ClusterConfig, sender: Sender<LoadEvent>) {
    std::thread::spawn(move || load_blocking(&path, &path, cluster, &sender, 0.0, Vec::new()));
}

/// Legge `path` e invia gli eventi di caricamento. Il file compare nella scena come `shown_as`
/// (diverso da `path` per un JT convertito: si mostra e si riapre il `.jt` originale).
/// L'avanzamento parte da `start` (dopo una conversione) e arriva a 1. `warnings` si aggiungono
/// agli avvisi della scena.
pub fn load_blocking(
    path: &Path,
    shown_as: &Path,
    cluster: crate::clusterer::ClusterConfig,
    sender: &Sender<LoadEvent>,
    start: f32,
    warnings: Vec<String>,
) {
    let scaled = move |p: f32| start + p * (1.0 - start);
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            let _ = sender.send(LoadEvent::Error(format!("Impossibile aprire il file '{}': {e}", path.display())));
            return;
        }
    };
    let file_size = match file.metadata() {
        Ok(m) => m.len(),
        Err(e) => {
            let _ = sender.send(LoadEvent::Error(format!(
                "Impossibile leggere le dimensioni di '{}': {e}",
                path.display()
            )));
            return;
        }
    };
    if file_size == 0 {
        let _ = sender.send(LoadEvent::Error("Il file selezionato è vuoto (0 byte)".to_string()));
        return;
    }
    if start == 0.0 {
        let _ = sender.send(LoadEvent::Started { path: shown_as.to_string_lossy().to_string(), file_size_bytes: file_size });
    }
    let _ = sender.send(LoadEvent::Progress {
        percent: scaled(0.05),
        message: format!("Memory mapping con memmap2 ({:.2} MB)...", file_size as f64 / (1024.0 * 1024.0)),
    });

    let progress = {
        let sender = sender.clone();
        move |p: f32, msg: String| {
            let _ = sender.send(LoadEvent::Progress { percent: scaled(0.10 + p * 0.85), message: msg });
        }
    };
    let finish = |mut scene: SceneGraph| {
        scene.into_file(file_label(shown_as), Some(shown_as.to_path_buf()));
        // Dima 3D lavora in millimetri. HSF e STEP dichiarano le unità (il lettore e il
        // convertitore danno millimetri); gli altri, se hanno coordinate da metri, si convertono
        let step = shown_as
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stp") || e.eq_ignore_ascii_case("step"));
        if !scene.units_mm && !step && scene.file_in_meters(0) {
            scene.scale_file(0, 1000.0);
        }
        scene.load_warnings.extend(warnings);
        let _ = sender.send(LoadEvent::Finished(scene));
    };

    // Tentativo di memory-mapping con memmap2
    match unsafe { memmap2::MmapOptions::new().map(&file) } {
        Ok(mmap) => {
            tracing::info!("File mappato con successo in memoria: {} bytes", mmap.len());
            let mut scene = super::shape_extractor::parse_wrl_with_config(&mmap, cluster, progress);
            if scene.source_data.is_empty() {
                scene.source_data = SourceData::Mmap(Arc::new(mmap));
            }
            finish(scene);
        }
        Err(mmap_err) => {
            tracing::warn!("memmap2 non riuscito ({mmap_err}); fallback a lettura su buffer heap");
            let _ = sender.send(LoadEvent::Progress {
                percent: scaled(0.10),
                message: "Fallback: lettura del file in buffer RAM...".to_string(),
            });
            match std::fs::read(path) {
                Ok(buffer) => {
                    let mut scene = super::shape_extractor::parse_wrl_with_config(&buffer, cluster, progress);
                    if scene.source_data.is_empty() {
                        scene.source_data = SourceData::Memory(Arc::new(buffer));
                    }
                    finish(scene);
                }
                Err(read_err) => {
                    let _ = sender.send(LoadEvent::Error(format!("Errore durante la lettura del file: {read_err}")));
                }
            }
        }
    }
}
/// Nome del nodo radice di un file: il nome del file con estensione.
pub fn file_label(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "File".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_async_loader_with_memmap() {
        let dir = std::env::temp_dir();
        let file_path = dir.join("test_async_loader.wrl");

        let content = b"#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }\n";
        {
            let mut f = std::fs::File::create(&file_path).expect("creazione file temp");
            f.write_all(content).expect("scrittura file temp");
        }

        let (tx, rx) = std::sync::mpsc::channel();
        load_wrl_async(file_path.clone(), tx);

        let mut received_finished = false;
        while let Ok(event) = rx.recv() {
            match event {
                LoadEvent::Finished(scene) => {
                    // Radice del file + la Shape
                    assert_eq!(scene.nodes.len(), 2);
                    assert_eq!(scene.display_name(scene.roots[0]), "test_async_loader.wrl");
                    assert_eq!(scene.sources.len(), 1);
                    assert!(!scene.sources[0].data.is_empty() && scene.source_data.is_empty());
                    received_finished = true;
                    break;
                }
                LoadEvent::Error(err) => {
                    panic!("Errore inaspettato durante il caricamento: {err}");
                }
                _ => {}
            }
        }

        assert!(received_finished, "Deve ricevere evento Finished");
        let _ = std::fs::remove_file(file_path);
    }

    /// Un file VRML 1.0 si apre, e per l'export resta il testo convertito, non quello originale.
    /// Dima 3D lavora in millimetri: un WRL con coordinate da metri (come quelli della stazione)
    /// si converte all'apertura, uno con coordinate da millimetri resta com'è.
    #[test]
    fn files_in_meters_are_converted_to_millimeters() {
        let load = |name: &str, points: &str| {
            let path = std::env::temp_dir().join(name);
            let wrl = format!(
                "#VRML V2.0 utf8\nShape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ {points} ] }} coordIndex [ 0 1 2 -1 ] }} }}\n"
            );
            std::fs::write(&path, wrl).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            load_wrl_async(path.clone(), tx);
            let scene = loop {
                match rx.recv().unwrap() {
                    LoadEvent::Finished(scene) => break scene,
                    LoadEvent::Error(e) => panic!("{e}"),
                    _ => {}
                }
            };
            let _ = std::fs::remove_file(path);
            scene
        };
        // Sportello in metri, nel sistema del veicolo
        let meters = load("dima3d_unita_metri.wrl", "2.9 -0.9 0.65, 3.1 -0.9 0.65, 2.9 -0.9 1.6");
        assert_eq!(meters.sources[0].scale, 1000.0);
        let v = meters.shapes_data[0].world_vertices[0];
        assert!((v[0] - 2900.0).abs() < 1e-2 && (v[2] - 650.0).abs() < 1e-2, "{v:?}");
        // Lo stesso in millimetri
        let millimeters = load("dima3d_unita_mm.wrl", "2900 -900 650, 3100 -900 650, 2900 -900 1600");
        assert_eq!(millimeters.sources[0].scale, 1.0);
        assert_eq!(millimeters.shapes_data[0].world_vertices[0], [2900.0, -900.0, 650.0]);
    }

    #[test]
    fn test_vrml1_opens_and_keeps_the_converted_text() {
        let file_path = std::env::temp_dir().join("test_loader_vrml1.wrl");
        std::fs::write(
            &file_path,
            b"#VRML V1.0 ascii\nSeparator { Coordinate3 { point [ 0 0 0, 1 0 0, 0 1 0 ] } IndexedFaceSet { coordIndex [ 0, 1, 2, -1 ] } }\n",
        )
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        load_wrl_async(file_path.clone(), tx);
        let mut loaded = None;
        while let Ok(event) = rx.recv() {
            match event {
                LoadEvent::Error(e) => panic!("{e}"),
                LoadEvent::Finished(scene) => loaded = Some(scene),
                _ => {}
            }
        }
        let scene = loaded.expect("Deve ricevere evento Finished");
        assert_eq!(scene.shapes_data.len(), 1);
        assert!(scene.sources[0].data.starts_with(b"#VRML V2.0"));
        let _ = std::fs::remove_file(file_path);
    }
}
