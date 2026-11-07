//! Ricostruzione della gerarchia della scena a partire dai percorsi dei contenitori
//! raccolti dal parser, più auto-clustering dei contenitori "piatti".
//!
//! Il modulo è volutamente puro (solo `std`): riceve i percorsi e una funzione di
//! clustering, e produce un'arena di nodi già ordinata in pre-order.
//! `SceneGraph::from_built()` la converte poi nei `SceneNode` definitivi.

use std::collections::HashMap;

use crate::parser::{ContainerKind, PathSeg};

/// Numero minimo di Shape figlie dirette di uno stesso contenitore oltre il quale
/// si tenta il raggruppamento geometrico in macro-componenti.
pub const CLUSTER_MIN_SHAPES: usize = 10;

/// Se dopo il clustering restano più di questi pezzi isolati, vengono
/// raccolti in un unico gruppo "Pezzi singoli" per non inondare l'albero.
pub const SINGLETON_WRAP_MIN: usize = 30;

/// Tipo di nodo prodotto dal builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltKind {
    Group,
    Transform,
    /// Istanza `USE` di un contenitore DEF
    Use,
    /// Macro-componente sintetico generato dal clustering geometrico
    Cluster,
    /// Raccolta sintetica dei pezzi rimasti isolati dopo il clustering
    Singles,
    Shape { mesh_id: u32 },
}

impl BuiltKind {
    pub fn is_shape(self) -> bool {
        matches!(self, BuiltKind::Shape { .. })
    }
}

impl From<ContainerKind> for BuiltKind {
    fn from(k: ContainerKind) -> Self {
        match k {
            ContainerKind::Group => BuiltKind::Group,
            ContainerKind::Transform => BuiltKind::Transform,
            ContainerKind::Use => BuiltKind::Use,
        }
    }
}

/// Nodo dell'arena prodotta dal builder.
#[derive(Debug, Clone)]
pub struct BuiltNode {
    pub name: Option<String>,
    pub kind: BuiltKind,
    pub children: Vec<u32>,
    /// Indice del padre, `NO_PARENT` per le radici
    pub parent: u32,
}

pub const NO_PARENT: u32 = u32::MAX;

/// Risultato della costruzione: arena in pre-order + indici delle radici.
#[derive(Debug, Default)]
pub struct BuiltTree {
    pub nodes: Vec<BuiltNode>,
    pub roots: Vec<u32>,
}

/// Albero temporaneo (ricorsivo) usato durante la costruzione.
struct TNode {
    name: Option<String>,
    kind: BuiltKind,
    children: Vec<TNode>,
    /// key (offset sorgente) → posizione nel vettore `children`
    index: HashMap<usize, usize>,
}

impl TNode {
    fn container(name: Option<String>, kind: BuiltKind) -> Self {
        Self { name, kind, children: Vec::new(), index: HashMap::new() }
    }
    fn shape(name: Option<String>, mesh_id: u32) -> Self {
        Self { name, kind: BuiltKind::Shape { mesh_id }, children: Vec::new(), index: HashMap::new() }
    }
}

