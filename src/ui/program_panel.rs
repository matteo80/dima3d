//! Finestra «Programma»: le righe di un programma del robot (`.TIS`/`.tip` con i punti del
//! `.tid`), con la traiettoria disegnata nella vista. Le righe si scelgono (Ctrl e Shift come
//! nell'albero), si copiano, tagliano, incollano, invertono, e si trascinano sopra o sotto per
//! cambiare l'ordine dei movimenti.

use super::theme::{icon, icon_text};
use crate::app::GestoreApp;
use crate::robot_program::{self as rp, Line, Motion, Program};

#[derive(Debug, Default)]
pub struct ProgramPanel {
    pub open: bool,
    pub program: Option<Program>,
    /// Righe scelte, in ordine crescente
    pub selected: Vec<usize>,
    /// Riga da cui parte la scelta con Shift (e dove si incolla dopo aver tagliato)
    anchor: Option<usize>,
    /// Modifiche non salvate
    pub modified: bool,
    undo: Vec<Vec<Line>>,
    redo: Vec<Vec<Line>>,
    /// Righe copiate o tagliate
    clipboard: Vec<Line>,
    /// Trascinamento in corso: riga prima della quale andranno le righe scelte
    drag: Option<Option<usize>>,
    /// Il puntatore è sulla finestra: Ctrl+C/X/V/Z, Canc e Ctrl+A valgono per le righe
    pub has_pointer: bool,
    /// Traiettoria nascosta nella vista
    pub hide_path: bool,
    /// Riga sotto il mouse: il suo punto si evidenzia nella vista
    pub hovered: Option<usize>,
}

impl ProgramPanel {
    fn lines(&self) -> &[Line] {
        self.program.as_ref().map_or(&[], |p| p.lines.as_slice())
    }

    /// Cambia le righe (con l'annulla). `f` ritorna le righe da scegliere dopo.
    fn edit(&mut self, f: impl FnOnce(&mut Vec<Line>, &[usize]) -> Vec<usize>) {
        let Some(program) = &mut self.program else { return };
        let before = program.lines.clone();
        let selected = f(&mut program.lines, &self.selected);
        if program.lines == before {
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        self.modified = true;
        self.anchor = selected.first().copied().or(self.anchor);
        self.selected = selected;
    }

    pub fn copy(&mut self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        self.clipboard = rp::take(self.lines(), &self.selected);
        let ending = self.program.as_ref().map_or("\r\n", |p| p.line_ending);
        Some(self.clipboard.iter().map(|l| l.0.as_str()).collect::<Vec<_>>().join(ending))
    }

    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        let first = self.selected[0];
        self.edit(|lines, sel| {
            rp::remove(lines, sel);
            Vec::new()
        });
        self.anchor = Some(first);
        Some(text)
    }

    /// Incolla sotto l'ultima riga scelta (senza scelta: dove si è tagliato, o in fondo).
    pub fn paste(&mut self, new: Vec<Line>) {
        if new.is_empty() {
            return;
        }
        let len = self.lines().len();
        let at = self.selected.last().map(|&i| i + 1).or(self.anchor).unwrap_or(len).min(len);
        self.edit(|lines, _| {
            let mut new = new;
            rp::indent_like(lines, at, &mut new);
            rp::insert(lines, at, new)
        });
    }

    pub fn reverse(&mut self) {
        self.edit(|lines, sel| {
            rp::reverse(lines, sel);
            sel.to_vec()
        });
    }

    pub fn delete(&mut self) {
        let first = self.selected.first().copied();
        self.edit(|lines, sel| {
            rp::remove(lines, sel);
            Vec::new()
        });
        self.anchor = first;
    }

    fn move_before(&mut self, before: usize) {
        self.edit(|lines, sel| rp::move_to(lines, sel, before));
    }

    pub fn select_all(&mut self) {
        self.selected = (0..self.lines().len()).collect();
    }

    pub fn undo(&mut self) {
        if let (Some(p), Some(before)) = (&mut self.program, self.undo.pop()) {
            self.redo.push(std::mem::replace(&mut p.lines, before));
            self.selected.clear();
            self.modified = true;
        }
    }

