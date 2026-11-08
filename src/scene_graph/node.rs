use std::sync::Arc;
use crate::scene_graph::Aabb;

/// Wrapper per i dati sorgente del WRL: supporta sia buffer in memoria che memory-mapping.
#[derive(Clone)]
pub enum SourceData {
    Empty,
    Memory(Vec<u8>),
    Mmap(Arc<memmap2::Mmap>),
}

impl std::fmt::Debug for SourceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceData::Empty => write!(f, "SourceData::Empty"),
            SourceData::Memory(v) => write!(f, "SourceData::Memory({} bytes)", v.len()),
            SourceData::Mmap(m) => write!(f, "SourceData::Mmap({} bytes)", m.len()),
        }
    }
}

impl Default for SourceData {
    fn default() -> Self {
        SourceData::Empty
    }
}

impl std::ops::Deref for SourceData {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        match self {
            SourceData::Empty => &[],
            SourceData::Memory(v) => v.as_slice(),
            SourceData::Mmap(m) => &m[..],
        }
    }
}

impl From<Vec<u8>> for SourceData {
    fn from(v: Vec<u8>) -> Self {
        SourceData::Memory(v)
    }
}

impl From<&[u8]> for SourceData {
    fn from(s: &[u8]) -> Self {
        SourceData::Memory(s.to_vec())
    }
}

impl From<Arc<memmap2::Mmap>> for SourceData {
    fn from(m: Arc<memmap2::Mmap>) -> Self {
        SourceData::Mmap(m)
    }
}

/// Identificatore univoco di un mesh caricato sulla GPU.
pub type MeshId = u32;

/// Nodo del grafo di scena. Usa un arena allocator (Vec<SceneNode> in SceneGraph).
#[derive(Debug, Clone)]
pub enum SceneNode {
    Group {
        name: Option<String>,
        children: Vec<u32>,
        visible: bool,
        selected: bool,
    },
    Transform {
        name: Option<String>,
        matrix: glam::Mat4,
        children: Vec<u32>,
        visible: bool,
        selected: bool,
    },
    Shape {
        name: Option<String>,
        mesh_id: MeshId,
        /// Offset byte nel file sorgente: inizio del nodo Shape
        source_start: usize,
        /// Offset byte nel file sorgente: fine del nodo Shape
        source_end: usize,
        /// Offset relativo da source_start: inizio del blocco `point [...]`
        point_range_start: usize,
        /// Offset relativo da source_start: fine del blocco `point [...]`
        point_range_end: usize,
        visible: bool,
        selected: bool,
    },
}

impl SceneNode {
    pub fn is_visible(&self) -> bool {
        match self {
            SceneNode::Group { visible, .. } => *visible,
            SceneNode::Transform { visible, .. } => *visible,
            SceneNode::Shape { visible, .. } => *visible,
        }
    }

    pub fn set_visible(&mut self, v: bool) {
        match self {
            SceneNode::Group { visible, .. } => *visible = v,
            SceneNode::Transform { visible, .. } => *visible = v,
            SceneNode::Shape { visible, .. } => *visible = v,
        }
    }

    pub fn is_selected(&self) -> bool {
        match self {
            SceneNode::Group { selected, .. } => *selected,
            SceneNode::Transform { selected, .. } => *selected,
            SceneNode::Shape { selected, .. } => *selected,
        }
    }

    pub fn set_selected(&mut self, v: bool) {
        match self {
            SceneNode::Group { selected, .. } => *selected = v,
            SceneNode::Transform { selected, .. } => *selected = v,
            SceneNode::Shape { selected, .. } => *selected = v,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            SceneNode::Group { name, .. } => name.as_deref(),
            SceneNode::Transform { name, .. } => name.as_deref(),
            SceneNode::Shape { name, .. } => name.as_deref(),
        }
    }

    pub fn children(&self) -> &[u32] {
        match self {
            SceneNode::Group { children, .. } => children,
            SceneNode::Transform { children, .. } => children,
            SceneNode::Shape { .. } => &[],
        }
    }
}

// ─── Metadati per nodo ────────────────────────────────────────────────────────

/// Indice "nessun padre" per i nodi radice.
pub const NO_PARENT: u32 = u32::MAX;

/// Ruolo del nodo nell'albero (serve alla UI per icone ed etichette).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Group,
    Transform,
    /// Istanza `USE` di un contenitore DEF
    Use,
    /// Macro-componente generato dal clustering geometrico
    Cluster,
    /// Raccolta dei pezzi isolati dopo il clustering
    Singles,
    Shape,
    /// Radice di un file caricato (una per matematica aperta)
    File,
}

/// Un file WRL caricato nella scena.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Nome mostrato (nome del file)
    pub name: String,
    pub path: Option<std::path::PathBuf>,
    /// Testo del file (o la conversione VRML 1.0 → 2.0), per l'export
    pub data: SourceData,
    /// Mesh del file: i mesh_id sono contigui
    pub meshes: std::ops::Range<u32>,
    /// Nodo radice del file
    pub node: u32,
    /// Fattore di scala applicato al file per portarlo nelle unità della scena (1 = come nel file)
    pub scale: f32,
    /// Il formato dichiara le sue unità e il lettore le ha già portate in millimetri (HSF): non
    /// serve indovinarle dalle dimensioni
    pub units_mm: bool,
    /// File STEP: come è stato tassellato (per "Converti in mesh…" dall'albero)
    pub step: Option<crate::settings::StepLoad>,
}

/// Informazioni accessorie, allineate 1:1 con `SceneGraph::nodes`.
#[derive(Debug, Clone, Copy)]
pub struct NodeInfo {
    pub parent: u32,
    pub kind: NodeKind,
    /// Numero di Shape (non eliminate) nel sotto-albero
    pub shape_count: u32,
    pub deleted: bool,
    /// Colore impostato dall'utente (RGBA sRGB), ereditato da tutto il sotto-albero.
    /// `None` = colore originale del file.
    pub color: Option<[f32; 4]>,
}

// ─── SceneGraph ───────────────────────────────────────────────────────────────

