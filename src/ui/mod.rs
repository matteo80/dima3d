pub mod about;
pub mod cabin_panel;
pub mod dock;
pub mod color_menu;
pub mod measure;
pub mod pdf_panel;
pub mod position_panel;
pub mod progress;
pub mod properties;
pub mod reduce_panel;
pub mod robot_panel;
pub mod scene_tree;
pub mod theme;
pub mod toolbar;
pub mod view_cube;
pub mod viewport;

/// Rinomina in corso nell'albero.
#[derive(Debug, Clone)]
pub struct Renaming {
    pub node: u32,
    pub text: String,
    /// Richiede il focus sul campo di testo al primo frame
    pub focus: bool,
}

/// Stato dell'interfaccia utente.
#[derive(Debug)]
pub struct UiState {
    /// Rinomina in corso (campo di testo sulla riga dell'albero)
    pub renaming: Option<Renaming>,
    /// Testo di ricerca nell'albero
    pub search: String,
    /// Risultato della ricerca: (testo, revisione scena, righe, numero di corrispondenze)
    pub search_cache: Option<(String, u64, Vec<(u32, u16)>, usize)>,
    /// Indice del nodo selezionato nel SceneGraph (se presente)
    pub selected_node: Option<u32>,
    /// Altri nodi selezionati oltre a `selected_node` (selezione multipla nell'albero)
    pub selected_extra: std::collections::BTreeSet<u32>,
    /// Estremo fisso della selezione a intervallo con Shift
    pub selection_anchor: Option<u32>,
    /// Scorrimento e altezza visibile dell'albero all'ultimo frame
    pub tree_view: (f32, f32),
    /// Nodi contenitore espansi nell'albero
    pub expanded: std::collections::HashSet<u32>,
    /// Richiesta di scorrere l'albero fino al nodo selezionato (es. dopo un click 3D)
    pub scroll_to_selected: bool,
    /// True durante il caricamento asincrono
    pub is_loading: bool,
    /// Progresso del caricamento (0.0 → 1.0)
    pub load_progress: f32,
    /// Messaggio di stato nella barra in basso
    pub status_message: String,
    /// Parti del file aperto non caricate o non disegnate: restano visibili finché il file è aperto
    pub load_warnings: Vec<String>,
    /// Proposte di conversione delle unità per i file appena aggiunti
    pub unit_suggestions: Vec<UnitSuggestion>,
    /// Strumento di misura attivo (il clic nella vista prende punti invece di selezionare)
    pub measure: Option<measure::Measure>,
    /// Pipetta attiva: il clic nella vista legge il colore di un pezzo invece di selezionare
    pub eyedropper: bool,
    /// Finestra Info aperta
    pub about: bool,
    /// Piano di sezione attivo
    pub section: Option<crate::renderer::Section>,
    /// Riferimento del trascinamento in corso nella viewport
    pub drag: Option<viewport::DragState>,
    /// Punto usato dall'ultimo scatto di rotella, riutilizzabile finché nulla si muove
    pub zoom_anchor: Option<viewport::ZoomAnchor>,
    /// Finestra «Posizione»
    pub position: position_panel::PositionPanel,
    /// Finestra della cabina aperta
    pub cabin_open: bool,
    /// Finestra «Misure PDF» (con il PDF aperto)
    pub pdf: pdf_panel::PdfPanel,
    /// Finestra dei robot aperta
    pub robot_open: bool,
    /// Cabina su cui lavora la finestra Cabina (`None`: l'ultima costruita)
    pub cabin_target: Option<crate::cabin::CabinChoice>,
    /// Selezione trascinando nell'albero: riga da cui è partita
    pub tree_sweep: Option<u32>,
    /// Selezione a rettangolo nell'albero (partita dallo spazio vuoto): punto di partenza, con la Y
    /// misurata dall'inizio delle righe (resta giusta se l'albero scorre)
    pub tree_band: Option<egui::Pos2>,
    /// Nodi copiati con Ctrl+C, da incollare con Ctrl+V
    pub clipboard: Vec<u32>,
    /// Testo da mettere negli appunti di sistema al prossimo frame
    pub clipboard_text: Option<String>,
}

