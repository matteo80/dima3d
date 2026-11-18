//! «Allinea su misure» (finestra Posizione): un pezzo che sul 3D non è nella posizione vera (es.
//! un supporto del jig) si sposta usando quote prese a mano in cabina con il metro.
//!
//! Ogni misura unisce un punto su un pezzo fermo (il riferimento, es. il paraurti) e un punto
//! sul pezzo da spostare (l'«Oggetto» in cima alla finestra). Si scrive il valore misurato e il
//! pezzo si sposta finché le quote sul 3D sono quelle misurate (minimi quadrati, Levenberg-
//! Marquardt). Una misura può essere la distanza o la sola componente lungo X, Y o Z della cabina.
//!
//! Posizione esatta: almeno [`EXACT_POINTS`] punti sull'oggetto, non in fila, ognuno misurato da
//! almeno [`EXACT_PER_POINT`] punti del riferimento (o assi diversi): ogni punto è fissato come la
//! punta di un treppiede e con tre punti l'oggetto non può più girare. Nessun massimo.
//! Posizione veloce: bastano [`MIN_FULL`] misure ([`MIN_MOVE`] per spostare soltanto); se le
//! misure lasciano una direzione libera il pezzo si sposta il meno possibile e il risultato lo
//! segnala come approssimato.

use glam::{DMat4, DQuat, DVec3, Mat4, Vec3};

use super::measure::MeasurePoint;
use crate::scene_graph::SceneGraph;

/// Misure minime per spostare e ruotare (posizione veloce, approssimata).
pub const MIN_FULL: usize = 4;
/// Posizione esatta: almeno tanti punti diversi sull'oggetto, ognuno misurato da almeno tanti
/// punti fissi del riferimento (nessun massimo).
pub const EXACT_POINTS: usize = 3;
pub const EXACT_PER_POINT: usize = 3;
/// Misure che servono per spostarlo soltanto.
pub const MIN_MOVE: usize = 3;

/// Cosa si è misurato tra i due punti.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Distance,
    X,
    Y,
    Z,
}

impl Kind {
    pub const ALL: [(Kind, &'static str, &'static str); 4] = [
        (Kind::Distance, "Distanza", "Distanza diretta tra i due punti (metro teso da punto a punto)"),
        (Kind::X, "ΔX", "Solo la distanza lungo X della cabina"),
        (Kind::Y, "ΔY", "Solo la distanza lungo Y della cabina"),
        (Kind::Z, "ΔZ", "Solo la distanza lungo Z della cabina (es. altezza con la livella)"),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(k, ..)| *k == self).map_or("", |(_, n, _)| n)
    }

    /// Valore della misura tra due punti (sempre positivo).
    pub fn value(self, d: DVec3) -> f64 {
        match self {
            Kind::Distance => d.length(),
            Kind::X => d.x.abs(),
            Kind::Y => d.y.abs(),
            Kind::Z => d.z.abs(),
        }
    }
}

/// Una quota presa a mano.
#[derive(Debug, Clone, PartialEq)]
pub struct HandMeasure {
    pub name: String,
    pub kind: Kind,
    /// I due punti sul 3D: di solito prima il riferimento, poi il pezzo da spostare (l'ordine non conta)
    pub points: [Option<MeasurePoint>; 2],
    /// Valore preso in cabina (mm); appena presi i punti vale come sul 3D
    pub measured: f32,
    /// Usata nel calcolo
    pub on: bool,
}

impl HandMeasure {
    /// Valore sul 3D adesso.
    pub fn current(&self) -> Option<f32> {
        let [Some(a), Some(b)] = &self.points else { return None };
        Some(self.kind.value((b.pos - a.pos).as_dvec3()) as f32)
    }
}

/// Stato di una misura rispetto al pezzo da spostare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// Manca un punto
    Incomplete,
    /// Nessuno dei due punti è sul pezzo da spostare
    NoMoving,
    /// Tutti e due i punti sono sul pezzo da spostare: la misura non cambia muovendolo
    BothMoving,
    /// Pronta: indice del punto sul pezzo da spostare
    Ready(usize),
}

impl RowState {
    pub fn problem(self) -> Option<&'static str> {
        match self {
            RowState::Incomplete => Some("Mancano i punti"),
            RowState::NoMoving => Some("Nessun punto sul pezzo da spostare (l'Oggetto in cima alla finestra)"),
            RowState::BothMoving => Some("Tutti e due i punti sono sul pezzo da spostare: uno va preso sul riferimento fermo"),
            RowState::Ready(_) => None,
        }
    }
}

