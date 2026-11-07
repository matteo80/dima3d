pub mod spatial;
pub mod union_find;

use crate::parser::ShapeInfo;
use crate::scene_graph::{Aabb, MacroComponent};

/// Parametri di configurazione del clustering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterConfig {
    /// Tolleranza per considerare due vertici "coincidenti" (in unità world-space)
    pub tolerance: f32,
    /// Numero minimo di vertici condivisi affinché due Shape siano connesse
    pub min_matching_vertices: usize,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            tolerance: 0.01,
            min_matching_vertices: 5,
        }
    }
}

/// Raggruppa un sottoinsieme di Shape (identificate dal loro `mesh_id`, cioè l'indice
/// in `shapes`) in gruppi geometricamente connessi.
///
/// Stessa logica di `cluster_flat` (vertici coincidenti entro `tolerance`, almeno
/// `min_matching_vertices` corrispondenze), ma la fase di ricerca dei vicini è
/// parallelizzata con rayon: la griglia è in sola lettura e ogni Shape produce
/// le proprie coppie; l'Union-Find finale è sequenziale (O(α(n))).
pub fn cluster_subset(shapes: &[ShapeInfo], mesh_ids: &[u32], config: &ClusterConfig) -> Vec<Vec<u32>> {
    use rayon::prelude::*;

    let n = mesh_ids.len();
    if n == 0 {
        return Vec::new();
    }

    // Fase 1: griglia con l'indice LOCALE (posizione in mesh_ids)
    let mut grid = spatial::SpatialGrid::new(config.tolerance.max(f32::EPSILON));
    for (local, &mesh) in mesh_ids.iter().enumerate() {
        for &v in &shapes[mesh as usize].world_vertices {
            grid.insert(v, local as u32);
        }
    }

    // Fase 2 (parallela): per ogni Shape, conta le corrispondenze con le altre
    let edges: Vec<(u32, u32)> = (0..n)
        .into_par_iter()
        .flat_map_iter(|local| {
            let shape = &shapes[mesh_ids[local] as usize];
            let mut matches: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
            for &v in &shape.world_vertices {
                grid.for_each_neighbor(v, |other| {
                    // Considera ogni coppia una sola volta (other > local)
                    if other as usize > local {
                        *matches.entry(other).or_insert(0) += 1;
                    }
                });
            }
            matches
                .into_iter()
                .filter(|&(_, c)| c >= config.min_matching_vertices)
                .map(move |(other, _)| (local as u32, other))
        })
        .collect();

    // Fase 3: Union-Find
    let mut uf = union_find::UnionFind::new(n);
    for (a, b) in edges {
        uf.union(a, b);
    }

    uf.groups()
        .into_values()
        .map(|members| members.into_iter().map(|l| mesh_ids[l as usize]).collect())
        .collect()
}

/// Raggruppa Shape piatte (senza gerarchia) in MacroComponent connessi geometricamente.
///
/// Algoritmo:
/// 1. Inserisce tutti i vertici world-space in un SpatialGrid
/// 2. Per ogni Shape, cerca Shape vicine tramite query sui vertici
/// 3. Unisce Shape connesse con Union-Find
/// 4. Costruisce i MacroComponent dai gruppi risultanti
///
/// Versione single-thread originale (mantenuta come riferimento/test).
#[allow(dead_code)]
pub fn cluster_flat(shapes: &[ShapeInfo], config: &ClusterConfig) -> Vec<MacroComponent> {
    if shapes.is_empty() {
        return Vec::new();
    }

    let n = shapes.len();
    let mut uf = union_find::UnionFind::new(n);
    let mut grid = spatial::SpatialGrid::new(config.tolerance);

    // Fase 1: inserisci tutti i vertici nella griglia
    for (shape_idx, shape) in shapes.iter().enumerate() {
        for &vertex in &shape.world_vertices {
            grid.insert(vertex, shape_idx as u32);
        }
    }

    // Fase 2: per ogni Shape, cerca vicini e unisci
    for (shape_idx, shape) in shapes.iter().enumerate() {
        let mut matches: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();

        for &vertex in &shape.world_vertices {
            for neighbor_idx in grid.query_neighbors(vertex) {
                if neighbor_idx != shape_idx as u32 {
                    *matches.entry(neighbor_idx).or_insert(0) += 1;
                }
            }
        }

        for (neighbor_idx, count) in matches {
            if count >= config.min_matching_vertices {
                uf.union(shape_idx as u32, neighbor_idx);
            }
        }
    }

    // Fase 3: costruisci i MacroComponent dai gruppi
    let groups = uf.groups();
    let mut macro_components = Vec::with_capacity(groups.len());

    for (comp_id, (_, member_indices)) in groups.iter().enumerate() {
        let mut bbox = Aabb::empty();
        let mut face_count: u32 = 0;
        let mut vertex_count: u32 = 0;

        for &shape_idx in member_indices {
            let shape = &shapes[shape_idx as usize];
            for &v in &shape.world_vertices {
                bbox.extend_point(glam::Vec3::from(v));
            }
            face_count += shape.face_count;
            vertex_count += shape.world_vertices.len() as u32;
        }

        macro_components.push(MacroComponent {
            id: comp_id as u32,
            shape_indices: member_indices.clone(),
            bbox,
            face_count,
            vertex_count,
        });
    }

    // Ordina per numero di facce decrescente (componenti più grandi prima)
    macro_components.sort_by(|a, b| b.face_count.cmp(&a.face_count));

    macro_components
}

#[cfg(test)]
mod tests {
    use super::ClusterConfig;

    /// Due pile di 6 pezzi uguali, ognuno 0,05 più in alto del precedente: con la tolleranza
    /// predefinita (0,01) restano separati, con 0,1 ogni pila diventa un componente. (Un solo
    /// componente con tutto il gruppo non viene creato: non aggiungerebbe nulla all'albero.)
    #[test]
    fn tolerance_from_the_settings_changes_the_grouping() {
        let mut wrl = String::from("#VRML V2.0 utf8\nDEF Assieme Group { children [\n");
        for i in 0..12 {
            let (x, z) = (if i < 6 { 0 } else { 100 }, (i % 6) as f32 * 0.05);
            let (x1, x2, x3) = (x + 1, x + 2, x + 3);
            wrl.push_str(&format!(
                "Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ {x} 0 {z}, {x1} 0 {z}, {x} 1 {z}, {x2} 0 {z}, {x3} 0 {z}, {x2} 1 {z} ] }} coordIndex [ 0, 1, 2, -1, 3, 4, 5, -1 ] }} }}\n"
            ));
        }
        wrl.push_str("] }\n");
        let clusters = |cfg: ClusterConfig| {
            let scene = crate::parser::shape_extractor::parse_wrl_with_config(wrl.as_bytes(), cfg, |_, _| {});
            scene.info.iter().filter(|i| i.kind == crate::scene_graph::NodeKind::Cluster).count()
        };
        assert_eq!(clusters(ClusterConfig::default()), 0);
        assert_eq!(clusters(ClusterConfig { tolerance: 0.1, min_matching_vertices: 5 }), 2);
        // Con 7 vertici in comune richiesti (ogni coppia ne ha al massimo 6) restano separati
        assert_eq!(clusters(ClusterConfig { tolerance: 0.1, min_matching_vertices: 7 }), 0);
    }
}