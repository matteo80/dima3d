use std::sync::mpsc::Receiver;
use crate::parser::LoadEvent;
use crate::{parser, renderer, scene_graph, ui};

/// Stato del caricamento del file WRL.
#[derive(Debug, Default)]
pub enum LoadState {
    #[default]
    Idle,
    Loading(String),
    Ready,
    Error(String),
}

/// Azione dell'utente sul grafo di scena.
/// Tutte le sorgenti (toolbar, albero, viewport, tastiera) passano da qui,
/// così stato della scena, renderer e UI restano coerenti.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Seleziona un nodo (o deseleziona). `reveal` espande l'albero e scorre fino al nodo.
    Select { node: Option<u32>, reveal: bool },
    /// Selezione multipla: `primary` è la riga corrente, `anchor` l'estremo fisso per Shift.
    SelectMany { nodes: Vec<u32>, primary: u32, anchor: u32 },
    ToggleVisible(u32),
    Hide(u32),
    Show(u32),
    Isolate(u32),
    IsolateMany(Vec<u32>),
    Delete(u32),
    ShowAll,
    /// Inquadra un nodo, oppure (None) tutte le parti visibili.
    Focus(Option<u32>),
    FocusMany(Vec<u32>),
    /// Più azioni applicate insieme: si annullano con un solo Ctrl+Z.
    Batch(Vec<Action>),
    /// Colore utente (RGBA sRGB) di un nodo e del suo sotto-albero; `None` = colore originale.
    SetColor(u32, Option<[f32; 4]>),
    /// Colori diversi su più nodi (es. colori casuali): un solo passo di Annulla, mai unito ai
    /// cambi di colore precedenti.
    SetColors(Vec<(u32, [f32; 4])>),
    /// Ripristina i colori originali di tutta la scena.
    ResetAllColors,
    /// Aggiunge un colore (sRGB) ai colori salvati nelle preferenze.
    SaveColor([f32; 3]),
    /// Toglie un colore salvato (indice nella lista).
    ForgetColor(usize),
    /// Rinomina un nodo (stringa vuota = nome automatico).
    Rename(u32, String),
    /// Apre il campo di rinomina nell'albero.
    StartRename(u32),
    /// Vista standard (fronte, alto, iso, ...), con passaggio animato.
    View(crate::renderer::camera::StandardView),
    /// Vista dalla direzione data (dal modello verso l'occhio): spigoli e vertici del cubo.
    ViewFrom(glam::Vec3),
    /// Annulla il rollio della vista.
    Straighten,
    /// Esporta un solo sotto-albero (es. un file dell'insieme) con "Salva come".
    ExportNode(u32),
    /// Esporta le parti visibili dei nodi (la selezione) con "Salva come".
    ExportSelected(Vec<u32>),
    /// Riapre i file aperti con le impostazioni correnti (es. nuovo raggruppamento automatico).
    ReloadFiles,
    /// Scala la geometria del file con questo nodo radice (es. 0.001 da millimetri a metri).
    /// Non entra in Annulla: si torna indietro con la scala inversa.
    ScaleFile(u32, f32),
    /// "Converti in mesh…" su un file STEP aperto (nodo radice del file).
    RemeshStep(u32),
    /// Sposta/ruota i sotto-alberi dei nodi con una matrice in coordinate mondo
    /// (vedi `scene_graph::placement`). Spostamenti consecutivi degli stessi nodi sono un solo
    /// passo di Annulla.
    Move { nodes: Vec<u32>, matrix: glam::Mat4 },
    /// Riporta i sotto-alberi dei nodi alla posizione del caricamento.
    ResetPosition(Vec<u32>),
    /// Copia i nodi negli appunti interni (vedi `scene_graph::copy`).
    Copy(Vec<u32>),
    /// Incolla gli appunti interni: un nuovo «file» per ogni file di provenienza.
    Paste,
    /// Sposta i nodi nell'albero dentro un gruppo dello stesso file (la geometria resta ferma).
    MoveInto { nodes: Vec<u32>, into: u32 },
    /// Raccoglie i nodi in un gruppo nuovo, poi apre la rinomina.
    NewGroup(Vec<u32>),
    /// Gruppo vuoto dentro un file o un gruppo (di un pezzo: nel suo gruppo), poi la rinomina.
    NewEmptyGroup(u32),
    /// Apre la cerniera salvata `index` a `angle` gradi (vedi `scene_graph::hinge`); aperture
    /// consecutive della stessa cerniera sono un solo passo di Annulla.
    SetHingeAngle { index: usize, angle: f32 },
}

impl Action {
    /// True per le azioni che modificano la scena (annullabili con Ctrl+Z).
    pub fn is_edit(&self) -> bool {
        if let Action::Batch(list) = self {
            return list.iter().any(Action::is_edit);
        }
        !matches!(
            self,
            Action::Select { .. }
                | Action::SelectMany { .. }
                | Action::Focus(_)
                | Action::FocusMany(_)
                | Action::StartRename(_)
                | Action::View(_)
                | Action::ViewFrom(_)
                | Action::Straighten
                | Action::ExportNode(_)
                | Action::ExportSelected(_)
                | Action::ScaleFile(..)
                | Action::RemeshStep(_)
                | Action::ReloadFiles
                | Action::SaveColor(_)
                | Action::ForgetColor(_)
                | Action::Copy(_)
                | Action::Paste
        )
    }
}

