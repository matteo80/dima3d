//! Finestra «Cabina»: misure della cabina di verniciatura e costruzione (vedi `crate::cabin`).

use super::theme::{icon, icon_text};
use crate::i18n::tr;
use crate::app::GestoreApp;
use crate::cabin::CabinParams;

/// Campo in millimetri con etichetta e spiegazione.
fn mm(ui: &mut egui::Ui, label: &str, help: &str, value: &mut f32) {
    ui.label(label).on_hover_text(help);
    ui.add(egui::DragValue::new(value).speed(10.0).range(0.0..=1.0e6).suffix(" mm").max_decimals(1))
        .on_hover_text(help);
    ui.end_row();
}

/// Come [`mm`], anche negativo (spostamenti).
fn mm_signed(ui: &mut egui::Ui, label: &str, help: &str, value: &mut f32) {
    ui.label(label).on_hover_text(help);
    ui.add(egui::DragValue::new(value).speed(10.0).range(-1.0e6..=1.0e6).suffix(" mm").max_decimals(1))
        .on_hover_text(help);
    ui.end_row();
}

/// Testo, font, misure, colore e posizione della scritta sulle pareti.
fn label_section(ui: &mut egui::Ui, label: &mut crate::cabin::CabinLabel) {
    use crate::cabin::LabelWall;
    egui::Grid::new("cabina_scritta").num_columns(2).show(ui, |ui| {
        ui.label(tr("Testo"));
        ui.add(egui::TextEdit::singleline(&mut label.text).hint_text(tr("es. il nome della cabina")).desired_width(200.0));
        ui.end_row();

        ui.label(tr("Font"));
        let fonts = crate::cabin::common_fonts();
        let current = std::path::Path::new(&label.font);
        let shown = fonts
            .iter()
            .find(|(_, p)| p.as_path() == current)
            .map(|(n, _)| (*n).to_string())
            .unwrap_or_else(|| current.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        egui::ComboBox::from_id_salt("cabina_font").selected_text(shown).width(200.0).show_ui(ui, |ui| {
            for (name, path) in &fonts {
                let text = path.to_string_lossy().into_owned();
                ui.selectable_value(&mut label.font, text, *name);
            }
            ui.separator();
            if ui.button(tr("Altro file…")).clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter(tr("Font (TTF, OTF)"), &["ttf", "TTF", "otf", "OTF", "ttc", "TTC"])
                    .set_directory(crate::cabin::fonts_dir())
                    .pick_file();
                if let Some(path) = picked {
                    label.font = path.to_string_lossy().into_owned();
                }
                ui.close_menu();
            }
        });
        ui.end_row();

        mm(ui, tr("Altezza lettere"), tr("Altezza delle maiuscole"), &mut label.height);
        mm(ui, tr("Rilievo"), tr("Spessore delle lettere, fuori dalla parete"), &mut label.depth);

        ui.label(tr("Colore"));
        ui.color_edit_button_rgb(&mut label.color);
        ui.end_row();

        ui.label(tr("Parete"));
        let wall_name = LabelWall::ALL.iter().find(|(w, _)| *w == label.wall).map_or("", |(_, n)| tr(n));
        egui::ComboBox::from_id_salt("cabina_parete").selected_text(wall_name).width(200.0).show_ui(ui, |ui| {
            for (wall, name) in LabelWall::ALL {
                ui.selectable_value(&mut label.wall, wall, tr(name));
            }
        });
        ui.end_row();

        ui.label(tr("Lato"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut label.inside, false, tr("Esterno"));
            ui.selectable_value(&mut label.inside, true, tr("Interno"));
        });
        ui.end_row();

        mm_signed(
            ui,
            tr("Spostamento"),
            tr("Dal centro della parete, verso destra per chi legge (0 = centrata)"),
            &mut label.offset,
        );
        mm(ui, tr("Quota"), tr("Altezza della base delle lettere dal grigliato"), &mut label.z);
    });
}

