//! Gestione del progetto aperto: file corrente, modifiche non esportate,
//! chiusura con conferma, Annulla/Ripeti.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::app::GestoreApp;
use crate::scene_graph::EditSnapshot;

/// Passi di annullamento conservati. Ogni passo è una copia dello stato
/// modificabile della scena (non della geometria).
pub const MAX_UNDO: usize = 30;

/// Modifiche consecutive dello stesso tipo (es. trascinamento nel selettore colore)
/// entro questo intervallo diventano un unico passo di annullamento.
const COALESCE_WINDOW: Duration = Duration::from_millis(1500);

/// Operazione in attesa di conferma (ci sono modifiche non esportate).
#[derive(Debug, Clone)]
pub enum Pending {
    Close,
    Open(PathBuf),
    Exit,
    /// Riapre i file aperti (es. per applicare nuovi parametri di raggruppamento)
    Reload(Vec<PathBuf>),
}

/// Export in corso in un thread separato: la scena è stata affidata al thread e torna con il
/// risultato. Intanto la finestra resta reattiva (prima si bloccava per 5–8 s sui file grandi).
pub struct ExportJob {
    rx: std::sync::mpsc::Receiver<(crate::scene_graph::SceneGraph, std::io::Result<crate::exporter::writer::ExportStats>)>,
    pub path: PathBuf,
    /// Export dell'insieme (dopo il quale la scena non ha più modifiche da esportare)
    whole: bool,
    /// Operazione da eseguire a export riuscito (es. chiusura dopo "Esporta…" nella conferma)
    then: Option<Pending>,
    pub started: Instant,
}

#[derive(Default)]
pub struct ProjectState {
    /// Export in corso
    pub export_job: Option<ExportJob>,
    /// File attualmente aperto
    pub current_path: Option<PathBuf>,
    /// File in caricamento
    pub loading_path: Option<PathBuf>,
    /// Il file in caricamento si aggiunge alla scena aperta invece di sostituirla
    pub loading_adds: bool,
    /// File da aggiungere dopo quello in caricamento
    pub add_queue: std::collections::VecDeque<PathBuf>,
    /// Revisione della scena all'ultimo caricamento/export
    pub saved_revision: u64,
    pub undo: Vec<EditSnapshot>,
    pub redo: Vec<EditSnapshot>,
    /// Ultima modifica "accorpabile" (nodo, istante)
    last_coalesce: Option<(u32, Instant)>,
    pub pending: Option<Pending>,
    /// Uscita confermata: non intercettare più la chiusura della finestra
    pub allow_exit: bool,
    /// Ultimo titolo impostato sulla finestra
    last_title: String,
}

impl ProjectState {
    pub fn on_loaded(&mut self, revision: u64) {
        self.current_path = self.loading_path.take();
        self.loading_adds = false;
        self.saved_revision = revision;
        self.undo.clear();
        self.redo.clear();
        self.last_coalesce = None;
    }

    fn reset(&mut self) {
        let allow_exit = self.allow_exit;
        let last_title = std::mem::take(&mut self.last_title);
        *self = ProjectState { allow_exit, last_title, ..Default::default() };
    }
}

impl GestoreApp {
    /// Ci sono modifiche (visibilità, eliminazioni, colori, nomi) non ancora esportate.
    pub fn is_dirty(&self) -> bool {
        self.scene_graph.as_ref().is_some_and(|s| s.revision != self.project.saved_revision)
    }

