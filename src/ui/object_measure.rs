//! «Misura dell'oggetto» (finestra Posizione): l'oggetto del robot si calcola da due file di punti.
//! Nel file **local** ci sono i punti presi sulla matematica in posizione 0, nel file **world**
//! gli stessi punti presi con il robot sul pezzo in cabina, con l'oggetto a 0 (stessi nomi). Il
//! moto rigido che porta i primi sui secondi (minimi quadrati) è l'oggetto; per ogni punto si
//! vede la deviazione che resta. Contano solo X Y Z dei punti.
//!
//! Se nella cartella c'è il file degli oggetti del robot (`_GLOBAL.TID` o simili) si può scegliere
//! l'oggetto attuale per confrontarlo con quello calcolato.

use std::path::{Path, PathBuf};

use glam::{DMat4, DVec3, Vec3};

use super::point_align::{in_a_row, MIN_POINTS};
use crate::robot_program::{decode, parse_point_line, Frame};
use crate::scene_graph::motion::fit_rigid;
use crate::scene_graph::placement::{parse_robot_base, BaseFormat, Pose, RobotBase};

/// Punti letti da un file, nell'ordine in cui sono scritti.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PointFile {
    pub path: PathBuf,
    pub points: Vec<(String, DVec3)>,
}

impl PointFile {
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    fn get(&self, name: &str) -> Option<DVec3> {
        self.points.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, p)| *p)
    }
}

/// Punti in coordinate dell'oggetto (`CONST POINT P1 = Object X Y Z …`), righe commentate escluse.
pub fn parse_point_file(path: PathBuf, text: &str) -> Result<PointFile, String> {
    let mut points: Vec<(String, DVec3)> = Vec::new();
    for line in text.lines() {
        let Some((name, p)) = parse_point_line(line) else { continue };
        let ([x, y, z, ..], Frame::Object) = (p.values.as_slice(), p.frame) else { continue };
        let pos = DVec3::new(*x, *y, *z);
        match points.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            Some(slot) => slot.1 = pos,
            None => points.push((name, pos)),
        }
    }
    if points.is_empty() {
        return Err(format!("Nessun punto Object in {}", path.display()));
    }
    Ok(PointFile { path, points })
}

pub fn load_point_file(path: &Path) -> Result<PointFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_point_file(path.to_path_buf(), &decode(&bytes))
}

/// File compagno: `…L.tid` ↔ `…W.tid` nella stessa cartella (maiuscole o minuscole).
pub fn partner(path: &Path, want_world: bool) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let last = stem.chars().last()?.to_ascii_uppercase();
    let (from, to) = if want_world { ('L', 'W') } else { ('W', 'L') };
    if last != from {
        return None;
    }
    let wanted = format!("{}{}", &stem[..stem.len() - 1], to);
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let ext = path.extension()?.to_str()?;
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.is_file()
            && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(&wanted))
            && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext))
    })
}

/// Oggetti del robot scritti in un file (`CONST BASE NOME = …` o `NOME : OBJECT := (…)`).
pub fn parse_bases(text: &str) -> Vec<RobotBase> {
    text.lines().filter(|l| !l.trim_start().starts_with("--")).filter_map(parse_robot_base).filter(|b| !b.name.is_empty()).collect()
}

/// File degli oggetti vicino ai punti: i `.tid` che iniziano con `_` nella cartella dei punti e
/// nelle cartelle accanto che iniziano con `_` (es. `R1\_GLOBAL.TI\_GLOBAL.TID`).
pub fn find_base_files(points: &Path) -> Vec<PathBuf> {
    let Some(dir) = points.parent() else { return Vec::new() };
    let globals = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d)
            .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        v.retain(|p| {
            p.is_file()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('_'))
                && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("tid"))
        });
        v.sort();
        v
    };
    let mut files = globals(dir);
    if let Some(up) = dir.parent() {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(up)
            .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        dirs.retain(|d| d.is_dir() && d.as_path() != dir && d.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('_')));
        dirs.sort();
        for d in dirs {
            files.extend(globals(&d));
        }
    }
    files
}

