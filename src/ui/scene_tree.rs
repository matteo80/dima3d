use super::theme::{icon, icon_text};
use crate::app::Action;
use crate::scene_graph::{NodeKind, SceneGraph};

/// Indentazione per livello di profondità (punti logici).
const INDENT: f32 = 14.0;

/// Disegna l'albero della scena nel pannello laterale.
///
/// L'albero è "virtualizzato": a ogni frame si calcola la lista piatta delle righe
/// visibili (solo i nodi i cui antenati sono espansi) e si disegnano esclusivamente
/// quelle che rientrano nell'area di scorrimento (`ScrollArea::show_rows`).
/// Così restano fluidi anche contenitori con decine di migliaia di Shape.
pub fn draw_scene_tree(ui: &mut egui::Ui, app: &mut crate::app::GestoreApp) {
    let mut actions: Vec<Action> = Vec::new();

    {
        let Some(scene) = &app.scene_graph else {
            ui.heading("Scena");
            ui.separator();
            ui.label(egui::RichText::new("Nessun file caricato.").weak());
            return;
        };

        // ── Intestazione ────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.heading("Scena");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                super::theme::flat_buttons(ui);
                if ui.button(icon_text(ui, icon::ARROWS_IN_LINE_VERTICAL, "")).on_hover_text("Comprimi tutto").clicked() {
                    app.ui_state.expanded.clear();
                }
                if ui.button(icon_text(ui, icon::ARROWS_OUT_LINE_VERTICAL, "")).on_hover_text("Espandi i macro-oggetti principali").clicked() {
                    for &r in &scene.roots {
                        app.ui_state.expanded.insert(r);
                    }
                }
            });
        });
        ui.label(
            egui::RichText::new(format!("{} macro-oggetti principali", scene.top_level_count()))
                .small()
                .weak(),
        );

        // ── Ricerca per nome ────────────────────────────────────────────────
        // Layout da destra: prima il pulsante, poi il campo occupa esattamente lo spazio rimasto
        // (una larghezza calcolata "a mano" faceva crescere il pannello a ogni frame).
        // Altezza limitata a una riga: un with_layout libero occuperebbe tutto il pannello
        // (il campo finiva a metà altezza e l'elenco sotto spariva).
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = ui
                .add_enabled(!app.ui_state.search.is_empty(), egui::Button::new(icon::X))
                .on_hover_text("Cancella ricerca")
                .clicked();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut app.ui_state.search)
                    .hint_text(format!("{} Cerca per nome…  (Ctrl+F)", icon::MAGNIFYING_GLASS))
                    .desired_width(f32::INFINITY),
            );
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                resp.request_focus();
            }
            if clear || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
                app.ui_state.search.clear();
            }
        });
        let query = app.ui_state.search.trim().to_lowercase();
        let search_mode = !query.is_empty();
        if search_mode {
            let stale = !matches!(&app.ui_state.search_cache,
                Some((q, rev, _, _)) if *q == query && *rev == scene.revision);
            if stale {
                let (rows, n) = search_rows(scene, &query);
                app.ui_state.search_cache = Some((query.clone(), scene.revision, rows, n));
            }
            let n = app.ui_state.search_cache.as_ref().map_or(0, |c| c.3);
            ui.label(
                egui::RichText::new(if n == 0 {
                    "Nessun elemento trovato".to_string()
                } else {
                    format!("{n} elementi trovati")
                })
                .small()
                .weak(),
            );
        }
        ui.separator();

        // ── Righe visibili (DFS pre-order sui soli nodi espansi, o risultati della ricerca) ──
        let rows = if search_mode {
            app.ui_state.search_cache.as_ref().map(|c| c.2.clone()).unwrap_or_default()
        } else {
            visible_rows(scene, &app.ui_state.expanded)
        };

        let row_height = ui.spacing().interact_size.y;
        let row_stride = row_height + ui.spacing().item_spacing.y;
        let mut scroll = egui::ScrollArea::both().auto_shrink([false, false]);

        // Frecce: su/giù cambiano riga (con Shift estendono la selezione),
        // sinistra/destra chiudono/aprono il nodo o salgono al padre/scendono al primo figlio.
        let selection = app.ui_state.selected_nodes();
        let keyboard_free = !ui.ctx().wants_keyboard_input()
            && app.project.pending.is_none()
            && app.ui_state.renaming.is_none();
        if keyboard_free && !rows.is_empty() {
            let moved = navigate_with_keys(
                ui.ctx(),
                scene,
                &rows,
                app.ui_state.selected_node,
                app.ui_state.selection_anchor,
                (!search_mode).then_some(&mut app.ui_state.expanded),
                &mut actions,
            );
            if let Some(row) = moved {
                let (offset, height) = app.ui_state.tree_view;
                let top = row as f32 * row_stride;
                if top < offset {
                    scroll = scroll.vertical_scroll_offset(top);
                } else if top + row_height > offset + height {
                    scroll = scroll.vertical_scroll_offset(top + row_height - height);
                }
            }
        }

        if app.ui_state.scroll_to_selected {
            if let Some(sel) = app.ui_state.selected_node {
                if let Some(i) = rows.iter().position(|&(n, _)| n == sel) {
                    let target = i as f32 * row_stride - ui.available_height() * 0.3;
                    scroll = scroll.vertical_scroll_offset(target.max(0.0));
                }
            }
            app.ui_state.scroll_to_selected = false;
        }

        let mut ctx = RowCtx {
            scene,
            selection: &selection,
            anchor: app.ui_state.selection_anchor.or(app.ui_state.selected_node),
            rows: &rows,
            expanded: &mut app.ui_state.expanded,
            renaming: &mut app.ui_state.renaming,
            default_color: app.settings.default_color,
            actions: &mut actions,
            search: search_mode.then_some(query.as_str()),
        };
        let output = scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
            for &(node_idx, depth) in &rows[range] {
                draw_row(ui, &mut ctx, node_idx, depth);
            }
        });
        app.ui_state.tree_view = (output.state.offset.y, output.inner_rect.height());
    }

    for a in actions {
        app.apply(a);
    }
}

