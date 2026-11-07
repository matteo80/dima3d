//! Riduzione delle facce nella scena aperta: calcolo in un thread, anteprima nella vista,
//! previsione della dimensione del WRL esportato e ripristino della geometria originale.
//!
//! I pezzi ridotti prendono il posto degli originali in `shapes_data` (così vista, selezione,
//! misure ed export lavorano già sul risultato); gli originali restano qui da parte. Ogni nuovo
//! calcolo riparte dagli originali, non dal risultato precedente.

use std::sync::atomic::AtomicU32;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use crate::app::GestoreApp;
use crate::exporter::ExportOptions;
use crate::parser::ShapeInfo;
use crate::reduce::{Params, Target};
use crate::scene_graph::SceneGraph;

/// Cosa fissa l'utente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Numero di facce totale
    #[default]
    Triangles,
    /// Scostamento massimo dalla superficie
    MaxError,
}

/// Valori del pannello "Riduci facce".
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub open: bool,
    pub mode: Mode,
    /// Facce da tenere (0 = da proporre all'apertura del pannello)
    pub triangles: u64,
    /// Errore massimo, nelle unità del file
    pub max_error: f32,
    pub lock_border: bool,
    /// Solo i pezzi selezionati invece di tutti quelli visibili
    pub selection_only: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Self { open: false, mode: Mode::Triangles, triangles: 0, max_error: 0.0, lock_border: false, selection_only: false }
    }
}

/// Esito dell'ultima riduzione.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Triangoli dei pezzi ridotti, prima e dopo
    pub before: u64,
    pub after: u64,
    /// Pezzi cambiati
    pub pieces: usize,
    /// Errore massimo usato (unità del file)
    pub max_error: f32,
    /// Dimensione del WRL esportato con e senza normali, con le opzioni di export correnti
    pub size_with_normals: u64,
    pub size_without_normals: u64,
    /// Formato compatto usato per la stima
    pub compact: bool,
    pub seconds: f32,
    /// Revisione della scena a cui si riferisce la stima
    pub revision: u64,
}

/// Calcolo in corso: la scena e gli originali sono nel thread e tornano con il risultato.
pub struct Job {
    rx: Receiver<(SceneGraph, Vec<Option<ShapeInfo>>, Summary)>,
    pub progress: Arc<AtomicU32>,
    pub started: Instant,
}

#[derive(Default)]
pub struct Reduction {
    pub panel: Panel,
    /// Geometria originale dei pezzi ridotti, indicizzata per mesh_id
    stash: Vec<Option<ShapeInfo>>,
    pub job: Option<Job>,
    pub summary: Option<Summary>,
}

impl Reduction {
    /// Ci sono pezzi ridotti nella scena.
    pub fn is_active(&self) -> bool {
        self.stash.iter().any(Option::is_some)
    }

    /// Dimentica originali e risultato (la scena è stata sostituita).
    pub fn forget_geometry(&mut self) {
        self.stash.clear();
        self.summary = None;
    }

    /// Scala anche gli originali messi da parte, come `SceneGraph::scale_file` fa con la scena.
    pub fn scale(&mut self, meshes: std::ops::Range<usize>, factor: f32) {
        let scale = glam::Mat4::from_scale(glam::Vec3::splat(factor));
        for s in self.stash.get_mut(meshes).into_iter().flatten().flatten() {
            for v in &mut s.world_vertices {
                *v = (glam::Vec3::from(*v) * factor).into();
            }
            s.matrix = scale * s.matrix;
        }
    }
}

/// Rimette nella scena gli originali messi da parte. Ritorna quanti pezzi ha ripristinato.
fn restore(scene: &mut SceneGraph, stash: &mut [Option<ShapeInfo>]) -> usize {
    let mut n = 0;
    for (i, slot) in stash.iter_mut().enumerate() {
        if let (Some(original), Some(current)) = (slot.take(), scene.shapes_data.get_mut(i)) {
            *current = original;
            n += 1;
        }
    }
    n
}

/// Ingombro della scena e fattore per passare dalle sue unità ai millimetri, se si capisce:
/// una scocca o un pezzo d'auto in metri misura tra 0,05 e 50, in millimetri tra 50 e 50.000.
pub fn unit_to_mm(scene: &SceneGraph) -> Option<f32> {
    let bbox = scene.compute_bounding_box();
    if !bbox.is_valid() {
        return None;
    }
    let size = (bbox.max - bbox.min).max_element();
    if (0.05..50.0).contains(&size) {
        Some(1000.0)
    } else if (50.0..50_000.0).contains(&size) {
        Some(1.0)
    } else {
        None
    }
}

