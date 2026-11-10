//! Preferenze utente persistenti (file di testo semplice in %APPDATA%\Dima3D).

use std::path::{Path, PathBuf};

pub use crate::renderer::camera::Axes;

/// Numero massimo di file recenti ricordati.
pub const MAX_RECENT: usize = 8;
/// Numero massimo di colori salvati dall'utente.
pub const MAX_SAVED_COLORS: usize = 12;

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

/// Tassellazione degli STEP (come le "Opzioni di mesh" Standard di FreeCAD).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepMesh {
    /// Deviazione di superficie: distanza massima tra mesh e superficie vera, in mm (o frazione
    /// della dimensione di ogni spigolo, se `relative`)
    pub deviation: f32,
    /// Deviazione angolare massima tra triangoli vicini, in gradi
    pub angle: f32,
    /// Deviazione relativa alla dimensione delle facce invece che in mm
    pub relative: bool,
}

impl Default for StepMesh {
    fn default() -> Self {
        // Gli stessi valori predefiniti di FreeCAD
        Self { deviation: 0.1, angle: 30.0, relative: false }
    }
}

impl StepMesh {
    /// Tassellazione veloce di "Visualizza STEP": per guardare il file, non per esportarlo
    pub const VIEW: StepMesh = StepMesh { deviation: 0.5, angle: 30.0, relative: false };
}

/// Come è stato aperto uno STEP: mesh veloce di sola visualizzazione, oppure convertito in mesh
/// con le tolleranze scelte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepLoad {
    View,
    Mesh(StepMesh),
}

impl StepLoad {
    /// Tolleranze passate al convertitore.
    pub fn mesh(self) -> StepMesh {
        match self {
            StepLoad::View => StepMesh::VIEW,
            StepLoad::Mesh(m) => m,
        }
    }

    /// Descrizione breve per l'albero: "visualizzazione" o "mesh 0.1 mm · 30°".
    pub fn describe(self) -> String {
        match self {
            StepLoad::View => "visualizzazione".to_string(),
            StepLoad::Mesh(m) => {
                let unit = if m.relative { " rel." } else { " mm" };
                format!("mesh {}{unit} · {}°", m.deviation, m.angle)
            }
        }
    }
}

/// Limiti della deviazione di superficie (mm, o frazione se relativa) e di quella angolare (°).
pub const STEP_DEVIATION_RANGE: std::ops::RangeInclusive<f32> = 0.001..=100.0;
pub const STEP_ANGLE_RANGE: std::ops::RangeInclusive<f32> = 1.0..=90.0;

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
    /// Export in formato compatto (vedi `ExportOptions::compact`)
    pub export_compact: bool,
    /// Normali scritte nel file esportato (WRL, Open Inventor e HSF)
    pub export_normals: bool,
    /// STL esportato in formato testo invece che binario
    pub export_stl_ascii: bool,
    /// Open Inventor esportato in formato binario invece che testo
    pub export_iv_binary: bool,
    /// IV in metri invece che in millimetri (senza la richiesta di scala della stazione v3)
    pub export_iv_meters: bool,
    /// Rotella al contrario: in avanti allontana invece di avvicinare
    pub invert_zoom: bool,
    /// Colori salvati dall'utente (sRGB), mostrati per primi nel menu Colore
    pub saved_colors: Vec<[f32; 3]>,
    /// Tassellazione all'apertura degli STEP
    pub step: StepMesh,
    /// Sezioni aperte della finestra Posizione: Oggetto, Trasformazione, Cerniere, Applica
    /// offset, Calcola offset
    pub position_sections: [bool; POSITION_SECTIONS],
    /// Verifica gli aggiornamenti su GitHub all'avvio
    pub check_updates: bool,
    /// Versione per cui non avvisare più all'avvio ("Salta questa versione")
    pub skipped_version: String,
}

