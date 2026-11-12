//! Cerniere salvate: parti mobili (porte, cofani, sportelli) che si aprono dalla vista con i
//! gradi indicati.
//!
//! L'asse è scritto nelle coordinate di un pezzo della parte che si apre (`ref_mesh`): così segue
//! la parte quando si sposta tutto l'oggetto con Posizione, e non cambia quando la parte ruota
//! attorno a lui (una rotazione attorno a un asse lascia l'asse dov'è). L'angolo è quello di
//! apertura dalla posizione chiusa (0°), tra i limiti `min` e `max`.

use glam::Vec3;

use super::SceneGraph;
use crate::scene_graph::placement::about_axis;

#[derive(Debug, Clone, PartialEq)]
pub struct SavedHinge {
    pub name: String,
    /// Nodi della parte che si apre
    pub nodes: Vec<u32>,
    /// Pezzo della parte a cui è attaccato l'asse
    pub ref_mesh: u32,
    /// Punto e direzione dell'asse nelle coordinate di `ref_mesh` (la sua posizione accumulata)
    pub point: Vec3,
    pub dir: Vec3,
    /// Apertura attuale e limiti (gradi)
    pub angle: f32,
    pub min: f32,
    pub max: f32,
}

impl SavedHinge {
    /// `nodi | angolo min max | px py pz dx dy dz | nome`, con i nodi e la mesh di riferimento
    /// già numerati dall'inizio del file (`ref` prima dei nodi).
    pub fn to_text(&self, nodes: &[u32], ref_mesh: u32) -> String {
        let list: Vec<String> = nodes.iter().map(u32::to_string).collect();
        let (p, d) = (self.point, self.dir);
        format!(
            "{ref_mesh} {} | {} {} {} | {} {} {} {} {} {} | {}",
            list.join(" "),
            self.angle,
            self.min,
            self.max,
            p.x,
            p.y,
            p.z,
            d.x,
            d.y,
            d.z,
            self.name.replace(['\r', '\n'], " ")
        )
    }

    /// Da [`to_text`](Self::to_text): la cerniera con i numeri del file ancora da tradurre
    /// (`ref_mesh` e `nodes` relativi al file).
    pub fn from_text(text: &str) -> Option<SavedHinge> {
        let mut parts = text.splitn(4, '|');
        let numbers: Vec<u32> = parts.next()?.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let (&ref_mesh, nodes) = numbers.split_first()?;
        let f = |s: &str| -> Option<Vec<f32>> { s.split_whitespace().map(|w| w.parse().ok()).collect() };
        let [angle, min, max] = <[f32; 3]>::try_from(f(parts.next()?)?).ok()?;
        let [px, py, pz, dx, dy, dz] = <[f32; 6]>::try_from(f(parts.next()?)?).ok()?;
        let dir = Vec3::new(dx, dy, dz).try_normalize()?;
        let name = parts.next().unwrap_or("").trim().to_string();
        (!nodes.is_empty() && min <= max).then(|| SavedHinge {
            name,
            nodes: nodes.to_vec(),
            ref_mesh,
            point: Vec3::new(px, py, pz),
            dir,
            angle,
            min,
            max,
        })
    }
}

impl SceneGraph {
    /// Cerniera ancora valida: la parte esiste e almeno un suo nodo non è eliminato.
    pub fn hinge_alive(&self, h: &SavedHinge) -> bool {
        (h.ref_mesh as usize) < self.shapes_data.len()
            && h.nodes.iter().any(|&n| self.info.get(n as usize).is_some_and(|i| !i.deleted))
    }

    /// Asse della cerniera nella scena: punto e direzione unitaria.
    pub fn hinge_axis(&self, h: &SavedHinge) -> (Vec3, Vec3) {
        let p = self.placement(h.ref_mesh);
        (p.transform_point3(h.point), p.transform_vector3(h.dir).normalize_or(h.dir))
    }

    /// Nuova cerniera dalla parte `nodes` e dall'asse nella scena (`origin`, `dir`), con la parte
    /// aperta di `angle` gradi. `None` se la parte non ha pezzi.
    pub fn new_hinge(&self, name: String, nodes: Vec<u32>, origin: Vec3, dir: Vec3, angle: f32) -> Option<SavedHinge> {
        let ref_mesh = *self.meshes_of(&nodes).first()?;
        let back = self.placement(ref_mesh).inverse();
        let dir = dir.try_normalize()?;
        Some(SavedHinge {
            name,
            nodes,
            ref_mesh,
            point: back.transform_point3(origin),
            dir: back.transform_vector3(dir).normalize_or(dir),
            angle,
            min: angle.min(0.0),
            max: angle.max(90.0),
        })
    }

