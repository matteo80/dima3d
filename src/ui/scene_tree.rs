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
        let mut scroll = egui::ScrollArea::both().auto_shrink([false, false]).drag_to_scroll(false);

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
            saved_colors: app.settings.saved_colors.clone(),
            can_paste: !app.ui_state.clipboard.is_empty(),
            actions: &mut actions,
            search: search_mode.then_some(query.as_str()),
            sweep: &mut app.ui_state.tree_sweep,
        };
        // Spazio vuoto dell'albero (come in FreeCAD): clic = niente selezionato, trascinando si
        // disegna un rettangolo e si selezionano le righe che tocca. Registrato prima delle
        // righe, così sulle righe vincono loro
        let tree_rect = ui.available_rect_before_wrap();
        let background = ui.interact(tree_rect, ui.id().with("albero_vuoto"), egui::Sense::click_and_drag());
        let output = scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
            for &(node_idx, depth) in &rows[range] {
                draw_row(ui, &mut ctx, node_idx, depth);
            }
        });
        app.ui_state.tree_view = (output.state.offset.y, output.inner_rect.height());
        let content_top = output.inner_rect.top() - output.state.offset.y;
        if background.clicked() && !ui.input(|i| i.modifiers.ctrl || i.modifiers.shift) {
            actions.push(Action::Select { node: None, reveal: false });
        }
        if background.drag_started() {
            if let Some(pos) = background.interact_pointer_pos() {
                app.ui_state.tree_band = Some(egui::pos2(pos.x, pos.y - content_top));
            }
        }
        if let (Some(start), Some(pos)) = (app.ui_state.tree_band, ui.ctx().pointer_latest_pos()) {
            let band = egui::Rect::from_two_pos(egui::pos2(start.x, start.y + content_top), pos);
            let visible = band.intersect(output.inner_rect);
            if visible.is_positive() {
                let accent = ui.visuals().selection.bg_fill;
                let painter = ui.painter_at(output.inner_rect);
                painter.rect_filled(visible, 0.0, accent.gamma_multiply(0.25));
                painter.rect_stroke(visible, 0.0, egui::Stroke::new(1.0_f32, accent), egui::StrokeKind::Inside);
            }
            // Righe toccate dal rettangolo (in verticale: le righe occupano tutta la larghezza)
            let first = ((band.top() - content_top) / row_stride).floor().max(0.0) as usize;
            let last = ((band.bottom() - content_top) / row_stride).floor();
            let mut nodes: Vec<u32> = if last < 0.0 || first >= rows.len() {
                Vec::new()
            } else {
                let last = (last as usize).min(rows.len() - 1);
                // Fuori dalla riga (nello spazio tra due righe) non conta
                (first..=last)
                    .filter(|&i| band.bottom() >= content_top + i as f32 * row_stride && band.top() <= content_top + i as f32 * row_stride + row_height)
                    .map(|i| rows[i].0)
                    .collect()
            };
            let mut sorted = nodes.clone();
            sorted.sort_unstable();
            if sorted != selection {
                match nodes.len() {
                    0 => actions.push(Action::Select { node: None, reveal: false }),
                    _ => {
                        let primary = nodes[0];
                        actions.push(Action::SelectMany { nodes: std::mem::take(&mut nodes), primary, anchor: primary });
                    }
                }
            }
            // Vicino ai bordi l'albero scorre
            if pos.y > output.inner_rect.bottom() - 8.0 || pos.y < output.inner_rect.top() + 8.0 {
                ui.ctx().request_repaint();
            }
        }
        // Selezione trascinando: finisce quando si lascia il tasto
        if !ui.input(|i| i.pointer.primary_down()) {
            app.ui_state.tree_sweep = None;
            app.ui_state.tree_band = None;
        }

        // Trascinamento in corso: cosa si sta spostando, accanto al puntatore
        if let (Some(dragged), Some(pos)) =
            (egui::DragAndDrop::payload::<DraggedNodes>(ui.ctx()), ui.ctx().pointer_latest_pos())
        {
            let text = match dragged.0.as_slice() {
                [one] => format!("{} Sposta «{}» in un gruppo", icon::ARROW_BEND_DOWN_RIGHT, scene.display_name(*one)),
                many => format!("{} Sposta {} elementi in un gruppo", icon::ARROW_BEND_DOWN_RIGHT, many.len()),
            };
            egui::Area::new(egui::Id::new("albero_trascina"))
                .fixed_pos(pos + egui::vec2(16.0, 12.0))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(text);
                    });
                });
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    for a in actions {
        app.apply(a);
    }
}