    pub fn redo(&mut self) {
        if let (Some(p), Some(after)) = (&mut self.program, self.redo.pop()) {
            self.undo.push(std::mem::replace(&mut p.lines, after));
            self.selected.clear();
            self.modified = true;
        }
    }

    /// Clic su una riga: da sola, con Ctrl si aggiunge o toglie, con Shift tutte dalla precedente.
    fn click(&mut self, i: usize, m: egui::Modifiers) {
        match (m.shift, m.command, self.anchor) {
            (true, ctrl, Some(a)) => {
                let range = a.min(i)..=a.max(i);
                if ctrl {
                    self.selected.extend(range);
                    self.selected.sort_unstable();
                    self.selected.dedup();
                } else {
                    self.selected = range.collect();
                }
            }
            (_, true, _) => {
                match self.selected.binary_search(&i) {
                    Ok(k) => {
                        self.selected.remove(k);
                    }
                    Err(k) => self.selected.insert(k, i),
                }
                self.anchor = Some(i);
            }
            _ => {
                self.selected = vec![i];
                self.anchor = Some(i);
            }
        }
    }

    fn load(&mut self, program: Program) {
        *self = ProgramPanel { open: true, program: Some(program), hide_path: self.hide_path, clipboard: std::mem::take(&mut self.clipboard), ..Default::default() };
    }

    /// Intestazione come nel controllore: robot : oggetto : programma.
    pub fn title(&self) -> String {
        let Some(p) = &self.program else { return String::new() };
        let stem = p.program_path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        let robot = p.argument_of("SELECT").or_else(|| p.argument_of("RSELECT"));
        [robot, p.argument_of("SETOBJECT"), Some(stem)].into_iter().flatten().collect::<Vec<_>>().join(" : ")
    }
}

/// Scorciatoie quando il puntatore è sulla finestra. Ritorna `true` se ne ha usata una.
pub fn shortcuts(ctx: &egui::Context, app: &mut GestoreApp) -> bool {
    let panel = &mut app.ui_state.program;
    if !panel.open || !panel.has_pointer || panel.program.is_none() {
        return false;
    }
    let (copy, cut, paste, undo, redo, delete, all, save) = ctx.input(|i| {
        let cmd = i.modifiers.command;
        let z = cmd && i.key_pressed(egui::Key::Z);
        let pasted = i.events.iter().find_map(|e| match e {
            egui::Event::Paste(t) => Some(Some(t.clone())),
            _ => None,
        });
        (
            i.events.iter().any(|e| matches!(e, egui::Event::Copy)) || (cmd && i.key_pressed(egui::Key::C)),
            i.events.iter().any(|e| matches!(e, egui::Event::Cut)) || (cmd && i.key_pressed(egui::Key::X)),
            pasted.or_else(|| (cmd && i.key_pressed(egui::Key::V)).then_some(None)),
            z && !i.modifiers.shift,
            (z && i.modifiers.shift) || (cmd && i.key_pressed(egui::Key::Y)),
            i.key_pressed(egui::Key::Delete),
            cmd && i.key_pressed(egui::Key::A),
            cmd && i.key_pressed(egui::Key::S),
        )
    });
    if copy || cut {
        let text = if cut { panel.cut() } else { panel.copy() };
        app.ui_state.clipboard_text = text;
    } else if let Some(text) = paste {
        // Il testo degli appunti (anche da un editor); senza testo le righe copiate qui
        let new = match text {
            Some(t) if !t.trim().is_empty() => t.lines().map(|l| Line(l.trim_end_matches('\r').to_string())).collect(),
            _ => panel.clipboard.clone(),
        };
        panel.paste(new);
    } else if undo {
        panel.undo();
    } else if redo {
        panel.redo();
    } else if delete {
        panel.delete();
    } else if all {
        panel.select_all();
    } else if save {
        save_program(app, false);
    } else {
        return false;
    }
    true
}

