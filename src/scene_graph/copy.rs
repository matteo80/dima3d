//! Copia e incolla di pezzi nella scena.
//!
//! Incollare crea un nuovo «file» nell'albero ("Copia di …"): un nodo radice con i duplicati dei
//! nodi copiati, nella stessa posizione degli originali. Il testo sorgente è quello del file
//! originale (condiviso, non duplicato), quindi la copia si esporta come gli altri pezzi.
//!
//! Il progetto salva una copia come istruzione per rifarla ([`CopyOf`]) più le sue modifiche.
//! La numerazione di nodi e mesh della copia dipende solo dai nodi copiati e dai rami già
//! eliminati al momento della copia (`skip`): rifacendo la copia sul file appena riaperto si
//! ottengono gli stessi numeri.

use std::collections::HashSet;

use glam::Mat4;

use super::node::{NodeKind, SceneGraph, SceneNode, SourceFile, NO_PARENT};

/// Da dove viene una copia incollata.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CopyOf {
    /// File da cui viene la copia (indice in `SceneGraph::sources`; può essere un'altra copia)
    pub source: usize,
    /// Nodi copiati, con i loro numeri fissi nel file (`NodeInfo::origin`)
    pub nodes: Vec<u32>,
    /// Rami eliminati dentro i nodi copiati al momento della copia (numeri fissi)
    pub skip: Vec<u32>,
}

impl SceneGraph {
    /// Indice in `sources` del file che contiene il nodo (risalendo fino alla radice: un gruppo
    /// creato in Dima 3D ha un numero dopo quelli degli altri file).
    pub fn source_of_node(&self, node: u32) -> Option<usize> {
        let root = self.root_of(node)?;
        self.sources.iter().position(|s| s.node == root)
    }

    /// Nodi da copiare, raggruppati per file e in ordine: si tolgono quelli eliminati e quelli già
    /// compresi in un altro nodo della lista.
    pub fn copy_groups(&self, nodes: &[u32]) -> Vec<(usize, Vec<u32>)> {
        let set: HashSet<u32> = nodes.iter().copied().collect();
        let mut kept: Vec<u32> = nodes
            .iter()
            .copied()
            .filter(|&n| self.info.get(n as usize).is_some_and(|i| !i.deleted))
            .filter(|&n| !self.ancestors(n).iter().any(|a| set.contains(a)))
            .collect();
        kept.sort_unstable();
        kept.dedup();
        let mut groups: Vec<(usize, Vec<u32>)> = Vec::new();
        for n in kept {
            let Some(source) = self.source_of_node(n) else { continue };
            match groups.iter_mut().find(|g| g.0 == source) {
                Some(g) => g.1.push(n),
                None => groups.push((source, vec![n])),
            }
        }
        groups
    }

    /// Nome della radice di una copia.
    pub fn copy_name(&self, nodes: &[u32]) -> String {
        match nodes {
            [] => "Copia".to_string(),
            [one] => format!("Copia di {}", self.display_name(*one)),
            [first, rest @ ..] => format!("Copia di {} e altri {}", self.display_name(*first), rest.len()),
        }
    }

