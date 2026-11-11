//! Cabina di verniciatura generata dalle misure: grigliato, pareti modulari, testate con le
//! aperture di ingresso e uscita, soffitto.
//!
//! Riferimento (coordinate veicolo, millimetri): X lungo la cabina dall'ingresso (X = 0) verso
//! l'uscita, Y = 0 sull'asse, Z = 0 sul piano del grigliato, Z verso l'alto. La forma è quella di una
//! cabina tipica: montanti 100×100 ogni 2 m, pannelli tra i montanti, zoccolo
//! scuro sotto il grigliato, fascia vetrata, grigliato a barre piatte diviso in moduli.
//!
//! La cabina diventa un file VRML 2.0 tenuto in memoria: nell'albero, nell'export e nella
//! finestra Posizione si comporta come un file aperto.

use std::fmt::Write;

/// Pareti su cui scrivere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelWall {
    #[default]
    Sides,
    Left,
    Right,
    Entry,
    Exit,
    /// Le due testate, ingresso e uscita
    Ends,
    All,
}

/// Una parete della cabina, per le scritte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wall {
    Left,
    Right,
    Entry,
    Exit,
}

impl LabelWall {
    pub const ALL: [(LabelWall, &'static str); 7] = [
        (LabelWall::Sides, "Pareti laterali"),
        (LabelWall::Left, "Parete sinistra"),
        (LabelWall::Right, "Parete destra"),
        (LabelWall::Entry, "Testata di ingresso"),
        (LabelWall::Exit, "Testata di uscita"),
        (LabelWall::Ends, "Testate di ingresso e uscita"),
        (LabelWall::All, "Tutte le pareti"),
    ];

    fn code(self) -> &'static str {
        match self {
            LabelWall::Sides => "laterali",
            LabelWall::Left => "sinistra",
            LabelWall::Right => "destra",
            LabelWall::Entry => "ingresso",
            LabelWall::Exit => "uscita",
            LabelWall::Ends => "testate",
            LabelWall::All => "tutte",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().map(|(w, _)| *w).find(|w| w.code() == code)
    }

    fn walls(self) -> &'static [Wall] {
        match self {
            LabelWall::Sides => &[Wall::Left, Wall::Right],
            LabelWall::Left => &[Wall::Left],
            LabelWall::Right => &[Wall::Right],
            LabelWall::Entry => &[Wall::Entry],
            LabelWall::Exit => &[Wall::Exit],
            LabelWall::Ends => &[Wall::Entry, Wall::Exit],
            LabelWall::All => &[Wall::Left, Wall::Right, Wall::Entry, Wall::Exit],
        }
    }
}

/// Scritta in rilievo sulle pareti (es. il nome della cabina). Testo vuoto: nessuna scritta.
#[derive(Debug, Clone, PartialEq)]
pub struct CabinLabel {
    pub text: String,
    /// File del font (TTF/OTF)
    pub font: String,
    /// Altezza delle maiuscole
    pub height: f32,
    /// Rilievo (spessore delle lettere)
    pub depth: f32,
    /// Colore (sRGB, 0–1)
    pub color: [f32; 3],
    pub wall: LabelWall,
    /// Sul lato interno delle pareti invece che su quello esterno
    pub inside: bool,
    /// Spostamento del centro della scritta dal centro della parete, verso destra per chi legge
    pub offset: f32,
    /// Quota della linea di base dal grigliato
    pub z: f32,
}

/// Cartella dei font di Windows.
pub fn fonts_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts")
}

/// Font comuni di Windows (nome mostrato, file), solo quelli presenti.
pub fn common_fonts() -> Vec<(&'static str, std::path::PathBuf)> {
    const LIST: [(&str, &str); 12] = [
        ("Arial", "arial.ttf"),
        ("Arial grassetto", "arialbd.ttf"),
        ("Arial Narrow", "ARIALN.TTF"),
        ("Calibri", "calibri.ttf"),
        ("Calibri grassetto", "calibrib.ttf"),
        ("Segoe UI", "segoeui.ttf"),
        ("Segoe UI grassetto", "segoeuib.ttf"),
        ("Consolas", "consola.ttf"),
        ("Tahoma", "tahoma.ttf"),
        ("Verdana", "verdana.ttf"),
        ("Times New Roman", "times.ttf"),
        ("Impact", "impact.ttf"),
    ];
    let dir = fonts_dir();
    LIST.iter().map(|(name, file)| (*name, dir.join(file))).filter(|(_, p)| p.is_file()).collect()
}

impl Default for CabinLabel {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: fonts_dir().join("arialbd.ttf").to_string_lossy().into_owned(),
            height: 500.0,
            depth: 20.0,
            color: [0.05, 0.25, 0.6],
            wall: LabelWall::Sides,
            inside: false,
            offset: 0.0,
            z: 250.0,
        }
    }
}

impl CabinLabel {
    /// `parete interno altezza rilievo spostamento quota r g b | font | testo`
    pub fn to_text(&self) -> String {
        let [r, g, b] = self.color;
        format!(
            "{} {} {} {} {} {} {r} {g} {b} | {} | {}",
            self.wall.code(),
            u8::from(self.inside),
            self.height,
            self.depth,
            self.offset,
            self.z,
            self.font.replace('|', ""),
            self.text.replace(['\r', '\n'], " ")
        )
    }

    pub fn from_text(text: &str) -> Option<Self> {
        let mut parts = text.splitn(3, '|');
        let numbers: Vec<&str> = parts.next()?.split_whitespace().collect();
        let font = parts.next()?.trim().to_string();
        let label = parts.next()?.trim().to_string();
        let [wall, inside, rest @ ..] = numbers.as_slice() else { return None };
        let v: Vec<f32> = rest.iter().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let [height, depth, offset, z, r, g, b] = v.as_slice() else { return None };
        Some(Self {
            text: label,
            font,
            height: *height,
            depth: *depth,
            color: [*r, *g, *b],
            wall: LabelWall::from_code(wall)?,
            inside: *inside == "1",
            offset: *offset,
            z: *z,
        })
    }
}

/// Misure della cabina, in millimetri.
#[derive(Debug, Clone, PartialEq)]
pub struct CabinParams {
    /// Lunghezza interna (lungo X)
    pub length: f32,
    /// Larghezza interna (lungo Y)
    pub width: f32,
    /// Altezza dal grigliato al soffitto
    pub height: f32,
    /// Profondità del fondo sotto il grigliato (le pareti scendono fin lì)
    pub depth: f32,
    /// Passo dei montanti delle pareti laterali
    pub module: f32,
    /// Lato dei montanti (sezione quadrata)
    pub post: f32,
    /// Spessore dei pannelli, del soffitto e delle testate
    pub panel: f32,
    /// Fascia vetrata nelle pareti laterali
    pub glass: bool,
    /// Quota inferiore e superiore della fascia vetrata (dal grigliato)
    pub glass_from: f32,
    pub glass_to: f32,
    /// Passo delle barre del grigliato (nei due versi)
    pub bar_pitch: f32,
    /// Larghezza e spessore delle barre piatte
    pub bar_width: f32,
    pub bar_thickness: f32,
    /// Moduli del grigliato: lunghezza lungo X e larghezza lungo Y
    pub grid_module_x: f32,
    pub grid_module_y: f32,
    /// Apertura di ingresso (testata a X = 0): larghezza e altezza dal grigliato
    pub entry_width: f32,
    pub entry_height: f32,
    /// Apertura di uscita (testata a X = lunghezza)
    pub exit_width: f32,
    pub exit_height: f32,
    /// Quota del piano del grigliato: 0 = a livello della tavola, negativa = grigliato ribassato
    /// (il robot lavora le parti basse); lo zero della cabina resta al piano della tavola
    pub grid_z: f32,
    /// Canalina lungo la cabina, nel pavimento
    pub channel: bool,
    /// Posizione dell'asse della canalina (Y), larghezza esterna, profondità interna, spessore
    /// delle lamiere e sporgenza del bordo sopra il pavimento
    pub channel_y: f32,
    pub channel_width: f32,
    pub channel_depth: f32,
    pub channel_wall: f32,
    pub channel_lip: f32,
    /// Tavola trasportatore (rulliera dello skid) sull'asse, per tutta la lunghezza
    pub conveyor: bool,
    /// Larghezza della tavola (ingombro dei carter)
    pub conveyor_width: f32,
    /// Distanza tra le due file di ruote (scartamento dei pattini dello skid)
    pub wheel_gauge: f32,
    pub wheel_diameter: f32,
    /// Passo delle ruote lungo X
    pub wheel_pitch: f32,
    /// Sporgenza delle ruote sopra il piano della tavola
    pub wheel_above: f32,
    /// Passo delle gambe del telaio
    pub leg_pitch: f32,
    /// Simbolo nel centro della cabina (sfera e tre assi), esportato con la cabina
    pub center_marker: bool,
    /// Lunghezza degli assi del simbolo
    pub marker_size: f32,
    /// Scritta sulle pareti
    pub label: CabinLabel,
}