fn open_dialog(app: &mut GestoreApp) {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Apri un programma del robot")
        .add_filter("Programmi del robot (TIS, TIP, TID)", &["tis", "TIS", "tip", "TIP", "tid", "TID"])
        .add_filter("Tutti i file", &["*"]);
    if let Some(dir) = app.settings.last_program.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(path) = dialog.pick_file() {
        open_program(app, &path);
    }
}

pub fn open_program(app: &mut GestoreApp, path: &std::path::Path) {
    match Program::load(path) {
        Ok(program) => {
            let moves = program.path();
            let drawn = moves.iter().filter(|m| m.position.is_some()).count();
            app.ui_state.status_message = format!(
                "Programma {}: {} movimenti, {drawn} con le coordinate dell'oggetto",
                program.program_path.display(),
                moves.len()
            );
            let mut settings = app.settings.clone();
            settings.last_program = Some(program.program_path.clone());
            app.apply_settings(settings);
            app.ui_state.program.load(program);
        }
        Err(e) => app.ui_state.status_message = format!("Programma non aperto: {e}"),
    }
}

fn save_program(app: &mut GestoreApp, ask: bool) {
    let panel = &mut app.ui_state.program;
    let Some(program) = &mut panel.program else { return };
    let target = if ask {
        let mut dialog = rfd::FileDialog::new().set_title("Salva il programma con nome");
        if let Some(ext) = program.program_path.extension().and_then(|e| e.to_str()) {
            dialog = dialog.add_filter(format!("Programma (.{ext})"), &[ext]);
        }
        if let Some(dir) = program.program_path.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = program.program_path.file_name().and_then(|n| n.to_str()) {
            dialog = dialog.set_file_name(name);
        }
        match dialog.save_file() {
            Some(p) => p,
            None => return,
        }
    } else {
        program.program_path.clone()
    };
    app.ui_state.status_message = match program.save_as(&target) {
        Ok(copied) => {
            if target != program.program_path {
                program.program_path = target.clone();
                if let Some(c) = &copied {
                    program.data_path = Some(c.clone());
                }
            }
            panel.modified = false;
            match copied {
                Some(c) => format!("Programma salvato in {}, punti copiati in {}", target.display(), c.display()),
                None => format!("Programma salvato in {}", target.display()),
            }
        }
        Err(e) => format!("Programma non salvato: {e}"),
    };
}

/// Cosa manca a un movimento per essere disegnato.
fn missing(program: &Program, name: &str) -> Option<&'static str> {
    match program.point(name) {
        None => Some("?"),
        Some(p) => match p.frame {
            rp::Frame::Object => None,
            rp::Frame::World => Some("World"),
            rp::Frame::Axis => Some("Axis"),
        },
    }
}

