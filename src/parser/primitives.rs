//! Primitive (cubo, sfera, cilindro, cono) trasformate in superfici a triangoli e quadrilateri.
//!
//! Le misure e gli assi sono quelli di Open Inventor e del VRML: centro nell'origine, asse del
//! cilindro e del cono lungo Y, punta del cono in alto. Le facce sono in senso antiorario viste
//! da fuori. Le normali le calcola il lettore con `creaseAngle` ([`CREASE_ANGLE`]): lisce sui
//! fianchi curvi, spigoli vivi tra fianco e basi e tra le facce del cubo.

use std::io::Write;

use glam::Vec3;

/// Lati attorno all'asse di sfera, cilindro e cono.
const SEGMENTS: u32 = 32;
/// Paralleli della sfera (da polo a polo).
const RINGS: u32 = 16;
/// Angolo oltre il quale due facce vicine hanno uno spigolo vivo: più dell'angolo tra due lati
/// (360°/32 ≈ 11°), meno dei 90° tra fianco e base.
pub(crate) const CREASE_ANGLE: f32 = 0.5;

/// Superficie di una primitiva: vertici e facce (indici dei vertici).
pub(crate) struct Mesh {
    pub points: Vec<Vec3>,
    pub faces: Vec<Vec<u32>>,
    /// Superficie chiusa (si può nascondere il lato interno)
    pub solid: bool,
}

pub(crate) fn cube(width: f32, height: f32, depth: f32) -> Mesh {
    let (x, y, z) = (width / 2.0, height / 2.0, depth / 2.0);
    let points = [
        [-x, -y, -z],
        [x, -y, -z],
        [x, y, -z],
        [-x, y, -z],
        [-x, -y, z],
        [x, -y, z],
        [x, y, z],
        [-x, y, z],
    ]
    .map(Vec3::from)
    .to_vec();
    let faces = [[4, 5, 6, 7], [1, 0, 3, 2], [5, 1, 2, 6], [0, 4, 7, 3], [7, 6, 2, 3], [0, 1, 5, 4]];
    Mesh { points, faces: faces.iter().map(|f| f.to_vec()).collect(), solid: true }
}

pub(crate) fn sphere(radius: f32) -> Mesh {
    let mut points = vec![Vec3::new(0.0, radius, 0.0)];
    for ring in 1..RINGS {
        let polar = std::f32::consts::PI * ring as f32 / RINGS as f32;
        for k in 0..SEGMENTS {
            let a = std::f32::consts::TAU * k as f32 / SEGMENTS as f32;
            points.push(radius * Vec3::new(polar.sin() * a.sin(), polar.cos(), polar.sin() * a.cos()));
        }
    }
    points.push(Vec3::new(0.0, -radius, 0.0));
    let bottom = points.len() as u32 - 1;
    let at = |ring: u32, k: u32| 1 + (ring - 1) * SEGMENTS + k % SEGMENTS;
    let mut faces = Vec::new();
    for k in 0..SEGMENTS {
        faces.push(vec![0, at(1, k), at(1, k + 1)]);
        for ring in 1..RINGS - 1 {
            faces.push(vec![at(ring, k), at(ring + 1, k), at(ring + 1, k + 1), at(ring, k + 1)]);
        }
        faces.push(vec![at(RINGS - 1, k + 1), at(RINGS - 1, k), bottom]);
    }
    Mesh { points, faces, solid: true }
}