impl Default for CabinParams {
    /// Misure tipiche di una cabina di verniciatura.
    fn default() -> Self {
        Self {
            length: 20000.0,
            width: 5700.0,
            height: 3100.0,
            depth: 1250.0,
            module: 2000.0,
            post: 100.0,
            panel: 50.0,
            glass: true,
            glass_from: 1000.0,
            glass_to: 3000.0,
            bar_pitch: 100.0,
            bar_width: 20.0,
            bar_thickness: 10.0,
            grid_module_x: 6000.0,
            grid_module_y: 2000.0,
            entry_width: 3000.0,
            entry_height: 2500.0,
            exit_width: 3000.0,
            exit_height: 2500.0,
            grid_z: 0.0,
            channel: false,
            channel_y: 0.0,
            channel_width: 200.0,
            channel_depth: 300.0,
            channel_wall: 10.0,
            channel_lip: 20.0,
            conveyor: false,
            conveyor_width: 1200.0,
            wheel_gauge: 1010.0,
            wheel_diameter: 184.0,
            wheel_pitch: 1000.0,
            wheel_above: 40.0,
            leg_pitch: 1200.0,
            center_marker: false,
            marker_size: 500.0,
            label: CabinLabel::default(),
        }
    }
}

impl CabinParams {
    /// Valori in ordine, per le preferenze (la scritta a parte: [`CabinLabel::to_text`]).
    /// Le misure della prima versione, la vetrata, poi le misure aggiunte dopo (grigliato,
    /// canalina, tavola) e le due caselle.
    pub fn to_text(&self) -> String {
        let mut copy = self.clone();
        let mut v: Vec<String> = copy.fields_mut().into_iter().map(|x| x.to_string()).collect();
        v.push(u8::from(self.glass).to_string());
        v.extend(copy.extra_fields_mut().into_iter().map(|x| x.to_string()));
        v.push(u8::from(self.channel).to_string());
        v.push(u8::from(self.conveyor).to_string());
        v.push(u8::from(self.center_marker).to_string());
        v.join(" ")
    }

    /// Dal testo di [`to_text`](Self::to_text) (anche quello delle versioni precedenti, senza le
    /// misure aggiunte dopo); `None` se non torna.
    pub fn from_text(text: &str) -> Option<Self> {
        let v: Vec<f32> = text.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let mut p = Self::default();
        let (n, extra) = (p.fields_mut().len(), p.extra_fields_mut().len());
        if v.len() != n + 1 && v.len() != n + 1 + extra + 3 {
            return None;
        }
        for (f, x) in p.fields_mut().into_iter().zip(&v[..n]) {
            *f = *x;
        }
        p.glass = v[n] != 0.0;
        if v.len() > n + 1 {
            for (f, x) in p.extra_fields_mut().into_iter().zip(&v[n + 1..n + 1 + extra]) {
                *f = *x;
            }
            p.channel = v[n + 1 + extra] != 0.0;
            p.conveyor = v[n + 2 + extra] != 0.0;
            p.center_marker = v[n + 3 + extra] != 0.0;
        }
        Some(p)
    }

    fn extra_fields_mut(&mut self) -> Vec<&mut f32> {
        vec![
            &mut self.grid_z,
            &mut self.channel_y,
            &mut self.channel_width,
            &mut self.channel_depth,
            &mut self.channel_wall,
            &mut self.channel_lip,
            &mut self.conveyor_width,
            &mut self.wheel_gauge,
            &mut self.wheel_diameter,
            &mut self.wheel_pitch,
            &mut self.wheel_above,
            &mut self.leg_pitch,
            &mut self.marker_size,
        ]
    }

    fn fields_mut(&mut self) -> Vec<&mut f32> {
        vec![
            &mut self.length,
            &mut self.width,
            &mut self.height,
            &mut self.depth,
            &mut self.module,
            &mut self.post,
            &mut self.panel,
            &mut self.glass_from,
            &mut self.glass_to,
            &mut self.bar_pitch,
            &mut self.bar_width,
            &mut self.bar_thickness,
            &mut self.grid_module_x,
            &mut self.grid_module_y,
            &mut self.entry_width,
            &mut self.entry_height,
            &mut self.exit_width,
            &mut self.exit_height,
        ]
    }

