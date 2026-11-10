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
use crate::scene_graph::linkage::FourBar;
use crate::scene_graph::motion::Motion;
use crate::scene_graph::placement::{about_axis, angle_for_distance, circle_through, parse_const_base, parse_offset, Pose};

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
    /// Come è stato calcolato il risultato (dalla geometria o dai Transform)
    pub result_note: Option<String>,
    /// Ultimo messaggio di errore del calcolo
    pub error: Option<String>,
    /// Rotazione su cerniera
    pub hinge: Hinge,
    /// Guida illustrata della cerniera a 4 perni aperta
    pub guide: bool,
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
            result_note: None,
            error: None,
            hinge: Hinge::default(),
            guide: false,
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
    /// Lo stesso pezzo in due posizioni (due matematiche già pronte): l'asse si calcola dal moto
    TwoPositions,
    /// Cerniera a quattro perni (cofano): due perni sulla cerniera fissa, due sulla mobile
    FourBar,
}

/// Nomi dei quattro perni, nell'ordine in cui si cliccano.
const PIN_NAMES: [&str; 4] = [
    "F1 (cerniera fissa, perno 1)",
    "F2 (cerniera fissa, perno 2)",
    "M1 (cerniera mobile, sulla biella di F1)",
    "M2 (cerniera mobile, sulla biella di F2)",
];

/// Promemoria nella finestra Posizione; la spiegazione completa è nella guida illustrata.
const FOUR_BAR_HINT: &str = "Ordine: F1 e F2 sulla cerniera fissa, poi M1 e M2 sulla mobile \
(M1 sulla stessa biella di F1). Su ogni perno 3 punti sul bordo, a triangolo.";

/// Passi della guida illustrata.
const FOUR_BAR_STEPS: [&str; 6] = [
    "Parti dalla posizione della matematica (di solito cofano chiuso) e premi «Scegli i 4 perni nella vista».",
    "Su ogni perno clicca 3 punti sul bordo del cerchio, lontani tra loro in modo da formare un triangolo \
     (non vicini e non in fila): dai 3 punti si trova il centro del perno.",
    "Ordine dei perni: prima F1 e F2 sulla cerniera fissa (quella avvitata alla scocca), poi M1 e M2 sulla \
     cerniera mobile (il braccio avvitato al cofano). M1 è in cima alla biella che parte da F1, M2 in cima \
     a quella che parte da F2.",
    "Con «Usa selezione» indica la parte che si apre (cofano + braccio) e, se vuoi che si muovano, le due bielle.",
    "Scrivi l'angolo di apertura del cofano o usa le frecce: le bielle si mettono da sole. Se si apre dalla \
     parte sbagliata premi «Inverti verso»; oltre il fine corsa il cofano si ferma dove arriva il meccanismo.",
    "Nella vista compaiono F1 F2 M1 M2 e le linee delle bielle: se le bielle risultano incrociate hai \
     invertito M1 e M2, premi «Scambia i perni mobili».",
];

