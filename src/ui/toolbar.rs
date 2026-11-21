use super::theme::{self, icon, icon_text};
use crate::i18n::tr;
use crate::app::Action;
use crate::settings::{RotationMode, ThemeMode};

/// Larghezza della barra sotto cui i pulsanti mostrano solo l'icona, finché non si è misurata
/// quella vera (con le scritte).
const FULL_LABELS_WIDTH: f32 = 1500.0;

/// Dove si ricorda la larghezza misurata della barra con le scritte.
fn full_width_id() -> egui::Id {
    egui::Id::new("toolbar_larghezza_con_scritte")
}

/// Avviso a destra nella toolbar.
const DIRTY: &str = tn!("Modifiche non esportate");

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
    // Con le scritte la barra deve starci tutta, compresi i pulsanti a destra: altrimenti si
    // sovrappongono. La larghezza necessaria si misura quando le scritte ci sono
    let needed: f32 = ui.ctx().data(|d| d.get_temp(full_width_id())).unwrap_or(FULL_LABELS_WIDTH);
    let compact = ui.max_rect().width() < needed;

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
    if tool(ui, !loading, compact, icon::FOLDER_OPEN, tr("Apri"), tr("Apri un file WRL, STEP, JT, STL, HSF o IV (puoi anche trascinarlo sulla finestra)  [Ctrl+O]")) {
        app.open_dialog();
    }
    let recent = app.settings.recent.clone();
    let mut open_path = None;
    ui.add_enabled_ui(!loading && !recent.is_empty(), |ui| {
        ui.menu_button(icon_text(ui, icon::CLOCK_COUNTER_CLOCKWISE, ""), |ui| {
            ui.label(egui::RichText::new(tr("File recenti")).strong());
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
        .on_hover_text(tr("File recenti"));
    });
    if let Some(p) = open_path {
        app.request_open(p);
    }
    // Progetto: i modelli aperti e le modifiche della scena (posizioni, colori, nomi…)
    let mut project_action: Option<u8> = None;
    ui.add_enabled_ui(!loading, |ui| {
        let text = icon_text(ui, icon::FLOPPY_DISK, if compact { "" } else { tr("Progetto") });
        ui.menu_button(text, |ui| {
            if ui.button(tr("Apri progetto…")).on_hover_text(tr("Riapre i modelli di un progetto .dima3d e le sue modifiche")).clicked() {
                project_action = Some(0);
                ui.close_menu();
            }
            if ui.add_enabled(has_scene, egui::Button::new(tr("Salva progetto")).shortcut_text("Ctrl+S")).clicked() {
                project_action = Some(1);
                ui.close_menu();
            }
            if ui.add_enabled(has_scene, egui::Button::new(tr("Salva progetto con nome…")).shortcut_text(tr("Ctrl+Maiusc+S"))).clicked() {
                project_action = Some(2);
                ui.close_menu();
            }
            ui.separator();
            ui.label(
                egui::RichText::new(tr(
                    "Il progetto (.dima3d) ricorda quali modelli sono aperti e le modifiche: posizioni, \
                     colori, nomi, pezzi nascosti o eliminati e la vista. La geometria non c'è: all'apertura \
                     si rileggono i modelli originali, quindi non vanno spostati o modificati (se mancano, \
                     Dima 3D chiede dove sono). La riduzione delle facce non viene salvata.",
                ))
                .small()
                .weak(),
            );
        })
        .response
        .on_hover_text(tr("Progetto: salva e riapri la scena con le modifiche  [Ctrl+S]"));
    });
    match project_action {
        Some(0) => app.open_project_dialog(),
        Some(1) => {
            app.save_project();
        }
        Some(2) => {
            app.save_project_as();
        }
        _ => {}
    }
    if tool(ui, has_scene && !loading, compact, icon::FILE_PLUS, tr("Aggiungi"), tr("Aggiunge uno o più file (WRL, STEP, JT, STL, HSF, IV) alla scena aperta, anche trascinandoli sulla finestra")) {
        app.add_dialog();
    }
    if tool(ui, has_scene, compact, icon::EXPORT, tr("Esporta"), tr("Salva solo le parti visibili in un nuovo file WRL, STL, IV o HSF  [Ctrl+E]\nSolo la selezione: Ctrl+Maiusc+E o il tasto destro")) {
        app.export_dialog(None);
    }
    let mut export_settings = app.settings.clone();
    let selected = app.ui_state.selected_nodes();
    let mut export_selection = false;
    ui.menu_button(icon_text(ui, icon::CARET_DOWN, ""), |ui| {
        let button = egui::Button::new(icon_text(ui, icon::EXPORT, tr("Esporta selezionati…    Ctrl+Maiusc+E")));
        if ui
            .add_enabled(has_scene && !selected.is_empty(), button)
            .on_hover_text(tr("Salva solo le parti visibili degli elementi selezionati, con i gruppi che li contengono"))
            .on_disabled_hover_text(tr("Seleziona prima uno o più elementi nell'albero o nella vista"))
            .clicked()
        {
            export_selection = true;
            ui.close_menu();
        }
        ui.separator();
        ui.label(egui::RichText::new(tr("Formato dell'export")).strong());
        ui.checkbox(&mut export_settings.export_compact, tr("Formato compatto")).on_hover_text(tr(
            "Niente rientri e numeri con la precisione che serve (un milionesimo dell'ingombro): il file è meno \
             della metà. Senza, si copia il testo del file originale.",
        ));
        ui.checkbox(&mut export_settings.export_normals, tr("Scrivi le normali")).on_hover_text(tr(
            "Con le normali gli spigoli vivi restano quelli del file. Senza, il visualizzatore le ricalcola \
             (creaseAngle 0,5) e il file è circa la metà.",
        ));
        ui.separator();
        ui.label(egui::RichText::new(tr("STL (scegli .stl nella finestra Salva)")).strong());
        ui.checkbox(&mut export_settings.export_stl_ascii, tr("In formato testo (ASCII)")).on_hover_text(tr(
            "Il testo tiene un nome per pezzo ma è circa 5 volte più grande. Il binario è letto da tutti i programmi. \
             In entrambi i casi colori e albero non esistono nel formato STL.",
        ));
        ui.separator();
        ui.label(egui::RichText::new(tr("Open Inventor (scegli .iv nella finestra Salva)")).strong());
        ui.checkbox(&mut export_settings.export_iv_binary, tr("In formato binario")).on_hover_text(tr(
            "Il binario è circa un terzo del testo e si apre molte volte più in fretta (FreeCAD, Coin3D). \
             Il testo si può leggere e modificare con un editor. Pezzi, nomi e colori restano in entrambi; \
             le normali seguono l'opzione «Scrivi le normali».",
        ));
        ui.checkbox(&mut export_settings.export_iv_meters, tr("In metri (non scalare)")).on_hover_text(tr(
            "Di solito l'IV si scrive in millimetri: la stazione v3 all'apertura chiede di scalarlo. \
             Con questa opzione si scrive in metri, come i WRL e gli HSF.",
        ));
        ui.separator();
        ui.label(egui::RichText::new(tr("Unità")).strong());
        ui.label(
            egui::RichText::new(tr(
                "WRL e HSF si scrivono in metri, IV in millimetri (salvo «In metri»), STL in millimetri.",
            ))
            .weak(),
        );
        ui.separator();
        ui.label(egui::RichText::new(tr("HSF (scegli .hsf nella finestra Salva)")).strong());
        ui.label(
            egui::RichText::new(tr(
                "Le normali seguono l'opzione «Scrivi le normali». Con le normali i pezzi hanno gli spigoli \
                 vivi calcolati qui (vertici sdoppiati sugli spigoli) e il file è più grande. Senza, il \
                 visualizzatore le ricalcola sui vertici uniti: il file è più piccolo ma l'ombreggiatura \
                 può sfumare sugli spigoli.",
            ))
            .weak(),
        );
    })
    .response
    .on_hover_text(tr("Opzioni dell'export ed export della selezione"));
    app.apply_settings(export_settings);
    if export_selection {
        app.export_selection_dialog(selected);
    }
    let reduce_open = app.project.reduction.panel.open;
    let reduce_text = icon_text(ui, icon::TRIANGLE_DASHED, if compact { "" } else { tr("Riduci") });
    let reduce = ui
        .add_enabled(has_scene && !loading, egui::Button::new(reduce_text).selected(reduce_open))
        .on_hover_text(tr("Riduci facce\nToglie triangoli dove non servono (come MeshLab), con anteprima e dimensione prevista del file"));
    if reduce.clicked() {
        app.project.reduction.panel.open = !reduce_open;
    }
    let position_open = app.ui_state.position.open;
    let position_text = icon_text(ui, icon::ARROWS_OUT_CARDINAL, if compact { "" } else { tr("Posizione") });
    let position = ui
        .add_enabled(has_scene && !loading, egui::Button::new(position_text).selected(position_open))
        .on_hover_text(tr("Posizione\nSposta e ruota un file o i pezzi selezionati (X Y Z, A B C), applica e calcola gli offset"));
    if position.clicked() {
        app.ui_state.position.open = !position_open;
    }
    let cabin_open = app.ui_state.cabin_open;
    let cabin_text = icon_text(ui, icon::CUBE_TRANSPARENT, if compact { "" } else { tr("Cabina") });
    let cabin = ui
        .add_enabled(!loading, egui::Button::new(cabin_text).selected(cabin_open))
        .on_hover_text(tr("Cabina\nCostruisce la cabina di verniciatura dalle misure: grigliato, pareti, testate con le aperture, soffitto"));
    if cabin.clicked() {
        app.ui_state.cabin_open = !cabin_open;
    }
    let robot_open = app.ui_state.robot_open;
    let robot_text = icon_text(ui, icon::ROBOT, if compact { "" } else { tr("Robot") });
    let robot = ui
        .add_enabled(!loading, egui::Button::new(robot_text).selected(robot_open))
        .on_hover_text(tr("Robot\nRobot generici con le coordinate del controllore (X Y Z A B C dal centro cabina), con il pannello a quota 0"));
    if robot.clicked() {
        app.ui_state.robot_open = !robot_open;
    }
    let program_open = app.ui_state.program.open;
    let program_text = icon_text(ui, icon::PATH, if compact { "" } else { tr("Programma") });
    let program = ui
        .add_enabled(!loading, egui::Button::new(program_text).selected(program_open))
        .on_hover_text(tr("Programma del robot\nApre un programma (.TIS / .tip) e disegna la traiettoria nella vista; righe da copiare, tagliare, incollare, invertire e trascinare"));
    if program.clicked() {
        app.ui_state.program.open = !program_open;
    }
    // Sempre attivo: il layout PDF non dipende dalla scena 3D
    let pdf_open = app.ui_state.pdf.open;
    let pdf_text = icon_text(ui, icon::FILE_PDF, if compact { "" } else { tr("Misure PDF") });
    let pdf = ui
        .add(egui::Button::new(pdf_text).selected(pdf_open))
        .on_hover_text(tr("Misure PDF\nApre un layout in PDF, calibra la scala su una distanza nota e misura Delta X, Delta Y e distanza in mm"));
    if pdf.clicked() {
        app.ui_state.pdf.open = !pdf_open;
    }
    if tool(ui, has_scene && !loading, compact, icon::X, tr("Chiudi"), tr("Chiude tutti i file aperti (chiede conferma se ci sono modifiche non esportate). Per chiuderne uno solo: tasto destro sul file nell'albero")) {
        app.request_close();
    }

    group_separator(ui);

    // ── Modifica ──────────────────────────────────────────────────────────────
    let (undo_steps, redo_steps) = (app.project.undo.len(), app.project.redo.len());
    let undo_hint = trf!("Annulla  [Ctrl+Z]  ({n} passi)", n = undo_steps);
    if tool(ui, has_scene && undo_steps > 0, true, icon::ARROW_U_UP_LEFT, tr("Annulla"), &undo_hint) {
        app.undo();
    }
    let redo_hint = trf!("Ripeti  [Ctrl+Y]  ({n} passi)", n = redo_steps);
    if tool(ui, has_scene && redo_steps > 0, true, icon::ARROW_U_UP_RIGHT, tr("Ripeti"), &redo_hint) {
        app.redo();
    }

    group_separator(ui);

    // ── Selezione ─────────────────────────────────────────────────────────────
    if tool(ui, has_selection, compact, icon::CUBE_FOCUS, tr("Isola"), tr("Mostra solo l'elemento selezionato  [I]")) {
        action = app.ui_state.isolate_action();
    }
    let (eye, label, hint) = if selection_visible || !has_selection {
        (icon::EYE_SLASH, tr("Nascondi"), tr("Nascondi l'elemento selezionato  [H]"))
    } else {
        (icon::EYE, tr("Mostra"), tr("Mostra l'elemento selezionato  [H]"))
    };
    if tool(ui, has_selection, compact, eye, label, hint) {
        action = app.toggle_selection_action();
    }
    if tool(ui, has_selection, compact, icon::TRASH, tr("Elimina"), tr("Rimuove l'elemento dalla scena e dall'export  [Canc]")) {
        action = app.ui_state.on_selection(Action::Delete);
    }
    let mut color_actions: Vec<Action> = Vec::new();
    ui.add_enabled_ui(has_selection, |ui| {
        let title = icon_text(ui, icon::PALETTE, if compact { "" } else { tr("Colore") });
        ui.menu_button(title, |ui| {
            if let (Some(n), Some(scene)) = (selection, &app.scene_graph) {
                crate::ui::color_menu::color_menu(
                    ui,
                    scene,
                    n,
                    app.settings.default_color,
                    &app.settings.saved_colors,
                    &mut color_actions,
                );
            }
        })
        .response
        .on_hover_text(tr("Colore dell'elemento selezionato (vale per tutto il suo contenuto)"));
    });
    let picking = app.ui_state.eyedropper;
    let pipette = ui
        .add_enabled(has_scene, egui::Button::new(icon_text(ui, icon::EYEDROPPER, "")).selected(picking))
        .on_hover_text(tr(
            "Pipetta: clic su un pezzo nella vista per leggerne il colore e darlo alla selezione. \
             Senza selezione il colore va tra i miei colori (menu Colore).  [Esc per annullare]",
        ));
    if pipette.clicked() {
        app.ui_state.eyedropper = !picking;
        if !picking {
            app.ui_state.measure = None;
            app.ui_state.status_message = if has_selection {
                tr("Pipetta: clic sul pezzo da cui leggere il colore").to_string()
            } else {
                tr("Pipetta: clic su un pezzo per salvarne il colore tra i miei colori").to_string()
            };
        }
    }
    let random_hint = tr("Colori casuali\nUn colore diverso per ogni pezzo selezionato; senza selezione, per tutti i pezzi \
                       della scena. Ogni clic dà nuovi colori (Ctrl+Z per tornare indietro)");
    if tool(ui, has_scene, true, icon::DICE_FIVE, tr("Colori casuali"), random_hint) {
        if let Some(scene) = &app.scene_graph {
            action = crate::ui::color_menu::random_colors_action(scene, &app.ui_state.selected_nodes());
        }
    }

    group_separator(ui);

    // ── Scena ─────────────────────────────────────────────────────────────────
    if tool(ui, has_scene, compact, icon::EYE, tr("Mostra tutto"), tr("Mostra tutte le parti nascoste")) {
        action = Some(Action::ShowAll);
    }
    let mut settings = app.settings.clone();
    let title = icon_text(ui, icon::SLIDERS_HORIZONTAL, if compact { "" } else { tr("Vista") });
    ui.menu_button(title, |ui| view_menu(ui, &mut settings, has_scene, &mut action))
        .response
        .on_hover_text(tr("Sfondo, rotazione, viste standard e colori generali"));

    // ── A destra: stato del documento e tema ──────────────────────────────────
    let left_end = ui.cursor().min.x;
    let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(icon_text(ui, icon::INFO, "")).on_hover_text(trf!("Info su {app}", app = crate::APP_NAME)).clicked() {
            app.ui_state.about = !app.ui_state.about;
        }
        let (sun_or_moon, next, hint) = match settings.theme {
            ThemeMode::Dark => (icon::SUN, ThemeMode::Light, tr("Passa al tema chiaro")),
            ThemeMode::Light => (icon::MOON, ThemeMode::Dark, tr("Passa al tema scuro")),
        };
        if ui.button(icon_text(ui, sun_or_moon, "")).on_hover_text(hint).clicked() {
            settings.theme = next;
            theme::apply(ui.ctx(), next);
            ui.ctx().request_repaint();
        }
        language_menu(ui, &mut settings.language);
        if app.is_dirty() {
            // Con la finestra stretta la scritta finirebbe sopra i pulsanti: resta l'icona
            let color = theme::warning_color(ui);
            let text = egui::RichText::new(tr(DIRTY)).small().color(color);
            let width = egui::WidgetText::from(text.clone())
                .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Small)
                .size()
                .x;
            let icon_width = 2.0 * ui.spacing().interact_size.y;
            if ui.available_width() >= width + ui.spacing().item_spacing.x {
                ui.label(text);
            } else if ui.available_width() >= icon_width {
                ui.label(egui::RichText::new(icon::WARNING_CIRCLE).color(color)).on_hover_text(tr(DIRTY));
            }
        }
        ui.min_rect().width()
    });
    if !compact {
        // Pulsanti di destra (senza l'avviso, che va e viene) + margine
        let needed = left_end - ui.max_rect().left() + right.inner.min(180.0) + 24.0;
        ui.ctx().data_mut(|d| d.insert_temp(full_width_id(), needed));
    }

    app.apply_settings(settings);
    if let Some(a) = action {
        app.apply(a);
    }
    let selected = app.ui_state.selected_nodes();
    for a in crate::ui::color_menu::spread_colors(color_actions, &selected) {
        app.apply(a);
    }
}

