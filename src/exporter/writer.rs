use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

use crate::scene_graph::{NodeKind, SceneGraph, SceneNode, NO_PARENT};

/// Statistiche dell'esportazione.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExportStats {
    pub shapes_written: usize,
    pub groups_written: usize,
}

/// Come scrivere il file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    /// Scrittura compatta dai dati letti: niente rientri, triangoli, numeri con la precisione
    /// che serve (un milionesimo dell'ingombro della scena). Senza, i campi del file originale
    /// vengono copiati così come sono (poligoni e cifre comprese).
    pub compact: bool,
    /// Normali scritte nel file (spigoli vivi del CAD). Senza, solo `creaseAngle`: le ricalcola
    /// il visualizzatore e il file è molto più piccolo.
    pub normals: bool,
    /// File in metri: la scena è in millimetri, coordinate e `Transform` si dividono per 1000
    /// (come i WRL della stazione e come li legge lo script dello scostamento).
    pub meters: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { compact: true, normals: true, meters: false }
    }
}

impl ExportOptions {
    /// Copia fedele dei campi originali (il comportamento delle prime versioni).
    #[cfg(test)]
    pub const VERBATIM: Self = Self { compact: false, normals: true, meters: false };

    /// Dalle unità della scena (mm) a quelle del file.
    fn unit(&self) -> glam::Mat4 {
        glam::Mat4::from_scale(glam::Vec3::splat(if self.meters { MM_TO_METERS } else { 1.0 }))
    }
}

/// Cosa esportare (sempre e solo i pezzi visibili).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// Tutta la scena.
    #[default]
    Visible,
    /// Un solo sotto-albero (es. un file dell'insieme), scritto come radice.
    Node(u32),
    /// I sotto-alberi dei nodi selezionati, con i gruppi che li contengono (l'albero resta
    /// quello della scena, senza gli altri pezzi).
    Selected(Vec<u32>),
}

impl Scope {
    /// Nodi da cui partire e se scrivere i nodi dei file (solo con più file nell'export).
    pub(super) fn roots(&self, scene: &SceneGraph, has_visible: &[bool]) -> (Vec<u32>, bool) {
        match self {
            Scope::Node(n) => (vec![*n], false),
            Scope::Visible => (scene.roots.clone(), scene.open_files().count() > 1),
            Scope::Selected(_) => {
                let files = scene.open_files().filter(|s| has_visible.get(s.node as usize) == Some(&true)).count();
                (scene.roots.clone(), files > 1)
            }
        }
    }

    /// Per ogni nodo: true se il suo sotto-albero contiene almeno una Shape visibile da esportare.
    pub(super) fn has_visible(&self, scene: &SceneGraph) -> Vec<bool> {
        let Scope::Selected(nodes) = self else { return compute_has_visible(scene) };
        let mut inside = vec![false; scene.nodes.len()];
        for &n in nodes {
            if (n as usize) < inside.len() && !inside[n as usize] {
                scene.for_each_subtree_node(n, |i| inside[i as usize] = true);
            }
        }
        visible_where(scene, |i| inside[i])
    }

    /// Nome dell'export per i formati che ne scrivono uno (il primo file, o il nodo esportato).
    pub(super) fn root_name(&self, scene: &SceneGraph) -> String {
        match self {
            Scope::Node(n) => scene.display_name(*n),
            _ => scene.sources.first().map(|s| s.name.clone()).unwrap_or_default(),
        }
    }
}

/// Millimetri → metri.
pub const MM_TO_METERS: f32 = 0.001;

/// Cifre decimali massime delle coordinate in metri: il micron. Un f32 di qualche metro non ne ha
/// di più, e le cifre in più sarebbero solo residui (2.9000001).
pub const METERS_DECIMALS: u32 = 6;

/// Esporta i soli elementi visibili in un nuovo file WRL "pulito", copiando i campi originali.
/// Vedi [`export_with`].
#[cfg(test)]
pub fn export_visible_wrl(scene: &SceneGraph, output_path: &std::path::Path) -> io::Result<ExportStats> {
    export_with(scene, &Scope::Visible, output_path, ExportOptions::VERBATIM)
}

/// Esporta le parti visibili di un solo sotto-albero (es. un file dell'insieme), copiando i
/// campi originali.
#[cfg(test)]
pub fn export_subtree_wrl(scene: &SceneGraph, node: u32, output_path: &std::path::Path) -> io::Result<ExportStats> {
    export_with(scene, &Scope::Node(node), output_path, ExportOptions::VERBATIM)
}

