//! Estrazione del colore dei materiali VRML (`appearance Appearance { material Material { ... } }`).
//!
//! Il colore è restituito come RGBA in spazio sRGB (come scritto nel file):
//! `rgb = diffuseColor` (oppure `emissiveColor` se il diffuse è nero),
//! `a = 1 - transparency`.

use std::collections::HashMap;

use crate::parser::lexer;

/// Colore RGBA (sRGB, alpha lineare).
pub type Rgba = [f32; 4];

/// Colore VRML di default di un Material senza `diffuseColor`.
const VRML_DEFAULT_DIFFUSE: [f32; 3] = [0.8, 0.8, 0.8];

/// Registro dei nomi DEF di `Material` e `Appearance` → (posizione, colore) di ogni definizione,
/// in ordine di file: lo stesso nome può essere ridefinito.
pub type MaterialRegistry = HashMap<String, Vec<(usize, Rgba)>>;

/// Colore del `DEF` valido per un `USE` in posizione `before`.
fn resolve(registry: &MaterialRegistry, name: &str, before: usize) -> Option<Rgba> {
    let defs = registry.get(name)?;
    crate::parser::latest_before(defs, before, |d| d.0).map(|d| d.1)
}

/// Legge i campi di un blocco `Material { ... }` (`open` = posizione della `{`).
pub fn parse_material_block(data: &[u8], open: usize, close: usize) -> Rgba {
    let mut diffuse: Option<[f32; 3]> = None;
    let mut emissive: Option<[f32; 3]> = None;
    let mut transparency = 0.0f32;

    let mut pos = open + 1;
    while pos < close {
        let Some((word, next)) = lexer::scan_word(data, pos) else { break };
        if next > close {
            break;
        }
        match word {
            "diffuseColor" | "emissiveColor" => {
                let (vals, after) = read_numbers(data, next, close, 3);
                if vals.len() == 3 {
                    let c = [vals[0], vals[1], vals[2]];
                    if word == "diffuseColor" {
                        diffuse = Some(c);
                    } else {
                        emissive = Some(c);
                    }
                }
                pos = after;
            }
            "transparency" => {
                let (vals, after) = read_numbers(data, next, close, 1);
                if let Some(&t) = vals.first() {
                    transparency = t;
                }
                pos = after;
            }
            _ => pos = next,
        }
    }

    let mut rgb = diffuse.unwrap_or(VRML_DEFAULT_DIFFUSE);
    // Alcuni esportatori usano diffuse nero + emissive per il colore "piatto"
    if rgb.iter().all(|&v| v <= 1e-4) {
        if let Some(e) = emissive.filter(|e| e.iter().any(|&v| v > 1e-4)) {
            rgb = e;
        }
    }
    let clamp = |v: f32| v.clamp(0.0, 1.0);
    [clamp(rgb[0]), clamp(rgb[1]), clamp(rgb[2]), clamp(1.0 - transparency)]
}

/// Legge fino a `n` numeri a partire da `pos` (fermandosi al primo token non numerico).
fn read_numbers(data: &[u8], mut pos: usize, end: usize, n: usize) -> (Vec<f32>, usize) {
    let mut out = Vec::with_capacity(n);
    while out.len() < n && pos < end {
        let Some((word, next)) = lexer::scan_word(data, pos) else { break };
        match word.parse::<f32>() {
            Ok(v) => {
                out.push(v);
                pos = next;
            }
            Err(_) => break,
        }
    }
    (out, pos)
}

