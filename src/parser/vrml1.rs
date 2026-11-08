//! Lettura dei file VRML 1.0: il testo viene convertito in memoria in VRML 2.0.
//!
//! In VRML 1.0 materiale, coordinate, normali e trasformazioni non stanno nella `Shape` ma in uno
//! stato che si accumula visitando i nodi in ordine (`Separator` lo salva e lo ripristina).
//! Convertendo il testo, albero, rendering ed export restano quelli del VRML 2.0: l'export di un
//! file 1.0 è un VRML 2.0 valido, e il file originale non viene toccato.
//!
//! * Nell'albero entrano solo i contenitori con un nome `DEF` e con delle superfici dentro: i
//!   `Separator` anonimi servono solo a delimitare lo stato.
//! * Le trasformazioni vengono applicate alle coordinate. Senza trasformazioni il testo dei numeri
//!   è copiato, senza perdere cifre.
//! * Di un `LOD` si legge il primo livello con un contenuto, come nel VRML 2.0.
//! * Linee, punti, primitive e `WWWInline` non vengono caricati: finiscono negli avvisi.

use std::collections::HashMap;
use std::io::Write;

use super::{delimiter_map, latest_before, lexer};

/// Oltre questa profondità si smette di scendere: protegge dai `USE` che citano sé stessi.
const MAX_DEPTH: usize = 200;

pub(crate) fn is_vrml1(data: &[u8]) -> bool {
    data.starts_with(b"#VRML V1.0")
}

pub(crate) struct Converted {
    /// Testo VRML 2.0
    pub text: Vec<u8>,
    /// Contenuto del file che non è stato caricato, da mostrare all'utente
    pub warnings: Vec<String>,
}

pub(crate) fn convert(data: &[u8]) -> Converted {
    let (braces, brackets) = delimiter_map::build_delimiter_maps(data);
    let mut conv = Conv {
        data,
        braces,
        brackets,
        defs: None,
        out: b"#VRML V2.0 utf8\n# Convertito da VRML 1.0 da Gestore WRL\n".to_vec(),
        shapes: 0,
        counts: Counts::default(),
    };
    let mut state = State::default();
    for item in conv.items(0, data.len()) {
        conv.visit(item, &mut state, 0);
    }
    let warnings = conv.counts.warnings();
    Converted { text: conv.out, warnings }
}

/// Nodi VRML 2.0 di Coin che formano la scena: si passano al lettore VRML 2.0 così come sono.
/// Gli altri nodi `VRML*` (`VRMLWorldInfo`, `VRMLViewpoint`, `VRMLBackground`...) non hanno
/// geometria e si ignorano.
fn is_vrml2_subtree(ty: &str) -> bool {
    matches!(
        ty,
        "VRMLTransform" | "VRMLGroup" | "VRMLShape" | "VRMLCollision" | "VRMLSwitch" | "VRMLLOD" | "VRMLAnchor"
            | "VRMLBillboard"
    )
}

/// Testo VRML 2.0 dai nomi dei nodi di Coin: `VRMLTransform` → `Transform`, ... (solo le parole
/// che cominciano con `VRML` seguito da una maiuscola, fuori dalle stringhe).
fn rename_vrml2(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < text.len() {
        let b = text[i];
        if in_string {
            out.push(b);
            if b == b'\\' && i + 1 < text.len() {
                out.push(text[i + 1]);
                i += 2;
                continue;
            }
            in_string = b != b'"';
            i += 1;
            continue;
        }
        let word_start = i == 0 || !(text[i - 1].is_ascii_alphanumeric() || text[i - 1] == b'_');
        if b == b'"' {
            in_string = true;
        } else if word_start && text[i..].starts_with(b"VRML") && text.get(i + 4).is_some_and(|c| c.is_ascii_uppercase()) {
            i += 4;
            continue;
        }
        out.push(b);
        i += 1;
    }
    out
}

/// Contenuto ignorato durante la conversione.
#[derive(Default)]
struct Counts {
    lines: usize,
    unsupported: usize,
    hidden_switches: usize,
    multi_colors: usize,
    broken: usize,
}

impl Counts {
    fn warnings(&self) -> Vec<String> {
        let mut list = Vec::new();
        if self.lines > 0 {
            list.push(format!(
                "VRML 1.0: {} nodi di linee o punti non caricati né esportati (si leggono solo le superfici)",
                self.lines
            ));
        }
        if self.unsupported > 0 {
            list.push(format!(
                "VRML 1.0: {} nodi non supportati (primitive, testo o WWWInline) non caricati né esportati",
                self.unsupported
            ));
        }
        if self.hidden_switches > 0 {
            list.push(format!(
                "VRML 1.0: {} nodi Switch spenti: il loro contenuto non è caricato né esportato",
                self.hidden_switches
            ));
        }
        if self.multi_colors > 0 {
            list.push(format!(
                "VRML 1.0: {} superfici con più colori mostrate con il solo primo colore",
                self.multi_colors
            ));
        }
        if self.broken > 0 {
            list.push(format!(
                "VRML 1.0: {} nodi non letti perché incompleti o con riferimenti non validi",
                self.broken
            ));
        }
        list
    }
}

/// Nodo del file: `Tipo { ... }`, `DEF Nome Tipo { ... }` o `USE Nome` (`ty` = "USE").
#[derive(Clone, Copy)]
struct Item<'a> {
    name: Option<&'a str>,
    ty: &'a str,
    /// Posizione del nome citato, per i `USE`
    pos: usize,
    open: usize,
    close: usize,
}

#[derive(Clone, Copy)]
struct Material {
    diffuse: [f32; 3],
    emissive: [f32; 3],
    specular: [f32; 3],
    shininess: f32,
    transparency: f32,
    multi_color: bool,
}

