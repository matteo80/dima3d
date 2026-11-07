/// Camera trackball con controllo orbit/pan/zoom.
/// La camera orbita attorno a un punto `target` a distanza `distance`.
///
/// L'orientamento è un quaternione (spazio camera → mondo), non una coppia
/// yaw/pitch: la rotazione avviene sempre attorno agli assi dello schermo,
/// quindi non ci sono blocchi ai poli né assi privilegiati (Y-up o Z-up).
pub struct ArcballCamera {
    /// Punto di interesse (centro di rotazione)
    pub target: glam::Vec3,
    /// Distanza dalla camera al target
    pub distance: f32,
    /// Orientamento della camera (locale → mondo). La camera guarda verso -Z locale.
    pub orientation: glam::Quat,
    /// Campo visivo verticale in radianti
    pub fov_y: f32,
    /// Aspect ratio (larghezza / altezza)
    pub aspect: f32,
    /// Ingombro dell'intera scena, da cui si ricavano i piani di clipping
    pub scene_bounds: Option<crate::scene_graph::Aabb>,
    /// Passaggio animato verso un orientamento (viste standard e cubo di orientamento)
    pub animation: Option<ViewAnimation>,
}

/// Rotazione animata della vista: da `from` a `to` in `ViewAnimation::DURATION`.
#[derive(Debug, Clone, Copy)]
pub struct ViewAnimation {
    pub from: glam::Quat,
    pub to: glam::Quat,
    pub start: std::time::Instant,
}

impl ViewAnimation {
    pub const DURATION: std::time::Duration = std::time::Duration::from_millis(300);
}

impl ArcballCamera {
    pub fn new() -> Self {
        Self {
            target: glam::Vec3::ZERO,
            distance: 10.0,
            // Leggermente dall'alto (equivalente al vecchio pitch = 0.3)
            orientation: glam::Quat::from_rotation_x(-0.3),
            fov_y: std::f32::consts::FRAC_PI_4,
            aspect: 16.0 / 9.0,
            scene_bounds: None,
            animation: None,
        }
    }

    /// Asse "destra" dello schermo in coordinate mondo.
    pub fn right(&self) -> glam::Vec3 {
        self.orientation * glam::Vec3::X
    }

    /// Asse "su" dello schermo in coordinate mondo.
    pub fn up(&self) -> glam::Vec3 {
        self.orientation * glam::Vec3::Y
    }

    /// Posizione della camera nel mondo.
    pub fn eye(&self) -> glam::Vec3 {
        self.target + self.orientation * glam::Vec3::new(0.0, 0.0, self.distance)
    }

    /// Matrice view (world → camera).
    pub fn view_matrix(&self) -> glam::Mat4 {
        glam::Mat4::look_at_rh(self.eye(), self.target, self.up())
    }

    /// Matrice di proiezione prospettica.
    ///
    /// Profondità invertita (1 = vicino, 0 = lontano): con il depth buffer in virgola mobile la
    /// precisione resta quasi uniforme anche con piani molto distanti. Serve ai file con pezzi
    /// lontanissimi, che altrimenti mostrano macchie sulle superfici ravvicinate.
    pub fn proj_matrix(&self) -> glam::Mat4 {
        let (near, far) = self.clip_planes();
        glam::Mat4::perspective_rh(self.fov_y, self.aspect, far, near)
    }

    /// Piani di clipping (vicino, lontano) ricalcolati dalla posizione corrente:
    /// la scena resta intera a qualsiasi zoom e anche con la camera tra i pezzi.
    pub fn clip_planes(&self) -> (f32, f32) {
        let (center, radius) = match self.scene_bounds.as_ref().filter(|b| b.is_valid()) {
            Some(b) => (b.center(), b.diagonal() * 0.5),
            None => (self.target, self.distance),
        };
        let depth = (center - self.eye()).dot(self.forward());
        let far = (depth + radius).max(self.distance * 2.0);
        let near = (depth - radius).max((far * 1e-4).min(self.distance * 0.05));
        (near, far)
    }