/// Nodi tra due righe dell'albero, estremi inclusi.
fn range_between(rows: &[(u32, u16)], a: u32, b: u32) -> Option<Vec<u32>> {
    let a = rows.iter().position(|r| r.0 == a)?;
    let b = rows.iter().position(|r| r.0 == b)?;
    Some(rows[a.min(b)..=a.max(b)].iter().map(|r| r.0).collect())
}

/// Azione della selezione verso `node`: normale o estesa dall'estremo fisso con Shift.
fn select_to(rows: &[(u32, u16)], anchor: Option<u32>, node: u32, extend: bool) -> Action {
    if extend {
        if let Some(anchor) = anchor {
            if let Some(nodes) = range_between(rows, anchor, node) {
                return Action::SelectMany { nodes, primary: node, anchor };
            }
        }
    }
    Action::Select { node: Some(node), reveal: false }
}

/// Gestisce le frecce. Ritorna la riga su cui si è spostata la selezione.
fn navigate_with_keys(
    ctx: &egui::Context,
    scene: &SceneGraph,
    rows: &[(u32, u16)],
    current: Option<u32>,
    anchor: Option<u32>,
    expanded: Option<&mut std::collections::HashSet<u32>>,
    actions: &mut Vec<Action>,
) -> Option<usize> {
    use egui::{Key, Modifiers};
    let (up, down, shift_up, shift_down, left, right) = ctx.input_mut(|i| {
        let mut key = |m, k| i.consume_key(m, k);
        // Prima le combinazioni con Shift: `Modifiers::NONE` accetterebbe anche quelle
        let shift_up = key(Modifiers::SHIFT, Key::ArrowUp);
        let shift_down = key(Modifiers::SHIFT, Key::ArrowDown);
        (
            key(Modifiers::NONE, Key::ArrowUp),
            key(Modifiers::NONE, Key::ArrowDown),
            shift_up,
            shift_down,
            key(Modifiers::NONE, Key::ArrowLeft),
            key(Modifiers::NONE, Key::ArrowRight),
        )
    });
    if !(up || down || shift_up || shift_down || left || right) {
        return None;
    }
    ctx.request_repaint(); // lo spostamento si vede già dal frame successivo

    let row = current.and_then(|c| rows.iter().position(|r| r.0 == c));
    let target = if up || shift_up {
        Some(row.map_or(0, |r| r.saturating_sub(1)))
    } else if down || shift_down {
        Some(row.map_or(0, |r| (r + 1).min(rows.len() - 1)))
    } else {
        let (row, expanded) = (row?, expanded?);
        let node = rows[row].0;
        let has_children = !scene.nodes[node as usize].children().is_empty();
        if left {
            if expanded.remove(&node) {
                None
            } else {
                scene.parent_of(node).and_then(|p| rows.iter().position(|r| r.0 == p))
            }
        } else if !has_children {
            None
        } else if expanded.insert(node) {
            None
        } else {
            Some(row + 1).filter(|&r| r < rows.len())
        }
    }?;
    let extend = shift_up || shift_down;
    let origin = anchor.or(current);
    actions.push(select_to(rows, origin, rows[target].0, extend));
    Some(target)
}

