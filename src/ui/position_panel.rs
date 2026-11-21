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
use crate::i18n::tr;
use crate::app::{Action, GestoreApp};
use crate::scene_graph::bbox::Aabb;
use crate::scene_graph::linkage::FourBar;
use crate::scene_graph::motion::Motion;
use crate::scene_graph::placement::{
    about_axis, angle_for_distance, circle_through, offset_in_world, parse_const_base, parse_offset, parse_robot_base, Pose,
};

/// Cosa si sposta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Un file intero (nodo radice)
    File(u32),
    /// I nodi selezionati nell'albero o nella vista
    Selection,
}

/// Su cosa si allinea l'oggetto, lungo gli assi scelti della cabina.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AlignMode {
    /// Centro del riquadro (bounding box)
    #[default]
    Center,
    /// Lato basso del riquadro
    Min,
    /// Lato alto del riquadro
    Max,
    /// Due copie dello stesso pezzo: dalla geometria, anche se sono girate in modo diverso
    SamePiece,
}

impl AlignMode {
    const ALL: [(AlignMode, &'static str, &'static str); 4] = [
        (AlignMode::Center, tn!("Centro"), tn!("Centro del riquadro dei pezzi")),
        (AlignMode::Min, "Min", tn!("Lato basso del riquadro (X, Y o Z più piccola)")),
        (AlignMode::Max, "Max", tn!("Lato alto del riquadro (X, Y o Z più grande)")),
        (AlignMode::SamePiece, tn!("Stesso pezzo"), tn!("Due copie dello stesso pezzo (una sola parte per lato): si sovrappongono dalla geometria")),
    ];
}

/// Spostamento (coordinate della cabina) che porta il riquadro `moving` su `reference` (o sullo
/// zero della scena) lungo gli assi accesi.
pub fn align_delta(moving: &Aabb, reference: Option<&Aabb>, mode: AlignMode, axes: [bool; 3]) -> Vec3 {
    let pick = |b: &Aabb| match mode {
        AlignMode::Min => b.min,
        AlignMode::Max => b.max,
        AlignMode::Center | AlignMode::SamePiece => b.center(),
    };
    let to = reference.map_or(Vec3::ZERO, pick);
    only_axes(to - pick(moving), axes)
}

/// Solo le componenti degli assi accesi.
fn only_axes(v: Vec3, axes: [bool; 3]) -> Vec3 {
    Vec3::select(glam::BVec3::new(axes[0], axes[1], axes[2]), v, Vec3::ZERO)
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
    /// «Applica offset»: coordinate usate (X Y Z A B C); quelle spente valgono 0
    pub offset_use: [bool; 6],
    /// Campo in cui incollare il risultato degli script
    pub pasted: String,
    pub calc: CalcMode,
    pub reference: Option<u32>,
    pub destination: Option<u32>,
    pub base_a: String,
    pub base_b: String,
    /// Offset calcolato in coordinate del riferimento (come gli script e i BASE del robot)
    pub result: Option<Pose>,
    /// Riga dell'oggetto del robot a cui applicare l'offset («Nuovo oggetto del robot»)
    pub robot_line: String,
    /// Come è stato calcolato il risultato (dalla geometria o dai Transform)
    pub result_note: Option<String>,
    /// Ultimo messaggio di errore del calcolo
    pub error: Option<String>,
    /// Rotazione su cerniera
    pub hinge: Hinge,
    /// Guida illustrata della cerniera a 4 perni aperta
    pub guide: bool,
    /// Guida illustrata di «Allinea su misure» aperta
    pub measures_guide: bool,
    /// «Allinea»: su cosa (vuoto = zero della scena), lungo quali assi, come
    pub align_reference: Vec<u32>,
    pub align_axes: [bool; 3],
    pub align_mode: AlignMode,
    /// «Allinea su punti»: riferimenti presi in cabina e sul 3D
    pub points: super::point_align::PointAlign,
    /// «Misura dell'oggetto»: file local e world
    pub measure: super::object_measure::ObjectMeasure,
    /// «Allinea su misure»: quote prese a mano tra un riferimento fermo e il pezzo da spostare
    pub distances: super::distance_align::DistanceAlign,
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
            offset_use: [true; 6],
            pasted: String::new(),
            calc: CalcMode::Objects,
            reference: None,
            destination: None,
            base_a: String::new(),
            base_b: String::new(),
            result: None,
            robot_line: String::new(),
            result_note: None,
            error: None,
            hinge: Hinge::default(),
            guide: false,
            measures_guide: false,
            align_reference: Vec::new(),
            align_axes: [true, true, true],
            align_mode: AlignMode::Center,
            points: Default::default(),
            measure: Default::default(),
            distances: Default::default(),
        }
    }
}

impl PositionPanel {
    /// Progetto o file nuovo: tutto da capo, tranne finestra aperta, unità, World e passo
    /// dell'angolo (scelte di chi lavora, non valori del progetto).
    pub fn reset_values(&mut self) {
        let step = self.hinge.step;
        let mode = self.hinge.mode;
        *self = Self {
            open: self.open,
            unit: self.unit,
            world: self.world,
            calc: self.calc,
            guide: self.guide,
            offset_use: self.offset_use,
            measures_guide: self.measures_guide,
            align_axes: self.align_axes,
            align_mode: self.align_mode,
            hinge: Hinge { step, mode, ..Hinge::default() },
            ..Self::default()
        };
    }
}

const AXES: [(&str, egui::Color32, &str); 6] = [
    ("X", egui::Color32::from_rgb(230, 80, 80), tn!("Sposta lungo X")),
    ("Y", egui::Color32::from_rgb(80, 190, 90), tn!("Sposta lungo Y")),
    ("Z", egui::Color32::from_rgb(90, 130, 240), tn!("Sposta lungo Z")),
    ("A", egui::Color32::from_rgb(230, 80, 80), tn!("Ruota attorno a X")),
    ("B", egui::Color32::from_rgb(80, 190, 90), tn!("Ruota attorno a Y")),
    ("C", egui::Color32::from_rgb(90, 130, 240), tn!("Ruota attorno a Z")),
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
    tn!("F1 (cerniera fissa, perno 1)"),
    tn!("F2 (cerniera fissa, perno 2)"),
    tn!("M1 (cerniera mobile, sulla biella di F1)"),
    tn!("M2 (cerniera mobile, sulla biella di F2)"),
];

/// Promemoria nella finestra Posizione; la spiegazione completa è nella guida illustrata.
const FOUR_BAR_HINT: &str = tn!("Ordine: F1 e F2 sulla cerniera fissa, poi M1 e M2 sulla mobile \
(M1 sulla stessa biella di F1). Su ogni perno 3 punti sul bordo, a triangolo.");

/// Passi della guida illustrata.
const FOUR_BAR_STEPS: [&str; 6] = [
    tn!("Parti dalla posizione della matematica (di solito cofano chiuso) e premi «Scegli i 4 perni nella vista»."),
    tn!("Su ogni perno clicca 3 punti sul bordo del cerchio, lontani tra loro in modo da formare un triangolo \
     (non vicini e non in fila): dai 3 punti si trova il centro del perno."),
    tn!("Ordine dei perni: prima F1 e F2 sulla cerniera fissa (quella avvitata alla scocca), poi M1 e M2 sulla \
     cerniera mobile (il braccio avvitato al cofano). M1 è in cima alla biella che parte da F1, M2 in cima \
     a quella che parte da F2."),
    tn!("Con «Usa selezione» indica la parte che si apre (cofano + braccio) e, se vuoi che si muovano, le due bielle."),
    tn!("Scrivi l'angolo di apertura del cofano o usa le frecce: le bielle si mettono da sole. Se si apre dalla \
     parte sbagliata premi «Inverti verso»; oltre il fine corsa il cofano si ferma dove arriva il meccanismo."),
    tn!("Nella vista compaiono F1 F2 M1 M2 e le linee delle bielle: se le bielle risultano incrociate hai \
     invertito M1 e M2, premi «Scambia i perni mobili»."),
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
    label(p(250.0, 75.0), egui::Align2::RIGHT_BOTTOM, tr("cofano"), text);
    let (m1, m2) = (p(95.0, 72.0), p(185.0, 95.0));
    painter.line_segment([p(70.0, 66.0), p(215.0, 103.0)], egui::Stroke::new(w(14.0), arm));
    label(p(222.0, 118.0), egui::Align2::LEFT_TOP, tr("cerniera mobile\n(braccio sul cofano)"), text);
    // Piastra fissa
    let plate = vec![p(60.0, 190.0), p(220.0, 190.0), p(210.0, 222.0), p(70.0, 222.0)];
    painter.add(egui::Shape::convex_polygon(plate, fixed, egui::Stroke::NONE));
    label(p(225.0, 205.0), egui::Align2::LEFT_CENTER, tr("cerniera fissa\n(sulla scocca)"), text);
    let (f1, f2) = (p(85.0, 195.0), p(180.0, 195.0));
    // Bielle
    painter.line_segment([f1, m1], egui::Stroke::new(w(12.0), link1));
    painter.line_segment([f2, m2], egui::Stroke::new(w(12.0), link2));
    label(p(70.0, 140.0), egui::Align2::RIGHT_CENTER, tr("biella 1"), text);
    label(p(196.0, 150.0), egui::Align2::LEFT_CENTER, tr("biella 2"), text);
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
        let (caption, color) = if ok { (tr("Sì: 3 punti lontani, a triangolo"), good) } else { (tr("No: punti vicini o in fila"), bad) };
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
    egui::Window::new(format!("{}  {}", icon::INFO, tr("Guida 4 perni")))
        .id(egui::Id::new("guida_quattro_perni"))
        .open(open)
        .resizable([true, false])
        .min_width(super::dock::NARROW_WIDTH)
        .default_width(320.0)
        .collapsible(true)
        .constrain(true)
        .default_height((ctx.screen_rect().height() - 120.0).max(150.0))
        .default_pos(guide_pos)
        .show(ctx, |ui| {
            egui::ScrollArea::both().max_height((ctx.screen_rect().height() - 120.0).max(150.0)).show(ui, |ui| {
                ui.set_width(ui.available_width().max(220.0));
                draw_mechanism(ui);
                ui.add_space(6.0);
                draw_three_points(ui);
                ui.add_space(6.0);
                for (i, step) in FOUR_BAR_STEPS.iter().enumerate() {
                    ui.label(format!("{}. {}", i + 1, tr(step)));
                    ui.add_space(3.0);
                }
                ui.label(
                    egui::RichText::new(
                        tr("Basta una cerniera: il cofano si muove tutto. Le bielle dell'altro lato si possono \
                         selezionare insieme a quelle di questo lato se le due cerniere sono allineate."),
                    )
                    .small()
                    .weak(),
                );
            });
        });
}

/// «Allinea su misure», guida: perché un punto si fissa con 3 misure. Tre miniature con i cerchi
/// delle distanze dai punti del riferimento: 1 misura = ovunque sul cerchio, 2 = due posizioni,
/// 3 = una sola.
fn draw_trilateration(ui: &mut egui::Ui) {
    let k = ((ui.available_width() - 2.0) / 330.0).clamp(0.6, 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(330.0, 180.0) * k, egui::Sense::hover());
    let (refc, partc, line) = (egui::Color32::from_rgb(40, 110, 220), egui::Color32::from_rgb(255, 140, 30), egui::Color32::from_rgb(235, 190, 30));
    let (good, bad) = (egui::Color32::from_rgb(60, 190, 90), egui::Color32::from_rgb(230, 70, 60));
    let font = egui::FontId::proportional(11.5);
    // Punto dell'oggetto e tre punti del riferimento, in coordinate di una miniatura (110 × 145,
    // sotto la scritta); la seconda posizione con 2 misure è lo specchio rispetto a R1-R2
    let target = egui::vec2(55.0, 55.0);
    let refs = [egui::vec2(25.0, 90.0), egui::vec2(85.0, 90.0), egui::vec2(98.0, 55.0)];
    for (i, n) in [1usize, 2, 3].iter().enumerate() {
        let o = rect.min + egui::vec2(110.0 * i as f32, 0.0) * k;
        let p = |v: egui::Vec2| o + v * k;
        // Ogni miniatura disegna solo nel suo riquadro (i cerchi non vanno nella vicina)
        let painter = ui.painter_at(egui::Rect::from_min_size(o, egui::vec2(106.0, 145.0) * k));
        for r in &refs[..*n] {
            let radius = (target - *r).length() * k;
            painter.circle_stroke(p(*r), radius, egui::Stroke::new(1.0_f32, line.gamma_multiply(0.6)));
            painter.line_segment([p(*r), p(target)], egui::Stroke::new(2.0_f32, line));
        }
        for r in &refs[..*n] {
            painter.circle(p(*r), 4.5 * k.max(0.8), refc, egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
        }
        if *n == 2 {
            // Seconda posizione possibile: lo specchio rispetto alla retta tra i due riferimenti
            let mirror = egui::vec2(target.x, 2.0 * refs[0].y - target.y);
            painter.circle(p(mirror), 5.0 * k.max(0.8), partc.gamma_multiply(0.5), egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
            painter.text(p(mirror) + egui::vec2(9.0, 0.0), egui::Align2::LEFT_CENTER, "?", font.clone(), bad);
            painter.text(p(target) + egui::vec2(9.0, 0.0), egui::Align2::LEFT_CENTER, "?", font.clone(), bad);
        }
        painter.circle(p(target), 5.0 * k.max(0.8), partc, egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
        let (caption, color) = match n {
            1 => (tr("1 misura: ovunque\nsul cerchio"), bad),
            2 => (tr("2 misure: due\nposizioni possibili"), bad),
            _ => (tr("3 misure: una sola\nposizione"), good),
        };
        ui.painter_at(rect).text(p(egui::vec2(55.0, 148.0)), egui::Align2::CENTER_TOP, caption, font.clone(), color);
    }
}

/// «Allinea su misure», guida: miniatura di uno schema di misure tra il riferimento (sotto, con
/// i punti fissi F1..F4) e l'oggetto da spostare (sopra, con i punti M1 M2 M3). `lines` = (punto
/// dell'oggetto, punto del riferimento).
fn draw_measure_scheme(ui: &mut egui::Ui, title: &str, lines: &[(usize, usize)], color: egui::Color32) {
    let k = ((ui.available_width() - 2.0) / 330.0).clamp(0.6, 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 150.0) * k, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let p = |x: f32, y: f32| rect.min + egui::vec2(x, y) * k;
    let font = egui::FontId::proportional(11.5);
    let text = ui.visuals().text_color();
    // Oggetto da spostare (es. supporto del jig) e riferimento fermo (es. paraurti)
    let part = vec![p(45.0, 30.0), p(115.0, 30.0), p(108.0, 72.0), p(52.0, 72.0)];
    painter.add(egui::Shape::convex_polygon(part, egui::Color32::from_rgb(90, 200, 230), egui::Stroke::new(1.0_f32, egui::Color32::BLACK)));
    painter.rect_filled(egui::Rect::from_min_max(p(8.0, 112.0), p(152.0, 132.0)), 8.0 * k, egui::Color32::from_rgb(40, 110, 220));
    let a = [p(64.0, 68.0), p(96.0, 68.0), p(80.0, 36.0)];
    let r = [p(16.0, 114.0), p(60.0, 114.0), p(100.0, 114.0), p(144.0, 114.0)];
    for &(i, j) in lines {
        painter.line_segment([a[i], r[j]], egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(235, 190, 30)));
    }
    let used = |i: usize| lines.iter().any(|l| l.0 == i);
    for (i, (q, name)) in a.iter().zip(["M1", "M2", "M3"]).enumerate() {
        if used(i) {
            painter.circle(*q, 4.0, egui::Color32::from_rgb(255, 140, 30), egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
            painter.text(*q + egui::vec2(0.0, -6.0), egui::Align2::CENTER_BOTTOM, name, font.clone(), egui::Color32::BLACK);
        }
    }
    // Solo i punti del riferimento usati, numerati da sinistra
    let mut number = 0;
    for (j, q) in r.iter().enumerate() {
        if !lines.iter().any(|l| l.1 == j) {
            continue;
        }
        number += 1;
        painter.circle(*q, 3.5, egui::Color32::WHITE, egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
        painter.text(*q + egui::vec2(0.0, 22.0) * k, egui::Align2::CENTER_CENTER, format!("F{number}"), font.clone(), text);
    }
    painter.text(p(80.0, 2.0), egui::Align2::CENTER_TOP, title, font.clone(), color);
}

/// Finestra con la guida illustrata di «Allinea su misure».
fn draw_measures_guide(ctx: &egui::Context, open: &mut bool) {
    let guide_pos = ctx
        .memory(|m| m.area_rect(egui::Id::new("finestra_posizione")))
        .map_or(ctx.screen_rect().right_top() + egui::vec2(-650.0, 60.0), |r| egui::pos2((r.left() - 370.0).max(0.0), r.top()));
    let (good, ok, bad) = (egui::Color32::from_rgb(60, 190, 90), egui::Color32::from_rgb(230, 160, 40), egui::Color32::from_rgb(230, 70, 60));
    egui::Window::new(format!("{}  {}", icon::INFO, tr("Guida misure")))
        .id(egui::Id::new("guida_misure"))
        .open(open)
        .resizable([true, false])
        .min_width(super::dock::NARROW_WIDTH)
        .default_width(340.0)
        .collapsible(true)
        .constrain(true)
        .default_pos(guide_pos)
        .show(ctx, |ui| {
            egui::ScrollArea::both().max_height((ctx.screen_rect().height() - 120.0).max(150.0)).show(ui, |ui| {
                ui.set_width(ui.available_width().max(220.0));
                ui.label(egui::RichText::new(tr("Perché «Precisa: 3 punti fissi × 3 punti M»")).strong());
                ui.label(
                    tr("Con il metro misuri la distanza tra un punto fisso F del riferimento fermo (in blu, es. il paraurti) \
                     e un punto M dell'oggetto da spostare (in arancione, es. il supporto del jig). Una distanza sola non \
                     dice dove sta il punto M: può essere ovunque su un cerchio (nello spazio, su una sfera)."),
                );
                draw_trilateration(ui);
                ui.label(
                    tr("Con 3 distanze da 3 punti fissi diversi il punto M è fissato, come la punta di un treppiede. \
                     Un punto solo però non basta: l'oggetto ci potrebbe girare attorno. Con 3 punti M fissati così \
                     (M1, M2, M3, lontani tra loro) l'oggetto non può più né spostarsi né ruotare. Per questo in \
                     «Precisa» da ogni punto fisso si misurano gli stessi 3 punti M."),
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tr("Quante misure prendere")).strong());
                ui.horizontal_wrapped(|ui| {
                    draw_measure_scheme(
                        ui,
                        tr("Precisa: 3 F × 3 M"),
                        &[(0, 0), (0, 1), (0, 3), (1, 0), (1, 1), (1, 3), (2, 0), (2, 1), (2, 3)],
                        good,
                    );
                    draw_measure_scheme(ui, tr("Veloce: 4 misure"), &[(0, 0), (0, 1), (1, 2), (1, 3)], ok);
                    draw_measure_scheme(ui, tr("No: tutte da un punto"), &[(0, 0), (0, 1), (0, 2), (0, 3)], bad);
                });
                ui.add_space(4.0);
                for (color, t) in [
                    (good, tr("Precisa: almeno 3 punti fissi F (lontani tra loro), da ognuno le distanze verso 3 punti M \
                            dell'oggetto (lontani tra loro e non in fila), meglio gli stessi M per tutti: 9 misure. Il punto \
                            fisso si clicca una volta sola, poi i 3 punti M. Non c'è un massimo: più punti danno più \
                            controllo. La scritta verde «Schema completo» conferma che ci sei.")),
                    (ok, tr("Veloce: con meno misure (minimo 4) Dima sposta l'oggetto il meno possibile e scrive \
                          «Posizione approssimata». Va bene se sul 3D l'oggetto è già quasi giusto. Se è solo spostato e non \
                          ruotato, con «Solo spostamento» bastano 3 misure, anche ΔX, ΔY o ΔZ.")),
                    (bad, tr("Tutte le misure verso lo stesso punto M: il punto è fissato ma l'oggetto ci gira attorno.")),
                    (bad, tr("Punti F e M quasi in un piano (es. tutti alla stessa altezza): la misura non dice da che parte \
                            del piano sta l'oggetto. Prendi almeno un punto fisso più in alto o più in basso degli altri.")),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("●").color(color));
                        ui.label(egui::RichText::new(t).small());
                    });
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tr("Come si fa")).strong());
                for (i, step) in [
                    tr("In cima alla finestra Posizione scegli come «Oggetto» il pezzo da spostare."),
                    tr("In cabina misura con il metro da un punto del riferimento a un punto dell'oggetto. Scegli punti \
                     facili da ritrovare sul 3D: spigoli, angoli, centri dei fori."),
                    tr("Scegli «Veloce» o «Precisa». Precisa: «Nuovo punto fisso», clicca sul 3D il punto F e poi i 3 punti \
                     M dell'oggetto (il clic si aggancia al vertice vicino); ripeti per gli altri fissi. Veloce: «Nuova \
                     misura», un punto F e un punto M per volta. In alternativa misura con lo strumento Misura (M) e \
                     premi «Dalla Misura»."),
                    tr("Scrivi in «Misurato» il valore del metro. In Veloce, se hai misurato solo l'altezza (livella) o solo \
                     lungo un asse, cambia il tipo in ΔZ, ΔX o ΔY."),
                    tr("Premi «Allinea». La colonna «Dopo» dice quanto resta su ogni misura: un valore grande indica una \
                     misura letta o cliccata male, che puoi spegnere. Annulla rimette l'oggetto dov'era."),
                ]
                .iter()
                .enumerate()
                {
                    ui.label(format!("{}. {step}", i + 1));
                    ui.add_space(3.0);
                }
            });
        });
}

