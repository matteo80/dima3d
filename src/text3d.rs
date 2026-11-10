//! Scritte in rilievo: i contorni dei caratteri di un font TrueType/OpenType diventano superfici
//! (fronte, retro e fianchi), da mettere nella scena come un pezzo qualsiasi.
//!
//! Coordinate del testo: `u` lungo la riga (verso destra per chi legge), `v` in alto, `w` fuori
//! dalla parete (il rilievo va da `w = 0` a `w = depth`). La linea di base è a `v = 0` e il testo
//! comincia a `u = 0`; l'altezza è quella delle maiuscole (la «H» del font).

use ab_glyph::{Font, FontRef, OutlineCurve};
use lyon_tessellation::math::point;
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::path::{Path, PathEvent};
use lyon_tessellation::{BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers};

/// Superficie di una scritta.
pub struct TextMesh {
    pub points: Vec<glam::Vec3>,
    /// Facce: triangoli (fronte e retro) e quadrilateri (fianchi)
    pub faces: Vec<Vec<u32>>,
    /// Lunghezza della riga (mm)
    pub width: f32,
}

/// Font dal file: TTF, OTF o il primo di una raccolta TTC.
fn load(bytes: &[u8]) -> Result<FontRef<'_>, String> {
    FontRef::try_from_slice(bytes)
        .or_else(|_| FontRef::try_from_slice_and_index(bytes, 0))
        .map_err(|_| "il file non è un font TrueType/OpenType leggibile".to_string())
}

/// Scritta `text` con il font in `bytes`: maiuscole alte `height` mm, rilievo `depth` mm.
pub fn text_mesh(bytes: &[u8], text: &str, height: f32, depth: f32) -> Result<TextMesh, String> {
    let font = load(bytes)?;
    // Scala: la «H» alta `height` (se il font non ce l'ha, il 70% dell'em)
    let em = font.units_per_em().unwrap_or(1000.0);
    let cap = font
        .outline(font.glyph_id('H'))
        .map(|o| o.bounds.max.y - o.bounds.min.y)
        .filter(|h| *h > 0.0)
        .unwrap_or(em * 0.7);
    let scale = height / cap;
    // Tolleranza dell'appiattimento delle curve: un millesimo dell'em, in unità del font
    let tolerance = em / 1000.0;

    let mut builder = Path::builder();
    let mut pen = 0.0f32;
    let mut previous = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = previous {
            pen += font.kern_unscaled(p, id);
        }
        if let Some(outline) = font.outline(id) {
            let mut open = false;
            let mut last = None;
            let at = |p: ab_glyph::Point| point(pen + p.x, p.y);
            for curve in &outline.curves {
                let (start, end) = match curve {
                    OutlineCurve::Line(a, b) => (*a, *b),
                    OutlineCurve::Quad(a, _, b) => (*a, *b),
                    OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
                };
                // Un nuovo contorno comincia dove il precedente non finisce
                if last != Some(start) {
                    if open {
                        builder.end(true);
                    }
                    builder.begin(at(start));
                    open = true;
                }
                match curve {
                    OutlineCurve::Line(_, b) => {
                        builder.line_to(at(*b));
                    }
                    OutlineCurve::Quad(_, c, b) => {
                        builder.quadratic_bezier_to(at(*c), at(*b));
                    }
                    OutlineCurve::Cubic(_, c1, c2, b) => {
                        builder.cubic_bezier_to(at(*c1), at(*c2), at(*b));
                    }
                }
                last = Some(end);
            }
            if open {
                builder.end(true);
            }
        }
        pen += font.h_advance_unscaled(id);
        previous = Some(id);
    }
    let path = builder.build();

    // Fronte e retro: il riempimento del contorno (regola non-zero, come TrueType)
    let mut fill: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::tolerance(tolerance).with_fill_rule(FillRule::NonZero),
            &mut BuffersBuilder::new(&mut fill, |v: FillVertex| v.position().to_array()),
        )
        .map_err(|e| format!("lettere non riempibili ({e:?})"))?;

    let mut points = Vec::new();
    let mut faces = Vec::new();
    let n = fill.vertices.len() as u32;
    for &[x, y] in &fill.vertices {
        points.push(glam::Vec3::new(x * scale, y * scale, depth));
    }
    for &[x, y] in &fill.vertices {
        points.push(glam::Vec3::new(x * scale, y * scale, 0.0));
    }
    for t in fill.indices.chunks_exact(3) {
        faces.push(vec![t[0], t[1], t[2]]);
        faces.push(vec![n + t[0], n + t[2], n + t[1]]);
    }
    // Fianchi: un quadrilatero per ogni tratto del contorno appiattito
    let mut side = |a: lyon_tessellation::math::Point, b: lyon_tessellation::math::Point| {
        if a == b {
            return;
        }
        let base = points.len() as u32;
        for (p, w) in [(a, 0.0), (b, 0.0), (b, depth), (a, depth)] {
            points.push(glam::Vec3::new(p.x * scale, p.y * scale, w));
        }
        faces.push(vec![base, base + 1, base + 2, base + 3]);
    };
    for event in path.iter().flattened(tolerance) {
        match event {
            PathEvent::Line { from, to } => side(from, to),
            PathEvent::End { last, first, close: true } => side(last, first),
            _ => {}
        }
    }
    Ok(TextMesh { points, faces, width: pen * scale })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arial() -> Option<Vec<u8>> {
        std::fs::read(r"C:\Windows\Fonts\arial.ttf").ok()
    }

    /// Altezza delle maiuscole e rilievo come richiesto; la «O» ha il buco (fronte senza
    /// triangoli al centro); la riga è più lunga con più lettere.
    #[test]
    fn letters_have_the_requested_size_and_holes() {
        let Some(font) = arial() else { return };
        let mesh = text_mesh(&font, "HO", 500.0, 20.0).unwrap();
        let (lo, hi) = mesh.points.iter().fold((glam::Vec3::MAX, glam::Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        // Le lettere tonde sporgono di poco sopra e sotto le maiuscole (come in ogni font)
        assert!(hi.y > 495.0 && hi.y < 530.0 && lo.y > -15.0, "maiuscole alte 500: {lo} {hi}");
        assert_eq!((lo.z, hi.z), (0.0, 20.0));
        assert!(mesh.width > 600.0 && mesh.width < 1200.0, "{}", mesh.width);
        // Centro della «O»: nessun triangolo del fronte lo copre
        let o_center = glam::Vec2::new((mesh.width * 0.72).min(hi.x), 250.0);
        let covered = mesh.faces.iter().filter(|f| f.len() == 3).any(|f| {
            let p: Vec<glam::Vec2> = f.iter().map(|&i| mesh.points[i as usize].truncate()).collect();
            let sign = |a: glam::Vec2, b: glam::Vec2, c: glam::Vec2| (b - a).perp_dot(c - a);
            let (d1, d2, d3) = (sign(p[0], p[1], o_center), sign(p[1], p[2], o_center), sign(p[2], p[0], o_center));
            !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
        });
        assert!(!covered, "la O è vuota al centro");
        assert!(text_mesh(&font, "HO HO", 500.0, 20.0).unwrap().width > mesh.width * 1.5);
        assert!(text_mesh(b"non un font", "H", 500.0, 20.0).is_err());
    }
}
