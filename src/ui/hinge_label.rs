//! Cerniere salvate nella vista: cliccando una parte che ha una cerniera compare un'etichetta con
//! i gradi di apertura; Ctrl + trascina sulla parte la apre con il mouse (vedi
//! `scene_graph::hinge`).

use glam::Vec3;

use super::theme::{icon, icon_text};
use crate::app::{Action, GestoreApp};
use crate::scene_graph::hinge::SavedHinge;

/// Trascinamento in corso di una cerniera: il punto preso, nelle coordinate del pezzo di
/// riferimento (segue la parte mentre ruota).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HingeDrag {
    pub index: usize,
    pub grab: Vec3,
}

impl GestoreApp {
    /// Salva una cerniera: la parte `nodes` gira sull'asse (`origin`, `dir`) ed è aperta di
    /// `angle` gradi dalla posizione chiusa. Un passo di Annulla.
    pub fn save_hinge(&mut self, name: &str, nodes: Vec<u32>, origin: Vec3, dir: Vec3, angle: f32) -> Option<usize> {
        let scene = self.scene_graph.as_ref()?;
        let name = if name.trim().is_empty() { format!("Cerniera {}", scene.hinges.len() + 1) } else { name.trim().to_string() };
        let Some(hinge) = scene.new_hinge(name.clone(), nodes, origin, dir, angle) else {
            self.ui_state.status_message = "Cerniera non salvata: scegli la parte che si apre".to_string();
            return None;
        };
        self.push_undo(None);
        let scene = self.scene_graph.as_mut()?;
        scene.hinges.push(hinge);
        scene.revision += 1;
        self.ui_state.status_message = format!("Cerniera «{name}» salvata: clicca la parte nella vista per aprirla");
        Some(scene.hinges.len() - 1)
    }

    /// Cambia nome o limiti di una cerniera (un passo di Annulla); l'angolo si cambia con
    /// `Action::SetHingeAngle`. Se l'angolo esce dai nuovi limiti la parte si porta al limite.
    pub fn edit_hinge(&mut self, index: usize, edit: impl FnOnce(&mut SavedHinge)) {
        let Some(old) = self.scene_graph.as_ref().and_then(|s| s.hinges.get(index)).cloned() else { return };
        let mut new = old.clone();
        edit(&mut new);
        new.max = new.max.max(new.min);
        if new == old {
            return;
        }
        // Nome scritto lettera per lettera, limiti trascinati: un solo passo di Annulla
        self.push_undo(Some(index as u32 | 0x2000_0000));
        let Some(scene) = self.scene_graph.as_mut() else { return };
        let angle = new.angle;
        new.angle = old.angle;
        scene.hinges[index] = new;
        scene.set_hinge_angle(index, angle);
        scene.revision += 1;
        self.sync_moved_geometry();
    }

    /// La posizione attuale della parte diventa la chiusa (0°), senza muoverla.
    pub fn hinge_closed_here(&mut self, index: usize) {
        if self.scene_graph.as_ref().is_none_or(|s| index >= s.hinges.len()) {
            return;
        }
        self.push_undo(None);
        if let Some(h) = self.scene_graph.as_mut().map(|s| &mut s.hinges[index]) {
            let shift = h.angle;
            h.min -= shift;
            h.max -= shift;
            h.angle = 0.0;
            self.ui_state.status_message = format!("{}: questa è la posizione chiusa (0°)", h.name);
        }
        if let Some(s) = self.scene_graph.as_mut() {
            s.revision += 1;
        }
    }

    /// La cerniera `index` passa alla parte `nodes` (per correggere un pezzo sbagliato): la parte di
    /// prima torna chiusa (0°, entro i limiti), l'asse resta dov'è e la nuova parte parte da 0°
    /// nella posizione in cui si trova. Un passo di Annulla.
    pub fn reassign_hinge(&mut self, index: usize, nodes: Vec<u32>) {
        let Some(scene) = &self.scene_graph else { return };
        let Some(old) = scene.hinges.get(index).cloned() else { return };
        if nodes.is_empty() || nodes == old.nodes {
            return;
        }
        if scene.meshes_of(&nodes).is_empty() {
            self.ui_state.status_message = "Cerniera non cambiata: la selezione non ha pezzi".to_string();
            return;
        }
        self.push_undo(None);
        let Some(scene) = self.scene_graph.as_mut() else { return };
        scene.set_hinge_angle(index, 0.0);
        let (o, d) = scene.hinge_axis(&scene.hinges[index]);
        let names: Vec<String> = nodes.iter().map(|&n| scene.display_name(n)).collect();
        if let Some(mut new) = scene.new_hinge(old.name.clone(), nodes, o, d, 0.0) {
            new.min = old.min.min(0.0);
            new.max = old.max.max(0.0);
            scene.hinges[index] = new;
        }
        scene.revision += 1;
        self.sync_moved_geometry();
        self.ui_state.status_message = format!("Cerniera «{}» assegnata a {}", old.name, names.join(", "));
    }

