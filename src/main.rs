// Nell'exe finale niente finestra di console dietro a quella del programma
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cabin;
mod clusterer;
mod exporter;
mod parser;
mod project;
mod project_file;
mod reduce;
mod reduction;
mod renderer;
mod scene_graph;
mod settings;
mod text3d;
mod ui;
mod update;

/// Nome del programma (titolo della finestra, finestra Info, file esportati).
pub const APP_NAME: &str = "Dima 3D";

fn main() -> eframe::Result<()> {
    // Inizializza il sistema di logging/tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Avvio {APP_NAME} {}", env!("CARGO_PKG_VERSION"));

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 600.0]);
    match eframe::icon_data::from_png_bytes(include_bytes!("../assets/icona_256.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => tracing::warn!("Icona della finestra non valida: {e}"),
    }

    let native_options = eframe::NativeOptions {
        viewport,
        // Abilita il backend wgpu richiesto dalla feature flag di eframe
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        native_options,
        Box::new(|cc| Ok(Box::new(app::GestoreApp::new(cc)))),
    )
}