/// Un file aggiunto sembra scritto in altre unità rispetto alla scena.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitSuggestion {
    /// Nodo radice del file
    pub node: u32,
    /// 0.001 (da millimetri a metri) o 1000 (da metri a millimetri)
    pub factor: f32,
    pub file: String,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            renaming: None,
            search: String::new(),
            search_cache: None,
            selected_node: None,
            selected_extra: Default::default(),
            selection_anchor: None,
            tree_view: (0.0, 0.0),
            expanded: std::collections::HashSet::new(),
            scroll_to_selected: false,
            is_loading: false,
            load_progress: 0.0,
            status_message: "Pronto. Apri un file 3D per iniziare.".to_string(),
            load_warnings: Vec::new(),
            unit_suggestions: Vec::new(),
            measure: None,
            eyedropper: false,
            about: false,
            section: None,
            drag: None,
            zoom_anchor: None,
            position: Default::default(),
            cabin_open: false,
            robot_open: false,
            pdf: Default::default(),
            cabin_target: None,
            tree_sweep: None,
            tree_band: None,
            clipboard: Vec::new(),
            clipboard_text: None,
        }
    }
}

impl UiState {
    /// Tutti i nodi selezionati, in ordine crescente.
    pub fn selected_nodes(&self) -> Vec<u32> {
        let mut nodes: Vec<u32> = self.selected_extra.iter().copied().collect();
        if let Some(n) = self.selected_node {
            if let Err(i) = nodes.binary_search(&n) {
                nodes.insert(i, n);
            }
        }
        nodes
    }

    /// Selezione di un solo nodo (o nessuno).
    pub fn select_single(&mut self, node: Option<u32>) {
        self.selected_node = node;
        self.selected_extra.clear();
        self.selection_anchor = node;
    }

    /// Toglie dalla selezione i nodi che non esistono più.
    pub fn drop_from_selection(&mut self, gone: impl Fn(u32) -> bool) {
        self.selected_extra.retain(|&n| !gone(n));
        if self.selected_node.is_some_and(&gone) {
            self.selected_node = self.selected_extra.pop_first();
        }
        if self.selection_anchor.is_some_and(&gone) {
            self.selection_anchor = self.selected_node;
        }
    }

    /// La stessa azione su tutti i nodi selezionati, annullabile in un colpo solo.
    pub fn on_selection(&self, action: impl Fn(u32) -> crate::app::Action) -> Option<crate::app::Action> {
        let nodes = self.selected_nodes();
        match nodes.as_slice() {
            [] => None,
            [n] => Some(action(*n)),
            _ => Some(crate::app::Action::Batch(nodes.into_iter().map(action).collect())),
        }
    }

    /// Isola la selezione.
    pub fn isolate_action(&self) -> Option<crate::app::Action> {
        let nodes = self.selected_nodes();
        match nodes.as_slice() {
            [] => None,
            [n] => Some(crate::app::Action::Isolate(*n)),
            _ => Some(crate::app::Action::IsolateMany(nodes)),
        }
    }

    /// Inquadra la selezione, oppure il modello se non c'è selezione.
    pub fn focus_action(&self) -> crate::app::Action {
        let nodes = self.selected_nodes();
        match nodes.as_slice() {
            [] => crate::app::Action::Focus(None),
            [n] => crate::app::Action::Focus(Some(*n)),
            _ => crate::app::Action::FocusMany(nodes),
        }
    }
}

#[cfg(test)]
mod selection_tests {
    use crate::app::{Action, GestoreApp};