/// Costruisce l'albero della scena.
///
/// * `shapes` — per ogni mesh (nell'ordine di `mesh_id`): percorso dei contenitori e nome DEF.
/// * `cluster_fn` — dato un insieme di `mesh_id`, ritorna i gruppi geometricamente connessi.
pub fn build_tree<'a, I, F>(shapes: I, mut cluster_fn: F) -> BuiltTree
where
    I: IntoIterator<Item = (&'a [PathSeg], Option<&'a str>)>,
    F: FnMut(&[u32]) -> Vec<Vec<u32>>,
{
    // ── 1. Trie dei percorsi ────────────────────────────────────────────────
    let mut root = TNode::container(None, BuiltKind::Group);
    for (mesh_id, (path, name)) in shapes.into_iter().enumerate() {
        let mut cur = &mut root;
        for seg in path {
            let pos = match cur.index.get(&seg.key) {
                Some(&p) => p,
                None => {
                    cur.children.push(TNode::container(seg.name.clone(), seg.kind.into()));
                    let p = cur.children.len() - 1;
                    cur.index.insert(seg.key, p);
                    p
                }
            };
            cur = &mut cur.children[pos];
        }
        cur.children.push(TNode::shape(name.map(str::to_string), mesh_id as u32));
    }

    // ── 2. Semplificazione catene di contenitori anonimi ───────────────────
    for child in &mut root.children {
        simplify(child);
    }
    // Una radice anonima con un solo figlio Shape diventa la Shape stessa
    for child in &mut root.children {
        hoist_single_shape(child);
    }

    // ── 3. Auto-clustering dei contenitori con molte Shape dirette ─────────
    let mut cluster_counter = 0u32;
    cluster_recursive(&mut root, true, &mut cluster_fn, &mut cluster_counter);

    // ── 4. Arena in pre-order ───────────────────────────────────────────────
    let mut out = BuiltTree::default();
    for child in root.children {
        let idx = flatten(child, NO_PARENT, &mut out.nodes);
        out.roots.push(idx);
    }
    out
}

/// Unisce un contenitore con il suo unico figlio contenitore quando almeno uno
/// dei due è anonimo (tipico delle esportazioni CAD: Transform→Group→Transform).
/// Se uno dei due è un USE, il nodo risultante resta un'istanza USE.
fn simplify(node: &mut TNode) {
    if node.kind.is_shape() {
        return;
    }
    loop {
        if node.children.len() != 1 {
            break;
        }
        let only = &node.children[0];
        if only.kind.is_shape() {
            break;
        }
        let parent_anon = node.name.is_none();
        let child_anon = only.name.is_none();
        if !(parent_anon || child_anon) {
            break;
        }
        let mut only = node.children.pop().unwrap();
        // Mantieni il nome disponibile (preferisci quello del padre se c'è)
        let name = node.name.take().or(only.name.take());
        // Il tipo più "informativo": Use > Transform > Group
        let kind = if node.kind == BuiltKind::Use || only.kind == BuiltKind::Use {
            BuiltKind::Use
        } else if node.kind == BuiltKind::Transform || only.kind == BuiltKind::Transform {
            BuiltKind::Transform
        } else {
            BuiltKind::Group
        };
        node.name = name;
        node.kind = kind;
        node.children = std::mem::take(&mut only.children);
        node.index.clear();
    }
    for child in &mut node.children {
        simplify(child);
        hoist_single_shape(child);
    }
}

/// Un contenitore anonimo (non USE) che contiene una sola Shape viene sostituito dalla Shape.
fn hoist_single_shape(node: &mut TNode) {
    if node.kind.is_shape() || node.kind == BuiltKind::Use || node.name.is_some() {
        return;
    }
    if node.children.len() == 1 && node.children[0].kind.is_shape() {
        let only = node.children.pop().unwrap();
        *node = only;
    }
}