    /// Toglie una cerniera (la parte resta dov'è). Un passo di Annulla.
    pub fn remove_hinge(&mut self, index: usize) {
        if self.scene_graph.as_ref().is_none_or(|s| index >= s.hinges.len()) {
            return;
        }
        self.push_undo(None);
        if let Some(scene) = self.scene_graph.as_mut() {
            let h = scene.hinges.remove(index);
            scene.revision += 1;
            self.ui_state.status_message = format!("Cerniera «{}» tolta (la parte resta dov'è)", h.name);
        }
    }

    /// Cerniera della selezione attuale (la parte cliccata nella vista o nell'albero).
    pub fn selected_hinge(&self) -> Option<usize> {
        let scene = self.scene_graph.as_ref()?;
        let node = self.ui_state.selected_node?;
        scene.hinge_of_node(node)
    }
}

/// Etichetta della cerniera della parte selezionata, vicino alla parte: nome, gradi, chiudi e apri.
pub fn draw_label(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    app: &mut GestoreApp,
    to_screen: impl Fn(Vec3) -> Option<egui::Pos2>,
) {
    let Some(index) = app.selected_hinge() else { return };
    let Some(scene) = &app.scene_graph else { return };
    let h = scene.hinges[index].clone();
    let (o, d) = scene.hinge_axis(&h);
    let mut bbox = crate::scene_graph::bbox::Aabb::empty();
    for &n in &h.nodes {
        bbox.extend_aabb(&scene.subtree_bounding_box(n));
    }
    let anchor = if bbox.is_valid() { bbox.center() } else { o };
    let Some(at) = to_screen(anchor).filter(|p| rect.contains(*p)) else { return };

    // Asse nella vista: una linea arancione lunga quanto la parte
    let painter = ui.painter_at(rect);
    let accent = egui::Color32::from_rgb(255, 140, 30);
    let half = if bbox.is_valid() { bbox.diagonal() * 0.6 } else { 500.0 };
    if let (Some(a), Some(b)) = (to_screen(o - d * half), to_screen(o + d * half)) {
        painter.line_segment([a, b], egui::Stroke::new(2.0_f32, accent));
    }

    // Spostamento dell'etichetta dalla parte (trascinando il titolo), ricordato per ogni cerniera
    let offset_id = egui::Id::new(("etichetta_cerniera_spostamento", index));
    let default_offset = egui::vec2(14.0, -14.0);
    let mut offset = ui.ctx().data(|d| d.get_temp::<egui::Vec2>(offset_id)).unwrap_or(default_offset);
    let mut angle = h.angle;
    let shown = egui::Area::new(egui::Id::new("etichetta_cerniera"))
        .order(egui::Order::Foreground)
        .fixed_pos(at + offset)
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let title = ui
                        .add(
                            egui::Label::new(egui::RichText::new(format!("{}  {}  {}", icon::DOTS_SIX, icon::DOOR_OPEN, h.name)).strong())
                                .selectable(false)
                                .sense(egui::Sense::click_and_drag()),
                        )
                        .on_hover_cursor(egui::CursorIcon::Grab)
                        .on_hover_text("Trascina per spostare l'etichetta; doppio clic la rimette vicino alla parte");
                    if title.dragged() {
                        offset += title.drag_delta();
                    }
                    if title.double_clicked() {
                        offset = default_offset;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.small_button(icon::CARET_LEFT).on_hover_text("Chiude di 1°").clicked() {
                        angle -= 1.0;
                    }
                    ui.add(
                        egui::DragValue::new(&mut angle)
                            .range(h.min..=h.max)
                            .speed(0.5)
                            .max_decimals(1)
                            .suffix("°"),
                    )
                    .on_hover_text("Apertura: trascina il numero o fai doppio clic e scrivi i gradi");
                    if ui.small_button(icon::CARET_RIGHT).on_hover_text("Apre di 1°").clicked() {
                        angle += 1.0;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(format!("Chiudi ({:.0}°)", h.min)).clicked() {
                        angle = h.min;
                    }
                    if ui.button(format!("Apri ({:.0}°)", h.max)).clicked() {
                        angle = h.max;
                    }
                });
                ui.label(egui::RichText::new("Ctrl + trascina la parte per aprirla col mouse").small().weak());
            });
        });
    ui.ctx().data_mut(|d| d.insert_temp(offset_id, offset));
    // Spostata lontano: una linea sottile la lega alla parte
    let label = shown.response.rect;
    if !label.expand(4.0).contains(at) && offset != default_offset {
        let end = egui::pos2(at.x.clamp(label.left(), label.right()), at.y.clamp(label.top(), label.bottom()));
        painter.line_segment([at, end], egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(140)));
        painter.circle_filled(at, 3.0, accent);
    }
    if (angle - h.angle).abs() > 1e-5 {
        app.apply(Action::SetHingeAngle { index, angle });
    }
}