/// Risultato del calcolo.
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceFit {
    /// Spostamento del pezzo in coordinate della scena
    pub matrix: Mat4,
    /// Differenza (3D dopo lo spostamento − misurato) di ogni misura; `None` se non è pronta
    pub residuals: Vec<Option<f32>>,
    /// Scarto quadratico medio e massimo delle misure usate
    pub rms: f32,
    pub max: f32,
    /// Le misure non fissano tutte le direzioni: il pezzo si è spostato il meno possibile
    pub free: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DistanceAlign {
    pub rows: Vec<HandMeasure>,
    /// Misura e punto (0 o 1) che si sta prendendo sul 3D
    pub picking: Option<(usize, usize)>,
    /// Il pezzo si sposta soltanto, senza ruotare
    pub only_move: bool,
    /// Numero per il nome della prossima misura
    counter: usize,
}

impl DistanceAlign {
    /// Misure che servono con la scelta attuale.
    pub fn minimum(&self) -> usize {
        if self.only_move { MIN_MOVE } else { MIN_FULL }
    }

    fn push(&mut self, points: [Option<MeasurePoint>; 2]) -> usize {
        self.counter += 1;
        let mut row = HandMeasure { name: format!("M{}", self.counter), kind: Kind::Distance, points, measured: 0.0, on: true };
        row.measured = row.current().unwrap_or(0.0);
        self.rows.push(row);
        self.rows.len() - 1
    }

    /// Nuova misura da prendere sul 3D: si comincia dal punto sul riferimento.
    pub fn add_and_pick(&mut self) {
        let i = self.push([None, None]);
        self.picking = Some((i, 0));
    }

    /// Nuova misura con i due punti già presi con lo strumento Misura.
    pub fn add_from(&mut self, a: MeasurePoint, b: MeasurePoint) {
        self.push([Some(a), Some(b)]);
    }

    /// Riprende i due punti di una misura.
    pub fn repick(&mut self, i: usize) {
        if i < self.rows.len() {
            self.picking = Some((i, 0));
        }
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.rows.len() {
            self.rows.remove(i);
            self.picking = None;
        }
    }

    /// Cambia il tipo di misura: il valore misurato riparte da quello del 3D.
    pub fn set_kind(&mut self, i: usize, kind: Kind) {
        if let Some(r) = self.rows.get_mut(i) {
            if r.kind != kind {
                r.kind = kind;
                r.measured = r.current().unwrap_or(r.measured);
            }
        }
    }

    /// Punto cliccato sul 3D.
    pub fn add_point(&mut self, p: MeasurePoint) {
        let Some((i, k)) = self.picking.filter(|(i, _)| *i < self.rows.len()) else {
            self.picking = None;
            return;
        };
        let row = &mut self.rows[i];
        row.points[k] = Some(p);
        if k == 0 {
            row.points[1] = None;
            self.picking = Some((i, 1));
        } else {
            row.measured = row.current().unwrap_or(row.measured);
            self.picking = None;
        }
    }

    /// Testo per chi sta cliccando.
    pub fn prompt(&self) -> Option<String> {
        let (i, k) = self.picking?;
        let r = self.rows.get(i)?;
        Some(match k {
            0 => format!("{}: clicca il punto sul riferimento fermo", r.name),
            _ => format!("{}: clicca il punto sul pezzo da spostare", r.name),
        })
    }

    /// Annulla la presa in corso; una misura appena aggiunta e ancora senza punti si toglie.
    pub fn stop_picking(&mut self) {
        if let Some((i, _)) = self.picking.take() {
            if self.rows.get(i).is_some_and(|r| r.points.iter().all(Option::is_none)) {
                self.rows.remove(i);
            }
        }
    }

    /// I punti seguono il pezzo su cui sono stati presi (come in «Allinea su punti»).
    pub fn follow(&mut self, scene: &SceneGraph) {
        for p in self.rows.iter_mut().flat_map(|r| r.points.iter_mut()).flatten() {
            let Some(a) = &p.anchor else { continue };
            let same = scene.shapes_data.get(a.mesh as usize).is_some_and(|s| (s.source_start, s.source_end) == a.key);
            if same {
                p.pos = scene.placement(a.mesh).transform_point3(a.local);
            } else {
                p.anchor = None;
            }
        }
    }

    /// Stato di ogni misura; `moving` = mesh del pezzo da spostare, in ordine.
    pub fn states(&self, moving: &[u32]) -> Vec<RowState> {
        let on_moving = |p: &MeasurePoint| p.anchor.is_some_and(|a| moving.binary_search(&a.mesh).is_ok());
        self.rows
            .iter()
            .map(|r| match &r.points {
                [Some(a), Some(b)] => match (on_moving(a), on_moving(b)) {
                    (false, false) => RowState::NoMoving,
                    (true, true) => RowState::BothMoving,
                    (true, false) => RowState::Ready(0),
                    (false, true) => RowState::Ready(1),
                },
                _ => RowState::Incomplete,
            })
            .collect()
    }

    /// Schema delle misure accese e pronte: per ogni punto diverso dell'oggetto, quante misure
    /// diverse ha (punti del riferimento o assi diversi). Punti entro mezzo millimetro = lo stesso.
    pub fn coverage(&self, moving: &[u32]) -> Vec<(Vec3, usize)> {
        let same = |a: Vec3, b: Vec3| a.distance(b) < 0.5;
        // (punto dell'oggetto, misure: punto fermo e tipo)
        let mut points: Vec<(Vec3, Vec<(Vec3, Kind)>)> = Vec::new();
        for (r, s) in self.rows.iter().zip(self.states(moving)) {
            let RowState::Ready(k) = s else { continue };
            let (Some(m), Some(f)) = (r.points[k], r.points[1 - k]) else { continue };
            if !r.on {
                continue;
            }
            let slot = match points.iter().position(|(p, _)| same(*p, m.pos)) {
                Some(i) => i,
                None => {
                    points.push((m.pos, Vec::new()));
                    points.len() - 1
                }
            };
            let list = &mut points[slot].1;
            if !list.iter().any(|(q, kind)| same(*q, f.pos) && *kind == r.kind) {
                list.push((f.pos, r.kind));
            }
        }
        points.into_iter().map(|(p, l)| (p, l.len())).collect()
    }

    /// Spostamento del pezzo che rende le quote sul 3D uguali a quelle misurate.
    pub fn fit(&self, moving: &[u32]) -> Result<DistanceFit, String> {
        let states = self.states(moving);
        // (punto fermo, punto che si muove, tipo, valore misurato)
        let used: Vec<(DVec3, DVec3, Kind, f64)> = self
            .rows
            .iter()
            .zip(&states)
            .filter(|(r, _)| r.on)
            .filter_map(|(r, s)| {
                let RowState::Ready(k) = *s else { return None };
                let m = r.points[k]?.pos.as_dvec3();
                let f = r.points[1 - k]?.pos.as_dvec3();
                Some((f, m, r.kind, r.measured as f64))
            })
            .collect();
        let min = self.minimum();
        if used.len() < min {
            return Err(format!("Servono almeno {min} misure pronte e accese (ora {})", used.len()));
        }
        let (matrix, free) = solve(&used, self.only_move);
        let residuals: Vec<Option<f32>> = self
            .rows
            .iter()
            .zip(&states)
            .map(|(r, s)| {
                let RowState::Ready(k) = *s else { return None };
                let m = matrix.transform_point3(r.points[k]?.pos.as_dvec3());
                let f = r.points[1 - k]?.pos.as_dvec3();
                Some((r.kind.value(m - f) - r.measured as f64) as f32)
            })
            .collect();
        let errors: Vec<f32> = self.rows.iter().zip(&residuals).filter(|(r, _)| r.on).filter_map(|(_, e)| *e).collect();
        let rms = (errors.iter().map(|e| e * e).sum::<f32>() / errors.len().max(1) as f32).sqrt();
        let max = errors.iter().fold(0.0f32, |m, e| m.max(e.abs()));
        Ok(DistanceFit { matrix: matrix.as_mat4(), residuals, rms, max, free })
    }
}

/// Moto rigido (attorno al centro dei punti che si muovono) che porta le misure ai valori voluti.
/// Un piccolo peso sullo spostamento stesso tiene fermo il pezzo nelle direzioni che le misure
/// non fissano (spostamento minimo); il `bool` dice se ce ne sono.
fn solve(used: &[(DVec3, DVec3, Kind, f64)], only_move: bool) -> (DMat4, bool) {
    // Peso dello spostamento rispetto alle misure: 10 mm di spostamento contano come 0,01 mm di misura
    const KEEP: f64 = 1e-3;
    let n = used.len() as f64;
    let center = used.iter().map(|u| u.1).sum::<DVec3>() / n;
    // Rotazione scritta come «mm al bordo»: così tutti i parametri hanno la stessa scala
    let size = used.iter().map(|u| (u.1 - center).length()).fold(1.0f64, f64::max);
    let dof = if only_move { 3 } else { 6 };
    let motion = |x: &[f64]| -> DMat4 {
        let t = DVec3::new(x[0], x[1], x[2]);
        let rot = if dof == 6 { DQuat::from_scaled_axis(DVec3::new(x[3], x[4], x[5]) / size) } else { DQuat::IDENTITY };
        DMat4::from_translation(center + t) * DMat4::from_quat(rot) * DMat4::from_translation(-center)
    };
    let residuals = |x: &[f64]| -> Vec<f64> {
        let m = motion(x);
        let measures = used.iter().map(|(f, p, kind, d)| kind.value(m.transform_point3(*p) - *f) - d);
        measures.chain(x.iter().map(|v| v * KEEP)).collect()
    };
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let jacobian = |x: &[f64], r0: &[f64]| -> Vec<Vec<f64>> {
        let h = 1e-6 * size.max(1.0);
        (0..dof)
            .map(|j| {
                let mut xh = x.to_vec();
                xh[j] += h;
                residuals(&xh).iter().zip(r0).map(|(a, b)| (a - b) / h).collect()
            })
            .collect()
    };
    let mut x = vec![0.0f64; dof];
    let mut r = residuals(&x);
    let mut lambda = 1e-3;
    for _ in 0..200 {
        let jt = jacobian(&x, &r);
        let jtj: Vec<Vec<f64>> = (0..dof).map(|a| (0..dof).map(|b| dot(&jt[a], &jt[b])).collect()).collect();
        let jtr: Vec<f64> = (0..dof).map(|a| dot(&jt[a], &r)).collect();
        let mut a = jtj.clone();
        for (k, row) in a.iter_mut().enumerate() {
            row[k] += lambda * jtj[k][k].max(1e-9);
        }
        let Some(step) = solve_linear(a, jtr.iter().map(|v| -v).collect()) else { break };
        let xn: Vec<f64> = x.iter().zip(&step).map(|(a, b)| a + b).collect();
        let rn = residuals(&xn);
        if cost(&rn) < cost(&r) {
            x = xn;
            r = rn;
            lambda = (lambda / 10.0).max(1e-12);
            if step.iter().map(|s| s * s).sum::<f64>().sqrt() < 1e-9 {
                break;
            }
        } else {
            lambda *= 10.0;
            if lambda > 1e12 {
                break;
            }
        }
    }
    // Misure che non fissano il pezzo: J^T J delle sole misure è (quasi) singolare
    let jt: Vec<Vec<f64>> = jacobian(&x, &r).into_iter().map(|col| col[..used.len()].to_vec()).collect();
    let jtj: Vec<Vec<f64>> = (0..dof).map(|a| (0..dof).map(|b| dot(&jt[a], &jt[b])).collect()).collect();
    (motion(&x), !well_conditioned(jtj))
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Sistema lineare con eliminazione di Gauss e pivot parziale.
#[allow(clippy::needless_range_loop)]
fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for i in c + 1..n {
            let f = a[i][c] / a[c][c];
            for k in c..n {
                a[i][k] -= f * a[c][k];
            }
            b[i] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| a[i][k] * x[k]).sum();
        x[i] = (b[i] - s) / a[i][i];
    }
    Some(x)
}