/// Assi della cabina nel suo centro (vista 3D): solo la terna, senza riquadro delle coordinate.
pub fn draw_center(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    center: glam::Vec3,
    m: glam::Mat4,
    size: f32,
    to_screen: impl Fn(glam::Vec3) -> Option<egui::Pos2>,
) {
    let painter = ui.painter_at(rect);
    let Some(origin) = to_screen(center) else { return };
    let length = (size * 0.04).max(500.0);
    for (axis, color, name) in [
        (glam::Vec3::X, egui::Color32::from_rgb(230, 70, 70), "X"),
        (glam::Vec3::Y, egui::Color32::from_rgb(80, 200, 80), "Y"),
        (glam::Vec3::Z, egui::Color32::from_rgb(80, 140, 255), "Z"),
    ] {
        if let Some(end) = to_screen(center + m.transform_vector3(axis).normalize_or_zero() * length) {
            painter.arrow(origin, end - origin, egui::Stroke::new(2.5_f32, color));
            painter.text(end, egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(13.0), color);
        }
    }
    painter.circle(origin, 5.0, egui::Color32::WHITE, egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
}

/// Passa a un'altra cabina: le misure nella finestra diventano le sue (con «Nuova cabina» restano).
fn choose_cabin(app: &mut GestoreApp, choice: crate::cabin::CabinChoice) {
    app.ui_state.cabin_target = Some(choice);
    if let crate::cabin::CabinChoice::Node(n) = choice {
        if let Some((_, _, params)) = app.cabins().into_iter().find(|c| c.0 == n) {
            let mut settings = app.settings.clone();
            settings.cabin = params;
            app.apply_settings(settings);
        }
    }
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp, pass: super::dock::Pass) {
    if !app.ui_state.cabin_open {
        return;
    }
    // Agganciata si disegna prima della vista, mobile dopo: l'altro passaggio non fa niente
    if (pass == super::dock::Pass::Docked) != (app.settings.dock_cabin.side != super::dock::Side::Floating) {
        return;
    }
    let mut dock = app.settings.dock_cabin;
    // Cabina scelta nell'albero: la finestra passa su di lei e ne prende le misure
    let selected = app.ui_state.selected_node;
    let last_key = egui::Id::new("cabina_ultima_selezione");
    if ctx.data(|d| d.get_temp::<Option<u32>>(last_key)) != Some(selected) {
        ctx.data_mut(|d| d.insert_temp(last_key, selected));
        let picked = selected.and_then(|n| {
            let scene = app.scene_graph.as_ref()?;
            let source = scene.sources.get(scene.source_of_node(n)?)?;
            source.cabin.is_some().then_some(source.node)
        });
        if let Some(node) = picked {
            if app.ui_state.cabin_target != Some(crate::cabin::CabinChoice::Node(node)) {
                choose_cabin(app, crate::cabin::CabinChoice::Node(node));
            }
        }
    }
    let cabins = app.cabins();
    let target = app.cabin_node();
    let mut choice: Option<crate::cabin::CabinChoice> = None;
    let mut p: CabinParams = app.settings.cabin.clone();
    let mut open = true;
    let mut build = false;
    let exists = app.cabin_node().is_some();
    let busy = app.ui_state.is_loading || app.project.busy();
    let current_center = app.center_in_cabin();
    let mut cell = app.settings.robots.clone();
    let mut apply_cell = false;
    let window = super::dock::Dockable {
        id: "finestra_cabina",
        title: format!("{}  {}", icon::CUBE_TRANSPARENT, tr("Cabina")),
        state: &mut dock,
        open: &mut open,
        default_width: 360.0,
        min_width: 300.0,
        default_height: 560.0,
        default_pos: egui::pos2(80.0, 120.0),
        scroll: true,
        fold: true,
    };
    super::dock::show(ctx, pass, window, |ui| {
            // ── Cabina da modificare ──────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr("Cabina")).strong());
                let label = |(_, name, c): &(u32, String, CabinParams)| format!("{name}  ({} × {} mm)", c.length, c.width);
                let current = match target {
                    Some(n) => cabins.iter().find(|c| c.0 == n).map(label).unwrap_or_default(),
                    None => tr("Nuova cabina").to_string(),
                };
                egui::ComboBox::from_id_salt("cabina_oggetto")
                    .width((ui.available_width() - 8.0).max(120.0))
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for c in &cabins {
                            if ui.selectable_label(target == Some(c.0), label(c)).clicked() {
                                choice = Some(crate::cabin::CabinChoice::Node(c.0));
                            }
                        }
                        if ui.selectable_label(target.is_none(), tr("Nuova cabina")).on_hover_text(tr("«Costruisci» aggiunge un'altra cabina senza toccare quelle che ci sono")).clicked() {
                            choice = Some(crate::cabin::CabinChoice::New);
                        }
                    })
                    .response
                    .on_hover_text(tr("Cabina su cui lavorano misure, «Aggiorna» e centro: scegliendola le misure diventano le sue (anche selezionandola nell'albero)"));
            });
            ui.separator();
            ui.label(
                egui::RichText::new(
                    tr("Zero all'ingresso della cabina, sull'asse, al piano del grigliato (della tavola, se il grigliato è \
                     ribassato). X verso l'uscita, Z in alto."),
                )
                .weak(),
            );
            let force = super::dock::fold_request(ui.ctx(), "finestra_cabina");
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                fold(ui, force, tr("Dimensioni"), |ui| {
                    egui::Grid::new("cabina_dimensioni").num_columns(2).show(ui, |ui| {
                        mm(ui, tr("Lunghezza"), tr("Lunghezza interna, dall'ingresso all'uscita (X)"), &mut p.length);
                        mm(ui, tr("Larghezza"), tr("Larghezza interna tra le pareti (Y)"), &mut p.width);
                        mm(ui, tr("Altezza"), tr("Dal grigliato al soffitto"), &mut p.height);
                        mm(ui, tr("Fondo sotto il grigliato"), tr("Fin dove scendono le pareti sotto il grigliato"), &mut p.depth);
                    });
                })
                .fully_open();
                fold(ui, force, tr("Pareti"), |ui| {
                    egui::Grid::new("cabina_pareti").num_columns(2).show(ui, |ui| {
                        mm(ui, tr("Passo dei montanti"), tr("Distanza tra i montanti delle pareti laterali"), &mut p.module);
                        mm(ui, tr("Montanti"), tr("Lato della sezione quadrata dei montanti"), &mut p.post);
                        mm(ui, tr("Spessore pannelli"), tr("Spessore di pannelli, testate e soffitto"), &mut p.panel);
                        ui.label(tr("Fascia vetrata"));
                        ui.checkbox(&mut p.glass, "");
                        ui.end_row();
                        if p.glass {
                            mm(ui, tr("Vetrata da"), tr("Quota inferiore della fascia vetrata, dal grigliato"), &mut p.glass_from);
                            mm(ui, tr("Vetrata a"), tr("Quota superiore della fascia vetrata, dal grigliato"), &mut p.glass_to);
                        }
                    });
                });
                fold(ui, force, tr("Grigliato"), |ui| {
                    egui::Grid::new("cabina_grigliato").num_columns(2).show(ui, |ui| {
                        mm(ui, tr("Passo delle barre"), tr("Distanza tra le barre, nei due versi"), &mut p.bar_pitch);
                        mm(ui, tr("Larghezza barre"), tr("Larghezza delle barre piatte"), &mut p.bar_width);
                        mm(ui, tr("Spessore barre"), tr("Spessore delle barre piatte (il grigliato ne ha due strati)"), &mut p.bar_thickness);
                        ui.label(tr("Spessore verso")).on_hover_text(tr("Da che parte del piano del grigliato stanno le barre"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut p.grid_up, true, tr("Alto"))
                                .on_hover_text(tr("Le barre stanno sopra il piano del grigliato: il piano è il fondo delle barre"));
                            ui.selectable_value(&mut p.grid_up, false, tr("Basso"))
                                .on_hover_text(tr("Le barre stanno sotto il piano del grigliato: il piano è la loro faccia superiore"));
                        });
                        ui.end_row();
                        mm(ui, tr("Moduli lungo X"), tr("Lunghezza dei moduli del grigliato"), &mut p.grid_module_x);
                        mm(ui, tr("Moduli lungo Y"), tr("Larghezza dei moduli del grigliato"), &mut p.grid_module_y);
                        ui.label(tr("Grigliato ribassato"));
                        let mut lowered = p.grid_z < 0.0;
                        if ui
                            .checkbox(&mut lowered, "")
                            .on_hover_text(tr("Grigliato sotto il piano della tavola, per far lavorare il robot sulle parti basse"))
                            .changed()
                        {
                            p.grid_z = if lowered { -500.0 } else { 0.0 };
                        }
                        ui.end_row();
                        if lowered {
                            mm_signed(
                                ui,
                                tr("Quota grigliato"),
                                tr("Piano del grigliato rispetto allo zero della cabina (piano della tavola): negativa = più in basso"),
                                &mut p.grid_z,
                            );
                        }
                    });
                });
                fold(ui, force, tr("Tavola trasportatore"), |ui| {
                    ui.checkbox(&mut p.conveyor, tr("Tavola sull'asse della cabina")).on_hover_text(
                        tr("Rulliera dello skid per tutta la lunghezza: ruote che escono dal piano della tavola (Z = 0), \
                         carter con le asole, grigliato in mezzo, telaio con le gambe fino al fondo"),
                    );
                    if p.conveyor {
                        egui::Grid::new("cabina_tavola").num_columns(2).show(ui, |ui| {
                            mm(ui, tr("Larghezza"), tr("Ingombro della tavola (carter), centrata sull'asse"), &mut p.conveyor_width);
                            mm(ui, tr("Scartamento ruote"), tr("Distanza tra le due file di ruote (come i pattini dello skid)"), &mut p.wheel_gauge);
                            mm(ui, tr("Diametro ruote"), tr("Diametro delle ruote"), &mut p.wheel_diameter);
                            mm(ui, tr("Passo ruote"), tr("Distanza tra le ruote lungo la cabina"), &mut p.wheel_pitch);
                            mm(ui, tr("Sporgenza ruote"), tr("Quanto le ruote escono sopra il piano della tavola"), &mut p.wheel_above);
                            mm(ui, tr("Passo gambe"), tr("Distanza tra le gambe del telaio"), &mut p.leg_pitch);
                        });
                    }
                });
                fold(ui, force, tr("Canalina"), |ui| {
                    ui.checkbox(&mut p.channel, tr("Canalina nel pavimento")).on_hover_text(
                        tr("Canale lungo tutta la cabina: sul grigliato, o sul piano della tavola se ci passa dentro"),
                    );
                    if p.channel {
                        egui::Grid::new("cabina_canalina").num_columns(2).show(ui, |ui| {
                            mm_signed(ui, tr("Posizione Y"), tr("Asse della canalina rispetto all'asse della cabina"), &mut p.channel_y);
                            mm(ui, tr("Larghezza"), tr("Larghezza esterna"), &mut p.channel_width);
                            mm(ui, tr("Profondità"), tr("Dal pavimento al fondo"), &mut p.channel_depth);
                            mm(ui, tr("Spessore"), tr("Spessore delle lamiere"), &mut p.channel_wall);
                            mm(ui, tr("Bordo"), tr("Quanto le sponde escono sopra il pavimento"), &mut p.channel_lip);
                        });
                    }
                });
                fold(ui, force, tr("Aperture"), |ui| {
                    egui::Grid::new("cabina_aperture").num_columns(2).show(ui, |ui| {
                        mm(ui, tr("Ingresso: larghezza"), tr("Apertura nella testata di ingresso, centrata sull'asse"), &mut p.entry_width);
                        mm(ui, tr("Ingresso: altezza"), tr("Dal grigliato"), &mut p.entry_height);
                        mm(ui, tr("Uscita: larghezza"), tr("Apertura nella testata di uscita, centrata sull'asse"), &mut p.exit_width);
                        mm(ui, tr("Uscita: altezza"), tr("Dal grigliato"), &mut p.exit_height);
                    });
                });
                fold(ui, force, tr("Scritta"), |ui| label_section(ui, &mut p.label));
                fold(ui, force, tr("Centro cabina"), |ui| {
                    ui.label(
                        egui::RichText::new(
                            tr("Punto della cabina (dall'ingresso, sull'asse, al piano del grigliato) che diventa lo zero della scena \
                             e delle coordinate dei robot. «Applica centro» sposta la cabina, il centro resta lì.\nSi vede con «Centro» sopra la vista."),
                        )
                        .weak(),
                    );
                    ui.horizontal(|ui| {
                        for (name, v) in ["X", "Y", "Z"].into_iter().zip(cell.center.iter_mut()) {
                            ui.label(name);
                            ui.add(egui::DragValue::new(v).speed(10.0).suffix(" mm").max_decimals(1));
                        }
                    });
                    ui.checkbox(&mut cell.marker, tr("Simbolo nel centro (si esporta)")).on_hover_text(
                        tr("Sfera gialla con gli assi X (rosso), Y (verde) e Z (blu), nel file «Centro cabina» («Centro e robot» con i robot): \
                         si vede, si seleziona e si esporta. Si applica con «Applica centro»"),
                    );
                    if cell.marker {
                        egui::Grid::new("cabina_simbolo").num_columns(2).show(ui, |ui| {
                            mm(ui, tr("Misura del simbolo"), tr("Lunghezza degli assi"), &mut cell.marker_size);
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!busy, egui::Button::new(icon_text(ui, icon::HAMMER, tr("Applica centro"))))
                            .on_hover_text(
                                tr("Sposta la cabina di meno il centro (dalla posizione in cui è stata costruita, come l'offset \
                                 degli script): il centro e i robot restano nello zero della scena, sul punto scelto della cabina. \
                                 Premuto di nuovo non si somma (Ctrl+Z per tornare indietro). I robot si cambiano nella finestra Robot"),
                            )
                            .clicked()
                        {
                            apply_cell = true;
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new(tr("Reset")))
                            .on_hover_text(tr("Centro a X 0 Y 0 Z 0 (inizio della cabina) e cabina dove è stata costruita"))
                            .clicked()
                        {
                            cell.center = [0.0; 3];
                            apply_cell = true;
                        }
                    });
                    match current_center {
                        Some(now) => {
                            ui.label(egui::RichText::new(trf!(
                                "Centro adesso nella cabina: X {x} Y {y} Z {z}",
                                x = format!("{:.1}", now.x),
                                y = format!("{:.1}", now.y),
                                z = format!("{:.1}", now.z)
                            )).monospace());
                            if (now - cell.center()).length() > 0.05 {
                                ui.colored_label(ui.visuals().warn_fg_color, tr("Centro non applicato"));
                            }
                        }
                        None => {
                            ui.label(egui::RichText::new(tr("Costruisci la cabina per spostarla sul centro.")).weak());
                        }
                    }
                });
            });
            let problems = p.problems();
            for problem in &problems {
                ui.colored_label(ui.visuals().warn_fg_color, format!("{} {problem}", icon::WARNING));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let label = if exists { tr("Aggiorna la cabina") } else { tr("Costruisci la cabina") };
                if ui
                    .add_enabled(problems.is_empty() && !busy, egui::Button::new(icon_text(ui, icon::HAMMER, label)))
                    .on_hover_text(if exists {
                        tr("Sostituisce la cabina costruita prima con queste misure")
                    } else {
                        tr("Aggiunge la cabina alla scena come file «Cabina»")
                    })
                    .clicked()
                {
                    build = true;
                }
                if ui.button(tr("Misure predefinite")).on_hover_text(tr("Misure tipiche di una cabina (la scritta resta)")).clicked() {
                    p = CabinParams { label: p.label.clone(), ..CabinParams::default() };
                }
            });
        });
    if p != app.settings.cabin || dock != app.settings.dock_cabin || cell != app.settings.robots {
        let mut settings = app.settings.clone();
        settings.cabin = p.clone();
        settings.dock_cabin = dock;
        settings.robots = cell.clone();
        app.apply_settings(settings);
    }
    if apply_cell {
        app.build_center(&cell);
    }
    if let Some(c) = choice {
        choose_cabin(app, c);
        return;
    }
    if build {
        app.build_cabin(p);
    }
    if !open {
        app.ui_state.cabin_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Testi disegnati (il primo fotogramma di una finestra serve solo a misurarla).
    fn texts(ctx: &egui::Context, app: &mut GestoreApp) -> Vec<String> {
        let _ = ctx.run(Default::default(), |ctx| draw(ctx, app, crate::ui::dock::Pass::Floating));
        let out = ctx.run(Default::default(), |ctx| draw(ctx, app, crate::ui::dock::Pass::Floating));
        let mut texts = Vec::new();
        for c in &out.shapes {
            if let egui::epaint::Shape::Text(t) = &c.shape {
                texts.push(t.galley.text().to_string());
            }
        }
        texts
    }

    /// Due cabine: «Nuova cabina» ne aggiunge una seconda («Cabina 2»); scegliendone una (dal menu
    /// o selezionandola nell'albero) le misure diventano le sue e «Aggiorna» cambia solo quella.
    #[test]
    fn selector_picks_the_cabin_to_edit() {
        use crate::cabin::CabinChoice;
        let mut app = GestoreApp::new_headless();
        app.ui_state.cabin_open = true;
        let ctx = egui::Context::default();
        app.build_cabin(CabinParams::default());
        app.poll_loading(&ctx);
        choose_cabin(&mut app, CabinChoice::New);
        assert!(app.cabin_node().is_none());
        app.build_cabin(CabinParams { length: 12000.0, ..CabinParams::default() });
        let cabins = app.cabins();
        assert_eq!(cabins.iter().map(|c| (c.1.as_str(), c.2.length)).collect::<Vec<_>>(), [("Cabina", 20000.0), ("Cabina 2", 12000.0)]);
        assert_eq!(app.cabin_node(), Some(cabins[1].0), "resta sulla nuova");

        // Dal menu: la prima, con le sue misure; «Aggiorna» cambia solo lei
        choose_cabin(&mut app, CabinChoice::Node(cabins[0].0));
        assert_eq!(app.settings.cabin.length, 20000.0);
        app.build_cabin(CabinParams { width: 6500.0, ..app.settings.cabin.clone() });
        let after = app.cabins();
        assert_eq!(after.iter().map(|c| (c.1.as_str(), c.2.length, c.2.width)).collect::<Vec<_>>(), [("Cabina 2", 12000.0, 5700.0), ("Cabina", 20000.0, 6500.0)]);

        // Dall'albero: un pezzo della seconda cabina → la finestra passa su di lei
        let s = app.scene_graph.as_ref().unwrap();
        let second = after.iter().find(|c| c.1 == "Cabina 2").unwrap().0;
        let piece = (0..s.nodes.len() as u32).find(|&n| s.source_of_node(n).is_some_and(|i| s.sources[i].node == second) && s.display_name(n) == "Soffitto").unwrap();
        app.ui_state.select_single(Some(piece));
        let texts = texts(&ctx, &mut app);
        assert_eq!(app.cabin_node(), Some(second));
        assert_eq!(app.settings.cabin.length, 12000.0, "misure della cabina selezionata");
        assert!(texts.iter().any(|t| t.starts_with("Cabina 2  (12000")), "{texts:?}");
    }

    /// La finestra si apre con il pulsante giusto; dopo la costruzione il centro cabina mostra le
    /// coordinate (metà lunghezza, asse, grigliato), anche dopo uno spostamento.
    #[test]
    fn window_builds_and_shows_the_center() {
        let mut app = GestoreApp::new_headless();
        app.ui_state.cabin_open = true;
        let ctx = egui::Context::default();
        let first = texts(&ctx, &mut app);
        assert!(first.iter().any(|t| t.ends_with("Costruisci la cabina")), "{first:?}");
        app.build_cabin(CabinParams::default());
        app.poll_loading(&ctx);
        let (center, _) = app.cabin_center().expect("centro");
        assert_eq!(center, glam::Vec3::new(10000.0, 0.0, 0.0));
        let node = app.cabin_node().unwrap();
        app.apply(crate::app::Action::Move { nodes: vec![node], matrix: glam::Mat4::from_translation(glam::Vec3::new(-1500.0, 200.0, -500.0)) });
        let (moved, _) = app.cabin_center().unwrap();
        assert_eq!(moved, glam::Vec3::new(8500.0, 200.0, -500.0));
        let after = texts(&ctx, &mut app);
        assert!(after.iter().any(|t| t.ends_with("Aggiorna la cabina")), "{after:?}");
    }
}

/// Sezione richiudibile; `force` la apre o la chiude («Espandi» e «Comprimi»).
fn fold<R>(
    ui: &mut egui::Ui,
    force: Option<bool>,
    title: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::CollapsingResponse<R> {
    egui::CollapsingHeader::new(title).open(force).show(ui, body)
}
