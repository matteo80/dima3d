//! Centro cabina fisso e robot generici posizionati con le coordinate del controllore.
//!
//! Il centro è un punto fisso della scena: il simbolo (sfera e assi) sta lì e i robot si
//! posizionano rispetto a lui, con X Y Z A B C come nei `CONST BASE` (vedi
//! [`crate::scene_graph::placement::Pose`]). Spostando la cabina con Posizione il centro resta
//! fermo. Sotto ogni robot si può mettere un pannello a quota 0 del centro, per controllare
//! l'allineamento con il programma di simulazione.
//!
//! Tutto diventa un file VRML 2.0 tenuto in memoria, «Centro e robot»: nell'albero, nell'export e
//! nel progetto si comporta come la cabina generata.

use std::fmt::Write;

use glam::{Mat4, Vec3};

use crate::cabin::Boxes;
use crate::scene_graph::placement::Pose;

/// Nome del file generato nella scena.
pub const FILE_NAME: &str = "Centro e robot";

/// Un robot: nome, posizione dal centro cabina e pannello sotto.
#[derive(Debug, Clone, PartialEq)]
pub struct RobotParams {
    pub name: String,
    /// X Y Z (mm) e A B C (gradi) dal centro cabina
    pub xyz: [f32; 3],
    pub abc: [f32; 3],
    /// Pannello sotto il robot, a quota 0 del centro
    pub plate: bool,
    /// Misure del pannello lungo X e lungo Y del robot
    pub plate_x: f32,
    pub plate_y: f32,
}

impl Default for RobotParams {
    fn default() -> Self {
        Self { name: "Robot".into(), xyz: [0.0; 3], abc: [0.0; 3], plate: false, plate_x: 1000.0, plate_y: 1000.0 }
    }
}

impl RobotParams {
    /// `x y z a b c pannello px py | nome`
    pub fn to_text(&self) -> String {
        let [x, y, z] = self.xyz;
        let [a, b, c] = self.abc;
        format!(
            "{x} {y} {z} {a} {b} {c} {} {} {} | {}",
            u8::from(self.plate),
            self.plate_x,
            self.plate_y,
            self.name.replace(['\r', '\n'], " ")
        )
    }

    pub fn from_text(text: &str) -> Option<Self> {
        let (numbers, name) = text.split_once('|')?;
        let v: Vec<f32> = numbers.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let [x, y, z, a, b, c, plate, px, py] = v.as_slice() else { return None };
        Some(Self {
            name: name.trim().to_string(),
            xyz: [*x, *y, *z],
            abc: [*a, *b, *c],
            plate: *plate != 0.0,
            plate_x: *px,
            plate_y: *py,
        })
    }

    /// Posa nel sistema del centro cabina.
    pub fn pose(&self) -> Pose {
        Pose { xyz: Vec3::from(self.xyz), abc: Vec3::from(self.abc) }
    }

    /// Misure da correggere (vuoto se vanno bene).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.xyz.iter().chain(&self.abc).any(|v| !v.is_finite()) {
            out.push(format!("{}: coordinate non valide", self.name));
        }
        if self.plate && !(self.plate_x > 0.0 && self.plate_y > 0.0) {
            out.push(format!("{}: il pannello deve avere misure maggiori di zero", self.name));
        }
        out
    }
}

/// Centro cabina fisso, simbolo e robot.
#[derive(Debug, Clone, PartialEq)]
pub struct RobotCell {
    /// Centro cabina nella scena (mm): resta fermo quando si sposta la cabina
    pub center: [f32; 3],
    /// Simbolo nel centro (sfera e tre assi), esportato
    pub marker: bool,
    /// Lunghezza degli assi del simbolo
    pub marker_size: f32,
    pub robots: Vec<RobotParams>,
}

impl Default for RobotCell {
    fn default() -> Self {
        Self { center: [0.0; 3], marker: false, marker_size: 500.0, robots: Vec::new() }
    }
}

impl RobotCell {
    /// `x y z simbolo misura` (i robot a parte, uno per riga: [`RobotParams::to_text`]).
    pub fn center_text(&self) -> String {
        let [x, y, z] = self.center;
        format!("{x} {y} {z} {} {}", u8::from(self.marker), self.marker_size)
    }

