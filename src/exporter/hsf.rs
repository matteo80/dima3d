//! Export HSF (HOOPS Stream Format), per i visualizzatori basati su HOOPS.
//!
//! Stessa struttura degli altri export: solo le parti visibili, un segmento per ogni gruppo e
//! pezzo, coordinate in world-space. Il file imita quelli scritti da HOOPS (vedi il lettore
//! `parser::hsf`):
//! - intestazione `;; HSF V21.00`, informazioni del file (`I`) e ingombro (`B`), poi il resto
//!   compresso con zlib (`Z`);
//! - i nomi interni dei segmenti sono unici e semplici (`part 12`): HOOPS riapre un segmento con
//!   lo stesso nome invece di crearne un altro, e alcuni caratteri hanno un significato nei
//!   percorsi. Il nome vero va nei metadati `name` (`m`), come nei file originali;
//! - le Shell hanno punti `float` e le facce compresse come strisce di triangoli (`3 a b c` per
//!   triangolo, interi a 8/16/32 bit secondo il numero di vertici), come in tutti i file
//!   originali: il visualizzatore usato in azienda non accetta la lista delle facce non compressa
//!   (il file si apre ma «non ha nessun corpo»), che quindi non si scrive;
//! - normali facoltative («Scrivi le normali»), polari come negli originali: un angolo
//!   θ = atan2(y, x) e un angolo φ = acos(z) per vertice (attributo 0x13 dopo le facce). Con le
//!   normali si scrivono i vertici di resa, sdoppiati sugli spigoli vivi, così gli spigoli restano
//!   quelli calcolati da Dima 3D; senza, i vertici sono uniti e il visualizzatore ricalcola le
//!   normali sfumando l'ombreggiatura anche sugli spigoli, ma il file è più piccolo;
//! - colore e trasparenza (`"`: diffuse e transmission) sul segmento del pezzo;
//! - gerarchia e opzioni utente (`U`) come nei file scritti da HOOPS Exchange, che il
//!   visualizzatore usato in azienda richiede per riconoscere i corpi (senza, il file si apre ma
//!   «non ha nessun corpo»): gruppo = `product occurrence` con `hx3d_type=product`; pezzo =
//!   `product occurrence` › `part` (`hx3d_type=part`) › `representation item` (colore) ›
//!   `internal container` › Shell;
//! - le scene HSF sono in metri: le coordinate restano nelle unità della scena e il segmento
//!   radice (`model_root`, come nei file originali) ha la scala verso i metri (0,001 se la scena
//!   è in millimetri, nessuna se è già in metri, come una scocca aperta da un WRL in metri).

use std::io::{self, BufWriter, Write};

use super::writer::{compute_has_visible, ExportStats};
use crate::scene_graph::{NodeKind, SceneGraph, SceneNode};

/// Millimetri → metri della scena HSF.
const MM_TO_METERS: f32 = 0.001;

/// Opzioni del file (`I`) come nei file originali scritti da HOOPS.
const FILE_INFO: i32 = 0x40c;

struct Ctx<'a> {
    scene: &'a SceneGraph,
    has_visible: Vec<bool>,
    write_file_nodes: bool,
    default_color: [f32; 4],
    normals: bool,
    segments: u32,
    stats: ExportStats,
}

fn push_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Opzioni utente (`U`): lunghezza a 16 bit (0xFFFF + 32 bit se più lunga), testo senza zero.
fn push_options(out: &mut Vec<u8>, text: &str) {
    out.push(b'U');
    if text.len() < 0xFFFF {
        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
    } else {
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        push_i32(out, text.len() as i32);
    }
    out.extend_from_slice(text.as_bytes());
}

