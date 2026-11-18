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
//!
//! «Salva» mette la misura nella scena: va nel gruppo «Misure» sotto l'albero e nel progetto, e
//! resta disegnata anche a strumento spento. I suoi punti seguono sempre i pezzi su cui sono presi.

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

/// Misura salvata nella scena.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneMeasure {
    pub name: String,
    pub points: [MeasurePoint; 2],
    /// Disegnata nella vista
    pub visible: bool,
}

impl SceneMeasure {
    /// Distanza e differenze di coordinate (secondo − primo).
    pub fn result(&self) -> (f32, Vec3) {
        let [a, b] = &self.points;
        (a.pos.distance(b.pos), b.pos - a.pos)
    }
}

/// Gruppo «Misure» della scena.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneMeasures {
    pub items: Vec<SceneMeasure>,
    /// Misura evidenziata (clic nel gruppo)
    pub selected: Option<usize>,
    /// Misura di cui si sta cambiando il nome
    pub renaming: Option<usize>,
}

impl SceneMeasures {
    /// Salva una misura con il primo nome libero («Misura 3»). Ritorna il suo indice.
    pub fn add(&mut self, a: MeasurePoint, b: MeasurePoint) -> usize {
        let next = self
            .items
            .iter()
            .filter_map(|m| m.name.strip_prefix("Misura ")?.trim().parse::<usize>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        self.items.push(SceneMeasure { name: format!("Misura {next}"), points: [a, b], visible: true });
        self.selected = Some(self.items.len() - 1);
        self.items.len() - 1
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
            let shift = |s: Option<usize>| s.and_then(|s| if s == i { None } else if s > i { Some(s - 1) } else { Some(s) });
            self.selected = shift(self.selected);
            self.renaming = shift(self.renaming);
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// I punti seguono i pezzi su cui sono stati presi; se il pezzo non c'è più restano fermi.
    pub fn follow(&mut self, scene: &crate::scene_graph::SceneGraph) {
        for p in self.items.iter_mut().flat_map(|m| m.points.iter_mut()) {
            let Some(a) = &p.anchor else { continue };
            let same = scene.shapes_data.get(a.mesh as usize).is_some_and(|s| (s.source_start, s.source_end) == a.key);
            if same {
                p.pos = scene.placement(a.mesh).transform_point3(a.local);
            } else {
                p.anchor = None;
            }
        }
    }
}

/// Misure salvate e visibili sopra la vista: segmento, punti e nome con la distanza. Quella
/// evidenziata è più spessa e di colore pieno.
pub fn draw_saved(
    ui: &egui::Ui,
    rect: egui::Rect,
    list: &SceneMeasures,
    to_screen: impl Fn(Vec3) -> Option<egui::Pos2>,
    scene_size: f32,
) {
    let painter = ui.painter_at(rect);
    let base = egui::Color32::from_rgb(120, 220, 255);
    for (i, m) in list.items.iter().enumerate().filter(|(_, m)| m.visible) {
        let selected = list.selected == Some(i);
        let color = if selected { egui::Color32::from_rgb(255, 214, 10) } else { base };
        let (Some(a), Some(b)) = (to_screen(m.points[0].pos), to_screen(m.points[1].pos)) else { continue };
        let width = if selected { 2.5_f32 } else { 1.5 };
        painter.line_segment([a, b], egui::Stroke::new(width + 2.0, egui::Color32::BLACK));
        painter.line_segment([a, b], egui::Stroke::new(width, color));
        for s in [a, b] {
            painter.circle_filled(s, 4.0, color);
            painter.circle_stroke(s, 4.0, egui::Stroke::new(1.2_f32, egui::Color32::BLACK));
        }
        let (d, _) = m.result();
        let text = format!("{}  {} {UNIT}", m.name, format_length(d, scene_size));
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), egui::Color32::BLACK);
        let bg = egui::Rect::from_center_size(a.lerp(b, 0.5) + egui::vec2(0.0, -12.0), galley.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(bg, 4.0, color.gamma_multiply(if selected { 1.0 } else { 0.85 }));
        painter.galley(bg.center() - galley.size() / 2.0, galley, egui::Color32::BLACK);
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
/// pulsante della calamita e «Salva». Ritorna true se si è premuto «Salva».
pub fn draw(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    measure: &mut Measure,
    to_screen: impl Fn(Vec3) -> Option<egui::Pos2>,
    scene_size: f32,
) -> bool {
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
    let mut save = false;
    let complete = measure.result().is_some();
    super::theme::overlay_frame(&child).show(&mut child, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).strong().size(15.0));
            ui.add_space(12.0);
            ui.toggle_value(&mut measure.magnet, super::theme::icon_text(ui, super::theme::icon::MAGNET, "Calamita")).on_hover_text(
                "I punti restano incollati ai pezzi: spostando o ruotando un pezzo (finestra Posizione, \
                 cerniera) il suo punto lo segue e la distanza si aggiorna",
            );
            save = ui
                .add_enabled(complete, egui::Button::new(super::theme::icon_text(ui, super::theme::icon::FLOPPY_DISK, "Salva")))
                .on_hover_text("Salva la misura nella scena: va nel gruppo «Misure» sotto l'albero e nel progetto")
                .on_disabled_hover_text("Prendi prima i due punti")
                .clicked();
        });
        ui.label(egui::RichText::new(detail).small());
    });
    save
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

    /// Misure salvate: nomi in ordine, il primo libero dopo il più alto; togliere sposta la
    /// misura evidenziata.
    #[test]
    fn saved_measures_names_and_removal() {
        let mut list = SceneMeasures::default();
        assert_eq!(list.add(pt(0.0, 0.0, 0.0), pt(3.0, 4.0, 0.0)), 0);
        list.add(pt(0.0, 0.0, 0.0), pt(1.0, 0.0, 0.0));
        list.add(pt(0.0, 0.0, 0.0), pt(2.0, 0.0, 0.0));
        assert_eq!(list.items[0].result().0, 5.0);
        assert_eq!(list.selected, Some(2), "l'ultima salvata è evidenziata");
        list.remove(0);
        assert_eq!(list.selected, Some(1));
        assert_eq!(list.items.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Misura 2", "Misura 3"]);
        list.add(pt(0.0, 0.0, 0.0), pt(1.0, 1.0, 1.0));
        assert_eq!(list.items[2].name, "Misura 4");
        list.remove(1);
        assert_eq!(list.selected, Some(1), "l'evidenziata scala di uno");
    }

    #[test]
    fn decimals_follow_the_scene_size() {
        assert_eq!(format_length(1234.5678, 5000.0), "1234.57");
        assert_eq!(format_length(1.23456789, 5.0), "1.2346");
        assert_eq!(format_length(0.0123456, 0.2), "0.012346");
    }
}