/// Esporta gli elementi visibili di `scope` (tutta la scena, un sotto-albero o la selezione).
///
/// * La gerarchia dei macro-oggetti viene mantenuta come `DEF Nome Group { children [...] }`
///   (i contenitori senza parti visibili vengono omessi): riaprendo il file
///   nell'applicazione si ritrova lo stesso albero.
/// * I sistemi di riferimento degli oggetti (i `Transform` del file e gli spostamenti fatti in
///   Dima 3D) restano come `Transform` dentro il `Group` dell'oggetto, e le coordinate dei pezzi
///   sono scritte in quei sistemi: lo scostamento tra due oggetti si ricava dai `Transform`, come
///   nei file del visualizzatore usato in azienda (vedi [`Frames`]).
/// * L'`appearance` originale viene copiata verbatim quando è autonoma (niente
///   DEF/USE al suo interno); altrimenti si usa un materiale neutro.
/// * Senza `compact` la geometria conserva i campi del file originale (vedi [`super::ifs`]);
///   i pezzi ricalcolati dal Gestore (es. ridotti) si scrivono comunque dai dati.
///
/// Con più file aperti ognuno diventa un `Group` col nome del file; con un solo file il suo
/// nodo non viene scritto, così riaprendo l'export l'albero è quello di partenza.
pub fn export_with(
    scene: &SceneGraph,
    scope: &Scope,
    output_path: &std::path::Path,
    options: ExportOptions,
) -> io::Result<ExportStats> {
    let file = std::fs::File::create(output_path)?;
    let mut w = BufWriter::with_capacity(1 << 20, file);
    let stats = write_scene(scene, scope, &mut w, options)?;
    w.flush()?;
    tracing::info!(
        "Esportato {}: {} shape, {} gruppi",
        output_path.display(),
        stats.shapes_written,
        stats.groups_written
    );
    Ok(stats)
}

/// Dimensione in byte del file che [`export_with`] scriverebbe, senza scriverlo.
pub fn estimate_size(scene: &SceneGraph, scope: &Scope, options: ExportOptions) -> u64 {
    let mut counter = ByteCounter(0);
    let _ = write_scene(scene, scope, &mut counter, options);
    counter.0
}

/// Conta i byte invece di scriverli.
struct ByteCounter(u64);

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_scene(scene: &SceneGraph, scope: &Scope, w: &mut impl Write, options: ExportOptions) -> io::Result<ExportStats> {
    let has_visible = scope.has_visible(scene);
    let (roots, write_file_nodes) = scope.roots(scene, &has_visible);
    let mut visible = 0;
    for &r in &roots {
        scene.for_each_subtree_node(r, |i| {
            if has_visible[i as usize] && matches!(scene.nodes[i as usize], SceneNode::Shape { .. }) {
                visible += 1;
            }
        });
    }

    writeln!(w, "#VRML V2.0 utf8")?;
    writeln!(w, "# Esportato da Dima 3D")?;
    writeln!(w, "# Shape visibili: {visible}")?;
    writeln!(w)?;

    let mut ctx = ExportCtx {
        scene,
        frames: Frames::new(scene),
        has_visible: &has_visible,
        write_file_nodes,
        options,
        decimals: coordinate_decimals(scene),
        buffer: Vec::new(),
        names: HashMap::new(),
        geometry_defs: (0..scene.sources.len().max(1)).map(|_| Default::default()).collect(),
        stats: ExportStats::default(),
    };
    for &root in &roots {
        ctx.write_node(w, root, 0, glam::Mat4::IDENTITY)?;
    }
    Ok(ctx.stats)
}

/// Cifre decimali delle coordinate nella scrittura compatta: passo di circa un milionesimo
/// dell'ingombro della scena (1 µm per una scocca in metri, 0,001 mm se in millimetri).
pub(super) fn coordinate_decimals(scene: &SceneGraph) -> u32 {
    let bbox = scene.compute_bounding_box();
    let extent = if bbox.is_valid() { (bbox.max - bbox.min).length() as f64 } else { 0.0 };
    if !(extent.is_finite() && extent > 0.0) {
        return 6;
    }
    (-(extent * 1e-6).log10()).ceil().clamp(0.0, 9.0) as u32
}

/// Cifre decimali delle normali (versori): errore massimo di circa 0,05°.
const NORMAL_DECIMALS: u32 = 3;

/// Numero con al più `decimals` cifre decimali, senza zeri finali (`1.5`, `-0.003`, `12`).
pub(crate) fn push_fixed(out: &mut Vec<u8>, value: f32, decimals: u32) {
    let scale = 10u64.pow(decimals);
    let scaled = (value as f64 * scale as f64).round();
    if !scaled.is_finite() || scaled.abs() >= 9.0e15 {
        // Fuori scala (non succede con coordinate reali): rappresentazione normale
        out.extend_from_slice(format!("{value}").as_bytes());
        return;
    }
    let scaled = scaled as i64;
    if scaled == 0 {
        out.push(b'0');
        return;
    }
    if scaled < 0 {
        out.push(b'-');
    }
    let abs = scaled.unsigned_abs();
    let (int, mut frac) = (abs / scale, abs % scale);
    push_uint(out, int);
    if frac != 0 {
        let mut digits = decimals as usize;
        while frac % 10 == 0 {
            frac /= 10;
            digits -= 1;
        }
        out.push(b'.');
        let mut tmp = Vec::with_capacity(20);
        push_uint(&mut tmp, frac);
        for _ in tmp.len()..digits {
            out.push(b'0');
        }
        out.extend_from_slice(&tmp);
    }
}