    /// Direzione di osservazione in coordinate mondo.
    pub fn forward(&self) -> glam::Vec3 {
        self.orientation * glam::Vec3::NEG_Z
    }

    /// Raggio (origine, direzione) che passa per un punto dello schermo in coordinate NDC.
    pub fn ray(&self, ndc_x: f32, ndc_y: f32) -> (glam::Vec3, glam::Vec3) {
        let inv = self.view_proj().inverse();
        // Profondità invertita: il piano vicino è a z = 1
        let near = inv.project_point3(glam::Vec3::new(ndc_x, ndc_y, 1.0));
        let far = inv.project_point3(glam::Vec3::new(ndc_x, ndc_y, 0.0));
        (near, (far - near).normalize_or_zero())
    }

    /// Porta la camera al nuovo orientamento ruotando attorno a `pivot`:
    /// il punto resta fermo sullo schermo, il resto gli gira attorno.
    fn rotate_about(&mut self, pivot: glam::Vec3, orientation: glam::Quat) {
        let orientation = orientation.normalize();
        let world_rot = orientation * self.orientation.inverse();
        self.target = pivot + world_rot * (self.target - pivot);
        self.orientation = orientation;
    }

    /// Matrice view-projection combinata.
    pub fn view_proj(&self) -> glam::Mat4 {
        self.proj_matrix() * self.view_matrix()
    }

