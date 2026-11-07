use std::collections::HashMap;

/// Griglia spaziale hash per ricerche di prossimità rapide.
/// Equivalente all'implementazione Python con cella = floor(coord / tolerance).
pub struct SpatialGrid {
    cells: HashMap<[i64; 3], Vec<([f32; 3], u32)>>,
    tolerance: f32,
}

impl SpatialGrid {
    pub fn new(tolerance: f32) -> Self {
        Self {
            cells: HashMap::new(),
            tolerance,
        }
    }

    /// Inserisce un punto con il suo indice Shape.
    pub fn insert(&mut self, point: [f32; 3], shape_idx: u32) {
        let cell = self.cell_of(point);
        self.cells.entry(cell).or_default().push((point, shape_idx));
    }

    /// Ritorna tutti gli `shape_idx` che hanno un vertice entro `tolerance` da `point`.
    /// Cerca nelle 27 celle adiacenti (3×3×3 neighborhood).
    #[allow(dead_code)]
    pub fn query_neighbors(&self, point: [f32; 3]) -> Vec<u32> {
        let [cx, cy, cz] = self.cell_of(point);
        let tol_sq = self.tolerance * self.tolerance;
        let mut results = Vec::new();

        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    let cell = [cx + dx, cy + dy, cz + dz];
                    if let Some(entries) = self.cells.get(&cell) {
                        for &(p, idx) in entries {
                            let dist_sq = (p[0] - point[0]).powi(2)
                                + (p[1] - point[1]).powi(2)
                                + (p[2] - point[2]).powi(2);
                            if dist_sq <= tol_sq {
                                results.push(idx);
                            }
                        }
                    }
                }
            }
        }

        results
    }

    /// Come `query_neighbors`, ma senza allocare: invoca `f` per ogni vertice vicino.
    pub fn for_each_neighbor(&self, point: [f32; 3], mut f: impl FnMut(u32)) {
        let [cx, cy, cz] = self.cell_of(point);
        let tol_sq = self.tolerance * self.tolerance;
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    if let Some(entries) = self.cells.get(&[cx + dx, cy + dy, cz + dz]) {
                        for &(p, idx) in entries {
                            let d0 = p[0] - point[0];
                            let d1 = p[1] - point[1];
                            let d2 = p[2] - point[2];
                            if d0 * d0 + d1 * d1 + d2 * d2 <= tol_sq {
                                f(idx);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Calcola la cella della griglia per un punto dato.
    fn cell_of(&self, point: [f32; 3]) -> [i64; 3] {
        [
            (point[0] / self.tolerance).floor() as i64,
            (point[1] / self.tolerance).floor() as i64,
            (point[2] / self.tolerance).floor() as i64,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_query() {
        let mut grid = SpatialGrid::new(0.1);
        grid.insert([0.0, 0.0, 0.0], 0);
        grid.insert([0.05, 0.05, 0.05], 1);
        grid.insert([10.0, 10.0, 10.0], 2);

        let neighbors = grid.query_neighbors([0.0, 0.0, 0.0]);
        assert!(neighbors.contains(&0));
        assert!(neighbors.contains(&1));
        assert!(!neighbors.contains(&2));
    }

    #[test]
    fn test_no_false_positives_beyond_tolerance() {
        let mut grid = SpatialGrid::new(0.1);
        grid.insert([0.0, 0.0, 0.0], 0);
        grid.insert([0.2, 0.0, 0.0], 1);

        let neighbors = grid.query_neighbors([0.0, 0.0, 0.0]);
        assert!(neighbors.contains(&0));
        assert!(!neighbors.contains(&1));
    }
}
