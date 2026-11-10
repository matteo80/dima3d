//! Riduzione delle facce dei pezzi: collasso degli spigoli guidato dall'errore quadrico (lo stesso
//! metodo di "Quadric Edge Collapse Decimation" di MeshLab), con la libreria meshoptimizer.
//!
//! Ogni pezzo viene prima "saldato" (i vertici nella stessa posizione diventano uno solo: i file
//! CAD ripetono i vertici lungo i bordi delle facce, che altrimenti resterebbero bloccati), poi
//! semplificato, e infine riceve normali nuove con un angolo di spigolo vivo, come `creaseAngle`.
//!
//! L'errore massimo è unico per tutta la scena: le superfici piatte perdono molti triangoli, i
//! raccordi pochi. Per un numero di facce richiesto si cerca l'errore che lo dà.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

use rayon::prelude::*;

use crate::parser::{ShapeInfo, SplitNormals};

/// Angolo oltre il quale due facce vicine formano uno spigolo vivo (come `creaseAngle 0.5` dei
/// WRL esportati senza normali).
pub const CREASE_ANGLE: f32 = 0.5;

/// Quante facce tenere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Numero totale di triangoli dei pezzi da ridurre: si cerca l'errore che lo dà
    Triangles(u64),
    /// Scostamento massimo dalla superficie originale, nelle unità del file
    MaxError(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub target: Target,
    /// Bordi aperti (contorni delle lamiere, fori, confini tra pezzi) mantenuti esattamente.
    /// Senza, i bordi si spostano al più dell'errore massimo.
    pub lock_border: bool,
}

/// Risultato di una riduzione.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Pezzo ridotto, parallelo ai pezzi richiesti (`None` se non cambia)
    pub shapes: Vec<Option<ShapeInfo>>,
    /// Errore massimo usato: scostamento massimo dalla superficie originale, nelle unità del file
    pub max_error: f32,
    pub triangles_before: u64,
    pub triangles_after: u64,
}

/// Mesh con un solo vertice per posizione e senza triangoli degeneri.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Welded {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Vertice saldato di ogni vertice della mesh di partenza (vuoto dopo la semplificazione)
    pub from_original: Vec<u32>,
}

/// Unisce i vertici con la stessa identica posizione e toglie i triangoli che ne risultano
/// degeneri (due angoli sullo stesso vertice) o con indici fuori dai vertici.
pub fn weld(vertices: &[[f32; 3]], indices: &[u32]) -> Welded {
    let mut map: HashMap<[u32; 3], u32> = HashMap::with_capacity(vertices.len());
    let mut remap = Vec::with_capacity(vertices.len());
    let mut positions = Vec::with_capacity(vertices.len());
    for v in vertices {
        // +0.0 e -0.0 sono lo stesso punto
        let key = v.map(|c| if c == 0.0 { 0 } else { c.to_bits() });
        let next = positions.len() as u32;
        let id = *map.entry(key).or_insert_with(|| {
            positions.push(*v);
            next
        });
        remap.push(id);
    }
    let mut out = Vec::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&c)) =
            (remap.get(tri[0] as usize), remap.get(tri[1] as usize), remap.get(tri[2] as usize))
        else {
            continue;
        };
        if a != b && b != c && a != c {
            out.extend_from_slice(&[a, b, c]);
        }
    }
    Welded { positions, indices: out, from_original: remap }
}

