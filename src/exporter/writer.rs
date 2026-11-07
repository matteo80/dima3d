use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

use crate::scene_graph::{NodeKind, SceneGraph, SceneNode, NO_PARENT};

/// Statistiche dell'esportazione.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExportStats {
    pub shapes_written: usize,
    pub groups_written: usize,
}

/// Esporta i soli elementi visibili in un nuovo file WRL "pulito".
///
/// * La gerarchia dei macro-oggetti viene mantenuta come `DEF Nome Group { children [...] }`
///   (i contenitori senza parti visibili vengono omessi): riaprendo il file
///   nell'applicazione si ritrova lo stesso albero.
/// * Le coordinate sono scritte in world-space (le matrici Transform sono già
///   applicate), quindi non servono più nodi Transform.
/// * L'`appearance` originale viene copiata verbatim quando è autonoma (niente
///   DEF/USE al suo interno); altrimenti si usa un materiale neutro.
/// * La geometria conserva i campi del file originale: vedi [`super::ifs`].
///
/// Con più file aperti ognuno diventa un `Group` col nome del file; con un solo file il suo
/// nodo non viene scritto, così riaprendo l'export l'albero è quello di partenza.
pub fn export_visible_wrl(scene: &SceneGraph, output_path: &std::path::Path) -> io::Result<ExportStats> {
    let files = scene.open_files().count();
    export_roots(scene, &scene.roots, files > 1, output_path)
}

/// Esporta le parti visibili di un solo sotto-albero (es. un file dell'insieme).
pub fn export_subtree_wrl(scene: &SceneGraph, node: u32, output_path: &std::path::Path) -> io::Result<ExportStats> {
    export_roots(scene, &[node], false, output_path)
}

fn export_roots(
    scene: &SceneGraph,
    roots: &[u32],
    write_file_nodes: bool,
    output_path: &std::path::Path,
) -> io::Result<ExportStats> {
    let file = std::fs::File::create(output_path)?;
    let mut w = BufWriter::with_capacity(1 << 20, file);

    let has_visible = compute_has_visible(scene);
    let mut visible = 0;
    for &r in roots {
        scene.for_each_subtree_node(r, |i| {
            if has_visible[i as usize] && matches!(scene.nodes[i as usize], SceneNode::Shape { .. }) {
                visible += 1;
            }
        });
    }

    writeln!(w, "#VRML V2.0 utf8")?;
    writeln!(w, "# Esportato da Gestore WRL")?;
    writeln!(w, "# Shape visibili: {visible}")?;
    writeln!(w)?;

    let mut ctx = ExportCtx {
        scene,
        has_visible: &has_visible,
        write_file_nodes,
        names: HashMap::new(),
        geometry_defs: (0..scene.sources.len().max(1)).map(|_| Default::default()).collect(),
        stats: ExportStats::default(),
    };
    for &root in roots {
        ctx.write_node(&mut w, root, 0)?;
    }

    w.flush()?;
    tracing::info!(
        "Esportato {}: {} shape, {} gruppi",
        output_path.display(),
        ctx.stats.shapes_written,
        ctx.stats.groups_written
    );
    Ok(ctx.stats)
}

/// Per ogni nodo: true se il suo sotto-albero contiene almeno una Shape visibile.
fn compute_has_visible(scene: &SceneGraph) -> Vec<bool> {
    let mut flags: Vec<bool> = scene
        .nodes
        .iter()
        .zip(&scene.info)
        .map(|(n, i)| !i.deleted && matches!(n, SceneNode::Shape { visible: true, .. }))
        .collect();
    // L'arena è in pre-order: i figli hanno indice maggiore del padre
    for i in (0..flags.len()).rev() {
        let p = scene.info[i].parent;
        if flags[i] && p != NO_PARENT && (p as usize) < i {
            flags[p as usize] = true;
        }
    }
    flags
}

struct ExportCtx<'a> {
    scene: &'a SceneGraph,
    has_visible: &'a [bool],
    /// Scrivere i nodi dei file come Group (con più file); altrimenti solo il loro contenuto
    write_file_nodes: bool,
    /// Nomi DEF già usati → contatore, per garantirne l'unicità
    names: HashMap<String, u32>,
    /// Nodi di geometria con DEF in ogni file sorgente, per risolvere i USE
    geometry_defs: Vec<super::ifs::GeometryDefs<'a>>,
    stats: ExportStats,
}

