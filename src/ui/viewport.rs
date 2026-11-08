use super::theme::{self, icon, icon_text};
use crate::app::Action;
use crate::renderer::camera::ArcballCamera;
use crate::scene_graph::SceneGraph;
use crate::settings::RotationMode;

/// Riferimento fissato all'inizio di un trascinamento nella viewport.
#[derive(Debug, Clone, Copy)]
pub struct DragState {
    /// Punto attorno a cui si ruota, e che segue il mouse nello spostamento
    pivot: glam::Vec3,
    /// Vista capovolta all'inizio del trascinamento (per la rotazione ad asse fisso)
    upside_down: bool,
}

/// Punto di riferimento dello zoom con la rotella. Il picking costa sui file grandi: si riusa
/// tra uno scatto e l'altro finché mouse e vista restano quelli lasciati dallo scatto precedente.
#[derive(Debug, Clone, Copy)]
pub struct ZoomAnchor {
    cursor: egui::Pos2,
    target: glam::Vec3,
    orientation: glam::Quat,
    point: glam::Vec3,
}

/// Raggio della camera che passa per una posizione del mouse.
fn pointer_ray(camera: &ArcballCamera, rect: egui::Rect, pos: egui::Pos2) -> (glam::Vec3, glam::Vec3) {
    let ndc_x = (pos.x - rect.min.x) / rect.width() * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y - rect.min.y) / rect.height() * 2.0;
    camera.ray(ndc_x, ndc_y)
}

/// Punto del pezzo visibile sotto il raggio, se c'è (oltre l'eventuale piano di sezione).
fn surface_point(
    scene: &SceneGraph,
    visibility: &[bool],
    section: Option<crate::renderer::Section>,
    (origin, dir): (glam::Vec3, glam::Vec3),
) -> Option<glam::Vec3> {
    pick(scene, visibility, section, (origin, dir)).map(|(_, t, _)| origin + dir * t)
}

/// Picking che ignora la parte tagliata dal piano di sezione.
fn pick(
    scene: &SceneGraph,
    visibility: &[bool],
    section: Option<crate::renderer::Section>,
    (origin, dir): (glam::Vec3, glam::Vec3),
) -> Option<(u32, f32, [glam::Vec3; 3])> {
    scene.pick_triangle_kept(origin, dir, visibility, |p| section.is_none_or(|s| s.keeps(p)))
}

