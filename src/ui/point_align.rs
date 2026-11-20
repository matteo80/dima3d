//! «Allinea su punti» (finestra Posizione): la matematica si sposta sui riferimenti presi in
//! cabina. Ogni coppia è un punto del programma del robot (coordinate dell'oggetto, preso sul
//! pezzo vero con lo spitze) e lo stesso punto cliccato sul 3D (agganciato al vertice vicino).
//! Con almeno [`MIN_POINTS`] coppie si trova lo spostamento rigido che le sovrappone meglio
//! (minimi quadrati, `fit_rigid`) e si mostra lo scarto di ogni punto.
//!
//! I punti presi sul 3D seguono il pezzo: dopo «Allinea» la distanza di ogni coppia è lo scarto
//! che resta.

use crate::i18n::tr;
use glam::{DVec3, Mat4, Vec3};

use super::measure::MeasurePoint;
use crate::scene_graph::motion::fit_rigid;
use crate::scene_graph::SceneGraph;

/// Coppie accese che servono per allineare.
pub const MIN_POINTS: usize = 4;

/// Un riferimento: il punto del programma e lo stesso punto sul 3D.
#[derive(Debug, Clone, PartialEq)]
pub struct PointPair {
    pub name: String,
    /// Dove deve andare (coordinate della scena = coordinate dell'oggetto del programma)
    pub target: Vec3,
    /// Punto cliccato sul 3D, legato al pezzo
    pub cad: Option<MeasurePoint>,
    /// Usata nel calcolo
    pub on: bool,
}

/// Risultato del calcolo.
#[derive(Debug, Clone, PartialEq)]
pub struct PointFit {
    /// Spostamento in coordinate della scena
    pub matrix: Mat4,
    /// Scarto di ogni coppia dopo lo spostamento (mm); `None` se la coppia non è completa
    pub residuals: Vec<Option<f32>>,
    /// Scarto quadratico medio e massimo delle coppie usate
    pub rms: f32,
    pub max: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PointAlign {
    pub pairs: Vec<PointPair>,
    /// Coppia di cui si sta prendendo il punto sul 3D
    pub picking: Option<usize>,
    /// Punto del programma scelto nell'elenco
    pub choice: Option<String>,
    /// Punto scritto a mano
    pub manual_name: String,
    pub manual: Vec3,
}

impl PointAlign {
    /// Aggiunge un riferimento (se c'è già un punto con lo stesso nome ne aggiorna le coordinate).
    pub fn add(&mut self, name: &str, target: Vec3) {
        match self.pairs.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name)) {
            Some(p) => p.target = target,
            None => self.pairs.push(PointPair { name: name.to_string(), target, cad: None, on: true }),
        }
    }

    /// Prima coppia senza punto sul 3D a partire da `from` (poi da capo).
    fn next_missing(&self, from: usize) -> Option<usize> {
        let n = self.pairs.len();
        (0..n).map(|k| (from + k) % n).find(|&i| self.pairs[i].cad.is_none())
    }

    /// Inizia a prendere i punti sul 3D: dalla coppia `i`, o dalla prima che non ce l'ha.
    pub fn start_picking(&mut self, i: Option<usize>) {
        self.picking = match i {
            Some(i) if i < self.pairs.len() => Some(i),
            _ => self.next_missing(0),
        };
    }

    /// Punto cliccato sul 3D: va alla coppia in scelta, poi si passa alla prossima senza punto.
    pub fn add_point(&mut self, p: MeasurePoint) {
        let Some(i) = self.picking.filter(|&i| i < self.pairs.len()) else {
            self.picking = None;
            return;
        };
        self.pairs[i].cad = Some(p);
        self.picking = self.next_missing(i + 1);
    }

    /// Testo per chi sta cliccando.
    pub fn prompt(&self) -> Option<String> {
        let i = self.picking?;
        let p = self.pairs.get(i)?;
        Some(trf!("Clicca sul 3D il punto {name} ({n} di {total})", name = p.name, n = i + 1, total = self.pairs.len()))
    }

    /// I punti presi sul 3D seguono il pezzo su cui sono stati presi; se il pezzo non c'è più
    /// (file chiuso) restano dove sono.
    pub fn follow(&mut self, scene: &SceneGraph) {
        for p in self.pairs.iter_mut().filter_map(|p| p.cad.as_mut()) {
            let Some(a) = &p.anchor else { continue };
            let same = scene.shapes_data.get(a.mesh as usize).is_some_and(|s| (s.source_start, s.source_end) == a.key);
            if same {
                p.pos = scene.placement(a.mesh).transform_point3(a.local);
            } else {
                p.anchor = None;
            }
        }
    }

    /// Spostamento che porta i punti del 3D sui punti del programma.
    pub fn fit(&self) -> Result<PointFit, String> {
        let used: Vec<&PointPair> = self.pairs.iter().filter(|p| p.on && p.cad.is_some()).collect();
        if used.len() < MIN_POINTS {
            return Err(trf!("Servono almeno {min} punti presi sul 3D e accesi (ora {n})", min = MIN_POINTS, n = used.len()));
        }
        let cad = |p: &PointPair| p.cad.as_ref().map(|c| c.pos);
        let a: Vec<DVec3> = used.iter().map(|p| cad(p).unwrap().as_dvec3()).collect();
        let b: Vec<DVec3> = used.iter().map(|p| p.target.as_dvec3()).collect();
        let (m, rms) = fit_rigid(&a, &b).ok_or(tr("Calcolo non riuscito"))?;
        if in_a_row(&a) {
            return Err(tr("I punti sul 3D sono tutti in fila: prendine qualcuno fuori dalla linea").to_string());
        }
        let matrix = m.as_mat4();
        let residuals: Vec<Option<f32>> =
            self.pairs.iter().map(|p| cad(p).map(|c| matrix.transform_point3(c).distance(p.target))).collect();
        let max = self
            .pairs
            .iter()
            .zip(&residuals)
            .filter(|(p, _)| p.on)
            .filter_map(|(_, r)| *r)
            .fold(0.0f32, f32::max);
        Ok(PointFit { matrix, residuals, rms: rms as f32, max })
    }
}

