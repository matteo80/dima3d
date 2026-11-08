pub mod delimiter_map;
pub mod hsf;
pub mod iv;
pub mod jt;
pub mod lexer;
pub mod loader;
pub mod material;
pub mod shape_extractor;
pub mod stl;
pub mod transform;
mod vrml1;

pub use loader::{load_wrl_async, load_wrl_async_with, LoadEvent};
#[allow(unused_imports)]
pub use shape_extractor::{parse_wrl, parse_wrl_with_progress, DefRegistry};

/// Definizione valida per un `USE` in posizione `before`, dato l'elenco dei `DEF` con lo stesso
/// nome in ordine di file: per lo standard VRML è l'ultima che lo precede. Un `USE` scritto prima
/// di ogni `DEF` non è VRML valido, ma alcuni esportatori lo producono: vale il primo `DEF`.
pub(crate) fn latest_before<T>(defs: &[T], before: usize, pos: impl Fn(&T) -> usize) -> Option<&T> {
    let preceding = defs.partition_point(|d| pos(d) < before);
    defs.get(preceding.saturating_sub(1))
}

/// Informazioni su una singola Shape estratta dal file WRL.
/// Contiene i byte range originali per l'export e i vertici world-space
/// già trasformati dalla matrice padre.
#[derive(Debug, Clone)]
pub struct ShapeInfo {
    /// Offset byte nel file sorgente dove inizia il nodo Shape
    pub source_start: usize,
    /// Offset byte nel file sorgente dove finisce il nodo Shape
    pub source_end: usize,
    /// Offset relativo da source_start: inizio del blocco `point [...]`
    pub point_range_start: usize,
    /// Offset relativo da source_start: fine del blocco `point [...]`
    pub point_range_end: usize,
    /// Matrice di trasformazione world-space accumulata dai Transform padre
    pub matrix: glam::Mat4,
    /// Vertici già trasformati in world-space (position * matrix)
    pub world_vertices: Vec<[f32; 3]>,
    /// Normali in world-space, una per vertice di resa: i vertici di `world_vertices` seguiti da
    /// quelli aggiunti da `split` (vedi [`ShapeInfo::render_vertex_count`])
    pub normals: Vec<[f32; 3]>,
    /// Spigoli vivi: presente quando un vertice ha normali diverse sulle facce che lo usano
    pub split: Option<Box<SplitNormals>>,
    /// Indici dei vertici triangolati da coordIndex
    pub indices: Vec<u32>,
    /// Numero di facce del mesh
    pub face_count: u32,
    /// Nome DEF della Shape (se presente)
    pub name: Option<String>,
    /// Colore originale del Material (RGBA sRGB, alpha = 1 - transparency), se presente
    pub color: Option<[f32; 4]>,
    /// Catena dei nodi contenitore (Group/Transform/USE) dalla radice fino alla Shape.
    /// Viene costruita bottom-up durante la discesa ricorsiva ed è usata per
    /// ricostruire la gerarchia nel SceneGraph.
    pub path: Vec<PathSeg>,
    /// Geometria ricalcolata dal Gestore (es. riduzione delle facce): l'export la scrive dai dati
    /// e non copia il testo originale del file
    pub rewritten: bool,
}

/// Vertici aggiunti per la sola resa. Un vertice su uno spigolo vivo ha una normale per ogni
/// faccia (`normalIndex` del file): sulla GPU servono più copie del vertice, una per normale.
/// La geometria (`world_vertices`, `indices`) resta quella del file: export, selezione e
/// raggruppamento non vedono le copie.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitNormals {
    /// Vertice di `world_vertices` di ogni copia, in ordine dopo i vertici originali
    pub extra: Vec<u32>,
    /// Triangoli sui vertici di resa, paralleli a `ShapeInfo::indices`
    pub indices: Vec<u32>,
}

impl ShapeInfo {
    /// Vertici sulla GPU: quelli del file più le copie degli spigoli vivi.
    pub fn render_vertex_count(&self) -> usize {
        self.world_vertices.len() + self.split.as_ref().map_or(0, |s| s.extra.len())
    }

    /// Triangoli sui vertici di resa.
    pub fn render_indices(&self) -> &[u32] {
        self.split.as_ref().map_or(&self.indices, |s| &s.indices)
    }

    /// Posizione del vertice di resa `r`.
    pub fn render_position(&self, r: usize) -> Option<[f32; 3]> {
        let n = self.world_vertices.len();
        let v = if r < n { r } else { *self.split.as_ref()?.extra.get(r - n)? as usize };
        self.world_vertices.get(v).copied()
    }

    /// Normale di ogni angolo dei triangoli, parallela a `indices` (per confronti e test).
    pub fn corner_normals(&self) -> Vec<[f32; 3]> {
        self.render_indices()
            .iter()
            .map(|&r| self.normals.get(r as usize).copied().unwrap_or([0.0; 3]))
            .collect()
    }
}

/// Tipo di nodo contenitore VRML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Group,
    Transform,
    /// Istanza `USE Nome` di un nodo DEF contenitore
    Use,
}

/// Un livello della gerarchia di contenitori di una Shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSeg {
    /// Chiave univoca tra fratelli: offset byte della `{` del contenitore
    /// (o del token `USE` per le istanze).
    pub key: usize,
    /// Nome DEF del contenitore, se presente
    pub name: Option<String>,
    pub kind: ContainerKind,
}