    /// Legge il centro da [`center_text`](Self::center_text) (i robot restano).
    pub fn set_center_text(&mut self, text: &str) -> Option<()> {
        let v: Vec<f32> = text.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let [x, y, z, marker, size] = v.as_slice() else { return None };
        self.center = [*x, *y, *z];
        self.marker = *marker != 0.0;
        self.marker_size = *size;
        Some(())
    }

    pub fn center(&self) -> Vec3 {
        Vec3::from(self.center)
    }

    /// C'è qualcosa da mettere nella scena.
    pub fn is_empty(&self) -> bool {
        !self.marker && self.robots.is_empty()
    }

    pub fn problems(&self) -> Vec<String> {
        let mut out: Vec<String> = self.robots.iter().flat_map(|r| r.problems()).collect();
        if self.center.iter().any(|v| !v.is_finite()) {
            out.push("centro cabina: coordinate non valide".into());
        }
        if self.marker && !(self.marker_size > 0.0) {
            out.push("simbolo del centro: la misura deve essere maggiore di zero".into());
        }
        out
    }

    /// Nome per un robot nuovo: «Robot N» con il primo N libero.
    pub fn next_name(&self) -> String {
        (1..).map(|n| format!("Robot {n}")).find(|n| !self.robots.iter().any(|r| &r.name == n)).unwrap_or_default()
    }
}

/// Nome adatto a un `DEF` VRML (lettere, cifre e `_`; non comincia con una cifra).
fn def_name(name: &str, fallback: &str) -> String {
    let mut out: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.is_empty() {
        out = fallback.to_string();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

const BODY: [f32; 3] = [0.82, 0.82, 0.78];
const JOINT: [f32; 3] = [0.3, 0.3, 0.32];
const PLATE: [f32; 3] = [0.2, 0.55, 0.9];

/// Robot generico a 6 assi nel suo sistema (base a Z = 0, braccio verso +X): base, colonna,
/// braccio, avambraccio e polso. Solo una sagoma di ingombro, non un modello reale.
fn robot_shapes(out: &mut String, name: &str) {
    let mut body = Boxes::default();
    let mut joints = Boxes::default();
    // Base e colonna (asse 1)
    joints.cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 120.0), 330.0);
    body.cylinder(Vec3::new(0.0, 0.0, 120.0), Vec3::new(0.0, 0.0, 450.0), 260.0);
    body.add([-220.0, -230.0, 450.0], [260.0, 230.0, 700.0]);
    // Spalla (asse 2) e braccio verticale
    joints.cylinder(Vec3::new(150.0, -260.0, 700.0), Vec3::new(150.0, 260.0, 700.0), 140.0);
    body.add([60.0, -120.0, 700.0], [240.0, 120.0, 1650.0]);
    // Gomito (asse 3) e avambraccio verso +X
    joints.cylinder(Vec3::new(150.0, -170.0, 1700.0), Vec3::new(150.0, 170.0, 1700.0), 120.0);
    body.add([150.0, -90.0, 1610.0], [1300.0, 90.0, 1790.0]);
    // Polso (assi 4-6) e flangia
    joints.cylinder(Vec3::new(1300.0, 0.0, 1700.0), Vec3::new(1450.0, 0.0, 1700.0), 75.0);
    joints.cylinder(Vec3::new(1450.0, 0.0, 1700.0), Vec3::new(1480.0, 0.0, 1700.0), 100.0);
    body.write(out, &format!("{name}_corpo"), BODY, 0.0);
    joints.write(out, &format!("{name}_assi"), JOINT, 0.0);
}

/// Simbolo del centro: sfera gialla e assi X (rosso), Y (verde), Z (blu) di lunghezza `a`.
fn marker_shapes(out: &mut String, c: Vec3, a: f32) {
    let w = (a * 0.04).max(2.0);
    out.push_str("DEF Centro_cabina Group { children [\n");
    let mut ball = Boxes::default();
    ball.sphere(c.to_array(), a * 0.12);
    ball.write(out, "Centro_sfera", [0.95, 0.85, 0.1], 0.0);
    for (name, end, color) in [
        ("Centro_asse_X", Vec3::new(a, w, w), [0.9, 0.2, 0.2]),
        ("Centro_asse_Y", Vec3::new(w, a, w), [0.2, 0.75, 0.2]),
        ("Centro_asse_Z", Vec3::new(w, w, a), [0.2, 0.4, 0.95]),
    ] {
        let mut axis = Boxes::default();
        axis.add((c - Vec3::splat(w)).to_array(), (c + end).to_array());
        axis.write(out, name, color, 0.0);
    }
    out.push_str("] }\n");
}