/// Applicazione principale.
pub struct GestoreApp {
    pub scene_graph: Option<scene_graph::SceneGraph>,
    pub renderer: Option<renderer::WgpuRenderer>,
    pub ui_state: ui::UiState,
    pub load_state: LoadState,
    pub load_rx: Option<Receiver<LoadEvent>>,
    pub wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Preferenze persistenti (sfondo, colore predefinito)
    pub settings: crate::settings::Settings,
    /// Annulla/Ripeti, file corrente, conferme
    pub project: crate::project::ProjectState,
    /// Verifica e download degli aggiornamenti
    pub update: crate::update::UpdateState,
    /// Avvio: fotogrammi disegnati finora (si ingrandisce la finestra quando è già visibile) e
    /// file da aprire passato a riga di comando (doppio clic su un file in Windows)
    pub startup_frames: u32,
    pub startup_path: Option<std::path::PathBuf>,
}

impl GestoreApp {
    /// Inizializza l'applicazione con il contesto nativo eframe (incluso il backend wgpu).
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.clone();
        let settings = crate::settings::Settings::load();
        parser::external::cleanup_old_conversions();
        ui::theme::install_fonts(&cc.egui_ctx);
        ui::theme::apply(&cc.egui_ctx, settings.theme);
        let renderer = wgpu_render_state.as_ref().map(|rs| {
            let mut r = renderer::WgpuRenderer::new(&rs.device, wgpu::TextureFormat::Rgba8UnormSrgb);
            r.background = settings.background;
            r.background_gradient = settings.background_gradient;
            r.default_color = settings.default_color;
            r
        });

