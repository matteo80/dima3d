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
    /// Ripristina i colori originali di tutta la scena.
    ResetAllColors,
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
    /// Riapre i file aperti con le impostazioni correnti (es. nuovo raggruppamento automatico).
    ReloadFiles,
    /// Scala la geometria del file con questo nodo radice (es. 0.001 da millimetri a metri).
    /// Non entra in Annulla: si torna indietro con la scala inversa.
    ScaleFile(u32, f32),
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
                | Action::ScaleFile(..)
                | Action::ReloadFiles
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
}

impl GestoreApp {
    /// Inizializza l'applicazione con il contesto nativo eframe (incluso il backend wgpu).
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.clone();
        let settings = crate::settings::Settings::load();
        parser::jt::cleanup_old_conversions();
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
        };
        // File passato a riga di comando o con "Apri con" di Windows
        if let Some(path) = std::env::args_os().nth(1) {
            app.open_wrl_async(path.into());
        }
        app
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

        if parser::jt::is_jt(&path) {
            parser::jt::convert_and_load(path, self.settings.cluster, tx);
        } else {
            parser::load_wrl_async_with(path, self.settings.cluster, tx);
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
        if action == Action::ReloadFiles {
            self.request_reload();
            return;
        }
        if let Action::ScaleFile(n, factor) = action {
            self.scale_file(n, factor);
            return;
        }
        if action.is_edit() {
            let coalesce = match &action {
                Action::SetColor(n, _) => Some(*n),
                Action::Batch(list) => match list.first() {
                    Some(Action::SetColor(n, _)) => Some(*n),
                    _ => None,
                },
                _ => None,
            };
            self.push_undo(coalesce);
        }
        self.apply_without_undo(action);
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

    fn apply_without_undo(&mut self, action: Action) {
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
            _ => {}
        }
        let Some(scene) = &mut self.scene_graph else {
            return;
        };
        match action {
            Action::Batch(_) | Action::IsolateMany(_) | Action::FocusMany(_) => {}
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
            Action::StartRename(_) | Action::ExportNode(_) | Action::ScaleFile(..) | Action::ReloadFiles => {}
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
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.project.pending.is_some() || self.project.export_job.is_some() {
            return;
        }
        let (open, save) = ctx.input(|inp| {
            let cmd = inp.modifiers.command;
            (cmd && inp.key_pressed(egui::Key::O), cmd && inp.key_pressed(egui::Key::S))
        });
        if open {
            self.open_dialog();
            return;
        }
        if self.scene_graph.is_none() {
            return;
        }
        if save {
            self.export_dialog(None);
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
        }
        if esc_key && self.ui_state.measure.is_some() {
            self.ui_state.measure = None;
            return;
        }
        let sel = self.ui_state.selected_node;
        let (h, i, del, esc, f, f2) = ctx.input(|inp| {
            (
                inp.key_pressed(egui::Key::H),
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

/// Apre nell'albero la radice di un file e, se contiene un solo elemento (tipico involucro
/// dell'assieme), anche quello.
fn expand_file_root(ui_state: &mut ui::UiState, scene: &scene_graph::SceneGraph, root: Option<u32>) {
    let Some(root) = root else { return };
    ui_state.expanded.insert(root);
    if let [only] = scene.nodes[root as usize].children() {
        ui_state.expanded.insert(*only);
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
        let root = current.roots.iter().copied().find(|&r| r >= first_node);
        let added = current.sources.len() - 1;
        if let Some(factor) = current.suggest_unit_scale(added) {
            self.ui_state.unit_suggestions.push(ui::UnitSuggestion {
                node: current.sources[added].node,
                factor,
                file: name.clone(),
            });
        }
        expand_file_root(&mut self.ui_state, current, root);
        let files = current.open_files().count();
        self.ui_state.status_message =
            format!("Aggiunto: {name} ({new_shapes} shape)  ·  {files} file aperti");
        if let Some(p) = path {
            let mut s = self.settings.clone();
            s.add_recent(&p);
            self.apply_settings(s);
        }
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
        // Polling non bloccante degli eventi di caricamento dal worker thread
        if let Some(rx) = &self.load_rx {
            let mut finished = false;
            while let Ok(event) = rx.try_recv() {
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
                        if far_parts > 0 {
                            self.ui_state.status_message.push_str("  ·  ");
                            self.ui_state.status_message.push_str(&far_parts_message(far_parts));
                        }
                        self.ui_state.is_loading = false;
                        self.ui_state.load_progress = 1.0;
                        self.ui_state.select_single(None);
                        self.ui_state.renaming = None;
                        self.ui_state.expanded.clear();
                        expand_file_root(&mut self.ui_state, &scene, scene.roots.first().copied());
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
            } else {
                // Richiedi repaint continuo durante il caricamento per animare la progress bar
                ctx.request_repaint();
            }
        }

        // Export in corso: a fine lavoro la scena torna al suo posto
        if self.project.export_job.is_some() {
            self.poll_export(ctx);
            // La finestra non si chiude a metà export (il file resterebbe troncato)
            if ctx.input(|i| i.viewport().close_requested()) && self.project.export_job.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.ui_state.status_message = "Attendi la fine dell'esportazione prima di uscire.".to_string();
            }
        }

        // Chiusura della finestra con modifiche non esportate → chiedi conferma
        if ctx.input(|i| i.viewport().close_requested()) && !self.project.allow_exit && self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.project.pending = Some(crate::project::Pending::Exit);
        }

        // File trascinati sulla finestra: con una scena aperta si aggiungono, altrimenti il
        // primo si apre e gli altri si aggiungono
        let dropped: Vec<std::path::PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.request_add(dropped);
        }

        self.handle_shortcuts(ctx);
        ui::draw_main_layout(ctx, self);
        crate::project::draw_confirm_dialog(ctx, self);
        crate::project::draw_unit_suggestion(ctx, self);
        crate::project::draw_export_progress(ctx, self);
        self.update_window_title(ctx);
    }
}
