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

use crate::i18n::tr;
use crate::log::gigabytes;

/// Memoria di Dima 3D e del PC: (usata da Dima, totale, libera) in MB. Letta al massimo una volta
/// al secondo; si aggiorna quando la finestra si ridisegna (niente ridisegni apposta, che con
/// scene grandi costerebbero lavoro alla scheda grafica).
fn memory_status(ctx: &egui::Context) -> Option<(u64, u64, u64)> {
    let id = egui::Id::new("memoria_barra_stato");
    let now = ctx.input(|i| i.time);
    if let Some((at, value)) = ctx.data(|d| d.get_temp::<(f64, Option<(u64, u64, u64)>)>(id)) {
        if now - at < 1.0 {
            return value;
        }
    }
    let value = crate::log::process_memory_mb()
        .zip(crate::log::memory_mb())
        .map(|(own, (total, free))| (own, total, free));
    ctx.data_mut(|d| d.insert_temp(id, (now, value)));
    value
}

/// Indicatore della RAM nella barra di stato.
fn memory_indicator(ui: &mut egui::Ui, own: u64, total: u64, free: u64) {
    // Meno del 10% libero: il PC comincia a usare il disco al posto della RAM
    let low = free * 10 < total;
    let mut hover = trf!(
        "Memoria usata da Dima 3D: {own} GB\nUsata da altri programmi: {others} GB\nLibera: {free} GB su {total} GB",
        own = gigabytes(own),
        others = gigabytes(total.saturating_sub(free).saturating_sub(own)),
        free = gigabytes(free),
        total = gigabytes(total)
    );
    if low {
        hover.push_str("\n\n");
        hover.push_str(tr("RAM quasi esaurita: aprire altri file può rallentare molto il PC"));
    }
    // Barra della RAM del PC: parte di Dima 3D, parte degli altri programmi, parte libera
    let text = egui::RichText::new(trf!("libera {n} GB", n = gigabytes(free))).small();
    let text = if low { text.color(theme::warning_color(ui)) } else { text.weak() };
    let label = ui.label(text);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(110.0, 10.0), egui::Sense::hover());
    let painter = ui.painter();
    let visuals = ui.visuals();
    let radius = 2.0;
    painter.rect_filled(rect, radius, visuals.extreme_bg_color);
    let total_f = total.max(1) as f32;
    let used = (total.saturating_sub(free)) as f32 / total_f;
    let dima = (own as f32 / total_f).min(used);
    let others_color = if low { theme::warning_color(ui) } else { visuals.widgets.inactive.bg_fill.gamma_multiply(1.6) };
    let mut used_rect = rect;
    used_rect.set_width(rect.width() * used);
    painter.rect_filled(used_rect, radius, others_color);
    let mut dima_rect = rect;
    dima_rect.set_width(rect.width() * dima);
    painter.rect_filled(dima_rect, radius, theme::accent_color(ui));
    painter.rect_stroke(rect, radius, visuals.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    let tag = ui.label(egui::RichText::new(tr("RAM")).small().weak());
    label.union(response).union(tag).on_hover_text(hover);
}

/// Fotogrammi al secondo mentre si muove la vista con il mouse, misurati ogni mezzo secondo.
/// L'ultimo valore resta finché non si muove di nuovo la vista; `None` prima del primo movimento.
fn view_fps(ctx: &egui::Context, moving: bool) -> Option<f32> {
    #[derive(Clone, Copy, Default)]
    struct Meter {
        was_moving: bool,
        time: f32,
        frames: u32,
        fps: Option<f32>,
    }
    let id = egui::Id::new("fps_barra_stato");
    let mut m: Meter = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    // Il primo fotogramma del movimento conterebbe anche l'attesa prima del clic
    if moving && m.was_moving {
        m.time += ctx.input(|i| i.unstable_dt);
        m.frames += 1;
        if m.time >= 0.5 {
            m.fps = Some(m.frames as f32 / m.time);
            m.time = 0.0;
            m.frames = 0;
        }
    } else {
        m.time = 0.0;
        m.frames = 0;
    }
    m.was_moving = moving;
    ctx.data_mut(|d| d.insert_temp(id, m));
    m.fps
}

/// Barra di stato: a sinistra il messaggio, a destra avvisi, avanzamento, statistiche della scena,
/// fotogrammi al secondo e memoria.
pub fn draw_status_bar(ui: &mut egui::Ui, app: &crate::app::GestoreApp) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some((own, total, free)) = memory_status(ui.ctx()) {
            memory_indicator(ui, own, total, free);
            ui.separator();
        }

        let moving = app.ui_state.drag.is_some();
        if let (Some(fps), Some(_)) = (view_fps(ui.ctx(), moving), &app.scene_graph) {
            let text = egui::RichText::new(format!("{fps:.0} fps")).small();
            let text = if fps < 15.0 {
                text.color(theme::warning_color(ui))
            } else if moving {
                text
            } else {
                text.weak()
            };
            ui.label(text).on_hover_text(
                tr("Fotogrammi al secondo mentre ruoti o sposti la vista (resta l'ultimo valore).\n\
                 Sopra 30 il movimento è fluido; sotto 15 va a scatti: nascondere i pezzi che \
                 non servono lo rende più leggero."),
            );
            ui.separator();
        }

        if let Some(scene) = &app.scene_graph {
            let (visible, total) = scene.shape_visibility_counts();
            let triangles: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
            let vertices: usize = scene.shapes_data.iter().map(|s| s.world_vertices.len()).sum();
            ui.label(
                egui::RichText::new(trf!(
                    "Pezzi {visible} / {total}   ·   Triangoli {triangles}   ·   Vertici {vertices}",
                    visible = thousands(visible),
                    total = thousands(total),
                    triangles = thousands(triangles),
                    vertices = thousands(vertices),
                ))
                .small()
                .weak(),
            )
            .on_hover_text(trf!(
                "Macro-oggetti: {n}\nComponenti raggruppati in automatico: {c}",
                n = scene.top_level_count(),
                c = scene.macro_components.len()
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
                1 => tr("1 avviso sul file").to_string(),
                n => trf!("{n} avvisi sul file", n = n),
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
            if matches!(app.load_state, crate::app::LoadState::Error(_)) {
                ui.scope(|ui| {
                    theme::flat_buttons(ui);
                    if ui
                        .button(egui::RichText::new(format!("{} Log", icon::FILE_TEXT)).small())
                        .on_hover_text(tr("Apre il log di Dima 3D, con i dettagli dell'errore"))
                        .clicked()
                    {
                        crate::log::open();
                    }
                });
            }
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
