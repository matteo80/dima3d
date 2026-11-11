//! Widget egui: visualizzatore del PDF a tile con calibrazione della scala e misure.
//!
//! Uso: `PdfMeasure::new(ctx)` una volta, poi `show(ui)` a ogni frame. Funziona dentro qualsiasi
//! `Ui` (finestra principale, `egui::Window`, pannello), quindi si può integrare in un'altra app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Key, Pos2, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Ui, Vec2,
};

use crate::pdf_measure::measure::{distance, format_mm, format_number, parse_number, Calibration, Measurement, PdfPoint};
use crate::pdf_measure::render::{Job, RenderEvent, Renderer};
use crate::pdf_measure::tiles::{self, TileKey};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;
const ZOOM_STEP: f32 = 1.25;
/// Tile tenute in memoria video (512×512×4 byte = 1 MB l'una).
const MAX_TILES: usize = 160;
/// Corona di tile precaricate attorno alla vista.
const PRELOAD: u32 = 1;
/// Lato massimo dell'anteprima a bassa risoluzione della pagina intera.
const OVERVIEW_SIDE: u32 = 2048;
const PAGE_MARGIN: f32 = 40.0;

const COLOR_CALIBRATION: Color32 = Color32::from_rgb(0xe5, 0x39, 0x35);
const COLOR_MEASURE: Color32 = Color32::from_rgb(0x00, 0xa2, 0xff);
const COLOR_PICK: Color32 = Color32::from_rgb(0xff, 0xd0, 0x00);
const COLOR_OK: Color32 = Color32::from_rgb(0x2e, 0x9e, 0x44);
const COLOR_WARN: Color32 = Color32::from_rgb(0xd0, 0x30, 0x30);
const CANVAS_BG: Color32 = Color32::from_rgb(0x40, 0x40, 0x40);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    None,
    Calibrate,
    Measure,
}

struct Doc {
    id: u64,
    path: PathBuf,
    name: String,
    page_sizes: Vec<[f32; 2]>,
}

struct CachedTile {
    texture: TextureHandle,
    last_used: u64,
}

struct Overview {
    doc: u64,
    page: usize,
    texture: TextureHandle,
}

struct CalibrationPrompt {
    p1: PdfPoint,
    p2: PdfPoint,
    text: String,
    error: Option<String>,
    focus: bool,
}

pub struct PdfMeasure {
    renderer: Renderer,
    library_error: Option<String>,
    next_doc_id: u64,
    loading: Option<(u64, PathBuf)>,
    doc: Option<Doc>,
    page: usize,

    /// Punti dello schermo per unità PDF (1,0 = 100%).
    zoom: f32,
    /// Angolo in alto a sinistra della pagina rispetto all'angolo della vista, in punti.
    origin: Vec2,
    fit_requested: bool,
    canvas_size: Vec2,
    canvas_rect: Option<Rect>,

    tiles: HashMap<TileKey, CachedTile>,
    frame: u64,
    overview: Option<Overview>,
    /// Scala dell'ultima vista completa: le sue tile restano sotto quelle nuove durante lo zoom.
    fallback_scale: Option<u32>,
    queued: usize,
    last_tile_ms: Option<f32>,

    tool: Tool,
    picks: Vec<PdfPoint>,
    calibration: Option<Calibration>,
    measurement: Option<Measurement>,
    prompt: Option<CalibrationPrompt>,
    error_message: Option<String>,
    status: String,
}

impl PdfMeasure {
    pub fn new(ctx: &egui::Context) -> Self {
        Self::with_library_dirs(ctx, default_library_dirs())
    }

    /// `library_dirs`: cartelle in cui cercare `pdfium.dll`, in ordine di preferenza.
    pub fn with_library_dirs(ctx: &egui::Context, library_dirs: Vec<PathBuf>) -> Self {
        Self {
            renderer: Renderer::spawn(ctx.clone(), library_dirs),
            library_error: None,
            next_doc_id: 1,
            loading: None,
            doc: None,
            page: 0,
            zoom: 1.5,
            origin: Vec2::splat(PAGE_MARGIN),
            fit_requested: false,
            canvas_size: Vec2::ZERO,
            canvas_rect: None,
            tiles: HashMap::new(),
            frame: 0,
            overview: None,
            fallback_scale: None,
            queued: 0,
            last_tile_ms: None,
            tool: Tool::None,
            picks: Vec::new(),
            calibration: None,
            measurement: None,
            prompt: None,
            error_message: None,
            status: "Nessun PDF caricato".into(),
        }
    }

