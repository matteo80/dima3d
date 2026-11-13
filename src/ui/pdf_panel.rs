//! Finestra «Misure PDF»: un layout in PDF su cui si calibra la scala con una distanza nota e
//! si misurano Delta X, Delta Y e distanza in mm (vedi `crate::pdf_measure`). Agganciabile come
//! le altre finestre. Di base si apre a tutto schermo, sopra la vista e i pannelli (sugli schermi
//! piccoli una finestra normale lascia poco spazio al PDF); «Riduci» la rimette mobile/agganciabile.

use std::path::PathBuf;

use super::theme::icon;
use crate::app::GestoreApp;
use crate::pdf_measure::PdfMeasure;

/// Stato della finestra.
pub struct PdfPanel {
    pub open: bool,
    /// A tutto schermo: copre tutto tranne la barra di stato, ignora l'aggancio
    pub full: bool,
    /// Creato alla prima apertura: avvia il thread di rendering e carica `tools\pdfium.dll`.
    /// Resta vivo quando la finestra si chiude, così il PDF e la scala restano.
    pub view: Option<PdfMeasure>,
    /// PDF da aprire al prossimo fotogramma (es. trascinato sulla finestra principale)
    pub pending: Option<PathBuf>,
}

impl Default for PdfPanel {
    fn default() -> Self {
        Self { open: false, full: true, view: None, pending: None }
    }
}

/// Altezza della barra di stato, che resta visibile sotto la finestra a tutto schermo
const STATUS_BAR: f32 = 26.0;

impl std::fmt::Debug for PdfPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfPanel")
            .field("open", &self.open)
            .field("full", &self.full)
            .field("file", &self.view.as_ref().and_then(|v| v.file_name()))
            .field("pending", &self.pending)
            .finish()
    }
}

impl PdfPanel {
    /// Apre la finestra e il PDF indicato.
    pub fn open_file(&mut self, path: PathBuf) {
        self.open = true;
        self.pending = Some(path);
    }
}

