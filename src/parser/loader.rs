use std::path::PathBuf;
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
    std::thread::spawn(move || {
        let path_str = path.to_string_lossy().to_string();

        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                let _ = sender.send(LoadEvent::Error(format!(
                    "Impossibile aprire il file '{}': {e}",
                    path.display()
                )));
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
            let _ = sender.send(LoadEvent::Error(
                "Il file selezionato è vuoto (0 byte)".to_string(),
            ));
            return;
        }

        let _ = sender.send(LoadEvent::Started {
            path: path_str,
            file_size_bytes: file_size,
        });

        let _ = sender.send(LoadEvent::Progress {
            percent: 0.05,
            message: format!(
                "Memory mapping con memmap2 ({:.2} MB)...",
                file_size as f64 / (1024.0 * 1024.0)
            ),
        });

        // Tentativo di memory-mapping con memmap2
        let mmap_res = unsafe { memmap2::MmapOptions::new().map(&file) };

        match mmap_res {
            Ok(mmap) => {
                tracing::info!(
                    "File mappato con successo in memoria: {} bytes",
                    mmap.len()
                );
                let sender_clone = sender.clone();
                let mut scene = super::shape_extractor::parse_wrl_with_config(&mmap, cluster, move |p, msg| {
                    let _ = sender_clone.send(LoadEvent::Progress {
                        percent: 0.10 + p * 0.85,
                        message: msg,
                    });
                });

                if scene.source_data.is_empty() {
                    scene.source_data = SourceData::Mmap(Arc::new(mmap));
                }
                scene.into_file(file_label(&path), Some(path.clone()));
                let _ = sender.send(LoadEvent::Finished(scene));
            }
            Err(mmap_err) => {
                tracing::warn!(
                    "memmap2 non riuscito ({mmap_err}); fallback a lettura su buffer heap"
                );
                let _ = sender.send(LoadEvent::Progress {
                    percent: 0.10,
                    message: "Fallback: lettura del file in buffer RAM...".to_string(),
                });

                match std::fs::read(&path) {
                    Ok(buffer) => {
                        let sender_clone = sender.clone();
                        let mut scene = super::shape_extractor::parse_wrl_with_config(
                            &buffer,
                            cluster,
                            move |p, msg| {
                                let _ = sender_clone.send(LoadEvent::Progress {
                                    percent: 0.10 + p * 0.85,
                                    message: msg,
                                });
                            },
                        );

                        if scene.source_data.is_empty() {
                            scene.source_data = SourceData::Memory(buffer);
                        }
                        scene.into_file(file_label(&path), Some(path.clone()));
                        let _ = sender.send(LoadEvent::Finished(scene));
                    }
                    Err(read_err) => {
                        let _ = sender.send(LoadEvent::Error(format!(
                            "Errore durante la lettura del file: {read_err}"
                        )));
                    }
                }
            }
        }
    });
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