/// Facce compresse (schema 1) come strisce da un triangolo: lunghezza dei dati più il byte dei
/// bit, bit per valore, poi `3 a b c` per ogni triangolo in interi little-endian.
fn push_strips(out: &mut Vec<u8>, indices: &[u32], vertices: usize) {
    let bits: u8 = if vertices <= 0xFF { 8 } else if vertices <= 0xFFFF { 16 } else { 32 };
    let bytes = bits as usize / 8;
    out.push(1);
    push_i32(out, (indices.len() / 3 * 4 * bytes + 1) as i32);
    out.push(bits);
    for t in indices.chunks_exact(3) {
        for v in [3, t[0], t[1], t[2]] {
            out.extend_from_slice(&v.to_le_bytes()[..bytes]);
        }
    }
}

/// Normale in coordinate polari come nei file originali: n = (cos θ sin φ, sin θ sin φ, cos φ).
fn polar(n: [f32; 3]) -> [f32; 2] {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len <= f32::EPSILON {
        return [0.0, 0.0];
    }
    [n[1].atan2(n[0]), (n[2] / len).clamp(-1.0, 1.0).acos()]
}

/// Testo sicuro dentro le opzioni `chiave=valore,…`: niente virgole né uguali.
fn option_value(text: &str) -> String {
    text.chars().map(|c| if c == ',' || c == '=' { '_' } else { c }).collect()
}

impl Ctx<'_> {
    /// Nodi da scrivere al posto di `idx`: sé stesso, oppure (nodi file non scritti) i figli.
    fn expand(&self, idx: u32, out: &mut Vec<u32>) {
        if !self.has_visible.get(idx as usize).copied().unwrap_or(false) {
            return;
        }
        let is_file = self.scene.info.get(idx as usize).is_some_and(|i| i.kind == NodeKind::File);
        if is_file && !self.write_file_nodes {
            for &c in self.scene.nodes[idx as usize].children() {
                self.expand(c, out);
            }
        } else {
            out.push(idx);
        }
    }

    /// Apre un segmento con un nome interno unico, le opzioni utente e il nome vero nei metadati.
    fn open_segment(&mut self, out: &mut Vec<u8>, kind: &str, options: &str, label: &str) {
        self.segments += 1;
        let raw = format!("{kind} {}", self.segments);
        out.push(b'(');
        out.push(raw.len() as u8);
        out.extend_from_slice(raw.as_bytes());
        if !options.is_empty() {
            push_options(out, options);
        }
        if !label.trim().is_empty() {
            out.push(b'm');
            push_i32(out, 1);
            out.extend_from_slice(b"name");
            push_i32(out, label.len() as i32 + 1);
            out.extend_from_slice(label.as_bytes());
            out.push(0);
        }
    }

    fn write_node(&mut self, out: &mut Vec<u8>, w: &mut dyn Write, idx: u32) -> io::Result<()> {
        let scene = self.scene;
        let label = scene.display_name(idx);
        match &scene.nodes[idx as usize] {
            SceneNode::Shape { mesh_id, .. } => {
                let Some(shape) = scene.shapes_data.get(*mesh_id as usize) else { return Ok(()) };
                if shape.indices.len() < 3 || shape.world_vertices.is_empty() {
                    return Ok(());
                }
                let [r, g, b, a] = scene.effective_user_color(idx).or(shape.color).unwrap_or(self.default_color);
                // product occurrence › part › representation item › internal container
                let product = format!("hx3d_type=product,name={}", option_value(&label));
                self.open_segment(out, "product occurrence", &product, &label);
                self.open_segment(out, "part", "hx3d_type=part", "");
                let item = format!("name={}", option_value(&label));
                self.open_segment(out, "representation item", &item, &label);
                // Colore delle facce: diffuse, più transmission se il pezzo è trasparente
                let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let transparency = (1.0 - a).clamp(0.0, 1.0);
                let transparent = transparency > 0.002;
                out.extend_from_slice(&[b'"', 0x01, if transparent { 0x09 } else { 0x01 }, 0, byte(r), byte(g), byte(b)]);
                if transparent {
                    let t = byte(transparency);
                    out.extend_from_slice(&[0, t, t, t]);
                }
                self.open_segment(out, "internal container", "", "");
                // Shell: float, prima passata (non è un riempimento), facce compresse in strisce,
                // con o senza normali, livello di dettaglio 0
                let with_normals =
                    self.normals && !shape.normals.is_empty() && shape.normals.len() == shape.render_vertex_count();
                out.extend_from_slice(&[b'S', 0, if with_normals { 0x1e } else { 0x16 }, 0]);
                if with_normals {
                    let count = shape.render_vertex_count();
                    push_i32(out, count as i32);
                    for r in 0..count {
                        shape.render_position(r).unwrap_or_default().iter().for_each(|&c| push_f32(out, c));
                    }
                    push_strips(out, shape.render_indices(), count);
                    // Normali polari su tutti i vertici, poi fine degli attributi
                    out.push(0x13);
                    for &n in &shape.normals {
                        let [theta, phi] = polar(n);
                        push_f32(out, theta);
                        push_f32(out, phi);
                    }
                    out.push(0);
                } else {
                    push_i32(out, shape.world_vertices.len() as i32);
                    for v in &shape.world_vertices {
                        v.iter().for_each(|&c| push_f32(out, c));
                    }
                    push_strips(out, &shape.indices, shape.world_vertices.len());
                }
                out.extend_from_slice(b"))))");
                self.stats.shapes_written += 1;
            }
            SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                let product = format!("hx3d_type=product,name={}", option_value(&label));
                self.open_segment(out, "product occurrence", &product, &label);
                for &c in children {
                    if self.has_visible.get(c as usize).copied().unwrap_or(false) {
                        self.write_node(out, w, c)?;
                    }
                }
                out.push(b')');
                self.stats.groups_written += 1;
            }
        }
        // Svuota a pezzi: il testo intero di una scocca occuperebbe centinaia di MB
        if out.len() > 1 << 22 {
            w.write_all(out)?;
            out.clear();
        }
        Ok(())
    }
}

