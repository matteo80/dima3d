//! Finestra «Programma»: le righe di un programma del robot (`.TIS`/`.tip` con i punti del
//! `.tid`), con la traiettoria disegnata nella vista. Le righe si scelgono (Ctrl e Shift come
//! nell'albero), si copiano, tagliano, incollano, invertono, e si trascinano sopra o sotto per
//! cambiare l'ordine dei movimenti.
//!
//! Si possono aprire più programmi: le loro traiettorie si vedono tutte insieme, le righe sono
//! quelle del programma attivo (scelto nell'elenco in alto). Gli altri restano con la loro scelta,
//! le modifiche e l'annulla, e tornano attivi con un clic.

use super::theme::{icon, icon_text};
use crate::i18n::tr;
use crate::app::GestoreApp;
use crate::robot_program::{self as rp, Line, Motion, Program};

/// Un programma aperto ma non attivo, con il suo stato.
#[derive(Debug)]
pub struct ProgramDoc {
    pub program: Program,
    selected: Vec<usize>,
    anchor: Option<usize>,
    modified: bool,
    undo: Vec<Vec<Line>>,
    redo: Vec<Vec<Line>>,
    hidden: bool,
}

#[derive(Debug, Default)]
pub struct ProgramPanel {
    pub open: bool,
    /// Programma attivo (le righe nella finestra)
    pub program: Option<Program>,
    /// Gli altri programmi aperti, nell'ordine dell'elenco senza quello attivo
    pub others: Vec<ProgramDoc>,
    /// Posto del programma attivo nell'elenco
    pub active: usize,
    /// Traiettoria del programma attivo nascosta
    pub hidden: bool,
    /// Programma con modifiche non salvate di cui si è chiesta la chiusura (posto nell'elenco)
    confirm_close: Option<usize>,
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
    /// Righe del programma o tabella dei punti
    pub view: ProgramView,
    /// «Modifica…»: riga e testo in scrittura
    editing: Option<(usize, String)>,
}

/// Cosa mostra la finestra, come i due pulsanti del controllore.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ProgramView {
    /// Le righe del programma
    #[default]
    Logic,
    /// I punti usati, con le coordinate
    Points,
}