/// Suggerimento del pulsante della lingua: in due lingue, così lo trova anche chi non legge quella in uso.
const LANGUAGE_HINT: &str = "Lingua · Language";

/// Pulsante della lingua nella barra, accanto al tema: bandierina e sigla, sempre (anche nella
/// barra compatta); il menu elenca le lingue con il loro nome.
fn language_menu(ui: &mut egui::Ui, language: &mut crate::i18n::Lang) {
    let flag = super::flags::flag(ui.ctx(), *language);
    let button = egui::Button::image_and_text(flag, language.short());
    egui::menu::menu_custom_button(ui, button, |ui| {
        for lang in crate::i18n::Lang::ALL {
            let item = egui::Button::image_and_text(super::flags::flag(ui.ctx(), lang), lang.name()).selected(lang == *language);
            if ui.add(item).clicked() {
                *language = lang;
                ui.close_menu();
            }
        }
    })
    .response
    .on_hover_text(LANGUAGE_HINT);
}

/// Contenuto del menu Vista.
fn view_menu(
    ui: &mut egui::Ui,
    settings: &mut crate::settings::Settings,
    has_scene: bool,
    action: &mut Option<Action>,
) {
    ui.label(egui::RichText::new(tr("Sfondo")).strong());
    ui.horizontal(|ui| {
        const PRESETS: &[([f32; 3], &str)] = &[
            ([0.38, 0.40, 0.44], tn!("Grigio (predefinito)")),
            ([0.12, 0.13, 0.15], tn!("Scuro")),
            ([1.0, 1.0, 1.0], tn!("Bianco")),
            ([0.80, 0.85, 0.92], tn!("Azzurro chiaro")),
            ([0.0, 0.0, 0.0], tn!("Nero")),
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
            if resp.on_hover_text(tr(name)).clicked() {
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
    ui.checkbox(&mut settings.background_gradient, tr("Sfumato"))
        .on_hover_text(tr("Più chiaro in alto e più scuro in basso, attorno al colore scelto"));

    ui.separator();
    ui.label(egui::RichText::new(tr("Rotazione")).strong());
    ui.horizontal(|ui| {
        let before = settings.rotation;
        ui.selectable_value(&mut settings.rotation, RotationMode::Trackball, tr("OpenInventor"))
            .on_hover_text(tr("Trackball come in FreeCAD: vicino al centro della finestra il modello segue il mouse, verso i bordi ruota attorno all'asse di vista"));
        ui.selectable_value(&mut settings.rotation, RotationMode::FixedAxis, tr("Asse fisso"))
            .on_hover_text(tr("L'orizzonte resta dritto: gira attorno all'asse verticale del modello"));
        if before != settings.rotation && settings.rotation == RotationMode::FixedAxis {
            *action = Some(Action::Straighten);
        }
    });
    ui.spacing_mut().slider_width = 120.0;
    ui.add(
        egui::Slider::new(&mut settings.rotation_sensitivity, crate::settings::SENSITIVITY_RANGE)
            .text(tr("Sensibilità"))
            .fixed_decimals(1),
    )
    .on_hover_text(tr("Velocità della rotazione con il mouse (1 = normale)"));
    ui.checkbox(&mut settings.invert_zoom, tr("Zoom invertito"))
        .on_hover_text(tr("Rotella in avanti per allontanarsi invece di avvicinarsi (anche dal tasto Zoom nella vista)"));

    ui.separator();
    ui.label(egui::RichText::new(tr("Aggiornamenti")).strong());
    ui.checkbox(&mut settings.check_updates, tr("Verifica all'avvio")).on_hover_text(tr(
        "All'avvio chiede a GitHub se c'è una versione nuova e, se c'è, propone di aggiornare. \
         Si può verificare anche a mano dalla finestra Info.",
    ));

    ui.separator();
    ui.label(egui::RichText::new(tr("Viste standard")).strong());
    ui.horizontal(|ui| {
        ui.label(tr("Assi del modello:"));
        for (axes, name, help) in crate::settings::Axes::ALL {
            ui.selectable_value(&mut settings.axes, axes, tr(name)).on_hover_text(tr(help));
        }
    });
    egui::Grid::new("viste_standard").num_columns(2).show(ui, |ui| {
        for (i, (v, name, key)) in crate::renderer::camera::StandardView::ALL.iter().enumerate() {
            if ui
                .add_enabled(has_scene, egui::Button::new(tr(name)).min_size(egui::vec2(110.0, 0.0)))
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
            .add_enabled(has_scene, egui::Button::new(tr("Raddrizza")).min_size(egui::vec2(110.0, 0.0)))
            .on_hover_text(tr("Annulla il rollio mantenendo la direzione di vista  [5]"))
            .clicked()
        {
            *action = Some(Action::Straighten);
        }
    });

    ui.separator();
    ui.label(egui::RichText::new(tr("Colori")).strong());
    let mut default_color = crate::ui::color_menu::to_color32(settings.default_color);
    ui.horizontal(|ui| {
        if ui.color_edit_button_srgba(&mut default_color).changed() {
            settings.default_color = crate::ui::color_menu::from_color32(default_color, 1.0);
        }
        ui.label(tr("Parti senza materiale"));
        if ui.button(tr("Predefinito")).clicked() {
            settings.default_color = crate::renderer::DEFAULT_MESH_COLOR;
        }
    });
    if ui
        .add_enabled(has_scene, egui::Button::new(icon_text(ui, icon::ARROW_COUNTER_CLOCKWISE, tr("Ripristina tutti i colori originali"))))
        .clicked()
    {
        *action = Some(Action::ResetAllColors);
        ui.close_menu();
    }
    ui.separator();
    ui.label(egui::RichText::new(tr("Raggruppamento automatico")).strong());
    ui.label(
        egui::RichText::new(tr(
            "All'apertura, i pezzi con abbastanza vertici in comune (entro la tolleranza, nelle unità del file) \
             diventano un componente.",
        ))
        .small()
        .weak(),
    );
    egui::Grid::new("raggruppamento").num_columns(2).show(ui, |ui| {
        ui.label(tr("Tolleranza"));
        ui.add(
            egui::Slider::new(&mut settings.cluster.tolerance, crate::settings::TOLERANCE_RANGE)
                .logarithmic(true)
                .max_decimals(4),
        )
        .on_hover_text(tr("Distanza entro cui due vertici di pezzi diversi contano come coincidenti (predefinito 0,01)"));
        ui.end_row();
        ui.label(tr("Vertici in comune"));
        ui.add(egui::Slider::new(&mut settings.cluster.min_matching_vertices, crate::settings::MIN_VERTICES_RANGE).logarithmic(true))
            .on_hover_text(tr("Quanti vertici coincidenti servono per unire due pezzi (predefinito 5)"));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(has_scene, egui::Button::new(tr("Ricarica i file aperti")))
            .on_hover_text(tr("Riapre i file con questi parametri (chiede conferma se ci sono modifiche non esportate)"))
            .clicked()
        {
            *action = Some(Action::ReloadFiles);
            ui.close_menu();
        }
        if ui.button(tr("Predefiniti")).clicked() {
            settings.cluster = Default::default();
        }
    });

    ui.separator();
    if ui.button(tr("Sfondo e colori predefiniti")).clicked() {
        let d = crate::settings::Settings::default();
        settings.background = d.background;
        settings.background_gradient = d.background_gradient;
        settings.default_color = d.default_color;
    }
}

#[cfg(test)]
mod tests {
    use crate::i18n::Lang;

    /// Un fotogramma del programma; restituisce i testi disegnati con il loro rettangolo.
    fn frame(ctx: &egui::Context, app: &mut crate::app::GestoreApp, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        frame_width(ctx, app, events, 2000.0)
    }

    /// `width` sotto i 1500 punti: all'avvio la barra è compatta (solo icone).
    fn frame_width(ctx: &egui::Context, app: &mut crate::app::GestoreApp, events: Vec<egui::Event>, width: f32) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0));
        let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
            crate::ui::draw_main_layout(ctx, app);
        });
        fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
        texts
    }

    fn click(ctx: &egui::Context, app: &mut crate::app::GestoreApp, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![], vec![]] {
            frame(ctx, app, events);
        }
    }

    /// La lingua si sceglie dalla barra principale: bandierina con «IT», menu con i nomi delle
    /// lingue; scelto «English» la barra passa in inglese.
    #[test]
    fn language_button_in_the_toolbar() {
        crate::i18n::set(Lang::It);
        let mut app = crate::app::GestoreApp::new_headless();
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        let texts = frame(&ctx, &mut app, vec![]);
        let find = |texts: &[(String, egui::Rect)], t: &str| texts.iter().find(|(s, _)| s == t).map(|x| x.1);
        let it = find(&texts, "IT").expect("pulsante della lingua");
        assert!(it.top() < 45.0, "nella barra in alto: {it:?}");
        assert!(texts.iter().any(|(s, _)| s.ends_with("Apri")), "barra in italiano");

        click(&ctx, &mut app, it.center());
        let texts = frame(&ctx, &mut app, vec![]);
        assert!(find(&texts, "Italiano").is_some(), "il menu elenca le lingue");
        let english = find(&texts, "English").expect("voce English");
        click(&ctx, &mut app, english.center());

        assert_eq!(app.settings.language, Lang::En);
        let texts = frame(&ctx, &mut app, vec![]);
        assert!(find(&texts, "EN").is_some(), "la sigla diventa EN");
        assert!(texts.iter().any(|(s, _)| s.ends_with("Open")), "barra in inglese");
        crate::i18n::set(Lang::It);
    }

    /// Anche nella barra compatta (solo icone) la sigla resta accanto alla bandierina, a sinistra
    /// del pulsante del tema.
    #[test]
    fn language_code_stays_in_the_compact_toolbar() {
        crate::i18n::set(Lang::It);
        let mut app = crate::app::GestoreApp::new_headless();
        let ctx = egui::Context::default();
        frame_width(&ctx, &mut app, vec![], 1280.0);
        let texts = frame_width(&ctx, &mut app, vec![], 1280.0);
        assert!(!texts.iter().any(|(s, _)| s.ends_with("Apri")), "barra compatta");
        let it = texts.iter().find(|(s, _)| s == "IT").expect("sigla IT").1;
        let theme = texts.iter().find(|(s, _)| s == super::icon::SUN).expect("pulsante del tema").1;
        assert!(it.right() < theme.left() && theme.left() - it.right() < 40.0, "accanto al tema: {it:?} {theme:?}");
    }
}
