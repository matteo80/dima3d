//! Cerniera a quattro perni (quadrilatero articolato), come quelle dei cofani motore.
//!
//! * **Cerniera fissa**: la piastra avvitata alla scocca, con due perni F1 e F2.
//! * **Cerniera mobile**: il braccio avvitato al cofano, con due perni M1 e M2.
//! * Due **bielle** uniscono F1–M1 e F2–M2.
//!
//! I quattro perni hanno assi paralleli, quindi il moto avviene nel piano perpendicolare ai
//! perni. Il comando è l'apertura del braccio (e quindi del cofano), cioè la sua rotazione φ:
//! con il braccio ruotato di φ, il vettore M1→M2 è noto, quindi M1 sta sia sul cerchio della
//! biella 1 (centro F1) sia sul cerchio della biella 2 spostato indietro di M1→M2. Delle due
//! intersezioni si tiene quella più vicina alla posizione precedente, procedendo a piccoli passi
//! dalla posizione attuale per restare sullo stesso ramo del meccanismo. Dove i cerchi non si
//! incontrano più il cofano è a fine corsa.

use glam::{DVec2, DVec3, Mat4, Vec3};

/// Posizione del meccanismo, rispetto a quella in cui sono stati scelti i perni.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkState {
    /// Rotazione della biella 1 attorno a F1 (radianti)
    pub theta: f64,
    /// Rotazione del braccio mobile, cioè l'apertura del cofano (radianti)
    pub phi: f64,
    /// Posizione di M1 e M2 nel piano
    pub m1: DVec2,
    pub m2: DVec2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FourBar {
    /// Direzione comune dei perni (unitaria): l'angolo positivo gira con la regola della mano destra
    pub normal: Vec3,
    /// Massimo scarto tra la direzione di un perno e quella comune (gradi)
    pub skew_deg: f32,
    origin: DVec3,
    u: DVec3,
    v: DVec3,
    f1: DVec2,
    f2: DVec2,
    m1: DVec2,
    m2: DVec2,
    /// Lunghezze: biella 1, biella 2, braccio (M1–M2)
    lengths: [f64; 3],
    pub state: LinkState,
}

/// Passo massimo dell'apertura nella ricerca (0,25°)
const STEP: f64 = 0.25 * std::f64::consts::PI / 180.0;

fn angle(p: DVec2) -> f64 {
    p.y.atan2(p.x)
}

fn rotate(p: DVec2, a: f64) -> DVec2 {
    let (s, c) = a.sin_cos();
    DVec2::new(c * p.x - s * p.y, s * p.x + c * p.y)
}

/// Porta `a` vicino a `reference` aggiungendo giri interi.
fn unwrap_near(a: f64, reference: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    a - ((a - reference) / tau).round() * tau
}

/// Le due intersezioni dei cerchi (c0, r0) e (c1, r1).
fn circles(c0: DVec2, r0: f64, c1: DVec2, r1: f64) -> Option<[DVec2; 2]> {
    let d = c1 - c0;
    let dist = d.length();
    if dist < 1e-9 || dist > r0 + r1 + 1e-9 || dist < (r0 - r1).abs() - 1e-9 {
        return None;
    }
    let a = (r0 * r0 - r1 * r1 + dist * dist) / (2.0 * dist);
    let h = (r0 * r0 - a * a).max(0.0).sqrt();
    let base = c0 + d * (a / dist);
    let perp = DVec2::new(-d.y, d.x) / dist;
    Some([base + perp * h, base - perp * h])
}