/// Matrice simmetrica (normalizzata sulla diagonale) lontana dall'essere singolare.
#[allow(clippy::needless_range_loop)]
fn well_conditioned(mut m: Vec<Vec<f64>>) -> bool {
    let n = m.len();
    let d: Vec<f64> = (0..n).map(|k| m[k][k].sqrt()).collect();
    if d.iter().any(|v| *v < 1e-9) {
        return false;
    }
    for i in 0..n {
        for j in 0..n {
            m[i][j] /= d[i] * d[j];
        }
    }
    // Cholesky: un pivot quasi nullo vuol dire una direzione libera
    for c in 0..n {
        let pivot = m[c][c];
        if pivot < 1e-6 {
            return false;
        }
        for i in c + 1..n {
            let f = m[i][c] / pivot;
            for k in c..n {
                m[i][k] -= f * m[c][k];
            }
        }
    }
    true
}

/// Misure sopra la vista: linea tra i due punti, nome, valore sul 3D e misurato.
pub fn draw_overlay(ui: &egui::Ui, rect: egui::Rect, align: &DistanceAlign, to_screen: impl Fn(Vec3) -> Option<egui::Pos2>) {
    if align.rows.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let color = egui::Color32::from_rgb(255, 200, 40);
    let font = egui::FontId::proportional(13.0);
    for (i, r) in align.rows.iter().enumerate() {
        let c = if r.on { color } else { color.gamma_multiply(0.35) };
        let s: Vec<Option<egui::Pos2>> = r.points.iter().map(|p| p.as_ref().and_then(|p| to_screen(p.pos))).collect();
        if let [Some(a), Some(b)] = s.as_slice() {
            painter.line_segment([*a, *b], egui::Stroke::new(3.0_f32, egui::Color32::from_black_alpha(140)));
            painter.line_segment([*a, *b], egui::Stroke::new(1.5_f32, c));
            let now = r.current().unwrap_or(0.0);
            let text = if (now - r.measured).abs() < 0.005 {
                format!("{}  {} {now:.1}", r.name, r.kind.name())
            } else {
                format!("{}  {} {now:.1} → {:.1}", r.name, r.kind.name(), r.measured)
            };
            let mid = a.lerp(*b, 0.5);
            let galley = painter.layout_no_wrap(text, font.clone(), egui::Color32::BLACK);
            let box_rect = egui::Rect::from_center_size(mid, galley.size() + egui::vec2(10.0, 4.0));
            painter.rect_filled(box_rect, 4.0, c);
            painter.galley(box_rect.min + egui::vec2(5.0, 2.0), galley, egui::Color32::BLACK);
        }
        for (k, p) in s.iter().enumerate() {
            let Some(p) = p else { continue };
            let active = align.picking == Some((i, k));
            painter.rect(
                egui::Rect::from_center_size(*p, egui::vec2(9.0, 9.0)),
                0.0,
                c,
                egui::Stroke::new(if active { 2.5_f32 } else { 1.2 }, egui::Color32::BLACK),
                egui::StrokeKind::Middle,
            );
        }
    }
    if let Some(text) = align.prompt() {
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            format!("{text}  ·  Esc per smettere"),
            egui::FontId::proportional(15.0),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::measure::Anchor;

    /// Punto su un pezzo (mesh) con la posizione data.
    fn on(mesh: u32, p: Vec3) -> MeasurePoint {
        MeasurePoint { pos: p, vertex: true, anchor: Some(Anchor { mesh, key: (0, 0), local: p }) }
    }

    /// Il supporto (mesh 1) sul 3D è spostato e ruotato rispetto alla cabina; con 6 quote prese
    /// dal paraurti (mesh 0) torna nella posizione vera.
    #[test]
    fn six_distances_put_the_part_back() {
        let truth = crate::scene_graph::placement::rigid(Vec3::new(40.0, -25.0, 60.0), Vec3::new(2.0, -3.0, 5.0), Vec3::new(0.0, 0.0, 1000.0));
        // Punti sul supporto come sono sul 3D, e dove sono davvero
        let support = [Vec3::new(-100.0, -50.0, 900.0), Vec3::new(120.0, -40.0, 950.0), Vec3::new(0.0, 80.0, 1100.0)];
        let rif = [Vec3::new(-300.0, 0.0, 400.0), Vec3::new(300.0, 0.0, 420.0), Vec3::new(0.0, 500.0, 300.0), Vec3::new(0.0, -400.0, 350.0)];
        let mut align = DistanceAlign::default();
        // Ogni punto del supporto da 3 punti del paraurti; l'ultima misura manca ancora
        let pairs = [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (3, 1), (1, 2), (3, 2)];
        for (b, s) in pairs {
            align.add_from(on(0, rif[b]), on(1, support[s]));
            let real = truth.transform_point3(support[s]);
            align.rows.last_mut().unwrap().measured = real.distance(rif[b]);
        }
        assert_eq!(align.coverage(&[1]).iter().map(|c| c.1).collect::<Vec<_>>(), [3, 3, 2]);
        // La terza misura di support[2] lungo Z, con i punti presi nell'ordine opposto
        align.add_from(on(1, support[2]), on(0, rif[2]));
        align.set_kind(8, Kind::Z);
        align.rows[8].measured = (truth.transform_point3(support[2]).z - rif[2].z).abs();
        let fit = align.fit(&[1]).unwrap();
        for s in support {
            let got = fit.matrix.transform_point3(s);
            assert!(got.distance(truth.transform_point3(s)) < 0.05, "{got} {}", truth.transform_point3(s));
        }
        assert!(fit.max < 0.01 && !fit.free, "{}", fit.max);
        // Il pezzo da spostare sbagliato: nessuna misura pronta
        assert!(align.fit(&[5]).unwrap_err().contains("almeno 4"));
        assert_eq!(align.states(&[0, 1])[0], RowState::BothMoving);
    }

    /// Tre distanze nella stessa direzione non fissano il pezzo (si sposta il meno possibile);
    /// solo traslazione con 3 assi sì.
    #[test]
    fn measures_must_fix_the_part() {
        let mut align = DistanceAlign { only_move: true, ..Default::default() };
        for k in 0..3 {
            align.add_from(on(0, Vec3::new(0.0, k as f32 * 100.0, 0.0)), on(1, Vec3::new(500.0, k as f32 * 100.0, 0.0)));
            align.set_kind(k, Kind::X);
            align.rows[k].measured = 510.0;
        }
        // Tre misure lungo X: si sposta solo in X, il minimo indispensabile
        let fit = align.fit(&[1]).unwrap();
        assert!(fit.free && fit.max < 0.01, "{fit:?}");
        assert!(fit.matrix.w_axis.truncate().distance(Vec3::new(10.0, 0.0, 0.0)) < 1e-2, "{:?}", fit.matrix.w_axis);
        align.set_kind(1, Kind::Y);
        align.set_kind(2, Kind::Z);
        align.rows[1].points[1] = Some(on(1, Vec3::new(500.0, 150.0, 0.0)));
        align.rows[1].measured = 60.0; // ΔY da 50 a 60
        align.rows[2].points[1] = Some(on(1, Vec3::new(500.0, 200.0, 30.0)));
        align.rows[2].measured = 25.0; // ΔZ da 30 a 25
        let fit = align.fit(&[1]).unwrap();
        assert!(!fit.free);
        let t = fit.matrix.w_axis.truncate();
        assert!(t.distance(Vec3::new(10.0, 10.0, -5.0)) < 1e-2, "{t}");
    }

    /// Quattro distanze bastano per una posizione veloce: tornano tutte, il pezzo si sposta poco
    /// e il risultato è segnato come approssimato.
    #[test]
    fn four_distances_quick_position() {
        let truth = crate::scene_graph::placement::rigid(Vec3::new(20.0, -10.0, 15.0), Vec3::new(1.0, 0.0, 2.0), Vec3::new(0.0, 0.0, 1000.0));
        let support = [Vec3::new(-100.0, -50.0, 900.0), Vec3::new(120.0, -40.0, 950.0)];
        let rif = [Vec3::new(-300.0, 0.0, 400.0), Vec3::new(300.0, 0.0, 420.0), Vec3::new(0.0, 500.0, 300.0)];
        let mut align = DistanceAlign::default();
        for (b, s) in [(0, 0), (2, 0), (1, 1), (2, 1)] {
            align.add_from(on(0, rif[b]), on(1, support[s]));
            align.rows.last_mut().unwrap().measured = truth.transform_point3(support[s]).distance(rif[b]);
        }
        let fit = align.fit(&[1]).unwrap();
        assert!(fit.free, "4 distanze non fissano tutto");
        assert!(fit.max < 0.05, "le misure tornano: {}", fit.max);
        let moved = support.iter().map(|s| fit.matrix.transform_point3(*s).distance(*s)).fold(0.0f32, f32::max);
        let needed = support.iter().map(|s| truth.transform_point3(*s).distance(*s)).fold(0.0f32, f32::max);
        assert!(moved <= needed + 0.1, "spostamento minimo: {moved} contro {needed}");
    }

    /// Presa sul 3D: prima il riferimento, poi il pezzo; Esc su una misura vuota la toglie.
    #[test]
    fn picking_two_points() {
        let mut align = DistanceAlign::default();
        align.add_and_pick();
        assert!(align.prompt().unwrap().contains("riferimento"));
        align.add_point(on(0, Vec3::ZERO));
        assert!(align.prompt().unwrap().contains("pezzo da spostare"));
        align.add_point(on(1, Vec3::new(3.0, 4.0, 0.0)));
        assert_eq!(align.picking, None);
        assert_eq!(align.rows[0].measured, 5.0, "il misurato parte dal valore del 3D");
        align.add_and_pick();
        align.stop_picking();
        assert_eq!(align.rows.len(), 1);
        assert_eq!(align.rows[0].name, "M1");
    }
}
