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

/// Colore con cui è disegnato un nodo: il colore utente (anche ereditato), altrimenti quello
/// del file del primo pezzo del sotto-albero, altrimenti `default`.
pub fn current_color(scene: &SceneGraph, node: u32, default: [f32; 4]) -> [f32; 4] {
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

/// Quadratino di colore cliccabile.
fn swatch(ui: &mut egui::Ui, col: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    ui.painter().rect_filled(rect, 3.0, col);
    let stroke = if resp.hovered() {
        egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color())
    } else {
        egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color().gamma_multiply(0.5))
    };
    ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);
    resp
}

/// Contenuto del menu "Colore" per un nodo: palette, miei colori, selettore, trasparenza,
/// ripristino.
pub fn color_menu(
    ui: &mut egui::Ui,
    scene: &SceneGraph,
    node: u32,
    default_color: [f32; 4],
    saved_colors: &[[f32; 3]],
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
            if swatch(ui, col).clicked() {
                actions.push(Action::SetColor(node, Some(from_color32(col, alpha))));
            }
        }
    });

    // Colori salvati dall'utente (tasto destro per toglierli)
    ui.add_space(4.0);
    ui.label(egui::RichText::new("I miei colori").small().weak());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for (i, &[r, g, b]) in saved_colors.iter().enumerate() {
            let col = to_color32([r, g, b, 1.0]);
            let resp = swatch(ui, col).on_hover_text("Clic: applica  ·  tasto destro: togli dai miei colori");
            if resp.clicked() {
                actions.push(Action::SetColor(node, Some([r, g, b, alpha])));
            }
            if resp.secondary_clicked() {
                actions.push(Action::ForgetColor(i));
            }
        }
        if ui
            .small_button(crate::ui::theme::icon_text(ui, crate::ui::theme::icon::PLUS, ""))
            .on_hover_text("Salva il colore attuale tra i miei colori (restano anche alla prossima apertura)")
            .clicked()
        {
            actions.push(Action::SaveColor([current[0], current[1], current[2]]));
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

/// Pezzi da colorare con "Colori casuali": per ogni nodo di partenza (la selezione, o le radici
/// della scena se non c'è selezione), un pezzo singolo resta sé stesso; in un gruppo, ogni pezzo è
/// il macro-oggetto che contiene la Shape (quello che si seleziona con un clic nella vista).
/// Un macro-oggetto selezionato riceve quindi un solo colore. Senza ripetizioni, nell'ordine
/// dell'albero.
pub fn random_color_pieces(scene: &SceneGraph, start: &[u32]) -> Vec<u32> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for &n in start {
        let mut shapes = Vec::new();
        scene.for_each_subtree_node(n, |i| {
            if matches!(scene.nodes[i as usize], SceneNode::Shape { .. }) {
                shapes.push(i);
            }
        });
        shapes.sort_unstable();
        for s in shapes {
            let piece = if s == n { s } else { scene.parent_of(s).unwrap_or(s) };
            if seen.insert(piece) {
                out.push(piece);
            }
        }
    }
    out
}

/// `count` colori ben distinguibili e diversi a ogni chiamata (`seed`): tinte distanziate con
/// la sezione aurea a partire da una tinta casuale, saturazione e luminosità che si alternano.
pub fn random_colors(count: usize, seed: u64) -> Vec<[f32; 4]> {
    // xorshift: abbastanza per mescolare il punto di partenza
    let mut x = seed | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1u64 << 53) as f64
    };
    let start = next();
    let shift = (next() * 6.0) as usize;
    const SATURATION: [f32; 3] = [0.75, 0.55, 0.9];
    const VALUE: [f32; 4] = [0.92, 0.75, 0.85, 0.65];
    (0..count)
        .map(|i| {
            let h = ((start + i as f64 * 0.618_033_988_75) % 1.0) as f32;
            let (s, v) = (SATURATION[(i + shift) % 3], VALUE[(i + shift) % 4]);
            let [r, g, b] = hsv_to_rgb(h, s, v);
            [r, g, b, 1.0]
        })
        .collect()
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let f = h6.fract();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match h6 as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Azione "Colori casuali" sulla selezione (o su tutta la scena se `selection` è vuota).
pub fn random_colors_action(scene: &SceneGraph, selection: &[u32]) -> Option<Action> {
    let start = if selection.is_empty() { &scene.roots[..] } else { selection };
    let pieces = random_color_pieces(scene, start);
    if pieces.is_empty() {
        return None;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9, |d| d.as_nanos() as u64);
    let colors = random_colors(pieces.len(), seed);
    Some(Action::SetColors(pieces.into_iter().zip(colors).collect()))
}