        let mut app = Self {
            scene_graph: None,
            renderer,
            ui_state: ui::UiState::default(),
            load_state: LoadState::Idle,
            load_rx: None,
            wgpu_render_state,
            settings,
            project: Default::default(),
            update: Default::default(),
            startup_frames: 0,
            startup_path: None,
        };
        crate::update::cleanup_previous();
        if app.settings.check_updates {
            app.check_for_updates(false);
        }
        // File passato a riga di comando o con "Apri con" di Windows
        // Si apre al primo fotogramma, come un file trascinato: così anche un progetto .dima3d
        // (con le cabine) passa dalla sua apertura e non dal caricamento dei modelli
        app.startup_path = std::env::args_os().nth(1).map(Into::into);
        app
    }

    /// Primi fotogrammi: ingrandisce la finestra quando è già visibile (ingrandita alla
    /// creazione Windows la mostra grande ma non «ingrandita») e apre il file della riga di comando.
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.startup_frames > 2 {
            return;
        }
        self.startup_frames += 1;
        if self.startup_frames == 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if let Some(path) = self.startup_path.take() {
            self.request_open(path);
        }
        ctx.request_repaint();
    }

    /// Applica le preferenze al renderer e le salva su disco.
    pub fn apply_settings(&mut self, settings: crate::settings::Settings) {
        if settings == self.settings {
            return;
        }
        if let Some(r) = &mut self.renderer {
            r.background = settings.background;
            r.background_gradient = settings.background_gradient;
            r.default_color = settings.default_color;
        }
        settings.save();
        self.settings = settings;
    }

    /// Inizializza una versione headless utile per test e simulazioni.
    pub fn new_headless() -> Self {
        Self {
            scene_graph: None,
            renderer: None,
            ui_state: ui::UiState::default(),
            load_state: LoadState::Idle,
            load_rx: None,
            wgpu_render_state: None,
            settings: crate::settings::Settings::default(),
            project: Default::default(),
            update: Default::default(),
            startup_frames: 0,
            startup_path: None,
        }
    }

    /// Avvia il caricamento asincrono di un file WRL.
    /// (Usare `request_open`, che chiede conferma se ci sono modifiche non esportate.)
    pub fn open_wrl_async(&mut self, path: std::path::PathBuf) {
        self.project.loading_path = Some(path.clone());
        self.project.loading_adds = false;
        let (tx, rx) = std::sync::mpsc::channel();
        self.load_state = LoadState::Loading(path.to_string_lossy().into_owned());
        self.ui_state.is_loading = true;
        self.ui_state.load_progress = 0.0;
        self.ui_state.status_message = format!("Apertura: {}", path.display());
        self.load_rx = Some(rx);

        // Progetto in apertura: raggruppamento e tassellazione degli STEP di quando è stato salvato
        let restore = self.project.restore.as_ref();
        let saved_step = restore.and_then(|r| r.file.files.iter().find(|f| f.path == path)).and_then(|f| f.step);
        let cluster = restore.map_or(self.settings.cluster, |r| r.file.cluster);
        self.project.loaded_cluster = Some(cluster);
        let step = saved_step.or(self.project.step_choice).unwrap_or(crate::settings::StepLoad::Mesh(self.settings.step));
        self.project.loading_step = parser::external::is_step(&path).then_some(step);
        if let Some(kind) = parser::external::Kind::of(&path, step.mesh()) {
            parser::external::convert_and_load(path, kind, cluster, tx);
        } else {
            parser::load_wrl_async_with(path, cluster, tx);
        }
    }

    /// Esegue un'azione utente.
    pub fn apply(&mut self, action: Action) {
        if let Action::StartRename(n) = action {
            self.start_rename(n);
            return;
        }
        if let Action::ExportNode(n) = action {
            self.export_node_dialog(n);
            return;
        }
        if let Action::ExportSelected(nodes) = action {
            self.export_selection_dialog(nodes);
            return;
        }
        if action == Action::ReloadFiles {
            self.request_reload();
            return;
        }
        if let Action::RemeshStep(n) = action {
            self.ask_remesh_step(n);
            return;
        }
        if let Action::Copy(nodes) = action {
            self.copy_nodes(nodes);
            return;
        }
        if action == Action::Paste {
            self.paste();
            return;
        }
        if let Action::NewGroup(nodes) = action {
            self.new_group(nodes);
            return;
        }
        if let Action::NewEmptyGroup(target) = action {
            self.create_group(|scene| scene.new_empty_group(target, "Nuovo gruppo"));
            return;
        }
        if let Action::MoveInto { nodes, into } = action {
            self.move_into(nodes, into);
            return;
        }
        if let Action::ScaleFile(n, factor) = action {
            self.scale_file(n, factor);
            return;
        }
        if let Action::SaveColor(c) = action {
            let mut settings = self.settings.clone();
            self.ui_state.status_message = if settings.save_color(c) {
                "Colore aggiunto ai miei colori".to_string()
            } else {
                "Il colore era già tra i miei colori".to_string()
            };
            self.apply_settings(settings);
            return;
        }
        if let Action::ForgetColor(i) = action {
            let mut settings = self.settings.clone();
            if i < settings.saved_colors.len() {
                settings.saved_colors.remove(i);
                self.ui_state.status_message = "Colore tolto dai miei colori".to_string();
            }
            self.apply_settings(settings);
            return;
        }
        if action.is_edit() {
            let coalesce = match &action {
                Action::SetColor(n, _) => Some(*n),
                // Bit alto: chiave diversa da quella dei colori dello stesso nodo
                Action::Move { nodes, .. } => nodes.first().map(|n| n | 0x8000_0000),
                Action::SetHingeAngle { index, .. } => Some(*index as u32 | 0x4000_0000),
                Action::Batch(list) => match list.first() {
                    Some(Action::SetColor(n, _)) => Some(*n),
                    Some(Action::Move { nodes, .. }) => nodes.first().map(|n| n | 0x8000_0000),
                    _ => None,
                },
                _ => None,
            };
            self.push_undo(coalesce);
        }
        self.apply_without_undo(action);
        self.sync_moved_geometry();
    }

    /// Porta gli spostamenti della scena sulla GPU e negli originali della riduzione delle facce.
    pub fn sync_moved_geometry(&mut self) {
        let Some(scene) = &mut self.scene_graph else { return };
        let moved = scene.take_moved();
        if moved.is_empty() {
            return;
        }
        let mut meshes: Vec<u32> = Vec::new();
        for m in &moved {
            self.project.reduction.transform(&m.meshes, m.matrix);
            meshes.extend_from_slice(&m.meshes);
        }
        meshes.sort_unstable();
        meshes.dedup();
        // La stima della dimensione dei file non cambia, ma l'ingombro sì
        if let (Some(renderer), Some(rs)) = (&mut self.renderer, &self.wgpu_render_state) {
            renderer.update_geometry(&scene.shapes_data, &meshes, &rs.queue);
            let all = scene.compute_bounding_box();
            if all.is_valid() {
                renderer.camera.scene_bounds = Some(all);
            }
        }
    }

    /// Inquadra l'unione dei sotto-alberi dati.
    fn fit_nodes(&mut self, nodes: &[u32]) {
        let Some(scene) = &self.scene_graph else { return };
        let mut bbox = scene_graph::Aabb::empty();
        for &n in nodes {
            let b = scene.subtree_bounding_box(n);
            if b.is_valid() {
                bbox.extend_aabb(&b);
            }
        }
        if let (Some(r), true) = (&mut self.renderer, bbox.is_valid()) {
            r.camera.fit_to_bbox(&bbox);
        }
    }

    pub(crate) fn apply_without_undo(&mut self, action: Action) {
        if let Action::Batch(list) = action {
            let count = list.len();
            for a in list {
                self.apply_without_undo(a);
            }
            self.ui_state.status_message = format!("Modifica applicata a {count} elementi");
            return;
        }
        match &action {
            Action::IsolateMany(nodes) => {
                if let Some(scene) = &mut self.scene_graph {
                    scene.isolate_many(nodes);
                }
                self.fit_nodes(nodes);
                self.ui_state.status_message = format!("Isolati: {} elementi", nodes.len());
                return;
            }
            Action::FocusMany(nodes) => {
                self.fit_nodes(nodes);
                return;
            }
            Action::Move { nodes, matrix } => {
                if let Some(scene) = &mut self.scene_graph {
                    let meshes = scene.meshes_of(nodes);
                    scene.transform_meshes(&meshes, *matrix);
                    self.ui_state.status_message = format!("Spostati {} pezzi", meshes.len());
                }
                return;
            }
            Action::SetHingeAngle { index, angle } => {
                if let Some(scene) = &mut self.scene_graph {
                    if let Some(done) = scene.set_hinge_angle(*index, *angle) {
                        let name = scene.hinges[*index].name.clone();
                        self.ui_state.status_message = format!("{name}: aperta a {done:.1}°");
                    }
                }
                return;
            }
            Action::ResetPosition(nodes) => {
                if let Some(scene) = &mut self.scene_graph {
                    let meshes = scene.meshes_of(nodes);
                    scene.reset_placements(&meshes);
                    self.ui_state.status_message = "Posizione ripristinata".to_string();
                }
                return;
            }
            _ => {}
        }
        let Some(scene) = &mut self.scene_graph else {
            return;
        };
        match action {
            Action::Batch(_)
            | Action::IsolateMany(_)
            | Action::FocusMany(_)
            | Action::Move { .. }
            | Action::SetHingeAngle { .. }
            | Action::ResetPosition(_)
            // Gestite in `apply`
            | Action::Copy(_)
            | Action::Paste
            | Action::NewGroup(_)
            | Action::NewEmptyGroup(_)
            | Action::MoveInto { .. } => {}
            Action::SelectMany { nodes, primary, anchor } => {
                self.ui_state.status_message = format!("Selezionati: {} elementi", nodes.len());
                self.ui_state.selected_node = Some(primary);
                self.ui_state.selected_extra = nodes.into_iter().filter(|&n| n != primary).collect();
                self.ui_state.selection_anchor = Some(anchor);
            }
            Action::Select { node, reveal } => {
                self.ui_state.select_single(node);
                if let (Some(n), true) = (node, reveal) {
                    for a in scene.ancestors(n) {
                        self.ui_state.expanded.insert(a);
                    }
                    self.ui_state.scroll_to_selected = true;
                }
                if let Some(n) = node {
                    let count = scene.info.get(n as usize).map(|i| i.shape_count).unwrap_or(0);
                    self.ui_state.status_message =
                        format!("Selezionato: {} ({} pezzi)", scene.display_name(n), count);
                }
            }
            Action::ToggleVisible(n) => {
                let v = scene.nodes.get(n as usize).map(|x| x.is_visible()).unwrap_or(false);
                scene.set_subtree_visible(n, !v);
            }
            Action::Hide(n) => {
                scene.set_subtree_visible(n, false);
                self.ui_state.status_message = format!("Nascosto: {}", scene.display_name(n));
            }
            Action::Show(n) => {
                scene.set_subtree_visible(n, true);
                self.ui_state.status_message = format!("Mostrato: {}", scene.display_name(n));
            }
            Action::Isolate(n) => {
                scene.isolate(n);
                self.ui_state.status_message = format!("Isolato: {}", scene.display_name(n));
                let bbox = scene.subtree_bounding_box(n);
                if let (Some(r), true) = (&mut self.renderer, bbox.is_valid()) {
                    r.camera.fit_to_bbox(&bbox);
                }
            }
            Action::Delete(n) => {
                let name = scene.display_name(n);
                let is_file = scene.info.get(n as usize).is_some_and(|i| i.kind == scene_graph::NodeKind::File);
                scene.delete_node(n);
                self.ui_state
                    .drop_from_selection(|n| scene.info.get(n as usize).is_none_or(|i| i.deleted));
                self.ui_state.status_message =
                    if is_file { format!("Chiuso il file: {name}") } else { format!("Eliminato: {name}") };
            }
            Action::ShowAll => {
                scene.show_all();
                self.ui_state.status_message = "Tutti i nodi visibili.".to_string();
            }
            Action::Focus(node) => {
                let bbox = match node {
                    Some(n) => scene.subtree_bounding_box(n),
                    None => {
                        let (b, excluded) = scene.main_visible_bounding_box();
                        if excluded > 0 {
                            self.ui_state.status_message = far_parts_message(excluded);
                        }
                        if b.is_valid() { b } else { scene.compute_bounding_box() }
                    }
                };
                if let (Some(r), true) = (&mut self.renderer, bbox.is_valid()) {
                    r.camera.fit_to_bbox(&bbox);
                    r.camera.scene_bounds = Some(scene.compute_bounding_box());
                }
            }
            Action::SetColor(n, color) => {
                scene.set_color(n, color);
                self.ui_state.status_message = match color {
                    Some(_) => format!("Colore impostato: {}", scene.display_name(n)),
                    None => format!("Colore originale: {}", scene.display_name(n)),
                };
            }
            Action::SetColors(list) => {
                for &(n, color) in &list {
                    scene.set_color(n, Some(color));
                }
                self.ui_state.status_message = format!("Colori casuali su {} pezzi", list.len());
            }
            Action::ResetAllColors => {
                scene.reset_all_colors();
                self.ui_state.status_message = "Ripristinati i colori originali.".to_string();
            }
            Action::Rename(n, name) => {
                let old = scene.display_name(n);
                scene.rename(n, &name);
                self.ui_state.status_message =
                    format!("Rinominato: da {old} a {}", scene.display_name(n));
            }
            Action::StartRename(_)
            | Action::ExportNode(_)
            | Action::ExportSelected(_)
            | Action::ScaleFile(..)
            | Action::RemeshStep(_)
            | Action::ReloadFiles
            | Action::SaveColor(_)
            | Action::ForgetColor(_) => {}
            Action::View(v) => {
                if let Some(r) = &mut self.renderer {
                    let q = crate::renderer::camera::ArcballCamera::standard_orientation(v, self.settings.axes);
                    r.camera.animate_to(q, std::time::Instant::now());
                }
            }
            Action::ViewFrom(dir) => {
                if let Some(r) = &mut self.renderer {
                    let q = crate::renderer::camera::ArcballCamera::orientation_for_direction(dir, self.settings.axes);
                    r.camera.animate_to(q, std::time::Instant::now());
                }
            }
            Action::Straighten => {
                if let Some(r) = &mut self.renderer {
                    r.camera.straighten(self.settings.axes);
                }
            }
        }
    }

    /// Apre il campo di rinomina sulla riga dell'albero del nodo.
    pub fn start_rename(&mut self, node: u32) {
        let Some(scene) = &self.scene_graph else { return };
        let current = scene
            .nodes
            .get(node as usize)
            .and_then(|n| n.name())
            .map(str::to_string)
            .unwrap_or_else(|| scene.display_name(node));
        for a in scene.ancestors(node) {
            self.ui_state.expanded.insert(a);
        }
        self.ui_state.select_single(Some(node));
        self.ui_state.scroll_to_selected = true;
        self.ui_state.renaming = Some(ui::Renaming { node, text: current, focus: true });
    }

    /// Nasconde la selezione, o la mostra se l'elemento corrente è nascosto.
    pub fn toggle_selection_action(&self) -> Option<Action> {
        let current = self.ui_state.selected_node?;
        let visible = self.scene_graph.as_ref()?.nodes.get(current as usize)?.is_visible();
        self.ui_state.on_selection(if visible { Action::Hide } else { Action::Show })
    }

    /// Scorciatoie da tastiera (attive solo se nessun campo di testo ha il focus).
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.project.pending.is_some() || self.project.busy() {
            return;
        }
        let (open, save, save_as, export) = ctx.input(|inp| {
            let cmd = inp.modifiers.command;
            let s = cmd && inp.key_pressed(egui::Key::S);
            (cmd && inp.key_pressed(egui::Key::O), s && !inp.modifiers.shift, s && inp.modifiers.shift, cmd && inp.key_pressed(egui::Key::E))
        });
        if open {
            self.open_dialog();
            return;
        }
        if self.scene_graph.is_none() || self.ui_state.is_loading {
            return;
        }
        if save {
            self.save_project();
            return;
        }
        if save_as {
            self.save_project_as();
            return;
        }
        if export {
            if ctx.input(|inp| inp.modifiers.shift) {
                self.export_selection_dialog(self.ui_state.selected_nodes());
            } else {
                self.export_dialog(None);
            }
            return;
        }
        let (undo, redo) = ctx.input(|inp| {
            let cmd = inp.modifiers.command;
            let z = inp.key_pressed(egui::Key::Z);
            (
                cmd && z && !inp.modifiers.shift,
                cmd && (inp.key_pressed(egui::Key::Y) || (z && inp.modifiers.shift)),
            )
        });
        if undo {
            self.undo();
            return;
        }
        if redo {
            self.redo();
            return;
        }
        // Copia e incolla: egui trasforma Ctrl+C / Ctrl+V in eventi (Ctrl+V solo se negli appunti
        // di sistema c'è del testo: per questo la copia ce ne mette)
        let (copy, paste) = ctx.input(|inp| {
            let cmd = inp.modifiers.command;
            (
                inp.events.iter().any(|e| matches!(e, egui::Event::Copy)) || (cmd && inp.key_pressed(egui::Key::C)),
                inp.events.iter().any(|e| matches!(e, egui::Event::Paste(_))) || (cmd && inp.key_pressed(egui::Key::V)),
            )
        });
        if copy {
            let nodes = self.ui_state.selected_nodes();
            if !nodes.is_empty() {
                self.apply(Action::Copy(nodes));
            }
            return;
        }
        if paste {
            self.apply(Action::Paste);
            return;
        }

        // Viste standard (tasti numerici, come nei CAD): 1 fronte, 3 destra, 7 alto, 0 iso;
        // con Ctrl la vista opposta. 5 = raddrizza.
        use crate::renderer::camera::StandardView as V;
        let view = ctx.input(|inp| {
            let ctrl = inp.modifiers.command;
            let key = |k| inp.key_pressed(k);
            if key(egui::Key::Num1) {
                Some(Action::View(if ctrl { V::Back } else { V::Front }))
            } else if key(egui::Key::Num3) {
                Some(Action::View(if ctrl { V::Left } else { V::Right }))
            } else if key(egui::Key::Num7) {
                Some(Action::View(if ctrl { V::Bottom } else { V::Top }))
            } else if key(egui::Key::Num0) {
                Some(Action::View(V::Iso))
            } else if key(egui::Key::Num5) {
                Some(Action::Straighten)
            } else {
                None
            }
        });
        if let Some(a) = view {
            self.apply(a);
        }
        // M: strumento di misura; Esc lo chiude (prima di togliere la selezione)
        let (m_key, esc_key) = ctx.input(|i| (i.key_pressed(egui::Key::M) && !i.modifiers.command, i.key_pressed(egui::Key::Escape)));
        if m_key {
            self.ui_state.measure = if self.ui_state.measure.is_some() { None } else { Some(Default::default()) };
            self.ui_state.eyedropper = false;
            self.ui_state.position.hinge.picking = false;
        }
        if esc_key && self.ui_state.position.hinge.picking {
            let hinge = &mut self.ui_state.position.hinge;
            hinge.picking = false;
            hinge.points.clear();
            self.ui_state.status_message = "Scelta dell'asse annullata".to_string();
            return;
        }
        if esc_key && self.ui_state.eyedropper {
            self.ui_state.eyedropper = false;
            self.ui_state.status_message = "Pipetta annullata".to_string();
            return;
        }
        if esc_key && self.ui_state.measure.is_some() {
            self.ui_state.measure = None;
            return;
        }
        let sel = self.ui_state.selected_node;
        let (h, i, del, esc, f, f2) = ctx.input(|inp| {
            (
                // H o Spazio: nasconde la selezione, o la mostra se è nascosta
                inp.key_pressed(egui::Key::H) || inp.key_pressed(egui::Key::Space),
                inp.key_pressed(egui::Key::I),
                inp.key_pressed(egui::Key::Delete),
                inp.key_pressed(egui::Key::Escape),
                inp.key_pressed(egui::Key::F) && !inp.modifiers.command,
                inp.key_pressed(egui::Key::F2),
            )
        });
        if let Some(n) = sel {
            if f2 {
                self.start_rename(n);
            }
            if h {
                if let Some(a) = self.toggle_selection_action() {
                    self.apply(a);
                }
            }
            if i {
                if let Some(a) = self.ui_state.isolate_action() {
                    self.apply(a);
                }
            }
            if del {
                if let Some(a) = self.ui_state.on_selection(Action::Delete) {
                    self.apply(a);
                }
            }
        }
        if esc {
            self.apply(Action::Select { node: None, reveal: false });
        }
        if f {
            let focus = self.ui_state.focus_action();
            self.apply(focus);
        }
    }
}