    /// Incolla una copia dei nodi `nodes` (tutti del file `source`) come nuovo file in coda alla
    /// scena, con nome `name`. Non si copiano i nodi in `skip` (con i loro discendenti) né i rami
    /// già eliminati. Ritorna la radice della copia e, per ogni mesh nuova, la mesh copiata.
    pub fn paste_copy(
        &mut self,
        source: usize,
        nodes: &[u32],
        skip: &HashSet<u32>,
        name: String,
    ) -> Option<(u32, Vec<u32>)> {
        if source >= self.sources.len() || nodes.is_empty() || nodes.iter().any(|&n| self.source_of_node(n) != Some(source)) {
            return None;
        }
        let is_inside = |scene: &SceneGraph, n: u32, top: u32| n == top || scene.ancestors(n).contains(&top);
        let origin = |scene: &SceneGraph, n: u32| scene.info[n as usize].origin;

        // Rami da non copiare, per rifare la stessa copia alla riapertura del progetto (con i
        // numeri fissi dei nodi, che non cambiano spostandoli nell'albero)
        let mut skipped: Vec<u32> = skip.iter().filter(|&&n| self.source_of_node(n) == Some(source)).map(|&n| origin(self, n)).collect();
        for n in self.file_nodes(source) {
            let info = &self.info[n as usize];
            let top_of_branch = info.deleted && (info.parent == NO_PARENT || !self.info[info.parent as usize].deleted);
            if top_of_branch && nodes.iter().any(|&top| is_inside(self, n, top)) {
                skipped.push(origin(self, n));
            }
        }
        skipped.sort_unstable();
        skipped.dedup();

        let root = self.nodes.len() as u32;
        let first_mesh = self.shapes_data.len() as u32;
        if !self.frames.is_empty() {
            self.frames.resize(root as usize, Mat4::IDENTITY);
            self.frames.push(Mat4::IDENTITY);
        }
        if !self.placements.is_empty() {
            self.placements.resize(first_mesh as usize, Mat4::IDENTITY);
        }
        self.nodes.push(SceneNode::Group { name: Some(name.clone()), children: Vec::new(), visible: true, selected: false });
        let mut root_info = self.info[self.sources[source].node as usize];
        root_info.parent = NO_PARENT;
        root_info.kind = NodeKind::File;
        root_info.deleted = false;
        root_info.color = None;
        root_info.renamed = false;
        root_info.origin = 0;
        root_info.created = false;
        root_info.restructured = false;
        self.info.push(root_info);
        let mut origins = Vec::new();

        for &top in nodes {
            // Visita in pre-order: i nodi nuovi restano in pre-order e contigui
            let mut stack = vec![(top, root)];
            while let Some((old, parent)) = stack.pop() {
                if skip.contains(&old) || self.info[old as usize].deleted {
                    continue;
                }
                let new = self.nodes.len() as u32;
                let mut node = self.nodes[old as usize].clone();
                let mut info = self.info[old as usize];
                node.set_selected(false);
                info.parent = parent;
                // Numeri fissi della copia: la posizione nella copia, come per un file aperto
                info.origin = new - root;
                info.created = false;
                info.restructured = false;
                if info.kind == NodeKind::File {
                    info.kind = NodeKind::Group;
                }
                if old == top {
                    // Il colore ereditato dai nodi sopra vale anche per la copia
                    info.color = self.effective_user_color(old);
                    node.set_visible(true);
                }
                let children = node.children().to_vec();
                match &mut node {
                    SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => children.clear(),
                    SceneNode::Shape { mesh_id, .. } => {
                        let m = *mesh_id;
                        origins.push(m);
                        let shape = self.shapes_data[m as usize].clone();
                        let bbox = self.mesh_bboxes.get(m as usize).copied().unwrap_or_else(super::Aabb::empty);
                        *mesh_id = self.shapes_data.len() as u32;
                        if !self.placements.is_empty() {
                            let p = self.placement(m);
                            self.placements.push(p);
                        }
                        self.shapes_data.push(shape);
                        self.mesh_bboxes.push(bbox);
                        self.mesh_to_node.push(new);
                    }
                }
                if !self.frames.is_empty() {
                    let frame = self.frames.get(old as usize).copied().unwrap_or(Mat4::IDENTITY);
                    self.frames.push(frame);
                }
                self.nodes.push(node);
                self.info.push(info);
                if let SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } =
                    &mut self.nodes[parent as usize]
                {
                    children.push(new);
                }
                stack.extend(children.into_iter().rev().map(|c| (c, new)));
            }
        }
        self.roots.push(root);
        self.recompute_shape_counts();