/// Una riga della tabella: un punto presente in tutti e due i file.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub local: DVec3,
    pub world: DVec3,
    /// Usato nel calcolo
    pub on: bool,
    /// Distanza tra il punto local portato dall'oggetto e il punto world (mm)
    pub deviation: Option<f64>,
    /// Scostamento dx dy dz = world − oggetto · local (mm)
    pub delta: Option<DVec3>,
    /// Punto world riportato sulla matematica in posizione 0 (oggetto⁻¹ · world)
    pub world_on_part: Option<DVec3>,
}

impl Row {
    /// Esito del singolo punto, dalla sua componente più grande (stesse soglie del calcolo)
    pub fn verdict(&self) -> Option<Verdict> {
        self.delta.map(|d| Verdict::from_max(d.abs().max_element()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectResult {
    pub matrix: DMat4,
    pub pose: Pose,
    /// Statistica sulle componenti dx dy dz dei punti usati (3 valori per punto), in mm:
    /// valore atteso = media con il segno (con i minimi quadrati viene sempre circa 0),
    /// media = media dei valori assoluti, dispersione = media dei quadrati (mm²),
    /// max = componente più grande in valore assoluto.
    pub expected: f64,
    pub mean: f64,
    pub variance: f64,
    pub max: f64,
    /// Punto con la componente più grande
    pub worst: Option<String>,
    pub verdict: Verdict,
}

/// Errore massimo per dire che il calcolo è buono o ancora accettabile (mm).
pub const TOL_OK: f64 = 3.0;
pub const TOL_ACCEPTABLE: f64 = 6.0;

/// Esito del calcolo, dall'errore massimo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Acceptable,
    NotOk,
}

impl Verdict {
    pub fn from_max(max: f64) -> Self {
        if max <= TOL_OK {
            Verdict::Ok
        } else if max <= TOL_ACCEPTABLE {
            Verdict::Acceptable
        } else {
            Verdict::NotOk
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Verdict::Ok => "OK",
            Verdict::Acceptable => "Accettabile",
            Verdict::NotOk => "Non OK",
        }
    }

    pub fn hint(self) -> String {
        match self {
            Verdict::Ok => format!("Tutti gli scostamenti entro {TOL_OK} mm"),
            Verdict::Acceptable => format!("Scostamento massimo tra {TOL_OK} e {TOL_ACCEPTABLE} mm: si può usare, meglio ricontrollare il punto peggiore"),
            Verdict::NotOk => format!(
                "Scostamento oltre {TOL_ACCEPTABLE} mm: riprendere il punto peggiore o escluderlo (servono almeno {MIN_POINTS} punti)"
            ),
        }
    }

    /// Colore (r, g, b) per la finestra e per il report
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Verdict::Ok => (60, 170, 80),
            Verdict::Acceptable => (220, 160, 40),
            Verdict::NotOk => (220, 70, 60),
        }
    }
}

/// Spiegazione dei valori della statistica (finestra e report).
pub const STATS_HELP: [(&str, &str); 4] = [
    (
        "Valore atteso",
        "Media degli scostamenti dx dy dz con il loro segno. Con il calcolo ai minimi quadrati viene sempre circa 0:          è solo un controllo che il calcolo sia centrato.",
    ),
    ("Media degli errori", "Media dei valori assoluti di dx dy dz di tutti i punti usati: di quanto sbaglia in media ogni coordinata."),
    (
        "Dispersione degli errori",
        "Media dei quadrati di dx dy dz (varianza, in mm²). Pesa di più gli scostamenti grandi; la sua radice è lo scarto tipico in mm.",
    ),
    ("Errore massimo", "Il dx, dy o dz più grande in valore assoluto: indica il punto peggiore (in rosso)."),
];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectMeasure {
    pub local: Option<PointFile>,
    pub world: Option<PointFile>,
    /// Punti esclusi dal calcolo (nomi in maiuscolo)
    pub off: Vec<String>,
    /// Oggetti letti dal file degli oggetti, e da quale file
    pub bases: Vec<RobotBase>,
    pub base_file: Option<PathBuf>,
    /// Oggetto attuale scelto per il confronto
    pub base: Option<String>,
    /// Ultimo errore di lettura
    pub error: Option<String>,
    /// Sul 3D i punti world (riportati sulla matematica) invece dei local
    pub show_world: bool,
}

impl ObjectMeasure {
    /// Carica il file local o world; se l'altro manca prova il compagno (`…L` ↔ `…W`) e cerca il
    /// file degli oggetti nella cartella.
    pub fn open(&mut self, path: &Path, world: bool) {
        match load_point_file(path) {
            Ok(f) => {
                self.error = None;
                if world { self.world = Some(f) } else { self.local = Some(f) }
                let other_missing = if world { self.local.is_none() } else { self.world.is_none() };
                if other_missing {
                    if let Some(Ok(f)) = partner(path, !world).map(|p| load_point_file(&p)) {
                        if world { self.local = Some(f) } else { self.world = Some(f) }
                    }
                }
                if self.bases.is_empty() {
                    if let Some(file) = find_base_files(path).into_iter().find(|f| self.read_bases(f).is_ok_and(|n| n > 0)) {
                        self.base_file = Some(file);
                    }
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Legge gli oggetti da un file; ritorna quanti ne ha trovati (se nessuno lascia quelli di prima).
    pub fn read_bases(&mut self, path: &Path) -> Result<usize, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bases = parse_bases(&decode(&bytes));
        if bases.is_empty() {
            return Ok(0);
        }
        if !self.base.as_ref().is_some_and(|b| bases.iter().any(|x| &x.name == b)) {
            self.base = (bases.len() == 1).then(|| bases[0].name.clone());
        }
        self.bases = bases;
        self.base_file = Some(path.to_path_buf());
        Ok(self.bases.len())
    }

    pub fn current_base(&self) -> Option<&RobotBase> {
        self.bases.iter().find(|b| Some(&b.name) == self.base.as_ref())
    }

    pub fn is_on(&self, name: &str) -> bool {
        !self.off.iter().any(|n| n.eq_ignore_ascii_case(name))
    }

    pub fn set_on(&mut self, name: &str, on: bool) {
        self.off.retain(|n| !n.eq_ignore_ascii_case(name));
        if !on {
            self.off.push(name.to_ascii_uppercase());
        }
    }

    /// Punti presenti in tutti e due i file, nell'ordine del file local.
    pub fn rows(&self) -> Vec<Row> {
        let (Some(l), Some(w)) = (&self.local, &self.world) else { return Vec::new() };
        l.points
            .iter()
            .filter_map(|(n, p)| Some(Row { name: n.clone(), local: *p, world: w.get(n)?, on: self.is_on(n), deviation: None, delta: None, world_on_part: None }))
            .collect()
    }

    /// Punti che ci sono in un file solo.
    pub fn unmatched(&self) -> Vec<String> {
        let (Some(l), Some(w)) = (&self.local, &self.world) else { return Vec::new() };
        let mut v: Vec<String> = l.points.iter().filter(|(n, _)| w.get(n).is_none()).map(|(n, _)| n.clone()).collect();
        v.extend(w.points.iter().filter(|(n, _)| l.get(n).is_none()).map(|(n, _)| n.clone()));
        v
    }

    /// Oggetto calcolato e righe con le deviazioni (anche dei punti esclusi, per vederli).
    pub fn compute(&self) -> (Vec<Row>, Result<ObjectResult, String>) {
        let mut rows = self.rows();
        if self.local.is_none() || self.world.is_none() {
            return (rows, Err("Apri il file local e il file world".to_string()));
        }
        let a: Vec<DVec3> = rows.iter().filter(|r| r.on).map(|r| r.local).collect();
        let b: Vec<DVec3> = rows.iter().filter(|r| r.on).map(|r| r.world).collect();
        if a.len() < MIN_POINTS {
            return (rows, Err(format!("Servono almeno {MIN_POINTS} punti presenti in tutti e due i file e accesi (ora {})", a.len())));
        }
        if in_a_row(&a) {
            return (rows, Err("I punti sono tutti in fila: l'oggetto non si può calcolare".to_string()));
        }
        let Some((matrix, _)) = fit_rigid(&a, &b) else { return (rows, Err("Calcolo non riuscito".to_string())) };
        let back = matrix.inverse();
        let (mut sum, mut abs, mut sq, mut max, mut worst, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, None, 0usize);
        for r in &mut rows {
            let d = r.world - matrix.transform_point3(r.local);
            r.delta = Some(d);
            r.world_on_part = Some(back.transform_point3(r.world));
            r.deviation = Some(d.length());
            if r.on {
                for c in d.to_array() {
                    sum += c;
                    abs += c.abs();
                    sq += c * c;
                    n += 1;
                    if c.abs() > max {
                        max = c.abs();
                        worst = Some(r.name.clone());
                    }
                }
            }
        }
        let n = n as f64;
        let pose = pose_of(matrix);
        let verdict = Verdict::from_max(max);
        (rows, Ok(ObjectResult { matrix, pose, expected: sum / n, mean: abs / n, variance: sq / n, max, worst, verdict }))
    }

    /// Riga da incollare nel file del robot: nome e formato dell'oggetto attuale, se scelto.
    pub fn robot_line(&self, pose: Pose) -> String {
        let (name, format) = match self.current_base() {
            Some(b) => (b.name.clone(), b.format),
            None => ("OGGETTO".to_string(), BaseFormat::ConstBase),
        };
        RobotBase { name, pose, format }.to_line()
    }

    /// Nome proposto per il report: quello del file local senza la `L` finale, accanto ai punti.
    pub fn report_path(&self) -> Option<PathBuf> {
        let f = self.local.as_ref().or(self.world.as_ref())?;
        let stem = f.path.file_stem()?.to_string_lossy().into_owned();
        let base = match stem.chars().last() {
            Some('L' | 'l' | 'W' | 'w') if stem.len() > 1 => stem[..stem.len() - 1].to_string(),
            _ => stem,
        };
        Some(f.path.with_file_name(format!("{base}_misura.html")))
    }

    /// Report HTML del calcolo: file usati, oggetto calcolato, statistica con l'esito e scostamenti
    /// dei punti. `date` è già scritta (es. «18/11/2026 07:39:50»).
    pub fn report_html(&self, rows: &[Row], res: &ObjectResult, date: &str) -> String {
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let path = |f: &Option<PointFile>| f.as_ref().map_or("—".to_string(), |f| esc(&f.path.display().to_string()));
        let title = self.report_path().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default();
        let (r, g, b) = res.verdict.rgb();
        let p = &res.pose;
        let mut h = String::new();
        h += "<!DOCTYPE html>\n<html lang=\"it\">\n<head>\n<meta charset=\"utf-8\">\n";
        h += &format!("<title>Misura dell'oggetto {}</title>\n", esc(&title));
        h += "<style>\nbody{font-family:Segoe UI,Arial,sans-serif;margin:24px;color:#222}\n\
              h1{font-size:22px}h2{font-size:18px;margin-top:28px}\n\
              table{border-collapse:collapse;margin:8px 0}td,th{border:1px solid #999;padding:4px 10px}\n\
              th{background:#eee}td.n{text-align:right;font-family:Consolas,monospace}\n\
              tr.worst td{color:#c62828;font-weight:bold}tr.off td{color:#999}\n\
              .esito{display:inline-block;padding:6px 14px;border-radius:4px;color:#fff;font-weight:bold}\n\
              .nota{color:#555;font-size:13px}\n</style>\n</head>\n<body>\n";
        h += "<h1>Misura dell'oggetto</h1>\n<h2>Dati</h2>\n<table>\n";
        h += &format!("<tr><th>Data</th><td>{}</td></tr>\n", esc(date));
        h += &format!("<tr><th>File coordinate world</th><td>{}</td></tr>\n", path(&self.world));
        h += &format!("<tr><th>File coordinate locali</th><td>{}</td></tr>\n", path(&self.local));
        let used = rows.iter().filter(|r| r.on).count();
        h += &format!("<tr><th>Punti usati</th><td>{used} di {}</td></tr>\n</table>\n", rows.len());
        h += "<h2>Oggetto calcolato</h2>\n<p class=\"nota\">world = oggetto · local; rotazione secondo Eulero Z-Y-X (A attorno a X, B attorno a Y, C attorno a Z)</p>\n<table>\n";
        for (k, (t, a)) in ["X", "Y", "Z"].iter().zip(["A", "B", "C"]).enumerate() {
            h += &format!(
                "<tr><th>Traslazione {t} [mm]</th><td class=\"n\">{:.3}</td><th>Rotazione {a} ({t}) [°]</th><td class=\"n\">{:.3}</td></tr>\n",
                p.xyz[k], p.abc[k]
            );
        }
        h += "</table>\n";
        h += &format!("<p><code>{}</code></p>\n", esc(&self.robot_line(*p)));
        if let Some(base) = self.current_base() {
            let d = difference(p, &base.pose);
            h += &format!("<p>Differenza dall'oggetto attuale {}: X {:+.3}  Y {:+.3}  Z {:+.3} mm, A {:+.3}  B {:+.3}  C {:+.3}°</p>\n",
                esc(&base.name), d.xyz.x, d.xyz.y, d.xyz.z, d.abc.x, d.abc.y, d.abc.z);
        }
        h += "<h2>Qualità della trasformazione</h2>\n<table>\n";
        let values = [format!("{:.2}", res.expected), format!("{:.2}", res.mean), format!("{:.2}", res.variance), format!("{:.2}", res.max)];
        let units = ["mm", "mm", "mm²", "mm"];
        for (((name, help), v), u) in STATS_HELP.iter().zip(&values).zip(units) {
            h += &format!("<tr><th>{name} [{u}]</th><td class=\"n\">{v}</td><td class=\"nota\">{}</td></tr>\n", esc(help));
        }
        h += "</table>\n";
        h += &format!(
            "<p><span class=\"esito\" style=\"background:rgb({r},{g},{b})\">{}</span> <span class=\"nota\">{}</span></p>\n",
            res.verdict.text(),
            esc(&res.verdict.hint())
        );
        h += "<h2>Scostamenti dei punti</h2>\n<p class=\"nota\">dx dy dz = punto world − oggetto · punto local [mm]</p>\n";
        h += "<table>\n<tr><th>Punto</th><th>dx</th><th>dy</th><th>dz</th><th>Distanza</th><th>Valido</th></tr>\n";
        for row in rows {
            let class = if !row.on { " class=\"off\"" } else if res.worst.as_ref() == Some(&row.name) { " class=\"worst\"" } else { "" };
            let d = row.delta.unwrap_or(DVec3::ZERO);
            h += &format!(
                "<tr{class}><td>{n}/{n}</td><td class=\"n\">{:.3}</td><td class=\"n\">{:.3}</td><td class=\"n\">{:.3}</td><td class=\"n\">{:.3}</td><td>{}</td></tr>\n",
                d.x,
                d.y,
                d.z,
                row.deviation.unwrap_or(0.0),
                if row.on { "sì" } else { "no" },
                n = esc(&row.name)
            );
        }
        h += "</table>\n";
        h += &format!(
            "<p class=\"nota\">Esito: OK fino a {TOL_OK} mm di errore massimo, accettabile fino a {TOL_ACCEPTABLE} mm. I punti non validi non entrano nel calcolo né nella statistica.</p>\n"
        );
        h += &format!("<p class=\"nota\">{} {}</p>\n</body>\n</html>\n", crate::APP_NAME, env!("CARGO_PKG_VERSION"));
        h
    }
}

/// X Y Z A B C di una matrice calcolata in doppia precisione.
fn pose_of(m: DMat4) -> Pose {
    let r = glam::DMat3::from_mat4(m);
    let abc = crate::scene_graph::placement::euler_abc(r.as_mat3());
    Pose { xyz: m.w_axis.truncate().as_vec3(), abc }
}

/// Differenza (calcolato − attuale) di X Y Z e degli angoli, gli angoli tra -180 e 180.
pub fn difference(computed: &Pose, current: &Pose) -> Pose {
    let wrap = |a: f32| (a + 180.0).rem_euclid(360.0) - 180.0;
    let d = computed.abc - current.abc;
    Pose { xyz: computed.xyz - current.xyz, abc: Vec3::new(wrap(d.x), wrap(d.y), wrap(d.z)) }
}

/// Punti sopra la vista, sulla matematica in posizione 0: i local o i world riportati con
/// l'oggetto calcolato (`world`), con nome, distanza e il colore dell'esito del punto.
pub fn draw_overlay(ui: &egui::Ui, rect: egui::Rect, rows: &[Row], world: bool, to_screen: impl Fn(Vec3) -> Option<egui::Pos2>) {
    let painter = ui.painter_at(rect);
    let font = egui::FontId::proportional(13.0);
    for r in rows {
        let pos = if world { r.world_on_part.unwrap_or(r.local) } else { r.local };
        let Some(s) = to_screen(pos.as_vec3()) else { continue };
        let color = match r.verdict() {
            Some(v) => {
                let (red, g, b) = v.rgb();
                egui::Color32::from_rgb(red, g, b)
            }
            None => egui::Color32::from_rgb(120, 230, 120),
        };
        let c = if r.on { color } else { egui::Color32::from_gray(150).gamma_multiply(0.6) };
        if world {
            // World: anello, per riconoscerli a colpo d'occhio dai local
            painter.circle(s, 5.5, egui::Color32::from_black_alpha(120), egui::Stroke::new(3.0_f32, c));
        } else {
            painter.circle(s, 5.0, c, egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
        }
        let text = match r.deviation {
            Some(d) => format!("{}  {d:.2}", r.name),
            None => r.name.clone(),
        };
        painter.text(s + egui::vec2(1.0, 1.0) + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, &text, font.clone(), egui::Color32::from_black_alpha(160));
        painter.text(s + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, text, font.clone(), c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_graph::placement::rigid;

    fn file(name: &str, points: &[(&str, Vec3)], comment: Option<&str>) -> String {
        let mut s = String::from("--POINT\n");
        for (n, p) in points {
            s += &format!("CONST POINT {n} = Object {} {} {} 90 0 0 0 0 0\n", p.x, p.y, p.z);
        }
        if let Some(c) = comment {
            s += &format!("--CONST POINT {c} = Object 1 2 3 0 0 0 0 0 0\n");
        }
        let _ = name;
        s
    }

    /// Valori inventati: l'oggetto ruotato di 90° attorno a Z e spostato; un punto preso male si
    /// vede nella sua deviazione e, escluso, non conta più.
    #[test]
    fn object_from_local_and_world_points() {
        let object = rigid(Vec3::new(-1200.0, -4100.0, 120.0), Vec3::new(1.5, -0.3, 91.0), Vec3::ZERO);
        let local = [
            ("P1", Vec3::new(4600.0, -860.0, -20.0)),
            ("P2", Vec3::new(4890.0, -480.0, 440.0)),
            ("P3", Vec3::new(4930.0, 0.0, 445.0)),
            ("P4", Vec3::new(4890.0, 480.0, 440.0)),
            ("P5", Vec3::new(4600.0, 860.0, -25.0)),
        ];
        let mut world: Vec<(&str, Vec3)> = local.iter().map(|(n, p)| (*n, object.transform_point3(*p))).collect();
        world[4].1 += Vec3::new(0.0, 0.0, 12.0); // P5 preso male
        world.reverse(); // l'ordine non conta: vanno per nome
        let mut m = ObjectMeasure {
            local: Some(parse_point_file("RIFL.tid".into(), &file("L", &local, Some("P9"))).unwrap()),
            world: Some(parse_point_file("RIFW.tid".into(), &file("W", &world, None)).unwrap()),
            ..Default::default()
        };
        assert_eq!(m.local.as_ref().unwrap().points.len(), 5, "la riga commentata non conta");
        let (rows, res) = m.compute();
        let res = res.unwrap();
        let worst = rows.iter().max_by(|a, b| a.deviation.unwrap().total_cmp(&b.deviation.unwrap())).unwrap();
        assert_eq!(worst.name, "P5");
        assert_eq!(res.worst.as_deref(), Some("P5"));
        // Statistica come nel software di calibrazione: sulle 15 componenti dx dy dz
        let comps: Vec<f64> = rows.iter().flat_map(|r| r.delta.unwrap().to_array()).collect();
        assert!(res.expected.abs() < 1e-6, "minimi quadrati: valore atteso 0, {}", res.expected);
        assert!((res.mean - comps.iter().map(|c| c.abs()).sum::<f64>() / 15.0).abs() < 1e-9);
        assert!((res.variance - comps.iter().map(|c| c * c).sum::<f64>() / 15.0).abs() < 1e-9);
        assert!((res.max - comps.iter().fold(0.0f64, |m, c| m.max(c.abs()))).abs() < 1e-12);
        let p5 = rows.iter().find(|r| r.name == "P5").unwrap();
        assert!(p5.delta.unwrap().z > 3.0 && p5.delta.unwrap().z < 12.0, "dx dy dz = world − oggetto·local: P5 sta più in alto, {:?}", p5.delta);
        assert_eq!(res.verdict, Verdict::Acceptable, "{}", res.max);
        let html = m.report_html(&rows, &res, "18/11/2026 08:00:00");
        assert!(html.contains("<tr class=\"worst\"><td>P5/P5</td>") && html.contains("Accettabile") && html.contains("RIF_misura"), "{html}");
        m.set_on("p5", false);
        let (rows, res) = m.compute();
        let res = res.unwrap();
        assert!(res.max < 1e-2, "senza P5 torna esatto: {}", res.max);
        assert_eq!(res.verdict, Verdict::Ok);
        assert!(rows[0].world_on_part.unwrap().distance(rows[0].local) < 1e-2, "world riportato sulla matematica");
        assert_eq!(rows[0].verdict(), Some(Verdict::Ok));
        assert_eq!(rows[4].verdict(), Some(Verdict::NotOk), "il punto escluso ha ancora il suo esito");
        assert!(rows[4].deviation.unwrap() > 5.0, "la deviazione del punto escluso si vede ancora");
        assert!(res.pose.xyz.distance(Vec3::new(-1200.0, -4100.0, 120.0)) < 1e-2, "{:?}", res.pose);
        assert!(res.pose.abc.distance(Vec3::new(1.5, -0.3, 91.0)) < 1e-3, "{:?}", res.pose);
        m.set_on("P2", false);
        m.set_on("P3", false);
        assert!(m.compute().1.unwrap_err().contains("almeno 4"));
    }

    /// Oggetto attuale dal file degli oggetti: la riga nuova ha lo stesso nome e formato.
    #[test]
    fn current_object_and_robot_line() {
        let bases = parse_bases("--CONST BASE VECCHIO = 1 2 3 0 0 0\nCONST BASE BASE_A = -1000 -4000 100 1 0 90\n");
        assert_eq!(bases.len(), 1);
        let m = ObjectMeasure { base: Some("BASE_A".into()), bases, ..Default::default() };
        let pose = Pose { xyz: Vec3::new(-1100.5, -4000.0, 101.0), abc: Vec3::new(1.5, 0.0, -179.0) };
        assert_eq!(m.robot_line(pose), "CONST BASE BASE_A = -1100.5 -4000 101 1.5 0 -179");
        let d = difference(&pose, &m.current_base().unwrap().pose);
        assert!((d.xyz.x + 100.5).abs() < 1e-3 && (d.abc.z - 91.0).abs() < 1e-3, "{d:?}");
    }

    /// Il file local trova da solo il world (…L ↔ …W) e il file degli oggetti nella cartella accanto.
    #[test]
    fn partner_and_objects_found_in_the_folders() {
        let root = std::env::temp_dir().join(format!("dima3d_misura_{}", std::process::id()));
        let points = root.join("R1").join("MEASURE.ti");
        let globals = root.join("R1").join("_GLOBAL.TI");
        std::fs::create_dir_all(&points).unwrap();
        std::fs::create_dir_all(&globals).unwrap();
        let p = [("P1", Vec3::ZERO), ("P2", Vec3::X * 100.0), ("P3", Vec3::Y * 100.0), ("P4", Vec3::Z * 100.0)];
        std::fs::write(points.join("PEZZOL.tid"), file("L", &p, None)).unwrap();
        std::fs::write(points.join("PEZZOW.TID"), file("W", &p, None)).unwrap();
        std::fs::write(points.join("_GLOBAL.TID"), "").unwrap();
        std::fs::write(globals.join("_GLOBAL.TID"), "CONST BASE BASE_A = 1 2 3 0 0 0\n").unwrap();
        let mut m = ObjectMeasure::default();
        m.open(&points.join("PEZZOL.tid"), false);
        assert!(m.world.as_ref().is_some_and(|w| w.name() == "PEZZOW.TID"), "{m:?}");
        assert_eq!(m.base.as_deref(), Some("BASE_A"), "un solo oggetto: scelto da solo");
        let (_, res) = m.compute();
        assert!(res.unwrap().max < 1e-6);
        std::fs::remove_dir_all(&root).ok();
    }
}
