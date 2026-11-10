//! Lettura dei file HSF (HOOPS Stream Format, versioni 20 e 21).
//!
//! Riferimento: la specifica pubblica degli opcode (docs.techsoft3d.com, sezione «HSF File
//! Format»), che permette di leggere e scrivere HSF senza il toolkit. Alcuni dettagli non sono
//! scritti nella specifica e sono stati ricavati dai file reali:
//! - le geometrie e la matrice (`S`, `%`, `X`, polilinee) hanno un byte in più subito dopo
//!   l'opcode: 0 = coordinate `float`, 1 = `double`;
//! - `B` (ingombro informativo) ha due byte prima delle coordinate;
//! - `m` (metadati del segmento): numero di voci, poi chiave di 4 caratteri (`name`), lunghezza e
//!   testo terminato da zero;
//! - nelle facce compresse la lunghezza comprende il byte dei bit per valore; i valori a 8, 16 e 32
//!   bit sono interi little-endian;
//! - il conteggio delle regioni delle facce ha la stessa larghezza dei valori;
//! - `:` apre gli attributi di una geometria (qui: un riferimento `r` con la sua matrice), chiusi
//!   da un byte 0; `r` ha un byte in più dopo l'indice.
//!
//! Struttura dei file letti: un albero di segmenti `(` … `)` con nomi, colori, matrici e
//! visibilità. Le Shell compaiono prima vuote (`TKSH2_NULL`) e marcate con `q`; più avanti nel
//! file un'altra Shell con l'indice della marcatura le riempie, oppure `&` copia un'altra Shell con
//! una matrice. I prototipi stanno spesso in un segmento con le facce nascoste e vengono copiati
//! nei pezzi visibili: le Shell con le facce nascoste non si caricano, e nemmeno quelle nei
//! segmenti speciali con il nome che inizia per `?` (`?Include Library`), che HOOPS disegna solo
//! dove sono richiamate.
//!
//! Non supportati (il file si apre fino a quel punto, con un avviso): compressione «edgebreaker»,
//! coordinate compresse con larghezze diverse da 8/16/32 bit, opcode non elencati sotto.

use std::collections::HashMap;

use glam::{Mat4, Vec3};

use crate::parser::{ContainerKind, PathSeg, ShapeInfo};

/// Le scene HSF di questi file sono in metri (i pezzi in mm hanno una scala 0,001): il Gestore
/// lavora in millimetri come per JT e WRL.
const METERS_TO_MM: f32 = 1000.0;

pub fn is_hsf(data: &[u8]) -> bool {
    data.starts_with(b";; HSF V")
}

/// Pezzi della scena e avvisi.
pub(crate) fn shapes(data: &[u8]) -> (Vec<ShapeInfo>, Vec<String>) {
    let mut reader = Reader::new(data);
    let mut warnings = Vec::new();
    if let Err(e) = reader.run() {
        warnings.push(format!("HSF letto solo in parte: {e}"));
    }
    let (shapes, mut w) = reader.finish();
    warnings.append(&mut w);
    (shapes, warnings)
}

// ── Flusso con la compressione zlib ─────────────────────────────────────────

/// Byte del file con i blocchi compressi espansi man mano: dopo l'opcode `Z` il resto del flusso
/// zlib si decomprime in coda a `out`, poi si torna a leggere il file.
struct Stream<'a> {
    raw: &'a [u8],
    raw_pos: usize,
    out: Vec<u8>,
    pos: usize,
}

type Res<T> = Result<T, String>;

impl<'a> Stream<'a> {
    fn ensure(&mut self, n: usize) -> Res<()> {
        let need = (self.pos + n).saturating_sub(self.out.len());
        if need > 0 {
            if self.raw_pos + need > self.raw.len() {
                return Err("fine del file inattesa".into());
            }
            self.out.extend_from_slice(&self.raw[self.raw_pos..self.raw_pos + need]);
            self.raw_pos += need;
        }
        Ok(())
    }
    fn at_end(&self) -> bool {
        self.pos >= self.out.len() && self.raw_pos >= self.raw.len()
    }
    fn bytes(&mut self, n: usize) -> Res<&[u8]> {
        self.ensure(n)?;
        let s = &self.out[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Res<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Res<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Res<i32> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> Res<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f64(&mut self) -> Res<f64> {
        let b = self.bytes(8)?;
        Ok(f64::from_le_bytes(b.try_into().unwrap()))
    }
    /// Un numero `float` o `double` secondo il byte di precisione.
    fn real(&mut self, double: bool) -> Res<f32> {
        if double {
            Ok(self.f64()? as f32)
        } else {
            self.f32()
        }
    }
    fn len_i32(&mut self, what: &str) -> Res<usize> {
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| format!("{what}: lunghezza negativa ({n})"))
    }
    fn text(&mut self, n: usize) -> Res<String> {
        let b = self.bytes(n)?;
        Ok(String::from_utf8_lossy(b).trim_end_matches('\0').to_string())
    }
    /// Opcode `Z`: espande il flusso zlib che segue nel file.
    fn start_compression(&mut self) -> Res<()> {
        if self.pos != self.out.len() {
            // `Z` dentro un blocco già espanso: non previsto dal formato
            return Err("compressione annidata".into());
        }
        let mut dec = flate2::Decompress::new(true);
        let raw = self.raw;
        let input = &raw[self.raw_pos..];
        // I file dei programmi di misura hanno migliaia di blocchi piccoli: la memoria cresce
        // raddoppiando invece di riservare ogni volta il resto del file
        let mut buf = Vec::with_capacity(1 << 16);
        loop {
            let before_in = dec.total_in() as usize;
            if buf.len() == buf.capacity() {
                buf.reserve(buf.capacity().max(1 << 16));
            }
            let status = dec
                .decompress_vec(&input[before_in..], &mut buf, flate2::FlushDecompress::None)
                .map_err(|e| format!("blocco compresso rovinato: {e}"))?;
            match status {
                flate2::Status::StreamEnd => break,
                _ if dec.total_in() as usize >= input.len() && buf.len() < buf.capacity() => {
                    return Err("blocco compresso troncato".into())
                }
                _ => {}
            }
        }
        self.raw_pos += dec.total_in() as usize;
        self.out.extend_from_slice(&buf);
        Ok(())
    }
}

// ── Scena ───────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Segment {
    parent: Option<usize>,
    /// Nome del segmento nel file (spesso tecnico: «representation item 01234»)
    raw_name: String,
    /// Nome dai metadati (`m name=…`), quello mostrato dai programmi HOOPS
    meta_name: Option<String>,
    matrix: Option<Mat4>,
    diffuse: Option<[f32; 3]>,
    /// Trasmissione media (0 = opaco)
    transmission: Option<f32>,
    /// Facce visibili (`V` con il bit delle facce)
    faces_visible: Option<bool>,
}

