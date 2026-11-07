//! Lettura dei file STL (binari e di testo), convertiti in VRML 2.0 come i file VRML 1.0: il
//! resto del programma (albero, export, riduzione) lavora sul testo convertito.
//!
//! Un STL è una lista di triangoli senza vertici condivisi, senza colori e senza unità (di solito
//! millimetri). I vertici uguali vengono uniti e le normali ricalcolate con un angolo di spigolo
//! vivo: quelle scritte nel file sono di faccia, spesso nulle o sbagliate.

/// Un `solid` del file: nome e triangoli (tre vertici ciascuno, uno dopo l'altro).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Solid {
    pub name: String,
    pub corners: Vec<[f32; 3]>,
}

/// Formato binario: 80 byte di intestazione, numero di triangoli, 50 byte per triangolo.
fn binary_count(data: &[u8]) -> Option<usize> {
    let n = u32::from_le_bytes(data.get(80..84)?.try_into().ok()?) as usize;
    (data.len() == 84 + n * 50).then_some(n)
}

/// Vero se `data` è un STL. Il binario si riconosce dalla lunghezza (molti iniziano anch'essi
/// con "solid"), il testo da `solid` seguito da `facet`.
pub fn is_stl(data: &[u8]) -> bool {
    if data.starts_with(b"#VRML") {
        return false;
    }
    if binary_count(data).is_some() {
        return true;
    }
    let head = &data[..data.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    trimmed.starts_with("solid") && (trimmed.contains("facet") || trimmed.contains("endsolid"))
}

/// Legge i solidi del file.
pub fn read(data: &[u8]) -> Result<Vec<Solid>, String> {
    if let Some(n) = binary_count(data) {
        let header = String::from_utf8_lossy(&data[..80]);
        let name = header
            .trim_start()
            .strip_prefix("solid")
            .map(|s| s.trim_matches(|c: char| c.is_whitespace() || c == '\0').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "STL".to_string());
        let mut corners = Vec::with_capacity(n * 3);
        for t in 0..n {
            let at = 84 + t * 50 + 12;
            for k in 0..3 {
                let v = &data[at + k * 12..at + k * 12 + 12];
                let f = |i: usize| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap());
                corners.push([f(0), f(1), f(2)]);
            }
        }
        return Ok(vec![Solid { name, corners }]);
    }

    let text = std::str::from_utf8(data).map_err(|_| "STL di testo con caratteri non validi".to_string())?;
    let mut solids = Vec::new();
    let mut current: Option<Solid> = None;
    for (n, line) in text.lines().enumerate() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("solid") => {
                if let Some(s) = current.take() {
                    solids.push(s);
                }
                let name = words.collect::<Vec<_>>().join(" ");
                current = Some(Solid { name: if name.is_empty() { "STL".to_string() } else { name }, corners: Vec::new() });
            }
            Some("vertex") => {
                let v: Vec<f32> = words.take(3).filter_map(|w| w.parse().ok()).collect();
                if v.len() != 3 {
                    return Err(format!("STL non valido alla riga {}: vertice incompleto", n + 1));
                }
                current.get_or_insert_with(|| Solid { name: "STL".to_string(), corners: Vec::new() }).corners.push([v[0], v[1], v[2]]);
            }
            Some("endsolid") => {
                if let Some(s) = current.take() {
                    solids.push(s);
                }
            }
            _ => {}
        }
    }
    if let Some(s) = current.take() {
        solids.push(s);
    }
    for s in &mut solids {
        // Un triangolo incompleto in fondo (file troncato) si scarta
        s.corners.truncate(s.corners.len() / 3 * 3);
    }
    solids.retain(|s| !s.corners.is_empty());
    if solids.is_empty() {
        return Err("Il file STL non contiene triangoli".to_string());
    }
    Ok(solids)
}

