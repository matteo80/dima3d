//! Finestra "Riduci facce": obiettivo, anteprima nella vista, dimensione prevista del WRL ed
//! export. Durante il calcolo un riquadro di avanzamento blocca i comandi.

use super::theme::{icon, icon_text};
use crate::app::GestoreApp;
use crate::reduction::{group_thousands, megabytes, unit_to_mm, Mode};

/// Facce proposte all'apertura: un milione (il valore che l'utente usava in MeshLab), o la metà
/// se il modello ne ha di meno.
fn suggested_triangles(current: u64) -> u64 {
    if current > 2_000_000 {
        1_000_000
    } else {
        (current / 2).max(1)
    }
}

/// Errore in testo leggibile: in millimetri se le unità del file sono note.
fn error_text(error: f32, to_mm: Option<f32>) -> String {
    match to_mm {
        Some(f) => format!("{} mm", trim(error * f)),
        None => format!("{} (unità del file)", trim(error)),
    }
}

fn trim(v: f32) -> String {
    let s = if v >= 1.0 { format!("{v:.2}") } else { format!("{v:.4}") };
    s.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp) {
    if let Some(job) = &app.project.reduction.job {
        let progress = job.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
        let seconds = job.started.elapsed().as_secs_f32();
        egui::Modal::new(egui::Id::new("riduzione_in_corso")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.heading("Riduzione delle facce");
            });
            ui.add_space(6.0);
            let step = if progress < 0.9 { "Ricerca dell'errore che dà le facce richieste" } else { "Normali e dimensione del file" };
            ui.add(egui::ProgressBar::new(progress).text(format!("{step}  ·  {seconds:.0} s")));
        });
        return;
    }
    if !app.project.reduction.panel.open {
        return;
    }
    let Some(scene) = &app.scene_graph else { return };
    let current = app.reduction_scope_triangles();
    let to_mm = unit_to_mm(scene);
    let revision = scene.revision;
    let bbox = scene.compute_bounding_box();
    let extent = if bbox.is_valid() { bbox.diagonal() } else { 1.0 };
    let has_selection = app.ui_state.selected_node.is_some();
    let active = app.project.reduction.is_active();
    let summary = app.project.reduction.summary.clone();
    let can_export = !app.project.busy();

    let mut open = true;
    let mut start = false;
    let mut restore = false;
    let mut export = false;
    let mut settings = app.settings.clone();
    let panel = &mut app.project.reduction.panel;
    if !has_selection {
        panel.selection_only = false;
    }
    if panel.triangles == 0 {
        panel.triangles = suggested_triangles(current);
    }
    if panel.max_error <= 0.0 {
        // 0,1 mm, o un centomillesimo dell'ingombro se le unità non sono chiare
        panel.max_error = to_mm.map_or(extent * 1e-5, |f| 0.1 / f);
    }

    egui::Window::new(format!("{}  Riduci facce", icon::TRIANGLE_DASHED))
        .id(egui::Id::new("riduci_facce"))
        .open(&mut open)
        .resizable(false)
        .collapsible(true)
        .default_pos(ctx.screen_rect().right_top() + egui::vec2(-430.0, 60.0))
        .show(ctx, |ui| {
            ui.set_width(370.0);
            egui::Grid::new("riduci_parametri").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Pezzi");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut panel.selection_only, false, "Tutti i visibili");
                    ui.add_enabled_ui(has_selection, |ui| {
                        ui.radio_value(&mut panel.selection_only, true, "Selezione")
                            .on_disabled_hover_text("Seleziona prima uno o più elementi nell'albero o nella vista");
                    });
                });
                ui.end_row();

                ui.label("Facce ora");
                ui.label(egui::RichText::new(group_thousands(current)).strong())
                    .on_hover_text("Facce originali dei pezzi scelti");
                ui.end_row();

                ui.label("Obiettivo");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut panel.mode, Mode::Triangles, "Numero di facce");
                    ui.selectable_value(&mut panel.mode, Mode::MaxError, "Errore massimo").on_hover_text(
                        "Di quanto la superficie ridotta può scostarsi da quella originale: \
                         i piani perdono molte facce, i raccordi poche",
                    );
                });
                ui.end_row();

                ui.label("");
                match panel.mode {
                    Mode::Triangles => {
                        ui.vertical(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut panel.triangles)
                                    .range(1..=current.max(1))
                                    .speed((current as f64 / 500.0).max(1.0))
                                    .custom_formatter(|v, _| group_thousands(v as u64))
                                    .custom_parser(|s| s.replace(['.', ' ', '\''], "").parse::<f64>().ok()),
                            )
                            .on_hover_text("Trascina, oppure fai doppio clic e scrivi il numero");
                            ui.horizontal(|ui| {
                                for (label, fraction) in [("50%", 0.5), ("25%", 0.25), ("10%", 0.1)] {
                                    if ui.small_button(label).clicked() {
                                        panel.triangles = ((current as f64 * fraction) as u64).max(1);
                                    }
                                }
                                if current > 1_000_000 && ui.small_button("1 milione").clicked() {
                                    panel.triangles = 1_000_000;
                                }
                            });
                        });
                    }
                    Mode::MaxError => {
                        let factor = to_mm.unwrap_or(1.0);
                        let mut shown = panel.max_error * factor;
                        let unit = if to_mm.is_some() { " mm" } else { " (unità del file)" };
                        let response = ui.add(
                            egui::DragValue::new(&mut shown)
                                .range(0.0001..=1000.0)
                                .speed(0.002)
                                .max_decimals(4)
                                .suffix(unit),
                        );
                        if response.changed() {
                            panel.max_error = shown / factor;
                        }
                    }
                }
                ui.end_row();
            });
            ui.checkbox(&mut panel.lock_border, "Blocca i bordi aperti").on_hover_text(
                "Contorni delle lamiere, fori e confini tra pezzi restano identici. Senza, si spostano al più \
                 dell'errore massimo e i pezzi si riducono molto di più.",
            );

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(icon_text(ui, icon::TRIANGLE_DASHED, "Calcola anteprima"))
                    .on_hover_text("Riduce i pezzi partendo sempre dalla geometria originale e mostra il risultato nella vista")
                    .clicked()
                {
                    start = true;
                }
                if ui
                    .add_enabled(active, egui::Button::new(icon_text(ui, icon::ARROW_COUNTER_CLOCKWISE, "Originale")))
                    .on_hover_text("Rimette la geometria originale di tutti i pezzi ridotti")
                    .clicked()
                {
                    restore = true;
                }
            });

            if let Some(s) = summary.as_ref() {
                ui.separator();
                if s.pieces == 0 {
                    ui.label("Nessuna faccia da togliere con questo obiettivo: la geometria resta quella originale.");
                }
                let percent = if s.before > 0 { s.after as f64 * 100.0 / s.before as f64 } else { 100.0 };
                egui::Grid::new("riduci_risultato").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label("Facce");
                    ui.label(format!(
                        "{} → {}  ({percent:.0}%)",
                        group_thousands(s.before),
                        group_thousands(s.after)
                    ));
                    ui.end_row();
                    ui.label("Pezzi ridotti");
                    ui.label(s.pieces.to_string());
                    ui.end_row();
                    ui.label("Errore massimo");
                    ui.label(error_text(s.max_error, to_mm));
                    ui.end_row();
                    ui.label("WRL previsto");
                    ui.label(
                        egui::RichText::new(format!(
                            "{} senza normali  ·  {} con normali",
                            megabytes(s.size_without_normals),
                            megabytes(s.size_with_normals)
                        ))
                        .strong(),
                    );
                    ui.end_row();
                });
                if s.revision != revision || s.compact != settings.export_compact {
                    ui.label(
                        egui::RichText::new("La scena è cambiata dopo il calcolo: la dimensione prevista è indicativa.")
                            .small()
                            .weak(),
                    );
                }
            }

            ui.separator();
            ui.checkbox(&mut settings.export_normals, "Scrivi le normali nel file").on_hover_text(
                "Con le normali gli spigoli vivi sono esattamente quelli calcolati qui. Senza, il visualizzatore \
                 le ricalcola (creaseAngle 0,5) e il file è circa la metà.",
            );
            ui.checkbox(&mut settings.export_compact, "Formato compatto").on_hover_text(
                "Niente rientri e numeri con la precisione che serve (un milionesimo dell'ingombro). Senza, i pezzi \
                 non ridotti copiano il testo del file originale.",
            );
            ui.add_space(4.0);
            if ui
                .add_enabled(can_export, egui::Button::new(icon_text(ui, icon::EXPORT, "Esporta WRL…")))
                .on_hover_text("Salva le parti visibili, con le facce ridotte, in un nuovo file")
                .clicked()
            {
                export = true;
            }
        });

    app.project.reduction.panel.open = open;
    app.apply_settings(settings);
    if start {
        app.start_reduction();
    }
    if restore {
        app.restore_original_geometry();
    }
    if export {
        app.export_dialog(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Testi disegnati in un frame, con la loro posizione.
    fn frame(ctx: &egui::Context, app: &mut GestoreApp, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
            crate::ui::draw_main_layout(ctx, app);
            draw(ctx, app);
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

    /// Il pannello si apre dalla toolbar, propone metà delle facce, e "Calcola anteprima" riduce
    /// la scena; a calcolo finito mostra il risultato e la dimensione prevista.
    #[test]
    fn panel_runs_a_reduction() {
        use crate::scene_graph::node::multi_file_tests::{file, BODY_M};
        let mut app = GestoreApp::new_headless();
        let scene = file(BODY_M, "scocca.wrl");
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        let current = app.reduction_scope_triangles();
        assert!(current > 1);
        app.project.reduction.panel.open = true;

        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts = frame(&ctx, &mut app, vec![]);
        }
        for t in ["Riduci facce", "Calcola anteprima", "Numero di facce", "Scrivi le normali nel file", "Esporta WRL…"] {
            assert!(texts.iter().any(|(s, _)| s.contains(t)), "manca «{t}»: {texts:?}");
        }
        assert_eq!(app.project.reduction.panel.triangles, suggested_triangles(current));

        // Clic su "Calcola anteprima"
        let at = texts.iter().find(|(s, _)| s.contains("Calcola anteprima")).map(|(_, p)| *p + egui::vec2(10.0, 6.0)).unwrap();
        let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![egui::Event::PointerMoved(at)], vec![click(true)], vec![click(false)]] {
            frame(&ctx, &mut app, events);
        }
        assert!(app.project.reduction.job.is_some(), "calcolo partito");
        let texts = frame(&ctx, &mut app, vec![]);
        assert!(texts.iter().any(|(s, _)| s.contains("Riduzione delle facce")), "{texts:?}");
        while !app.poll_reduction() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut texts = Vec::new();
        for _ in 0..2 {
            texts = frame(&ctx, &mut app, vec![]);
        }
        assert!(texts.iter().any(|(s, _)| s.contains("WRL previsto")), "{texts:?}");
        assert!(texts.iter().any(|(s, _)| s.contains("senza normali")), "{texts:?}");
    }

    #[test]
    fn suggestions_and_texts() {
        assert_eq!(suggested_triangles(3_716_448), 1_000_000);
        assert_eq!(suggested_triangles(1_500_000), 750_000);
        assert_eq!(suggested_triangles(0), 1);
        assert_eq!(error_text(0.00024, Some(1000.0)), "0,24 mm");
        assert_eq!(error_text(0.1, Some(1.0)), "0,1 mm");
        assert_eq!(error_text(2.5, None), "2,5 (unità del file)");
    }
}