    fn key(k: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        vec![egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }]
    }

    /// Un frame completo dell'interfaccia con gli eventi dati. Ritorna le posizioni dei testi.
    fn frame(
        ctx: &egui::Context,
        app: &mut GestoreApp,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Vec<(String, egui::Pos2)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            events,
            modifiers,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| super::draw_main_layout(ctx, app));
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
        texts
    }

    /// Click vero con il mouse sulla riga dell'albero che contiene `name`.
    fn click(ctx: &egui::Context, app: &mut GestoreApp, name: &str, modifiers: egui::Modifiers) {
        let texts = frame(ctx, app, vec![], modifiers);
        let (_, pos) = texts
            .iter()
            .find(|(t, _)| t.contains(name))
            .unwrap_or_else(|| panic!("{name} non disegnato: {texts:?}"));
        let pos = *pos + egui::vec2(10.0, 6.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        frame(ctx, app, vec![egui::Event::PointerMoved(pos)], modifiers);
        frame(ctx, app, vec![button(true)], modifiers);
        frame(ctx, app, vec![button(false)], modifiers);
        frame(ctx, app, vec![], modifiers);
    }

    fn names(app: &GestoreApp) -> Vec<String> {
        let scene = app.scene_graph.as_ref().unwrap();
        app.ui_state.selected_nodes().iter().map(|&n| scene.display_name(n)).collect()
    }

    fn setup() -> (egui::Context, GestoreApp, [String; 3]) {
        let mut app = GestoreApp::new_headless();
        let scene = crate::parser::parse_wrl(include_bytes!("../../esempi/test_colori.wrl"));
        let root = scene.roots[0];
        app.ui_state.expanded.insert(root);
        let kids = scene.nodes[root as usize].children();
        assert!(kids.len() >= 3, "la scena di prova deve avere almeno 3 figli");
        let names = [0, 1, 2].map(|i| scene.display_name(kids[i]));
        app.scene_graph = Some(scene);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![], egui::Modifiers::NONE);
        }
        (ctx, app, names)
    }

    #[test]
    fn shift_click_selects_the_range() {
        let (ctx, mut app, [a, b, c]) = setup();
        click(&ctx, &mut app, &a, egui::Modifiers::NONE);
        assert_eq!(names(&app), vec![a.clone()]);
        click(&ctx, &mut app, &c, egui::Modifiers::SHIFT);
        assert_eq!(names(&app), vec![a.clone(), b.clone(), c.clone()]);
        // L'estremo fisso resta il primo click
        click(&ctx, &mut app, &b, egui::Modifiers::SHIFT);
        assert_eq!(names(&app), vec![a.clone(), b.clone()]);
        // Ctrl toglie o aggiunge un singolo elemento
        click(&ctx, &mut app, &a, egui::Modifiers::COMMAND);
        assert_eq!(names(&app), vec![b.clone()]);
        // Un click normale torna alla selezione singola
        click(&ctx, &mut app, &c, egui::Modifiers::NONE);
        assert_eq!(names(&app), vec![c]);
    }

    #[test]
    fn arrow_keys_move_and_extend_the_selection() {
        let (ctx, mut app, [a, b, c]) = setup();
        click(&ctx, &mut app, &a, egui::Modifiers::NONE);
        assert!(!ctx.wants_keyboard_input(), "il click non deve bloccare la tastiera");
        let press = |ctx: &egui::Context, app: &mut GestoreApp, k, m| {
            frame(ctx, app, key(k, m), m);
            frame(ctx, app, vec![], egui::Modifiers::NONE);
        };
        press(&ctx, &mut app, egui::Key::ArrowDown, egui::Modifiers::NONE);
        assert_eq!(names(&app), vec![b.clone()]);
        press(&ctx, &mut app, egui::Key::ArrowDown, egui::Modifiers::SHIFT);
        assert_eq!(names(&app), vec![b.clone(), c.clone()]);
        press(&ctx, &mut app, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
        assert_eq!(names(&app), vec![b.clone()]);
        press(&ctx, &mut app, egui::Key::ArrowUp, egui::Modifiers::NONE);
        assert_eq!(names(&app), vec![a]);

        // Sinistra sale al padre, destra lo riapre e scende
        press(&ctx, &mut app, egui::Key::ArrowLeft, egui::Modifiers::NONE);
        let root = app.scene_graph.as_ref().unwrap().roots[0];
        assert_eq!(app.ui_state.selected_node, Some(root));
        press(&ctx, &mut app, egui::Key::ArrowLeft, egui::Modifiers::NONE);
        assert!(!app.ui_state.expanded.contains(&root));
        press(&ctx, &mut app, egui::Key::ArrowRight, egui::Modifiers::NONE);
        assert!(app.ui_state.expanded.contains(&root));
    }

    #[test]
    fn actions_apply_to_the_whole_selection_with_one_undo() {
        let (ctx, mut app, [a, _, c]) = setup();
        click(&ctx, &mut app, &a, egui::Modifiers::NONE);
        click(&ctx, &mut app, &c, egui::Modifiers::SHIFT);
        let selected = app.ui_state.selected_nodes();
        assert_eq!(selected.len(), 3);
        let visible = |app: &GestoreApp, n: u32| app.scene_graph.as_ref().unwrap().nodes[n as usize].is_visible();

        let hide = app.ui_state.on_selection(Action::Hide).unwrap();
        app.apply(hide);
        assert!(selected.iter().all(|&n| !visible(&app, n)));
        assert_eq!(app.project.undo.len(), 1);
        app.undo();
        assert!(selected.iter().all(|&n| visible(&app, n)));

        let isolate = app.ui_state.isolate_action().unwrap();
        app.apply(isolate);
        assert!(selected.iter().all(|&n| visible(&app, n)));
        app.undo();

        let delete = app.ui_state.on_selection(Action::Delete).unwrap();
        app.apply(delete);
        assert!(app.ui_state.selected_nodes().is_empty());
        assert_eq!(app.project.undo.len(), 1);
        app.undo();
        // Dopo l'annullamento i nodi esistono di nuovo, ma non tornano selezionati
        let scene = app.scene_graph.as_ref().unwrap();
        assert!(selected.iter().all(|&n| !scene.info[n as usize].deleted));
    }
}

