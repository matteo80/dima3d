//! Struttura dell'albero modificata dall'utente: pezzi spostati in un altro gruppo dello stesso
//! file e gruppi nuovi.
//!
//! Spostare un nodo cambia solo il padre e gli elenchi dei figli: i numeri dei nodi restano gli
//! stessi, quindi selezione, appunti, Annulla e finestra Posizione restano validi. Per questo
//! l'ordine dei numeri non è più l'ordine dell'albero: chi deve visitare i figli prima dei padri
//! usa [`SceneGraph::tree_order`]. La geometria non si muove.

use std::collections::{HashMap, HashSet};

use super::node::{NodeKind, SceneGraph, SceneNode, NO_PARENT};

impl SceneGraph {
    /// Nodi non eliminati in ordine d'albero (ogni padre prima dei suoi figli), dalle radici.
    pub fn tree_order(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<u32> = self.roots.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n as usize) else { continue };
            out.push(n);
            stack.extend(node.children().iter().rev());
        }
        out
    }

    /// Radice del nodo (risalendo i padri; vale anche per i nodi eliminati).
    pub fn root_of(&self, node: u32) -> Option<u32> {
        let mut n = node;
        let mut steps = 0;
        loop {
            let parent = self.info.get(n as usize)?.parent;
            if parent == NO_PARENT {
                return Some(n);
            }
            n = parent;
            steps += 1;
            if steps > self.nodes.len() {
                return None; // anello: non dovrebbe succedere
            }
        }
    }

    /// Tutti i nodi di un file, eliminati compresi, in ordine di numero.
    pub fn file_nodes(&self, source: usize) -> Vec<u32> {
        let Some(root) = self.sources.get(source).map(|s| s.node) else { return Vec::new() };
        // Il numero della radice di ogni nodo, calcolato una volta per catena
        let mut roots: Vec<u32> = vec![u32::MAX; self.nodes.len()];
        let mut out = Vec::new();
        for n in 0..self.nodes.len() as u32 {
            let mut chain = Vec::new();
            let mut cur = n;
            let found = loop {
                if roots[cur as usize] != u32::MAX {
                    break roots[cur as usize];
                }
                chain.push(cur);
                match self.info[cur as usize].parent {
                    NO_PARENT => break cur,
                    p => cur = p,
                }
                if chain.len() > self.nodes.len() {
                    break u32::MAX;
                }
            };
            for c in chain {
                roots[c as usize] = found;
            }
            if found == root {
                out.push(n);
            }
        }
        out
    }

    /// Numero fisso → nodo, per i nodi di un file.
    pub fn origin_map(&self, source: usize) -> HashMap<u32, u32> {
        self.file_nodes(source).into_iter().map(|n| (self.info[n as usize].origin, n)).collect()
    }

    /// Nodi da spostare: si tolgono quelli eliminati e quelli dentro un altro nodo della lista.
    fn tops(&self, nodes: &[u32]) -> Vec<u32> {
        let set: HashSet<u32> = nodes.iter().copied().collect();
        let mut tops: Vec<u32> = nodes
            .iter()
            .copied()
            .filter(|&n| self.info.get(n as usize).is_some_and(|i| !i.deleted))
            .filter(|&n| !self.ancestors(n).iter().any(|a| set.contains(a)))
            .collect();
        tops.sort_unstable();
        tops.dedup();
        tops
    }

    /// Controlla che `nodes` si possano spostare dentro `target` (che può non esistere ancora).
    fn check_move(&self, nodes: &[u32], target: u32) -> Result<(), String> {
        let source = self.source_of_node(target).ok_or("gruppo di destinazione non valido")?;
        for &n in nodes {
            if self.info[n as usize].parent == NO_PARENT {
                return Err("un file intero non si sposta in un gruppo".to_string());
            }
            if self.source_of_node(n) != Some(source) {
                return Err("si sposta solo dentro lo stesso file (per un altro file: Copia e Incolla)".to_string());
            }
            if n == target || self.ancestors(target).contains(&n) {
                return Err(format!("«{}» non può andare dentro sé stesso", self.display_name(n)));
            }
        }
        Ok(())
    }

    /// Stacca `n` dal padre e lo aggiunge in fondo ai figli di `target`.
    fn reparent(&mut self, n: u32, target: u32) {
        let old = self.info[n as usize].parent;
        if old == target {
            return;
        }
        if let SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } = &mut self.nodes[old as usize] {
            children.retain(|&c| c != n);
        }
        if let SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } = &mut self.nodes[target as usize] {
            children.push(n);
        }
        self.info[n as usize].parent = target;
        self.info[old as usize].restructured = true;
        self.info[target as usize].restructured = true;
    }

    /// Sposta i nodi (e il loro contenuto) dentro il gruppo `target`, in fondo ai suoi figli.
    /// Se `target` è un pezzo, li sposta nel suo gruppo. Ritorna quanti nodi ha spostato.
    pub fn move_into(&mut self, nodes: &[u32], target: u32) -> Result<usize, String> {
        let info = self.info.get(target as usize).copied().ok_or("destinazione inesistente")?;
        if info.deleted {
            return Err("destinazione eliminata".to_string());
        }
        let target = match self.nodes[target as usize] {
            SceneNode::Shape { .. } => self.parent_of(target).ok_or("destinazione non valida")?,
            _ => target,
        };
        let tops: Vec<u32> = self.tops(nodes).into_iter().filter(|&n| self.info[n as usize].parent != target).collect();
        if tops.is_empty() {
            return Ok(0);
        }
        self.check_move(&tops, target)?;
        for &n in &tops {
            self.reparent(n, target);
        }
        self.recompute_shape_counts();
        self.revision += 1;
        Ok(tops.len())
    }

    /// Raccoglie i nodi in un gruppo nuovo, al posto del primo di loro, nel gruppo che li contiene
    /// tutti. Il gruppo ha il sistema di riferimento del gruppo in cui nasce. Ritorna il gruppo.
    pub fn new_group(&mut self, nodes: &[u32], name: &str) -> Result<u32, String> {
        let tops = self.tops(nodes);
        let first = *tops.first().ok_or("nessun elemento da raggruppare")?;
        // Gruppo comune: il più vicino che li contiene tutti
        let mut parent = self.parent_of(first).ok_or("un file intero non si raggruppa")?;
        while !tops.iter().all(|&n| self.ancestors(n).contains(&parent)) {
            parent = self.parent_of(parent).ok_or("elementi di file diversi")?;
        }
        let source = self.source_of_node(parent).ok_or("file non trovato")?;
        for &n in &tops {
            if self.source_of_node(n) != Some(source) {
                return Err("si raggruppa solo dentro lo stesso file".to_string());
            }
            if self.info[n as usize].parent == NO_PARENT {
                return Err("un file intero non si raggruppa".to_string());
            }
        }

        let at = match &self.nodes[parent as usize] {
            SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } => children.iter().position(|&c| tops.contains(&c)),
            _ => None,
        };
        let group = self.add_group(parent, source, at, name);
        for &n in &tops {
            self.reparent(n, group);
        }
        self.recompute_shape_counts();
        self.revision += 1;
        Ok(group)
    }

    /// Gruppo vuoto dentro `target` (un file o un gruppo; se è un pezzo, nel suo gruppo), in fondo
    /// ai figli: poi ci si trascinano dentro gli elementi. Ritorna il gruppo.
    pub fn new_empty_group(&mut self, target: u32, name: &str) -> Result<u32, String> {
        let info = self.info.get(target as usize).copied().ok_or("elemento inesistente")?;
        if info.deleted {
            return Err("elemento eliminato".to_string());
        }
        let parent = match self.nodes[target as usize] {
            SceneNode::Shape { .. } => self.parent_of(target).ok_or("pezzo senza gruppo")?,
            _ => target,
        };
        let source = self.source_of_node(parent).ok_or("file non trovato")?;
        let group = self.add_group(parent, source, None, name);
        self.recompute_shape_counts();
        self.revision += 1;
        Ok(group)
    }

    /// Nodo di gruppo nuovo e vuoto nel contenitore `parent` (file `source`), alla posizione `at`
    /// tra i figli (`None`: in fondo), con il sistema di riferimento del contenitore.
    fn add_group(&mut self, parent: u32, source: usize, at: Option<usize>, name: &str) -> u32 {
        let group = self.nodes.len() as u32;
        let origin = self.sources[source].next_origin;
        self.sources[source].next_origin += 1;
        self.nodes.push(SceneNode::Group { name: Some(name.to_string()), children: Vec::new(), visible: true, selected: false });
        let mut info = self.info[parent as usize];
        info.parent = parent;
        info.kind = NodeKind::Group;
        info.color = None;
        info.renamed = false;
        info.origin = origin;
        info.created = true;
        info.restructured = false;
        info.deleted = false;
        self.info.push(info);
        if !self.frames.is_empty() {
            let frame = self.frames.get(parent as usize).copied().unwrap_or(glam::Mat4::IDENTITY);
            self.frames.resize(group as usize, glam::Mat4::IDENTITY);
            self.frames.push(frame);
        }
        if let SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } = &mut self.nodes[parent as usize] {
            let at = at.unwrap_or(children.len()).min(children.len());
            children.insert(at, group);
        }
        self.info[parent as usize].restructured = true;
        group
    }

    /// Ricostruisce la struttura salvata in un progetto: gruppi creati (numero fisso, nome) e
    /// figli dei contenitori cambiati (numero fisso del contenitore → numeri fissi dei figli, in
    /// ordine). Ritorna quante voci non si sono potute applicare.
    pub fn apply_structure(&mut self, source: usize, groups: &[(u32, String)], children: &[(u32, Vec<u32>)]) -> usize {
        let mut skipped = 0;
        let mut map = self.origin_map(source);
        let Some(root) = self.sources.get(source).map(|s| s.node) else { return groups.len() + children.len() };
        for (origin, name) in groups {
            if map.contains_key(origin) {
                skipped += 1;
                continue;
            }
            let group = self.nodes.len() as u32;
            self.nodes.push(SceneNode::Group { name: Some(name.clone()), children: Vec::new(), visible: true, selected: false });
            let mut info = self.info[root as usize];
            info.parent = root;
            info.kind = NodeKind::Group;
            info.color = None;
            info.renamed = false;
            info.origin = *origin;
            info.created = true;
            info.restructured = false;
            self.info.push(info);
            if let SceneNode::Group { children, .. } = &mut self.nodes[root as usize] {
                children.push(group);
            }
            if !self.frames.is_empty() {
                self.frames.resize(group as usize, glam::Mat4::IDENTITY);
                self.frames.push(glam::Mat4::IDENTITY);
            }
            let next = &mut self.sources[source].next_origin;
            *next = (*next).max(origin + 1);
            map.insert(*origin, group);
        }
        for (parent, list) in children {
            let Some(&p) = map.get(parent) else {
                skipped += 1;
                continue;
            };
            let nodes: Vec<u32> = list.iter().filter_map(|o| map.get(o).copied()).collect();
            skipped += list.len() - nodes.len();
            for &n in &nodes {
                if n != p && !self.ancestors(p).contains(&n) {
                    self.reparent(n, p);
                }
            }
            // Ordine dei figli come nel salvataggio
            if let SceneNode::Group { children, .. } | SceneNode::Transform { children, .. } = &mut self.nodes[p as usize] {
                let mut ordered: Vec<u32> = nodes.iter().copied().filter(|n| children.contains(n)).collect();
                ordered.extend(children.iter().copied().filter(|c| !nodes.contains(c)));
                *children = ordered;
            }
            self.info[p as usize].restructured = true;
        }
        // Sistema di riferimento dei gruppi creati: quello del gruppo in cui stanno
        if !self.frames.is_empty() {
            for (origin, _) in groups {
                if let Some(&g) = map.get(origin) {
                    let parent = self.info[g as usize].parent;
                    self.frames[g as usize] = self.frames.get(parent as usize).copied().unwrap_or(glam::Mat4::IDENTITY);
                }
            }
        }
        self.recompute_shape_counts();
        self.revision += 1;
        skipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_graph::node::multi_file_tests::two_files;

    fn by_name(scene: &SceneGraph, name: &str) -> u32 {
        (0..scene.nodes.len() as u32).find(|&n| !scene.info[n as usize].deleted && scene.display_name(n) == name).unwrap()
    }

    #[test]
    fn a_piece_moves_into_another_group_of_the_same_file() {
        let mut scene = two_files();
        let (telaio, trave) = (by_name(&scene, "Telaio"), by_name(&scene, "Trave"));
        let root_a = scene.sources[0].node;
        let vertices = scene.shapes_data.clone();
        assert_eq!(scene.move_into(&[trave], root_a), Ok(1));
        assert_eq!(scene.parent_of(trave), Some(root_a));
        assert!(!scene.nodes[telaio as usize].children().contains(&trave));
        assert_eq!(scene.nodes[root_a as usize].children().last(), Some(&trave));
        assert_eq!(scene.info[telaio as usize].shape_count, 1);
        assert_eq!(scene.info[root_a as usize].shape_count, 2);
        // La geometria non si muove, i numeri dei nodi restano
        assert_eq!(scene.shapes_data.len(), vertices.len());
        assert_eq!(scene.display_name(trave), "Trave");
        assert!(scene.info[telaio as usize].restructured && scene.info[root_a as usize].restructured);
        // Spostato su un pezzo: va nel gruppo del pezzo
        let copia = by_name(&scene, "Copia");
        assert_eq!(scene.move_into(&[trave], copia), Ok(1));
        assert_eq!(scene.parent_of(trave), Some(telaio));
        // Già lì: niente da fare
        assert_eq!(scene.move_into(&[trave], telaio), Ok(0));
    }

    #[test]
    fn moves_that_make_no_sense_are_refused() {
        let mut scene = two_files();
        let (telaio, trave, sportello) = (by_name(&scene, "Telaio"), by_name(&scene, "Trave"), by_name(&scene, "Sportello"));
        let root_a = scene.sources[0].node;
        assert!(scene.move_into(&[trave], sportello).unwrap_err().contains("stesso file"));
        assert!(scene.move_into(&[telaio], trave).unwrap_err().contains("sé stesso"));
        assert!(scene.move_into(&[root_a], sportello).is_err());
        assert_eq!(scene.parent_of(trave), Some(telaio), "niente è cambiato");
    }

    #[test]
    fn selected_pieces_become_a_new_group() {
        let mut scene = two_files();
        let (telaio, trave, copia) = (by_name(&scene, "Telaio"), by_name(&scene, "Trave"), by_name(&scene, "Copia"));
        let next = scene.sources[0].next_origin;
        let group = scene.new_group(&[copia, trave], "Lato sinistro").unwrap();
        assert_eq!(scene.display_name(group), "Lato sinistro");
        assert_eq!(scene.parent_of(group), Some(telaio));
        assert_eq!(scene.nodes[telaio as usize].children(), &[group]);
        assert_eq!(scene.nodes[group as usize].children(), &[trave, copia], "ordine dell'albero");
        assert_eq!(scene.info[group as usize].shape_count, 2);
        assert_eq!(scene.info[group as usize].origin, next);
        assert!(scene.info[group as usize].created);
        assert_eq!(scene.source_of_node(group), Some(0), "il gruppo nuovo resta nel suo file");
        let order = scene.tree_order();
        let at = |n: u32| order.iter().position(|&x| x == n).unwrap();
        assert!(at(telaio) < at(group) && at(group) < at(trave));
        // Pezzi di due file: no
        let sportello = by_name(&scene, "Sportello");
        assert!(scene.new_group(&[trave, sportello], "x").is_err());
    }

    /// L'export segue l'albero modificato: gruppo nuovo e pezzo spostato ci sono, con la
    /// geometria nello stesso posto; un pezzo nascosto dentro il gruppo nuovo non si esporta.
    #[test]
    fn export_follows_the_new_tree() {
        let mut scene = two_files();
        let (trave, copia) = (by_name(&scene, "Trave"), by_name(&scene, "Copia"));
        let sportello = by_name(&scene, "Sportello");
        let pannello = by_name(&scene, "Pannello");
        let group = scene.new_group(&[trave, copia], "Lato sinistro").unwrap();
        scene.move_into(&[pannello], scene.sources[1].node).unwrap();
        scene.set_subtree_visible(copia, false);
        let path = std::env::temp_dir().join("dima3d_export_albero.wrl");
        crate::exporter::export_with(&scene, &crate::exporter::writer::Scope::Visible, &path, crate::exporter::ExportOptions::default()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let lato = text.find("DEF Lato_sinistro Group").expect("gruppo nuovo nell'export");
        assert!(text[lato..].find("DEF Trave").is_some(), "{text}");
        assert!(!text.contains("DEF Copia "), "pezzo nascosto: {text}");
        let again = crate::parser::parse_wrl(text.as_bytes());
        assert_eq!(again.shapes_data.len(), 3);
        let names: Vec<String> = (0..again.nodes.len() as u32).map(|n| again.display_name(n)).collect();
        assert!(names.contains(&"Lato_sinistro".to_string()) && names.contains(&"Sportello".to_string()), "{names:?}");
        assert_eq!(scene.parent_of(trave), Some(group));
        assert_eq!(scene.parent_of(sportello), Some(scene.sources[1].node));
        // Stessi vertici dei pezzi visibili (al millesimo, in qualunque ordine)
        let hidden = match scene.nodes[copia as usize] {
            SceneNode::Shape { mesh_id, .. } => mesh_id,
            _ => unreachable!(),
        };
        let key = |v: &[f32; 3]| v.map(|c| (c * 1000.0).round() as i64);
        let mut a: Vec<[i64; 3]> = scene
            .shapes_data
            .iter()
            .enumerate()
            .filter(|(m, _)| *m as u32 != hidden)
            .flat_map(|(_, s)| s.world_vertices.iter().map(key))
            .collect();
        let mut b: Vec<[i64; 3]> = again.shapes_data.iter().flat_map(|s| s.world_vertices.iter().map(key)).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    /// Struttura salvata e riapplicata su un file appena riaperto: stesso albero.
    #[test]
    fn saved_structure_is_rebuilt() {
        let mut scene = two_files();
        let (trave, copia) = (by_name(&scene, "Trave"), by_name(&scene, "Copia"));
        let root_a = scene.sources[0].node;
        let group = scene.new_group(&[copia], "Gruppo nuovo").unwrap();
        scene.move_into(&[trave], root_a).unwrap();
        let origin = |s: &SceneGraph, n: u32| s.info[n as usize].origin;
        let groups = vec![(origin(&scene, group), "Gruppo nuovo".to_string())];
        let children: Vec<(u32, Vec<u32>)> = scene
            .file_nodes(0)
            .into_iter()
            .filter(|&n| scene.info[n as usize].restructured)
            .map(|n| (origin(&scene, n), scene.nodes[n as usize].children().iter().map(|&c| origin(&scene, c)).collect()))
            .collect();

        let mut fresh = two_files();
        assert_eq!(fresh.apply_structure(0, &groups, &children), 0);
        let shape = |s: &SceneGraph| {
            let mut rows = Vec::new();
            let mut stack = vec![(s.sources[0].node, 0)];
            while let Some((n, d)) = stack.pop() {
                rows.push((d, s.display_name(n), s.info[n as usize].shape_count));
                stack.extend(s.nodes[n as usize].children().iter().rev().map(|&c| (c, d + 1)));
            }
            rows
        };
        assert_eq!(shape(&fresh), shape(&scene));
        assert_eq!(fresh.sources[0].next_origin, scene.sources[0].next_origin);
    }
}