    /// Misure da correggere prima di costruire (vuoto se vanno bene).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let positive = [
            (self.length, "lunghezza"),
            (self.width, "larghezza"),
            (self.height, "altezza"),
            (self.module, "passo dei montanti"),
            (self.post, "montanti"),
            (self.panel, "spessore dei pannelli"),
            (self.bar_pitch, "passo delle barre"),
            (self.bar_width, "larghezza delle barre"),
            (self.bar_thickness, "spessore delle barre"),
            (self.grid_module_x, "moduli del grigliato"),
            (self.grid_module_y, "moduli del grigliato"),
        ];
        for (v, name) in positive {
            if v.is_nan() || v <= 0.0 {
                out.push(format!("{name}: deve essere maggiore di zero"));
            }
        }
        if self.depth < 0.0 {
            out.push("fondo sotto il grigliato: non può essere negativo".into());
        }
        if self.grid_z.is_nan() || self.grid_z > 0.0 || self.grid_z - 2.0 * self.bar_thickness < -self.depth {
            out.push("quota del grigliato: da 0 (a livello) in giù, sopra il fondo della cabina".into());
        }
        if self.channel {
            let w = self.channel_width;
            if w.is_nan() || w <= 2.0 * self.channel_wall || self.channel_depth <= 0.0 || self.channel_wall <= 0.0 {
                out.push("canalina: larghezza, profondità e spessore devono essere maggiori di zero (larghezza più di due spessori)".into());
            }
            if self.channel_y.abs() + w / 2.0 > self.width / 2.0 {
                out.push("canalina: deve stare tra le pareti".into());
            }
            if self.channel_lip < 0.0 {
                out.push("canalina: il bordo non può essere negativo".into());
            }
        }
        if self.center_marker && (self.marker_size.is_nan() || self.marker_size <= 0.0) {
            out.push("simbolo del centro: la misura deve essere maggiore di zero".into());
        }
        if self.conveyor {
            let positive = [
                (self.conveyor_width, "larghezza"),
                (self.wheel_gauge, "scartamento"),
                (self.wheel_diameter, "diametro delle ruote"),
                (self.wheel_pitch, "passo delle ruote"),
                (self.leg_pitch, "passo delle gambe"),
            ];
            for (v, name) in positive {
                if v.is_nan() || v <= 0.0 {
                    out.push(format!("tavola: {name} deve essere maggiore di zero"));
                }
            }
            if self.conveyor_width > self.width {
                out.push("tavola: più larga della cabina".into());
            }
            if self.wheel_gauge + 0.4 * self.wheel_diameter + 100.0 > self.conveyor_width {
                out.push("tavola: le ruote non stanno nella larghezza (scartamento troppo grande)".into());
            }
            if self.wheel_above < 0.0 || self.wheel_above >= self.wheel_diameter {
                out.push("tavola: la sporgenza delle ruote va da 0 al diametro".into());
            }
            if self.wheel_pitch < self.wheel_diameter + 30.0 {
                out.push("tavola: il passo delle ruote deve superare il diametro".into());
            }
        }
        if self.glass && !(0.0 <= self.glass_from && self.glass_from < self.glass_to && self.glass_to <= self.height) {
            out.push("fascia vetrata: deve stare tra il grigliato e il soffitto, dal basso verso l'alto".into());
        }
        for (w, h, name) in [(self.entry_width, self.entry_height, "ingresso"), (self.exit_width, self.exit_height, "uscita")] {
            if w < 0.0 || h < 0.0 || w > self.width || h > self.height {
                out.push(format!("apertura di {name}: deve stare nella testata (larghezza e altezza della cabina)"));
            }
        }
        // Troppi pezzi: misure probabilmente sbagliate (es. metri invece di millimetri)
        let bars = (self.length / self.bar_pitch.max(1.0)) * (self.width / self.bar_pitch.max(1.0));
        if bars > 1.0e6 || self.length / self.module.max(1.0) > 1000.0 {
            out.push("troppi elementi: controlla che le misure siano in millimetri".into());
        }
        let label = &self.label;
        if !label.text.trim().is_empty() {
            if label.height.is_nan() || label.height <= 0.0 {
                out.push("scritta: l'altezza delle lettere deve essere maggiore di zero".into());
            }
            if label.depth.is_nan() || label.depth < 0.0 {
                out.push("scritta: il rilievo non può essere negativo".into());
            }
            if !std::path::Path::new(&label.font).is_file() {
                out.push(format!("scritta: font non trovato ({})", label.font));
            }
        }
        out
    }
}

/// Insieme di parallelepipedi che diventano una sola `Shape`.
#[derive(Default)]
struct Boxes {
    points: Vec<[f32; 3]>,
    faces: Vec<Vec<u32>>,
}

impl Boxes {
    /// Parallelepipedo tra due angoli opposti (si ignorano quelli senza volume).
    fn add(&mut self, a: [f32; 3], b: [f32; 3]) {
        let lo = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let hi = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        if (0..3).any(|i| hi[i] - lo[i] <= 0.01) {
            return;
        }
        let base = self.points.len() as u32;
        for k in 0..8u32 {
            self.points.push([
                if k & 1 != 0 { hi[0] } else { lo[0] },
                if k & 2 != 0 { hi[1] } else { lo[1] },
                if k & 4 != 0 { hi[2] } else { lo[2] },
            ]);
        }
        // Facce viste da fuori in senso antiorario
        for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            self.faces.push(f.map(|i| base + i).to_vec());
        }
    }

    /// Sfera di raggio `r` con centro `c`.
    fn sphere(&mut self, c: [f32; 3], r: f32) {
        const MERIDIANS: u32 = 16;
        const PARALLELS: u32 = 8;
        let base = self.points.len() as u32;
        // Poli e anelli intermedi
        self.points.push([c[0], c[1], c[2] - r]);
        for i in 1..PARALLELS {
            let lat = -std::f32::consts::FRAC_PI_2 + i as f32 / PARALLELS as f32 * std::f32::consts::PI;
            for j in 0..MERIDIANS {
                let lon = j as f32 / MERIDIANS as f32 * std::f32::consts::TAU;
                self.points.push([c[0] + r * lat.cos() * lon.cos(), c[1] + r * lat.cos() * lon.sin(), c[2] + r * lat.sin()]);
            }
        }
        self.points.push([c[0], c[1], c[2] + r]);
        let ring = |i: u32, j: u32| base + 1 + (i - 1) * MERIDIANS + j % MERIDIANS;
        let top = base + 1 + (PARALLELS - 1) * MERIDIANS;
        for j in 0..MERIDIANS {
            self.faces.push(vec![base, ring(1, j + 1), ring(1, j)]);
            for i in 1..PARALLELS - 1 {
                self.faces.push(vec![ring(i, j), ring(i, j + 1), ring(i + 1, j + 1), ring(i + 1, j)]);
            }
            self.faces.push(vec![ring(PARALLELS - 1, j), ring(PARALLELS - 1, j + 1), top]);
        }
    }

    /// Cilindro con l'asse lungo Y da `y0` a `y1`, centro dell'asse in (`x`, `z`).
    fn cylinder_y(&mut self, x: f32, z: f32, radius: f32, y0: f32, y1: f32) {
        const SIDES: u32 = 20;
        if radius <= 0.01 || (y1 - y0).abs() <= 0.01 {
            return;
        }
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        let base = self.points.len() as u32;
        for k in 0..SIDES {
            let a = k as f32 / SIDES as f32 * std::f32::consts::TAU;
            let (px, pz) = (x + radius * a.cos(), z + radius * a.sin());
            self.points.push([px, y0, pz]);
            self.points.push([px, y1, pz]);
        }
        for k in 0..SIDES {
            let (a, b) = (base + 2 * k, base + 2 * ((k + 1) % SIDES));
            self.faces.push(vec![a, a + 1, b + 1, b]);
        }
        // Basi: verso -Y e verso +Y
        self.faces.push((0..SIDES).map(|k| base + 2 * k).collect());
        self.faces.push((0..SIDES).rev().map(|k| base + 2 * k + 1).collect());
    }

    fn write(&self, out: &mut String, name: &str, color: [f32; 3], transparency: f32) {
        if self.faces.is_empty() {
            return;
        }
        let [r, g, b] = color;
        let _ = writeln!(
            out,
            "DEF {name} Shape {{ appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} transparency {transparency} }} }}"
        );
        out.push_str("geometry IndexedFaceSet { solid TRUE creaseAngle 0.5\ncoord Coordinate { point [\n");
        for [x, y, z] in &self.points {
            let _ = writeln!(out, "{} {} {},", round(*x), round(*y), round(*z));
        }
        out.push_str("] }\ncoordIndex [\n");
        for f in &self.faces {
            for i in f {
                let _ = write!(out, "{i} ");
            }
            out.push_str("-1\n");
        }
        out.push_str("] } }\n");
    }
}