/// Voce scelta nel menu del tasto destro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    Change,
    Comment,
    Uncomment,
    Reverse,
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

    pub fn comment(&mut self) {
        let mark = self.program.as_ref().map_or("--", Program::comment_mark);
        self.edit(|lines, sel| {
            rp::comment(lines, sel, mark);
            sel.to_vec()
        });
    }

    pub fn uncomment(&mut self) {
        self.edit(|lines, sel| {
            rp::uncomment(lines, sel);
            sel.to_vec()
        });
    }

    /// «Modifica…»: il testo della prima riga scelta si scrive in una finestrella.
    fn start_change(&mut self) {
        if let Some(&i) = self.selected.first() {
            if let Some(l) = self.lines().get(i) {
                self.editing = Some((i, l.command().to_string()));
            }
        }
    }

    /// Conferma di «Modifica…»: la riga prende il testo nuovo, con il suo rientro.
    fn finish_change(&mut self) {
        let Some((i, text)) = self.editing.take() else { return };
        self.edit(|lines, _| {
            if let Some(l) = lines.get_mut(i) {
                let indent: String = l.0.chars().take_while(|c| c.is_whitespace()).collect();
                l.0 = format!("{indent}{}", text.trim());
            }
            vec![i]
        });
    }

    /// Voce del menu del tasto destro. Ritorna il testo per gli appunti (Copia, Taglia).
    fn apply_menu(&mut self, action: MenuAction) -> Option<String> {
        match action {
            MenuAction::Undo => self.undo(),
            MenuAction::Redo => self.redo(),
            MenuAction::Cut => return self.cut(),
            MenuAction::Copy => return self.copy(),
            MenuAction::Paste => {
                let new = self.clipboard.clone();
                self.paste(new);
            }
            MenuAction::Delete => self.delete(),
            MenuAction::Change => self.start_change(),
            MenuAction::Comment => self.comment(),
            MenuAction::Uncomment => self.uncomment(),
            MenuAction::Reverse => self.reverse(),
        }
        None
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

    /// Aggiunge un programma e lo rende attivo. Se è già aperto (stesso file) torna attivo quello.
    fn load(&mut self, program: Program) {
        self.open = true;
        if let Some(j) = self.position_of(&program.program_path) {
            self.switch_to(j);
            return;
        }
        if let Some(d) = self.take_active() {
            let at = self.active.min(self.others.len());
            self.others.insert(at, d);
        }
        self.active = self.others.len();
        self.set_active(ProgramDoc { program, selected: Vec::new(), anchor: None, modified: false, undo: Vec::new(), redo: Vec::new(), hidden: false });
    }

    /// Ingombro dei punti disegnati dei programmi aperti e visibili (vuoto se non ce ne sono).
    pub fn bounds(&self) -> crate::scene_graph::Aabb {
        let mut b = crate::scene_graph::Aabb::empty();
        if !self.open || self.hide_path {
            return b;
        }
        let active = self.program.as_ref().filter(|_| !self.hidden);
        for p in self.others.iter().filter(|d| !d.hidden).map(|d| &d.program).chain(active) {
            for m in p.path() {
                if let Some(v) = m.position {
                    b.extend_point(v);
                }
            }
        }
        b
    }

    /// Quanti programmi sono aperti.
    pub fn count(&self) -> usize {
        self.others.len() + usize::from(self.program.is_some())
    }

    /// Posto nell'elenco del programma aperto da `path`.
    fn position_of(&self, path: &std::path::Path) -> Option<usize> {
        (0..self.count()).find(|&j| self.get(j).is_some_and(|(p, ..)| p.program_path == path))
    }

    /// Programma al posto `j` dell'elenco: (programma, modificato, nascosto).
    fn get(&self, j: usize) -> Option<(&Program, bool, bool)> {
        if j == self.active {
            return self.program.as_ref().map(|p| (p, self.modified, self.hidden));
        }
        let k = if j < self.active { j } else { j - 1 };
        self.others.get(k).map(|d| (&d.program, d.modified, d.hidden))
    }

    fn take_active(&mut self) -> Option<ProgramDoc> {
        let program = self.program.take()?;
        self.drag = None;
        self.hovered = None;
        self.confirm_close = None;
        self.editing = None;
        Some(ProgramDoc {
            program,
            selected: std::mem::take(&mut self.selected),
            anchor: self.anchor.take(),
            modified: std::mem::replace(&mut self.modified, false),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            hidden: std::mem::replace(&mut self.hidden, false),
        })
    }

    fn set_active(&mut self, d: ProgramDoc) {
        self.program = Some(d.program);
        self.selected = d.selected;
        self.anchor = d.anchor;
        self.modified = d.modified;
        self.undo = d.undo;
        self.redo = d.redo;
        self.hidden = d.hidden;
    }

    /// Rende attivo il programma al posto `j` dell'elenco.
    pub fn switch_to(&mut self, j: usize) {
        if j == self.active || j >= self.count() {
            return;
        }
        if let Some(d) = self.take_active() {
            let at = self.active.min(self.others.len());
            self.others.insert(at, d);
        }
        let d = self.others.remove(j);
        self.set_active(d);
        self.active = j;
    }

    /// Chiude il programma al posto `j` dell'elenco (senza salvare).
    pub fn close(&mut self, j: usize) {
        self.confirm_close = None;
        if j >= self.count() {
            return;
        }
        if j == self.active {
            self.take_active();
            if !self.others.is_empty() {
                let k = j.min(self.others.len() - 1);
                let d = self.others.remove(k);
                self.set_active(d);
                self.active = k;
            } else {
                self.active = 0;
            }
        } else if j < self.active {
            self.others.remove(j);
            self.active -= 1;
        } else {
            self.others.remove(j - 1);
        }
    }

    /// Mostra o nasconde nella vista la traiettoria del programma al posto `j`.
    fn toggle_hidden(&mut self, j: usize) {
        if j == self.active {
            self.hidden = !self.hidden;
        } else if let Some(d) = self.others.get_mut(if j < self.active { j } else { j - 1 }) {
            d.hidden = !d.hidden;
        }
    }

}

