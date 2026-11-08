//! Finestra «Posizione»: sposta e ruota un file o i pezzi selezionati, come la finestra «Edita e
//! salva» del visualizzatore usato in azienda, e calcola gli offset come gli script
//! BaseOffsetCalculator (due `CONST BASE`) e WRLOffsetCalculator (due oggetti dello stesso WRL).
//!
//! * X, Y, Z spostano; A, B, C ruotano attorno agli assi X, Y, Z.
//! * «Modifica in coordinate World» spento: assi e centro sono le coordinate dell'oggetto (i
//!   `Transform` del file, che seguono gli spostamenti già fatti); acceso: gli assi e l'origine
//!   della cabina. Tutti i pezzi di un oggetto usano lo stesso riferimento.
//! * Gli offset (X Y Z A B C, rotazioni ad assi fissi X, poi Y, poi Z) applicati in coordinate
//!   dell'oggetto portano una copia della matematica nella posizione calcolata dagli script.
//!
//! Valori in mm: le unità della scena (HSF e STEP sono in mm, i file in metri si convertono
//! all'apertura).

use glam::{Mat4, Vec3};

use super::theme::{icon, icon_text};
use crate::app::{Action, GestoreApp};
use crate::scene_graph::placement::{about_axis, circle_through, parse_const_base, parse_offset, Pose};

/// Cosa si sposta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Un file intero (nodo radice)
    File(u32),
    /// I nodi selezionati nell'albero o nella vista
    Selection,
}

/// Calcolo dell'offset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CalcMode {
    /// Tra due oggetti della scena (come WRLOffsetCalculator)
    #[default]
    Objects,
    /// Tra due `CONST BASE` (come BaseOffsetCalculator)
    Bases,
}

#[derive(Debug, Clone)]
pub struct PositionPanel {
    pub open: bool,
    pub target: Option<Target>,
    /// 0..2 = X Y Z (spostamento), 3..5 = A B C (rotazione)
    pub axis: usize,
    /// Quanto si è spostato con il cursore dall'ultima scelta di oggetto/asse (mm o gradi)
    pub value: f32,
    /// Passo del cursore e delle frecce («Unità»)
    pub unit: f32,
    /// «Modifica in coordinate World»
    pub world: bool,
    /// Revisione della scena dopo l'ultimo spostamento fatto da qui: se cambia (Annulla, altre
    /// modifiche) il valore del cursore riparte da 0
    revision: u64,
    /// Chiave (oggetto, asse, world) a cui si riferisce `value`
    key: Option<(Target, usize, bool)>,
    /// Selezione vista all'ultimo disegno: quando cambia, l'oggetto diventa la selezione
    last_selection: Vec<u32>,
    /// Offset da applicare
    pub offset: Pose,
    /// Campo in cui incollare il risultato degli script
    pub pasted: String,
    pub calc: CalcMode,
    pub reference: Option<u32>,
    pub destination: Option<u32>,
    pub base_a: String,
    pub base_b: String,
    pub result: Option<Pose>,
    /// Ultimo messaggio di errore del calcolo
    pub error: Option<String>,
    /// Rotazione su cerniera
    pub hinge: Hinge,
}

impl Default for PositionPanel {
    fn default() -> Self {
        Self {
            open: false,
            target: None,
            axis: 0,
            value: 0.0,
            unit: 1.0,
            world: false,
            revision: 0,
            key: None,
            last_selection: Vec::new(),
            offset: Pose::default(),
            pasted: String::new(),
            calc: CalcMode::Objects,
            reference: None,
            destination: None,
            base_a: String::new(),
            base_b: String::new(),
            result: None,
            error: None,
            hinge: Hinge::default(),
        }
    }
}

const AXES: [(&str, egui::Color32, &str); 6] = [
    ("X", egui::Color32::from_rgb(230, 80, 80), "Sposta lungo X"),
    ("Y", egui::Color32::from_rgb(80, 190, 90), "Sposta lungo Y"),
    ("Z", egui::Color32::from_rgb(90, 130, 240), "Sposta lungo Z"),
    ("A", egui::Color32::from_rgb(230, 80, 80), "Ruota attorno a X"),
    ("B", egui::Color32::from_rgb(80, 190, 90), "Ruota attorno a Y"),
    ("C", egui::Color32::from_rgb(90, 130, 240), "Ruota attorno a Z"),
];

const UNITS: [f32; 6] = [0.01, 0.1, 1.0, 5.0, 10.0, 100.0];

/// Come si definisce l'asse della rotazione su cerniera.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum HingeMode {
    /// Due punti cliccati (es. i centri delle due cerniere)
    #[default]
    TwoPoints,
    /// Il centro di due fori, ognuno da tre punti sul bordo
    TwoHoles,
    /// Un foro (tre punti sul bordo): l'asse passa per il centro, perpendicolare al foro
    OneHole,
    /// Un punto e uno degli assi X, Y, Z (0, 1, 2)
    PointAxis(usize),
}

impl HingeMode {
    /// Clic necessari.
    pub fn points(self) -> usize {
        match self {
            HingeMode::TwoPoints => 2,
            HingeMode::TwoHoles => 6,
            HingeMode::OneHole => 3,
            HingeMode::PointAxis(_) => 1,
        }
    }