/// Disegna il viewport 3D interattivo nativo con pipeline wgpu.
pub fn draw_viewport(ui: &mut egui::Ui, app: &mut crate::app::GestoreApp) {
    let available_size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available_size, egui::Sense::click_and_drag());

    let mut actions: Vec<Action> = Vec::new();
    let mut eyedropper_message: Option<String> = None;

    // ── 1. Gestione interazione telecamera con mouse / trackpad ──────────────────
    let section = app.ui_state.section;
    if let Some(renderer) = &mut app.renderer {
        renderer.section = section;
        let (left_down, middle_down) = ui.input(|i| {
            (
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Middle),
            )
        });

        let world_up = app.settings.axes.up();

        // Il perno è sempre dove si è cliccato: sul pezzo sotto il mouse o, sul vuoto, sul piano
        // focale nello stesso punto dello schermo. La vista non si ricentra mai.
        if response.drag_started() {
            renderer.camera.animation = None; // il mouse prende il controllo della vista
            let press = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
            let ray = press.map(|pos| pointer_ray(&renderer.camera, rect, pos));
            let hit = ray
                .zip(app.scene_graph.as_ref())
                .and_then(|(ray, scene)| surface_point(scene, &renderer.visibility, section, ray));
            let pivot = hit
                .or_else(|| ray.map(|(o, d)| renderer.camera.point_on_target_plane(o, d)))
                .unwrap_or(renderer.camera.target);
            app.ui_state.drag = Some(DragState {
                pivot,
                upside_down: renderer.camera.up().dot(world_up) < 0.0,
            });
        } else if !response.dragged() {
            app.ui_state.drag = None;
        }
        let drag = app.ui_state.drag.unwrap_or(DragState {
            pivot: renderer.camera.target,
            upside_down: false,
        });

        // Modalità zoom: tasto sinistro + rotella premuti insieme.
        // Mouse verso l'alto → zoom in, verso il basso → zoom out.
        let zoom_sign = if app.settings.invert_zoom { -1.0 } else { 1.0 };
        if response.dragged() && left_down && middle_down {
            let delta = response.drag_delta();
            renderer.camera.dolly(drag.pivot, delta.y * zoom_sign);
            ui.ctx().request_repaint();
        }
        // Rotazione (tasto sinistro trascinato)
        // Shift + tasto sinistro → rollio attorno all'asse di vista
        else if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let sensitivity = app.settings.rotation_sensitivity;
            let per_point = 0.004 * sensitivity;
            let (dx, dy) = (-delta.x * per_point, delta.y * per_point);
            if ui.input(|i| i.modifiers.shift) {
                renderer.camera.roll_about(drag.pivot, delta.x * per_point);
            } else {
                match app.settings.rotation {
                    RotationMode::Trackball => {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let ndc = |p: egui::Pos2| {
                                glam::vec2(
                                    (p.x - rect.min.x) / rect.width() * 2.0 - 1.0,
                                    1.0 - (p.y - rect.min.y) / rect.height() * 2.0,
                                )
                            };
                            renderer.camera.orbit_trackball(drag.pivot, ndc(pos - delta), ndc(pos), sensitivity);
                        }
                    }
                    RotationMode::FixedAxis => renderer.camera.orbit_fixed_axis(
                        drag.pivot,
                        world_up,
                        drag.upside_down,
                        dx,
                        dy,
                    ),
                }
            }
            ui.ctx().request_repaint();
        }

        // Pan traslazione (tasto destro o centrale trascinato, senza il sinistro)
        else if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let delta = response.drag_delta();
            renderer.camera.pan_at(drag.pivot, delta.x, delta.y, rect.height());
            ui.ctx().request_repaint();
        }

        // Zoom con la rotella, verso il punto sotto il mouse
        if let Some(cursor) = response.hover_pos() {
            let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
            if scroll_delta != 0.0 {
                let camera = &renderer.camera;
                let reusable = app.ui_state.zoom_anchor.filter(|a| {
                    a.cursor == cursor
                        && a.target == camera.target
                        && a.orientation == camera.orientation
                });
                let point = reusable.map(|a| a.point).unwrap_or_else(|| {
                    let ray = pointer_ray(camera, rect, cursor);
                    app.scene_graph
                        .as_ref()
                        .and_then(|scene| surface_point(scene, &renderer.visibility, section, ray))
                        .unwrap_or_else(|| camera.point_on_target_plane(ray.0, ray.1))
                });
                renderer.camera.zoom_about(point, (-scroll_delta * zoom_sign * 0.002).exp());
                app.ui_state.zoom_anchor = Some(ZoomAnchor {
                    cursor,
                    target: renderer.camera.target,
                    orientation: renderer.camera.orientation,
                    point,
                });
                ui.ctx().request_repaint();
            }
        }

        // Click singolo: selezione 3D tramite ray picking
        //  - click        → seleziona il macro-oggetto che contiene il pezzo
        //  - Ctrl + click → seleziona il singolo pezzo
        //  - click di nuovo sul macro già selezionato → scende al singolo pezzo
        //  - click sul vuoto → deseleziona
        // (il secondo click di un doppio click non cambia la selezione)
        // Strumento di misura: il clic prende un punto del pezzo (agganciato al vertice vicino)
        let measuring = app.ui_state.measure.is_some();
        if measuring && response.clicked() {
            if let (Some(scene), Some(pos)) = (&app.scene_graph, response.interact_pointer_pos()) {
                renderer.sync_with_scene(scene, &app.ui_state.selected_nodes());
                let (origin, dir) = pointer_ray(&renderer.camera, rect, pos);
                if let Some((_, t, triangle)) = pick(scene, &renderer.visibility, section, (origin, dir)) {
                    let camera = &renderer.camera;
                    let point = super::measure::snap(triangle, origin + dir * t, |v| to_screen(camera, rect, v), pos);
                    if let Some(m) = &mut app.ui_state.measure {
                        m.add_point(point);
                    }
                }
            }
        }

        // Pipetta: il clic legge il colore del pezzo e lo dà alla selezione (o lo salva)
        let picking_color = app.ui_state.eyedropper && !measuring;
        if picking_color && response.clicked() {
            if let (Some(scene), Some(pos)) = (&app.scene_graph, response.interact_pointer_pos()) {
                renderer.sync_with_scene(scene, &app.ui_state.selected_nodes());
                let ray = pointer_ray(&renderer.camera, rect, pos);
                if let Some((mesh, _, _)) = pick(scene, &renderer.visibility, section, ray) {
                    let color = super::color_menu::mesh_color(scene, mesh, app.settings.default_color);
                    let name = scene.mesh_to_node.get(mesh as usize).map(|&n| scene.display_name(n)).unwrap_or_default();
                    let selection = app.ui_state.selected_nodes();
                    if selection.is_empty() {
                        actions.push(Action::SaveColor([color[0], color[1], color[2]]));
                        eyedropper_message = Some(format!("Colore di {name} salvato tra i miei colori"));
                    } else {
                        let count = selection.len();
                        actions.push(Action::Batch(selection.into_iter().map(|n| Action::SetColor(n, Some(color))).collect()));
                        eyedropper_message = Some(format!("Colore di {name} dato a {count} elementi"));
                    }
                    app.ui_state.eyedropper = false;
                }
            }
        }

        if !measuring && !picking_color && response.clicked() && !response.double_clicked() {
            if let (Some(scene), Some(pos)) = (&app.scene_graph, response.interact_pointer_pos()) {
                // Allinea i flag prima del picking (le mesh nascoste non sono selezionabili)
                renderer.sync_with_scene(scene, &app.ui_state.selected_nodes());

                let (origin, dir) = pointer_ray(&renderer.camera, rect, pos);

                let picked_node = pick(scene, &renderer.visibility, section, (origin, dir))
                    .and_then(|(mesh, _, _)| scene.mesh_to_node.get(mesh as usize).copied())
                    .filter(|&n| n != crate::scene_graph::NO_PARENT);

                let target = picked_node.map(|shape_node| {
                    let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                    let macro_node = scene.parent_of(shape_node).unwrap_or(shape_node);
                    if ctrl || app.ui_state.selected_node == Some(macro_node) {
                        shape_node
                    } else {
                        macro_node
                    }
                });
                actions.push(Action::Select { node: target, reveal: true });
            }
        }

        // Doppio click: inquadra la selezione (o tutte le parti visibili)
        if response.double_clicked() && !measuring {
            actions.push(app.ui_state.focus_action());
        }

        // Allinea visibilità/selezione GPU allo stato della scena (costo solo se cambiato)
        if let Some(scene) = &app.scene_graph {
            renderer.sync_with_scene(scene, &app.ui_state.selected_nodes());
        }
    }

    // Le azioni modificano scena/camera: applicale prima del rendering del frame
    for a in actions.drain(..) {
        app.apply(a);
    }
    if let Some(m) = eyedropper_message {
        app.ui_state.status_message = m;
    }
    if let (Some(renderer), Some(scene)) = (&mut app.renderer, &app.scene_graph) {
        renderer.sync_with_scene(scene, &app.ui_state.selected_nodes());
    }

    // Passaggio animato verso una vista (viste standard, cubo di orientamento)
    if let Some(renderer) = &mut app.renderer {
        if renderer.camera.update_animation(std::time::Instant::now()) {
            ui.ctx().request_repaint();
        }
    }

    // ── 2. Rendering wgpu della scena 3D ─────────────────────────────────────────
    let mut rendered_texture_id = None;

    if let (Some(renderer), Some(rs)) = (&mut app.renderer, &app.wgpu_render_state) {
        let ppp = ui.ctx().pixels_per_point();
        let pixel_width = (rect.width() * ppp).round() as u32;
        let pixel_height = (rect.height() * ppp).round() as u32;

        let mut egui_renderer = rs.renderer.write();
        rendered_texture_id = renderer.render_frame(
            &rs.device,
            &rs.queue,
            &mut egui_renderer,
            pixel_width,
            pixel_height,
        );
    }

    // ── 3. Visualizzazione a schermo del frame renderizzato ──────────────────────
    if let Some(texture_id) = rendered_texture_id {
        ui.painter().image(
            texture_id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        // Ripiego se il backend wgpu non è disponibile (es. test headless)
        let [r, g, b] = app.settings.background;
        ui.painter().rect_filled(rect, 0.0, crate::ui::color_menu::to_color32([r, g, b, 1.0]));
    }

    // Terna degli assi e centro di rotazione: i riferimenti per orientarsi mentre si ruota
    if let (Some(renderer), true) = (&app.renderer, app.scene_graph.is_some()) {
        let camera = &renderer.camera;
        draw_axes(ui.painter(), rect, camera);

        let rotating = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary))
            && !ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        if let (Some(drag), true) = (app.ui_state.drag, rotating) {
            let p = camera.view_proj().project_point3(drag.pivot);
            let pos = rect.min
                + egui::vec2((p.x + 1.0) * 0.5 * rect.width(), (1.0 - p.y) * 0.5 * rect.height());
            if rect.contains(pos) {
                let painter = ui.painter();
                painter.circle_filled(pos, 4.0, egui::Color32::from_rgb(255, 170, 40));
                painter.circle_stroke(pos, 5.0, egui::Stroke::new(1.0_f32, egui::Color32::BLACK));
            }
        }
    }

    // Piano di sezione: contorno del taglio sull'ingombro della scena e comandi
    if let (Some(renderer), Some(scene)) = (&app.renderer, &app.scene_graph) {
        if let Some(s) = &mut app.ui_state.section {
            let (main, _) = scene.main_visible_bounding_box();
            let bbox = if main.is_valid() { main } else { scene.compute_bounding_box() };
            if bbox.is_valid() {
                let camera = &renderer.camera;
                draw_section_outline(ui, rect, s, &bbox, |v| to_screen(camera, rect, v));
                if !draw_section_controls(ui, rect, s, &bbox) {
                    app.ui_state.section = None;
                }
            }
        }
    }

    // Misura in corso: punti, segmento e quota
    if let (Some(renderer), Some(m), Some(scene)) = (&app.renderer, &app.ui_state.measure, &app.scene_graph) {
        let camera = &renderer.camera;
        let size = scene.compute_bounding_box().diagonal();
        super::measure::draw(ui, rect, m, |v| to_screen(camera, rect, v), size);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if app.ui_state.eyedropper && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }

    // ── 4. Comandi sopra la vista ────────────────────────────────────────────────
    let dragging_file = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    if dragging_file {
        ui.painter().rect_stroke(
            rect.shrink(3.0),
            6.0,
            egui::Stroke::new(3.0_f32, theme::accent_color(ui)),
            egui::StrokeKind::Inside,
        );
    }
    if app.scene_graph.is_none() && !app.ui_state.is_loading && !app.project.busy() {
        draw_empty_state(ui, rect, app.settings.background, dragging_file);
    }
    if app.scene_graph.is_some() {
        let start = app.scene_graph.as_ref().map(default_section);
        let mut settings = app.settings.clone();
        let ui_state = &mut app.ui_state;
        let tools = ViewTools {
            measure: &mut ui_state.measure,
            eyedropper: &mut ui_state.eyedropper,
            section: &mut ui_state.section,
            invert_zoom: &mut settings.invert_zoom,
        };
        draw_view_controls(ui, rect, &mut actions, tools, start.flatten());
        app.apply_settings(settings);
        if let Some(renderer) = &app.renderer {
            actions.extend(super::view_cube::draw(ui, rect, &renderer.camera, app.settings.axes));
        }
    }

    for a in actions {
        app.apply(a);
    }
}