/// Cosa chiede l'elenco delle cerniere (eseguito dopo il disegno, con [`run`]).
#[derive(Debug, Clone, PartialEq)]
pub enum HingeCmd {
    Angle(usize, f32),
    Edit(usize, SavedHinge),
    ClosedHere(usize),
    /// Assegna la cerniera alla selezione attuale
    Reassign(usize),
    Remove(usize),
    Select(Vec<u32>),
}

/// Elenco delle cerniere salvate (finestra Posizione): nome, gradi, parte, limiti, chiusa qui,
/// togli. `selection`: nodi selezionati e il loro nome, per «Cambia parte».
pub fn draw_list(ui: &mut egui::Ui, hinges: &[(usize, SavedHinge, String)], selection: (&[u32], &str)) -> Vec<HingeCmd> {
    let mut out = Vec::new();
    if hinges.is_empty() {
        ui.label(
            egui::RichText::new("Nessuna cerniera salvata: scegli l'asse e la parte, poi «Salva cerniera».")
                .small()
                .weak(),
        );
        return out;
    }
    for (i, h, part) in hinges {
        let i = *i;
        let mut e = h.clone();
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(false, egui::RichText::new(format!("{}  {}", icon::DOOR_OPEN, h.name)).strong())
                .on_hover_text("Seleziona la parte: nella vista compare l'etichetta con i gradi")
                .clicked()
            {
                out.push(HingeCmd::Select(h.nodes.clone()));
            }
            let mut angle = h.angle;
            ui.add(egui::DragValue::new(&mut angle).range(h.min..=h.max).speed(0.5).max_decimals(1).suffix("°"))
                .on_hover_text("Apertura");
            if (angle - h.angle).abs() > 1e-5 {
                out.push(HingeCmd::Angle(i, angle));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Nome");
            ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(120.0));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Parte");
            ui.label(egui::RichText::new(part).italics());
            let (nodes, name) = selection;
            if ui
                .add_enabled(!nodes.is_empty() && nodes != h.nodes.as_slice(), egui::Button::new(icon_text(ui, icon::PENCIL_SIMPLE, "Cambia parte")).small())
                .on_hover_text(format!(
                    "La cerniera passa alla selezione ({name}): la parte di prima torna chiusa, la nuova parte da 0° dove si trova"
                ))
                .on_disabled_hover_text("Seleziona prima (nella vista o nell'albero) la parte giusta")
                .clicked()
            {
                out.push(HingeCmd::Reassign(i));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Da");
            ui.add(egui::DragValue::new(&mut e.min).range(-360.0..=h.max).speed(1.0).max_decimals(1).suffix("°"))
                .on_hover_text("Limite di chiusura");
            ui.label("a");
            ui.add(egui::DragValue::new(&mut e.max).range(h.min..=360.0).speed(1.0).max_decimals(1).suffix("°"))
                .on_hover_text("Limite di apertura");
            if ui.small_button("0° qui").on_hover_text("La posizione attuale diventa quella chiusa (0°), senza muovere la parte").clicked() {
                out.push(HingeCmd::ClosedHere(i));
            }
            if ui.small_button(icon::TRASH).on_hover_text("Togli la cerniera (la parte resta dov'è)").clicked() {
                out.push(HingeCmd::Remove(i));
            }
        });
        if e.min != h.min || e.max != h.max || e.name != h.name {
            out.push(HingeCmd::Edit(i, e));
        }
        ui.separator();
    }
    out
}

/// Esegue i comandi dell'elenco.
pub fn run(app: &mut GestoreApp, cmds: Vec<HingeCmd>) {
    for cmd in cmds {
        match cmd {
            HingeCmd::Angle(i, angle) => app.apply(Action::SetHingeAngle { index: i, angle }),
            HingeCmd::Edit(i, e) => app.edit_hinge(i, |h| {
                h.name = e.name;
                h.min = e.min;
                h.max = e.max.max(e.min);
                h.angle = h.angle.clamp(h.min, h.max);
            }),
            HingeCmd::ClosedHere(i) => app.hinge_closed_here(i),
            HingeCmd::Reassign(i) => {
                let nodes = app.ui_state.selected_nodes();
                app.reassign_hinge(i, nodes);
            }
            HingeCmd::Remove(i) => app.remove_hinge(i),
            HingeCmd::Select(nodes) => {
                app.ui_state.select_single(nodes.first().copied());
                app.ui_state.selected_extra.extend(nodes.iter().skip(1).copied());
            }
        }
    }
}