    /// Nome del file aperto (per titolo e messaggi); con più file, il primo non chiuso.
    pub fn file_name(&self) -> Option<String> {
        if let Some(first) = self.scene_graph.as_ref().and_then(|s| s.open_files().next()) {
            return Some(first.name.clone());
        }
        self.project
            .current_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// Aggiorna il titolo della finestra: "nome.wrl* — Gestore WRL" (con più file: "nome.wrl + 2 file").
    pub fn update_window_title(&mut self, ctx: &egui::Context) {
        let others = self.scene_graph.as_ref().map_or(0, |s| s.open_files().count().saturating_sub(1));
        let name = self.file_name().map(|n| if others > 0 { format!("{n} + {others} file") } else { n });
        let title = match name {
            Some(n) if self.is_dirty() => format!("{n}* — Gestore WRL"),
            Some(n) => format!("{n} — Gestore WRL"),
            None => "Gestore WRL".to_string(),
        };
        if title != self.project.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.project.last_title = title;
        }
    }

    /// Salva lo stato corrente nella pila di annullamento.
    pub fn push_undo(&mut self, coalesce: Option<u32>) {
        let Some(scene) = &self.scene_graph else { return };
        let now = Instant::now();
        if let (Some(n), Some((last, t))) = (coalesce, self.project.last_coalesce) {
            if n == last && now.duration_since(t) < COALESCE_WINDOW {
                self.project.last_coalesce = Some((n, now));
                return;
            }
        }
        self.project.last_coalesce = coalesce.map(|n| (n, now));
        self.project.undo.push(scene.snapshot());
        if self.project.undo.len() > MAX_UNDO {
            self.project.undo.remove(0);
        }
        self.project.redo.clear();
    }

    pub fn undo(&mut self) {
        let Some(scene) = &mut self.scene_graph else { return };
        let Some(prev) = self.project.undo.pop() else {
            self.ui_state.status_message = "Niente da annullare.".to_string();
            return;
        };
        let current = scene.swap_snapshot(prev);
        self.project.redo.push(current);
        self.project.last_coalesce = None;
        self.after_history_change("Annullato");
    }

    pub fn redo(&mut self) {
        let Some(scene) = &mut self.scene_graph else { return };
        let Some(next) = self.project.redo.pop() else {
            self.ui_state.status_message = "Niente da ripetere.".to_string();
            return;
        };
        let current = scene.swap_snapshot(next);
        self.project.undo.push(current);
        self.project.last_coalesce = None;
        self.after_history_change("Ripetuto");
    }

    fn after_history_change(&mut self, what: &str) {
        let Some(scene) = &self.scene_graph else { return };
        // La selezione può puntare a un nodo eliminato
        self.ui_state
            .drop_from_selection(|n| scene.info.get(n as usize).is_none_or(|i| i.deleted));
        self.ui_state.renaming = None;
        self.ui_state.status_message = format!(
            "{what}  (annullabili: {}, ripetibili: {})",
            self.project.undo.len(),
            self.project.redo.len()
        );
    }

    /// Chiude il progetto, chiedendo conferma se ci sono modifiche non esportate.
    pub fn request_close(&mut self) {
        if self.is_dirty() {
            self.project.pending = Some(Pending::Close);
        } else {
            self.close_project();
        }
    }

    /// Apre un file, chiedendo conferma se ci sono modifiche non esportate.
    pub fn request_open(&mut self, path: PathBuf) {
        if self.ui_state.is_loading || self.project.export_job.is_some() {
            return;
        }
        if self.is_dirty() {
            self.project.pending = Some(Pending::Open(path));
        } else {
            self.open_wrl_async(path);
        }
    }

    /// Aggiunge file alla scena aperta (senza scena: il primo si apre, gli altri si aggiungono).
    /// Si caricano uno alla volta; quelli in più attendono in coda.
    pub fn request_add(&mut self, paths: Vec<PathBuf>) {
        if self.project.export_job.is_some() {
            return;
        }
        let mut paths = paths.into_iter();
        if self.scene_graph.is_none() && !self.ui_state.is_loading {
            let Some(first) = paths.next() else { return };
            self.project.add_queue.extend(paths);
            self.request_open(first);
            return;
        }
        self.project.add_queue.extend(paths);
        self.start_next_add();
    }