/// Triangoli semplificati finché l'errore resta sotto `max_error` (nelle unità del file).
///
/// Solo l'errore di forma: provato anche a far pesare le normali (riflessi più fedeli sulle
/// superfici lucide), ma il loro contributo si somma all'errore e su una scocca da 3,3 milioni di
/// facce ridotta a 1 milione lasciava spostare la lamiera esterna di decine di mm, con i rinforzi
/// interni che la attraversavano a chiazze. Le opzioni di regolarizzazione dei triangoli davano
/// striature attorno ai raccordi.
fn simplified_indices(mesh: &Welded, max_error: f32, lock_border: bool) -> Vec<u32> {
    if mesh.positions.is_empty() || max_error <= 0.0 {
        return mesh.indices.clone();
    }
    let bytes: &[u8] = bytemuck::cast_slice(&mesh.positions);
    let Ok(adapter) = meshopt::VertexDataAdapter::new(bytes, 12, 0) else {
        return mesh.indices.clone();
    };
    let mut options = meshopt::SimplifyOptions::ErrorAbsolute;
    if lock_border {
        options |= meshopt::SimplifyOptions::LockBorder;
    }
    meshopt::simplify(&mesh.indices, &adapter, 0, max_error, options, None)
}

/// Semplifica finché l'errore resta sotto `max_error`; i vertici non più usati vengono tolti.
#[cfg(test)]
pub fn simplify_to_error(mesh: &Welded, max_error: f32, lock_border: bool) -> Welded {
    compact(&mesh.positions, &simplified_indices(mesh, max_error, lock_border)).0
}

/// Tiene solo i vertici usati dai triangoli, nell'ordine in cui compaiono. Ritorna anche, per
/// ogni vertice tenuto, la sua posizione in `positions`.
fn compact(positions: &[[f32; 3]], indices: &[u32]) -> (Welded, Vec<u32>) {
    let mut new_index = vec![u32::MAX; positions.len()];
    let mut out_positions = Vec::new();
    let mut kept = Vec::new();
    let out_indices = indices
        .iter()
        .map(|&i| {
            let slot = &mut new_index[i as usize];
            if *slot == u32::MAX {
                *slot = out_positions.len() as u32;
                out_positions.push(positions[i as usize]);
                kept.push(i);
            }
            *slot
        })
        .collect();
    (Welded { positions: out_positions, indices: out_indices, from_original: Vec::new() }, kept)
}

/// Normali del file originale attorno a ogni vertice saldato (senza ripetizioni), in forma CSR:
/// quelle del vertice `w` sono `normals[start[w]..start[w + 1]]`.
struct NormalCandidates {
    start: Vec<u32>,
    normals: Vec<[f32; 3]>,
}

impl NormalCandidates {
    fn new(original: &ShapeInfo, welded: &Welded) -> Option<Self> {
        let corners = original.corner_normals();
        if corners.len() != original.indices.len() || welded.from_original.len() != original.world_vertices.len() {
            return None;
        }
        let n = welded.positions.len();
        let mut per_vertex: Vec<Vec<[f32; 3]>> = vec![Vec::new(); n];
        for (&o, normal) in original.indices.iter().zip(&corners) {
            let Some(&w) = welded.from_original.get(o as usize) else { continue };
            let list = &mut per_vertex[w as usize];
            let v = glam::Vec3::from(*normal);
            if v.length_squared() > 0.5 && !list.iter().any(|m| glam::Vec3::from(*m).dot(v) > 0.9999) {
                list.push(*normal);
            }
        }
        let mut start = Vec::with_capacity(n + 1);
        let mut normals = Vec::new();
        start.push(0);
        for list in per_vertex {
            normals.extend(list);
            start.push(normals.len() as u32);
        }
        Some(Self { start, normals })
    }

    fn of(&self, w: u32) -> &[[f32; 3]] {
        &self.normals[self.start[w as usize] as usize..self.start[w as usize + 1] as usize]
    }
}

/// Scarto massimo tra la normale del file e quella ricalcolata sui triangoli ridotti perché si
/// tenga la prima (coseno di 5°).
const TRANSFER_COS: f32 = 0.996_2;