    /// Orbita la camera attorno al target (in radianti).
    /// `delta_x` ruota attorno all'asse verticale dello schermo,
    /// `delta_y` attorno all'asse orizzontale dello schermo. Nessun limite:
    /// si può girare l'oggetto in qualsiasi direzione, anche capovolgerlo.
    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.orbit_about(self.target, delta_x, delta_y);
    }

    /// Come `orbit`, ma attorno a un punto qualsiasi della scena.
    pub fn orbit_about(&mut self, pivot: glam::Vec3, delta_x: f32, delta_y: f32) {
        // Rotazioni negli assi locali della camera → moltiplicazione a destra
        let rot = glam::Quat::from_rotation_y(delta_x) * glam::Quat::from_rotation_x(-delta_y);
        self.rotate_about(pivot, self.orientation * rot);
    }

    /// Rotazione a trackball (stile OpenInventor): il mouse trascina una sfera virtuale centrata
    /// nella finestra, e `pivot` resta fermo sullo schermo. `last` e `current` sono le posizioni
    /// del mouse in NDC (da -1 a 1, Y verso l'alto).
    pub fn orbit_trackball(&mut self, pivot: glam::Vec3, last: glam::Vec2, current: glam::Vec2, sensitivity: f32) {
        let arc = glam::Quat::from_rotation_arc(trackball_point(last), trackball_point(current));
        let (axis, angle) = arc.to_axis_angle();
        let rotation = glam::Quat::from_axis_angle(axis, angle * sensitivity);
        // Il modello gira con il mouse, quindi la camera gira al contrario
        self.rotate_about(pivot, self.orientation * rotation.inverse());
    }

    /// Rotazione ad asse fisso: l'orizzontale gira attorno all'asse verticale del modello,
    /// quindi non si accumula rollio. Il verticale non ha limiti: si può passare oltre i poli.
    /// `upside_down` va deciso a inizio trascinamento e inverte l'orizzontale, così a modello
    /// capovolto il pezzo continua a seguire il mouse.
    pub fn orbit_fixed_axis(
        &mut self,
        pivot: glam::Vec3,
        world_up: glam::Vec3,
        upside_down: bool,
        delta_x: f32,
        delta_y: f32,
    ) {
        let yaw = if upside_down { -delta_x } else { delta_x };
        let orientation = glam::Quat::from_axis_angle(world_up, yaw)
            * self.orientation
            * glam::Quat::from_rotation_x(-delta_y);
        self.rotate_about(pivot, orientation);
    }

    /// Rollio attorno all'asse di vista passante per `pivot` (in radianti).
    pub fn roll_about(&mut self, pivot: glam::Vec3, angle: f32) {
        self.rotate_about(pivot, self.orientation * glam::Quat::from_rotation_z(angle));
    }

    /// Sposta la vista perché `pivot` segua esattamente il mouse.
    /// `delta_x` / `delta_y` e `viewport_height` sono nella stessa unità (punti dello schermo).
    pub fn pan_at(&mut self, pivot: glam::Vec3, delta_x: f32, delta_y: f32, viewport_height: f32) {
        let depth = (pivot - self.eye()).dot(self.forward()).max(self.distance * 1e-3);
        let per_point = 2.0 * depth * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        self.target += (self.up() * delta_y - self.right() * delta_x) * per_point;
    }

    /// Zoom verso `pivot` (`factor` < 1 avvicina): il punto resta fermo sullo schermo.
    pub fn zoom_about(&mut self, pivot: glam::Vec3, factor: f32) {
        let distance = self.distance * factor;
        if distance < 1e-5 {
            return;
        }
        self.target = pivot + (self.target - pivot) * factor;
        self.distance = distance;
    }

    /// Punto sul piano del target (perpendicolare alla vista) lungo il raggio dato:
    /// riferimento per lo zoom quando sotto il mouse non c'è nessun pezzo.
    pub fn point_on_target_plane(&self, origin: glam::Vec3, dir: glam::Vec3) -> glam::Vec3 {
        let forward = self.forward();
        let along = dir.dot(forward);
        if along < 1e-4 {
            return self.target;
        }
        origin + dir * ((self.target - origin).dot(forward) / along)
    }

    /// Orienta la camera su una vista standard nel sistema di assi del modello.
    pub fn set_standard_view(&mut self, view: StandardView, axes: Axes) {
        self.animation = None;
        self.orientation = Self::standard_orientation(view, axes);
    }

    /// Orientamento di una vista standard.
    pub fn standard_orientation(view: StandardView, axes: Axes) -> glam::Quat {
        let (back, up) = axes.view_direction(view);
        orientation_from(back, up)
    }

    /// Orientamento che guarda il modello dalla direzione `back` (dal target verso l'occhio),
    /// con l'asse verticale del modello in alto sullo schermo. Guardando lungo l'asse verticale
    /// il "su" è quello della vista dall'alto o dal basso.
    pub fn orientation_for_direction(back: glam::Vec3, axes: Axes) -> glam::Quat {
        let back = back.normalize();
        let up = if back.cross(axes.up()).length_squared() < 1e-6 {
            let view = if back.dot(axes.up()) > 0.0 { StandardView::Top } else { StandardView::Bottom };
            axes.view_direction(view).1
        } else {
            axes.up()
        };
        orientation_from(back, up)
    }

    /// Avvia il passaggio animato verso un orientamento (lungo l'arco più breve).
    pub fn animate_to(&mut self, target: glam::Quat, now: std::time::Instant) {
        let from = self.orientation;
        let to = if from.dot(target) < 0.0 { -target } else { target };
        self.animation = Some(ViewAnimation { from, to, start: now });
    }

    /// Avanza l'animazione; ritorna true se è ancora in corso (serve un altro frame).
    pub fn update_animation(&mut self, now: std::time::Instant) -> bool {
        let Some(a) = self.animation else { return false };
        let t = now.saturating_duration_since(a.start).as_secs_f32() / ViewAnimation::DURATION.as_secs_f32();
        if t >= 1.0 {
            self.orientation = a.to.normalize();
            self.animation = None;
            return false;
        }
        // Partenza e arrivo morbidi
        let s = t * t * (3.0 - 2.0 * t);
        self.orientation = a.from.slerp(a.to, s).normalize();
        true
    }

    /// Raddrizza la vista: mantiene la direzione di osservazione e riporta l'asse
    /// verticale del modello verso l'alto dello schermo (annulla il rollio).
    pub fn straighten(&mut self, axes: Axes) {
        let dir = (self.orientation * glam::Vec3::Z).normalize();
        if axes.up().cross(dir).length_squared() < 1e-6 {
            return; // si guarda esattamente lungo l'asse verticale: nessun "su" definito
        }
        self.orientation = orientation_from(dir, axes.up());
    }
    /// Zoom continuo da trascinamento verticale (`pixels` > 0 = mouse verso il basso = allontana).
    /// Esponenziale: la velocità percepita è la stessa a qualsiasi distanza.
    pub fn dolly(&mut self, pivot: glam::Vec3, pixels: f32) {
        self.zoom_about(pivot, (pixels * 0.005).exp());
    }

    /// Adatta la camera all'AABB dato, centrando il target e impostando
    /// la distanza in modo che l'intera scena sia visibile.
    pub fn fit_to_bbox(&mut self, bbox: &crate::scene_graph::Aabb) {
        self.target = bbox.center();
        let diag = bbox.diagonal();
        // Distanza tale che la bounding sphere sia nel frustum
        let half_fov = self.fov_y * 0.5;
        self.distance = (diag * 0.5 / half_fov.tan()).max(0.001) * 1.2;
    }
}

