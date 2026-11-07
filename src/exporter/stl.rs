//! Export in STL delle parti visibili: binario (compatto, letto da tutti) o di testo, con un
//! `solid` per pezzo. Le coordinate sono quelle della scena (world-space, unità del file); colori
//! e gerarchia non esistono nel formato STL e si perdono.

use std::io::{self, BufWriter, Write};

use super::writer::{compute_has_visible, ExportStats};
use crate::scene_graph::{SceneGraph, SceneNode};

/// Pezzi visibili da scrivere (mesh_id e nome), nell'ordine dell'albero.
fn visible_shapes(scene: &SceneGraph, node: Option<u32>) -> Vec<(u32, String)> {
    let has_visible = compute_has_visible(scene);
    let roots = node.map_or_else(|| scene.roots.clone(), |n| vec![n]);
    let mut out = Vec::new();
    for root in roots {
        scene.for_each_subtree_node(root, |i| {
            if let (true, SceneNode::Shape { mesh_id, .. }) = (has_visible[i as usize], &scene.nodes[i as usize]) {
                out.push((*mesh_id, scene.display_name(i)));
            }
        });
    }
    out
}

/// Triangoli di una Shape in world-space, con l'orientamento corretto anche se la matrice era
/// speculare (le normali di faccia restano verso l'esterno).
fn triangles(shape: &crate::parser::ShapeInfo) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
    let mirrored = glam::Mat3::from_mat4(shape.matrix).determinant() < 0.0;
    let v = &shape.world_vertices;
    shape.indices.chunks_exact(3).filter_map(move |t| {
        let (a, mut b, mut c) = (*v.get(t[0] as usize)?, *v.get(t[1] as usize)?, *v.get(t[2] as usize)?);
        if mirrored {
            std::mem::swap(&mut b, &mut c);
        }
        Some([a, b, c])
    })
}

fn facet_normal(t: &[[f32; 3]; 3]) -> [f32; 3] {
    let p = |i: usize| glam::Vec3::from(t[i]);
    (p(1) - p(0)).cross(p(2) - p(0)).normalize_or_zero().to_array()
}

/// Dimensione di un STL binario con `triangles` triangoli.
pub fn binary_size(triangles: u64) -> u64 {
    84 + 50 * triangles
}

/// Dimensione dell'STL binario delle parti visibili.
pub fn estimate_binary_size(scene: &SceneGraph, node: Option<u32>) -> u64 {
    let triangles: usize = visible_shapes(scene, node)
        .iter()
        .filter_map(|(m, _)| scene.shapes_data.get(*m as usize))
        .map(|s| triangles(s).count())
        .sum();
    binary_size(triangles as u64)
}