/// Lista piatta (nodo, profondità) dei nodi da mostrare.
fn visible_rows(scene: &SceneGraph, expanded: &std::collections::HashSet<u32>) -> Vec<(u32, u16)> {
    let mut rows = Vec::new();
    let mut stack: Vec<(u32, u16)> = scene.roots.iter().rev().map(|&r| (r, 0)).collect();
    while let Some((idx, depth)) = stack.pop() {
        rows.push((idx, depth));
        if expanded.contains(&idx) {
            let children = scene.nodes[idx as usize].children();
            stack.extend(children.iter().rev().map(|&c| (c, depth + 1)));
        }
    }
    rows
}

/// Righe da mostrare per una ricerca: i nodi il cui nome contiene `query`
/// (minuscolo) più i loro antenati, per mantenere il contesto della gerarchia.
/// Ritorna anche il numero di corrispondenze.
fn search_rows(scene: &SceneGraph, query: &str) -> (Vec<(u32, u16)>, usize) {
    let n = scene.nodes.len();
    let mut include = vec![false; n];
    let mut matches = 0;
    for i in 0..n {
        if !scene.info[i].deleted && scene.display_name(i as u32).to_lowercase().contains(query) {
            include[i] = true;
            matches += 1;
        }
    }
    // Propaga agli antenati (arena in pre-order: il padre ha indice minore)
    for i in (0..n).rev() {
        let p = scene.info[i].parent;
        if include[i] && p != crate::scene_graph::NO_PARENT {
            include[p as usize] = true;
        }
    }
    let mut rows = Vec::new();
    let mut stack: Vec<(u32, u16)> =
        scene.roots.iter().rev().filter(|&&r| include[r as usize]).map(|&r| (r, 0)).collect();
    while let Some((idx, depth)) = stack.pop() {
        rows.push((idx, depth));
        let children = scene.nodes[idx as usize].children();
        stack.extend(
            children.iter().rev().filter(|&&c| include[c as usize]).map(|&c| (c, depth + 1)),
        );
    }
    (rows, matches)
}

#[cfg(test)]
mod tests {
    #[test]
    fn search_shows_matches_with_ancestors() {
        let scene = crate::parser::parse_wrl(include_bytes!("../../esempi/test_colori.wrl"));
        let (rows, n) = super::search_rows(&scene, "blu");
        assert_eq!(n, 2); // Cubo_Blu, Cubo_Blu_Riusato
        let names: Vec<String> = rows.iter().map(|&(i, _)| scene.display_name(i)).collect();
        assert_eq!(names[0], "Assieme");
        assert!(names.contains(&"Cubo_Blu".to_string()));
        assert!(!names.contains(&"Cubo_Rosso".to_string()));
        let (_, none) = super::search_rows(&scene, "inesistente");
        assert_eq!(none, 0);
    }
}

/// Stato condiviso dal disegno delle righe.
struct RowCtx<'a> {
    /// Testo cercato (minuscolo) in modalità ricerca
    search: Option<&'a str>,
    scene: &'a SceneGraph,
    /// Nodi selezionati, in ordine crescente
    selection: &'a [u32],
    /// Estremo fisso della selezione con Shift
    anchor: Option<u32>,
    /// Righe mostrate, per le selezioni a intervallo
    rows: &'a [(u32, u16)],
    expanded: &'a mut std::collections::HashSet<u32>,
    renaming: &'a mut Option<crate::ui::Renaming>,
    default_color: [f32; 4],
    actions: &'a mut Vec<Action>,
}