/// Sezioni richiudibili della finestra Posizione
pub const POSITION_SECTIONS: usize = 5;

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
            export_compact: true,
            export_normals: true,
            export_stl_ascii: false,
            export_iv_binary: false,
            export_iv_meters: false,
            invert_zoom: false,
            saved_colors: Vec::new(),
            step: StepMesh::default(),
            position_sections: [true, true, true, true, false],
            check_updates: true,
            skipped_version: String::new(),
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
    let path = base.join("Dima3D").join("impostazioni.txt");
    // Preferenze salvate con il nome precedente del programma: si portano nella cartella nuova
    let old = base.join("Gestore-WRL").join("impostazioni.txt");
    if !path.exists() && old.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::copy(&old, &path);
    }
    Some(path)
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
    /// Opzioni dell'export scelte dall'utente.
    pub fn export_options(&self) -> crate::exporter::ExportOptions {
        // I WRL si scrivono in metri, come quelli della stazione
        crate::exporter::ExportOptions { compact: self.export_compact, normals: self.export_normals, meters: true }
    }

    /// Aggiunge un colore in cima ai colori salvati (senza duplicati). False se c'era già.
    pub fn save_color(&mut self, color: [f32; 3]) -> bool {
        let same = |c: &[f32; 3]| c.iter().zip(&color).all(|(a, b)| (a - b).abs() < 1.0 / 512.0);
        let existed = self.saved_colors.iter().any(same);
        self.saved_colors.retain(|c| !same(c));
        self.saved_colors.insert(0, color);
        self.saved_colors.truncate(MAX_SAVED_COLORS);
        !existed
    }

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
                "export_compatto" => s.export_compact = value != "0",
                "export_normali" => s.export_normals = value != "0",
                "export_stl_testo" => s.export_stl_ascii = value == "1",
                "export_iv_binario" => s.export_iv_binary = value == "1",
                "export_iv_metri" => s.export_iv_meters = value == "1",
                "zoom_invertito" => s.invert_zoom = value == "1",
                "verifica_aggiornamenti" => s.check_updates = value != "0",
                "versione_saltata" => s.skipped_version = value.to_string(),
                "step_deviazione" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.step.deviation = v.clamp(*STEP_DEVIATION_RANGE.start(), *STEP_DEVIATION_RANGE.end());
                    }
                }
                "step_angolo" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.step.angle = v.clamp(*STEP_ANGLE_RANGE.start(), *STEP_ANGLE_RANGE.end());
                    }
                }
                "step_relativa" => s.step.relative = value == "1",
                "posizione_sezioni" => {
                    for (open, c) in s.position_sections.iter_mut().zip(value.chars()) {
                        *open = c == '1';
                    }
                }
                "colore_salvato" => {
                    if let Some(v) = parse_floats::<3>(value) {
                        if s.saved_colors.len() < MAX_SAVED_COLORS {
                            s.saved_colors.push(v);
                        }
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
            "tema = {}\nsfondo = {r} {g} {b}\ncolore_predefinito = {dr} {dg} {db} {da}\nsfondo_sfumato = {}\nraggruppamento_tolleranza = {}\nraggruppamento_vertici = {}\nassi = {}\nrotazione = {}\nsensibilita_rotazione = {}\nexport_compatto = {}\nexport_normali = {}\nexport_stl_testo = {}\nexport_iv_binario = {}\nexport_iv_metri = {}\nzoom_invertito = {}\nstep_deviazione = {}\nstep_angolo = {}\nstep_relativa = {}\n",
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
            u8::from(self.export_compact),
            u8::from(self.export_normals),
            u8::from(self.export_stl_ascii),
            u8::from(self.export_iv_binary),
            u8::from(self.export_iv_meters),
            u8::from(self.invert_zoom),
            self.step.deviation,
            self.step.angle,
            u8::from(self.step.relative),
        );
        let sections: String = self.position_sections.iter().map(|&o| if o { '1' } else { '0' }).collect();
        out.push_str(&format!("posizione_sezioni = {sections}\n"));
        out.push_str(&format!("verifica_aggiornamenti = {}\n", u8::from(self.check_updates)));
        if !self.skipped_version.is_empty() {
            out.push_str(&format!("versione_saltata = {}\n", self.skipped_version));
        }
        for [r, g, b] in &self.saved_colors {
            out.push_str(&format!("colore_salvato = {r} {g} {b}\n"));
        }
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
        // I test cliccano nella finestra (es. le sezioni della Posizione): non devono toccare le
        // preferenze vere dell'utente
        if cfg!(test) {
            return;
        }
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
            export_compact: false,
            export_normals: false,
            export_stl_ascii: true,
            export_iv_binary: true,
            export_iv_meters: true,
            invert_zoom: true,
            saved_colors: vec![[0.1, 0.2, 0.3], [1.0, 0.5, 0.0]],
            step: StepMesh { deviation: 0.025, angle: 12.5, relative: true },
            position_sections: [false, true, false, true, true],
            check_updates: false,
            skipped_version: "0.3.1".to_string(),
        };
        s.add_recent(Path::new(r"C:\a b\uno.wrl"));
        s.add_recent(Path::new(r"C:\due.wrl"));
        s.add_recent(Path::new(r"C:\a b\uno.wrl"));
        assert_eq!(s.recent[0], PathBuf::from(r"C:\a b\uno.wrl"));
        assert_eq!(s.recent.len(), 2);
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert_eq!(Settings::from_text("garbage"), Settings::default());
    }

    #[test]
    fn saved_colors_are_unique_and_limited() {
        let mut s = Settings::default();
        assert!(s.save_color([1.0, 0.0, 0.0]));
        assert!(s.save_color([0.0, 1.0, 0.0]));
        assert!(!s.save_color([1.0, 0.001, 0.0]), "quasi uguale: non si ripete");
        assert_eq!(s.saved_colors, vec![[1.0, 0.001, 0.0], [0.0, 1.0, 0.0]]);
        for i in 0..20 {
            s.save_color([i as f32 / 20.0, 0.5, 0.5]);
        }
        assert_eq!(s.saved_colors.len(), MAX_SAVED_COLORS);
        assert_eq!(s.saved_colors[0], [19.0 / 20.0, 0.5, 0.5]);
    }
}