/// Converte un STL in testo VRML 2.0: una Shape per `solid`, con le normali a spigolo vivo.
/// Coordinate con un passo di un milionesimo dell'ingombro (come l'export compatto), normali con
/// 4 decimali.
pub(crate) fn convert(data: &[u8]) -> super::vrml1::Converted {
    use crate::exporter::writer::{push_fixed, push_uint};
    let mut out: Vec<u8> = b"#VRML V2.0 utf8\n# Convertito da STL dal Gestore WRL\n\n".to_vec();
    let mut warnings = Vec::new();
    let solids = match read(data) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(e);
            Vec::new()
        }
    };
    let mut bbox = crate::scene_graph::Aabb::empty();
    for s in &solids {
        for &p in &s.corners {
            bbox.extend_point(glam::Vec3::from(p));
        }
    }
    let extent = if bbox.is_valid() { bbox.diagonal() as f64 } else { 1.0 };
    let decimals = if extent > 0.0 && extent.is_finite() { (-(extent * 1e-6).log10()).ceil().clamp(0.0, 9.0) as u32 } else { 6 };
    let vector = |out: &mut Vec<u8>, v: &[f32; 3], d: u32| {
        push_fixed(out, v[0], d);
        out.push(b' ');
        push_fixed(out, v[1], d);
        out.push(b' ');
        push_fixed(out, v[2], d);
        out.push(b'\n');
    };
    let triangle = |out: &mut Vec<u8>, t: &[u32]| {
        push_uint(out, t[0] as u64);
        out.push(b' ');
        push_uint(out, t[1] as u64);
        out.push(b' ');
        push_uint(out, t[2] as u64);
        out.extend_from_slice(b" -1\n");
    };
    let mut names = std::collections::HashMap::new();
    for solid in solids {
        let order: Vec<u32> = (0..solid.corners.len() as u32).collect();
        let mesh = crate::reduce::weld(&solid.corners, &order);
        if mesh.indices.is_empty() {
            warnings.push(format!("Solido «{}»: solo triangoli degeneri, ignorato", solid.name));
            continue;
        }
        let (normals, split) = crate::reduce::crease_normals(&mesh.positions, &mesh.indices, crate::reduce::CREASE_ANGLE);
        let base = crate::exporter::sanitize_identifier(&solid.name);
        let count: &mut u32 = names.entry(base.clone()).or_insert(0);
        *count += 1;
        let name = if *count == 1 { base } else { format!("{base}_{count}") };
        out.reserve(mesh.positions.len() * 30 + mesh.indices.len() * 12 + normals.len() * 22);
        out.extend_from_slice(format!("DEF {name} Shape {{\ngeometry IndexedFaceSet {{\nsolid FALSE\ncoord Coordinate {{ point [\n").as_bytes());
        for p in &mesh.positions {
            vector(&mut out, p, decimals);
        }
        out.extend_from_slice(b"] }\ncoordIndex [\n");
        for t in mesh.indices.chunks_exact(3) {
            triangle(&mut out, t);
        }
        out.extend_from_slice(b"]\nnormal Normal { vector [\n");
        for n in &normals {
            vector(&mut out, n, 4);
        }
        out.extend_from_slice(b"] }\nnormalIndex [\n");
        let render = split.as_ref().map_or(&mesh.indices, |s| &s.indices);
        for t in render.chunks_exact(3) {
            triangle(&mut out, t);
        }
        out.extend_from_slice(b"]\n}\n}\n");
    }
    super::vrml1::Converted { text: out, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cubo unitario: 12 triangoli, 8 vertici distinti.
    fn cube_triangles() -> Vec<[[f32; 3]; 3]> {
        let p = |i: usize| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32];
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut tris = Vec::new();
        for q in quads {
            tris.push([p(q[0]), p(q[1]), p(q[2])]);
            tris.push([p(q[0]), p(q[2]), p(q[3])]);
        }
        tris
    }

    pub(crate) fn binary_cube() -> Vec<u8> {
        let tris = cube_triangles();
        let mut data = b"solid cubo binario".to_vec();
        data.resize(80, 0);
        data.extend((tris.len() as u32).to_le_bytes());
        for t in &tris {
            data.extend([0u8; 12]); // normale nulla, come in molti file
            for v in t {
                for c in v {
                    data.extend(c.to_le_bytes());
                }
            }
            data.extend([0u8; 2]);
        }
        data
    }

    fn ascii_cube(name: &str) -> String {
        let mut s = format!("solid {name}\n");
        for t in cube_triangles() {
            s.push_str("  facet normal 0 0 0\n    outer loop\n");
            for v in t {
                s.push_str(&format!("      vertex {} {} {}\n", v[0], v[1], v[2]));
            }
            s.push_str("    endloop\n  endfacet\n");
        }
        s.push_str(&format!("endsolid {name}\n"));
        s
    }

    #[test]
    fn binary_and_ascii_are_recognized() {
        assert!(is_stl(&binary_cube()));
        assert!(is_stl(ascii_cube("pezzo").as_bytes()));
        assert!(!is_stl(b"#VRML V2.0 utf8\nShape {}"));
        assert!(!is_stl(b"solid ma non un stl"));
    }

    #[test]
    fn binary_cube_opens_with_sharp_edges() {
        let scene = crate::parser::parse_wrl(&binary_cube());
        assert_eq!(scene.shapes_data.len(), 1);
        let s = &scene.shapes_data[0];
        assert_eq!(s.world_vertices.len(), 8, "vertici uniti");
        assert_eq!(s.face_count, 12);
        // Spigoli vivi: ogni angolo ha la normale della sua faccia (assiale)
        for n in s.corner_normals() {
            let v = glam::Vec3::from(n);
            assert!((v.abs().max_element() - 1.0).abs() < 1e-4, "{v:?}");
        }
        assert_eq!(scene.display_name(scene.mesh_to_node[0]), "cubo_binario");
    }

    #[test]
    fn ascii_solids_become_named_pieces() {
        let text = format!("{}{}", ascii_cube("primo pezzo"), ascii_cube("secondo"));
        let scene = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(scene.shapes_data.len(), 2);
        let names: Vec<String> = scene.mesh_to_node.iter().map(|&n| scene.display_name(n)).collect();
        assert_eq!(names, vec!["primo_pezzo", "secondo"]);
        assert!(scene.load_warnings.is_empty());
    }

    #[test]
    fn broken_files_give_a_message() {
        let scene = crate::parser::parse_wrl(b"solid rotto\nfacet normal 0 0 1\nouter loop\nvertex 1 2\nendloop\nendfacet\nendsolid");
        assert!(scene.shapes_data.is_empty());
        assert!(scene.load_warnings.iter().any(|w| w.contains("riga 4")), "{:?}", scene.load_warnings);
    }
}

#[cfg(test)]
mod timing {
    /// `GESTORE_STL=percorso cargo test --release tempi_stl -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tempi_stl() {
        let data = std::fs::read(std::env::var("GESTORE_STL").unwrap()).unwrap();
        let t = std::time::Instant::now();
        let solids = super::read(&data).unwrap();
        println!("lettura {:?}", t.elapsed());
        let s = &solids[0];
        let order: Vec<u32> = (0..s.corners.len() as u32).collect();
        let t = std::time::Instant::now();
        let mesh = crate::reduce::weld(&s.corners, &order);
        println!("saldatura {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = crate::reduce::crease_normals(&mesh.positions, &mesh.indices, crate::reduce::CREASE_ANGLE);
        println!("normali {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let c = super::convert(&data);
        println!("conversione completa {:?} ({:.1} MB di testo)", t.elapsed(), c.text.len() as f64 / 1e6);
        let t = std::time::Instant::now();
        let _ = crate::parser::parse_wrl(&c.text);
        println!("parsing del testo {:?}", t.elapsed());
    }
}