/// Orientamento della camera che guarda lungo `-back` (`back` = dal target verso l'occhio)
/// con `up` proiettato in alto sullo schermo.
fn orientation_from(back: glam::Vec3, up: glam::Vec3) -> glam::Quat {
    let back = back.normalize();
    let right = up.cross(back).normalize();
    let up = back.cross(right);
    glam::Quat::from_mat3(&glam::Mat3::from_cols(right, up, back)).normalize()
}

/// Sistema di assi del modello, da cui dipendono le viste standard e il "su".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axes {
    /// Coordinate veicolo (scocche, automotive): X lungo la vettura verso il retro, Y verso il
    /// lato destro, Z in alto. Il fronte della vettura sta verso X minore.
    #[default]
    Vehicle,
    /// Z in alto come in FreeCAD: fronte guardando lungo +Y.
    ZUp,
    /// Y in alto (VRML generico): fronte guardando lungo -Z.
    YUp,
}

impl Axes {
    /// Asse verticale del modello.
    pub fn up(self) -> glam::Vec3 {
        match self {
            Axes::Vehicle | Axes::ZUp => glam::Vec3::Z,
            Axes::YUp => glam::Vec3::Y,
        }
    }

    /// (direzione dal target verso l'occhio, "su" dello schermo) di una vista standard.
    pub fn view_direction(self, view: StandardView) -> (glam::Vec3, glam::Vec3) {
        use glam::Vec3 as V;
        use StandardView as S;
        let up = self.up();
        match self {
            Axes::Vehicle => match view {
                S::Front => (-V::X, up),
                S::Back => (V::X, up),
                S::Right => (V::Y, up),
                S::Left => (-V::Y, up),
                // Dall'alto e dal basso con il muso della vettura in alto sullo schermo
                S::Top => (V::Z, -V::X),
                S::Bottom => (-V::Z, -V::X),
                // Tre quarti anteriore sinistro, dall'alto (isometria vera)
                S::Iso => (V::new(-1.0, -1.0, 1.0).normalize(), up),
            },
            Axes::ZUp => match view {
                S::Front => (-V::Y, up),
                S::Back => (V::Y, up),
                S::Right => (V::X, up),
                S::Left => (-V::X, up),
                S::Top => (V::Z, V::Y),
                S::Bottom => (-V::Z, -V::Y),
                S::Iso => (V::new(1.0, -1.0, 1.0).normalize(), up),
            },
            Axes::YUp => match view {
                S::Front => (V::Z, up),
                S::Back => (-V::Z, up),
                S::Right => (V::X, up),
                S::Left => (-V::X, up),
                S::Top => (V::Y, -V::Z),
                S::Bottom => (-V::Y, V::Z),
                S::Iso => (V::new(1.0, 1.0, 1.0).normalize(), up),
            },
        }
    }

    pub const ALL: [(Axes, &'static str, &'static str); 3] = [
        (Axes::Vehicle, "Veicolo", "Coordinate vettura: X verso il retro, Y a destra, Z in alto. Fronte = muso della vettura"),
        (Axes::ZUp, "Z in alto", "Come FreeCAD: Z in alto, fronte guardando lungo +Y"),
        (Axes::YUp, "Y in alto", "VRML generico: Y in alto, fronte guardando lungo -Z"),
    ];
}