/// Sezione iniziale: perpendicolare a X (in coordinate veicolo, il taglio trasversale della
/// scocca) a metà dell'ingombro.
fn default_section(scene: &SceneGraph) -> Option<crate::renderer::Section> {
    let (main, _) = scene.main_visible_bounding_box();
    let bbox = if main.is_valid() { main } else { scene.compute_bounding_box() };
    bbox.is_valid().then(|| crate::renderer::Section { axis: 0, position: bbox.center().x, flip: false })
}

/// Rettangolo del taglio: l'ingombro della scena sezionato dal piano.
fn draw_section_outline(
    ui: &egui::Ui,
    rect: egui::Rect,
    s: &crate::renderer::Section,
    bbox: &crate::scene_graph::Aabb,
    to_screen: impl Fn(glam::Vec3) -> Option<egui::Pos2>,
) {
    let (a, b) = ((s.axis + 1) % 3, (s.axis + 2) % 3);
    let corner = |u: f32, v: f32| {
        let mut p = glam::Vec3::ZERO;
        p[s.axis] = s.position;
        p[a] = u;
        p[b] = v;
        to_screen(p)
    };
    let points: Option<Vec<egui::Pos2>> = [
        corner(bbox.min[a], bbox.min[b]),
        corner(bbox.max[a], bbox.min[b]),
        corner(bbox.max[a], bbox.max[b]),
        corner(bbox.min[a], bbox.max[b]),
    ]
    .into_iter()
    .collect();
    if let Some(points) = points {
        let color = egui::Color32::from_rgb(255, 214, 10);
        ui.painter_at(rect).add(egui::Shape::closed_line(points, egui::Stroke::new(1.5_f32, color)));
    }
}