/// Esporta le parti visibili (di tutta la scena o del sotto-albero `node`) in HSF.
/// `default_color` è il colore dei pezzi senza materiale; `normals` scrive le normali dei pezzi.
pub fn export_hsf(
    scene: &SceneGraph,
    node: Option<u32>,
    path: &std::path::Path,
    default_color: [f32; 4],
    normals: bool,
) -> io::Result<ExportStats> {
    let files = scene.open_files().count();
    let (roots, write_file_nodes) = match node {
        Some(n) => (vec![n], false),
        None => (scene.roots.clone(), files > 1),
    };
    // La scena è in millimetri (i file in metri si convertono all'apertura): l'HSF in metri
    let to_meters = MM_TO_METERS;
    let mut ctx = Ctx {
        scene,
        has_visible: compute_has_visible(scene),
        write_file_nodes,
        default_color,
        normals,
        segments: 0,
        stats: ExportStats::default(),
    };
    let mut top = Vec::new();
    for r in roots {
        ctx.expand(r, &mut top);
    }

    // Ingombro delle parti scritte
    let mut lo = glam::Vec3::splat(f32::MAX);
    let mut hi = glam::Vec3::splat(f32::MIN);
    let mut stack = top.clone();
    while let Some(n) = stack.pop() {
        if !ctx.has_visible.get(n as usize).copied().unwrap_or(false) {
            continue;
        }
        match &scene.nodes[n as usize] {
            SceneNode::Shape { mesh_id, .. } => {
                if let Some(b) = scene.mesh_bboxes.get(*mesh_id as usize).filter(|b| b.is_valid()) {
                    lo = lo.min(b.min);
                    hi = hi.max(b.max);
                }
            }
            other => stack.extend_from_slice(other.children()),
        }
    }
    if lo.x > hi.x {
        lo = glam::Vec3::ZERO;
        hi = glam::Vec3::ZERO;
    }

    let mut file = BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?);
    let mut head = b";; HSF V21.00 \n".to_vec();
    head.push(b'I');
    push_i32(&mut head, FILE_INFO);
    head.extend_from_slice(&[b'B', 0, 0]);
    for c in (lo * to_meters).to_array().into_iter().chain((hi * to_meters).to_array()) {
        push_f32(&mut head, c);
    }
    head.push(b'Z');
    file.write_all(&head)?;

    let mut z = flate2::write::ZlibEncoder::new(file, flate2::Compression::default());
    let mut out = Vec::with_capacity(1 << 23);
    // Segmento radice con la scala verso i metri, se serve
    let root_name = match node {
        Some(n) => scene.display_name(n),
        None => scene.sources.first().map(|s| s.name.clone()).unwrap_or_default(),
    };
    out.extend_from_slice(&[b'(', 10]);
    out.extend_from_slice(b"model_root");
    if !root_name.trim().is_empty() {
        out.push(b'm');
        push_i32(&mut out, 1);
        out.extend_from_slice(b"name");
        push_i32(&mut out, root_name.len() as i32 + 1);
        out.extend_from_slice(root_name.as_bytes());
        out.push(0);
    }
    if (to_meters - 1.0).abs() > 1e-6 {
        out.extend_from_slice(&[b'%', 0]);
        for v in [to_meters, 0.0, 0.0, 0.0, to_meters, 0.0, 0.0, 0.0, to_meters, 0.0, 0.0, 0.0] {
            push_f32(&mut out, v);
        }
    }
    for n in top {
        ctx.write_node(&mut out, &mut z, n)?;
    }
    out.push(b')');
    // Fine della compressione dentro il blocco, terminatore fuori, come negli originali
    out.push(b'z');
    z.write_all(&out)?;
    let mut file = z.finish()?;
    file.write_all(&[0])?;
    file.flush()?;
    Ok(ctx.stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &[u8] = br#"#VRML V2.0 utf8
DEF Assieme Transform { translation 10 0 0 children [
    DEF Piastra Shape { appearance Appearance { material Material { diffuseColor 1 0.5 0 transparency 0.25 } }
      geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] } coordIndex [ 0 1 2 3 -1 ] } }
    DEF Nascosto Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 5, 1 0 5, 0 1 5 ] } coordIndex [ 0 1 2 -1 ] } }
    DEF Lato Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 1, 1 0 1, 0 1 1 ] } coordIndex [ 0 1 2 -1 ] } }
] }
"#;

    #[test]
    fn exported_hsf_reopens_with_geometry_names_and_colors() {
        let mut scene = crate::parser::parse_wrl(SCENE);
        let hidden = (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == "Nascosto").unwrap();
        scene.set_subtree_visible(hidden, false);
        let path = std::env::temp_dir().join("gestore_wrl_test.hsf");
        let stats = export_hsf(&scene, None, &path, [0.5, 0.5, 0.5, 1.0], true).unwrap();
        assert_eq!(stats.shapes_written, 2);
        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b";; HSF V21.00 \nI"));

        let again = crate::parser::parse_wrl(&data);
        assert!(again.load_warnings.is_empty(), "{:?}", again.load_warnings);
        assert_eq!(again.shapes_data.len(), 2);
        let names: Vec<_> = (0..again.nodes.len() as u32).map(|n| again.display_name(n)).collect();
        for wanted in ["Assieme", "Piastra", "Lato"] {
            assert!(names.iter().any(|n| n == wanted), "manca {wanted}: {names:?}");
        }
        assert!(!names.iter().any(|n| n == "Nascosto"));
        // La scena è in millimetri: il file è in metri e si riapre con le stesse misure
        let piastra = again.shapes_data.iter().find(|s| s.world_vertices.len() == 4).unwrap();
        assert!(piastra.world_vertices.iter().any(|v| (v[0] - 11.0).abs() < 1e-4 && (v[1] - 1.0).abs() < 1e-4), "{:?}", piastra.world_vertices);
        assert_eq!(piastra.indices.len(), 6);
        let c = piastra.color.unwrap();
        assert!((c[0] - 1.0).abs() < 0.01 && (c[1] - 0.5).abs() < 0.01 && (c[3] - 0.75).abs() < 0.01, "{c:?}");
        let _ = std::fs::remove_file(path);
    }

    /// Scena in millimetri (una parte di 1,5 m): scala 0,001 sulla radice, ingombro in metri.
    #[test]
    fn millimeter_scene_keeps_its_size() {
        let scene = crate::parser::parse_wrl(
            br#"#VRML V2.0 utf8
DEF Pezzo Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1500 0 0, 0 800 0 ] } coordIndex [ 0 1 2 -1 ] } }
"#,
        );
        let path = std::env::temp_dir().join("gestore_wrl_test_mm.hsf");
        export_hsf(&scene, None, &path, [0.5, 0.5, 0.5, 1.0], true).unwrap();
        let data = std::fs::read(&path).unwrap();
        // Ingombro dopo `I` (5 byte) e `B` con i suoi 2 byte: x massima = 1,5 m
        let max_x = f32::from_le_bytes(data[15 + 5 + 3 + 12..15 + 5 + 3 + 16].try_into().unwrap());
        assert!((max_x - 1.5).abs() < 1e-6, "{max_x}");
        let again = crate::parser::parse_wrl(&data);
        let v = &again.shapes_data[0].world_vertices;
        assert!(v.iter().any(|v| (v[0] - 1500.0).abs() < 1e-3), "{v:?}");
        let _ = std::fs::remove_file(path);
    }

    /// Facce sempre compresse in strisce; normali polari (0x13) solo se richieste, scritte sui
    /// vertici di resa (tanti quante le normali del pezzo).
    #[test]
    fn faces_are_compressed_and_normals_are_optional() {
        let scene = crate::parser::parse_wrl(
            br#"#VRML V2.0 utf8
DEF Cubo Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 100 0 0, 100 100 0, 0 100 0, 0 0 100, 100 0 100, 100 100 100, 0 100 100 ] }
  coordIndex [ 0 3 2 1 -1, 4 5 6 7 -1, 0 1 5 4 -1, 1 2 6 5 -1, 2 3 7 6 -1, 3 0 4 7 -1 ] creaseAngle 0.5 } }
