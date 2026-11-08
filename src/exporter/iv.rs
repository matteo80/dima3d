//! Export Open Inventor (`.iv`, versione 2.1), di testo o binario, il formato nativo di Coin3D
//! (FreeCAD) e dei programmi basati su Open Inventor.
//!
//! Stessa struttura dell'export WRL: solo le parti visibili, un `Separator` con nome per ogni
//! gruppo e pezzo, coordinate in world-space (nessuna trasformazione), colori e trasparenza in un
//! `Material`. `ShapeHints` con `UNKNOWN_ORDERING` dà l'illuminazione sui due lati (come
//! `solid FALSE` del VRML). Le normali, se richieste, sono quelle dei pezzi con
//! `PER_VERTEX_INDEXED`; senza, `creaseAngle 0.5` e il visualizzatore le ricalcola.
//!
//! Formato binario (ricavato dai file scritti da Coin e verificato rileggendoli con Coin):
//! intestazione `#Inventor V2.1 binary  \n`, poi numeri a 32 bit big-endian. Le stringhe sono
//! lunghezza + caratteri completati con zeri a multipli di 4 byte. Ogni nodo: [`DEF` nome] classe,
//! parola di flag (2 = gruppo), numero di campi, i campi (nome, valore, parola 0) e, per i gruppi,
//! numero di figli e figli. I campi multipli hanno il numero di valori davanti.

use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

use super::writer::{compute_has_visible, coordinate_decimals, push_fixed, push_uint, ExportStats};
use crate::scene_graph::{NodeKind, SceneGraph, SceneNode};

/// Cifre decimali delle normali nel formato di testo (errore massimo ~0,05°).
const NORMAL_DECIMALS: u32 = 3;