/// Le righe, con scelta e trascinamento.
fn lines_ui(ui: &mut egui::Ui, panel: &mut ProgramPanel) {
    let Some(program) = &panel.program else { return };
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let row_h = ui.fonts(|f| f.row_height(&font)) + 4.0;
    let n = program.lines.len();
    let visuals = ui.visuals().clone();
    let accent = super::theme::accent_color(ui);
    let warn = visuals.warn_fg_color;
    let id = ui.make_persistent_id("righe_programma");
    let mut clicked = None;
    let mut drag_start = None;
    let mut hovered = None;
    let dragging = panel.drag.is_some();
    ui.spacing_mut().item_spacing.y = 0.0;
    let frame = egui::Frame::new().fill(visuals.extreme_bg_color).inner_margin(2.0);
    frame.show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, n, |ui, range| {
            let top0 = ui.max_rect().top() - range.start as f32 * row_h;
            let width = ui.available_width();
            for i in range {
                let rect = egui::Rect::from_min_size(egui::pos2(ui.max_rect().left(), top0 + i as f32 * row_h), egui::vec2(width, row_h));
                let resp = ui.interact(rect, id.with(i), egui::Sense::click_and_drag());
                let selected = panel.selected.binary_search(&i).is_ok();
                if selected {
                    ui.painter().rect_filled(rect, 2.0, visuals.selection.bg_fill);
                } else if resp.hovered() && !dragging {
                    ui.painter().rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
                }
                let line = &program.lines[i];
                let motion = line.motion();
                let color = if selected {
                    visuals.selection.stroke.color
                } else if line.is_comment() {
                    visuals.weak_text_color()
                } else if motion.is_some() {
                    accent
                } else {
                    visuals.text_color()
                };
                let text_pos = rect.left_center() + egui::vec2(4.0, 0.0);
                ui.painter().text(text_pos, egui::Align2::LEFT_CENTER, &line.0, font.clone(), color);
                if let Some(why) = motion.and_then(|(_, name)| missing(program, name)) {
                    ui.painter().text(rect.right_center() - egui::vec2(4.0, 0.0), egui::Align2::RIGHT_CENTER, why, egui::FontId::proportional(11.0), warn);
                }
                ui.allocate_rect(rect, egui::Sense::hover());
                if resp.clicked() {
                    clicked = Some((i, ui.input(|inp| inp.modifiers)));
                }
                if resp.drag_started() {
                    drag_start = Some(i);
                }
                if resp.hovered() {
                    hovered = Some(i);
                }
            }
            // Riga davanti alla quale vanno le righe trascinate
            if let (Some(_), Some(pointer)) = (panel.drag, ui.ctx().pointer_latest_pos()) {
                let target = (((pointer.y - top0) / row_h).round().max(0.0) as usize).min(n);
                panel.drag = Some(Some(target));
                let y = top0 + target as f32 * row_h;
                let x = ui.max_rect().x_range();
                ui.painter().line_segment([egui::pos2(x.min, y), egui::pos2(x.max, y)], egui::Stroke::new(2.0_f32, accent));
                // Vicino ai bordi la lista scorre da sola
                let clip = ui.clip_rect();
                if pointer.y < clip.top() + row_h {
                    ui.scroll_with_delta(egui::vec2(0.0, row_h * 0.5));
                } else if pointer.y > clip.bottom() - row_h {
                    ui.scroll_with_delta(egui::vec2(0.0, -row_h * 0.5));
                }
                ui.ctx().request_repaint();
            }
        });
    });
    panel.hovered = hovered;
    if let Some((i, m)) = clicked {
        panel.click(i, m);
    }
    if let Some(i) = drag_start {
        if panel.selected.binary_search(&i).is_err() {
            panel.selected = vec![i];
            panel.anchor = Some(i);
        }
        panel.drag = Some(None);
    }
    if panel.drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
        if let Some(Some(target)) = panel.drag.take() {
            panel.move_before(target);
        }
    }
}

