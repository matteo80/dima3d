//! Posizione dei pezzi: spostamenti e rotazioni applicati dopo il caricamento.
//!
//! La geometria della scena è già in coordinate mondo (`ShapeInfo::world_vertices`): uno
//! spostamento viene applicato direttamente a vertici, normali e ingombri, come la conversione
//! delle unità. Così selezione, misure, sezione e proprietà restano corrette senza saperne niente,
//! e la matrice della Shape (che l'export usa per le normali del file) segue lo spostamento.
//!
//! Ogni mesh ricorda la posizione accumulata (`SceneGraph::placements`): serve a «Ripristina» e ad
//! Annulla/Ripeti, che riportano le mesh alla posizione salvata applicando la differenza.
//! Ogni cambiamento di geometria resta in `moved` finché l'applicazione non lo ha portato su
//! scheda video e negli originali della riduzione delle facce.

use glam::{Mat3, Mat4, Vec3};
use rayon::prelude::*;

use super::{Aabb, SceneGraph, SceneNode};

/// Spostamento applicato a un gruppo di mesh (da riportare su GPU e negli originali messi da parte).
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub meshes: Vec<u32>,
    pub matrix: Mat4,
}

/// Applica `m` (coordinate mondo) a una Shape: vertici, normali e matrice.
pub fn transform_shape(s: &mut crate::parser::ShapeInfo, m: Mat4) {
    let normal_m = Mat3::from_mat4(m).inverse().transpose();
    for v in &mut s.world_vertices {
        *v = m.transform_point3(Vec3::from(*v)).into();
    }
    for n in &mut s.normals {
        *n = (normal_m * Vec3::from(*n)).normalize_or_zero().into();
    }
    s.matrix = m * s.matrix;
}

/// Rototraslazione con rotazioni attorno agli assi X, Y, Z (gradi, applicate in quest'ordine
/// attorno a `pivot`) seguite dallo spostamento `t`.
pub fn rigid(t: Vec3, angles_deg: Vec3, pivot: Vec3) -> Mat4 {
    let r = Mat4::from_rotation_z(angles_deg.z.to_radians())
        * Mat4::from_rotation_y(angles_deg.y.to_radians())
        * Mat4::from_rotation_x(angles_deg.x.to_radians());
    Mat4::from_translation(t + pivot) * r * Mat4::from_translation(-pivot)
}

/// Rotazione di `degrees` attorno alla retta per `origin` con direzione `dir` (regola della mano
/// destra: pollice lungo `dir`).
pub fn about_axis(origin: Vec3, dir: Vec3, degrees: f32) -> Mat4 {
    let d = dir.normalize_or_zero();
    if d == Vec3::ZERO {
        return Mat4::IDENTITY;
    }
    Mat4::from_translation(origin) * Mat4::from_axis_angle(d, degrees.to_radians()) * Mat4::from_translation(-origin)
}

/// Circonferenza per tre punti (es. sul bordo di un foro): centro e normale del suo piano.
/// `None` se i punti sono allineati o coincidenti.
pub fn circle_through(a: Vec3, b: Vec3, c: Vec3) -> Option<(Vec3, Vec3)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2 <= 1e-12 * (ab.length_squared() * ac.length_squared()).max(f32::MIN_POSITIVE) {
        return None;
    }
    let center = a + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / (2.0 * n2);
    Some((center, n.normalize()))
}

/// Posizione e orientamento come nei `CONST BASE` e nella finestra del visualizzatore usato in
/// azienda: X Y Z e angoli A B C in gradi, rotazioni ad assi fissi prima attorno a X (A), poi a Y
/// (B), poi a Z (C): R = Rz(C)·Ry(B)·Rx(A), la convenzione "xyz" di scipy degli script di calcolo
/// degli offset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
    pub xyz: Vec3,
    pub abc: Vec3,
}

impl Pose {
    pub fn matrix(&self) -> Mat4 {
        rigid(self.xyz, self.abc, Vec3::ZERO)
    }

    /// Posa di una matrice (l'eventuale scala viene ignorata).
    pub fn from_matrix(m: Mat4) -> Pose {
        let (_, rotation, translation) = m.to_scale_rotation_translation();
        Pose { xyz: translation, abc: euler_abc(Mat3::from_quat(rotation)) }
    }