/// Cilindro (`top_radius` = `bottom_radius`) o cono (`top_radius` = 0, punta in alto).
pub(crate) fn cylinder(bottom_radius: f32, top_radius: f32, height: f32, side: bool, top: bool, bottom: bool) -> Mesh {
    let h = height / 2.0;
    let ring = |r: f32, y: f32| {
        (0..SEGMENTS).map(move |k| {
            let a = std::f32::consts::TAU * k as f32 / SEGMENTS as f32;
            Vec3::new(r * a.sin(), y, r * a.cos())
        })
    };
    let mut points: Vec<Vec3> = ring(bottom_radius, -h).collect();
    let cone = top_radius == 0.0;
    if cone {
        points.push(Vec3::new(0.0, h, 0.0));
    } else {
        points.extend(ring(top_radius, h));
    }
    let up = |k: u32| if cone { SEGMENTS } else { SEGMENTS + k % SEGMENTS };
    let down = |k: u32| k % SEGMENTS;
    let mut faces = Vec::new();
    if side {
        for k in 0..SEGMENTS {
            if cone {
                faces.push(vec![down(k), down(k + 1), up(k)]);
            } else {
                faces.push(vec![down(k), down(k + 1), up(k + 1), up(k)]);
            }
        }
    }
    if top && !cone {
        faces.push((0..SEGMENTS).map(up).collect());
    }
    if bottom {
        faces.push((0..SEGMENTS).rev().map(down).collect());
    }
    Mesh { points, faces, solid: side && bottom && (top || cone) }
}

impl Mesh {
    /// `IndexedFaceSet` VRML 2.0 con i punti trasformati da `matrix`.
    pub(crate) fn write_vrml2(&self, out: &mut Vec<u8>, matrix: glam::Mat4) {
        let mirrored = glam::Mat3::from_mat4(matrix).determinant() < 0.0;
        let _ = writeln!(
            out,
            "IndexedFaceSet {{ solid {} convex FALSE creaseAngle {CREASE_ANGLE}{}",
            if self.solid { "TRUE" } else { "FALSE" },
            if mirrored { " ccw FALSE" } else { "" }
        );
        out.extend_from_slice(b"coord Coordinate { point [\n");
        for p in &self.points {
            let p = matrix.transform_point3(*p);
            let _ = writeln!(out, "{} {} {},", p.x, p.y, p.z);
        }
        out.extend_from_slice(b"] }\ncoordIndex [\n");
        for f in &self.faces {
            for i in f {
                let _ = write!(out, "{i} ");
            }
            out.extend_from_slice(b"-1\n");
        }
        out.extend_from_slice(b"] }\n");
    }
}