impl ExportCtx<'_> {
    fn write_node(&mut self, w: &mut impl Write, idx: u32, depth: usize) -> io::Result<()> {
        if !self.has_visible.get(idx as usize).copied().unwrap_or(false) {
            return Ok(());
        }
        // Copia del riferimento con lifetime 'a: i borrow sui nodi non bloccano `&mut self`
        let scene = self.scene;
        let ind = "  ".repeat(depth);
        let is_file = scene.info.get(idx as usize).is_some_and(|i| i.kind == NodeKind::File);
        if is_file && !self.write_file_nodes {
            for &c in scene.nodes[idx as usize].children() {
                self.write_node(w, c, depth)?;
            }
            return Ok(());
        }
        let label = scene.display_name(idx);
        // Il nodo di un file prende il nome senza estensione (es. "Scocca_B" da "Scocca_B.wrl")
        let label = match label.rsplit_once('.') {
            Some((stem, ext)) if is_file && ext.eq_ignore_ascii_case("wrl") => stem.to_string(),
            _ => label,
        };
        let name = self.unique_def(&label);

        match &scene.nodes[idx as usize] {
            SceneNode::Shape { mesh_id, source_start, source_end, .. } => {
                let Some(shape) = scene.shapes_data.get(*mesh_id as usize) else {
                    return Ok(());
                };
                writeln!(w, "{ind}DEF {name} Shape {{")?;

                // Priorità: colore impostato dall'utente > appearance originale (se autonoma)
                // > colore letto dal Material (es. via USE) > materiale neutro
                let source = scene.source_for_mesh(*mesh_id);
                let defs = &mut self.geometry_defs[scene.source_index(*mesh_id).unwrap_or(0)];
                let src = source.get(*source_start..(*source_end).min(source.len()));
                let user_color = scene.effective_user_color(idx);
                match (user_color, src.and_then(extract_appearance)) {
                    (None, Some(app)) => {
                        w.write_all(ind.as_bytes())?;
                        w.write_all(b"  ")?;
                        w.write_all(app)?;
                        writeln!(w)?;
                    }
                    _ => {
                        let [r, g, b, a] = user_color
                            .or(shape.color)
                            .unwrap_or([0.75, 0.78, 0.84, 1.0]);
                        let transparency = (1.0 - a).clamp(0.0, 1.0);
                        if transparency > 0.0005 {
                            writeln!(
                                w,
                                "{ind}  appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} transparency {transparency} }} }}"
                            )?;
                        } else {
                            writeln!(
                                w,
                                "{ind}  appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} }} }}"
                            )?;
                        }
                    }
                }

                writeln!(w, "{ind}  geometry IndexedFaceSet {{")?;
                // Campi originali del file (poligoni, normali...); se non riutilizzabili,
                // i triangoli calcolati dal parser
                let original = src.is_some()
                    && super::ifs::write_fields(
                        w,
                        &ind,
                        source,
                        *source_start,
                        source_end.saturating_sub(1),
                        shape,
                        user_color.is_some(),
                        defs,
                    )?;
                if !original {
                    let pad = format!("{ind}    ");
                    writeln!(w, "{pad}solid FALSE")?;
                    writeln!(w, "{pad}creaseAngle 0.5")?;
                    super::ifs::write_points(w, &pad, shape)?;
                    write!(w, "{pad}coordIndex [")?;
                    for (i, tri) in shape.indices.chunks_exact(3).enumerate() {
                        if i % 8 == 0 {
                            write!(w, "\n{pad}  ")?;
                        }
                        write!(w, "{} {} {} -1, ", tri[0], tri[1], tri[2])?;
                    }
                    writeln!(w, "\n{pad}]")?;
                }
                writeln!(w, "{ind}  }}")?;
                writeln!(w, "{ind}}}")?;
                self.stats.shapes_written += 1;
            }
            SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                writeln!(w, "{ind}DEF {name} Group {{")?;
                writeln!(w, "{ind}  children [")?;
                for &c in children {
                    self.write_node(w, c, depth + 2)?;
                }
                writeln!(w, "{ind}  ]")?;
                writeln!(w, "{ind}}}")?;
                self.stats.groups_written += 1;
            }
        }
        Ok(())
    }

    /// Converte un'etichetta in un identificatore VRML valido e univoco nel file.
    fn unique_def(&mut self, label: &str) -> String {
        let base = sanitize_identifier(label);
        let n = self.names.entry(base.clone()).or_insert(0);
        *n += 1;
        if *n == 1 {
            base
        } else {
            format!("{base}_{n}")
        }
    }
}

