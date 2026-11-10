//! Finestra Info: nome, versione, origine del codice, licenze.

use super::theme::{icon, icon_text};
use crate::app::GestoreApp;

/// Pagina del progetto (codice sorgente).
pub const PROJECT_URL: &str = "https://github.com/matteo80/dima3d";

/// Librerie principali mostrate nella finestra (l'elenco completo è in licenses\librerie-rust.txt).
const LIBRARIES: &[(&str, &str, &str)] = &[
    ("egui / eframe", "interfaccia", "MIT o Apache 2.0"),
    ("wgpu", "grafica 3D", "MIT o Apache 2.0"),
    ("glam", "matematica 3D", "MIT o Apache 2.0"),
    ("rayon", "calcolo in parallelo", "MIT o Apache 2.0"),
    ("meshoptimizer (meshopt)", "riduzione delle facce", "MIT"),
    ("Phosphor Icons (egui-phosphor)", "icone", "MIT"),
    ("OpenCASCADE 8.0.1", "convertitore STEP", "LGPL 2.1 con eccezione"),
    ("JTReader, SharpCompress", "convertitore JT", "MIT"),
];

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp) {
    if !app.ui_state.about {
        return;
    }
    let mut open = true;
    egui::Window::new(format!("{}  Info", icon::INFO))
        .id(egui::Id::new("finestra_info"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                if let Some(texture) = logo(ctx) {
                    ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(72.0, 72.0)));
                }
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(crate::APP_NAME).size(26.0).strong());
                    ui.label(format!("Versione {}", env!("CARGO_PKG_VERSION")));
                    ui.label(
                        egui::RichText::new("Visualizza, posiziona ed esporta matematiche 3D di grandi dimensioni")
                            .weak(),
                    );
                });
            });
            ui.add_space(6.0);
            ui.label("Formati: WRL (VRML 1.0 e 2.0), STEP, JT, STL, HSF, Open Inventor (IV).");

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Come è stato scritto").strong());
            ui.label(
                "Il codice è stato scritto interamente con l'intelligenza artificiale (Claude, di Anthropic). \
                 L'autore del progetto non ha scritto codice: ha definito le richieste, provato il programma \
                 sulle matematiche reali e deciso cosa correggere.",
            );

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Licenza").strong());
            ui.label("Software libero con licenza MIT. Il codice sorgente è pubblico:");
            ui.hyperlink_to(PROJECT_URL, PROJECT_URL);

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Librerie usate").strong());
            egui::Grid::new("info_librerie").num_columns(3).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
                for (name, what, license) in LIBRARIES {
                    ui.label(*name);
                    ui.label(egui::RichText::new(*what).weak());
                    ui.label(egui::RichText::new(*license).small());
                    ui.end_row();
                }
            });
            ui.label(
                egui::RichText::new(
                    "Testi completi delle licenze e note di copyright di tutte le librerie (203 solo per il \
                     programma) nella cartella «licenses» accanto all'eseguibile.",
                )
                .small(),
            );
            let folder = licenses_folder();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(folder.is_some(), egui::Button::new(icon_text(ui, icon::FOLDER_OPEN, "Apri la cartella delle licenze")))
                    .on_disabled_hover_text("Cartella «licenses» non trovata accanto all'eseguibile")
                    .clicked()
                {
                    if let Some(f) = &folder {
                        let _ = std::process::Command::new("explorer").arg(f).spawn();
                    }
                }
                if ui.button("Chiudi").clicked() {
                    app.ui_state.about = false;
                }
            });
        });
    if !open {
        app.ui_state.about = false;
    }
}

/// Cartella delle licenze del pacchetto (accanto all'exe).
fn licenses_folder() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("licenses");
    dir.is_dir().then_some(dir)
}