#[cfg(test)]
mod layout_tests {
    /// Regressione: il pannello Scena si allargava a ogni frame fino a coprire la vista 3D.
    #[test]
    fn scene_panel_width_is_stable() {
        let mut app = crate::app::GestoreApp::new_headless();
        let scene = crate::parser::parse_wrl(include_bytes!("../../esempi/test_colori.wrl"));
        app.ui_state.expanded.insert(scene.roots[0]);
        app.scene_graph = Some(scene);

        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            ..Default::default()
        };
        let panel_width = |ctx: &egui::Context| {
            egui::containers::panel::PanelState::load(ctx, egui::Id::new("scene_tree_panel"))
                .map(|s| s.rect.width())
                .unwrap_or(0.0)
        };
        // Tutti i testi disegnati con la loro posizione verticale
        fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, f32)> {
            fn walk(s: &egui::Shape, out: &mut Vec<(String, f32)>) {
                match s {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos.y)),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            let mut out = Vec::new();
            shapes.iter().for_each(|c| walk(&c.shape, &mut out));
            out
        }
        let mut widths = Vec::new();
        for frame in 0..60 {
            if frame == 30 {
                app.ui_state.search = "blu".to_string(); // anche con una ricerca attiva
            }
            let out = ctx.run(input(), |ctx| super::draw_main_layout(ctx, &mut app));
            widths.push(panel_width(&ctx));

            // Regressione: il campo di ricerca occupava tutta l'altezza del pannello,
            // finiva a metà e l'elenco dei macro-oggetti spariva.
            if frame == 5 || frame == 35 {
                let t = texts(&out.shapes);
                let y_of = |needle: &str| t.iter().find(|(s, _)| s.contains(needle)).map(|p| p.1);
                let root = y_of("Assieme").unwrap_or_else(|| panic!("radice non disegnata: {t:?}"));
                assert!(root < 200.0, "elenco troppo in basso (y = {root})");
                if frame == 5 {
                    let field = y_of("Cerca per nome").expect("campo di ricerca non disegnato");
                    assert!(field < 120.0, "campo di ricerca a metà pannello (y = {field})");
                }
            }
        }
        let (first, last) = (widths[2], *widths.last().unwrap());
        assert!(first > 150.0 && first < 400.0, "larghezza iniziale anomala: {first}");
        assert!((last - first).abs() < 1.0, "il pannello cambia larghezza: {widths:?}");
    }

    /// Regressione: un nome CAD lunghissimo nelle proprietà allargava il pannello Scena.
    #[test]
    fn properties_do_not_widen_the_panel() {
        let long = "XX00ABC12345_006-PANNELLO_SPORTELLO_CARBURANTE_123456_CON_UN_NOME_MOLTO_MOLTO_LUNGO";
        let wrl = format!(
            "#VRML V2.0 utf8\nDEF {long} Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ 0 0 0, 1 0 0, 0 1 0 ] }} coordIndex [ 0, 1, 2, -1 ] }} }}\n"
        );
        let mut app = crate::app::GestoreApp::new_headless();
        let mut scene = crate::parser::parse_wrl(wrl.as_bytes());
        scene.into_file("pezzo.wrl".into(), None);
        let shape = scene.mesh_to_node[0];
        app.ui_state.expanded.insert(scene.roots[0]);
        app.scene_graph = Some(scene);
        app.ui_state.select_single(Some(shape));

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let mut widths = Vec::new();
        for _ in 0..10 {
            ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
                super::draw_main_layout(ctx, &mut app)
            });
            widths.push(
                egui::containers::panel::PanelState::load(&ctx, egui::Id::new("scene_tree_panel"))
                    .map_or(0.0, |s| s.rect.width()),
            );
        }
        assert!(widths.iter().all(|&w| w < 320.0), "il pannello si allarga: {widths:?}");
    }

    /// Due file aperti: l'albero mostra un nodo per file, e compare la proposta di conversione
    /// delle unità per il file in millimetri; "Converti" la applica e chiude la finestra.
    #[test]
    fn several_files_are_shown_with_the_unit_proposal() {
        use crate::scene_graph::node::multi_file_tests::{file, BODY_M, DOOR_MM};
        let mut app = crate::app::GestoreApp::new_headless();
        let scene = file(BODY_M, "scocca.wrl");
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.add_loaded_file(file(DOOR_MM, "sportello.wrl"));
        assert_eq!(app.ui_state.unit_suggestions.len(), 1);

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let mut texts = Vec::new();
        let mut convert_at = None;
        for _ in 0..4 {
            let out = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
                super::draw_main_layout(ctx, &mut app);
                crate::project::draw_unit_suggestion(ctx, &mut app);
            });
            texts.clear();
            fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
                match s {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            convert_at = texts.iter().find(|(t, _)| t == "Converti").map(|(_, p)| *p + egui::vec2(8.0, 6.0));
        }
        for name in ["scocca.wrl", "sportello.wrl", "Unità di misura diverse"] {
            assert!(texts.iter().any(|(t, _)| t.contains(name)), "manca «{name}»: {texts:?}");
        }

        // Click su "Converti"
        let at = convert_at.expect("pulsante Converti non disegnato");
        let click = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![click(true)], vec![click(false)], vec![]] {
            ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                super::draw_main_layout(ctx, &mut app);
                crate::project::draw_unit_suggestion(ctx, &mut app);
            });
        }
        assert!(app.ui_state.unit_suggestions.is_empty());
        let scene = app.scene_graph.as_ref().unwrap();
        assert_eq!(scene.sources[1].scale, 0.001);
        assert!(app.is_dirty(), "la conversione cambia l'export");
    }
}

