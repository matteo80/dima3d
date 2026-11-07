use crate::app::Action;
use crate::scene_graph::{SceneGraph, SceneNode};

/// Colori rapidi (sRGB).
pub const PALETTE: &[[u8; 3]] = &[
    [220, 50, 47],   // rosso
    [240, 140, 30],  // arancio
    [235, 200, 40],  // giallo
    [80, 175, 75],   // verde
    [40, 160, 160],  // ottanio
    [50, 110, 210],  // blu
    [130, 80, 190],  // viola
    [210, 90, 160],  // rosa
    [140, 100, 70],  // marrone
    [245, 245, 245], // bianco
    [160, 160, 165], // grigio
    [45, 45, 50],    // antracite
];

pub fn to_color32(c: [f32; 4]) -> egui::Color32 {
    let u = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(u(c[0]), u(c[1]), u(c[2]))
}

pub fn from_color32(c: egui::Color32, alpha: f32) -> [f32; 4] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, alpha]
}

/// Colore da mostrare come punto di partenza nel selettore per `node`.
fn current_color(scene: &SceneGraph, node: u32, default: [f32; 4]) -> [f32; 4] {
    if let Some(c) = scene.effective_user_color(node) {
        return c;
    }
    // Primo pezzo del sotto-albero con un colore originale
    let mut found = None;
    scene.for_each_subtree_node(node, |i| {
        if found.is_none() {
            if let SceneNode::Shape { mesh_id, .. } = scene.nodes[i as usize] {
                found = scene.shapes_data.get(mesh_id as usize).and_then(|s| s.color);
            }
        }
    });
    found.unwrap_or(default)
}

/// Contenuto del menu "Colore" per un nodo: selettore, trasparenza, palette, ripristino.
pub fn color_menu(
    ui: &mut egui::Ui,
    scene: &SceneGraph,
    node: u32,
    default_color: [f32; 4],
    actions: &mut Vec<Action>,
) {
    let current = current_color(scene, node, default_color);
    let mut c32 = to_color32(current);
    let mut alpha = current[3];

    ui.label(egui::RichText::new(scene.display_name(node)).strong());
    ui.add_space(2.0);

    // Palette rapida
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for &[r, g, b] in PALETTE {
            let col = egui::Color32::from_rgb(r, g, b);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, col);
            if resp.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color()),
                    egui::StrokeKind::Outside,
                );
            }
            if resp.clicked() {
                actions.push(Action::SetColor(node, Some(from_color32(col, alpha))));
            }
        }
    });
    ui.separator();

    // Selettore libero (si applica in tempo reale)
    if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::Opaque) {
        actions.push(Action::SetColor(node, Some(from_color32(c32, alpha))));
    }

    let mut transparency = (1.0 - alpha) * 100.0;
    if ui
        .add(egui::Slider::new(&mut transparency, 0.0..=90.0).suffix(" %").text("Trasparenza"))
        .changed()
    {
        alpha = 1.0 - transparency / 100.0;
        actions.push(Action::SetColor(node, Some(from_color32(c32, alpha))));
    }

    ui.separator();
    let has_user = scene.info.get(node as usize).is_some_and(|i| i.color.is_some());
    if ui
        .add_enabled(has_user, egui::Button::new(crate::ui::theme::icon_text(ui, crate::ui::theme::icon::ARROW_COUNTER_CLOCKWISE, "Colore originale")))
        .on_hover_text("Rimuove il colore impostato su questo elemento")
        .clicked()
    {
        actions.push(Action::SetColor(node, None));
        ui.close_menu();
    }
}

/// Con più nodi selezionati, estende a tutti il colore scelto nel menu.
pub fn spread_colors(actions: Vec<Action>, nodes: &[u32]) -> Vec<Action> {
    if nodes.len() < 2 {
        return actions;
    }
    actions
        .into_iter()
        .map(|a| match a {
            Action::SetColor(_, color) => {
                Action::Batch(nodes.iter().map(|&n| Action::SetColor(n, color)).collect())
            }
            other => other,
        })
        .collect()
}