pub fn draw(ctx: &egui::Context, app: &mut GestoreApp, pass: super::dock::Pass) {
    if !app.ui_state.program.open {
        return;
    }
    if (pass == super::dock::Pass::Docked) != (app.settings.dock_program.side != super::dock::Side::Floating) {
        return;
    }
    let mut dock = app.settings.dock_program;
    let mut open = true;
    let (mut choose, mut save, mut save_as) = (false, false, false);
    let mut clipboard_text = None;
    let panel = &mut app.ui_state.program;
    let window = super::dock::Dockable {
        id: "finestra_programma",
        title: format!("{}  Programma", icon::PATH),
        state: &mut dock,
        open: &mut open,
        default_width: 300.0,
        min_width: 240.0,
        default_height: 640.0,
        default_pos: egui::pos2(140.0, 120.0),
        scroll: false,
    };
    let pointer = super::dock::show(ctx, pass, window, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button(icon_text(ui, icon::FOLDER_OPEN, "Apri…")).on_hover_text("Programma .TIS o .tip (o il suo file dei punti .tid): i punti si leggono dal .tid con lo stesso nome e dai .tid globali della cartella (che iniziano con _)").clicked() {
                choose = true;
            }
            let has = panel.program.is_some();
            if ui.add_enabled(has && panel.modified, egui::Button::new(icon_text(ui, icon::FLOPPY_DISK, ""))).on_hover_text("Salva sopra il programma  [Ctrl+S con il mouse sulla finestra]").clicked() {
                save = true;
            }
            if ui.add_enabled(has, egui::Button::new("Salva con nome…")).on_hover_text("Salva in un altro file: il file dei punti si copia accanto con lo stesso nome").clicked() {
                save_as = true;
            }
        });
        if panel.program.is_none() {
            ui.label(egui::RichText::new("Apri un programma del robot per vedere la traiettoria. I punti in coordinate dell'oggetto si disegnano nella scena così come sono: metti la matematica con il riferimento oggetto nello zero.").weak());
            return ui.ui_contains_pointer();
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let some = !panel.selected.is_empty();
            if ui.add_enabled(some, egui::Button::new(icon_text(ui, icon::COPY, ""))).on_hover_text("Copia le righe scelte  [Ctrl+C]").clicked() {
                clipboard_text = panel.copy();
            }
            if ui.add_enabled(some, egui::Button::new(icon_text(ui, icon::SCISSORS, ""))).on_hover_text("Taglia le righe scelte  [Ctrl+X]").clicked() {
                clipboard_text = panel.cut();
            }
            if ui.add_enabled(!panel.clipboard.is_empty(), egui::Button::new(icon_text(ui, icon::CLIPBOARD, ""))).on_hover_text("Incolla sotto la riga scelta  [Ctrl+V, anche testo copiato da un editor]").clicked() {
                let new = panel.clipboard.clone();
                panel.paste(new);
            }
            if ui.add_enabled(panel.selected.len() > 1, egui::Button::new(icon_text(ui, icon::ARROWS_DOWN_UP, "Inverti"))).on_hover_text("Inverte l'ordine delle righe scelte (il primo punto diventa l'ultimo)").clicked() {
                panel.reverse();
            }
            if ui.add_enabled(some, egui::Button::new(icon_text(ui, icon::TRASH, ""))).on_hover_text("Elimina le righe scelte  [Canc]").clicked() {
                panel.delete();
            }
            ui.separator();
            if ui.add_enabled(!panel.undo.is_empty(), egui::Button::new(icon_text(ui, icon::ARROW_U_UP_LEFT, ""))).on_hover_text("Annulla  [Ctrl+Z]").clicked() {
                panel.undo();
            }
            if ui.add_enabled(!panel.redo.is_empty(), egui::Button::new(icon_text(ui, icon::ARROW_U_UP_RIGHT, ""))).on_hover_text("Ripeti  [Ctrl+Y]").clicked() {
                panel.redo();
            }
        });
        let mut show = !panel.hide_path;
        ui.checkbox(&mut show, "Traiettoria nella vista").on_hover_text("Linea dei movimenti LIN (piena) e PTP (tratteggiata), con i punti; le righe scelte mostrano il nome del punto");
        panel.hide_path = !show;
        if let Some(p) = &panel.program {
            let moves = p.path();
            let drawn = moves.iter().filter(|m| m.position.is_some()).count();
            let title = panel.title();
            ui.label(egui::RichText::new(format!("{}{}", title, if panel.modified { "  •" } else { "" })).strong())
                .on_hover_text(p.program_path.display().to_string());
            let mut info = format!("{} movimenti, {drawn} nella vista", moves.len());
            if drawn < moves.len() {
                info.push_str(" (gli altri sono in World, Axis o senza punto)");
            }
            ui.label(egui::RichText::new(info).weak().small());
        }
        lines_ui(ui, panel);
        ui.ui_contains_pointer()
    });
    let panel = &mut app.ui_state.program;
    panel.has_pointer = pointer.unwrap_or(false);
    if clipboard_text.is_some() {
        app.ui_state.clipboard_text = clipboard_text;
    }
    if dock != app.settings.dock_program {
        let mut settings = app.settings.clone();
        settings.dock_program = dock;
        app.apply_settings(settings);
    }
    if choose {
        open_dialog(app);
    }
    if save || save_as {
        save_program(app, save_as);
    }
    if !open {
        app.ui_state.program.open = false;
        app.ui_state.program.has_pointer = false;
    }
}

