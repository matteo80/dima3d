//! Rendering in un thread dedicato con pdfium.
//!
//! Il thread possiede la libreria, il documento e l'ultima pagina caricata (così pdfium non
//! rianalizza il contenuto della pagina per ogni tile). La GUI sostituisce in blocco la lista dei
//! lavori quando cambia la vista: i lavori superati non vengono mai eseguiti.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use egui::{Color32, ColorImage};
use pdfium_render::prelude::*;

use crate::pdf_measure::tiles::TileKey;

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Pagina intera a bassa risoluzione, mostrata sotto le tile mentre arrivano.
    Overview { doc: u64, page: usize, max_side: u32 },
    Tile(TileKey),
}

impl Job {
    fn doc(&self) -> u64 {
        match self {
            Job::Overview { doc, .. } => *doc,
            Job::Tile(k) => k.doc,
        }
    }
    fn page(&self) -> usize {
        match self {
            Job::Overview { page, .. } => *page,
            Job::Tile(k) => k.page,
        }
    }
}

pub enum RenderEvent {
    LibraryError(String),
    Opened { doc: u64, page_sizes: Vec<[f32; 2]> },
    OpenFailed { doc: u64, error: String },
    Overview { doc: u64, page: usize, scale: f32, image: ColorImage },
    Tile { key: TileKey, image: ColorImage, millis: f32 },
    Failed { job: Job, error: String },
}

#[derive(Default)]
struct Shared {
    open: Option<(u64, PathBuf)>,
    /// Lavori in attesa, il più importante in fondo (si estrae con `pop`).
    jobs: Vec<Job>,
    in_flight: Option<Job>,
    stop: bool,
}

pub struct Renderer {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    rx: mpsc::Receiver<RenderEvent>,
    thread: Option<JoinHandle<()>>,
}