/// Layout principale dell'applicazione.
/// Organizza i pannelli egui: toolbar (top), scene tree (left), viewport (center), status (bottom).
pub fn draw_main_layout(ctx: &egui::Context, app: &mut crate::app::GestoreApp) {
    // ── TopPanel: toolbar ──────────────────────────────────────────────────────
    egui::TopBottomPanel::top("toolbar_panel")
        .resizable(false)
        .exact_height(42.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                toolbar::draw_toolbar(ui, app);
            });
        });

    // ── BottomPanel: status bar ────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("status_bar")
        .resizable(false)
        .exact_height(26.0)
        .show(ctx, |ui| {
            progress::draw_status_bar(ui, app);
        });

    // ── SidePanel: scene tree ──────────────────────────────────────────────────
    egui::SidePanel::left("scene_tree_panel")
        .resizable(true)
        .min_width(200.0)
        .default_width(280.0)
        // Rete di sicurezza: il pannello non può mai togliere più di metà finestra alla vista 3D
        .max_width((ctx.screen_rect().width() * 0.5).max(220.0))
        .show(ctx, |ui| {
            // Proprietà della selezione in fondo; l'albero occupa il resto
            if app.ui_state.selected_node.is_some() && app.scene_graph.is_some() {
                egui::TopBottomPanel::bottom("properties_panel")
                    .resizable(false)
                    .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
                    .show_inside(ui, |ui| properties::draw(ui, app));
            }
            // L'albero gestisce da sé lo ScrollArea (virtualizzato con show_rows)
            scene_tree::draw_scene_tree(ui, app);
        });

    // ── Finestre agganciate ai bordi (prima della vista, che prende lo spazio che resta) ──
    reduce_panel::draw(ctx, app, dock::Pass::Docked);
    position_panel::draw(ctx, app, dock::Pass::Docked);
    cabin_panel::draw(ctx, app, dock::Pass::Docked);
    robot_panel::draw(ctx, app, dock::Pass::Docked);
    pdf_panel::draw(ctx, app, dock::Pass::Docked);

    // ── CentralPanel: viewport 3D ──────────────────────────────────────────────
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
        viewport::draw_viewport(ui, app);
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::app::GestoreApp;

    fn screen() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))
    }

    /// Un frame dell'interfaccia completa; ritorna i testi disegnati con la loro posizione.
    pub fn frame(ctx: &egui::Context, app: &mut GestoreApp, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
        let out = ctx.run(egui::RawInput { screen_rect: Some(screen()), events, ..Default::default() }, |ctx| {
            super::draw_main_layout(ctx, app);
        });
        fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
        texts
    }

    /// Clic sinistro (pressione e rilascio) in un punto.
    pub fn click(ctx: &egui::Context, app: &mut GestoreApp, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]] {
            frame(ctx, app, events);
        }
    }
}