/// Riquadro dei comandi della sezione, sotto la barra delle viste. Ritorna false se chiuso.
fn draw_section_controls(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    s: &mut crate::renderer::Section,
    bbox: &crate::scene_graph::Aabb,
) -> bool {
    let area = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 54.0), egui::vec2(rect.width().min(560.0), 40.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Min)));
    let mut open = true;
    theme::overlay_frame(&child).show(&mut child, |ui| {
        theme::flat_buttons(ui);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Sezione").strong());
            for (axis, name) in ["X", "Y", "Z"].iter().enumerate() {
                if ui.selectable_label(s.axis == axis, *name).on_hover_text(format!("Taglio perpendicolare all'asse {name}")).clicked()
                    && s.axis != axis
                {
                    s.axis = axis;
                    s.position = bbox.center()[axis];
                }
            }
            let (lo, hi) = (bbox.min[s.axis], bbox.max[s.axis]);
            ui.spacing_mut().slider_width = 200.0;
            let decimals = if (hi - lo) >= 100.0 { 1 } else { 3 };
            ui.add(egui::Slider::new(&mut s.position, lo..=hi).fixed_decimals(decimals))
                .on_hover_text("Posizione del piano lungo l'asse");
            if ui
                .button(icon_text(ui, icon::ARROWS_LEFT_RIGHT, "Inverti"))
                .on_hover_text("Tiene l'altra metà del modello")
                .clicked()
            {
                s.flip = !s.flip;
            }
            if ui.button(icon::X).on_hover_text("Chiudi la sezione").clicked() {
                open = false;
            }
        });
    });
    open
}