/// Grafo di scena con arena allocator.
/// I nodi sono indicizzati da `u32` (indice nel Vec `nodes`).
#[derive(Debug, Default)]
pub struct SceneGraph {
    /// Arena di tutti i nodi
    pub nodes: Vec<SceneNode>,
    /// Metadati (padre, tipo, conteggio pezzi) allineati a `nodes`
    pub info: Vec<NodeInfo>,
    /// Indici dei nodi radice
    pub roots: Vec<u32>,
    /// Macro-componenti (risultato del clustering)
    pub macro_components: Vec<MacroComponent>,
    /// Dati sorgente del file appena letto dal parser. `into_file` li sposta in `sources`.
    pub source_data: SourceData,
    /// Unità del file appena letto già in millimetri (vedi `SourceFile::units_mm`)
    pub units_mm: bool,
    /// File caricati, ognuno con la sua radice nell'albero (vuoto per una scena appena letta
    /// dal parser e non ancora trasformata con `into_file`)
    pub sources: Vec<SourceFile>,
    /// Dati geometrici di ogni Shape caricata (mesh_id indicizza questo vettore)
    pub shapes_data: Vec<crate::parser::ShapeInfo>,
    /// Bounding box world-space di ogni mesh (allineato a `shapes_data`)
    pub mesh_bboxes: Vec<Aabb>,
    /// mesh_id → indice del nodo Shape nell'arena
    pub mesh_to_node: Vec<u32>,
    /// Parti del file che esistono ma non sono state caricate, da mostrare all'utente
    pub load_warnings: Vec<String>,
    /// Incrementato a ogni modifica di visibilità/struttura: il renderer
    /// risincronizza i propri flag solo quando cambia.
    pub revision: u64,
}