fn cluster_recursive<F>(node: &mut TNode, is_root: bool, cluster_fn: &mut F, counter: &mut u32)
where
    F: FnMut(&[u32]) -> Vec<Vec<u32>>,
{
    if node.kind.is_shape() {
        return;
    }
    // Prima i figli (i cluster creati qui non vanno ri-clusterizzati)
    for child in &mut node.children {
        cluster_recursive(child, false, cluster_fn, counter);
    }

    let shape_count = node.children.iter().filter(|c| c.kind.is_shape()).count();
    if shape_count < CLUSTER_MIN_SHAPES {
        return;
    }

    // Separa Shape dirette e altri contenitori
    let old_children = std::mem::take(&mut node.children);
    let mut containers = Vec::new();
    let mut shapes_by_mesh: HashMap<u32, TNode> = HashMap::with_capacity(shape_count);
    let mut mesh_order: Vec<u32> = Vec::with_capacity(shape_count);
    for c in old_children {
        match c.kind {
            BuiltKind::Shape { mesh_id } => {
                mesh_order.push(mesh_id);
                shapes_by_mesh.insert(mesh_id, c);
            }
            _ => containers.push(c),
        }
    }

    let mut groups = cluster_fn(&mesh_order);
    groups.retain(|g| !g.is_empty());
    let multi = groups.iter().filter(|g| g.len() >= 2).count();
    let all_in_one = groups.len() == 1;

    // Nessun gruppo significativo: ripristina l'ordine originale
    let useless = multi == 0 || (all_in_one && !is_root && containers.is_empty());
    if useless {
        node.children = containers;
        for m in mesh_order {
            if let Some(s) = shapes_by_mesh.remove(&m) {
                node.children.push(s);
            }
        }
        node.index.clear();
        return;
    }

    // Gruppi più grandi prima; a parità, ordine di apparizione nel file
    for g in &mut groups {
        g.sort_unstable();
    }
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));

    let mut new_children = containers;
    let mut singles = Vec::new();
    for g in groups {
        if g.len() == 1 {
            if let Some(s) = shapes_by_mesh.remove(&g[0]) {
                singles.push(s);
            }
            continue;
        }
        *counter += 1;
        let mut cluster = TNode::container(Some(format!("Componente {}", *counter)), BuiltKind::Cluster);
        for m in g {
            if let Some(s) = shapes_by_mesh.remove(&m) {
                cluster.children.push(s);
            }
        }
        new_children.push(cluster);
    }
    // Shape eventualmente non restituite dalla funzione di clustering
    for m in mesh_order {
        if let Some(s) = shapes_by_mesh.remove(&m) {
            singles.push(s);
        }
    }
    if singles.len() >= SINGLETON_WRAP_MIN {
        let mut wrap = TNode::container(Some("Pezzi singoli".to_string()), BuiltKind::Singles);
        wrap.children = singles;
        new_children.push(wrap);
    } else {
        new_children.extend(singles);
    }
    node.children = new_children;
    node.index.clear();
}

