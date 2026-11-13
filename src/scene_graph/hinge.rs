//! Cerniere salvate: parti mobili (porte, cofani, sportelli) che si aprono dalla vista con i
//! gradi indicati.
//!
//! L'asse è scritto nelle coordinate di un pezzo della parte che si apre (`ref_mesh`): così segue
//! la parte quando si sposta tutto l'oggetto con Posizione, e non cambia quando la parte ruota
//! attorno a lui (una rotazione attorno a un asse lascia l'asse dov'è). L'angolo è quello di
//! apertura dalla posizione chiusa (0°), tra i limiti `min` e `max`.
//!
//! Cerniera a 4 perni (cofani, vedi `linkage.rs`): i perni sono scritti nelle coordinate del pezzo
//! di riferimento come sarebbe con il meccanismo a 0°. Dalla posizione attuale del pezzo e
//! dall'angolo si ricava quella a 0° (il meccanismo dice di quanto si è mosso il braccio), quindi
//! anche qui la cerniera segue l'oggetto spostato con Posizione. Ad ogni apertura si muovono la
//! parte e le due bielle.

use glam::{Mat4, Vec3};

use super::SceneGraph;
use super::linkage::{FourBar, LinkState};
use crate::scene_graph::placement::about_axis;

/// Parte a 4 perni di una cerniera salvata.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedLinkage {
    /// Centri dei perni F1, F2, M1, M2 con il meccanismo a 0°, nelle coordinate del pezzo di
    /// riferimento a 0° (la direzione comune dei perni è `SavedHinge::dir`)
    pub pins: [Vec3; 4],
    /// Nodi delle bielle 1 (F1–M1) e 2 (F2–M2), anche vuote
    pub links: [Vec<u32>; 2],
    /// Angolo del meccanismo con la cerniera a 0° (gradi): cambia con «0° qui»
    pub zero: f32,
}

/// Apertura massima cercata per il fine corsa del meccanismo (gradi)
const FOUR_BAR_MAX: f32 = 120.0;

/// Fine corsa in apertura del meccanismo dalla posizione dei perni (gradi, arrotondato per
/// difetto al decimo), al massimo [`FOUR_BAR_MAX`]. `bar` è nella posizione dei perni.
fn four_bar_top(bar: &FourBar) -> f32 {
    match bar.solve(FOUR_BAR_MAX) {
        Ok(_) => FOUR_BAR_MAX,
        Err(s) => (s.phi.to_degrees() as f32 * 10.0).floor() / 10.0,
    }
}

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
    /// Cerniera a 4 perni; `None` per quelle su un asse (`point` e `dir`)
    pub linkage: Option<SavedLinkage>,
}

