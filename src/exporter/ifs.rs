//! Riscrittura di un `IndexedFaceSet` a partire dal testo originale del file.
//!
//! I campi vengono copiati così come sono (poligoni, `normalIndex`, `solid`, `creaseAngle`,
//! colori, coordinate di texture...), senza passare dai dati triangolati usati per il rendering.
//! Le sole modifiche sono quelle richieste dall'esportazione in world-space:
//! * `coord` e `normal` vengono trasformati dalla matrice della Shape (con matrice identità
//!   il testo è copiato, senza perdere le cifre oltre la precisione di `f32`);
//! * `ccw` viene invertito se la matrice è speculare;
//! * `DEF` viene rimosso e `USE` sostituito dal nodo a cui si riferisce, perché il file
//!   esportato non contiene necessariamente il `DEF` originale.

use std::collections::HashMap;
use std::io::{self, Write};

use super::writer::{matching_brace, matching_bracket, skip_comment, skip_string};
use crate::parser::{lexer, ShapeInfo};

/// Valore di un campo VRML (posizioni assolute nel file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value<'a> {
    /// `Tipo { ... }`, senza l'eventuale `DEF Nome`: da `start` alla `}` inclusa
    Node { start: usize, open: usize, close: usize },
    /// `USE Nome`
    Use(&'a [u8]),
    /// Vettore `[ ... ]` o valore semplice (`TRUE`, `0.5`, `NULL`): `src[start..end]`
    Raw { start: usize, end: usize },
}

#[derive(Debug, Clone, Copy)]
struct Field<'a> {
    name: &'a [u8],
    /// Posizione del nome: serve a risolvere `USE` sul `DEF` precedente
    pos: usize,
    value: Value<'a>,
}

fn skip_separators(src: &[u8], mut i: usize) -> usize {
    while i < src.len() {
        match src[i] {
            b'#' => i = skip_comment(src, i),
            b',' => i += 1,
            c if c.is_ascii_whitespace() => i += 1,
            _ => break,
        }
    }
    i
}

/// Parola a partire da `i` (vuota se `i` è su un delimitatore).
fn read_token(src: &[u8], i: usize) -> (&[u8], usize) {
    let mut end = i;
    while end < src.len()
        && !src[end].is_ascii_whitespace()
        && !matches!(src[end], b'{' | b'}' | b'[' | b']' | b',' | b'"' | b'#')
    {
        end += 1;
    }
    (&src[i..end], end)
}

/// Legge il valore di un campo e la posizione successiva.
fn read_value(src: &[u8], i: usize, end: usize) -> Option<(Value<'_>, usize)> {
    let i = skip_separators(src, i);
    if i >= end {
        return None;
    }
    match src[i] {
        b'[' => {
            let close = matching_bracket(src, i).filter(|&c| c < end)?;
            return Some((Value::Raw { start: i, end: close + 1 }, close + 1));
        }
        b'"' => {
            let after = skip_string(src, i);
            return Some((Value::Raw { start: i, end: after }, after));
        }
        _ => {}
    }
    let (token, after) = read_token(src, i);
    match token {
        b"" => None,
        b"USE" => {
            let name_pos = skip_separators(src, after);
            let (name, after_name) = read_token(src, name_pos);
            (!name.is_empty()).then_some((Value::Use(name), after_name))
        }
        b"DEF" => {
            let name_pos = skip_separators(src, after);
            let (_, after_name) = read_token(src, name_pos);
            read_value(src, after_name, end)
        }
        _ => {
            let open = skip_separators(src, after);
            if open < end && src[open] == b'{' {
                let close = matching_brace(src, open).filter(|&c| c < end)?;
                Some((Value::Node { start: i, open, close }, close + 1))
            } else {
                Some((Value::Raw { start: i, end: after }, after))
            }
        }
    }
}