/// Geometria di una Shell nelle coordinate del suo segmento.
#[derive(Clone, Default)]
struct Mesh {
    points: Vec<[f32; 3]>,
    triangles: Vec<u32>,
}

enum Content {
    /// Shell vuota, in attesa di essere riempita
    Pending,
    Mesh(Mesh),
    /// Copia di un'altra Shell marcata, trasformata
    Copy { tag: usize, matrix: Mat4 },
}

struct Slot {
    segment: usize,
    content: Content,
}

#[derive(Clone, Copy)]
enum Object {
    Segment(usize),
    Slot(usize),
}

struct Reader<'a> {
    s: Stream<'a>,
    segments: Vec<Segment>,
    /// Segmenti aperti (il primo è la radice implicita)
    stack: Vec<usize>,
    slots: Vec<Slot>,
    tags: Vec<Object>,
    last: Option<Object>,
    /// Dentro `:` (attributi di una geometria): il riferimento letto e la sua matrice
    geometry_attrs: Option<(Option<usize>, Mat4)>,
    warnings: Vec<String>,
}

fn mat_from_rows(m: &[f32; 12]) -> Mat4 {
    // HOOPS usa vettori riga: p' = p·M con le righe (r0, r1, r2, traslazione)
    Mat4::from_cols(
        glam::Vec4::new(m[0], m[1], m[2], 0.0),
        glam::Vec4::new(m[3], m[4], m[5], 0.0),
        glam::Vec4::new(m[6], m[7], m[8], 0.0),
        glam::Vec4::new(m[9], m[10], m[11], 1.0),
    )
}

/// Campioni di coordinate quantizzate (`quantize_and_pack_floats`): flusso di bit letto dal bit
/// più alto di parole da 32 bit little-endian. Verificato sul cubo di prova: con l'ordine dei byte
/// semplice le strisce non formano facce piane.
fn unpack_samples(data: &[u8], bits: u8, n: usize) -> Res<Vec<u32>> {
    if !(1..=32).contains(&bits) {
        return Err(format!("campioni a {bits} bit non validi"));
    }
    let bits = bits as usize;
    if data.len() * 8 < n * bits {
        return Err("dati compressi troppo corti".into());
    }
    let word = |i: usize| -> u64 {
        let mut b = [0u8; 4];
        for (k, x) in b.iter_mut().enumerate() {
            *x = data.get(4 * i + k).copied().unwrap_or(0);
        }
        u32::from_le_bytes(b) as u64
    };
    let mut out = Vec::with_capacity(n);
    for j in 0..n {
        let start = j * bits;
        let (w, off) = (start / 32, start % 32);
        // Due parole consecutive come un numero da 64 bit, dal bit più alto
        let pair = (word(w) << 32) | word(w + 1);
        let v = (pair >> (64 - off - bits)) & ((1u64 << bits) - 1);
        out.push(v as u32);
    }
    Ok(out)
}

/// Valori senza segno a `bits` bit delle liste di facce: interi little-endian (8/16/32 bit).
fn unpack(data: &[u8], bits: u8, n: usize) -> Res<Vec<u32>> {
    let size = match bits {
        8 => 1,
        16 => 2,
        32 => 4,
        _ => return Err(format!("valori a {bits} bit non supportati")),
    };
    if data.len() < n * size {
        return Err("dati compressi troppo corti".into());
    }
    Ok((0..n)
        .map(|i| {
            let b = &data[i * size..];
            match size {
                1 => b[0] as u32,
                2 => u16::from_le_bytes([b[0], b[1]]) as u32,
                _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            }
        })
        .collect())
}

/// Triangoli da strisce e ventagli (`TKSH_TRISTRIPS`): conteggio negativo = ventaglio.
fn triangles_from_strips(list: &[u32], bits: u8, point_count: usize) -> Res<Vec<u32>> {
    let mut out = Vec::with_capacity(list.len() * 2);
    let sign = 1i64 << (bits - 1);
    let mut k = 0;
    while k < list.len() {
        let mut c = list[k] as i64;
        if bits < 32 && c >= sign {
            c -= sign << 1;
        } else if bits == 32 {
            c = list[k] as i32 as i64;
        }
        k += 1;
        let fan = c < 0;
        let n = c.unsigned_abs() as usize;
        let Some(v) = list.get(k..k + n) else {
            return Err("lista delle strisce troncata".into());
        };
        if v.iter().any(|&i| i as usize >= point_count) {
            return Err("indice di vertice fuori dalla Shell".into());
        }
        for i in 2..n {
            let t = if fan {
                [v[0], v[i - 1], v[i]]
            } else if (i - 2) % 2 == 0 {
                [v[i - 2], v[i - 1], v[i]]
            } else {
                [v[i - 1], v[i - 2], v[i]]
            };
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                out.extend_from_slice(&t);
            }
        }
        k += n;
    }
    Ok(out)
}