impl SavedHinge {
    /// Chiave della riga nel progetto: le cerniere a 4 perni hanno la loro, che le versioni
    /// vecchie ignorano (le aprirebbero come cerniere su un asse).
    pub fn text_key(&self) -> &'static str {
        if self.linkage.is_some() { "cerniera_perni" } else { "cerniera" }
    }

    /// `nodi | angolo min max | px py pz dx dy dz | nome`, con i nodi e la mesh di riferimento
    /// già numerati dall'inizio del file (`ref` prima dei nodi).
    ///
    /// A 4 perni (riga «cerniera_perni», bielle già numerate dall'inizio del file):
    /// `ref nodi | angolo min max | dx dy dz | F1 F2 M1 M2 (12 numeri) zero | biella 1 / biella 2 | nome`.
    pub fn to_text(&self, nodes: &[u32], ref_mesh: u32) -> String {
        let list: Vec<String> = nodes.iter().map(u32::to_string).collect();
        let (p, d) = (self.point, self.dir);
        if let Some(l) = &self.linkage {
            let pins: Vec<String> = l.pins.iter().flat_map(|p| p.to_array()).map(|v| v.to_string()).collect();
            let links: Vec<String> =
                l.links.iter().map(|n| n.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")).collect();
            return format!(
                "{ref_mesh} {} | {} {} {} | {} {} {} | {} {} | {} | {}",
                list.join(" "),
                self.angle,
                self.min,
                self.max,
                d.x,
                d.y,
                d.z,
                pins.join(" "),
                l.zero,
                links.join(" / "),
                self.name.replace(['\r', '\n'], " ")
            );
        }
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
            linkage: None,
        })
    }

    /// Da [`to_text`](Self::to_text) di una cerniera a 4 perni (riga «cerniera_perni»).
    pub fn from_text_four(text: &str) -> Option<SavedHinge> {
        let mut parts = text.splitn(6, '|');
        let numbers: Vec<u32> = parts.next()?.split_whitespace().map(|w| w.parse().ok()).collect::<Option<_>>()?;
        let (&ref_mesh, nodes) = numbers.split_first()?;
        let f = |s: &str| -> Option<Vec<f32>> { s.split_whitespace().map(|w| w.parse().ok()).collect() };
        let [angle, min, max] = <[f32; 3]>::try_from(f(parts.next()?)?).ok()?;
        let [dx, dy, dz] = <[f32; 3]>::try_from(f(parts.next()?)?).ok()?;
        let dir = Vec3::new(dx, dy, dz).try_normalize()?;
        let v = <[f32; 13]>::try_from(f(parts.next()?)?).ok()?;
        let pins = [0, 1, 2, 3].map(|k| Vec3::new(v[3 * k], v[3 * k + 1], v[3 * k + 2]));
        let mut links =
            parts.next()?.split('/').map(|s| s.split_whitespace().map(|w| w.parse().ok()).collect::<Option<Vec<u32>>>());
        let links = [links.next().flatten()?, links.next().flatten()?];
        let name = parts.next().unwrap_or("").trim().to_string();
        (!nodes.is_empty() && min <= max && v.iter().all(|x| x.is_finite())).then(|| SavedHinge {
            name,
            nodes: nodes.to_vec(),
            ref_mesh,
            point: pins[0],
            dir,
            angle,
            min,
            max,
            linkage: Some(SavedLinkage { pins, links, zero: v[12] }),
        })
    }
}

impl SceneGraph {
    /// Cerniera ancora valida: la parte esiste e almeno un suo nodo non è eliminato.
    pub fn hinge_alive(&self, h: &SavedHinge) -> bool {
        (h.ref_mesh as usize) < self.shapes_data.len()
            && h.nodes.iter().any(|&n| self.info.get(n as usize).is_some_and(|i| !i.deleted))
    }

    /// Asse della cerniera nella scena: punto e direzione unitaria. A 4 perni: il perno fisso 1
    /// e la direzione dei perni.
    pub fn hinge_axis(&self, h: &SavedHinge) -> (Vec3, Vec3) {
        let p = match self.linkage_now(h) {
            Some((_, frame)) => frame,
            None => self.placement(h.ref_mesh),
        };
        (p.transform_point3(h.point), p.transform_vector3(h.dir).normalize_or(h.dir))
    }

    /// Cerniera a 4 perni: il meccanismo (nelle coordinate del pezzo di riferimento a 0°) nella
    /// posizione dell'angolo attuale, e la posizione del pezzo di riferimento a 0°.
    fn linkage_now(&self, h: &SavedHinge) -> Option<(FourBar, Mat4)> {
        let l = h.linkage.as_ref()?;
        let mut bar = FourBar::from_pins(l.pins.map(|p| (p, h.dir))).ok()?;
        let s = bar.solve(h.angle + l.zero).unwrap_or_else(|s| s);
        let frame = self.placement(h.ref_mesh) * bar.transforms(&s)[0].inverse();
        bar.state = s;
        Some((bar, frame))
    }

    /// Spostamenti nella scena di parte, biella 1 e biella 2 per portare il meccanismo dalla
    /// posizione attuale a `s`.
    fn linkage_moves(bar: &FourBar, frame: Mat4, s: &LinkState) -> [Mat4; 3] {
        let (old, new) = (bar.transforms(&bar.state), bar.transforms(s));
        let back = frame.inverse();
        [0, 1, 2].map(|k| frame * new[k] * old[k].inverse() * back)
    }

    /// Centri dei perni nella scena, nella posizione attuale (F1, F2, M1, M2). `None` se la
    /// cerniera è su un asse.
    pub fn hinge_pins(&self, h: &SavedHinge) -> Option<[Vec3; 4]> {
        let (bar, frame) = self.linkage_now(h)?;
        Some(bar.pins(&bar.state).map(|p| frame.transform_point3(p)))
    }

