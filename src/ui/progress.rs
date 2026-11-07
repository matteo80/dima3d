use super::theme::{self, icon};

/// Numero con il separatore delle migliaia.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Barra di stato: a sinistra il messaggio, a destra avvisi, avanzamento e statistiche della scena.
pub fn draw_status_bar(ui: &mut egui::Ui, app: &crate::app::GestoreApp) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(scene) = &app.scene_graph {
            let (visible, total) = scene.shape_visibility_counts();
            let triangles: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
            let vertices: usize = scene.shapes_data.iter().map(|s| s.world_vertices.len()).sum();
            ui.label(
                egui::RichText::new(format!(
                    "Pezzi {} / {}   ·   Triangoli {}   ·   Vertici {}",
                    thousands(visible),
                    thousands(total),
                    thousands(triangles),
                    thousands(vertices),
                ))
                .small()
                .weak(),
            )
            .on_hover_text(format!(
                "Macro-oggetti: {}\nComponenti raggruppati in automatico: {}",
                scene.top_level_count(),
                scene.macro_components.len()
            ));
        }

        if app.ui_state.is_loading {
            let progress = egui::ProgressBar::new(app.ui_state.load_progress)
                .desired_width(180.0)
                .text(format!("{:.0}%", app.ui_state.load_progress * 100.0));
            ui.add(progress);
        }

        // Avvisi sul file aperto: restano qui anche quando il messaggio di stato cambia
        let warnings = &app.ui_state.load_warnings;
        if !warnings.is_empty() {
            let text = match warnings.len() {
                1 => "1 avviso sul file".to_string(),
                n => format!("{n} avvisi sul file"),
            };
            let color = theme::warning_color(ui);
            ui.scope(|ui| {
                theme::flat_buttons(ui);
                ui.menu_button(egui::RichText::new(format!("{} {text}", icon::WARNING)).color(color), |ui| {
                    ui.set_max_width(460.0);
                    for warning in warnings {
                        ui.label(warning);
                    }
                });
            });
        }

        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let text = egui::RichText::new(&app.ui_state.status_message);
            let text = match &app.load_state {
                crate::app::LoadState::Error(_) => text.color(theme::error_color(ui)),
                _ => text,
            };
            ui.add(egui::Label::new(text).truncate());
        });
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands_uses_the_italian_separator() {
        assert_eq!(super::thousands(7), "7");
        assert_eq!(super::thousands(958), "958");
        assert_eq!(super::thousands(1533), "1.533");
        assert_eq!(super::thousands(3_700_000), "3.700.000");
    }
}