/// Testo VRML 2.0 di centro e robot, in coordinate della scena.
pub fn vrml(cell: &RobotCell) -> String {
    let mut out = String::from("#VRML V2.0 utf8\n# Centro cabina e robot generati da Dima 3D (millimetri)\n");
    let center = cell.center();
    if cell.marker {
        marker_shapes(&mut out, center, cell.marker_size);
    }
    for (i, robot) in cell.robots.iter().enumerate() {
        let name = def_name(&robot.name, &format!("Robot_{}", i + 1));
        let m = Mat4::from_translation(center) * robot.pose().matrix();
        let (_, rotation, translation) = m.to_scale_rotation_translation();
        let (axis, angle) = rotation.to_axis_angle();
        let _ = writeln!(out, "DEF {name}_gruppo Group {{ children [");
        let _ = writeln!(
            out,
            "DEF {name} Transform {{ translation {} {} {} rotation {} {} {} {} children [",
            translation.x, translation.y, translation.z, axis.x, axis.y, axis.z, angle
        );
        robot_shapes(&mut out, &name);
        out.push_str("] }\n");
        if robot.plate {
            // Pannello a quota 0 del centro, sotto il robot, girato come il robot attorno a Z
            let x_dir = m.transform_vector3(Vec3::X);
            let yaw = if x_dir.truncate().length() > 1e-4 { x_dir.y.atan2(x_dir.x) } else { 0.0 };
            let (u, v) = (Vec3::new(yaw.cos(), yaw.sin(), 0.0), Vec3::new(-yaw.sin(), yaw.cos(), 0.0));
            let base = Vec3::new(translation.x, translation.y, center.z);
            let (hx, hy, t) = (robot.plate_x / 2.0, robot.plate_y / 2.0, 10.0);
            let corners: Vec<Vec3> = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)].iter().map(|&(a, b)| base + u * a + v * b).collect();
            let _ = writeln!(
                out,
                "DEF {name}_pannello Shape {{ appearance Appearance {{ material Material {{ diffuseColor {} {} {} transparency 0.3 }} }}",
                PLATE[0], PLATE[1], PLATE[2]
            );
            out.push_str("geometry IndexedFaceSet { solid FALSE\ncoord Coordinate { point [\n");
            for dz in [-t, 0.0] {
                for p in &corners {
                    let _ = writeln!(out, "{} {} {},", p.x, p.y, p.z + dz);
                }
            }
            out.push_str("] }\ncoordIndex [\n4 5 6 7 -1\n3 2 1 0 -1\n0 1 5 4 -1\n1 2 6 5 -1\n2 3 7 6 -1\n3 0 4 7 -1\n] } }\n");
        }
        out.push_str("] }\n");
    }
    out
}

/// Scena di centro e robot, pronta da aggiungere come file «Centro e robot».
pub fn scene(cell: &RobotCell) -> crate::scene_graph::SceneGraph {
    let mut scene = crate::parser::parse_wrl(vrml(cell).as_bytes());
    scene.units_mm = true;
    scene.into_file(FILE_NAME.to_string(), None);
    scene.sources[0].robots = Some(cell.clone());
    scene
}

impl crate::app::GestoreApp {
    /// Nodo radice del file «Centro e robot», se c'è.
    pub fn robot_node(&self) -> Option<u32> {
        self.scene_graph.as_ref()?.open_files().filter(|f| f.robots.is_some()).last().map(|f| f.node)
    }

    /// Centro e robot messi nella scena l'ultima volta.
    pub fn applied_robot_cell(&self) -> Option<RobotCell> {
        self.scene_graph.as_ref()?.open_files().filter(|f| f.robots.is_some()).last()?.robots.clone()
    }

    /// Mette nella scena centro e robot (sostituisce quelli di prima); senza niente da mostrare
    /// toglie il file. Le misure restano nelle preferenze.
    pub fn build_robot_cell(&mut self, cell: RobotCell) {
        if !self.robot_cell_ready(&cell) {
            return;
        }
        let mut settings = self.settings.clone();
        settings.robots = cell.clone();
        self.apply_settings(settings);
        self.place_robot_cell(cell);
    }

