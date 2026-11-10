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
    pub const ALL: [(LabelWall, &'static str); 6] = [
        (LabelWall::Sides, "Pareti laterali"),
        (LabelWall::Left, "Parete sinistra"),
        (LabelWall::Right, "Parete destra"),
        (LabelWall::Entry, "Testata di ingresso"),
        (LabelWall::Exit, "Testata di uscita"),
        (LabelWall::All, "Tutte le pareti"),
    ];

    fn code(self) -> &'static str {
        match self {
            LabelWall::Sides => "laterali",
            LabelWall::Left => "sinistra",
            LabelWall::Right => "destra",
            LabelWall::Entry => "ingresso",
            LabelWall::Exit => "uscita",
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
            label: CabinLabel::default(),
        }
    }
}

impl CabinParams {
    /// Valori in ordine, per le preferenze (la scritta a parte: [`CabinLabel::to_text`]).
    pub fn to_text(&self) -> String {
        let mut v: Vec<String> = self.values().iter().map(|x| x.to_string()).collect();
        v.push(u8::from(self.glass).to_string());
        v.join(" ")
    }

    /// Dal testo di [`to_text`](Self::to_text); `None` se non torna.
    pub fn from_text(text: &str) -> Option<Self> {
        let mut v: Vec<f32> = text.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let glass = v.pop()? != 0.0;
        let mut p = Self { glass, ..Self::default() };
        let fields = p.fields_mut();
        if v.len() != fields.len() {
            return None;
        }
        for (f, x) in fields.into_iter().zip(v) {
            *f = x;
        }
        Some(p)
    }

    fn values(&self) -> Vec<f32> {
        let mut copy = self.clone();
        copy.fields_mut().into_iter().map(|f| *f).collect()
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
    faces: Vec<[u32; 4]>,
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
            self.faces.push(f.map(|i| base + i));
        }
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
        for [a, b, c, d] in &self.faces {
            let _ = writeln!(out, "{a} {b} {c} {d} -1");
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

/// Testo VRML 2.0 della cabina.
pub fn vrml(p: &CabinParams) -> String {
    let mut out = String::from("#VRML V2.0 utf8\n# Cabina generata da Dima 3D (millimetri)\n");
    let (l, half, h, d, t) = (p.length, p.width / 2.0, p.height, p.depth, p.panel);

    // Grigliato: barre piatte con il piano superiore a Z = 0, un pezzo per modulo
    out.push_str("DEF Grigliato Group { children [\n");
    let xs = stations(l, p.grid_module_x);
    let ys = stations(p.width, p.grid_module_y);
    for (i, x) in xs.windows(2).enumerate() {
        for (j, y) in ys.windows(2).enumerate() {
            let (x0, x1, y0, y1) = (x[0], x[1], y[0] - half, y[1] - half);
            let mut bars = Boxes::default();
            let (bw, bt) = (p.bar_width / 2.0, p.bar_thickness);
            // Barre lungo X e barre lungo Y, sui bordi del modulo e ogni `bar_pitch`
            for y in stations(y1 - y0, p.bar_pitch) {
                let y = (y0 + y).clamp(y0 + bw, y1 - bw);
                bars.add([x0, y - bw, -bt], [x1, y + bw, 0.0]);
            }
            for x in stations(x1 - x0, p.bar_pitch) {
                let x = (x0 + x).clamp(x0 + bw, x1 - bw);
                bars.add([x - bw, y0, -bt * 2.0], [x + bw, y1, -bt]);
            }
            bars.write(&mut out, &format!("Modulo_{}_{}", i + 1, j + 1), DARK, 0.0);
        }
    }
    out.push_str("] }\n");

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
        if let Some(old) = replaced {
            self.apply(crate::app::Action::Delete(old));
        }
        let scene = scene(&params);
        if self.scene_graph.is_some() {
            self.add_loaded_file(scene);
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

        app.build_cabin(CabinParams { length: 12000.0, ..CabinParams::default() });
        assert_eq!(cabins(&app), 1, "sostituita, non aggiunta");
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
        let (text, problem) = vrml_with_label(&CabinParams { label, ..CabinParams::default() });
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

    #[test]
    fn params_roundtrip_and_checks() {
        let p = CabinParams { length: 12345.0, glass_to: 2900.0, ..CabinParams::default() };
        assert_eq!(CabinParams::from_text(&p.to_text()), Some(p.clone()));
        let no_glass = CabinParams { glass: false, ..p };
        assert_eq!(CabinParams::from_text(&no_glass.to_text()), Some(no_glass));
        assert_eq!(CabinParams::from_text("1 2 3"), None);
        let wrong = CabinParams { length: 20.0, width: 5.7, bar_pitch: 0.0, entry_width: 9000.0, ..CabinParams::default() };
        let problems = wrong.problems();
        assert!(problems.iter().any(|p| p.contains("passo delle barre")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("ingresso")), "{problems:?}");
    }
}