/// Posizione sullo schermo di un punto 3D (None se dietro la camera).
fn to_screen(camera: &ArcballCamera, rect: egui::Rect, p: glam::Vec3) -> Option<egui::Pos2> {
    let clip = camera.view_proj() * p.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let n = clip.truncate() / clip.w;
    Some(rect.min + egui::vec2((n.x + 1.0) * 0.5 * rect.width(), (1.0 - n.y) * 0.5 * rect.height()))
}

/// Invito ad aprire un file, al centro della vista vuota.
fn draw_empty_state(ui: &egui::Ui, rect: egui::Rect, background: [f32; 3], dragging_file: bool) {
    // Lo sfondo della vista è scelto dall'utente: il testo si adatta a quello, non al tema
    let light_background = background.iter().sum::<f32>() / 3.0 > 0.5;
    let color = if light_background {
        egui::Color32::from_rgb(70, 74, 82)
    } else {
        egui::Color32::from_rgb(222, 226, 232)
    };
    let painter = ui.painter();
    let center = rect.center();
    painter.text(
        center - egui::vec2(0.0, 34.0),
        egui::Align2::CENTER_CENTER,
        icon::CUBE,
        egui::FontId::proportional(56.0),
        color,
    );
    let title = if dragging_file { "Rilascia per aprire il file" } else { "Trascina qui un file WRL o JT" };
    painter.text(
        center + egui::vec2(0.0, 18.0),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(18.0),
        color,
    );
    painter.text(
        center + egui::vec2(0.0, 44.0),
        egui::Align2::CENTER_CENTER,
        "oppure premi Ctrl+O",
        egui::FontId::proportional(13.5),
        color,
    );
}

/// Inquadratura e viste in alto a sinistra, guida ai comandi in alto a destra.
/// Strumenti della vista che i comandi sopra la vista accendono e spengono.
struct ViewTools<'a> {
    measure: &'a mut Option<super::measure::Measure>,
    eyedropper: &'a mut bool,
    section: &'a mut Option<crate::renderer::Section>,
    invert_zoom: &'a mut bool,
}