impl GestoreApp {
    /// Unisce alla scena aperta un file appena letto (`scene` è già passata da `into_file`).
    pub fn add_loaded_file(&mut self, scene: scene_graph::SceneGraph) {
        let path = self.project.loading_path.take();
        self.project.loading_adds = false;
        self.ui_state.is_loading = false;
        self.ui_state.load_progress = 1.0;
        self.load_state = LoadState::Ready;
        let Some(current) = &mut self.scene_graph else { return };

        let was_clean = current.revision == self.project.saved_revision;
        let (first_node, first_mesh, first_macro) =
            (current.nodes.len() as u32, current.shapes_data.len() as u32, current.macro_components.len());
        let (_, new_shapes) = scene.shape_visibility_counts();
        let name = scene.sources.first().map(|s| s.name.clone()).unwrap_or_default();
        let mut warnings: Vec<String> = scene.load_warnings.iter().map(|w| format!("{name}: {w}")).collect();
        current.append(scene);
        // Annulla/Ripeti restano validi e non tolgono il file: aggiungerlo non è una modifica
        for s in self.project.undo.iter_mut().chain(self.project.redo.iter_mut()) {
            s.extend_from(current, first_node, first_mesh, first_macro);
        }
        if was_clean {
            self.project.saved_revision = current.revision;
        }

        if let (Some(renderer), Some(rs)) = (&mut self.renderer, &self.wgpu_render_state) {
            let undrawn = renderer.append_meshes(&current.shapes_data, &rs.device);
            if undrawn > 0 {
                warnings.push(format!(
                    "{name}: {undrawn} pezzi troppo grandi per un buffer della scheda video: caricati ma non disegnati"
                ));
            }
            // Inquadra l'insieme, mantenendo l'orientamento della vista
            let all = current.compute_bounding_box();
            let (main, _) = current.main_visible_bounding_box();
            if all.is_valid() {
                renderer.camera.fit_to_bbox(if main.is_valid() { &main } else { &all });
                renderer.camera.scene_bounds = Some(all);
            }
        }
        self.ui_state.load_warnings.extend(warnings);
        let added = current.sources.len() - 1;
        if let Some(factor) = current.suggest_unit_scale(added) {
            self.ui_state.unit_suggestions.push(ui::UnitSuggestion {
                node: current.sources[added].node,
                factor,
                file: name.clone(),
            });
        }
        let files = current.open_files().count();
        self.ui_state.status_message =
            format!("Aggiunto: {name} ({new_shapes} shape)  ·  {files} file aperti");
        if current.sources.last().is_some_and(|s| s.scale == 1000.0) {
            self.ui_state.status_message.push_str(CONVERTED);
        }
        if let Some(p) = path {
            let mut s = self.settings.clone();
            s.add_recent(&p);
            self.apply_settings(s);
        }
    }
}