/// Direzione del punto della trackball sotto il mouse: sfera di raggio 0,8 al centro della
/// finestra (lo stesso raggio di FreeCAD), raccordata con un'iperbole verso i bordi.
fn trackball_point(ndc: glam::Vec2) -> glam::Vec3 {
    const RADIUS: f32 = 0.8;
    let d2 = ndc.length_squared();
    let z = if d2 < RADIUS * RADIUS * 0.5 {
        (RADIUS * RADIUS - d2).sqrt()
    } else {
        RADIUS * RADIUS * 0.5 / d2.sqrt()
    };
    ndc.extend(z).normalize()
}

/// Viste standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Iso,
}

impl StandardView {
    pub const ALL: [(StandardView, &'static str, &'static str); 7] = [
        (StandardView::Front, "Fronte", "1"),
        (StandardView::Back, "Retro", "Ctrl+1"),
        (StandardView::Right, "Destra", "3"),
        (StandardView::Left, "Sinistra", "Ctrl+3"),
        (StandardView::Top, "Alto", "7"),
        (StandardView::Bottom, "Basso", "Ctrl+7"),
        (StandardView::Iso, "Isometrica", "0"),
    ];

    /// Etichetta breve per i pulsanti della viewport.
    pub fn short(self) -> &'static str {
        match self {
            StandardView::Front => "Fronte",
            StandardView::Back => "Retro",
            StandardView::Left => "Sx",
            StandardView::Right => "Dx",
            StandardView::Top => "Alto",
            StandardView::Bottom => "Basso",
            StandardView::Iso => "Iso",
        }
    }
}

impl Default for ArcballCamera {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dolly_direction() {
        let mut cam = ArcballCamera::new();
        let d = cam.distance;
        cam.dolly(cam.target, -100.0); // mouse verso l'alto
        assert!(cam.distance < d);
        cam.dolly(cam.target, 200.0);
        assert!(cam.distance > d);
    }

    /// Posizione di un punto sullo schermo (NDC + profondità).
    fn on_screen(cam: &ArcballCamera, p: glam::Vec3) -> glam::Vec3 {
        cam.view_proj().project_point3(p)
    }

    #[test]
    fn test_pivot_stays_still_on_screen() {
        let pivot = glam::Vec3::new(3.0, 1.0, -2.0);
        let world_up = glam::Vec3::Z;
        let moves: [fn(&mut ArcballCamera, glam::Vec3, glam::Vec3); 5] = [
            |c, p, _| c.orbit_about(p, 0.7, -0.4),
            |c, p, _| c.orbit_trackball(p, glam::vec2(0.1, 0.2), glam::vec2(0.5, -0.3), 1.0),
            |c, p, up| c.orbit_fixed_axis(p, up, false, 0.7, -0.4),
            |c, p, _| c.roll_about(p, 0.5),
            |c, p, _| c.zoom_about(p, 0.5),
        ];
        for mv in moves {
            let mut cam = ArcballCamera::new();
            cam.distance = 20.0;
            let before = on_screen(&cam, pivot).truncate();
            mv(&mut cam, pivot, world_up);
            let after = on_screen(&cam, pivot).truncate();
            assert!((after - before).length() < 1e-3, "{before} → {after}");
        }
    }