/// Stato di visita del VRML 1.0. Gli intervalli sono posizioni nel file sorgente.
#[derive(Clone, Copy)]
struct State<'a> {
    matrix: glam::Mat4,
    material: Option<Material>,
    coords: Option<(usize, usize)>,
    normals: Option<(usize, usize)>,
    normal_binding: &'a str,
    material_binding: &'a str,
    ordering: &'a str,
    shape_type: &'a str,
    crease_angle: Option<f32>,
}

impl Default for State<'_> {
    fn default() -> Self {
        Self {
            matrix: glam::Mat4::IDENTITY,
            material: None,
            coords: None,
            normals: None,
            normal_binding: "DEFAULT",
            material_binding: "DEFAULT",
            ordering: "UNKNOWN_ORDERING",
            shape_type: "UNKNOWN_SHAPE_TYPE",
            crease_angle: None,
        }
    }
}

struct Conv<'a> {
    data: &'a [u8],
    braces: HashMap<usize, usize>,
    brackets: HashMap<usize, usize>,
    /// nome → (posizione, inizio del nodo), in ordine di file. Costruito al primo `USE`.
    defs: Option<HashMap<&'a str, Vec<(usize, usize)>>>,
    out: Vec<u8>,
    shapes: usize,
    counts: Counts,
}