impl GestoreApp {
    /// Pezzi su cui lavorare: quelli visibili, oppure quelli dentro la selezione.
    fn reduction_scope(&self, scene: &SceneGraph) -> Vec<u32> {
        let mut visible = Vec::new();
        scene.mesh_visibility_into(&mut visible);
        let deleted = |m: u32| {
            scene.mesh_to_node.get(m as usize).is_none_or(|&n| scene.info.get(n as usize).is_none_or(|i| i.deleted))
        };
        let mut ids: Vec<u32> = if self.project.reduction.panel.selection_only {
            let mut ids = Vec::new();
            for n in self.ui_state.selected_nodes() {
                scene.for_each_subtree_mesh(n, |m| ids.push(m));
            }
            ids.sort_unstable();
            ids.dedup();
            ids
        } else {
            (0..scene.shapes_data.len() as u32).filter(|&m| visible[m as usize]).collect()
        };
        ids.retain(|&m| !deleted(m));
        ids
    }

    /// Facce attuali dei pezzi su cui lavorare, contando la geometria originale.
    pub fn reduction_scope_triangles(&self) -> u64 {
        let Some(scene) = &self.scene_graph else { return 0 };
        let stash = &self.project.reduction.stash;
        self.reduction_scope(scene)
            .iter()
            .map(|&m| {
                let s = stash.get(m as usize).and_then(Option::as_ref).unwrap_or(&scene.shapes_data[m as usize]);
                (s.indices.len() / 3) as u64
            })
            .sum()
    }

    /// Avvia il calcolo con i valori del pannello. Ritorna false se non può partire.
    pub fn start_reduction(&mut self) -> bool {
        if self.project.reduction.job.is_some() || self.project.export_job.is_some() || self.ui_state.is_loading {
            return false;
        }
        let Some(scene) = &self.scene_graph else { return false };
        let ids = self.reduction_scope(scene);
        let panel = &self.project.reduction.panel;
        let target = match panel.mode {
            Mode::Triangles => Target::Triangles(panel.triangles),
            Mode::MaxError => Target::MaxError(panel.max_error),
        };
        let params = Params { target, lock_border: panel.lock_border };
        let compact = self.settings.export_compact;
        let Some(mut scene) = self.scene_graph.take() else { return false };
        let mut stash = std::mem::take(&mut self.project.reduction.stash);
        let progress = Arc::new(AtomicU32::new(0));
        let shared = progress.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let started = Instant::now();
            // Si riparte sempre dalla geometria originale
            restore(&mut scene, &mut stash);
            stash.resize(scene.shapes_data.len(), None);
            let refs: Vec<&ShapeInfo> = ids.iter().map(|&m| &scene.shapes_data[m as usize]).collect();
            let outcome = crate::reduce::reduce_many(&refs, params, &shared);
            let mut pieces = 0;
            for (&m, reduced) in ids.iter().zip(outcome.shapes) {
                if let Some(r) = reduced {
                    stash[m as usize] = Some(std::mem::replace(&mut scene.shapes_data[m as usize], r));
                    pieces += 1;
                }
            }
            scene.revision += 1;
            let size = |normals| crate::exporter::estimate_size(&scene, None, ExportOptions { compact, normals });
            let summary = Summary {
                before: outcome.triangles_before,
                after: outcome.triangles_after,
                pieces,
                max_error: outcome.max_error,
                size_with_normals: size(true),
                size_without_normals: size(false),
                compact,
                seconds: started.elapsed().as_secs_f32(),
                revision: scene.revision,
            };
            let _ = tx.send((scene, stash, summary));
        });
        self.ui_state.status_message = "Riduzione delle facce in corso…".to_string();
        self.project.reduction.job = Some(Job { rx, progress, started: Instant::now() });
        true
    }

    /// Controlla il calcolo in corso; a fine lavoro rimette la scena e aggiorna la vista.
    pub fn poll_reduction(&mut self) -> bool {
        let Some(job) = &self.project.reduction.job else { return false };
        let Ok((scene, stash, summary)) = job.rx.try_recv() else { return false };
        self.project.reduction.job = None;
        self.project.reduction.stash = stash;
        self.ui_state.status_message = format!(
            "Riduzione: {} → {} facce in {} pezzi ({:.1} s)",
            group_thousands(summary.before),
            group_thousands(summary.after),
            summary.pieces,
            summary.seconds
        );
        self.project.reduction.summary = Some(summary);
        self.scene_graph = Some(scene);
        self.upload_geometry();
        true
    }

    /// Rimette la geometria originale di tutti i pezzi ridotti.
    pub fn restore_original_geometry(&mut self) {
        let Some(scene) = &mut self.scene_graph else { return };
        let n = restore(scene, &mut self.project.reduction.stash);
        self.project.reduction.forget_geometry();
        if n > 0 {
            scene.revision += 1;
            self.ui_state.status_message = format!("Geometria originale ripristinata ({n} pezzi)");
            self.upload_geometry();
        }
    }

    /// Ricarica la geometria della scena sulla GPU mantenendo la vista.
    fn upload_geometry(&mut self) {
        if let (Some(renderer), Some(rs), Some(scene)) = (&mut self.renderer, &self.wgpu_render_state, &self.scene_graph) {
            renderer.upload_meshes(&scene.shapes_data, &rs.device);
        }
    }
}

