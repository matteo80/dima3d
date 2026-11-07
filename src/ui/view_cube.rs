//! Cubo di orientamento: ruota con il modello e, cliccato, porta alla vista corrispondente.
//! Il centro di una faccia dà la vista standard, i bordi e gli angoli le viste intermedie
//! (26 direzioni). I nomi delle facce seguono il sistema di assi scelto (Fronte = muso in
//! coordinate veicolo).

use crate::app::Action;
use crate::renderer::camera::{ArcballCamera, Axes, StandardView};
use glam::Vec3;

/// Mezzo lato del cubo sullo schermo, in punti.
const HALF: f32 = 30.0;
/// Oltre questa frazione del mezzo lato si è sul bordo di una faccia (spigolo o angolo).
const EDGE: f32 = 0.6;
/// Distanza del centro del cubo dall'angolo in alto a destra della vista.
const OFFSET: egui::Vec2 = egui::vec2(-72.0, 112.0);

/// Le sei facce: normale e due assi nel piano della faccia.
const FACES: [(Vec3, Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y, Vec3::Z),
    (Vec3::NEG_X, Vec3::Y, Vec3::Z),
    (Vec3::Y, Vec3::Z, Vec3::X),
    (Vec3::NEG_Y, Vec3::Z, Vec3::X),
    (Vec3::Z, Vec3::X, Vec3::Y),
    (Vec3::NEG_Z, Vec3::X, Vec3::Y),
];

const VIEWS: [StandardView; 6] = [
    StandardView::Front,
    StandardView::Back,
    StandardView::Right,
    StandardView::Left,
    StandardView::Top,
    StandardView::Bottom,
];

/// Vista standard che guarda la faccia con normale `n` (es. Fronte per -X in coordinate veicolo).
fn view_of_face(n: Vec3, axes: Axes) -> Option<StandardView> {
    VIEWS.into_iter().find(|&v| axes.view_direction(v).0.abs_diff_eq(n, 1e-4))
}

/// Proiezione sullo schermo (x a destra, y in basso), in mezzi lati.
fn project(camera: &ArcballCamera, p: Vec3) -> glam::Vec2 {
    glam::vec2(p.dot(camera.right()), -p.dot(camera.up()))
}

/// Direzione dal modello verso l'occhio
fn towards_viewer(camera: &ArcballCamera) -> Vec3 {
    -camera.forward()
}

/// Componente lungo un asse della faccia della zona colpita: -1, 0 (centro) o 1.
fn step(x: f32) -> f32 {
    if x > EDGE {
        1.0
    } else if x < -EDGE {
        -1.0
    } else {
        0.0
    }
}

/// Direzione (non normalizzata, componenti -1/0/1) della zona del cubo sotto il punto `p`,
/// dato in mezzi lati rispetto al centro del cubo.
pub fn hit(camera: &ArcballCamera, p: glam::Vec2) -> Option<Vec3> {
    let viewer = towards_viewer(camera);
    for (n, eu, ev) in FACES {
        if n.dot(viewer) <= 0.02 {
            continue; // faccia nascosta o vista di taglio
        }
        let (c, a, b) = (project(camera, n), project(camera, eu), project(camera, ev));
        let det = a.x * b.y - a.y * b.x;
        if det.abs() < 1e-6 {
            continue;
        }
        let d = p - c;
        let u = (d.x * b.y - d.y * b.x) / det;
        let v = (a.x * d.y - a.y * d.x) / det;
        if u.abs() <= 1.0 && v.abs() <= 1.0 {
            return Some(n + eu * step(u) + ev * step(v));
        }
    }
    None
}

/// Azione per un clic sulla zona `dir`: vista standard sulle facce, direzione libera altrove.
pub fn action_for(dir: Vec3, axes: Axes) -> Action {
    let components = dir.abs().cmpgt(Vec3::splat(0.5)).bitmask().count_ones();
    match (components, view_of_face(dir, axes)) {
        (1, Some(view)) => Action::View(view),
        _ => Action::ViewFrom(dir.normalize()),
    }
}

