//! Finestre agganciabili ai bordi (come i pannelli di FreeCAD): mobili e ridimensionabili, oppure
//! agganciate a sinistra, a destra, in alto o in basso accanto alla vista 3D.
//!
//! Trascinando una finestra mobile vicino a un bordo compare l'anteprima dell'aggancio («calamita»);
//! lasciandola lì si aggancia. Una finestra agganciata si allarga trascinando il suo bordo e si
//! sgancia con il pulsante nella barra del titolo o trascinando la barra verso il centro.
//!
//! Ogni finestra si disegna due volte per fotogramma: prima dei pannelli della vista
//! ([`Pass::Docked`], solo se agganciata) e dopo ([`Pass::Floating`], solo se mobile), perché in
//! egui i pannelli laterali vanno creati prima di quello centrale.

use super::theme::{icon, icon_text};

/// Dove sta la finestra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Floating,
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn code(self) -> &'static str {
        match self {
            Side::Floating => "mobile",
            Side::Left => "sinistra",
            Side::Right => "destra",
            Side::Top => "alto",
            Side::Bottom => "basso",
        }
    }

    fn from_code(code: &str) -> Option<Side> {
        [Side::Floating, Side::Left, Side::Right, Side::Top, Side::Bottom].into_iter().find(|s| s.code() == code)
    }

    fn vertical(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

/// Stato di una finestra: dove sta e quanto è larga (o alta, se agganciata in alto o in basso).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockState {
    pub side: Side,
    /// Larghezza da agganciata a sinistra/destra, altezza da agganciata in alto/basso (0: predefinita)
    pub width: f32,
    pub height: f32,
}

impl Default for DockState {
    fn default() -> Self {
        Self { side: Side::Floating, width: 0.0, height: 0.0 }
    }
}

impl DockState {
    /// `destra 380 260`
    pub fn to_text(self) -> String {
        format!("{} {} {}", self.side.code(), self.width, self.height)
    }

    pub fn from_text(text: &str) -> Option<DockState> {
        let mut words = text.split_whitespace();
        let side = Side::from_code(words.next()?)?;
        let width = words.next().and_then(|w| w.parse().ok()).unwrap_or(0.0);
        let height = words.next().and_then(|w| w.parse().ok()).unwrap_or(0.0);
        Some(DockState { side, width, height })
    }
}

/// In che momento del fotogramma si disegna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Prima della vista 3D: le finestre agganciate
    Docked,
    /// Dopo: le finestre mobili
    Floating,
}

/// Descrizione di una finestra agganciabile.
pub struct Dockable<'a> {
    pub id: &'static str,
    pub title: String,
    pub state: &'a mut DockState,
    pub open: &'a mut bool,
    pub default_width: f32,
    pub min_width: f32,
    /// Altezza iniziale da mobile (il contenuto più alto scorre)
    pub default_height: f32,
    /// Posizione iniziale da mobile
    pub default_pos: egui::Pos2,
    /// Da agganciata il contenuto scorre in verticale; `false` per un contenuto che occupa
    /// esattamente lo spazio della finestra (es. la vista del PDF)
    pub scroll: bool,
}

/// Distanza dal bordo entro cui una finestra trascinata si aggancia.
const MAGNET: f32 = 48.0;

/// Trascinamento di una finestra mobile, tra un fotogramma e l'altro.
#[derive(Clone, Copy, Default)]
struct DragTrack {
    last_min: Option<egui::Pos2>,
    dragging: bool,
}

/// Bordo vicino al puntatore, dentro l'area libera `area` (tra barra degli strumenti e barra di stato).
fn magnet_side(area: egui::Rect, pointer: egui::Pos2) -> Option<Side> {
    if !area.expand(MAGNET).contains(pointer) {
        return None;
    }
    let distances = [
        (pointer.x - area.left(), Side::Left),
        (area.right() - pointer.x, Side::Right),
        (pointer.y - area.top(), Side::Top),
        (area.bottom() - pointer.y, Side::Bottom),
    ];
    distances.into_iter().filter(|(d, _)| *d < MAGNET).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, s)| s)
}

/// Bordo dell'area toccato dalla finestra trascinata `window` (come in FreeCAD: basta portare
/// la finestra contro il bordo, anche se il puntatore è lontano).
fn touched_side(area: egui::Rect, window: egui::Rect) -> Option<Side> {
    const TOUCH: f32 = 6.0;
    let distances = [
        (window.left() - area.left(), Side::Left),
        (area.right() - window.right(), Side::Right),
        (window.top() - area.top(), Side::Top),
        (area.bottom() - window.bottom(), Side::Bottom),
    ];
    // Una finestra alta quasi quanto l'area tocca sempre sopra o sotto: lì non conta
    let tall = window.height() > area.height() * 0.85;
    let wide = window.width() > area.width() * 0.85;
    distances
        .into_iter()
        .filter(|(d, s)| *d < TOUCH && !(s.vertical() && tall) && !(!s.vertical() && wide))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}

