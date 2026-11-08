//! Lettura dei file Open Inventor (`.iv`, versioni 2.x, di testo o binari).
//!
//! L'Inventor di testo è quasi identico al VRML 1.0 (che ne deriva): lo legge il convertitore
//! `vrml1`, che conosce anche i nodi solo Inventor (`BaseColor`, strisce di triangoli...).
//!
//! Il binario viene prima tradotto nel testo equivalente. Formato (lo stesso scritto dall'export
//! IV, verificato con Coin): numeri a 32 bit big-endian, stringhe con la lunghezza davanti e
//! completate con zeri a multipli di 4 byte. Ogni nodo: [`DEF` nome] classe, parola di flag
//! (2 = gruppo, 1 = engine), per i nodi non standard la descrizione `fields`, il numero di campi,
//! i campi (nome, valore, parola di flag) e, per i gruppi, il numero di figli e i figli.
//!
//! Il binario non scrive il tipo dei campi: per i nodi standard lo dà la tabella `field_type`.
//! Un campo sconosciuto ferma la lettura: si tiene quello letto fino a lì, con un avviso.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;

/// Oltre questa profondità il file è considerato rotto.
const MAX_DEPTH: usize = 500;

pub(crate) fn is_inventor(data: &[u8]) -> bool {
    data.starts_with(b"#Inventor V")
}

fn header_line(data: &[u8]) -> &[u8] {
    let end = data.iter().position(|&b| b == b'\n').unwrap_or(data.len());
    &data[..end]
}

fn is_binary(data: &[u8]) -> bool {
    header_line(data).windows(6).any(|w| w == b"binary")
}

/// Testo Inventor del file (lo stesso, se è già di testo) e avvisi.
pub(crate) fn ascii(data: &[u8]) -> (Cow<'_, [u8]>, Vec<String>) {
    if !is_binary(data) {
        return (Cow::Borrowed(data), Vec::new());
    }
    let start = header_line(data).len() + 1;
    let mut bin = Binary { data, pos: start.min(data.len()), out: Vec::with_capacity(data.len() * 2) };
    bin.out.extend_from_slice(b"#Inventor V2.1 ascii\n");
    let mut warnings = Vec::new();
    while bin.data.len() - bin.pos >= 4 {
        // Zeri di riempimento in fondo
        if bin.data[bin.pos..].iter().all(|&b| b == 0) {
            break;
        }
        let mark = bin.out.len();
        if let Err(e) = bin.node(0) {
            warnings.push(format!("IV binario letto solo in parte: {e}"));
            close_braces(&mut bin.out, mark);
            break;
        }
    }
    (Cow::Owned(bin.out), warnings)
}

/// Chiude le parentesi rimaste aperte da un nodo interrotto, tenendo quello già scritto.
fn close_braces(out: &mut Vec<u8>, from: usize) {
    // Un vettore o un campo lasciato a metà: si torna all'ultima riga completa
    if let Some(last) = out[from..].iter().rposition(|&b| b == b'\n') {
        out.truncate(from + last + 1);
    } else {
        out.truncate(from);
    }
    let (mut braces, mut brackets) = (0i64, 0i64);
    let mut in_string = false;
    let mut prev = 0u8;
    for &b in &out[from..] {
        if in_string {
            in_string = !(b == b'"' && prev != b'\\');
        } else {
            match b {
                b'"' => in_string = true,
                b'{' => braces += 1,
                b'}' => braces -= 1,
                b'[' => brackets += 1,
                b']' => brackets -= 1,
                _ => {}
            }
        }
        prev = b;
    }
    if brackets > 0 {
        // Un vettore aperto a metà: meglio buttare la riga del campo
        out.truncate(from);
        return;
    }
    for _ in 0..braces.max(0) {
        out.extend_from_slice(b"}\n");
    }
}

struct Binary<'a> {
    data: &'a [u8],
    pos: usize,
    out: Vec<u8>,
}

impl<'a> Binary<'a> {
    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.data.get(self.pos..self.pos + 4).ok_or("file troncato")?;
        self.pos += 4;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, String> {
        self.u32().map(f32::from_bits)
    }

