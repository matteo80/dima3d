//! Log del programma: `%APPDATA%\Dima3D\dima3d.log` (in debug anche sulla console).
//!
//! Serve a capire perché un file non si apre su un altro PC: aperture, conversioni (con tutto
//! quello che il convertitore scrive), errori e panic. Quando supera `MAX_SIZE` il file diventa
//! `dima3d.old.log` e se ne comincia uno nuovo.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const MAX_SIZE: u64 = 1024 * 1024;

/// Percorso del log (`None` senza `%APPDATA%`).
pub fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("Dima3D").join("dima3d.log"))
}

/// Ora locale per le righe del log.
struct LocalTime;

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let (y, mo, d, h, mi, s) = local_now();
        write!(w, "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
    }
}

#[cfg(windows)]
fn local_now() -> (u16, u16, u16, u16, u16, u16) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(t: *mut SystemTime);
    }
    let mut t = SystemTime::default();
    // SAFETY: GetLocalTime scrive solo nella struttura passata, che ha il layout di SYSTEMTIME
    unsafe { GetLocalTime(&mut t) };
    (t.year, t.month, t.day, t.hour, t.minute, t.second)
}

#[cfg(not(windows))]
fn local_now() -> (u16, u16, u16, u16, u16, u16) {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    (1970, 1, 1, (s / 3600 % 24) as u16, (s / 60 % 60) as u16, (s % 60) as u16)
}

/// RAM del PC e quella libera, in MB (per capire se un file grande può starci).
#[cfg(windows)]
pub fn memory_mb() -> Option<(u64, u64)> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(m: *mut MemoryStatusEx) -> i32;
    }
    let mut m = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: struttura MEMORYSTATUSEX con la lunghezza impostata, come chiede l'API
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some((m.total_phys >> 20, m.avail_phys >> 20))
}

#[cfg(not(windows))]
pub fn memory_mb() -> Option<(u64, u64)> {
    None
}

/// MB scritti in GB, con una cifra decimale e la virgola.
pub fn gigabytes(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1024.0).replace('.', ",")
}

/// Memoria fisica usata da Dima 3D (working set), in MB.
#[cfg(windows)]
pub fn process_memory_mb() -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
    }
    let mut c = ProcessMemoryCounters { cb: std::mem::size_of::<ProcessMemoryCounters>() as u32, ..Default::default() };
    // SAFETY: struttura PROCESS_MEMORY_COUNTERS con la dimensione in cb; GetCurrentProcess è uno
    // pseudo-handle che non va chiuso
    (unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0).then_some(c.working_set_size as u64 >> 20)
}

#[cfg(not(windows))]
pub fn process_memory_mb() -> Option<u64> {
    None
}

/// Apre il log e installa il registratore (e il panic hook che scrive nel log).
pub fn init() {
    let file = path().and_then(|p| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_SIZE) {
            let _ = std::fs::rename(&p, p.with_file_name("dima3d.old.log"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(&p).ok()
    });
    // wgpu_hal ripete avvisi innocui a ogni ridimensionamento della finestra
    let mut filter =
        tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into());
    for quiet in ["wgpu_hal=error", "naga=warn"] {
        if let Ok(d) = quiet.parse() {
            filter = filter.add_directive(d);
        }
    }
    let file_layer = file.map(|mut f| {
        let _ = writeln!(f);
        tracing_subscriber::fmt::layer().with_writer(Mutex::new(f)).with_ansi(false).with_target(false).with_timer(LocalTime)
    });
    let console_layer = cfg!(debug_assertions).then(|| tracing_subscriber::fmt::layer().with_timer(LocalTime));
    tracing_subscriber::registry().with(filter).with(file_layer).with(console_layer).init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("Errore interno (panic): {info}");
        default_hook(info);
    }));
}

/// Apre il log con il programma associato ai .log (di solito il Blocco note).
pub fn open() {
    if let Some(p) = path().filter(|p| p.exists()) {
        let _ = std::process::Command::new("explorer").arg(p).spawn();
    }
}