/// Disegno del meccanismo come si vede di lato: piastra fissa, bielle, braccio e cofano, con i
/// perni numerati nell'ordine dei clic.
fn draw_mechanism(ui: &mut egui::Ui) {
    // Disegnato su 330 × 230 e ridotto alla larghezza della finestra (mai ingrandito: la finestra
    // si allargherebbe a ogni disegno)
    let k = ((ui.available_width() - 2.0) / 330.0).clamp(0.5, 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(330.0, 230.0) * k, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let p = |x: f32, y: f32| rect.min + egui::vec2(x, y) * k;
    let text = ui.visuals().text_color();
    let label = |pos: egui::Pos2, anchor: egui::Align2, t: &str, color: egui::Color32| {
        painter.text(pos, anchor, t, egui::FontId::proportional(12.0 * k.min(1.0).max(0.8)), color);
    };
    let w = |x: f32| x * k;
    let (fixed, link1, link2, arm, hood) = (
        egui::Color32::from_rgb(220, 0, 220),
        egui::Color32::from_rgb(235, 220, 40),
        egui::Color32::from_rgb(40, 190, 60),
        egui::Color32::from_rgb(40, 70, 230),
        egui::Color32::from_rgb(200, 100, 20),
    );
    // Cofano e braccio
    painter.line_segment([p(20.0, 30.0), p(320.0, 105.0)], egui::Stroke::new(w(7.0), hood));
    label(p(250.0, 75.0), egui::Align2::RIGHT_BOTTOM, "cofano", text);
    let (m1, m2) = (p(95.0, 72.0), p(185.0, 95.0));
    painter.line_segment([p(70.0, 66.0), p(215.0, 103.0)], egui::Stroke::new(w(14.0), arm));
    label(p(222.0, 118.0), egui::Align2::LEFT_TOP, "cerniera mobile\n(braccio sul cofano)", text);
    // Piastra fissa
    let plate = vec![p(60.0, 190.0), p(220.0, 190.0), p(210.0, 222.0), p(70.0, 222.0)];
    painter.add(egui::Shape::convex_polygon(plate, fixed, egui::Stroke::NONE));
    label(p(225.0, 205.0), egui::Align2::LEFT_CENTER, "cerniera fissa\n(sulla scocca)", text);
    let (f1, f2) = (p(85.0, 195.0), p(180.0, 195.0));
    // Bielle
    painter.line_segment([f1, m1], egui::Stroke::new(w(12.0), link1));
    painter.line_segment([f2, m2], egui::Stroke::new(w(12.0), link2));
    label(p(70.0, 140.0), egui::Align2::RIGHT_CENTER, "biella 1", text);
    label(p(196.0, 150.0), egui::Align2::LEFT_CENTER, "biella 2", text);
    // Perni, con l'ordine dei clic
    for (c, name) in [(f1, "1  F1"), (f2, "2  F2"), (m1, "3  M1"), (m2, "4  M2")] {
        painter.circle(c, w(7.0), egui::Color32::from_gray(150), egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
        let pos = c + egui::vec2(-12.0, -8.0) * k;
        painter.text(pos, egui::Align2::RIGHT_BOTTOM, name, egui::FontId::proportional(14.0), egui::Color32::from_rgb(255, 140, 30));
    }
}

/// Disegno dei 3 clic su un perno: a triangolo sì, vicini o in fila no.
fn draw_three_points(ui: &mut egui::Ui) {
    let k = ((ui.available_width() - 2.0) / 330.0).clamp(0.6, 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(330.0 * k, 130.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let text = ui.visuals().text_color();
    let accent = egui::Color32::from_rgb(255, 140, 30);
    let (good, bad) = (egui::Color32::from_rgb(60, 190, 90), egui::Color32::from_rgb(230, 70, 60));
    let r = 34.0;
    for (i, ok) in [(0, true), (1, false)] {
        let c = rect.min + egui::vec2((85.0 + 165.0 * i as f32) * k, 50.0);
        painter.circle(c, r, egui::Color32::from_gray(150), egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
        let angles: [f32; 3] = if ok { [-90.0, 30.0, 150.0] } else { [-20.0, 0.0, 20.0] };
        let pts: Vec<egui::Pos2> = angles.iter().map(|a| c + egui::vec2(a.to_radians().cos(), a.to_radians().sin()) * r).collect();
        if ok {
            painter.add(egui::Shape::closed_line(pts.clone(), egui::Stroke::new(1.5_f32, good)));
            painter.circle_stroke(c, 3.0, egui::Stroke::new(1.5_f32, good));
        }
        for (i, q) in pts.iter().enumerate() {
            painter.circle(*q, 4.5, accent, egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
            painter.text(*q + (*q - c).normalized() * 11.0, egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(12.0), text);
        }
        let (caption, color) = if ok { ("Sì: 3 punti lontani, a triangolo", good) } else { ("No: punti vicini o in fila", bad) };
        painter.text(c + egui::vec2(0.0, r + 22.0), egui::Align2::CENTER_CENTER, caption, egui::FontId::proportional(12.0), color);
        if !ok {
            let d = r * 0.8;
            for (a, b) in [(egui::vec2(-d, -d), egui::vec2(d, d)), (egui::vec2(-d, d), egui::vec2(d, -d))] {
                painter.line_segment([c + a, c + b], egui::Stroke::new(3.0_f32, bad));
            }
        }
    }
}

/// Finestra con la guida illustrata della cerniera a 4 perni.
fn draw_four_bar_guide(ctx: &egui::Context, open: &mut bool) {
    // A sinistra della finestra Posizione, per non coprirla
    let guide_pos = ctx
        .memory(|m| m.area_rect(egui::Id::new("finestra_posizione")))
        .map_or(ctx.screen_rect().right_top() + egui::vec2(-650.0, 60.0), |r| egui::pos2((r.left() - 370.0).max(0.0), r.top()));
    egui::Window::new(format!("{}  Guida 4 perni", icon::INFO))
        .id(egui::Id::new("guida_quattro_perni"))
        .open(open)
        .resizable([true, false])
        .min_width(220.0)
        .default_width(320.0)
        .collapsible(true)
        .constrain(true)
        .default_height((ctx.screen_rect().height() - 120.0).max(150.0))
        .default_pos(guide_pos)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 120.0).max(150.0)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                draw_mechanism(ui);
                ui.add_space(6.0);
                draw_three_points(ui);
                ui.add_space(6.0);
                for (i, step) in FOUR_BAR_STEPS.iter().enumerate() {
                    ui.label(format!("{}. {step}", i + 1));
                    ui.add_space(3.0);
                }
                ui.label(
                    egui::RichText::new(
                        "Basta una cerniera: il cofano si muove tutto. Le bielle dell'altro lato si possono \
                         selezionare insieme a quelle di questo lato se le due cerniere sono allineate.",
                    )
                    .small()
                    .weak(),
                );
            });
        });
}

/// Il meccanismo dai 12 punti cliccati (3 sul bordo di ogni perno).
fn build_four_bar(p: &[Vec3]) -> Result<FourBar, String> {
    let mut pins = [(Vec3::ZERO, Vec3::Z); 4];
    for (k, pin) in pins.iter_mut().enumerate() {
        let [a, b, c] = p.get(k * 3..k * 3 + 3).and_then(|s| <[Vec3; 3]>::try_from(s).ok()).ok_or("Punti mancanti")?;
        *pin = circle_through(a, b, c).ok_or_else(|| format!("{}: i tre punti sono in fila, cliccali lontani a triangolo sul bordo", PIN_NAMES[k]))?;
    }
    FourBar::from_pins(pins)
}

impl HingeMode {
    /// Clic necessari.
    pub fn points(self) -> usize {
        match self {
            HingeMode::TwoPoints => 2,
            HingeMode::TwoHoles => 6,
            HingeMode::OneHole => 3,
            HingeMode::PointAxis(_) => 1,
            HingeMode::TwoPositions => 0,
            HingeMode::FourBar => 12,
        }
    }

    /// Cosa cliccare per il punto `i` (da 0).
    pub fn prompt(self, i: usize) -> String {
        match self {
            HingeMode::TwoPoints => ["Clic sul primo punto dell'asse", "Clic sul secondo punto dell'asse"][i.min(1)].to_string(),
            HingeMode::TwoHoles => format!("Foro {}: clic sul bordo, punto {} di 3", i / 3 + 1, i % 3 + 1),
            HingeMode::OneHole => format!("Clic sul bordo del foro, punto {} di 3", i + 1),
            HingeMode::PointAxis(_) => "Clic sul punto per cui passa l'asse".to_string(),
            HingeMode::TwoPositions => String::new(),
            HingeMode::FourBar => format!("perno {} di 4, {}: punto {} di 3 sul bordo (a triangolo)", (i / 3).min(3) + 1, PIN_NAMES[(i / 3).min(3)], i % 3 + 1),
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
#[derive(Debug, Clone, PartialEq)]
pub struct Hinge {
    pub mode: HingeMode,
    /// Il clic nella vista prende i punti dell'asse invece di selezionare
    pub picking: bool,
    pub points: Vec<Vec3>,
    /// Punto e direzione (unitaria) dell'asse
    pub axis: Option<(Vec3, Vec3)>,
    /// Angolo applicato da quando l'asse è stato scelto (gradi)
    pub angle: f32,
    /// Passo delle frecce e del cursore dell'angolo (gradi), separato dall'Unità della trasformazione
    pub step: f32,
    pub error: Option<String>,
    /// «Da due posizioni»: nodi della prima e della seconda posizione e moto trovato
    pub first: Vec<u32>,
    pub second: Vec<u32>,
    pub motion: Option<Motion>,
    /// Distanza voluta tra i due punti della Misura (mm)
    pub wanted: f32,
    /// «4 perni»: meccanismo, parte che si apre, bielle 1 e 2
    pub four: Option<FourBar>,
    pub arm: Vec<u32>,
    pub links: [Vec<u32>; 2],
    /// Un pezzo della parte che si apre e la sua posizione con il meccanismo a 0°: dopo Annulla o
    /// altre modifiche si ritrova a che angolo è il meccanismo
    pub arm_ref: Option<(u32, Mat4)>,
}

impl Default for Hinge {
    fn default() -> Self {
        Self {
            mode: HingeMode::default(),
            picking: false,
            points: Vec::new(),
            axis: None,
            angle: 0.0,
            step: 1.0,
            error: None,
            first: Vec::new(),
            second: Vec::new(),
            motion: None,
            wanted: 0.0,
            four: None,
            arm: Vec::new(),
            links: [Vec::new(), Vec::new()],
            arm_ref: None,
        }
    }
}

/// Passi dell'angolo della cerniera
const HINGE_STEPS: [f32; 6] = [0.1, 1.0, 5.0, 10.0, 45.0, 90.0];

impl Hinge {
    /// Inizia la scelta dell'asse.
    pub fn start(&mut self) {
        self.picking = true;
        self.points.clear();
        self.axis = None;
        self.angle = 0.0;
        self.error = None;
        self.four = None;
        self.arm_ref = None;
    }

    /// Punto cliccato nella vista; all'ultimo calcola l'asse.
    pub fn add_point(&mut self, p: Vec3) {
        if !self.picking {
            return;
        }
        self.points.push(p);
        if self.points.len() >= self.mode.points() {
            self.picking = false;
            if self.mode == HingeMode::FourBar {
                match build_four_bar(&self.points) {
                    Ok(four) => {
                        self.four = Some(four);
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.points.clear();
                    }
                }
                return;
            }
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
            painter.line_segment([a, b], egui::Stroke::new(2.0_f32, accent));
        }
        if let (Some(a), Some(b)) = (to_screen(o), to_screen(o + d * scene_size.max(1.0) * 0.05)) {
            painter.arrow(a, b - a, egui::Stroke::new(3.0_f32, accent));
        }
    }
    // «4 perni»: bielle e braccio nella posizione attuale, oppure i perni già cliccati
    let label_pin = |pos: Vec3, name: &str| {
        if let Some(s) = to_screen(pos) {
            painter.circle(s, 6.0, egui::Color32::TRANSPARENT, egui::Stroke::new(2.0_f32, accent));
            painter.text(s + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(14.0), accent);
        }
    };
    let names = ["F1", "F2", "M1", "M2"];
    if let Some(four) = hinge.four.as_ref().filter(|_| hinge.mode == HingeMode::FourBar) {
        let pins = four.pins(&four.state);
        let link = egui::Color32::from_rgb(255, 200, 40);
        let arm = egui::Color32::from_rgb(60, 200, 255);
        for (a, b, color) in [(0, 2, link), (1, 3, link), (2, 3, arm)] {
            if let (Some(p), Some(q)) = (to_screen(pins[a]), to_screen(pins[b])) {
                painter.line_segment([p, q], egui::Stroke::new(2.5_f32, color));
            }
        }
        for (p, name) in pins.iter().zip(names) {
            label_pin(*p, name);
        }
        return;
    }
    if hinge.mode == HingeMode::FourBar {
        for (k, c) in hinge.points.chunks_exact(3).enumerate() {
            if let Some((center, _)) = circle_through(c[0], c[1], c[2]) {
                label_pin(center, names[k.min(3)]);
            }
        }
    }
    for p in &hinge.points {
        if let Some(s) = to_screen(*p) {
            painter.circle(s, 4.5, accent, egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
        }
    }
    if let Some(text) = hinge.prompt() {
        let what = if hinge.mode == HingeMode::FourBar { "Perni della cerniera" } else { "Asse della cerniera" };
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            format!("{what}: {text}  ·  Esc per annullare"),
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

/// Sezione richiudibile; `open` si aggiorna quando si apre o si chiude (resta nelle impostazioni).
fn section<R>(ui: &mut egui::Ui, id: &str, title: &str, open: &mut bool, body: impl FnOnce(&mut egui::Ui) -> R) {
    let r = egui::CollapsingHeader::new(egui::RichText::new(title).strong()).id_salt(id).default_open(*open).show(ui, body);
    if r.header_response.clicked() {
        *open = !*open;
    }
}

/// Frecce, cursore e campo dell'angolo (gradi).
fn angle_row(ui: &mut egui::Ui, angle: &mut f32, step: f32, hint: &str) {
    let slider = (ui.available_width() - 175.0).clamp(40.0, 200.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("Angolo");
        if ui.small_button(icon::CARET_LEFT).clicked() {
            *angle -= step;
        }
        ui.spacing_mut().slider_width = slider;
        // Il cursore non riarrotonda i valori scritti a mano (es. 23° con passo 10)
        ui.add(egui::Slider::new(angle, -180.0..=180.0).step_by(step as f64).clamping(egui::SliderClamping::Never).show_value(false));
        if ui.small_button(icon::CARET_RIGHT).clicked() {
            *angle += step;
        }
        ui.add(egui::DragValue::new(angle).speed(step).max_decimals(3).suffix(" °")).on_hover_text(hint);
    });
}

/// Scelta del passo dell'angolo.
fn step_row(ui: &mut egui::Ui, step: &mut f32) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Passo");
        egui::ComboBox::from_id_salt("posizione_passo_cerniera")
            .width(70.0)
            .selected_text(format!("{} °", step))
            .show_ui(ui, |ui| {
                for v in HINGE_STEPS {
                    ui.selectable_value(step, v, format!("{v} °"));
                }
            })
            .response
            .on_hover_text("Passo delle frecce e del cursore dell'angolo");
    });
}

/// Nome di un gruppo di nodi scelto con «Usa selezione», accorciato per la finestra, e intero.
fn slot_text(app: &GestoreApp, slot: &[u32]) -> (String, String) {
    let text = match slot {
        [] => "—".to_string(),
        [n] => node_label(app, Some(*n)),
        many => format!("{} elementi", many.len()),
    };
    // Nomi lunghi (percorsi dei file) accorciati: il pulsante resta nella finestra
    let short = if text.chars().count() > 24 {
        format!("…{}", text.chars().rev().take(23).collect::<Vec<_>>().into_iter().rev().collect::<String>())
    } else {
        text.clone()
    };
    (short, text)
}

fn pose_grid(ui: &mut egui::Ui, _id: &str, pose: &Pose) {
    // Righe che vanno a capo: la finestra si può stringere
    for (first, values, unit) in [(0, [pose.xyz.x, pose.xyz.y, pose.xyz.z], "mm"), (3, [pose.abc.x, pose.abc.y, pose.abc.z], "°")] {
        ui.horizontal_wrapped(|ui| {
            for (k, v) in values.iter().enumerate() {
                ui.label(egui::RichText::new(AXES[first + k].0).color(AXES[first + k].1).strong());
                ui.monospace(format!("{:+.3}", v + 0.0));
                ui.add_space(4.0);
            }
            ui.label(unit);
        });
    }
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
    {
        // «4 perni»: posizione di riferimento della parte che si apre, presa con il meccanismo a 0°
        let h = &mut panel.hinge;
        if let Some(four) = &h.four {
            if h.arm_ref.is_none() {
                // Posizione che avrebbe a 0°: quella attuale riportata indietro dal meccanismo
                let back = four.transforms(&four.state)[0].inverse();
                h.arm_ref = scene.meshes_of(&h.arm).first().map(|&m| (m, back * scene.placement(m)));
            }
        }
    }
    if revision != panel.revision {
        let h = &mut panel.hinge;
        if let (Some(four), HingeMode::FourBar) = (h.four.as_mut(), h.mode) {
            // Annulla o altre modifiche: l'angolo si ricava dalla posizione della parte che si apre
            if let Some((mesh, start)) = h.arm_ref {
                match four.state_from_arm(scene.placement(mesh) * start.inverse()) {
                    Some(s) => {
                        four.state = s;
                        h.angle = s.phi.to_degrees() as f32;
                        if four.at_start() {
                            h.angle = 0.0;
                        }
                    }
                    None => {
                        h.four = None;
                        h.arm_ref = None;
                        h.points.clear();
                        h.angle = 0.0;
                        h.error = Some("La parte che si apre è stata spostata in un altro modo: scegli di nuovo i perni".to_string());
                    }
                }
            }
        } else {
            // Modifica fatta altrove (Annulla, altri comandi): l'angolo riparte da 0
            h.angle = 0.0;
        }
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
    // Misura tra un punto fermo e uno del pezzo che ruota: (punto che ruota, punto fermo, calamita)
    let measured = app.ui_state.measure.as_ref().and_then(|m| {
        let [a, b] = m.points.as_slice() else { return None };
        let moving = scene.meshes_of(&nodes);
        let moves = |p: &crate::ui::measure::MeasurePoint| p.anchor.is_some_and(|x| moving.binary_search(&x.mesh).is_ok());
        match (moves(a), moves(b)) {
            (true, false) => Some((a.pos, b.pos, m.magnet)),
            (false, true) => Some((b.pos, a.pos, m.magnet)),
            _ => None,
        }
    });

    let mut open = true;
    let mut actions: Vec<Action> = Vec::new();
    let mut copy: Option<String> = None;
    let mut status: Option<String> = None;
    let mut start_picking = false;
    // Dopo «Calcola l'asse»: i pezzi della prima posizione che si muovono diventano l'oggetto da ruotare
    let mut select_first: Option<Vec<u32>> = None;
    let mut sections = app.settings.position_sections;
    // Su schermi bassi la finestra non deve uscire: il contenuto scorre
    let max_height = (ctx.screen_rect().height() - 120.0).max(150.0);

    egui::Window::new(format!("{}  Posizione", icon::ARROWS_OUT_CARDINAL))
        .id(egui::Id::new("finestra_posizione"))
        .open(&mut open)
        // Si allarga e si stringe trascinando il bordo; il testo va a capo. L'altezza segue il
        // contenuto, fino a quella dello schermo (poi scorre)
        .resizable([true, false])
        .min_width(220.0)
        .default_width(270.0)
        .collapsible(true)
        .constrain(true)
        // Lo spazio per lo scorrimento è alto quanto lo schermo (altrimenti egui lo ferma a 420)
        .default_height(max_height)
        .default_pos(ctx.screen_rect().right_top() + egui::vec2(-300.0, 60.0))
        .show(ctx, |ui| {
            // Il contenuto prende la larghezza della finestra, non di più: con auto_shrink spento la
            // barra di scorrimento allargava la finestra a ogni disegno fino al bordo dello schermo
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_enabled_ui(!busy, |ui| {
                // ── Oggetto ────────────────────────────────────────────────────
                section(ui, "posizione_sez_oggetto", "Oggetto", &mut sections[0], |ui| {
                let label = match target {
                    Some(Target::File(n)) => files.iter().find(|(f, _)| *f == n).map(|(_, s)| s.clone()).unwrap_or_default(),
                    Some(Target::Selection) => format!("Pezzi selezionati ({})", selection.len()),
                    None => "—".to_string(),
                };
                egui::ComboBox::from_id_salt("posizione_oggetto").width((ui.available_width() - 40.0).max(80.0)).selected_text(label).show_ui(ui, |ui| {
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
                ui.horizontal_wrapped(|ui| {
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
                });

                // ── Trasformazione ─────────────────────────────────────────────
                section(ui, "posizione_sez_trasformazione", "Trasformazione", &mut sections[1], |ui| {
                ui.horizontal_wrapped(|ui| {
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
                let slider = (ui.available_width() - 185.0).clamp(40.0, 200.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Modifica");
                    if ui.small_button(icon::CARET_LEFT).clicked() {
                        panel.value -= panel.unit;
                    }
                    let range = if rotation { 180.0 } else { (panel.unit * 200.0).max(panel.value.abs()) };
                    ui.spacing_mut().slider_width = slider;
                    ui.add(egui::Slider::new(&mut panel.value, -range..=range).step_by(panel.unit as f64).clamping(egui::SliderClamping::Never).show_value(false));
                    if ui.small_button(icon::CARET_RIGHT).clicked() {
                        panel.value += panel.unit;
                    }
                    ui.add(egui::DragValue::new(&mut panel.value).speed(panel.unit).max_decimals(3).suffix(suffix))
                        .on_hover_text("Trascina, oppure fai doppio clic e scrivi il valore");
                });
                ui.horizontal_wrapped(|ui| {
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
                });

                // ── Rotazione su un asse (cerniere) ─────────────────────────────
                section(ui, "posizione_cerniera", "Rotazione su cerniera", &mut sections[2], |ui| {
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
                            ui.selectable_value(&mut h.mode, HingeMode::TwoPositions, "Da due posizioni").on_hover_text(
                                "Lo stesso pezzo in due posizioni (es. chiuso e aperto, due matematiche già pronte): \
                                 l'asse si calcola dal movimento, anche senza cerniere nel 3D",
                            );
                            ui.selectable_value(&mut h.mode, HingeMode::FourBar, "4 perni (cofano)").on_hover_text(
                                "Cerniera con due bielle: due perni sulla cerniera fissa e due sulla mobile. \
                                 Il cofano non gira su un asse fisso: le bielle si calcolano in automatico",
                            );
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
                                    h.motion = None;
                                    h.error = None;
                                    h.four = None;
                                    h.arm_ref = None;
                                }
                            }
                            h.angle = 0.0;
                        }
                        if h.mode == HingeMode::TwoPositions {
                            egui::Grid::new("posizione_due_posizioni").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                                for (label, slot) in [("Prima posizione", &mut h.first), ("Seconda posizione", &mut h.second)] {
                                    ui.label(label);
                                    let text = match slot.as_slice() {
                                        [] => "—".to_string(),
                                        [n] => node_label(app, Some(*n)),
                                        many => format!("{} elementi", many.len()),
                                    };
                                    // Nomi lunghi (percorsi dei file) accorciati: il pulsante resta nella finestra
                                    let short = if text.chars().count() > 24 {
                                        format!("…{}", text.chars().rev().take(23).collect::<Vec<_>>().into_iter().rev().collect::<String>())
                                    } else {
                                        text.clone()
                                    };
                                    ui.label(egui::RichText::new(short).strong()).on_hover_text(text);
                                    if ui
                                        .add_enabled(!selection.is_empty(), egui::Button::new("Usa selezione").small())
                                        .on_hover_text("Il file o i pezzi selezionati nell'albero")
                                        .clicked()
                                    {
                                        *slot = selection.clone();
                                    }
                                    ui.end_row();
                                }
                            });
                            if ui
                                .add_enabled(!h.first.is_empty() && !h.second.is_empty(), egui::Button::new(icon_text(ui, icon::CROSSHAIR, "Calcola l'asse")))
                                .on_hover_text("Confronta i pezzi delle due posizioni; la prima è quella che poi si ruota")
                                .clicked()
                            {
                                match scene.motion_between(&h.first, &h.second) {
                                    Ok(m) => {
                                        h.axis = Some((m.point, m.dir));
                                        h.angle = 0.0;
                                        h.points.clear();
                                        h.error = None;
                                        status = Some(format!("Asse trovato: {:.2}° tra le due posizioni", m.angle));
                                        // Si ruotano solo i pezzi che si muovono: le parti fisse restano
                                        let mut moving: Vec<u32> = if m.fixed + m.other + m.unmatched == 0 {
                                            h.first.clone()
                                        } else {
                                            m.moving_meshes.iter().filter_map(|&x| scene.mesh_to_node.get(x as usize).copied()).collect()
                                        };
                                        moving.sort_unstable();
                                        moving.dedup();
                                        h.motion = Some(m);
                                        select_first = Some(moving);
                                    }
                                    Err(e) => {
                                        h.axis = None;
                                        h.motion = None;
                                        h.error = Some(e);
                                    }
                                }
                            }
                            if let Some(m) = &h.motion {
                                let mut lines = vec![
                                    format!("Angolo tra le posizioni {:.2}°", m.angle),
                                    format!("Pezzi che ruotano {}, fermi {}", m.moving, m.fixed),
                                    format!("Scarto {:.3} mm, scorrimento lungo l'asse {:.3} mm", m.rms, m.slide.abs()),
                                ];
                                if m.other > 0 {
                                    lines.push(format!("{} pezzi si muovono in un altro modo (es. pistoni): esclusi", m.other));
                                }
                                if m.unmatched > 0 {
                                    lines.push(format!("{} pezzi senza corrispondente: esclusi", m.unmatched));
                                }
                                ui.label(egui::RichText::new(lines.join("\n")).small());
                                if m.slide.abs() > 1.0 {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(230, 160, 40),
                                        "Non è una rotazione pura: il pezzo scorre anche lungo l'asse",
                                    );
                                }
                            }
                        }
                        if h.mode == HingeMode::FourBar {
                            ui.label(egui::RichText::new(FOUR_BAR_HINT).small());
                            if ui
                                .add(egui::Button::new(icon_text(ui, icon::INFO, "Guida illustrata")).small().selected(panel.guide))
                                .on_hover_text("Disegni e passi: quali perni cliccare e in che ordine")
                                .clicked()
                            {
                                panel.guide = !panel.guide;
                            }
                            // Le parti si cambiano solo a 0°: si muovono dalla posizione dei perni cliccati
                            // Spostamento dei pezzi di ogni parte dalla posizione dei perni cliccati a quella
                            // attuale: un pezzo tolto torna chiuso, uno aggiunto si porta all'apertura attuale
                            let current = h.four.as_ref().map(|f| f.transforms(&f.state));
                            let mut slot_moves: Vec<Action> = Vec::new();
                            let mut arm_changed = false;
                            let [link1, link2] = &mut h.links;
                            {
                                for (k, (label, hint, slot)) in [
                                    ("Parte che si apre", "Cofano e braccio della cerniera mobile (con i suoi perni)", &mut h.arm),
                                    ("Biella 1", "Biella tra perno fisso 1 e perno mobile 1 (facoltativa)", link1),
                                    ("Biella 2", "Biella tra perno fisso 2 e perno mobile 2 (facoltativa)", link2),
                                ]
                                .into_iter()
                                .enumerate()
                                {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(label).on_hover_text(hint);
                                        let (short, full) = slot_text(app, slot);
                                        ui.label(egui::RichText::new(short).strong()).on_hover_text(full);
                                        let mut replace: Option<Vec<u32>> = None;
                                        if ui
                                            .add_enabled(!selection.is_empty(), egui::Button::new("Usa selezione").small())
                                            .on_hover_text("Il file o i pezzi selezionati nell'albero (al posto di quelli indicati prima)")
                                            .on_disabled_hover_text("Seleziona prima i pezzi nell'albero o nella vista")
                                            .clicked()
                                        {
                                            replace = Some(selection.clone());
                                        }
                                        if ui.add_enabled(!slot.is_empty(), egui::Button::new("×").small()).on_hover_text("Togli (i pezzi tornano chiusi)").clicked() {
                                            replace = Some(Vec::new());
                                        }
                                        if let Some(new) = replace {
                                            if let Some(t) = current.filter(|_| h.four.as_ref().is_some_and(|f| !f.at_start())) {
                                                if !slot.is_empty() {
                                                    slot_moves.push(Action::Move { nodes: slot.clone(), matrix: t[k].inverse() });
                                                }
                                                if !new.is_empty() {
                                                    slot_moves.push(Action::Move { nodes: new.clone(), matrix: t[k] });
                                                }
                                            }
                                            *slot = new;
                                            arm_changed |= k == 0;
                                        }
                                    });
                                }
                            }
                            if arm_changed {
                                h.arm_ref = None;
                            }
                            if !slot_moves.is_empty() {
                                actions.push(Action::Batch(slot_moves));
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            if h.mode == HingeMode::TwoPositions {
                                // L'asse si calcola dalle due posizioni, non dai clic
                            } else if h.picking {
                                if ui.button("Annulla scelta").clicked() {
                                    h.picking = false;
                                    h.points.clear();
                                }
                                ui.label(egui::RichText::new(h.prompt().unwrap_or_default()).italics());
                            } else if h.mode == HingeMode::FourBar {
                                let text = if h.four.is_some() { "Scegli di nuovo i perni" } else { "Scegli i 4 perni nella vista" };
                                if ui
                                    .add_enabled(h.four.as_ref().is_none_or(FourBar::at_start), egui::Button::new(icon_text(ui, icon::CROSSHAIR, text)))
                                    .on_hover_text("3 clic sul bordo di ogni perno: prima i 2 della cerniera fissa, poi i 2 della mobile")
                                    .on_disabled_hover_text("Riporta prima il cofano a 0°")
                                    .clicked()
                                {
                                    h.start();
                                    start_picking = true;
                                }
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
                        if let (HingeMode::FourBar, Some(four)) = (h.mode, h.four.as_mut()) {
                            let warn = egui::Color32::from_rgb(230, 160, 40);
                            let [l1, l2, l3] = four.lengths();
                            ui.label(
                                egui::RichText::new(format!("Biella 1 {l1:.1} mm  ·  biella 2 {l2:.1} mm  ·  tra i perni mobili {l3:.1} mm"))
                                    .small()
                                    .weak(),
                            );
                            if four.skew_deg > 1.0 {
                                ui.colored_label(warn, format!("I perni non sono del tutto paralleli (scarto {:.1}°): si usa la direzione media", four.skew_deg));
                            }
                            let at_zero = four.at_start();
                            if four.crossed() {
                                ui.colored_label(
                                    warn,
                                    "Le bielle risultano incrociate: di solito vuol dire che i perni mobili sono stati cliccati \
                                     nell'ordine sbagliato. Se nel pezzo non si incrociano, premi «Scambia i perni mobili».",
                                );
                            }
                            if ui
                                .add_enabled(at_zero, egui::Button::new("Scambia i perni mobili").small())
                                .on_hover_text("Il mobile 1 diventa il mobile 2: per correggere l'abbinamento perno fisso ↔ perno mobile")
                                .on_disabled_hover_text("Riporta prima il cofano a 0°")
                                .clicked()
                            {
                                let mut p = h.points.clone();
                                if p.len() == 12 {
                                    p[6..12].rotate_left(3);
                                    match build_four_bar(&p) {
                                        Ok(f) => {
                                            *four = f;
                                            h.points = p;
                                            h.error = None;
                                        }
                                        Err(e) => h.error = Some(e),
                                    }
                                }
                            }
                            if h.arm.is_empty() {
                                ui.colored_label(warn, "Indica la parte che si apre (cofano + braccio) per poterla ruotare");
                            }
                            let before = h.angle;
                            angle_row(ui, &mut h.angle, h.step, "Apertura del cofano dalla posizione in cui sono stati scelti i perni");
                            step_row(ui, &mut h.step);
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button("Inverti verso").on_hover_text("L'angolo positivo apre dall'altra parte").clicked() {
                                    four.flip();
                                    h.angle = -h.angle;
                                }
                                if ui.add_enabled(!at_zero, egui::Button::new("Torna a 0°").small()).on_hover_text("Riporta il cofano dov'era quando hai scelto i perni").clicked() {
                                    h.angle = 0.0;
                                }
                            });
                            if h.angle != before && !h.arm.is_empty() {
                                let s = match four.solve(h.angle) {
                                    Ok(s) => {
                                        h.error = None;
                                        s
                                    }
                                    Err(s) => {
                                        h.error = Some(format!(
                                            "Fine corsa: il meccanismo arriva a {:.2}°",
                                            s.phi.to_degrees()
                                        ));
                                        s
                                    }
                                };
                                let (old, new) = (four.transforms(&four.state), four.transforms(&s));
                                let moves: Vec<Action> = [&h.arm, &h.links[0], &h.links[1]]
                                    .into_iter()
                                    .zip(old.iter().zip(new.iter()))
                                    .filter(|(nodes, _)| !nodes.is_empty())
                                    .map(|(nodes, (o, n))| Action::Move { nodes: nodes.clone(), matrix: *n * o.inverse() })
                                    .collect();
                                actions.push(Action::Batch(moves));
                                four.state = s;
                                h.angle = s.phi.to_degrees() as f32;
                                if four.at_start() || h.angle.abs() < 1e-4 {
                                    h.angle = 0.0;
                                }
                            } else if h.angle != before {
                                h.angle = before;
                            }
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
                            angle_row(ui, &mut h.angle, h.step, "Angolo dalla posizione in cui è stato scelto l'asse");
                            // Quota presa con il metro: l'angolo che porta la Misura a quella distanza
                            if let Some((moving, fixed, magnet)) = measured {
                                if h.wanted <= 0.0 {
                                    h.wanted = (moving.distance(fixed) * 10.0).round() / 10.0;
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Distanza");
                                    ui.add(egui::DragValue::new(&mut h.wanted).speed(0.1).range(0.0..=f32::MAX).max_decimals(2).suffix(" mm"))
                                        .on_hover_text("Distanza voluta tra i due punti della Misura (es. presa con il metro in cabina)");
                                    if ui
                                        .add_enabled(magnet, egui::Button::new("Porta a questa distanza"))
                                        .on_hover_text("Ruota il pezzo dell'angolo più piccolo che dà questa distanza")
                                        .on_disabled_hover_text("Accendi la calamita della Misura: il punto deve seguire il pezzo")
                                        .clicked()
                                        && !nodes.is_empty()
                                    {
                                        match angle_for_distance(o, d, moving, fixed, h.wanted) {
                                            Ok(delta) => {
                                                // Lo spostamento lo fa il controllo dell'angolo qui sotto
                                                h.angle += delta;
                                                h.error = None;
                                                status = Some(format!("Ruotato di {delta:.2}°: distanza {:.2} mm", h.wanted));
                                            }
                                            Err((min, max)) => {
                                                h.error = Some(format!("Distanza non raggiungibile ruotando su questo asse: da {min:.1} a {max:.1} mm"));
                                            }
                                        }
                                    }
                                });
                                if !magnet {
                                    ui.label(egui::RichText::new("Accendi la calamita della Misura per portare la distanza").small().weak());
                                }
                            } else if app.ui_state.measure.is_some() {
                                ui.label(
                                    egui::RichText::new("Misura con un punto fermo e uno sul pezzo che ruota: qui si porta la distanza a una quota")
                                        .small()
                                        .weak(),
                                );
                            }
                            step_row(ui, &mut h.step);
                            if h.angle != before && !nodes.is_empty() {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: about_axis(o, d, h.angle - before) });
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button("Inverti verso").on_hover_text("L'angolo positivo gira dall'altra parte").clicked() {
                                    h.axis = Some((o, -d));
                                    h.angle = -h.angle;
                                }
                                if let Some(m) = h.motion.as_ref().filter(|_| h.mode == HingeMode::TwoPositions) {
                                    let target = if d.dot(m.dir) >= 0.0 { m.angle } else { -m.angle };
                                    if ui
                                        .small_button(format!("Vai alla seconda posizione ({target:.2}°)"))
                                        .on_hover_text("Ruota la prima posizione fino a sovrapporla alla seconda")
                                        .clicked()
                                        && !nodes.is_empty()
                                    {
                                        actions.push(Action::Move { nodes: nodes.clone(), matrix: about_axis(o, d, target - h.angle) });
                                        h.angle = target;
                                    }
                                }
                            });
                        }
                    });

                // ── Applica offset ─────────────────────────────────────────────
                section(ui, "posizione_offset", "Applica offset", &mut sections[3], |ui| {
                        // Righe che vanno a capo: la finestra si può stringere
                        ui.horizontal_wrapped(|ui| {
                            for k in 0..3 {
                                ui.label(egui::RichText::new(AXES[k].0).color(AXES[k].1).strong());
                                ui.add(egui::DragValue::new(&mut panel.offset.xyz[k]).speed(0.1).max_decimals(3).suffix(" mm"));
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            for k in 0..3 {
                                ui.label(egui::RichText::new(AXES[k + 3].0).color(AXES[k + 3].1).strong());
                                ui.add(egui::DragValue::new(&mut panel.offset.abc[k]).speed(0.1).max_decimals(3).suffix(" °"));
                            }
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
                                    .desired_width(f32::INFINITY)
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
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Applica").on_hover_text("Sposta l'oggetto di questo offset").clicked() && !nodes.is_empty() {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: in_frame(panel.offset.matrix(), frame, panel.world) });
                            }
                            if ui.small_button("Azzera").clicked() {
                                panel.offset = Pose::default();
                            }
                        });
                    });

                // ── Calcola offset ─────────────────────────────────────────────
                section(ui, "posizione_calcolo", "Calcola offset", &mut sections[4], |ui| {
                        ui.horizontal_wrapped(|ui| {
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
                                            let title = format!("{}  ->  {}", node_label(app, Some(r)), node_label(app, Some(d)));
                                            let a = Pose::from_matrix(scene.object_frame(r));
                                            // Stessa geometria: lo scostamento viene dai vertici, quindi vale anche
                                            // quando la posizione è scritta nelle coordinate (file caricati separati,
                                            // export di Dima 3D). Altrimenti dai Transform del file, come lo script.
                                            match scene.geometric_offset(r, d) {
                                                Ok(fit) => {
                                                    let m = a.matrix();
                                                    panel.result_note = Some("Calcolato dalla geometria: i due oggetti sono lo stesso pezzo".to_string());
                                                    Ok((title, Pose::from_matrix(m.inverse() * fit.matrix * m)))
                                                }
                                                Err(why) => {
                                                    let b = Pose::from_matrix(scene.object_frame(d));
                                                    panel.result_note = Some(format!(
                                                        "Calcolato dai Transform del file (non dalla geometria: {why})"
                                                    ));
                                                    Ok((title, a.offset_to(&b)))
                                                }
                                            }
                                        }
                                        (Some(_), Some(_)) => Err("Riferimento e destinazione devono essere diversi".to_string()),
                                        _ => Err("Scegli riferimento e destinazione".to_string()),
                                    });
                                }
                            }
                            CalcMode::Bases => {
                                for (label, text) in [("Origine", &mut panel.base_a), ("Destinazione", &mut panel.base_b)] {
                                    ui.label(label);
                                    ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("CONST BASE NOME = X Y Z A B C"));
                                }
                                if ui.button("Calcola").clicked() {
                                    panel.result_note = None;
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
                            if let Some(note) = &panel.result_note {
                                ui.label(egui::RichText::new(note).weak().small());
                            }
                            ui.horizontal_wrapped(|ui| {
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
        });
    if panel.guide {
        draw_four_bar_guide(ctx, &mut panel.guide);
    }
    if sections != app.settings.position_sections {
        let mut s = app.settings.clone();
        s.position_sections = sections;
        app.apply_settings(s);
    }

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
    if let Some(first) = select_first {
        let file = match first.as_slice() {
            [n] => files.iter().any(|(f, _)| f == n).then_some(*n),
            _ => None,
        };
        app.ui_state.select_single(first.first().copied());
        app.ui_state.selected_extra.extend(first.iter().skip(1).copied());
        panel.last_selection = app.ui_state.selected_nodes();
        panel.target = Some(file.map_or(Target::Selection, Target::File));
    }
    app.ui_state.position = panel;
    if start_picking {
        // La pipetta si spegne; la misura resta (con la calamita serve mentre si ruota) ma i clic
        // vanno alla scelta dell'asse finché non è finita
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
            // Un disegno in più: dopo un cambiamento la finestra (ridimensionabile) si assesta
            self.run(app, vec![]);
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

    /// Lo stesso pezzo caricato due volte, con lo spostamento già scritto nelle coordinate (come
    /// nell'export di Dima 3D o in due file caricati separati): dai Transform sarebbe 0, dalla
    /// geometria è lo spostamento vero.
    #[test]
    fn offset_of_two_copies_with_the_move_in_the_coordinates() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let (rif, other) = (node(&app, "Riferimento", 0), node(&app, "Riferimento", 1));
        let moved = Mat4::from_translation(Vec3::new(-200.0, 0.0, 700.0)) * Mat4::from_rotation_y(0.209_439);
        {
            let s = app.scene_graph.as_mut().unwrap();
            for m in s.meshes_of(&[other]) {
                crate::scene_graph::placement::transform_shape(&mut s.shapes_data[m as usize], moved);
            }
            let by_transform = Pose::from_matrix(s.object_frame(rif)).offset_to(&Pose::from_matrix(s.object_frame(other)));
            assert!(by_transform.matrix().abs_diff_eq(Mat4::IDENTITY, 1e-3), "dai Transform non si vede: {by_transform:?}");
        }
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Calcola offset", 0);
        app.ui_state.select_single(Some(rif));
        ui.click(&mut app, "Usa selezione", 0);
        app.ui_state.select_single(Some(other));
        ui.click(&mut app, "Usa selezione", 1);
        ui.click(&mut app, "Calcola", 0);
        let frame = Pose::from_matrix(app.scene_graph.as_ref().unwrap().object_frame(rif)).matrix();
        let expected = frame.inverse() * moved * frame;
        let result = app.ui_state.position.result.expect("offset calcolato");
        assert!(result.matrix().abs_diff_eq(expected, 1e-2), "{result:?} invece di {:?}", Pose::from_matrix(expected));
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.starts_with("Calcolato dalla geometria")), "metodo indicato");

        // «Applica» sul riferimento con questo offset lo porta sulla copia spostata
        app.ui_state.position.offset = result;
        app.ui_state.select_single(Some(rif));
        ui.click(&mut app, "Applica", 0);
        let (va, vb) = (first_vertex(&app, rif), first_vertex(&app, other));
        assert!((va - vb).length() < 1e-2, "{va:?} {vb:?}");
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
        assert_eq!(app.ui_state.position.hinge.step, 1.0, "passo di 1° di default");
        app.ui_state.position.hinge.step = 90.0;
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

        // Un valore scritto a mano fuori passo resta com'è (il cursore non lo riporta al passo)
        app.ui_state.position.hinge.step = 10.0;
        app.ui_state.position.hinge.angle = 23.0;
        ui.run(&mut app, vec![]);
        ui.run(&mut app, vec![]);
        assert_eq!(app.ui_state.position.hinge.angle, 23.0);
    }

    /// Cerniera a 4 perni nel piano XZ (perni lungo Y): F1 (0,0), F2 (50,0), M1 (-10,80),
    /// M2 (45,70). Braccio e bielle hanno un vertice su ogni perno.
    const COFANO: &[u8] = br#"#VRML V2.0 utf8
DEF Braccio Group { children [ Shape { geometry IndexedFaceSet { coord Coordinate { point [ -10 0 80, 45 0 70, 20 0 120 ] } coordIndex [ 0 1 2 -1 ] } } ] }
DEF Biella1 Group { children [ Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, -10 0 80, -5 5 40 ] } coordIndex [ 0 1 2 -1 ] } } ] }
DEF Biella2 Group { children [ Shape { geometry IndexedFaceSet { coord Coordinate { point [ 50 0 0, 45 0 70, 47 5 35 ] } coordIndex [ 0 1 2 -1 ] } } ] }
DEF Piastra Group { children [ Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 50 0 0, 25 0 -20 ] } coordIndex [ 0 1 2 -1 ] } } ] }
"#;

    /// Si scelgono i perni, la parte che si apre e le bielle; la freccia dell'angolo apre il
    /// cofano tenendo uniti i perni, con un solo Annulla; dopo Annulla l'angolo torna a 0.
    #[test]
    fn four_pin_hinge_opens_the_hood_with_its_links() {
        let mut app = GestoreApp::new_headless();
        let mut scene = crate::parser::parse_wrl(COFANO);
        scene.into_file("cofano.wrl".into(), None);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.ui_state.position.open = true;
        let ui = Ui { ctx: egui::Context::default() };
        let [arm, l1, l2, plate] = ["Braccio", "Biella1", "Biella2", "Piastra"].map(|n| node(&app, n, 0));
        let wid = |ui: &Ui| ui.ctx.memory(|m| m.area_rect(egui::Id::new("finestra_posizione"))).map(|r| r.width());
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "4 perni (cofano)", 0);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.contains("Su ogni perno 3 punti sul bordo, a triangolo")), "promemoria visibile");
        // La finestra resta stretta e alta quanto il contenuto, non quanto lo schermo
        let window = ui.ctx.memory(|m| m.area_rect(egui::Id::new("finestra_posizione"))).expect("finestra");
        assert!(window.width() < 300.0 && window.height() < 900.0, "{window:?}");
        ui.click(&mut app, "Guida illustrata", 0);
        let texts = ui.run(&mut app, vec![]);
        for wanted in ["1  F1", "4  M2", "cerniera fissa
(sulla scocca)", "Sì: 3 punti lontani, a triangolo"] {
            assert!(texts.iter().any(|(t, _)| t == wanted), "{wanted}: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        }
        let (guide, window) = ui.ctx.memory(|m| (m.area_rect(egui::Id::new("guida_quattro_perni")), m.area_rect(egui::Id::new("finestra_posizione"))));
        assert!(!guide.unwrap().intersects(window.unwrap()), "la guida si apre accanto alla finestra, non sopra: {guide:?} {window:?}");
        assert!(guide.unwrap().width() < 360.0, "{guide:?}");
        ui.click(&mut app, "Guida illustrata", 0);
        assert!(!app.ui_state.position.guide);
        for (k, n) in [arm, l1, l2].into_iter().enumerate() {
            app.ui_state.select_single(Some(n));
            ui.click(&mut app, "Usa selezione", k);
        }
        assert_eq!(app.ui_state.position.hinge.arm, vec![arm]);
        assert_eq!(app.ui_state.position.hinge.links, [vec![l1], vec![l2]]);
        ui.click(&mut app, "Scegli i 4 perni nella vista", 0);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.contains("perno 1 di 4, F1 (cerniera fissa, perno 1): punto 1 di 3 sul bordo (a triangolo)")));
        assert!(wid(&ui).unwrap() < 300.0, "il testo del clic va a capo: la finestra resta stretta");
        for (x, z) in [(0.0, 0.0), (50.0, 0.0), (-10.0, 80.0), (45.0, 70.0)] {
            for (dx, dz) in [(3.0, 0.0), (0.0, 3.0), (-3.0, 0.0)] {
                app.ui_state.position.hinge.add_point(Vec3::new(x + dx, 0.0, z + dz));
            }
        }
        assert!(app.ui_state.position.hinge.four.is_some(), "{:?}", app.ui_state.position.hinge.error);
        ui.run(&mut app, vec![]);

        let vertex = |app: &GestoreApp, n: u32, i: usize| {
            let s = app.scene_graph.as_ref().unwrap();
            Vec3::from(s.shapes_data[s.meshes_of(&[n])[0] as usize].world_vertices[i])
        };
        app.ui_state.position.hinge.step = 10.0;
        // Con questi clic il verso positivo chiude il cofano, già quasi a fine corsa: si inverte
        ui.click(&mut app, "Inverti verso", 0);
        // Seconda freccia a destra: quella dell'angolo (la prima è della Trasformazione)
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        assert!((app.ui_state.position.hinge.angle - 20.0).abs() < 1e-3, "{}", app.ui_state.position.hinge.angle);
        let (m1, m2) = (vertex(&app, arm, 0), vertex(&app, arm, 1));
        assert!(m1.distance(Vec3::new(-10.0, 0.0, 80.0)) > 5.0, "il braccio si è mosso");
        assert!(m1.distance(vertex(&app, l1, 1)) < 1e-2 && m2.distance(vertex(&app, l2, 1)) < 1e-2, "perni mobili uniti alle bielle");
        assert!(vertex(&app, l1, 0).distance(Vec3::ZERO) < 1e-3 && vertex(&app, l2, 0).distance(Vec3::new(50.0, 0.0, 0.0)) < 1e-3, "perni fissi fermi");
        assert!((m1.distance(Vec3::ZERO) - 80.6226).abs() < 1e-2 && (m2.distance(Vec3::new(50.0, 0.0, 0.0)) - 70.1783).abs() < 1e-2);
        assert!((m1.distance(m2) - 55.9017).abs() < 1e-2);
        assert_eq!(vertex(&app, plate, 2), Vec3::new(25.0, 0.0, -20.0), "la piastra non si muove");
        assert_eq!(app.project.undo.len(), 1, "i passi dell'angolo sono un solo Annulla");

        // Il braccio ruota di 20° attorno a Y (verso dato dal primo perno)
        let turn = (m2 - m1).normalize().dot(Vec3::new(55.0, 0.0, -10.0).normalize()).acos().to_degrees();
        assert!((turn - 20.0).abs() < 1e-2, "{turn}");

        app.undo();
        ui.run(&mut app, vec![]);
        assert!(vertex(&app, arm, 0).distance(Vec3::new(-10.0, 0.0, 80.0)) < 1e-3, "annullato");
        assert_eq!(app.ui_state.position.hinge.angle, 0.0, "dopo Annulla l'angolo segue la posizione");
        assert!(app.ui_state.position.hinge.four.is_some(), "i perni restano");
        app.redo();
        ui.run(&mut app, vec![]);
        assert!((app.ui_state.position.hinge.angle - 20.0).abs() < 1e-3, "dopo Ripeti di nuovo 20°");

        // Oltre il fine corsa il cofano si ferma dove arriva il meccanismo
        app.ui_state.position.hinge.step = 25.0;
        ui.click(&mut app, icon::CARET_LEFT, 1);
        let h = &app.ui_state.position.hinge;
        assert!((h.angle + 0.956).abs() < 1e-2, "{}", h.angle);
        assert!(h.error.as_deref().is_some_and(|e| e.starts_with("Fine corsa")), "{:?}", h.error);
        assert!(vertex(&app, arm, 0).distance(vertex(&app, l1, 1)) < 1e-2);

        // Parte sbagliata a cofano aperto: si cambia lo stesso. La piastra messa per errore come
        // biella 1 si muove con lei e, tolta, torna al suo posto; la biella giusta torna unita al braccio
        app.ui_state.position.hinge.step = 10.0;
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        ui.click(&mut app, icon::CARET_RIGHT, 1);
        let plate_start = vertex(&app, plate, 2);
        app.ui_state.select_single(Some(plate));
        ui.click(&mut app, "Usa selezione", 1);
        assert_eq!(app.ui_state.position.hinge.links[0], vec![plate]);
        assert!(vertex(&app, plate, 2).distance(plate_start) > 1.0, "la piastra segue la biella 1");
        assert!(vertex(&app, l1, 0).distance(Vec3::ZERO) < 1e-3 && vertex(&app, l1, 1).distance(Vec3::new(-10.0, 0.0, 80.0)) < 1e-3, "la biella tolta torna chiusa");
        app.ui_state.select_single(Some(l1));
        ui.click(&mut app, "Usa selezione", 1);
        assert_eq!(app.ui_state.position.hinge.links[0], vec![l1]);
        assert!(vertex(&app, plate, 2).distance(plate_start) < 1e-3, "la piastra torna al suo posto");
        assert!(vertex(&app, arm, 0).distance(vertex(&app, l1, 1)) < 1e-2, "biella di nuovo unita al braccio");
        ui.click(&mut app, "×", 1);
        assert!(app.ui_state.position.hinge.links[0].is_empty());
        assert!(vertex(&app, l1, 1).distance(Vec3::new(-10.0, 0.0, 80.0)) < 1e-3);

        // Tornato a 0° (anche se il calcolo dà «-0,000°») si possono scegliere di nuovo i perni
        ui.click(&mut app, "Torna a 0°", 0);
        assert!(app.ui_state.position.hinge.four.as_ref().unwrap().at_start());
        assert_eq!(app.ui_state.position.hinge.angle, 0.0);
        ui.click(&mut app, "Scegli di nuovo i perni", 0);
        assert!(app.ui_state.position.hinge.picking);
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

    /// Prova su matematiche vere (non nel repo): GESTORE_PROVA_CERNIERA = WRL separati da ';'.
    /// Ogni freccia con passo 1° ruota il file di un grado attorno a un asse Z per un suo vertice.
    #[test]
    #[ignore]
    fn hinge_one_degree_steps_on_real_files() {
        let Ok(list) = std::env::var("GESTORE_PROVA_CERNIERA") else { return };
        for path in list.split(';') {
            let mut app = GestoreApp::new_headless();
            let mut scene = crate::parser::parse_wrl(&std::fs::read(path).unwrap());
            scene.into_file(path.into(), None);
            app.project.on_loaded(scene.revision);
            app.scene_graph = Some(scene);
            app.ui_state.position.open = true;
            app.ui_state.select_single(Some(0));
            let ui = Ui { ctx: egui::Context::default() };
            ui.run(&mut app, vec![]);
            let mesh = app.scene_graph.as_ref().unwrap().meshes_of(&[0])[0] as usize;
            let vertex = |app: &GestoreApp, i: usize| Vec3::from(app.scene_graph.as_ref().unwrap().shapes_data[mesh].world_vertices[i]);
            let n = app.scene_graph.as_ref().unwrap().shapes_data[mesh].world_vertices.len();
            let (pivot, far) = (vertex(&app, 0), vertex(&app, n - 1));
            let h = &mut app.ui_state.position.hinge;
            h.mode = HingeMode::PointAxis(2);
            h.start();
            h.add_point(pivot);
            assert!(h.axis.is_some());
            let mut last = far;
            for k in 1..=25 {
                ui.click(&mut app, icon::CARET_RIGHT, 1);
                assert_eq!(app.ui_state.position.hinge.angle, k as f32);
                let now = vertex(&app, n - 1);
                let want = about_axis(pivot, Vec3::Z, k as f32).transform_point3(far);
                assert!((now - want).length() < 1e-2 * far.distance(pivot).max(1.0), "{path} {k}°: {now:?} invece di {want:?}");
                assert!(now != last || far.truncate() == pivot.truncate(), "{path} {k}°: non si è mosso");
                last = now;
            }
            println!("{path}: 25 passi da 1° ok, {} pezzi", app.scene_graph.as_ref().unwrap().shapes_data.len());
        }
    }

    /// «Da due posizioni»: sportello chiuso e aperto (85° attorno a un asse noto) in due file;
    /// l'asse trovato porta il file chiuso sopra quello aperto, con un solo Annulla.
    #[test]
    fn hinge_axis_from_two_positions() {
        let point = Vec3::new(1000.0, -500.0, 950.0);
        let dir = Vec3::new(0.0, 0.3, 1.0).normalize();
        let scene = crate::scene_graph::motion::tests::scene_with(about_axis(point, dir, 85.0));
        let mut app = GestoreApp::new_headless();
        app.project.on_loaded(scene.revision);
        let files: Vec<u32> = scene.open_files().map(|f| f.node).collect();
        app.scene_graph = Some(scene);
        app.ui_state.position.open = true;
        let ui = Ui { ctx: egui::Context::default() };
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Da due posizioni", 0);
        app.ui_state.select_single(Some(files[0]));
        ui.click(&mut app, "Usa selezione", 0);
        app.ui_state.select_single(Some(files[1]));
        ui.click(&mut app, "Usa selezione", 1);
        ui.click(&mut app, "Calcola l'asse", 0);
        let h = &app.ui_state.position.hinge;
        let m = h.motion.clone().unwrap_or_else(|| panic!("{:?}", h.error));
        assert!((m.angle - 85.0).abs() < 0.01, "{m:?}");
        // Si ruota solo lo sportello della prima posizione: la cerniera fissa resta
        let first_door = {
            let s = app.scene_graph.as_ref().unwrap();
            s.mesh_to_node[s.meshes_of(&[files[0]])[1] as usize]
        };
        assert_eq!(app.ui_state.position.target, Some(Target::Selection));
        assert_eq!(app.ui_state.selected_nodes(), vec![first_door]);

        let door = |app: &GestoreApp, file: usize| {
            let s = app.scene_graph.as_ref().unwrap();
            s.meshes_of(&[files[file]]).iter().map(|&m| s.shapes_data[m as usize].world_vertices.clone()).collect::<Vec<_>>()
        };
        let closed = door(&app, 0);
        ui.click(&mut app, "Vai alla seconda posizione (85.00°)", 0);
        assert_eq!(app.ui_state.position.hinge.angle, m.angle);
        let (now, open) = (door(&app, 0), door(&app, 1));
        let worst = now.iter().flatten().zip(open.iter().flatten()).map(|(a, b)| Vec3::from(*a).distance(Vec3::from(*b))).fold(0.0, f32::max);
        assert!(worst < 0.05, "scarto {worst}");
        app.undo();
        let back = door(&app, 0).iter().flatten().zip(closed.iter().flatten()).map(|(a, b)| Vec3::from(*a).distance(Vec3::from(*b))).fold(0.0, f32::max);
        assert!(back < 0.01, "Annulla riporta lo sportello chiuso: {back}");
        assert_eq!(door(&app, 0)[0], closed[0], "la cerniera fissa non si è mossa");
    }

    /// Misura con calamita tra un punto fermo e un vertice del pezzo: «Porta a questa distanza»
    /// gira il pezzo sulla cerniera finché la distanza è quella scritta.
    #[test]
    fn hinge_brings_the_measure_to_a_distance() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let rif = node(&app, "Riferimento", 0);
        app.ui_state.select_single(Some(rif));
        ui.run(&mut app, vec![]);
        let h = &mut app.ui_state.position.hinge;
        h.mode = HingeMode::PointAxis(2);
        h.start();
        h.add_point(Vec3::new(100.0, 200.0, 300.0));
        let mesh = app.scene_graph.as_ref().unwrap().meshes_of(&[rif])[0];
        let vertex = |app: &GestoreApp| Vec3::from(app.scene_graph.as_ref().unwrap().shapes_data[mesh as usize].world_vertices[1]);
        let fixed = Vec3::new(100.0, 400.0, 300.0);
        let point = crate::ui::measure::MeasurePoint::free(vertex(&app), true);
        let point = crate::ui::measure::Measure::anchored(app.scene_graph.as_ref().unwrap(), mesh, point);
        app.ui_state.measure = Some(crate::ui::measure::Measure { points: vec![point, crate::ui::measure::MeasurePoint::free(fixed, false)], magnet: false });
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.starts_with("Accendi la calamita")), "senza calamita il comando è spento");
        app.ui_state.measure.as_mut().unwrap().magnet = true;
        ui.run(&mut app, vec![]);
        assert_eq!(app.ui_state.position.hinge.wanted, 150.0, "parte dalla distanza attuale");
        app.ui_state.position.hinge.wanted = 200.0;
        ui.click(&mut app, "Porta a questa distanza", 0);
        assert!((vertex(&app).distance(fixed) - 200.0).abs() < 1e-2, "{:?}", vertex(&app));
        assert_eq!(app.project.undo.len(), 1);
        // Irraggiungibile: messaggio con le distanze possibili, nessuno spostamento
        app.ui_state.position.hinge.wanted = 1000.0;
        ui.click(&mut app, "Porta a questa distanza", 0);
        let e = app.ui_state.position.hinge.error.clone().unwrap_or_default();
        assert!(e.contains("da 150.0 a 250.0 mm"), "{e}");
    }

    /// Come sopra sulle matematiche vere (non nel repo): GESTORE_PROVA_ASSE = «chiuso|aperto;…».
    #[test]
    #[ignore]
    fn hinge_axis_from_two_positions_on_real_files() {
        let Ok(list) = std::env::var("GESTORE_PROVA_ASSE") else { return };
        for pair in list.split(';') {
            let (a, b) = pair.split_once('|').unwrap();
            let mut scene = crate::parser::parse_wrl(&std::fs::read(a).unwrap());
            scene.into_file(a.into(), None);
            let mut other = crate::parser::parse_wrl(&std::fs::read(b).unwrap());
            other.into_file(b.into(), None);
            scene.append(other);
            let mut app = GestoreApp::new_headless();
            app.project.on_loaded(scene.revision);
            let files: Vec<u32> = scene.open_files().map(|f| f.node).collect();
            app.scene_graph = Some(scene);
            app.ui_state.position.open = true;
            let ui = Ui { ctx: egui::Context::default() };
            ui.run(&mut app, vec![]);
            ui.click(&mut app, "Da due posizioni", 0);
            app.ui_state.select_single(Some(files[0]));
            ui.click(&mut app, "Usa selezione", 0);
            app.ui_state.select_single(Some(files[1]));
            ui.click(&mut app, "Usa selezione", 1);
            ui.click(&mut app, "Calcola l'asse", 0);
            let m = app.ui_state.position.hinge.motion.clone().expect("asse");
            let texts = ui.run(&mut app, vec![]);
            let go = texts.iter().find(|(t, _)| t.starts_with("Vai alla seconda posizione")).expect("pulsante").0.clone();
            ui.click(&mut app, &go, 0);
            let s = app.scene_graph.as_ref().unwrap();
            let (ma, mb) = (s.meshes_of(&[files[0]]), s.meshes_of(&[files[1]]));
            let worst = ma
                .iter()
                .zip(&mb)
                .flat_map(|(x, y)| s.shapes_data[*x as usize].world_vertices.iter().zip(&s.shapes_data[*y as usize].world_vertices))
                .map(|(p, q)| Vec3::from(*p).distance(Vec3::from(*q)))
                .fold(0.0, f32::max);
            println!("{go}: {} pezzi ruotati, {} fermi; dopo la rotazione scarto massimo dalla seconda posizione {worst:.4} mm", m.moving, m.fixed);
            assert!(worst < 0.05);
        }
    }
}
