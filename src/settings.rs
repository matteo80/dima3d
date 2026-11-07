//! Preferenze utente persistenti (file di testo semplice in %APPDATA%\Gestore-WRL).

use std::path::{Path, PathBuf};

pub use crate::renderer::camera::Axes;

/// Numero massimo di file recenti ricordati.
pub const MAX_RECENT: usize = 8;

/// Come il trascinamento ruota il modello.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RotationMode {
    /// Trackball come lo stile OpenInventor di FreeCAD: al centro della finestra il modello
    /// segue il mouse, verso i bordi il movimento diventa un rollio.
    #[default]
    Trackball,
    /// Attorno all'asse verticale del modello: l'orizzonte resta dritto.
    FixedAxis,
}

/// Tema dell'interfaccia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Tema dell'interfaccia
    pub theme: ThemeMode,
    /// Colore di sfondo della viewport (sRGB)
    pub background: [f32; 3],
    /// Colore delle parti senza materiale (sRGB)
    pub default_color: [f32; 4],
    /// Sfondo sfumato: più chiaro in alto, più scuro in basso, attorno a `background`
    pub background_gradient: bool,
    /// Raggruppamento automatico all'apertura: tolleranza (unità del file) e vertici in comune
    pub cluster: crate::clusterer::ClusterConfig,
    /// Sistema di assi del modello per le viste standard e il "su"
    pub axes: Axes,
    /// Modalità di rotazione della vista
    pub rotation: RotationMode,
    /// Moltiplicatore della velocità di rotazione (1 = normale)
    pub rotation_sensitivity: f32,
    /// File aperti di recente (il più recente per primo)
    pub recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::default(),
            background: crate::renderer::DEFAULT_BACKGROUND,
            default_color: crate::renderer::DEFAULT_MESH_COLOR,
            background_gradient: false,
            cluster: crate::clusterer::ClusterConfig::default(),
            axes: Axes::default(),
            rotation: RotationMode::default(),
            rotation_sensitivity: 1.0,
            recent: Vec::new(),
        }
    }
}

/// Limiti della tolleranza del raggruppamento automatico (unità del file).
pub const TOLERANCE_RANGE: std::ops::RangeInclusive<f32> = 0.0001..=10.0;
/// Limiti dei vertici in comune richiesti per unire due pezzi.
pub const MIN_VERTICES_RANGE: std::ops::RangeInclusive<usize> = 1..=100;

/// Limiti del selettore di sensibilità.
pub const SENSITIVITY_RANGE: std::ops::RangeInclusive<f32> = 0.2..=3.0;

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("Gestore-WRL").join("impostazioni.txt"))
}

fn parse_floats<const N: usize>(s: &str) -> Option<[f32; N]> {
    let v: Vec<f32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    (v.len() == N).then(|| {
        let mut out = [0.0; N];
        for (o, x) in out.iter_mut().zip(v) {
            *o = x.clamp(0.0, 1.0);
        }
        out
    })
}

impl Settings {
    /// Aggiunge un file in cima ai recenti (senza duplicati).
    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn from_text(text: &str) -> Self {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "tema" => s.theme = if value == "chiaro" { ThemeMode::Light } else { ThemeMode::Dark },
                "sfondo" => {
                    if let Some(v) = parse_floats::<3>(value) {
                        s.background = v;
                    }
                }
                "colore_predefinito" => {
                    if let Some(v) = parse_floats::<4>(value) {
                        s.default_color = v;
                    }
                }
                // La vecchia chiave "z_alto" (Y/Z) si ignora: si riparte dal sistema veicolo
                "assi" => {
                    s.axes = match value {
                        "z" => Axes::ZUp,
                        "y" => Axes::YUp,
                        _ => Axes::Vehicle,
                    }
                }
                "sfondo_sfumato" => s.background_gradient = value == "1",
                "raggruppamento_tolleranza" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.cluster.tolerance = v.clamp(*TOLERANCE_RANGE.start(), *TOLERANCE_RANGE.end());
                    }
                }
                "raggruppamento_vertici" => {
                    if let Ok(v) = value.parse::<usize>() {
                        s.cluster.min_matching_vertices = v.clamp(*MIN_VERTICES_RANGE.start(), *MIN_VERTICES_RANGE.end());
                    }
                }
                "rotazione" => {
                    s.rotation = if value == "asse_fisso" { RotationMode::FixedAxis } else { RotationMode::Trackball }
                }
                "sensibilita_rotazione" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.rotation_sensitivity = v.clamp(*SENSITIVITY_RANGE.start(), *SENSITIVITY_RANGE.end());
                    }
                }
                "recente" => {
                    if !value.is_empty() && s.recent.len() < MAX_RECENT {
                        s.recent.push(PathBuf::from(value));
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let [r, g, b] = self.background;
        let [dr, dg, db, da] = self.default_color;
        let mut out = format!(
            "tema = {}\nsfondo = {r} {g} {b}\ncolore_predefinito = {dr} {dg} {db} {da}\nsfondo_sfumato = {}\nraggruppamento_tolleranza = {}\nraggruppamento_vertici = {}\nassi = {}\nrotazione = {}\nsensibilita_rotazione = {}\n",
            match self.theme {
                ThemeMode::Dark => "scuro",
                ThemeMode::Light => "chiaro",
            },
            u8::from(self.background_gradient),
            self.cluster.tolerance,
            self.cluster.min_matching_vertices,
            match self.axes {
                Axes::Vehicle => "veicolo",
                Axes::ZUp => "z",
                Axes::YUp => "y",
            },
            match self.rotation {
                RotationMode::Trackball => "openinventor",
                RotationMode::FixedAxis => "asse_fisso",
            },
            self.rotation_sensitivity,
        );
        for p in &self.recent {
            out.push_str(&format!("recente = {}\n", p.display()));
        }
        out
    }

    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Self::from_text(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, self.to_text()) {
            tracing::warn!("Impossibile salvare le impostazioni: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let mut s = Settings {
            theme: ThemeMode::Light,
            background: [0.1, 0.2, 0.3],
            default_color: [0.5, 0.6, 0.7, 1.0],
            background_gradient: true,
            cluster: crate::clusterer::ClusterConfig { tolerance: 0.5, min_matching_vertices: 12 },
            axes: Axes::YUp,
            rotation: RotationMode::FixedAxis,
            rotation_sensitivity: 1.5,
            recent: Vec::new(),
        };
        s.add_recent(Path::new(r"C:\a b\uno.wrl"));
        s.add_recent(Path::new(r"C:\due.wrl"));
        s.add_recent(Path::new(r"C:\a b\uno.wrl"));
        assert_eq!(s.recent[0], PathBuf::from(r"C:\a b\uno.wrl"));
        assert_eq!(s.recent.len(), 2);
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert_eq!(Settings::from_text("garbage"), Settings::default());
    }
}