/// Triangoli da una lista di poligoni (conteggio, indici…); i fori (conteggio negativo) si saltano.
fn triangles_from_polygons(list: &[i64], point_count: usize) -> Res<Vec<u32>> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < list.len() {
        let c = list[k];
        k += 1;
        let n = c.unsigned_abs() as usize;
        let Some(v) = list.get(k..k + n) else {
            return Err("lista delle facce troncata".into());
        };
        if v.iter().any(|&i| i < 0 || i as usize >= point_count) {
            return Err("indice di vertice fuori dalla Shell".into());
        }
        if c > 0 {
            for i in 2..n {
                out.extend_from_slice(&[v[0] as u32, v[i - 1] as u32, v[i] as u32]);
            }
        }
        k += n;
    }
    Ok(out)
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        let root = Segment::default();
        Self {
            s: Stream { raw: data, raw_pos: 0, out: Vec::new(), pos: 0 },
            segments: vec![root],
            stack: vec![0],
            slots: Vec::new(),
            tags: Vec::new(),
            last: None,
            geometry_attrs: None,
            warnings: Vec::new(),
        }
    }

    fn current(&self) -> usize {
        *self.stack.last().unwrap()
    }

    fn run(&mut self) -> Res<()> {
        // Intestazione: commento «;; HSF V21.00 » fino a fine riga
        loop {
            let b = self.s.u8()?;
            if b == b'\n' {
                break;
            }
        }
        while !self.s.at_end() {
            let at = self.s.pos;
            let op = self.s.u8()?;
            self.opcode(op).map_err(|e| format!("{e} (opcode 0x{op:02x} a {at})"))?;
            if op == b'D' || (op == 0 && self.stack.len() == 1 && self.geometry_attrs.is_none()) {
                // Dizionario o terminazione: il resto sono indici per lo streaming
                break;
            }
        }
        Ok(())
    }

    fn opcode(&mut self, op: u8) -> Res<()> {
        let s = &mut self.s;
        match op {
            0x00 => {
                // Fine degli attributi di una geometria, oppure fine del file
                if let Some((reference, matrix)) = self.geometry_attrs.take() {
                    if let Some(tag) = reference {
                        let segment = self.current();
                        self.slots.push(Slot { segment, content: Content::Copy { tag, matrix } });
                    }
                }
            }
            0x01 => {}       // pausa
            b'Z' => s.start_compression()?,
            b'z' => {}
            b';' => {
                let n = s.u16()? as usize;
                s.bytes(n)?;
            }
            b'?' => {
                // Condizioni del segmento (es. "cad_default"): un testo, niente geometria
                let n = s.u8()? as usize;
                s.bytes(n)?;
            }
            b'I' => {
                s.u32()?;
            }
            b'B' | b'b' => {
                s.u8()?;
                let kind = s.u8()?;
                s.bytes(if kind == 0 { 24 } else { 16 })?;
            }
            b'(' => {
                let n = s.u8()? as usize;
                let name = s.text(n)?;
                let parent = self.current();
                self.segments.push(Segment { parent: Some(parent), raw_name: name, ..Default::default() });
                let id = self.segments.len() - 1;
                self.stack.push(id);
                self.last = Some(Object::Segment(id));
            }
            b')' => {
                if self.stack.len() > 1 {
                    let id = self.stack.pop().unwrap();
                    self.last = Some(Object::Segment(id));
                }
            }
            b's' => {
                let tag = s.len_i32("riapertura")?;
                match self.tags.get(tag) {
                    Some(Object::Segment(id)) => {
                        let id = *id;
                        self.stack.push(id);
                    }
                    _ => return Err(format!("riapertura di un segmento sconosciuto ({tag})")),
                }
            }
            b'q' => {
                let obj = self.last.ok_or("marcatura senza oggetto")?;
                self.tags.push(obj);
            }
            b'U' => {
                let mut n = s.u16()? as usize;
                if n == 0xFFFF {
                    n = s.len_i32("opzioni")?;
                }
                s.bytes(n)?;
            }
            b'm' => {
                let count = s.len_i32("metadati")?;
                for _ in 0..count {
                    let key = self.s.bytes(4)?.to_vec();
                    let n = self.s.len_i32("metadati")?;
                    let value = self.s.text(n)?;
                    if key == b"name" && !value.trim().is_empty() {
                        let id = self.current();
                        self.segments[id].meta_name = Some(value);
                    }
                }
            }
            b'%' => {
                let double = s.u8()? != 0;
                let mut m = [0f32; 12];
                for v in &mut m {
                    *v = s.real(double)?;
                }
                let matrix = mat_from_rows(&m);
                if let Some(attrs) = &mut self.geometry_attrs {
                    attrs.1 = matrix;
                } else {
                    let id = self.current();
                    self.segments[id].matrix = Some(matrix);
                }
            }
            b'V' => {
                let mask = s.u8()?;
                let value = s.u8()?;
                if mask & 0x80 != 0 {
                    let high = s.u16()?;
                    s.u16()?;
                    if high & 0x8000 != 0 {
                        s.u8()?;
                        s.u8()?;
                    }
                }
                if mask & 0x01 != 0 && self.geometry_attrs.is_none() {
                    let id = self.current();
                    self.segments[id].faces_visible = Some(value & 0x01 != 0);
                }
            }
            b'J' => {
                let n = s.u8()? as usize;
                s.bytes(n)?;
                let n = s.u16()? as usize;
                s.bytes(n)?;
            }
            b'-' => {
                let pattern = s.u8()?;
                let n = if pattern == 0xFF { s.u8()? as usize } else { 1 };
                s.bytes(n)?;
            }
            b'"' => self.color()?,
            b'~' => {
                let geometry = self.geometry_bits()?;
                let rgb = self.s.bytes(3)?;
                let rgb = [rgb[0], rgb[1], rgb[2]].map(|c| c as f32 / 255.0);
                if geometry & 1 != 0 && self.geometry_attrs.is_none() {
                    let id = self.current();
                    self.segments[id].diffuse = Some(rgb);
                }
            }
            b'R' => self.rendering_options()?,
            b'H' => self.heuristics()?,
            b'S' => self.shell()?,
            b'X' => {
                let double = s.u8()? != 0;
                s.bytes(if double { 24 } else { 12 })?;
            }
            0x10 => self.polypolyline()?,
            b'L' => {
                // Polilinea: un byte di opzioni (0 = float, 1 = double), numero di punti, poi
                // x y z. Si salta: la scena mostra solo superfici
                let flags = s.u8()?;
                if flags > 1 {
                    return Err(format!("polilinea con opzioni {flags:02x} non supportata"));
                }
                let n = s.len_i32("polilinea")?;
                s.bytes(n * if flags == 1 { 24 } else { 12 })?;
            }
            b'&' => {
                let from = s.len_i32("copia")?;
                s.i32()?;
                let to = s.i32()?;
                s.i32()?;
                s.u8()?;
                let mut m = [0f32; 12];
                for v in &mut m {
                    *v = s.f32()?;
                }
                let content = Content::Copy { tag: from, matrix: mat_from_rows(&m) };
                if to < 0 {
                    let segment = self.current();
                    self.slots.push(Slot { segment, content });
                } else {
                    self.fill(to as usize, content)?;
                }
            }
            b':' => {
                if self.geometry_attrs.is_some() {
                    return Err("attributi di geometria annidati".into());
                }
                self.geometry_attrs = Some((None, Mat4::IDENTITY));
            }
            b'r' => {
                let tag = s.len_i32("riferimento")?;
                s.u8()?;
                match (&mut self.geometry_attrs, self.tags.get(tag)) {
                    (Some(attrs), Some(Object::Slot(_))) => attrs.0 = Some(tag),
                    (None, Some(Object::Slot(_))) => {
                        let segment = self.current();
                        self.slots.push(Slot { segment, content: Content::Copy { tag, matrix: Mat4::IDENTITY } });
                    }
                    _ => self.warnings.push(format!("Riferimento a un segmento ({tag}) non supportato: ignorato")),
                }
            }
            b'D' => {}
            _ => {
                let c = if op.is_ascii_graphic() { format!(" «{}»", op as char) } else { String::new() };
                return Err(format!("opcode{c} non supportato"));
            }
        }
        Ok(())
    }

    fn fill(&mut self, tag: usize, content: Content) -> Res<()> {
        match self.tags.get(tag) {
            Some(Object::Slot(slot)) => {
                self.slots[*slot].content = content;
                Ok(())
            }
            _ => Err(format!("geometria per una marcatura sconosciuta ({tag})")),
        }
    }

    /// Bit delle geometrie interessate: 1 byte, poi altri se è acceso il bit alto.
    fn geometry_bits(&mut self) -> Res<u32> {
        let mut g = self.s.u8()? as u32;
        if g & 0x80 != 0 {
            g |= (self.s.u8()? as u32) << 8;
            if g & 0x8000 != 0 {
                g |= (self.s.u16()? as u32) << 16;
            }
        }
        Ok(g)
    }

    fn color_component(&mut self) -> Res<Option<[f32; 3]>> {
        let n = self.s.u8()? as usize;
        if n == 0 {
            let b = self.s.bytes(3)?;
            Ok(Some([b[0], b[1], b[2]].map(|c| c as f32 / 255.0)))
        } else {
            // Colore per nome («light blue»): non interpretato
            self.s.bytes(n)?;
            Ok(None)
        }
    }

    fn color(&mut self) -> Res<()> {
        let geometry = self.geometry_bits()?;
        let mut channels = self.s.u8()? as u32;
        if channels & 0x80 != 0 {
            channels |= (self.s.u8()? as u32) << 8;
        }
        let mut diffuse = None;
        let mut transmission = None;
        for bit in [0x01, 0x02, 0x04, 0x08, 0x10] {
            if channels & bit != 0 {
                let c = self.color_component()?;
                match bit {
                    0x01 => diffuse = c,
                    0x08 => transmission = c,
                    _ => {}
                }
            }
        }
        if channels & 0x20 != 0 {
            self.s.f32()?; // lucentezza
        }
        if channels & 0x40 != 0 {
            self.s.f32()?; // indice
        }
        if channels & 0x100 != 0 {
            self.color_component()?;
        }
        if channels & 0x200 != 0 {
            self.color_component()?;
        }
        if geometry & 1 != 0 && self.geometry_attrs.is_none() {
            let id = self.current();
            let seg = &mut self.segments[id];
            if diffuse.is_some() {
                seg.diffuse = diffuse;
            }
            if let Some(t) = transmission {
                seg.transmission = Some((t[0] + t[1] + t[2]) / 3.0);
            }
        }
        Ok(())
    }

    /// Opzioni di resa: si leggono solo per saltarle.
    fn rendering_options(&mut self) -> Res<()> {
        let s = &mut self.s;
        let mask = s.u32()?;
        let extended = if mask & 0x8000_0000 != 0 { s.u32()? } else { 0 };
        s.u32()?;
        if mask & 0x8000_0000 != 0 {
            s.u32()?;
        }
        if mask & 0x3000 != 0 {
            s.u8()?; // algoritmo delle superfici nascoste
        }
        if mask & 0xC_0000 != 0 {
            s.u8()?; // tecnologia e quantizzazione
        }
        if mask & 0x0020_0000 != 0 {
            s.i32()?; // spostamento delle facce
        }
        if mask & 0x0010_0000 != 0 {
            let lock_mask = s.u32()?;
            s.u32()?;
            if lock_mask & 0x04 != 0 {
                s.u32()?;
                s.u32()?;
            }
            if lock_mask & 0x0800_0000 != 0 {
                s.u32()?;
                s.u32()?;
            }
        }
        let unsupported = mask & (0x0001_0000 | 0x0002_0000 | 0x0040_0000 | 0x0080_0000 | 0x0100_0000 | 0x0400_0000 | 0x0800_0000 | 0x1000_0000 | 0x4000_0000);
        if unsupported != 0 || extended != 0 {
            return Err(format!("opzioni di resa non supportate ({mask:08x} {extended:08x})"));
        }
        Ok(())
    }

    /// Euristiche di disegno: si leggono solo per saltarle.
    fn heuristics(&mut self) -> Res<()> {
        let s = &mut self.s;
        let mut mask = s.u16()? as u32;
        if mask & 0x8000 != 0 {
            mask |= (s.u16()? as u32) << 16;
        }
        s.u16()?;
        if mask & 0x8000 != 0 {
            s.u16()?;
        }
        let with_data = 0x04 | 0x08 | 0x40 | 0x80 | 0x1_0000 | 0x10_0000 | 0x20_0000 | 0x40_0000 | 0x80_0000 | 0x100_0000;
        if mask & with_data != 0 {
            return Err(format!("euristiche non supportate ({mask:08x})"));
        }
        Ok(())
    }

    fn polypolyline(&mut self) -> Res<()> {
        let s = &mut self.s;
        let double = s.u8()? != 0;
        let sub = s.u16()?;
        let count = s.len_i32("polilinea")?;
        let primitives = if sub & 0x02 != 0 {
            let n = s.len_i32("polilinea")?;
            s.bytes(n * 4)?;
            n
        } else if sub & 0x04 == 0 {
            let mut total = 0usize;
            let mut n = 0usize;
            while total < count {
                total += s.len_i32("polilinea")?;
                n += 1;
            }
            n
        } else {
            1
        };
        if sub & 0xFF00 != 0 {
            // Coordinate «2D»: per X, Y e Z (2 bit ciascuna, dal bit 8) 1 = sempre zero,
            // 2 = un valore per tutti, 3 = un valore per polilinea; i punti hanno solo le altre.
            // Visto nei file reali: XY per tutti i punti, poi una Z per polilinea
            if sub & 0x01 != 0 {
                return Err("polilinee 2D compresse non supportate".into());
            }
            let mut dims = 0;
            let mut extra = 0;
            for k in 0..3 {
                match (sub >> (8 + 2 * k)) & 0x03 {
                    0 => dims += 1,
                    1 => {}
                    2 => extra += 1,
                    _ => extra += primitives,
                }
            }
            s.bytes((count * dims + extra) * if double { 8 } else { 4 })?;
            return Ok(());
        }
        if sub & 0x01 != 0 {
            s.u8()?;
            if sub & 0x08 == 0 {
                s.bytes(24)?;
            }
            // Bit per campione come intero da 4 byte (non un byte come nelle Shell): visto nei
            // file reali, 16 seguito dalla lunghezza 350·3·2
            s.u32()?;
            let n = s.len_i32("polilinea")?;
            s.bytes(n)?;
        } else {
            s.bytes(count * 3 * if double { 8 } else { 4 })?;
        }
        Ok(())
    }

    fn shell(&mut self) -> Res<()> {
        let s = &mut self.s;
        let double = s.u8()? != 0;
        let sub = s.u8()?;
        let sub2 = if sub & 0x80 != 0 { s.u16()? } else { 0 };
        let null = sub2 & 0x02 != 0;
        let bounding_only = sub & 0x20 != 0;
        let refinement = if sub & 0x10 == 0 && !null && !bounding_only { Some(s.len_i32("Shell")?) } else { None };
        s.u8()?; // livello di dettaglio
        if null {
            let segment = self.current();
            self.slots.push(Slot { segment, content: Content::Pending });
            self.last = Some(Object::Slot(self.slots.len() - 1));
            return Ok(());
        }
        if bounding_only {
            s.bytes(24)?;
            return Ok(());
        }
        if sub2 & 0x01 != 0 {
            return Err("Shell come insieme di geometrie non supportata".into());
        }
        if sub & 0x40 != 0 {
            return Err("Shell con compressione edgebreaker non supportata".into());
        }
        // Punti
        let points: Vec<[f32; 3]> = if sub & 0x01 != 0 {
            s.u8()?; // schema (CS_TRIVIAL)
            let count = s.len_i32("punti")?;
            let mut bbox = [0f32; 6];
            for v in &mut bbox {
                *v = s.f32()?;
            }
            let bits = s.u8()?;
            let n = s.len_i32("punti")?;
            let data = s.bytes(n)?;
            let q = unpack_samples(data, bits, count * 3)?;
            let max = if bits == 32 { u32::MAX as f64 } else { ((1u64 << bits) - 1) as f64 };
            (0..count)
                .map(|j| {
                    std::array::from_fn(|k| {
                        let t = q[3 * j + k] as f64 / max;
                        (bbox[k] as f64 + (bbox[k + 3] - bbox[k]) as f64 * t) as f32
                    })
                })
                .collect()
        } else {
            let count = s.len_i32("punti")?;
            let mut pts = Vec::with_capacity(count);
            for _ in 0..count {
                pts.push([s.real(double)?, s.real(double)?, s.real(double)?]);
            }
            pts
        };
        // Facce
        let tristrips = sub & 0x04 != 0;
        let (triangles, face_count) = if sub & 0x02 != 0 {
            s.u8()?; // schema (CS_TRIVIAL)
            let len = s.len_i32("facce")?;
            let bits = s.u8()?;
            let nbytes = len.saturating_sub(1);
            let data = s.bytes(nbytes)?;
            let values = unpack(data, bits, nbytes * 8 / bits.max(1) as usize)?;
            if tristrips {
                let t = triangles_from_strips(&values, bits, points.len())?;
                let n = t.len() / 3;
                (t, n)
            } else {
                let signed: Vec<i64> = values.iter().map(|&v| sign_extend(v, bits)).collect();
                let t = triangles_from_polygons(&signed, points.len())?;
                let n = count_faces(&signed);
                (t, n)
            }
        } else {
            let len = s.len_i32("facce")?;
            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                values.push(s.i32()? as i64);
            }
            if tristrips {
                let v: Vec<u32> = values.iter().map(|&v| v as i32 as u32).collect();
                let t = triangles_from_strips(&v, 32, points.len())?;
                let n = t.len() / 3;
                (t, n)
            } else {
                let n = count_faces(&values);
                (triangles_from_polygons(&values, points.len())?, n)
            }
        };
        if sub & 0x08 != 0 {
            self.optionals(points.len(), face_count)?;
        }
        let content = Content::Mesh(Mesh { points, triangles });
        match refinement {
            Some(tag) => self.fill(tag, content)?,
            None => {
                let segment = self.current();
                self.slots.push(Slot { segment, content });
                self.last = Some(Object::Slot(self.slots.len() - 1));
            }
        }
        Ok(())
    }

    /// Attributi di vertici e facce: normali e regioni si saltano (le normali si ricalcolano).
    fn optionals(&mut self, vertices: usize, faces: usize) -> Res<()> {
        let s = &mut self.s;
        loop {
            let op = s.u8()?;
            match op {
                0x00 => return Ok(()),
                0x01 => {
                    // Normali compresse su tutti i vertici: schema, bit, lunghezza, dati
                    s.u8()?;
                    s.u8()?;
                    let n = s.len_i32("normali")?;
                    s.bytes(n)?;
                }
                // Normali non compresse su tutti i vertici: x y z per vertice
                0x11 => {
                    s.bytes(vertices * 12)?;
                }
                // Normali polari su tutti i vertici: due angoli per vertice (nei file esportati
                // dai programmi di misura, dopo le facce a strisce)
                0x13 => {
                    s.bytes(vertices * 8)?;
                }
                0x33 => {
                    let scheme = s.u8()?;
                    let size = match scheme & 0xF8 {
                        0 => 4,
                        8 => 1,
                        16 => 2,
                        _ => return Err(format!("regioni delle facce: formato {scheme:02x}")),
                    };
                    let read = |s: &mut Stream| -> Res<usize> {
                        let b = s.bytes(size)?;
                        Ok(b.iter().rev().fold(0usize, |a, &x| (a << 8) | x as usize))
                    };
                    match scheme & 0x07 {
                        0 => {
                            s.bytes(faces * size)?;
                        }
                        1 => {
                            let n = read(s)?;
                            s.bytes(n * size)?;
                        }
                        2 => {
                            let n = read(s)?;
                            s.bytes((n + 1) * size)?;
                        }
                        3 => {
                            let n = read(s)?;
                            s.bytes(2 * n * size)?;
                        }
                        k => return Err(format!("regioni delle facce: schema {k}")),
                    }
                }
                _ => return Err(format!("attributo di Shell 0x{op:02x} non supportato")),
            }
        }
    }

    // ── Risultato ───────────────────────────────────────────────────────────

    fn world_matrix(&self, mut seg: usize) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let units = Mat4::from_scale(Vec3::splat(METERS_TO_MM));
        loop {
            let s = &self.segments[seg];
            if let Some(local) = s.matrix {
                m = local * m;
            }
            match s.parent {
                Some(p) => seg = p,
                None => return units * m,
            }
        }
    }

    fn inherited<T: Copy>(&self, mut seg: usize, f: impl Fn(&Segment) -> Option<T>) -> Option<T> {
        loop {
            let s = &self.segments[seg];
            if let Some(v) = f(s) {
                return Some(v);
            }
            seg = s.parent?;
        }
    }

    /// Geometria di una marcatura nelle coordinate del suo segmento, seguendo le copie.
    fn resolve(&self, tag: usize, depth: usize) -> Option<Mesh> {
        let Some(Object::Slot(slot)) = self.tags.get(tag) else { return None };
        self.slot_mesh(*slot, depth)
    }

    fn slot_mesh(&self, slot: usize, depth: usize) -> Option<Mesh> {
        match &self.slots[slot].content {
            Content::Pending => None,
            Content::Mesh(m) => Some(m.clone()),
            Content::Copy { tag, matrix } => {
                if depth > 16 {
                    return None;
                }
                let mut m = self.resolve(*tag, depth + 1)?;
                for p in &mut m.points {
                    *p = matrix.transform_point3(Vec3::from(*p)).to_array();
                }
                Some(m)
            }
        }
    }

    fn display_name(&self, seg: usize) -> Option<String> {
        let s = &self.segments[seg];
        if let Some(n) = &s.meta_name {
            return Some(n.clone());
        }
        // Il nome del primo segmento (il file) è tra apici inversi
        if s.parent == Some(0) {
            let n = s.raw_name.trim_matches('`').trim();
            if !n.is_empty() {
                return Some(n.to_string());
            }
        }
        None
    }

    fn finish(self) -> (Vec<ShapeInfo>, Vec<String>) {
        let mut warnings = self.warnings.clone();
        let mut hidden = 0usize;
        let mut empty = 0usize;
        let mut prepared = Vec::new();
        for (i, slot) in self.slots.iter().enumerate() {
            // `?Include Library` e simili: segmenti speciali di HOOPS, mai disegnati direttamente,
            // contengono i prototipi usati con `r`
            if self.inherited(slot.segment, |s| s.faces_visible) == Some(false)
                || self.inherited(slot.segment, |s| s.raw_name.starts_with('?').then_some(())).is_some()
            {
                hidden += 1;
                continue;
            }
            let Some(mesh) = self.slot_mesh(i, 0) else {
                empty += 1;
                continue;
            };
            if mesh.triangles.is_empty() {
                empty += 1;
                continue;
            }
            if std::env::var("GESTORE_HSF_DEBUG").is_ok() {
                eprintln!("slot {i}: {} punti, {} triangoli, {:?}", mesh.points.len(), mesh.triangles.len() / 3, &mesh.triangles[..mesh.triangles.len().min(12)]);
            }
            if std::env::var("GESTORE_HSF_DEBUG").is_ok() {
                let w = self.world_matrix(slot.segment);
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for p in &mesh.points {
                    let q = w.transform_point3(Vec3::from(*p));
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
                let kind = match &slot.content { Content::Copy { tag, .. } => format!("copia di {tag}"), _ => "propria".into() };
                eprintln!("slot {i} seg {} {:?} {kind}: {lo:?} {hi:?}", slot.segment, self.display_name(slot.segment));
            }
            prepared.push((slot.segment, mesh));
        }
        if empty > 0 {
            warnings.push(format!("{empty} Shell senza geometria ignorate"));
        }
        tracing::info!("HSF: {} pezzi, {hidden} prototipi nascosti", prepared.len());

        // Percorso dei segmenti con nome (gli altri si attraversano senza creare nodi)
        let mut path_cache: HashMap<usize, Vec<PathSeg>> = HashMap::new();
        let mut path_of = |seg: usize| -> Vec<PathSeg> {
            if let Some(p) = path_cache.get(&seg) {
                return p.clone();
            }
            let mut chain = Vec::new();
            let mut cur = Some(seg);
            while let Some(c) = cur {
                if c != 0 {
                    if let Some(name) = self.display_name(c) {
                        chain.push(PathSeg { key: c, name: Some(name), kind: ContainerKind::Group, frame: Mat4::IDENTITY });
                    }
                }
                cur = self.segments[c].parent;
            }
            chain.reverse();
            path_cache.insert(seg, chain.clone());
            chain
        };

        use rayon::prelude::*;
        let jobs: Vec<(Mesh, Mat4, Option<[f32; 4]>, Vec<PathSeg>)> = prepared
            .into_iter()
            .map(|(seg, mesh)| {
                let diffuse = self.inherited(seg, |s| s.diffuse);
                let transmission = self.inherited(seg, |s| s.transmission).unwrap_or(0.0);
                let color = diffuse.map(|d| [d[0], d[1], d[2], (1.0 - transmission).clamp(0.0, 1.0)]);
                (mesh, self.world_matrix(seg), color, path_of(seg))
            })
            .collect();
        let shapes = jobs
            .into_par_iter()
            .filter_map(|(mesh, world, color, path)| {
                let points: Vec<[f32; 3]> =
                    mesh.points.iter().map(|p| world.transform_point3(Vec3::from(*p)).to_array()).collect();
                let welded = crate::reduce::weld(&points, &mesh.triangles);
                if welded.indices.is_empty() {
                    return None;
                }
                let (normals, split) =
                    crate::reduce::crease_normals(&welded.positions, &welded.indices, crate::reduce::CREASE_ANGLE);
                Some(ShapeInfo {
                    source_start: 0,
                    source_end: 0,
                    point_range_start: 0,
                    point_range_end: 0,
                    matrix: Mat4::IDENTITY,
                    face_count: (welded.indices.len() / 3) as u32,
                    world_vertices: welded.positions,
                    normals,
                    split,
                    indices: welded.indices,
                    name: None,
                    color,
                    path,
                    rewritten: true,
                })
            })
            .collect();
        (shapes, warnings)
    }
}