/// Punti quasi in fila (o tutti nello stesso posto): la rotazione attorno alla loro retta resta
/// libera e il calcolo non vale.
pub fn in_a_row(a: &[DVec3]) -> bool {
    if a.is_empty() {
        return true;
    }
    let center = a.iter().copied().sum::<DVec3>() / a.len() as f64;
    let spread = a.iter().map(|p| (*p - center).length()).fold(0.0, f64::max);
    let spread_out = a.iter().any(|p| {
        let d = (*p - center).normalize_or_zero();
        a.iter().any(|q| d.cross(*q - center).length() > spread * 0.02)
    });
    spread < 1e-3 || !spread_out
}

/// Punti del programma e del 3D sopra la vista, con la linea che li unisce.
pub fn draw_overlay(ui: &egui::Ui, rect: egui::Rect, align: &PointAlign, to_screen: impl Fn(Vec3) -> Option<egui::Pos2>) {
    if align.pairs.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let target_color = egui::Color32::from_rgb(60, 200, 255);
    let cad_color = egui::Color32::from_rgb(255, 140, 30);
    let font = egui::FontId::proportional(13.0);
    for (i, p) in align.pairs.iter().enumerate() {
        let alpha = if p.on { 1.0 } else { 0.35 };
        let t = to_screen(p.target);
        let c = p.cad.as_ref().and_then(|c| to_screen(c.pos));
        if let (Some(t), Some(c)) = (t, c) {
            painter.add(egui::Shape::dashed_line(&[c, t], egui::Stroke::new(1.5_f32, cad_color.gamma_multiply(alpha)), 5.0, 4.0));
        }
        if let Some(t) = t {
            // Riferimento del programma: mirino
            let stroke = egui::Stroke::new(2.0_f32, target_color.gamma_multiply(alpha));
            painter.circle_stroke(t, 6.0, stroke);
            painter.line_segment([t - egui::vec2(9.0, 0.0), t + egui::vec2(9.0, 0.0)], stroke);
            painter.line_segment([t - egui::vec2(0.0, 9.0), t + egui::vec2(0.0, 9.0)], stroke);
            painter.text(t + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, &p.name, font.clone(), target_color.gamma_multiply(alpha));
        }
        if let Some(c) = c {
            let picking = align.picking == Some(i);
            painter.circle(c, 4.5, cad_color.gamma_multiply(alpha), egui::Stroke::new(if picking { 2.5_f32 } else { 1.5 }, egui::Color32::BLACK));
        }
    }
    if let Some(text) = align.prompt() {
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            format!("{text}  ·  {}", tr("Esc per smettere")),
            egui::FontId::proportional(15.0),
            cad_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(p: Vec3) -> Option<MeasurePoint> {
        Some(MeasurePoint::free(p, true))
    }

    /// Quattro riferimenti spostati e ruotati: lo spostamento trovato li rimette sopra.
    #[test]
    fn four_points_bring_the_part_onto_the_references() {
        let moved = crate::scene_graph::placement::rigid(Vec3::new(12.0, -7.5, 3.0), Vec3::new(0.4, -0.3, 1.2), Vec3::ZERO);
        let cad = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(800.0, 0.0, 0.0), Vec3::new(0.0, 500.0, 0.0), Vec3::new(400.0, 250.0, 300.0)];
        let mut align = PointAlign::default();
        for (k, c) in cad.iter().enumerate() {
            align.add(&format!("RIF_{}", k + 1), moved.transform_point3(*c));
        }
        assert_eq!(align.fit().unwrap_err(), "Servono almeno 4 punti presi sul 3D e accesi (ora 0)");
        align.start_picking(None);
        for c in cad {
            align.add_point(MeasurePoint::free(c, true));
        }
        assert_eq!(align.picking, None, "presi tutti: la scelta finisce");
        let fit = align.fit().unwrap();
        assert!(fit.matrix.abs_diff_eq(moved, 1e-3), "{:?}", fit.matrix);
        assert!(fit.rms < 1e-3 && fit.max < 1e-3);
        // Un punto preso male si vede nel suo scarto; spento, non conta più
        align.pairs[3].cad = pt(cad[3] + Vec3::new(0.0, 0.0, 5.0));
        align.add("RIF_5", moved.transform_point3(Vec3::new(800.0, 500.0, 0.0)));
        align.pairs[4].cad = pt(Vec3::new(800.0, 500.0, 0.0));
        let fit = align.fit().unwrap();
        let worst = (0..5).max_by(|&a, &b| fit.residuals[a].unwrap().total_cmp(&fit.residuals[b].unwrap())).unwrap();
        assert_eq!(worst, 3);
        align.pairs[3].on = false;
        let fit = align.fit().unwrap();
        assert!(fit.max < 1e-3, "senza il punto sbagliato torna esatto");
        assert!(fit.residuals[3].unwrap() > 4.0, "lo scarto del punto spento si vede ancora");
    }

    #[test]
    fn points_in_a_row_are_not_enough() {
        let mut align = PointAlign::default();
        for k in 0..4 {
            let p = Vec3::new(k as f32 * 100.0, 0.0, 0.0);
            align.add(&format!("P{k}"), p + Vec3::Y);
            align.pairs[k].cad = pt(p);
        }
        assert!(align.fit().unwrap_err().contains("in fila"));
    }

    /// Il clic va alla coppia scelta, poi alla prossima senza punto; lo stesso nome aggiorna.
    #[test]
    fn picking_goes_to_the_next_missing_point() {
        let mut align = PointAlign::default();
        for k in 0..3 {
            align.add(&format!("P{k}"), Vec3::ZERO);
        }
        align.add("p1", Vec3::ONE);
        assert_eq!(align.pairs.len(), 3);
        assert_eq!(align.pairs[1].target, Vec3::ONE);
        align.start_picking(Some(1));
        align.add_point(MeasurePoint::free(Vec3::X, true));
        assert_eq!(align.picking, Some(2));
        align.add_point(MeasurePoint::free(Vec3::Y, true));
        assert_eq!(align.picking, Some(0), "riparte dalla prima che manca");
        assert!(align.prompt().unwrap().contains("P0"));
        align.add_point(MeasurePoint::free(Vec3::Z, true));
        assert_eq!(align.picking, None);
        // Ripresa di un punto già preso: solo quello
        align.start_picking(Some(0));
        align.add_point(MeasurePoint::free(Vec3::ONE, true));
        assert_eq!((align.picking, align.pairs[0].cad.as_ref().unwrap().pos), (None, Vec3::ONE));
    }
}