    /// Scostamento di `dest` nel sistema locale di `self` (come BaseOffsetCalculator): la posa da
    /// applicare, in coordinate dell'oggetto, a un oggetto in `self` per portarlo in `dest`.
    pub fn offset_to(&self, dest: &Pose) -> Pose {
        Pose::from_matrix(self.matrix().inverse() * dest.matrix())
    }

    /// Valori arrotondati ai millesimi, come li scrivono gli script: "X Y Z A B C".
    pub fn to_text(&self) -> String {
        let v = [self.xyz.x, self.xyz.y, self.xyz.z, self.abc.x, self.abc.y, self.abc.z];
        v.iter().map(|x| format!("{:.3}", x + 0.0)).collect::<Vec<_>>().join(" ")
    }
}

/// Angoli A, B, C (gradi) di una rotazione R = Rz(C)·Ry(B)·Rx(A).
pub fn euler_abc(r: Mat3) -> Vec3 {
    // r.col(j).k = elemento (riga k, colonna j)
    let (r00, r10, r20) = (r.x_axis.x, r.x_axis.y, r.x_axis.z);
    let (r01, r11) = (r.y_axis.x, r.y_axis.y);
    let (r21, r22) = (r.y_axis.z, r.z_axis.z);
    // atan2 invece di asin: preciso anche vicino a B = ±90°
    let b = (-r20).atan2((r00 * r00 + r10 * r10).sqrt());
    let (a, c) = if (r00 * r00 + r10 * r10).sqrt() > 1e-6 {
        (r21.atan2(r22), r10.atan2(r00))
    } else {
        // B = ±90°: solo A ± C è definito, come scipy si mette A = 0
        (0.0, (-r01).atan2(r11))
    };
    Vec3::new(a.to_degrees(), b.to_degrees(), c.to_degrees())
}

/// Numeri del testo, nell'ordine (accetta anche la virgola decimale: "12,5").
fn numbers(text: &str) -> Vec<f32> {
    text.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '+' | '-' | 'e' | 'E')))
        .filter_map(|t| {
            let t = if t.contains('.') { t.to_string() } else { t.replace(',', ".") };
            t.parse::<f32>().ok()
        })
        .collect()
}

/// `CONST BASE NOME = X Y Z A B C` → (nome, posa).
pub fn parse_const_base(text: &str) -> Option<(String, Pose)> {
    let upper = text.to_ascii_uppercase();
    let at = upper.find("BASE")?;
    let rest = &text[at + 4..];
    let (name, values) = rest.split_once('=')?;
    let v = numbers(values);
    (v.len() >= 6).then(|| {
        (name.trim().to_string(), Pose { xyz: Vec3::new(v[0], v[1], v[2]), abc: Vec3::new(v[3], v[4], v[5]) })
    })
}

/// Offset incollato: righe "X  +12.000 mm" … "C  -0.500°" (risultato degli script), oppure sei
/// numeri X Y Z A B C.
pub fn parse_offset(text: &str) -> Option<Pose> {
    let mut found = [None; 6];
    for line in text.lines() {
        let t = line.trim_start();
        let Some(first) = t.chars().next() else { continue };
        let Some(k) = "XYZABC".find(first.to_ascii_uppercase()) else { continue };
        let after = &t[first.len_utf8()..];
        if !after.starts_with([' ', '\t', '=', ':']) {
            continue;
        }
        if let Some(&v) = numbers(after).first() {
            found[k] = Some(v);
        }
    }
    let v: Vec<f32> = if found.iter().all(Option::is_some) {
        found.iter().map(|v| v.unwrap()).collect()
    } else {
        numbers(text)
    };
    (v.len() == 6).then(|| Pose { xyz: Vec3::new(v[0], v[1], v[2]), abc: Vec3::new(v[3], v[4], v[5]) })
}