    /// Cosa cliccare per il punto `i` (da 0).
    pub fn prompt(self, i: usize) -> String {
        match self {
            HingeMode::TwoPoints => ["Clic sul primo punto dell'asse", "Clic sul secondo punto dell'asse"][i.min(1)].to_string(),
            HingeMode::TwoHoles => format!("Foro {}: clic sul bordo, punto {} di 3", i / 3 + 1, i % 3 + 1),
            HingeMode::OneHole => format!("Clic sul bordo del foro, punto {} di 3", i + 1),
            HingeMode::PointAxis(_) => "Clic sul punto per cui passa l'asse".to_string(),
        }
    }

    /// Asse (punto, direzione) dai punti cliccati.
    pub fn axis(self, p: &[Vec3]) -> Result<(Vec3, Vec3), &'static str> {
        let line = |a: Vec3, b: Vec3| {
            let d = b - a;
            if d.length_squared() > 1e-12 { Ok((a, d.normalize())) } else { Err("I due punti coincidono") }
        };
        match (self, p) {
            (HingeMode::TwoPoints, [a, b]) => line(*a, *b),
            (HingeMode::TwoHoles, [a, b, c, d, e, f]) => {
                let (c1, _) = circle_through(*a, *b, *c).ok_or("I tre punti del primo foro sono allineati")?;
                let (c2, _) = circle_through(*d, *e, *f).ok_or("I tre punti del secondo foro sono allineati")?;
                line(c1, c2)
            }
            (HingeMode::OneHole, [a, b, c]) => circle_through(*a, *b, *c).ok_or("I tre punti del foro sono allineati"),
            (HingeMode::PointAxis(k), [a]) => {
                let mut d = Vec3::ZERO;
                d[k.min(2)] = 1.0;
                Ok((*a, d))
            }
            _ => Err("Punti mancanti"),
        }
    }
}

/// Rotazione su cerniera: asse scelto nella vista e angolo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hinge {
    pub mode: HingeMode,
    /// Il clic nella vista prende i punti dell'asse invece di selezionare
    pub picking: bool,
    pub points: Vec<Vec3>,
    /// Punto e direzione (unitaria) dell'asse
    pub axis: Option<(Vec3, Vec3)>,
    /// Angolo applicato da quando l'asse è stato scelto (gradi)
    pub angle: f32,
    pub error: Option<String>,
}

impl Hinge {
    /// Inizia la scelta dell'asse.
    pub fn start(&mut self) {
        self.picking = true;
        self.points.clear();
        self.axis = None;
        self.angle = 0.0;
        self.error = None;
    }

    /// Punto cliccato nella vista; all'ultimo calcola l'asse.
    pub fn add_point(&mut self, p: Vec3) {
        if !self.picking {
            return;
        }
        self.points.push(p);
        if self.points.len() >= self.mode.points() {
            self.picking = false;
            match self.mode.axis(&self.points) {
                Ok(axis) => {
                    self.axis = Some(axis);
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.points.clear();
                }
            }
        }
    }

    /// Testo per chi sta scegliendo l'asse.
    pub fn prompt(&self) -> Option<String> {
        self.picking.then(|| self.mode.prompt(self.points.len()))
    }
}

/// Punti e asse della cerniera sopra la vista.
pub fn draw_overlay(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    hinge: &Hinge,
    to_screen: impl Fn(Vec3) -> Option<egui::Pos2>,
    scene_size: f32,
) {
    let painter = ui.painter_at(rect);
    let accent = egui::Color32::from_rgb(255, 140, 30);
    if let Some((o, d)) = hinge.axis {
        let half = scene_size.max(1.0) * 0.6;
        if let (Some(a), Some(b)) = (to_screen(o - d * half), to_screen(o + d * half)) {
            painter.line_segment([a, b], egui::Stroke::new(2.0, accent));
        }
        if let (Some(a), Some(b)) = (to_screen(o), to_screen(o + d * scene_size.max(1.0) * 0.05)) {
            painter.arrow(a, b - a, egui::Stroke::new(3.0, accent));
        }
    }
    for p in &hinge.points {
        if let Some(s) = to_screen(*p) {
            painter.circle(s, 4.5, accent, egui::Stroke::new(1.5, egui::Color32::BLACK));
        }
    }
    if let Some(text) = hinge.prompt() {
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            format!("Asse della cerniera: {text}  ·  Esc per annullare"),
            egui::FontId::proportional(15.0),
            accent,
        );
    }
}

/// Movimento elementare: `amount` mm lungo l'asse 0..2, o gradi attorno all'asse 3..5.
pub fn axis_motion(axis: usize, amount: f32) -> Mat4 {
    let mut v = Vec3::ZERO;
    v[axis % 3] = amount;
    if axis < 3 {
        Mat4::from_translation(v)
    } else {
        crate::scene_graph::placement::rigid(Vec3::ZERO, v, Vec3::ZERO)
    }
}

/// `motion` espresso nel riferimento `frame` (coordinate dell'oggetto) diventa una matrice in
/// coordinate mondo; con `world` resta com'è (assi e origine della cabina).
pub fn in_frame(motion: Mat4, frame: Mat4, world: bool) -> Mat4 {
    if world {
        return motion;
    }
    // Solo posizione e orientamento del riferimento: la scala delle unità non deve entrare
    let f = Pose::from_matrix(frame).matrix();
    f * motion * f.inverse()
}