/// Il meccanismo dai 12 punti cliccati (3 sul bordo di ogni perno).
fn build_four_bar(p: &[Vec3]) -> Result<FourBar, String> {
    let mut pins = [(Vec3::ZERO, Vec3::Z); 4];
    for (k, pin) in pins.iter_mut().enumerate() {
        let [a, b, c] = p.get(k * 3..k * 3 + 3).and_then(|s| <[Vec3; 3]>::try_from(s).ok()).ok_or(tr("Punti mancanti"))?;
        *pin = circle_through(a, b, c)
            .ok_or_else(|| trf!("{pin}: i tre punti sono in fila, cliccali lontani a triangolo sul bordo", pin = tr(PIN_NAMES[k])))?;
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
            HingeMode::TwoPoints => [tr("Clic sul primo punto dell'asse"), tr("Clic sul secondo punto dell'asse")][i.min(1)].to_string(),
            HingeMode::TwoHoles => trf!("Foro {hole}: clic sul bordo, punto {n} di 3", hole = i / 3 + 1, n = i % 3 + 1),
            HingeMode::OneHole => trf!("Clic sul bordo del foro, punto {n} di 3", n = i + 1),
            HingeMode::PointAxis(_) => tr("Clic sul punto per cui passa l'asse").to_string(),
            HingeMode::TwoPositions => String::new(),
            HingeMode::FourBar => trf!(
                "perno {k} di 4, {pin}: punto {n} di 3 sul bordo (a triangolo)",
                k = (i / 3).min(3) + 1,
                pin = tr(PIN_NAMES[(i / 3).min(3)]),
                n = i % 3 + 1
            ),
        }
    }

    /// Asse (punto, direzione) dai punti cliccati.
    pub fn axis(self, p: &[Vec3]) -> Result<(Vec3, Vec3), &'static str> {
        let line = |a: Vec3, b: Vec3| {
            let d = b - a;
            if d.length_squared() > 1e-12 { Ok((a, d.normalize())) } else { Err(tr("I due punti coincidono")) }
        };
        match (self, p) {
            (HingeMode::TwoPoints, [a, b]) => line(*a, *b),
            (HingeMode::TwoHoles, [a, b, c, d, e, f]) => {
                let (c1, _) = circle_through(*a, *b, *c).ok_or(tr("I tre punti del primo foro sono allineati"))?;
                let (c2, _) = circle_through(*d, *e, *f).ok_or(tr("I tre punti del secondo foro sono allineati"))?;
                line(c1, c2)
            }
            (HingeMode::OneHole, [a, b, c]) => circle_through(*a, *b, *c).ok_or(tr("I tre punti del foro sono allineati")),
            (HingeMode::PointAxis(k), [a]) => {
                let mut d = Vec3::ZERO;
                d[k.min(2)] = 1.0;
                Ok((*a, d))
            }
            _ => Err(tr("Punti mancanti")),
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
    /// Nome della cerniera da salvare
    pub save_name: String,
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
            save_name: String::new(),
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
        let what = if hinge.mode == HingeMode::FourBar { tr("Perni della cerniera") } else { tr("Asse della cerniera") };
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            trf!("{what}: {text}  ·  Esc per annullare", what = what, text = text),
            egui::FontId::proportional(15.0),
            accent,
        );
    }
}

