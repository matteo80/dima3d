//! Finestra «Robot»: robot generici posizionati dal centro cabina con X Y Z A B C (vedi
//! `crate::robot`), ognuno con il suo pannello facoltativo a quota 0.

use super::theme::{icon, icon_text};
use crate::app::GestoreApp;
use crate::robot::{RobotCell, RobotParams};

/// Campo con etichetta colorata come nella finestra Posizione.
fn value(ui: &mut egui::Ui, label: &str, color: egui::Color32, v: &mut f32, suffix: &str, speed: f64) {
    ui.label(egui::RichText::new(label).color(color).strong());
    ui.add(egui::DragValue::new(v).speed(speed).suffix(suffix).max_decimals(3));
}

/// Riga incollata dalla flash: `CONST BASE NOME = X Y Z A B C`, le righe degli script o sei numeri.
fn parse_pasted(text: &str) -> Option<(Option<String>, crate::scene_graph::placement::Pose)> {
    if let Some((name, pose)) = crate::scene_graph::placement::parse_const_base(text) {
        return Some(((!name.is_empty()).then_some(name), pose));
    }
    crate::scene_graph::placement::parse_offset(text).map(|p| (None, p))
}

/// Un robot nella lista. Ritorna `true` se va tolto.
fn robot_ui(ui: &mut egui::Ui, index: usize, robot: &mut RobotParams) -> bool {
    let mut remove = false;
    let id = ui.make_persistent_id(("robot", index));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut robot.name).desired_width(140.0)).on_hover_text("Nome del robot");
            if ui.small_button(icon::TRASH).on_hover_text("Togli questo robot").clicked() {
                remove = true;
            }
        })
        .body(|ui| {
            let (red, green, blue) =
                (egui::Color32::from_rgb(230, 80, 80), egui::Color32::from_rgb(80, 190, 80), egui::Color32::from_rgb(90, 140, 255));
            egui::Grid::new(("robot_coordinate", index)).num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
                let [x, y, z] = &mut robot.xyz;
                value(ui, "X", red, x, " mm", 10.0);
                value(ui, "Y", green, y, " mm", 10.0);
                value(ui, "Z", blue, z, " mm", 10.0);
                ui.end_row();
                let [a, b, c] = &mut robot.abc;
                value(ui, "A", red, a, " °", 0.5);
                value(ui, "B", green, b, " °", 0.5);
                value(ui, "C", blue, c, " °", 0.5);
                ui.end_row();
            });
            // Incolla: la riga letta dalla flash
            let key = ui.make_persistent_id(("robot_incolla", index));
            let mut pasted: String = ui.data(|d| d.get_temp(key)).unwrap_or_default();
            let response = ui.add(
                egui::TextEdit::singleline(&mut pasted)
                    .hint_text(format!("{}  Incolla CONST BASE … = X Y Z A B C", icon::CLIPBOARD_TEXT))
                    .desired_width(f32::INFINITY),
            );
            if response.changed() {
                if let Some((name, pose)) = parse_pasted(&pasted) {
                    robot.xyz = pose.xyz.to_array();
                    robot.abc = pose.abc.to_array();
                    if let Some(name) = name {
                        robot.name = name;
                    }
                    pasted.clear();
                }
            }
            ui.data_mut(|d| d.insert_temp(key, pasted));
            ui.horizontal(|ui| {
                ui.checkbox(&mut robot.plate, "Pannello sotto, a quota 0")
                    .on_hover_text("Lastra sotto il robot a Z 0 del centro, girata come il robot: per controllare l'allineamento con la simulazione");
            });
            if robot.plate {
                ui.horizontal(|ui| {
                    ui.label("Misure");
                    ui.add(egui::DragValue::new(&mut robot.plate_x).speed(10.0).range(1.0..=1.0e5).suffix(" mm")).on_hover_text("Lungo X del robot");
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut robot.plate_y).speed(10.0).range(1.0..=1.0e5).suffix(" mm")).on_hover_text("Lungo Y del robot");
                });
            }
        });
    remove
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp, pass: super::dock::Pass) {
    if !app.ui_state.robot_open {
        return;
    }
    if (pass == super::dock::Pass::Docked) != (app.settings.dock_robot.side != super::dock::Side::Floating) {
        return;
    }
    let mut dock = app.settings.dock_robot;
    let mut cell: RobotCell = app.settings.robots.clone();
    let mut open = true;
    let mut apply = false;
    let busy = app.ui_state.is_loading || app.project.busy();
    let applied = app
        .scene_graph
        .as_ref()
        .and_then(|s| s.open_files().filter(|f| f.robots.is_some()).last().and_then(|f| f.robots.clone()));
    let window = super::dock::Dockable {
        id: "finestra_robot",
        title: format!("{}  Robot", icon::ROBOT),
        state: &mut dock,
        open: &mut open,
        default_width: 380.0,
        min_width: 320.0,
        default_height: 520.0,
        default_pos: egui::pos2(120.0, 140.0),
    };
    super::dock::show(ctx, pass, window, |ui| {
        let [cx, cy, cz] = cell.center;
        ui.label(
            egui::RichText::new(format!(
                "Coordinate dal centro cabina ({cx:.1} {cy:.1} {cz:.1}), come nei CONST BASE: A attorno a X, B a Y, C a Z. \
                 Il centro si cambia nella finestra Cabina."
            ))
            .weak(),
        );
        ui.horizontal(|ui| {
            if ui.button(icon_text(ui, icon::PLUS, "Aggiungi robot")).clicked() {
                let name = cell.next_name();
                cell.robots.push(RobotParams { name, ..Default::default() });
            }
            if !cell.robots.is_empty() && ui.button("Togli tutti").clicked() {
                cell.robots.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(520.0).auto_shrink([false, true]).show(ui, |ui| {
            if cell.robots.is_empty() {
                ui.label(egui::RichText::new("Nessun robot: «Aggiungi robot» per inserirne uno.").weak());
            }
            let mut remove = None;
            for (i, robot) in cell.robots.iter_mut().enumerate() {
                if robot_ui(ui, i, robot) {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                cell.robots.remove(i);
            }
        });
        let problems = cell.problems();
        for problem in &problems {
            ui.colored_label(ui.visuals().warn_fg_color, format!("{} {problem}", icon::WARNING));
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(problems.is_empty() && !busy, egui::Button::new(icon_text(ui, icon::HAMMER, "Applica alla scena")))
                .on_hover_text("Mette nella scena i robot (e il simbolo del centro) come file «Centro e robot», al posto di quelli di prima")
                .clicked()
            {
                apply = true;
            }
            if applied.as_ref() != Some(&cell) && !(applied.is_none() && cell.is_empty()) {
                ui.colored_label(ui.visuals().warn_fg_color, "Modifiche non applicate");
            }
        });
    });
    if cell != app.settings.robots || dock != app.settings.dock_robot {
        let mut settings = app.settings.clone();
        settings.robots = cell.clone();
        settings.dock_robot = dock;
        app.apply_settings(settings);
    }
    if apply {
        app.build_robot_cell(cell);
    }
    if !open {
        app.ui_state.robot_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_lines_from_the_controller() {
        let (name, pose) = parse_pasted("CONST BASE R11 = 8000.5 -1800 0 0 0 -90").unwrap();
        assert_eq!(name.as_deref(), Some("R11"));
        assert_eq!(pose.xyz.to_array(), [8000.5, -1800.0, 0.0]);
        assert_eq!(pose.abc.to_array(), [0.0, 0.0, -90.0]);
        let (name, pose) = parse_pasted("1 2 3 4 5 6").unwrap();
        assert!(name.is_none());
        assert_eq!(pose.abc.z, 6.0);
        assert!(parse_pasted("niente").is_none());
    }

    /// «Aggiungi robot» due volte e «Applica alla scena»: due robot nel file «Centro e robot».
    #[test]
    fn window_adds_robots_and_applies() {
        let mut app = GestoreApp::new_headless();
        app.ui_state.robot_open = true;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let run = |app: &mut GestoreApp, events: Vec<egui::Event>| {
            let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                draw(ctx, app, crate::ui::dock::Pass::Floating)
            });
            let mut texts = Vec::new();
            fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match s {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            texts
        };
        let click = |app: &mut GestoreApp, label: &str| {
            run(app, vec![]);
            let texts = run(app, vec![]);
            let at = texts.iter().find(|(t, _)| t.ends_with(label)).unwrap_or_else(|| panic!("manca {label}: {texts:?}")).1.center();
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            run(app, vec![egui::Event::PointerMoved(at)]);
            run(app, vec![button(true)]);
            run(app, vec![button(false)]);
        };
        click(&mut app, "Aggiungi robot");
        click(&mut app, "Aggiungi robot");
        assert_eq!(app.settings.robots.robots.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Robot 1", "Robot 2"]);
        click(&mut app, "Applica alla scena");
        app.poll_loading(&ctx);
        assert!(app.robot_node().is_some(), "{}", app.ui_state.status_message);
    }
}