/// `1234567` → `1.234.567`
pub fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Dimensione leggibile: `64,2 MB`.
pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    let text = if mb >= 100.0 { format!("{mb:.0}") } else { format!("{mb:.1}") };
    format!("{} MB", text.replace('.', ","))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una scena con una superficie ondulata fitta: la riduzione nel thread toglie facce, la vista
    /// e l'export usano il risultato, il ripristino rimette tutto com'era.
    #[test]
    fn reduce_preview_and_restore() {
        let mut points = String::new();
        let mut index = String::new();
        let n = 40;
        for i in 0..=n {
            for j in 0..=n {
                let (x, y) = (i as f32 * 0.025, j as f32 * 0.025);
                points.push_str(&format!("{x} {y} {}, ", (x * 6.0).sin() * 0.05));
            }
        }
        for i in 0..n {
            for j in 0..n {
                let a = i * (n + 1) + j;
                index.push_str(&format!("{} {} {} {} -1, ", a, a + n + 1, a + n + 2, a + 1));
            }
        }
        let wrl = format!(
            "#VRML V2.0 utf8\nDEF Lamiera Group {{ children [ Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ {points} ] }} coordIndex [ {index} ] }} }} ] }}"
        );
        let mut app = GestoreApp::new_headless();
        let mut scene = crate::parser::parse_wrl(wrl.as_bytes());
        scene.into_file("lamiera.wrl".into(), None);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        let original = app.scene_graph.as_ref().unwrap().shapes_data[0].clone();
        assert_eq!(app.reduction_scope_triangles(), 3200);

        app.project.reduction.panel.triangles = 800;
        assert!(app.start_reduction());
        assert!(app.scene_graph.is_none(), "la scena è nel thread");
        assert!(!app.start_reduction(), "un calcolo alla volta");
        while !app.poll_reduction() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let summary = app.project.reduction.summary.clone().unwrap();
        assert_eq!((summary.before, summary.pieces), (3200, 1));
        assert!((760..=840).contains(&summary.after), "{summary:?}");
        assert!(summary.size_without_normals < summary.size_with_normals);
        assert!(app.is_dirty(), "la riduzione cambia l'export");
        assert!(app.project.reduction.is_active());
        let scene = app.scene_graph.as_ref().unwrap();
        assert_eq!(scene.shapes_data[0].face_count as u64, summary.after);
        // Il conteggio del pannello si riferisce sempre agli originali
        assert_eq!(app.reduction_scope_triangles(), 3200);

        // Un secondo calcolo riparte dagli originali
        app.project.reduction.panel.triangles = 2000;
        assert!(app.start_reduction());
        while !app.poll_reduction() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let after = app.project.reduction.summary.as_ref().unwrap().after;
        assert!((1900..=2100).contains(&after), "{after}");

        // La stima è la dimensione vera del file
        let path = std::env::temp_dir().join("gestore_wrl_riduzione_test.wrl");
        let scene = app.scene_graph.as_ref().unwrap();
        crate::exporter::export_with(scene, None, &path, ExportOptions { compact: true, normals: false }).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), app.project.reduction.summary.as_ref().unwrap().size_without_normals);
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        assert_eq!(again.shapes_data[0].face_count as u64, after);
        let _ = std::fs::remove_file(path);

        app.restore_original_geometry();
        let scene = app.scene_graph.as_ref().unwrap();
        assert_eq!(scene.shapes_data[0].indices, original.indices);
        assert_eq!(scene.shapes_data[0].world_vertices, original.world_vertices);
        assert!(!app.project.reduction.is_active());
    }

    #[test]
    fn numbers_are_formatted_for_italian_readers() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1000), "1.000");
        assert_eq!(group_thousands(3716448), "3.716.448");
        assert_eq!(megabytes(30 * 1024 * 1024 + 100_000), "30,1 MB");
        assert_eq!(megabytes(536 * 1024 * 1024), "536 MB");
    }
}