fn flatten(node: TNode, parent: u32, out: &mut Vec<BuiltNode>) -> u32 {
    let idx = out.len() as u32;
    out.push(BuiltNode { name: node.name, kind: node.kind, children: Vec::new(), parent });
    let mut child_ids = Vec::with_capacity(node.children.len());
    for c in node.children {
        child_ids.push(flatten(c, idx, out));
    }
    out[idx as usize].children = child_ids;
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(key: usize, name: Option<&str>, kind: ContainerKind) -> PathSeg {
        PathSeg { key, name: name.map(str::to_string), kind }
    }

    fn no_cluster(ids: &[u32]) -> Vec<Vec<u32>> {
        ids.iter().map(|&i| vec![i]).collect()
    }

    fn label(t: &BuiltTree, i: u32) -> String {
        let n = &t.nodes[i as usize];
        match n.kind {
            BuiltKind::Shape { mesh_id } => format!("S{mesh_id}"),
            k => format!("{:?}:{}", k, n.name.clone().unwrap_or_default()),
        }
    }

    #[test]
    fn hierarchy_is_preserved() {
        let a = seg(10, Some("Assieme"), ContainerKind::Transform);
        let p1 = seg(20, Some("Pezzo1"), ContainerKind::Transform);
        let p2 = seg(30, Some("Pezzo2"), ContainerKind::Transform);
        let paths = vec![
            vec![a.clone(), p1.clone()],
            vec![a.clone(), p1.clone()],
            vec![a.clone(), p2.clone()],
        ];
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), None)), no_cluster);
        assert_eq!(t.roots.len(), 1);
        let root = t.roots[0];
        assert_eq!(label(&t, root), "Transform:Assieme");
        let kids = &t.nodes[root as usize].children;
        assert_eq!(kids.len(), 2);
        assert_eq!(label(&t, kids[0]), "Transform:Pezzo1");
        assert_eq!(t.nodes[kids[0] as usize].children.len(), 2);
        assert_eq!(t.nodes[kids[0] as usize].parent, root);
    }

    #[test]
    fn anonymous_chains_collapse() {
        // Transform anonimo → Group "Motore" → 2 shape  ==> "Motore" con 2 shape
        let paths = vec![
            vec![seg(1, None, ContainerKind::Transform), seg(2, Some("Motore"), ContainerKind::Group)],
            vec![seg(1, None, ContainerKind::Transform), seg(2, Some("Motore"), ContainerKind::Group)],
        ];
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), None)), no_cluster);
        assert_eq!(t.roots.len(), 1);
        assert_eq!(label(&t, t.roots[0]), "Transform:Motore");
        assert_eq!(t.nodes[t.roots[0] as usize].children.len(), 2);
    }

    #[test]
    fn single_shape_wrapper_is_hoisted() {
        let paths = vec![vec![seg(1, None, ContainerKind::Transform)]];
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), Some("Vite"))), no_cluster);
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(label(&t, t.roots[0]), "S0");
        assert_eq!(t.nodes[0].name.as_deref(), Some("Vite"));
    }

    #[test]
    fn use_instances_are_distinct() {
        let paths = vec![
            vec![seg(1, None, ContainerKind::Transform), seg(100, Some("Ruota"), ContainerKind::Use)],
            vec![seg(2, None, ContainerKind::Transform), seg(200, Some("Ruota"), ContainerKind::Use)],
        ];
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), None)), no_cluster);
        assert_eq!(t.roots.len(), 2);
        assert_eq!(label(&t, t.roots[0]), "Use:Ruota");
    }

    #[test]
    fn flat_file_is_clustered() {
        // 40 shape piatte: 0..20 connesse, 20..25 connesse, 25..40 isolate
        let paths: Vec<Vec<PathSeg>> = (0..40).map(|_| Vec::new()).collect();
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), None)), |ids: &[u32]| {
            let mut g: Vec<Vec<u32>> = vec![
                ids.iter().copied().filter(|&i| i < 20).collect(),
                ids.iter().copied().filter(|&i| (20..25).contains(&i)).collect(),
            ];
            g.extend(ids.iter().copied().filter(|&i| i >= 25).map(|i| vec![i]));
            g
        });
        // 2 componenti + 15 singoli (sotto soglia → restano in radice)
        assert_eq!(t.roots.len(), 2 + 15);
        assert_eq!(label(&t, t.roots[0]), "Cluster:Componente 1");
        assert_eq!(t.nodes[t.roots[0] as usize].children.len(), 20);
        assert_eq!(label(&t, t.roots[1]), "Cluster:Componente 2");
        assert_eq!(t.nodes[t.roots[1] as usize].children.len(), 5);
        // ogni mesh compare esattamente una volta
        let mut seen: Vec<u32> = t.nodes.iter().filter_map(|n| match n.kind {
            BuiltKind::Shape { mesh_id } => Some(mesh_id), _ => None }).collect();
        seen.sort();
        assert_eq!(seen, (0..40).collect::<Vec<_>>());
    }

    #[test]
    fn many_singletons_are_wrapped() {
        let paths: Vec<Vec<PathSeg>> = (0..50).map(|_| Vec::new()).collect();
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), None)), |ids: &[u32]| {
            let mut g = vec![vec![0, 1, 2]];
            g.extend(ids.iter().copied().filter(|&i| i > 2).map(|i| vec![i]));
            g
        });
        assert_eq!(t.roots.len(), 2);
        assert_eq!(label(&t, t.roots[1]), "Singles:Pezzi singoli");
        assert_eq!(t.nodes[t.roots[1] as usize].children.len(), 47);
    }

    #[test]
    fn no_useful_clusters_keeps_order() {
        let paths: Vec<Vec<PathSeg>> = (0..12).map(|_| vec![seg(1, Some("Viti"), ContainerKind::Group)]).collect();
        let t = build_tree(paths.iter().map(|p| (p.as_slice(), None)), no_cluster);
        let kids = &t.nodes[t.roots[0] as usize].children;
        assert_eq!(kids.len(), 12);
        assert_eq!(label(&t, kids[0]), "S0");
        assert_eq!(label(&t, kids[11]), "S11");
    }
}