pub(crate) fn push_uint(out: &mut Vec<u8>, mut v: u64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[i..]);
}

/// Per ogni nodo: true se il suo sotto-albero contiene almeno una Shape visibile.
pub(super) fn compute_has_visible(scene: &SceneGraph) -> Vec<bool> {
    visible_where(scene, |_| true)
}

/// Come [`compute_has_visible`], contando solo le Shape per cui `keep` è vero.
fn visible_where(scene: &SceneGraph, keep: impl Fn(usize) -> bool) -> Vec<bool> {
    let mut flags: Vec<bool> = scene
        .nodes
        .iter()
        .zip(&scene.info)
        .enumerate()
        .map(|(k, (n, i))| !i.deleted && matches!(n, SceneNode::Shape { visible: true, .. }) && keep(k))
        .collect();
    // Al contrario dell'ordine dell'albero i figli vengono prima del padre
    for &i in scene.tree_order().iter().rev() {
        let p = scene.info[i as usize].parent;
        if flags[i as usize] && p != NO_PARENT {
            flags[p as usize] = true;
        }
    }
    flags
}

/// Sistemi di riferimento da scrivere come `Transform`.
///
/// Il sistema di un contenitore è quello letto dal file (`SceneGraph::frames`, con la scala del
/// file portata nelle coordinate) composto con lo spostamento fatto in Dima 3D, se tutti i suoi
/// pezzi sono stati spostati allo stesso modo. Le coordinate restano esatte qualunque sistema si
/// scelga: sono scritte relative al `Transform` effettivamente scritto sopra di loro.
struct Frames {
    /// Spostamento comune ai pezzi di ogni nodo (`None`: pezzi spostati in modo diverso, o nessuno)
    common: Vec<Option<glam::Mat4>>,
}

impl Frames {
    fn new(scene: &SceneGraph) -> Frames {
        let mut common: Vec<Option<Option<glam::Mat4>>> = vec![None; scene.nodes.len()];
        // Al contrario dell'ordine dell'albero i figli sono già pronti quando si arriva al padre
        for i in scene.tree_order().into_iter().rev().map(|i| i as usize) {
            common[i] = match &scene.nodes[i] {
                SceneNode::Shape { mesh_id, .. } => Some(Some(scene.placement(*mesh_id))),
                SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                    let mut it = children.iter().map(|&c| common[c as usize]).filter(|c| c.is_some());
                    match it.next() {
                        None => None,
                        Some(first) => Some(if it.all(|c| c == first) { first.flatten() } else { None }),
                    }
                }
            };
        }
        Frames { common: common.into_iter().map(Option::flatten).collect() }
    }

    /// Sistema dell'oggetto `n` in coordinate della scena.
    fn of(&self, scene: &SceneGraph, n: u32) -> glam::Mat4 {
        let file = scene.frames.get(n as usize).copied().unwrap_or(glam::Mat4::IDENTITY);
        let scale = scene.source_of_node(n).map_or(1.0, |i| scene.sources[i].scale);
        let units = glam::Mat4::from_scale(glam::Vec3::splat(scale));
        let placement = self.common.get(n as usize).copied().flatten().unwrap_or(glam::Mat4::IDENTITY);
        placement * units * file * units.inverse()
    }
}

/// `m` come scala uniforme positiva, rotazione e traslazione (i soli che un `Transform` scrive
/// senza `scaleOrientation`); `None` se non si può (specchiatura, scala diversa sugli assi).
fn similarity(m: glam::Mat4) -> Option<(f32, glam::Quat, glam::Vec3)> {
    let (scale, rotation, translation) = m.to_scale_rotation_translation();
    let s = scale.x;
    let uniform = s > 0.0 && (scale - glam::Vec3::splat(s)).abs().max_element() <= s * 1e-5;
    let rebuilt = glam::Mat4::from_scale_rotation_translation(glam::Vec3::splat(s), rotation, translation);
    let size = m.to_cols_array().iter().fold(1.0f32, |a, v| a.max(v.abs()));
    (uniform && rebuilt.abs_diff_eq(m, size * 1e-5) && rotation.is_finite()).then_some((s, rotation, translation))
}