/// Identificatore VRML: niente spazi/delimitatori, non inizia con cifra/segno,
/// non coincide con parole riservate.
pub fn sanitize_identifier(label: &str) -> String {
    let mut out: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let starts_bad = out
        .chars()
        .next()
        .map(|c| c.is_ascii_digit() || c == '-' || c == '.' || c == '+')
        .unwrap_or(true);
    const RESERVED: &[&str] = &[
        "DEF", "USE", "PROTO", "EXTERNPROTO", "IS", "ROUTE", "TO", "TRUE", "FALSE", "NULL",
        "eventIn", "eventOut", "exposedField", "field",
    ];
    if starts_bad || RESERVED.contains(&out.as_str()) {
        out.insert(0, '_');
    }
    out
}

/// Cerca il campo `appearance` di primo livello in un nodo Shape (`src` inizia con `{`).
/// Ritorna i byte `appearance Appearance { ... }` se il blocco è autonomo
/// (senza DEF/USE, che potrebbero riferirsi a nodi non esportati).
pub fn extract_appearance(src: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    let mut depth = 0i32;
    while i < src.len() {
        let c = src[i];
        match c {
            b'#' => i = skip_comment(src, i),
            b'"' => i = skip_string(src, i),
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                i += 1;
            }
            c if depth == 1 && (c.is_ascii_alphabetic() || c == b'_') => {
                let start = i;
                while i < src.len() && (src[i].is_ascii_alphanumeric() || src[i] == b'_') {
                    i += 1;
                }
                if &src[start..i] != b"appearance" {
                    continue;
                }
                // Trova la `{` del nodo Appearance
                let mut j = i;
                while j < src.len() && src[j] != b'{' {
                    if src[j] == b'#' {
                        j = skip_comment(src, j);
                        continue;
                    }
                    if src[j] == b'}' || src[j] == b']' {
                        return None; // es. "appearance USE Mat"
                    }
                    j += 1;
                }
                let close = matching_brace(src, j)?;
                let block = &src[start..=close];
                if contains_word(block, b"USE") || contains_word(block, b"DEF") {
                    return None;
                }
                return Some(block);
            }
            _ => i += 1,
        }
    }
    None
}

pub(super) fn skip_comment(src: &[u8], mut i: usize) -> usize {
    while i < src.len() && src[i] != b'\n' {
        i += 1;
    }
    i
}

