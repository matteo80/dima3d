use super::theme::{self, icon, icon_text};
use crate::app::Action;
use crate::settings::{RotationMode, ThemeMode};

/// Sotto questa larghezza di finestra i pulsanti mostrano solo l'icona.
const FULL_LABELS_WIDTH: f32 = 1180.0;

/// Pulsante con icona: il testo sparisce nelle finestre strette e resta nel suggerimento.
fn tool(ui: &mut egui::Ui, enabled: bool, compact: bool, icon: &str, label: &str, hint: &str) -> bool {
    let text = icon_text(ui, icon, if compact { "" } else { label });
    let response = ui.add_enabled(enabled, egui::Button::new(text));
    let response = if compact {
        response.on_hover_text(format!("{label}\n{hint}"))
    } else {
        response.on_hover_text(hint)
    };
    response.clicked()
}

fn group_separator(ui: &mut egui::Ui) {
    ui.add(egui::Separator::default().spacing(10.0));
}

/// Disegna la toolbar principale dell'applicazione: File, Modifica, Selezione, Scena.
pub fn draw_toolbar(ui: &mut egui::Ui, app: &mut crate::app::GestoreApp) {
    theme::flat_buttons(ui);
    let compact = ui.ctx().screen_rect().width() < FULL_LABELS_WIDTH;

    let selection = app.ui_state.selected_node;
    let has_scene = app.scene_graph.is_some();
    let has_selection = selection.is_some() && has_scene;
    let selection_visible = match (selection, &app.scene_graph) {
        (Some(n), Some(scene)) => scene.nodes.get(n as usize).map(|x| x.is_visible()).unwrap_or(false),
        _ => false,
    };
    let loading = app.ui_state.is_loading;
    let mut action: Option<Action> = None;

    // ── File ──────────────────────────────────────────────────────────────────
    if tool(ui, !loading, compact, icon::FOLDER_OPEN, "Apri", "Apri un file WRL (puoi anche trascinarlo sulla finestra)  [Ctrl+O]") {
        app.open_dialog();
    }
    let recent = app.settings.recent.clone();
    let mut open_path = None;
    ui.add_enabled_ui(!loading && !recent.is_empty(), |ui| {
        ui.menu_button(icon_text(ui, icon::CLOCK_COUNTER_CLOCKWISE, ""), |ui| {
            ui.label(egui::RichText::new("File recenti").strong());
            for p in &recent {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let response = ui
                    .add_enabled(p.exists(), egui::Button::new(name))
                    .on_hover_text(p.display().to_string());
                if response.clicked() {
                    open_path = Some(p.clone());
                    ui.close_menu();
                }
            }
        })
        .response
        .on_hover_text("File recenti");
    });
    if let Some(p) = open_path {
        app.request_open(p);
    }
    if tool(ui, has_scene && !loading, compact, icon::FILE_PLUS, "Aggiungi", "Aggiunge uno o più file WRL alla scena aperta (anche trascinandoli sulla finestra)") {
        app.add_dialog();
    }
    if tool(ui, has_scene, compact, icon::EXPORT, "Esporta", "Salva solo le parti visibili in un nuovo file WRL  [Ctrl+S]") {
        app.export_dialog(None);
    }
    let mut export_settings = app.settings.clone();
    ui.menu_button(icon_text(ui, icon::CARET_DOWN, ""), |ui| {
        ui.label(egui::RichText::new("Formato dell'export").strong());
        ui.checkbox(&mut export_settings.export_compact, "Formato compatto").on_hover_text(
            "Niente rientri e numeri con la precisione che serve (un milionesimo dell'ingombro): il file è meno              della metà. Senza, si copia il testo del file originale.",
        );
        ui.checkbox(&mut export_settings.export_normals, "Scrivi le normali").on_hover_text(
            "Con le normali gli spigoli vivi restano quelli del file. Senza, il visualizzatore le ricalcola              (creaseAngle 0,5) e il file è circa la metà.",
        );
    })
    .response
    .on_hover_text("Opzioni dell'export");
    app.apply_settings(export_settings);
    let reduce_open = app.project.reduction.panel.open;
    let reduce_text = icon_text(ui, icon::TRIANGLE_DASHED, if compact { "" } else { "Riduci" });
    let reduce = ui
        .add_enabled(has_scene && !loading, egui::Button::new(reduce_text).selected(reduce_open))
        .on_hover_text("Riduci facce
Toglie triangoli dove non servono (come MeshLab), con anteprima e dimensione prevista del file");
    if reduce.clicked() {
        app.project.reduction.panel.open = !reduce_open;
    }
    if tool(ui, has_scene && !loading, compact, icon::X, "Chiudi", "Chiude tutti i file aperti (chiede conferma se ci sono modifiche non esportate). Per chiuderne uno solo: tasto destro sul file nell'albero") {
        app.request_close();
    }

    group_separator(ui);

    // ── Modifica ──────────────────────────────────────────────────────────────
    let (undo_steps, redo_steps) = (app.project.undo.len(), app.project.redo.len());
    let undo_hint = format!("Annulla  [Ctrl+Z]  ({undo_steps} passi)");
    if tool(ui, has_scene && undo_steps > 0, true, icon::ARROW_U_UP_LEFT, "Annulla", &undo_hint) {
        app.undo();
    }
    let redo_hint = format!("Ripeti  [Ctrl+Y]  ({redo_steps} passi)");
    if tool(ui, has_scene && redo_steps > 0, true, icon::ARROW_U_UP_RIGHT, "Ripeti", &redo_hint) {
        app.redo();
    }

    group_separator(ui);

    // ── Selezione ─────────────────────────────────────────────────────────────
    if tool(ui, has_selection, compact, icon::CUBE_FOCUS, "Isola", "Mostra solo l'elemento selezionato  [I]") {
        action = app.ui_state.isolate_action();
    }
    let (eye, label, hint) = if selection_visible || !has_selection {
        (icon::EYE_SLASH, "Nascondi", "Nascondi l'elemento selezionato  [H]")
    } else {
        (icon::EYE, "Mostra", "Mostra l'elemento selezionato  [H]")
    };
    if tool(ui, has_selection, compact, eye, label, hint) {
        action = app.toggle_selection_action();
    }
    if tool(ui, has_selection, compact, icon::TRASH, "Elimina", "Rimuove l'elemento dalla scena e dall'export  [Canc]") {
        action = app.ui_state.on_selection(Action::Delete);
    }
    let mut color_actions: Vec<Action> = Vec::new();
    ui.add_enabled_ui(has_selection, |ui| {
        let title = icon_text(ui, icon::PALETTE, if compact { "" } else { "Colore" });
        ui.menu_button(title, |ui| {
            if let (Some(n), Some(scene)) = (selection, &app.scene_graph) {
                crate::ui::color_menu::color_menu(ui, scene, n, app.settings.default_color, &mut color_actions);
            }
        })
        .response
        .on_hover_text("Colore dell'elemento selezionato (vale per tutto il suo contenuto)");
    });

    group_separator(ui);

    // ── Scena ─────────────────────────────────────────────────────────────────
    if tool(ui, has_scene, compact, icon::EYE, "Mostra tutto", "Mostra tutte le parti nascoste") {
        action = Some(Action::ShowAll);
    }
    let mut settings = app.settings.clone();
    let title = icon_text(ui, icon::SLIDERS_HORIZONTAL, if compact { "" } else { "Vista" });
    ui.menu_button(title, |ui| view_menu(ui, &mut settings, has_scene, &mut action))
        .response
        .on_hover_text("Sfondo, rotazione, viste standard e colori generali");

    // ── A destra: stato del documento e tema ──────────────────────────────────
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let (sun_or_moon, next, hint) = match settings.theme {
            ThemeMode::Dark => (icon::SUN, ThemeMode::Light, "Passa al tema chiaro"),
            ThemeMode::Light => (icon::MOON, ThemeMode::Dark, "Passa al tema scuro"),
        };
        if ui.button(icon_text(ui, sun_or_moon, "")).on_hover_text(hint).clicked() {
            settings.theme = next;
            theme::apply(ui.ctx(), next);
            ui.ctx().request_repaint();
        }
        if app.is_dirty() {
            let color = theme::warning_color(ui);
            ui.label(egui::RichText::new("Modifiche non esportate").small().color(color));
        }
    });

    app.apply_settings(settings);
    if let Some(a) = action {
        app.apply(a);
    }
    let selected = app.ui_state.selected_nodes();
    for a in crate::ui::color_menu::spread_colors(color_actions, &selected) {
        app.apply(a);
    }
}

