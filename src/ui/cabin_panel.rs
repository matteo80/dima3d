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

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp) {
    if !app.ui_state.cabin_open {
        return;
    }
    let mut p: CabinParams = app.settings.cabin;
    let mut open = true;
    let mut build = false;
    let exists = app.cabin_node().is_some();
    let busy = app.ui_state.is_loading || app.project.busy();
    egui::Window::new(format!("{}  Cabina", icon::CUBE_TRANSPARENT))
        .id(egui::Id::new("finestra_cabina"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos(egui::pos2(80.0, 120.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(
                    "Zero all'ingresso della cabina, sull'asse, al piano del grigliato. X verso l'uscita, Z in alto.",
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
                    });
                });
                ui.collapsing("Aperture", |ui| {
                    egui::Grid::new("cabina_aperture").num_columns(2).show(ui, |ui| {
                        mm(ui, "Ingresso: larghezza", "Apertura nella testata di ingresso, centrata sull'asse", &mut p.entry_width);
                        mm(ui, "Ingresso: altezza", "Dal grigliato", &mut p.entry_height);
                        mm(ui, "Uscita: larghezza", "Apertura nella testata di uscita, centrata sull'asse", &mut p.exit_width);
                        mm(ui, "Uscita: altezza", "Dal grigliato", &mut p.exit_height);
                    });
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
                if ui.button("Misure predefinite").on_hover_text("Le misure delle cabine dei file di riferimento").clicked() {
                    p = CabinParams::default();
                }
            });
        });
    if p != app.settings.cabin {
        let mut settings = app.settings.clone();
        settings.cabin = p;
        app.apply_settings(settings);
    }
    if build {
        app.build_cabin(p);
    }
    if !open {
        app.ui_state.cabin_open = false;
    }
}