    #[test]
    fn test_trackball_follows_the_mouse() {
        use glam::vec2;
        let mut cam = ArcballCamera::new();
        cam.distance = 20.0;
        // Il punto del modello rivolto verso chi guarda, con il mouse al centro
        let front = cam.target - cam.forward() * 2.0;
        let before = on_screen(&cam, front);

        cam.orbit_trackball(cam.target, vec2(0.0, 0.0), vec2(0.1, 0.0), 1.0);
        assert!(on_screen(&cam, front).x > before.x + 1e-3, "mouse a destra → pezzo a destra");

        let mut cam = ArcballCamera::new();
        cam.distance = 20.0;
        cam.orbit_trackball(cam.target, vec2(0.0, 0.0), vec2(0.0, 0.1), 1.0);
        assert!(on_screen(&cam, front).y > before.y + 1e-3, "mouse in alto → pezzo in alto");

        // Vicino al bordo un moto tangenziale diventa rollio, la direzione di vista cambia poco
        let mut cam = ArcballCamera::new();
        let (view, up) = (cam.forward(), cam.up());
        cam.orbit_trackball(cam.target, vec2(0.95, 0.0), vec2(0.95, 0.2), 1.0);
        assert!(cam.forward().angle_between(view) < cam.up().angle_between(up));

        // La sensibilità moltiplica l'angolo
        let angle = |s: f32| {
            let mut cam = ArcballCamera::new();
            let start = cam.forward();
            cam.orbit_trackball(cam.target, vec2(0.0, 0.0), vec2(0.1, 0.0), s);
            cam.forward().angle_between(start)
        };
        assert!((angle(2.0) / angle(1.0) - 2.0).abs() < 0.01);

        // Mouse fermo: nessuna rotazione e nessun NaN, anche esattamente al centro
        let mut cam = ArcballCamera::new();
        let orientation = cam.orientation;
        cam.orbit_trackball(cam.target, vec2(0.0, 0.0), vec2(0.0, 0.0), 1.0);
        assert!(cam.orientation.is_finite() && cam.orientation.angle_between(orientation) < 1e-3);
    }

    #[test]
    fn test_pan_follows_the_mouse_exactly() {
        let mut cam = ArcballCamera::new();
        cam.distance = 20.0;
        let pivot = cam.target + cam.forward() * -8.0; // più vicino del target
        let height = 600.0;
        let before = on_screen(&cam, pivot).truncate();
        cam.pan_at(pivot, 30.0, 15.0, height);
        let moved = on_screen(&cam, pivot).truncate() - before;
        // NDC → punti: 1 NDC verticale = height / 2. Y dello schermo verso il basso.
        assert!((moved.y * height / 2.0 + 15.0).abs() < 1e-2, "{moved}");
        assert!((moved.x * cam.aspect * height / 2.0 - 30.0).abs() < 1e-2, "{moved}");
    }

    #[test]
    fn test_fixed_axis_adds_no_roll() {
        let mut cam = ArcballCamera::new();
        cam.set_standard_view(StandardView::Iso, Axes::ZUp);
        for i in 0..200 {
            // Cerchi col mouse: con la rotazione libera qui si accumulerebbe rollio
            let a = i as f32 * 0.2;
            cam.orbit_fixed_axis(cam.target, glam::Vec3::Z, false, a.cos() * 0.02, a.sin() * 0.02);
        }
        assert!(cam.right().z.abs() < 1e-3, "right = {}", cam.right());
    }

    #[test]
    fn test_fixed_axis_can_pass_over_the_pole() {
        let mut cam = ArcballCamera::new();
        cam.set_standard_view(StandardView::Front, Axes::ZUp);
        let start = cam.eye();
        for _ in 0..360 {
            cam.orbit_fixed_axis(cam.target, glam::Vec3::Z, false, 0.0, std::f32::consts::TAU / 360.0);
        }
        assert!((cam.eye() - start).length() < 1e-2);
    }

    #[test]
    fn test_clip_planes_follow_the_camera() {
        let mut cam = ArcballCamera::new();
        let mut bbox = crate::scene_graph::Aabb::empty();
        bbox.extend_point(glam::Vec3::splat(-1000.0));
        bbox.extend_point(glam::Vec3::splat(1000.0));
        cam.fit_to_bbox(&bbox);
        cam.scene_bounds = Some(bbox);

        // Da lontano: tutta la scena tra i due piani
        let (near, far) = cam.clip_planes();
        assert!(near > 0.0 && near < far);
        assert!(far >= cam.distance + 1000.0);

        // Molto da vicino su un dettaglio: il target non deve finire tagliato
        cam.zoom_about(glam::Vec3::splat(1000.0), 1e-4);
        let (near, far) = cam.clip_planes();
        assert!(near < cam.distance && far > cam.distance, "{near} {} {far}", cam.distance);
        assert!(cam.view_matrix().is_finite() && cam.proj_matrix().is_finite());
    }