pub(super) fn skip_string(src: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < src.len() {
        match src[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

pub(super) fn matching_brace(src: &[u8], open: usize) -> Option<usize> {
    matching_close(src, open, b'{', b'}')
}

pub(super) fn matching_bracket(src: &[u8], open: usize) -> Option<usize> {
    matching_close(src, open, b'[', b']')
}

fn matching_close(src: &[u8], open: usize, open_ch: u8, close_ch: u8) -> Option<usize> {
    if src.get(open) != Some(&open_ch) {
        return None;
    }
    let mut depth = 0i32;
    let mut i = open;
    while i < src.len() {
        match src[i] {
            b'#' => {
                i = skip_comment(src, i);
                continue;
            }
            b'"' => {
                i = skip_string(src, i);
                continue;
            }
            c if c == open_ch => depth += 1,
            c if c == close_ch => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn contains_word(hay: &[u8], word: &[u8]) -> bool {
    let is_id = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    hay.windows(word.len()).enumerate().any(|(i, w)| {
        w == word
            && (i == 0 || !is_id(hay[i - 1]))
            && hay.get(i + word.len()).map(|&c| !is_id(c)).unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_is_extracted() {
        let src = b"{ appearance Appearance { material Material { diffuseColor 1 0 0 } } geometry IndexedFaceSet { } }";
        let a = extract_appearance(src).unwrap();
        assert_eq!(a, &b"appearance Appearance { material Material { diffuseColor 1 0 0 } }"[..]);
    }

    #[test]
    fn appearance_with_use_is_rejected() {
        let src = b"{ appearance Appearance { material USE Rosso } geometry IndexedFaceSet { } }";
        assert!(extract_appearance(src).is_none());
        let src = b"{ appearance USE Look geometry IndexedFaceSet { } }";
        assert!(extract_appearance(src).is_none());
    }

    #[test]
    fn identifiers_are_sanitized() {
        assert_eq!(sanitize_identifier("Componente 1"), "Componente_1");
        assert_eq!(sanitize_identifier("1Pezzo"), "_1Pezzo");
        assert_eq!(sanitize_identifier("TRUE"), "_TRUE");
        assert_eq!(sanitize_identifier("Part-1.v2"), "Part-1.v2");
    }

    #[test]
    fn export_roundtrip_keeps_hierarchy_and_world_coords() {
        let wrl = br#"#VRML V2.0 utf8
DEF Assieme Transform {
    translation 10 0 0
    children [
        DEF A Group { children [
            Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
                    geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
        ] }
        DEF B Group { children [
            Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 5, 1 0 5, 0 1 5 ] } coordIndex [ 0, 1, 2, -1 ] } }
        ] }
    ]
}
"#;
        let mut scene = crate::parser::parse_wrl(wrl);
        let root = scene.roots[0];
        let b = scene.nodes[root as usize].children()[1];
        scene.set_subtree_visible(b, false);

        let path = std::env::temp_dir().join("gestore_wrl_export_test.wrl");
        let stats = export_visible_wrl(&scene, &path).unwrap();
        assert_eq!(stats.shapes_written, 1);

        let out = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("DEF Assieme Group"));
        assert!(text.contains("DEF A Group"));
        assert!(!text.contains("DEF B Group"));
        assert!(text.contains("diffuseColor 1 0 0"));
        // Coordinate world-space: x traslata di 10
        assert!(text.contains("10 0 0, 11 0 0, 10 1 0"));

        // Il file esportato si riapre con la stessa gerarchia
        let again = crate::parser::parse_wrl(&out);
        assert_eq!(again.all_shape_indices().len(), 1);
        assert_eq!(again.display_name(again.roots[0]), "Assieme");
        let _ = std::fs::remove_file(path);
    }

    /// Con due file: l'export dell'insieme ha un Group per file; i `USE` di ogni file trovano
    /// il `DEF` del proprio file (entrambi i file definiscono "Tri"); riaprendo, la geometria è uguale.
    #[test]
    fn export_of_several_files() {
        use crate::scene_graph::node::multi_file_tests::two_files;
        let scene = two_files();
        let path = std::env::temp_dir().join("gestore_wrl_export_insieme.wrl");
        let stats = export_visible_wrl(&scene, &path).unwrap();
        assert_eq!(stats.shapes_written, 4);
        let out = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("DEF a Group") && text.contains("DEF b Group"), "{text}");
        let again = crate::parser::parse_wrl(&out);
        let vertices: Vec<_> = again.shapes_data.iter().map(|s| s.world_vertices.clone()).collect();
        let original: Vec<_> = scene.shapes_data.iter().map(|s| s.world_vertices.clone()).collect();
        assert_eq!(vertices, original);
        let names: Vec<String> = again.roots.iter().map(|&r| again.display_name(r)).collect();
        assert_eq!(names, vec!["a", "b"]);

        // Un solo file: il suo contenuto, senza il nodo del file
        let single = std::env::temp_dir().join("gestore_wrl_export_singolo.wrl");
        let stats = export_subtree_wrl(&scene, scene.roots[1], &single).unwrap();
        assert_eq!(stats.shapes_written, 2);
        let again = crate::parser::parse_wrl(&std::fs::read(&single).unwrap());
        assert_eq!(again.display_name(again.roots[0]), "Sportello");
        assert_eq!(again.shapes_data[1].world_vertices, scene.shapes_data[3].world_vertices);

        // Chiuso un file, l'insieme torna a essere un file solo: niente Group del file
        let mut scene = scene;
        scene.delete_node(scene.roots[0]);
        export_visible_wrl(&scene, &path).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        assert_eq!(again.display_name(again.roots[0]), "Sportello");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(single);
    }

    #[test]
    fn export_writes_user_colors_and_names() {
        let wrl = br#"#VRML V2.0 utf8
DEF Assieme Group { children [
    DEF A Group { children [
        Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
                geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
    ] }
    DEF B Group { children [
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 5, 1 0 5, 0 1 5 ] } coordIndex [ 0, 1, 2, -1 ] } }
    ] }
] }
"#;
        let mut scene = crate::parser::parse_wrl(wrl);
        let root = scene.roots[0];
        let a = scene.nodes[root as usize].children()[0];
        scene.set_color(a, Some([0.0, 0.5, 1.0, 0.5]));
        scene.rename(root, "Telaio principale");

        let path = std::env::temp_dir().join("gestore_wrl_export_colors_test.wrl");
        export_visible_wrl(&scene, &path).unwrap();
        let out = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("DEF Telaio_principale Group"));
        assert!(text.contains("diffuseColor 0 0.5 1 transparency 0.5"));
        assert!(!text.contains("diffuseColor 1 0 0"));

        let again = crate::parser::parse_wrl(&out);
        let colors: Vec<_> = again.shapes_data.iter().filter_map(|s| s.color).collect();
        assert!(colors.contains(&[0.0, 0.5, 1.0, 0.5]));
        let _ = std::fs::remove_file(path);
    }
}
