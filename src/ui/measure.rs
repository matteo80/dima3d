//! Misura della distanza tra due punti cliccati sui pezzi.
//!
//! Con lo strumento attivo il clic nella vista non seleziona: prende il punto del pezzo sotto
//! il mouse. Vicino a un vertice del triangolo colpito (entro `SNAP_RADIUS` punti sullo
//! schermo) il punto si aggancia al vertice, come nei CAD: così si misura da spigolo a spigolo.
//! Il terzo clic inizia una nuova misura.
//!
//! Calamita (da accendere ogni volta): i punti restano incollati al pezzo su cui sono stati presi.
//! Spostando o ruotando il pezzo (finestra Posizione, cerniera, Annulla) il punto lo segue, mentre
//! un punto su un pezzo fermo (la scocca) resta dov'è: la distanza si aggiorna mentre si gira il
//! pezzo. Serve a riportare sul 3D le quote prese con il metro in cabina.

use glam::{Mat4, Vec3};

/// Distanza massima sullo schermo, in punti, per agganciare un vertice.
pub const SNAP_RADIUS: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasurePoint {
    pub pos: Vec3,
    /// Agganciato a un vertice della mesh
    pub vertex: bool,
    /// Pezzo su cui è stato preso il punto, per la calamita
    pub anchor: Option<Anchor>,
}

impl MeasurePoint {
    /// Punto non legato a un pezzo.
    pub fn free(pos: Vec3, vertex: bool) -> Self {
        Self { pos, vertex, anchor: None }
    }
}

/// Pezzo di un punto della misura e posizione del punto nel pezzo prima del suo piazzamento.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub mesh: u32,
    /// Identità del pezzo (inizio e fine nel file): se la scena cambia (file chiuso o riaperto)
    /// l'indice può indicare un altro pezzo, e il punto si stacca
    pub key: (usize, usize),
    /// `inv(piazzamento) * pos`
    pub local: Vec3,
}

/// Strumento di misura attivo, con i punti presi finora (0, 1 o 2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measure {
    pub points: Vec<MeasurePoint>,
    /// Calamita: i punti seguono i pezzi spostati o ruotati
    pub magnet: bool,
}

impl Measure {
    /// Lega il punto al pezzo colpito.
    pub fn anchored(scene: &crate::scene_graph::SceneGraph, mesh: u32, mut p: MeasurePoint) -> MeasurePoint {
        if let Some(shape) = scene.shapes_data.get(mesh as usize) {
            let local = scene.placement(mesh).inverse().transform_point3(p.pos);
            p.anchor = Some(Anchor { mesh, key: (shape.source_start, shape.source_end), local });
        }
        p
    }

    /// Da chiamare a ogni fotogramma. Calamita accesa: i punti vanno dove il piazzamento attuale
    /// del loro pezzo li porta. Spenta: restano fermi e si ricalcola solo la posizione nel pezzo,
    /// così accendendola dopo uno spostamento il punto parte da dove si vede.
    pub fn follow(&mut self, scene: &crate::scene_graph::SceneGraph) {
        let magnet = self.magnet;
        for p in &mut self.points {
            let Some(a) = &mut p.anchor else { continue };
            let same = scene.shapes_data.get(a.mesh as usize).is_some_and(|s| (s.source_start, s.source_end) == a.key);
            if !same {
                p.anchor = None;
                continue;
            }
            let placement: Mat4 = scene.placement(a.mesh);
            if magnet {
                p.pos = placement.transform_point3(a.local);
            } else {
                a.local = placement.inverse().transform_point3(p.pos);
            }
        }
    }

    /// Aggiunge un punto; dopo una misura completa ne inizia una nuova.
    pub fn add_point(&mut self, p: MeasurePoint) {
        if self.points.len() >= 2 {
            self.points.clear();
        }
        self.points.push(p);
    }

    /// Distanza e differenze di coordinate (secondo − primo), a misura completa.
    pub fn result(&self) -> Option<(f32, Vec3)> {
        match self.points.as_slice() {
            [a, b] => Some((a.pos.distance(b.pos), b.pos - a.pos)),
            _ => None,
        }
    }
}