    #[test]
    fn test_ray_starts_near_and_points_into_the_scene() {
        let mut cam = ArcballCamera::new();
        cam.distance = 12.0;
        let (origin, dir) = cam.ray(0.0, 0.0);
        assert!((dir - cam.forward()).length() < 1e-4, "{dir}");
        assert!(origin.distance(cam.eye()) < cam.distance);
        // Profondità invertita: più vicino = valore più alto
        let near = on_screen(&cam, cam.eye() + cam.forward() * 1.0).z;
        let far = on_screen(&cam, cam.eye() + cam.forward() * 11.0).z;
        assert!(near > far, "{near} {far}");
    }

    #[test]
    fn test_point_on_target_plane() {
        let mut cam = ArcballCamera::new();
        cam.distance = 12.0;
        // Il raggio centrale incontra il piano proprio nel target
        let (o, d) = cam.ray(0.0, 0.0);
        assert!((cam.point_on_target_plane(o, d) - cam.target).length() < 1e-3);
        // Un raggio laterale resta sul piano
        let (o, d) = cam.ray(0.5, -0.5);
        let p = cam.point_on_target_plane(o, d);
        assert!((p - cam.target).dot(cam.forward()).abs() < 1e-3);
        assert!((p - cam.target).length() > 1.0);
    }

    #[test]
    fn test_camera_orbit_full_turn_vertical() {
        // Una rotazione verticale di 360° (passando sopra il polo) deve
        // riportare la camera al punto di partenza: nessun blocco ai poli.
        let mut cam = ArcballCamera::new();
        let start = cam.eye();
        for _ in 0..360 {
            cam.orbit(0.0, std::f32::consts::TAU / 360.0);
        }
        assert!((cam.eye() - start).length() < 1e-3);
    }

    #[test]
    fn test_camera_orbit_over_the_top() {
        // Ruotando di 90° in verticale la camera si porta sopra il target
        let mut cam = ArcballCamera::new();
        cam.orientation = glam::Quat::IDENTITY;
        cam.orbit(0.0, std::f32::consts::FRAC_PI_2);
        let dir = (cam.eye() - cam.target).normalize();
        assert!((dir - glam::Vec3::Y).length() < 1e-4);
        // La view matrix resta valida (nessun NaN) anche esattamente al polo
        assert!(cam.view_matrix().is_finite());
    }