/// Icona del programma, caricata una volta.
fn logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("logo_info");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icona_256.png")).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    let texture = ctx.load_texture("logo_info", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Barra e finestra Info insieme, come nel programma; testi disegnati con la loro posizione.
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

    fn click(ctx: &egui::Context, app: &mut GestoreApp, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]] {
            frame(ctx, app, events);
        }
    }

    /// Con o senza le scritte la barra sta nella finestra: il pulsante Info (l'ultimo) si vede
    /// appena sopra e appena sotto la larghezza in cui le scritte spariscono.
    #[test]
    fn toolbar_fits_the_window() {
        for width in [1100.0, 1399.0, 1400.0, 1500.0] {
            let mut app = GestoreApp::new_headless();
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0));
            let mut info = None;
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
                    crate::ui::draw_main_layout(ctx, &mut app);
                });
                for c in &out.shapes {
                    if let egui::epaint::Shape::Text(t) = &c.shape {
                        if t.galley.text().starts_with(icon::INFO) {
                            info = Some(t.pos.x + t.galley.rect.width());
                        }
                    }
                }
            }
            let right = info.unwrap_or_else(|| panic!("pulsante Info non disegnato a {width}"));
            assert!(right < width, "a {width} px il pulsante Info finisce a {right}");
        }
    }

    /// «Modifiche non esportate» non finisce sopra i pulsanti a sinistra (es. «Vista»): se non c'è
    /// posto resta solo l'icona di avviso.
    #[test]
    fn unsaved_changes_label_never_covers_the_buttons() {
        for width in [1100.0, 1250.0, 1399.0, 1400.0, 1500.0, 1700.0] {
            let mut app = GestoreApp::new_headless();
            let scene = crate::parser::parse_wrl(include_bytes!("../../esempi/test_colori.wrl"));
            app.project.on_loaded(scene.revision);
            let root = scene.roots[0];
            app.scene_graph = Some(scene);
            app.apply(crate::app::Action::Hide(root));
            assert!(app.is_dirty());
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0));
            let mut rects: Vec<(String, egui::Rect)> = Vec::new();
            for _ in 0..2 {
                rects.clear();
                let out = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| {
                    crate::ui::draw_main_layout(ctx, &mut app);
                });
                for c in &out.shapes {
                    if let egui::epaint::Shape::Text(t) = &c.shape {
                        rects.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2())));
                    }
                }
            }
            let toolbar_y = rects.iter().find(|(t, _)| t.starts_with(icon::INFO)).expect("Info").1.center().y;
            let row: Vec<&(String, egui::Rect)> = rects.iter().filter(|(_, r)| (r.center().y - toolbar_y).abs() < 6.0).collect();
            let warning = row
                .iter()
                .find(|(t, _)| t == "Modifiche non esportate" || t.starts_with(icon::WARNING_CIRCLE))
                .unwrap_or_else(|| panic!("avviso assente a {width}"));
            for (t, r) in &row {
                if t != &warning.0 {
                    assert!(!r.intersects(warning.1.shrink(0.5)), "a {width} px «{}» copre «{t}»", warning.0);
                }
            }
        }
    }

    /// Il pulsante Info apre la finestra con nome, versione e la nota sul codice scritto con AI;
    /// «Chiudi» la chiude.
    #[test]
    fn info_window_opens_from_the_toolbar() {
        let mut app = GestoreApp::new_headless();
        let ctx = egui::Context::default();
        let texts = frame(&ctx, &mut app, vec![]);
        let (_, at) = texts.iter().find(|(t, _)| t.starts_with(icon::INFO)).expect("pulsante Info");
        click(&ctx, &mut app, *at + egui::vec2(6.0, 8.0));
        assert!(app.ui_state.about, "finestra aperta");
        frame(&ctx, &mut app, vec![]);
        let texts = frame(&ctx, &mut app, vec![]);
        let all: Vec<&str> = texts.iter().map(|t| t.0.as_str()).collect();
        assert!(all.contains(&crate::APP_NAME), "{all:?}");
        assert!(all.iter().any(|t| t.starts_with("Versione ")));
        assert!(all.iter().any(|t| t.contains("intelligenza artificiale")));
        let (_, at) = texts.iter().find(|(t, _)| t == "Chiudi").expect("Chiudi");
        click(&ctx, &mut app, *at + egui::vec2(8.0, 6.0));
        assert!(!app.ui_state.about);
    }
}