    /// Avvia il caricamento del prossimo file in coda, se non se ne sta già caricando uno.
    pub fn start_next_add(&mut self) {
        if self.ui_state.is_loading || self.project.pending.is_some() {
            return;
        }
        if let Some(path) = self.project.add_queue.pop_front() {
            self.open_wrl_async(path);
            self.project.loading_adds = self.scene_graph.is_some();
        }
    }

    /// Riapre tutti i file aperti, nello stesso ordine, con le impostazioni correnti
    /// (chiede conferma se ci sono modifiche non esportate).
    pub fn request_reload(&mut self) {
        let Some(scene) = &self.scene_graph else { return };
        let paths: Vec<PathBuf> = scene.open_files().filter_map(|s| s.path.clone()).collect();
        if paths.is_empty() || self.ui_state.is_loading || self.project.export_job.is_some() {
            return;
        }
        if self.is_dirty() {
            self.project.pending = Some(Pending::Reload(paths));
        } else {
            self.reload(paths);
        }
    }

    fn reload(&mut self, paths: Vec<PathBuf>) {
        let mut paths = paths.into_iter();
        let Some(first) = paths.next() else { return };
        self.open_wrl_async(first);
        self.project.add_queue = paths.collect();
    }

    /// Finestra di scelta di uno o più file da aggiungere alla scena.
    pub fn add_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("VRML World", &["wrl", "WRL", "vrml"]);
        if let Some(dir) = self.settings.recent.first().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(paths) = dialog.pick_files() {
            self.request_add(paths);
        }
    }

    /// Esporta un solo sotto-albero (es. un file dell'insieme) con la finestra "Salva come".
    pub fn export_node_dialog(&mut self, node: u32) {
        let Some(scene) = &self.scene_graph else { return };
        let name = scene.display_name(node);
        let stem = name.strip_suffix(".wrl").or(name.strip_suffix(".WRL")).unwrap_or(&name);
        let mut dialog = rfd::FileDialog::new()
            .add_filter("VRML World", &["wrl"])
            .set_file_name(format!("{}_export.wrl", crate::exporter::sanitize_identifier(stem)));
        let file_dir = scene
            .sources
            .iter()
            .find(|s| s.node == node)
            .or(scene.sources.first())
            .and_then(|s| s.path.as_ref())
            .and_then(|p| p.parent());
        if let Some(dir) = file_dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else { return };
        self.start_export(path, Some(node), None);
    }

    /// Avvia l'export in un thread: `node` = solo quel sotto-albero, altrimenti tutto il visibile.
    pub fn start_export(&mut self, path: PathBuf, node: Option<u32>, then: Option<Pending>) -> bool {
        if self.project.export_job.is_some() {
            return false;
        }
        let Some(scene) = self.scene_graph.take() else { return false };
        let (tx, rx) = std::sync::mpsc::channel();
        let out = path.clone();
        std::thread::spawn(move || {
            let result = match node {
                Some(n) => crate::exporter::export_subtree_wrl(&scene, n, &out),
                None => crate::exporter::export_visible_wrl(&scene, &out),
            };
            let _ = tx.send((scene, result));
        });
        self.ui_state.status_message = format!("Esportazione in corso: {}", path.display());
        self.project.export_job = Some(ExportJob { rx, path, whole: node.is_none(), then, started: Instant::now() });
        true
    }

    /// Controlla l'export in corso; a fine lavoro rimette a posto la scena. Ritorna true se finito.
    pub fn poll_export(&mut self, ctx: &egui::Context) -> bool {
        let Some(job) = &self.project.export_job else { return false };
        let Ok((scene, result)) = job.rx.try_recv() else { return false };
        let job = self.project.export_job.take().unwrap();
        let seconds = job.started.elapsed().as_secs_f32();
        let ok = result.is_ok();
        self.ui_state.status_message = match result {
            Ok(stats) => {
                if job.whole {
                    self.project.saved_revision = scene.revision;
                }
                format!("Esportato: {} ({} shape, {seconds:.1} s)", job.path.display(), stats.shapes_written)
            }
            Err(e) => format!("Errore export: {e}"),
        };
        self.scene_graph = Some(scene);
        if let (true, Some(then)) = (ok, job.then) {
            self.run_pending(ctx, then);
        }
        true
    }

    /// Apre la finestra di scelta file.
    pub fn open_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("VRML World", &["wrl", "WRL", "vrml"]);
        if let Some(dir) = self.settings.recent.first().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.request_open(path);
        }
    }

    /// Chiude il progetto senza conferma.
    pub fn close_project(&mut self) {
        let name = self.file_name();
        self.scene_graph = None;
        if let Some(r) = &mut self.renderer {
            r.clear_meshes();
        }
        self.project.reset();
        self.load_state = crate::app::LoadState::Idle;
        let status = match name {
            Some(n) => format!("Chiuso: {n}"),
            None => "Progetto chiuso.".to_string(),
        };
        self.ui_state = crate::ui::UiState { status_message: status, ..Default::default() };
    }

    /// Esporta le parti visibili con la finestra "Salva come" (in un thread, vedi `start_export`).
    /// `then` viene eseguita a export riuscito. Ritorna true se l'export è partito.
    pub fn export_dialog(&mut self, then: Option<Pending>) -> bool {
        let Some(scene) = &self.scene_graph else { return false };
        let several = scene.open_files().count() > 1;
        let suggested = self
            .project
            .current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| format!("{}{}_export.wrl", s.to_string_lossy(), if several { "_insieme" } else { "" }))
            .unwrap_or_else(|| "export.wrl".to_string());
        let mut dialog = rfd::FileDialog::new()
            .add_filter("VRML World", &["wrl"])
            .set_file_name(suggested);
        if let Some(dir) = self.project.current_path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else { return false };
        self.start_export(path, None, then)
    }

    /// Esegue l'operazione rimasta in attesa dopo la conferma.
    fn run_pending(&mut self, ctx: &egui::Context, pending: Pending) {
        match pending {
            Pending::Close => self.close_project(),
            Pending::Open(path) => {
                self.open_wrl_async(path);
            }
            Pending::Exit => {
                self.project.allow_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::Reload(paths) => self.reload(paths),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Action, GestoreApp};

    fn app_with_example() -> GestoreApp {
        let mut app = GestoreApp::new_headless();
        let scene = crate::parser::parse_wrl(include_bytes!("../esempi/test_colori.wrl"));
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app
    }

    fn root_children(app: &GestoreApp) -> Vec<u32> {
        let s = app.scene_graph.as_ref().unwrap();
        s.nodes[s.roots[0] as usize].children().to_vec()
    }

    #[test]
    fn undo_redo_delete_color_rename() {
        let mut app = app_with_example();
        assert!(!app.is_dirty());
        let kids = root_children(&app);
        let (a, b) = (kids[0], kids[1]);
        let name_b = app.scene_graph.as_ref().unwrap().display_name(b);

        app.apply(Action::Delete(a));
        app.apply(Action::Rename(b, "Nuovo".into()));
        assert!(app.is_dirty());
        assert_eq!(root_children(&app).len(), kids.len() - 1);

        app.undo(); // annulla rinomina
        assert_eq!(app.scene_graph.as_ref().unwrap().display_name(b), name_b);
        app.undo(); // annulla eliminazione
        assert_eq!(root_children(&app), kids);
        assert!(!app.scene_graph.as_ref().unwrap().info[a as usize].deleted);

        app.redo();
        assert_eq!(root_children(&app).len(), kids.len() - 1);
        app.redo();
        assert_eq!(app.scene_graph.as_ref().unwrap().display_name(b), "Nuovo");

        // Una nuova modifica svuota "Ripeti"
        app.undo();
        app.apply(Action::Hide(b));
        assert!(app.project.redo.is_empty());

        // Le azioni di sola vista non creano passi
        let n = app.project.undo.len();
        app.apply(Action::Select { node: Some(b), reveal: true });
        app.apply(Action::Focus(None));
        assert_eq!(app.project.undo.len(), n);
    }

    #[test]
    fn color_drag_is_one_undo_step() {
        let mut app = app_with_example();
        let a = root_children(&app)[0];
        for i in 0..20 {
            app.apply(Action::SetColor(a, Some([i as f32 / 20.0, 0.0, 0.0, 1.0])));
        }
        assert_eq!(app.project.undo.len(), 1);
        app.undo();
        assert!(app.scene_graph.as_ref().unwrap().info[a as usize].color.is_none());
    }

    /// Aggiungere un file: la scena resta "pulita" se lo era, Annulla toglie le modifiche
    /// fatte prima ma non il file aggiunto.
    #[test]
    fn adding_a_file_keeps_undo_and_clean_state() {
        use crate::scene_graph::node::multi_file_tests::{file, FILE_A, FILE_B};
        let mut app = GestoreApp::new_headless();
        let scene = file(FILE_A, "a.wrl");
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);

        app.add_loaded_file(file(FILE_B, "b.wrl"));
        assert!(!app.is_dirty());
        assert_eq!(app.scene_graph.as_ref().unwrap().open_files().count(), 2);

        let telaio = app.scene_graph.as_ref().unwrap().nodes[0].children()[0];
        app.apply(Action::Hide(telaio));
        app.add_loaded_file(file(FILE_B, "c.wrl"));
        assert!(app.is_dirty());
        app.undo();
        let scene = app.scene_graph.as_ref().unwrap();
        assert!(scene.nodes[telaio as usize].is_visible());
        assert_eq!(scene.roots.len(), 3);
        let mut vis = Vec::new();
        scene.mesh_visibility_into(&mut vis);
        assert_eq!(vis, vec![true; 6]);
        app.redo();
        assert!(!app.scene_graph.as_ref().unwrap().nodes[telaio as usize].is_visible());
    }

    /// Più file reali nella stessa scena, con il percorso dell'app (lettura mappata, aggiunta,
    /// export dell'insieme e di ogni file, riapertura):
    /// `GESTORE_WRL_FILES="a.wrl|b.wrl|..." cargo test --release piu_file_reali -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn piu_file_reali() {
        use crate::parser::LoadEvent;
        let list = std::env::var("GESTORE_WRL_FILES").expect("impostare GESTORE_WRL_FILES");
        let paths: Vec<std::path::PathBuf> = list.split('|').map(Into::into).collect();
        let load = |p: &std::path::PathBuf| {
            let (tx, rx) = std::sync::mpsc::channel();
            crate::parser::load_wrl_async(p.clone(), tx);
            loop {
                match rx.recv().unwrap() {
                    LoadEvent::Finished(s) => return s,
                    LoadEvent::Error(e) => panic!("{e}"),
                    _ => {}
                }
            }
        };

        let mut app = GestoreApp::new_headless();
        let total = std::time::Instant::now();
        let mut single_shapes = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            let t = std::time::Instant::now();
            let scene = load(p);
            let parse = t.elapsed();
            single_shapes.push(scene.shapes_data.len());
            let t = std::time::Instant::now();
            if i == 0 {
                app.project.on_loaded(scene.revision);
                app.scene_graph = Some(scene);
            } else {
                app.add_loaded_file(scene);
            }
            println!(
                "{}: {} shape, lettura {parse:?}, unione {:?}",
                p.file_name().unwrap().to_string_lossy(),
                single_shapes[i],
                t.elapsed()
            );
        }
        let scene = app.scene_graph.as_ref().unwrap();
        let vertices: usize = scene.shapes_data.iter().map(|s| s.world_vertices.len()).sum();
        let triangles: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
        println!(
            "Insieme: {} file, {} shape, {} nodi, {vertices} vertici, {triangles} triangoli, tempo totale {:?}",
            scene.open_files().count(),
            scene.shapes_data.len(),
            scene.nodes.len(),
            total.elapsed()
        );
        assert!(!app.is_dirty());
        assert_eq!(scene.shapes_data.len(), single_shapes.iter().sum::<usize>());
        // Costo di un passo di Annulla: copia di nodi, metadati, radici e macro-componenti
        let t = std::time::Instant::now();
        let snap = scene.snapshot();
        let names: usize = scene.nodes.iter().filter_map(|n| n.name()).map(str::len).sum();
        let children: usize = scene.nodes.iter().map(|n| n.children().len() * 4).sum();
        let bytes = scene.nodes.len()
            * (std::mem::size_of::<crate::scene_graph::SceneNode>() + std::mem::size_of::<crate::scene_graph::NodeInfo>())
            + names
            + children
            + scene.mesh_to_node.len() * 4;
        println!(
            "Passo di Annulla: circa {:.1} MB, copia in {:?} ({} nodi)",
            bytes as f64 / 1e6,
            t.elapsed(),
            scene.nodes.len()
        );
        drop(snap);
        for s in &app.ui_state.unit_suggestions {
            println!("Proposta unità: {} × {}", s.file, s.factor);
        }

        // Export dell'insieme e riapertura: stessa geometria, un Group per file
        let out = std::env::temp_dir().join("gestore_piu_file_reali.wrl");
        let t = std::time::Instant::now();
        let stats = crate::exporter::export_visible_wrl(scene, &out).unwrap();
        println!("Export dell'insieme: {} shape in {:?}", stats.shapes_written, t.elapsed());
        let again = crate::parser::parse_wrl(&std::fs::read(&out).unwrap());
        assert_eq!(again.shapes_data.len(), scene.shapes_data.len());
        // L'export scrive le shape nell'ordine dell'albero (pre-order = ordine dell'arena), che
        // il raggruppamento automatico può rendere diverso da quello del file
        let tree_order = |range: std::ops::Range<u32>| -> Vec<u32> {
            scene
                .nodes
                .iter()
                .filter_map(|n| match n {
                    crate::scene_graph::SceneNode::Shape { mesh_id, .. } if range.contains(mesh_id) => Some(*mesh_id),
                    _ => None,
                })
                .collect()
        };
        let all = tree_order(0..scene.shapes_data.len() as u32);
        let same = again
            .shapes_data
            .iter()
            .zip(&all)
            .filter(|(a, &m)| {
                let b = &scene.shapes_data[m as usize];
                a.world_vertices == b.world_vertices && a.indices == b.indices
            })
            .count();
        println!(
            "Riapertura: {} radici ({}), shape con geometria identica {same}/{}",
            again.roots.len(),
            again.roots.iter().map(|&r| again.display_name(r)).collect::<Vec<_>>().join(", "),
            scene.shapes_data.len()
        );
        assert_eq!(again.roots.len(), paths.len());
        assert_eq!(same, scene.shapes_data.len());

        // Export di ogni file da solo: stesse shape del file aperto da solo
        for (i, src) in scene.sources.iter().enumerate() {
            let one = std::env::temp_dir().join("gestore_piu_file_reali_uno.wrl");
            crate::exporter::export_subtree_wrl(scene, src.node, &one).unwrap();
            let again = crate::parser::parse_wrl(&std::fs::read(&one).unwrap());
            assert_eq!(again.shapes_data.len(), single_shapes[i], "{}", src.name);
            let order = tree_order(src.meshes.clone());
            let same = again
                .shapes_data
                .iter()
                .zip(&order)
                .filter(|(a, &m)| a.world_vertices == scene.shapes_data[m as usize].world_vertices)
                .count();
            println!("Export di {}: {} shape, vertici identici {same}/{}", src.name, single_shapes[i], order.len());
            assert_eq!(same, order.len(), "{}", src.name);
            let _ = std::fs::remove_file(one);
        }
        let _ = std::fs::remove_file(out);
    }

    /// L'export gira in un thread: intanto la scena non c'è, poi torna intatta; un export
    /// dell'insieme azzera le modifiche da esportare e fa partire l'operazione in attesa.
    #[test]
    fn export_runs_in_the_background_and_gives_the_scene_back() {
        let mut app = app_with_example();
        let a = root_children(&app)[0];
        app.apply(Action::Hide(a));
        assert!(app.is_dirty());
        let nodes = app.scene_graph.as_ref().unwrap().nodes.len();
        let path = std::env::temp_dir().join("gestore_export_thread.wrl");
        assert!(app.start_export(path.clone(), None, Some(super::Pending::Close)));
        assert!(app.scene_graph.is_none() && app.project.export_job.is_some());
        assert!(!app.start_export(path.clone(), None, None), "un export alla volta");

        let ctx = egui::Context::default();
        let t = std::time::Instant::now();
        while !app.poll_export(&ctx) {
            assert!(t.elapsed().as_secs() < 10, "export bloccato");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Export riuscito → l'operazione in attesa (chiusura) è stata eseguita
        assert!(app.scene_graph.is_none() && app.project.export_job.is_none());
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        assert!(!again.shapes_data.is_empty());

        // Senza operazione in attesa la scena torna, senza modifiche da esportare
        let mut app = app_with_example();
        app.apply(Action::Hide(root_children(&app)[0]));
        app.start_export(path.clone(), None, None);
        while !app.poll_export(&ctx) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(app.scene_graph.as_ref().unwrap().nodes.len(), nodes);
        assert!(!app.is_dirty());
        let _ = std::fs::remove_file(path);
    }

    /// "Ricarica i file aperti": il primo si riapre, gli altri seguono in coda; con modifiche
    /// non esportate prima si chiede conferma.
    #[test]
    fn reload_reopens_the_open_files_in_order() {
        use crate::scene_graph::node::multi_file_tests::{FILE_A, FILE_B};
        let dir = std::env::temp_dir();
        let (pa, pb) = (dir.join("gestore_ricarica_a.wrl"), dir.join("gestore_ricarica_b.wrl"));
        let open = |data: &[u8], p: &std::path::PathBuf| {
            let mut s = crate::parser::parse_wrl(data);
            s.into_file("x".into(), Some(p.clone()));
            s
        };
        let mut app = GestoreApp::new_headless();
        let scene = open(FILE_A, &pa);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.add_loaded_file(open(FILE_B, &pb));

        let telaio = app.scene_graph.as_ref().unwrap().nodes[0].children()[0];
        app.apply(Action::Hide(telaio));
        app.apply(Action::ReloadFiles);
        assert!(matches!(&app.project.pending, Some(super::Pending::Reload(p)) if *p == vec![pa.clone(), pb.clone()]));
        assert!(!app.ui_state.is_loading);

        // Senza modifiche: si riapre subito
        let mut app = GestoreApp::new_headless();
        let scene = open(FILE_A, &pa);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.add_loaded_file(open(FILE_B, &pb));
        app.apply(Action::ReloadFiles);
        assert_eq!(app.project.loading_path.as_ref(), Some(&pa));
        assert!(!app.project.loading_adds, "il primo file sostituisce la scena");
        assert_eq!(app.project.add_queue, vec![pb]);
    }

    #[test]
    fn close_asks_only_when_dirty() {
        let mut app = app_with_example();
        app.request_close();
        assert!(app.scene_graph.is_none() && app.project.pending.is_none());

        let mut app = app_with_example();
        let a = root_children(&app)[0];
        app.apply(Action::Hide(a));
        app.request_close();
        assert!(app.scene_graph.is_some());
        assert!(matches!(app.project.pending, Some(super::Pending::Close)));
    }
}