/// Intestazione di un programma come nel controllore: robot : oggetto : programma.
fn program_title(p: &Program) -> String {
    let stem = p.program_path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    let robot = p.argument_of("SELECT").or_else(|| p.argument_of("RSELECT"));
    [robot, p.argument_of("SETOBJECT"), Some(stem)].into_iter().flatten().collect::<Vec<_>>().join(" : ")
}

/// Scorciatoie quando il puntatore è sulla finestra. Ritorna `true` se ne ha usata una.
pub fn shortcuts(ctx: &egui::Context, app: &mut GestoreApp) -> bool {
    let panel = &mut app.ui_state.program;
    if !panel.open || !panel.has_pointer || panel.program.is_none() || panel.editing.is_some() {
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
        .set_title(tr("Apri un programma del robot"))
        .add_filter(tr("Programmi del robot (TIS, TIP)"), &["tis", "TIS", "tip", "TIP"])
        .add_filter(tr("Tutti i file"), &["*"]);
    if let Some(dir) = app.settings.last_program.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    // Anche più programmi insieme: si aggiungono a quelli già aperti
    for path in dialog.pick_files().unwrap_or_default() {
        open_program(app, &path);
    }
}

pub fn open_program(app: &mut GestoreApp, path: &std::path::Path) {
    match Program::load(path) {
        Ok(program) => {
            let moves = program.path();
            let drawn = moves.iter().filter(|m| m.position.is_some()).count();
            app.ui_state.status_message = trf!(
                "Programma {path}: {n} movimenti, {drawn} con le coordinate dell'oggetto",
                path = program.program_path.display(),
                n = moves.len(),
                drawn = drawn
            );
            let mut settings = app.settings.clone();
            settings.last_program = Some(program.program_path.clone());
            app.apply_settings(settings);
            app.ui_state.program.load(program);
            // Senza matematica la vista inquadra la traiettoria
            if app.scene_graph.is_none() {
                fit_to_programs(app);
            }
        }
        Err(e) => app.ui_state.status_message = trf!("Programma non aperto: {e}", e = e),
    }
}

/// Inquadra i programmi aperti (quando non c'è una matematica caricata).
pub fn fit_to_programs(app: &mut GestoreApp) {
    let b = app.ui_state.program.bounds();
    if let (Some(r), true) = (&mut app.renderer, b.is_valid()) {
        r.camera.fit_to_bbox(&b);
        r.camera.scene_bounds = Some(b);
    }
}

fn save_program(app: &mut GestoreApp, ask: bool) {
    let panel = &mut app.ui_state.program;
    let Some(program) = &mut panel.program else { return };
    let target = if ask {
        let mut dialog = rfd::FileDialog::new().set_title(tr("Salva il programma con nome"));
        if let Some(ext) = program.program_path.extension().and_then(|e| e.to_str()) {
            dialog = dialog.add_filter(trf!("Programma (.{ext})", ext = ext), &[ext]);
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
                Some(c) => trf!("Programma salvato in {path}, punti copiati in {data}", path = target.display(), data = c.display()),
                None => trf!("Programma salvato in {path}", path = target.display()),
            }
        }
        Err(e) => trf!("Programma non salvato: {e}", e = e),
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

/// Menu del tasto destro sulle righe, come nel controllore (fino a «Inverti sequenza»).
fn menu_items(panel: &ProgramPanel) -> [(Option<MenuAction>, &'static str, &'static str, bool); 13] {
    let some = !panel.selected.is_empty();
    let lines = panel.lines();
    let any = |f: &dyn Fn(&Line) -> bool| panel.selected.iter().any(|&i| lines.get(i).is_some_and(f));
    [
        (Some(MenuAction::Undo), icon::ARROW_U_UP_LEFT, tr("Annulla"), !panel.undo.is_empty()),
        (Some(MenuAction::Redo), icon::ARROW_U_UP_RIGHT, tr("Ripeti"), !panel.redo.is_empty()),
        (None, "", "", false),
        (Some(MenuAction::Cut), icon::SCISSORS, tr("Taglia"), some),
        (Some(MenuAction::Copy), icon::COPY, tr("Copia"), some),
        (Some(MenuAction::Paste), icon::CLIPBOARD, tr("Incolla"), !panel.clipboard.is_empty()),
        (Some(MenuAction::Delete), icon::X, tr("Elimina"), some),
        (Some(MenuAction::Change), icon::PENCIL_SIMPLE, tr("Modifica…"), some),
        (None, "", "", false),
        (Some(MenuAction::Comment), icon::CHAT_TEXT, tr("Commenta riga"), any(&|l| !l.is_comment())),
        (Some(MenuAction::Uncomment), icon::CHAT_SLASH, tr("Togli commento"), any(&|l| l.is_comment())),
        (None, "", "", false),
        (Some(MenuAction::Reverse), icon::ARROWS_DOWN_UP, tr("Inverti sequenza"), panel.selected.len() > 1),
    ]
}

fn context_menu(ui: &mut egui::Ui, items: &[(Option<MenuAction>, &str, &str, bool)], chosen: &mut Option<MenuAction>) {
    for &(action, ic, label, enabled) in items {
        let Some(action) = action else {
            ui.separator();
            continue;
        };
        if ui.add_enabled(enabled, egui::Button::new(icon_text(ui, ic, label))).clicked() {
            *chosen = Some(action);
            ui.close_menu();
        }
    }
}

/// Le righe, con scelta e trascinamento. Ritorna la voce scelta nel menu del tasto destro.
fn lines_ui(ui: &mut egui::Ui, panel: &mut ProgramPanel) -> Option<MenuAction> {
    let mut chosen = None;
    let mut right_clicked = None;
    let items = menu_items(panel);
    let Some(program) = &panel.program else { return None };
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
                // Tasto destro su una riga non scelta: si sceglie lei sola, poi il menu
                if resp.secondary_clicked() {
                    right_clicked = Some(i);
                }
                resp.context_menu(|ui| context_menu(ui, &items, &mut chosen));
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
    if let Some(i) = right_clicked {
        if panel.selected.binary_search(&i).is_err() {
            panel.selected = vec![i];
            panel.anchor = Some(i);
        }
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
    chosen
}

/// Tabella dei punti usati dal programma, come la seconda vista del controllore. Clic su un punto:
/// si scelgono le righe che lo usano (e il punto si evidenzia nella vista).
fn points_ui(ui: &mut egui::Ui, panel: &mut ProgramPanel) {
    let Some(program) = &panel.program else { return };
    let used = program.used_points();
    let extra = used.iter().filter_map(|u| u.point).map(|p| p.values.len().saturating_sub(6)).max().unwrap_or(0).min(6);
    let visuals = ui.visuals().clone();
    let warn = visuals.warn_fg_color;
    let mut pick: Option<Vec<usize>> = None;
    let mut hovered = None;
    let frame = egui::Frame::new().fill(visuals.extreme_bg_color).inner_margin(4.0);
    frame.show(ui, |ui| {
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("punti_programma").striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                let head = |ui: &mut egui::Ui, t: &str| {
                    ui.label(egui::RichText::new(t).strong());
                };
                for h in [tr("Nome"), "Coord", "X", "Y", "Z", "A", "B", "C"] {
                    head(ui, h);
                }
                for k in 0..extra {
                    head(ui, &format!("Aux{}", k + 1));
                }
                ui.end_row();
                for u in &used {
                    let chosen = !u.lines.is_empty() && u.lines.iter().all(|i| panel.selected.binary_search(i).is_ok());
                    let r = ui.selectable_label(chosen, egui::RichText::new(&u.name).monospace());
                    if r.clicked() {
                        pick = Some(u.lines.clone());
                    }
                    if r.hovered() {
                        hovered = u.lines.first().copied();
                    }
                    let mono = |ui: &mut egui::Ui, t: String| {
                        ui.label(egui::RichText::new(t).monospace());
                    };
                    match u.point {
                        Some(p) => {
                            mono(ui, format!("{:?}", p.frame));
                            for k in 0..6 + extra {
                                mono(ui, p.values.get(k).map(|v| format!("{v:.3}")).unwrap_or_default());
                            }
                        }
                        None => {
                            ui.label(egui::RichText::new(tr("manca")).color(warn).small());
                        }
                    }
                    ui.end_row();
                }
            });
        });
    });
    panel.hovered = hovered;
    if let Some(lines) = pick {
        panel.anchor = lines.first().copied();
        panel.selected = lines;
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
        title: format!("{}  {}", icon::PATH, tr("Programma")),
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
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui.button(icon_text(ui, icon::FOLDER_OPEN, tr("Apri…"))).on_hover_text(tr("Programma .TIS o .tip: i punti si leggono dal .tid con lo stesso nome e dai .tid globali della cartella (che iniziano con _). Si possono sceglierne più di uno: si aggiungono a quelli aperti")).clicked() {
                choose = true;
            }
            let has = panel.program.is_some();
            if ui.add_enabled(has && panel.modified, egui::Button::new(icon_text(ui, icon::FLOPPY_DISK, ""))).on_hover_text(tr("Salva sopra il programma  [Ctrl+S con il mouse sulla finestra]")).clicked() {
                save = true;
            }
            if ui.add_enabled(has, egui::Button::new(tr("Salva con nome…"))).on_hover_text(tr("Salva in un altro file: il file dei punti si copia accanto con lo stesso nome")).clicked() {
                save_as = true;
            }
        });
        if panel.program.is_none() {
            ui.label(egui::RichText::new(tr("Apri un programma del robot per vedere la traiettoria. I punti in coordinate dell'oggetto si disegnano nella scena così come sono: metti la matematica con il riferimento oggetto nello zero.")).weak());
            return ui.ui_contains_pointer();
        }
        // Programmi aperti: clic per renderlo attivo, occhio per la traiettoria, X per chiudere
        {
            // close: (posto, confermato)
            let (mut pick, mut toggle, mut close) = (None, None, None::<(usize, bool)>);
            for j in 0..panel.count() {
                let Some((p, modified, hidden)) = panel.get(j) else { continue };
                let name = format!("{}{}", program_title(p), if modified { "  •" } else { "" });
                let path = p.program_path.display().to_string();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let eye = if hidden { icon::EYE_SLASH } else { icon::EYE };
                    if ui.small_button(eye).on_hover_text(if hidden { tr("Mostra la traiettoria") } else { tr("Nascondi la traiettoria") }).clicked() {
                        toggle = Some(j);
                    }
                    if ui.small_button(icon::X).on_hover_text(tr("Chiudi il programma")).clicked() {
                        close = Some((j, false));
                    }
                    if ui.selectable_label(j == panel.active, name).on_hover_text(path).clicked() {
                        pick = Some(j);
                    }
                });
                if panel.confirm_close == Some(j) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(tr("Modifiche non salvate:")).color(ui.visuals().warn_fg_color).small());
                        if ui.small_button(tr("Chiudi senza salvare")).clicked() {
                            close = Some((j, true));
                        }
                        if ui.small_button(tr("Annulla")).clicked() {
                            panel.confirm_close = None;
                        }
                    });
                }
            }
            if let Some(j) = toggle {
                panel.toggle_hidden(j);
            }
            if let Some((c, confirmed)) = close {
                if !confirmed && panel.get(c).is_some_and(|(_, modified, _)| modified) {
                    panel.confirm_close = Some(c);
                } else {
                    panel.close(c);
                }
            } else if let Some(j) = pick {
                panel.switch_to(j);
            }
            ui.separator();
            if panel.program.is_none() {
                return ui.ui_contains_pointer();
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let some = !panel.selected.is_empty();
            if ui.add_enabled(some, egui::Button::new(icon_text(ui, icon::COPY, ""))).on_hover_text(tr("Copia le righe scelte  [Ctrl+C]")).clicked() {
                clipboard_text = panel.copy();
            }
            if ui.add_enabled(some, egui::Button::new(icon_text(ui, icon::SCISSORS, ""))).on_hover_text(tr("Taglia le righe scelte  [Ctrl+X]")).clicked() {
                clipboard_text = panel.cut();
            }
            if ui.add_enabled(!panel.clipboard.is_empty(), egui::Button::new(icon_text(ui, icon::CLIPBOARD, ""))).on_hover_text(tr("Incolla sotto la riga scelta  [Ctrl+V, anche testo copiato da un editor]")).clicked() {
                let new = panel.clipboard.clone();
                panel.paste(new);
            }
            if ui.add_enabled(panel.selected.len() > 1, egui::Button::new(icon_text(ui, icon::ARROWS_DOWN_UP, tr("Inverti")))).on_hover_text(tr("Inverte l'ordine delle righe scelte (il primo punto diventa l'ultimo)")).clicked() {
                panel.reverse();
            }
            if ui.add_enabled(some, egui::Button::new(icon_text(ui, icon::TRASH, ""))).on_hover_text(tr("Elimina le righe scelte  [Canc]")).clicked() {
                panel.delete();
            }
            ui.separator();
            if ui.add_enabled(!panel.undo.is_empty(), egui::Button::new(icon_text(ui, icon::ARROW_U_UP_LEFT, ""))).on_hover_text(tr("Annulla  [Ctrl+Z]")).clicked() {
                panel.undo();
            }
            if ui.add_enabled(!panel.redo.is_empty(), egui::Button::new(icon_text(ui, icon::ARROW_U_UP_RIGHT, ""))).on_hover_text(tr("Ripeti  [Ctrl+Y]")).clicked() {
                panel.redo();
            }
        });
        let mut show = !panel.hide_path;
        ui.checkbox(&mut show, tr("Traiettorie nella vista")).on_hover_text(tr("Linea dei movimenti LIN (piena) e PTP (tratteggiata), con i punti; le righe scelte mostrano il nome del punto. I programmi non attivi si vedono più tenui"));
        panel.hide_path = !show;
        if let Some(p) = &panel.program {
            let moves = p.path();
            let drawn = moves.iter().filter(|m| m.position.is_some()).count();
            let mut info = trf!("{n} movimenti, {drawn} nella vista", n = moves.len(), drawn = drawn);
            if drawn < moves.len() {
                info.push_str(tr(" (gli altri sono in World, Axis o senza punto)"));
            }
            ui.label(egui::RichText::new(info).weak().small());
        }
        // Le due viste del controllore, subito sopra l'elenco (come nel software del controllore): righe del
        // programma o punti con le coordinate
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.selectable_value(&mut panel.view, ProgramView::Logic, icon_text(ui, icon::TEXT_ALIGN_LEFT, tr("Logica")))
                .on_hover_text(tr("Righe del programma"));
            ui.selectable_value(&mut panel.view, ProgramView::Points, icon_text(ui, icon::TABLE, tr("Punti")))
                .on_hover_text(tr("Punti con le coordinate"));
        });
        match panel.view {
            ProgramView::Logic => {
                if let Some(action) = lines_ui(ui, panel) {
                    if let Some(text) = panel.apply_menu(action) {
                        clipboard_text = Some(text);
                    }
                }
            }
            ProgramView::Points => points_ui(ui, panel),
        }
        ui.ui_contains_pointer()
    });
    // «Modifica…»: il testo della riga in una finestrella
    let panel = &mut app.ui_state.program;
    let (mut ok, mut cancel) = (false, false);
    if let Some((i, text)) = &mut panel.editing {
        egui::Window::new(trf!("Modifica riga {n}", n = *i + 1)).id(egui::Id::new("programma_modifica_riga")).collapsible(false).resizable(false).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(text).font(egui::TextStyle::Monospace).desired_width(360.0));
            r.request_focus();
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            ui.horizontal(|ui| {
                ok |= ui.button(tr("OK")).clicked();
                cancel |= ui.button(tr("Annulla")).clicked();
            });
        });
    }
    if ok {
        panel.finish_change();
    } else if cancel {
        panel.editing = None;
    }
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
    if !panel.open || panel.hide_path {
        return;
    }
    // Prima gli altri programmi, più tenui e senza righe scelte; sopra quello attivo
    for d in panel.others.iter().filter(|d| !d.hidden) {
        draw_path(ui, rect, &d.program, &|_| false, true, &to_screen);
    }
    if let (Some(program), false) = (&panel.program, panel.hidden) {
        let selected = |line: usize| panel.selected.binary_search(&line).is_ok() || panel.hovered == Some(line);
        draw_path(ui, rect, program, &selected, false, &to_screen);
    }
}