impl GestoreApp {
    /// Copia: ricorda i nodi da incollare (gli appunti sono interni a Dima 3D).
    pub fn copy_nodes(&mut self, nodes: Vec<u32>) {
        let Some(scene) = &self.scene_graph else { return };
        let kept: Vec<u32> = scene.copy_groups(&nodes).into_iter().flat_map(|(_, n)| n).collect();
        let label = match kept.as_slice() {
            [] => return,
            [one] => scene.display_name(*one),
            many => format!("{} elementi", many.len()),
        };
        self.ui_state.status_message = format!("Copiato: {label}  ·  Ctrl+V per incollare");
        // Un testo anche negli appunti di Windows: senza, egui non segnala Ctrl+V
        self.ui_state.clipboard_text = Some(format!("Dima 3D: {label}"));
        self.ui_state.clipboard = kept;
    }

    /// Sposta i nodi nell'albero (vedi `SceneGraph::move_into`); un solo passo di Annulla, e
    /// nessuno se non cambia niente.
    pub fn move_into(&mut self, nodes: Vec<u32>, into: u32) {
        if self.scene_graph.is_none() || nodes.is_empty() {
            return;
        }
        self.push_undo(None);
        let Some(scene) = &mut self.scene_graph else { return };
        let target = scene.display_name(into);
        let result = scene.move_into(&nodes, into);
        if let Some(parent) = scene.parent_of(nodes[0]) {
            self.ui_state.expanded.insert(parent);
        }
        let changed = matches!(result, Ok(n) if n > 0);
        self.ui_state.status_message = match result {
            Ok(0) => "Già in questo gruppo".to_string(),
            Ok(1) => format!("Spostato in «{target}»"),
            Ok(n) => format!("Spostati {n} elementi in «{target}»"),
            Err(e) => format!("Non spostato: {e}"),
        };
        if !changed {
            self.project.undo.pop();
        }
    }