/// Campi di primo livello del nodo con graffe in `open`/`close`.
fn read_fields(src: &[u8], open: usize, close: usize) -> Vec<Field<'_>> {
    let mut fields = Vec::new();
    let mut i = open + 1;
    loop {
        i = skip_separators(src, i);
        if i >= close {
            return fields;
        }
        let pos = i;
        let (name, after) = read_token(src, i);
        if name.is_empty() {
            return fields;
        }
        let Some((value, next)) = read_value(src, after, close) else {
            return fields;
        };
        fields.push(Field { name, pos, value });
        i = next;
    }
}

/// Indice dei nodi di geometria con `DEF` del file, per risolvere i `USE`.
/// Viene costruito solo al primo `USE` incontrato: la maggior parte dei file non ne ha.
#[derive(Default)]
pub(super) struct GeometryDefs<'a> {
    /// nome → (inizio del nodo, chiusura), in ordine di file
    map: Option<HashMap<&'a [u8], Vec<(usize, usize)>>>,
}

impl<'a> GeometryDefs<'a> {
    /// Il `DEF` valido per un `USE` in posizione `before` è l'ultimo che lo precede.
    fn resolve(&mut self, src: &'a [u8], name: &[u8], before: usize) -> Option<(usize, usize)> {
        let map = self.map.get_or_insert_with(|| scan_defs(src));
        let defs = map.get(name)?;
        let n = defs.partition_point(|d| d.0 < before);
        n.checked_sub(1).map(|i| defs[i])
    }
}

fn scan_defs(src: &[u8]) -> HashMap<&[u8], Vec<(usize, usize)>> {
    let is_id = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut map: HashMap<&[u8], Vec<(usize, usize)>> = HashMap::new();
    let mut i = 0;
    while i < src.len() {
        match src[i] {
            b'#' => i = skip_comment(src, i),
            b'"' => i = skip_string(src, i),
            b'D' if src[i..].starts_with(b"DEF")
                && (i == 0 || !is_id(src[i - 1]))
                && src.get(i + 3).is_some_and(|c| c.is_ascii_whitespace()) =>
            {
                let name_pos = skip_separators(src, i + 3);
                let (name, after_name) = read_token(src, name_pos);
                let start = skip_separators(src, after_name);
                let (ty, after_ty) = read_token(src, start);
                i = after_ty.max(i + 3);
                if matches!(ty, b"Coordinate" | b"Normal" | b"Color" | b"TextureCoordinate") {
                    let open = skip_separators(src, after_ty);
                    if let Some(close) = matching_brace(src, open) {
                        map.entry(name).or_default().push((start, close));
                        i = close + 1;
                    }
                } else if ty == b"IndexedFaceSet" {
                    // Non si salta il contenuto: può definire a sua volta `coord DEF`...
                    let open = skip_separators(src, after_ty);
                    if let Some(close) = matching_brace(src, open) {
                        map.entry(name).or_default().push((start, close));
                    }
                }
            }
            _ => i += 1,
        }
    }
    map
}

/// Testo `Tipo { ... }` del nodo di un campo, risolvendo `USE`. `None` se assente o non risolto.
fn node_range<'a>(
    src: &'a [u8],
    field: &Field<'a>,
    defs: &mut GeometryDefs<'a>,
) -> Option<(usize, usize)> {
    match field.value {
        Value::Node { start, close, .. } => Some((start, close)),
        Value::Use(name) => defs.resolve(src, name, field.pos),
        Value::Raw { .. } => None,
    }
}

/// Vero se `coordIndex` contiene almeno un poligono valido per `vertices` vertici.
fn has_polygon(src: &[u8], start: usize, end: usize, vertices: usize) -> bool {
    let mut run = 0;
    for idx in lexer::scan_numbers_i32(src, start, end) {
        if idx == -1 {
            run = 0;
        } else if idx >= 0 && (idx as usize) < vertices {
            run += 1;
            if run >= 3 {
                return true;
            }
        }
    }
    false
}