"#,
        );
        let shell = |normals: bool| {
            let path = std::env::temp_dir().join(format!("gestore_wrl_test_normali_{normals}.hsf"));
            export_hsf(&scene, None, &path, [0.5, 0.5, 0.5, 1.0], normals).unwrap();
            let data = std::fs::read(&path).unwrap();
            let again = crate::parser::parse_wrl(&data);
            assert_eq!(again.shapes_data.len(), 1);
            assert_eq!(again.shapes_data[0].indices.len(), 36);
            let _ = std::fs::remove_file(path);
            let z = data.iter().position(|&b| b == b'Z').unwrap();
            let mut plain = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&data[z + 1..]), &mut plain).unwrap();
            let s = plain.windows(4).position(|w| w == b"S  " || w == b"S  ").unwrap();
            let sub = plain[s + 2];
            let count = i32::from_le_bytes(plain[s + 4..s + 8].try_into().unwrap());
            assert_eq!(*data.last().unwrap(), 0);
            (sub, count)
        };
        assert_eq!(shell(false), (0x16, 8));
        let render = scene.shapes_data[0].render_vertex_count() as i32;
        assert_eq!(scene.shapes_data[0].normals.len(), render as usize);
        assert_eq!(shell(true), (0x1e, render));
    }

    #[test]
    fn polar_normals_point_like_the_originals() {
        for n in [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.6, -0.48, 0.64]] {
            let [t, p] = polar(n);
            let back = [t.cos() * p.sin(), t.sin() * p.sin(), p.cos()];
            assert!((0..3).all(|k| (back[k] - n[k]).abs() < 1e-5), "{n:?} → {back:?}");
        }
    }

    #[test]
    fn same_names_stay_separate_segments() {
        let scene = crate::parser::parse_wrl(
            br#"#VRML V2.0 utf8
DEF Pezzo Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }
DEF Pezzo Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 9, 1 0 9, 0 1 9 ] } coordIndex [ 0 1 2 -1 ] } }
"#,
        );
        let path = std::env::temp_dir().join("gestore_wrl_test_nomi.hsf");
        export_hsf(&scene, None, &path, [0.5, 0.5, 0.5, 1.0], true).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        assert_eq!(again.shapes_data.len(), 2);
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod real_file {
    /// Esporta in HSF dei file reali e li rilegge: `GESTORE_HSF_EXPORT="a.iv|b.hsf|..."`,
    /// `GESTORE_HSF_OUT` = cartella di destinazione (per provarli nel visualizzatore HOOPS).
    /// Con `GESTORE_HSF_ANONIMO` i file si chiamano `prova_1.hsf`, … e dentro hanno solo nomi
    /// generici (`gruppo 3`, `pezzo 7`), da portare fuori senza i nomi delle matematiche.
    #[test]
    #[ignore]
    fn hsf_da_verificare() {
        let list = std::env::var("GESTORE_HSF_EXPORT").expect("GESTORE_HSF_EXPORT");
        let dir = std::path::PathBuf::from(std::env::var("GESTORE_HSF_OUT").expect("GESTORE_HSF_OUT"));
        let anonymous = std::env::var_os("GESTORE_HSF_ANONIMO").is_some();
        std::fs::create_dir_all(&dir).unwrap();
        for (k, src) in list.split('|').enumerate() {
            let src = std::path::Path::new(src);
            let mut scene = crate::parser::parse_wrl(&std::fs::read(src).unwrap());
            let out = if anonymous {
                for n in 0..scene.nodes.len() as u32 {
                    let kind = if matches!(scene.nodes[n as usize], crate::scene_graph::SceneNode::Shape { .. }) { "pezzo" } else { "gruppo" };
                    scene.rename(n, &format!("{kind} {n}"));
                }
                for (i, s) in scene.sources.iter_mut().enumerate() {
                    s.name = format!("modello {}", i + 1);
                }
                dir.join(format!("prova_{}.hsf", k + 1))
            } else {
                dir.join(format!("{}_gestore.hsf", src.file_stem().unwrap().to_string_lossy()))
            };
            let t = std::time::Instant::now();
            let normals = std::env::var_os("GESTORE_HSF_SENZA_NORMALI").is_none();
            let stats = super::export_hsf(&scene, None, &out, [0.5, 0.5, 0.5, 1.0], normals).unwrap();
            let size = std::fs::metadata(&out).unwrap().len();
            let written = t.elapsed();
            let again = crate::parser::parse_wrl(&std::fs::read(&out).unwrap());
            let tris = |s: &crate::scene_graph::SceneGraph| s.shapes_data.iter().map(|s| s.indices.len() / 3).sum::<usize>();
            let (a, b) = (scene.compute_bounding_box(), again.compute_bounding_box());
            println!(
                "{}: {} pezzi, {:.1} MB in {written:?}; riletto {} pezzi, triangoli {} → {}, ingombro {:?}..{:?} → {:?}..{:?}, avvisi {:?}",
                out.display(),
                stats.shapes_written,
                size as f64 / 1e6,
                again.shapes_data.len(),
                tris(&scene),
                tris(&again),
                a.min,
                a.max,
                b.min,
                b.max,
                again.load_warnings
            );
            // In lettura l'HSF unisce i vertici con la stessa posizione: spariscono solo i
            // triangoli di area nulla tra vertici sovrapposti (36 su 3,3 milioni nella scocca)
            assert!(tris(&again) <= tris(&scene) && tris(&scene) - tris(&again) <= tris(&scene) / 10_000);
            assert_eq!(again.shapes_data.len(), stats.shapes_written as usize);
        }
    }
}