/// Normale di ogni angolo dei triangoli: si parte da quella ricalcolata sui triangoli ridotti
/// (spigoli vivi oltre `crease_angle`) e, se il vertice aveva nel file una normale entro 5° da
/// questa, si tiene quella del file, cioè della superficie vera del CAD. Lo scarto piccolo evita
/// che la normale di un vertice in mezzo a un raccordo venga stesa su un triangolo lungo della
/// superficie vicina (striature scure a ventaglio).
fn transferred_corner_normals(
    reduced: &Welded,
    kept: &[u32],
    candidates: &NormalCandidates,
    crease_angle: f32,
) -> Vec<[f32; 3]> {
    let computed = corner_crease_normals(&reduced.positions, &reduced.indices, crease_angle);
    reduced
        .indices
        .iter()
        .zip(computed)
        .map(|(&v, normal)| {
            let reference = glam::Vec3::from(normal);
            candidates
                .of(kept[v as usize])
                .iter()
                .map(|n| (glam::Vec3::from(*n).dot(reference), *n))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .filter(|(dot, _)| *dot > TRANSFER_COS)
                .map_or(normal, |(_, n)| n)
        })
        .collect()
}

/// Normali con spigoli vivi: ogni angolo di triangolo media le facce attorno al suo vertice che
/// formano con la propria faccia un angolo minore di `crease_angle`. Un vertice con normali
/// diverse sulle sue facce viene sdoppiato per la resa ([`SplitNormals`]).
pub fn crease_normals(positions: &[[f32; 3]], indices: &[u32], crease_angle: f32) -> (Vec<[f32; 3]>, Option<Box<SplitNormals>>) {
    split_by_normal(positions.len(), indices, &corner_crease_normals(positions, indices, crease_angle))
}

fn unit_of(v: glam::Vec3) -> Option<glam::Vec3> {
    let largest = v.abs().max_element();
    (largest > 0.0 && largest.is_finite()).then(|| (v / largest).normalize())
}

/// Normale di ogni angolo dei triangoli con l'angolo di spigolo vivo (parallela a `indices`).
fn corner_crease_normals(positions: &[[f32; 3]], indices: &[u32], crease_angle: f32) -> Vec<[f32; 3]> {
    let n = positions.len();
    let p = |i: u32| glam::Vec3::from(positions[i as usize]);
    // Normale di faccia pesata con l'area, e il suo versore
    let face: Vec<glam::Vec3> = indices
        .chunks_exact(3)
        .map(|t| (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])))
        .collect();
    let unit: Vec<glam::Vec3> = face.iter().map(|&f| unit_of(f).unwrap_or(glam::Vec3::ZERO)).collect();

    // Facce attorno a ogni vertice (CSR)
    let mut start = vec![0u32; n + 1];
    for &i in indices {
        start[i as usize + 1] += 1;
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut faces_of = vec![0u32; indices.len()];
    for (c, &i) in indices.iter().enumerate() {
        faces_of[fill[i as usize] as usize] = (c / 3) as u32;
        fill[i as usize] += 1;
    }

    let cos_limit = crease_angle.cos();
    indices
        .iter()
        .enumerate()
        .map(|(c, &v)| {
            let t = c / 3;
            let around = &faces_of[start[v as usize] as usize..start[v as usize + 1] as usize];
            let mut sum = glam::Vec3::ZERO;
            for &f in around {
                if f as usize == t || unit[f as usize].dot(unit[t]) >= cos_limit {
                    sum += face[f as usize];
                }
            }
            unit_of(sum).map_or([0.0, 1.0, 0.0], |u| u.to_array())
        })
        .collect()
}