impl SceneGraph {
    /// Coordinate dell'oggetto di un nodo, nello stato attuale: il sistema dei `Transform` del
    /// file (scendendo lungo una catena di contenitori singoli, come lo script compone i
    /// Transform dal padre al figlio), con la conversione delle unità e gli spostamenti fatti nel
    /// Gestore. Senza Transform è l'origine (coordinate della cabina).
    pub fn object_frame(&self, node: u32) -> Mat4 {
        let mut n = node as usize;
        loop {
            match self.nodes.get(n) {
                Some(SceneNode::Shape { mesh_id, .. }) => {
                    return self.shapes_data.get(*mesh_id as usize).map_or(Mat4::IDENTITY, |s| s.matrix);
                }
                Some(SceneNode::Group { children, .. } | SceneNode::Transform { children, .. })
                    if children.len() == 1
                        && !matches!(self.nodes.get(children[0] as usize), Some(SceneNode::Shape { .. }) | None) =>
                {
                    n = children[0] as usize;
                }
                _ => break,
            }
        }
        let base = self.frames.get(n).copied().unwrap_or(Mat4::IDENTITY);
        let mut first = None;
        self.for_each_subtree_mesh(n as u32, |m| {
            first.get_or_insert(m);
        });
        let Some(m) = first else { return base };
        let scale = self.source_index(m).map_or(1.0, |i| self.sources[i].scale);
        self.placement(m) * Mat4::from_scale(Vec3::splat(scale)) * base
    }

    /// Posizione accumulata di una mesh (identità se non è mai stata spostata).
    pub fn placement(&self, mesh: u32) -> Mat4 {
        self.placements.get(mesh as usize).copied().unwrap_or(Mat4::IDENTITY)
    }

    /// Mesh spostate: (mesh, posizione) per quelle diverse dall'identità.
    pub fn moved_placements(&self) -> Vec<(u32, Mat4)> {
        self.placements
            .iter()
            .enumerate()
            .filter(|(_, p)| **p != Mat4::IDENTITY)
            .map(|(i, p)| (i as u32, *p))
            .collect()
    }