/// `true` per i file da aprire nella finestra Misure PDF invece che come modelli 3D.
pub fn is_pdf(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp, pass: super::dock::Pass) {
    if !app.ui_state.pdf.open {
        return;
    }
    let full = app.ui_state.pdf.full;
    let wanted = if full || app.settings.dock_pdf.side == super::dock::Side::Floating {
        super::dock::Pass::Floating
    } else {
        super::dock::Pass::Docked
    };
    if pass != wanted {
        return;
    }
    let mut dock = app.settings.dock_pdf;
    let mut open = true;
    let panel = &mut app.ui_state.pdf;
    let view = panel.view.get_or_insert_with(|| PdfMeasure::new(ctx));
    if let Some(path) = panel.pending.take() {
        view.open(path);
    }
    let title = match view.file_name() {
        Some(name) => format!("{}  Misure PDF — {name}", icon::FILE_PDF),
        None => format!("{}  Misure PDF", icon::FILE_PDF),
    };
    let mut toggle = false;
    let size_button = |ui: &mut egui::Ui, toggle: &mut bool| {
        let (glyph, text, hover) = if full {
            (icon::CORNERS_IN, "Riduci", "Torna una finestra normale, spostabile e agganciabile ai bordi")
        } else {
            (icon::CORNERS_OUT, "Tutto schermo", "La finestra copre la vista e i pannelli")
        };
        if ui.button(super::theme::icon_text(ui, glyph, text)).on_hover_text(hover).clicked() {
            *toggle = true;
        }
    };
    if full {
        let screen = ctx.screen_rect();
        let rect = egui::Rect::from_min_max(screen.min, screen.max - egui::vec2(0.0, STATUS_BAR));
        egui::Window::new(&title)
            .id(egui::Id::new("finestra_pdf_tutto_schermo"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .title_bar(true)
            .fixed_rect(rect)
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                view.show_with(ui, |ui| size_button(ui, &mut toggle));
            });
    } else {
        let window = super::dock::Dockable {
            id: "finestra_pdf",
            title,
            state: &mut dock,
            open: &mut open,
            default_width: 640.0,
            min_width: 360.0,
            default_height: 620.0,
            default_pos: egui::pos2(160.0, 110.0),
            scroll: false,
        };
        super::dock::show(ctx, pass, window, |ui| {
            // Da mobile la finestra non si stringe sotto una vista utilizzabile
            ui.set_min_size(egui::vec2(340.0, 320.0));
            view.show_with(ui, |ui| size_button(ui, &mut toggle));
        });
    }
    if toggle {
        app.ui_state.pdf.full = !full;
    }
    if dock != app.settings.dock_pdf {
        let mut settings = app.settings.clone();
        settings.dock_pdf = dock;
        app.apply_settings(settings);
    }
    if !open {
        app.ui_state.pdf.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dock::{Pass, Side};

    /// Un fotogramma come nel programma; ritorna i testi disegnati con la loro posizione.
    fn frame(ctx: &egui::Context, app: &mut GestoreApp) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let out = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
            crate::ui::draw_main_layout(ctx, app);
            draw(ctx, app, Pass::Floating);
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

    fn find<'a>(texts: &'a [(String, egui::Rect)], text: &str) -> Option<&'a egui::Rect> {
        texts.iter().find(|(t, _)| t == text).map(|(_, r)| r)
    }

    /// La finestra si disegna da mobile e da agganciata a destra, con barra e risultati.
    #[test]
    fn window_floating_and_docked() {
        let mut app = GestoreApp::new_headless();
        app.ui_state.pdf.open = true;
        app.ui_state.pdf.full = false;
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let texts = frame(&ctx, &mut app);
        assert!(find(&texts, &format!("{}  Misure PDF", icon::FILE_PDF)).is_some(), "titolo");
        assert!(find(&texts, "Apri PDF").is_some(), "barra del widget");
        assert!(app.ui_state.pdf.view.is_some(), "widget creato alla prima apertura");

        app.settings.dock_pdf = crate::ui::dock::DockState { side: Side::Right, width: 640.0, height: 0.0 };
        frame(&ctx, &mut app);
        let texts = frame(&ctx, &mut app);
        let open_button = find(&texts, "Apri PDF").expect("barra del widget da agganciata");
        assert!(open_button.left() > 600.0, "agganciata a destra: {open_button:?}");
        // Stretta (640 punti): risultati sotto la vista, su una riga
        assert!(find(&texts, "Distanza").is_some());
        assert!(find(&texts, "RISULTATO MISURA").is_none());
    }

    /// Di base la finestra copre tutto lo schermo (tranne la barra di stato), anche se è
    /// agganciata nelle preferenze; «Riduci» la rimette come prima.
    #[test]
    fn full_screen_by_default() {
        let mut app = GestoreApp::new_headless();
        app.settings.dock_pdf = crate::ui::dock::DockState { side: Side::Right, width: 640.0, height: 0.0 };
        app.ui_state.pdf.open = true;
        assert!(app.ui_state.pdf.full);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let texts = frame(&ctx, &mut app);
        let open_button = find(&texts, "Apri PDF").expect("barra del widget");
        assert!(open_button.left() < 60.0, "a tutto schermo parte da sinistra: {open_button:?}");
        // Larga: risultati a destra
        assert!(find(&texts, "RISULTATO MISURA").is_some());
        let results = find(&texts, "RISULTATO MISURA").unwrap();
        assert!(results.left() > 1000.0, "risultati sul bordo destro dello schermo: {results:?}");
        assert!(texts.iter().any(|(t, _)| t.contains("Riduci")), "pulsante Riduci");
    }

    #[test]
    fn dropped_pdf_goes_to_the_pdf_window() {
        assert!(is_pdf(std::path::Path::new("C:/x/layout.PDF")));
        assert!(!is_pdf(std::path::Path::new("C:/x/scocca.wrl")));
        let mut panel = PdfPanel::default();
        panel.open_file(PathBuf::from("layout.pdf"));
        assert!(panel.open);
        assert_eq!(panel.pending.as_deref(), Some(std::path::Path::new("layout.pdf")));
    }
}