    fn f64(&mut self) -> Result<f64, String> {
        let bytes = self.data.get(self.pos..self.pos + 8).ok_or("file troncato")?;
        self.pos += 8;
        Ok(f64::from_be_bytes(bytes.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<&'a str, String> {
        let len = self.u32()? as usize;
        let end = self.pos.checked_add(len).filter(|&e| e <= self.data.len()).ok_or("stringa oltre la fine del file")?;
        let s = std::str::from_utf8(&self.data[self.pos..end]).map_err(|_| format!("testo non valido alla posizione {}", self.pos))?;
        self.pos = end.next_multiple_of(4).min(self.data.len());
        Ok(s)
    }

    /// Stringa successiva senza consumarla, se c'è e sembra una parola.
    fn peek_word(&self) -> Option<&'a str> {
        let len = u32::from_be_bytes(self.data.get(self.pos..self.pos + 4)?.try_into().ok()?) as usize;
        if !(1..=256).contains(&len) {
            return None;
        }
        let bytes = self.data.get(self.pos + 4..self.pos + 4 + len)?;
        let s = std::str::from_utf8(bytes).ok()?;
        s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_|() ".contains(&b)).then_some(s)
    }

    fn node(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("nodi annidati troppo in profondità".into());
        }
        let first = self.string()?;
        match first {
            "NULL" => {
                self.out.extend_from_slice(b"NULL\n");
                return Ok(());
            }
            "USE" => {
                let name = self.string()?;
                let _ = writeln!(self.out, "USE {}", ascii_name(name));
                return Ok(());
            }
            _ => {}
        }
        let class = if first == "DEF" {
            let name = self.string()?;
            let class = self.string()?;
            let _ = write!(self.out, "DEF {} ", ascii_name(name));
            class
        } else {
            first
        };
        if class.is_empty() || !class.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(format!("nodo non valido alla posizione {}", self.pos));
        }
        let flags = self.u32()?;
        if flags & 1 != 0 {
            return Err(format!("engine «{class}» non supportato"));
        }
        let _ = writeln!(self.out, "{class} {{");

        // Nodo non standard: descrizione dei campi (tipo e nome)
        let mut described: HashMap<&'a str, &'a str> = HashMap::new();
        if self.peek_word() == Some("fields") {
            self.string()?;
            let n = self.u32()?;
            self.out.extend_from_slice(b"fields [ ");
            for i in 0..n {
                let ty = self.string()?;
                let name = self.string()?;
                described.insert(name, ty);
                let _ = write!(self.out, "{}{ty} {name}", if i > 0 { ", " } else { "" });
            }
            self.out.extend_from_slice(b" ]\n");
        }