/// Asse per i centri dei perni: i due più lontani tra loro. `None` se coincidono.
pub fn pins_axis(centers: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let mut best: Option<(f32, Vec3, Vec3)> = None;
    for (i, &a) in centers.iter().enumerate() {
        for &b in &centers[i + 1..] {
            let d = a.distance(b);
            if best.is_none_or(|(m, _, _)| d > m) {
                best = Some((d, a, b));
            }
        }
    }
    let (d, a, b) = best?;
    if d < 1e-3 {
        return None;
    }
    // Verso: il primo componente più grande positivo (una porta: verso l'alto)
    let mut dir = (b - a) / d;
    let k = if dir.x.abs() >= dir.y.abs() && dir.x.abs() >= dir.z.abs() { 0 } else if dir.y.abs() >= dir.z.abs() { 1 } else { 2 };
    if dir[k] < 0.0 {
        dir = -dir;
    }
    Some(((a + b) / 2.0, dir))
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

/// Due offset uguali come si leggono (millesimi): la stessa matrice.
fn same_offset(a: &Pose, b: &Pose) -> bool {
    a.matrix().abs_diff_eq(b.matrix(), 5e-4)
}

/// Movimento elementare del cursore («Trasformazione»). World spento: assi e origine dell'oggetto.
/// World acceso: assi della cabina, ma le rotazioni girano attorno all'origine dell'oggetto, così
/// X Y Z dell'oggetto restano quelli impostati (come scrivere X Y Z A B C di una posa).
pub fn slider_motion(motion: Mat4, frame: Mat4, world: bool) -> Mat4 {
    if !world {
        return in_frame(motion, frame, false);
    }
    let origin = Pose::from_matrix(frame).xyz;
    Mat4::from_translation(origin) * motion * Mat4::from_translation(-origin)
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

/// Testo accorciato al centro (inizio…fine) se supera `max` caratteri.
fn middle_ellipsis(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 5 {
        return text.to_string();
    }
    let head = (max - 1) / 2;
    let tail = max - 1 - head;
    format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
}

/// Nome del file aperto che contiene `node`, se la scena ne ha più di uno.
fn file_label(app: &GestoreApp, node: u32) -> Option<String> {
    let s = app.scene_graph.as_ref()?;
    if s.open_files().count() < 2 {
        return None;
    }
    let source = s.source_of_node(node)?;
    s.sources.get(source).map(|f| f.name.clone())
}

/// Sezione richiudibile; `open` si aggiorna quando si apre o si chiude (resta nelle impostazioni)
/// e comanda la sezione: «Espandi» e «Comprimi» la cambiano per tutte.
fn section<R>(ui: &mut egui::Ui, id: &str, title: &str, open: &mut bool, body: impl FnOnce(&mut egui::Ui) -> R) {
    let r = egui::CollapsingHeader::new(egui::RichText::new(title).strong()).id_salt(id).open(Some(*open)).show(ui, body);
    if r.header_response.clicked() {
        *open = !*open;
    }
}

/// Frecce, cursore e campo dell'angolo (gradi).
fn angle_row(ui: &mut egui::Ui, angle: &mut f32, step: f32, hint: &str) {
    let slider = (ui.available_width() - 175.0).clamp(40.0, 200.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Angolo"));
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
        ui.label(tr("Passo"));
        egui::ComboBox::from_id_salt("posizione_passo_cerniera")
            .width(70.0)
            .selected_text(format!("{} °", step))
            .show_ui(ui, |ui| {
                for v in HINGE_STEPS {
                    ui.selectable_value(step, v, format!("{v} °"));
                }
            })
            .response
            .on_hover_text(tr("Passo delle frecce e del cursore dell'angolo"));
    });
}

/// Nome di un gruppo di nodi scelto con «Usa selezione», accorciato per la finestra, e intero.
fn slot_text(app: &GestoreApp, slot: &[u32]) -> (String, String) {
    let text = match slot {
        [] => "—".to_string(),
        [n] => node_label(app, Some(*n)),
        many => trf!("{n} elementi", n = many.len()),
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

/// «Misura dell'oggetto»: scelta del file local (0), world (1) o degli oggetti del robot (2).
fn pick_measure_file(m: &mut super::object_measure::ObjectMeasure, kind: u8, last_program: Option<&std::path::Path>) {
    let title = match kind {
        0 => tr("Apri il file local (punti presi sulla matematica)"),
        1 => tr("Apri il file world (punti presi con il robot)"),
        _ => tr("Apri il file degli oggetti del robot"),
    };
    let near = [m.local.as_ref(), m.world.as_ref()].into_iter().flatten().map(|f| f.path.clone()).next();
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(tr("Dati del robot (TID)"), &["tid", "TID"])
        .add_filter(tr("Tutti i file"), &["*"]);
    if let Some(dir) = near.as_deref().or(last_program).and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(path) = dialog.pick_file() else { return };
    match kind {
        0 | 1 => m.open(&path, kind == 1),
        _ => match m.read_bases(&path) {
            Ok(0) => m.error = Some(trf!("Nessun oggetto (CONST BASE o OBJECT) in {path}", path = path.display())),
            Ok(_) => m.error = None,
            Err(e) => m.error = Some(e),
        },
    }
}

/// «Misura dell'oggetto»: salva il report HTML e lo apre nel browser; ritorna il messaggio.
fn save_measure_report(m: &super::object_measure::ObjectMeasure) -> Option<String> {
    let (rows, Ok(res)) = m.compute() else { return None };
    let mut dialog = rfd::FileDialog::new().set_title(tr("Salva il report della misura")).add_filter(tr("Pagina HTML"), &["html", "htm"]);
    if let Some(p) = m.report_path() {
        if let Some(dir) = p.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = p.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
    }
    let path = dialog.save_file()?;
    let (y, mo, d, h, mi, s) = crate::log::local_now();
    let html = m.report_html(&rows, &res, &format!("{d:02}/{mo:02}/{y} {h:02}:{mi:02}:{s:02}"));
    Some(match std::fs::write(&path, html) {
        Ok(()) => {
            #[cfg(windows)]
            let _ = std::process::Command::new("explorer").arg(&path).spawn();
            trf!("Report salvato: {path}", path = path.display())
        }
        Err(e) => trf!("Report non salvato: {e}", e = e),
    })
}

/// «Applica offset»: l'offset con le sole coordinate accese (le spente valgono 0).
fn used_offset(p: &Pose, used: &[bool; 6]) -> Pose {
    let pick = |v: f32, on: bool| if on { v } else { 0.0 };
    Pose {
        xyz: Vec3::new(pick(p.xyz.x, used[0]), pick(p.xyz.y, used[1]), pick(p.xyz.z, used[2])),
        abc: Vec3::new(pick(p.abc.x, used[3]), pick(p.abc.y, used[4]), pick(p.abc.z, used[5])),
    }
}

/// Larghezza sotto cui il contenuto non si stringe più: compare la barra orizzontale.
pub const CONTENT_MIN_WIDTH: f32 = 260.0;

/// Testo dell'offset come lo scrivono gli script (si può incollare in «Applica offset»).
fn offset_text(title: &str, p: &Pose) -> String {
    format!(
        "{title}\n\nPOSIZIONE\nX   {:+12.3} mm\nY   {:+12.3} mm\nZ   {:+12.3} mm\n\nORIENTAMENTO\nA   {:+12.3}°\nB   {:+12.3}°\nC   {:+12.3}°",
        p.xyz.x, p.xyz.y, p.xyz.z, p.abc.x, p.abc.y, p.abc.z
    )
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp, pass: super::dock::Pass) {
    if !app.ui_state.position.open {
        return;
    }
    // Agganciata si disegna prima della vista, mobile dopo: l'altro passaggio non fa niente
    if (pass == super::dock::Pass::Docked) != (app.settings.dock_position.side != super::dock::Side::Floating) {
        return;
    }
    let mut dock = app.settings.dock_position;
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
                        h.error = Some(tr("La parte che si apre è stata spostata in un altro modo: scegli di nuovo i perni").to_string());
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

    // «Allinea su punti»: i punti presi sul 3D seguono i pezzi; riferimenti dal programma attivo
    panel.points.follow(scene);
    panel.distances.follow(scene);
    // Due punti presi con lo strumento Misura: diventano una misura di «Allinea su misure»
    let tool_points: Option<(crate::ui::measure::MeasurePoint, crate::ui::measure::MeasurePoint)> =
        app.ui_state.measure.as_ref().and_then(|m| match m.points.as_slice() {
            [a, b] => Some((*a, *b)),
            _ => None,
        });
    let program_points: Vec<(String, Vec3)> = app
        .ui_state
        .program
        .program
        .as_ref()
        .map(|p| p.used_points().into_iter().filter_map(|u| Some((u.name.clone(), u.point?.position()?))).collect())
        .unwrap_or_default();

    let mut open = true;
    let mut actions: Vec<Action> = Vec::new();
    let mut copy: Option<String> = None;
    let mut status: Option<String> = None;
    let mut start_picking = false;
    // «Misura dell'oggetto»: file da scegliere (0 local, 1 world, 2 oggetti), dopo la finestra
    let mut pick_measure: Option<u8> = None;
    let mut save_report = false;
    // Dopo «Calcola l'asse»: i pezzi della prima posizione che si muovono diventano l'oggetto da ruotare
    let mut select_first: Option<Vec<u32>> = None;
    let mut save_hinge: Option<(String, Vec<u32>, Vec3, Vec3, f32)> = None;
    // «4 perni»: nome, parte, bielle, perni a 0°, direzione, apertura
    #[allow(clippy::type_complexity)]
    let mut save_four: Option<(String, Vec<u32>, [Vec<u32>; 2], [Vec3; 4], Vec3, f32)> = None;
    let part_name = |nodes: &[u32]| match nodes {
        [] => "—".to_string(),
        [n] => scene.display_name(*n),
        many => trf!("{n} elementi", n = many.len()),
    };
    let saved_hinges: Vec<(usize, crate::scene_graph::hinge::SavedHinge, String)> =
        scene.live_hinges().map(|(i, h)| (i, h.clone(), part_name(&h.nodes))).collect();
    let selection_name = part_name(&selection);
    // Due o più pezzi selezionati: l'asse può passare per i loro centri (i perni della cerniera)
    let pins: Option<(Vec3, Vec3)> = (selection.len() >= 2)
        .then(|| {
            let centers: Vec<Vec3> = selection
                .iter()
                .map(|&n| scene.subtree_bounding_box(n))
                .filter(|b| b.is_valid())
                .map(|b| b.center())
                .collect();
            pins_axis(&centers)
        })
        .flatten();
    let mut hinge_cmds: Vec<super::hinge_label::HingeCmd> = Vec::new();
    let mut sections = app.settings.position_sections;
    // Su schermi bassi la finestra non deve uscire: il contenuto scorre
    let max_height = (ctx.screen_rect().height() - 120.0).max(150.0);

    // Si allarga e si stringe trascinando il bordo; il testo va a capo. Si aggancia ai bordi
    let window = super::dock::Dockable {
        id: "finestra_posizione",
        title: format!("{}  {}", icon::ARROWS_OUT_CARDINAL, tr("Posizione")),
        state: &mut dock,
        open: &mut open,
        default_width: 270.0,
        // La finestra si stringe fin quasi al titolo (dock::NARROW_WIDTH); sotto questa larghezza
        // il contenuto scorre in orizzontale
        min_width: CONTENT_MIN_WIDTH,
        // Lo spazio per lo scorrimento è alto quanto lo schermo (altrimenti egui lo ferma a 420)
        default_height: max_height,
        default_pos: ctx.screen_rect().right_top() + egui::vec2(-300.0, 60.0),
        scroll: true,
        fold: true,
    };
    super::dock::show(ctx, pass, window, |ui| {
            if let Some(open) = super::dock::fold_request(ui.ctx(), "finestra_posizione") {
                sections.fill(open);
            }
            // Il contenuto prende la larghezza della finestra, non di più: con auto_shrink spento la
            // barra di scorrimento allargava la finestra a ogni disegno fino al bordo dello schermo
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_enabled_ui(!busy, |ui| {
                // ── Oggetto ────────────────────────────────────────────────────
                section(ui, "posizione_sez_oggetto", tr("Oggetto"), &mut sections[0], |ui| {
                let label = match target {
                    Some(Target::File(n)) => files.iter().find(|(f, _)| *f == n).map(|(_, s)| s.clone()).unwrap_or_default(),
                    Some(Target::Selection) => trf!("Pezzi selezionati ({n})", n = selection.len()),
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
                            trf!("Pezzi selezionati ({n})", n = selection.len()),
                        );
                    });
                });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(tr("Coordinate dell'oggetto")).strong());
                    if ui.small_button(icon::COPY).on_hover_text(tr("Copia X Y Z A B C")).clicked() {
                        copy = Some(frame_pose.to_text());
                    }
                });
                pose_grid(ui, "posizione_coordinate", &frame_pose);
                ui.label(
                    egui::RichText::new(if has_frame {
                        tr("Dai Transform del file, con gli spostamenti fatti qui")
                    } else {
                        tr("Nessun Transform nel file: coincidono con la cabina")
                    })
                    .small()
                    .weak(),
                );
                });

                // ── Trasformazione ─────────────────────────────────────────────
                section(ui, "posizione_sez_trasformazione", tr("Trasformazione"), &mut sections[1], |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (k, (name, color, hint)) in AXES.iter().enumerate() {
                        let text = egui::RichText::new(*name).color(*color).strong().size(16.0);
                        if ui.add(egui::Button::new(text).selected(panel.axis == k).min_size(egui::vec2(36.0, 26.0))).on_hover_text(tr(hint)).clicked() {
                            panel.axis = k;
                        }
                    }
                });
                let rotation = panel.axis >= 3;
                let suffix = if rotation { " °" } else { " mm" };
                let before = panel.value;
                let slider = (ui.available_width() - 185.0).clamp(40.0, 200.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Modifica"));
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
                        .on_hover_text(tr("Trascina, oppure fai doppio clic e scrivi il valore"));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Unità"));
                    egui::ComboBox::from_id_salt("posizione_unita")
                        .width(70.0)
                        .selected_text(format!("{}", panel.unit))
                        .show_ui(ui, |ui| {
                            for u in UNITS {
                                ui.selectable_value(&mut panel.unit, u, format!("{u}"));
                            }
                        })
                        .response
                        .on_hover_text(tr("Passo del cursore e delle frecce (mm o gradi)"));
                    ui.checkbox(&mut panel.world, tr("Modifica in coordinate World")).on_hover_text(
                        tr("Acceso: assi della cabina; le rotazioni girano attorno all'origine dell'oggetto, \
                         quindi X Y Z restano. Spento: assi e centro dell'oggetto (le sue coordinate, \
                         uguali per tutti i suoi pezzi)"),
                    );
                });
                if panel.value != before && !nodes.is_empty() {
                    let motion = axis_motion(panel.axis, panel.value - before);
                    actions.push(Action::Move { nodes: nodes.clone(), matrix: slider_motion(motion, frame, panel.world) });
                }
                if ui.button(icon_text(ui, icon::ARROW_COUNTER_CLOCKWISE, tr("Ripristina")))
                    .on_hover_text(tr("Riporta l'oggetto nella posizione in cui è stato aperto"))
                    .clicked()
                    && !nodes.is_empty()
                {
                    actions.push(Action::ResetPosition(nodes.clone()));
                }
                });

                // ── Allinea ────────────────────────────────────────────────────
                section(ui, "posizione_allinea", tr("Allinea"), &mut sections[5], |ui| {
                    ui.label(
                        egui::RichText::new(tr("L'oggetto qui sopra si sposta sul riferimento lungo gli assi scelti della cabina, senza ruotare."))
                            .weak()
                            .small(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("Su"));
                        let (short, full) = slot_text(app, &panel.align_reference);
                        let zero = panel.align_reference.is_empty();
                        ui.label(egui::RichText::new(if zero { tr("Zero della scena").to_string() } else { short }).strong())
                            .on_hover_text(if zero { tr("Centro cabina (lo zero della scena)").to_string() } else { full });
                        if ui
                            .add_enabled(!selection.is_empty(), egui::Button::new(tr("Usa selezione")).small())
                            .on_hover_text(tr("Il riferimento diventa la selezione: un pezzo, un gruppo o più pezzi insieme"))
                            .clicked()
                        {
                            panel.align_reference = selection.clone();
                        }
                        if !zero && ui.small_button(tr("Zero")).on_hover_text(tr("Allinea sullo zero della scena (centro cabina)")).clicked() {
                            panel.align_reference.clear();
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for k in 0..3 {
                            ui.checkbox(&mut panel.align_axes[k], egui::RichText::new(AXES[k].0).color(AXES[k].1).strong());
                        }
                        ui.separator();
                        for (mode, name, hint) in AlignMode::ALL {
                            ui.selectable_value(&mut panel.align_mode, mode, tr(name)).on_hover_text(tr(hint));
                        }
                    });
                    let reference: Vec<u32> =
                        panel.align_reference.iter().copied().filter(|&n| (n as usize) < scene.nodes.len()).collect();
                    let bbox = |list: &[u32]| {
                        let mut b = Aabb::empty();
                        for &n in list {
                            let s = scene.subtree_bounding_box(n);
                            if s.is_valid() {
                                b.extend_aabb(&s);
                            }
                        }
                        b.is_valid().then_some(b)
                    };
                    let axes = panel.align_axes;
                    let delta: Result<Vec3, String> = match bbox(&nodes) {
                        None => Err(tr("Scegli l'oggetto da spostare").to_string()),
                        Some(_) if !axes.contains(&true) => Err(tr("Scegli almeno un asse").to_string()),
                        Some(_) if reference.iter().any(|r| nodes.contains(r)) => Err(tr("Il riferimento è anche l'oggetto da spostare").to_string()),
                        Some(moving) if panel.align_mode == AlignMode::SamePiece => match (nodes.as_slice(), reference.as_slice()) {
                            ([n], [r]) => scene
                                .geometric_offset(*n, *r)
                                .map(|fit| {
                                    let c = moving.center();
                                    only_axes(fit.matrix.transform_point3(c) - c, axes)
                                })
                                .map_err(|why| trf!("Non sono lo stesso pezzo: {why}", why = why)),
                            _ => Err(tr("«Stesso pezzo»: un solo oggetto da spostare e un solo riferimento").to_string()),
                        },
                        Some(moving) => Ok(align_delta(&moving, bbox(&reference).as_ref(), panel.align_mode, axes)),
                    };
                    match &delta {
                        Ok(d) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(tr("Spostamento"));
                                for k in (0..3).filter(|&k| axes[k]) {
                                    ui.label(egui::RichText::new(AXES[k].0).color(AXES[k].1).strong());
                                    ui.monospace(format!("{:+.2}", d[k] + 0.0));
                                }
                                ui.label("mm");
                            });
                        }
                        Err(e) => {
                            ui.label(egui::RichText::new(e).small().weak());
                        }
                    }
                    let ok = delta.as_ref().ok().filter(|d| d.length() > 1e-4).copied();
                    if ui.add_enabled(ok.is_some(), egui::Button::new(icon_text(ui, icon::ALIGN_CENTER_HORIZONTAL, tr("Allinea")))).clicked() {
                        if let Some(d) = ok {
                            actions.push(Action::Move { nodes: nodes.clone(), matrix: Mat4::from_translation(d) });
                            status = Some(trf!(
                                "Allineato: spostato di X {x} Y {y} Z {z} mm",
                                x = format!("{:+.2}", d.x),
                                y = format!("{:+.2}", d.y),
                                z = format!("{:+.2}", d.z)
                            ));
                        }
                    }
                });

                // ── Allinea su punti ───────────────────────────────────────────
                section(ui, "posizione_punti", tr("Allinea su punti"), &mut sections[6], |ui| {
                    let pa = &mut panel.points;
                    ui.label(
                        egui::RichText::new(trf!(
                            "L'oggetto qui sopra si sposta sui riferimenti presi in cabina (punti del programma). \
                             Almeno {n} punti; sul 3D il clic si aggancia al vertice vicino.",
                            n = super::point_align::MIN_POINTS
                        ))
                        .weak()
                        .small(),
                    );
                    if program_points.is_empty() {
                        ui.label(
                            egui::RichText::new(tr("Apri il programma con i riferimenti nella finestra Programma (punti Object)"))
                                .weak()
                                .small(),
                        );
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            egui::ComboBox::from_id_salt("posizione_punti_programma")
                                .width(110.0)
                                .selected_text(pa.choice.clone().unwrap_or_else(|| tr("Punto…").to_string()))
                                .show_ui(ui, |ui| {
                                    for (name, _) in &program_points {
                                        ui.selectable_value(&mut pa.choice, Some(name.clone()), name);
                                    }
                                })
                                .response
                                .on_hover_text(tr("Punti del programma attivo, nell'ordine in cui li usa"));
                            let chosen = pa.choice.as_ref().and_then(|c| program_points.iter().find(|(n, _)| n == c)).cloned();
                            if ui.add_enabled(chosen.is_some(), egui::Button::new(icon_text(ui, icon::PLUS, tr("Aggiungi")))).clicked() {
                                if let Some((name, p)) = chosen {
                                    pa.add(&name, p);
                                }
                            }
                            if ui.button(tr("Tutti")).on_hover_text(tr("Aggiunge tutti i punti del programma attivo")).clicked() {
                                for (name, p) in &program_points {
                                    pa.add(name, *p);
                                }
                            }
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("A mano"));
                        ui.add(egui::TextEdit::singleline(&mut pa.manual_name).desired_width(60.0).hint_text(tr("Nome")));
                        for k in 0..3 {
                            ui.label(egui::RichText::new(AXES[k].0).color(AXES[k].1).strong());
                            ui.add(egui::DragValue::new(&mut pa.manual[k]).speed(0.1).max_decimals(3));
                        }
                        let name = pa.manual_name.trim().to_string();
                        if ui.add_enabled(!name.is_empty(), egui::Button::new(icon::PLUS).small()).on_hover_text(tr("Aggiunge il punto scritto a mano")).clicked() {
                            let p = pa.manual;
                            pa.add(&name, p);
                            pa.manual_name.clear();
                        }
                    });
                    let fit = pa.fit();
                    let residuals = fit.as_ref().ok().map(|f| f.residuals.clone());
                    // Pezzi che si spostano: i punti presi altrove non seguono l'allineamento
                    let moving = scene.meshes_of(&nodes);
                    let mut outside = 0;
                    let mut remove: Option<usize> = None;
                    let mut pick: Option<Option<usize>> = None;
                    if !pa.pairs.is_empty() {
                        egui::Grid::new("posizione_punti_tabella").num_columns(6).spacing([6.0, 3.0]).striped(true).show(ui, |ui| {
                            ui.label("");
                            ui.label(egui::RichText::new(tr("Punto")).small().strong());
                            ui.label(egui::RichText::new(tr("Sul 3D")).small().strong());
                            ui.label(egui::RichText::new(tr("Ora")).small().strong()).on_hover_text(tr("Distanza tra il punto del 3D e il riferimento (mm)"));
                            ui.label(egui::RichText::new(tr("Dopo")).small().strong()).on_hover_text(tr("Scarto che resta dopo «Allinea» (mm)"));
                            ui.label("");
                            ui.end_row();
                            for (i, p) in pa.pairs.iter_mut().enumerate() {
                                ui.checkbox(&mut p.on, "").on_hover_text(tr("Usa questo punto nel calcolo"));
                                let t = p.target;
                                ui.label(egui::RichText::new(&p.name).monospace())
                                    .on_hover_text(format!("X {:.3}  Y {:.3}  Z {:.3}", t.x, t.y, t.z));
                                let picking = pa.picking == Some(i);
                                let (text, hover) = match (&p.cad, picking) {
                                    (_, true) => (format!("{} …", icon::CROSSHAIR), tr("Clicca il punto sul 3D (Esc per smettere)")),
                                    (Some(_), _) => (icon::CHECK.to_string(), tr("Preso: clic per riprenderlo")),
                                    (None, _) => (format!("{} {}", icon::CROSSHAIR, tr("Prendi")), tr("Clicca poi il punto sul 3D")),
                                };
                                if ui.add(egui::Button::new(text).small().selected(picking)).on_hover_text(hover).clicked() {
                                    pick = Some(Some(i));
                                }
                                if p.cad.as_ref().and_then(|c| c.anchor).is_some_and(|a| moving.binary_search(&a.mesh).is_err()) {
                                    outside += 1;
                                }
                                let weak = |s: String| if p.on { egui::RichText::new(s).monospace() } else { egui::RichText::new(s).monospace().weak() };
                                ui.label(weak(p.cad.as_ref().map_or("—".to_string(), |c| format!("{:.2}", c.pos.distance(t)))));
                                ui.label(weak(residuals.as_ref().and_then(|r| r[i]).map_or("—".to_string(), |r| format!("{r:.2}"))));
                                if ui.small_button(icon::X).on_hover_text(tr("Togli il punto")).clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            let missing = pa.pairs.iter().any(|p| p.cad.is_none());
                            if ui.add_enabled(missing, egui::Button::new(icon_text(ui, icon::CROSSHAIR, tr("Prendi i mancanti"))))
                                .on_hover_text(tr("Clicca sul 3D i punti che mancano, uno dopo l'altro"))
                                .clicked()
                            {
                                pick = Some(None);
                            }
                            if ui.button(tr("Svuota")).on_hover_text(tr("Toglie tutti i punti")).clicked() {
                                pa.pairs.clear();
                                pa.picking = None;
                            }
                        });
                    }
                    if let Some(i) = remove {
                        pa.pairs.remove(i);
                        pa.picking = None;
                    }
                    if let Some(i) = pick {
                        pa.start_picking(i);
                        panel.distances.stop_picking();
                        panel.hinge.picking = false;
                        start_picking = true;
                    }
                    match &fit {
                        Ok(f) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(tr("Scarto"));
                                ui.monospace(trf!("medio {rms}  max {max}", rms = format!("{:.2}", f.rms), max = format!("{:.2}", f.max)));
                                ui.label("mm");
                            });
                            let pose = Pose::from_matrix(f.matrix);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(tr("Spostamento (coordinate World)")).strong());
                                if ui.small_button(icon::COPY).on_hover_text(tr("Copia X Y Z A B C")).clicked() {
                                    copy = Some(offset_text("Offset World", &pose));
                                }
                            });
                            pose_grid(ui, "posizione_punti_spostamento", &pose);
                            if outside > 0 {
                                ui.label(
                                    egui::RichText::new(trf!(
                                        "{outside} punti sono su pezzi che non fanno parte dell'oggetto qui sopra: non lo seguiranno",
                                        outside = outside
                                    ))
                                    .small()
                                    .color(egui::Color32::from_rgb(230, 160, 60)),
                                );
                            }
                            let ready = !nodes.is_empty() && !f.matrix.abs_diff_eq(Mat4::IDENTITY, 1e-6);
                            if ui
                                .add_enabled(ready, egui::Button::new(icon_text(ui, icon::ALIGN_CENTER_HORIZONTAL, tr("Allinea"))))
                                .on_hover_text(tr("Sposta l'oggetto qui sopra sui riferimenti"))
                                .clicked()
                            {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: f.matrix });
                                status = Some(trf!(
                                    "Allineato sui punti: scarto medio {rms} mm, massimo {max} mm",
                                    rms = format!("{:.2}", f.rms),
                                    max = format!("{:.2}", f.max)
                                ));
                            }
                        }
                        Err(e) => {
                            if !pa.pairs.is_empty() {
                                ui.label(egui::RichText::new(e).small().weak());
                            }
                        }
                    }
                });

                // ── Allinea su misure (quote prese a mano) ──────────────────────
                section(ui, "posizione_misure", tr("Allinea su misure"), &mut sections[8], |ui| {
                    use super::distance_align::{Mode, RowState, EXACT_FIXED, EXACT_PER_FIXED, MIN_FULL, MIN_MOVE};
                    ui.label(
                        egui::RichText::new(
                            tr("L'oggetto qui sopra si sposta finché le quote sul 3D sono quelle prese a mano in cabina, \
                             dai punti fissi F (riferimento fermo) ai punti M dell'oggetto."),
                        )
                        .weak()
                        .small(),
                    );
                    let da = &mut panel.distances;
                    // Veloce o Precisa: due pulsanti, quello scelto acceso
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut da.mode, Mode::Quick, trf!("Veloce ({n} misure)", n = MIN_FULL)).on_hover_text(trf!(
                            "Posizione veloce, approssimata: bastano {full} misure ({moves} con «Solo spostamento»), \
                             ognuna da un punto fisso a un punto dell'oggetto. L'oggetto si sposta il meno possibile.",
                            full = MIN_FULL,
                            moves = MIN_MOVE
                        ));
                        ui.selectable_value(&mut da.mode, Mode::Precise, trf!("Precisa ({fixed}×{per})", fixed = EXACT_FIXED, per = EXACT_PER_FIXED))
                            .on_hover_text(trf!(
                                "Posizione esatta: per ogni punto fisso un clic e poi {per} clic sull'oggetto; almeno \
                                 {fixed} punti fissi, solo distanze. Meglio usare gli stessi punti M per tutti i fissi.",
                                fixed = EXACT_FIXED,
                                per = EXACT_PER_FIXED
                            ));
                        if ui
                            .add(egui::Button::new(icon_text(ui, icon::INFO, tr("Guida"))).small().selected(panel.measures_guide))
                            .on_hover_text(tr("Quante misure prendere e dove, con i disegni"))
                            .clicked()
                        {
                            panel.measures_guide = !panel.measures_guide;
                        }
                    });
                    let precise = da.mode == Mode::Precise;
                    ui.label(
                        egui::RichText::new(if precise {
                            trf!(
                                "«Nuovo punto fisso»: clicca il punto F sul riferimento, poi {per} punti M sull'oggetto. \
                                 Ripeti per almeno {fixed} punti fissi e scrivi le distanze.",
                                fixed = EXACT_FIXED,
                                per = EXACT_PER_FIXED
                            )
                        } else {
                            tr("«Nuova misura»: clicca il punto F sul riferimento e il punto M sull'oggetto, poi scrivi la quota.")
                                .to_string()
                        })
                        .weak()
                        .small(),
                    );
                    let moving = scene.meshes_of(&nodes);
                    let mut pick: Option<Option<usize>> = None;
                    ui.horizontal_wrapped(|ui| {
                        let (text, hover) = if precise {
                            (
                                tr("Nuovo punto fisso"),
                                trf!("Clicca sul 3D il punto sul riferimento fermo, poi {per} punti sull'oggetto da spostare", per = EXACT_PER_FIXED),
                            )
                        } else {
                            (tr("Nuova misura"), tr("Clicca sul 3D il punto sul riferimento fermo, poi quello sull'oggetto da spostare").to_string())
                        };
                        if ui.add_enabled(!nodes.is_empty(), egui::Button::new(icon_text(ui, icon::PLUS, text))).on_hover_text(hover).clicked() {
                            pick = Some(None);
                        }
                        if ui
                            .add_enabled(tool_points.is_some(), egui::Button::new(icon_text(ui, icon::RULER, tr("Dalla Misura"))))
                            .on_hover_text(tr("Usa i due punti presi con lo strumento Misura (tasto M)"))
                            .clicked()
                        {
                            if let Some((a, b)) = tool_points {
                                // Il punto sull'oggetto da spostare va per secondo (M)
                                let on_moving = |p: &crate::ui::measure::MeasurePoint| p.anchor.is_some_and(|x| moving.binary_search(&x.mesh).is_ok());
                                if on_moving(&a) && !on_moving(&b) { da.add_from(b, a) } else { da.add_from(a, b) }
                            }
                        }
                    });
                    let fit = da.fit(&moving);
                    let states = da.states(&moving);
                    let labels = da.labels();
                    let mut remove: Option<usize> = None;
                    let mut kinds: Option<(usize, super::distance_align::Kind)> = None;
                    if !da.rows.is_empty() {
                        let residuals = fit.as_ref().ok().map(|f| f.residuals.clone());
                        let picking_row = da.picking.map(|(p, _)| p);
                        let columns = if precise { 6 } else { 7 };
                        egui::Grid::new("posizione_misure_tabella").num_columns(columns).spacing([6.0, 3.0]).striped(true).show(ui, |ui| {
                            ui.label("");
                            ui.label(egui::RichText::new(tr("Misura")).small().strong())
                                .on_hover_text(tr("F = punto fisso sul riferimento, M = punto sull'oggetto da spostare"));
                            if !precise {
                                ui.label(egui::RichText::new(tr("Tipo")).small().strong());
                            }
                            ui.label(egui::RichText::new(tr("Sul 3D")).small().strong()).on_hover_text(tr("Valore adesso sul 3D (mm)"));
                            ui.label(egui::RichText::new(tr("Misurato")).small().strong()).on_hover_text(tr("Valore preso in cabina: scrivilo qui (mm)"));
                            ui.label(egui::RichText::new(tr("Dopo")).small().strong())
                                .on_hover_text(tr("Differenza che resta dopo «Allinea» tra il 3D e il misurato (mm)"));
                            ui.label("");
                            ui.end_row();
                            for (i, r) in da.rows.iter_mut().enumerate() {
                                ui.checkbox(&mut r.on, "").on_hover_text(tr("Usa questa misura nel calcolo"));
                                let picking = picking_row == Some(i);
                                let again = if precise { tr("Clic per riprendere il punto M") } else { tr("Clic per riprendere i due punti") };
                                let hover = match states[i].problem() {
                                    Some(p) if !picking => format!("{p}\n{again}"),
                                    _ => again.to_string(),
                                };
                                // In Precisa il fisso si scrive solo sulla prima misura del gruppo
                                let first = i == 0 || labels[i - 1][0] != labels[i][0];
                                let fixed = if precise && !first { "    ".to_string() } else { labels[i][0].clone() };
                                let mut name = egui::RichText::new(format!("{fixed}–{}", labels[i][1])).monospace();
                                if !picking && !matches!(states[i], RowState::Ready(_)) {
                                    name = name.color(egui::Color32::from_rgb(230, 160, 60));
                                }
                                if ui.add(egui::Button::new(name).small().selected(picking)).on_hover_text(hover).clicked() {
                                    pick = Some(Some(i));
                                }
                                if !precise {
                                    let mut kind = r.kind;
                                    egui::ComboBox::from_id_salt(("posizione_misure_tipo", i))
                                        .width(70.0)
                                        .selected_text(kind.name())
                                        .show_ui(ui, |ui| {
                                            for (k, n, h) in super::distance_align::Kind::ALL {
                                                ui.selectable_value(&mut kind, k, tr(n)).on_hover_text(tr(h));
                                            }
                                        });
                                    if kind != r.kind {
                                        kinds = Some((i, kind));
                                    }
                                }
                                let on = r.on;
                                let weak = |s: String| if on { egui::RichText::new(s).monospace() } else { egui::RichText::new(s).monospace().weak() };
                                let current = r.current();
                                let shown = current.map_or("—".to_string(), |v| format!("{v:.2}"));
                                // In Precisa le quote sono distanze: un ΔX/ΔY/ΔZ da Veloce lo dice
                                let shown = if precise && r.kind != super::distance_align::Kind::Distance { format!("{} {shown}", r.kind.name()) } else { shown };
                                ui.label(weak(shown));
                                ui.add_enabled(
                                    current.is_some(),
                                    egui::DragValue::new(&mut r.measured).speed(0.5).max_decimals(2).range(0.0..=f32::MAX),
                                )
                                .on_hover_text(tr("Trascina, oppure fai doppio clic e scrivi il valore"));
                                ui.label(weak(residuals.as_ref().and_then(|v| v[i]).map_or("—".to_string(), |e| format!("{e:+.2}"))));
                                if ui.small_button(icon::X).on_hover_text(tr("Togli la misura")).clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if !precise {
                                ui.checkbox(&mut da.only_move, tr("Solo spostamento"))
                                    .on_hover_text(tr("Il pezzo si sposta senza ruotare: bastano 3 misure (es. una per asse)"));
                            }
                            if ui.button(tr("Svuota")).on_hover_text(tr("Toglie tutte le misure")).clicked() {
                                da.rows.clear();
                                da.stop_picking();
                            }
                        });
                    }
                    if let Some((i, k)) = kinds {
                        da.set_kind(i, k);
                    }
                    if let Some(i) = remove {
                        da.remove(i);
                    }
                    if let Some(i) = pick {
                        match i {
                            Some(i) => da.repick(i),
                            None => da.add_and_pick(),
                        }
                        panel.points.picking = None;
                        panel.hinge.picking = false;
                        start_picking = true;
                    }
                    let da = &panel.distances;
                    if !da.rows.is_empty() {
                        let ready = states.iter().filter(|s| matches!(s, RowState::Ready(_))).count();
                        if precise {
                            // Schema: almeno 3 punti fissi con 3 punti dell'oggetto ciascuno
                            let coverage = da.coverage(&moving);
                            let complete = da.complete_fixed(&moving);
                            let per_fixed = coverage.iter().map(|c| c.1.to_string()).collect::<Vec<_>>().join(" · ");
                            let (text, color) = if complete >= EXACT_FIXED {
                                (
                                    trf!(
                                        "Schema completo: {complete} punti fissi con {per} misure, posizione esatta",
                                        complete = complete,
                                        per = EXACT_PER_FIXED
                                    ),
                                    egui::Color32::from_rgb(60, 190, 90),
                                )
                            } else {
                                (
                                    trf!(
                                        "Punti fissi: {n} (misure per fisso: {list}). Servono almeno {fixed} fissi con {per} misure ciascuno",
                                        n = coverage.len(),
                                        list = if per_fixed.is_empty() { "—".to_string() } else { per_fixed },
                                        fixed = EXACT_FIXED,
                                        per = EXACT_PER_FIXED
                                    ),
                                    egui::Color32::from_rgb(230, 160, 40),
                                )
                            };
                            ui.label(egui::RichText::new(text).small().color(color));
                        } else {
                            ui.label(egui::RichText::new(trf!("Misure pronte: {ready}, ne servono almeno {min}", ready = ready, min = da.minimum())).small());
                        }
                    }
                    match &fit {
                        Ok(f) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(tr("Scarto"));
                                ui.monospace(trf!("medio {rms}  max {max}", rms = format!("{:.2}", f.rms), max = format!("{:.2}", f.max)));
                                ui.label("mm");
                            });
                            let pose = Pose::from_matrix(f.matrix);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(tr("Spostamento (coordinate World)")).strong());
                                if ui.small_button(icon::COPY).on_hover_text(tr("Copia X Y Z A B C")).clicked() {
                                    copy = Some(offset_text("Offset World", &pose));
                                }
                            });
                            pose_grid(ui, "posizione_misure_spostamento", &pose);
                            if f.free {
                                ui.label(
                                    egui::RichText::new(
                                        tr("Posizione approssimata: le misure non fissano tutte le direzioni e l'oggetto si sposta il meno \
                                         possibile. Se l'oggetto è anche ruotato usa «Precisa», con i punti lontani e non in fila."),
                                    )
                                    .small()
                                    .color(egui::Color32::from_rgb(230, 160, 60)),
                                );
                            }
                            let ready = !nodes.is_empty() && !f.matrix.abs_diff_eq(Mat4::IDENTITY, 1e-6);
                            if ui
                                .add_enabled(ready, egui::Button::new(icon_text(ui, icon::ALIGN_CENTER_HORIZONTAL, tr("Allinea"))))
                                .on_hover_text(tr("Sposta l'oggetto qui sopra perché le quote sul 3D siano quelle misurate (Annulla lo rimette)"))
                                .clicked()
                            {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: f.matrix });
                                status = Some(trf!(
                                    "Allineato sulle misure: scarto medio {rms} mm, massimo {max} mm",
                                    rms = format!("{:.2}", f.rms),
                                    max = format!("{:.2}", f.max)
                                ));
                            }
                        }
                        Err(e) => {
                            if !da.rows.is_empty() {
                                ui.label(egui::RichText::new(e).small().weak());
                            }
                        }
                    }
                });

                // ── Misura dell'oggetto (file local e world) ─────────────────────
                section(ui, "posizione_misura_oggetto", tr("Misura dell'oggetto"), &mut sections[7], |ui| {
                    let m = &mut panel.measure;
                    ui.label(
                        egui::RichText::new(
                            tr("Oggetto del robot da due file di punti con gli stessi nomi: local (presi sulla matematica in \
                             posizione 0) e world (presi con il robot in cabina, oggetto a 0). Contano solo X Y Z."),
                        )
                        .weak()
                        .small(),
                    );
                    for (world, title) in [(false, "Local"), (true, "World")] {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(title).strong());
                            let file = if world { &m.world } else { &m.local };
                            let text = file.as_ref().map_or("—".to_string(), |f| trf!("{name}  ({n} punti)", name = f.name(), n = f.points.len()));
                            let label = ui.label(egui::RichText::new(text).monospace());
                            if let Some(f) = file {
                                label.on_hover_text(f.path.display().to_string());
                            }
                            if ui.small_button(icon::FOLDER_OPEN).on_hover_text(trf!("Apri il file {kind} (.tid)", kind = title.to_lowercase())).clicked() {
                                pick_measure = Some(if world { 1 } else { 0 });
                            }
                        });
                    }
                    if let Some(e) = &m.error {
                        ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                    }
                    let (rows, result) = m.compute();
                    let mut toggle: Option<(String, bool)> = None;
                    if !rows.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(tr("Punti sul 3D"));
                            ui.selectable_value(&mut m.show_world, false, tr("Local"))
                                .on_hover_text(tr("Punti presi sulla matematica in posizione 0 (pallini)"));
                            ui.selectable_value(&mut m.show_world, true, tr("World")).on_hover_text(
                                tr("Punti presi con il robot, riportati sulla matematica con l'oggetto calcolato (anelli): \
                                 passando da Local a World si vede di quanto si sposta ogni punto"),
                            );
                        });
                        let worst = result.as_ref().ok().and_then(|r| r.worst.clone());
                        let red = egui::Color32::from_rgb(230, 80, 70);
                        egui::Grid::new("posizione_misura_tabella").num_columns(6).spacing([10.0, 3.0]).striped(true).show(ui, |ui| {
                            ui.label(egui::RichText::new(tr("Punto")).small().strong());
                            for (t, axis) in [("dx", "X"), ("dy", "Y"), ("dz", "Z")] {
                                ui.label(egui::RichText::new(t).small().strong())
                                    .on_hover_text(trf!("Scostamento in {axis}: punto world − oggetto calcolato · punto local (mm)", axis = axis));
                            }
                            ui.label(egui::RichText::new(tr("Dist.")).small().strong()).on_hover_text(tr("Distanza tra i due punti (mm)"));
                            ui.label(egui::RichText::new(tr("Valido")).small().strong()).on_hover_text(tr("Usa questo punto nel calcolo e nella statistica"));
                            ui.end_row();
                            for r in &rows {
                                let is_worst = r.on && worst.as_ref() == Some(&r.name);
                                let style = |t: String| {
                                    let rich = egui::RichText::new(t).monospace();
                                    if !r.on {
                                        rich.weak()
                                    } else if is_worst {
                                        rich.color(red)
                                    } else {
                                        rich
                                    }
                                };
                                ui.label(style(format!("{0}/{0}", r.name))).on_hover_text(format!(
                                    "Local  X {:.3}  Y {:.3}  Z {:.3}
World  X {:.3}  Y {:.3}  Z {:.3}",
                                    r.local.x, r.local.y, r.local.z, r.world.x, r.world.y, r.world.z
                                ));
                                match r.delta {
                                    Some(d) => {
                                        for c in d.to_array() {
                                            ui.label(style(format!("{c:.2}")));
                                        }
                                    }
                                    None => {
                                        for _ in 0..3 {
                                            ui.label(style("—".to_string()));
                                        }
                                    }
                                }
                                ui.label(style(r.deviation.map_or("—".to_string(), |d| format!("{d:.2}"))));
                                let mut on = r.on;
                                egui::ComboBox::from_id_salt(("posizione_misura_valido", &r.name))
                                    .width(48.0)
                                    .selected_text(if on { tr("sì") } else { tr("no") })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut on, true, tr("sì"));
                                        ui.selectable_value(&mut on, false, tr("no"));
                                    });
                                if on != r.on {
                                    toggle = Some((r.name.clone(), on));
                                }
                                ui.end_row();
                            }
                        });
                    }
                    let unmatched = m.unmatched();
                    if !unmatched.is_empty() {
                        ui.label(egui::RichText::new(trf!("In un file solo: {list}", list = unmatched.join(", "))).small().weak());
                    }
                    if let Some((name, on)) = toggle {
                        m.set_on(&name, on);
                    }
                    match &result {
                        Ok(res) => {
                            // Statistica, come nel software di calibrazione, con la spiegazione al passaggio del mouse
                            ui.label(egui::RichText::new(tr("Statistica")).strong());
                            let values = [
                                format!("{:.2} mm", res.expected),
                                format!("{:.2} mm", res.mean),
                                format!("{:.2} mm²", res.variance),
                                format!("{:.2} mm", res.max),
                            ];
                            egui::Grid::new("posizione_misura_statistica").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
                                for ((name, help), v) in super::object_measure::STATS_HELP.iter().zip(values) {
                                    ui.label(tr(name)).on_hover_text(tr(help));
                                    ui.label(egui::RichText::new(v).monospace()).on_hover_text(tr(help));
                                    ui.end_row();
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                let (r, g, b) = res.verdict.rgb();
                                let text = egui::RichText::new(format!(" {} ", res.verdict.text())).strong().color(egui::Color32::WHITE)
                                    .background_color(egui::Color32::from_rgb(r, g, b));
                                ui.label(text).on_hover_text(trf!(
                                    "OK fino a {ok} mm di errore massimo, accettabile fino a {acceptable} mm",
                                    ok = super::object_measure::TOL_OK,
                                    acceptable = super::object_measure::TOL_ACCEPTABLE
                                ));
                                let mut hint = res.verdict.hint();
                                if let (Some(w), super::object_measure::Verdict::Acceptable | super::object_measure::Verdict::NotOk) = (&res.worst, res.verdict) {
                                    hint = format!("{hint} ({w})");
                                }
                                ui.label(egui::RichText::new(hint).small());
                            });
                            if ui.button(icon_text(ui, icon::FLOPPY_DISK, tr("Salva il report HTML"))).on_hover_text(tr("Report con file, oggetto, statistica e scostamenti dei punti")).clicked() {
                                save_report = true;
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(tr("Oggetto calcolato")).strong());
                                if ui.small_button(icon::COPY).on_hover_text(tr("Copia X Y Z A B C")).clicked() {
                                    copy = Some(offset_text(tr("Oggetto calcolato"), &res.pose));
                                }
                            });
                            pose_grid(ui, "posizione_misura_oggetto_calcolato", &res.pose);
                            // Confronto con l'oggetto attuale del robot
                            ui.horizontal_wrapped(|ui| {
                                ui.label(tr("Oggetto attuale"));
                                egui::ComboBox::from_id_salt("posizione_misura_base")
                                    .width(120.0)
                                    .selected_text(m.base.clone().unwrap_or_else(|| "—".to_string()))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut m.base, None, "—");
                                        for b in &m.bases {
                                            ui.selectable_value(&mut m.base, Some(b.name.clone()), &b.name);
                                        }
                                    })
                                    .response
                                    .on_hover_text(m.base_file.as_ref().map_or(tr("Nessun file degli oggetti").to_string(), |f| f.display().to_string()));
                                if ui.small_button(icon::FOLDER_OPEN).on_hover_text(tr("Apri il file degli oggetti del robot (es. _GLOBAL.TID)")).clicked() {
                                    pick_measure = Some(2);
                                }
                            });
                            if let Some(b) = m.current_base() {
                                let d = super::object_measure::difference(&res.pose, &b.pose);
                                ui.label(egui::RichText::new(tr("Differenza (calcolato − attuale)")).small())
                                    .on_hover_text(tr("Nelle stazioni in tracking la X dipende dalla posizione sul convogliatore"));
                                pose_grid(ui, "posizione_misura_differenza", &d);
                            }
                            let line = m.robot_line(res.pose);
                            ui.add(egui::Label::new(egui::RichText::new(&line).monospace()).wrap());
                            if ui.button(icon_text(ui, icon::COPY, tr("Copia la riga"))).on_hover_text(tr("Riga dell'oggetto da incollare nel file del robot")).clicked() {
                                copy = Some(line.clone());
                            }
                        }
                        Err(e) => {
                            ui.label(egui::RichText::new(e).small().weak());
                        }
                    }
                });

                // ── Rotazione su un asse (cerniere) ─────────────────────────────
                section(ui, "posizione_cerniera", tr("Rotazione su cerniera"), &mut sections[2], |ui| {
                        let h = &mut panel.hinge;
                        let before_mode = h.mode;
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(&mut h.mode, HingeMode::TwoPoints, tr("Due punti"))
                                .on_hover_text(tr("Due clic sull'asse, es. i centri delle due cerniere"));
                            ui.selectable_value(&mut h.mode, HingeMode::TwoHoles, tr("Due fori"))
                                .on_hover_text(tr("Tre clic sul bordo di ogni foro: l'asse unisce i due centri"));
                            ui.selectable_value(&mut h.mode, HingeMode::OneHole, tr("Un foro"))
                                .on_hover_text(tr("Tre clic sul bordo: l'asse passa per il centro, perpendicolare al foro (come un perno)"));
                            let axis_k = match h.mode { HingeMode::PointAxis(k) => Some(k), _ => None };
                            if ui.selectable_label(axis_k.is_some(), tr("Punto + asse")).on_hover_text(tr("Un clic e la direzione X, Y o Z della cabina")).clicked() {
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
                            ui.selectable_value(&mut h.mode, HingeMode::TwoPositions, tr("Da due posizioni")).on_hover_text(
                                tr("Lo stesso pezzo in due posizioni (es. chiuso e aperto, due matematiche già pronte): \
                                 l'asse si calcola dal movimento, anche senza cerniere nel 3D"),
                            );
                            ui.selectable_value(&mut h.mode, HingeMode::FourBar, tr("4 perni (cofano)")).on_hover_text(
                                tr("Cerniera con due bielle: due perni sulla cerniera fissa e due sulla mobile. \
                                 Il cofano non gira su un asse fisso: le bielle si calcolano in automatico"),
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
                                for (label, slot) in [(tr("Prima posizione"), &mut h.first), (tr("Seconda posizione"), &mut h.second)] {
                                    ui.label(label);
                                    let text = match slot.as_slice() {
                                        [] => "—".to_string(),
                                        [n] => node_label(app, Some(*n)),
                                        many => trf!("{n} elementi", n = many.len()),
                                    };
                                    // Nomi lunghi (percorsi dei file) accorciati: il pulsante resta nella finestra
                                    let short = if text.chars().count() > 24 {
                                        format!("…{}", text.chars().rev().take(23).collect::<Vec<_>>().into_iter().rev().collect::<String>())
                                    } else {
                                        text.clone()
                                    };
                                    ui.label(egui::RichText::new(short).strong()).on_hover_text(text);
                                    if ui
                                        .add_enabled(!selection.is_empty(), egui::Button::new(tr("Usa selezione")).small())
                                        .on_hover_text(tr("Il file o i pezzi selezionati nell'albero"))
                                        .clicked()
                                    {
                                        *slot = selection.clone();
                                    }
                                    ui.end_row();
                                }
                            });
                            if ui
                                .add_enabled(!h.first.is_empty() && !h.second.is_empty(), egui::Button::new(icon_text(ui, icon::CROSSHAIR, tr("Calcola l'asse"))))
                                .on_hover_text(tr("Confronta i pezzi delle due posizioni; la prima è quella che poi si ruota"))
                                .clicked()
                            {
                                match scene.motion_between(&h.first, &h.second) {
                                    Ok(m) => {
                                        h.axis = Some((m.point, m.dir));
                                        h.angle = 0.0;
                                        h.points.clear();
                                        h.error = None;
                                        status = Some(trf!("Asse trovato: {angle}° tra le due posizioni", angle = format!("{:.2}", m.angle)));
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
                                    trf!("Angolo tra le posizioni {angle}°", angle = format!("{:.2}", m.angle)),
                                    trf!("Pezzi che ruotano {moving}, fermi {fixed}", moving = m.moving, fixed = m.fixed),
                                    trf!(
                                        "Scarto {rms} mm, scorrimento lungo l'asse {slide} mm",
                                        rms = format!("{:.3}", m.rms),
                                        slide = format!("{:.3}", m.slide.abs())
                                    ),
                                ];
                                if m.other > 0 {
                                    lines.push(trf!("{n} pezzi si muovono in un altro modo (es. pistoni): esclusi", n = m.other));
                                }
                                if m.unmatched > 0 {
                                    lines.push(trf!("{n} pezzi senza corrispondente: esclusi", n = m.unmatched));
                                }
                                ui.label(egui::RichText::new(lines.join("\n")).small());
                                if m.slide.abs() > 1.0 {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(230, 160, 40),
                                        tr("Non è una rotazione pura: il pezzo scorre anche lungo l'asse"),
                                    );
                                }
                            }
                        }
                        if h.mode == HingeMode::FourBar {
                            ui.label(egui::RichText::new(tr(FOUR_BAR_HINT)).small());
                            if ui
                                .add(egui::Button::new(icon_text(ui, icon::INFO, tr("Guida illustrata"))).small().selected(panel.guide))
                                .on_hover_text(tr("Disegni e passi: quali perni cliccare e in che ordine"))
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
                                    (tr("Parte che si apre"), tr("Cofano e braccio della cerniera mobile (con i suoi perni)"), &mut h.arm),
                                    (tr("Biella 1"), tr("Biella tra perno fisso 1 e perno mobile 1 (facoltativa)"), link1),
                                    (tr("Biella 2"), tr("Biella tra perno fisso 2 e perno mobile 2 (facoltativa)"), link2),
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
                                            .add_enabled(!selection.is_empty(), egui::Button::new(tr("Usa selezione")).small())
                                            .on_hover_text(tr("Il file o i pezzi selezionati nell'albero (al posto di quelli indicati prima)"))
                                            .on_disabled_hover_text(tr("Seleziona prima i pezzi nell'albero o nella vista"))
                                            .clicked()
                                        {
                                            replace = Some(selection.clone());
                                        }
                                        if ui.add_enabled(!slot.is_empty(), egui::Button::new("×").small()).on_hover_text(tr("Togli (i pezzi tornano chiusi)")).clicked() {
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
                                if ui.button(tr("Annulla scelta")).clicked() {
                                    h.picking = false;
                                    h.points.clear();
                                }
                                ui.label(egui::RichText::new(h.prompt().unwrap_or_default()).italics());
                            } else if h.mode == HingeMode::FourBar {
                                let text = if h.four.is_some() { tr("Scegli di nuovo i perni") } else { tr("Scegli i 4 perni nella vista") };
                                if ui
                                    .add_enabled(h.four.as_ref().is_none_or(FourBar::at_start), egui::Button::new(icon_text(ui, icon::CROSSHAIR, text)))
                                    .on_hover_text(tr("3 clic sul bordo di ogni perno: prima i 2 della cerniera fissa, poi i 2 della mobile"))
                                    .on_disabled_hover_text(tr("Riporta prima il cofano a 0°"))
                                    .clicked()
                                {
                                    h.start();
                                    start_picking = true;
                                }
                            } else if let Some((o, d)) = pins {
                                // Perni selezionati: l'asse passa per i loro centri
                                if ui
                                    .button(icon_text(ui, icon::CROSSHAIR, &trf!("Asse dai {n} perni selezionati", n = selection.len())))
                                    .on_hover_text(
                                        tr("L'asse passa per i centri dei pezzi selezionati (con più di due, per i due più lontani). \
                                         Per sceglierlo con i clic nella vista, togli prima la selezione multipla"),
                                    )
                                    .clicked()
                                {
                                    h.start();
                                    h.picking = false;
                                    h.axis = Some((o, d));
                                    status = Some(tr("Asse dai perni: ora seleziona la parte che si apre, poi «Salva cerniera»").to_string());
                                }
                            } else if ui
                                .button(icon_text(ui, icon::CROSSHAIR, if h.axis.is_some() { tr("Scegli un altro asse") } else { tr("Scegli l'asse nella vista") }))
                                .on_hover_text(
                                    tr("Clic sui pezzi nella vista (si aggancia ai vertici vicini, come le Misure). \
                                     Oppure seleziona i due perni (Shift + clic nella vista) e il pulsante crea l'asse da loro"),
                                )
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
                                egui::RichText::new(trf!(
                                    "Biella 1 {l1} mm  ·  biella 2 {l2} mm  ·  tra i perni mobili {l3} mm",
                                    l1 = format!("{l1:.1}"),
                                    l2 = format!("{l2:.1}"),
                                    l3 = format!("{l3:.1}")
                                ))
                                    .small()
                                    .weak(),
                            );
                            if four.skew_deg > 1.0 {
                                ui.colored_label(
                                    warn,
                                    trf!("I perni non sono del tutto paralleli (scarto {skew}°): si usa la direzione media", skew = format!("{:.1}", four.skew_deg)),
                                );
                            }
                            let at_zero = four.at_start();
                            if four.crossed() {
                                ui.colored_label(
                                    warn,
                                    tr("Le bielle risultano incrociate: di solito vuol dire che i perni mobili sono stati cliccati \
                                     nell'ordine sbagliato. Se nel pezzo non si incrociano, premi «Scambia i perni mobili»."),
                                );
                            }
                            if ui
                                .add_enabled(at_zero, egui::Button::new(tr("Scambia i perni mobili")).small())
                                .on_hover_text(tr("Il mobile 1 diventa il mobile 2: per correggere l'abbinamento perno fisso ↔ perno mobile"))
                                .on_disabled_hover_text(tr("Riporta prima il cofano a 0°"))
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
                                ui.colored_label(warn, tr("Indica la parte che si apre (cofano + braccio) per poterla ruotare"));
                            }
                            let before = h.angle;
                            angle_row(ui, &mut h.angle, h.step, tr("Apertura del cofano dalla posizione in cui sono stati scelti i perni"));
                            step_row(ui, &mut h.step);
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button(tr("Inverti verso")).on_hover_text(tr("L'angolo positivo apre dall'altra parte")).clicked() {
                                    four.flip();
                                    h.angle = -h.angle;
                                }
                                if ui.add_enabled(!at_zero, egui::Button::new(tr("Torna a 0°")).small()).on_hover_text(tr("Riporta il cofano dov'era quando hai scelto i perni")).clicked() {
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
                                        h.error = Some(trf!(
                                            "Fine corsa: il meccanismo arriva a {angle}°",
                                            angle = format!("{:.2}", s.phi.to_degrees())
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
                            // Cerniera salvata: si apre poi dalla vista, cliccando il cofano
                            ui.label(
                                egui::RichText::new(
                                    tr("Per salvare: indica sopra la parte che si apre (e le bielle), dai un nome e premi \
                                     «Salva cerniera». Poi basta cliccare il cofano nella vista per aprirlo con i gradi."),
                                )
                                .small()
                                .weak(),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut h.save_name).hint_text(tr("Nome, es. Cofano")).desired_width(130.0));
                                if ui
                                    .add_enabled(!h.arm.is_empty(), egui::Button::new(icon_text(ui, icon::DOOR_OPEN, tr("Salva cerniera"))))
                                    .on_hover_text(
                                        tr("Ricorda perni, parte che si apre e bielle nel progetto. \
                                         La posizione in cui hai scelto i perni è 0°; il limite di apertura è il fine corsa"),
                                    )
                                    .on_disabled_hover_text(tr("Indica prima la parte che si apre (cofano + braccio)"))
                                    .clicked()
                                {
                                    let pins = four.pins_at_start();
                                    // Cofano ancora chiuso: il verso si sceglie perché si apra
                                    let normal = if four.at_start() { scene.outward_four_bar(&h.arm, pins, four.normal) } else { four.normal };
                                    save_four = Some((h.save_name.clone(), h.arm.clone(), h.links.clone(), pins, normal, h.angle));
                                    h.save_name.clear();
                                }
                            });
                        }
                        if save_four.is_some() {
                            // Salvata: il meccanismo ora è della cerniera (etichetta nella vista)
                            h.four = None;
                            h.arm_ref = None;
                            h.points.clear();
                            h.arm.clear();
                            h.links = Default::default();
                            h.angle = 0.0;
                        }
                        if let Some((o, d)) = h.axis {
                            ui.label(
                                egui::RichText::new(trf!(
                                    "Asse per {point}, direzione {dir}",
                                    point = format!("{:.2} {:.2} {:.2}", o.x, o.y, o.z),
                                    dir = format!("{:.3} {:.3} {:.3}", d.x, d.y, d.z)
                                ))
                                .small()
                                .weak(),
                            );
                            let before = h.angle;
                            angle_row(ui, &mut h.angle, h.step, tr("Angolo dalla posizione in cui è stato scelto l'asse"));
                            // Quota presa con il metro: l'angolo che porta la Misura a quella distanza
                            if let Some((moving, fixed, magnet)) = measured {
                                if h.wanted <= 0.0 {
                                    h.wanted = (moving.distance(fixed) * 10.0).round() / 10.0;
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(tr("Distanza"));
                                    ui.add(egui::DragValue::new(&mut h.wanted).speed(0.1).range(0.0..=f32::MAX).max_decimals(2).suffix(" mm"))
                                        .on_hover_text(tr("Distanza voluta tra i due punti della Misura (es. presa con il metro in cabina)"));
                                    if ui
                                        .add_enabled(magnet, egui::Button::new(tr("Porta a questa distanza")))
                                        .on_hover_text(tr("Ruota il pezzo dell'angolo più piccolo che dà questa distanza"))
                                        .on_disabled_hover_text(tr("Accendi la calamita della Misura: il punto deve seguire il pezzo"))
                                        .clicked()
                                        && !nodes.is_empty()
                                    {
                                        match angle_for_distance(o, d, moving, fixed, h.wanted) {
                                            Ok(delta) => {
                                                // Lo spostamento lo fa il controllo dell'angolo qui sotto
                                                h.angle += delta;
                                                h.error = None;
                                                status = Some(trf!(
                                                    "Ruotato di {delta}°: distanza {dist} mm",
                                                    delta = format!("{delta:.2}"),
                                                    dist = format!("{:.2}", h.wanted)
                                                ));
                                            }
                                            Err((min, max)) => {
                                                h.error = Some(trf!(
                                                    "Distanza non raggiungibile ruotando su questo asse: da {min} a {max} mm",
                                                    min = format!("{min:.1}"),
                                                    max = format!("{max:.1}")
                                                ));
                                            }
                                        }
                                    }
                                });
                                if !magnet {
                                    ui.label(egui::RichText::new(tr("Accendi la calamita della Misura per portare la distanza")).small().weak());
                                }
                            } else if app.ui_state.measure.is_some() {
                                ui.label(
                                    egui::RichText::new(tr("Misura con un punto fermo e uno sul pezzo che ruota: qui si porta la distanza a una quota"))
                                        .small()
                                        .weak(),
                                );
                            }
                            step_row(ui, &mut h.step);
                            if h.angle != before && !nodes.is_empty() {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: about_axis(o, d, h.angle - before) });
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button(tr("Inverti verso")).on_hover_text(tr("L'angolo positivo gira dall'altra parte")).clicked() {
                                    h.axis = Some((o, -d));
                                    h.angle = -h.angle;
                                }
                                if let Some(m) = h.motion.as_ref().filter(|_| h.mode == HingeMode::TwoPositions) {
                                    let target = if d.dot(m.dir) >= 0.0 { m.angle } else { -m.angle };
                                    if ui
                                        .small_button(trf!("Vai alla seconda posizione ({angle}°)", angle = format!("{target:.2}")))
                                        .on_hover_text(tr("Ruota la prima posizione fino a sovrapporla alla seconda"))
                                        .clicked()
                                        && !nodes.is_empty()
                                    {
                                        actions.push(Action::Move { nodes: nodes.clone(), matrix: about_axis(o, d, target - h.angle) });
                                        h.angle = target;
                                    }
                                }
                            });
                            // Cerniera salvata: si apre poi dalla vista, cliccando la parte
                            ui.label(
                                egui::RichText::new(
                                    tr("Per salvare: seleziona la parte che si apre (nella vista o nell'albero), \
                                     dai un nome e premi «Salva cerniera». Senza la parte giusta la cerniera non gira."),
                                )
                                .small()
                                .weak(),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.label(tr("Parte"));
                                ui.label(egui::RichText::new(part_name(&nodes)).italics());
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut h.save_name).hint_text(tr("Nome, es. Porta ant. sx")).desired_width(130.0));
                                if ui
                                    .add_enabled(!nodes.is_empty(), egui::Button::new(icon_text(ui, icon::DOOR_OPEN, tr("Salva cerniera"))))
                                    .on_hover_text(
                                        tr("Ricorda asse e parte (l'oggetto scelto sopra) nel progetto: poi basta cliccare la parte \
                                         nella vista per aprirla con i gradi. La posizione in cui hai scelto l'asse è 0°"),
                                    )
                                    .on_disabled_hover_text(tr("Scegli prima l'oggetto (la parte che si apre)"))
                                    .clicked()
                                {
                                    // Parte ancora chiusa: il verso si sceglie perché si apra verso l'esterno
                                    let d = if h.angle.abs() < 1e-4 { scene.outward_dir(&nodes, o, d) } else { d };
                                    save_hinge = Some((h.save_name.clone(), nodes.clone(), o, d, h.angle));
                                    h.save_name.clear();
                                    // Salvata: l'asse ora è della cerniera (si vede con la parte selezionata)
                                    h.points.clear();
                                    h.axis = None;
                                    h.angle = 0.0;
                                    h.motion = None;
                                }
                            });
                        }
                        ui.separator();
                        ui.label(egui::RichText::new(tr("Cerniere salvate")).strong());
                        hinge_cmds = super::hinge_label::draw_list(ui, &saved_hinges, (&selection, &selection_name));
                    });

                // ── Applica offset ─────────────────────────────────────────────
                section(ui, "posizione_offset", tr("Applica offset"), &mut sections[3], |ui| {
                        // Righe che vanno a capo: la finestra si può stringere. La casella accanto a
                        // ogni coordinata dice se usarla: spenta vale 0 quando si applica
                        egui::Grid::new("posizione_offset_valori").num_columns(4).spacing([6.0, 3.0]).show(ui, |ui| {
                            // Righe X|A, Y|B, Z|C: due colonne stanno nella finestra stretta
                            for row in 0..3 {
                                for k in [row, row + 3] {
                                    ui.spacing_mut().icon_spacing = 2.0;
                                    ui.checkbox(&mut panel.offset_use[k], egui::RichText::new(AXES[k].0).color(AXES[k].1).strong())
                                        .on_hover_text(trf!("Usa {axis} quando si applica l'offset (spenta vale 0)", axis = AXES[k].0));
                                    let (value, suffix) =
                                        if k < 3 { (&mut panel.offset.xyz[k], " mm") } else { (&mut panel.offset.abc[k - 3], " °") };
                                    ui.add_enabled(panel.offset_use[k], egui::DragValue::new(value).speed(0.1).max_decimals(3).suffix(suffix));
                                }
                                ui.end_row();
                            }
                        });
                        // Offset con le sole coordinate accese
                        let offset = used_offset(&panel.offset, &panel.offset_use);
                        ui.label(
                            egui::RichText::new(if panel.world {
                                tr("In coordinate della cabina (World acceso)")
                            } else {
                                tr("In coordinate dell'oggetto, come gli offset degli script")
                            })
                            .small()
                            .weak(),
                        );
                        let paste = ui
                            .add(
                                egui::TextEdit::multiline(&mut panel.pasted)
                                    .desired_rows(1)
                                    .desired_width(f32::INFINITY)
                                    .hint_text(format!("{}  {}", icon::CLIPBOARD_TEXT, tr("Incolla qui (Ctrl+V) il risultato dello script"))),
                            )
                            .on_hover_text(tr("Il testo copiato dagli script, oppure sei numeri X Y Z A B C"));
                        if paste.changed() && !panel.pasted.trim().is_empty() {
                            match parse_offset(&panel.pasted) {
                                Some(p) => {
                                    panel.offset = p;
                                    panel.error = None;
                                    panel.pasted.clear();
                                    status = Some(tr("Offset incollato: controlla i valori e premi Applica").to_string());
                                }
                                None => panel.error = Some(tr("Nel testo incollato non c'è un offset X Y Z A B C").to_string()),
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(tr("Applica")).on_hover_text(tr("Sposta l'oggetto di questo offset")).clicked() && !nodes.is_empty() {
                                actions.push(Action::Move { nodes: nodes.clone(), matrix: in_frame(offset.matrix(), frame, panel.world) });
                            }
                            if ui.small_button(tr("Azzera")).clicked() {
                                panel.offset = Pose::default();
                            }
                        });

                        // Lo stesso offset su un oggetto del programma del robot, senza conti a mano
                        ui.separator();
                        ui.label(egui::RichText::new(tr("Nuovo oggetto del robot")).strong()).on_hover_text(
                            tr("Incolla la riga dell'oggetto: viene riscritta, nello stesso formato, spostata dell'offset qui sopra"),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut panel.robot_line)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace)
                                .hint_text(tr("CONST BASE NOME = X Y Z A B C")),
                        )
                        .on_hover_text(tr("CONST BASE NOME = X Y Z A B C\noppure\nNOME : OBJECT := (X, Y, Z, A, B, C)"));
                        if !panel.robot_line.trim().is_empty() {
                            match parse_robot_base(&panel.robot_line) {
                                Some(base) => {
                                    // Con World acceso l'offset è quello visto dalla scena per l'oggetto qui
                                    // sopra: si riporta nelle sue coordinate, che sono quelle del BASE
                                    let offset = if panel.world {
                                        let f = frame_pose.matrix();
                                        Pose::from_matrix(f.inverse() * offset.matrix() * f)
                                    } else {
                                        offset
                                    };
                                    let line = base.moved(&offset, false).to_line();
                                    ui.add(egui::Label::new(egui::RichText::new(&line).monospace().strong()).wrap());
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button(icon_text(ui, icon::COPY, tr("Copia"))).clicked() {
                                            copy = Some(line.clone());
                                        }
                                        ui.label(
                                            egui::RichText::new(if panel.world {
                                                tr("Offset in coordinate della cabina (World acceso)")
                                            } else {
                                                tr("Offset in coordinate dell'oggetto")
                                            })
                                            .small()
                                            .weak(),
                                        );
                                    });
                                }
                                None => {
                                    ui.label(egui::RichText::new(tr("Riga non riconosciuta")).small().color(egui::Color32::from_rgb(230, 90, 80)));
                                }
                            }
                        }
                    });

                // ── Calcola offset ─────────────────────────────────────────────
                section(ui, "posizione_calcolo", tr("Calcola offset"), &mut sections[4], |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(&mut panel.calc, CalcMode::Objects, tr("Tra due oggetti"))
                                .on_hover_text(tr("Come WRLOffsetCalculator: due oggetti della scena, dalle loro coordinate"));
                            ui.selectable_value(&mut panel.calc, CalcMode::Bases, tr("Tra due CONST BASE"))
                                .on_hover_text(tr("Come BaseOffsetCalculator: CONST BASE NOME = X Y Z A B C"));
                        });
                        // (titolo, offset nel riferimento)
                        let mut calc_result: Option<Result<(String, Pose), String>> = None;
                        match panel.calc {
                            CalcMode::Objects => {
                                egui::Grid::new("posizione_oggetti").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                                    for (label, slot) in [(tr("Riferimento"), &mut panel.reference), (tr("Destinazione"), &mut panel.destination)] {
                                        ui.label(label);
                                        // Nomi lunghi accorciati al centro (il pulsante resta nella finestra);
                                        // sotto, il file: due pezzi con lo stesso nome si distinguono
                                        let full = node_label(app, *slot);
                                        let file = slot.and_then(|n| file_label(app, n));
                                        let hover = match &file {
                                            Some(f) => trf!("{full}\nFile: {f}", full = full, f = f),
                                            None => full.clone(),
                                        };
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(middle_ellipsis(&full, 26)).strong()).on_hover_text(&hover);
                                            if let Some(f) = &file {
                                                ui.label(egui::RichText::new(middle_ellipsis(f, 30)).small().weak()).on_hover_text(&hover);
                                            }
                                        });
                                        if ui.add_enabled(app.ui_state.selected_node.is_some(), egui::Button::new(tr("Usa selezione")).small())
                                            .on_hover_text(tr("L'oggetto selezionato nell'albero"))
                                            .clicked()
                                        {
                                            *slot = app.ui_state.selected_node;
                                        }
                                        ui.end_row();
                                    }
                                });
                                if ui.button(tr("Calcola")).clicked() {
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
                                                    panel.result_note = Some(tr("Calcolato dalla geometria: i due oggetti sono lo stesso pezzo").to_string());
                                                    Ok((title, Pose::from_matrix(m.inverse() * fit.matrix * m)))
                                                }
                                                Err(why) => {
                                                    let b = Pose::from_matrix(scene.object_frame(d));
                                                    panel.result_note = Some(trf!(
                                                        "Calcolato dai Transform del file (non dalla geometria: {why})",
                                                        why = why
                                                    ));
                                                    Ok((title, a.offset_to(&b)))
                                                }
                                            }
                                        }
                                        (Some(_), Some(_)) => Err(tr("Riferimento e destinazione devono essere diversi").to_string()),
                                        _ => Err(tr("Scegli riferimento e destinazione").to_string()),
                                    });
                                }
                            }
                            CalcMode::Bases => {
                                for (label, text) in [(tr("Origine"), &mut panel.base_a), (tr("Destinazione"), &mut panel.base_b)] {
                                    ui.label(label);
                                    ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text(tr("CONST BASE NOME = X Y Z A B C")))
                                        .on_hover_text(tr("CONST BASE NOME = X Y Z A B C\noppure\nNOME : OBJECT := (X, Y, Z, A, B, C)"));
                                }
                                let (mut calc, mut swap) = (false, false);
                                ui.horizontal(|ui| {
                                    calc = ui.button(tr("Calcola")).clicked();
                                    swap = ui
                                        .button(icon_text(ui, icon::ARROWS_DOWN_UP, tr("Inverti")))
                                        .on_hover_text(tr("Scambia origine e destinazione (con un risultato già calcolato, lo ricalcola)"))
                                        .clicked();
                                });
                                if swap {
                                    std::mem::swap(&mut panel.base_a, &mut panel.base_b);
                                }
                                if calc || (swap && panel.result.is_some()) {
                                    panel.result_note = None;
                                    calc_result = Some(match (parse_const_base(&panel.base_a), parse_const_base(&panel.base_b)) {
                                        (Some((na, a)), Some((nb, b))) => Ok((format!("{na}  ->  {nb}"), a.offset_to(&b))),
                                        (None, _) => Err(tr("BASE di origine non riconosciuto").to_string()),
                                        (_, None) => Err(tr("BASE di destinazione non riconosciuto").to_string()),
                                    });
                                }
                            }
                        }
                        match calc_result {
                            Some(Ok((title, p))) => {
                                panel.result = Some(p);
                                panel.error = None;
                                status = Some(trf!("Offset calcolato: {title}", title = title));
                            }
                            Some(Err(e)) => {
                                panel.result = None;
                                panel.error = Some(e);
                            }
                            None => {}
                        }
                        if let Some(p) = panel.result {
                            let in_world = offset_in_world(&p, &frame_pose);
                            // (titolo, spiegazione, valori, World acceso con «Usa come offset»: None = come è)
                            let rows: Vec<(&str, &str, Pose, Option<bool>)> = if same_offset(&p, &in_world) {
                                // Oggetto senza Transform né rotazioni: nell'oggetto e in World è lo stesso
                                vec![("Offset", tr("Vale sia con World spento sia acceso"), p, None)]
                            } else {
                                vec![
                                    (
                                        tr("Offset in coordinate dell'oggetto"),
                                        tr("Negli assi e nell'origine del riferimento: come gli script e i BASE del robot"),
                                        p,
                                        Some(false),
                                    ),
                                    (
                                        tr("Offset in coordinate World"),
                                        tr("Lo stesso spostamento visto dalla scena, per l'oggetto scelto in alto: \
                                         assi e origine della scena"),
                                        in_world,
                                        Some(true),
                                    ),
                                ]
                            };
                            for (k, (title, hover, pose, world)) in rows.into_iter().enumerate() {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(title).strong()).on_hover_text(hover);
                                pose_grid(ui, &format!("posizione_risultato_{k}"), &pose);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(icon_text(ui, icon::COPY, tr("Copia"))).clicked() {
                                        copy = Some(offset_text(if world == Some(true) { "Offset World" } else { "Offset" }, &pose));
                                    }
                                    let hover = match world {
                                        Some(true) => tr("Copia i valori in «Applica offset», con World acceso"),
                                        Some(false) => tr("Copia i valori in «Applica offset», con World spento"),
                                        None => tr("Copia i valori in «Applica offset»"),
                                    };
                                    if ui.button(tr("Usa come offset")).on_hover_text(hover).clicked() {
                                        panel.offset = pose;
                                        if let Some(w) = world {
                                            panel.world = w;
                                        }
                                    }
                                });
                            }
                            if let Some(note) = &panel.result_note {
                                ui.label(egui::RichText::new(note).weak().small());
                            }
                        }
                    });
                if let Some(e) = &panel.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
            });
            });
        });
    if panel.measures_guide {
        draw_measures_guide(ctx, &mut panel.measures_guide);
    }
    if panel.guide {
        draw_four_bar_guide(ctx, &mut panel.guide);
    }
    if sections != app.settings.position_sections || dock != app.settings.dock_position {
        let mut s = app.settings.clone();
        s.position_sections = sections;
        s.dock_position = dock;
        app.apply_settings(s);
    }

    if let Some(text) = copy {
        ctx.copy_text(text);
        status = Some(tr("Copiato negli appunti").to_string());
    }
    panel.open = open;
    if panel.hinge.picking {
        // La scelta dell'asse della cerniera ha la precedenza
        panel.points.picking = None;
        panel.distances.stop_picking();
    }
    if !open {
        panel.hinge.picking = false;
        panel.points.picking = None;
        panel.distances.stop_picking();
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
    if let Some(kind) = pick_measure {
        pick_measure_file(&mut panel.measure, kind, app.settings.last_program.as_deref());
    }
    if save_report {
        if let Some(s) = save_measure_report(&panel.measure) {
            status = Some(s);
        }
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
    // Dopo le azioni: il messaggio della finestra è più preciso di quello dello spostamento
    if let Some(s) = status {
        app.ui_state.status_message = s;
    }
    if let Some((name, nodes, o, d, angle)) = save_hinge {
        app.save_hinge(&name, nodes, o, d, angle);
    }
    if let Some((name, nodes, links, pins, normal, angle)) = save_four {
        app.save_four_bar_hinge(&name, nodes, links, pins, normal, angle);
    }
    super::hinge_label::run(app, hinge_cmds);
    // Il valore del cursore resta valido dopo i propri spostamenti
    if let Some(s) = &app.scene_graph {
        app.ui_state.position.revision = s.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asse dai perni: per i centri dei due più lontani, verso il componente maggiore positivo.
    #[test]
    fn pins_axis_goes_through_the_farthest_centers() {
        let (o, d) = pins_axis(&[Vec3::new(10.0, 0.0, 900.0), Vec3::new(10.0, 0.0, 100.0), Vec3::new(10.0, 0.0, 500.0)]).unwrap();
        assert!((o - Vec3::new(10.0, 0.0, 500.0)).length() < 1e-4 && (d - Vec3::Z).length() < 1e-6, "{o} {d}");
        assert!(pins_axis(&[Vec3::ONE, Vec3::ONE]).is_none());
        assert!(pins_axis(&[Vec3::ONE]).is_none());
    }

    /// Due perni selezionati: il pulsante crea l'asse dai loro centri; poi si sceglie la parte, si
    /// salva, e «Cambia parte» la passa a un'altra selezione (la prima torna chiusa).
    #[test]
    fn hinge_from_selected_pins_and_change_of_part() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let (rif, dest) = (node(&app, "Riferimento", 0), node(&app, "Destinazione", 0));
        app.apply(Action::SelectMany { nodes: vec![rif, dest], primary: rif, anchor: rif });
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Asse dai 2 perni selezionati", 0);
        let (o, d) = app.ui_state.position.hinge.axis.expect("asse dai perni");
        let s = app.scene_graph.as_ref().unwrap();
        let (a, b) = (s.subtree_bounding_box(rif).center(), s.subtree_bounding_box(dest).center());
        assert!((o - (a + b) / 2.0).length() < 1e-3 && d.cross((b - a).normalize()).length() < 1e-4);

        // La parte giusta: la seconda copia del riferimento
        let part = node(&app, "Riferimento", 1);
        app.ui_state.select_single(Some(part));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Salva cerniera", 0);
        assert_eq!(app.scene_graph.as_ref().unwrap().hinges[0].nodes, vec![part]);
        let before = first_vertex(&app, part);
        app.apply(Action::SetHingeAngle { index: 0, angle: 40.0 });
        assert!((first_vertex(&app, part) - before).length() > 1.0, "gira");

        let other = node(&app, "Destinazione", 1);
        app.ui_state.select_single(Some(other));
        app.reassign_hinge(0, vec![other]);
        let s = app.scene_graph.as_ref().unwrap();
        assert_eq!((s.hinges[0].nodes.clone(), s.hinges[0].angle), (vec![other], 0.0));
        assert!((first_vertex(&app, part) - before).length() < 1e-3, "la parte di prima torna chiusa");
        let (o2, d2) = s.hinge_axis(&s.hinges[0]);
        assert!(d2.cross(d).length() < 1e-4 && (o2 - o).cross(d).length() < 1e-2, "stesso asse");
        app.undo();
        assert_eq!(app.scene_graph.as_ref().unwrap().hinges[0].nodes, vec![part]);
    }

    /// «Inverti verso» di una cerniera salvata: la parte torna chiusa, poi gli angoli positivi la
    /// aprono dall'altra parte; un Annulla torna com'era.
    #[test]
    fn flipping_a_saved_hinge_closes_it_and_opens_the_other_way() {
        let mut app = app();
        let part = node(&app, "Riferimento", 0);
        let closed = first_vertex(&app, part);
        let i = app.save_hinge("Porta", vec![part], Vec3::new(0.0, 0.0, 0.0), Vec3::Z, 0.0).unwrap();
        app.apply(Action::SetHingeAngle { index: i, angle: 30.0 });
        let one_way = first_vertex(&app, part);
        app.flip_hinge(i);
        assert!((first_vertex(&app, part) - closed).length() < 1e-3, "chiusa");
        assert_eq!(app.scene_graph.as_ref().unwrap().hinges[i].angle, 0.0);
        app.apply(Action::SetHingeAngle { index: i, angle: 30.0 });
        let other_way = first_vertex(&app, part);
        // Stesso arco dall'altra parte: simmetrici rispetto alla posizione chiusa
        let turn = |deg: f32| Mat4::from_rotation_z(deg.to_radians()).transform_point3(closed);
        assert!((one_way - turn(30.0)).length() < 1e-2, "{one_way} {}", turn(30.0));
        assert!((other_way - turn(-30.0)).length() < 1e-2, "{other_way} {}", turn(-30.0));
        app.undo();
        app.undo();
        assert!((first_vertex(&app, part) - one_way).length() < 1e-3, "Annulla");
    }

    /// Progetto nuovo: valori, offset e cerniera in lavorazione ripartono da zero.
    #[test]
    fn reset_values_keeps_only_the_choices() {
        let mut p = PositionPanel { open: true, unit: 10.0, value: 35.0, ..Default::default() };
        p.offset.xyz.x = 5300.0;
        p.hinge.axis = Some((Vec3::ZERO, Vec3::Z));
        p.hinge.step = 5.0;
        p.reset_values();
        assert!(p.open && p.unit == 10.0 && p.hinge.step == 5.0);
        assert_eq!((p.value, p.offset.xyz.x), (0.0, 0.0));
        assert!(p.hinge.axis.is_none());
    }

    #[test]
    fn long_names_are_shortened_in_the_middle() {
        assert_eq!(middle_ellipsis("corto", 26), "corto");
        let name = "Componente_00001_parte_lunga_ABCDEF";
        let short = middle_ellipsis(name, 26);
        assert_eq!(short.chars().count(), 26);
        assert!(short.starts_with("Componente_0") && short.ends_with("_ABCDEF"), "{short}");
    }

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
            // Alta: la finestra deve stare tutta sullo schermo, senza scorrere
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1800.0));
            let out = self.ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                draw(ctx, app, crate::ui::dock::Pass::Floating);
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

    #[test]
    fn align_delta_on_the_chosen_axes() {
        let b = |min: [f32; 3], max: [f32; 3]| Aabb { min: Vec3::from(min), max: Vec3::from(max) };
        let moving = b([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]);
        let reference = b([100.0, 50.0, -10.0], [120.0, 60.0, 0.0]);
        let only_x = [true, false, false];
        assert_eq!(align_delta(&moving, Some(&reference), AlignMode::Center, only_x), Vec3::new(105.0, 0.0, 0.0));
        assert_eq!(align_delta(&moving, Some(&reference), AlignMode::Min, [true, true, true]), Vec3::new(100.0, 50.0, -10.0));
        assert_eq!(align_delta(&moving, Some(&reference), AlignMode::Max, [false, false, true]), Vec3::new(0.0, 0.0, -30.0));
        assert_eq!(align_delta(&moving, None, AlignMode::Center, [false, true, false]), Vec3::new(0.0, -10.0, 0.0), "sullo zero");
    }

    /// «Allinea su punti»: quattro vertici del file presi sul 3D e i riferimenti (gli stessi punti
    /// spostati e ruotati, come presi in cabina): «Allinea» porta il file sui riferimenti, i punti
    /// del 3D lo seguono e Annulla lo rimette dov'era.
    #[test]
    fn align_on_points_brings_the_file_onto_the_references() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        app.settings.position_sections[6] = true;
        let file = app.scene_graph.as_ref().unwrap().open_files().nth(1).unwrap().node;
        app.ui_state.position.target = Some(Target::File(file));
        let moved = crate::scene_graph::placement::rigid(Vec3::new(15.0, -8.0, 4.0), Vec3::new(0.5, -0.4, 2.0), Vec3::ZERO);
        {
            let s = app.scene_graph.as_ref().unwrap();
            let meshes = s.meshes_of(&[file]);
            // Vertici lontani tra loro: gli angoli del riquadro più vicini
            let all: Vec<(u32, Vec3)> = meshes
                .iter()
                .flat_map(|&m| s.shapes_data[m as usize].world_vertices.iter().map(move |v| (m, Vec3::from(*v))))
                .collect();
            let b = s.subtree_bounding_box(file);
            let corners = [b.min, Vec3::new(b.max.x, b.min.y, b.min.z), Vec3::new(b.min.x, b.max.y, b.min.z), b.max];
            for (k, c) in corners.iter().enumerate() {
                let &(mesh, v) = all.iter().min_by(|a, b| a.1.distance(*c).total_cmp(&b.1.distance(*c))).unwrap();
                let pa = &mut app.ui_state.position.points;
                pa.add(&format!("RIF_{}", k + 1), moved.transform_point3(v));
                pa.pairs[k].cad = Some(crate::ui::measure::Measure::anchored(s, mesh, crate::ui::measure::MeasurePoint::free(v, true)));
            }
        }
        let before = first_vertex(&app, file);
        // La sezione «Allinea» è chiusa: il secondo «Allinea» è il pulsante dei punti
        // Un disegno in più: la sezione appena aperta allarga la finestra
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Allinea", 1);
        assert!(app.ui_state.status_message.starts_with("Allineato sui punti"), "{}", app.ui_state.status_message);
        let after = first_vertex(&app, file);
        assert!(after.distance(moved.transform_point3(before)) < 1e-2, "{after:?}");
        for p in &app.ui_state.position.points.pairs {
            let d = p.cad.unwrap().pos.distance(p.target);
            assert!(d < 1e-2, "{} resta a {d}", p.name);
        }
        app.undo();
        assert!(first_vertex(&app, file).distance(before) < 1e-3);
    }

    /// «Applica offset»: le coordinate con la casella spenta valgono 0.
    #[test]
    fn offset_uses_only_the_checked_coordinates() {
        let p = Pose { xyz: Vec3::new(1.0, 2.0, 3.0), abc: Vec3::new(4.0, 5.0, 6.0) };
        let used = used_offset(&p, &[true, true, false, true, true, false]);
        assert_eq!(used, Pose { xyz: Vec3::new(1.0, 2.0, 0.0), abc: Vec3::new(4.0, 5.0, 0.0) });
        assert_eq!(used_offset(&p, &[true; 6]), p);
    }

    /// «Allinea su misure»: nove quote prese «in cabina» tra il primo file (fermo) e il secondo
    /// (spostato e ruotato sul 3D): «Allinea» porta il secondo dove le quote tornano, Annulla lo
    /// rimette dov'era.
    #[test]
    fn align_on_hand_measures_moves_the_part() {
        use crate::ui::measure::{Measure, MeasurePoint};
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        app.settings.position_sections = [false; crate::settings::POSITION_SECTIONS];
        app.settings.position_sections[8] = true;
        let (fixed, part) = {
            let mut files = app.scene_graph.as_ref().unwrap().open_files();
            (files.next().unwrap().node, files.next().unwrap().node)
        };
        app.ui_state.position.target = Some(Target::File(part));
        let truth = crate::scene_graph::placement::rigid(Vec3::new(6.0, -4.0, 3.0), Vec3::new(0.5, -0.4, 1.5), Vec3::ZERO);
        {
            let s = app.scene_graph.as_ref().unwrap();
            let vertices = |node: u32| -> Vec<(u32, Vec3)> {
                s.meshes_of(&[node])
                    .iter()
                    .flat_map(|&m| s.shapes_data[m as usize].world_vertices.iter().map(move |v| (m, Vec3::from(*v))))
                    .collect()
            };
            // Angoli del riquadro, legati al pezzo (non serve che siano vertici)
            let corners = |node: u32| -> Vec<(u32, Vec3)> {
                let mesh = vertices(node)[0].0;
                let b = s.subtree_bounding_box(node);
                [b.min, Vec3::new(b.max.x, b.min.y, b.min.z), Vec3::new(b.min.x, b.max.y, b.min.z), Vec3::new(b.min.x, b.min.y, b.max.z)]
                    .iter()
                    .map(|c| (mesh, *c))
                    .collect()
            };
            let (f, p) = (corners(fixed), corners(part));
            let da = &mut app.ui_state.position.distances;
            da.mode = crate::ui::distance_align::Mode::Precise;
            // Precisa: tre punti fissi (non nel piano dei punti del pezzo: tutti in basso il calcolo
            // non distingue sopra da sotto), ognuno misurato verso gli stessi tre punti del pezzo
            for (i, j) in [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (3, 0), (3, 1), (3, 2)] {
                let a = Measure::anchored(s, f[i].0, MeasurePoint::free(f[i].1, true));
                let b = Measure::anchored(s, p[j].0, MeasurePoint::free(p[j].1, true));
                da.add_from(a, b);
                da.rows.last_mut().unwrap().measured = truth.transform_point3(p[j].1).distance(f[i].1);
            }
        }
        let before = first_vertex(&app, part);
        ui.run(&mut app, vec![]);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.starts_with("Schema completo: 3 punti fissi")), "{texts:?}");
        assert!(texts.iter().any(|(t, _)| t == "F1–M1") && texts.iter().any(|(t, _)| t == "    –M3"), "nomi F/M");
        // Il primo «Allinea» è il titolo della sezione chiusa
        ui.click(&mut app, "Allinea", 1);
        assert!(app.ui_state.status_message.starts_with("Allineato sulle misure"), "{}", app.ui_state.status_message);
        let after = first_vertex(&app, part);
        assert!(after.distance(truth.transform_point3(before)) < 0.1, "{after:?} {:?}", truth.transform_point3(before));
        for r in &app.ui_state.position.distances.rows {
            assert!((r.current().unwrap() - r.measured).abs() < 0.05, "{:?} {}", r.current(), r.measured);
        }
        app.undo();
        assert!(first_vertex(&app, part).distance(before) < 1e-3);
        // Guida illustrata: si apre e si disegna
        ui.click(&mut app, "Guida", 0);
        assert!(app.ui_state.position.measures_guide);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t == "Quante misure prendere"), "guida disegnata");
    }

    /// La finestra si stringe sotto la larghezza del contenuto (che scorre in orizzontale) e
    /// non si riallarga da sola a ogni disegno.
    #[test]
    fn position_window_narrower_than_its_content() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        app.settings.dock_position.width = 130.0;
        app.settings.position_sections = [true; crate::settings::POSITION_SECTIONS];
        for _ in 0..4 {
            ui.run(&mut app, vec![]);
        }
        let window = ui.ctx.memory(|m| m.area_rect(egui::Id::new("finestra_posizione"))).expect("finestra");
        assert!(window.width() < 200.0 && window.width() < CONTENT_MIN_WIDTH, "{window:?}");
    }

    /// «Allinea su misure» nel progetto: quote, tipi, valori, caselle e punti legati ai pezzi
    /// tornano uguali riaprendolo; un pezzo spostato porta con sé il suo punto.
    #[test]
    fn hand_measures_go_in_the_project() {
        use crate::ui::distance_align::Kind;
        use crate::ui::measure::{Measure, MeasurePoint};
        let mut app = app();
        let (fixed, part) = {
            let mut files = app.scene_graph.as_ref().unwrap().open_files();
            (files.next().unwrap().node, files.next().unwrap().node)
        };
        app.apply(Action::Move { nodes: vec![part], matrix: Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)) });
        {
            let s = app.scene_graph.as_ref().unwrap();
            let (mf, mp) = (s.meshes_of(&[fixed])[0], s.meshes_of(&[part])[0]);
            let vertex = |m: u32| Vec3::from(s.shapes_data[m as usize].world_vertices[0]);
            let da = &mut app.ui_state.position.distances;
            da.add_from(Measure::anchored(s, mf, MeasurePoint::free(vertex(mf), true)), Measure::anchored(s, mp, MeasurePoint::free(vertex(mp), true)));
            da.add_from(MeasurePoint::free(Vec3::new(1.0, 2.0, 3.0), false), Measure::anchored(s, mp, MeasurePoint::free(vertex(mp) + Vec3::X, false)));
            da.add_and_pick();
            da.stop_picking();
            da.add_and_pick();
            da.add_point(MeasurePoint::free(Vec3::ONE, true));
            da.stop_picking();
            da.rows[0].measured = 1234.5;
            da.set_kind(1, Kind::Z);
            da.rows[1].on = false;
            da.only_move = true;
            da.mode = crate::ui::distance_align::Mode::Precise;
        }
        assert_eq!(app.ui_state.position.distances.rows.len(), 3, "l'ultima con un punto solo");
        assert!(app.is_dirty());
        for (k, f) in app.scene_graph.as_mut().unwrap().sources.iter_mut().enumerate() {
            f.path = Some(std::path::PathBuf::from(format!(r"D:\prova\skid{k}.wrl")));
        }
        let scene = app.scene_graph.as_ref().unwrap();
        let mut file = crate::project_file::ProjectFile::capture(scene, Default::default(), None);
        file.capture_measures(scene, &app.ui_state.position.distances);
        let path = std::path::Path::new(r"D:\prova\misure.dima3d");
        let text = file.to_text(path);
        assert!(text.contains("[allinea su misure]\nmodo = precisa\nsolo_spostamento = 1\nmisura = distanza 1234.5 1 | "), "{text}");
        let back = crate::project_file::ProjectFile::from_text(&text, path).unwrap();
        let mut other = super::tests::app();
        let scene = other.scene_graph.as_mut().unwrap();
        let sources: Vec<Option<usize>> = (0..back.files.len()).map(Some).collect();
        for (i, entry) in back.files.iter().enumerate() {
            crate::project_file::ProjectFile::apply_entry(entry, scene, i);
        }
        let (rows, lost) = back.restore_measures(scene, &sources);
        assert_eq!(lost, 0);
        let expected = app.ui_state.position.distances.rows.clone();
        assert_eq!(rows.len(), 3);
        for (r, e) in rows.iter().zip(&expected) {
            assert_eq!((r.kind, r.measured, r.on), (e.kind, e.measured, e.on));
            for (p, q) in r.points.iter().zip(&e.points) {
                match (p, q) {
                    (Some(p), Some(q)) => {
                        assert!(p.pos.distance(q.pos) < 1e-4, "{p:?} {q:?}");
                        assert_eq!((p.vertex, p.anchor.map(|a| (a.mesh, a.key))), (q.vertex, q.anchor.map(|a| (a.mesh, a.key))));
                    }
                    (None, None) => {}
                    _ => panic!("punto perso: {p:?} {q:?}"),
                }
            }
        }
        let restored = crate::ui::distance_align::DistanceAlign::restored(rows, back.only_move, back.mode);
        assert!(restored.only_move && restored.mode == crate::ui::distance_align::Mode::Precise);
        assert_eq!(restored.labels(), app.ui_state.position.distances.labels(), "stessi nomi F/M");
    }

    /// «Misura dell'oggetto»: con i file local e world e l'oggetto attuale la sezione mostra le
    /// deviazioni, l'oggetto calcolato e la riga con lo stesso nome; escludere un punto ricalcola.
    #[test]
    fn object_measure_from_local_and_world_files() {
        use crate::ui::object_measure::{parse_bases, parse_point_file};
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        app.settings.position_sections[7] = true;
        let object = crate::scene_graph::placement::rigid(Vec3::new(-1000.0, -4000.0, 100.0), Vec3::new(0.0, 0.0, 90.0), Vec3::ZERO);
        let local = [Vec3::new(4600.0, -860.0, -20.0), Vec3::new(4900.0, -480.0, 440.0), Vec3::new(4900.0, 480.0, 440.0), Vec3::new(4600.0, 860.0, -20.0)];
        let text = |pts: &mut dyn Iterator<Item = Vec3>| {
            pts.enumerate().map(|(k, p)| format!("CONST POINT P{} = Object {} {} {} 0 0 0 0 0 0
", k + 1, p.x, p.y, p.z)).collect::<String>()
        };
        let m = &mut app.ui_state.position.measure;
        m.local = Some(parse_point_file("RIFL.tid".into(), &text(&mut local.iter().copied())).unwrap());
        m.world = Some(parse_point_file("RIFW.tid".into(), &text(&mut local.iter().map(|p| object.transform_point3(*p)))).unwrap());
        m.bases = parse_bases("CONST BASE BASE_A = -1001 -4000 100 0 0 90
");
        m.base = Some("BASE_A".into());
        ui.run(&mut app, vec![]);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t == "CONST BASE BASE_A = -1000 -4000 100 0 0 90"), "riga dell'oggetto");
        assert!(texts.iter().any(|(t, _)| t == "Differenza (calcolato − attuale)"));
        assert!(texts.iter().filter(|(t, _)| t == "0.00").count() >= 4, "deviazioni dei punti");
        app.ui_state.position.measure.set_on("P4", false);
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.starts_with("Servono almeno 4 punti")), "tre punti non bastano");
    }

    /// «Allinea»: lungo X sul riferimento scelto con «Usa selezione»; «Stesso pezzo» rimette
    /// la copia spostata sopra l'originale.
    #[test]
    fn align_moves_the_object_onto_the_reference() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let (rif, dest, copy) = (node(&app, "Riferimento", 0), node(&app, "Destinazione", 0), node(&app, "Riferimento", 1));
        let center = |app: &GestoreApp, n| app.scene_graph.as_ref().unwrap().subtree_bounding_box(n).center();
        app.ui_state.select_single(Some(dest));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Allinea", 0);
        ui.click(&mut app, "Usa selezione", 0);
        assert_eq!(app.ui_state.position.align_reference, [dest]);
        app.ui_state.select_single(Some(rif));
        app.ui_state.position.align_axes = [true, false, false];
        let before = center(&app, rif);
        ui.click(&mut app, "Allinea", 1);
        let after = center(&app, rif);
        assert!((after.x - center(&app, dest).x).abs() < 1e-3, "{after:?}");
        assert!((after.y - before.y).abs() < 1e-3 && (after.z - before.z).abs() < 1e-3);

        app.apply(Action::Move { nodes: vec![copy], matrix: Mat4::from_translation(Vec3::new(10.0, 20.0, 30.0)) });
        app.ui_state.select_single(Some(rif));
        ui.click(&mut app, "Usa selezione", 0);
        app.ui_state.select_single(Some(copy));
        app.ui_state.position.align_axes = [true, true, true];
        app.ui_state.position.align_mode = AlignMode::SamePiece;
        ui.click(&mut app, "Allinea", 1);
        let (va, vb) = (first_vertex(&app, copy), first_vertex(&app, rif));
        assert!((va - vb).length() < 1e-2, "{va:?} {vb:?}");
    }

    /// Cerniera salvata: «Salva cerniera» ricorda asse e parte; la parte si apre ai gradi scritti
    /// (entro i limiti), l'asse segue l'oggetto spostato con Posizione, Annulla torna indietro e
    /// il progetto la riporta uguale.
    #[test]
    fn saved_hinge_opens_follows_the_object_and_goes_in_the_project() {
        let mut app = app();
        let ui = Ui { ctx: egui::Context::default() };
        let rif = node(&app, "Riferimento", 0);
        app.ui_state.select_single(Some(rif));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Due punti", 0);
        ui.click(&mut app, "Scegli l'asse nella vista", 0);
        app.ui_state.position.hinge.add_point(Vec3::new(100.0, 200.0, 300.0));
        app.ui_state.position.hinge.add_point(Vec3::new(100.0, 200.0, 310.0));
        app.ui_state.position.hinge.save_name = "Porta".into();
        ui.click(&mut app, "Salva cerniera", 0);
        let scene = app.scene_graph.as_ref().unwrap();
        assert_eq!(scene.hinges.len(), 1, "{}", app.ui_state.status_message);
        assert_eq!(scene.hinges[0].name, "Porta");
        let h = &app.ui_state.position.hinge;
        assert!(h.axis.is_none() && h.points.is_empty(), "salvata: niente asse fisso nella vista");
        assert_eq!(app.selected_hinge(), Some(0), "la parte selezionata ha la cerniera");
        let file_root = app.scene_graph.as_ref().unwrap().sources[0].node;
        assert_eq!(app.scene_graph.as_ref().unwrap().hinge_of_node(file_root), None, "tutto il file non è la parte");
        let texts = ui.run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t.ends_with("Porta")), "nell'elenco");

        let vertex = |app: &GestoreApp, i: usize| {
            let s = app.scene_graph.as_ref().unwrap();
            Vec3::from(s.shapes_data[s.meshes_of(&[rif])[0] as usize].world_vertices[i])
        };
        app.apply(Action::SetHingeAngle { index: 0, angle: 90.0 });
        assert!((vertex(&app, 1) - Vec3::new(50.0, 200.0, 300.0)).length() < 1e-3, "{:?}", vertex(&app, 1));
        app.apply(Action::SetHingeAngle { index: 0, angle: 200.0 });
        assert_eq!(app.scene_graph.as_ref().unwrap().hinges[0].angle, 90.0, "fermo al limite");
        let undo_steps = app.project.undo.len();

        // Tutto l'oggetto si sposta: l'asse lo segue, chiudendo la parte torna al suo posto
        let shift = Vec3::new(1000.0, 0.0, 0.0);
        app.apply(Action::Move { nodes: vec![rif], matrix: Mat4::from_translation(shift) });
        let (o, _) = { let s = app.scene_graph.as_ref().unwrap(); s.hinge_axis(&s.hinges[0]) };
        assert!((o - Vec3::new(1100.0, 200.0, 300.0)).length() < 1e-3, "{o}");
        app.apply(Action::SetHingeAngle { index: 0, angle: 0.0 });
        assert!((vertex(&app, 1) - (Vec3::new(100.0, 250.0, 300.0) + shift)).length() < 1e-3, "{:?}", vertex(&app, 1));

        // Progetto: stessa cerniera, stesso asse
        app.apply(Action::SetHingeAngle { index: 0, angle: 30.0 });
        // Nel progetto vanno solo i modelli con un file (qui letti dalla memoria)
        for (k, f) in app.scene_graph.as_mut().unwrap().sources.iter_mut().enumerate() {
            f.path = Some(std::path::PathBuf::from(format!(r"D:\prova\skid{k}.wrl")));
        }
        let file = crate::project_file::ProjectFile::capture(app.scene_graph.as_ref().unwrap(), Default::default(), None);
        let path = std::path::Path::new(r"D:\prova\cerniere.dima3d");
        let text = file.to_text(path);
        assert!(text.contains("cerniera = "), "{text}");
        let back = crate::project_file::ProjectFile::from_text(&text, path).unwrap();
        let mut other = super::tests::app();
        let scene = other.scene_graph.as_mut().unwrap();
        for (i, entry) in back.files.iter().enumerate() {
            assert_eq!(crate::project_file::ProjectFile::apply_entry(entry, scene, i), 0);
        }
        let (a, b) = (app.scene_graph.as_ref().unwrap(), other.scene_graph.as_ref().unwrap());
        assert_eq!(b.hinges.len(), 1);
        assert_eq!((b.hinges[0].name.as_str(), b.hinges[0].angle, b.hinges[0].nodes.clone()), ("Porta", 30.0, a.hinges[0].nodes.clone()));
        let ((o1, d1), (o2, d2)) = (a.hinge_axis(&a.hinges[0]), b.hinge_axis(&b.hinges[0]));
        assert!((o1 - o2).length() < 1e-3 && (d1 - d2).length() < 1e-5);

        // Annulla: si torna indietro passo per passo, fino a prima della cerniera
        for _ in 0..10 {
            app.undo();
        }
        assert!(app.scene_graph.as_ref().unwrap().hinges.is_empty());
        assert!((vertex(&app, 1) - Vec3::new(100.0, 250.0, 300.0)).length() < 1e-3);
        assert!(undo_steps >= 2, "salvataggio e apertura sono passi distinti");
    }

    /// Trascinando la parte con il mouse il punto preso segue il mouse lungo l'arco: in una vista
    /// dall'alto, asse Z e punto a 100 mm, 100·π/2 pixel verso +Y aprono di 90°.
    #[test]
    fn dragging_the_part_follows_the_mouse() {
        let mut app = app();
        let rif = node(&app, "Riferimento", 0);
        let i = app.save_hinge("Sportello", vec![rif], Vec3::ZERO, Vec3::Z, 0.0).unwrap();
        let s = app.scene_graph.as_ref().unwrap();
        let screen = |v: Vec3| Some(glam::vec2(v.x, -v.y));
        let pixels = 100.0 * std::f32::consts::FRAC_PI_2;
        let deg = s.hinge_drag_degrees(i, Vec3::new(100.0, 0.0, 0.0), glam::vec2(0.0, -pixels), screen).unwrap();
        assert!((deg - 90.0).abs() < 0.5, "{deg}");
        let back = s.hinge_drag_degrees(i, Vec3::new(100.0, 0.0, 0.0), glam::vec2(0.0, pixels), screen).unwrap();
        assert!((back + 90.0).abs() < 0.5, "{back}");
        assert!(s.hinge_drag_degrees(i, Vec3::ZERO, glam::vec2(0.0, 10.0), screen).is_none(), "sull'asse");
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
        // Oggetto con un Transform ruotato (C = 90°): nell'oggetto e in World l'offset è diverso
        let rif = node(&app, "Riferimento", 0);
        app.ui_state.select_single(Some(rif));
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

        // Il nuovo oggetto del robot: la base di origine spostata dell'offset è la destinazione,
        // sia con l'offset nell'oggetto (World spento) sia con quello in World (acceso)
        app.ui_state.position.robot_line = "CONST BASE BASE_A = 150 2500 -1200 0.5 -20 -85".into();
        for (k, world) in [(0, false), (1, true)] {
            ui.click(&mut app, "Usa come offset", k);
            assert_eq!(app.ui_state.position.world, world);
            let texts = ui.run(&mut app, vec![]);
            assert!(
                texts.iter().any(|(t, _)| t == "CONST BASE BASE_A = 180 3100 40 1.2 -12.5 -91"),
                "World {world}: {:?}",
                texts.iter().map(|t| &t.0).filter(|t| t.contains("BASE_A")).collect::<Vec<_>>()
            );
        }
        // La riga World è lo stesso spostamento visto dalla scena per l'oggetto scelto
        let w = app.ui_state.position.offset;
        let f = Pose::from_matrix(app.scene_graph.as_ref().unwrap().object_frame(rif));
        assert!(w.matrix().abs_diff_eq(offset_in_world(&p, &f).matrix(), 1e-2), "{w:?}");
        assert!((w.xyz - p.xyz).length() > 1.0, "diverso da quello nell'oggetto: {w:?}");
    }

    /// Matematica senza Transform (nelle coordinate del BASE con cui è stata fatta, come un
    /// paraurti): l'offset tra due BASE si applica uguale con World spento o acceso, e la riga
    /// World coincide con quella nell'oggetto, come nel visualizzatore usato in azienda.
    #[test]
    fn offset_between_bases_on_a_file_without_transform() {
        const PEZZO: &[u8] = br#"#VRML V2.0 utf8
DEF Cantonale Group { children [
  Shape { geometry IndexedFaceSet { coord Coordinate { point [ 3800 -700 10, 3900 -700 10, 3800 -600 90 ] } coordIndex [ 0 1 2 -1 ] } }
] }
"#;
        let mut app = GestoreApp::new_headless();
        let mut scene = crate::parser::parse_wrl(PEZZO);
        scene.into_file("pezzo.wrl".into(), None);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.ui_state.position.open = true;
        let ui = Ui { ctx: egui::Context::default() };
        let piece = node(&app, "Cantonale", 0);
        let vertices = |app: &GestoreApp| {
            let s = app.scene_graph.as_ref().unwrap();
            s.shapes_data[s.meshes_of(&[piece])[0] as usize].world_vertices.clone()
        };
        let start = vertices(&app);
        app.ui_state.select_single(Some(piece));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "Calcola offset", 0);
        ui.click(&mut app, "Tra due CONST BASE", 0);
        app.ui_state.position.base_a = "CONST BASE BASE_A = -1900 3600 900 0.5 -0.5 -90".into();
        app.ui_state.position.base_b = "CONST BASE BASE_B = -2300 3300 1700 4 -8 -86".into();
        ui.click(&mut app, "Calcola", 0);
        let p = app.ui_state.position.result.expect("offset");
        let expected: Vec<Vec3> = start.iter().map(|v| p.matrix().transform_point3(Vec3::from(*v))).collect();

        // Un solo risultato, senza «oggetto» o «World»
        let texts = ui.run(&mut app, vec![]);
        assert_eq!(texts.iter().filter(|(t, _)| t == "Usa come offset").count(), 1);
        assert!(!texts.iter().any(|(t, _)| t == "Offset in coordinate World"));
        let mut moved = Vec::new();
        for world in [false, true] {
            app.ui_state.position.world = world;
            ui.click(&mut app, "Usa come offset", 0);
            assert_eq!(app.ui_state.position.world, world, "il selettore resta com'è");
            assert!(app.ui_state.position.offset.matrix().abs_diff_eq(p.matrix(), 1e-3), "{:?}", app.ui_state.position.offset);
            ui.click(&mut app, "Applica", 0);
            moved.push(vertices(&app));
            app.undo();
        }
        for got in moved {
            for (a, b) in got.iter().zip(&expected) {
                assert!((Vec3::from(*a) - *b).length() < 0.05, "{a:?} invece di {b:?}");
            }
        }
    }

    /// Con World acceso, scrivere a mano X Y Z e poi A B C con il cursore dà la posa X Y Z A B C:
    /// le rotazioni girano attorno all'origine dell'oggetto, X Y Z non cambiano.
    #[test]
    fn world_rotations_keep_the_object_position() {
        let target = Pose { xyz: Vec3::new(240.5, -515.25, 930.75), abc: Vec3::new(3.5, 9.25, 6.0) };
        for frame0 in [Mat4::IDENTITY, Mat4::from_translation(Vec3::new(30.0, -20.0, 5.0))] {
            let mut frame = frame0;
            for axis in 0..6 {
                let v = if axis < 3 { target.xyz[axis] } else { target.abc[axis - 3] };
                frame = slider_motion(axis_motion(axis, v), frame, true) * frame;
                if axis >= 3 {
                    let xyz = Pose::from_matrix(frame).xyz;
                    assert!((xyz - frame0.w_axis.truncate() - target.xyz).length() < 1e-2, "dopo {axis}: {xyz:?}");
                }
            }
            let want = Mat4::from_translation(frame0.w_axis.truncate()) * target.matrix();
            assert!(frame.abs_diff_eq(want, 1e-2), "{:?}", Pose::from_matrix(frame));
        }
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
        assert!(window.width() < 300.0 && window.height() < 950.0, "{window:?}");
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

    fn cofano_app() -> GestoreApp {
        let mut app = GestoreApp::new_headless();
        let mut scene = crate::parser::parse_wrl(COFANO);
        scene.into_file("cofano.wrl".into(), None);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.ui_state.position.open = true;
        app
    }

    /// Cerniera a 4 perni salvata: si apre con i gradi muovendo cofano e bielle, si ferma al fine
    /// corsa, segue l'oggetto spostato, «0° qui» e «Inverti verso» tengono il meccanismo, il
    /// progetto la riporta uguale.
    #[test]
    fn saved_four_pin_hinge_opens_the_hood() {
        let mut app = cofano_app();
        let ui = Ui { ctx: egui::Context::default() };
        let [arm, l1, l2, plate] = ["Braccio", "Biella1", "Biella2", "Piastra"].map(|n| node(&app, n, 0));
        ui.run(&mut app, vec![]);
        ui.click(&mut app, "4 perni (cofano)", 0);
        for (k, n) in [arm, l1, l2].into_iter().enumerate() {
            app.ui_state.select_single(Some(n));
            ui.click(&mut app, "Usa selezione", k);
        }
        ui.click(&mut app, "Scegli i 4 perni nella vista", 0);
        for (x, z) in [(0.0, 0.0), (50.0, 0.0), (-10.0, 80.0), (45.0, 70.0)] {
            for (dx, dz) in [(3.0, 0.0), (0.0, 3.0), (-3.0, 0.0)] {
                app.ui_state.position.hinge.add_point(Vec3::new(x + dx, 0.0, z + dz));
            }
        }
        app.ui_state.position.hinge.save_name = "Cofano".into();
        ui.run(&mut app, vec![]);
        // Senza «Inverti verso»: al salvataggio il verso positivo è quello in cui il cofano si apre
        ui.click(&mut app, "Salva cerniera", 0);
        let h = app.scene_graph.as_ref().unwrap().hinges.first().cloned().expect(&app.ui_state.status_message);
        assert_eq!(h.name, "Cofano");
        assert_eq!(h.linkage.as_ref().unwrap().links, [vec![l1], vec![l2]]);
        assert!(h.max > 20.0 && h.max <= 120.0, "fine corsa {}", h.max);
        assert!(app.ui_state.position.hinge.four.is_none(), "il meccanismo ora è della cerniera");
        app.ui_state.select_single(Some(arm));
        assert_eq!(app.selected_hinge(), Some(0));

        let vertex = |app: &GestoreApp, n: u32, i: usize| {
            let s = app.scene_graph.as_ref().unwrap();
            Vec3::from(s.shapes_data[s.meshes_of(&[n])[0] as usize].world_vertices[i])
        };
        let joined = |app: &GestoreApp| {
            vertex(app, arm, 0).distance(vertex(app, l1, 1)) < 1e-2 && vertex(app, arm, 1).distance(vertex(app, l2, 1)) < 1e-2
        };
        app.apply(Action::SetHingeAngle { index: 0, angle: 20.0 });
        let (m1, m2) = (vertex(&app, arm, 0), vertex(&app, arm, 1));
        assert!(m1.distance(Vec3::new(-10.0, 0.0, 80.0)) > 5.0, "il cofano si è aperto");
        assert!(joined(&app), "perni mobili uniti alle bielle");
        assert!(vertex(&app, l1, 0).distance(Vec3::ZERO) < 1e-3 && vertex(&app, l2, 0).distance(Vec3::new(50.0, 0.0, 0.0)) < 1e-3, "perni fissi fermi");
        let turn = (m2 - m1).normalize().dot(Vec3::new(55.0, 0.0, -10.0).normalize()).acos().to_degrees();
        assert!((turn - 20.0).abs() < 1e-2, "{turn}");
        assert_eq!(vertex(&app, plate, 2), Vec3::new(25.0, 0.0, -20.0));
        // Oltre il fine corsa si ferma al limite
        app.apply(Action::SetHingeAngle { index: 0, angle: 500.0 });
        let max = app.scene_graph.as_ref().unwrap().hinges[0].angle;
        assert!((max - h.max).abs() < 1e-3 && joined(&app), "{max}");

        // Tutto l'oggetto si sposta e gira: la cerniera lo segue
        let moved = Mat4::from_translation(Vec3::new(1000.0, 200.0, -50.0)) * Mat4::from_rotation_z(0.7);
        app.apply(Action::Move { nodes: vec![arm, l1, l2, plate], matrix: moved });
        app.apply(Action::SetHingeAngle { index: 0, angle: 0.0 });
        assert!(vertex(&app, arm, 0).distance(moved.transform_point3(Vec3::new(-10.0, 0.0, 80.0))) < 1e-2, "chiuso al suo posto");
        app.apply(Action::SetHingeAngle { index: 0, angle: 20.0 });
        assert!(joined(&app));
        assert!(vertex(&app, l1, 0).distance(moved.transform_point3(Vec3::ZERO)) < 1e-2);
        let open20 = vertex(&app, arm, 2);

        // «0° qui» a 20°: altri 10° portano il meccanismo a 30°
        app.hinge_closed_here(0);
        app.apply(Action::SetHingeAngle { index: 0, angle: 10.0 });
        assert!(joined(&app));
        let s = app.scene_graph.as_ref().unwrap();
        assert!((s.hinges[0].max - (h.max - 20.0)).abs() < 1e-3, "il limite segue lo 0");
        // «Inverti verso»: torna a 0° (il meccanismo a 20°), poi i gradi positivi lo chiudono
        app.flip_hinge(0);
        assert!(vertex(&app, arm, 2).distance(open20) < 1e-2, "di nuovo dove era «0° qui»");
        app.apply(Action::SetHingeAngle { index: 0, angle: 20.0 });
        assert!(vertex(&app, arm, 0).distance(moved.transform_point3(Vec3::new(-10.0, 0.0, 80.0))) < 1e-2, "chiuso");
        assert!(joined(&app));
        app.apply(Action::SetHingeAngle { index: 0, angle: 5.0 });

        // Progetto: stessa cerniera, stessi perni
        for (k, f) in app.scene_graph.as_mut().unwrap().sources.iter_mut().enumerate() {
            f.path = Some(std::path::PathBuf::from(format!(r"D:\prova\cofano{k}.wrl")));
        }
        let file = crate::project_file::ProjectFile::capture(app.scene_graph.as_ref().unwrap(), Default::default(), None);
        let path = std::path::Path::new(r"D:\prova\cofano.dima3d");
        let text = file.to_text(path);
        assert!(text.contains("cerniera_perni = "), "{text}");
        let back = crate::project_file::ProjectFile::from_text(&text, path).unwrap();
        let mut other = cofano_app();
        let scene = other.scene_graph.as_mut().unwrap();
        for (i, entry) in back.files.iter().enumerate() {
            assert_eq!(crate::project_file::ProjectFile::apply_entry(entry, scene, i), 0);
        }
        let (a, b) = (app.scene_graph.as_ref().unwrap(), other.scene_graph.as_ref().unwrap());
        assert_eq!(b.hinges.len(), 1);
        assert_eq!(b.hinges[0].linkage.as_ref().unwrap().links, [vec![l1], vec![l2]]);
        let (pa, pb) = (a.hinge_pins(&a.hinges[0]).unwrap(), b.hinge_pins(&b.hinges[0]).unwrap());
        assert!(pa.iter().zip(pb).all(|(p, q)| p.distance(q) < 1e-2), "{pa:?} {pb:?}");
        other.apply(Action::SetHingeAngle { index: 0, angle: 20.0 });
        assert!(joined(&other));
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