fn draw_view_controls(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    actions: &mut Vec<Action>,
    tools: ViewTools,
    section_start: Option<crate::renderer::Section>,
) {
    let ViewTools { measure, eyedropper, section, invert_zoom } = tools;
    use crate::renderer::camera::StandardView as V;
    let area = rect.shrink(10.0);

    let mut left = ui.new_child(
        egui::UiBuilder::default().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Min)),
    );
    theme::overlay_frame(&left).show(&mut left, |ui| {
        theme::flat_buttons(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui
                .button(icon_text(ui, icon::CORNERS_OUT, "Centra"))
                .on_hover_text("Inquadra le parti visibili  [F = inquadra la selezione]")
                .clicked()
            {
                actions.push(Action::Focus(None));
            }
            ui.separator();
            for (v, name, key) in V::ALL {
                if matches!(v, V::Back | V::Left | V::Bottom) {
                    continue; // le viste opposte sono nel menu Vista e con Ctrl
                }
                if ui.button(v.short()).on_hover_text(format!("Vista {name}  [{key}]")).clicked() {
                    actions.push(Action::View(v));
                }
            }
            if ui
                .button(icon_text(ui, icon::ARROW_UP, ""))
                .on_hover_text("Raddrizza la vista (annulla il rollio)  [5]")
                .clicked()
            {
                actions.push(Action::Straighten);
            }
            ui.separator();
            let active = measure.is_some();
            if ui
                .selectable_label(active, icon_text(ui, icon::RULER, "Misura"))
                .on_hover_text("Distanza tra due punti dei pezzi: clic sul primo e sul secondo punto  [M]")
                .clicked()
            {
                *measure = if active { None } else { Some(Default::default()) };
                *eyedropper = false;
            }
            let cutting = section.is_some();
            if ui
                .selectable_label(cutting, icon_text(ui, icon::SCISSORS, "Sezione"))
                .on_hover_text("Piano di sezione: taglia il modello lungo X, Y o Z")
                .clicked()
            {
                *section = if cutting { None } else { section_start };
            }
            ui.separator();
            let inverted = *invert_zoom;
            if ui
                .selectable_label(inverted, icon_text(ui, icon::ARROWS_DOWN_UP, "Zoom"))
                .on_hover_text(if inverted {
                    "Zoom invertito: rotella in avanti per allontanarsi. Clic per tornare al verso normale"
                } else {
                    "Inverti lo zoom della rotella (e di sinistro + rotella premuti)"
                })
                .clicked()
            {
                *invert_zoom = !inverted;
            }
        });
    });

    let mut right = ui.new_child(
        egui::UiBuilder::default().max_rect(area).layout(egui::Layout::right_to_left(egui::Align::Min)),
    );
    theme::overlay_frame(&right).show(&mut right, |ui| {
        theme::flat_buttons(ui);
        ui.menu_button(icon_text(ui, icon::QUESTION, ""), |ui| {
            ui.label(egui::RichText::new("Comandi del mouse").strong());
            egui::Grid::new("comandi_mouse").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                for (input, what) in [
                    ("Sinistro", "Ruota attorno al punto cliccato"),
                    ("Shift + sinistro", "Rollio"),
                    ("Destro o rotella premuta", "Sposta"),
                    ("Rotella", "Zoom verso il mouse (verso invertibile con il tasto Zoom)"),
                    ("Sinistro + rotella premuti", "Zoom"),
                    ("Click", "Seleziona il macro-oggetto"),
                    ("Ctrl + click", "Seleziona il singolo pezzo"),
                    ("Doppio click", "Inquadra la selezione"),
                ] {
                    ui.label(egui::RichText::new(input).strong());
                    ui.label(what);
                    ui.end_row();
                }
            });
        })
        .response
        .on_hover_text("Comandi del mouse");
    });
}