/// Nodi da spostare e nodo da cui prendere le coordinate dell'oggetto.
fn target_nodes(app: &GestoreApp, target: Target) -> Vec<u32> {
    match target {
        Target::File(n) => vec![n],
        Target::Selection => app.ui_state.selected_nodes(),
    }
}

/// Nome mostrato di un nodo, o "—".
fn node_label(app: &GestoreApp, node: Option<u32>) -> String {
    match (node, &app.scene_graph) {
        (Some(n), Some(s)) if (n as usize) < s.nodes.len() => s.display_name(n),
        _ => "—".to_string(),
    }
}

fn pose_grid(ui: &mut egui::Ui, id: &str, pose: &Pose) {
    egui::Grid::new(id).num_columns(6).spacing([10.0, 2.0]).show(ui, |ui| {
        for (k, v) in [pose.xyz.x, pose.xyz.y, pose.xyz.z].iter().enumerate() {
            ui.label(egui::RichText::new(AXES[k].0).color(AXES[k].1).strong());
            ui.monospace(format!("{:+.3}", v + 0.0));
        }
        ui.label("mm");
        ui.end_row();
        for (k, v) in [pose.abc.x, pose.abc.y, pose.abc.z].iter().enumerate() {
            ui.label(egui::RichText::new(AXES[k + 3].0).color(AXES[k + 3].1).strong());
            ui.monospace(format!("{:+.3}", v + 0.0));
        }
        ui.label("°");
        ui.end_row();
    });
}