/// Decimo di millimetro, senza zeri inutili.
fn round(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

const DARK: [f32; 3] = [0.4, 0.4, 0.4];
const PANEL: [f32; 3] = [0.75, 0.75, 0.75];
const GLASS: [f32; 3] = [0.1, 0.2, 0.3];
const CEILING: [f32; 3] = [0.8, 0.8, 0.8];

/// Posizioni regolari da `0` a `end` con passo `step` (l'ultima è sempre `end`).
fn stations(end: f32, step: f32) -> Vec<f32> {
    let n = (end / step).ceil().max(1.0) as usize;
    (0..=n).map(|k| (k as f32 * step).min(end)).collect::<Vec<_>>().into_iter().fold(Vec::new(), |mut v, x| {
        if v.last().is_none_or(|&l: &f32| x - l > 0.01) {
            v.push(x);
        }
        v
    })
}

const CHANNEL: [f32; 3] = [0.45, 0.3, 0.2];
const FRAME: [f32; 3] = [0.15, 0.3, 0.5];
const WHEEL: [f32; 3] = [0.45, 0.5, 0.5];

/// Parti di `y0..y1` non coperte dalle fasce `busy` (più strette di 1 mm: scartate).
fn free_bands(y0: f32, y1: f32, busy: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut free = vec![(y0, y1)];
    for &(b0, b1) in busy {
        free = free
            .into_iter()
            .flat_map(|(a, b)| if b1 <= a || b0 >= b { vec![(a, b)] } else { vec![(a, b0.max(a)), (b1.min(b), b)] })
            .filter(|(a, b)| b - a > 1.0)
            .collect();
    }
    free
}

/// Grigliato a barre piatte nel rettangolo `x`×`y`, con il piano superiore a `top`: barre lungo X
/// sopra, barre lungo Y sotto, sui bordi e ogni `bar_pitch`.
fn grating(bars: &mut Boxes, p: &CabinParams, [x0, x1]: [f32; 2], [y0, y1]: [f32; 2], top: f32) {
    let (bw, bt) = ((p.bar_width / 2.0).min((y1 - y0) / 2.0), p.bar_thickness);
    for y in stations(y1 - y0, p.bar_pitch) {
        let y = (y0 + y).clamp(y0 + bw, y1 - bw);
        bars.add([x0, y - bw, top - bt], [x1, y + bw, top]);
    }
    let bw = (p.bar_width / 2.0).min((x1 - x0) / 2.0);
    for x in stations(x1 - x0, p.bar_pitch) {
        let x = (x0 + x).clamp(x0 + bw, x1 - bw);
        bars.add([x - bw, y0, top - bt * 2.0], [x + bw, y1, top - bt]);
    }
}

/// Tavola trasportatore sull'asse, per tutta la lunghezza: due file di ruote su assi trasversali,
/// che sporgono dal piano (Z = 0) attraverso le asole dei carter; telaio con longheroni, traversi
/// e gambe fino al fondo; grigliato tra i carter. Con il grigliato ribassato, sponde di lamiera
/// dal grigliato al piano della tavola.
fn conveyor(out: &mut String, p: &CabinParams, channel: Option<(f32, f32)>) {
    let l = p.length;
    let (half, g2, r) = (p.conveyor_width / 2.0, p.wheel_gauge / 2.0, p.wheel_diameter / 2.0);
    let ww = 0.4 * p.wheel_diameter; // larghezza delle ruote
    let axle_z = p.wheel_above - r;
    let slot = (g2 - ww / 2.0 - 8.0, g2 + ww / 2.0 + 8.0); // asola dei carter
    let inner = (g2 - 70.0).min(slot.0 - 20.0).max(0.0); // bordo interno dei carter
    let beam = (slot.1 + 5.0, (slot.1 + 40.0).min(half)); // longheroni
    let (beam_top, beam_bottom) = (-10.0, -10.0 - (r * 2.3).max(120.0));
    let wheel_x: Vec<f32> = {
        let n = (l / p.wheel_pitch).floor().max(0.0) as usize;
        let start = (l - n.saturating_sub(1) as f32 * p.wheel_pitch) / 2.0;
        (0..n).map(|k| start + k as f32 * p.wheel_pitch).filter(|&x| x - r > 0.0 && x + r < l).collect()
    };

    out.push_str("DEF Tavola Group { children [\n");
    let mut wheels = Boxes::default();
    for &x in &wheel_x {
        for sign in [1.0f32, -1.0] {
            let c = sign * g2;
            wheels.cylinder_y(x, axle_z, r, c - ww / 2.0, c + ww / 2.0);
        }
        wheels.cylinder_y(x, axle_z, (r * 0.3).max(10.0), -beam.1, beam.1);
    }
    wheels.write(out, "Ruote", WHEEL, 0.0);

    // Carter: lamiere al piano della tavola, con le asole dove escono le ruote
    let mut covers = Boxes::default();
    let t = 3.0;
    for sign in [1.0f32, -1.0] {
        let y = |a: f32, b: f32| (sign * a, sign * b);
        let (a, b) = y(inner, slot.0);
        covers.add([0.0, a, -t], [l, b, 0.0]);
        let (a, b) = y(slot.1, half);
        covers.add([0.0, a, -t], [l, b, 0.0]);
        // Nella fascia delle asole: tratti tra una ruota e l'altra
        let (a, b) = y(slot.0, slot.1);
        let mut from = 0.0;
        for &x in &wheel_x {
            covers.add([from, a, -t], [x - r - 10.0, b, 0.0]);
            from = x + r + 10.0;
        }
        covers.add([from, a, -t], [l, b, 0.0]);
    }
    covers.write(out, "Carter", PANEL, 0.0);

    // Grigliato tra i carter, al piano della tavola (non dove passa la canalina)
    let mut deck = Boxes::default();
    let busy: Vec<(f32, f32)> = channel.into_iter().collect();
    for x in stations(l, p.grid_module_x).windows(2) {
        for (y0, y1) in free_bands(-inner, inner, &busy) {
            grating(&mut deck, p, [x[0], x[1]], [y0, y1], 0.0);
        }
    }
    deck.write(out, "Grigliato_tavola", DARK, 0.0);

    // Telaio: longheroni sotto i carter, traversi e gambe ogni `leg_pitch`, piastre sul fondo
    let mut frame = Boxes::default();
    for sign in [1.0f32, -1.0] {
        frame.add([0.0, sign * beam.0, beam_bottom], [l, sign * beam.1, beam_top]);
    }
    let leg = 60.0;
    for x in stations(l, p.leg_pitch) {
        let x = x.clamp(75.0, l - 75.0);
        frame.add([x - leg / 2.0, -beam.1, beam_bottom - leg], [x + leg / 2.0, beam.1, beam_bottom]);
        for sign in [1.0f32, -1.0] {
            let y = sign * (beam.1 - leg / 2.0);
            frame.add([x - leg / 2.0, y - leg / 2.0, -p.depth + 15.0], [x + leg / 2.0, y + leg / 2.0, beam_bottom - leg]);
            frame.add([x - 75.0, y - 75.0, -p.depth], [x + 75.0, y + 75.0, -p.depth + 15.0]);
        }
    }
    frame.write(out, "Telaio", FRAME, 0.0);

    // Sponde: dal grigliato ribassato al piano della tavola
    if p.grid_z < 0.0 {
        let mut sides = Boxes::default();
        for sign in [1.0f32, -1.0] {
            sides.add([0.0, sign * half, p.grid_z - 2.0 * p.bar_thickness], [l, sign * (half + t), 0.0]);
        }
        sides.write(out, "Sponde", PANEL, 0.0);
    }
    out.push_str("] }\n");
}

/// Testo VRML 2.0 della cabina.
pub fn vrml(p: &CabinParams) -> String {
    let mut out = String::from("#VRML V2.0 utf8\n# Cabina generata da Dima 3D (millimetri)\n");
    let (l, half, h, d, t) = (p.length, p.width / 2.0, p.height, p.depth, p.panel);

    // Fasce lungo Y occupate da tavola e canalina: lì il grigliato non c'è
    let conveyor_band = p.conveyor.then(|| (-p.conveyor_width / 2.0, p.conveyor_width / 2.0));
    let channel_band = p.channel.then(|| (p.channel_y - p.channel_width / 2.0, p.channel_y + p.channel_width / 2.0));
    let busy: Vec<(f32, f32)> = conveyor_band.into_iter().chain(channel_band).collect();

    // Grigliato: barre piatte con il piano superiore alla quota del grigliato, un pezzo per modulo
    out.push_str("DEF Grigliato Group { children [\n");
    let xs = stations(l, p.grid_module_x);
    let ys = stations(p.width, p.grid_module_y);
    for (i, x) in xs.windows(2).enumerate() {
        for (j, y) in ys.windows(2).enumerate() {
            let mut bars = Boxes::default();
            for (y0, y1) in free_bands(y[0] - half, y[1] - half, &busy) {
                grating(&mut bars, p, [x[0], x[1]], [y0, y1], p.grid_z);
            }
            bars.write(&mut out, &format!("Modulo_{}_{}", i + 1, j + 1), DARK, 0.0);
        }
    }
    out.push_str("] }\n");

    if p.conveyor {
        conveyor(&mut out, p, channel_band);
    }
    if p.center_marker {
        // Centro cabina: sfera e tre assi (X rosso verso l'uscita, Y verde, Z blu in alto)
        let (c, a) = ([l / 2.0, 0.0, 0.0], p.marker_size);
        let w = (a * 0.04).max(2.0);
        out.push_str("DEF Centro_cabina Group { children [\n");
        let mut ball = Boxes::default();
        ball.sphere(c, a * 0.12);
        ball.write(&mut out, "Centro_sfera", [0.95, 0.85, 0.1], 0.0);
        for (name, end, color) in [
            ("Centro_asse_X", [c[0] + a, w, w], [0.9, 0.2, 0.2]),
            ("Centro_asse_Y", [c[0] + w, a, w], [0.2, 0.75, 0.2]),
            ("Centro_asse_Z", [c[0] + w, w, a], [0.2, 0.4, 0.95]),
        ] {
            let mut axis = Boxes::default();
            axis.add([c[0] - w, -w, -w], end);
            axis.write(&mut out, name, color, 0.0);
        }
        out.push_str("] }\n");
    }
    if let Some((y0, y1)) = channel_band {
        // Sul piano della tavola se ci passa dentro, altrimenti su quello del grigliato
        let inside = conveyor_band.is_some_and(|(a, b)| y0 < b && y1 > a);
        let floor = if inside { 0.0 } else { p.grid_z };
        let (t, top, bottom) = (p.channel_wall, floor + p.channel_lip, floor - p.channel_depth);
        let mut c = Boxes::default();
        c.add([0.0, y0, bottom - t], [l, y1, bottom]);
        c.add([0.0, y0, bottom], [l, y0 + t, top]);
        c.add([0.0, y1 - t, bottom], [l, y1, top]);
        c.write(&mut out, "Canalina", CHANNEL, 0.0);
    }

    // Pareti laterali: montanti, zoccolo sotto il grigliato, pannelli, vetrata
    for (side, sign) in [("sinistra", 1.0f32), ("destra", -1.0)] {
        let y_in = sign * half;
        let y_out = sign * (half + t);
        let post_out = sign * (half + p.post);
        let mut posts = Boxes::default();
        let mut base = Boxes::default();
        let mut panels = Boxes::default();
        let mut glass = Boxes::default();
        let xs = stations(l, p.module);
        for &x in &xs {
            let x = x.clamp(p.post / 2.0, l - p.post / 2.0);
            posts.add([x - p.post / 2.0, y_in, -d], [x + p.post / 2.0, post_out, h]);
        }
        base.add([0.0, y_in, -d], [l, y_out, 0.0]);
        if p.glass {
            panels.add([0.0, y_in, 0.0], [l, y_out, p.glass_from]);
            glass.add([0.0, y_in, p.glass_from], [l, y_out, p.glass_to]);
            panels.add([0.0, y_in, p.glass_to], [l, y_out, h]);
        } else {
            panels.add([0.0, y_in, 0.0], [l, y_out, h]);
        }
        let _ = writeln!(out, "DEF Parete_{side} Group {{ children [");
        posts.write(&mut out, &format!("Montanti_{side}"), DARK, 0.0);
        base.write(&mut out, &format!("Zoccolo_{side}"), DARK, 0.0);
        panels.write(&mut out, &format!("Pannelli_{side}"), PANEL, 0.0);
        glass.write(&mut out, &format!("Vetrata_{side}"), GLASS, 0.5);
        out.push_str("] }\n");
    }

    // Testate: parete piena tranne l'apertura, centrata sull'asse e appoggiata al grigliato
    for (name, x0, x1, ow, oh) in [
        ("ingresso", -t, 0.0, p.entry_width, p.entry_height),
        ("uscita", l, l + t, p.exit_width, p.exit_height),
    ] {
        let mut wall = Boxes::default();
        let (outer, ow) = (half + t, ow / 2.0);
        wall.add([x0, -outer, -d], [x1, outer, 0.0]);
        wall.add([x0, ow, 0.0], [x1, outer, h]);
        wall.add([x0, -outer, 0.0], [x1, -ow, h]);
        wall.add([x0, -ow, oh], [x1, ow, h]);
        wall.write(&mut out, &format!("Testata_{name}"), PANEL, 0.0);
    }

    // Soffitto: semitrasparente, per vedere dentro dall'alto
    let mut ceiling = Boxes::default();
    ceiling.add([-t, -half - t, h], [l + t, half + t, h + t]);
    ceiling.write(&mut out, "Soffitto", CEILING, 0.6);
    out
}

/// Come [`vrml`], con le scritte; il secondo valore è il motivo se non si sono potute fare.
pub fn vrml_with_label(p: &CabinParams) -> (String, Option<String>) {
    let mut out = vrml(p);
    let problem = if p.label.text.trim().is_empty() { None } else { label_shapes(p, &mut out).err() };
    (out, problem)
}

/// Scritte in rilievo sulle pareti scelte, leggibili da chi guarda la faccia (esterna o interna).
fn label_shapes(p: &CabinParams, out: &mut String) -> Result<(), String> {
    use glam::Vec3;
    let label = &p.label;
    let bytes = std::fs::read(&label.font).map_err(|e| format!("font non letto: {} ({e})", label.font))?;
    let mesh = crate::text3d::text_mesh(&bytes, label.text.trim(), label.height, label.depth)?;
    let (half, l) = (p.width / 2.0, p.length);
    // Le scritte esterne stanno davanti ai montanti, che sporgono più dei pannelli
    let outer = half + p.panel.max(p.post);
    for &wall in label.wall.walls() {
        // Normale verso l'esterno, punto della faccia esterna e di quella interna (al centro, Z = 0)
        let (normal, outside, inside, name) = match wall {
            Wall::Left => (Vec3::Y, Vec3::new(l / 2.0, outer, 0.0), Vec3::new(l / 2.0, half, 0.0), "sinistra"),
            Wall::Right => (-Vec3::Y, Vec3::new(l / 2.0, -outer, 0.0), Vec3::new(l / 2.0, -half, 0.0), "destra"),
            Wall::Entry => (-Vec3::X, Vec3::new(-p.panel, 0.0, 0.0), Vec3::ZERO, "ingresso"),
            Wall::Exit => (Vec3::X, Vec3::new(l + p.panel, 0.0, 0.0), Vec3::new(l, 0.0, 0.0), "uscita"),
        };
        let (w, origin) = if label.inside { (-normal, inside) } else { (normal, outside) };
        // Destra di chi guarda la faccia: (direzione dello sguardo) × alto
        let u = (-w).cross(Vec3::Z);
        let shift = label.offset - mesh.width / 2.0;
        let mut text = String::new();
        let [r, g, b] = label.color;
        let suffix = if label.inside { "_interna" } else { "" };
        let _ = writeln!(
            text,
            "DEF Scritta_{name}{suffix} Shape {{ appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} }} }}"
        );
        text.push_str("geometry IndexedFaceSet { solid FALSE creaseAngle 0.5\ncoord Coordinate { point [\n");
        for q in &mesh.points {
            let v = origin + u * (q.x + shift) + Vec3::Z * (q.y + label.z) + w * q.z;
            let _ = writeln!(text, "{} {} {},", round(v.x), round(v.y), round(v.z));
        }
        text.push_str("] }\ncoordIndex [\n");
        for f in &mesh.faces {
            for i in f {
                let _ = write!(text, "{i} ");
            }
            text.push_str("-1\n");
        }
        text.push_str("] } }\n");
        out.push_str(&text);
    }
    Ok(())
}

/// Scena della cabina, pronta da aprire o aggiungere come file «Cabina».
pub fn scene(p: &CabinParams) -> crate::scene_graph::SceneGraph {
    let (text, problem) = vrml_with_label(p);
    let mut scene = crate::parser::parse_wrl(text.as_bytes());
    scene.load_warnings.extend(problem.map(|e| format!("scritta non fatta: {e}")));
    // Le misure sono già in millimetri: niente proposte di cambio di unità
    scene.units_mm = true;
    scene.into_file("Cabina".to_string(), None);
    scene.sources[0].cabin = Some(p.clone());
    scene
}

impl crate::app::GestoreApp {
    /// Nodo radice della cabina generata, se ce n'è una aperta (l'ultima).
    pub fn cabin_node(&self) -> Option<u32> {
        self.scene_graph.as_ref()?.open_files().filter(|f| f.cabin.is_some()).last().map(|f| f.node)
    }

    /// Centro della cabina aperta (a metà lunghezza, sull'asse, al piano del grigliato) nella
    /// scena, e la sua matrice di posizione (gli spostamenti fatti con Posizione).
    pub fn cabin_center(&self) -> Option<(glam::Vec3, glam::Mat4)> {
        let scene = self.scene_graph.as_ref()?;
        let source = scene.open_files().filter(|f| f.cabin.is_some()).last()?;
        let p = source.cabin.as_ref()?;
        let m = scene.placement(source.meshes.start);
        Some((m.transform_point3(glam::Vec3::new(p.length / 2.0, 0.0, 0.0)), m))
    }

    /// Costruisce la cabina con le misure date: sostituisce quella generata prima, se c'è.
    pub fn build_cabin(&mut self, params: CabinParams) {
        let problems = params.problems();
        if !problems.is_empty() {
            self.ui_state.status_message = format!("Cabina non costruita: {}", problems.join("; "));
            return;
        }
        if self.ui_state.is_loading || self.project.busy() {
            return;
        }
        let mut settings = self.settings.clone();
        settings.cabin = params.clone();
        self.apply_settings(settings);
        let replaced = self.cabin_node();
        // La cabina nuova va dove era la vecchia (spostata con Posizione o con il centro)
        let placement = replaced.and(self.cabin_center()).map(|(_, m)| m);
        if let Some(old) = replaced {
            self.apply(crate::app::Action::Delete(old));
        }
        let scene = scene(&params);
        if self.scene_graph.is_some() {
            self.add_loaded_file(scene);
            if let (Some(m), Some(node)) = (placement.filter(|m| *m != glam::Mat4::IDENTITY), self.cabin_node()) {
                self.apply(crate::app::Action::Move { nodes: vec![node], matrix: m });
            }
        } else {
            // Scena vuota: la cabina si apre come un file, con lo stesso percorso del caricamento
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = tx.send(crate::parser::LoadEvent::Finished(scene));
            self.project.loading_path = None;
            self.project.loading_adds = false;
            self.ui_state.is_loading = true;
            self.load_rx = Some(rx);
        }
        self.ui_state.status_message = format!(
            "Cabina {}: {} × {} × {} mm",
            if replaced.is_some() { "aggiornata" } else { "costruita" },
            params.length,
            params.width,
            params.height
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(scene: &crate::scene_graph::SceneGraph) -> (glam::Vec3, glam::Vec3) {
        let b = scene.compute_bounding_box();
        (b.min, b.max)
    }

    #[test]
    fn cabin_has_the_declared_size_and_parts() {
        let p = CabinParams::default();
        assert!(p.problems().is_empty(), "{:?}", p.problems());
        let scene = scene(&p);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        let (min, max) = bbox(&scene);
        // Ingombro: testate e pareti fuori dalle misure interne, fondo sotto il grigliato
        assert_eq!(min, glam::Vec3::new(-p.panel, -p.width / 2.0 - p.post, -p.depth));
        assert_eq!(max, glam::Vec3::new(p.length + p.panel, p.width / 2.0 + p.post, p.height + p.panel));
        let names: Vec<String> = (0..scene.nodes.len() as u32).map(|n| scene.display_name(n)).collect();
        for part in ["Grigliato", "Parete_sinistra", "Vetrata_destra", "Testata_ingresso", "Soffitto", "Modulo_1_1"] {
            assert!(names.iter().any(|n| n == part), "manca {part}: {names:?}");
        }
        // Grigliato: 20 m / 6 m → 4 moduli lungo X, 5,7 m / 2 m → 3 lungo Y
        assert!(names.iter().any(|n| n == "Modulo_4_3") && !names.iter().any(|n| n == "Modulo_5_1"));
        assert_eq!(scene.sources[0].name, "Cabina");
    }

    /// L'apertura di ingresso è vuota: un raggio lungo X sull'asse a metà altezza
    /// dell'apertura non tocca la testata.
    #[test]
    fn openings_are_empty() {
        let p = CabinParams { glass: false, ..CabinParams::default() };
        let scene = scene(&p);
        let visible = vec![true; scene.shapes_data.len()];
        // Raggio lungo X da fuori dell'ingresso: cosa colpisce per primo
        let inside = |y: f32, z: f32| {
            let hit = scene.pick(glam::Vec3::new(-1000.0, y, z), glam::Vec3::X, &visible);
            hit.and_then(|m| scene.shapes_data[m as usize].name.clone()).as_deref() == Some("Testata_ingresso")
        };
        assert!(!inside(0.0, p.entry_height / 2.0), "apertura libera");
        assert!(inside(0.0, p.entry_height + 100.0), "sopra l'apertura c'è la parete");
        assert!(inside(p.width / 2.0 - 10.0, 1000.0), "ai lati c'è la parete");
    }

    /// Dal programma: con la scena vuota la cabina si apre come file; «Aggiorna» la sostituisce
    /// (sempre una sola); l'export la scrive.
    #[test]
    fn build_and_update_from_the_app() {
        let mut app = crate::app::GestoreApp::new_headless();
        let ctx = egui::Context::default();
        app.build_cabin(CabinParams::default());
        app.poll_loading(&ctx);
        let cabins = |app: &crate::app::GestoreApp| {
            app.scene_graph.as_ref().map_or(0, |s| s.open_files().filter(|f| f.name == "Cabina").count())
        };
        assert_eq!(cabins(&app), 1, "{}", app.ui_state.status_message);
        assert!(app.cabin_node().is_some());

        let shift = glam::Vec3::new(500.0, -300.0, 100.0);
        app.apply(crate::app::Action::Move { nodes: vec![app.cabin_node().unwrap()], matrix: glam::Mat4::from_translation(shift) });
        app.build_cabin(CabinParams { length: 12000.0, ..CabinParams::default() });
        assert_eq!(cabins(&app), 1, "sostituita, non aggiunta");
        // Resta dove era stata spostata
        assert_eq!(app.cabin_center().unwrap().0, glam::Vec3::new(6000.0, 0.0, 0.0) + shift);
        assert!(app.ui_state.status_message.contains("aggiornata"), "{}", app.ui_state.status_message);
        let scene = app.scene_graph.as_ref().unwrap();
        let node = app.cabin_node().unwrap();
        let length = scene.subtree_bounding_box(node);
        assert_eq!(length.max.x - length.min.x, 12000.0 + 2.0 * CabinParams::default().panel);
        assert_eq!(app.settings.cabin.length, 12000.0, "misure ricordate");

        let path = std::env::temp_dir().join("dima3d_cabina_export.wrl");
        crate::exporter::export_with(scene, &crate::exporter::Scope::Visible, &path, Default::default()).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        let _ = std::fs::remove_file(&path);
        // Solo la cabina aperta: quella sostituita non si esporta
        assert_eq!(again.shapes_data.len(), scene.info[node as usize].shape_count as usize);

        // Misure sbagliate: niente cabina nuova, il motivo nella barra di stato
        app.build_cabin(CabinParams { bar_pitch: 0.0, ..CabinParams::default() });
        assert!(app.ui_state.status_message.starts_with("Cabina non costruita"));
        assert_eq!(cabins(&app), 1);
    }

    /// Salva il VRML della cabina con le misure predefinite:
    /// `GESTORE_CABINA_OUT=cabina.wrl cargo test anteprima_cabina -- --ignored`
    #[test]
    #[ignore]
    fn anteprima_cabina() {
        // GESTORE_CABINA_SCRITTA: testo di prova sulle pareti laterali
        let label = CabinLabel { text: std::env::var("GESTORE_CABINA_SCRITTA").unwrap_or_default(), ..CabinLabel::default() };
        // GESTORE_CABINA_TAVOLA=1: con tavola, canalina a lato e grigliato ribassato
        let full = std::env::var("GESTORE_CABINA_TAVOLA").is_ok();
        let p = CabinParams { label, conveyor: full, channel: full, channel_y: 1800.0, grid_z: if full { -500.0 } else { 0.0 }, ..CabinParams::default() };
        let (text, problem) = vrml_with_label(&p);
        assert!(problem.is_none(), "{problem:?}");
        std::fs::write(std::env::var("GESTORE_CABINA_OUT").expect("GESTORE_CABINA_OUT"), text).unwrap();
    }

    /// Scritta sulle pareti laterali: davanti alla faccia esterna (o interna), centrata a metà
    /// cabina, alla quota e con l'altezza scelte. Font mancante: cabina senza scritta e avviso.
    #[test]
    fn label_sits_on_the_chosen_walls() {
        let font = fonts_dir().join("arial.ttf");
        if !font.is_file() {
            return;
        }
        let label = CabinLabel { text: "Cabina 1".into(), font: font.to_string_lossy().into_owned(), ..CabinLabel::default() };
        let p = CabinParams { label: label.clone(), ..CabinParams::default() };
        assert!(p.problems().is_empty(), "{:?}", p.problems());
        let scene = scene(&p);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        let bounds = |name: &str| {
            let s = scene.shapes_data.iter().find(|s| s.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("manca {name}"));
            s.world_vertices.iter().fold((glam::Vec3::MAX, glam::Vec3::MIN), |(a, b), &v| (a.min(v.into()), b.max(v.into())))
        };
        let outer = p.width / 2.0 + p.post;
        let (lo, hi) = bounds("Scritta_sinistra");
        assert!((lo.y - outer).abs() < 0.2 && (hi.y - outer - label.depth).abs() < 0.2, "fuori dalla parete: {lo} {hi}");
        assert!(((lo.x + hi.x) / 2.0 - p.length / 2.0).abs() < 50.0, "centrata: {lo} {hi}");
        assert!(lo.z > label.z - 30.0 && hi.z < label.z + label.height + 30.0, "quota: {lo} {hi}");
        let (lo, hi) = bounds("Scritta_destra");
        assert!(hi.y < -outer + 0.2 && lo.y > -outer - label.depth - 0.2, "{lo} {hi}");

        // All'interno della parete di uscita, spostata a destra di chi legge (dentro, guardando
        // l'uscita, la destra è verso -Y)
        let inside = CabinLabel { wall: LabelWall::Exit, inside: true, offset: 1000.0, ..label.clone() };
        let scene2 = super::scene(&CabinParams { label: inside, ..CabinParams::default() });
        let s = scene2.shapes_data.iter().find(|s| s.name.as_deref() == Some("Scritta_uscita_interna")).unwrap();
        let (lo, hi) = s.world_vertices.iter().fold((glam::Vec3::MAX, glam::Vec3::MIN), |(a, b), &v| (a.min(v.into()), b.max(v.into())));
        assert!((hi.x - p.length).abs() < 0.2 && (lo.x - (p.length - label.depth)).abs() < 0.2, "{lo} {hi}");
        assert!(((lo.y + hi.y) / 2.0 + 1000.0).abs() < 50.0, "spostata verso -Y: {lo} {hi}");

        // Si legge dritta da fuori: l'asta della «L» sta a sinistra di chi guarda (sulla parete
        // sinistra, guardando verso -Y, la sinistra è +X; sulla destra è -X)
        let l = super::scene(&CabinParams { label: CabinLabel { text: "L".into(), ..label.clone() }, ..CabinParams::default() });
        for (name, stroke_at_max_x) in [("Scritta_sinistra", true), ("Scritta_destra", false)] {
            let s = l.shapes_data.iter().find(|s| s.name.as_deref() == Some(name)).unwrap();
            let xs: Vec<f32> = s.world_vertices.iter().map(|v| v[0]).collect();
            let (min, max) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
            let top: Vec<f32> = s.world_vertices.iter().filter(|v| v[2] > label.z + label.height * 0.8).map(|v| v[0]).collect();
            let stroke = top.iter().sum::<f32>() / top.len() as f32;
            assert_eq!(stroke > (min + max) / 2.0, stroke_at_max_x, "{name}: asta a {stroke}, lettera da {min} a {max}");
        }

        // Sulle due testate: una scritta fuori dall'ingresso e una fuori dall'uscita
        let ends = super::scene(&CabinParams { label: CabinLabel { wall: LabelWall::Ends, ..label.clone() }, ..CabinParams::default() });
        let (lo, _) = part(&ends, "Scritta_ingresso").expect("scritta sull'ingresso");
        assert!(lo.x < -p.panel + 0.2);
        let (_, hi) = part(&ends, "Scritta_uscita").expect("scritta sull'uscita");
        assert!(hi.x > p.length + p.panel - 0.2);
        assert!(part(&ends, "Scritta_sinistra").is_none());
        assert_eq!(CabinLabel::from_text(&CabinLabel { wall: LabelWall::Ends, ..label.clone() }.to_text()).unwrap().wall, LabelWall::Ends);

        let missing = CabinParams { label: CabinLabel { font: "C:\\non\\esiste.ttf".into(), ..label }, ..CabinParams::default() };
        assert!(missing.problems().iter().any(|p| p.contains("font non trovato")));
        let scene3 = super::scene(&missing);
        assert!(scene3.load_warnings.iter().any(|w| w.starts_with("scritta non fatta")), "{:?}", scene3.load_warnings);
    }

    #[test]
    fn label_text_roundtrip() {
        let label = CabinLabel {
            text: "Linea 2 | primer".into(),
            font: "C:\\Fonts con spazi\\font.ttf".into(),
            color: [0.1, 0.2, 0.3],
            wall: LabelWall::All,
            inside: true,
            offset: -250.0,
            ..CabinLabel::default()
        };
        assert_eq!(CabinLabel::from_text(&label.to_text()), Some(label));
        assert_eq!(CabinLabel::from_text("rotto"), None);
    }

    /// Ingombro dei vertici di una parte della cabina.
    fn part(scene: &crate::scene_graph::SceneGraph, name: &str) -> Option<(glam::Vec3, glam::Vec3)> {
        let s = scene.shapes_data.iter().find(|s| s.name.as_deref() == Some(name))?;
        Some(s.world_vertices.iter().fold((glam::Vec3::MAX, glam::Vec3::MIN), |(a, b), &v| (a.min(v.into()), b.max(v.into()))))
    }

    /// Tavola, canalina e grigliato ribassato: ogni parte dove deve stare, il grigliato non passa
    /// sotto la tavola né sopra la canalina.
    #[test]
    fn conveyor_channel_and_lowered_grid() {
        let p = CabinParams { conveyor: true, channel: true, channel_y: 2000.0, grid_z: -500.0, ..CabinParams::default() };
        assert!(p.problems().is_empty(), "{:?}", p.problems());
        let scene = scene(&p);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        for name in ["Ruote", "Carter", "Grigliato_tavola", "Telaio", "Sponde", "Canalina"] {
            assert!(part(&scene, name).is_some(), "manca {name}");
        }
        // Ruote: sporgono di `wheel_above` sopra lo zero, sulle due file
        let (lo, hi) = part(&scene, "Ruote").unwrap();
        assert!((hi.z - p.wheel_above).abs() < 0.5, "{hi}");
        assert!(lo.z < p.wheel_above - p.wheel_diameter + 0.5);
        // Carter e grigliato della tavola a Z = 0, entro la larghezza
        let (lo, hi) = part(&scene, "Carter").unwrap();
        assert_eq!((hi.z, lo.y, hi.y), (0.0, -p.conveyor_width / 2.0, p.conveyor_width / 2.0));
        // Telaio fino al fondo
        assert_eq!(part(&scene, "Telaio").unwrap().0.z, -p.depth);
        // Sponde dal grigliato ribassato al piano della tavola
        let (lo, hi) = part(&scene, "Sponde").unwrap();
        assert_eq!(hi.z, 0.0);
        assert!(lo.z <= p.grid_z - p.bar_thickness);
        // Grigliato alla quota scelta, fuori dalla fascia della tavola e della canalina
        let modules: Vec<_> = scene.shapes_data.iter().filter(|s| s.name.as_deref().is_some_and(|n| n.starts_with("Modulo_"))).collect();
        let band = |v: &[f32; 3], a: f32, b: f32| v[1] > a + 0.5 && v[1] < b - 0.5;
        for m in &modules {
            for v in &m.world_vertices {
                assert!(v[2] <= p.grid_z + 0.01 && v[2] >= p.grid_z - 2.0 * p.bar_thickness - 0.01, "quota {v:?}");
                assert!(!band(v, -p.conveyor_width / 2.0, p.conveyor_width / 2.0), "sotto la tavola {v:?}");
                assert!(!band(v, p.channel_y - p.channel_width / 2.0, p.channel_y + p.channel_width / 2.0), "sulla canalina {v:?}");
            }
        }
        // Canalina sul grigliato ribassato: bordo sopra, fondo sotto
        let (lo, hi) = part(&scene, "Canalina").unwrap();
        assert_eq!(hi.z, p.grid_z + p.channel_lip);
        assert_eq!(lo.z, p.grid_z - p.channel_depth - p.channel_wall);
        assert_eq!((lo.y, hi.y), (1900.0, 2100.0));

        // Canalina dentro la tavola: sul piano della tavola, e il grigliato della tavola si apre
        let q = CabinParams { channel_y: 0.0, ..p.clone() };
        let scene = super::scene(&q);
        assert_eq!(part(&scene, "Canalina").unwrap().1.z, q.channel_lip);
        let deck = scene.shapes_data.iter().find(|s| s.name.as_deref() == Some("Grigliato_tavola")).unwrap();
        assert!(deck.world_vertices.iter().all(|v| v[1].abs() >= q.channel_width / 2.0 - 0.5), "grigliato sopra la canalina");

        // Senza le caselle: niente tavola né canalina, grigliato a livello
        let plain = super::scene(&CabinParams::default());
        assert!(part(&plain, "Ruote").is_none() && part(&plain, "Canalina").is_none());
        // Grigliato più giù del fondo: rifiutato
        let deep = CabinParams { grid_z: -2000.0, ..CabinParams::default() };
        assert!(deep.problems().iter().any(|e| e.contains("quota del grigliato")));
    }

    /// Simbolo del centro: a metà lunghezza, sull'asse, a Z = 0; solo con la casella.
    #[test]
    fn center_marker_sits_in_the_center() {
        let p = CabinParams { center_marker: true, ..CabinParams::default() };
        assert!(p.problems().is_empty());
        let scene = scene(&p);
        let (lo, hi) = part(&scene, "Centro_sfera").expect("sfera");
        let c = (lo + hi) / 2.0;
        assert!((c - glam::Vec3::new(p.length / 2.0, 0.0, 0.0)).length() < 1.0, "{c}");
        let (_, hi) = part(&scene, "Centro_asse_X").unwrap();
        assert!((hi.x - (p.length / 2.0 + p.marker_size)).abs() < 0.5);
        let (_, hi) = part(&scene, "Centro_asse_Z").unwrap();
        assert!((hi.z - p.marker_size).abs() < 0.5);
        assert!(part(&super::scene(&CabinParams::default()), "Centro_sfera").is_none());
    }

    #[test]
    fn params_roundtrip_and_checks() {
        let p = CabinParams { length: 12345.0, glass_to: 2900.0, ..CabinParams::default() };
        assert_eq!(CabinParams::from_text(&p.to_text()), Some(p.clone()));
        let no_glass = CabinParams { glass: false, ..p };
        assert_eq!(CabinParams::from_text(&no_glass.to_text()), Some(no_glass));
        assert_eq!(CabinParams::from_text("1 2 3"), None);
        let full = CabinParams { conveyor: true, channel: true, center_marker: true, marker_size: 700.0, grid_z: -450.0, wheel_pitch: 900.0, channel_y: -1500.0, ..CabinParams::default() };
        assert_eq!(CabinParams::from_text(&full.to_text()), Some(full));
        // Preferenze della versione precedente (18 misure e la vetrata): si leggono ancora
        let old = "20000 5700 3100 1250 2000 100 50 1000 3000 100 20 10 6000 2000 3000 2500 3000 2500 1";
        assert_eq!(CabinParams::from_text(old), Some(CabinParams::default()));
        let wrong = CabinParams { length: 20.0, width: 5.7, bar_pitch: 0.0, entry_width: 9000.0, ..CabinParams::default() };
        let problems = wrong.problems();
        assert!(problems.iter().any(|p| p.contains("passo delle barre")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("ingresso")), "{problems:?}");
    }
}