/// Vertici di resa dalle normali degli angoli: un vertice con normali diverse sui suoi
/// triangoli viene sdoppiato ([`SplitNormals`]); normali entro ~2° contano come uguali.
fn split_by_normal(n: usize, indices: &[u32], corner: &[[f32; 3]]) -> (Vec<[f32; 3]>, Option<Box<SplitNormals>>) {
    let mut normals: Vec<Option<[f32; 3]>> = vec![None; n];
    let mut extra: Vec<u32> = Vec::new();
    let mut extra_normals: Vec<[f32; 3]> = Vec::new();
    // Copie già create per ogni vertice: (normale, vertice di resa)
    let mut copies: HashMap<u32, Vec<([f32; 3], u32)>> = HashMap::new();
    let mut render_indices = Vec::with_capacity(indices.len());
    for (&v, &normal) in indices.iter().zip(corner) {
        let same = |a: [f32; 3]| glam::Vec3::from(a).dot(glam::Vec3::from(normal)) > 0.9994;
        let render = match normals[v as usize] {
            None => {
                normals[v as usize] = Some(normal);
                v
            }
            Some(first) if same(first) => v,
            Some(_) => {
                let list = copies.entry(v).or_default();
                match list.iter().find(|(m, _)| same(*m)) {
                    Some(&(_, r)) => r,
                    None => {
                        let r = (n + extra.len()) as u32;
                        extra.push(v);
                        extra_normals.push(normal);
                        list.push((normal, r));
                        r
                    }
                }
            }
        };
        render_indices.push(render);
    }
    let mut all: Vec<[f32; 3]> = normals.into_iter().map(|n| n.unwrap_or([0.0, 1.0, 0.0])).collect();
    all.extend(extra_normals);
    let split = (!extra.is_empty()).then(|| Box::new(SplitNormals { extra, indices: render_indices }));
    (all, split)
}

/// Pezzo ridotto con l'errore massimo dato; `None` se il numero di triangoli non cambia.
///
/// I vertici tenuti sono punti della mesh originale: dove la normale ricalcolata è vicina a quella
/// del file, ricevono quella del file (vedi [`transferred_corner_normals`]).
pub fn reduce_shape(original: &ShapeInfo, welded: &Welded, max_error: f32, lock_border: bool) -> Option<ShapeInfo> {
    let (mesh, kept) = compact(&welded.positions, &simplified_indices(welded, max_error, lock_border));
    if mesh.indices.len() >= original.indices.len() {
        return None;
    }
    let corner = match NormalCandidates::new(original, welded) {
        Some(candidates) => transferred_corner_normals(&mesh, &kept, &candidates, CREASE_ANGLE),
        None => corner_crease_normals(&mesh.positions, &mesh.indices, CREASE_ANGLE),
    };
    let (normals, split) = split_by_normal(mesh.positions.len(), &mesh.indices, &corner);
    Some(ShapeInfo {
        source_start: original.source_start,
        source_end: original.source_end,
        point_range_start: original.point_range_start,
        point_range_end: original.point_range_end,
        matrix: original.matrix,
        face_count: (mesh.indices.len() / 3) as u32,
        world_vertices: mesh.positions,
        normals,
        split,
        indices: mesh.indices,
        name: original.name.clone(),
        color: original.color,
        path: original.path.clone(),
        rewritten: true,
    })
}

/// Riduce in parallelo i pezzi indicati, partendo dalla loro geometria originale.
/// `progress` va da 0 a 1000 durante il lavoro (per la barra di avanzamento).
pub fn reduce_many(originals: &[&ShapeInfo], params: Params, progress: &AtomicU32) -> Outcome {
    let triangles = |s: &ShapeInfo| (s.indices.len() / 3) as u64;
    let triangles_before: u64 = originals.iter().map(|s| triangles(s)).sum();
    let mut bbox = crate::scene_graph::Aabb::empty();
    for s in originals {
        for &p in &s.world_vertices {
            bbox.extend_point(glam::Vec3::from(p));
        }
    }
    let extent = if bbox.is_valid() { (bbox.max - bbox.min).length().max(1e-9) } else { 1.0 };
    let welded: Vec<Welded> = originals.par_iter().map(|s| weld(&s.world_vertices, &s.indices)).collect();
    progress.store(50, Relaxed);

    let max_error = match params.target {
        Target::MaxError(e) => e.max(0.0),
        Target::Triangles(n) if n >= triangles_before => 0.0,
        Target::Triangles(n) => search_error(&welded, n, params.lock_border, extent, |step| progress.store(50 + step * 80, Relaxed)),
    };
    progress.store(900, Relaxed);
    let shapes: Vec<Option<ShapeInfo>> = originals
        .par_iter()
        .zip(welded.par_iter())
        .map(|(s, w)| if max_error > 0.0 { reduce_shape(s, w, max_error, params.lock_border) } else { None })
        .collect();
    let triangles_after = shapes
        .iter()
        .zip(originals)
        .map(|(r, s)| r.as_ref().map_or_else(|| triangles(s), triangles))
        .sum();
    progress.store(1000, Relaxed);
    Outcome { shapes, max_error, triangles_before, triangles_after }
}