    /// Solo il centro (posizione e simbolo), dalla finestra Cabina: i robot nella scena restano
    /// quelli applicati dalla finestra Robot, e le modifiche non applicate restano lì.
    pub fn build_center(&mut self, center: &RobotCell) {
        let robots = self.applied_robot_cell().map(|c| c.robots).unwrap_or_default();
        let cell = RobotCell { robots, ..center.clone() };
        if !self.robot_cell_ready(&cell) {
            return;
        }
        let mut settings = self.settings.clone();
        settings.robots.center = cell.center;
        settings.robots.marker = cell.marker;
        settings.robots.marker_size = cell.marker_size;
        self.apply_settings(settings);
        self.place_robot_cell(cell);
    }

    fn robot_cell_ready(&mut self, cell: &RobotCell) -> bool {
        let problems = cell.problems();
        if !problems.is_empty() {
            self.ui_state.status_message = format!("Robot non aggiornati: {}", problems.join("; "));
            return false;
        }
        !(self.ui_state.is_loading || self.project.busy())
    }

    fn place_robot_cell(&mut self, cell: RobotCell) {
        let replaced = self.robot_node();
        if let Some(old) = replaced {
            self.apply(crate::app::Action::Delete(old));
        }
        if cell.is_empty() {
            self.ui_state.status_message = "Centro e robot tolti dalla scena".to_string();
            return;
        }
        self.add_generated(scene(&cell));
        let n = cell.robots.len();
        self.ui_state.status_message = format!(
            "Centro e robot {}: {n} robot{}",
            if replaced.is_some() { "aggiornati" } else { "aggiunti" },
            if cell.marker { ", simbolo del centro" } else { "" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(scene: &crate::scene_graph::SceneGraph, name: &str) -> Option<(Vec3, Vec3)> {
        let s = scene.shapes_data.iter().find(|s| s.name.as_deref() == Some(name))?;
        Some(s.world_vertices.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), &v| (a.min(v.into()), b.max(v.into()))))
    }

    fn cell() -> RobotCell {
        RobotCell {
            center: [10000.0, 0.0, 0.0],
            marker: true,
            marker_size: 500.0,
            robots: vec![
                RobotParams { name: "R11".into(), xyz: [-2000.0, 1800.0, 0.0], abc: [0.0, 0.0, -90.0], plate: true, plate_x: 1200.0, plate_y: 800.0 },
                RobotParams { name: "Robot 2".into(), xyz: [1500.0, -1800.0, 300.0], abc: [0.0, 0.0, 90.0], ..Default::default() },
            ],
        }
    }

    #[test]
    fn text_roundtrip() {
        let c = cell();
        for r in &c.robots {
            assert_eq!(RobotParams::from_text(&r.to_text()).as_ref(), Some(r));
        }
        let mut back = RobotCell::default();
        back.set_center_text(&c.center_text()).unwrap();
        assert_eq!((back.center, back.marker, back.marker_size), (c.center, c.marker, c.marker_size));
        assert!(RobotParams::from_text("1 2 3").is_none());
    }

    /// Il centro applicato dalla finestra Cabina non tocca i robot: nella scena restano quelli
    /// applicati prima, nelle preferenze quelli in modifica.
    #[test]
    fn center_alone_keeps_the_robots() {
        let mut app = crate::app::GestoreApp::new_headless();
        let ctx = egui::Context::default();
        app.build_robot_cell(cell());
        app.poll_loading(&ctx);
        // Un robot in più non ancora applicato
        let mut editing = cell();
        editing.robots.push(RobotParams { name: "Nuovo".into(), ..Default::default() });
        let mut settings = app.settings.clone();
        settings.robots = editing.clone();
        app.apply_settings(settings);

        let center = RobotCell { center: [500.0, 0.0, 0.0], marker: true, marker_size: 300.0, robots: Vec::new() };
        app.build_center(&center);
        app.poll_loading(&ctx);
        let applied = app.applied_robot_cell().unwrap();
        assert_eq!(applied.center, [500.0, 0.0, 0.0]);
        assert_eq!(applied.robots, cell().robots, "robot nella scena invariati");
        assert_eq!(app.settings.robots.robots.len(), 3, "le modifiche restano");
        assert_eq!(app.settings.robots.center, [500.0, 0.0, 0.0]);
    }

    /// Simbolo nel centro fisso; robot posizionati dal centro con X Y Z A B C (il braccio, verso
    /// +X del robot, gira con C); pannello a quota 0 del centro, sotto il robot, con le sue misure.
    #[test]
    fn robots_and_marker_are_where_the_coordinates_say() {
        let c = cell();
        let scene = scene(&c);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        let (lo, hi) = part(&scene, "Centro_sfera").unwrap();
        assert!(((lo + hi) / 2.0 - c.center()).length() < 1.0);

        // R11 a (8000, 1800, 0), C = -90°: il suo X va verso -Y della scena
        let (lo, hi) = part(&scene, "R11_corpo").unwrap();
        assert!(lo.z >= -0.5 && (hi.z - 1790.0).abs() < 1.0, "{lo} {hi}");
        assert!(lo.y < 1800.0 - 1200.0 && hi.y < 1800.0 + 300.0, "braccio verso -Y: {lo} {hi}");
        assert!((lo.x + hi.x) / 2.0 > 7700.0 && (lo.x + hi.x) / 2.0 < 8300.0, "{lo} {hi}");

        // Pannello: centrato sotto il robot, a Z 0, girato come lui (1200 lungo -Y, 800 lungo X)
        let (lo, hi) = part(&scene, "R11_pannello").unwrap();
        assert!((hi.z - 0.0).abs() < 0.01 && (lo.z + 10.0).abs() < 0.01);
        assert!(((hi.x - lo.x) - 800.0).abs() < 0.5 && ((hi.y - lo.y) - 1200.0).abs() < 0.5, "{lo} {hi}");
        assert!(((lo + hi) / 2.0 - Vec3::new(8000.0, 1800.0, -5.0)).length() < 0.5);

        // Il secondo robot è alzato di 300 e senza pannello; nome con spazio → DEF valido
        let (lo, _) = part(&scene, "Robot_2_assi").unwrap(); // la base del robot
        assert!((lo.z - 300.0).abs() < 0.5);
        assert!(part(&scene, "Robot_2_pannello").is_none());
        // Nell'albero il Transform del robot porta il suo nome
        let names: Vec<String> = (0..scene.nodes.len() as u32).map(|n| scene.display_name(n)).collect();
        assert!(names.iter().any(|n| n == "R11"), "{names:?}");
    }

    /// Dal programma: aggiunti, aggiornati senza doppioni; spostando la cabina il centro resta
    /// fermo; senza niente da mostrare il file sparisce.
    #[test]
    fn build_from_the_app_and_center_stays_fixed() {
        let mut app = crate::app::GestoreApp::new_headless();
        let ctx = egui::Context::default();
        app.build_cabin(crate::cabin::CabinParams::default());
        app.poll_loading(&ctx);
        app.build_robot_cell(cell());
        let count = |app: &crate::app::GestoreApp| {
            app.scene_graph.as_ref().map_or(0, |s| s.open_files().filter(|f| f.name == FILE_NAME).count())
        };
        assert_eq!(count(&app), 1, "{}", app.ui_state.status_message);
        // Centro della sfera nella scena (con l'eventuale spostamento del file)
        let sphere = |app: &crate::app::GestoreApp| {
            let s = app.scene_graph.as_ref().unwrap();
            let file = s.open_files().filter(|f| f.robots.is_some()).last().unwrap();
            let mesh = file.meshes.clone().find(|&m| s.shapes_data[m as usize].name.as_deref() == Some("Centro_sfera")).unwrap();
            let shape = &s.shapes_data[mesh as usize];
            let mid = shape.world_vertices.iter().map(|&v| Vec3::from(v)).sum::<Vec3>() / shape.world_vertices.len() as f32;
            s.placement(mesh).transform_point3(mid)
        };
        let before = sphere(&app);
        // La cabina si sposta, il centro no
        let node = app.cabin_node().unwrap();
        app.apply(crate::app::Action::Move { nodes: vec![node], matrix: Mat4::from_translation(Vec3::new(-1500.0, 200.0, 0.0)) });
        assert!((sphere(&app) - before).length() < 0.01);
        assert_eq!(app.cabin_center().unwrap().0, Vec3::new(8500.0, 200.0, 0.0));

        let mut moved = cell();
        moved.robots.push(RobotParams { name: moved.next_name(), ..Default::default() });
        app.build_robot_cell(moved);
        assert_eq!(count(&app), 1, "sostituito");
        assert_eq!(app.settings.robots.robots.len(), 3, "ricordati nelle preferenze");

        app.build_robot_cell(RobotCell::default());
        assert_eq!(count(&app), 0);
    }
}