impl SceneGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Costruisce l'arena definitiva a partire dall'albero prodotto dal builder.
    pub fn from_built(
        tree: crate::scene_graph::builder::BuiltTree,
        shapes_data: Vec<crate::parser::ShapeInfo>,
    ) -> Self {
        use crate::scene_graph::builder::BuiltKind;
        use rayon::prelude::*;

        let mut scene = SceneGraph::new();
        scene.nodes.reserve(tree.nodes.len());
        scene.info.reserve(tree.nodes.len());
        scene.mesh_to_node = vec![NO_PARENT; shapes_data.len()];

        for (idx, b) in tree.nodes.into_iter().enumerate() {
            let (node, kind) = match b.kind {
                BuiltKind::Shape { mesh_id } => {
                    let s = &shapes_data[mesh_id as usize];
                    scene.mesh_to_node[mesh_id as usize] = idx as u32;
                    (
                        SceneNode::Shape {
                            name: b.name,
                            mesh_id,
                            source_start: s.source_start,
                            source_end: s.source_end,
                            point_range_start: s.point_range_start,
                            point_range_end: s.point_range_end,
                            visible: true,
                            selected: false,
                        },
                        NodeKind::Shape,
                    )
                }
                BuiltKind::Transform => (
                    SceneNode::Transform {
                        name: b.name,
                        // I vertici sono già in world-space: la matrice è informativa
                        matrix: glam::Mat4::IDENTITY,
                        children: b.children,
                        visible: true,
                        selected: false,
                    },
                    NodeKind::Transform,
                ),
                other => (
                    SceneNode::Group { name: b.name, children: b.children, visible: true, selected: false },
                    match other {
                        BuiltKind::Use => NodeKind::Use,
                        BuiltKind::Cluster => NodeKind::Cluster,
                        BuiltKind::Singles => NodeKind::Singles,
                        _ => NodeKind::Group,
                    },
                ),
            };
            scene.nodes.push(node);
            scene.info.push(NodeInfo { parent: b.parent, kind, shape_count: 0, deleted: false, color: None });
        }
        scene.roots = tree.roots;

        scene.mesh_bboxes = shapes_data
            .par_iter()
            .map(|s| {
                let mut bb = Aabb::empty();
                for &v in &s.world_vertices {
                    bb.extend_point(glam::Vec3::from(v));
                }
                bb
            })
            .collect();
        scene.shapes_data = shapes_data;
        scene.recompute_shape_counts();

        // MacroComponent per i cluster (statistiche e compatibilità)
        for idx in 0..scene.nodes.len() as u32 {
            if scene.info[idx as usize].kind != NodeKind::Cluster {
                continue;
            }
            let shape_indices: Vec<u32> = scene.nodes[idx as usize].children().to_vec();
            let mut bbox = Aabb::empty();
            let mut face_count = 0;
            let mut vertex_count = 0;
            for &si in &shape_indices {
                if let SceneNode::Shape { mesh_id, .. } = scene.nodes[si as usize] {
                    let s = &scene.shapes_data[mesh_id as usize];
                    bbox.extend_aabb(&scene.mesh_bboxes[mesh_id as usize]);
                    face_count += s.face_count;
                    vertex_count += s.world_vertices.len() as u32;
                }
            }
            scene.macro_components.push(MacroComponent {
                id: scene.macro_components.len() as u32,
                shape_indices,
                bbox,
                face_count,
                vertex_count,
            });
        }
        scene
    }

    /// Rende la scena appena letta un "file": un nodo radice con il nome del file sopra le radici
    /// esistenti, e il testo sorgente in `sources`. Gli indici dei nodi crescono di 1 (l'arena resta
    /// in pre-order).
    pub fn into_file(&mut self, name: String, path: Option<std::path::PathBuf>) {
        let shift = |i: &mut u32| {
            if *i != NO_PARENT {
                *i += 1;
            }
        };
        for node in &mut self.nodes {
            if let SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } = node {
                children.iter_mut().for_each(shift);
            }
        }
        for inf in &mut self.info {
            inf.parent = if inf.parent == NO_PARENT { 0 } else { inf.parent + 1 };
        }
        self.mesh_to_node.iter_mut().for_each(shift);
        for mc in &mut self.macro_components {
            mc.shape_indices.iter_mut().for_each(shift);
        }
        let children: Vec<u32> = self.roots.iter().map(|r| r + 1).collect();
        let shape_count = children.iter().map(|&c| self.info[c as usize - 1].shape_count).sum();
        self.nodes.insert(
            0,
            SceneNode::Group { name: Some(name.clone()), children, visible: true, selected: false },
        );
        self.info.insert(
            0,
            NodeInfo { parent: NO_PARENT, kind: NodeKind::File, shape_count, deleted: false, color: None },
        );
        self.roots = vec![0];
        self.sources = vec![SourceFile {
            name,
            path,
            data: std::mem::take(&mut self.source_data),
            meshes: 0..self.shapes_data.len() as u32,
            node: 0,
            scale: 1.0,
            units_mm: self.units_mm,
            step: None,
        }];
        self.revision += 1;
    }

    /// Aggiunge in coda i file di un'altra scena (già passata da `into_file`). Nodi e mesh esistenti
    /// mantengono i loro indici; quelli nuovi vengono spostati dopo.
    pub fn append(&mut self, mut other: SceneGraph) {
        let node_shift = self.nodes.len() as u32;
        let mesh_shift = self.shapes_data.len() as u32;
        let shift = |i: &mut u32| {
            if *i != NO_PARENT {
                *i += node_shift;
            }
        };
        for node in &mut other.nodes {
            match node {
                SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                    children.iter_mut().for_each(shift)
                }
                SceneNode::Shape { mesh_id, .. } => *mesh_id += mesh_shift,
            }
        }
        for inf in &mut other.info {
            shift(&mut inf.parent);
        }
        other.mesh_to_node.iter_mut().for_each(shift);
        for mc in &mut other.macro_components {
            mc.shape_indices.iter_mut().for_each(shift);
            mc.id += self.macro_components.len() as u32;
        }
        for s in &mut other.sources {
            s.node += node_shift;
            s.meshes = s.meshes.start + mesh_shift..s.meshes.end + mesh_shift;
        }
        self.roots.extend(other.roots.iter().map(|r| r + node_shift));
        self.nodes.append(&mut other.nodes);
        self.info.append(&mut other.info);
        self.mesh_to_node.append(&mut other.mesh_to_node);
        self.macro_components.append(&mut other.macro_components);
        self.shapes_data.append(&mut other.shapes_data);
        self.mesh_bboxes.append(&mut other.mesh_bboxes);
        self.sources.append(&mut other.sources);
        self.load_warnings.append(&mut other.load_warnings);
        self.revision += 1;
    }

    /// Indice in `sources` del file che contiene la mesh (None per una scena senza file).
    pub fn source_index(&self, mesh_id: u32) -> Option<usize> {
        let i = self.sources.partition_point(|s| s.meshes.end <= mesh_id);
        self.sources.get(i).filter(|s| s.meshes.contains(&mesh_id)).map(|_| i)
    }

    /// Testo sorgente della mesh: quello del suo file, o `source_data` se la scena non ha file.
    pub fn source_for_mesh(&self, mesh_id: u32) -> &[u8] {
        match self.source_index(mesh_id) {
            Some(i) => &self.sources[i].data,
            None => &self.source_data,
        }
    }

    /// Scala la geometria di un file (es. 0.001 per passare da millimetri a metri). La scala entra
    /// nella matrice di ogni Shape: l'export scrive quindi le coordinate già convertite.
    pub fn scale_file(&mut self, source: usize, factor: f32) {
        let Some(file) = self.sources.get_mut(source) else { return };
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        file.scale *= factor;
        let range = file.meshes.start as usize..file.meshes.end as usize;
        let scale = glam::Mat4::from_scale(glam::Vec3::splat(factor));
        for s in &mut self.shapes_data[range.clone()] {
            for v in &mut s.world_vertices {
                *v = (glam::Vec3::from(*v) * factor).into();
            }
            s.matrix = scale * s.matrix;
        }
        for b in &mut self.mesh_bboxes[range] {
            if b.is_valid() {
                b.min *= factor;
                b.max *= factor;
            }
        }
        self.revision += 1;
    }

    /// Ingombro delle mesh di un file.
    pub fn file_bounding_box(&self, source: usize) -> Aabb {
        let mut bbox = Aabb::empty();
        if let Some(file) = self.sources.get(source) {
            for b in &self.mesh_bboxes[file.meshes.start as usize..file.meshes.end as usize] {
                if b.is_valid() {
                    bbox.extend_aabb(b);
                }
            }
        }
        bbox
    }

    /// Fattore (0.001 o 1000) che porterebbe un file nelle unità del file di riferimento (il primo
    /// aperto; per il primo, il secondo), se il file sembra scritto in altre unità. Il riferimento è
    /// un solo file: altri file aggiunti e non ancora convertiti non devono falsare il confronto.
    /// Convertito, il file deve cadere dentro l'ingombro del riferimento con una grandezza
    /// plausibile (da 1/300 a 1,5 volte la sua); in più, così com'è,
    /// deve essere fuori posto (es. sportello in millimetri lontano da una scocca in metri) o di
    /// grandezza assurda (es. scocca in metri dentro una in millimetri: 1000 volte più piccola).
    /// Se le unità di entrambi i file sono note (es. due HSF), non c'è niente da indovinare.
    pub fn suggest_unit_scale(&self, source: usize) -> Option<f32> {
        let file = self.file_bounding_box(source);
        let (reference, others) = self
            .sources
            .iter()
            .enumerate()
            .filter(|(i, s)| *i != source && self.info.get(s.node as usize).is_some_and(|inf| !inf.deleted))
            .map(|(i, _)| (i, self.file_bounding_box(i)))
            .find(|(_, b)| b.is_valid())
            .unwrap_or((usize::MAX, Aabb::empty()));
        if !file.is_valid() || !others.is_valid() {
            return None;
        }
        if self.sources[source].units_mm && self.sources[reference].units_mm {
            return None;
        }
        let (size, reference) = (file.diagonal(), others.diagonal());
        if !(size > 0.0 && reference > 0.0) {
            return None;
        }
        let margin = glam::Vec3::splat(reference * 0.05);
        let inside = |p: glam::Vec3| p.cmpge(others.min - margin).all() && p.cmple(others.max + margin).all();
        let plausible_size = |s: f32| (reference / 300.0..=reference * 1.5).contains(&s);
        let misplaced = !inside(file.center());
        let absurd_size = !(reference / 300.0..=reference * 300.0).contains(&size);
        if !(misplaced || absurd_size) {
            return None;
        }
        [0.001f32, 1000.0].into_iter().find(|&f| inside(file.center() * f) && plausible_size(size * f))
    }

    /// Macro-oggetti principali: i figli dei nodi dei file (le radici, per una scena senza file).
    pub fn top_level_count(&self) -> usize {
        self.roots
            .iter()
            .map(|&r| match self.info.get(r as usize) {
                Some(i) if i.kind == NodeKind::File => self.nodes[r as usize].children().len(),
                _ => 1,
            })
            .sum()
    }

    /// File ancora presenti nella scena (non chiusi).
    pub fn open_files(&self) -> impl Iterator<Item = &SourceFile> {
        self.sources.iter().filter(|s| self.info.get(s.node as usize).is_some_and(|i| !i.deleted))
    }

    /// Calcola l'AABB (bounding box) complessivo di tutte le geometrie della scena.
    pub fn compute_bounding_box(&self) -> Aabb {
        let mut bbox = Aabb::empty();
        if self.mesh_bboxes.len() == self.shapes_data.len() {
            for b in &self.mesh_bboxes {
                if b.is_valid() {
                    bbox.extend_aabb(b);
                }
            }
        } else {
            for s in &self.shapes_data {
                for &v in &s.world_vertices {
                    bbox.extend_point(glam::Vec3::from(v));
                }
            }
        }
        bbox
    }

    /// Ingombro da inquadrare per le Shape visibili: il gruppo principale, senza i pochi pezzi
    /// molto lontani (vedi `main_group`). Ritorna anche quanti pezzi sono rimasti fuori.
    pub fn main_visible_bounding_box(&self) -> (Aabb, usize) {
        let boxes: Vec<Aabb> = self
            .nodes
            .iter()
            .filter_map(|node| match node {
                SceneNode::Shape { mesh_id, visible: true, .. } => self.mesh_bboxes.get(*mesh_id as usize),
                _ => None,
            })
            .filter(|b| b.is_valid())
            .cloned()
            .collect();
        main_group(&boxes)
    }

    /// AABB del sotto-albero di un nodo.
    pub fn subtree_bounding_box(&self, node_idx: u32) -> Aabb {
        let mut bbox = Aabb::empty();
        self.for_each_subtree_mesh(node_idx, |m| {
            if let Some(b) = self.mesh_bboxes.get(m as usize) {
                if b.is_valid() {
                    bbox.extend_aabb(b);
                }
            }
        });
        bbox
    }

    /// Aggiunge un nodo all'arena e ritorna il suo indice.
    #[allow(dead_code)]
    pub fn add_node(&mut self, node: SceneNode) -> u32 {
        let idx = self.nodes.len() as u32;
        let kind = match &node {
            SceneNode::Shape { .. } => NodeKind::Shape,
            SceneNode::Transform { .. } => NodeKind::Transform,
            SceneNode::Group { .. } => NodeKind::Group,
        };
        let shape_count = u32::from(kind == NodeKind::Shape);
        self.nodes.push(node);
        self.info.push(NodeInfo { parent: NO_PARENT, kind, shape_count, deleted: false, color: None });
        idx
    }

    /// Ritorna tutti gli indici dei nodi Shape nell'arena.
    #[allow(dead_code)]
    pub fn all_shape_indices(&self) -> Vec<u32> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| matches!(n, SceneNode::Shape { .. }).then_some(i as u32))
            .collect()
    }

    /// (Shape visibili, Shape totali) — un solo passaggio, senza allocazioni.
    pub fn shape_visibility_counts(&self) -> (usize, usize) {
        let mut visible = 0;
        let mut total = 0;
        for n in &self.nodes {
            if let SceneNode::Shape { visible: v, .. } = n {
                total += 1;
                if *v {
                    visible += 1;
                }
            }
        }
        (visible, total)
    }

    pub fn parent_of(&self, node_idx: u32) -> Option<u32> {
        self.info
            .get(node_idx as usize)
            .map(|i| i.parent)
            .filter(|&p| p != NO_PARENT)
    }

    /// Antenati di un nodo, dal padre diretto fino alla radice.
    pub fn ancestors(&self, node_idx: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = self.parent_of(node_idx);
        while let Some(p) = cur {
            out.push(p);
            cur = self.parent_of(p);
        }
        out
    }

    /// Etichetta leggibile di un nodo.
    pub fn display_name(&self, node_idx: u32) -> String {
        let node = &self.nodes[node_idx as usize];
        if let Some(n) = node.name() {
            return n.to_string();
        }
        match (node, self.info.get(node_idx as usize).map(|i| i.kind)) {
            (SceneNode::Shape { mesh_id, .. }, _) => format!("Shape #{mesh_id}"),
            (_, Some(NodeKind::Transform)) => format!("Transform #{node_idx}"),
            (_, Some(NodeKind::Use)) => format!("Istanza #{node_idx}"),
            (_, Some(NodeKind::File)) => format!("File #{node_idx}"),
            _ => format!("Group #{node_idx}"),
        }
    }

    /// Visita tutti i nodi del sotto-albero (incluso `node_idx`) senza ricorsione.
    pub fn for_each_subtree_node(&self, node_idx: u32, mut f: impl FnMut(u32)) {
        let mut stack = vec![node_idx];
        while let Some(idx) = stack.pop() {
            if let Some(node) = self.nodes.get(idx as usize) {
                f(idx);
                stack.extend_from_slice(node.children());
            }
        }
    }

    /// Visita i mesh_id di tutte le Shape del sotto-albero.
    pub fn for_each_subtree_mesh(&self, node_idx: u32, mut f: impl FnMut(u32)) {
        self.for_each_subtree_node(node_idx, |i| {
            if let SceneNode::Shape { mesh_id, .. } = self.nodes[i as usize] {
                f(mesh_id);
            }
        });
    }

    /// Scrive in `out` (indicizzato per mesh_id) la visibilità di ogni mesh.
    pub fn mesh_visibility_into(&self, out: &mut Vec<bool>) {
        out.clear();
        out.resize(self.shapes_data.len(), false);
        for node in &self.nodes {
            if let SceneNode::Shape { mesh_id, visible: true, .. } = node {
                if let Some(slot) = out.get_mut(*mesh_id as usize) {
                    *slot = true;
                }
            }
        }
    }

    /// Copia dello stato modificabile (visibilità, struttura, nomi, colori) per Annulla/Ripeti.
    /// La geometria (`shapes_data`, `mesh_bboxes`) non cambia mai e non viene copiata.
    pub fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            nodes: self.nodes.clone(),
            info: self.info.clone(),
            roots: self.roots.clone(),
            mesh_to_node: self.mesh_to_node.clone(),
            macro_components: self.macro_components.clone(),
        }
    }

    /// Ripristina uno stato salvato con `snapshot` e ritorna lo stato corrente.
    pub fn swap_snapshot(&mut self, mut s: EditSnapshot) -> EditSnapshot {
        std::mem::swap(&mut self.nodes, &mut s.nodes);
        std::mem::swap(&mut self.info, &mut s.info);
        std::mem::swap(&mut self.roots, &mut s.roots);
        std::mem::swap(&mut self.mesh_to_node, &mut s.mesh_to_node);
        std::mem::swap(&mut self.macro_components, &mut s.macro_components);
        self.revision += 1;
        s
    }

    /// Imposta (o rimuove con `None`) il colore utente di un nodo.
    /// I colori impostati sui discendenti vengono rimossi: il nuovo colore vale per tutto il sotto-albero.
    pub fn set_color(&mut self, node_idx: u32, color: Option<[f32; 4]>) {
        if node_idx as usize >= self.nodes.len() {
            return;
        }
        let mut sub = Vec::new();
        self.for_each_subtree_node(node_idx, |i| sub.push(i));
        for i in sub {
            self.info[i as usize].color = None;
        }
        self.info[node_idx as usize].color = color;
        self.revision += 1;
    }

    /// Rimuove tutti i colori utente.
    pub fn reset_all_colors(&mut self) {
        for inf in &mut self.info {
            inf.color = None;
        }
        self.revision += 1;
    }

    /// Colore utente effettivo di un nodo: il suo o quello dell'antenato più vicino che ne ha uno.
    pub fn effective_user_color(&self, node_idx: u32) -> Option<[f32; 4]> {
        let mut cur = node_idx;
        loop {
            let inf = self.info.get(cur as usize)?;
            if inf.color.is_some() {
                return inf.color;
            }
            if inf.parent == NO_PARENT {
                return None;
            }
            cur = inf.parent;
        }
    }

    /// Scrive in `out` (indicizzato per mesh_id) il colore utente ereditato di ogni mesh.
    /// Visita a stack dalle radici: O(nodi).
    pub fn mesh_user_colors_into(&self, out: &mut Vec<Option<[f32; 4]>>) {
        out.clear();
        out.resize(self.shapes_data.len(), None);
        let mut stack: Vec<(u32, Option<[f32; 4]>)> = self.roots.iter().map(|&r| (r, None)).collect();
        while let Some((idx, inherited)) = stack.pop() {
            let Some(node) = self.nodes.get(idx as usize) else { continue };
            let color = self.info[idx as usize].color.or(inherited);
            if let SceneNode::Shape { mesh_id, .. } = node {
                if let Some(slot) = out.get_mut(*mesh_id as usize) {
                    *slot = color;
                }
            }
            for &c in node.children() {
                stack.push((c, color));
            }
        }
    }

    /// Rinomina un nodo (`None` o stringa vuota = nome automatico).
    pub fn rename(&mut self, node_idx: u32, new_name: &str) {
        let trimmed = new_name.trim();
        let value = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if let Some(node) = self.nodes.get_mut(node_idx as usize) {
            match node {
                SceneNode::Group { name, .. }
                | SceneNode::Transform { name, .. }
                | SceneNode::Shape { name, .. } => *name = value,
            }
            self.revision += 1;
        }
    }

    /// Rende visibili/invisibili tutti i nodi nel sotto-albero radicato in `node_idx`.
    pub fn set_subtree_visible(&mut self, node_idx: u32, visible: bool) {
        let mut stack = vec![node_idx];
        while let Some(idx) = stack.pop() {
            if let Some(node) = self.nodes.get_mut(idx as usize) {
                node.set_visible(visible);
                stack.extend_from_slice(node.children());
            }
        }
        if visible {
            // Un figlio mostrato rende "visibili" anche i contenitori che lo contengono
            for a in self.ancestors(node_idx) {
                self.nodes[a as usize].set_visible(true);
            }
        }
        self.revision += 1;
    }

    /// Mostra tutti i nodi.
    pub fn show_all(&mut self) {
        for (node, info) in self.nodes.iter_mut().zip(&self.info) {
            if !info.deleted {
                node.set_visible(true);
            }
        }
        self.revision += 1;
    }

    /// Isola un sotto-albero: nasconde tutto, poi mostra solo i discendenti di `node_idx`
    /// (e marca come visibili i suoi antenati, così l'albero resta navigabile).
    pub fn isolate(&mut self, node_idx: u32) {
        for node in &mut self.nodes {
            node.set_visible(false);
        }
        self.set_subtree_visible(node_idx, true);
    }

    /// Come `isolate`, per più sotto-alberi insieme.
    pub fn isolate_many(&mut self, nodes: &[u32]) {
        for node in &mut self.nodes {
            node.set_visible(false);
        }
        for &n in nodes {
            self.set_subtree_visible(n, true);
        }
    }

    /// Elimina un nodo e tutti i suoi discendenti.
    /// I nodi eliminati diventano tombstone (Group vuoti invisibili) e il nodo viene
    /// staccato dal padre: gli indici degli altri nodi restano validi.
    pub fn delete_node(&mut self, node_idx: u32) {
        let Some(info) = self.info.get(node_idx as usize).copied() else {
            return;
        };
        if info.deleted {
            return;
        }

        // Stacca dal padre (o dalle radici)
        if info.parent == NO_PARENT {
            self.roots.retain(|&r| r != node_idx);
        } else {
            match &mut self.nodes[info.parent as usize] {
                SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => {
                    children.retain(|&c| c != node_idx);
                }
                SceneNode::Shape { .. } => {}
            }
        }

        // Tombstone di tutto il sotto-albero
        let mut to_delete = Vec::new();
        self.for_each_subtree_node(node_idx, |i| to_delete.push(i));
        for idx in to_delete {
            self.nodes[idx as usize] =
                SceneNode::Group { name: None, children: Vec::new(), visible: false, selected: false };
            let inf = &mut self.info[idx as usize];
            inf.deleted = true;
            inf.shape_count = 0;
        }
        for n in self.mesh_to_node.iter_mut() {
            if *n != NO_PARENT && self.info[*n as usize].deleted {
                *n = NO_PARENT;
            }
        }

        // Aggiorna i conteggi degli antenati
        let removed = info.shape_count;
        let mut cur = info.parent;
        while cur != NO_PARENT {
            let inf = &mut self.info[cur as usize];
            inf.shape_count = inf.shape_count.saturating_sub(removed);
            cur = inf.parent;
        }

        // Rimuove dai macro-componenti le Shape eliminate
        let info_ref = &self.info;
        for mc in &mut self.macro_components {
            mc.shape_indices.retain(|&s| !info_ref[s as usize].deleted);
        }
        self.macro_components.retain(|mc| !mc.shape_indices.is_empty());

        self.revision += 1;
    }

    /// Ricalcola `shape_count` per tutti i nodi (bottom-up sfruttando il pre-order).
    pub fn recompute_shape_counts(&mut self) {
        for (i, node) in self.nodes.iter().enumerate() {
            self.info[i].shape_count =
                u32::from(matches!(node, SceneNode::Shape { .. }) && !self.info[i].deleted);
        }
        // In pre-order i figli hanno indice maggiore del padre: scorrendo al contrario
        // ogni nodo ha già il conteggio definitivo quando lo si somma al padre.
        for i in (0..self.nodes.len()).rev() {
            let (parent, count) = (self.info[i].parent, self.info[i].shape_count);
            if parent != NO_PARENT && (parent as usize) < i {
                self.info[parent as usize].shape_count += count;
            }
        }
    }

    /// Ray picking CPU: ritorna il mesh_id della Shape visibile colpita più vicina.
    /// Test AABB (slab) seguito da Möller–Trumbore sui triangoli, in parallelo sulle mesh.
    pub fn pick(&self, origin: glam::Vec3, dir: glam::Vec3, mesh_visible: &[bool]) -> Option<u32> {
        self.pick_hit(origin, dir, mesh_visible).map(|(m, _)| m)
    }

    /// Come `pick`, ma ritorna anche la distanza del punto colpito lungo il raggio.
    pub fn pick_hit(&self, origin: glam::Vec3, dir: glam::Vec3, mesh_visible: &[bool]) -> Option<(u32, f32)> {
        self.pick_triangle(origin, dir, mesh_visible).map(|(m, t, _)| (m, t))
    }

    /// Come `pick_hit`, con in più i tre vertici (world-space) del triangolo colpito.
    pub fn pick_triangle(
        &self,
        origin: glam::Vec3,
        dir: glam::Vec3,
        mesh_visible: &[bool],
    ) -> Option<(u32, f32, [glam::Vec3; 3])> {
        self.pick_triangle_kept(origin, dir, mesh_visible, |_| true)
    }

    /// Come `pick_triangle`, considerando solo i punti per cui `keep` è vero (es. il lato
    /// visibile di un piano di sezione): si "vede" attraverso la parte tagliata.
    pub fn pick_triangle_kept(
        &self,
        origin: glam::Vec3,
        dir: glam::Vec3,
        mesh_visible: &[bool],
        keep: impl Fn(glam::Vec3) -> bool + Sync,
    ) -> Option<(u32, f32, [glam::Vec3; 3])> {
        use rayon::prelude::*;
        let inv_dir = dir.recip();
        (0..self.shapes_data.len())
            .into_par_iter()
            .filter(|&m| mesh_visible.get(m).copied().unwrap_or(false))
            .filter_map(|m| {
                let bb = self.mesh_bboxes.get(m)?;
                ray_aabb(origin, inv_dir, bb)?;
                let s = &self.shapes_data[m];
                let mut best = f32::INFINITY;
                let mut best_tri = [glam::Vec3::ZERO; 3];
                for tri in s.indices.chunks_exact(3) {
                    let (Some(a), Some(b), Some(c)) = (
                        s.world_vertices.get(tri[0] as usize),
                        s.world_vertices.get(tri[1] as usize),
                        s.world_vertices.get(tri[2] as usize),
                    ) else {
                        continue;
                    };
                    let (a, b, c) = (glam::Vec3::from(*a), glam::Vec3::from(*b), glam::Vec3::from(*c));
                    if let Some(t) = ray_triangle(origin, dir, a, b, c) {
                        if t < best && keep(origin + dir * t) {
                            best = t;
                            best_tri = [a, b, c];
                        }
                    }
                }
                best.is_finite().then_some((m as u32, best, best_tri))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

/// Sotto questo numero di pezzi l'inquadratura li comprende sempre tutti.
const MAIN_GROUP_MIN_PARTS: usize = 20;
/// Un pezzo è lontano oltre questo multiplo della distanza entro cui sta il 90% degli altri.
const MAIN_GROUP_FAR_FACTOR: f32 = 20.0;

/// Unione degli ingombri senza i pezzi molto lontani dal gruppo principale, con il loro numero.
/// In `Scocca_B.wrl` 26 pezzi su 1532 stanno a migliaia di unità dagli altri (che
/// occupano circa 60 unità): inquadrandoli tutti, il tappo diventava un puntino. Tocca solo la vista.
fn main_group(boxes: &[Aabb]) -> (Aabb, usize) {
    let union = |skip: Option<f32>, median: glam::Vec3| {
        let mut bbox = Aabb::empty();
        let mut excluded = 0;
        for b in boxes {
            if skip.is_some_and(|limit| b.center().distance(median) > limit) {
                excluded += 1;
            } else {
                bbox.extend_aabb(b);
            }
        }
        (bbox, excluded)
    };
    if boxes.len() < MAIN_GROUP_MIN_PARTS {
        return union(None, glam::Vec3::ZERO);
    }

    let centers: Vec<glam::Vec3> = boxes.iter().map(Aabb::center).collect();
    let mut median = glam::Vec3::ZERO;
    let mut values = vec![0.0f32; centers.len()];
    for axis in 0..3 {
        for (v, c) in values.iter_mut().zip(&centers) {
            *v = c[axis];
        }
        let mid = values.len() / 2;
        median[axis] = *values.select_nth_unstable_by(mid, f32::total_cmp).1;
    }
    let mut distances: Vec<f32> = centers.iter().map(|c| c.distance(median)).collect();
    let p90 = distances.len() * 9 / 10;
    let typical = *distances.select_nth_unstable_by(p90, f32::total_cmp).1;
    if typical <= 0.0 {
        return union(None, median);
    }
    union(Some(typical * MAIN_GROUP_FAR_FACTOR), median)
}

/// Intersezione raggio/AABB (metodo degli slab). Ritorna la distanza d'ingresso.
fn ray_aabb(origin: glam::Vec3, inv_dir: glam::Vec3, bb: &Aabb) -> Option<f32> {
    if !bb.is_valid() {
        return None;
    }
    let t1 = (bb.min - origin) * inv_dir;
    let t2 = (bb.max - origin) * inv_dir;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    (tmax >= tmin.max(0.0)).then_some(tmin)
}

/// Intersezione raggio/triangolo (Möller–Trumbore, two-sided).
fn ray_triangle(origin: glam::Vec3, dir: glam::Vec3, a: glam::Vec3, b: glam::Vec3, c: glam::Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tv = origin - a;
    let u = tv.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tv.cross(e1);
    let v = dir.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv_det;
    (t > 0.0).then_some(t)
}

#[cfg(test)]
mod main_group_tests {
    use super::*;

    fn cube(center: glam::Vec3, half: f32) -> Aabb {
        let mut b = Aabb::empty();
        b.extend_point(center - glam::Vec3::splat(half));
        b.extend_point(center + glam::Vec3::splat(half));
        b
    }

    /// `n` pezzi su una griglia di circa 5 × 2 × 2 unità.
    fn car(n: usize) -> Vec<Aabb> {
        (0..n)
            .map(|i| {
                let f = i as f32;
                cube(glam::Vec3::new(f % 50.0 * 0.1, f % 7.0 * 0.3, f % 5.0 * 0.4), 0.05)
            })
            .collect()
    }

    #[test]
    fn far_parts_in_other_units_are_left_out() {
        let mut boxes = car(500);
        let car_box = main_group(&boxes).0;
        boxes.push(cube(glam::Vec3::new(3200.0, 780.0, 530.0), 30.0));
        boxes.push(cube(glam::Vec3::new(2200.0, 700.0, 1540.0), 30.0));
        let (bbox, excluded) = main_group(&boxes);
        assert_eq!(excluded, 2);
        assert_eq!((bbox.min, bbox.max), (car_box.min, car_box.max));
    }

    #[test]
    fn a_normal_assembly_stays_whole() {
        let (_, excluded) = main_group(&car(500));
        assert_eq!(excluded, 0);

        // Pezzi sparsi a distanze comparabili: nessuno è "fuori scala"
        let spread: Vec<Aabb> =
            (0..100).map(|i| cube(glam::Vec3::new(i as f32 * 10.0, 0.0, 0.0), 1.0)).collect();
        let (bbox, excluded) = main_group(&spread);
        assert_eq!(excluded, 0);
        assert_eq!(bbox.max.x, 991.0);
    }

    #[test]
    fn few_parts_are_never_left_out() {
        let boxes = vec![cube(glam::Vec3::ZERO, 1.0), cube(glam::Vec3::splat(5000.0), 1.0)];
        let (bbox, excluded) = main_group(&boxes);
        assert_eq!(excluded, 0);
        assert!(bbox.max.x > 5000.0);
    }

    #[test]
    fn identical_positions_and_empty_input_are_safe() {
        let same = vec![cube(glam::Vec3::ONE, 1.0); 50];
        assert_eq!(main_group(&same).1, 0);
        let (bbox, excluded) = main_group(&[]);
        assert!(!bbox.is_valid());
        assert_eq!(excluded, 0);
    }
}

#[cfg(test)]
pub(crate) mod multi_file_tests {
    use super::*;

    pub const FILE_A: &[u8] = br#"#VRML V2.0 utf8
DEF Telaio Group { children [
  DEF Trave Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
    geometry DEF Tri IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
  DEF Copia Shape { geometry USE Tri }
] }
"#;
    pub const FILE_B: &[u8] = br#"#VRML V2.0 utf8
DEF Sportello Transform { translation 5 0 0 children [
  DEF Pannello Shape { appearance Appearance { material Material { diffuseColor 0 0 1 } }
    geometry DEF Tri IndexedFaceSet { coord Coordinate { point [ 0 0 1, 2 0 1, 0 2 1 ] } coordIndex [ 0, 1, 2, -1 ] } }
  DEF Gemello Shape { geometry USE Tri }
] }
"#;

    pub fn file(data: &[u8], name: &str) -> SceneGraph {
        let mut s = crate::parser::parse_wrl(data);
        s.into_file(name.to_string(), None);
        s
    }

    /// Scena con i due file di prova, nell'ordine A, B.
    pub fn two_files() -> SceneGraph {
        let mut scene = file(FILE_A, "a.wrl");
        scene.append(file(FILE_B, "b.wrl"));
        scene
    }

    /// Invarianti dell'arena: pre-order, padri coerenti, mesh_to_node corretto.
    fn check_arena(scene: &SceneGraph) {
        for (i, node) in scene.nodes.iter().enumerate() {
            for &c in node.children() {
                assert!(c as usize > i, "pre-order: figlio {c} di {i}");
                assert_eq!(scene.info[c as usize].parent, i as u32);
            }
            if let SceneNode::Shape { mesh_id, .. } = node {
                assert_eq!(scene.mesh_to_node[*mesh_id as usize], i as u32);
            }
        }
        for &r in &scene.roots {
            assert_eq!(scene.info[r as usize].parent, NO_PARENT);
        }
    }

    #[test]
    fn a_file_gets_its_own_root() {
        let scene = file(FILE_A, "a.wrl");
        check_arena(&scene);
        assert_eq!(scene.roots, vec![0]);
        assert_eq!(scene.info[0].kind, NodeKind::File);
        assert_eq!(scene.display_name(0), "a.wrl");
        assert_eq!(scene.info[0].shape_count, 2);
        assert_eq!(scene.display_name(scene.nodes[0].children()[0]), "Telaio");
        assert!(scene.source_data.is_empty() && scene.sources[0].data.starts_with(b"#VRML"));
    }

    #[test]
    fn two_files_share_one_scene() {
        let scene = two_files();
        check_arena(&scene);
        assert_eq!(scene.roots.len(), 2);
        assert_eq!(scene.shapes_data.len(), 4);
        assert_eq!(scene.sources.iter().map(|s| s.meshes.clone()).collect::<Vec<_>>(), vec![0..2, 2..4]);
        assert_eq!(scene.sources[1].node, scene.roots[1]);
        assert_eq!(scene.display_name(scene.roots[1]), "b.wrl");
        assert_eq!(scene.info[scene.roots[1] as usize].shape_count, 2);
        // Ogni mesh legge il testo del proprio file
        assert_eq!(scene.source_index(1), Some(0));
        assert_eq!(scene.source_index(3), Some(1));
        assert_eq!(scene.source_index(4), None);
        assert!(scene.source_for_mesh(2).windows(9).any(|w| w == b"Sportello"));
        // Geometria del secondo file al suo posto (traslazione 5)
        assert_eq!(scene.shapes_data[2].world_vertices[0], [5.0, 0.0, 1.0]);

        // Nascondere un file nasconde solo le sue mesh
        let mut scene = scene;
        scene.set_subtree_visible(scene.roots[1], false);
        let mut vis = Vec::new();
        scene.mesh_visibility_into(&mut vis);
        assert_eq!(vis, vec![true, true, false, false]);
    }

    /// Con un piano di sezione il clic attraversa la parte tagliata e prende il pezzo dietro.
    #[test]
    fn picking_ignores_the_cut_side() {
        let scene = two_files(); // A a z = 0 (x 0..1), B a z = 1 (x 5..7)
        let visible = vec![true; 4];
        // Raggio dall'alto verso il basso nel punto (5.2, 0.2): colpisce solo B
        let (o, d) = (glam::Vec3::new(5.2, 0.2, 10.0), glam::Vec3::NEG_Z);
        assert_eq!(scene.pick_triangle(o, d, &visible).map(|h| h.0), Some(2));
        // Tagliando via z > 0.5, B non c'è più e il raggio non trova nulla
        assert!(scene.pick_triangle_kept(o, d, &visible, |p| p.z <= 0.5).is_none());
        // Due superfici sovrapposte: A a z = 0 e la sua copia; con il taglio a z < -1 nessuna
        let (o, d) = (glam::Vec3::new(0.2, 0.2, 10.0), glam::Vec3::NEG_Z);
        assert!(scene.pick_triangle(o, d, &visible).is_some());
        assert!(scene.pick_triangle_kept(o, d, &visible, |p| p.z < -1.0).is_none());
    }

    #[test]
    fn closing_a_file_keeps_the_other() {
        let mut scene = two_files();
        scene.delete_node(scene.sources[0].node);
        assert_eq!(scene.open_files().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["b.wrl"]);
        let mut vis = Vec::new();
        scene.mesh_visibility_into(&mut vis);
        assert_eq!(vis, vec![false, false, true, true]);
    }

    /// Un file in millimetri aggiunto a una scena in metri: proposto ÷1000, e dopo la
    /// conversione la geometria e l'export sono in metri.
    /// Scocca in metri (X 0..5, Y -1..1, Z 0..1,8)
    pub const BODY_M: &[u8] = br#"#VRML V2.0 utf8
DEF Scocca Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 -1 0, 5 -1 0, 5 1 0, 0 1 1.8 ] } coordIndex [ 0, 1, 2, -1, 0, 2, 3, -1 ] } }
"#;
    /// Sportello in millimetri, sulla fiancata della scocca una volta convertito
    pub const DOOR_MM: &[u8] = br#"#VRML V2.0 utf8