fn skip_string(data: &[u8], open: usize) -> usize {
    let mut i = open + 1;
    while i < data.len() {
        match data[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

impl<'a> Conv<'a> {
    /// Legge il nodo che inizia in `pos`, se è un nodo, e la posizione dopo di esso.
    fn read_item(&self, pos: usize) -> Option<(Item<'a>, usize)> {
        let (word, next) = lexer::scan_word(self.data, pos)?;
        if word == "USE" {
            let name_pos = lexer::skip_whitespace_comments(self.data, next);
            let (name, after) = lexer::scan_word(self.data, name_pos)?;
            let item = Item { name: Some(name), ty: "USE", pos: name_pos, open: 0, close: 0 };
            return Some((item, after));
        }
        let (name, ty, after_ty) = if word == "DEF" {
            let (name, after_name) = lexer::scan_word(self.data, next)?;
            let (ty, after_ty) = lexer::scan_word(self.data, after_name)?;
            (Some(name), ty, after_ty)
        } else {
            (None, word, next)
        };
        let open = lexer::skip_whitespace_comments(self.data, after_ty);
        if self.data.get(open) != Some(&b'{') {
            return None;
        }
        let close = *self.braces.get(&open)?;
        Some((Item { name, ty, pos, open, close }, close + 1))
    }

    /// Nodi di primo livello nell'intervallo; i campi semplici e i vettori vengono saltati.
    fn items(&self, start: usize, end: usize) -> Vec<Item<'a>> {
        let mut items = Vec::new();
        let mut pos = start;
        loop {
            pos = lexer::skip_whitespace_comments(self.data, pos);
            if pos >= end {
                return items;
            }
            match self.data[pos] {
                b'"' => pos = skip_string(self.data, pos),
                b'[' => pos = self.brackets.get(&pos).map_or(pos + 1, |&c| c + 1),
                b'{' => pos = self.braces.get(&pos).map_or(pos + 1, |&c| c + 1),
                b',' | b'}' | b']' => pos += 1,
                _ => {
                    if let Some((item, next)) = self.read_item(pos) {
                        items.push(item);
                        pos = next;
                    } else {
                        let Some((_, next)) = lexer::scan_word(self.data, pos) else {
                            return items;
                        };
                        pos = next;
                    }
                }
            }
        }
    }

    fn children(&self, item: Item<'a>) -> Vec<Item<'a>> {
        self.items(item.open + 1, item.close)
    }

    /// Campi di un nodo foglia: (nome, inizio del valore, fine del valore).
    fn fields(&self, item: Item<'a>) -> Vec<(&'a str, usize, usize)> {
        let mut fields = Vec::new();
        let mut current: Option<(&'a str, usize)> = None;
        let mut has_value = false;
        let mut pos = item.open + 1;
        loop {
            pos = lexer::skip_whitespace_comments(self.data, pos);
            if pos >= item.close {
                break;
            }
            match self.data[pos] {
                b',' => pos += 1,
                b'[' => {
                    pos = self.brackets.get(&pos).map_or(pos + 1, |&c| c + 1);
                    has_value = true;
                }
                b'{' => {
                    pos = self.braces.get(&pos).map_or(pos + 1, |&c| c + 1);
                    has_value = true;
                }
                b'"' => {
                    pos = skip_string(self.data, pos);
                    has_value = true;
                }
                c => {
                    let Some((word, next)) = lexer::scan_word(self.data, pos) else {
                        break;
                    };
                    // Una parola è un campo nuovo, salvo che sia il valore del campo appena letto
                    // (`value OVERALL`, `vertexOrdering CLOCKWISE`)
                    if c.is_ascii_alphabetic() && (current.is_none() || has_value) {
                        if let Some((name, start)) = current {
                            fields.push((name, start, pos));
                        }
                        current = Some((word, next));
                        has_value = false;
                    } else {
                        has_value = true;
                    }
                    pos = next;
                }
            }
        }
        if let Some((name, start)) = current {
            fields.push((name, start, item.close));
        }
        fields
    }

    fn numbers_i32(&self, fields: &[(&'a str, usize, usize)], name: &str) -> Vec<i32> {
        match fields.iter().find(|f| f.0 == name) {
            Some(&(_, start, end)) => lexer::scan_numbers_i32(self.data, start, end),
            None => Vec::new(),
        }
    }

    fn numbers(&self, fields: &[(&'a str, usize, usize)], name: &str) -> Option<Vec<f32>> {
        let &(_, start, end) = fields.iter().find(|f| f.0 == name)?;
        Some(lexer::scan_numbers_f32(self.data, start, end))
    }

    fn vec3(&self, fields: &[(&'a str, usize, usize)], name: &str, default: [f32; 3]) -> [f32; 3] {
        match self.numbers(fields, name).as_deref() {
            Some([x, y, z, ..]) => [*x, *y, *z],
            _ => default,
        }
    }

    fn scalar(&self, fields: &[(&'a str, usize, usize)], name: &str, default: f32) -> f32 {
        self.numbers(fields, name).and_then(|n| n.first().copied()).unwrap_or(default)
    }

    fn rotation(&self, fields: &[(&'a str, usize, usize)], name: &str) -> glam::Mat4 {
        match self.numbers(fields, name).as_deref() {
            Some(&[x, y, z, angle, ..]) => {
                let axis = glam::Vec3::new(x, y, z).normalize_or_zero();
                if axis == glam::Vec3::ZERO {
                    glam::Mat4::IDENTITY
                } else {
                    glam::Mat4::from_axis_angle(axis, angle)
                }
            }
            _ => glam::Mat4::IDENTITY,
        }
    }

    /// Parola singola del campo (`value OVERALL`).
    fn word(&self, fields: &[(&'a str, usize, usize)], name: &str) -> Option<&'a str> {
        let &(_, start, _) = fields.iter().find(|f| f.0 == name)?;
        lexer::scan_word(self.data, start).map(|(w, _)| w)
    }

    /// Contenuto di un vettore, senza le quadre (in VRML 1.0 un valore solo può non averle).
    fn inner(&self, start: usize, end: usize) -> (usize, usize) {
        let open = lexer::skip_whitespace_comments(self.data, start);
        match self.brackets.get(&open) {
            Some(&close) => (open + 1, close),
            None => (start, end),
        }
    }

    fn visit(&mut self, item: Item<'a>, state: &mut State<'a>, depth: usize) {
        if depth > MAX_DEPTH {
            self.counts.broken += 1;
            return;
        }
        let fields = |conv: &Self| conv.fields(item);
        match item.ty {
            "USE" => match self.resolve(item) {
                // Un nodo VRML 2.0 resta un `USE`, che il lettore VRML 2.0 sa risolvere
                Some(target) if is_vrml2_subtree(target.ty) => {
                    let text = format!("USE {}", item.name.unwrap_or_default());
                    self.vrml2(text.as_bytes(), state);
                }
                // Il nodo viene rivisitato con lo stato corrente, che può essere diverso da quello
                // del `DEF`. Il nome resta: l'istanza compare nell'albero come l'originale.
                Some(target) => self.visit(target, state, depth + 1),
                None => {
                    tracing::warn!("USE non trovato: {:?}", item.name);
                    self.counts.broken += 1;
                }
            },
            "Separator" | "WWWAnchor" => {
                let mut inner = *state;
                self.group(item, &mut inner, depth);
            }
            "Group" => self.group(item, state, depth),
            "TransformSeparator" => {
                let matrix = state.matrix;
                self.group(item, state, depth);
                state.matrix = matrix;
            }
            "LOD" => self.lod(item, state, depth),
            "Switch" => self.switch(item, state, depth),
            // VRMLMaterial (Coin) in una scena Inventor: vale come materiale, con i campi singoli
            "Material" | "VRMLMaterial" => {
                let f = fields(self);
                let diffuse = self.numbers(&f, "diffuseColor").unwrap_or_default();
                state.material = Some(Material {
                    diffuse: self.vec3(&f, "diffuseColor", [0.8, 0.8, 0.8]),
                    emissive: self.vec3(&f, "emissiveColor", [0.0; 3]),
                    specular: self.vec3(&f, "specularColor", [0.0; 3]),
                    shininess: self.scalar(&f, "shininess", 0.2),
                    transparency: self.scalar(&f, "transparency", 0.0),
                    multi_color: diffuse.len() > 3,
                });
            }
            "Coordinate3" => {
                let f = fields(self);
                state.coords = f.iter().find(|x| x.0 == "point").map(|x| self.inner(x.1, x.2));
            }
            "Normal" => {
                let f = fields(self);
                state.normals = f.iter().find(|x| x.0 == "vector").map(|x| self.inner(x.1, x.2));
            }
            "NormalBinding" => {
                let f = fields(self);
                state.normal_binding = self.word(&f, "value").unwrap_or("DEFAULT");
            }
            "MaterialBinding" => {
                let f = fields(self);
                state.material_binding = self.word(&f, "value").unwrap_or("DEFAULT");
            }
            "ShapeHints" => {
                let f = fields(self);
                state.ordering = self.word(&f, "vertexOrdering").unwrap_or("UNKNOWN_ORDERING");
                state.shape_type = self.word(&f, "shapeType").unwrap_or("UNKNOWN_SHAPE_TYPE");
                state.crease_angle = self.numbers(&f, "creaseAngle").and_then(|n| n.first().copied());
            }
            "Translation" => {
                let f = fields(self);
                let t = self.vec3(&f, "translation", [0.0; 3]);
                state.matrix *= glam::Mat4::from_translation(t.into());
            }
            "Rotation" => {
                let f = fields(self);
                state.matrix *= self.rotation(&f, "rotation");
            }
            "Scale" => {
                let f = fields(self);
                let s = self.vec3(&f, "scaleFactor", [1.0; 3]);
                state.matrix *= glam::Mat4::from_scale(s.into());
            }
            "Transform" => {
                let f = fields(self);
                let translation = glam::Mat4::from_translation(self.vec3(&f, "translation", [0.0; 3]).into());
                let center: glam::Vec3 = self.vec3(&f, "center", [0.0; 3]).into();
                let orientation = self.rotation(&f, "scaleOrientation");
                let scale = glam::Mat4::from_scale(self.vec3(&f, "scaleFactor", [1.0; 3]).into());
                state.matrix *= translation
                    * glam::Mat4::from_translation(center)
                    * self.rotation(&f, "rotation")
                    * orientation
                    * scale
                    * orientation.inverse()
                    * glam::Mat4::from_translation(-center);
            }
            "MatrixTransform" => {
                let f = fields(self);
                if let Some(m) = self.numbers(&f, "matrix").and_then(|n| <[f32; 16]>::try_from(n).ok()) {
                    // Il testo elenca le righe con la traslazione in fondo: sono le colonne di glam
                    state.matrix *= glam::Mat4::from_cols_array(&m);
                }
            }
            "IndexedFaceSet" => {
                let local = self.with_vertex_property(item, state);
                self.face_set(item, &local, None);
            }
            // Sotto-albero VRML 2.0 dentro una scena Inventor (Coin): passa al lettore VRML 2.0
            ty if is_vrml2_subtree(ty) => {
                let text = rename_vrml2(&self.data[item.pos..=item.close]);
                self.vrml2(&text, state);
            }
            // Nodi solo Open Inventor
            "BaseColor" => {
                let f = fields(self);
                let rgb = self.numbers(&f, "rgb").unwrap_or_default();
                let mut m = state.material.unwrap_or(Material {
                    diffuse: [0.8; 3],
                    emissive: [0.0; 3],
                    specular: [0.0; 3],
                    shininess: 0.2,
                    transparency: 0.0,
                    multi_color: false,
                });
                if let [r, g, b, ..] = rgb[..] {
                    m.diffuse = [r, g, b];
                }
                m.multi_color = rgb.len() > 3;
                state.material = Some(m);
            }
            "RotationXYZ" => {
                let f = fields(self);
                let axis = match self.word(&f, "axis") {
                    Some("Y") => glam::Vec3::Y,
                    Some("Z") => glam::Vec3::Z,
                    _ => glam::Vec3::X,
                };
                state.matrix *= glam::Mat4::from_axis_angle(axis, self.scalar(&f, "angle", 0.0));
            }
            "ResetTransform" => {
                let f = fields(self);
                if self.word(&f, "whatToReset").is_none_or(|w| w.contains("TRANSFORM")) {
                    state.matrix = glam::Mat4::IDENTITY;
                }
            }
            "IndexedTriangleStripSet" => {
                let f = fields(self);
                let index = self.numbers_i32(&f, "coordIndex");
                let faces = strips_to_faces(index.split(|&i| i < 0));
                let local = self.with_vertex_property(item, state);
                self.face_set(item, &local, Some(faces));
            }
            "TriangleStripSet" | "FaceSet" => {
                let f = fields(self);
                let Some((start, end)) = state.coords else {
                    self.counts.broken += 1;
                    return;
                };
                let vertices = (lexer::scan_numbers_f32(self.data, start, end).len() / 3) as i32;
                let mut next = self.scalar(&f, "startIndex", 0.0) as i32;
                let mut groups: Vec<Vec<i32>> = Vec::new();
                let mut counts = self.numbers_i32(&f, "numVertices");
                if counts.is_empty() {
                    counts.push(-1);
                }
                for n in counts {
                    // -1: tutti i vertici rimasti
                    let n = if n < 0 { vertices - next } else { n };
                    groups.push((next..(next + n).min(vertices)).collect());
                    next += n;
                }
                let faces = if item.ty == "FaceSet" {
                    groups.iter().flat_map(|g| g.iter().copied().chain([-1])).collect()
                } else {
                    strips_to_faces(groups.iter().map(Vec::as_slice))
                };
                self.face_set(item, state, Some(faces));
            }
            "IndexedLineSet" | "PointSet" | "LineSet" => self.counts.lines += 1,
            "Cube" | "Sphere" | "Cone" | "Cylinder" | "AsciiText" | "WWWInline" | "Text2" | "Text3"
            | "NurbsSurface" | "IndexedNurbsSurface" => {
                self.counts.unsupported += 1;
            }
            // Info, luci, camere, texture...: senza effetto sulla geometria
            _ => {}
        }
    }

    /// Inventor 2.1: vertici e normali di una superficie possono stare nel suo campo
    /// `vertexProperty` (un nodo `VertexProperty`) invece che nello stato.
    fn with_vertex_property(&mut self, item: Item<'a>, state: &State<'a>) -> State<'a> {
        let mut local = *state;
        let fields = self.fields(item);
        let Some(&(_, start, end)) = fields.iter().find(|f| f.0 == "vertexProperty") else { return local };
        let Some(mut vp) = self.items(start, end).into_iter().next() else { return local };
        if vp.ty == "USE" {
            match self.resolve(vp) {
                Some(target) => vp = target,
                None => return local,
            }
        }
        let f = self.fields(vp);
        if let Some(&(_, s, e)) = f.iter().find(|x| x.0 == "vertex") {
            local.coords = Some(self.inner(s, e));
        }
        if let Some(&(_, s, e)) = f.iter().find(|x| x.0 == "normal") {
            local.normals = Some(self.inner(s, e));
            local.normal_binding = self.word(&f, "normalBinding").unwrap_or("PER_VERTEX_INDEXED");
        }
        local
    }

    /// Scrive un pezzo di VRML 2.0 così com'è, dentro un `Transform` con la posizione accumulata
    /// dai nodi Inventor che lo precedono.
    fn vrml2(&mut self, text: &[u8], state: &State<'a>) {
        if state.matrix == glam::Mat4::IDENTITY {
            self.out.extend_from_slice(text);
            self.out.push(b'\n');
        } else {
            let (scale, rotation, translation) = state.matrix.to_scale_rotation_translation();
            let (axis, angle) = rotation.to_axis_angle();
            let _ = write!(
                self.out,
                "Transform {{ translation {} {} {} rotation {} {} {} {} scale {} {} {} children [\n",
                translation.x, translation.y, translation.z, axis.x, axis.y, axis.z, angle, scale.x, scale.y, scale.z
            );
            self.out.extend_from_slice(text);
            self.out.extend_from_slice(b"\n] }\n");
        }
        // Il contenitore con nome che lo contiene va tenuto
        self.shapes += 1;
    }

    fn resolve(&mut self, item: Item<'a>) -> Option<Item<'a>> {
        if self.defs.is_none() {
            self.defs = Some(self.scan_defs());
        }
        let defs = self.defs.as_ref()?.get(item.name?)?;
        let &(_, start) = latest_before(defs, item.pos, |d| d.0)?;
        self.read_item(start).map(|(target, _)| target)
    }

    fn scan_defs(&self) -> HashMap<&'a str, Vec<(usize, usize)>> {
        let mut defs: HashMap<&'a str, Vec<(usize, usize)>> = HashMap::new();
        let mut pos = 0;
        while let Some((word, next)) = lexer::scan_word(self.data, pos) {
            let at = lexer::skip_whitespace_comments(self.data, pos);
            pos = match word {
                "\"" => skip_string(self.data, at),
                "[" => self.brackets.get(&at).map_or(next, |&c| c + 1),
                "DEF" => {
                    let Some((name, _)) = lexer::scan_word(self.data, next) else {
                        break;
                    };
                    defs.entry(name).or_default().push((at, at));
                    // Si riparte dopo il nome: anche il contenuto può avere dei `DEF`
                    lexer::scan_word(self.data, next).map_or(next, |(_, after)| after)
                }
                _ => next,
            };
        }
        defs
    }

    /// Scrive un contenitore con nome solo se contiene delle superfici.
    fn with_group(&mut self, name: Option<&str>, body: impl FnOnce(&mut Self)) {
        let mark = self.out.len();
        let before = self.shapes;
        if let Some(name) = name {
            let _ = writeln!(self.out, "DEF {name} Group {{ children [");
        }
        body(self);
        if self.shapes == before {
            self.out.truncate(mark);
        } else if name.is_some() {
            self.out.extend_from_slice(b"] }\n");
        }
    }

    fn group(&mut self, item: Item<'a>, state: &mut State<'a>, depth: usize) {
        self.with_group(item.name, |conv| {
            for child in conv.children(item) {
                conv.visit(child, state, depth + 1);
            }
        });
    }

    /// Primo livello con un contenuto: gli altri sono versioni semplificate dello stesso pezzo.
    fn lod(&mut self, item: Item<'a>, state: &State<'a>, depth: usize) {
        self.with_group(item.name, |conv| {
            for child in conv.children(item) {
                let before = (conv.shapes, conv.counts.lines, conv.counts.unsupported);
                let mut level = *state;
                conv.visit(child, &mut level, depth + 1);
                if (conv.shapes, conv.counts.lines, conv.counts.unsupported) != before {
                    break;
                }
            }
        });
    }

    fn switch(&mut self, item: Item<'a>, state: &mut State<'a>, depth: usize) {
        let choice = self.scalar(&self.fields(item), "whichChild", -1.0) as i32;
        let children = self.children(item);
        // -3 in VRML 1.0: tutti i figli, come un Group
        let active: Vec<Item<'a>> = match choice {
            -3 => children,
            _ => match usize::try_from(choice).ok().and_then(|i| children.get(i)) {
                Some(&child) => vec![child],
                None => {
                    self.counts.hidden_switches += usize::from(!children.is_empty());
                    Vec::new()
                }
            },
        };
        self.with_group(item.name, |conv| {
            for child in active {
                conv.visit(child, state, depth + 1);
            }
        });
    }

    /// Superficie con le facce di `coordIndex` o, per le strisce e i nodi senza indici di
    /// Inventor, con quelle già costruite (`built`: le normali le ricalcola il parser).
    fn face_set(&mut self, item: Item<'a>, state: &State<'a>, built: Option<Vec<i32>>) {
        let fields = self.fields(item);
        let Some((coord_start, coord_end)) = state.coords else {
            self.counts.broken += 1;
            return;
        };
        let text_index = fields.iter().find(|f| f.0 == "coordIndex").map(|&(_, s, e)| (s, e));
        let data = self.data;
        let verbatim = built.is_none();
        let indices = match (built, text_index) {
            (Some(faces), _) => faces,
            (None, Some((s, e))) => lexer::scan_numbers_i32(data, s, e),
            (None, None) => {
                self.counts.broken += 1;
                return;
            }
        };
        let (index_start, index_end) = text_index.unwrap_or((0, 0));
        let points = lexer::scan_numbers_f32(data, coord_start, coord_end);
        let vertices = points.len() / 3;

        // Un `Coordinate3` può servire più superfici. Ogni superficie tiene solo i suoi vertici,
        // altrimenti il suo ingombro sarebbe quello di tutte le altre.
        let mut new_index = vec![u32::MAX; vertices];
        let mut used = 0u32;
        let (mut side, mut has_face, mut non_triangles) = (0usize, false, false);
        for &i in &indices {
            if i == -1 {
                has_face |= side >= 3;
                non_triangles |= side > 3;
                side = 0;
            } else if let Some(slot) = usize::try_from(i).ok().and_then(|i| new_index.get_mut(i)) {
                side += 1;
                if *slot == u32::MAX {
                    *slot = used;
                    used += 1;
                }
            }
        }
        has_face |= side >= 3;
        non_triangles |= side > 3;
        if !has_face {
            self.counts.broken += 1;
            return;
        }
        let all_used = used as usize == vertices;
        let identity = state.matrix == glam::Mat4::IDENTITY;
        let linear = glam::Mat3::from_mat4(state.matrix);
        let mirrored = linear.determinant() < 0.0;

        match item.name {
            Some(name) => { let _ = writeln!(self.out, "DEF {name} Shape {{"); }
            None => self.out.extend_from_slice(b"Shape {\n"),
        }
        if let Some(m) = state.material {
            let [r, g, b] = m.diffuse;
            let _ = write!(self.out, "appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b}");
            let [r, g, b] = m.emissive;
            let _ = write!(self.out, " emissiveColor {r} {g} {b}");
            let [r, g, b] = m.specular;
            let _ = write!(self.out, " specularColor {r} {g} {b}");
            let _ = writeln!(self.out, " shininess {} transparency {} }} }}", m.shininess, m.transparency);
            if m.multi_color && !matches!(state.material_binding, "DEFAULT" | "OVERALL") {
                self.counts.multi_colors += 1;
            }
        }
        self.out.extend_from_slice(b"geometry IndexedFaceSet {\n");
        // Con l'ordine dei vertici ignoto la superficie va mostrata da entrambi i lati
        let known = state.ordering != "UNKNOWN_ORDERING";
        let solid = known && state.shape_type == "SOLID";
        let _ = writeln!(self.out, "solid {}", if solid { "TRUE" } else { "FALSE" });
        if (state.ordering == "CLOCKWISE") != mirrored {
            self.out.extend_from_slice(b"ccw FALSE\n");
        }
        if let Some(angle) = state.crease_angle {
            let _ = writeln!(self.out, "creaseAngle {angle}");
        }
        if non_triangles {
            self.out.extend_from_slice(b"convex FALSE\n");
        }

        self.out.extend_from_slice(b"coord Coordinate { point [\n");
        if all_used && identity {
            self.out.extend_from_slice(&data[coord_start..coord_end]);
        } else {
            let mut kept = vec![[0.0f32; 3]; used as usize];
            for (p, &slot) in points.chunks_exact(3).zip(&new_index) {
                if let Some(v) = kept.get_mut(slot as usize) {
                    *v = state.matrix.transform_point3(glam::Vec3::new(p[0], p[1], p[2])).into();
                }
            }
            for [x, y, z] in kept {
                let _ = writeln!(self.out, "{x} {y} {z},");
            }
        }
        self.out.extend_from_slice(b"\n] }\n");

        // Normali originali, con il loro legame tradotto in VRML 2.0 (`normalPerVertex`,
        // `normalIndex`). `OVERALL` e gli altri legami: le calcola il parser dalla geometria.
        // Se gli indici non sono validi il parser le ricalcola comunque.
        let normal_index = fields
            .iter()
            .find(|f| f.0 == "normalIndex")
            .map(|&(_, s, e)| self.inner(s, e))
            .filter(|&(s, e)| lexer::scan_numbers_i32(data, s, e).iter().any(|&i| i != -1));
        let binding = match state.normal_binding {
            _ if !verbatim => None,
            "DEFAULT" | "PER_VERTEX_INDEXED" => Some((true, normal_index)),
            "PER_FACE_INDEXED" => Some((false, normal_index)),
            "PER_FACE" => Some((false, None)),
            "PER_VERTEX" => Some((true, None)),
            _ => None,
        };
        if let (Some((per_vertex, index)), Some((start, end))) = (binding, state.normals) {
            self.out.extend_from_slice(b"normal Normal { vector [\n");
            if identity {
                self.out.extend_from_slice(&data[start..end]);
            } else {
                let matrix = linear.inverse().transpose();
                for n in lexer::scan_numbers_f32(data, start, end).chunks_exact(3) {
                    let v = (matrix * glam::Vec3::new(n[0], n[1], n[2])).normalize_or_zero();
                    let _ = writeln!(self.out, "{} {} {},", v.x, v.y, v.z);
                }
            }
            self.out.extend_from_slice(b"\n] }\n");
            if !per_vertex {
                self.out.extend_from_slice(b"normalPerVertex FALSE\n");
            }
            match (index, state.normal_binding) {
                (Some((s, e)), _) => {
                    self.out.extend_from_slice(b"normalIndex [\n");
                    self.out.extend_from_slice(&data[s..e]);
                    self.out.extend_from_slice(b"\n]\n");
                }
                // Una normale per angolo, in ordine
                (None, "PER_VERTEX") => {
                    self.out.extend_from_slice(b"normalIndex [\n");
                    let mut next = 0;
                    for &i in &indices {
                        if i == -1 {
                            self.out.extend_from_slice(b"-1\n");
                        } else {
                            let _ = write!(self.out, "{next} ");
                            next += 1;
                        }
                    }
                    self.out.extend_from_slice(b"\n]\n");
                }
                // Per vertice senza `normalIndex`: le normali seguono `coordIndex`, che qui sotto
                // viene rinumerato se la superficie usa solo una parte dei vertici
                (None, _) if per_vertex && !all_used => {
                    let (s, e) = self.inner(index_start, index_end);
                    self.out.extend_from_slice(b"normalIndex [\n");
                    self.out.extend_from_slice(&data[s..e]);
                    self.out.extend_from_slice(b"\n]\n");
                }
                (None, _) => {}
            }
        }

        self.out.extend_from_slice(b"coordIndex [\n");
        if all_used && verbatim {
            let (start, end) = self.inner(index_start, index_end);
            self.out.extend_from_slice(&data[start..end]);
        } else {
            for &i in &indices {
                match usize::try_from(i).ok().and_then(|i| new_index.get(i)) {
                    Some(&n) => { let _ = write!(self.out, "{n} "); }
                    None if i == -1 => self.out.extend_from_slice(b"-1\n"),
                    None => {}
                }
            }
        }
        self.out.extend_from_slice(b"\n] }\n}\n");
        self.shapes += 1;
    }
}

/// Triangoli delle strisce (`v0 v1 v2`, `v2 v1 v3`, ... con il verso alternato), con -1 dopo
/// ognuno; i triangoli degeneri sono scartati.
fn strips_to_faces<'s>(strips: impl Iterator<Item = &'s [i32]>) -> Vec<i32> {
    let mut faces = Vec::new();
    for strip in strips {
        for (k, w) in strip.windows(3).enumerate() {
            let [a, b, c] = [w[0], w[1], w[2]];
            if a == b || b == c || a == c {
                continue;
            }
            let t = if k % 2 == 0 { [a, b, c] } else { [b, a, c] };
            faces.extend_from_slice(&t);
            faces.push(-1);
        }
    }
    faces
}

#[cfg(test)]
mod tests {
    use crate::parser::parse_wrl;
    use crate::scene_graph::SceneGraph;

    fn parse(body: &str) -> SceneGraph {
        parse_wrl(format!("#VRML V1.0 ascii\n{body}").as_bytes())
    }

    const TRIANGLE: &str = "Coordinate3 { point [ 0 0 0, 1 0 0, 0 1 0 ] } IndexedFaceSet { coordIndex [ 0, 1, 2, -1 ] }";

    #[test]
    fn separator_isolates_material_and_coordinates() {
        let scene = parse(&format!(
            r#"Info {{ string "testo con {{ graffa e # cancelletto" }}
            Separator {{
                Material {{ diffuseColor 1 0 0 }}
                Separator {{
                    Material {{ diffuseColor [ 0 1 0 ] transparency 0.25 }}
                    {TRIANGLE}
                }}
                IndexedFaceSet {{ coordIndex [ 0, 1, 2, -1 ] }}
                Coordinate3 {{ point [ 5 5 5, 6 5 5, 5 6 5 ] }}
                IndexedFaceSet {{ coordIndex [ 0, 1, 2, -1 ] }}
            }}"#
        ));
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 2, "senza coordinate la seconda superficie non si può leggere");
        assert_eq!(s[0].color, Some([0.0, 1.0, 0.0, 0.75]));
        // Il materiale del Separator interno non sopravvive, le coordinate sono le ultime lette
        assert_eq!(s[1].color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(s[1].world_vertices[0], [5.0, 5.0, 5.0]);
        assert_eq!(scene.load_warnings.len(), 1, "{:?}", scene.load_warnings);
    }

    #[test]
    fn lod_loads_first_level_and_lines_become_a_warning() {
        let scene = parse(&format!(
            r#"DEF Pezzo Separator {{
                DEF Solo_Linee Separator {{ LOD {{ range [ 10 ] center 1 2 3
                    Separator {{ Coordinate3 {{ point [ 0 0 0, 1 1 1 ] }} IndexedLineSet {{ coordIndex [ 0, 1, -1 ] }} }}
                    Separator {{ Coordinate3 {{ point [ 0 0 0, 1 1 1 ] }} IndexedLineSet {{ coordIndex [ 0, 1, -1 ] }} }}
                }} }}
                DEF Vuoto Separator {{ LOD {{ range [ ] Separator {{ }} }} }}
                DEF Superficie Separator {{ LOD {{ range [ 10 ] center 1 2 3
                    Separator {{ Separator {{ {TRIANGLE} }} Separator {{ {TRIANGLE} }} }}
                    Separator {{ Coordinate3 {{ point [ 9 9 9, 8 9 9, 9 8 9 ] }} IndexedFaceSet {{ coordIndex [ 0, 1, 2, -1 ] }} }}
                }} }}
            }}"#
        ));
        assert_eq!(scene.shapes_data.len(), 2, "solo il primo livello del LOD");
        assert_eq!(scene.shapes_data[0].world_vertices[0], [0.0, 0.0, 0.0]);
        assert_eq!(scene.load_warnings.len(), 1);
        assert!(scene.load_warnings[0].contains("1 nodi di linee"), "{:?}", scene.load_warnings);
        // Solo i contenitori con un nome e con delle superfici
        let names: Vec<String> = (0..scene.nodes.len() as u32)
            .filter(|&n| !matches!(scene.nodes[n as usize], crate::scene_graph::SceneNode::Shape { .. }))
            .map(|n| scene.display_name(n))
            .collect();
        assert_eq!(names, ["Pezzo", "Superficie"]);
    }

    #[test]
    fn transforms_accumulate_in_order_and_mirror_flips_ccw() {
        let scene = parse(&format!(
            "Separator {{ Translation {{ translation 10 0 0 }} Scale {{ scaleFactor 2 2 2 }} {TRIANGLE} }}
             Separator {{ Rotation {{ rotation 0 0 1 1.5707964 }} {TRIANGLE} }}
             Separator {{ Transform {{ translation 1 0 0 scaleFactor 3 3 3 center 1 0 0 }} {TRIANGLE} }}
             Separator {{ MatrixTransform {{ matrix 1 0 0 0  0 1 0 0  0 0 1 0  4 5 6 1 }} {TRIANGLE} }}
             Separator {{ Scale {{ scaleFactor -1 1 1 }} {TRIANGLE} }}"
        ));
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-5);
        let s = &scene.shapes_data;
        assert_eq!(s[0].world_vertices[1], [12.0, 0.0, 0.0]);
        assert!(close(s[1].world_vertices[1], [0.0, 1.0, 0.0]), "{:?}", s[1].world_vertices);
        // Centro in 1 0 0: il punto 0 0 0 passa per -1 → -3 → -2 → -1
        assert_eq!(s[2].world_vertices[0], [-1.0, 0.0, 0.0]);
        assert_eq!(s[3].world_vertices[0], [4.0, 5.0, 6.0]);
        assert_eq!(s[4].world_vertices[1], [-1.0, 0.0, 0.0]);
        let text = String::from_utf8_lossy(&scene.source_data);
        assert_eq!(text.matches("ccw FALSE").count(), 1, "solo la superficie specchiata");
    }

    #[test]
    fn use_is_visited_again_with_the_current_state() {
        let scene = parse(&format!(
            "DEF Coord Coordinate3 {{ point [ 0 0 0, 1 0 0, 0 1 0 ] }}
             DEF Pezzo Separator {{ Material {{ diffuseColor 0 0 1 }} IndexedFaceSet {{ coordIndex [ 0, 1, 2, -1 ] }} }}
             Coordinate3 {{ point [ 7 7 7, 8 7 7, 7 8 7 ] }}
             Translation {{ translation 100 0 0 }}
             USE Coord
             USE Pezzo
             DEF Pezzo Separator {{ {TRIANGLE} }}
             USE Mancante"
        ));
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 3);
        assert_eq!(s[1].world_vertices[0], [100.0, 0.0, 0.0], "USE delle coordinate e traslazione corrente");
        assert_eq!(s[1].color, Some([0.0, 0.0, 1.0, 1.0]));
        assert!(scene.load_warnings.iter().any(|w| w.contains("1 nodi non letti")), "{:?}", scene.load_warnings);
    }

    #[test]
    fn use_of_itself_does_not_hang() {
        let scene = parse("DEF A Separator { USE A }");
        assert!(scene.shapes_data.is_empty());
        assert!(!scene.load_warnings.is_empty());
    }

    #[test]
    fn shared_coordinates_keep_only_the_vertices_of_each_surface() {
        let scene = parse(
            "Coordinate3 { point [ 0 0 0, 1 0 0, 0 1 0, 500 0 0, 501 0 0, 500 1 0 ] }
             Normal { vector [ 0 0 1, 0 0 1, 0 0 1, 0 0 1, 0 0 1, 0 0 1 ] }
             IndexedFaceSet { coordIndex [ 0, 1, 2, -1 ] normalIndex [ 0, 1, 2, -1 ] }
             IndexedFaceSet { coordIndex [ 5, 3, 4, -1 ] }",
        );
        let s = &scene.shapes_data;
        assert_eq!(s[0].world_vertices.len(), 3);
        assert_eq!(s[1].world_vertices, [[500.0, 1.0, 0.0], [500.0, 0.0, 0.0], [501.0, 0.0, 0.0]]);
        assert_eq!(s[1].indices, [0, 1, 2]);
    }

    #[test]
    fn switch_loads_only_the_active_child() {
        let at = |x: i32| format!("Separator {{ Coordinate3 {{ point [ {x} 0 0, {x} 1 0, {x} 0 1 ] }} IndexedFaceSet {{ coordIndex [ 0, 1, 2, -1 ] }} }}");
        let (a, b) = (at(1), at(2));
        let xs = |body: String| -> Vec<f32> {
            parse(&body).shapes_data.iter().map(|s| s.world_vertices[0][0]).collect()
        };
        assert_eq!(xs(format!("Switch {{ whichChild 1 {a} {b} }}")), [2.0]);
        assert_eq!(xs(format!("Switch {{ whichChild -3 {a} {b} }}")), [1.0, 2.0]);
        let off = parse(&format!("Switch {{ {a} {b} }}"));
        assert!(off.shapes_data.is_empty());
        assert!(off.load_warnings[0].contains("Switch"));
    }

    /// Normali originali conservate secondo il legame; `OVERALL` si ricalcola dalla geometria.
    #[test]
    fn normals_follow_the_binding() {
        // Due triangoli: le normali sono 1 0 0, 0 1 0, 0 0 -1, -1 0 0, 0 -1 0, 0 0 1
        let with = |binding: &str, normal_index: &str| {
            parse(&format!(
                "NormalBinding {{ value {binding} }}
                 Coordinate3 {{ point [ 0 0 0, 1 0 0, 0 1 0, 9 9 9, 1 1 0 ] }}
                 Normal {{ vector [ 1 0 0, 0 1 0, 0 0 -1, -1 0 0, 0 -1 0, 0 0 1 ] }}
                 IndexedFaceSet {{ coordIndex [ 0, 1, 2, -1, 1, 4, 2, -1 ] {normal_index} }}"
            ))
            .shapes_data[0]
                .corner_normals()
        };
        let [x, y, nz, nx, ny, z] =
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(with("PER_VERTEX_INDEXED", "normalIndex [ 2, 0, 1, -1, 3, 4, 5, -1 ]"), [nz, x, y, nx, ny, z]);
        // Senza `normalIndex` le normali seguono i vertici, anche se la superficie ne usa solo
        // una parte (il vertice 3 non è usato)
        assert_eq!(with("DEFAULT", ""), [x, y, nz, y, ny, nz]);
        assert_eq!(with("PER_VERTEX", ""), [x, y, nz, nx, ny, z]);
        assert_eq!(with("PER_FACE", ""), [x, x, x, y, y, y]);
        assert_eq!(with("PER_FACE_INDEXED", "normalIndex [ 5, 3 ]"), [z, z, z, nx, nx, nx]);
        assert_eq!(with("OVERALL", ""), [z; 6], "ricalcolate");
    }

    /// Il testo dei numeri è copiato; l'export è un VRML 2.0 che si riapre uguale.
    #[test]
    fn export_is_valid_vrml2_and_reopens_identical() {
        let scene = parse(
            "DEF Pezzo Separator {
                Material { diffuseColor 0.2 0.8 0.8 }
                ShapeHints { vertexOrdering COUNTERCLOCKWISE }
                Normal { vector [ 0 0 1, 0 0 1, 0 0 1 ] }
                Coordinate3 { point [ 1060.1234567 0 0, 1 0 0, 0 1 0 ] }
                IndexedFaceSet { coordIndex [ 0, 1, 2, -1 ] normalIndex [ 0, 1, 2, -1 ] }
            }",
        );
        assert!(String::from_utf8_lossy(&scene.source_data).contains("1060.1234567"));

        let path = std::env::temp_dir().join("test_vrml1_export.wrl");
        crate::exporter::export_visible_wrl(&scene, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert!(text.starts_with("#VRML V2.0"));
        assert!(!text.contains("Separator") && !text.contains("Coordinate3") && !text.contains("V1.0"), "{text}");
        assert!(text.contains("1060.1234567"), "{text}");

        let again = parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data[0].world_vertices, scene.shapes_data[0].world_vertices);
        assert_eq!(again.shapes_data[0].normals, scene.shapes_data[0].normals);
        assert_eq!(again.shapes_data[0].color, scene.shapes_data[0].color);
        assert_eq!(again.display_name(again.roots[0]), "Pezzo");
    }
}