/// Rettangolo occupato da una finestra agganciata a `side` dentro `area`.
fn preview_rect(area: egui::Rect, side: Side, width: f32, height: f32) -> egui::Rect {
    match side {
        Side::Left => egui::Rect::from_min_size(area.min, egui::vec2(width, area.height())),
        Side::Right => egui::Rect::from_min_max(egui::pos2(area.right() - width, area.top()), area.max),
        Side::Top => egui::Rect::from_min_size(area.min, egui::vec2(area.width(), height)),
        Side::Bottom => egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - height), area.max),
        Side::Floating => area,
    }
}

/// Area libera per le finestre: lo schermo senza barra degli strumenti e barra di stato.
pub(crate) fn free_area(ctx: &egui::Context) -> egui::Rect {
    let screen = ctx.screen_rect();
    egui::Rect::from_min_max(screen.min + egui::vec2(0.0, 42.0), screen.max - egui::vec2(0.0, 26.0))
}

/// Disegna la finestra nel passaggio giusto. Ritorna il valore di `add` se è stata disegnata.
pub fn show<R>(ctx: &egui::Context, pass: Pass, w: Dockable, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let default_height = 260.0;
    if w.state.width <= 0.0 {
        w.state.width = w.default_width;
    }
    if w.state.height <= 0.0 {
        w.state.height = default_height;
    }
    match (pass, w.state.side) {
        (Pass::Docked, Side::Floating) | (Pass::Floating, Side::Left | Side::Right | Side::Top | Side::Bottom) => None,
        (Pass::Docked, side) => Some(docked(ctx, side, w, add)),
        (Pass::Floating, _) => floating(ctx, w, add),
    }
}

fn docked<R>(ctx: &egui::Context, side: Side, w: Dockable, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let id = egui::Id::new(w.id).with("aggancio");
    let body = |ui: &mut egui::Ui, state: &mut DockState, open: &mut bool| {
        // Barra del titolo: trascinandola verso il centro la finestra si sgancia
        let header = ui.horizontal(|ui| {
            // Solo il titolo è trascinabile: i pulsanti accanto devono ricevere i clic
            let title = ui
                .add(egui::Label::new(egui::RichText::new(&w.title).strong()).selectable(false).sense(egui::Sense::drag()))
                .on_hover_cursor(egui::CursorIcon::Grab);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(icon_text(ui, icon::X, "")).on_hover_text("Chiudi").clicked() {
                    *open = false;
                }
                if ui
                    .button(icon_text(ui, icon::ARROW_SQUARE_OUT, ""))
                    .on_hover_text("Sgancia: torna una finestra mobile (o trascina la barra del titolo)")
                    .clicked()
                {
                    state.side = Side::Floating;
                }
            });
            title
        });
        let drag = header.inner;
        if drag.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if drag.drag_stopped() {
            let moved = drag.interact_pointer_pos().zip(ui.ctx().input(|i| i.pointer.press_origin()));
            if let Some((now, from)) = moved {
                if now.distance(from) > 60.0 {
                    state.side = Side::Floating;
                    // La finestra mobile ricompare dove è stata lasciata
                    ui.ctx().data_mut(|d| d.insert_temp(id.with("rilascio"), now - egui::vec2(40.0, 10.0)));
                }
            }
        }
        ui.separator();
        if w.scroll {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, add).inner
        } else {
            add(ui)
        }
    };
    let (state, open) = (w.state, w.open);
    let frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::same(8));
    if side.vertical() {
        let panel = match side {
            Side::Top => egui::TopBottomPanel::top(id),
            _ => egui::TopBottomPanel::bottom(id),
        };
        let r = panel
            .resizable(true)
            .default_height(state.height)
            .height_range(80.0..=(ctx.screen_rect().height() * 0.7).max(120.0))
            .frame(frame)
            .show(ctx, |ui| body(ui, state, open));
        state.height = r.response.rect.height();
        r.inner
    } else {
        let panel = match side {
            Side::Left => egui::SidePanel::left(id),
            _ => egui::SidePanel::right(id),
        };
        let r = panel
            .resizable(true)
            .default_width(state.width)
            .min_width(w.min_width)
            .max_width((ctx.screen_rect().width() * 0.5).max(w.min_width + 20.0))
            .frame(frame)
            .show(ctx, |ui| body(ui, state, open));
        state.width = r.response.rect.width();
        r.inner
    }
}