/// Testo VRML 2.0 con le primitive usate come `geometry` (`Box`, `Sphere`, `Cylinder`, `Cone`)
/// sostituite dall'`IndexedFaceSet` equivalente. `None` se non ce ne sono.
pub(crate) fn expand_vrml2(text: &[u8]) -> Option<Vec<u8>> {
    let (braces, _) = super::delimiter_map::build_delimiter_maps(text);
    let mut out = Vec::new();
    let mut copied = 0;
    let mut pos = 0;
    let mut in_string = false;
    while pos < text.len() {
        let b = text[pos];
        if in_string {
            if b == b'\\' {
                pos += 1;
            } else if b == b'"' {
                in_string = false;
            }
            pos += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            pos += 1;
            continue;
        }
        if b == b'#' {
            while pos < text.len() && text[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        let word_start = pos == 0 || !(text[pos - 1].is_ascii_alphanumeric() || text[pos - 1] == b'_');
        if word_start && b.is_ascii_uppercase() {
            if let Some((word, after)) = super::lexer::scan_word(text, pos) {
                let open = super::lexer::skip_whitespace_comments(text, after);
                if let (true, Some(&close)) =
                    (matches!(word, "Box" | "Sphere" | "Cylinder" | "Cone"), braces.get(&open))
                {
                    out.extend_from_slice(&text[copied..pos]);
                    vrml2_mesh(word, &text[open + 1..close]).write_vrml2(&mut out, glam::Mat4::IDENTITY);
                    pos = close + 1;
                    copied = pos;
                    continue;
                }
                pos = after.max(pos + 1);
                continue;
            }
        }
        pos += 1;
    }
    if copied == 0 {
        return None;
    }
    out.extend_from_slice(&text[copied..]);
    Some(out)
}

/// Superficie di una primitiva VRML 2.0 dai suoi campi (testo tra le graffe).
fn vrml2_mesh(kind: &str, fields: &[u8]) -> Mesh {
    let field = Fields::read(fields);
    match kind {
        "Box" => {
            let [x, y, z] = field.numbers("size", [2.0, 2.0, 2.0]);
            cube(x, y, z)
        }
        "Sphere" => sphere(field.number("radius", 1.0)),
        "Cylinder" => cylinder(
            field.number("radius", 1.0),
            field.number("radius", 1.0),
            field.number("height", 2.0),
            field.flag("side"),
            field.flag("top"),
            field.flag("bottom"),
        ),
        _ => cylinder(field.number("bottomRadius", 1.0), 0.0, field.number("height", 2.0), field.flag("side"), false, field.flag("bottom")),
    }
}

/// Campi semplici di un nodo: nome seguito dai suoi numeri o da una parola.
struct Fields<'t> {
    words: Vec<&'t str>,
}

impl<'t> Fields<'t> {
    fn read(text: &'t [u8]) -> Self {
        let text = std::str::from_utf8(text).unwrap_or("");
        Fields { words: text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect() }
    }

    fn after(&self, name: &str) -> &[&'t str] {
        match self.words.iter().position(|&w| w == name) {
            Some(i) => &self.words[i + 1..],
            None => &[],
        }
    }

    fn number(&self, name: &str, default: f32) -> f32 {
        self.after(name).first().and_then(|w| w.parse().ok()).unwrap_or(default)
    }

    fn numbers(&self, name: &str, default: [f32; 3]) -> [f32; 3] {
        let v: Vec<f32> = self.after(name).iter().take(3).map_while(|w| w.parse().ok()).collect();
        <[f32; 3]>::try_from(v).unwrap_or(default)
    }

    /// Campo `TRUE`/`FALSE`, vero se manca (le parti di cilindro e cono ci sono tutte).
    fn flag(&self, name: &str) -> bool {
        self.after(name).first() != Some(&"FALSE")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tutte le facce viste da fuori in senso antiorario: la normale di ogni faccia punta via dal
    /// centro (per il cono si guarda il centro della base).
    fn outward(mesh: &Mesh) -> bool {
        mesh.faces.iter().all(|f| {
            let p = |i: usize| mesh.points[f[i] as usize];
            let center = f.iter().map(|&i| mesh.points[i as usize]).sum::<Vec3>() / f.len() as f32;
            let normal = (p(1) - p(0)).cross(p(2) - p(0));
            normal.dot(center) > 0.0
        })
    }

    #[test]
    fn primitives_face_outwards_and_have_the_right_size() {
        let c = cube(2.0, 4.0, 6.0);
        assert!(outward(&c));
        assert_eq!(c.points.iter().map(|p| p.y).fold(f32::MIN, f32::max), 2.0);
        let s = sphere(3.0);
        assert!(outward(&s) && s.points.iter().all(|p| (p.length() - 3.0).abs() < 1e-5));
        let cyl = cylinder(1.0, 1.0, 2.0, true, true, true);
        assert!(outward(&cyl) && cyl.solid);
        assert_eq!(cyl.faces.len() as u32, SEGMENTS + 2);
        let open = cylinder(1.0, 1.0, 2.0, true, false, true);
        assert!(!open.solid && open.faces.len() as u32 == SEGMENTS + 1);
        let cone = cylinder(1.0, 0.0, 2.0, true, false, true);
        assert!(outward(&cone) && cone.solid);
        assert_eq!(cone.points.last(), Some(&Vec3::new(0.0, 1.0, 0.0)), "punta in alto");
    }

    #[test]
    fn vrml2_primitives_become_face_sets() {
        let text = br#"Shape { geometry Box { size 1 2 3 } } Shape { geometry DEF S Sphere { radius 2 } }
            WorldInfo { info "Box { }" } Shape { geometry Cylinder { top FALSE } }"#;
        let out = String::from_utf8(expand_vrml2(text).unwrap()).unwrap();
        assert_eq!(out.matches("IndexedFaceSet").count(), 3, "{out}");
        assert!(out.contains("DEF S IndexedFaceSet") && out.contains("\"Box { }\""), "{out}");
        assert!(expand_vrml2(b"Shape { geometry IndexedFaceSet { } }").is_none());
    }
}