    /// Nuovo gruppo con i nodi dati (vedi `SceneGraph::new_group`): si annulla con Ctrl+Z e
    /// appena creato si rinomina.
    pub fn new_group(&mut self, nodes: Vec<u32>) {
        self.create_group(|scene| scene.new_group(&nodes, "Nuovo gruppo"));
    }

    /// Crea un gruppo con `make` (un passo di Annulla), lo seleziona e ne apre la rinomina.
    fn create_group(&mut self, make: impl FnOnce(&mut crate::scene_graph::SceneGraph) -> Result<u32, String>) {
        let Some(scene) = &self.scene_graph else { return };
        let (first_node, first_mesh, first_macro) =
            (scene.nodes.len() as u32, scene.shapes_data.len() as u32, scene.macro_components.len());
        self.push_undo(None);
        let Some(scene) = &mut self.scene_graph else { return };
        match make(scene) {
            Ok(group) => {
                // Negli stati da annullare il gruppo non c'è ancora
                for s in &mut self.project.undo {
                    s.extend_removed(scene, first_node, first_mesh, first_macro);
                }
                if let Some(p) = scene.parent_of(group) {
                    self.ui_state.expanded.insert(p);
                }
                self.ui_state.expanded.insert(group);
                self.apply_without_undo(Action::Select { node: Some(group), reveal: true });
                self.ui_state.status_message = "Gruppo creato: scrivi il nome e premi Invio".to_string();
                self.start_rename(group);
            }
            Err(e) => {
                // Niente è cambiato: lo stato salvato per Annulla non serve
                self.project.undo.pop();
                self.ui_state.status_message = format!("Gruppo non creato: {e}");
            }
        }
    }