/// Esporta le parti visibili (di tutta la scena o del sotto-albero `node`) in STL.
pub fn export_stl(scene: &SceneGraph, node: Option<u32>, path: &std::path::Path, ascii: bool) -> io::Result<ExportStats> {
    let shapes = visible_shapes(scene, node);
    let mut w = BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?);
    if ascii {
        let mut names = std::collections::HashMap::new();
        for (mesh, label) in &shapes {
            let Some(shape) = scene.shapes_data.get(*mesh as usize) else { continue };
            // Nome senza spazi, unico nel file (alcuni programmi leggono solo la prima parola)
            let base = super::sanitize_identifier(label);
            let n: &mut u32 = names.entry(base.clone()).or_insert(0);
            *n += 1;
            let name = if *n == 1 { base } else { format!("{base}_{n}") };
            writeln!(w, "solid {name}")?;
            for t in triangles(shape) {
                let [nx, ny, nz] = facet_normal(&t);
                writeln!(w, "  facet normal {nx} {ny} {nz}\n    outer loop")?;
                for [x, y, z] in t {
                    writeln!(w, "      vertex {x} {y} {z}")?;
                }
                writeln!(w, "    endloop\n  endfacet")?;
            }
            writeln!(w, "endsolid {name}")?;
        }
    } else {
        let total: usize = shapes
            .iter()
            .filter_map(|(m, _)| scene.shapes_data.get(*m as usize))
            .map(|s| triangles(s).count())
            .sum();
        let count = u32::try_from(total).map_err(|_| io::Error::other("troppi triangoli per un STL binario"))?;
        let mut header = b"Gestore WRL - STL binario".to_vec();
        header.resize(80, b' ');
        w.write_all(&header)?;
        w.write_all(&count.to_le_bytes())?;
        let mut record = [0u8; 50];
        for (mesh, _) in &shapes {
            let Some(shape) = scene.shapes_data.get(*mesh as usize) else { continue };
            for t in triangles(shape) {
                let values = std::iter::once(facet_normal(&t)).chain(t);
                for (i, v) in values.enumerate() {
                    for (k, c) in v.iter().enumerate() {
                        let at = i * 12 + k * 4;
                        record[at..at + 4].copy_from_slice(&c.to_le_bytes());
                    }
                }
                w.write_all(&record)?;
            }
        }
    }
    w.flush()?;
    Ok(ExportStats { shapes_written: shapes.len(), groups_written: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_PIECES: &[u8] = br#"#VRML V2.0 utf8
DEF Assieme Transform { translation 10 0 0 children [
    DEF Piastra Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] } coordIndex [ 0 1 2 3 -1 ] } }
    DEF Specchio Transform { scale -1 1 1 children [
        DEF Lato Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 1, 1 0 1, 0 1 1 ] } coordIndex [ 0 1 2 -1 ] } }
    ] }
    DEF Nascosto Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 5, 1 0 5, 0 1 5 ] } coordIndex [ 0 1 2 -1 ] } }
] }
"#;

    fn scene() -> SceneGraph {
        let mut scene = crate::parser::parse_wrl(TWO_PIECES);
        let hidden = (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == "Nascosto").unwrap();
        scene.set_subtree_visible(hidden, false);
        scene
    }

    /// Binario: dimensione esatta, riapertura con gli stessi triangoli in world-space e normali
    /// verso +Z anche per il pezzo speculare.
    #[test]
    fn binary_round_trip() {
        let scene = scene();
        let path = std::env::temp_dir().join("gestore_wrl_export_test.stl");
        let stats = export_stl(&scene, None, &path, false).unwrap();
        assert_eq!(stats.shapes_written, 2);
        let data = std::fs::read(&path).unwrap();
        assert_eq!(data.len() as u64, binary_size(3));
        let solids = crate::parser::stl::read(&data).unwrap();
        let corners = &solids[0].corners;
        assert_eq!(corners.len(), 9);
        assert!(corners.contains(&[10.0, 0.0, 0.0]) && corners.contains(&[9.0, 0.0, 1.0]), "{corners:?}");
        for t in data[84..].chunks_exact(50) {
            let nz = f32::from_le_bytes(t[8..12].try_into().unwrap());
            assert!((nz - 1.0).abs() < 1e-6, "normale {nz}");
        }
        let again = crate::parser::parse_wrl(&data);
        assert_eq!(again.shapes_data[0].face_count, 3);
        let _ = std::fs::remove_file(path);
    }

    /// Testo: un `solid` per pezzo con il suo nome; si riapre con due pezzi.
    #[test]
    fn ascii_round_trip() {
        let scene = scene();
        let path = std::env::temp_dir().join("gestore_wrl_export_test_ascii.stl");
        export_stl(&scene, None, &path, true).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("solid Piastra") && text.contains("endsolid Lato"), "{text}");
        assert!(!text.contains("Nascosto"));
        let again = crate::parser::parse_wrl(text.as_bytes());
        let mut names: Vec<String> = again.mesh_to_node.iter().map(|&n| again.display_name(n)).collect();
        names.sort();
        assert_eq!(names, vec!["Lato", "Piastra"]);
        let mut vertices: Vec<usize> = again.shapes_data.iter().map(|s| s.world_vertices.len()).collect();
        vertices.sort();
        assert_eq!(vertices, vec![3, 4]);
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod real_file {
    /// `GESTORE_WRL_FILE=percorso cargo test --release stl_file_reale -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stl_file_reale() {
        let path = std::env::var("GESTORE_WRL_FILE").expect("impostare GESTORE_WRL_FILE");
        let scene = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        let total: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
        for ascii in [false, true] {
            let out = std::env::temp_dir().join(if ascii { "gestore_wrl_reale_ascii.stl" } else { "gestore_wrl_reale.stl" });
            let t = std::time::Instant::now();
            super::export_stl(&scene, None, &out, ascii).unwrap();
            let written = t.elapsed();
            let data = std::fs::read(&out).unwrap();
            let t = std::time::Instant::now();
            let again = crate::parser::parse_wrl(&data);
            let tris: usize = again.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
            println!(
                "{}: {:.1} MB scritto in {written:?}, riaperto in {:?}: {} pezzi, {tris} triangoli (originale {total}), avvisi {:?}",
                if ascii { "ASCII" } else { "binario" },
                data.len() as f64 / 1e6,
                t.elapsed(),
                again.shapes_data.len(),
                again.load_warnings
            );
            if !ascii {
                assert_eq!(data.len() as u64, super::estimate_binary_size(&scene, None));
            }
        }
    }
}