/// Scrive i campi dell'`IndexedFaceSet` della Shape con graffe in `open`/`close`.
/// `drop_colors`: omette i colori per vertice/faccia (coprirebbero il colore scelto dall'utente).
///
/// Ritorna `false`, senza aver scritto nulla, se la geometria originale non è riutilizzabile
/// (es. `coordIndex` assente): il chiamante scrive allora i triangoli calcolati dal parser.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_fields<'a>(
    w: &mut impl Write,
    ind: &str,
    src: &'a [u8],
    open: usize,
    close: usize,
    shape: &ShapeInfo,
    drop_colors: bool,
    defs: &mut GeometryDefs<'a>,
) -> io::Result<bool> {
    let Some(geometry) = read_fields(src, open, close).into_iter().find(|f| f.name == b"geometry")
    else {
        return Ok(false);
    };
    // `geometry USE Nome`: i campi sono quelli del nodo a cui si riferisce
    let Some((start, ifs_close)) = node_range(src, &geometry, defs) else {
        return Ok(false);
    };
    let (ty, after_type) = read_token(src, start);
    if ty != b"IndexedFaceSet" {
        return Ok(false);
    }
    let ifs_open = skip_separators(src, after_type);
    let fields = read_fields(src, ifs_open, ifs_close);
    let find = |name: &[u8]| fields.iter().find(|f| f.name == name);

    match find(b"coordIndex").map(|f| f.value) {
        Some(Value::Raw { start, end })
            if has_polygon(src, start, end, shape.world_vertices.len()) => {}
        _ => return Ok(false),
    }

    let identity = shape.matrix == glam::Mat4::IDENTITY;
    let linear = glam::Mat3::from_mat4(shape.matrix);
    let mirrored = linear.determinant() < 0.0;
    let normal_matrix = linear.inverse().transpose();

    // Nodi da riscrivere e campi che dipendono da loro
    let normal = find(b"normal").and_then(|f| node_range(src, f, defs));
    let keep_normal = normal.is_some() && (identity || normal_matrix.is_finite());
    let color = find(b"color")
        .filter(|_| !drop_colors)
        .and_then(|f| node_range(src, f, defs));
    let tex_coord = find(b"texCoord").and_then(|f| node_range(src, f, defs));
    let coord = find(b"coord").and_then(|f| node_range(src, f, defs));

    let pad = format!("{ind}    ");
    for field in &fields {
        match field.name {
            b"coord" => match coord.filter(|_| identity) {
                Some((s, e)) => write_line(w, &pad, b"coord", &src[s..=e])?,
                None => write_points(w, &pad, shape)?,
            },
            b"normal" => {
                if !keep_normal {
                    continue;
                }
                let (s, e) = normal.unwrap_or_default();
                if identity {
                    write_line(w, &pad, b"normal", &src[s..=e])?;
                } else {
                    write_normals(w, &pad, src, s, e, normal_matrix)?;
                }
            }
            b"color" => {
                if let Some((s, e)) = color {
                    write_line(w, &pad, b"color", &src[s..=e])?;
                }
            }
            b"texCoord" => {
                if let Some((s, e)) = tex_coord {
                    write_line(w, &pad, b"texCoord", &src[s..=e])?;
                }
            }
            b"ccw" => {} // scritto in fondo, tenendo conto delle matrici speculari
            b"normalIndex" | b"normalPerVertex" if !keep_normal => {}
            b"colorIndex" | b"colorPerVertex" if color.is_none() => {}
            b"texCoordIndex" if tex_coord.is_none() => {}
            name => {
                if let Value::Raw { start, end } = field.value {
                    write_line(w, &pad, name, &src[start..end])?;
                }
            }
        }
    }

    let ccw = !matches!(
        find(b"ccw").map(|f| f.value),
        Some(Value::Raw { start, end }) if &src[start..end] == b"FALSE"
    );
    if ccw == mirrored {
        writeln!(w, "{pad}ccw FALSE")?;
    }
    Ok(true)
}

fn write_line(w: &mut impl Write, pad: &str, name: &[u8], value: &[u8]) -> io::Result<()> {
    w.write_all(pad.as_bytes())?;
    w.write_all(name)?;
    w.write_all(b" ")?;
    w.write_all(value)?;
    w.write_all(b"\n")
}