/// Errore che porta il totale dei triangoli a circa `target` (entro il 2%). Su una superficie i
/// triangoli necessari crescono circa come 1/errore: la ricerca procede per secanti sulla scala
/// logaritmica, con la bisezione come ripiego, tra un decimilionesimo e un centesimo
/// dell'ingombro. Di solito bastano 3–5 passate.
fn search_error(welded: &[Welded], target: u64, lock_border: bool, extent: f32, mut step: impl FnMut(u32)) -> f32 {
    let count = |e: f64| -> u64 {
        welded.par_iter().map(|w| (simplified_indices(w, e as f32, lock_border).len() / 3) as u64).sum()
    };
    let tolerance = (target / 50).max(1);
    let goal = (target.max(1) as f64).ln();
    // Estremi: a `lo` troppi triangoli, a `hi` troppo pochi (in log dell'errore, log dei triangoli)
    let mut lo: Option<(f64, f64)> = None;
    let mut hi: Option<(f64, f64)> = None;
    // Estremo aggiornato all'ultimo passo (1 = lo, -1 = hi), per la correzione "Illinois"
    let mut side = 0i8;
    let (min_e, max_e) = (((extent as f64) * 1e-7).ln(), ((extent as f64) * 1e-2).ln());
    let mut e = ((extent as f64) * 1e-4).ln();
    let mut best: Option<(u64, f64)> = None;
    for i in 0..12 {
        step(i.min(10));
        let n = count(e.exp());
        let gap = n.abs_diff(target);
        // Il risultato più vicino all'obiettivo, sopra o sotto
        if best.is_none_or(|(g, _)| gap < g) {
            best = Some((gap, e));
        }
        if gap <= tolerance {
            break;
        }
        let point = (e, (n.max(1) as f64).ln());
        // Illinois: se lo stesso estremo si sposta due volte di fila, l'altro pesa la metà, così
        // la secante non resta inchiodata a un estremo (la curva è molto convessa)
        if n > target {
            if side == 1 {
                if let Some(h) = &mut hi {
                    h.1 = goal + (h.1 - goal) / 2.0;
                }
            }
            lo = Some(point);
            side = 1;
        } else {
            if side == -1 {
                if let Some(l) = &mut lo {
                    l.1 = goal + (l.1 - goal) / 2.0;
                }
            }
            hi = Some(point);
            side = -1;
        }
        e = match (lo, hi) {
            (Some(a), Some(b)) => {
                // Secante tra gli estremi, tenuta dentro l'intervallo; se cade troppo vicino a un
                // estremo si dimezza
                let t = if (a.1 - b.1).abs() > 1e-12 { (a.1 - goal) / (a.1 - b.1) } else { 0.5 };
                let t = if (0.05..=0.95).contains(&t) { t } else { 0.5 };
                a.0 + t * (b.0 - a.0)
            }
            // Un solo estremo: si sposta con pendenza -1 (triangoli ∝ 1/errore), almeno di un po'
            (Some(a), None) => (a.0 + (a.1 - goal).max(0.1)).min(max_e),
            (None, Some(b)) => (b.0 - (goal - b.1).max(0.1)).max(min_e),
            (None, None) => unreachable!(),
        };
        if lo.is_some_and(|a| a.0 >= max_e) || hi.is_some_and(|b| b.0 <= min_e) {
            break; // obiettivo fuori portata (es. bordi bloccati)
        }
    }
    best.map_or(e, |(_, e)| e).exp() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Griglia n×n di quadrati con i vertici ripetuti per ogni quadrato (come le facce separate
    /// dei file CAD), sollevata da `height(x, y)`.
    fn grid(n: usize, height: impl Fn(f32, f32) -> f32) -> (Vec<[f32; 3]>, Vec<u32>) {
        let mut v = Vec::new();
        let mut idx = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let b = v.len() as u32;
                let (x, y) = (i as f32, j as f32);
                for (px, py) in [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)] {
                    v.push([px, py, height(px, py)]);
                }
                idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        (v, idx)
    }

    fn area(m: &Welded) -> f32 {
        m.indices
            .chunks_exact(3)
            .map(|t| {
                let p = |i: u32| glam::Vec3::from(m.positions[i as usize]);
                (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
            })
            .sum()
    }

    fn shape(v: Vec<[f32; 3]>, idx: Vec<u32>) -> ShapeInfo {
        let mut scene = crate::parser::parse_wrl(
            b"#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }",
        );
        let mut s = scene.shapes_data.remove(0);
        s.face_count = (idx.len() / 3) as u32;
        s.world_vertices = v;
        s.indices = idx;
        s
    }

    #[test]
    fn weld_merges_repeated_vertices_and_drops_degenerate_triangles() {
        let (v, idx) = grid(3, |_, _| 0.0);
        let w = weld(&v, &idx);
        assert_eq!(w.positions.len(), 16);
        assert_eq!(w.indices.len(), idx.len());
        let w = weld(&[[0.0; 3], [0.0; 3], [1.0, 0.0, 0.0], [-0.0, 0.0, 0.0]], &[0, 1, 2, 0, 2, 3, 0, 2, 9]);
        assert_eq!(w.indices.len(), 0);
    }

    #[test]
    fn flat_grid_is_reduced_keeping_its_outline() {
        let (v, idx) = grid(20, |_, _| 0.0);
        let w = weld(&v, &idx);
        let r = simplify_to_error(&w, 1e-3, true);
        assert!(r.indices.len() < w.indices.len() / 4, "{}", r.indices.len() / 3);
        // Bordi bloccati: tutti gli 80 punti del contorno restano, l'area non cambia
        let border = r.positions.iter().filter(|p| p[0] == 0.0 || p[1] == 0.0 || p[0] == 20.0 || p[1] == 20.0).count();
        assert_eq!(border, 80);
        assert!((area(&r) - 400.0).abs() < 1e-2, "{}", area(&r));
        // Senza bordi bloccati il piano scende a pochi triangoli
        let free = simplify_to_error(&w, 1e-3, false);
        assert!(free.indices.len() < r.indices.len(), "{}", free.indices.len() / 3);
        assert!((area(&free) - 400.0).abs() < 1e-2);
    }

    #[test]
    fn curved_areas_keep_more_triangles_than_flat_ones() {
        // Metà piana, metà ondulata
        let wave = |x: f32, y: f32| if x > 10.0 { ((x - 10.0) * 0.8).sin() * (y * 0.5).cos() } else { 0.0 };
        let (v, idx) = grid(20, wave);
        let w = weld(&v, &idx);
        let r = simplify_to_error(&w, 0.02, false);
        let (mut flat, mut curved) = (0, 0);
        for t in r.indices.chunks_exact(3) {
            let cx: f32 = t.iter().map(|&i| r.positions[i as usize][0]).sum::<f32>() / 3.0;
            if cx < 10.0 { flat += 1 } else { curved += 1 }
        }
        assert!(curved > flat * 5, "piano {flat}, curvo {curved}");
        // L'errore resta sotto il limite: ogni vertice tenuto è sulla superficie originale
        for p in &r.positions {
            assert!((p[2] - wave(p[0], p[1])).abs() < 1e-5);
        }
    }

    #[test]
    fn crease_normals_split_sharp_edges_only() {
        // Due quadrati piegati a 90° lungo lo spigolo x=1
        let p = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, -1.0], [1.0, 1.0, -1.0]];
        let idx = vec![0, 1, 2, 0, 2, 3, 1, 4, 5, 1, 5, 2];
        let (normals, split) = crease_normals(&p, &idx, CREASE_ANGLE);
        let split = split.expect("spigolo vivo");
        assert_eq!(split.extra.len(), 2, "vertici 1 e 2 sdoppiati");
        assert_eq!(normals.len(), 8);
        for (c, &r) in split.indices.iter().enumerate() {
            let expected = if c < 6 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
            let n = glam::Vec3::from(normals[r as usize]);
            assert!(n.dot(glam::Vec3::from(expected)) > 0.999, "angolo {c}: {n:?}");
        }
        // Piano: nessuna copia
        let (v, idx) = grid(2, |_, _| 0.0);
        let w = weld(&v, &idx);
        let (normals, split) = crease_normals(&w.positions, &w.indices, CREASE_ANGLE);
        assert!(split.is_none());
        assert!(normals.iter().all(|n| (n[2] - 1.0).abs() < 1e-6));
    }

    /// Su una superficie curva le normali dei pezzi ridotti sono quelle del file, cioè della
    /// superficie vera, anche dove i triangoli ridotti sono grandi.
    #[test]
    fn reduced_shapes_keep_the_original_normals() {
        let height = |x: f32, y: f32| (x * 0.3).sin() * 2.0 + y * 0.1;
        let (v, idx) = grid(30, height);
        let mut s = shape(v, idx);
        // Normali "del CAD": quelle analitiche della superficie
        s.normals = s
            .world_vertices
            .iter()
            .map(|p| glam::Vec3::new(-(p[0] * 0.3).cos() * 0.6, -0.1, 1.0).normalize().to_array())
            .collect();
        s.split = None;
        let w = weld(&s.world_vertices, &s.indices);
        let r = reduce_shape(&s, &w, 0.05, false).expect("ridotto");
        assert!(r.indices.len() < s.indices.len() / 3);
        for (c, n) in r.corner_normals().iter().enumerate() {
            let p = r.world_vertices[r.indices[c] as usize];
            let exact = glam::Vec3::new(-(p[0] * 0.3).cos() * 0.6, -0.1, 1.0).normalize();
            assert!(glam::Vec3::from(*n).dot(exact) > 0.9999, "angolo {c}: {n:?} invece di {exact:?}");
        }
    }

    #[test]
    fn reduce_many_reaches_the_total_target() {
        let wave = |x: f32, y: f32| (x * 0.4).sin() * (y * 0.3).cos() * 3.0;
        let (v, idx) = grid(40, wave);
        let a = shape(v.clone(), idx.clone());
        let b = shape(v.iter().map(|p| [p[0] + 50.0, p[1], p[2]]).collect(), idx);
        let progress = AtomicU32::new(0);
        let out = reduce_many(&[&a, &b], Params { target: Target::Triangles(2000), lock_border: false }, &progress);
        assert_eq!(out.triangles_before, 6400);
        let total: u64 = out.shapes.iter().map(|s| s.as_ref().unwrap().face_count as u64).sum();
        assert_eq!(total, out.triangles_after);
        assert!((1900..=2100).contains(&total), "{total}");
        assert!(out.max_error > 0.0);
        for s in out.shapes.iter().flatten() {
            assert!(s.rewritten && s.face_count as usize * 3 == s.indices.len());
            assert_eq!(s.normals.len(), s.render_vertex_count());
        }
        assert_eq!(progress.into_inner(), 1000);

        // Obiettivo sopra i triangoli attuali: nulla cambia
        let out = reduce_many(&[&a], Params { target: Target::Triangles(10_000), lock_border: false }, &AtomicU32::new(0));
        assert!(out.shapes[0].is_none());
        assert_eq!(out.triangles_after, 3200);
    }

    /// Prova sul file reale: `GESTORE_WRL_FILE=percorso cargo test --release riduzione_reale -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn riduzione_reale() {
        let path = std::env::var("GESTORE_WRL_FILE").expect("impostare GESTORE_WRL_FILE");
        let data = std::fs::read(&path).unwrap();
        let t = std::time::Instant::now();
        let scene = crate::parser::parse_wrl(&data);
        let total: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
        let bbox = scene.compute_bounding_box();
        println!(
            "Lettura {:?}: {} pezzi, {total} triangoli, ingombro {:?}",
            t.elapsed(),
            scene.shapes_data.len(),
            bbox.max - bbox.min
        );
        let mb = |b: u64| b as f64 / 1e6;
        use crate::exporter::{estimate_size, ExportOptions};
        let size = |scene: &crate::scene_graph::SceneGraph, compact, normals| {
            let t = std::time::Instant::now();
            let s = estimate_size(scene, &crate::exporter::writer::Scope::Visible, ExportOptions { compact, normals, meters: false });
            format!("{:.1} MB ({:?})", mb(s), t.elapsed())
        };
        println!(
            "Export completo: copia fedele {}, compatto {}, compatto senza normali {}",
            size(&scene, false, true),
            size(&scene, true, true),
            size(&scene, true, false)
        );
        if std::env::var("SOLO_DIMENSIONI").is_ok() {
            let refs: Vec<&ShapeInfo> = scene.shapes_data.iter().collect();
            for n in [1_000_000u64, 500_000] {
                let out = reduce_many(&refs, Params { target: Target::Triangles(n), lock_border: false }, &AtomicU32::new(0));
                let mut reduced = crate::parser::parse_wrl(&data);
                for (i, s) in out.shapes.into_iter().enumerate() {
                    if let Some(s) = s {
                        reduced.shapes_data[i] = s;
                    }
                }
                println!(
                    "Ridotto a {} triangoli: compatto {}, senza normali {}",
                    out.triangles_after,
                    size(&reduced, true, true),
                    size(&reduced, true, false)
                );
                if n == 1_000_000 {
                    let path = std::env::temp_dir().join("gestore_wrl_ridotto.wrl");
                    crate::exporter::export_with(&reduced, &crate::exporter::writer::Scope::Visible, &path, ExportOptions { compact: true, normals: false, meters: false }).unwrap();
                    let back = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
                    let tris: usize = back.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
                    println!("Riaperto: {} pezzi, {tris} triangoli, file in {}", back.shapes_data.len(), path.display());
                    let with = std::env::temp_dir().join("gestore_wrl_ridotto_normali.wrl");
                    crate::exporter::export_with(&reduced, &crate::exporter::writer::Scope::Visible, &with, ExportOptions { compact: true, normals: true, meters: false }).unwrap();
                }
            }
            return;
        }
        let refs: Vec<&ShapeInfo> = scene.shapes_data.iter().collect();
        for target in [Target::Triangles(1_000_000), Target::Triangles(500_000), Target::MaxError(0.01)] {
            for lock in [false, true] {
                let t = std::time::Instant::now();
                let out = reduce_many(&refs, Params { target, lock_border: lock }, &AtomicU32::new(0));
                let copies: usize = out.shapes.iter().flatten().map(|s| s.split.as_ref().map_or(0, |x| x.extra.len())).sum();
                println!(
                    "{target:?}, bordi bloccati {lock}: {} triangoli, errore {:.5}, in {:?} ({copies} copie per spigoli vivi)",
                    out.triangles_after,
                    out.max_error,
                    t.elapsed()
                );
            }
        }
    }
}