fn sign_extend(v: u32, bits: u8) -> i64 {
    if bits >= 32 {
        return v as i32 as i64;
    }
    let sign = 1i64 << (bits - 1);
    let v = v as i64;
    if v >= sign {
        v - (sign << 1)
    } else {
        v
    }
}

/// Facce di una lista di poligoni: i fori (conteggio negativo) fanno parte della faccia precedente.
fn count_faces(list: &[i64]) -> usize {
    let mut k = 0;
    let mut n = 0;
    while k < list.len() {
        let c = list[k];
        if c > 0 {
            n += 1;
        }
        k += 1 + c.unsigned_abs() as usize;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_read_from_the_high_bit_of_each_word() {
        // Parola 0x12345678 in little-endian: a 16 bit dà 0x1234 poi 0x5678, a 8 bit 0x12 0x34…
        let data = 0x1234_5678u32.to_le_bytes();
        assert_eq!(unpack_samples(&data, 16, 2).unwrap(), vec![0x1234, 0x5678]);
        assert_eq!(unpack_samples(&data, 8, 4).unwrap(), vec![0x12, 0x34, 0x56, 0x78]);
        // A cavallo di due parole: 12 bit
        let mut d = 0xABCD_EF01u32.to_le_bytes().to_vec();
        d.extend_from_slice(&0x2345_6789u32.to_le_bytes());
        assert_eq!(unpack_samples(&d, 12, 5).unwrap(), vec![0xABC, 0xDEF, 0x012, 0x345, 0x678]);
    }

    #[test]
    fn strips_and_fans_become_triangles() {
        // Striscia 0 1 2 3 → (0,1,2) (2,1,3); ventaglio di 4 → (4,5,6) (4,6,7)
        let list = [4, 0, 1, 2, 3, (-4i32) as u16 as u32, 4, 5, 6, 7];
        let t = triangles_from_strips(&list, 16, 8).unwrap();
        assert_eq!(t, vec![0, 1, 2, 2, 1, 3, 4, 5, 6, 4, 6, 7]);
    }

    #[test]
    fn degenerate_strip_triangles_are_dropped() {
        let t = triangles_from_strips(&[3, 5, 5, 6], 8, 7).unwrap();
        assert!(t.is_empty());
    }

    #[test]
    fn row_matrix_translates_last() {
        let m = mat_from_rows(&[0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 10.0, 0.0, 0.0]);
        // L'asse x va su y, poi la traslazione
        let p = m.transform_point3(Vec3::new(1.0, 0.0, 0.0));
        assert!((p - Vec3::new(10.0, 1.0, 0.0)).length() < 1e-6);
    }

    /// File costruito a mano come quelli reali: segmento con nome e colore, Shell vuota marcata,
    /// riempita in un blocco compresso dopo una pausa.
    fn sample() -> Vec<u8> {
        sample_with(&[], &[])
    }

    /// Come `sample`, con altre geometrie nel pezzo e attributi dopo le facce della Shell.
    fn sample_with(extra: &[u8], optionals: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut tree = Vec::new();
        tree.push(b'(');
        tree.push(4);
        tree.extend_from_slice(b"pz 1");
        tree.push(b'm');
        tree.extend_from_slice(&1i32.to_le_bytes());
        tree.extend_from_slice(b"name");
        tree.extend_from_slice(&6i32.to_le_bytes());
        tree.extend_from_slice(b"Lato\0\0");
        // Colore delle facce: diffuse rosso
        tree.extend_from_slice(&[b'"', 0x01, 0x01, 0, 255, 0, 0]);
        // Shell vuota + marcatura
        tree.extend_from_slice(&[b'S', 0, 0x90, 0x02, 0x00, 0, b'q']);
        tree.extend_from_slice(extra);
        tree.push(b')');
        // Riempimento: punti non compressi, strisce compresse a 8 bit
        let mut geo = vec![b'S', 0, if optionals.is_empty() { 0x06 } else { 0x0e }];
        geo.extend_from_slice(&0i32.to_le_bytes());
        geo.push(0);
        geo.extend_from_slice(&4i32.to_le_bytes());
        for p in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]] {
            for c in p {
                geo.extend_from_slice(&c.to_le_bytes());
            }
        }
        geo.push(1);
        geo.extend_from_slice(&6i32.to_le_bytes());
        geo.push(8);
        geo.extend_from_slice(&[4, 0, 1, 2, 3]);
        geo.extend_from_slice(optionals);
        geo.extend_from_slice(&[b'z']);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&geo).unwrap();
        let geo = z.finish().unwrap();

        let mut f = b";; HSF V21.00 \n".to_vec();
        f.extend_from_slice(&tree);
        f.push(0x01);
        f.push(b'Z');
        f.extend_from_slice(&geo);
        f.push(0x00);
        f
    }

    #[test]
    fn reads_tree_color_and_refined_shell() {
        let data = sample();
        assert!(is_hsf(&data));
        let (shapes, warnings) = shapes(&data);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(shapes.len(), 1);
        let s = &shapes[0];
        assert_eq!(s.indices.len(), 6);
        assert_eq!(s.color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(s.path.len(), 1);
        assert_eq!(s.path[0].name.as_deref(), Some("Lato"));
        // Metri → millimetri
        let max_x = s.world_vertices.iter().map(|p| p[0]).fold(0.0, f32::max);
        assert_eq!(max_x, 1000.0);
    }

    #[test]
    fn hidden_prototypes_are_not_loaded_but_their_copies_are() {
        // Segmento nascosto con la Shell (marcatura 0), poi una copia spostata in un segmento visibile
        let mut f = b";; HSF V21.00 \n".to_vec();
        f.extend_from_slice(&[b'(', 3]);
        f.extend_from_slice(b"lib");
        f.extend_from_slice(&[b'V', 0x01, 0x00]);
        f.extend_from_slice(&[b'S', 0, 0x10 | 0x04, 0]);
        f.extend_from_slice(&3i32.to_le_bytes());
        for p in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in p {
                f.extend_from_slice(&c.to_le_bytes());
            }
        }
        f.extend_from_slice(&4i32.to_le_bytes());
        f.extend_from_slice(&[3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
        f.extend_from_slice(&[b'q', b')', b'(', 2]);
        f.extend_from_slice(b"pz");
        f.extend_from_slice(&[b'S', 0, 0x90, 0x02, 0x00, 0, b'q', b')']);
        f.push(b'&');
        for v in [0i32, 0, 1, 0] {
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.push(1);
        for v in [1f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.002] {
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.push(0);
        let (shapes, warnings) = shapes(&f);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(shapes.len(), 1);
        assert!(shapes[0].world_vertices.iter().all(|p| (p[2] - 2.0).abs() < 1e-4));
    }

    #[test]
    fn unknown_opcode_keeps_what_was_read() {
        let mut f = sample();
        f.pop();
        f.push(b'!');
        let (shapes, warnings) = shapes(&f);
        assert_eq!(shapes.len(), 1);
        assert_eq!(warnings.len(), 1);
    }

    /// Come nei file dei programmi di misura: normali polari dopo le facce, polilinee semplici e
    /// polilinee «2D» con una Z per polilinea.
    #[test]
    fn polar_normals_and_polylines_are_skipped() {
        let mut optionals = vec![0x13];
        optionals.extend_from_slice(&[0u8; 4 * 8]);
        optionals.push(0);
        let mut extra = vec![b'L', 0];
        extra.extend_from_slice(&2i32.to_le_bytes());
        extra.extend_from_slice(&[0u8; 2 * 12]);
        // Due polilinee da 2 e 3 punti: XY per punto, poi una Z per polilinea
        extra.extend_from_slice(&[0x10, 0]);
        extra.extend_from_slice(&0x3000u16.to_le_bytes());
        for n in [5i32, 2, 3] {
            extra.extend_from_slice(&n.to_le_bytes());
        }
        extra.extend_from_slice(&[0u8; (5 * 2 + 2) * 4]);
        let (shapes, warnings) = shapes(&sample_with(&extra, &optionals));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].indices.len(), 6);
    }

    /// Le condizioni di un segmento (opcode `?`) si saltano: prima il file si leggeva solo fino lì.
    #[test]
    fn segment_conditions_are_skipped() {
        let mut extra = vec![b'?', 11];
        extra.extend_from_slice(b"cad_default");
        let (shapes, warnings) = shapes(&sample_with(&extra, &[]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(shapes.len(), 1);
    }

    /// Polilinea con i punti in doppia precisione (opzioni 01): si salta e la lettura prosegue.
    #[test]
    fn double_precision_polylines_are_skipped() {
        let mut extra = vec![b'L', 0x01];
        extra.extend_from_slice(&2i32.to_le_bytes());
        for v in [4.5f64, -0.46, 1.95, 4.53, -0.46, 1.96] {
            extra.extend_from_slice(&v.to_le_bytes());
        }
        let (shapes, warnings) = shapes(&sample_with(&extra, &[]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(shapes.len(), 1);
    }

    /// File veri dell'utente (non nel repository): `GESTORE_HSF_DIR` = cartella con gli .hsf.
    #[test]
    #[ignore]
    fn file_reali() {
        let dir = std::env::var("GESTORE_HSF_DIR").expect("GESTORE_HSF_DIR");
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) != Some("hsf") {
                continue;
            }
            let data = std::fs::read(&p).unwrap();
            let t = std::time::Instant::now();
            let (shapes, warnings) = shapes(&data);
            let tris: usize = shapes.iter().map(|s| s.indices.len() / 3).sum();
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for s in &shapes {
                for v in &s.world_vertices {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
            }
            println!(
                "{}: {} byte: {} pezzi, {tris} triangoli, {:?}, ingombro {lo:?} {hi:?}, avvisi {warnings:?}",
                p.file_name().unwrap().to_string_lossy(),
                data.len(),
                shapes.len(),
                t.elapsed()
            );
        }
    }
}