/// Contenuto del menu Vista.
fn view_menu(
    ui: &mut egui::Ui,
    settings: &mut crate::settings::Settings,
    has_scene: bool,
    action: &mut Option<Action>,
) {
    ui.label(egui::RichText::new("Sfondo").strong());
    ui.horizontal(|ui| {
        const PRESETS: &[([f32; 3], &str)] = &[
            ([0.38, 0.40, 0.44], "Grigio (predefinito)"),
            ([0.12, 0.13, 0.15], "Scuro"),
            ([1.0, 1.0, 1.0], "Bianco"),
            ([0.80, 0.85, 0.92], "Azzurro chiaro"),
            ([0.0, 0.0, 0.0], "Nero"),
        ];
        for (c, name) in PRESETS {
            let col = crate::ui::color_menu::to_color32([c[0], c[1], c[2], 1.0]);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, col);
            ui.painter().rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
                egui::StrokeKind::Inside,
            );
            if resp.on_hover_text(*name).clicked() {
                settings.background = *c;
            }
        }
    });
    let [r, g, b] = settings.background;
    let mut bg = crate::ui::color_menu::to_color32([r, g, b, 1.0]);
    if egui::color_picker::color_picker_color32(ui, &mut bg, egui::color_picker::Alpha::Opaque) {
        let c = crate::ui::color_menu::from_color32(bg, 1.0);
        settings.background = [c[0], c[1], c[2]];
    }
    ui.checkbox(&mut settings.background_gradient, "Sfumato")
        .on_hover_text("Più chiaro in alto e più scuro in basso, attorno al colore scelto");

    ui.separator();
    ui.label(egui::RichText::new("Rotazione").strong());
    ui.horizontal(|ui| {
        let before = settings.rotation;
        ui.selectable_value(&mut settings.rotation, RotationMode::Trackball, "OpenInventor")
            .on_hover_text("Trackball come in FreeCAD: vicino al centro della finestra il modello segue il mouse, verso i bordi ruota attorno all'asse di vista");
        ui.selectable_value(&mut settings.rotation, RotationMode::FixedAxis, "Asse fisso")
            .on_hover_text("L'orizzonte resta dritto: gira attorno all'asse verticale del modello");
        if before != settings.rotation && settings.rotation == RotationMode::FixedAxis {
            *action = Some(Action::Straighten);
        }
    });
    ui.spacing_mut().slider_width = 120.0;
    ui.add(
        egui::Slider::new(&mut settings.rotation_sensitivity, crate::settings::SENSITIVITY_RANGE)
            .text("Sensibilità")
            .fixed_decimals(1),
    )
    .on_hover_text("Velocità della rotazione con il mouse (1 = normale)");

    ui.separator();
    ui.label(egui::RichText::new("Viste standard").strong());
    ui.horizontal(|ui| {
        ui.label("Assi del modello:");
        for (axes, name, help) in crate::settings::Axes::ALL {
            ui.selectable_value(&mut settings.axes, axes, name).on_hover_text(help);
        }
    });
    egui::Grid::new("viste_standard").num_columns(2).show(ui, |ui| {
        for (i, (v, name, key)) in crate::renderer::camera::StandardView::ALL.iter().enumerate() {
            if ui
                .add_enabled(has_scene, egui::Button::new(*name).min_size(egui::vec2(110.0, 0.0)))
                .on_hover_text(format!("[{key}]"))
                .clicked()
            {
                *action = Some(Action::View(*v));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
        if ui
            .add_enabled(has_scene, egui::Button::new("Raddrizza").min_size(egui::vec2(110.0, 0.0)))
            .on_hover_text("Annulla il rollio mantenendo la direzione di vista  [5]")
            .clicked()
        {
            *action = Some(Action::Straighten);
        }
    });

    ui.separator();
    ui.label(egui::RichText::new("Colori").strong());
    let mut default_color = crate::ui::color_menu::to_color32(settings.default_color);
    ui.horizontal(|ui| {
        if ui.color_edit_button_srgba(&mut default_color).changed() {
            settings.default_color = crate::ui::color_menu::from_color32(default_color, 1.0);
        }
        ui.label("Parti senza materiale");
        if ui.button("Predefinito").clicked() {
            settings.default_color = crate::renderer::DEFAULT_MESH_COLOR;
        }
    });
    if ui
        .add_enabled(has_scene, egui::Button::new(icon_text(ui, icon::ARROW_COUNTER_CLOCKWISE, "Ripristina tutti i colori originali")))
        .clicked()
    {
        *action = Some(Action::ResetAllColors);
        ui.close_menu();
    }
    ui.separator();
    ui.label(egui::RichText::new("Raggruppamento automatico").strong());
    ui.label(
        egui::RichText::new(
            "All'apertura, i pezzi con abbastanza vertici in comune (entro la tolleranza, nelle unità del file) \
             diventano un componente.",
        )
        .small()
        .weak(),
    );
    egui::Grid::new("raggruppamento").num_columns(2).show(ui, |ui| {
        ui.label("Tolleranza");
        ui.add(
            egui::Slider::new(&mut settings.cluster.tolerance, crate::settings::TOLERANCE_RANGE)
                .logarithmic(true)
                .max_decimals(4),
        )
        .on_hover_text("Distanza entro cui due vertici di pezzi diversi contano come coincidenti (predefinito 0,01)");
        ui.end_row();
        ui.label("Vertici in comune");
        ui.add(egui::Slider::new(&mut settings.cluster.min_matching_vertices, crate::settings::MIN_VERTICES_RANGE).logarithmic(true))
            .on_hover_text("Quanti vertici coincidenti servono per unire due pezzi (predefinito 5)");
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(has_scene, egui::Button::new("Ricarica i file aperti"))
            .on_hover_text("Riapre i file con questi parametri (chiede conferma se ci sono modifiche non esportate)")
            .clicked()
        {
            *action = Some(Action::ReloadFiles);
            ui.close_menu();
        }
        if ui.button("Predefiniti").clicked() {
            settings.cluster = Default::default();
        }
    });

    ui.separator();
    if ui.button("Sfondo e colori predefiniti").clicked() {
        let d = crate::settings::Settings::default();
        settings.background = d.background;
        settings.background_gradient = d.background_gradient;
        settings.default_color = d.default_color;
    }
}