/// Traiettoria nella vista 3D: linee tra i punti in coordinate dell'oggetto, i punti e i nomi
/// di quelli scelti (e di quello sotto il mouse, nella lista o nella vista).
pub fn draw_overlay(ui: &egui::Ui, rect: egui::Rect, panel: &ProgramPanel, to_screen: impl Fn(glam::Vec3) -> Option<egui::Pos2>) {
    let Some(program) = &panel.program else { return };
    if !panel.open || panel.hide_path {
        return;
    }
    let painter = ui.painter_at(rect);
    let lin = egui::Color32::from_rgb(255, 170, 40);
    let ptp = egui::Color32::from_rgb(200, 200, 210);
    let event = egui::Color32::from_rgb(60, 210, 230);
    let highlight = egui::Color32::from_rgb(255, 240, 120);
    let shadow = egui::Color32::from_black_alpha(160);
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p));
    let selected = |line: usize| panel.selected.binary_search(&line).is_ok() || panel.hovered == Some(line);
    let label = |at: egui::Pos2, text: &str| {
        let font = egui::FontId::proportional(12.0);
        let pos = at + egui::vec2(7.0, -7.0);
        painter.text(pos + egui::vec2(1.0, 1.0), egui::Align2::LEFT_BOTTOM, text, font.clone(), shadow);
        painter.text(pos, egui::Align2::LEFT_BOTTOM, text, font, highlight);
    };

    let moves = program.path();
    let screen: Vec<Option<egui::Pos2>> = moves.iter().map(|m| m.position.and_then(&to_screen)).collect();
    for (k, pair) in moves.windows(2).enumerate() {
        let (Some(a), Some(b)) = (screen[k], screen[k + 1]) else { continue };
        if pair[1].motion == Motion::Lin {
            painter.line_segment([a, b], egui::Stroke::new(2.0_f32, lin));
        } else {
            painter.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.5_f32, ptp), 8.0, 5.0));
        }
        // Freccia a metà, per il verso
        let d = b - a;
        if d.length() > 40.0 {
            let dir = d.normalized();
            let mid = a + d * 0.5;
            let side = egui::vec2(-dir.y, dir.x) * 4.0;
            let color = if pair[1].motion == Motion::Lin { lin } else { ptp };
            painter.add(egui::Shape::convex_polygon(vec![mid + dir * 6.0, mid - dir * 4.0 + side, mid - dir * 4.0 - side], color, egui::Stroke::NONE));
        }
    }
    // Punto più vicino al mouse
    let near = pointer.and_then(|p| {
        screen
            .iter()
            .enumerate()
            .filter_map(|(k, s)| s.map(|s| (k, s.distance(p))))
            .filter(|(_, d)| *d < 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    });
    for (k, (m, s)) in moves.iter().zip(&screen).enumerate() {
        let Some(s) = *s else { continue };
        let on = selected(m.line) || near == Some(k);
        let color = if m.motion == Motion::Lin { lin } else { ptp };
        painter.circle_filled(s, if on { 5.0 } else { 2.5 }, if on { highlight } else { color });
        if on {
            label(s, &m.name);
        }
    }
    // Punti dei comandi sulla traiettoria (es. pistola accesa o spenta)
    for e in program.events() {
        let Some(s) = to_screen(e.position) else { continue };
        let on = selected(e.line);
        let r = if on { 5.0 } else { 3.5 };
        painter.rect_filled(egui::Rect::from_center_size(s, egui::vec2(r, r) * 2.0), 0.0, if on { highlight } else { event });
        if on || pointer.is_some_and(|p| p.distance(s) < 8.0) {
            let command = program.lines[e.line].call().map(|(c, _)| c.to_string()).unwrap_or_default();
            label(s, &format!("{command} {}", e.name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(names: &[&str]) -> ProgramPanel {
        let text = names.iter().map(|n| format!("  {n}\r\n")).collect::<String>();
        let program = Program::from_text("prova.tis".into(), &text);
        ProgramPanel { open: true, program: Some(program), ..Default::default() }
    }

    fn names(p: &ProgramPanel) -> Vec<&str> {
        p.lines().iter().map(Line::command).collect()
    }

    #[test]
    fn click_with_ctrl_and_shift() {
        let mut p = panel(&["a", "b", "c", "d", "e"]);
        p.click(1, egui::Modifiers::NONE);
        p.click(3, egui::Modifiers::SHIFT);
        assert_eq!(p.selected, [1, 2, 3]);
        p.click(2, egui::Modifiers::COMMAND);
        assert_eq!(p.selected, [1, 3]);
        p.click(0, egui::Modifiers::NONE);
        assert_eq!(p.selected, [0]);
    }

    #[test]
    fn cut_paste_reverse_and_undo() {
        let mut p = panel(&["a", "b", "c", "d"]);
        p.selected = vec![0, 1];
        assert_eq!(p.cut().as_deref(), Some("  a\r\n  b"));
        assert_eq!(names(&p), ["c", "d"]);
        p.selected = vec![1];
        let new = p.clipboard.clone();
        p.paste(new);
        assert_eq!(names(&p), ["c", "d", "a", "b"]);
        assert_eq!(p.selected, [2, 3]);
        p.reverse();
        assert_eq!(names(&p), ["c", "d", "b", "a"]);
        p.move_before(0);
        assert_eq!(names(&p), ["b", "a", "c", "d"]);
        assert!(p.modified);
        p.undo();
        p.undo();
        assert_eq!(names(&p), ["c", "d", "a", "b"]);
        p.redo();
        assert_eq!(names(&p), ["c", "d", "b", "a"]);
        // Dopo aver tagliato senza scelta si incolla dove si è tagliato
        p.selected = vec![1];
        p.cut();
        let new = p.clipboard.clone();
        p.paste(new);
        assert_eq!(names(&p), ["c", "d", "b", "a"]);
    }

    /// Finestra vera: clic su «d», trascinata sopra «b»; Ctrl+Z con il mouse sulla finestra.
    #[test]
    fn drag_a_line_up_in_the_window() {
        let mut app = GestoreApp::new_headless();
        app.settings.dock_program = Default::default();
        app.ui_state.program = panel(&["a", "b", "c", "d"]);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let run_with = |app: &mut GestoreApp, events: Vec<egui::Event>, modifiers| {
            let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, modifiers, ..Default::default() }, |ctx| {
                shortcuts(ctx, app);
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
        let run = |app: &mut GestoreApp, events| run_with(app, events, egui::Modifiers::NONE);
        run(&mut app, vec![]);
        let texts = run(&mut app, vec![]);
        let at = |t: &str| texts.iter().find(|(s, _)| s == t).unwrap_or_else(|| panic!("manca {t}: {texts:?}")).1.center();
        let (d, b) = (at("  d"), at("  b"));
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(&mut app, vec![egui::Event::PointerMoved(d)]);
        run(&mut app, vec![button(d, true)]);
        // Sopra la metà di «b»: le righe vanno prima di «b»
        let target = b - egui::vec2(0.0, 4.0);
        for k in 1..=6 {
            run(&mut app, vec![egui::Event::PointerMoved(d + (target - d) * k as f32 / 6.0)]);
        }
        run(&mut app, vec![button(target, false)]);
        run(&mut app, vec![]);
        assert_eq!(names(&app.ui_state.program), ["a", "d", "b", "c"]);
        assert_eq!(app.ui_state.program.selected, [1]);
        assert!(app.ui_state.program.has_pointer);
        let z = egui::Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
        run_with(&mut app, vec![z], egui::Modifiers::COMMAND);
        assert_eq!(names(&app.ui_state.program), ["a", "b", "c", "d"]);
    }

    #[test]
    fn title_like_the_controller() {
        let program = Program::from_text("PEZZO_AP.TIS".into(), "  SELECT( \"R11\")\r\n  SETOBJECT( PEZZO)\r\n");
        let p = ProgramPanel { program: Some(program), ..Default::default() };
        assert_eq!(p.title(), "R11 : PEZZO : PEZZO_AP");
    }
}