/// Interpreta il valore di un campo nodo SFNode a partire da `pos`
/// (`DEF Nome Tipo { ... }`, `Tipo { ... }`, `USE Nome`, `NULL`).
/// Il nome di un eventuale `DEF` viene ignorato (i DEF sono già nel registro).
enum NodeValue<'a> {
    Inline { ty: &'a str, open: usize, close: usize },
    Use(&'a str),
    None,
}

fn read_node_value<'a>(
    data: &'a [u8],
    pos: usize,
    end: usize,
    brace_map: &HashMap<usize, usize>,
) -> (NodeValue<'a>, usize) {
    let Some((w, next)) = lexer::scan_word(data, pos) else { return (NodeValue::None, end) };
    match w {
        "USE" => match lexer::scan_word(data, next) {
            Some((name, after)) => (NodeValue::Use(name), after),
            None => (NodeValue::None, end),
        },
        "DEF" => {
            let Some((_name, after_name)) = lexer::scan_word(data, next) else {
                return (NodeValue::None, end);
            };
            read_node_value(data, after_name, end, brace_map)
        }
        "NULL" => (NodeValue::None, next),
        ty => {
            let b = lexer::skip_whitespace_comments(data, next);
            if b < end && data[b] == b'{' {
                if let Some(&close) = brace_map.get(&b) {
                    return (NodeValue::Inline { ty, open: b, close }, close + 1);
                }
            }
            (NodeValue::None, next)
        }
    }
}

/// Colore di un blocco `Appearance { ... }`: cerca il campo `material`.
/// Un Appearance senza material in VRML è "non illuminato bianco": in quel caso `None`.
pub fn parse_appearance_block(
    data: &[u8],
    open: usize,
    close: usize,
    brace_map: &HashMap<usize, usize>,
    registry: &MaterialRegistry,
) -> Option<Rgba> {
    let mut pos = open + 1;
    while pos < close {
        let (word, next) = lexer::scan_word(data, pos)?;
        if next > close {
            break;
        }
        if word == "material" {
            return match read_node_value(data, next, close, brace_map).0 {
                NodeValue::Inline { ty: "Material", open, close } => {
                    Some(parse_material_block(data, open, close))
                }
                NodeValue::Use(name) => resolve(registry, name, next),
                _ => None,
            };
        }
        if data[pos] == b'{' || data[pos] == b'[' {
            // salta blocchi di altri campi (texture, textureTransform, ...)
            if let Some(&c) = brace_map.get(&pos) {
                pos = c + 1;
                continue;
            }
        }
        pos = next;
    }
    None
}

/// Colore di una Shape (`open`/`close` = posizioni delle graffe della Shape).
pub fn shape_color(
    data: &[u8],
    open: usize,
    close: usize,
    brace_map: &HashMap<usize, usize>,
    registry: &MaterialRegistry,
) -> Option<Rgba> {
    let mut pos = open + 1;
    while pos < close {
        let (word, next) = lexer::scan_word(data, pos)?;
        if next > close {
            break;
        }
        match word {
            "appearance" => {
                return match read_node_value(data, next, close, brace_map).0 {
                    NodeValue::Inline { ty: "Appearance", open, close } => {
                        parse_appearance_block(data, open, close, brace_map, registry)
                    }
                    NodeValue::Use(name) => resolve(registry, name, next),
                    _ => None,
                };
            }
            "geometry" => {
                // salta il nodo geometria (può essere enorme)
                let (_, after) = read_node_value(data, next, close, brace_map);
                pos = after;
            }
            _ => pos = next,
        }
    }
    None
}