DEF Sportello Shape { geometry IndexedFaceSet { coord Coordinate { point [ 3000 -960 1000, 3200 -960 1000, 3100 -960 1200 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;

    #[test]
    fn a_file_in_other_units_is_detected_and_converted() {
        let (metres, millimetres) = (BODY_M, DOOR_MM);
        let mut scene = file(metres, "scocca.wrl");
        scene.append(file(millimetres, "sportello.wrl"));
        assert_eq!(scene.suggest_unit_scale(1), Some(0.001));
        assert_eq!(scene.suggest_unit_scale(0), None, "la scocca non è dentro lo sportello");

        scene.scale_file(1, 0.001);
        let v = scene.shapes_data[1].world_vertices[0];
        assert!((v[0] - 3.0).abs() < 1e-6 && (v[1] + 0.96).abs() < 1e-6 && (v[2] - 1.0).abs() < 1e-6);
        assert!((scene.mesh_bboxes[1].max.x - 3.2).abs() < 1e-5);
        assert_eq!(scene.sources[1].scale, 0.001);
        assert_eq!(scene.suggest_unit_scale(1), None);

        let path = std::env::temp_dir().join("gestore_wrl_export_scala.wrl");
        crate::exporter::export_visible_wrl(&scene, &path).unwrap();
        let again = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        let w = again.shapes_data[1].world_vertices[2];
        assert!((w[0] - 3.1).abs() < 1e-6 && (w[2] - 1.2).abs() < 1e-6, "{w:?}");
        let _ = std::fs::remove_file(path);

        // Due file nelle stesse unità: nessuna proposta
        assert_eq!(two_files().suggest_unit_scale(1), None);

        // Al contrario: scocca in metri aggiunta a una in millimetri (cade "dentro", ma è
        // 1000 volte più piccola)
        let scocca_mm = br#"#VRML V2.0 utf8
Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 -1000 0, 5000 -1000 0, 5000 1000 0, 0 1000 1800 ] } coordIndex [ 0, 1, 2, -1, 0, 2, 3, -1 ] } }
"#;
        let mut scene = file(scocca_mm, "mm.wrl");
        scene.append(file(metres, "m.wrl"));
        assert_eq!(scene.suggest_unit_scale(1), Some(1000.0));

        // Più file in millimetri aggiunti alla scocca in metri: ognuno è confrontato con la
        // scocca, non con l'insieme che comprende gli altri file non ancora convertiti
        let mut scene = file(metres, "scocca.wrl");
        scene.append(file(millimetres, "sportello.wrl"));
        scene.append(file(millimetres, "supporto.wrl"));
        assert_eq!(scene.suggest_unit_scale(2), Some(0.001));

        // Unità dichiarate dal formato (HSF): due file in millimetri lontani e di grandezze
        // diverse (supporto e parete di cabina) non sono confusi con un file in metri
        let mm = |data: &[u8], name: &str| {
            let mut s = crate::parser::parse_wrl(data);
            s.units_mm = true;
            s.into_file(name.to_string(), None);
            s
        };
        let supporto = br#"#VRML V2.0 utf8
Shape { geometry IndexedFaceSet { coord Coordinate { point [ -90 -400 0, 230 70 0, 0 0 260 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;
        let parete = br#"#VRML V2.0 utf8
Shape { geometry IndexedFaceSet { coord Coordinate { point [ -30 0 -2500, 0 1500 -2500, 0 0 500 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;
        let mut scene = file(supporto, "supporto.wrl");
        scene.append(file(parete, "parete.wrl"));
        assert_eq!(scene.suggest_unit_scale(1), Some(0.001), "senza unità note la parete sembra in metri");
        let mut scene = mm(supporto, "supporto.hsf");
        scene.append(mm(parete, "parete.hsf"));
        assert_eq!(scene.suggest_unit_scale(1), None);
        // Un file con le unità note accanto a uno in metri: la proposta resta
        let mut scene = file(metres, "scocca.wrl");
        scene.append(mm(millimetres, "sportello.hsf"));
        assert_eq!(scene.suggest_unit_scale(1), Some(0.001));
    }

    #[test]
    fn undo_states_receive_the_added_file() {
        let mut scene = file(FILE_A, "a.wrl");
        let mut before = scene.snapshot();
        let (n, m, k) = (scene.nodes.len() as u32, scene.shapes_data.len() as u32, scene.macro_components.len());
        scene.append(file(FILE_B, "b.wrl"));
        before.extend_from(&scene, n, m, k);
        let now = scene.swap_snapshot(before);
        assert_eq!(scene.roots.len(), 2);
        assert_eq!(scene.nodes.len(), now.nodes.len());
        check_arena(&scene);
    }
}

/// Stato modificabile della scena (vedi `SceneGraph::snapshot`).
#[derive(Debug, Clone)]
pub struct EditSnapshot {
    nodes: Vec<SceneNode>,
    info: Vec<NodeInfo>,
    roots: Vec<u32>,
    mesh_to_node: Vec<u32>,
    macro_components: Vec<MacroComponent>,
}

impl EditSnapshot {
    /// Aggiunge a uno stato salvato i file arrivati dopo, presi dalla scena corrente a partire
    /// dal nodo `first_node`, dalla mesh `first_mesh` e dal macro-componente `first_macro`.
    /// Così Annulla/Ripeti non tolgono un file aggiunto: aggiungere un file non è una modifica.
    pub fn extend_from(&mut self, scene: &SceneGraph, first_node: u32, first_mesh: u32, first_macro: usize) {
        let (n, m) = (first_node as usize, first_mesh as usize);
        if self.nodes.len() != n || self.mesh_to_node.len() != m {
            return; // stato di un'altra scena: non dovrebbe succedere
        }
        self.nodes.extend_from_slice(&scene.nodes[n..]);
        self.info.extend_from_slice(&scene.info[n..]);
        self.roots.extend(scene.roots.iter().filter(|&&r| r >= first_node));
        self.mesh_to_node.extend_from_slice(&scene.mesh_to_node[m..]);
        self.macro_components.extend_from_slice(&scene.macro_components[first_macro.min(scene.macro_components.len())..]);
    }
}

// ─── MacroComponent ───────────────────────────────────────────────────────────

/// Un gruppo logico di Shape connesse geometricamente.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MacroComponent {
    pub id: u32,
    /// Indici degli SceneNode Shape che fanno parte di questo componente
    pub shape_indices: Vec<u32>,
    /// Bounding box world-space del componente
    pub bbox: Aabb,
    pub face_count: u32,
    pub vertex_count: u32,
}