        let fields = self.u32()?;
        for _ in 0..fields {
            let name = self.string()?;
            let ty = described
                .get(name)
                .copied()
                .or_else(|| field_type(class, name))
                .ok_or_else(|| format!("campo «{name}» del nodo «{class}» sconosciuto"))?;
            let _ = write!(self.out, "{name} ");
            self.value(ty, depth)?;
            self.out.push(b'\n');
            let field_flags = self.u32()?;
            if field_flags & 2 != 0 {
                return Err(format!("campo «{name}» collegato a un engine: non supportato"));
            }
        }
        // I nodi VRML 2.0 di Coin sono gruppi, ma i figli stanno solo nel campo `children`
        if flags & 2 != 0 && !class.starts_with("VRML") {
            let children = self.u32()?;
            for _ in 0..children {
                self.node(depth + 1)?;
            }
        }
        self.out.extend_from_slice(b"}\n");
        Ok(())
    }

    fn value(&mut self, ty: &str, depth: usize) -> Result<(), String> {
        if let Some(single) = ty.strip_prefix("MF") {
            let n = self.u32()? as usize;
            // Ogni valore occupa almeno 4 byte: un numero più grande è un file rotto
            if n > (self.data.len() - self.pos) / 4 + 1 {
                return Err(format!("{n} valori oltre la fine del file"));
            }
            self.out.extend_from_slice(b"[");
            for i in 0..n {
                if i > 0 {
                    self.out.extend_from_slice(if i % 8 == 0 { b",\n" } else { b", " });
                }
                self.single(single, depth)?;
            }
            self.out.extend_from_slice(b"]");
            Ok(())
        } else if let Some(single) = ty.strip_prefix("SF") {
            self.single(single, depth)
        } else {
            Err(format!("tipo di campo «{ty}» sconosciuto"))
        }
    }

    fn floats(&mut self, n: usize) -> Result<(), String> {
        for i in 0..n {
            if i > 0 {
                self.out.push(b' ');
            }
            let v = self.f32()?;
            let _ = write!(self.out, "{v}");
        }
        Ok(())
    }

    fn single(&mut self, ty: &str, depth: usize) -> Result<(), String> {
        match ty {
            "Bool" => {
                let v = self.u32()?;
                self.out.extend_from_slice(if v != 0 { b"TRUE" } else { b"FALSE" });
            }
            "Int32" | "Long" | "Short" => {
                let v = self.u32()? as i32;
                let _ = write!(self.out, "{v}");
            }
            "UInt32" | "ULong" | "UShort" => {
                let v = self.u32()?;
                let _ = write!(self.out, "{v}");
            }
            "Float" => self.floats(1)?,
            "Double" => {
                let v = self.f64()?;
                let _ = write!(self.out, "{v}");
            }
            "Vec2f" => self.floats(2)?,
            "Vec3f" | "Color" => self.floats(3)?,
            "Vec4f" | "Rotation" | "Plane" => self.floats(4)?,
            "Matrix" => self.floats(16)?,
            // Nome della costante, o il suo valore numerico
            "Enum" | "BitMask" => match self.peek_word() {
                Some(_) => {
                    let s = self.string()?;
                    self.out.extend_from_slice(s.as_bytes());
                }
                None => {
                    let v = self.u32()?;
                    let _ = write!(self.out, "{v}");
                }
            },
            "String" | "Name" => {
                let s = self.string()?;
                self.out.push(b'"');
                for c in s.bytes() {
                    if c == b'"' || c == b'\\' {
                        self.out.push(b'\\');
                    }
                    self.out.push(c);
                }
                self.out.push(b'"');
            }
            "Node" => {
                self.out.push(b'\n');
                self.node(depth + 1)?;
            }
            other => return Err(format!("tipo di campo «{other}» non supportato")),
        }
        Ok(())
    }
}

/// Nome `DEF`/`USE` valido nel testo (il binario ammette qualunque carattere).
fn ascii_name(name: &str) -> Cow<'_, str> {
    let ok = |(i, c): (usize, char)| c.is_ascii_alphanumeric() || c == '_' || (i > 0 && c.is_ascii_digit());
    if !name.is_empty() && name.char_indices().all(ok) && !name.starts_with(|c: char| c.is_ascii_digit()) {
        Cow::Borrowed(name)
    } else {
        let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
            s.insert(0, '_');
        }
        Cow::Owned(s)
    }
}