/// Colore con cui è disegnata la Shape `mesh` (per la pipetta).
pub fn mesh_color(scene: &SceneGraph, mesh: u32, default: [f32; 4]) -> [f32; 4] {
    let node = scene.mesh_to_node.get(mesh as usize).copied();
    node.and_then(|n| scene.effective_user_color(n))
        .or_else(|| scene.shapes_data.get(mesh as usize).and_then(|s| s.color))
        .unwrap_or(default)
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Due macro-oggetti (Rosso con due pezzi, Blu con uno) in un gruppo.
    pub const TWO_PARTS: &[u8] = br#"#VRML V2.0 utf8
DEF Gruppo Group { children [
  DEF Rosso Transform { children [
    DEF R1 Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
      geometry IndexedFaceSet { coord Coordinate { point [ -10 -10 0, 10 -10 0, 10 10 0, -10 10 0 ] } coordIndex [ 0 1 2 3 -1 ] } }
    DEF R2 Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
      geometry IndexedFaceSet { coord Coordinate { point [ -10 -10 -50, 10 -10 -50, 0 10 -50 ] } coordIndex [ 0 1 2 -1 ] } }
  ] }
  DEF Blu Transform { translation 100 0 0 children [
    DEF B1 Shape { appearance Appearance { material Material { diffuseColor 0 0 1 } }
      geometry IndexedFaceSet { coord Coordinate { point [ -1 -1 0, 1 -1 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }
  ] }
] }
"#;

    pub fn node(scene: &SceneGraph, name: &str) -> u32 {
        (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == name).unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn random_colors_go_to_whole_pieces() {
        let scene = crate::parser::parse_wrl(TWO_PARTS);
        let (rosso, blu, r1) = (node(&scene, "Rosso"), node(&scene, "Blu"), node(&scene, "R1"));
        // Senza selezione: un colore per macro-oggetto
        let mut all = random_color_pieces(&scene, &scene.roots);
        all.sort_unstable();
        assert_eq!(all, { let mut v = vec![rosso, blu]; v.sort_unstable(); v });
        // Un macro-oggetto selezionato: un colore solo; un pezzo singolo: sé stesso
        assert_eq!(random_color_pieces(&scene, &[rosso]), vec![rosso]);
        assert_eq!(random_color_pieces(&scene, &[r1, blu]), vec![r1, blu]);
        assert_eq!(random_color_pieces(&scene, &[r1, rosso]), vec![r1, rosso], "senza ripetizioni");
    }

    #[test]
    fn random_colors_are_distinct_and_change_every_time() {
        let a = random_colors(40, 1);
        let b = random_colors(40, 2);
        assert_ne!(a, b);
        for c in &a {
            assert!(c.iter().all(|v| (0.0..=1.0).contains(v)) && c[3] == 1.0, "{c:?}");
        }
        // Colori vicini nella lista ben distinti (distanza RGB)
        for w in a.windows(2) {
            let d: f32 = (0..3).map(|k| (w[0][k] - w[1][k]).powi(2)).sum::<f32>().sqrt();
            assert!(d > 0.15, "{w:?}");
        }
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]);
        assert_eq!(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), [0.0, 0.0, 1.0]);
    }

    /// "Colori casuali" dalla toolbar: un colore per pezzo, un solo passo di Annulla.
    #[test]
    fn random_colors_button_colors_the_scene_and_undoes_in_one_step() {
        let mut app = crate::app::GestoreApp::new_headless();
        let scene = crate::parser::parse_wrl(TWO_PARTS);
        app.project.on_loaded(scene.revision);
        app.scene_graph = Some(scene);
        let ctx = egui::Context::default();
        let texts = crate::ui::tests::frame(&ctx, &mut app, vec![]);
        let dice = crate::ui::theme::icon::DICE_FIVE;
        let at = texts.iter().find(|(s, _)| s == dice).map(|(_, p)| *p + egui::vec2(6.0, 6.0)).expect("pulsante dei colori casuali");
        crate::ui::tests::click(&ctx, &mut app, at);
        let scene = app.scene_graph.as_ref().unwrap();
        let (rosso, blu) = (node(scene, "Rosso"), node(scene, "Blu"));
        let (a, b) = (scene.info[rosso as usize].color.unwrap(), scene.info[blu as usize].color.unwrap());
        assert_ne!(a, b);
        app.undo();
        let scene = app.scene_graph.as_ref().unwrap();
        assert!(scene.info.iter().all(|i| i.color.is_none()));
    }
}
