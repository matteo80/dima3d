use std::collections::HashMap;

/// Union-Find con union by rank e path compression.
/// Identico all'implementazione Python di riferimento.
pub struct UnionFind {
    parent: Vec<u32>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub fn new(size: usize) -> Self {
        Self {
            parent: (0..size as u32).collect(),
            rank: vec![0; size],
        }
    }

    /// Trova il rappresentante di `x` con path compression (iterativo).
    pub fn find(&mut self, mut x: u32) -> u32 {
        // Fase 1: trova la radice
        let mut root = x;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        // Fase 2: path compression
        while self.parent[x as usize] != root {
            let next = self.parent[x as usize];
            self.parent[x as usize] = root;
            x = next;
        }
        root
    }

    /// Unisce i set di `a` e `b`. Ritorna `true` se erano in set diversi.
    pub fn union(&mut self, a: u32, b: u32) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }

        // Union by rank
        match self.rank[ra as usize].cmp(&self.rank[rb as usize]) {
            std::cmp::Ordering::Less => {
                self.parent[ra as usize] = rb;
            }
            std::cmp::Ordering::Greater => {
                self.parent[rb as usize] = ra;
            }
            std::cmp::Ordering::Equal => {
                self.parent[rb as usize] = ra;
                self.rank[ra as usize] += 1;
            }
        }
        true
    }

    /// Ritorna tutti i gruppi come HashMap: rappresentante → lista di membri.
    pub fn groups(&mut self) -> HashMap<u32, Vec<u32>> {
        let n = self.parent.len();
        let mut groups: HashMap<u32, Vec<u32>> = HashMap::new();
        for i in 0..n as u32 {
            let root = self.find(i);
            groups.entry(root).or_default().push(i);
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_union() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(2, 3);
        assert_eq!(uf.find(0), uf.find(1));
        assert_ne!(uf.find(0), uf.find(2));
        assert_eq!(uf.find(2), uf.find(3));
    }

    #[test]
    fn test_groups_count() {
        let mut uf = UnionFind::new(4);
        uf.union(0, 1);
        uf.union(2, 3);
        let groups = uf.groups();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_path_compression() {
        let mut uf = UnionFind::new(6);
        // Catena: 0←1←2←3←4←5
        for i in 0..5u32 {
            uf.parent[(i + 1) as usize] = i;
        }
        // find(5) deve trovare la radice e comprimere il percorso
        let root = uf.find(5);
        assert_eq!(root, 0);
        // Tutti dovrebbero puntare direttamente alla radice dopo path compression
        assert_eq!(uf.parent[5], 0);
    }
}