/// Riquadro "Esportazione in corso" (blocca i comandi finché la scena non torna dal thread).
pub fn draw_export_progress(ctx: &egui::Context, app: &GestoreApp) {
    let Some(job) = &app.project.export_job else { return };
    egui::Modal::new(egui::Id::new("export_in_corso")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.heading("Esportazione in corso");
        });
        ui.add_space(6.0);
        let name = job.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ui.label(format!("{name}  ·  {:.0} s", job.started.elapsed().as_secs_f32()));
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

/// Testo del comando di conversione per un fattore di scala.
pub fn unit_label(factor: f32) -> &'static str {
    if factor < 1.0 { "Converti da millimetri a metri (÷1000)" } else { "Converti da metri a millimetri (×1000)" }
}

/// Proposte di conversione delle unità per i file appena aggiunti (vedi `suggest_unit_scale`).
pub fn draw_unit_suggestion(ctx: &egui::Context, app: &mut GestoreApp) {
    let list = app.ui_state.unit_suggestions.clone();
    if list.is_empty() {
        return;
    }
    let mut convert: Vec<crate::ui::UnitSuggestion> = Vec::new();
    let mut dismiss = false;
    egui::Window::new("Unità di misura diverse")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.label(
                "Questi file sembrano scritti in unità diverse dal primo file aperto: così come sono \
                 finiscono lontani dal resto. Convertiti, si posizionano sopra.",
            );
            ui.add_space(6.0);
            for s in &list {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&s.file).strong());
                    let unit = if s.factor < 1.0 { "da millimetri a metri" } else { "da metri a millimetri" };
                    ui.label(egui::RichText::new(unit).weak());
                    if list.len() > 1 && ui.small_button("Converti").clicked() {
                        convert.push(s.clone());
                    }
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let all = if list.len() > 1 { "Converti tutti" } else { "Converti" };
                if ui.button(all).clicked() {
                    convert = list.clone();
                }
                if ui.button("Lascia così").clicked() {
                    dismiss = true;
                }
            });
            ui.label(
                egui::RichText::new("Si può convertire anche dopo, con il tasto destro sul file nell'albero: «Unità di misura»")
                    .small()
                    .weak(),
            );
        });
    for s in convert {
        app.apply(crate::app::Action::ScaleFile(s.node, s.factor));
    }
    if dismiss {
        app.ui_state.unit_suggestions.clear();
    }
}
/// Finestra modale "modifiche non esportate".
pub fn draw_confirm_dialog(ctx: &egui::Context, app: &mut GestoreApp) {
    let Some(pending) = app.project.pending.clone() else { return };
    let what = match &pending {
        Pending::Close => "chiudere il progetto",
        Pending::Open(_) => "aprire un altro file",
        Pending::Exit => "uscire",
        Pending::Reload(_) => "ricaricare i file",
    };

    let mut choice: Option<u8> = None; // 0 = esporta, 1 = continua, 2 = annulla
    let modal = egui::Modal::new(egui::Id::new("conferma_modifiche")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading("Modifiche non esportate");
        ui.add_space(6.0);
        ui.label(format!(
            "Hai modificato la scena (visibilità, eliminazioni, colori o nomi) e non l'hai ancora esportata.\n\
             Vuoi esportarla prima di {what}?"
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(crate::ui::theme::icon_text(ui, crate::ui::theme::icon::EXPORT, "Esporta…")).clicked() {
                choice = Some(0);
            }
            if ui.button("Continua senza esportare").clicked() {
                choice = Some(1);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Annulla").clicked() {
                    choice = Some(2);
                }
            });
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(2); // Esc o click fuori
    }

    match choice {
        Some(0) => {
            // L'operazione in attesa parte quando l'export è finito bene
            if app.export_dialog(Some(pending)) {
                app.project.pending = None;
            }
        }
        Some(1) => {
            app.project.pending = None;
            app.run_pending(ctx, pending);
        }
        Some(2) => app.project.pending = None,
        _ => {}
    }
}