/// Pre-scansione lineare di tutto il file: registra i `DEF Nome Material { }` e
/// `DEF Nome Appearance { }` ovunque si trovino (anche dentro Shape/Transform DEF).
/// Opera sui byte cercando il token `DEF`: non tokenizza gli array numerici.
pub fn prescan_materials(data: &[u8], brace_map: &HashMap<usize, usize>) -> MaterialRegistry {
    let mut reg = MaterialRegistry::new();
    let len = data.len();
    let mut i = 0;
    while i + 3 < len {
        // Ricerca veloce della 'D' iniziale
        match data[i..].iter().position(|&b| b == b'D') {
            Some(off) => i += off,
            None => break,
        }
        let boundary_before = i == 0 || {
            let p = data[i - 1];
            p.is_ascii_whitespace() || p == b'{' || p == b'[' || p == b','
        };
        if !(boundary_before && data[i..].starts_with(b"DEF")
            && data.get(i + 3).is_some_and(|b| b.is_ascii_whitespace()))
        {
            i += 1;
            continue;
        }
        let Some((name, after_name)) = lexer::scan_word(data, i + 3) else { break };
        let Some((ty, after_ty)) = lexer::scan_word(data, after_name) else { break };
        if ty == "Material" || ty == "Appearance" {
            let b = lexer::skip_whitespace_comments(data, after_ty);
            if b < len && data[b] == b'{' {
                if let Some(&close) = brace_map.get(&b) {
                    let color = if ty == "Material" {
                        Some(parse_material_block(data, b, close))
                    } else {
                        parse_appearance_block(data, b, close, brace_map, &reg)
                    };
                    if let Some(c) = color {
                        reg.entry(name.to_string()).or_default().push((b, c));
                    }
                }
            }
        }
        i = after_name;
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::delimiter_map::build_delimiter_maps;

    fn color_of(src: &[u8]) -> Option<Rgba> {
        let (braces, _) = build_delimiter_maps(src);
        let reg = prescan_materials(src, &braces);
        // graffa dell'ultima "Shape {"
        let at = src.windows(7).rposition(|w| w == b"Shape {").unwrap();
        let open = at + 6;
        shape_color(src, open, braces[&open], &braces, &reg)
    }

    #[test]
    fn inline_material() {
        let c = color_of(b"Shape { appearance Appearance { material Material { diffuseColor 1 0 0.5 transparency 0.25 } } geometry IndexedFaceSet { } }").unwrap();
        assert_eq!(c, [1.0, 0.0, 0.5, 0.75]);
    }

    /// Un nome ridefinito vale per i `USE` che seguono, non per quelli già scritti prima.
    #[test]
    fn redefined_material_applies_from_there_on() {
        let src = b"Shape { appearance Appearance { material DEF M Material { diffuseColor 1 0 0 } } }\n\
            Shape { appearance Appearance { material USE M } }\n\
            Shape { appearance Appearance { material DEF M Material { diffuseColor 0 1 0 } } }\n\
            Shape { appearance Appearance { material USE M } }";
        let (braces, _) = build_delimiter_maps(src);
        let reg = prescan_materials(src, &braces);
        let opens: Vec<usize> =
            src.windows(7).enumerate().filter(|(_, w)| *w == b"Shape {").map(|(i, _)| i + 6).collect();
        let color = |open: usize| shape_color(src, open, braces[&open], &braces, &reg).unwrap();
        assert_eq!(color(opens[1]), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(color(opens[3]), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn material_def_use() {
        let src = b"Shape { appearance Appearance { material DEF Rosso Material { diffuseColor 1 0 0 } } }\n\
                    Shape { appearance Appearance { material USE Rosso } geometry IndexedFaceSet { } }";
        assert_eq!(color_of(src).unwrap(), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn appearance_def_use() {
        let src = b"DEF Look Appearance { material Material { diffuseColor 0 1 0 } }\n\
                    Shape { appearance USE Look geometry IndexedFaceSet { } }";
        assert_eq!(color_of(src).unwrap(), [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn emissive_fallback_and_defaults() {
        let c = color_of(b"Shape { appearance Appearance { material Material { diffuseColor 0 0 0 emissiveColor 0 0 1 } } }").unwrap();
        assert_eq!(c, [0.0, 0.0, 1.0, 1.0]);
        let c = color_of(b"Shape { appearance Appearance { material Material { } } }").unwrap();
        assert_eq!(c, [0.8, 0.8, 0.8, 1.0]);
        assert!(color_of(b"Shape { geometry IndexedFaceSet { } }").is_none());
    }
}