/// Nodi trascinati nell'albero (tutta la selezione, se la riga ne fa parte).
#[derive(Debug, Clone, PartialEq)]
pub struct DraggedNodes(pub Vec<u32>);

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
    // Propaga agli antenati (un nodo spostato nell'albero può avere un numero minore del padre)
    for i in 0..n {
        if !include[i] || scene.info[i].deleted {
            continue;
        }
        let mut p = scene.info[i].parent;
        while p != crate::scene_graph::NO_PARENT && !include[p as usize] {
            include[p as usize] = true;
            p = scene.info[p as usize].parent;
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
    use crate::app::{Action, GestoreApp};
    use crate::ui::tests::frame;

    /// Due file aperti, con tutto l'albero espanso.
    fn app_two_files() -> GestoreApp {
        let mut app = GestoreApp::new_headless();
        let scene = crate::scene_graph::node::multi_file_tests::two_files();
        app.project.on_loaded(scene.revision);
        app.ui_state.expanded = (0..scene.nodes.len() as u32).collect();
        app.scene_graph = Some(scene);
        app
    }

    fn node(app: &GestoreApp, name: &str) -> u32 {
        let s = app.scene_graph.as_ref().unwrap();
        (0..s.nodes.len() as u32).find(|&n| !s.info[n as usize].deleted && s.display_name(n) == name).unwrap()
    }

    /// Trascina con il mouse la riga `from` sulla riga `to`.
    fn drag(ctx: &egui::Context, app: &mut GestoreApp, from: &str, to: &str) -> Vec<String> {
        // Prima che l'albero abbia finito di scorrere (es. fino alla selezione) le righe si muovono
        frame(ctx, app, vec![]);
        let texts = frame(ctx, app, vec![]);
        let row = |name: &str| {
            texts
                .iter()
                .find(|(t, _)| t.ends_with(&format!(" {name}")))
                .map(|(_, p)| *p + egui::vec2(12.0, 6.0))
                .unwrap_or_else(|| panic!("riga {name} non disegnata"))
        };
        let (a, b) = (row(from), row(to));
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(ctx, app, vec![egui::Event::PointerMoved(a)]);
        frame(ctx, app, vec![button(a, true)]);
        let mut during = Vec::new();
        for k in 1..=6 {
            let t = k as f32 / 6.0;
            let texts = frame(ctx, app, vec![egui::Event::PointerMoved(a + (b - a) * t)]);
            during.extend(texts.into_iter().map(|t| t.0));
        }
        frame(ctx, app, vec![button(b, false)]);
        frame(ctx, app, vec![]);
        during
    }

    /// Trascinando una riga su un gruppo dello stesso file il pezzo ci va dentro (un Annulla lo
    /// rimette); su un altro file no.
    #[test]
    fn dragging_a_row_moves_the_piece_into_the_group() {
        let mut app = app_two_files();
        let ctx = egui::Context::default();
        let (trave, telaio) = (node(&app, "Trave"), node(&app, "Telaio"));
        let root_a = app.scene_graph.as_ref().unwrap().sources[0].node;
        // Si trascina una riga selezionata (da una non selezionata si seleziona)
        app.ui_state.select_single(Some(trave));
        let during = drag(&ctx, &mut app, "Trave", "a.wrl");
        assert!(during.iter().any(|t| t.contains("Sposta «Trave»")), "etichetta durante il trascinamento");
        let s = app.scene_graph.as_ref().unwrap();
        assert_eq!(s.parent_of(trave), Some(root_a), "{}", app.ui_state.status_message);
        assert_eq!(app.ui_state.status_message, "Spostato in «a.wrl»");
        assert!(app.is_dirty());

        app.undo();
        assert_eq!(app.scene_graph.as_ref().unwrap().parent_of(trave), Some(telaio));

        // In un altro file: rifiutato, niente da annullare
        let undo = app.project.undo.len();
        app.ui_state.select_single(Some(node(&app, "Pannello")));
        drag(&ctx, &mut app, "Pannello", "Telaio");
        assert!(app.ui_state.status_message.starts_with("Non spostato"), "{}", app.ui_state.status_message);
        assert_eq!(app.project.undo.len(), undo);
    }

    /// Trascinando una riga selezionata si sposta tutta la selezione; lasciandola su un pezzo
    /// finisce nel gruppo del pezzo. L'export riaperto ha lo stesso albero.
    #[test]
    fn dragging_the_selection_onto_a_piece_and_exporting() {
        let mut app = app_two_files();
        let ctx = egui::Context::default();
        let (trave, copia) = (node(&app, "Trave"), node(&app, "Copia"));
        app.apply(Action::NewGroup(vec![trave]));
        let lato = app.ui_state.selected_node.unwrap();
        app.apply(Action::Rename(lato, "Lato".into()));
        app.ui_state.renaming = None;
        app.ui_state.status_message.clear();
        app.ui_state.expanded.insert(lato);
        // Selezionati Copia e Trave (Trave è già nel gruppo e resta dov'è)
        app.ui_state.select_single(Some(copia));
        app.ui_state.selected_extra.insert(trave);

        let during = drag(&ctx, &mut app, "Copia", "Lato");
        assert!(during.iter().any(|t| t.contains("Sposta 2 elementi")), "etichetta della selezione");
        let s = app.scene_graph.as_ref().unwrap();
        assert_eq!(s.parent_of(copia), Some(lato), "{}", app.ui_state.status_message);
        assert_eq!(s.parent_of(trave), Some(lato));

        // Su un pezzo (non selezionato): nel gruppo del pezzo
        app.undo();
        assert_eq!(app.scene_graph.as_ref().unwrap().parent_of(copia), Some(node(&app, "Telaio")));
        app.ui_state.select_single(Some(copia));
        drag(&ctx, &mut app, "Copia", "Trave");
        let s = app.scene_graph.as_ref().unwrap();
        assert_eq!(s.parent_of(copia), Some(lato), "{}", app.ui_state.status_message);

        let path = std::env::temp_dir().join("gestore_albero_trascinato.wrl");
        crate::exporter::export_with(s, &crate::exporter::Scope::Visible, &path, Default::default()).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        let _ = std::fs::remove_file(&path);
        let find = |name: &str| (0..again.nodes.len() as u32).find(|&n| again.display_name(n) == name).unwrap();
        let lato = find("Lato");
        assert_eq!(again.parent_of(find("Trave")), Some(lato));
        assert_eq!(again.parent_of(find("Copia")), Some(lato));
        assert_eq!(again.display_name(again.parent_of(lato).unwrap()), "Telaio");
    }

    /// Trascinando da una riga non selezionata si selezionano le righe su cui si passa (anche
    /// tornando indietro); niente si sposta.
    #[test]
    fn dragging_from_an_unselected_row_selects_the_rows() {
        let mut app = app_two_files();
        let ctx = egui::Context::default();
        let before = state_of(&app);
        drag(&ctx, &mut app, "Telaio", "Pannello");
        let names = |app: &GestoreApp| -> Vec<String> {
            let s = app.scene_graph.as_ref().unwrap();
            app.ui_state.selected_nodes().iter().map(|&n| s.display_name(n)).collect()
        };
        let selected = names(&app);
        for name in ["Telaio", "Trave", "Copia", "b.wrl", "Pannello"] {
            assert!(selected.iter().any(|n| n == name), "{name} non selezionato: {selected:?}");
        }
        assert_eq!(state_of(&app), before, "niente spostato");
        assert!(app.ui_state.tree_sweep.is_none(), "finita al rilascio");
        // Al contrario, dal basso verso l'alto
        app.ui_state.select_single(None);
        drag(&ctx, &mut app, "Copia", "Trave");
        let selected = names(&app);
        assert_eq!(selected.len(), 2, "{selected:?}");
    }

    /// Come in FreeCAD: dallo spazio vuoto sotto le righe si trascina un rettangolo che seleziona
    /// le righe toccate; un clic nello spazio vuoto toglie la selezione.
    #[test]
    fn band_from_the_empty_space_selects_rows() {
        let mut app = app_two_files();
        let ctx = egui::Context::default();
        let before = state_of(&app);
        frame(&ctx, &mut app, vec![]);
        let texts = frame(&ctx, &mut app, vec![]);
        let row = |name: &str| texts.iter().find(|(t, _)| t.ends_with(&format!(" {name}"))).map(|(_, p)| *p).unwrap();
        let pannello = row("Pannello");
        // Spazio vuoto: sotto l'ultima riga, a destra dei nomi
        let start = pannello + egui::vec2(150.0, 120.0);
        let end = row("Trave") + egui::vec2(150.0, 6.0);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);
        frame(&ctx, &mut app, vec![button(start, true)]);
        for k in 1..=6 {
            frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start + (end - start) * (k as f32 / 6.0))]);
        }
        let s = app.scene_graph.as_ref().unwrap();
        let selected: Vec<String> = app.ui_state.selected_nodes().iter().map(|&n| s.display_name(n)).collect();
        for name in ["Trave", "Copia", "b.wrl", "Pannello"] {
            assert!(selected.iter().any(|n| n == name), "{name} non selezionato: {selected:?}");
        }
        assert!(!selected.iter().any(|n| n == "Telaio"), "sopra il rettangolo: {selected:?}");
        frame(&ctx, &mut app, vec![button(end, false)]);
        assert!(app.ui_state.tree_band.is_none());
        assert_eq!(state_of(&app), before, "niente spostato");

        // Clic nello spazio vuoto: niente selezionato
        for events in [vec![egui::Event::PointerMoved(start)], vec![button(start, true)], vec![button(start, false)], vec![]] {
            frame(&ctx, &mut app, events);
        }
        assert!(app.ui_state.selected_nodes().is_empty());
    }

    /// Padre di ogni nodo: per controllare che niente si sia spostato nell'albero.
    fn state_of(app: &GestoreApp) -> Vec<Option<u32>> {
        let s = app.scene_graph.as_ref().unwrap();
        (0..s.nodes.len() as u32).map(|n| s.parent_of(n)).collect()
    }

    /// «Nuovo gruppo con la selezione»: gruppo nel gruppo comune, rinomina aperta, un Annulla.
    #[test]
    fn new_group_collects_the_selection() {
        let mut app = app_two_files();
        let (trave, copia, telaio) = (node(&app, "Trave"), node(&app, "Copia"), node(&app, "Telaio"));
        app.apply(Action::NewGroup(vec![trave, copia]));
        let group = app.ui_state.selected_node.expect("gruppo selezionato");
        let s = app.scene_graph.as_ref().unwrap();
        assert_eq!(s.parent_of(group), Some(telaio));
        assert_eq!(s.nodes[group as usize].children(), &[trave, copia]);
        assert_eq!(app.ui_state.renaming.as_ref().map(|r| r.node), Some(group), "rinomina aperta");
        app.apply(Action::Rename(group, "Lato".into()));
        assert_eq!(app.scene_graph.as_ref().unwrap().display_name(group), "Lato");
        app.undo();
        app.undo();
        let s = app.scene_graph.as_ref().unwrap();
        assert!(s.info[group as usize].deleted);
        assert_eq!(s.parent_of(trave), Some(telaio));
    }

    /// «Nuovo gruppo vuoto»: in fondo al file (o al gruppo del pezzo cliccato), rinomina aperta;
    /// ci si sposta dentro un pezzo; un Annulla lo toglie.
    #[test]
    fn new_empty_group_in_a_file_or_beside_a_piece() {
        let mut app = app_two_files();
        let (trave, telaio) = (node(&app, "Trave"), node(&app, "Telaio"));
        let root_a = app.scene_graph.as_ref().unwrap().sources[0].node;
        app.apply(Action::NewEmptyGroup(root_a));
        let group = app.ui_state.selected_node.expect("gruppo selezionato");
        {
            let s = app.scene_graph.as_ref().unwrap();
            assert_eq!(s.parent_of(group), Some(root_a));
            assert_eq!(s.nodes[root_a as usize].children().last(), Some(&group), "in fondo al file");
            assert!(s.nodes[group as usize].children().is_empty());
            assert_eq!(s.display_name(group), "Nuovo gruppo");
        }
        assert_eq!(app.ui_state.renaming.as_ref().map(|r| r.node), Some(group), "rinomina aperta");
        app.apply(Action::MoveInto { nodes: vec![trave], into: group });
        assert_eq!(app.scene_graph.as_ref().unwrap().parent_of(trave), Some(group));
        app.undo();
        app.undo();
        let s = app.scene_graph.as_ref().unwrap();
        assert!(s.info[group as usize].deleted);
        assert_eq!(s.parent_of(trave), Some(telaio));

        // Su un pezzo: nel suo gruppo
        app.apply(Action::NewEmptyGroup(trave));
        let beside = app.ui_state.selected_node.unwrap();
        assert_eq!(app.scene_graph.as_ref().unwrap().parent_of(beside), Some(telaio));
    }

    /// La ricerca mostra un pezzo spostato sotto il suo nuovo gruppo.
    #[test]
    fn search_follows_moved_pieces() {
        let mut scene = crate::scene_graph::node::multi_file_tests::two_files();
        let trave = (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == "Trave").unwrap();
        let root_a = scene.sources[0].node;
        let group = scene.new_group(&[root_a, trave], "Nuovo").err();
        assert!(group.is_some(), "un file non si raggruppa");
        let group = scene.new_group(&[trave], "Nuovo").unwrap();
        let (rows, n) = super::search_rows(&scene, "trave");
        assert_eq!(n, 1);
        let names: Vec<String> = rows.iter().map(|&(i, _)| scene.display_name(i)).collect();
        assert_eq!(names, ["a.wrl", "Telaio", "Nuovo", "Trave"]);
        assert!(rows.iter().any(|r| r.0 == group));
    }

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
    /// Colori salvati dall'utente, per il menu Colore
    saved_colors: Vec<[f32; 3]>,
    /// Ci sono pezzi copiati da incollare
    can_paste: bool,
    actions: &'a mut Vec<Action>,
    /// Selezione trascinando in corso: riga di partenza
    sweep: &'a mut Option<u32>,
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

    let row = ui.horizontal(|ui| {
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
        // Parte di una cerniera salvata (solo il nodo scelto come parte, non i pezzi dentro)
        let hinges: Vec<&str> =
            scene.live_hinges().filter(|(_, h)| h.nodes.contains(&node_idx)).map(|(_, h)| h.name.as_str()).collect();
        let hinge_mark = if hinges.is_empty() { String::new() } else { format!("  {}", icon::DOOR_OPEN) };
        let mut text = egui::RichText::new(format!("{icon} {label}{hinge_mark}"));
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
        // Clic per selezionare. Trascinando da una riga selezionata la selezione si sposta dentro
        // un altro gruppo; da una riga non selezionata si selezionano le righe su cui si passa
        let mut resp = ui.add(egui::SelectableLabel::new(is_selected, text)).interact(egui::Sense::click_and_drag());
        if !hinges.is_empty() {
            resp = resp.on_hover_text(format!("Cerniera: {}", hinges.join(", ")));
        }
        let movable = is_selected && info.kind != NodeKind::File;
        if movable && ctx.sweep.is_none() {
            resp.dnd_set_drag_payload(DraggedNodes(ctx.selection.to_vec()));
        } else if resp.drag_started() && ctx.sweep.is_none() {
            *ctx.sweep = Some(node_idx);
            ctx.actions.push(Action::Select { node: Some(node_idx), reveal: false });
        }
        if let (Some(start), Some(pos)) = (*ctx.sweep, ui.ctx().pointer_latest_pos()) {
            let over = ui.max_rect().y_range().contains(pos.y) && resp.rect.y_range().contains(pos.y);
            if over && ui.input(|i| i.pointer.primary_down()) {
                if let Some(nodes) = range_between(ctx.rows, start, node_idx) {
                    let mut sorted = nodes.clone();
                    sorted.sort_unstable();
                    if sorted.as_slice() != ctx.selection {
                        ctx.actions.push(Action::SelectMany { nodes, primary: node_idx, anchor: start });
                    }
                }
            }
        }

        if has_children {
            ui.label(egui::RichText::new(format!("({})", info.shape_count)).small().weak());
        }
        if info.kind == NodeKind::File {
            if let Some(step) = scene.sources.iter().find(|s| s.node == node_idx).and_then(|s| s.step) {
                ui.label(egui::RichText::new(format!("STEP · {}", step.describe())).small().weak());
            }
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
        let saved_colors = &ctx.saved_colors[..];
        let can_paste = ctx.can_paste;
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
                (icon::EYE_SLASH, "Nascondi    Spazio", on_targets(Action::Hide))
            } else {
                (icon::EYE, "Mostra    Spazio", on_targets(Action::Show))
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
            if ui.button(icon_text(ui, icon::COPY, "Copia    Ctrl+C")).clicked() {
                actions.push(Action::Copy(group.map_or_else(|| vec![node_idx], <[u32]>::to_vec)));
                ui.close_menu();
            }
            if ui.add_enabled(can_paste, egui::Button::new(icon_text(ui, icon::CLIPBOARD, "Incolla    Ctrl+V"))).clicked() {
                actions.push(Action::Paste);
                ui.close_menu();
            }
            if info.kind != NodeKind::File
                && ui
                    .button(icon_text(ui, icon::FOLDER_SIMPLE_PLUS, "Nuovo gruppo con la selezione"))
                    .on_hover_text("Raccoglie gli elementi in un gruppo nuovo, nello stesso file. Si possono anche trascinare le righe dentro un gruppo.")
                    .clicked()
            {
                actions.push(Action::NewGroup(group.map_or_else(|| vec![node_idx], <[u32]>::to_vec)));
                ui.close_menu();
            }
            if group.is_none()
                && ui
                    .button(icon_text(ui, icon::FOLDER_SIMPLE_PLUS, "Nuovo gruppo vuoto"))
                    .on_hover_text(if info.kind == NodeKind::Shape {
                        "Aggiunge un gruppo vuoto nel gruppo di questo pezzo: poi ci si trascinano dentro le righe"
                    } else {
                        "Aggiunge un gruppo vuoto qui dentro: poi ci si trascinano dentro le righe"
                    })
                    .clicked()
            {
                actions.push(Action::NewEmptyGroup(node_idx));
                ui.close_menu();
            }
            ui.separator();
            if ui.button(icon_text(ui, icon::PENCIL_SIMPLE, "Rinomina    F2")).clicked() {
                actions.push(Action::StartRename(node_idx));
                ui.close_menu();
            }
            ui.menu_button(icon_text(ui, icon::PALETTE, "Colore"), |ui| {
                let mut chosen = Vec::new();
                crate::ui::color_menu::color_menu(ui, scene, node_idx, default_color, saved_colors, &mut chosen);
                actions.extend(crate::ui::color_menu::spread_colors(chosen, group.unwrap_or(&[])));
            });
            ui.separator();
            if ui
                .button(icon_text(ui, icon::EXPORT, "Esporta selezionati…    Ctrl+Maiusc+E"))
                .on_hover_text("Salva solo le parti visibili degli elementi selezionati, con i gruppi che li contengono")
                .clicked()
            {
                actions.push(Action::ExportSelected(group.map_or_else(|| vec![node_idx], <[u32]>::to_vec)));
                ui.close_menu();
            }
            let is_file = info.kind == NodeKind::File;
            if is_file && group.is_none() {
                if ui.button(icon_text(ui, icon::EXPORT, "Esporta questo file…")).clicked() {
                    actions.push(Action::ExportNode(node_idx));
                    ui.close_menu();
                }
                if scene.sources.iter().any(|s| s.node == node_idx && s.step.is_some()) {
                    if ui.button(icon_text(ui, icon::CUBE, "Converti in mesh…")).clicked() {
                        actions.push(Action::RemeshStep(node_idx));
                        ui.close_menu();
                    }
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
    })
    .response;

    // Destinazione di un trascinamento: la riga si evidenzia, al rilascio si sposta
    if let Some(dragged) = row.dnd_hover_payload::<DraggedNodes>() {
        if !dragged.0.contains(&node_idx) {
            let stroke = egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color);
            ui.painter().rect_stroke(row.rect.expand(1.0), 3.0, stroke, egui::StrokeKind::Inside);
        }
    }
    if let Some(dragged) = row.dnd_release_payload::<DraggedNodes>() {
        if !dragged.0.contains(&node_idx) {
            ctx.actions.push(Action::MoveInto { nodes: dragged.0.clone(), into: node_idx });
        }
    }
}