/// Nome della zona: la vista della faccia o l'unione delle viste (es. "Fronte + Sinistra").
pub fn label_for(dir: Vec3, axes: Axes) -> String {
    let names: Vec<&str> = FACES
        .iter()
        .filter(|(n, _, _)| n.dot(dir) > 0.5)
        .filter_map(|(n, _, _)| view_of_face(*n, axes))
        .map(view_name)
        .collect();
    names.join(" + ")
}

fn view_name(v: StandardView) -> &'static str {
    StandardView::ALL.iter().find(|x| x.0 == v).map_or("", |x| x.1)
}

/// Disegna il cubo e gestisce il clic. Ritorna l'azione da applicare.
pub fn draw(ui: &mut egui::Ui, rect: egui::Rect, camera: &ArcballCamera, axes: Axes) -> Option<Action> {
    let center = rect.right_top() + OFFSET;
    let area = egui::Rect::from_center_size(center, egui::vec2(HALF * 3.6, HALF * 3.6));
    let response = ui.interact(area, ui.id().with("cubo_orientamento"), egui::Sense::click());
    let to_screen = |p: Vec3| {
        let q = project(camera, p);
        center + egui::vec2(q.x, q.y) * HALF
    };
    let hovered = response.hover_pos().and_then(|pos| hit(camera, glam::vec2(pos.x - center.x, pos.y - center.y) / HALF));

    let dark = ui.visuals().dark_mode;
    let accent = super::theme::accent_color(ui);
    let stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(if dark { 40 } else { 90 }));
    let viewer = towards_viewer(camera);
    let painter = ui.painter();

    // Facce visibili, dalla più inclinata alla più frontale
    let mut faces: Vec<_> = FACES.iter().filter(|(n, _, _)| n.dot(viewer) > 0.02).collect();
    faces.sort_by(|a, b| a.0.dot(viewer).total_cmp(&b.0.dot(viewer)));
    for &&(n, eu, ev) in &faces {
        let facing = n.dot(viewer);
        let corner = |u: f32, v: f32| to_screen(n + eu * u + ev * v);
        let shade = (200.0 + 45.0 * facing) as u8;
        let fill = egui::Color32::from_rgba_unmultiplied(shade, shade, (shade as f32 * 1.03).min(255.0) as u8, 235);
        painter.add(egui::Shape::convex_polygon(
            vec![corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)],
            fill,
            stroke,
        ));

        // Zona sotto il mouse (centro, bordo o angolo) su questa faccia
        if let Some(d) = hovered.filter(|d| d.dot(n) > 0.5) {
            let range = |s: f32| if s > 0.5 { (EDGE, 1.0) } else if s < -0.5 { (-1.0, -EDGE) } else { (-EDGE, EDGE) };
            let ((u0, u1), (v0, v1)) = (range(d.dot(eu)), range(d.dot(ev)));
            painter.add(egui::Shape::convex_polygon(
                vec![corner(u0, v0), corner(u1, v0), corner(u1, v1), corner(u0, v1)],
                accent.gamma_multiply(0.85),
                egui::Stroke::NONE,
            ));
        }

        if facing > 0.35 {
            if let Some(view) = view_of_face(n, axes) {
                // Il nome sta dentro la faccia: la larghezza disponibile è quella del lato più
                // orizzontale sullo schermo
                let (a, b) = (project(camera, eu), project(camera, ev));
                let horizontal = if a.x.abs() * b.y.abs() >= b.x.abs() * a.y.abs() { a } else { b };
                let room = horizontal.length() * HALF * 2.0 * 0.86;
                let color = egui::Color32::from_gray(35).gamma_multiply(facing.clamp(0.5, 1.0));
                let base = egui::FontId::proportional(11.5);
                let width = painter.layout_no_wrap(view_name(view).to_string(), base.clone(), color).size().x;
                let size = (base.size * (room / width).min(1.0)).max(7.0);
                painter.text(
                    to_screen(n),
                    egui::Align2::CENTER_CENTER,
                    view_name(view),
                    egui::FontId::proportional(size),
                    color,
                );
            }
        }
    }

    let response = match hovered {
        Some(d) => response.on_hover_text(label_for(d, axes)),
        None => response,
    };
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked().then_some(()).and(hovered).map(|d| action_for(d, axes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso_camera() -> ArcballCamera {
        let mut cam = ArcballCamera::new();
        cam.set_standard_view(StandardView::Iso, Axes::Vehicle);
        cam
    }

    #[test]
    fn faces_are_named_after_the_vehicle_views() {
        assert_eq!(view_of_face(Vec3::NEG_X, Axes::Vehicle), Some(StandardView::Front));
        assert_eq!(view_of_face(Vec3::Y, Axes::Vehicle), Some(StandardView::Right));
        assert_eq!(view_of_face(Vec3::Z, Axes::Vehicle), Some(StandardView::Top));
        assert_eq!(view_of_face(Vec3::NEG_Y, Axes::ZUp), Some(StandardView::Front));
        assert_eq!(view_of_face(Vec3::Z, Axes::YUp), Some(StandardView::Front));
    }

    #[test]
    fn clicking_the_face_centres_gives_the_standard_views() {
        let cam = iso_camera();
        // Dalla vista iso (anteriore sinistra, dall'alto) si vedono Fronte, Sinistra e Alto
        for (n, view) in [
            (Vec3::NEG_X, StandardView::Front),
            (Vec3::NEG_Y, StandardView::Left),
            (Vec3::Z, StandardView::Top),
        ] {
            let d = hit(&cam, project(&cam, n)).expect("faccia visibile");
            assert_eq!(d, n);
            assert_eq!(action_for(d, Axes::Vehicle), Action::View(view));
        }
        // Le facce opposte non sono cliccabili: il centro del cubo è l'angolo verso chi guarda
        let corner = hit(&cam, glam::Vec2::ZERO).unwrap();
        assert_eq!(corner, Vec3::new(-1.0, -1.0, 1.0));
        assert_eq!(label_for(corner, Axes::Vehicle), "Fronte + Sinistra + Alto");
        assert!(hit(&cam, glam::vec2(5.0, 5.0)).is_none(), "fuori dal cubo");
    }

    #[test]
    fn edges_give_intermediate_views() {
        let cam = iso_camera();
        // A metà dello spigolo tra Fronte e Sinistra
        let p = project(&cam, Vec3::new(-1.0, -1.0, 0.0)) * 0.97;
        let d = hit(&cam, p).unwrap();
        assert_eq!(d, Vec3::new(-1.0, -1.0, 0.0));
        assert_eq!(label_for(d, Axes::Vehicle), "Fronte + Sinistra");
        match action_for(d, Axes::Vehicle) {
            Action::ViewFrom(v) => assert!((v - Vec3::new(-1.0, -1.0, 0.0).normalize()).length() < 1e-5),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn after_a_click_the_clicked_face_faces_the_viewer() {
        let mut cam = iso_camera();
        for n in [Vec3::NEG_X, Vec3::NEG_Y, Vec3::Z] {
            let d = hit(&cam, project(&cam, n)).unwrap();
            cam.orientation = match action_for(d, Axes::Vehicle) {
                Action::View(v) => ArcballCamera::standard_orientation(v, Axes::Vehicle),
                Action::ViewFrom(dir) => ArcballCamera::orientation_for_direction(dir, Axes::Vehicle),
                other => panic!("{other:?}"),
            };
            assert!(towards_viewer(&cam).abs_diff_eq(n, 1e-4));
            // Poi si torna all'iso per provare la faccia successiva
            cam.set_standard_view(StandardView::Iso, Axes::Vehicle);
        }
    }
}