/// Valore di un campo.
enum Value<'a> {
    /// Campo singolo (`SFFloat`)
    Float(f32),
    /// Campo multiplo con un solo valore (`MFFloat`, es. la trasparenza del Material)
    OneFloat(f32),
    Enum(&'a str),
    /// Colori o vettori (`MFColor`, `MFVec3f`), con le cifre decimali per il testo
    Vec3s(&'a [[f32; 3]], u32),
    /// Indici dei triangoli (`MFInt32`), ognuno chiuso da -1
    Triangles(&'a [u32]),
}

/// Scrittura dei nodi nei due formati.
trait IvWriter {
    fn header(&mut self) -> io::Result<()>;
    /// Inizio di un nodo; per i gruppi `children` è il numero di figli che seguiranno.
    fn begin(&mut self, class: &str, def: Option<&str>, fields: &[(&str, Value)], children: Option<u32>) -> io::Result<()>;
    /// Fine di un gruppo (dopo i figli).
    fn end_group(&mut self) -> io::Result<()>;
    /// Svuota il buffer sul file.
    fn finish(&mut self) -> io::Result<()>;
    /// Nodo senza figli.
    fn leaf(&mut self, class: &str, fields: &[(&str, Value)]) -> io::Result<()> {
        self.begin(class, None, fields, None)
    }
}

struct Ascii<W: Write> {
    w: W,
    buffer: Vec<u8>,
}

impl<W: Write> IvWriter for Ascii<W> {
    fn header(&mut self) -> io::Result<()> {
        self.w.write_all(b"#Inventor V2.1 ascii\n# Esportato da Gestore WRL\n\n")
    }

    fn begin(&mut self, class: &str, def: Option<&str>, fields: &[(&str, Value)], children: Option<u32>) -> io::Result<()> {
        let out = &mut self.buffer;
        out.clear();
        if let Some(name) = def {
            out.extend_from_slice(b"DEF ");
            out.extend_from_slice(name.as_bytes());
            out.push(b' ');
        }
        out.extend_from_slice(class.as_bytes());
        out.extend_from_slice(b" {");
        for (name, value) in fields {
            out.push(b' ');
            out.extend_from_slice(name.as_bytes());
            out.push(b' ');
            match value {
                Value::Float(v) | Value::OneFloat(v) => out.extend_from_slice(v.to_string().as_bytes()),
                Value::Enum(e) => out.extend_from_slice(e.as_bytes()),
                Value::Vec3s(list, decimals) => {
                    out.extend_from_slice(b"[\n");
                    for v in *list {
                        push_fixed(out, v[0], *decimals);
                        out.push(b' ');
                        push_fixed(out, v[1], *decimals);
                        out.push(b' ');
                        push_fixed(out, v[2], *decimals);
                        out.extend_from_slice(b",\n");
                    }
                    out.push(b']');
                }
                Value::Triangles(indices) => {
                    out.extend_from_slice(b"[\n");
                    for t in indices.chunks_exact(3) {
                        for &i in t {
                            push_uint(out, i as u64);
                            out.push(b',');
                        }
                        out.extend_from_slice(b"-1,\n");
                    }
                    out.push(b']');
                }
            }
        }
        out.extend_from_slice(if children.is_some() { b"\n" } else { b" }\n" });
        self.w.write_all(out)
    }

    fn end_group(&mut self) -> io::Result<()> {
        self.w.write_all(b"}\n")
    }

    fn finish(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

struct Binary<W: Write> {
    w: W,
    buffer: Vec<u8>,
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// Stringa: lunghezza, caratteri, zeri fino a un multiplo di 4 byte.
fn push_string(out: &mut Vec<u8>, s: &str) {
    push_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
    out.resize(out.len().next_multiple_of(4), 0);
}

impl<W: Write> IvWriter for Binary<W> {
    fn header(&mut self) -> io::Result<()> {
        self.w.write_all(b"#Inventor V2.1 binary  \n")
    }

    fn begin(&mut self, class: &str, def: Option<&str>, fields: &[(&str, Value)], children: Option<u32>) -> io::Result<()> {
        let out = &mut self.buffer;
        out.clear();
        if let Some(name) = def {
            push_string(out, "DEF");
            push_string(out, name);
        }
        push_string(out, class);
        push_u32(out, if children.is_some() { 2 } else { 0 });
        push_u32(out, fields.len() as u32);
        for (name, value) in fields {
            push_string(out, name);
            match value {
                Value::Float(v) => push_f32(out, *v),
                Value::OneFloat(v) => {
                    push_u32(out, 1);
                    push_f32(out, *v);
                }
                Value::Enum(e) => push_string(out, e),
                Value::Vec3s(list, _) => {
                    push_u32(out, list.len() as u32);
                    out.reserve(list.len() * 12);
                    for v in *list {
                        v.iter().for_each(|&c| push_f32(out, c));
                    }
                }
                Value::Triangles(indices) => {
                    push_u32(out, (indices.len() / 3 * 4) as u32);
                    out.reserve(indices.len() / 3 * 16);
                    for t in indices.chunks_exact(3) {
                        t.iter().for_each(|&i| push_u32(out, i));
                        push_u32(out, u32::MAX); // -1
                    }
                }
            }
            push_u32(out, 0); // flag del campo
        }
        if let Some(n) = children {
            push_u32(out, n);
        }
        self.w.write_all(out)
    }

    fn end_group(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

struct Ctx<'a> {
    scene: &'a SceneGraph,
    has_visible: Vec<bool>,
    write_file_nodes: bool,
    normals: bool,
    decimals: u32,
    default_color: [f32; 4],
    names: HashMap<String, u32>,
    stats: ExportStats,
}

impl Ctx<'_> {
    fn unique(&mut self, label: &str) -> String {
        let base = super::sanitize_identifier(label);
        let n = self.names.entry(base.clone()).or_insert(0);
        *n += 1;
        if *n == 1 { base } else { format!("{base}_{n}") }
    }

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

    fn write_node(&mut self, w: &mut dyn IvWriter, idx: u32) -> io::Result<()> {
        let scene = self.scene;
        let label = scene.display_name(idx);
        let is_file = scene.info.get(idx as usize).is_some_and(|i| i.kind == NodeKind::File);
        let label = match label.rsplit_once('.') {
            Some((stem, ext)) if is_file && ext.eq_ignore_ascii_case("wrl") => stem.to_string(),
            _ => label,
        };
        let name = self.unique(&label);
        match &scene.nodes[idx as usize] {
            SceneNode::Shape { mesh_id, .. } => {
                let Some(shape) = scene.shapes_data.get(*mesh_id as usize) else { return Ok(()) };
                let [r, g, b, a] = scene.effective_user_color(idx).or(shape.color).unwrap_or(self.default_color);
                let with_normals =
                    self.normals && !shape.normals.is_empty() && shape.normals.len() == shape.render_vertex_count();
                let parts = 4 + u32::from(with_normals) * 2;
                w.begin("Separator", Some(&name), &[], Some(parts))?;
                let color = [[r, g, b]];
                let transparency = (1.0 - a).clamp(0.0, 1.0);
                if transparency > 0.0005 {
                    w.leaf("Material", &[("diffuseColor", Value::Vec3s(&color, 4)), ("transparency", Value::OneFloat(transparency))])?;
                } else {
                    w.leaf("Material", &[("diffuseColor", Value::Vec3s(&color, 4))])?;
                }
                let mut hints = vec![("vertexOrdering", Value::Enum("UNKNOWN_ORDERING")), ("shapeType", Value::Enum("UNKNOWN_SHAPE_TYPE"))];
                if !with_normals {
                    hints.push(("creaseAngle", Value::Float(0.5)));
                }
                w.leaf("ShapeHints", &hints)?;
                w.leaf("Coordinate3", &[("point", Value::Vec3s(&shape.world_vertices, self.decimals))])?;
                if with_normals {
                    w.leaf("Normal", &[("vector", Value::Vec3s(&shape.normals, NORMAL_DECIMALS))])?;
                    w.leaf("NormalBinding", &[("value", Value::Enum("PER_VERTEX_INDEXED"))])?;
                    w.leaf(
                        "IndexedFaceSet",
                        &[("coordIndex", Value::Triangles(&shape.indices)), ("normalIndex", Value::Triangles(shape.render_indices()))],
                    )?;
                } else {
                    w.leaf("IndexedFaceSet", &[("coordIndex", Value::Triangles(&shape.indices))])?;
                }
                w.end_group()?;
                self.stats.shapes_written += 1;
            }
            SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                let mut kids = Vec::new();
                for &c in children {
                    if self.has_visible.get(c as usize).copied().unwrap_or(false) {
                        kids.push(c);
                    }
                }
                w.begin("Separator", Some(&name), &[], Some(kids.len() as u32))?;
                for c in kids {
                    self.write_node(w, c)?;
                }
                w.end_group()?;
                self.stats.groups_written += 1;
            }
        }
        Ok(())
    }
}

fn write_all(w: &mut dyn IvWriter, ctx: &mut Ctx, top: &[u32]) -> io::Result<()> {
    w.header()?;
    // Un solo nodo radice: i lettori Inventor leggono il primo nodo del file
    w.begin("Separator", None, &[], Some(top.len() as u32))?;
    for &n in top {
        ctx.write_node(w, n)?;
    }
    w.end_group()?;
    w.finish()
}

/// Esporta le parti visibili (di tutta la scena o del sotto-albero `node`) in Open Inventor.
/// `default_color` è il colore dei pezzi senza materiale.
pub fn export_iv(
    scene: &SceneGraph,
    node: Option<u32>,
    path: &std::path::Path,
    binary: bool,
    normals: bool,
    default_color: [f32; 4],
) -> io::Result<ExportStats> {
    let file = BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?);
    let files = scene.open_files().count();
    let (roots, write_file_nodes) = match node {
        Some(n) => (vec![n], false),
        None => (scene.roots.clone(), files > 1),
    };
    let mut ctx = Ctx {
        scene,
        has_visible: compute_has_visible(scene),
        write_file_nodes,
        normals,
        decimals: coordinate_decimals(scene),
        default_color,
        names: HashMap::new(),
        stats: ExportStats::default(),
    };
    let mut top = Vec::new();
    for r in roots {
        ctx.expand(r, &mut top);
    }
    if binary {
        write_all(&mut Binary { w: file, buffer: Vec::new() }, &mut ctx, &top)?;
    } else {
        write_all(&mut Ascii { w: file, buffer: Vec::new() }, &mut ctx, &top)?;
    }
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
    DEF Lato.v2 Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 1, 1 0 1, 0 1 1 ] } coordIndex [ 0 1 2 -1 ] } }
] }
"#;

    pub(super) fn scene() -> SceneGraph {
        let mut scene = crate::parser::parse_wrl(SCENE);
        let hidden = (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == "Nascosto").unwrap();
        scene.set_subtree_visible(hidden, false);
        scene
    }

    #[test]
    fn ascii_has_named_separators_colors_and_world_coordinates() {
        let path = std::env::temp_dir().join("gestore_wrl_test.iv");
        let stats = export_iv(&scene(), None, &path, false, true, [0.5, 0.5, 0.5, 1.0]).unwrap();
        assert_eq!(stats.shapes_written, 2);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("#Inventor V2.1 ascii\n"), "{text}");
        for wanted in [
            "DEF Assieme Separator",
            "DEF Piastra Separator",
            "DEF Lato_v2 Separator",
            "diffuseColor [\n1 0.5 0,\n]",
            "transparency 0.25",
            "vertexOrdering UNKNOWN_ORDERING",
            "Coordinate3 { point [\n10 0 0,\n11 0 0,",
            "NormalBinding { value PER_VERTEX_INDEXED }",
        ] {
            assert!(text.contains(wanted), "manca «{wanted}»:\n{text}");
        }
        assert!(!text.contains("Nascosto"));
        assert_eq!(text.matches('{').count(), text.matches('}').count());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn binary_strings_and_numbers_are_aligned_big_endian() {
        let mut out = Vec::new();
        push_string(&mut out, "DEF");
        push_string(&mut out, "Material");
        assert_eq!(out, b"\0\0\0\x03DEF\0\0\0\0\x08Material");
        let path = std::env::temp_dir().join("gestore_wrl_test_bin.iv");
        export_iv(&scene(), None, &path, true, false, [0.5, 0.5, 0.5, 1.0]).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"#Inventor V2.1 binary  \n"));
        assert_eq!(data.len() % 4, 0);
        // Le coordinate in world-space ci sono come float big-endian
        let ten = 10.0f32.to_be_bytes();
        assert!(data.windows(4).any(|w| w == ten));
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod real_file {
    /// Scrive in %TEMP% gli IV (testo, binario, binario con normali) della scena di prova e, con
    /// `GESTORE_WRL_FILE`, di un file reale, da rileggere con Coin:
    /// `cargo test --release iv_da_verificare -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn iv_da_verificare() {
        let mut scenes = vec![("prova", super::tests::scene())];
        if let Ok(path) = std::env::var("GESTORE_WRL_FILE") {
            scenes.push(("reale", crate::parser::parse_wrl(&std::fs::read(&path).unwrap())));
        }
        for (name, scene) in &scenes {
            for (suffix, binary, normals) in [("testo", false, true), ("bin", true, false), ("bin_normali", true, true)] {
                let out = std::env::temp_dir().join(format!("gestore_iv_{name}_{suffix}.iv"));
                let t = std::time::Instant::now();
                let stats = super::export_iv(scene, None, &out, binary, normals, [0.5, 0.5, 0.5, 1.0]).unwrap();
                let size = std::fs::metadata(&out).unwrap().len();
                println!("{}: {} pezzi, {:.1} MB in {:?}", out.display(), stats.shapes_written, size as f64 / 1e6, t.elapsed());
            }
        }
    }
}