/// I segmenti a–b e c–d si incrociano.
fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(a, b, c), side(a, b, d));
    let (d3, d4) = (side(c, d, a), side(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

impl FourBar {
    /// Dai quattro perni, ognuno come (centro, direzione dell'asse) di un foro: F1, F2, M1, M2.
    pub fn from_pins(pins: [(Vec3, Vec3); 4]) -> Result<FourBar, String> {
        let first = pins[0].1.as_dvec3().normalize_or_zero();
        let mut sum = DVec3::ZERO;
        for (_, n) in &pins {
            let n = n.as_dvec3().normalize_or_zero();
            sum += if n.dot(first) < 0.0 { -n } else { n };
        }
        let normal = sum.normalize_or_zero();
        if normal == DVec3::ZERO {
            return Err("Direzione dei perni non valida".to_string());
        }
        let skew = pins
            .iter()
            .map(|(_, n)| n.as_dvec3().normalize_or_zero().dot(normal).abs().clamp(0.0, 1.0).acos().to_degrees())
            .fold(0.0, f64::max);
        if skew > 15.0 {
            return Err(format!(
                "I perni non sono paralleli (scarto {skew:.0}°): controlla di aver cliccato sul bordo tondo di ogni perno"
            ));
        }
        let origin = pins[0].0.as_dvec3();
        let ground = pins[1].0.as_dvec3() - origin;
        let mut u = ground - normal * ground.dot(normal);
        if u.length() < 1e-6 {
            u = normal.any_orthonormal_vector();
        }
        let u = u.normalize();
        let v = normal.cross(u);
        let flat = |p: Vec3| {
            let d = p.as_dvec3() - origin;
            DVec2::new(d.dot(u), d.dot(v))
        };
        let [f1, f2, m1, m2] = pins.map(|(c, _)| flat(c));
        let lengths = [(m1 - f1).length(), (m2 - f2).length(), (m2 - m1).length()];
        let names = ["Perno fisso 1 e perno mobile 1", "Perno fisso 2 e perno mobile 2", "I due perni mobili"];
        for (l, name) in lengths.iter().zip(names) {
            if *l < 0.5 {
                return Err(format!("{name} coincidono: scegli perni diversi"));
            }
        }
        if (f2 - f1).length() < 0.5 {
            return Err("I due perni fissi coincidono: scegli perni diversi".to_string());
        }
        Ok(FourBar {
            normal: normal.as_vec3(),
            skew_deg: skew as f32,
            origin,
            u,
            v,
            f1,
            f2,
            m1,
            m2,
            lengths,
            state: LinkState { theta: 0.0, phi: 0.0, m1, m2 },
        })
    }

    /// Il meccanismo è nella posizione in cui sono stati scelti i perni (a meno degli
    /// arrotondamenti: «-0,000°» vale 0).
    pub fn at_start(&self) -> bool {
        self.state.phi.abs() < 1e-6
    }

    /// Lunghezze in mm: biella 1, biella 2, braccio tra i perni mobili.
    pub fn lengths(&self) -> [f32; 3] {
        self.lengths.map(|l| l as f32)
    }

    /// Le bielle F1–M1 e F2–M2 si incrociano (di solito vuol dire perni mobili scambiati).
    pub fn crossed(&self) -> bool {
        segments_cross(self.f1, self.m1, self.f2, self.m2)
    }

    /// Posizione con il braccio ruotato di `phi`; M1 è l'intersezione più vicina a `near`.
    fn at(&self, phi: f64, near: DVec2, theta_near: f64) -> Option<LinkState> {
        let arm = rotate(self.m2 - self.m1, phi);
        let [a, b] = circles(self.f1, self.lengths[0], self.f2 - arm, self.lengths[1])?;
        let m1 = if a.distance_squared(near) <= b.distance_squared(near) { a } else { b };
        let theta = unwrap_near(angle(m1 - self.f1) - angle(self.m1 - self.f1), theta_near);
        Some(LinkState { theta, phi, m1, m2: m1 + arm })
    }

    /// Cerca la posizione con l'apertura `phi` (gradi, da quella in cui sono stati scelti i
    /// perni), partendo da quella attuale. `Err` con la posizione di fine corsa se il meccanismo
    /// si blocca prima.
    pub fn solve(&self, phi_deg: f32) -> Result<LinkState, LinkState> {
        let target = f64::from(phi_deg).to_radians();
        let start = self.state;
        let steps = ((target - start.phi).abs() / STEP).ceil().max(1.0) as usize;
        let mut s = start;
        for k in 1..=steps {
            let phi = if k == steps { target } else { start.phi + (target - start.phi) * k as f64 / steps as f64 };
            match self.at(phi, s.m1, s.theta) {
                Some(next) => s = next,
                None => return Err(self.limit(s, phi)),
            }
        }
        Ok(s)
    }

    /// Fine corsa tra la posizione `ok` e l'apertura `beyond`, che non si raggiunge.
    fn limit(&self, mut ok: LinkState, mut beyond: f64) -> LinkState {
        for _ in 0..50 {
            let mid = (ok.phi + beyond) * 0.5;
            match self.at(mid, ok.m1, ok.theta) {
                Some(s) => ok = s,
                None => beyond = mid,
            }
        }
        ok
    }

    fn point(&self, p: DVec2) -> Vec3 {
        (self.origin + self.u * p.x + self.v * p.y).as_vec3()
    }

    fn about(&self, center: DVec2, a: f64) -> Mat4 {
        crate::scene_graph::placement::about_axis(self.point(center), self.normal, a.to_degrees() as f32)
    }

    /// Spostamenti (braccio mobile, biella 1, biella 2) dalla posizione in cui sono stati scelti
    /// i perni a `s`.
    pub fn transforms(&self, s: &LinkState) -> [Mat4; 3] {
        let shift = self.point(s.m1) - self.point(self.m1);
        let arm = Mat4::from_translation(shift) * self.about(self.m1, s.phi);
        let link2 = angle(s.m2 - self.f2) - angle(self.m2 - self.f2);
        [arm, self.about(self.f1, s.theta), self.about(self.f2, link2)]
    }

    /// Centri dei quattro perni nella posizione in cui sono stati scelti (F1, F2, M1, M2).
    pub fn pins_at_start(&self) -> [Vec3; 4] {
        [self.f1, self.f2, self.m1, self.m2].map(|p| self.point(p))
    }

    /// Centri dei quattro perni nella posizione `s` (F1, F2, M1, M2).
    pub fn pins(&self, s: &LinkState) -> [Vec3; 4] {
        [self.point(self.f1), self.point(self.f2), self.point(s.m1), self.point(s.m2)]
    }

    /// Posizione del meccanismo ricavata dallo spostamento del braccio mobile `arm` (rispetto a
    /// quando sono stati scelti i perni), es. dopo Annulla. `None` se il braccio è stato spostato
    /// in un modo che il meccanismo non permette.
    pub fn state_from_arm(&self, arm: Mat4) -> Option<LinkState> {
        let flat = |p: Vec3| {
            let d = p.as_dvec3() - self.origin;
            DVec2::new(d.dot(self.u), d.dot(self.v))
        };
        let m1 = flat(arm.transform_point3(self.point(self.m1)));
        let m2 = flat(arm.transform_point3(self.point(self.m2)));
        let tol = 0.05 + 1e-3 * self.lengths.iter().copied().fold(0.0, f64::max);
        if ((m1 - self.f1).length() - self.lengths[0]).abs() > tol || ((m2 - self.f2).length() - self.lengths[1]).abs() > tol {
            return None;
        }
        let theta = unwrap_near(angle(m1 - self.f1) - angle(self.m1 - self.f1), self.state.theta);
        let phi = unwrap_near(angle(m2 - m1) - angle(self.m2 - self.m1), self.state.phi);
        Some(LinkState { theta, phi, m1, m2 })
    }

    /// L'angolo positivo gira dall'altra parte.
    pub fn flip(&mut self) {
        self.normal = -self.normal;
        self.v = -self.v;
        for p in [&mut self.f1, &mut self.f2, &mut self.m1, &mut self.m2, &mut self.state.m1, &mut self.state.m2] {
            p.y = -p.y;
        }
        self.state.theta = -self.state.theta;
        self.state.phi = -self.state.phi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Meccanismo nel piano XZ con i perni lungo Y, simile a una cerniera da cofano: piastra con
    /// F1 (0,0) e F2 (50,0), bielle lunghe circa 81 e 70, braccio tra i perni mobili circa 56.
    fn hood() -> FourBar {
        let y = Vec3::Y;
        let p = |x: f32, z: f32| Vec3::new(x, 0.0, z);
        // Un perno girato al contrario e uno spostato lungo l'asse non cambiano il meccanismo
        FourBar::from_pins([(p(0.0, 0.0), y), (p(50.0, 0.0), -y), (p(-10.0, 80.0), y), (p(45.0, 70.0) + y * 3.0, y)]).unwrap()
    }

    #[test]
    fn opening_keeps_every_link_length() {
        let mut bar = hood();
        let pins0 = bar.pins(&bar.state);
        for target in [10.0, 25.0, 60.0, -0.5, 0.0] {
            let s = bar.solve(target).expect("apertura raggiungibile");
            assert!((s.phi.to_degrees() - f64::from(target)).abs() < 1e-3, "apertura {} invece di {target}", s.phi.to_degrees());
            let [f1, f2, m1, m2] = bar.pins(&s);
            let [l1, l2, l3] = bar.lengths();
            assert!((f1.distance(m1) - l1).abs() < 1e-2);
            assert!((f2.distance(m2) - l2).abs() < 1e-2);
            assert!((m1.distance(m2) - l3).abs() < 1e-2);
            // Il braccio porta i suoi perni dove li mette il meccanismo, le bielle anche
            let [arm, l1, l2] = bar.transforms(&s);
            assert!(arm.transform_point3(pins0[2]).distance(m1) < 1e-2);
            assert!(arm.transform_point3(pins0[3]).distance(m2) < 1e-2);
            assert!(l1.transform_point3(pins0[2]).distance(m1) < 1e-2);
            assert!(l2.transform_point3(pins0[3]).distance(m2) < 1e-2);
            // I perni fissi non si muovono
            assert!(f1.distance(pins0[0]) < 1e-3 && f2.distance(pins0[1]) < 1e-3);
            bar.state = s;
        }
    }

    #[test]
    fn state_is_recovered_from_the_arm_position() {
        let mut bar = hood();
        let s = bar.solve(18.0).unwrap();
        let [arm, ..] = bar.transforms(&s);
        let back = bar.state_from_arm(arm).expect("posizione del meccanismo");
        assert!((back.phi - s.phi).abs() < 1e-4 && (back.theta - s.theta).abs() < 1e-4);
        bar.state = s;
        // Uno spostamento qualsiasi del braccio non è una posizione del meccanismo
        assert!(bar.state_from_arm(Mat4::from_translation(Vec3::new(30.0, 0.0, 0.0))).is_none());
    }

    #[test]
    fn mechanism_stops_at_its_limit() {
        let bar = hood();
        // Il cofano di prova è chiuso quasi a fine corsa: si chiude ancora di meno di 1°
        let closed = bar.solve(-5.0).expect_err("-5° non si raggiungono");
        assert!((-1.0..-0.5).contains(&closed.phi.to_degrees()), "fine corsa a {}°", closed.phi.to_degrees());
        // Il fine corsa è una posizione vera del meccanismo
        let [f1, f2, m1, m2] = bar.pins(&closed);
        let [l1, l2, l3] = bar.lengths();
        assert!((f1.distance(m1) - l1).abs() < 1e-2 && (f2.distance(m2) - l2).abs() < 1e-2 && (m1.distance(m2) - l3).abs() < 1e-2);
    }

    #[test]
    fn swapped_moving_pins_cross_the_links() {
        let bar = hood();
        assert!(!bar.crossed());
        let y = Vec3::Y;
        let [f1, f2, m1, m2] = bar.pins(&bar.state);
        let swapped = FourBar::from_pins([(f1, y), (f2, y), (m2, y), (m1, y)]).unwrap();
        assert!(swapped.crossed());
    }

    #[test]
    fn flip_reverses_the_opening() {
        let mut bar = hood();
        let a = bar.solve(12.0).unwrap();
        let [arm, ..] = bar.transforms(&a);
        bar.flip();
        let b = bar.solve(-12.0).unwrap();
        let [arm2, ..] = bar.transforms(&b);
        let p = Vec3::new(10.0, 0.0, 120.0);
        assert!(arm.transform_point3(p).distance(arm2.transform_point3(p)) < 1e-2);
    }

    #[test]
    fn pins_must_be_parallel_and_distinct() {
        let (y, x) = (Vec3::Y, Vec3::X);
        let p = |a: f32, b: f32| Vec3::new(a, 0.0, b);
        assert!(FourBar::from_pins([(p(0.0, 0.0), y), (p(40.0, 0.0), x), (p(0.0, 90.0), y), (p(40.0, 90.0), y)]).is_err());
        assert!(FourBar::from_pins([(p(0.0, 0.0), y), (p(0.0, 0.0), y), (p(0.0, 90.0), y), (p(40.0, 90.0), y)]).is_err());
    }
}