    pub fn open(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.doc = None;
        self.tiles.clear();
        self.overview = None;
        self.fallback_scale = None;
        self.reset_all();
        self.status = format!("Apertura di {}…", file_name(&path));
        self.renderer.open(id, path.clone());
        self.loading = Some((id, path));
    }

    pub fn path(&self) -> Option<&Path> {
        self.doc.as_ref().map(|d| d.path.as_path())
    }

    /// Nome del file aperto (per il titolo della finestra che ospita il widget).
    pub fn file_name(&self) -> Option<&str> {
        self.doc.as_ref().map(|d| d.name.as_str())
    }

    pub fn calibration(&self) -> Option<&Calibration> {
        self.calibration.as_ref().filter(|c| c.page == self.page)
    }

    pub fn measurement(&self) -> Option<&Measurement> {
        self.measurement.as_ref()
    }

    pub fn page_count(&self) -> usize {
        self.doc.as_ref().map_or(0, |d| d.page_sizes.len())
    }

    /// Tile in memoria e tile ancora in coda (diagnostica).
    pub fn tile_stats(&self) -> (usize, usize) {
        (self.tiles.len(), self.queued)
    }

    /// Disegna tutto: barra degli strumenti, risultati a destra (sotto, se lo spazio è stretto),
    /// vista del PDF al centro. Occupa tutto lo spazio disponibile in `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        self.frame += 1;
        self.process_events(ui.ctx());
        // Esc solo durante una scelta di punti: altrimenti è dell'app che ospita il widget.
        if self.tool != Tool::None && self.prompt.is_none() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.cancel_operation();
        }

        let id = ui.id().with("pdf_measure");
        let compact = ui.available_width() < 720.0;
        egui::TopBottomPanel::top(id.with("toolbar")).show_inside(ui, |ui| self.toolbar(ui));
        if compact {
            egui::TopBottomPanel::bottom(id.with("results_compact"))
                .resizable(false)
                .show_inside(ui, |ui| self.compact_results(ui));
        } else {
            egui::SidePanel::right(id.with("results"))
                .resizable(false)
                .exact_width(260.0)
                .show_inside(ui, |ui| self.side_panel(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(CANVAS_BG))
            .show_inside(ui, |ui| self.canvas(ui));

        self.calibration_window(ui.ctx());
        self.error_window(ui.ctx());
    }

    // ---------------------------------------------------------------- eventi dal renderer

    fn process_events(&mut self, ctx: &egui::Context) {
        while let Some(event) = self.renderer.try_recv() {
            match event {
                RenderEvent::LibraryError(e) => self.library_error = Some(e),
                RenderEvent::Opened { doc, page_sizes } => {
                    if let Some((id, path)) = self.loading.take_if(|(id, _)| *id == doc) {
                        let name = file_name(&path);
                        self.status = name.clone();
                        self.doc = Some(Doc { id, path, name, page_sizes });
                        self.page = 0;
                        self.fit_requested = true;
                    }
                }
                RenderEvent::OpenFailed { doc, error } => {
                    if let Some((_, path)) = self.loading.take_if(|(id, _)| *id == doc) {
                        self.status = "Nessun PDF caricato".into();
                        self.error_message =
                            Some(format!("Impossibile aprire il PDF.\n{}\n\n{error}", path.display()));
                    }
                }
                RenderEvent::Overview { doc, page, image, .. } => {
                    if self.doc.as_ref().is_some_and(|d| d.id == doc) {
                        let texture = ctx.load_texture("pdf-overview", image, TextureOptions::LINEAR);
                        self.overview = Some(Overview { doc, page, texture });
                    }
                }
                RenderEvent::Tile { key, image, millis } => {
                    if self.doc.as_ref().is_some_and(|d| d.id == key.doc) {
                        let texture = ctx.load_texture("pdf-tile", image, TextureOptions::LINEAR);
                        self.tiles.insert(key, CachedTile { texture, last_used: self.frame });
                        self.last_tile_ms = Some(millis);
                    }
                }
                RenderEvent::Failed { error, .. } => self.status = format!("Errore di rendering: {error}"),
            }
        }
        self.evict_tiles();
    }

    fn evict_tiles(&mut self) {
        if self.tiles.len() <= MAX_TILES {
            return;
        }
        let mut by_age: Vec<(u64, TileKey)> = self.tiles.iter().map(|(k, t)| (t.last_used, *k)).collect();
        by_age.sort_unstable_by_key(|(age, _)| *age);
        let excess = self.tiles.len() - MAX_TILES;
        for (_, key) in by_age.into_iter().take(excess) {
            self.tiles.remove(&key);
        }
    }

    // ---------------------------------------------------------------- barra degli strumenti

    fn toolbar(&mut self, ui: &mut Ui) {
        let loaded = self.doc.is_some();
        let active = self.tool != Tool::None;
        let calibrated = self.calibration().is_some();
        let page_count = self.doc.as_ref().map_or(0, |d| d.page_sizes.len());

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.library_error.is_none(), egui::Button::new("Apri PDF")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Seleziona layout PDF")
                    .add_filter("File PDF", &["pdf"])
                    .add_filter("Tutti i file", &["*"])
                    .pick_file()
                {
                    self.open(path);
                }
            }
            ui.separator();
            if ui.add_enabled(loaded && self.page > 0, egui::Button::new("◀ Pagina")).clicked() {
                self.set_page(self.page - 1);
            }
            if ui.add_enabled(loaded && self.page + 1 < page_count, egui::Button::new("Pagina ▶")).clicked() {
                self.set_page(self.page + 1);
            }
            ui.label(if loaded {
                format!("Pagina: {} / {}", self.page + 1, page_count)
            } else {
                "Pagina: - / -".into()
            });
            ui.separator();
            let center = self.canvas_size * 0.5;
            if ui.add_enabled(loaded, egui::Button::new("Zoom −")).clicked() {
                self.zoom_at(center, 1.0 / ZOOM_STEP);
            }
            if ui.add_enabled(loaded, egui::Button::new("Zoom +")).clicked() {
                self.zoom_at(center, ZOOM_STEP);
            }
            if ui.add_enabled(loaded, egui::Button::new("Adatta")).clicked() {
                self.fit_requested = true;
            }
            ui.label(format!("Zoom: {}%", (self.zoom * 100.0).round()));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(loaded && !active, egui::Button::new("Calibra scala")).clicked() {
                self.start_calibration();
            }
            if ui.add_enabled(calibrated && !active, egui::Button::new("Misura")).clicked() {
                self.start_measurement();
            }
            if ui
                .add_enabled(self.measurement.is_some() && !active, egui::Button::new("Cancella misura"))
                .clicked()
            {
                self.cancel_operation();
                self.measurement = None;
            }
            if ui.add_enabled(calibrated && !active, egui::Button::new("Reset scala")).clicked() {
                self.reset_all();
            }
            if ui.add_enabled(active, egui::Button::new("Annulla selezione")).clicked() {
                self.cancel_operation();
            }
            ui.add_space(10.0);
            match self.calibration() {
                Some(c) => ui.label(
                    RichText::new(format!("Scala: calibrata ({:.6} mm/unità PDF)", c.mm_per_unit))
                        .color(COLOR_OK)
                        .strong(),
                ),
                None => ui.label(RichText::new("Scala: non calibrata").color(COLOR_WARN).strong()),
            };
        });
        ui.add_space(2.0);
    }

    // ---------------------------------------------------------------- pannello dei risultati

    fn side_panel(&mut self, ui: &mut Ui) {
        ui.add_space(14.0);
        ui.label(RichText::new("RISULTATO MISURA").size(15.0).strong());
        ui.add_space(8.0);

        let live = self.live_measure(ui.ctx());
        let shown = live.as_ref().or(self.measurement.as_ref());
        let rows = [
            ("Delta X", shown.map(|m| m.dx_mm), false),
            ("Delta Y", shown.map(|m| m.dy_mm), false),
            ("Distanza", shown.map(|m| m.d_mm), true),
        ];
        for (title, value, highlight) in rows {
            ui.label(RichText::new(title).weak());
            match value {
                Some(v) => {
                    let mut text = RichText::new(format_mm(v)).strong().size(if highlight { 18.0 } else { 15.0 });
                    if highlight {
                        text = text.color(COLOR_MEASURE);
                    }
                    if live.is_some() {
                        text = text.italics();
                    }
                    ui.label(text);
                    ui.label(format!("{} m", format_number(v / 1000.0, 3)));
                }
                None => {
                    ui.label(RichText::new("-").strong().size(15.0));
                }
            }
            ui.add_space(6.0);
        }

        ui.separator();
        ui.add_space(6.0);
        let (text, color) = self.mode_text();
        ui.label(RichText::new(text).strong().size(13.0).color(color));
        ui.add_space(16.0);
        ui.label(
            RichText::new(
                "Rotella: scroll verticale\nShift + rotella: orizzontale\nCtrl + rotella: zoom\n\
                 Trascina (qualsiasi tasto): pan\nEsc: annulla selezione\nTrascina qui un PDF per aprirlo",
            )
            .weak(),
        );

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(6.0);
            if let Some(ms) = self.last_tile_ms {
                ui.label(RichText::new(format!("Ultima tile: {ms:.0} ms · in coda: {}", self.queued)).small().weak());
            }
            ui.add(egui::Label::new(RichText::new(&self.status).small().weak()).wrap());
        });
    }

    /// Durante la misura, col primo punto scelto, il risultato segue il mouse.
    fn live_measure(&self, ctx: &egui::Context) -> Option<Measurement> {
        match (self.tool, self.picks.as_slice(), self.calibration(), self.hover_pdf(ctx)) {
            (Tool::Measure, [p1], Some(c), Some(p2)) => Some(c.measure(*p1, p2)),
            _ => None,
        }
    }

    /// Risultati su una riga, sotto la vista, quando lo spazio è stretto.
    fn compact_results(&mut self, ui: &mut Ui) {
        let live = self.live_measure(ui.ctx());
        let shown = live.as_ref().or(self.measurement.as_ref());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let value = |v: Option<f64>| v.map_or_else(|| "-".to_string(), format_mm);
            ui.label(RichText::new("ΔX").weak());
            ui.label(RichText::new(value(shown.map(|m| m.dx_mm))).strong());
            ui.label(RichText::new("ΔY").weak());
            ui.label(RichText::new(value(shown.map(|m| m.dy_mm))).strong());
            ui.label(RichText::new("Distanza").weak());
            let mut d = RichText::new(value(shown.map(|m| m.d_mm))).strong().size(16.0).color(COLOR_MEASURE);
            if live.is_some() {
                d = d.italics();
            }
            ui.label(d);
        });
        let (text, color) = self.mode_text();
        ui.label(RichText::new(text.replace('\n', " · ")).strong().color(color));
        ui.add_space(4.0);
    }

    fn mode_text(&self) -> (String, Color32) {
        let step = if self.picks.is_empty() { "il primo" } else { "il secondo" };
        match self.tool {
            Tool::Calibrate => (format!("CALIBRAZIONE\nSeleziona {step} punto"), COLOR_CALIBRATION),
            Tool::Measure => (format!("MISURA\nSeleziona {step} punto"), COLOR_MEASURE),
            Tool::None if self.measurement.is_some() => {
                ("Misura completata\nPremi Misura per una nuova misura".into(), COLOR_OK)
            }
            Tool::None if self.calibration().is_some() => ("Scala calibrata\nPremi Misura per iniziare".into(), COLOR_OK),
            Tool::None => (
                "Nessuna operazione\nPrima calibra la scala".into(),
                Color32::from_gray(160),
            ),
        }
    }

    // ---------------------------------------------------------------- vista del PDF

    fn canvas(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        self.canvas_size = rect.size();
        self.canvas_rect = Some(rect);

        let Some(doc) = &self.doc else {
            self.queued = 0;
            let message = if let Some(e) = &self.library_error {
                format!("Libreria PDF non disponibile.\n\n{e}\n\nMetti pdfium.dll accanto all'eseguibile.")
            } else if self.loading.is_some() {
                "Apertura del PDF…".into()
            } else {
                "Apri un PDF o trascinalo qui".into()
            };
            painter.text(rect.center(), Align2::CENTER_CENTER, message, FontId::proportional(16.0), Color32::from_gray(200));
            return;
        };
        let doc_id = doc.id;
        let page_size = doc.page_sizes[self.page];
        let page_vec = vec2(page_size[0], page_size[1]);

        if self.fit_requested {
            self.fit_requested = false;
            self.zoom = ((rect.width() - 30.0) / page_vec.x)
                .min((rect.height() - 30.0) / page_vec.y)
                .clamp(MIN_ZOOM, MAX_ZOOM);
        }
        self.handle_canvas_input(ui, &response, rect);
        self.clamp_origin(rect.size(), page_vec);

        // Pagina allineata ai pixel fisici: le tile combaciano senza sfocature.
        let ppp = ui.ctx().pixels_per_point();
        let origin = ((rect.min + self.origin).to_vec2() * ppp).round() / ppp;
        let origin = origin.to_pos2();
        let page_rect = Rect::from_min_size(origin, page_vec * self.zoom);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

        painter.rect_filled(page_rect.translate(vec2(3.0, 3.0)), 0.0, Color32::from_black_alpha(90));
        painter.rect_filled(page_rect, 0.0, Color32::WHITE);
        let mut jobs = Vec::new();
        match &self.overview {
            Some(o) if o.doc == doc_id && o.page == self.page => {
                painter.image(o.texture.id(), page_rect, uv, Color32::WHITE);
            }
            _ => jobs.push(Job::Overview { doc: doc_id, page: self.page, max_side: OVERVIEW_SIDE }),
        }

        // Zona visibile in unità PDF.
        let view_min = (rect.min - origin) / self.zoom;
        let view_max = (rect.max - origin) / self.zoom;
        let scale = self.zoom * ppp;
        let scale_bits = scale.to_bits();
        let frame = self.frame;
        let page_index = self.page;
        let to_screen = |px: [u32; 4], s: f32| {
            let k = self.zoom / s;
            Rect::from_min_size(origin + vec2(px[0] as f32, px[1] as f32) * k, vec2(px[2] as f32, px[3] as f32) * k)
        };

        // Tile della vista precedente (es. prima dello zoom), scalate, finché arrivano le nuove.
        if let Some(fb) = self.fallback_scale.filter(|fb| *fb != scale_bits) {
            let s = f32::from_bits(fb);
            let page_px = tiles::page_px(page_size, s);
            let view = [view_min.x * s, view_min.y * s, view_max.x * s, view_max.y * s];
            if let Some([c0, r0, c1, r1]) = tiles::tile_range(view, page_px, 0) {
                for row in r0..=r1 {
                    for col in c0..=c1 {
                        let key = TileKey { doc: doc_id, page: page_index, scale_bits: fb, col, row };
                        if let Some(t) = self.tiles.get_mut(&key) {
                            t.last_used = frame;
                            let r = to_screen(tiles::tile_px_rect(col, row, page_px), s);
                            painter.image(t.texture.id(), r, uv, Color32::WHITE);
                        }
                    }
                }
            }
        }

        // Tile alla scala attuale.
        let page_px = tiles::page_px(page_size, scale);
        let view = [view_min.x * scale, view_min.y * scale, view_max.x * scale, view_max.y * scale];
        let visible = tiles::tile_range(view, page_px, 0);
        let in_flight = self.renderer.in_flight();
        let mut all_visible_ready = true;
        for (col, row) in tiles::prioritized(view, page_px, PRELOAD) {
            let key = TileKey { doc: doc_id, page: page_index, scale_bits, col, row };
            let is_visible = visible.is_some_and(|[c0, r0, c1, r1]| (c0..=c1).contains(&col) && (r0..=r1).contains(&row));
            if let Some(t) = self.tiles.get_mut(&key) {
                t.last_used = frame;
                if is_visible {
                    let r = to_screen(tiles::tile_px_rect(col, row, page_px), scale);
                    painter.image(t.texture.id(), r, uv, Color32::WHITE);
                }
            } else {
                all_visible_ready &= !is_visible;
                let job = Job::Tile(key);
                if in_flight.as_ref() != Some(&job) {
                    jobs.push(job);
                }
            }
        }
        if all_visible_ready {
            self.fallback_scale = Some(scale_bits);
        }
        self.queued = jobs.len();
        self.renderer.set_jobs(jobs);

        self.draw_overlays(&painter, origin, ui.ctx());

        let cursor = if response.dragged() {
            CursorIcon::Grabbing
        } else if self.tool != Tool::None {
            CursorIcon::Crosshair
        } else {
            CursorIcon::Default
        };
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    fn handle_canvas_input(&mut self, ui: &Ui, response: &egui::Response, rect: Rect) {
        if response.dragged() {
            self.origin += response.drag_delta();
        }
        if response.hovered() {
            let (scroll, zoom, pointer) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if zoom != 1.0 {
                if let Some(p) = pointer {
                    self.zoom_at(p - rect.min, zoom);
                }
            } else if scroll != Vec2::ZERO {
                self.origin += scroll;
            }
        }
        if response.clicked() && self.tool != Tool::None && self.prompt.is_none() {
            if let Some(pos) = response.interact_pointer_pos() {
                let p = (pos - rect.min - self.origin) / self.zoom;
                self.pick([p.x as f64, p.y as f64]);
            }
        }
    }

    /// Zoom attorno a un punto della vista (relativo al suo angolo), che resta fermo.
    fn zoom_at(&mut self, anchor: Vec2, factor: f32) {
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new_zoom - self.zoom).abs() < 1e-6 {
            return;
        }
        let pdf = (anchor - self.origin) / self.zoom;
        self.zoom = new_zoom;
        self.origin = anchor - pdf * new_zoom;
    }

    /// Pagina più piccola della vista: centrata. Più grande: non si allontana oltre il margine.
    fn clamp_origin(&mut self, canvas: Vec2, page: Vec2) {
        let size = page * self.zoom;
        for axis in 0..2 {
            let (c, p) = (canvas[axis], size[axis]);
            self.origin[axis] = if p + 2.0 * PAGE_MARGIN <= c {
                (c - p) * 0.5
            } else {
                self.origin[axis].clamp(c - p - PAGE_MARGIN, PAGE_MARGIN)
            };
        }
    }

    fn hover_pdf(&self, ctx: &egui::Context) -> Option<PdfPoint> {
        let doc = self.doc.as_ref()?;
        let pos = ctx.input(|i| i.pointer.hover_pos())?;
        let canvas = self.canvas_rect.filter(|r| r.contains(pos))?;
        let p = (pos - canvas.min - self.origin) / self.zoom;
        let size = doc.page_sizes[self.page];
        (p.x >= 0.0 && p.y >= 0.0 && p.x <= size[0] && p.y <= size[1]).then(|| [p.x as f64, p.y as f64])
    }

    // ---------------------------------------------------------------- overlay

    fn draw_overlays(&self, painter: &egui::Painter, origin: Pos2, ctx: &egui::Context) {
        let to_screen = |p: PdfPoint| origin + vec2(p[0] as f32, p[1] as f32) * self.zoom;
        if let Some(c) = self.calibration() {
            segment(painter, to_screen(c.p1), to_screen(c.p2), COLOR_CALIBRATION, "CALIBRAZIONE", true);
        }
        if let Some(m) = &self.measurement {
            let label = format!("MISURA  {}", format_mm(m.d_mm));
            segment(painter, to_screen(m.p1), to_screen(m.p2), COLOR_MEASURE, &label, false);
        }
        for (i, p) in self.picks.iter().enumerate() {
            marker(painter, to_screen(*p), COLOR_PICK, &format!("P{}", i + 1));
        }
        // Linea elastica dal primo punto al mouse.
        if let ([p1], Some(pos)) = (self.picks.as_slice(), ctx.input(|i| i.pointer.hover_pos())) {
            if painter.clip_rect().contains(pos) {
                let a = to_screen(*p1);
                painter.add(Shape::dashed_line(&[a, pos], Stroke::new(2.0_f32, COLOR_PICK), 6.0, 4.0));
            }
        }
    }

    // ---------------------------------------------------------------- calibrazione e misura

    pub fn start_calibration(&mut self) {
        self.cancel_operation();
        self.tool = Tool::Calibrate;
        self.calibration = None;
        self.measurement = None;
    }

    pub fn start_measurement(&mut self) {
        if self.calibration().is_none() {
            self.error_message = Some("Prima calibra la scala con una distanza conosciuta.".into());
            return;
        }
        self.cancel_operation();
        self.tool = Tool::Measure;
        self.measurement = None;
    }

    fn pick(&mut self, p: PdfPoint) {
        let Some(doc) = &self.doc else { return };
        let size = doc.page_sizes[self.page];
        if !(0.0..=size[0] as f64).contains(&p[0]) || !(0.0..=size[1] as f64).contains(&p[1]) {
            return;
        }
        self.picks.push(p);
        if self.picks.len() < 2 {
            return;
        }
        let (p1, p2) = (self.picks[0], self.picks[1]);
        match self.tool {
            Tool::Calibrate => {
                if distance(p1, p2) <= 0.0 {
                    self.picks.clear();
                    self.error_message = Some("Calibrazione non valida: i due punti coincidono.".into());
                } else {
                    self.prompt = Some(CalibrationPrompt { p1, p2, text: String::new(), error: None, focus: true });
                }
            }
            Tool::Measure => {
                if let Some(c) = self.calibration() {
                    self.measurement = Some(c.measure(p1, p2));
                }
                self.tool = Tool::None;
                self.picks.clear();
            }
            Tool::None => self.picks.clear(),
        }
    }

    fn calibration_window(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &mut self.prompt else { return };
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("Distanza reale")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Inserisci la distanza reale tra i due punti [mm]:");
                let edit = ui.add(egui::TextEdit::singleline(&mut prompt.text).desired_width(220.0));
                if prompt.focus {
                    edit.request_focus();
                    prompt.focus = false;
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    confirm = true;
                }
                if let Some(e) = &prompt.error {
                    ui.colored_label(COLOR_WARN, e);
                }
                ui.horizontal(|ui| {
                    confirm |= ui.button("OK").clicked();
                    cancel |= ui.button("Annulla").clicked();
                });
                cancel |= ui.input(|i| i.key_pressed(Key::Escape));
            });
        if cancel {
            self.prompt = None;
            self.cancel_operation();
        } else if confirm {
            let value = parse_number(&prompt.text);
            match value.and_then(|v| Calibration::new(self.page, prompt.p1, prompt.p2, v)) {
                Some(c) => {
                    self.calibration = Some(c);
                    self.prompt = None;
                    self.tool = Tool::None;
                    self.picks.clear();
                }
                None => {
                    prompt.error = Some("Scrivi un numero maggiore di zero (es. 5000 o 5.000,5).".into());
                    prompt.focus = true;
                }
            }
        }
    }

    fn error_window(&mut self, ctx: &egui::Context) {
        let Some(message) = &self.error_message else { return };
        let mut close = false;
        egui::Window::new("Attenzione")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(message);
                ui.add_space(6.0);
                close = ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter));
            });
        if close {
            self.error_message = None;
        }
    }

    fn cancel_operation(&mut self) {
        self.tool = Tool::None;
        self.picks.clear();
        self.prompt = None;
    }

    fn reset_all(&mut self) {
        self.cancel_operation();
        self.calibration = None;
        self.measurement = None;
    }

    fn set_page(&mut self, page: usize) {
        self.page = page;
        // La scala vale solo per la pagina su cui è stata calibrata.
        if self.calibration.as_ref().is_some_and(|c| c.page != page) {
            self.reset_all();
        }
        self.cancel_operation();
        self.fallback_scale = None;
        self.origin = Vec2::splat(PAGE_MARGIN);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Cartelle in cui cercare `pdfium.dll`: accanto all'eseguibile, nelle sottocartelle `pdfium` e
/// `tools`, nella cartella corrente e, nelle build di sviluppo, in `third_party/pdfium/bin`.
pub fn default_library_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("pdfium"));
        dirs.push(exe_dir.join("tools"));
        dirs.push(exe_dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    #[cfg(debug_assertions)]
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("third_party/pdfium/bin"));
    dirs
}