    /// Incolla: una copia dei nodi copiati, nella stessa posizione, come nuovo elemento in fondo
    /// all'albero (uno per file di provenienza). Si annulla con Ctrl+Z.
    pub fn paste(&mut self) {
        if self.ui_state.is_loading || self.project.busy() {
            return;
        }
        let Some(scene) = &self.scene_graph else { return };
        let groups = scene.copy_groups(&self.ui_state.clipboard);
        if groups.is_empty() {
            self.ui_state.status_message = "Niente da incollare: seleziona un pezzo e copialo con Ctrl+C".to_string();
            return;
        }
        self.push_undo(None);
        let Some(scene) = &mut self.scene_graph else { return };
        let (first_node, first_mesh, first_macro) =
            (scene.nodes.len() as u32, scene.shapes_data.len() as u32, scene.macro_components.len());
        let mut roots = Vec::new();
        for (source, nodes) in groups {
            let name = scene.copy_name(&nodes);
            let mesh = scene.shapes_data.len() as u32;
            if let Some((root, origins)) = scene.paste_copy(source, &nodes, &Default::default(), name) {
                self.project.reduction.copy_stash(mesh, &origins);
                roots.push(root);
            }
        }
        // Negli stati da annullare la copia non c'è ancora
        for s in &mut self.project.undo {
            s.extend_removed(scene, first_node, first_mesh, first_macro);
        }
        if let (Some(renderer), Some(rs)) = (&mut self.renderer, &self.wgpu_render_state) {
            renderer.append_meshes(&scene.shapes_data, &rs.device);
            let all = scene.compute_bounding_box();
            if all.is_valid() {
                renderer.camera.scene_bounds = Some(all);
            }
        }
        let pieces: u32 = roots.iter().map(|&r| scene.info[r as usize].shape_count).sum();
        let names: Vec<String> = roots.iter().map(|&r| scene.display_name(r)).collect();
        let select = match roots.as_slice() {
            [] => return,
            [one] => Action::Select { node: Some(*one), reveal: true },
            many => Action::SelectMany { nodes: many.to_vec(), primary: many[0], anchor: many[0] },
        };
        self.apply_without_undo(select);
        self.ui_state.status_message = format!(
            "Incollato: {} ({pieces} pezzi) nella stessa posizione dell'originale: spostalo con la finestra Posizione",
            names.join(", ")
        );
    }
}

impl GestoreApp {
    /// Scala un file (vedi `SceneGraph::scale_file`), ricarica la geometria sulla GPU e inquadra
    /// l'insieme.
    pub fn scale_file(&mut self, node: u32, factor: f32) {
        self.ui_state.unit_suggestions.retain(|s| s.node != node);
        let Some(scene) = &mut self.scene_graph else { return };
        let Some(source) = scene.sources.iter().position(|s| s.node == node) else { return };
        scene.scale_file(source, factor);
        let meshes = scene.sources[source].meshes.clone();
        self.project.reduction.scale(meshes.start as usize..meshes.end as usize, factor);
        // La stima della dimensione non vale più
        self.project.reduction.summary = None;
        let file = &scene.sources[source];
        self.ui_state.status_message = format!(
            "{}: {} (scala complessiva {})",
            file.name,
            if factor < 1.0 { "convertito da millimetri a metri" } else { "convertito da metri a millimetri" },
            file.scale
        );
        if let (Some(renderer), Some(rs)) = (&mut self.renderer, &self.wgpu_render_state) {
            let orientation = renderer.camera.orientation;
            renderer.upload_meshes(&scene.shapes_data, &rs.device);
            let all = scene.compute_bounding_box();
            let (main, _) = scene.main_visible_bounding_box();
            if all.is_valid() {
                renderer.camera.fit_to_bbox(if main.is_valid() { &main } else { &all });
                renderer.camera.scene_bounds = Some(all);
            }
            renderer.camera.orientation = orientation;
        }
    }
}