/// La Shape con coordinate e normali portate nel file da `to_file` (unità del file e sistema
/// del `Transform` scritto sopra di lei).
fn localize(shape: &crate::parser::ShapeInfo, to_file: glam::Mat4) -> crate::parser::ShapeInfo {
    let normal = glam::Mat3::from_mat4(to_file).inverse().transpose();
    let mut local = shape.clone();
    for v in &mut local.world_vertices {
        *v = to_file.transform_point3(glam::Vec3::from(*v)).to_array();
    }
    for n in &mut local.normals {
        *n = (normal * glam::Vec3::from(*n)).normalize_or_zero().to_array();
    }
    local.matrix = to_file * shape.matrix;
    // Il sistema scritto è quello del file: le coordinate originali si possono copiare tali e quali
    let size = local.matrix.to_cols_array().iter().fold(1.0f32, |a, v| a.max(v.abs()));
    if local.matrix.abs_diff_eq(glam::Mat4::IDENTITY, size * 1e-6) {
        local.matrix = glam::Mat4::IDENTITY;
    }
    local
}

struct ExportCtx<'a> {
    scene: &'a SceneGraph,
    frames: Frames,
    has_visible: &'a [bool],
    /// Scrivere i nodi dei file come Group (con più file); altrimenti solo il loro contenuto
    write_file_nodes: bool,
    options: ExportOptions,
    /// Cifre decimali delle coordinate nella scrittura compatta
    decimals: u32,
    /// Testo della geometria in preparazione (riusato tra le Shape)
    buffer: Vec<u8>,
    /// Nomi DEF già usati → contatore, per garantirne l'unicità
    names: HashMap<String, u32>,
    /// Nodi di geometria con DEF in ogni file sorgente, per risolvere i USE
    geometry_defs: Vec<super::ifs::GeometryDefs<'a>>,
    stats: ExportStats,
}

impl ExportCtx<'_> {
    /// `frame`: sistema di riferimento scritto sopra il nodo (i `Transform` degli antenati).
    fn write_node(&mut self, w: &mut impl Write, idx: u32, depth: usize, frame: glam::Mat4) -> io::Result<()> {
        if !self.has_visible.get(idx as usize).copied().unwrap_or(false) {
            return Ok(());
        }
        // Copia del riferimento con lifetime 'a: i borrow sui nodi non bloccano `&mut self`
        let scene = self.scene;
        let ind = if self.options.compact { String::new() } else { "  ".repeat(depth) };
        let sub = if self.options.compact { "" } else { "  " };
        let is_file = scene.info.get(idx as usize).is_some_and(|i| i.kind == NodeKind::File);
        if is_file && !self.write_file_nodes {
            for &c in scene.nodes[idx as usize].children() {
                self.write_node(w, c, depth, frame)?;
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
                let local;
                let to_file = frame.inverse() * self.options.unit();
                let (shape, decimals) = if to_file == glam::Mat4::IDENTITY {
                    (shape, self.decimals)
                } else {
                    local = localize(shape, to_file);
                    // Coordinate in unità più grandi (es. metri): servono più cifre decimali
                    let scale = glam::Mat3::from_mat4(to_file).determinant().abs().cbrt();
                    let extra = -scale.log10().round() as i32;
                    let most = if self.options.meters { METERS_DECIMALS as i32 } else { 9 };
                    (&local, (self.decimals as i32 + extra).clamp(0, most) as u32)
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
                        w.write_all(sub.as_bytes())?;
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
                                "{ind}{sub}appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} transparency {transparency} }} }}"
                            )?;
                        } else {
                            writeln!(
                                w,
                                "{ind}{sub}appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} }} }}"
                            )?;
                        }
                    }
                }

                writeln!(w, "{ind}{sub}geometry IndexedFaceSet {{")?;
                // Campi originali del file (poligoni, normali...); se non riutilizzabili,
                // i triangoli calcolati dal parser
                let original = !self.options.compact
                    && !shape.rewritten
                    && src.is_some()
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
                    let pad = if self.options.compact { String::new() } else { format!("{ind}    ") };
                    self.buffer.clear();
                    write_geometry(&mut self.buffer, &pad, shape, self.options.normals, decimals);
                    w.write_all(&self.buffer)?;
                }
                writeln!(w, "{ind}{sub}}}")?;
                writeln!(w, "{ind}}}")?;
                self.stats.shapes_written += 1;
            }
            SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                writeln!(w, "{ind}DEF {name} Group {{")?;
                writeln!(w, "{ind}{sub}children [")?;
                // Sistema dell'oggetto diverso da quello scritto sopra: un Transform nel Group.
                // Mai sul Group di un file: lo spostamento deve stare nel Group dell'oggetto, da
                // cui lo script dello scostamento comincia a comporre i Transform.
                let unit = self.options.unit();
                let relative = frame.inverse() * unit * self.frames.of(scene, idx) * unit.inverse();
                let size = relative.to_cols_array().iter().fold(1.0f32, |a, v| a.max(v.abs()));
                let transform = (!is_file && !relative.abs_diff_eq(glam::Mat4::IDENTITY, size * 1e-6))
                    .then(|| similarity(relative))
                    .flatten();
                let inner = match transform {
                    Some((scale, rotation, translation)) => {
                        let (axis, angle) = rotation.to_axis_angle();
                        let pad = format!("{ind}{sub}{sub}");
                        writeln!(w, "{pad}Transform {{")?;
                        // Al micron: senza i residui dei calcoli tra matrici (1.6650001)
                        let decimals = if self.options.meters { 6 } else { 3 };
                        let mut line = Vec::new();
                        for v in translation.to_array() {
                            line.push(b' ');
                            push_fixed(&mut line, v, decimals);
                        }
                        write!(w, "{pad}{sub}translation")?;
                        w.write_all(&line)?;
                        writeln!(w)?;
                        if angle.abs() > 0.0 {
                            writeln!(w, "{pad}{sub}rotation {} {} {} {angle}", axis.x, axis.y, axis.z)?;
                        }
                        if (scale - 1.0).abs() > 1e-7 {
                            writeln!(w, "{pad}{sub}scale {scale} {scale} {scale}")?;
                        }
                        writeln!(w, "{pad}{sub}children [")?;
                        frame * glam::Mat4::from_scale_rotation_translation(glam::Vec3::splat(scale), rotation, translation)
                    }
                    None => frame,
                };
                let child_depth = if transform.is_some() { depth + 4 } else { depth + 2 };
                for &c in children {
                    self.write_node(w, c, child_depth, inner)?;
                }
                if transform.is_some() {
                    let pad = format!("{ind}{sub}{sub}");
                    writeln!(w, "{pad}{sub}]")?;
                    writeln!(w, "{pad}}}")?;
                }
                writeln!(w, "{ind}{sub}]")?;
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

