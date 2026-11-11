//! Finestra «Cabina»: misure della cabina di verniciatura e costruzione (vedi `crate::cabin`).

use super::theme::{icon, icon_text};
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
        ui.label("Testo");
        ui.add(egui::TextEdit::singleline(&mut label.text).hint_text("es. il nome della cabina").desired_width(200.0));
        ui.end_row();

        ui.label("Font");
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
            if ui.button("Altro file…").clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("Font (TTF, OTF)", &["ttf", "TTF", "otf", "OTF", "ttc", "TTC"])
                    .set_directory(crate::cabin::fonts_dir())
                    .pick_file();
                if let Some(path) = picked {
                    label.font = path.to_string_lossy().into_owned();
                }
                ui.close_menu();
            }
        });
        ui.end_row();

        mm(ui, "Altezza lettere", "Altezza delle maiuscole", &mut label.height);
        mm(ui, "Rilievo", "Spessore delle lettere, fuori dalla parete", &mut label.depth);

        ui.label("Colore");
        ui.color_edit_button_rgb(&mut label.color);
        ui.end_row();

        ui.label("Parete");
        let wall_name = LabelWall::ALL.iter().find(|(w, _)| *w == label.wall).map_or("", |(_, n)| *n);
        egui::ComboBox::from_id_salt("cabina_parete").selected_text(wall_name).width(200.0).show_ui(ui, |ui| {
            for (wall, name) in LabelWall::ALL {
                ui.selectable_value(&mut label.wall, wall, name);
            }
        });
        ui.end_row();

        ui.label("Lato");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut label.inside, false, "Esterno");
            ui.selectable_value(&mut label.inside, true, "Interno");
        });
        ui.end_row();

        mm_signed(
            ui,
            "Spostamento",
            "Dal centro della parete, verso destra per chi legge (0 = centrata)",
            &mut label.offset,
        );
        mm(ui, "Quota", "Altezza della base delle lettere dal grigliato", &mut label.z);
    });
}

/// Coordinate del centro cabina nella scena (mm) e rotazione attorno a Z.
fn center_text(center: glam::Vec3, m: glam::Mat4) -> String {
    let x_axis = m.transform_vector3(glam::Vec3::X);
    let angle = x_axis.y.atan2(x_axis.x).to_degrees();
    let mut text = format!("X {:.1}\nY {:.1}\nZ {:.1}", center.x, center.y, center.z);
    if angle.abs() > 0.005 {
        text.push_str(&format!("\nRotazione Z {angle:.2}°"));
    }
    text
}