/// `coord` con i vertici in world-space calcolati dal parser.
pub(super) fn write_points(w: &mut impl Write, pad: &str, shape: &ShapeInfo) -> io::Result<()> {
    write!(w, "{pad}coord Coordinate {{ point [")?;
    for (i, [x, y, z]) in shape.world_vertices.iter().enumerate() {
        if i % 4 == 0 {
            write!(w, "\n{pad}  ")?;
        }
        write!(w, "{x} {y} {z}, ")?;
    }
    writeln!(w, "\n{pad}] }}")
}

/// `normal` con i vettori del nodo `Normal` (`src[start..=close]`) portati in world-space.
fn write_normals(
    w: &mut impl Write,
    pad: &str,
    src: &[u8],
    start: usize,
    close: usize,
    matrix: glam::Mat3,
) -> io::Result<()> {
    let (_, after_type) = read_token(src, start);
    let open = skip_separators(src, after_type);
    let vectors = read_fields(src, open, close)
        .into_iter()
        .find(|f| f.name == b"vector")
        .and_then(|f| match f.value {
            Value::Raw { start, end } => Some(lexer::scan_numbers_f32(src, start, end)),
            _ => None,
        })
        .unwrap_or_default();

    write!(w, "{pad}normal Normal {{ vector [")?;
    for (i, n) in vectors.chunks_exact(3).enumerate() {
        if i % 4 == 0 {
            write!(w, "\n{pad}  ")?;
        }
        let v = (matrix * glam::Vec3::new(n[0], n[1], n[2])).normalize_or_zero();
        write!(w, "{} {} {}, ", v.x, v.y, v.z)?;
    }
    writeln!(w, "\n{pad}] }}")
}

#[cfg(test)]
mod tests {
    use crate::parser::ShapeInfo;
    use crate::scene_graph::SceneGraph;

    fn export(name: &str, scene: &SceneGraph) -> String {
        let path = std::env::temp_dir().join(format!("gestore_wrl_ifs_{name}.wrl"));
        crate::exporter::export_visible_wrl(scene, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);
        text
    }

    fn export_wrl(name: &str, wrl: &str) -> String {
        export(name, &crate::parser::parse_wrl(wrl.as_bytes()))
    }