    /// Mesh dei sotto-alberi dei nodi dati, senza doppioni e in ordine.
    pub fn meshes_of(&self, nodes: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        for &n in nodes {
            if (n as usize) < self.nodes.len() {
                self.for_each_subtree_mesh(n, |m| out.push(m));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Sposta le mesh con `m` (coordinate mondo): vertici, normali, matrice e ingombri; la
    /// posizione accumulata di ognuna diventa `m · posizione`.
    pub fn transform_meshes(&mut self, meshes: &[u32], m: Mat4) {
        if meshes.is_empty() || m == Mat4::IDENTITY || !m.is_finite() {
            return;
        }
        let n = self.shapes_data.len();
        let mut chosen = vec![false; n];
        let mut list = Vec::with_capacity(meshes.len());
        for &i in meshes {
            if let Some(c) = chosen.get_mut(i as usize) {
                if !*c {
                    *c = true;
                    list.push(i);
                }
            }
        }
        if list.is_empty() {
            return;
        }
        // Le mesh di un'auto intera sono milioni di vertici: in parallelo
        let boxes: Vec<(usize, Aabb)> = self
            .shapes_data
            .par_iter_mut()
            .enumerate()
            .filter(|(i, _)| chosen[*i])
            .map(|(i, s)| {
                transform_shape(s, m);
                let mut bb = Aabb::empty();
                for &v in &s.world_vertices {
                    bb.extend_point(Vec3::from(v));
                }
                (i, bb)
            })
            .collect();
        if self.mesh_bboxes.len() == n {
            for (i, bb) in boxes {
                self.mesh_bboxes[i] = bb;
            }
        }
        if self.placements.len() < n {
            self.placements.resize(n, Mat4::IDENTITY);
        }
        for &i in &list {
            self.placements[i as usize] = m * self.placements[i as usize];
        }
        self.refresh_macro_bboxes(&chosen);
        list.sort_unstable();
        self.moved.push(super::placement::Moved { meshes: list, matrix: m });
        self.revision += 1;
    }

    /// Porta ogni mesh di `meshes` alla posizione `target(mesh)`, applicando la differenza dalla
    /// posizione attuale. Le mesh con la stessa differenza si spostano insieme.
    pub fn set_placements(&mut self, meshes: &[u32], target: impl Fn(u32) -> Mat4) {
        let mut groups: Vec<(Mat4, Vec<u32>)> = Vec::new();
        for &m in meshes {
            let (current, wanted) = (self.placement(m), target(m));
            if current == wanted {
                continue;
            }
            let delta = wanted * current.inverse();
            match groups.iter_mut().find(|(d, _)| *d == delta) {
                Some((_, list)) => list.push(m),
                None => groups.push((delta, vec![m])),
            }
        }
        for (delta, list) in groups {
            self.transform_meshes(&list, delta);
            // Valore esatto, senza l'errore di arrotondamento del prodotto
            for &m in &list {
                if let Some(p) = self.placements.get_mut(m as usize) {
                    *p = target(m);
                }
            }
        }
    }

    /// «Ripristina»: riporta le mesh alla posizione del caricamento.
    pub fn reset_placements(&mut self, meshes: &[u32]) {
        self.set_placements(meshes, |_| Mat4::IDENTITY);
    }

    /// Spostamenti in attesa di essere riportati su GPU e negli originali della riduzione.
    pub fn take_moved(&mut self) -> Vec<Moved> {
        std::mem::take(&mut self.moved)
    }

    /// Ricalcola l'ingombro dei macro-componenti che contengono mesh spostate.
    fn refresh_macro_bboxes(&mut self, moved: &[bool]) {
        let mesh_of = |node: u32| match self.nodes.get(node as usize) {
            Some(SceneNode::Shape { mesh_id, .. }) => Some(*mesh_id as usize),
            _ => None,
        };
        let mut updates = Vec::new();
        for (c, mc) in self.macro_components.iter().enumerate() {
            let meshes: Vec<usize> = mc.shape_indices.iter().filter_map(|&n| mesh_of(n)).collect();
            if !meshes.iter().any(|&m| moved.get(m).copied().unwrap_or(false)) {
                continue;
            }
            let mut bb = Aabb::empty();
            for m in meshes {
                if let Some(b) = self.mesh_bboxes.get(m).filter(|b| b.is_valid()) {
                    bb.extend_aabb(b);
                }
            }
            updates.push((c, bb));
        }
        for (c, bb) in updates {
            self.macro_components[c].bbox = bb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_graph::node::multi_file_tests::two_files;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    fn node(scene: &SceneGraph, name: &str) -> u32 {
        (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == name).unwrap()
    }

    #[test]
    fn rigid_rotates_about_the_pivot_then_moves() {
        // 90° attorno a Z con perno (1,0,0): il punto (2,0,0) va in (1,1,0), poi +10 in X
        let m = rigid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 90.0), Vec3::X);
        assert!(close(m.transform_point3(Vec3::new(2.0, 0.0, 0.0)).into(), [11.0, 1.0, 0.0]));
        // Ordine X, poi Y, poi Z: (0,1,0) → X 90° → (0,0,1) → Y 90° → (1,0,0) → Z 90° → (0,1,0)
        let r = rigid(Vec3::ZERO, Vec3::new(90.0, 90.0, 90.0), Vec3::ZERO);
        assert!(close(r.transform_point3(Vec3::Y).into(), [0.0, 1.0, 0.0]));
        assert!(close(r.transform_point3(Vec3::Z).into(), [1.0, 0.0, 0.0]));
    }

    /// Spostare un file muove vertici, normali, ingombri e matrice delle sue mesh; l'altro file
    /// resta dov'è. Ripristina torna esattamente alla posizione di partenza.
    #[test]
    fn moving_a_file_and_resetting_it() {
        let mut scene = two_files();
        let before: Vec<_> = scene.shapes_data.iter().map(|s| (s.world_vertices.clone(), s.normals.clone())).collect();
        let file_b = scene.sources[1].node;
        let meshes = scene.meshes_of(&[file_b]);
        assert_eq!(meshes, vec![2, 3]);

        let m = rigid(Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, 90.0), Vec3::ZERO);
        scene.transform_meshes(&meshes, m);
        // Pannello: (5,0,1) ruotato di 90° attorno a Z → (0,5,1), poi +100 in Z
        assert!(close(scene.shapes_data[2].world_vertices[0], [0.0, 5.0, 101.0]));
        // Normale del triangolo nel piano XY: resta lungo Z
        assert!(scene.shapes_data[2].normals.iter().all(|n| n[2].abs() > 0.99));
        assert!((scene.mesh_bboxes[2].min.z - 101.0).abs() < 1e-4);
        assert_eq!(scene.shapes_data[2].matrix, m * Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        assert_eq!(scene.placement(2), m);
        assert_eq!(scene.shapes_data[0].world_vertices, before[0].0, "il file A non si muove");
        assert_eq!(scene.placement(0), Mat4::IDENTITY);
        assert_eq!(scene.take_moved(), vec![Moved { meshes: vec![2, 3], matrix: m }]);
        assert!(scene.take_moved().is_empty());

        // Un secondo spostamento si accumula
        let up = Mat4::from_translation(Vec3::new(0.0, 0.0, 1.0));
        scene.transform_meshes(&[2], up);
        assert_eq!(scene.placement(2), up * m);
        assert_eq!(scene.moved_placements().len(), 2);

        scene.reset_placements(&meshes);
        for (i, (v, n)) in before.iter().enumerate() {
            for (a, b) in scene.shapes_data[i].world_vertices.iter().zip(v) {
                assert!(close(*a, *b), "mesh {i}: {a:?} invece di {b:?}");
            }
            for (a, b) in scene.shapes_data[i].normals.iter().zip(n) {
                assert!(close(*a, *b));
            }
        }
        assert!(scene.moved_placements().is_empty());
        assert_eq!(scene.take_moved().len(), 3, "spostamento, spostamento e ritorno (due differenze diverse)");
    }

    /// Un pezzo solo, scelto dal suo nodo.
    #[test]
    fn moving_one_piece() {
        let mut scene = two_files();
        let pannello = node(&scene, "Pannello");
        let meshes = scene.meshes_of(&[pannello]);
        assert_eq!(meshes.len(), 1);
        scene.transform_meshes(&meshes, Mat4::from_translation(Vec3::X));
        assert!(close(scene.shapes_data[meshes[0] as usize].world_vertices[0], [6.0, 0.0, 1.0]));
        let gemello = node(&scene, "Gemello");
        let other = scene.meshes_of(&[gemello])[0];
        assert!(close(scene.shapes_data[other as usize].world_vertices[0], [5.0, 0.0, 1.0]));
    }

    /// Annulla e Ripeti riportano le mesh alla posizione salvata nello stato.
    #[test]
    fn undo_states_carry_the_position() {
        let mut scene = two_files();
        let saved = scene.snapshot();
        let m = Mat4::from_translation(Vec3::new(0.0, 7.0, 0.0));
        scene.transform_meshes(&[0, 1], m);
        scene.take_moved();
        let moved_state = scene.swap_snapshot(saved);
        assert!(close(scene.shapes_data[0].world_vertices[1], [1.0, 0.0, 0.0]), "annullato");
        assert_eq!(scene.placement(0), Mat4::IDENTITY);
        assert_eq!(scene.take_moved().len(), 1);
        scene.swap_snapshot(moved_state);
        assert!(close(scene.shapes_data[0].world_vertices[1], [1.0, 7.0, 0.0]), "ripetuto");
        assert_eq!(scene.placement(1), m);
    }

    /// Convertire le unità di un file spostato: la posizione resta coerente con «Ripristina».
    #[test]
    fn scaling_keeps_the_position_consistent() {
        let mut scene = two_files();
        let meshes = scene.meshes_of(&[scene.sources[1].node]);
        let original = scene.shapes_data[2].world_vertices.clone();
        scene.transform_meshes(&meshes, Mat4::from_translation(Vec3::new(0.0, 0.0, 3.0)));
        scene.scale_file(1, 0.001);
        scene.reset_placements(&meshes);
        for (a, b) in scene.shapes_data[2].world_vertices.iter().zip(&original) {
            assert!(close(*a, (Vec3::from(*b) * 0.001).into()), "{a:?}");
        }
    }

    fn close_pose(p: &Pose, xyz: [f32; 3], abc: [f32; 3], tol: f32) -> bool {
        (p.xyz - Vec3::from(xyz)).abs().max_element() < tol && (p.abc - Vec3::from(abc)).abs().max_element() < tol
    }

    /// Lo scostamento tra due CONST BASE è quello di BaseOffsetCalculator (valori calcolati con
    /// le stesse formule, scipy "xyz", su un esempio).
    #[test]
    fn base_offset_matches_the_script() {
        let (name_o, o) = parse_const_base("CONST BASE BASE_A = 150 2500 -1200 0.5 -20 -85").unwrap();
        let (name_d, d) = parse_const_base("const base BASE_B = 180 3100 40 1.2 -12.5 -91").unwrap();
        assert_eq!((name_o.as_str(), name_d.as_str()), ("BASE_A", "BASE_B"));
        let off = o.offset_to(&d);
        assert!(close_pose(&off, [-135.1081, 94.1207, 1367.9865], [-1.3675, 7.3426, -5.9709], 2e-3), "{off:?}");
        // Applicato all'origine, l'offset riporta alla destinazione
        let back = Pose::from_matrix(o.matrix() * off.matrix());
        assert!(close_pose(&back, [180.0, 3100.0, 40.0], [1.2, -12.5, -91.0], 2e-3), "{back:?}");
        assert!(parse_const_base("BASE senza valori = 1 2").is_none());
    }

    #[test]
    fn angles_survive_a_round_trip() {
        for abc in [[10.0, -35.0, 120.0], [0.0, 0.0, 0.0], [-170.0, 80.0, 45.0], [0.287, -22.743, -89.71]] {
            let p = Pose { xyz: Vec3::new(1.0, 2.0, 3.0), abc: Vec3::from(abc) };
            let q = Pose::from_matrix(p.matrix());
            assert!(close_pose(&q, [1.0, 2.0, 3.0], abc, 1e-3), "{abc:?} → {q:?}");
        }
        // B = 90°: si ottiene una rotazione uguale, con A = 0
        let p = Pose { xyz: Vec3::ZERO, abc: Vec3::new(20.0, 90.0, 50.0) };
        let q = Pose::from_matrix(p.matrix());
        assert!(q.matrix().abs_diff_eq(p.matrix(), 1e-4) && q.abc.x == 0.0, "{q:?}");
    }

    /// Il risultato copiato dagli script si incolla così com'è; vanno bene anche sei numeri.
    #[test]
    fn pasted_offsets_are_read() {
        let script = "BASE_A  ->  BASE_B\n\nPOSIZIONE\nX     -135.108 mm\nY      +94.121 mm\nZ    +1367.987 mm\n\n\
                      ORIENTAMENTO\nA       -1.368°\nB       +7.343°\nC       -5.971°";
        let p = parse_offset(script).unwrap();
        assert!(close_pose(&p, [-135.108, 94.121, 1367.987], [-1.368, 7.343, -5.971], 1e-4), "{p:?}");
        let p = parse_offset("10 -20,5 30 1 2 3").unwrap();
        assert!(close_pose(&p, [10.0, -20.5, 30.0], [1.0, 2.0, 3.0], 1e-6), "{p:?}");
        assert!(parse_offset("1 2 3").is_none());
        assert_eq!(Pose { xyz: Vec3::new(1.0, -0.0004, 2.5), abc: Vec3::ZERO }.to_text(), "1.000 -0.000 2.500 0.000 0.000 0.000");
    }

    /// Due oggetti nello stesso WRL con i loro Transform (come in WRLOffsetCalculator): le
    /// coordinate dell'oggetto sono i Transform composti; l'offset tra i due, applicato in
    /// coordinate dell'oggetto alla copia del primo in un'altra matematica uguale, la porta
    /// esattamente nella posizione del secondo.
    #[test]
    fn object_coordinates_and_the_skid_offset() {
        const SKID: &[u8] = br#"#VRML V2.0 utf8
DEF Riferimento Group { children [
  Transform { translation 100 200 300 rotation 0 0 1 1.5707963 children [
    Transform { translation 10 0 0 children [
      Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 50 0 0, 0 50 0 ] } coordIndex [ 0 1 2 -1 ] } }
    ] } ] } ] }
DEF Destinazione Group { children [
  Transform { translation 400 -50 20 rotation 1 0 0 0.3 children [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 50 0 0, 0 50 0 ] } coordIndex [ 0 1 2 -1 ] } }
  ] } ] }