/// Tipo dei campi dei nodi VRML 2.0 di Coin (`VRMLTransform`, `VRMLShape`...): quelli del
/// VRML97, dove i campi dei materiali sono singoli e i figli sono un campo con dei nodi.
fn vrml97_field_type(class: &str, field: &str) -> Option<&'static str> {
    let specific = match (class, field) {
        ("VRMLSwitch", "choice") | ("VRMLLOD", "level") => Some("MFNode"),
        ("VRMLSwitch", "whichChoice") => Some("SFInt32"),
        ("VRMLLOD", "range") => Some("MFFloat"),
        ("VRMLColor", "color") => Some("MFColor"),
        ("VRMLTextureCoordinate", "point") => Some("MFVec2f"),
        ("VRMLTextureTransform", "center" | "scale" | "translation") => Some("SFVec2f"),
        ("VRMLTextureTransform", "rotation") => Some("SFFloat"),
        ("VRMLTransform", "scale") => Some("SFVec3f"),
        ("VRMLBox", "size") => Some("SFVec3f"),
        ("VRMLWorldInfo", "info") => Some("MFString"),
        ("VRMLNavigationInfo", "type") => Some("MFString"),
        ("VRMLNavigationInfo", "avatarSize") => Some("MFFloat"),
        ("VRMLViewpoint" | "VRMLAnchor", "description") => Some("SFString"),
        ("VRMLBackground", "groundColor" | "skyColor") => Some("MFColor"),
        ("VRMLBackground", "groundAngle" | "skyAngle") => Some("MFFloat"),
        ("VRMLIndexedFaceSet" | "VRMLIndexedLineSet" | "VRMLPointSet", "color") => Some("SFNode"),
        ("VRMLIndexedFaceSet", "normal") => Some("SFNode"),
        ("VRMLIndexedFaceSet", "texCoord") => Some("SFNode"),
        _ => None,
    };
    specific.or(match field {
        "children" | "addChildren" | "removeChildren" => Some("MFNode"),
        "appearance" | "geometry" | "material" | "texture" | "textureTransform" | "coord" | "proxy" => Some("SFNode"),
        "coordIndex" | "normalIndex" | "colorIndex" | "texCoordIndex" => Some("MFInt32"),
        "ccw" | "convex" | "solid" | "colorPerVertex" | "normalPerVertex" | "collide" | "on" | "headlight"
        | "jump" | "repeatS" | "repeatT" | "bottom" | "side" | "top" | "loop" | "global" => Some("SFBool"),
        "translation" | "center" | "bboxCenter" | "bboxSize" | "axisOfRotation" | "position" | "direction"
        | "location" | "attenuation" => Some("SFVec3f"),
        "rotation" | "scaleOrientation" | "orientation" => Some("SFRotation"),
        "ambientIntensity" | "shininess" | "transparency" | "creaseAngle" | "intensity" | "radius" | "height"
        | "bottomRadius" | "speed" | "visibilityLimit" | "fieldOfView" | "beamWidth" | "cutOffAngle" => Some("SFFloat"),
        "diffuseColor" | "specularColor" | "emissiveColor" | "color" => Some("SFColor"),
        "point" | "vector" => Some("MFVec3f"),
        "url" | "parameter" | "backUrl" | "bottomUrl" | "frontUrl" | "leftUrl" | "rightUrl" | "topUrl" => Some("MFString"),
        "title" | "description" => Some("SFString"),
        _ => None,
    })
}