fn marker(painter: &egui::Painter, p: Pos2, color: Color32, label: &str) {
    let dark = Stroke::new(2.0_f32, Color32::from_gray(32));
    let line = Stroke::new(2.0_f32, color);
    painter.line_segment([p - vec2(12.0, 0.0), p + vec2(12.0, 0.0)], line);
    painter.line_segment([p - vec2(0.0, 12.0), p + vec2(0.0, 12.0)], line);
    painter.circle(p, 6.0, color, dark);
    painter.text(p + vec2(18.0, -12.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), color);
}

fn segment(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, label: &str, dashed: bool) {
    let stroke = Stroke::new(3.0_f32, color);
    if dashed {
        painter.add(Shape::dashed_line(&[a, b], stroke, 8.0, 5.0));
    } else {
        painter.line_segment([a, b], stroke);
    }
    marker(painter, a, color, "P1");
    marker(painter, b, color, "P2");
    let galley = painter.layout_no_wrap(label.to_owned(), FontId::proportional(12.0), Color32::from_gray(32));
    let mid = pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    let box_rect = Rect::from_center_size(mid, galley.size() + vec2(20.0, 12.0));
    painter.rect_filled(box_rect, 3.0, Color32::WHITE);
    painter.rect_stroke(box_rect, 3.0, Stroke::new(2.0_f32, color), StrokeKind::Inside);
    painter.galley(box_rect.center() - galley.size() * 0.5, galley, Color32::from_gray(32));
}