fn draw_row(ui: &mut egui::Ui, ctx: &mut RowCtx, node_idx: u32, depth: u16) {
    let scene = ctx.scene;
    let node = &scene.nodes[node_idx as usize];
    let info = scene.info[node_idx as usize];
    let has_children = !node.children().is_empty();
    let is_expanded = ctx.expanded.contains(&node_idx);
    let is_visible = node.is_visible();
    let is_selected = ctx.selection.binary_search(&node_idx).is_ok();
    // Il menu contestuale su una riga della selezione multipla vale per tutta la selezione
    let group: Option<&[u32]> = (is_selected && ctx.selection.len() > 1).then_some(ctx.selection);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().button_padding.x = 2.0;
        ui.add_space(depth as f32 * INDENT);

        // Freccia espandi/comprimi
        if has_children {
            let arrow = if is_expanded { icon::CARET_DOWN } else { icon::CARET_RIGHT };
            if ui.add(egui::Button::new(arrow).frame(false).min_size(egui::vec2(14.0, 0.0))).clicked() {
                if is_expanded {
                    ctx.expanded.remove(&node_idx);
                } else {
                    ctx.expanded.insert(node_idx);
                }
            }
        } else {
            ui.add_space(14.0);
        }

        // Occhio visibilità (agisce su tutto il sotto-albero)
        let eye = if is_visible { icon::EYE } else { icon::EYE_SLASH };
        if ui
            .add(egui::Button::new(eye).frame(false))
            .on_hover_text(if is_visible { "Nascondi" } else { "Mostra" })
            .clicked()
        {
            ctx.actions.push(Action::ToggleVisible(node_idx));
        }

        // Quadratino colore: colore impostato sul nodo, oppure colore originale del pezzo
        let swatch = info.color.or_else(|| match node {
            crate::scene_graph::SceneNode::Shape { mesh_id, .. } => {
                scene.shapes_data.get(*mesh_id as usize).and_then(|s| s.color)
            }
            _ => None,
        });
        if let Some(c) = swatch {
            let (rect, r) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, crate::ui::color_menu::to_color32(c));
            if info.color.is_some() {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0_f32, ui.visuals().strong_text_color()),
                    egui::StrokeKind::Outside,
                );
                r.on_hover_text("Colore impostato manualmente");
            }
        }

        let icon = match info.kind {
            NodeKind::Cluster => icon::PACKAGE,
            NodeKind::Singles => icon::STACK,
            NodeKind::Use => icon::LINK,
            NodeKind::Group | NodeKind::Transform => icon::FOLDER_SIMPLE,
            NodeKind::Shape => icon::CUBE,
            NodeKind::File => icon::FILE,
        };

        // Campo di rinomina al posto dell'etichetta
        if let Some(ren) = ctx.renaming.as_mut().filter(|r| r.node == node_idx) {
            ui.label(icon);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut ren.text)
                    .desired_width(200.0),
            );
            if ren.focus {
                edit.request_focus();
                ren.focus = false;
            }
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if escape {
                *ctx.renaming = None;
            } else if edit.lost_focus() || (enter && edit.has_focus()) {
                let text = std::mem::take(&mut ren.text);
                ctx.actions.push(Action::Rename(node_idx, text));
                *ctx.renaming = None;
            }
            return;
        }

        // Etichetta selezionabile
        let label = scene.display_name(node_idx);
        let is_match = ctx.search.is_some_and(|q| label.to_lowercase().contains(q));
        let mut text = egui::RichText::new(format!("{icon} {label}"));
        if !is_visible {
            text = text.weak().strikethrough();
        } else if ctx.search.is_some() && !is_match {
            text = text.weak(); // antenato mostrato solo come contesto
        } else if has_children {
            text = text.strong();
        }
        if is_match {
            text = text.underline();
        }
        let resp = ui.add(egui::SelectableLabel::new(is_selected, text));

        if has_children {
            ui.label(egui::RichText::new(format!("({})", info.shape_count)).small().weak());
        }

        if resp.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            if modifiers.shift {
                ctx.actions.push(select_to(ctx.rows, ctx.anchor, node_idx, true));
            } else if modifiers.command || modifiers.ctrl {
                // Ctrl: aggiunge o toglie un elemento
                let mut nodes = ctx.selection.to_vec();
                match nodes.binary_search(&node_idx) {
                    Ok(i) => {
                        nodes.remove(i);
                    }
                    Err(i) => nodes.insert(i, node_idx),
                }
                ctx.actions.push(match nodes.last().copied() {
                    Some(last) => {
                        let primary = if is_selected { last } else { node_idx };
                        Action::SelectMany { nodes, primary, anchor: node_idx }
                    }
                    None => Action::Select { node: None, reveal: false },
                });
            } else {
                // In ricerca: espandi il percorso, così resta visibile anche cancellando il filtro
                ctx.actions.push(Action::Select { node: Some(node_idx), reveal: ctx.search.is_some() });
            }
        }
        if resp.double_clicked() {
            ctx.actions.push(Action::Focus(Some(node_idx)));
        }

        let actions = &mut *ctx.actions;
        let default_color = ctx.default_color;
        // La stessa azione sulla riga o su tutta la selezione multipla di cui fa parte
        let on_targets = |action: fn(u32) -> Action| match group {
            Some(nodes) => Action::Batch(nodes.iter().map(|&n| action(n)).collect()),
            None => action(node_idx),
        };

        // Menu contestuale (tasto destro)
        resp.context_menu(|ui| {
            if ui.button(icon_text(ui, icon::CUBE_FOCUS, "Isola")).clicked() {
                match group {
                    Some(nodes) => actions.push(Action::IsolateMany(nodes.to_vec())),
                    None => {
                        actions.push(Action::Select { node: Some(node_idx), reveal: false });
                        actions.push(Action::Isolate(node_idx));
                    }
                }
                ui.close_menu();
            }
            let (eye, label, action) = if is_visible {
                (icon::EYE_SLASH, "Nascondi", on_targets(Action::Hide))
            } else {
                (icon::EYE, "Mostra", on_targets(Action::Show))
            };
            if ui.button(icon_text(ui, eye, label)).clicked() {
                actions.push(action);
                ui.close_menu();
            }
            if ui.button(icon_text(ui, icon::CORNERS_OUT, "Inquadra")).clicked() {
                actions.push(match group {
                    Some(nodes) => Action::FocusMany(nodes.to_vec()),
                    None => Action::Focus(Some(node_idx)),
                });
                ui.close_menu();
            }
            ui.separator();
            if ui.button(icon_text(ui, icon::PENCIL_SIMPLE, "Rinomina    F2")).clicked() {
                actions.push(Action::StartRename(node_idx));
                ui.close_menu();
            }
            ui.menu_button(icon_text(ui, icon::PALETTE, "Colore"), |ui| {
                let mut chosen = Vec::new();
                crate::ui::color_menu::color_menu(ui, scene, node_idx, default_color, &mut chosen);
                actions.extend(crate::ui::color_menu::spread_colors(chosen, group.unwrap_or(&[])));
            });
            ui.separator();
            let is_file = info.kind == NodeKind::File;
            if is_file && group.is_none() {
                if ui.button(icon_text(ui, icon::EXPORT, "Esporta questo file…")).clicked() {
                    actions.push(Action::ExportNode(node_idx));
                    ui.close_menu();
                }
                ui.menu_button(icon_text(ui, icon::RULER, "Unità di misura"), |ui| {
                    let scale = scene.sources.iter().find(|s| s.node == node_idx).map_or(1.0, |s| s.scale);
                    if scale != 1.0 {
                        ui.label(egui::RichText::new(format!("Scala applicata: {scale}")).weak());
                    }
                    for factor in [0.001, 1000.0] {
                        if ui.button(crate::project::unit_label(factor)).clicked() {
                            actions.push(Action::ScaleFile(node_idx, factor));
                            ui.close_menu();
                        }
                    }
                });
            }
            let (delete_icon, delete_label) =
                if is_file && group.is_none() { (icon::X, "Chiudi file") } else { (icon::TRASH, "Elimina") };
            if ui.button(icon_text(ui, delete_icon, delete_label)).clicked() {
                actions.push(on_targets(Action::Delete));
                ui.close_menu();
            }
        });
    });
}