impl Renderer {
    /// `library_dirs`: cartelle dove cercare `pdfium.dll`, in ordine; poi quella di sistema.
    pub fn spawn(ctx: egui::Context, library_dirs: Vec<PathBuf>) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        let worker_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("pdf-tile-renderer".into())
            .spawn(move || worker(worker_shared, tx, ctx, library_dirs))
            .expect("avvio del thread di rendering");
        Self { shared, rx, thread: Some(thread) }
    }

    /// Apre un documento: i lavori in attesa vengono scartati.
    pub fn open(&self, doc: u64, path: PathBuf) {
        let (lock, cvar) = &*self.shared;
        let mut s = lock.lock().unwrap();
        s.jobs.clear();
        s.open = Some((doc, path));
        cvar.notify_one();
    }

    /// Sostituisce i lavori in attesa; `jobs` è in ordine di priorità, il primo è il più urgente.
    pub fn set_jobs(&self, mut jobs: Vec<Job>) {
        jobs.reverse();
        let (lock, cvar) = &*self.shared;
        let mut s = lock.lock().unwrap();
        if s.jobs != jobs {
            s.jobs = jobs;
            cvar.notify_one();
        }
    }

    pub fn in_flight(&self) -> Option<Job> {
        self.shared.0.lock().unwrap().in_flight.clone()
    }

    pub fn try_recv(&self) -> Option<RenderEvent> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        {
            let (lock, cvar) = &*self.shared;
            let mut s = lock.lock().unwrap();
            s.stop = true;
            s.jobs.clear();
            cvar.notify_one();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn bind_pdfium(dirs: &[PathBuf]) -> Result<Box<dyn PdfiumLibraryBindings>, String> {
    let mut errors = Vec::new();
    for dir in dirs {
        let path = Pdfium::pdfium_platform_library_name_at_path(dir);
        if path.is_file() {
            match Pdfium::bind_to_library(&path) {
                Ok(b) => return Ok(b),
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
    }
    Pdfium::bind_to_system_library().map_err(|e| {
        errors.push(format!("libreria di sistema: {e}"));
        format!("pdfium.dll non trovata o non caricabile.\n{}", errors.join("\n"))
    })
}

enum Task {
    Open(u64, PathBuf),
    Job(Job),
}

fn worker(
    shared: Arc<(Mutex<Shared>, Condvar)>,
    tx: mpsc::Sender<RenderEvent>,
    ctx: egui::Context,
    library_dirs: Vec<PathBuf>,
) {
    let send = |ev: RenderEvent| {
        let _ = tx.send(ev);
        ctx.request_repaint();
    };
    let pdfium = match bind_pdfium(&library_dirs) {
        Ok(b) => Pdfium::new(b),
        Err(e) => {
            send(RenderEvent::LibraryError(e));
            return;
        }
    };

    // L'ordine conta: la pagina va chiusa prima del documento.
    let mut page: Option<(usize, PdfPage)> = None;
    let mut doc: Option<(u64, PdfDocument)> = None;

    loop {
        let task = {
            let (lock, cvar) = &*shared;
            let mut s = lock.lock().unwrap();
            loop {
                if s.stop {
                    return;
                }
                if let Some((id, path)) = s.open.take() {
                    break Task::Open(id, path);
                }
                if let Some(job) = s.jobs.pop() {
                    s.in_flight = Some(job.clone());
                    break Task::Job(job);
                }
                s = cvar.wait(s).unwrap();
            }
        };

        match task {
            Task::Open(id, path) => {
                page = None;
                doc = None;
                match open_document(&pdfium, &path) {
                    Ok((d, page_sizes)) => {
                        doc = Some((id, d));
                        send(RenderEvent::Opened { doc: id, page_sizes });
                    }
                    Err(error) => send(RenderEvent::OpenFailed { doc: id, error }),
                }
            }
            Task::Job(job) => {
                let event = match &doc {
                    Some((id, d)) if *id == job.doc() => {
                        if page.as_ref().map(|p| p.0) != Some(job.page()) {
                            drop(page.take()); // chiude la pagina precedente prima di aprirne un'altra
                            page = d.pages().get(job.page() as PdfPageIndex).ok().map(|p| (job.page(), p));
                        }
                        match &page {
                            Some((_, p)) => Some(run_job(p, &job)),
                            None => Some(RenderEvent::Failed { job: job.clone(), error: "pagina non leggibile".into() }),
                        }
                    }
                    _ => None, // documento già sostituito: lavoro superato
                };
                if let Some(ev) = event {
                    send(ev);
                }
                shared.0.lock().unwrap().in_flight = None;
            }
        }
    }
}

fn open_document<'a>(pdfium: &'a Pdfium, path: &Path) -> Result<(PdfDocument<'a>, Vec<[f32; 2]>), String> {
    let d = pdfium.load_pdf_from_file(path, None).map_err(|e| e.to_string())?;
    let n = d.pages().len();
    if n == 0 {
        return Err("Il PDF non contiene pagine.".into());
    }
    // Dimensioni già ruotate secondo /Rotate, senza caricare (e analizzare) le pagine.
    let sizes = (0..n)
        .map(|i| {
            d.pages()
                .page_size(i)
                .map(|r| [r.width().value, r.height().value])
                .unwrap_or([595.0, 842.0])
        })
        .collect();
    Ok((d, sizes))
}

fn run_job(page: &PdfPage, job: &Job) -> RenderEvent {
    let t0 = Instant::now();
    let size = [page.width().value, page.height().value];
    match job {
        Job::Overview { doc, page: index, max_side } => {
            let scale = (*max_side as f32 / size[0].max(size[1])).min(4.0);
            let px = crate::pdf_measure::tiles::page_px(size, scale);
            match render_region(page, scale, 0, 0, px[0], px[1]) {
                Ok(image) => RenderEvent::Overview { doc: *doc, page: *index, scale, image },
                Err(e) => RenderEvent::Failed { job: job.clone(), error: e.to_string() },
            }
        }
        Job::Tile(key) => {
            let [x, y, w, h] = key.px_rect(size);
            if w == 0 || h == 0 {
                return RenderEvent::Failed { job: job.clone(), error: "tile fuori pagina".into() };
            }
            match render_region(page, key.scale(), x, y, w, h) {
                Ok(image) => RenderEvent::Tile {
                    key: *key,
                    image,
                    millis: t0.elapsed().as_secs_f32() * 1000.0,
                },
                Err(e) => RenderEvent::Failed { job: job.clone(), error: e.to_string() },
            }
        }
    }
}

/// Renderizza la zona [x, x+w) × [y, y+h) della pagina scalata di `scale` pixel per unità PDF.
pub fn render_region(page: &PdfPage, scale: f32, x: u32, y: u32, w: u32, h: u32) -> Result<ColorImage, PdfiumError> {
    let config = PdfRenderConfig::new()
        .set_fixed_size(w as Pixels, h as Pixels)
        .clip(0, 0, w as Pixels, h as Pixels)
        .render_form_data(false)
        .render_annotations(true)
        .set_reverse_byte_order(true) // BGRA → RGBA
        .reset_matrix(PdfMatrix::new(scale, 0.0, 0.0, scale, -(x as f32), -(y as f32)))?;
    let bitmap = page.render_with_config(&config)?;
    let raw = bitmap.as_raw_bytes();
    let (w, h) = (w as usize, h as usize);
    let stride = if h > 0 { raw.len() / h } else { 0 };
    let mut pixels = Vec::with_capacity(w * h);
    for row in raw.chunks_exact(stride.max(1)).take(h) {
        pixels.extend(row[..w * 4].chunks_exact(4).map(|p| Color32::from_rgb(p[0], p[1], p[2])));
    }
    Ok(ColorImage { size: [w, h], pixels })
}
