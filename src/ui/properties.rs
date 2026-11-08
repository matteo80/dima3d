//! Proprietà dell'elemento selezionato, in fondo al pannello Scena: nome, tipo, file di
//! provenienza, pezzi, triangoli, vertici e ingombro (con più elementi selezionati, il totale).

use crate::scene_graph::{NodeKind, SceneGraph, SceneNode};

/// Riepilogo di uno o più sotto-alberi.
#[derive(Debug, Clone)]
pub struct Summary {
    pub pieces: usize,
    pub triangles: usize,
    pub vertices: usize,
    pub bbox: crate::scene_graph::Aabb,
    /// File da cui vengono i pezzi
    pub files: Vec<String>,
}

/// Somma dei pezzi visibili e nascosti dei sotto-alberi `nodes`.
pub fn summarize(scene: &SceneGraph, nodes: &[u32]) -> Summary {
    let mut meshes = Vec::new();
    for &n in nodes {
        scene.for_each_subtree_mesh(n, |m| meshes.push(m));
    }
    meshes.sort_unstable();
    meshes.dedup();
    let mut s = Summary {
        pieces: meshes.len(),
        triangles: 0,
        vertices: 0,
        bbox: crate::scene_graph::Aabb::empty(),
        files: Vec::new(),
    };
    for &m in &meshes {
        let shape = &scene.shapes_data[m as usize];
        s.triangles += shape.indices.len() / 3;
        s.vertices += shape.world_vertices.len();
        if let Some(b) = scene.mesh_bboxes.get(m as usize).filter(|b| b.is_valid()) {
            s.bbox.extend_aabb(b);
        }
        if let Some(i) = scene.source_index(m) {
            let name = &scene.sources[i].name;
            if !s.files.contains(name) {
                s.files.push(name.clone());
            }
        }
    }
    s
}

fn kind_label(scene: &SceneGraph, node: u32) -> &'static str {
    match (scene.info.get(node as usize).map(|i| i.kind), scene.nodes.get(node as usize)) {
        (Some(NodeKind::File), _) => "File",
        (Some(NodeKind::Cluster), _) => "Componente (raggruppamento automatico)",
        (Some(NodeKind::Singles), _) => "Pezzi singoli",
        (Some(NodeKind::Use), _) => "Istanza (USE)",
        (_, Some(SceneNode::Shape { .. })) => "Pezzo",
        (Some(NodeKind::Transform), _) => "Transform",
        _ => "Gruppo",
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Disegna il riquadro delle proprietà (niente se non c'è selezione).
pub fn draw(ui: &mut egui::Ui, app: &crate::app::GestoreApp) {
    let (Some(scene), Some(primary)) = (&app.scene_graph, app.ui_state.selected_node) else { return };
    let nodes = app.ui_state.selected_nodes();
    if scene.nodes.get(primary as usize).is_none() {
        return;
    }
    let s = summarize(scene, &nodes);
    let size = scene.compute_bounding_box().diagonal();
    let fmt = |v: f32| super::measure::format_length(v, size);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Proprietà").strong());
        if nodes.len() > 1 {
            ui.label(egui::RichText::new(format!("({} elementi selezionati)", nodes.len())).weak());
        }
    });
    // I valori lunghi (nomi CAD) si troncano: non devono allargare il pannello.
    // Larghezza misurata fuori dalla griglia (dentro, lo spazio disponibile è quello della cella).
    let width = (ui.available_width() - 90.0).max(60.0);
    egui::Grid::new("proprieta").num_columns(2).spacing([10.0, 2.0]).striped(false).show(ui, |ui| {
        let mut row = |name: &str, value: String| {
            ui.label(egui::RichText::new(name).weak());
            ui.scope(|ui| {
                ui.set_max_width(width);
                ui.add(egui::Label::new(value.clone()).truncate()).on_hover_text(value);
            });
            ui.end_row();
        };
        if nodes.len() == 1 {
            row("Nome", scene.display_name(primary));
            row("Tipo", kind_label(scene, primary).to_string());
        }
        match s.files.as_slice() {
            [] => {}
            [one] => row("File", one.clone()),
            many => row("File", format!("{} file", many.len())),
        }
        row("Pezzi", thousands(s.pieces));
        row("Triangoli", thousands(s.triangles));
        row("Vertici", thousands(s.vertices));
        if s.bbox.is_valid() {
            let d = s.bbox.max - s.bbox.min;
            row("Dimensioni", format!("{} × {} × {} {}", fmt(d.x), fmt(d.y), fmt(d.z), super::measure::UNIT));
            row("Min", format!("{}  {}  {} {}", fmt(s.bbox.min.x), fmt(s.bbox.min.y), fmt(s.bbox.min.z), super::measure::UNIT));
            row("Max", format!("{}  {}  {} {}", fmt(s.bbox.max.x), fmt(s.bbox.max.y), fmt(s.bbox.max.z), super::measure::UNIT));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_graph::node::multi_file_tests::two_files;

    #[test]
    fn summary_of_a_file_and_of_a_selection_across_files() {
        let scene = two_files();
        let b = scene.roots[1];
        let s = summarize(&scene, &[b]);
        assert_eq!((s.pieces, s.triangles, s.vertices), (2, 2, 6));
        assert_eq!(s.files, vec!["b.wrl".to_string()]);
        // FILE_B: triangolo (0,0,1)-(2,0,1)-(0,2,1) traslato di 5 in X
        assert_eq!(s.bbox.min, glam::Vec3::new(5.0, 0.0, 1.0));
        assert_eq!(s.bbox.max, glam::Vec3::new(7.0, 2.0, 1.0));

        // Tutto: un nodo e un suo discendente non contano due volte
        let telaio = scene.nodes[scene.roots[0] as usize].children()[0];
        let s = summarize(&scene, &[scene.roots[0], telaio, b]);
        assert_eq!(s.pieces, 4);
        assert_eq!(s.files.len(), 2);
        assert_eq!(kind_label(&scene, b), "File");
        assert_eq!(kind_label(&scene, scene.mesh_to_node[0]), "Pezzo");
    }

    #[test]
    fn large_numbers_have_separators() {
        assert_eq!(thousands(3716448), "3.716.448");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1.000");
    }
}