/// Testo dell'offset come lo scrivono gli script (si può incollare in «Applica offset»).
fn offset_text(title: &str, p: &Pose) -> String {
    format!(
        "{title}\n\nPOSIZIONE\nX   {:+12.3} mm\nY   {:+12.3} mm\nZ   {:+12.3} mm\n\nORIENTAMENTO\nA   {:+12.3}°\nB   {:+12.3}°\nC   {:+12.3}°",
        p.xyz.x, p.xyz.y, p.xyz.z, p.abc.x, p.abc.y, p.abc.z
    )
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp) {
    if !app.ui_state.position.open {
        return;
    }
    let Some(scene) = &app.scene_graph else { return };
    let busy = app.project.busy() || app.ui_state.is_loading;
    let files: Vec<(u32, String)> = scene.open_files().map(|f| (f.node, f.name.clone())).collect();
    let selection = app.ui_state.selected_nodes();
    let revision = scene.revision;

    let mut panel = app.ui_state.position.clone();
    // Oggetto: quello scelto se ancora valido, altrimenti la selezione o il primo file
    let valid = |t: &Target| match t {
        Target::File(n) => files.iter().any(|(f, _)| f == n),
        Target::Selection => !selection.is_empty(),
    };
    if selection != panel.last_selection {
        if !selection.is_empty() {
            panel.target = Some(Target::Selection);
        }
        panel.last_selection = selection.clone();
    }
    if !panel.target.as_ref().is_some_and(valid) {
        panel.target = if !selection.is_empty() {
            Some(Target::Selection)
        } else {
            files.first().map(|(n, _)| Target::File(*n))
        };
    }
    let target = panel.target;
    let key = target.map(|t| (t, panel.axis, panel.world));
    if revision != panel.revision {
        // Modifica fatta altrove (Annulla, altri comandi): l'angolo riparte da 0
        panel.hinge.angle = 0.0;
    }
    if key != panel.key || revision != panel.revision {
        panel.value = 0.0;
        panel.key = key;
        panel.revision = revision;
    }
    let nodes = target.map(|t| target_nodes(app, t)).unwrap_or_default();
    let frame = nodes.first().map_or(Mat4::IDENTITY, |&n| scene.object_frame(n));
    let frame_pose = Pose::from_matrix(frame);
    let has_frame = frame != Mat4::IDENTITY;

    let mut open = true;
    let mut actions: Vec<Action> = Vec::new();
    let mut copy: Option<String> = None;
    let mut status: Option<String> = None;
    let mut start_picking = false;

    egui::Window::new(format!("{}  Posizione", icon::ARROWS_OUT_CARDINAL))
        .id(egui::Id::new("finestra_posizione"))
        .open(&mut open)
        .resizable(false)
        .collapsible(true)
        .default_pos(ctx.screen_rect().right_top() + egui::vec2(-400.0, 60.0))
        .show(ctx, |ui| {
            ui.set_width(340.0);
            ui.add_enabled_ui(!busy, |ui| {
                // ── Oggetto ────────────────────────────────────────────────────
                ui.label(egui::RichText::new("Oggetto").strong());
                let label = match target {
                    Some(Target::File(n)) => files.iter().find(|(f, _)| *f == n).map(|(_, s)| s.clone()).unwrap_or_default(),
                    Some(Target::Selection) => format!("Pezzi selezionati ({})", selection.len()),
                    None => "—".to_string(),
                };
                egui::ComboBox::from_id_salt("posizione_oggetto").width(330.0).selected_text(label).show_ui(ui, |ui| {
                    for (n, name) in &files {
                        ui.selectable_value(&mut panel.target, Some(Target::File(*n)), name);
                    }
                    ui.add_enabled_ui(!selection.is_empty(), |ui| {
                        ui.selectable_value(
                            &mut panel.target,
                            Some(Target::Selection),
                            format!("Pezzi selezionati ({})", selection.len()),
                        );
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Coordinate dell'oggetto").strong());
                    if ui.small_button(icon::COPY).on_hover_text("Copia X Y Z A B C").clicked() {
                        copy = Some(frame_pose.to_text());
                    }
                });
                pose_grid(ui, "posizione_coordinate", &frame_pose);
                ui.label(
                    egui::RichText::new(if has_frame {
                        "Dai Transform del file, con gli spostamenti fatti qui"
                    } else {
                        "Nessun Transform nel file: coincidono con la cabina"
                    })
                    .small()
                    .weak(),
                );

                // ── Trasformazione ─────────────────────────────────────────────
                ui.separator();
                ui.label(egui::RichText::new("Trasformazione").strong());
                ui.horizontal(|ui| {
                    for (k, (name, color, hint)) in AXES.iter().enumerate() {
                        let text = egui::RichText::new(*name).color(*color).strong().size(16.0);
                        if ui.add(egui::Button::new(text).selected(panel.axis == k).min_size(egui::vec2(36.0, 26.0))).on_hover_text(*hint).clicked() {
                            panel.axis = k;
                        }
                    }
                });
                let rotation = panel.axis >= 3;
                let suffix = if rotation { " °" } else { " mm" };
                let before = panel.value;
                ui.horizontal(|ui| {
                    ui.label("Modifica");
                    if ui.small_button(icon::CARET_LEFT).clicked() {
                        panel.value -= panel.unit;
                    }
                    let range = if rotation { 180.0 } else { (panel.unit * 200.0).max(panel.value.abs()) };
                    ui.spacing_mut().slider_width = 150.0;
                    ui.add(egui::Slider::new(&mut panel.value, -range..=range).step_by(panel.unit as f64).show_value(false));
                    if ui.small_button(icon::CARET_RIGHT).clicked() {
                        panel.value += panel.unit;
                    }
                    ui.add(egui::DragValue::new(&mut panel.value).speed(panel.unit).max_decimals(3).suffix(suffix))
                        .on_hover_text("Trascina, oppure fai doppio clic e scrivi il valore");
                });
                ui.horizontal(|ui| {
                    ui.label("Unità");
                    egui::ComboBox::from_id_salt("posizione_unita")
                        .width(70.0)
                        .selected_text(format!("{}", panel.unit))
                        .show_ui(ui, |ui| {
                            for u in UNITS {
                                ui.selectable_value(&mut panel.unit, u, format!("{u}"));
                            }
                        })
                        .response
                        .on_hover_text("Passo del cursore e delle frecce (mm o gradi)");
                    ui.checkbox(&mut panel.world, "Modifica in coordinate World").on_hover_text(
                        "Acceso: assi e origine della cabina. Spento: assi e centro dell'oggetto \
                         (le sue coordinate, uguali per tutti i suoi pezzi)",
                    );
                });
                if panel.value != before && !nodes.is_empty() {
                    let motion = axis_motion(panel.axis, panel.value - before);
                    actions.push(Action::Move { nodes: nodes.clone(), matrix: in_frame(motion, frame, panel.world) });
                }
                if ui.button(icon_text(ui, icon::ARROW_COUNTER_CLOCKWISE, "Ripristina"))
                    .on_hover_text("Riporta l'oggetto nella posizione in cui è stato aperto")
                    .clicked()
                    && !nodes.is_empty()
                {
                    actions.push(Action::ResetPosition(nodes.clone()));
                }

                // ── Rotazione su un asse (cerniere) ─────────────────────────────
                ui.separator();
                egui::CollapsingHeader::new(egui::RichText::new("Rotazione su un asse (cerniere)").strong())
                    .id_salt("posizione_cerniera")
                    .default_open(true)
                    .show(ui, |ui| {
                        let h = &mut panel.hinge;
                        let before_mode = h.mode;
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(&mut h.mode, HingeMode::TwoPoints, "Due punti")
                                .on_hover_text("Due clic sull'asse, es. i centri delle due cerniere");
                            ui.selectable_value(&mut h.mode, HingeMode::TwoHoles, "Due fori")
                                .on_hover_text("Tre clic sul bordo di ogni foro: l'asse unisce i due centri");
                            ui.selectable_value(&mut h.mode, HingeMode::OneHole, "Un foro")
                                .on_hover_text("Tre clic sul bordo: l'asse passa per il centro, perpendicolare al foro (come un perno)");
                            let axis_k = match h.mode { HingeMode::PointAxis(k) => Some(k), _ => None };
                            if ui.selectable_label(axis_k.is_some(), "Punto + asse").on_hover_text("Un clic e la direzione X, Y o Z della cabina").clicked() {
                                h.mode = HingeMode::PointAxis(axis_k.unwrap_or(2));
                            }
                            if let Some(k) = axis_k {
                                for (j, name) in ["X", "Y", "Z"].iter().enumerate() {
                                    let text = egui::RichText::new(*name).color(AXES[j].1).strong();
                                    if ui.selectable_label(k == j, text).clicked() {
                                        h.mode = HingeMode::PointAxis(j);
                                    }
                                }
                            }
                        });
                        if h.mode != before_mode {
                            // Un asse da un punto si aggiorna subito, gli altri si rifanno
                            match (before_mode, h.mode, h.points.first().copied()) {
                                (HingeMode::PointAxis(_), HingeMode::PointAxis(_), Some(p)) if h.points.len() == 1 => {
                                    h.axis = h.mode.axis(&[p]).ok();
                                }
                                _ => {
                                    h.points.clear();
                                    h.axis = None;
                                    h.picking = false;
                                }
                            }
                            h.angle = 0.0;
                        }
                        ui.horizontal(|ui| {
                            if h.picking {
                                if ui.button("Annulla scelta").clicked() {
                                    h.picking = false;
                                    h.points.clear();
                                }
                                ui.label(egui::RichText::new(h.prompt().unwrap_or_default()).italics());
                            } else if ui
                                .button(icon_text(ui, icon::CROSSHAIR, if h.axis.is_some() { "Scegli un altro asse" } else { "Scegli l'asse nella vista" }))
                                .on_hover_text("Clic sui pezzi nella vista (si aggancia ai vertici vicini, come le Misure)")
                                .clicked()
                            {
                                h.start();
                                start_picking = true;
                            }
                        });
                        if let Some(e) = &h.error {
                            ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                        }
                        if let Some((o, d)) = h.axis {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Asse per {:.2} {:.2} {:.2}, direzione {:.3} {:.3} {:.3}",
                                    o.x, o.y, o.z, d.x, d.y, d.z
                                ))
                                .small()
                                .weak(),
                            );
                            let before = h.angle;
                            ui.horizontal(|ui| {
                                ui.label("Angolo");
                                if ui.small_button(icon::CARET_LEFT).clicked() {
                                    h.angle -= panel.unit;
                                }
                                ui.spacing_mut().slider_width = 150.0;
                                ui.add(egui::Slider::new(&mut h.angle, -180.0..=180.0).step_by(panel.unit as f64).show_value(false));
                                if ui.small_button(icon::CARET_RIGHT).clicked() {
                                    h.angle += panel.unit;
                                }
                                ui.add(egui::DragValue::new(&mut h.angle).speed(panel.unit).max_decimals(3).suffix(" °"))
                                    .on_hover_text("Angolo dalla posizione in cui è stato scelto l'asse");
                            });
                            if h.angle != before && !nodes.is_empty() {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: about_axis(o, d, h.angle - before) });
                            }
                            if ui.small_button("Inverti verso").on_hover_text("L'angolo positivo gira dall'altra parte").clicked() {
                                h.axis = Some((o, -d));
                                h.angle = -h.angle;
                            }
                        }
                    });

                // ── Applica offset ─────────────────────────────────────────────
                ui.separator();
                egui::CollapsingHeader::new(egui::RichText::new("Applica offset").strong())
                    .id_salt("posizione_offset")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("posizione_offset_campi").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
                            for k in 0..3 {
                                ui.label(egui::RichText::new(AXES[k].0).color(AXES[k].1).strong());
                                ui.add(egui::DragValue::new(&mut panel.offset.xyz[k]).speed(0.1).max_decimals(3).suffix(" mm"));
                            }
                            ui.end_row();
                            for k in 0..3 {
                                ui.label(egui::RichText::new(AXES[k + 3].0).color(AXES[k + 3].1).strong());
                                ui.add(egui::DragValue::new(&mut panel.offset.abc[k]).speed(0.1).max_decimals(3).suffix(" °"));
                            }
                            ui.end_row();
                        });
                        ui.label(
                            egui::RichText::new(if panel.world {
                                "In coordinate della cabina (World acceso)"
                            } else {
                                "In coordinate dell'oggetto, come gli offset degli script"
                            })
                            .small()
                            .weak(),
                        );
                        let paste = ui
                            .add(
                                egui::TextEdit::multiline(&mut panel.pasted)
                                    .desired_rows(1)
                                    .desired_width(330.0)
                                    .hint_text(format!("{}  Incolla qui (Ctrl+V) il risultato dello script", icon::CLIPBOARD_TEXT)),
                            )
                            .on_hover_text("Il testo copiato dagli script, oppure sei numeri X Y Z A B C");
                        if paste.changed() && !panel.pasted.trim().is_empty() {
                            match parse_offset(&panel.pasted) {
                                Some(p) => {
                                    panel.offset = p;
                                    panel.error = None;
                                    panel.pasted.clear();
                                    status = Some("Offset incollato: controlla i valori e premi Applica".to_string());
                                }
                                None => panel.error = Some("Nel testo incollato non c'è un offset X Y Z A B C".to_string()),
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Applica").on_hover_text("Sposta l'oggetto di questo offset").clicked() && !nodes.is_empty() {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: in_frame(panel.offset.matrix(), frame, panel.world) });
                            }
                            if ui.small_button("Azzera").clicked() {
                                panel.offset = Pose::default();
                            }
                        });
                    });

                // ── Calcola offset ─────────────────────────────────────────────
                egui::CollapsingHeader::new(egui::RichText::new("Calcola offset").strong())
                    .id_salt("posizione_calcolo")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut panel.calc, CalcMode::Objects, "Tra due oggetti")
                                .on_hover_text("Come WRLOffsetCalculator: due oggetti della scena, dalle loro coordinate");
                            ui.selectable_value(&mut panel.calc, CalcMode::Bases, "Tra due CONST BASE")
                                .on_hover_text("Come BaseOffsetCalculator: CONST BASE NOME = X Y Z A B C");
                        });
                        let mut calc_result: Option<Result<(String, Pose), String>> = None;
                        match panel.calc {
                            CalcMode::Objects => {
                                egui::Grid::new("posizione_oggetti").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                                    for (label, slot) in [("Riferimento", &mut panel.reference), ("Destinazione", &mut panel.destination)] {
                                        ui.label(label);
                                        ui.label(egui::RichText::new(node_label(app, *slot)).strong());
                                        if ui.add_enabled(app.ui_state.selected_node.is_some(), egui::Button::new("Usa selezione").small())
                                            .on_hover_text("L'oggetto selezionato nell'albero")
                                            .clicked()
                                        {
                                            *slot = app.ui_state.selected_node;
                                        }
                                        ui.end_row();
                                    }
                                });
                                if ui.button("Calcola").clicked() {
                                    calc_result = Some(match (panel.reference, panel.destination) {
                                        (Some(r), Some(d)) if r != d => {
                                            let a = Pose::from_matrix(scene.object_frame(r));
                                            let b = Pose::from_matrix(scene.object_frame(d));
                                            Ok((format!("{}  ->  {}", node_label(app, Some(r)), node_label(app, Some(d))), a.offset_to(&b)))
                                        }
                                        (Some(_), Some(_)) => Err("Riferimento e destinazione devono essere diversi".to_string()),
                                        _ => Err("Scegli riferimento e destinazione".to_string()),
                                    });
                                }
                            }
                            CalcMode::Bases => {
                                for (label, text) in [("Origine", &mut panel.base_a), ("Destinazione", &mut panel.base_b)] {
                                    ui.label(label);
                                    ui.add(egui::TextEdit::singleline(text).desired_width(330.0).font(egui::TextStyle::Monospace).hint_text("CONST BASE NOME = X Y Z A B C"));
                                }
                                if ui.button("Calcola").clicked() {
                                    calc_result = Some(match (parse_const_base(&panel.base_a), parse_const_base(&panel.base_b)) {
                                        (Some((na, a)), Some((nb, b))) => Ok((format!("{na}  ->  {nb}"), a.offset_to(&b))),
                                        (None, _) => Err("BASE di origine non riconosciuto".to_string()),
                                        (_, None) => Err("BASE di destinazione non riconosciuto".to_string()),
                                    });
                                }
                            }
                        }
                        match calc_result {
                            Some(Ok((title, p))) => {
                                panel.result = Some(p);
                                panel.error = None;
                                status = Some(format!("Offset calcolato: {title}"));
                            }
                            Some(Err(e)) => {
                                panel.result = None;
                                panel.error = Some(e);
                            }
                            None => {}
                        }
                        if let Some(p) = panel.result {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("Scostamento").strong());
                            pose_grid(ui, "posizione_risultato", &p);
                            ui.horizontal(|ui| {
                                if ui.button(icon_text(ui, icon::COPY, "Copia")).clicked() {
                                    copy = Some(offset_text("Offset", &p));
                                }
                                if ui.button("Usa come offset").on_hover_text("Copia i valori in «Applica offset»").clicked() {
                                    panel.offset = p;
                                }
                            });
                        }
                    });
                if let Some(e) = &panel.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
            });
        });

    if let Some(text) = copy {
        ctx.copy_text(text);
        status = Some("Copiato negli appunti".to_string());
    }
    if let Some(s) = status {
        app.ui_state.status_message = s;
    }
    panel.open = open;
    if !open {
        panel.hinge.picking = false;
    }
    app.ui_state.position = panel;
    if start_picking {
        // Un solo strumento alla volta sui clic nella vista
        app.ui_state.measure = None;
        app.ui_state.eyedropper = false;
    }
    for a in actions {
        app.apply(a);
    }
    // Il valore del cursore resta valido dopo i propri spostamenti
    if let Some(s) = &app.scene_graph {
        app.ui_state.position.revision = s.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Due oggetti con i loro Transform: il riferimento è ruotato di 90° attorno a Z.
    const SKID: &[u8] = br#"#VRML V2.0 utf8
DEF Riferimento Group { children [
  Transform { translation 100 200 300 rotation 0 0 1 1.5707963 children [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 50 0 0, 0 50 0 ] } coordIndex [ 0 1 2 -1 ] } }
  ] } ] }