    /// Indice della cerniera della parte che contiene il nodo `node` (il nodo è della parte o
    /// tutti i suoi pezzi lo sono): l'ultima creata se sono più d'una.
    pub fn hinge_of_node(&self, node: u32) -> Option<usize> {
        let meshes = self.meshes_of(&[node]);
        let mut chain = self.ancestors(node);
        chain.push(node);
        self.hinges.iter().enumerate().rev().find_map(|(i, h)| {
            if !self.hinge_alive(h) {
                return None;
            }
            let part = self.meshes_of(&h.nodes);
            // La selezione sta dentro la parte (non la contiene soltanto: scegliendo tutta la
            // scocca non compare l'etichetta della porta)
            let inside = h.nodes.iter().any(|n| chain.contains(n))
                || (!meshes.is_empty() && meshes.iter().all(|m| part.binary_search(m).is_ok()));
            inside.then_some(i)
        })
    }

    /// Indice della cerniera la cui parte contiene la mesh `mesh`.
    pub fn hinge_of_mesh(&self, mesh: u32) -> Option<usize> {
        let node = *self.mesh_to_node.get(mesh as usize)?;
        self.hinge_of_node(node)
    }

    /// Porta la cerniera `index` a `angle` gradi (fermandosi ai limiti): ruota la parte attorno
    /// all'asse. Ritorna l'angolo raggiunto.
    pub fn set_hinge_angle(&mut self, index: usize, angle: f32) -> Option<f32> {
        let h = self.hinges.get(index)?.clone();
        let wanted = angle.clamp(h.min, h.max);
        if !wanted.is_finite() {
            return None;
        }
        if (wanted - h.angle).abs() > 1e-5 {
            let (o, d) = self.hinge_axis(&h);
            let meshes = self.meshes_of(&h.nodes);
            self.transform_meshes(&meshes, about_axis(o, d, wanted - h.angle));
        }
        self.hinges[index].angle = wanted;
        Some(wanted)
    }

    /// Quanti gradi corrisponde uno spostamento del mouse sullo schermo, trascinando la parte
    /// presa nel punto `grab` (scena): il punto preso segue il mouse lungo il suo arco.
    /// `to_screen` proietta sulla vista. `None` se l'asse passa per il punto preso.
    pub fn hinge_drag_degrees(
        &self,
        index: usize,
        grab: Vec3,
        delta: glam::Vec2,
        to_screen: impl Fn(Vec3) -> Option<glam::Vec2>,
    ) -> Option<f32> {
        let h = self.hinges.get(index)?;
        let (o, d) = self.hinge_axis(h);
        let radial = (grab - o) - d * (grab - o).dot(d);
        let r = radial.length();
        if r < 1e-3 {
            return None;
        }
        // Dove va il punto preso per un piccolo angolo positivo, sullo schermo
        let step = 0.01_f32; // radianti
        let moved = about_axis(o, d, step.to_degrees()).transform_point3(grab);
        let (a, b) = (to_screen(grab)?, to_screen(moved)?);
        let along = b - a;
        let pixels_per_radian = along.length() / step;
        // Arco quasi di taglio (si muove verso chi guarda): un passo fisso per pixel
        let degrees = if pixels_per_radian < 20.0 {
            let side = glam::Vec2::new(-along.y, along.x);
            let dir2 = if along.length() > 1e-6 { along.normalize() } else { side.normalize_or_zero() };
            delta.dot(dir2) * 0.5
        } else {
            (delta.dot(along / along.length()) / pixels_per_radian).to_degrees()
        };
        Some(degrees)
    }

    /// Cerniere ancora valide (indice e cerniera).
    pub fn live_hinges(&self) -> impl Iterator<Item = (usize, &SavedHinge)> {
        self.hinges.iter().enumerate().filter(|(_, h)| self.hinge_alive(h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_roundtrip() {
        let h = SavedHinge {
            name: "Porta | ant. sx".into(),
            nodes: vec![4, 9],
            ref_mesh: 2,
            point: Vec3::new(1.5, -2.0, 3.0),
            dir: Vec3::Z,
            angle: 35.5,
            min: 0.0,
            max: 70.0,
        };
        let back = SavedHinge::from_text(&h.to_text(&h.nodes, h.ref_mesh)).unwrap();
        assert_eq!(back, h);
        assert!(SavedHinge::from_text("1 2 | 0 0 | 0 0 0 0 0 1 | x").is_none());
        assert!(SavedHinge::from_text("1 | 0 0 90 | 0 0 0 0 0 1 | senza nodi").is_none());
    }
}