impl GestoreApp {
    /// Polling non bloccante degli eventi di caricamento dal worker thread.
    pub fn poll_loading(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.load_rx {
            let mut finished = false;
            while let Ok(mut event) = rx.try_recv() {
                if let LoadEvent::Finished(scene) = &mut event {
                    self.project.mark_loaded_step(scene);
                }
                match event {
                    LoadEvent::Started { path, file_size_bytes } => {
                        let mb = file_size_bytes as f64 / (1024.0 * 1024.0);
                        self.ui_state.status_message = format!("Caricamento {path} ({mb:.1} MB)...");
                        self.ui_state.load_progress = 0.05;
                    }
                    LoadEvent::Progress { percent, message } => {
                        self.ui_state.load_progress = percent;
                        self.ui_state.status_message = message;
                    }
                    LoadEvent::Finished(scene) if self.project.loading_adds && self.scene_graph.is_some() => {
                        self.add_loaded_file(scene);
                        finished = true;
                        break;
                    }
                    LoadEvent::Finished(scene) => {
                        let (_, shape_count) = scene.shape_visibility_counts();
                        let macro_count = scene.top_level_count();
                        let (main_bbox, far_parts) = scene.main_visible_bounding_box();
                        self.ui_state.load_warnings = scene.load_warnings.clone();

                        // Se il renderer GPU è disponibile, carica le mesh e inquadra la scena
                        if let (Some(renderer), Some(rs)) =
                            (&mut self.renderer, &self.wgpu_render_state)
                        {
                            let undrawn = renderer.upload_meshes(&scene.shapes_data, &rs.device);
                            match undrawn {
                                0 => {}
                                1 => self.ui_state.load_warnings.push(
                                    "1 pezzo troppo grande per un buffer della scheda video: caricato ma non disegnato"
                                        .to_string(),
                                ),
                                n => self.ui_state.load_warnings.push(format!(
                                    "{n} pezzi troppo grandi per un buffer della scheda video: caricati ma non disegnati"
                                )),
                            }
                            let bbox = scene.compute_bounding_box();
                            if bbox.is_valid() {
                                renderer.camera.set_standard_view(
                                    crate::renderer::camera::StandardView::Iso,
                                    self.settings.axes,
                                );
                                renderer.camera.fit_to_bbox(if main_bbox.is_valid() { &main_bbox } else { &bbox });
                                renderer.camera.scene_bounds = Some(bbox);
                            }
                        }

                        self.ui_state.status_message = format!(
                            "Caricato: {} shape in {} macro-oggetti principali",
                            shape_count, macro_count
                        );
                        if scene.sources.first().is_some_and(|s| s.scale == 1000.0) {
                            self.ui_state.status_message.push_str(CONVERTED);
                        }
                        if far_parts > 0 {
                            self.ui_state.status_message.push_str("  ·  ");
                            self.ui_state.status_message.push_str(&far_parts_message(far_parts));
                        }
                        self.ui_state.is_loading = false;
                        self.ui_state.load_progress = 1.0;
                        self.ui_state.select_single(None);
                        self.ui_state.renaming = None;
                        self.ui_state.expanded.clear();
                        // Gli appunti puntano ai nodi della scena precedente
                        self.ui_state.clipboard.clear();
                        // Finestra Posizione: valori, offset, calcoli e cerniera ripartono da zero
                        // (restano solo finestra aperta, unità e World)
                        self.ui_state.position.reset_values();
                        self.load_state = LoadState::Ready;
                        self.project.on_loaded(scene.revision);
                        if let Some(p) = self.project.current_path.clone() {
                            let mut s = self.settings.clone();
                            s.add_recent(&p);
                            self.apply_settings(s);
                        }
                        self.scene_graph = Some(scene);
                        finished = true;
                        break;
                    }
                    LoadEvent::Error(err) => {
                        self.project.loading_path = None;
                        self.project.loading_adds = false;
                        self.ui_state.status_message = format!("Errore: {err}");
                        self.ui_state.is_loading = false;
                        self.load_state = LoadState::Error(err);
                        finished = true;
                        break;
                    }
                }
            }

            if finished {
                self.load_rx = None;
                self.start_next_add();
                self.finish_restore();
            } else {
                // Richiedi repaint continuo durante il caricamento per animare la progress bar
                ctx.request_repaint();
            }
        }

    }
}

/// File aperto con coordinate da metri e convertito nei millimetri di Dima 3D.
const CONVERTED: &str =
    "  ·  convertito da metri a millimetri (per annullare: tasto destro sul file, «Unità di misura»)";

/// Avviso quando l'inquadratura lascia fuori pezzi molto lontani dal gruppo principale.
fn far_parts_message(count: usize) -> String {
    let parts = if count == 1 { "1 pezzo molto lontano".to_string() } else { format!("{count} pezzi molto lontani") };
    format!("{parts} dal gruppo principale fuori inquadratura: seleziona la radice nell'albero e premi F per vederli")
}

impl Default for GestoreApp {
    fn default() -> Self {
        Self::new_headless()
    }
}

impl eframe::App for GestoreApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.start(ctx);
        self.poll_loading(ctx);
        if let Some(text) = self.ui_state.clipboard_text.take() {
            ctx.copy_text(text);
        }

        // Export o riduzione in corso: a fine lavoro la scena torna al suo posto
        if self.project.export_job.is_some() {
            self.poll_export(ctx);
        }
        if self.project.reduction.job.is_some() && !self.poll_reduction() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        // La finestra non si chiude a metà lavoro (il file resterebbe troncato, la scena persa)
        if ctx.input(|i| i.viewport().close_requested()) && self.project.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.ui_state.status_message = "Attendi la fine del lavoro in corso prima di uscire.".to_string();
        }

        // Chiusura della finestra con modifiche non esportate → chiedi conferma
        if ctx.input(|i| i.viewport().close_requested()) && !self.project.allow_exit && self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.project.pending = Some(crate::project::Pending::Exit);
        }

        // File trascinati sulla finestra: con una scena aperta si aggiungono, altrimenti il
        // primo si apre e gli altri si aggiungono. Un PDF si apre nella finestra Misure PDF.
        let (pdfs, dropped): (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) = ctx
            .input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>())
            .into_iter()
            .partition(|p| crate::ui::pdf_panel::is_pdf(p));
        if let Some(pdf) = pdfs.into_iter().next() {
            self.ui_state.pdf.open_file(pdf);
        }
        if !dropped.is_empty() {
            self.request_add(dropped);
        }

        self.handle_shortcuts(ctx);
        ui::draw_main_layout(ctx, self);
        crate::project::draw_confirm_dialog(ctx, self);
        crate::project::draw_step_prompt(ctx, self);
        crate::project::draw_unit_suggestion(ctx, self);
        crate::project::draw_export_progress(ctx, self);
        // Finestre mobili (quelle agganciate le disegna `draw_main_layout`, prima della vista)
        crate::ui::reduce_panel::draw(ctx, self, crate::ui::dock::Pass::Floating);
        crate::ui::position_panel::draw(ctx, self, crate::ui::dock::Pass::Floating);
        crate::ui::cabin_panel::draw(ctx, self, crate::ui::dock::Pass::Floating);
        crate::ui::robot_panel::draw(ctx, self, crate::ui::dock::Pass::Floating);
        crate::ui::pdf_panel::draw(ctx, self, crate::ui::dock::Pass::Floating);
        crate::ui::about::draw(ctx, self);
        crate::update::draw(ctx, self);
        self.update_window_title(ctx);
    }
}