DEF Destinazione Group { children [
  Transform { translation 400 -50 20 rotation 1 0 0 0.3 children [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 50 0 0, 0 50 0 ] } coordIndex [ 0 1 2 -1 ] } }
  ] } ] }
"#;

    fn app() -> GestoreApp {
        let mut app = GestoreApp::new_headless();
        let mut scene = crate::parser::parse_wrl(SKID);
        scene.into_file("skid.wrl".into(), None);
        let mut copy = crate::parser::parse_wrl(SKID);
        copy.into_file("copia.wrl".into(), None);
        scene.append(copy);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.ui_state.position.open = true;
        app
    }

    fn node(app: &GestoreApp, name: &str, nth: usize) -> u32 {
        let s = app.scene_graph.as_ref().unwrap();
        (0..s.nodes.len() as u32).filter(|&n| s.display_name(n) == name).nth(nth).unwrap()
    }

    fn first_vertex(app: &GestoreApp, n: u32) -> Vec3 {
        let s = app.scene_graph.as_ref().unwrap();
        Vec3::from(s.shapes_data[s.meshes_of(&[n])[0] as usize].world_vertices[0])
    }

    struct Ui {
        ctx: egui::Context,
    }

    impl Ui {
        fn run(&self, app: &mut GestoreApp, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1000.0));
            let out = self.ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                draw(ctx, app);
            });
            fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match s {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            let mut texts = Vec::new();
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            texts
        }

        fn press_at(&self, app: &mut GestoreApp, at: egui::Pos2) {
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]] {
                self.run(app, events);
            }
        }

        /// Clic sull'`nth` testo uguale a `label`.
        fn click(&self, app: &mut GestoreApp, label: &str, nth: usize) {
            let texts = self.run(app, vec![]);
            let at = texts
                .iter()
                .filter(|(t, _)| t == label || t.ends_with(label) && t.chars().next().is_some_and(|c| c as u32 >= 0xE000))
                .nth(nth)
                .unwrap_or_else(|| panic!("{label}: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>()))
                .1
                .center();
            self.press_at(app, at);
        }
    }

    /// Frecce sull'asse X: in coordinate dell'oggetto (ruotato di 90° attorno a Z) l'oggetto va
    /// lungo Y della cabina; con World acceso va lungo X.
    #[test]
    fn arrows_move_along_the_object_axes_or_the_cabin_axes() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let rif = node(&app, "Riferimento", 0);
        app.ui_state.select_single(Some(rif));
        ui.run(&mut app, vec![]);
        let texts = ui.run(&mut app, vec![]);
        for wanted in ["Oggetto", "Coordinate dell'oggetto", "Trasformazione", "Ripristina", "Applica offset", "Pezzi selezionati (1)", "+100.000", "+90.000"] {
            assert!(texts.iter().any(|(t, _)| t.contains(wanted)), "{wanted}: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        }
        let start = first_vertex(&app, rif);
        ui.click(&mut app, icon::CARET_RIGHT, 0);
        ui.click(&mut app, icon::CARET_RIGHT, 0);
        let d = first_vertex(&app, rif) - start;
        assert!((d - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-3, "locale: {d:?}");
        assert_eq!(app.project.undo.len(), 1, "i passi delle frecce sono un solo Annulla");
        assert_eq!(app.ui_state.position.value, 2.0);

        ui.click(&mut app, "Modifica in coordinate World", 0);
        assert!(app.ui_state.position.world);
        assert_eq!(app.ui_state.position.value, 0.0, "cambiando riferimento il cursore riparte da 0");
        let start = first_vertex(&app, rif);
        ui.click(&mut app, icon::CARET_RIGHT, 0);
        let d = first_vertex(&app, rif) - start;
        assert!((d - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-3, "World: {d:?}");

        ui.click(&mut app, "Ripristina", 0);
        let d = first_vertex(&app, rif) - Vec3::new(100.0, 200.0, 300.0);
        assert!(d.length() < 1e-3, "ripristinato: {d:?}");
    }

    /// Flusso dello skid: calcolo dell'offset tra i due oggetti, poi applicato alla copia del
    /// riferimento nell'altra matematica, che va a coincidere con la destinazione. L'offset
    /// incollato dagli script dà lo stesso risultato.
    #[test]
    fn calculated_or_pasted_offset_brings_the_copy_onto_the_destination() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let (rif, dest, copy) = (node(&app, "Riferimento", 0), node(&app, "Destinazione", 0), node(&app, "Riferimento", 1));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Calcola offset", 0);
        app.ui_state.select_single(Some(rif));
        ui.click(&mut app, "Usa selezione", 0);
        app.ui_state.select_single(Some(dest));
        ui.click(&mut app, "Usa selezione", 1);
        ui.click(&mut app, "Calcola", 0);
        let expected = {
            let s = app.scene_graph.as_ref().unwrap();
            Pose::from_matrix(s.object_frame(rif)).offset_to(&Pose::from_matrix(s.object_frame(dest)))
        };
        let result = app.ui_state.position.result.expect("offset calcolato");
        assert!(result.matrix().abs_diff_eq(expected.matrix(), 1e-3), "{result:?}");
        ui.click(&mut app, "Usa come offset", 0);
        assert_eq!(app.ui_state.position.offset, result);

        // La copia del riferimento, selezionata, riceve l'offset
        app.ui_state.select_single(Some(copy));
        ui.click(&mut app, "Applica", 0);
        let s = app.scene_graph.as_ref().unwrap();
        let (a, b) = (s.object_frame(copy), s.object_frame(dest));
        assert!(a.abs_diff_eq(b, 1e-2), "{:?} invece di {:?}", Pose::from_matrix(a), Pose::from_matrix(b));
        let (va, vb) = (first_vertex(&app, copy), first_vertex(&app, dest));
        assert!((va - vb).length() < 1e-2, "{va:?} {vb:?}");

        // Di nuovo, con l'offset incollato come lo scrive lo script
        app.undo();
        app.ui_state.position.offset = Pose::default();
        let texts = ui.run(&mut app, vec![]);
        let field = texts.iter().find(|(t, _)| t.contains("Incolla qui")).expect("campo Incolla").1.center();
        ui.press_at(&mut app, field);
        ui.run(&mut app, vec![egui::Event::Paste(offset_text("Riferimento  ->  Destinazione", &result))]);
        let pasted = app.ui_state.position.offset;
        assert!(pasted.matrix().abs_diff_eq(result.matrix(), 1e-3), "{pasted:?}");
        ui.click(&mut app, "Applica", 0);
        let s = app.scene_graph.as_ref().unwrap();
        assert!(s.object_frame(copy).abs_diff_eq(s.object_frame(dest), 1e-2));
    }

    /// Tra due CONST BASE: lo stesso risultato di BaseOffsetCalculator.
    #[test]
    fn offset_between_two_bases() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Calcola offset", 0);
        ui.click(&mut app, "Tra due CONST BASE", 0);
        app.ui_state.position.base_a = "CONST BASE BASE_A = 150 2500 -1200 0.5 -20 -85".into();
        app.ui_state.position.base_b = "CONST BASE BASE_B = 180 3100 40 1.2 -12.5 -91".into();
        ui.click(&mut app, "Calcola", 0);
        let p = app.ui_state.position.result.expect("offset");
        assert!((p.xyz - Vec3::new(-135.1081, 94.1207, 1367.9865)).abs().max_element() < 2e-3, "{p:?}");
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.starts_with("+1367.98")), "risultato mostrato");
        assert!(app.ui_state.status_message.contains("BASE_A  ->  BASE_B"));
    }

    /// Cerniera: asse verticale da due punti, la freccia dell'angolo ruota il pezzo attorno
    /// all'asse (i punti sull'asse restano), un solo Annulla; «Inverti verso» gira dall'altra parte.
    #[test]
    fn hinge_rotates_the_piece_about_the_picked_axis() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let rif = node(&app, "Riferimento", 0);
        app.ui_state.select_single(Some(rif));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Due punti", 0);
        ui.click(&mut app, "Scegli l'asse nella vista", 0);
        assert!(app.ui_state.position.hinge.picking);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t == "Clic sul primo punto dell'asse"), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        app.ui_state.position.hinge.add_point(Vec3::new(100.0, 200.0, 300.0));
        app.ui_state.position.hinge.add_point(Vec3::new(100.0, 200.0, 310.0));
        let (o, d) = app.ui_state.position.hinge.axis.expect("asse");
        assert_eq!(o, Vec3::new(100.0, 200.0, 300.0));
        assert!((d - Vec3::Z).length() < 1e-6);

        let vertex = |app: &GestoreApp, i: usize| {
            let s = app.scene_graph.as_ref().unwrap();
            Vec3::from(s.shapes_data[s.meshes_of(&[rif])[0] as usize].world_vertices[i])
        };
        assert!((vertex(&app, 1) - Vec3::new(100.0, 250.0, 300.0)).length() < 1e-3);
        app.ui_state.position.unit = 90.0;
        // Seconda freccia a destra: quella dell'angolo (la prima è della Trasformazione)
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        assert_eq!(app.ui_state.position.hinge.angle, 90.0);
        assert!((vertex(&app, 0) - Vec3::new(100.0, 200.0, 300.0)).length() < 1e-3, "il punto sull'asse resta");
        assert!((vertex(&app, 1) - Vec3::new(50.0, 200.0, 300.0)).length() < 1e-3, "{:?}", vertex(&app, 1));
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        assert!((vertex(&app, 1) - Vec3::new(100.0, 150.0, 300.0)).length() < 1e-3, "{:?}", vertex(&app, 1));
        assert_eq!(app.project.undo.len(), 1, "i passi dell'angolo sono un solo Annulla");

        ui.click(&mut app, "Inverti verso", 0);
        assert_eq!(app.ui_state.position.hinge.angle, -180.0);
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        // Verso invertito: +90 gira all'indietro, da 180° a 90°
        assert!((vertex(&app, 1) - Vec3::new(50.0, 200.0, 300.0)).length() < 1e-3, "{:?}", vertex(&app, 1));

        app.undo();
        assert!((vertex(&app, 1) - Vec3::new(100.0, 250.0, 300.0)).length() < 1e-3, "annullato");
        ui.run(&mut app, vec![]);
        assert_eq!(app.ui_state.position.hinge.angle, 0.0, "dopo Annulla l'angolo riparte da 0");
    }

    #[test]
    fn hinge_modes_build_the_axis() {
        let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        // Due fori di raggio 5 sull'asse X, in x = 0 e x = 100
        let hole = |x: f32| [c(x, 5.0, 0.0), c(x, 0.0, 5.0), c(x, -5.0, 0.0)];
        let mut pts = hole(0.0).to_vec();
        pts.extend(hole(100.0));
        let (o, d) = HingeMode::TwoHoles.axis(&pts).unwrap();
        assert!(o.length() < 1e-4 && (d - Vec3::X).length() < 1e-4, "{o:?} {d:?}");
        let (o, d) = HingeMode::OneHole.axis(&hole(50.0)).unwrap();
        assert!((o - c(50.0, 0.0, 0.0)).length() < 1e-4 && d.x.abs() > 0.999, "{o:?} {d:?}");
        assert_eq!(HingeMode::PointAxis(1).axis(&[c(1.0, 2.0, 3.0)]).unwrap(), (c(1.0, 2.0, 3.0), Vec3::Y));
        assert!(HingeMode::TwoPoints.axis(&[c(1.0, 1.0, 1.0), c(1.0, 1.0, 1.0)]).is_err());

        // Punti allineati: errore e si ricomincia
        let mut h = Hinge { mode: HingeMode::OneHole, ..Default::default() };
        h.start();
        for p in [c(0.0, 0.0, 0.0), c(1.0, 0.0, 0.0), c(2.0, 0.0, 0.0)] {
            h.add_point(p);
        }
        assert!(h.axis.is_none() && h.error.is_some() && h.points.is_empty() && !h.picking);
    }
}