/// Terna X/Y/Z nell'angolo in basso a sinistra, orientata come il modello.
fn draw_axes(painter: &egui::Painter, rect: egui::Rect, camera: &ArcballCamera) {
    const LENGTH: f32 = 30.0;
    let center = rect.left_bottom() + egui::vec2(48.0, -48.0);
    let mut axes = [
        (glam::Vec3::X, "X", egui::Color32::from_rgb(230, 80, 80)),
        (glam::Vec3::Y, "Y", egui::Color32::from_rgb(90, 200, 90)),
        (glam::Vec3::Z, "Z", egui::Color32::from_rgb(90, 140, 240)),
    ];
    // Prima gli assi che puntano lontano da chi guarda, così quelli vicini restano sopra
    axes.sort_by(|a, b| camera.forward().dot(b.0).total_cmp(&camera.forward().dot(a.0)));
    for (axis, label, color) in axes {
        let tip = center + egui::vec2(camera.right().dot(axis), -camera.up().dot(axis)) * LENGTH;
        painter.line_segment([center, tip], egui::Stroke::new(2.0_f32, color));
        painter.text(
            tip + (tip - center).normalized() * 8.0,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::color_menu::tests::{node, TWO_PARTS};
    use crate::ui::tests::{click, frame};

    /// App con un renderer vero (per il picking), se c'è una scheda video. Chi la usa prende
    /// prima `gpu_lock`, che deve durare più dell'app.
    fn app_with_renderer() -> Option<crate::app::GestoreApp> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
        let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).ok()?;
        let mut app = crate::app::GestoreApp::new_headless();
        let scene = crate::parser::parse_wrl(TWO_PARTS);
        let mut r = crate::renderer::WgpuRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        r.upload_meshes(&scene.shapes_data, &device);
        // Vista dall'alto sul quadrato rosso, che riempie la vista
        let red = scene.subtree_bounding_box(node(&scene, "R1"));
        r.camera.fit_to_bbox(&red);
        r.camera.orientation = glam::Quat::IDENTITY;
        r.camera.scene_bounds = Some(scene.compute_bounding_box());
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        app.renderer = Some(r);
        Some(app)
    }

    /// Centro della vista 3D: tra il bordo sinistro dei comandi sopra la vista e il fondo.
    fn viewport_center(texts: &[(String, egui::Pos2)]) -> egui::Pos2 {
        let centra = texts.iter().find(|(s, _)| s.contains("Centra")).map(|(_, p)| *p).expect("comandi della vista");
        egui::pos2((centra.x + 1280.0) / 2.0 + 20.0, (centra.y + 760.0) / 2.0)
    }

    /// Pipetta: con Blu selezionato, il clic sul pezzo rosso dà il rosso a Blu e spegne la
    /// pipetta; senza selezione il colore va tra i miei colori.
    #[test]
    fn eyedropper_copies_the_color_of_the_clicked_piece() {
        let _gpu = crate::renderer::gpu_lock();
        let Some(mut app) = app_with_renderer() else { return };
        let ctx = egui::Context::default();
        let texts = frame(&ctx, &mut app, vec![]);
        let center = viewport_center(&texts);
        let blu = node(app.scene_graph.as_ref().unwrap(), "Blu");
        app.ui_state.select_single(Some(blu));
        app.ui_state.eyedropper = true;
        click(&ctx, &mut app, center);
        assert!(!app.ui_state.eyedropper, "la pipetta si spegne dopo la lettura");
        let scene = app.scene_graph.as_ref().unwrap();
        assert_eq!(scene.info[blu as usize].color, Some([1.0, 0.0, 0.0, 1.0]), "{}", app.ui_state.status_message);
        assert_eq!(app.ui_state.selected_node, Some(blu), "la selezione non cambia");
        assert!(app.ui_state.status_message.contains("R1"), "{}", app.ui_state.status_message);

        app.ui_state.select_single(None);
        app.ui_state.eyedropper = true;
        click(&ctx, &mut app, center);
        assert_eq!(app.settings.saved_colors.first(), Some(&[1.0, 0.0, 0.0]));
    }

    /// Il tasto "Zoom" della vista inverte il verso della rotella.
    #[test]
    fn zoom_button_inverts_the_wheel() {
        let _gpu = crate::renderer::gpu_lock();
        let Some(mut app) = app_with_renderer() else { return };
        let ctx = egui::Context::default();
        let texts = frame(&ctx, &mut app, vec![]);
        let center = viewport_center(&texts);
        let wheel = |app: &mut crate::app::GestoreApp| {
            let before = app.renderer.as_ref().unwrap().camera.distance;
            let scroll = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 50.0), modifiers: Default::default() };
            frame(&ctx, app, vec![egui::Event::PointerMoved(center), scroll]);
            app.renderer.as_ref().unwrap().camera.distance / before
        };
        let normal = wheel(&mut app);
        assert!(normal < 1.0, "rotella in avanti avvicina: {normal}");
        let button = texts.iter().find(|(s, _)| s.ends_with("Zoom")).map(|(_, p)| *p + egui::vec2(4.0, 6.0)).expect("tasto Zoom");
        click(&ctx, &mut app, button);
        assert!(app.settings.invert_zoom);
        let inverted = wheel(&mut app);
        assert!(inverted > 1.0, "invertito: rotella in avanti allontana: {inverted}");
    }
}