/// Traiettoria di un programma; `faded` per quelli non attivi.
fn draw_path(
    ui: &egui::Ui,
    rect: egui::Rect,
    program: &Program,
    selected: &dyn Fn(usize) -> bool,
    faded: bool,
    to_screen: &dyn Fn(glam::Vec3) -> Option<egui::Pos2>,
) {
    let painter = ui.painter_at(rect);
    let dim = |c: egui::Color32| if faded { c.gamma_multiply(0.45) } else { c };
    let lin = dim(egui::Color32::from_rgb(255, 170, 40));
    let ptp = dim(egui::Color32::from_rgb(200, 200, 210));
    let event = dim(egui::Color32::from_rgb(60, 210, 230));
    let highlight = egui::Color32::from_rgb(255, 240, 120);
    let shadow = egui::Color32::from_black_alpha(160);
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p));
    let label = |at: egui::Pos2, text: &str| {
        let font = egui::FontId::proportional(12.0);
        let pos = at + egui::vec2(7.0, -7.0);
        painter.text(pos + egui::vec2(1.0, 1.0), egui::Align2::LEFT_BOTTOM, text, font.clone(), shadow);
        painter.text(pos, egui::Align2::LEFT_BOTTOM, text, font, highlight);
    };

    let moves = program.path();
    let screen: Vec<Option<egui::Pos2>> = moves.iter().map(|m| m.position.and_then(to_screen)).collect();
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

    /// Senza matematica, un programma con dei punti apre la vista 3D (niente schermata vuota);
    /// senza punti disegnabili resta la schermata vuota.
    #[test]
    fn a_program_opens_the_3d_view_without_a_math() {
        let mut app = GestoreApp::new_headless();
        let ctx = egui::Context::default();
        let empty = |app: &mut GestoreApp| {
            crate::ui::tests::frame(&ctx, app, vec![]).iter().any(|(t, _)| t == "Trascina qui un file 3D")
        };
        assert!(empty(&mut app));
        let mut program = Program::from_text("prova.tis".into(), "  LIN( A_1)
  LIN( A_2)
");
        crate::robot_program::parse_points(
            "CONST POINT A_1 = Object 100 0 0 0 0 0
CONST POINT A_2 = Object 900 400 50 0 0 0
",
            &mut program.points,
        );
        app.ui_state.program.load(program.clone());
        let b = app.ui_state.program.bounds();
        assert_eq!((b.min, b.max), (glam::Vec3::new(100.0, 0.0, 0.0), glam::Vec3::new(900.0, 400.0, 50.0)));
        assert!(!empty(&mut app), "la vista c'è anche senza matematica");
        app.ui_state.program.hide_path = true;
        assert!(empty(&mut app), "traiettoria nascosta: niente da vedere");
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

    /// Più programmi aperti: ognuno tiene righe, scelta e annulla; lo stesso file non si apre due
    /// volte; chiudendo quello attivo diventa attivo il vicino.
    #[test]
    fn several_programs_keep_their_state() {
        let prog = |name: &str, lines: &[&str]| {
            let text = lines.iter().map(|n| format!("  {n}\r\n")).collect::<String>();
            Program::from_text(name.into(), &text)
        };
        let mut p = ProgramPanel::default();
        p.load(prog("A.TIS", &["a1", "a2"]));
        p.selected = vec![1];
        p.delete();
        assert_eq!(names(&p), ["a1"]);
        p.load(prog("B.TIS", &["b1", "b2", "b3"]));
        p.load(prog("C.TIS", &["c1"]));
        assert_eq!((p.count(), p.active), (3, 2));
        assert_eq!(names(&p), ["c1"]);

        // Torna su A: le sue modifiche e l'annulla ci sono ancora
        p.switch_to(0);
        assert_eq!(names(&p), ["a1"]);
        assert!(p.modified);
        p.undo();
        assert_eq!(names(&p), ["a1", "a2"]);
        // Riaprire B lo rende attivo, senza doppioni
        p.load(prog("B.TIS", &["altro"]));
        assert_eq!((p.count(), p.active), (3, 1));
        assert_eq!(names(&p), ["b1", "b2", "b3"]);
        let order: Vec<String> = (0..p.count()).map(|j| p.get(j).unwrap().0.program_path.display().to_string()).collect();
        assert_eq!(order, ["A.TIS", "B.TIS", "C.TIS"]);

        // Traiettoria nascosta per un programma solo
        p.toggle_hidden(2);
        assert!(p.get(2).unwrap().2 && !p.get(1).unwrap().2);

        // Chiudo B (attivo): diventa attivo C, al suo posto
        p.close(1);
        assert_eq!((p.count(), p.active), (2, 1));
        assert_eq!(names(&p), ["c1"]);
        p.close(0);
        assert_eq!((p.count(), p.active), (1, 0));
        p.close(0);
        assert!(p.program.is_none() && p.count() == 0);
    }

    /// Finestra vera: tasto destro su «b» la sceglie e apre il menu; «Commenta riga» la commenta.
    /// La vista dei punti mostra la tabella.
    #[test]
    fn right_click_opens_the_menu_in_the_window() {
        let mut app = GestoreApp::new_headless();
        app.settings.dock_program = Default::default();
        app.ui_state.program = panel(&["LIN( a)", "LIN( b)"]);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let run = |app: &mut GestoreApp, events: Vec<egui::Event>| {
            let out = ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| draw(ctx, app, crate::ui::dock::Pass::Floating));
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
        let click = |app: &mut GestoreApp, at: egui::Pos2, button: egui::PointerButton| {
            let ev = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: Default::default() };
            run(app, vec![egui::Event::PointerMoved(at)]);
            run(app, vec![ev(true)]);
            run(app, vec![ev(false)]);
            run(app, vec![])
        };
        run(&mut app, vec![]);
        let texts = run(&mut app, vec![]);
        let b = texts.iter().find(|(t, _)| t == "  LIN( b)").expect("riga b").1.center();
        let texts = click(&mut app, b, egui::PointerButton::Secondary);
        assert_eq!(app.ui_state.program.selected, [1]);
        let item = texts.iter().find(|(t, _)| t.ends_with("Commenta riga")).unwrap_or_else(|| panic!("menu: {texts:?}")).1.center();
        click(&mut app, item, egui::PointerButton::Primary);
        assert_eq!(names(&app.ui_state.program), ["LIN( a)", "--LIN( b)"]);

        app.ui_state.program.view = ProgramView::Points;
        let texts = run(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t == "Coord"), "tabella dei punti");
    }

    /// Voci del menu del tasto destro: commenta, togli commento, modifica, inverti.
    #[test]
    fn right_click_menu_actions() {
        let mut p = panel(&["LIN( A)", "LIN( B)", "LIN( C)"]);
        p.selected = vec![0, 2];
        p.apply_menu(MenuAction::Comment);
        assert_eq!(names(&p), ["--LIN( A)", "LIN( B)", "--LIN( C)"]);
        p.apply_menu(MenuAction::Uncomment);
        assert_eq!(names(&p), ["LIN( A)", "LIN( B)", "LIN( C)"]);
        p.apply_menu(MenuAction::Reverse);
        assert_eq!(names(&p), ["LIN( C)", "LIN( B)", "LIN( A)"]);
        p.selected = vec![1];
        p.apply_menu(MenuAction::Change);
        assert_eq!(p.editing, Some((1, "LIN( B)".to_string())));
        p.editing.as_mut().unwrap().1 = "PTP( Z) ".into();
        p.finish_change();
        assert_eq!(p.lines()[1].0, "  PTP( Z)", "tiene il rientro");
        assert_eq!(p.apply_menu(MenuAction::Copy).as_deref(), Some("  PTP( Z)"));
        p.apply_menu(MenuAction::Undo);
        assert_eq!(names(&p), ["LIN( C)", "LIN( B)", "LIN( A)"]);
    }

    #[test]
    fn title_like_the_controller() {
        let program = Program::from_text("PEZZO_AP.TIS".into(), "  SELECT( \"R11\")\r\n  SETOBJECT( PEZZO)\r\n");
        assert_eq!(program_title(&program), "R11 : PEZZO : PEZZO_AP");
    }
}