/// Tipo dei campi dei nodi standard di Open Inventor 2.1 / Coin.
fn field_type(class: &str, field: &str) -> Option<&'static str> {
    if class.starts_with("VRML") {
        return vrml97_field_type(class, field);
    }
    let specific = match (class, field) {
        ("Complexity", "value") => Some("SFFloat"),
        ("Complexity", "type") => Some("SFEnum"),
        ("Coordinate4", "point") => Some("MFVec4f"),
        ("TextureCoordinate2", "point") => Some("MFVec2f"),
        ("Info", "string") | ("Label", "label") => Some("SFString"),
        ("AsciiText" | "Text2" | "Text3", "string") => Some("MFString"),
        ("AsciiText", "width") => Some("MFFloat"),
        ("Text3" | "Cylinder" | "Cone", "parts") | ("ResetTransform", "whatToReset") => Some("SFBitMask"),
        ("Font" | "FontStyle", "name") => Some("SFName"),
        ("Font" | "FontStyle", "size") => Some("SFFloat"),
        ("Environment", "ambientColor" | "fogColor") => Some("SFColor"),
        ("Environment", "attenuation") => Some("SFVec3f"),
        ("Environment", "ambientIntensity" | "fogVisibility") => Some("SFFloat"),
        ("LevelOfDetail", "screenArea") => Some("MFFloat"),
        ("Texture2", "image") => Some("SFImage"),
        _ => None,
    };
    specific.or(match field {
        "renderCaching" | "boundingBoxCaching" | "renderCulling" | "pickCulling" => Some("SFEnum"),
        "ambientColor" | "diffuseColor" | "specularColor" | "emissiveColor" | "rgb" => Some("MFColor"),
        "shininess" | "transparency" | "range" => Some("MFFloat"),
        "value" | "vertexOrdering" | "shapeType" | "faceType" | "normalBinding" | "materialBinding" | "axis" | "style"
        | "model" | "viewportMapping" | "wrapS" | "wrapT" | "family" | "justification" | "fogType" | "units"
        | "type" => Some("SFEnum"),
        "point" | "vector" | "vertex" | "normal" => Some("MFVec3f"),
        "texCoord" => Some("MFVec2f"),
        "orderedRGBA" => Some("MFUInt32"),
        "vertexProperty" => Some("SFNode"),
        "coordIndex" | "materialIndex" | "normalIndex" | "textureCoordIndex" | "numVertices" => Some("MFInt32"),
        "startIndex" | "whichChild" => Some("SFInt32"),
        "translation" | "scaleFactor" | "center" | "direction" | "position" | "location" => Some("SFVec3f"),
        "rotation" | "scaleOrientation" | "orientation" => Some("SFRotation"),
        "matrix" => Some("SFMatrix"),
        "on" => Some("SFBool"),
        "color" | "blendColor" => Some("SFColor"),
        "angle" | "creaseAngle" | "intensity" | "dropOffRate" | "cutOffAngle" | "aspectRatio" | "nearDistance"
        | "farDistance" | "focalDistance" | "heightAngle" | "height" | "width" | "depth" | "radius"
        | "bottomRadius" | "pointSize" | "lineWidth" | "textureQuality" | "spacing" => Some("SFFloat"),
        "linePattern" => Some("SFUShort"),
        "filename" => Some("SFString"),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(out: &mut Vec<u8>, v: &str) {
        out.extend_from_slice(&(v.len() as u32).to_be_bytes());
        out.extend_from_slice(v.as_bytes());
        out.resize(out.len().next_multiple_of(4), 0);
    }
    fn u(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    fn f(out: &mut Vec<u8>, v: f32) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    #[test]
    fn binary_becomes_inventor_text() {
        let mut d = b"#Inventor V2.1 binary  \n".to_vec();
        s(&mut d, "DEF");
        s(&mut d, "Pezzo");
        s(&mut d, "Separator");
        u(&mut d, 2);
        u(&mut d, 0);
        u(&mut d, 3);
        // BaseColor { rgb [ 1 0 0 ] }
        s(&mut d, "BaseColor");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "rgb");
        u(&mut d, 1);
        [1.0, 0.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0);
        // Coordinate3 { point [ 0 0 0, 1 0 0, 0 1 0 ] }
        s(&mut d, "Coordinate3");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "point");
        u(&mut d, 3);
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0);
        // IndexedFaceSet { coordIndex [ 0 1 2 -1 ] }
        s(&mut d, "IndexedFaceSet");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "coordIndex");
        u(&mut d, 4);
        [0, 1, 2, u32::MAX].into_iter().for_each(|v| u(&mut d, v));
        u(&mut d, 0);

        let (text, warnings) = ascii(&d);
        assert!(warnings.is_empty(), "{warnings:?}");
        let text = std::str::from_utf8(&text).unwrap();
        assert!(text.starts_with("#Inventor V2.1 ascii\nDEF Pezzo Separator {"), "{text}");
        assert!(text.contains("rgb [1 0 0]") && text.contains("coordIndex [0, 1, 2, -1]"), "{text}");
        let scene = crate::parser::parse_wrl(&d);
        assert_eq!(scene.shapes_data.len(), 1);
        assert_eq!(scene.shapes_data[0].world_vertices[1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn unknown_field_keeps_what_was_read() {
        let mut d = b"#Inventor V2.1 binary  \n".to_vec();
        s(&mut d, "Separator");
        u(&mut d, 2);
        u(&mut d, 0);
        u(&mut d, 2);
        s(&mut d, "Coordinate3");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "point");
        u(&mut d, 3);
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0);
        s(&mut d, "Misterioso");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "boh");
        u(&mut d, 7);
        let (text, warnings) = ascii(&d);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let text = std::str::from_utf8(&text).unwrap();
        assert_eq!(text.matches('{').count(), text.matches('}').count(), "{text}");
        assert!(text.contains("Coordinate3"));
    }

    #[test]
    fn extension_nodes_describe_their_fields() {
        let mut d = b"#Inventor V2.1 binary  \n".to_vec();
        s(&mut d, "SoMioNodo");
        u(&mut d, 0);
        s(&mut d, "fields");
        u(&mut d, 1);
        s(&mut d, "SFFloat");
        s(&mut d, "spessore");
        u(&mut d, 1);
        s(&mut d, "spessore");
        f(&mut d, 2.5);
        u(&mut d, 0);
        let (text, warnings) = ascii(&d);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(std::str::from_utf8(&text).unwrap().contains("fields [ SFFloat spessore ]\nspessore 2.5"));
    }

    /// L'export IV binario e di testo si riaprono con la stessa geometria.
    #[test]
    fn exported_iv_reopens_identical() {
        let scene = crate::parser::parse_wrl(
            br#"#VRML V2.0 utf8
DEF Assieme Transform { translation 10 0 0 children [
  DEF Piastra Shape { appearance Appearance { material Material { diffuseColor 1 0.5 0 transparency 0.25 } }
    geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] } coordIndex [ 0 1 2 3 -1 ] } }
  DEF Lato Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 1, 1 0 1, 0 1 1 ] } coordIndex [ 0 1 2 -1 ] } }
] }
"#,
        );
        for binary in [false, true] {
            let path = std::env::temp_dir().join(format!("gestore_iv_rilettura_{binary}.iv"));
            crate::exporter::iv::export_iv(&scene, None, &path, binary, true, [0.5, 0.5, 0.5, 1.0]).unwrap();
            let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
            assert!(again.load_warnings.is_empty(), "{:?}", again.load_warnings);
            assert_eq!(again.shapes_data.len(), 2);
            let mut a: Vec<_> = scene.shapes_data.iter().map(|s| s.world_vertices.clone()).collect();
            let mut b: Vec<_> = again.shapes_data.iter().map(|s| s.world_vertices.clone()).collect();
            a.sort_by(|x, y| x.partial_cmp(y).unwrap());
            b.sort_by(|x, y| x.partial_cmp(y).unwrap());
            assert_eq!(a, b, "binario {binary}");
            let names: Vec<_> = (0..again.nodes.len() as u32).map(|n| again.display_name(n)).collect();
            assert!(names.iter().any(|n| n == "Piastra"), "{names:?}");
            let piastra = again.shapes_data.iter().find(|s| s.world_vertices.len() == 4).unwrap();
            assert_eq!(piastra.color.map(|c| (c[1] * 10.0).round()), Some(5.0));
            let _ = std::fs::remove_file(path);
        }
    }

    /// File veri (non nel repository): `GESTORE_IV_DIR` = cartella con gli .iv.
    /// IV binario come quelli del visualizzatore usato in azienda: un involucro Inventor con la
    /// posa (`Translation`, `RotationXYZ`) e dentro la scena VRML 2.0 di Coin, con i figli nel
    /// campo `children` e senza numero di figli dopo i campi.
    #[test]
    fn coin_vrml2_nodes_in_a_binary_file() {
        let mut d = b"#Inventor V2.1 binary  \n".to_vec();
        s(&mut d, "Separator");
        u(&mut d, 2);
        u(&mut d, 0);
        u(&mut d, 3);
        // Translation { translation 100 0 0 }
        s(&mut d, "Translation");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "translation");
        [100.0, 0.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0);
        // RotationXYZ {} (angolo 0)
        s(&mut d, "RotationXYZ");
        u(&mut d, 0);
        u(&mut d, 0);
        // VRMLNavigationInfo { type ["EXAMINE"] } : da ignorare
        // VRMLTransform { children [ DEF Pezzo VRMLShape {...} ] translation 0 10 0 }
        s(&mut d, "VRMLTransform");
        u(&mut d, 2);
        u(&mut d, 2);
        s(&mut d, "children");
        u(&mut d, 1);
        s(&mut d, "DEF");
        s(&mut d, "Pezzo");
        s(&mut d, "VRMLShape");
        u(&mut d, 0);
        u(&mut d, 2);
        s(&mut d, "appearance");
        s(&mut d, "VRMLAppearance");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "material");
        s(&mut d, "VRMLMaterial");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "diffuseColor");
        [1.0, 0.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0); // flag di diffuseColor
        u(&mut d, 0); // flag di material
        u(&mut d, 0); // flag di appearance
        s(&mut d, "geometry");
        s(&mut d, "VRMLIndexedFaceSet");
        u(&mut d, 0);
        u(&mut d, 2);
        s(&mut d, "coord");
        s(&mut d, "VRMLCoordinate");
        u(&mut d, 0);
        u(&mut d, 1);
        s(&mut d, "point");
        u(&mut d, 3);
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0); // flag di point
        u(&mut d, 0); // flag di coord
        s(&mut d, "coordIndex");
        u(&mut d, 4);
        [0, 1, 2, u32::MAX].into_iter().for_each(|v| u(&mut d, v));
        u(&mut d, 0); // flag di coordIndex
        u(&mut d, 0); // flag di geometry
        u(&mut d, 0); // flag di children
        s(&mut d, "translation");
        [0.0, 10.0, 0.0].into_iter().for_each(|v| f(&mut d, v));
        u(&mut d, 0);

        let (_, warnings) = ascii(&d);
        assert!(warnings.is_empty(), "{warnings:?}");
        let scene = crate::parser::parse_wrl(&d);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        assert_eq!(scene.shapes_data.len(), 1);
        let shape = &scene.shapes_data[0];
        assert_eq!(shape.world_vertices[1], [101.0, 10.0, 0.0], "posa Inventor + Transform VRML");
        assert_eq!(shape.color, Some([1.0, 0.0, 0.0, 1.0]));
        assert!((0..scene.nodes.len() as u32).any(|n| scene.display_name(n) == "Pezzo"));
    }

    /// IV di testo con `VRMLMaterial` come materiale e i vertici nel `VertexProperty`.
    #[test]
    fn vertex_property_and_vrml_material_in_text() {
        let text = br#"#Inventor V2.1 ascii
Separator {
  Translation { translation 0 0 5 }
  VRMLMaterial { diffuseColor 0 0 1 transparency 0.5 }
  ShapeHints { vertexOrdering COUNTERCLOCKWISE creaseAngle 0.5 }
  DEF Lamiera IndexedFaceSet {
    vertexProperty VertexProperty {
      vertex [ 0 0 0, 2 0 0, 0 2 0 ]
      normal [ 0 0 1, 0 0 1, 0 0 1 ]
      normalBinding PER_VERTEX_INDEXED
    }
    coordIndex [ 0, 1, 2, -1 ]
  }
}
"#;
        let scene = crate::parser::parse_wrl(text);
        assert!(scene.load_warnings.is_empty(), "{:?}", scene.load_warnings);
        assert_eq!(scene.shapes_data.len(), 1);
        let shape = &scene.shapes_data[0];
        assert_eq!(shape.world_vertices[1], [2.0, 0.0, 5.0]);
        assert_eq!(shape.color, Some([0.0, 0.0, 1.0, 0.5]));
        assert!(shape.normals.iter().all(|n| n[2] > 0.99), "{:?}", shape.normals);
    }

    #[test]
    #[ignore]
    fn iv_testo() {
        // GESTORE_IV_FILE=binario.iv GESTORE_IV_OUT=testo.iv: salva la traduzione in testo
        let src = std::env::var("GESTORE_IV_FILE").expect("GESTORE_IV_FILE");
        let data = std::fs::read(src).unwrap();
        let (text, warnings) = ascii(&data);
        std::fs::write(std::env::var("GESTORE_IV_OUT").expect("GESTORE_IV_OUT"), &text).unwrap();
        println!("{} byte, avvisi {warnings:?}", text.len());
    }

    #[test]
    #[ignore]
    fn iv_reali() {
        let dir = std::env::var("GESTORE_IV_DIR").expect("GESTORE_IV_DIR");
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("iv")) {
                continue;
            }
            let data = std::fs::read(&p).unwrap();
            let t = std::time::Instant::now();
            let scene = crate::parser::parse_wrl(&data);
            let tris: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
            println!(
                "{}: {:.1} MB: {} pezzi, {tris} triangoli, {:?}, avvisi {:?}",
                p.file_name().unwrap().to_string_lossy(),
                data.len() as f64 / 1e6,
                scene.shapes_data.len(),
                t.elapsed(),
                scene.load_warnings
            );
        }
    }
}