        let original = &self.sources[source];
        let copy = SourceFile {
            name,
            path: None,
            data: original.data.clone(),
            meshes: first_mesh..self.shapes_data.len() as u32,
            node: root,
            scale: original.scale,
            units_mm: original.units_mm,
            step: None,
            copy_of: Some(CopyOf { source, nodes: nodes.iter().map(|&n| self.info[n as usize].origin).collect(), skip: skipped }),
            next_origin: self.nodes.len() as u32 - root,
        };
        self.sources.push(copy);
        self.revision += 1;
        Some((root, origins))
    }

    /// Rifà la copia descritta da `copy` (es. alla riapertura di un progetto). Ritorna la radice.
    pub fn redo_copy(&mut self, copy: &CopyOf, name: String) -> Option<u32> {
        let map = self.origin_map(copy.source);
        let nodes: Vec<u32> = copy.nodes.iter().map(|n| map.get(n).copied()).collect::<Option<_>>()?;
        let skip: HashSet<u32> = copy.skip.iter().filter_map(|n| map.get(n).copied()).collect();
        self.paste_copy(copy.source, &nodes, &skip, name).map(|(root, _)| root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_graph::node::multi_file_tests::two_files;

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
        assert_eq!(scene.shapes_data.len(), scene.mesh_to_node.len());
        assert_eq!(scene.shapes_data.len(), scene.mesh_bboxes.len());
    }

    fn by_name(scene: &SceneGraph, name: &str) -> u32 {
        (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == name).unwrap()
    }

    #[test]
    fn pasted_copy_is_a_new_file_in_the_same_place() {
        let mut scene = two_files();
        let telaio = by_name(&scene, "Telaio");
        let shapes = scene.shapes_data.len();
        let groups = scene.copy_groups(&[telaio]);
        assert_eq!(groups, vec![(0, vec![telaio])]);
        let name = scene.copy_name(&groups[0].1);
        let (root, origins) = scene.paste_copy(0, &groups[0].1, &HashSet::new(), name).unwrap();
        assert_eq!(origins.len(), 2);
        check_arena(&scene);

        assert_eq!(scene.display_name(root), "Copia di Telaio");
        assert_eq!(scene.info[root as usize].kind, NodeKind::File);
        assert_eq!(scene.roots.last(), Some(&root));
        assert_eq!(scene.info[root as usize].shape_count, 2);
        assert_eq!(scene.shapes_data.len(), shapes + 2);
        // Stessa geometria degli originali
        let mut originals = Vec::new();
        scene.for_each_subtree_mesh(telaio, |m| originals.push(m));
        let mut copies = Vec::new();
        scene.for_each_subtree_mesh(root, |m| copies.push(m));
        for (a, b) in originals.iter().zip(&copies) {
            assert_eq!(scene.shapes_data[*a as usize].world_vertices, scene.shapes_data[*b as usize].world_vertices);
        }
        let source = scene.sources.last().unwrap();
        assert_eq!(source.node, root);
        assert!(source.path.is_none());
        assert_eq!(source.copy_of.as_ref().unwrap().source, 0);
        assert_eq!(scene.source_index(copies[0]), Some(scene.sources.len() - 1));
        // La copia è indipendente: eliminarla non tocca l'originale
        scene.delete_node(root);
        assert_eq!(scene.info[telaio as usize].shape_count, 2);
    }

    #[test]
    fn nested_and_multi_file_selections_are_grouped() {
        let scene = two_files();
        let (telaio, trave, sportello) =
            (by_name(&scene, "Telaio"), by_name(&scene, "Trave"), by_name(&scene, "Sportello"));
        let groups = scene.copy_groups(&[trave, sportello, telaio]);
        assert_eq!(groups, vec![(0, vec![telaio]), (1, vec![sportello])]);
    }

    /// Una copia fatta dopo aver eliminato un pezzo si rifà uguale sul file appena riaperto.
    #[test]
    fn copy_is_repeatable_on_the_reopened_file() {
        let mut scene = two_files();
        let (telaio, trave) = (by_name(&scene, "Telaio"), by_name(&scene, "Trave"));
        scene.delete_node(trave);
        scene.paste_copy(0, &[telaio], &HashSet::new(), "Copia".into()).unwrap();
        let copy = scene.sources.last().unwrap().copy_of.clone().unwrap();
        assert_eq!(copy.skip, vec![trave - scene.sources[0].node]);
        let pasted = scene.shapes_data.len() - scene.sources.last().unwrap().meshes.start as usize;
        assert_eq!(pasted, 1);

        let mut fresh = two_files();
        fresh.redo_copy(&copy, "Copia".into()).unwrap();
        check_arena(&fresh);
        assert_eq!(fresh.nodes.len(), scene.nodes.len());
        assert_eq!(fresh.shapes_data.len(), scene.shapes_data.len());
        let n = scene.sources.last().unwrap().node as usize;
        for i in n..scene.nodes.len() {
            assert_eq!(fresh.display_name(i as u32), scene.display_name(i as u32));
        }
    }

    #[test]
    fn copy_of_a_copy() {
        let mut scene = two_files();
        let sportello = by_name(&scene, "Sportello");
        let (first, _) = scene.paste_copy(1, &[sportello], &HashSet::new(), "A".into()).unwrap();
        let inner = scene.nodes[first as usize].children()[0];
        let groups = scene.copy_groups(&[inner]);
        assert_eq!(groups[0].0, 2);
        let (second, _) = scene.paste_copy(2, &groups[0].1, &HashSet::new(), "B".into()).unwrap();
        check_arena(&scene);
        assert_eq!(scene.info[second as usize].shape_count, 2);
        assert_eq!(scene.sources[3].copy_of.as_ref().unwrap().source, 2);
    }
}