/// Punto da misurare: il vertice del triangolo colpito più vicino al mouse sullo schermo, se
/// entro `SNAP_RADIUS`, altrimenti il punto colpito sulla superficie.
pub fn snap(
    triangle: [Vec3; 3],
    hit: Vec3,
    to_screen: impl Fn(Vec3) -> Option<egui::Pos2>,
    cursor: egui::Pos2,
) -> MeasurePoint {
    triangle
        .into_iter()
        .filter_map(|v| to_screen(v).map(|p| (v, p.distance(cursor))))
        .filter(|(_, d)| *d <= SNAP_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(MeasurePoint::free(hit, false), |(v, _)| MeasurePoint::free(v, true))
}

/// Una lunghezza con i decimali adatti alla grandezza della scena: un file in millimetri
/// (scena di migliaia di unità) con 2 decimali, uno in metri con 4.
/// Unità delle misure: il Gestore lavora in millimetri (HSF e JT sono convertiti, un file in
/// metri aggiunto a uno in millimetri riceve la proposta di scala).
pub const UNIT: &str = "mm";

pub fn format_length(value: f32, scene_size: f32) -> String {
    let decimals = if scene_size >= 100.0 { 2 } else if scene_size >= 1.0 { 4 } else { 6 };
    format!("{value:.decimals$}")
}

/// Disegna punti, segmento e quota sopra la vista; in basso al centro il riepilogo, con il
/// pulsante della calamita.
pub fn draw(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    measure: &mut Measure,
    to_screen: impl Fn(Vec3) -> Option<egui::Pos2>,
    scene_size: f32,
) {
    let painter = ui.painter_at(rect);
    let accent = egui::Color32::from_rgb(255, 214, 10);
    let outline = egui::Stroke::new(1.5_f32, egui::Color32::BLACK);
    let screen: Vec<Option<egui::Pos2>> = measure.points.iter().map(|p| to_screen(p.pos)).collect();
    if let [Some(a), Some(b)] = screen.as_slice() {
        painter.line_segment([*a, *b], egui::Stroke::new(4.0_f32, egui::Color32::BLACK));
        painter.line_segment([*a, *b], egui::Stroke::new(2.0_f32, accent));
    }
    for (p, s) in measure.points.iter().zip(&screen) {
        let Some(s) = s else { continue };
        if measure.magnet && p.anchor.is_some() {
            // Anello: il punto è incollato al pezzo
            painter.circle_stroke(*s, 10.0, egui::Stroke::new(2.0_f32, accent));
        }
        if p.vertex {
            let r = egui::Rect::from_center_size(*s, egui::vec2(9.0, 9.0));
            painter.rect_filled(r, 0.0, accent);
            painter.rect_stroke(r, 0.0, outline, egui::StrokeKind::Outside);
        } else {
            painter.circle_filled(*s, 4.5, accent);
            painter.circle_stroke(*s, 4.5, outline);
        }
    }

    let fmt = |v: f32| format_length(v, scene_size);
    let (title, detail) = match (measure.result(), measure.points.len()) {
        (Some((d, delta)), _) => (
            format!("Distanza  {} {UNIT}", fmt(d)),
            format!("ΔX {}   ΔY {}   ΔZ {}  ({UNIT})", fmt(delta.x), fmt(delta.y), fmt(delta.z)),
        ),
        (None, 1) => ("Clic sul secondo punto".to_string(), point_text(&measure.points[0], &fmt)),
        _ => (
            "Misura: clic sul primo punto".to_string(),
            "Vicino a un vertice il punto si aggancia al vertice  ·  Esc o M per uscire".to_string(),
        ),
    };
    // Quota a metà del segmento
    if let ([Some(a), Some(b)], Some((d, _))) = (screen.as_slice(), measure.result()) {
        let mid = egui::pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0) + egui::vec2(0.0, -14.0);
        let galley = painter.layout_no_wrap(format!("{} {UNIT}", fmt(d)), egui::FontId::proportional(14.0), egui::Color32::BLACK);
        let bg = egui::Rect::from_center_size(mid, galley.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(bg, 4.0, accent);
        painter.galley(bg.center() - galley.size() / 2.0, galley, egui::Color32::BLACK);
    }

    let area = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.bottom() - 48.0),
        egui::vec2(rect.width().min(560.0), 60.0),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Center)));
    super::theme::overlay_frame(&child).show(&mut child, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).strong().size(15.0));
            ui.add_space(12.0);
            ui.toggle_value(&mut measure.magnet, super::theme::icon_text(ui, super::theme::icon::MAGNET, "Calamita")).on_hover_text(
                "I punti restano incollati ai pezzi: spostando o ruotando un pezzo (finestra Posizione, \
                 cerniera) il suo punto lo segue e la distanza si aggiorna",
            );
        });
        ui.label(egui::RichText::new(detail).small());
    });
}

fn point_text(p: &MeasurePoint, fmt: &impl Fn(f32) -> String) -> String {
    let what = if p.vertex { "Vertice" } else { "Punto" };
    format!("{what}  X {}   Y {}   Z {}  ({UNIT})", fmt(p.pos.x), fmt(p.pos.y), fmt(p.pos.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32, z: f32) -> MeasurePoint {
        MeasurePoint::free(Vec3::new(x, y, z), false)
    }

    #[test]
    fn two_points_give_distance_and_deltas_then_a_new_measure_starts() {
        let mut m = Measure::default();
        m.add_point(pt(1.0, 2.0, 3.0));
        assert!(m.result().is_none());
        m.add_point(pt(4.0, 6.0, 3.0));
        let (d, delta) = m.result().unwrap();
        assert_eq!(d, 5.0);
        assert_eq!(delta, Vec3::new(3.0, 4.0, 0.0));
        m.add_point(pt(0.0, 0.0, 0.0));
        assert_eq!(m.points.len(), 1);
    }

    #[test]
    fn points_near_a_vertex_snap_to_it() {
        let tri = [Vec3::ZERO, Vec3::X, Vec3::Y];
        // Sullo schermo: 100 punti per unità
        let to_screen = |v: Vec3| Some(egui::pos2(v.x * 100.0, v.y * 100.0));
        let near = snap(tri, Vec3::new(0.95, 0.03, 0.0), to_screen, egui::pos2(95.0, 3.0));
        assert_eq!(near, MeasurePoint::free(Vec3::X, true));
        let middle = snap(tri, Vec3::new(0.3, 0.3, 0.0), to_screen, egui::pos2(30.0, 30.0));
        assert_eq!(middle, MeasurePoint::free(Vec3::new(0.3, 0.3, 0.0), false));
        // Un vertice dietro la camera (non proiettabile) non aggancia
        let hidden = snap(tri, Vec3::new(0.95, 0.03, 0.0), |_| None, egui::pos2(95.0, 3.0));
        assert!(!hidden.vertex);
    }

    #[test]
    fn decimals_follow_the_scene_size() {
        assert_eq!(format_length(1234.5678, 5000.0), "1234.57");
        assert_eq!(format_length(1.23456789, 5.0), "1.2346");
        assert_eq!(format_length(0.0123456, 0.2), "0.012346");
    }
}