fn floating<R>(ctx: &egui::Context, w: Dockable, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let id = egui::Id::new(w.id);
    let release_key = egui::Id::new(w.id).with("aggancio").with("rilascio");
    let mut window = egui::Window::new(&w.title)
        .id(id)
        .open(w.open)
        .resizable(true)
        .collapsible(true)
        .constrain(true)
        .min_width(w.min_width)
        .default_width(w.state.width)
        .default_height(w.default_height)
        .default_pos(w.default_pos);
    if let Some(pos) = ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(release_key)) {
        window = window.current_pos(pos);
    }
    let response = window.show(ctx, add)?;

    // Calamita: la finestra si muove con il tasto premuto → si sta trascinando
    let track_key = id.with("trascinamento");
    let mut track: DragTrack = ctx.data(|d| d.get_temp(track_key)).unwrap_or_default();
    let (down, pointer) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
    let min = response.response.rect.min;
    if down && track.last_min.is_some_and(|m| m != min) {
        track.dragging = true;
    }
    let area = free_area(ctx);
    let rect = response.response.rect;
    let candidate = if track.dragging {
        pointer.and_then(|p| magnet_side(area, p)).or_else(|| touched_side(area, rect))
    } else {
        None
    };
    if let Some(side) = candidate {
        let rect = preview_rect(area, side, w.state.width, w.state.height);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, track_key));
        let accent = ctx.style().visuals.selection.bg_fill;
        painter.rect_filled(rect, 4.0, accent.gamma_multiply(0.25));
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(2.0_f32, accent), egui::StrokeKind::Inside);
    }
    if !down {
        if let Some(side) = candidate {
            w.state.side = side;
        }
        track.dragging = false;
    }
    track.last_min = Some(min);
    ctx.data_mut(|d| d.insert_temp(track_key, track));
    response.inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnet_picks_the_nearest_edge() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 42.0), egui::pos2(1280.0, 774.0));
        assert_eq!(magnet_side(area, egui::pos2(10.0, 400.0)), Some(Side::Left));
        assert_eq!(magnet_side(area, egui::pos2(1275.0, 400.0)), Some(Side::Right));
        assert_eq!(magnet_side(area, egui::pos2(600.0, 50.0)), Some(Side::Top));
        assert_eq!(magnet_side(area, egui::pos2(600.0, 770.0)), Some(Side::Bottom));
        assert_eq!(magnet_side(area, egui::pos2(600.0, 400.0)), None, "al centro niente aggancio");
        assert_eq!(magnet_side(area, egui::pos2(20.0, 50.0)), Some(Side::Top), "nell'angolo vince il bordo più vicino");
    }

    /// Un fotogramma come nel programma: disposizione con le finestre agganciate, poi le mobili.
    fn frame(ctx: &egui::Context, app: &mut crate::app::GestoreApp, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
            crate::ui::draw_main_layout(ctx, app);
            crate::ui::cabin_panel::draw(ctx, app, Pass::Floating);
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

    /// Trascinando la finestra Cabina al bordo destro si aggancia (la vista si stringe), resta
    /// agganciata nelle preferenze; «Sgancia» la rimette mobile.
    #[test]
    fn dragging_a_window_to_the_edge_docks_it() {
        let mut app = crate::app::GestoreApp::new_headless();
        app.ui_state.cabin_open = true;
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        let texts = frame(&ctx, &mut app, vec![]);
        let title = texts.iter().find(|(t, _)| *t == format!("{}  Cabina", icon::CUBE_TRANSPARENT)).expect("titolo").1.center();
        let target = egui::pos2(1270.0, 400.0);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(title)]);
        frame(&ctx, &mut app, vec![button(title, true)]);
        for k in 1..=8 {
            let p = title + (target - title) * (k as f32 / 8.0);
            frame(&ctx, &mut app, vec![egui::Event::PointerMoved(p)]);
        }
        frame(&ctx, &mut app, vec![button(target, false)]);
        assert_eq!(app.settings.dock_cabin.side, Side::Right, "agganciata a destra");
        let texts = frame(&ctx, &mut app, vec![]);
        let texts = frame(&ctx, &mut app, vec![]).into_iter().chain(texts).collect::<Vec<_>>();
        // La finestra ora è un pannello sul lato destro
        let docked_title = texts.iter().find(|(t, _)| *t == format!("{}  Cabina", icon::CUBE_TRANSPARENT)).expect("titolo agganciato").1;
        assert!(docked_title.left() > 800.0, "a destra: {docked_title:?}");

        // «Sgancia»
        let undock = texts.iter().find(|(t, _)| t == icon::ARROW_SQUARE_OUT).expect("pulsante Sgancia").1.center();
        for events in [vec![egui::Event::PointerMoved(undock)], vec![button(undock, true)], vec![button(undock, false)], vec![]] {
            frame(&ctx, &mut app, events);
        }
        assert_eq!(app.settings.dock_cabin.side, Side::Floating);
    }

    #[test]
    fn window_against_the_edge_docks() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 42.0), egui::pos2(1280.0, 774.0));
        let at = |x: f32, y: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(360.0, 500.0));
        assert_eq!(touched_side(area, at(920.0, 200.0)), Some(Side::Right));
        assert_eq!(touched_side(area, at(0.0, 200.0)), Some(Side::Left));
        assert_eq!(touched_side(area, at(500.0, 42.0)), Some(Side::Top));
        assert_eq!(touched_side(area, at(500.0, 200.0)), None);
    }

    #[test]
    fn state_text_roundtrip() {
        let s = DockState { side: Side::Bottom, width: 380.5, height: 240.0 };
        assert_eq!(DockState::from_text(&s.to_text()), Some(s));
        assert_eq!(DockState::from_text("boh"), None);
    }
}