    #[test]
    fn test_standard_views() {
        let mut cam = ArcballCamera::new();
        let dir = |c: &ArcballCamera| (c.eye() - c.target).normalize();
        let close = |a: glam::Vec3, b: glam::Vec3| (a - b).length() < 1e-4;
        use glam::Vec3 as V;

        // Y in alto: fronte da +Z, destra da +X, alto da +Y
        cam.set_standard_view(StandardView::Front, Axes::YUp);
        assert!(close(dir(&cam), V::Z) && close(cam.up(), V::Y) && close(cam.right(), V::X));
        cam.set_standard_view(StandardView::Right, Axes::YUp);
        assert!(close(dir(&cam), V::X));
        cam.set_standard_view(StandardView::Top, Axes::YUp);
        assert!(close(dir(&cam), V::Y));

        // Z in alto (FreeCAD): fronte da -Y, alto da +Z, "su" = +Z
        cam.set_standard_view(StandardView::Front, Axes::ZUp);
        assert!(close(dir(&cam), -V::Y) && close(cam.up(), V::Z) && close(cam.right(), V::X));
        cam.set_standard_view(StandardView::Top, Axes::ZUp);
        assert!(close(dir(&cam), V::Z));
        cam.set_standard_view(StandardView::Right, Axes::ZUp);
        assert!(close(dir(&cam), V::X) && close(cam.up(), V::Z));

        // Veicolo (X verso il retro, Y a destra, Z in alto)
        // Fronte: davanti al muso, il lato destro della vettura appare a sinistra
        cam.set_standard_view(StandardView::Front, Axes::Vehicle);
        assert!(close(dir(&cam), -V::X) && close(cam.up(), V::Z) && close(cam.right(), -V::Y));
        // Destra: dal lato destro, il muso appare a destra
        cam.set_standard_view(StandardView::Right, Axes::Vehicle);
        assert!(close(dir(&cam), V::Y) && close(cam.right(), -V::X));
        cam.set_standard_view(StandardView::Left, Axes::Vehicle);
        assert!(close(dir(&cam), -V::Y) && close(cam.right(), V::X));
        cam.set_standard_view(StandardView::Back, Axes::Vehicle);
        assert!(close(dir(&cam), V::X) && close(cam.right(), V::Y));
        // Alto: muso in alto sullo schermo, lato destro a destra
        cam.set_standard_view(StandardView::Top, Axes::Vehicle);
        assert!(close(dir(&cam), V::Z) && close(cam.up(), -V::X) && close(cam.right(), V::Y));
        cam.set_standard_view(StandardView::Bottom, Axes::Vehicle);
        assert!(close(dir(&cam), -V::Z) && close(cam.up(), -V::X));
        cam.set_standard_view(StandardView::Iso, Axes::Vehicle);
        let d = dir(&cam);
        assert!(d.x < 0.0 && d.y < 0.0 && d.z > 0.0 && cam.right().z.abs() < 1e-4 && cam.up().z > 0.0);
    }
    #[test]
    fn test_view_animation_reaches_the_target() {
        let mut cam = ArcballCamera::new();
        cam.set_standard_view(StandardView::Front, Axes::Vehicle);
        let start = cam.orientation;
        let target = ArcballCamera::orientation_for_direction(glam::Vec3::Z, Axes::Vehicle);
        let t0 = std::time::Instant::now();
        cam.animate_to(target, t0);
        assert!(cam.update_animation(t0 + ViewAnimation::DURATION / 2));
        let half = cam.orientation;
        assert!(half.angle_between(start) > 0.1 && half.angle_between(target) > 0.1);
        assert!(!cam.update_animation(t0 + ViewAnimation::DURATION));
        assert!(cam.orientation.angle_between(target) < 1e-4);
        assert!(cam.animation.is_none());
    }

    #[test]
    fn test_orientation_for_any_direction() {
        let close = |a: glam::Vec3, b: glam::Vec3| (a - b).length() < 1e-4;
        // Spigolo fronte-sinistra: si guarda da -X -Y, con Z in alto
        let q = ArcballCamera::orientation_for_direction(glam::Vec3::new(-1.0, -1.0, 0.0), Axes::Vehicle);
        let mut cam = ArcballCamera::new();
        cam.orientation = q;
        let dir = (cam.eye() - cam.target).normalize();
        assert!(close(dir, glam::Vec3::new(-1.0, -1.0, 0.0).normalize()) && close(cam.up(), glam::Vec3::Z));
        // Dall'alto: come la vista Alto (muso in alto sullo schermo)
        cam.orientation = ArcballCamera::orientation_for_direction(glam::Vec3::Z, Axes::Vehicle);
        assert!(close(cam.up(), -glam::Vec3::X));
    }

    #[test]
    fn test_straighten_removes_roll() {
        let mut cam = ArcballCamera::new();
        cam.set_standard_view(StandardView::Iso, Axes::YUp);
        let before = cam.eye();
        cam.roll_about(cam.target, 0.7);
        cam.straighten(Axes::YUp);
        assert!((cam.eye() - before).length() < 1e-3);
        // "su" dello schermo senza componente laterale: right è orizzontale
        assert!(cam.right().y.abs() < 1e-4);
        assert!(cam.up().y > 0.0);
    }

    #[test]
    fn test_camera_fit_to_bbox() {
        let mut cam = ArcballCamera::new();
        let mut bbox = crate::scene_graph::Aabb::empty();
        bbox.extend_point(glam::Vec3::new(-10.0, -10.0, -10.0));
        bbox.extend_point(glam::Vec3::new(10.0, 10.0, 10.0));

        cam.fit_to_bbox(&bbox);
        assert_eq!(cam.target, glam::Vec3::ZERO);
        assert!(cam.distance > 20.0);
    }
}