/// Campi di un `IndexedFaceSet` scritti dai dati della Shape (triangoli in world-space).
fn write_geometry(out: &mut Vec<u8>, pad: &str, shape: &crate::parser::ShapeInfo, normals: bool, decimals: u32) {
    let line = |out: &mut Vec<u8>, text: &str| {
        out.extend_from_slice(pad.as_bytes());
        out.extend_from_slice(text.as_bytes());
        out.push(b'\n');
    };
    let triangle = |out: &mut Vec<u8>, t: &[u32]| {
        push_uint(out, t[0] as u64);
        out.push(b' ');
        push_uint(out, t[1] as u64);
        out.push(b' ');
        push_uint(out, t[2] as u64);
        out.extend_from_slice(b" -1\n");
    };
    let vector = |out: &mut Vec<u8>, v: &[f32; 3], decimals: u32| {
        push_fixed(out, v[0], decimals);
        out.push(b' ');
        push_fixed(out, v[1], decimals);
        out.push(b' ');
        push_fixed(out, v[2], decimals);
        out.push(b'\n');
    };
    let with_normals = normals && !shape.normals.is_empty() && shape.normals.len() == shape.render_vertex_count();
    line(out, "solid FALSE");
    if !with_normals {
        line(out, "creaseAngle 0.5");
    }
    // Matrice speculare: i triangoli in world-space girano al contrario
    if glam::Mat3::from_mat4(shape.matrix).determinant() < 0.0 {
        line(out, "ccw FALSE");
    }
    line(out, "coord Coordinate { point [");
    for p in &shape.world_vertices {
        vector(out, p, decimals);
    }
    line(out, "] }");
    line(out, "coordIndex [");
    for t in shape.indices.chunks_exact(3) {
        triangle(out, t);
    }
    line(out, "]");
    if with_normals {
        line(out, "normal Normal { vector [");
        for n in &shape.normals {
            vector(out, n, NORMAL_DECIMALS);
        }
        line(out, "] }");
        line(out, "normalIndex [");
        for t in shape.render_indices().chunks_exact(3) {
            triangle(out, t);
        }
        line(out, "]");
    }
}