/// Assi della cabina nel suo centro, con le coordinate (vista 3D).
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
    let text = format!("Centro cabina\n{}", center_text(center, m));
    let galley = painter.layout_no_wrap(text, egui::FontId::monospace(12.0), egui::Color32::WHITE);
    let at = origin + egui::vec2(12.0, 12.0);
    painter.rect_filled(
        egui::Rect::from_min_size(at, galley.size()).expand(4.0),
        4.0,
        egui::Color32::from_black_alpha(170),
    );
    painter.galley(at, galley, egui::Color32::WHITE);
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
    let mut p: CabinParams = app.settings.cabin.clone();
    let mut open = true;
    let mut build = false;
    let exists = app.cabin_node().is_some();
    let busy = app.ui_state.is_loading || app.project.busy();
    let mut center = app.settings.cabin_center;
    let current_center = app.cabin_center();
    let center_coordinates = current_center.map(|(c, m)| center_text(c, m));
    let mut move_to: Option<glam::Vec3> = None;
    let window = super::dock::Dockable {
        id: "finestra_cabina",
        title: format!("{}  Cabina", icon::CUBE_TRANSPARENT),
        state: &mut dock,
        open: &mut open,
        default_width: 360.0,
        min_width: 300.0,
        default_height: 560.0,
        default_pos: egui::pos2(80.0, 120.0),
    };
    super::dock::show(ctx, pass, window, |ui| {
            ui.label(
                egui::RichText::new(
                    "Zero all'ingresso della cabina, sull'asse, al piano del grigliato (della tavola, se il grigliato è \
                     ribassato). X verso l'uscita, Z in alto.",
                )
                .weak(),
            );
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                ui.collapsing("Dimensioni", |ui| {
                    egui::Grid::new("cabina_dimensioni").num_columns(2).show(ui, |ui| {
                        mm(ui, "Lunghezza", "Lunghezza interna, dall'ingresso all'uscita (X)", &mut p.length);
                        mm(ui, "Larghezza", "Larghezza interna tra le pareti (Y)", &mut p.width);
                        mm(ui, "Altezza", "Dal grigliato al soffitto", &mut p.height);
                        mm(ui, "Fondo sotto il grigliato", "Fin dove scendono le pareti sotto il grigliato", &mut p.depth);
                    });
                })
                .fully_open();
                ui.collapsing("Pareti", |ui| {
                    egui::Grid::new("cabina_pareti").num_columns(2).show(ui, |ui| {
                        mm(ui, "Passo dei montanti", "Distanza tra i montanti delle pareti laterali", &mut p.module);
                        mm(ui, "Montanti", "Lato della sezione quadrata dei montanti", &mut p.post);
                        mm(ui, "Spessore pannelli", "Spessore di pannelli, testate e soffitto", &mut p.panel);
                        ui.label("Fascia vetrata");
                        ui.checkbox(&mut p.glass, "");
                        ui.end_row();
                        if p.glass {
                            mm(ui, "Vetrata da", "Quota inferiore della fascia vetrata, dal grigliato", &mut p.glass_from);
                            mm(ui, "Vetrata a", "Quota superiore della fascia vetrata, dal grigliato", &mut p.glass_to);
                        }
                    });
                });
                ui.collapsing("Grigliato", |ui| {
                    egui::Grid::new("cabina_grigliato").num_columns(2).show(ui, |ui| {
                        mm(ui, "Passo delle barre", "Distanza tra le barre, nei due versi", &mut p.bar_pitch);
                        mm(ui, "Larghezza barre", "Larghezza delle barre piatte", &mut p.bar_width);
                        mm(ui, "Spessore barre", "Spessore delle barre piatte", &mut p.bar_thickness);
                        mm(ui, "Moduli lungo X", "Lunghezza dei moduli del grigliato", &mut p.grid_module_x);
                        mm(ui, "Moduli lungo Y", "Larghezza dei moduli del grigliato", &mut p.grid_module_y);
                        ui.label("Grigliato ribassato");
                        let mut lowered = p.grid_z < 0.0;
                        if ui
                            .checkbox(&mut lowered, "")
                            .on_hover_text("Grigliato sotto il piano della tavola, per far lavorare il robot sulle parti basse")
                            .changed()
                        {
                            p.grid_z = if lowered { -500.0 } else { 0.0 };
                        }
                        ui.end_row();
                        if lowered {
                            mm_signed(
                                ui,
                                "Quota grigliato",
                                "Piano del grigliato rispetto allo zero della cabina (piano della tavola): negativa = più in basso",
                                &mut p.grid_z,
                            );
                        }
                    });
                });
                ui.collapsing("Tavola trasportatore", |ui| {
                    ui.checkbox(&mut p.conveyor, "Tavola sull'asse della cabina").on_hover_text(
                        "Rulliera dello skid per tutta la lunghezza: ruote che escono dal piano della tavola (Z = 0), \
                         carter con le asole, grigliato in mezzo, telaio con le gambe fino al fondo",
                    );
                    if p.conveyor {
                        egui::Grid::new("cabina_tavola").num_columns(2).show(ui, |ui| {
                            mm(ui, "Larghezza", "Ingombro della tavola (carter), centrata sull'asse", &mut p.conveyor_width);
                            mm(ui, "Scartamento ruote", "Distanza tra le due file di ruote (come i pattini dello skid)", &mut p.wheel_gauge);
                            mm(ui, "Diametro ruote", "Diametro delle ruote", &mut p.wheel_diameter);
                            mm(ui, "Passo ruote", "Distanza tra le ruote lungo la cabina", &mut p.wheel_pitch);
                            mm(ui, "Sporgenza ruote", "Quanto le ruote escono sopra il piano della tavola", &mut p.wheel_above);
                            mm(ui, "Passo gambe", "Distanza tra le gambe del telaio", &mut p.leg_pitch);
                        });
                    }
                });
                ui.collapsing("Canalina", |ui| {
                    ui.checkbox(&mut p.channel, "Canalina nel pavimento").on_hover_text(
                        "Canale lungo tutta la cabina: sul grigliato, o sul piano della tavola se ci passa dentro",
                    );
                    if p.channel {
                        egui::Grid::new("cabina_canalina").num_columns(2).show(ui, |ui| {
                            mm_signed(ui, "Posizione Y", "Asse della canalina rispetto all'asse della cabina", &mut p.channel_y);
                            mm(ui, "Larghezza", "Larghezza esterna", &mut p.channel_width);
                            mm(ui, "Profondità", "Dal pavimento al fondo", &mut p.channel_depth);
                            mm(ui, "Spessore", "Spessore delle lamiere", &mut p.channel_wall);
                            mm(ui, "Bordo", "Quanto le sponde escono sopra il pavimento", &mut p.channel_lip);
                        });
                    }
                });
                ui.collapsing("Aperture", |ui| {
                    egui::Grid::new("cabina_aperture").num_columns(2).show(ui, |ui| {
                        mm(ui, "Ingresso: larghezza", "Apertura nella testata di ingresso, centrata sull'asse", &mut p.entry_width);
                        mm(ui, "Ingresso: altezza", "Dal grigliato", &mut p.entry_height);
                        mm(ui, "Uscita: larghezza", "Apertura nella testata di uscita, centrata sull'asse", &mut p.exit_width);
                        mm(ui, "Uscita: altezza", "Dal grigliato", &mut p.exit_height);
                    });
                });
                ui.collapsing("Scritta", |ui| label_section(ui, &mut p.label));
                ui.collapsing("Centro cabina", |ui| {
                    ui.checkbox(&mut center, "Mostra il centro nella vista").on_hover_text(
                        "Assi della cabina a metà lunghezza, sull'asse, al piano del grigliato, con le coordinate: \
                         seguono la cabina quando la sposti con Posizione",
                    );
                    ui.checkbox(&mut p.center_marker, "Simbolo nel centro (si esporta con la cabina)").on_hover_text(
                        "Sfera gialla con gli assi X (rosso, verso l'uscita), Y (verde) e Z (blu): fa parte della cabina, \
                         si vede, si seleziona e si esporta con lei. Si applica con «Aggiorna la cabina»",
                    );
                    if p.center_marker {
                        egui::Grid::new("cabina_simbolo").num_columns(2).show(ui, |ui| {
                            mm(ui, "Misura del simbolo", "Lunghezza degli assi", &mut p.marker_size);
                        });
                    }
                    match center_coordinates {
                        Some(text) => {
                            ui.label(egui::RichText::new(text).monospace());
                        }
                        None => {
                            ui.label(egui::RichText::new("Costruisci la cabina per vederne il centro.").weak());
                        }
                    }
                    // Nuove coordinate del centro: la cabina si sposta (senza ruotare) fin lì
                    if let Some((current, _)) = current_center {
                        let key = egui::Id::new("cabina_centro_nuovo");
                        let mut target: glam::Vec3 = ui.data(|d| d.get_temp(key)).unwrap_or(current);
                        ui.label("Porta il centro in (coordinate della scena):");
                        ui.horizontal(|ui| {
                            for (name, v) in [("X", &mut target.x), ("Y", &mut target.y), ("Z", &mut target.z)] {
                                ui.label(name);
                                ui.add(egui::DragValue::new(v).speed(10.0).suffix(" mm").max_decimals(1));
                            }
                        });
                        ui.horizontal(|ui| {
                            let changed = (target - current).length() > 0.001;
                            if ui
                                .add_enabled(changed && !busy, egui::Button::new("Sposta la cabina"))
                                .on_hover_text("Sposta la cabina in modo che il suo centro vada in queste coordinate (Ctrl+Z per tornare indietro)")
                                .clicked()
                            {
                                move_to = Some(target - current);
                            }
                            if ui.add_enabled(changed, egui::Button::new("Annulla")).clicked() {
                                target = current;
                            }
                        });
                        if move_to.is_some() {
                            ui.data_mut(|d| d.remove::<glam::Vec3>(key));
                        } else {
                            ui.data_mut(|d| d.insert_temp(key, target));
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
                let label = if exists { "Aggiorna la cabina" } else { "Costruisci la cabina" };
                if ui
                    .add_enabled(problems.is_empty() && !busy, egui::Button::new(icon_text(ui, icon::HAMMER, label)))
                    .on_hover_text(if exists {
                        "Sostituisce la cabina costruita prima con queste misure"
                    } else {
                        "Aggiunge la cabina alla scena come file «Cabina»"
                    })
                    .clicked()
                {
                    build = true;
                }
                if ui.button("Misure predefinite").on_hover_text("Misure tipiche di una cabina (la scritta resta)").clicked() {
                    p = CabinParams { label: p.label.clone(), ..CabinParams::default() };
                }
            });
        });
    if p != app.settings.cabin || center != app.settings.cabin_center || dock != app.settings.dock_cabin {
        let mut settings = app.settings.clone();
        settings.cabin = p.clone();
        settings.cabin_center = center;
        settings.dock_cabin = dock;
        app.apply_settings(settings);
    }
    if build {
        app.build_cabin(p);
    }
    if let (Some(delta), Some(node)) = (move_to, app.cabin_node()) {
        app.apply(crate::app::Action::Move { nodes: vec![node], matrix: glam::Mat4::from_translation(delta) });
        app.ui_state.status_message = "Cabina spostata: il centro è nelle coordinate scelte".to_string();
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
        assert_eq!(center_text(moved, glam::Mat4::IDENTITY), "X 8500.0\nY 200.0\nZ -500.0");
        let rotated = glam::Mat4::from_rotation_z(90f32.to_radians());
        assert!(center_text(moved, rotated).ends_with("Rotazione Z 90.00°"));
        let after = texts(&ctx, &mut app);
        assert!(after.iter().any(|t| t.ends_with("Aggiorna la cabina")), "{after:?}");
    }
}