    #[test]
    fn polygons_and_fields_are_kept() {
        let text = export_wrl(
            "campi",
            r#"#VRML V2.0 utf8
Group { children [ Shape { geometry IndexedFaceSet {
    solid FALSE
    creaseAngle 1.2
    convex FALSE
    coord DEF Punti Coordinate { point [ 0.0249995994567871 0 0, 1 0 0, 1 1 0, 0 1 0, 2 0 0 ] }
    coordIndex [ 0 1 2 3 -1, 1 4 2 -1 ]
    normal DEF Versi Normal { vector [ 0 0 1, 0 0 -1 ] }
    normalPerVertex FALSE
    normalIndex [ 0 1 ]
} } ] }"#,
        );
        assert!(text.contains("coordIndex [ 0 1 2 3 -1, 1 4 2 -1 ]"), "{text}");
        assert!(text.contains("solid FALSE"));
        assert!(text.contains("creaseAngle 1.2"));
        assert!(text.contains("convex FALSE"));
        assert!(text.contains("normalPerVertex FALSE"));
        assert!(text.contains("normalIndex [ 0 1 ]"));
        assert!(text.contains("vector [ 0 0 1, 0 0 -1 ]"));
        // Senza trasformazioni le coordinate sono copiate con tutte le cifre del file
        assert!(text.contains("0.0249995994567871"));
        assert!(!text.contains("DEF Punti") && !text.contains("DEF Versi"));

        let again = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data[0].face_count, 2);
        assert_eq!(again.shapes_data[0].indices.len(), 9);
    }

    #[test]
    fn absent_fields_are_not_invented() {
        let text = export_wrl(
            "assenti",
            "#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { \
             coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }",
        );
        assert!(!text.contains("solid"), "{text}");
        assert!(!text.contains("creaseAngle"));
        assert!(!text.contains("ccw"));
    }

    #[test]
    fn transform_moves_points_and_rotates_normals() {
        let text = export_wrl(
            "normali",
            r#"#VRML V2.0 utf8
Transform { translation 5 0 0 rotation 0 0 1 1.5707963 children [ Shape { geometry IndexedFaceSet {
    coord Coordinate { point [ 1 0 0, 2 0 0, 2 1 0, 1 1 0 ] }
    coordIndex [ 0 1 2 3 -1 ]
    normal Normal { vector [ 1 0 0, 1 0 0, 1 0 0, 1 0 0 ] }
} } ] }"#,
        );
        assert!(text.contains("coordIndex [ 0 1 2 3 -1 ]"));
        let again = crate::parser::parse_wrl(text.as_bytes());
        let s = &again.shapes_data[0];
        assert!((s.world_vertices[0][0] - 5.0).abs() < 1e-5, "{:?}", s.world_vertices[0]);
        assert!((s.world_vertices[0][1] - 1.0).abs() < 1e-5);
        let n = s.normals[0];
        assert!(n[0].abs() < 1e-5 && (n[1] - 1.0).abs() < 1e-5, "{n:?}");
    }

    #[test]
    fn mirror_transform_flips_ccw() {
        let shape = |ccw: &str| {
            format!(
                "#VRML V2.0 utf8\nTransform {{ scale -1 1 1 children [ Shape {{ geometry IndexedFaceSet {{ {ccw} \
                 coord Coordinate {{ point [ 0 0 0, 1 0 0, 0 1 0 ] }} coordIndex [ 0 1 2 -1 ] }} }} ] }}"
            )
        };
        assert!(export_wrl("specchio1", &shape("")).contains("ccw FALSE"));
        assert!(!export_wrl("specchio2", &shape("ccw FALSE")).contains("ccw"));
    }

    #[test]
    fn use_is_replaced_by_the_preceding_def() {
        let text = export_wrl(
            "riusi",
            r#"#VRML V2.0 utf8
Shape { geometry IndexedFaceSet {
    coord DEF P Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] }
    coordIndex [ 0 1 2 -1 ]
    normal DEF N Normal { vector [ 0 0 1, 0 0 1, 0 0 1 ] }
    color DEF C Color { color [ 1 0 0 ] }
    colorPerVertex FALSE
} }
Shape { geometry IndexedFaceSet {
    coord USE P
    coordIndex [ 2 1 0 -1 ]
    normal USE N
    color USE C
    colorPerVertex FALSE
} }
Shape { geometry IndexedFaceSet {
    coord DEF P Coordinate { point [ 5 5 5, 6 5 5, 5 6 5 ] }
    coordIndex [ 0 1 2 -1 ]
} }
Shape { geometry IndexedFaceSet {
    coord USE P
    coordIndex [ 0 2 1 -1 ]
    color USE Inesistente
    colorPerVertex FALSE
} }"#,
        );
        assert!(!text.contains("USE") && !text.contains("DEF P"), "{text}");
        assert_eq!(text.matches("colorPerVertex FALSE").count(), 2);
        assert_eq!(text.matches("color Color { color [ 1 0 0 ] }").count(), 2);
        assert_eq!(text.matches("point [ 5 5 5").count(), 2);

        let again = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data.len(), 4);
        assert_eq!(again.shapes_data[1].indices, vec![2, 1, 0]);
        assert_eq!(again.shapes_data[3].world_vertices[0], [5.0, 5.0, 5.0]);
    }

    /// La geometria riusata conserva i poligoni, e il file esportato non dipende dal `DEF`.
    #[test]
    fn geometry_use_is_replaced_by_its_definition() {
        let text = export_wrl(
            "geometria",
            r#"#VRML V2.0 utf8
Shape { geometry DEF G IndexedFaceSet {
    coord DEF P Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] }
    coordIndex [ 0 1 2 3 -1 ]
} }
Transform { translation 5 0 0 children [ Shape { geometry USE G } ] }"#,
        );
        assert!(!text.contains("USE") && !text.contains("DEF G ") && !text.contains("DEF P "), "{text}");
        assert_eq!(text.matches("coordIndex [ 0 1 2 3 -1 ]").count(), 2, "{text}");

        let again = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data.len(), 2);
        assert_eq!(again.shapes_data[0].face_count, 1);
        let xs: Vec<f32> = again.shapes_data.iter().map(|s| s.world_vertices[0][0]).collect();
        assert!(xs.contains(&0.0) && xs.contains(&5.0), "{xs:?}");
    }

    /// I pezzi dentro un LOD si esportano come tutti gli altri, una volta sola.
    #[test]
    fn lod_content_is_exported() {
        let text = export_wrl(
            "lod",
            r#"#VRML V2.0 utf8
Transform { translation 5 0 0 children LOD { range 10 level [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 9 0 0, 9 1 0, 9 0 1 ] } coordIndex [ 0 1 2 -1 ] } }
] } }"#,
        );
        let again = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data.len(), 1);
        assert_eq!(again.shapes_data[0].world_vertices[0], [5.0, 0.0, 0.0]);
    }

    #[test]
    fn user_color_replaces_vertex_colors() {
        let wrl = r#"#VRML V2.0 utf8
DEF A Group { children [ Shape { geometry IndexedFaceSet {
    coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] }
    coordIndex [ 0 1 2 -1 ]
    color Color { color [ 1 0 0 ] }
    colorPerVertex FALSE
    colorIndex [ 0 ]
} } ] }"#;
        let mut scene = crate::parser::parse_wrl(wrl.as_bytes());
        assert!(export("colori1", &scene).contains("colorIndex [ 0 ]"));

        scene.set_color(scene.roots[0], Some([0.0, 1.0, 0.0, 1.0]));
        let text = export("colori2", &scene);
        assert!(!text.contains("colorIndex") && !text.contains("color Color"), "{text}");
        assert!(!text.contains("colorPerVertex"));
    }

    #[test]
    fn missing_coord_index_falls_back_to_triangles() {
        let text = export_wrl(
            "ripiego",
            "#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { \
             coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0, 5 0 0, 6 0 0, 5 1 0 ] } } }",
        );
        let again = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data[0].indices, vec![0, 1, 2, 3, 4, 5]);
    }

    /// Prova su un file reale: `GESTORE_WRL_FILE=percorso cargo test file_reale -- --ignored`
    #[test]
    #[ignore]
    fn file_reale_round_trip() {
        let path = std::env::var("GESTORE_WRL_FILE").expect("impostare GESTORE_WRL_FILE");
        let data = std::fs::read(&path).unwrap();
        let t = std::time::Instant::now();
        let scene = crate::parser::parse_wrl(&data);
        println!("Parsing: {:?}", t.elapsed());
        let out = std::env::temp_dir().join("gestore_wrl_file_reale.wrl");
        let t = std::time::Instant::now();
        crate::exporter::export_visible_wrl(&scene, &out).unwrap();
        let exported = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        let again = crate::parser::parse_wrl(&exported);
        println!(
            "{} → {} byte ({} → {} shape, {:?})",
            data.len(),
            exported.len(),
            scene.shapes_data.len(),
            again.shapes_data.len(),
            t.elapsed()
        );

        let identity =
            scene.shapes_data.iter().filter(|s| s.matrix == glam::Mat4::IDENTITY).count();
        println!("Shape con matrice identità: {identity}");
        let split: Vec<_> = scene.shapes_data.iter().filter_map(|s| s.split.as_ref()).collect();
        let vertices: usize = scene.shapes_data.iter().map(|s| s.world_vertices.len()).sum();
        let copies: usize = split.iter().map(|s| s.extra.len()).sum();
        println!(
            "Spigoli vivi da normalIndex: {} shape su {}, {copies} copie di vertici su {vertices}",
            split.len(),
            scene.shapes_data.len()
        );

        // L'albero raggruppa le Shape in componenti: l'ordine nel file esportato cambia,
        // quindi si confrontano gli insiemi. Le coordinate devono tornare identiche al bit.
        let keys = |shapes: &[ShapeInfo]| {
            let mut k: Vec<_> = shapes
                .iter()
                .map(|s| {
                    let bits: Vec<[u32; 3]> =
                        s.world_vertices.iter().map(|v| v.map(f32::to_bits)).collect();
                    let color = s.color.map(|c| c.map(|v| (v * 1e5).round() as i64));
                    (bits, s.indices.clone(), s.face_count, color)
                })
                .collect();
            k.sort();
            k
        };
        let (a, b) = (keys(&scene.shapes_data), keys(&again.shapes_data));
        assert_eq!(a.len(), b.len());
        let different = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert_eq!(different, 0, "{different} shape diverse dopo l'export");

        // Le normali definiscono la resa: si confrontano a meno di un piccolo angolo,
        // abbinando le Shape tramite i vertici (le istanze identiche sono interscambiabili)
        let normals = |shapes: &[ShapeInfo]| {
            let mut k: Vec<(Vec<[u32; 3]>, Vec<u32>, Vec<[f32; 3]>)> = shapes
                .iter()
                .map(|s| {
                    let bits = s.world_vertices.iter().map(|v| v.map(f32::to_bits)).collect();
                    (bits, s.indices.clone(), s.corner_normals())
                })
                .collect();
            k.sort_by(|x, y| (&x.0, &x.1).cmp(&(&y.0, &y.1)));
            k
        };
        let (na, nb) = (normals(&scene.shapes_data), normals(&again.shapes_data));
        // Una normale calcolata dal parser nasce dalle coordinate del file; riaprendo l'export
        // nasce da quelle in world-space, arrotondate a f32: sui triangoli piccolissimi cambia.
        // Se coincide col ricalcolo dai vertici world originali, l'export non ha alterato nulla.
        let (mut compared, mut rounding, mut unexplained) = (0usize, 0usize, 0usize);
        let mut histogram = [0usize; 4]; // fino a 5°, 20°, 90°, oltre
        let mut shapes_off = 0usize;
        let same = |p: [f32; 3], q: [f32; 3]| glam::Vec3::from(p).dot(glam::Vec3::from(q)) >= 0.999;
        for (x, y) in na.iter().zip(&nb) {
            let mut from_world: Option<Vec<[f32; 3]>> = None;
            let before = rounding + unexplained;
            for (i, (&p, &q)) in x.2.iter().zip(&y.2).enumerate() {
                compared += 1;
                if same(p, q) {
                    continue;
                }
                let deg = glam::Vec3::from(p).dot(glam::Vec3::from(q)).clamp(-1.0, 1.0).acos().to_degrees();
                histogram[[5.0, 20.0, 90.0].iter().filter(|&&l| deg > l).count()] += 1;
                let world = from_world.get_or_insert_with(|| {
                    let vertices: Vec<[f32; 3]> = x.0.iter().map(|v| v.map(f32::from_bits)).collect();
                    crate::renderer::mesh_buffer::compute_normals(&vertices, &x.1)
                });
                if same(world[x.1[i] as usize], q) {
                    rounding += 1;
                } else {
                    unexplained += 1;
                }
            }
            if rounding + unexplained > before {
                shapes_off += 1;
            }
        }
        println!(
            "Normali: {compared} confrontate, {rounding} diverse per arrotondamento, {unexplained} non spiegate, in {shapes_off} shape"
        );
        println!("Angolo: <5° {} · <20° {} · <90° {} · oltre {}", histogram[0], histogram[1], histogram[2], histogram[3]);
        assert_eq!(unexplained, 0, "normali alterate dall'export");
    }
}