"#;
        let mut scene = crate::scene_graph::node::multi_file_tests::file(SKID, "skid.wrl");
        let reference = node(&scene, "Riferimento");
        let destination = node(&scene, "Destinazione");
        let f_ref = Pose::from_matrix(scene.object_frame(reference));
        // Transform composti: (100,200,300) + Rz(90°)·(10,0,0) = (100,210,300), C = 90°
        assert!(close_pose(&f_ref, [100.0, 210.0, 300.0], [0.0, 0.0, 90.0], 1e-3), "{f_ref:?}");
        let f_dest = Pose::from_matrix(scene.object_frame(destination));
        assert!(close_pose(&f_dest, [400.0, -50.0, 20.0], [0.3f32.to_degrees(), 0.0, 0.0], 1e-3), "{f_dest:?}");
        let offset = f_ref.offset_to(&f_dest);

        // Seconda matematica uguale: la copia del riferimento va dove sta la destinazione
        scene.append(crate::scene_graph::node::multi_file_tests::file(SKID, "copia.wrl"));
        let copy = (0..scene.nodes.len() as u32).filter(|&n| scene.display_name(n) == "Riferimento").last().unwrap();
        assert_ne!(copy, reference);
        let frame = scene.object_frame(copy);
        let meshes = scene.meshes_of(&[copy]);
        scene.transform_meshes(&meshes, frame * offset.matrix() * frame.inverse());
        let moved = Pose::from_matrix(scene.object_frame(copy));
        assert!(close_pose(&moved, f_dest.xyz.into(), f_dest.abc.into(), 2e-3), "{moved:?} invece di {f_dest:?}");
        // I vertici coincidono con quelli della destinazione (stessa geometria nel suo sistema)
        let dest_mesh = scene.meshes_of(&[destination])[0] as usize;
        for (a, b) in scene.shapes_data[meshes[0] as usize].world_vertices.iter().zip(&scene.shapes_data[dest_mesh].world_vertices) {
            assert!(close(*a, *b) || (Vec3::from(*a) - Vec3::from(*b)).length() < 1e-2, "{a:?} {b:?}");
        }
        // In metri convertiti in mm le coordinate seguono la scala
        scene.scale_file(1, 1000.0);
        let scaled = Pose::from_matrix(scene.object_frame(copy));
        assert!(close_pose(&scaled, (f_dest.xyz * 1000.0).into(), f_dest.abc.into(), 2.0), "{scaled:?}");
    }

    /// Centro e asse di un foro da tre punti del bordo; rotazione attorno a un asse qualsiasi.
    #[test]
    fn hole_centre_and_rotation_about_an_axis() {
        // Foro di raggio 5 centrato in (10, 20, 30), nel piano perpendicolare a Y
        let c = Vec3::new(10.0, 20.0, 30.0);
        let on = |deg: f32| c + Vec3::new(deg.to_radians().cos(), 0.0, deg.to_radians().sin()) * 5.0;
        let (centre, normal) = circle_through(on(10.0), on(100.0), on(250.0)).unwrap();
        assert!((centre - c).length() < 1e-3, "{centre:?}");
        assert!(normal.y.abs() > 0.999, "{normal:?}");
        assert!(circle_through(Vec3::ZERO, Vec3::X, Vec3::X * 2.0).is_none(), "punti allineati");

        // 90° attorno all'asse verticale per (1, 0, 0): (2, 0, 0) va in (1, 1, 0)
        let m = about_axis(Vec3::X, Vec3::Z, 90.0);
        assert!(close(m.transform_point3(Vec3::new(2.0, 0.0, 0.0)).into(), [1.0, 1.0, 0.0]));
        assert!(close(m.transform_point3(Vec3::new(1.0, 0.0, 7.0)).into(), [1.0, 0.0, 7.0]), "i punti sull'asse restano");
        assert_eq!(about_axis(Vec3::X, Vec3::ZERO, 30.0), Mat4::IDENTITY);
    }
}