    /// Centri dei perni nella scena con il meccanismo a 0° (dove sono stati scelti).
    pub fn hinge_pins_at_zero(&self, h: &SavedHinge) -> Option<[Vec3; 4]> {
        let (_, frame) = self.linkage_now(h)?;
        Some(h.linkage.as_ref()?.pins.map(|p| frame.transform_point3(p)))
    }

    /// Fine corsa in apertura di una cerniera a 4 perni, nei gradi della cerniera.
    pub fn hinge_travel(&self, h: &SavedHinge) -> Option<f32> {
        let l = h.linkage.as_ref()?;
        let bar = FourBar::from_pins(l.pins.map(|p| (p, h.dir))).ok()?;
        Some(four_bar_top(&bar) - l.zero)
    }

    /// Nuova cerniera a 4 perni: parte `nodes` (cofano e braccio), bielle `links`, perni nella
    /// scena con il meccanismo a 0° (`pins`, direzione `normal`); il meccanismo ora è a `mech`
    /// gradi e la cerniera parte da `angle`. Il limite di apertura è il fine corsa.
    #[allow(clippy::too_many_arguments)]
    pub fn new_four_bar_hinge(
        &self,
        name: String,
        nodes: Vec<u32>,
        links: [Vec<u32>; 2],
        pins: [Vec3; 4],
        normal: Vec3,
        mech: f32,
        angle: f32,
    ) -> Option<SavedHinge> {
        let ref_mesh = *self.meshes_of(&nodes).first()?;
        let normal = normal.try_normalize()?;
        let bar = FourBar::from_pins(pins.map(|p| (p, normal))).ok()?;
        let zero = mech - angle;
        let top = four_bar_top(&bar);
        let s = bar.solve(mech).unwrap_or_else(|s| s);
        // Dalla scena alle coordinate del pezzo di riferimento con il meccanismo a 0°
        let back = (bar.transforms(&s)[0].inverse() * self.placement(ref_mesh)).inverse();
        let pins = pins.map(|p| back.transform_point3(p));
        Some(SavedHinge {
            name,
            nodes,
            ref_mesh,
            point: pins[0],
            dir: back.transform_vector3(normal).normalize_or(normal),
            angle,
            min: angle.min(0.0),
            max: (top - zero).max(angle),
            linkage: Some(SavedLinkage { pins, links, zero }),
        })
    }