/// Identificatore VRML: solo lettere/cifre ASCII, `_` e `-` (il punto non è ammesso dalla specifica
/// e MeshLab rifiuta il file), non inizia con cifra/segno, non coincide con parole riservate.
pub fn sanitize_identifier(label: &str) -> String {
    let mut out: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
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
        assert_eq!(sanitize_identifier("Part-1.v2"), "Part-1_v2");
        assert_eq!(sanitize_identifier("Telaio.asm"), "Telaio_asm");
        assert_eq!(sanitize_identifier("Città"), "Citt_");
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
        // Il Transform dell'oggetto resta, e le coordinate sono quelle del file
        assert!(text.contains("translation 10 0 0"), "{text}");
        assert!(text.contains("0 0 0, 1 0 0, 0 1 0"), "{text}");

        // Il file esportato si riapre con la stessa gerarchia e gli stessi punti
        let again = crate::parser::parse_wrl(&out);
        assert_eq!(again.all_shape_indices().len(), 1);
        assert_eq!(again.shapes_data[0].world_vertices[0], [10.0, 0.0, 0.0]);
        assert_eq!(again.display_name(again.roots[0]), "Assieme");
        let _ = std::fs::remove_file(path);
    }

    /// Lo stesso pezzo aperto due volte, la seconda copia spostata in Dima 3D: l'export scrive lo
    /// spostamento come Transform nel Group dell'oggetto, quindi riaprendo il file lo scostamento
    /// si ricava dai Transform (come fa lo script), e la geometria è la stessa.
    #[test]
    fn export_keeps_object_frames_as_transforms() {
        use crate::scene_graph::node::multi_file_tests::{file, FILE_B};
        use crate::scene_graph::placement::Pose;
        let mut scene = file(FILE_B, "a.wrl");
        scene.append(file(FILE_B, "b.wrl"));
        let find = |s: &SceneGraph, nth: usize| {
            (0..s.nodes.len() as u32).filter(|&n| !s.info[n as usize].deleted && s.display_name(n).starts_with("Sportello")).nth(nth).unwrap()
        };
        let (a, b) = (find(&scene, 0), find(&scene, 1));
        let moved = glam::Mat4::from_translation(glam::Vec3::new(215.0, 0.0, 1665.0)) * glam::Mat4::from_rotation_y(0.209_439);
        let meshes = scene.meshes_of(&[b]);
        scene.transform_meshes(&meshes, moved);
        let offset = |s: &SceneGraph, a: u32, b: u32| Pose::from_matrix(s.object_frame(a)).offset_to(&Pose::from_matrix(s.object_frame(b))).matrix();
        let before = offset(&scene, a, b);

        for options in [ExportOptions::default(), ExportOptions::VERBATIM] {
            let path = std::env::temp_dir().join("dima3d_export_transform.wrl");
            export_with(&scene, &Scope::Visible, &path, options).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            let again = crate::parser::parse_wrl(text.as_bytes());
            let _ = std::fs::remove_file(&path);
            // Il Transform sta dentro il Group dell'oggetto spostato
            let group = text.find("DEF Sportello_2 Group").expect("oggetto spostato");
            let transform = text[group..].find("Transform").unwrap();
            assert!(transform < text[group..].find("Shape").unwrap(), "{text}");
            let (a2, b2) = (find(&again, 0), find(&again, 1));
            let after = offset(&again, a2, b2);
            assert!(after.abs_diff_eq(before, 1e-2), "{options:?}: {:?} invece di {:?}", Pose::from_matrix(after), Pose::from_matrix(before));
            assert_eq!(again.shapes_data.len(), scene.shapes_data.len());
            for (p, q) in scene.shapes_data.iter().zip(&again.shapes_data) {
                for (x, y) in p.world_vertices.iter().zip(&q.world_vertices) {
                    assert!(glam::Vec3::from(*x).distance(glam::Vec3::from(*y)) < 1e-3, "{options:?}: {x:?} {y:?}");
                }
            }
        }
    }

    /// Export in metri (i WRL della stazione): coordinate e `Transform` divisi per 1000, come li
    /// legge lo script dello scostamento (che moltiplica le `translation` per 1000).
    #[test]
    fn export_in_meters_divides_coordinates_and_transforms() {
        use crate::scene_graph::placement::Pose;
        let wrl = br#"#VRML V2.0 utf8
DEF Pezzo Group { children [ Transform { translation 215 0 1665 rotation 0 1 0 0.209439 children [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 4775.5 0 -102.5, 4800 0 -102.5, 4775.5 10 -102.5 ] } coordIndex [ 0 1 2 -1 ] } }
] } ] }
"#;
        let scene = crate::parser::parse_wrl(wrl);
        let pezzo = (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == "Pezzo").unwrap();
        for options in [ExportOptions { meters: true, ..Default::default() }, ExportOptions { meters: true, ..ExportOptions::VERBATIM }] {
            let path = std::env::temp_dir().join("dima3d_export_metri.wrl");
            export_with(&scene, &Scope::Visible, &path, options).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            let _ = std::fs::remove_file(&path);
            assert!(text.contains("translation 0.215 0 1.665\n"), "{options:?}\n{text}");
            let again = crate::parser::parse_wrl(text.as_bytes());
            let pezzo2 = (0..again.nodes.len() as u32).find(|&n| again.display_name(n) == "Pezzo").unwrap();
            let pose = Pose::from_matrix(again.object_frame(pezzo2));
            let expected = Pose::from_matrix(scene.object_frame(pezzo));
            assert!((pose.xyz * 1000.0 - expected.xyz).length() < 1e-2, "{pose:?} {expected:?}");
            assert!((pose.abc - expected.abc).length() < 1e-3);
            for (a, b) in scene.shapes_data[0].world_vertices.iter().zip(&again.shapes_data[0].world_vertices) {
                assert!((glam::Vec3::from(*a) - glam::Vec3::from(*b) * 1000.0).length() < 1e-2, "{a:?} {b:?}");
            }
        }
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

    /// "Esporta selezionati": solo i pezzi scelti (e visibili), con i gruppi che li contengono; il
    /// nodo del file solo se la selezione tocca più file. Uguale in tutti i formati.
    #[test]
    fn export_of_the_selection() {
        use crate::scene_graph::node::multi_file_tests::two_files;
        let mut scene = two_files();
        let shapes_of = |scene: &SceneGraph, root: u32| {
            let mut v = Vec::new();
            scene.for_each_subtree_node(root, |i| {
                if matches!(scene.nodes[i as usize], SceneNode::Shape { .. }) {
                    v.push(i);
                }
            });
            v
        };
        let (a, b) = (shapes_of(&scene, scene.roots[0]), shapes_of(&scene, scene.roots[1]));
        let dir = std::env::temp_dir();
        let path = dir.join("gestore_wrl_export_selezione.wrl");

        // Un pezzo per file: due pezzi, ognuno nel Group del suo file
        let scope = Scope::Selected(vec![a[0], b[1]]);
        let stats = export_with(&scene, &scope, &path, ExportOptions::default()).unwrap();
        assert_eq!(stats.shapes_written, 2);
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        let names: Vec<String> = again.roots.iter().map(|&r| again.display_name(r)).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(estimate_size(&scene, &scope, ExportOptions::default()), std::fs::metadata(&path).unwrap().len());

        // Il gruppo di un file solo: niente nodo del file, si ritrova l'albero di partenza
        let scope = Scope::Selected(vec![scene.roots[1]]);
        assert_eq!(export_with(&scene, &scope, &path, ExportOptions::default()).unwrap().shapes_written, 2);
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        assert_eq!(again.display_name(again.roots[0]), "Sportello");

        // I pezzi nascosti della selezione restano fuori
        scene.set_subtree_visible(b[0], false);
        assert_eq!(export_with(&scene, &scope, &path, ExportOptions::default()).unwrap().shapes_written, 1);

        // Gli altri formati scrivono gli stessi pezzi
        let scope = Scope::Selected(vec![a[0], b[1]]);
        let stl = dir.join("gestore_export_selezione.stl");
        assert_eq!(super::super::stl::export_stl(&scene, &scope, &stl, false).unwrap().shapes_written, 2);
        let iv = dir.join("gestore_export_selezione.iv");
        let color = [0.5, 0.5, 0.5, 1.0];
        assert_eq!(super::super::iv::export_iv(&scene, &scope, &iv, false, true, false, color).unwrap().shapes_written, 2);
        let hsf = dir.join("gestore_export_selezione.hsf");
        assert_eq!(super::super::hsf::export_hsf(&scene, &scope, &hsf, color, true).unwrap().shapes_written, 2);

        // Selezione senza pezzi visibili: un file vuoto, nessun pezzo
        let scope = Scope::Selected(vec![b[0]]);
        assert_eq!(export_with(&scene, &scope, &path, ExportOptions::default()).unwrap().shapes_written, 0);
        for p in [path, stl, iv, hsf] {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Confronto tra due WRL con la stessa geometria scritta in modo diverso (es. jt2wrl prima e
    /// dopo la scrittura compatta): `GESTORE_WRL_A=a.wrl GESTORE_WRL_B=b.wrl cargo test --release
    /// confronto_due_file -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn confronto_due_file() {
        let read = |k: &str| crate::parser::parse_wrl(&std::fs::read(std::env::var(k).expect(k)).unwrap());
        let (a, b) = (read("GESTORE_WRL_A"), read("GESTORE_WRL_B"));
        assert_eq!(a.shapes_data.len(), b.shapes_data.len(), "numero di pezzi");
        let (mut worst, mut worst_normal, mut worst_rel) = (0.0f32, 1.0f32, 0.0f32);
        for (x, y) in a.shapes_data.iter().zip(&b.shapes_data) {
            assert_eq!(x.indices, y.indices);
            assert_eq!(x.world_vertices.len(), y.world_vertices.len());
            for (p, q) in x.world_vertices.iter().zip(&y.world_vertices) {
                let d = (glam::Vec3::from(*p) - glam::Vec3::from(*q)).length();
                worst = worst.max(d);
                worst_rel = worst_rel.max(d / glam::Vec3::from(*p).length().max(1e-6));
            }
            for (n, m) in x.corner_normals().iter().zip(y.corner_normals()) {
                worst_normal = worst_normal.min(glam::Vec3::from(*n).dot(glam::Vec3::from(m)));
            }
            let (cx, cy) = (x.color.unwrap_or_default(), y.color.unwrap_or_default());
            assert!(cx.iter().zip(cy).all(|(u, v)| (u - v).abs() < 1e-5), "{cx:?} {cy:?}");
        }
        println!(
            "{} pezzi uguali; scostamento massimo dei vertici {worst:e} (relativo {worst_rel:e}), normali entro {:.3}°",
            a.shapes_data.len(),
            worst_normal.clamp(-1.0, 1.0).acos().to_degrees()
        );
    }

    #[test]
    fn fixed_numbers_are_short_and_exact() {
        let f = |v: f32, d: u32| {
            let mut out = Vec::new();
            push_fixed(&mut out, v, d);
            String::from_utf8(out).unwrap()
        };
        assert_eq!(f(1.5, 3), "1.5");
        assert_eq!(f(-0.003, 3), "-0.003");
        assert_eq!(f(12.0, 6), "12");
        assert_eq!(f(0.0004, 3), "0");
        assert_eq!(f(-0.0004, 3), "0");
        assert_eq!(f(1063.766416, 3), "1063.766");
        assert_eq!(f(0.05, 6), "0.05");
        assert_eq!(f(-4.7266731, 6), "-4.726673");
        assert_eq!(f(1.0e9, 0), "1000000000");
    }

    /// Scrittura compatta: riaprendo, stessi pezzi, vertici entro la precisione, stesse normali e
    /// colori; la stima della dimensione è esatta; senza normali il file è più piccolo.
    #[test]
    fn compact_export_round_trip() {
        let wrl = br#"#VRML V2.0 utf8
DEF Assieme Transform { translation 10 0 0 children [
    DEF A Group { children [
        Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
                geometry IndexedFaceSet {
                    coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0, 1 0 -1, 1 1 -1 ] }
                    coordIndex [ 0 1 2 3 -1, 1 4 5 2 -1 ]
                    normal Normal { vector [ 0 0 1, 1 0 0 ] }
                    normalPerVertex FALSE
                } }
    ] }
    DEF B Group { children [
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0.1234567 0 5, 1 0 5, 0 1 5 ] } coordIndex [ 0, 1, 2, -1 ] } }
    ] }
] }
"#;
        let scene = crate::parser::parse_wrl(wrl);
        let path = std::env::temp_dir().join("gestore_wrl_export_compatto.wrl");
        let options = ExportOptions { compact: true, normals: true, meters: false };
        export_with(&scene, &Scope::Visible, &path, options).unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_eq!(estimate_size(&scene, &Scope::Visible, options), out.len() as u64);
        let text = String::from_utf8_lossy(&out);
        assert!(!text.contains("\n  "), "niente rientri:\n{text}");
        assert!(text.contains("DEF Assieme Group") && text.contains("DEF A Group"));
        // Ingombro ~6: 6 decimali (passo di un milionesimo); coordinate nel sistema dell'oggetto
        assert!(text.contains("translation 10 0 0\n"), "{text}");
        assert!(text.contains("\n0.123457 0 5\n"), "{text}");

        let again = crate::parser::parse_wrl(&out);
        assert_eq!(again.shapes_data.len(), 2);
        for (a, b) in scene.shapes_data.iter().zip(&again.shapes_data) {
            assert_eq!(a.indices, b.indices);
            for (p, q) in a.world_vertices.iter().zip(&b.world_vertices) {
                assert!((glam::Vec3::from(*p) - glam::Vec3::from(*q)).length() < 1e-5);
            }
            for (n, m) in a.corner_normals().iter().zip(b.corner_normals()) {
                assert!(glam::Vec3::from(*n).dot(glam::Vec3::from(m)) > 0.9999, "{n:?} {m:?}");
            }
            if a.color.is_some() {
                assert_eq!(a.color, b.color);
            }
        }

        let lighter = ExportOptions { compact: true, normals: false, meters: false };
        export_with(&scene, &Scope::Visible, &path, lighter).unwrap();
        let out = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(!text.contains("normal") && text.contains("creaseAngle 0.5"), "{text}");
        assert!(estimate_size(&scene, &Scope::Visible, lighter) < estimate_size(&scene, &Scope::Visible, options));
        let _ = std::fs::remove_file(path);
    }

    /// I pezzi ricalcolati (es. ridotti) si scrivono dai dati anche nella copia fedele.
    #[test]
    fn rewritten_shapes_are_written_from_data() {
        let wrl = b"#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] } coordIndex [ 0 1 2 3 -1 ] } }";
        let mut scene = crate::parser::parse_wrl(wrl);
        let s = &mut scene.shapes_data[0];
        s.indices = vec![0, 1, 2];
        s.face_count = 1;
        s.rewritten = true;
        let path = std::env::temp_dir().join("gestore_wrl_export_ricalcolato.wrl");
        export_visible_wrl(&scene, &path).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        assert_eq!(again.shapes_data[0].indices, vec![0, 1, 2]);
        let _ = std::fs::remove_file(path);
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