    /// Verso dei perni (`normal` o `-normal`) per cui un angolo positivo apre il cofano `nodes`:
    /// quello in cui il meccanismo si muove (un cofano chiuso di solito è a fine corsa da una
    /// parte); se si muove da tutte e due, quello che allontana la parte dal centro della scena.
    pub fn outward_four_bar(&self, nodes: &[u32], pins: [Vec3; 4], normal: Vec3) -> Vec3 {
        let Ok(bar) = FourBar::from_pins(pins.map(|p| (p, normal))) else { return normal };
        let (open, close) = (bar.solve(10.0), bar.solve(-10.0));
        let (open, close) = match (open, close) {
            (Ok(o), Ok(c)) => (o, c),
            (Err(_), Ok(_)) => return -normal,
            _ => return normal,
        };
        let mut part = super::Aabb::empty();
        for &n in nodes {
            part.extend_aabb(&self.subtree_bounding_box(n));
        }
        let (scene, _) = self.main_visible_bounding_box();
        if !part.is_valid() || !scene.is_valid() {
            return normal;
        }
        let (c, center) = (part.center(), scene.center());
        let far = |s: &LinkState| bar.transforms(s)[0].transform_point3(c).distance(center);
        if far(&close) > far(&open) + 1e-3 { -normal } else { normal }
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
            linkage: None,
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
        if let Some(l) = &h.linkage {
            if (wanted - h.angle).abs() <= 1e-5 {
                return Some(h.angle);
            }
            let (bar, frame) = self.linkage_now(&h)?;
            // Oltre il fine corsa si ferma lì
            let s = bar.solve(wanted + l.zero).unwrap_or_else(|s| s);
            let reached = s.phi.to_degrees() as f32 - l.zero;
            if !reached.is_finite() {
                return None;
            }
            let [arm, link1, link2] = Self::linkage_moves(&bar, frame, &s);
            let part = self.meshes_of(&h.nodes);
            self.transform_meshes(&part, arm);
            for (nodes, m) in [(&l.links[0], link1), (&l.links[1], link2)] {
                let mut meshes = self.meshes_of(nodes);
                // Una biella dentro la parte si muove già con lei
                meshes.retain(|m| part.binary_search(m).is_err());
                self.transform_meshes(&meshes, m);
            }
            self.hinges[index].angle = reached;
            return Some(reached);
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
        let step = 0.01_f32; // radianti
        // Dove va il punto preso per un piccolo angolo positivo, sullo schermo
        let along = if let Some(l) = &h.linkage {
            // Il braccio non gira su un asse fisso: un passo del meccanismo (all'indietro se in
            // avanti è a fine corsa)
            let (bar, frame) = self.linkage_now(h)?;
            let mut out = None;
            for sign in [1.0_f32, -1.0] {
                if let Ok(s) = bar.solve(h.angle + l.zero + sign * step.to_degrees()) {
                    let moved = Self::linkage_moves(&bar, frame, &s)[0].transform_point3(grab);
                    out = Some((to_screen(moved)? - to_screen(grab)?) * sign);
                    break;
                }
            }
            out?
        } else {
            let (o, d) = self.hinge_axis(h);
            let radial = (grab - o) - d * (grab - o).dot(d);
            if radial.length() < 1e-3 {
                return None;
            }
            let moved = about_axis(o, d, step.to_degrees()).transform_point3(grab);
            to_screen(moved)? - to_screen(grab)?
        };
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

    /// Verso dell'asse (`d` o `-d`) per cui un angolo positivo apre la parte `nodes` verso
    /// l'esterno: il suo centro si allontana dal centro della scena (porte verso fuori, cofani e
    /// portelloni verso l'alto). `d` se non si capisce.
    pub fn outward_dir(&self, nodes: &[u32], o: Vec3, d: Vec3) -> Vec3 {
        let mut part = super::Aabb::empty();
        for &n in nodes {
            part.extend_aabb(&self.subtree_bounding_box(n));
        }
        let (scene, _) = self.main_visible_bounding_box();
        if !part.is_valid() || !scene.is_valid() {
            return d;
        }
        let (c, s) = (part.center(), scene.center());
        let opened = about_axis(o, d, 10.0).transform_point3(c);
        if opened.distance(s) < c.distance(s) - 1e-3 { -d } else { d }
    }

    /// Cerniere ancora valide (indice e cerniera).
    pub fn live_hinges(&self) -> impl Iterator<Item = (usize, &SavedHinge)> {
        self.hinges.iter().enumerate().filter(|(_, h)| self.hinge_alive(h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scocca in coordinate veicolo (X verso il retro, Y a destra, Z in alto) e porta sinistra
    /// (Y negativa) con la cerniera sul bordo anteriore: chiusa, il verso scelto la apre verso
    /// l'esterno (Y più negativa), qualunque sia il verso di partenza.
    #[test]
    fn closed_door_opens_outward() {
        let wrl = br#"#VRML V2.0 utf8
DEF Scocca Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 -900 0, 4000 -900 0, 4000 900 1500, 0 900 1500 ] } coordIndex [ 0 1 2 3 -1 ] } }
DEF Porta Shape { geometry IndexedFaceSet { coord Coordinate { point [ 1900 -900 500, 2900 -900 500, 2900 -900 1100, 1900 -900 1100 ] } coordIndex [ 0 1 2 3 -1 ] } }
"#;
        let scene = crate::parser::parse_wrl(wrl);
        let door = (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == "Porta").unwrap();
        let hinge = Vec3::new(1900.0, -900.0, 800.0);
        for start in [Vec3::Z, -Vec3::Z] {
            let d = scene.outward_dir(&[door], hinge, start);
            // Bordo posteriore della porta aperto di 30°: va verso Y più negativa
            let back = about_axis(hinge, d, 30.0).transform_point3(Vec3::new(2900.0, -900.0, 800.0));
            assert!(back.y < -1200.0, "verso {d}: {back}");
        }
    }

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
            linkage: None,
        };
        let back = SavedHinge::from_text(&h.to_text(&h.nodes, h.ref_mesh)).unwrap();
        assert_eq!(back, h);
        assert!(SavedHinge::from_text("1 2 | 0 0 | 0 0 0 0 0 1 | x").is_none());
        assert!(SavedHinge::from_text("1 | 0 0 90 | 0 0 0 0 0 1 | senza nodi").is_none());
    }
}
