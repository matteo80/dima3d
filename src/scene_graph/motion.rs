//! Asse di rotazione da due posizioni dello stesso pezzo (es. sportello chiuso e aperto, due
//! matematiche già pronte), per i pezzi le cui cerniere non sono nel 3D.
//!
//! 1. I pezzi delle due posizioni si accoppiano (stesso ordine nel file, oppure stesso nome) e per
//!    ogni coppia con lo stesso numero di vertici si trova il moto rigido che porta la prima nella
//!    seconda, ai minimi quadrati (metodo dei quaternioni di Horn).
//! 2. I pezzi fermi (le parti fisse delle cerniere, se ci sono) si scartano; quelli mossi si
//!    raggruppano per moto (un pistone a gas si muove diversamente dallo sportello) e si tiene il
//!    gruppo più grande.
//! 3. Il moto del gruppo è una rotazione attorno a un asse più uno scorrimento lungo l'asse
//!    (teorema di Chasles): per una cerniera vera lo scorrimento è circa zero, ed è il controllo
//!    di qualità mostrato all'utente.

use glam::{DMat3, DMat4, DQuat, DVec3, Mat4, Vec3};

use super::SceneGraph;

/// Vertici usati al massimo per pezzo nell'adattamento (presi a intervalli regolari).
const SAMPLES: usize = 4000;

/// Moto rigido che porta `a` su `b` (punti corrispondenti) ai minimi quadrati, con lo scarto
/// quadratico medio. `None` con meno di tre punti.
pub fn fit_rigid(a: &[DVec3], b: &[DVec3]) -> Option<(DMat4, f64)> {
    if a.len() != b.len() || a.len() < 3 {
        return None;
    }
    let n = a.len() as f64;
    let ca = a.iter().copied().sum::<DVec3>() / n;
    let cb = b.iter().copied().sum::<DVec3>() / n;
    // Covarianza incrociata S[i][j] = Σ (a_i)(b_j)
    let mut s = [[0.0f64; 3]; 3];
    for (p, q) in a.iter().zip(b) {
        let (p, q) = ((*p - ca).to_array(), (*q - cb).to_array());
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += p[i] * q[j];
            }
        }
    }
    let [[sxx, sxy, sxz], [syx, syy, syz], [szx, szy, szz]] = s;
    let nm = [
        [sxx + syy + szz, syz - szy, szx - sxz, sxy - syx],
        [syz - szy, sxx - syy - szz, sxy + syx, szx + sxz],
        [szx - sxz, sxy + syx, -sxx + syy - szz, syz + szy],
        [sxy - syx, szx + sxz, syz + szy, -sxx - syy + szz],
    ];
    let q = largest_eigenvector(nm);
    let rot = DQuat::from_xyzw(q[1], q[2], q[3], q[0]).normalize();
    let r = DMat3::from_quat(rot);
    let t = cb - r * ca;
    let m = DMat4::from_rotation_translation(rot, t);
    let rms = (a.iter().zip(b).map(|(p, q)| (r * *p + t - *q).length_squared()).sum::<f64>() / n).sqrt();
    Some((m, rms))
}

/// Autovettore dell'autovalore più grande di una matrice 4×4 simmetrica (metodo di Jacobi).
fn largest_eigenvector(mut a: [[f64; 4]; 4]) -> [f64; 4] {
    let mut v = [[0.0f64; 4]; 4];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..64 {
        let off: f64 = (0..4).flat_map(|i| (0..4).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        if off < 1e-30 {
            break;
        }
        for p in 0..3 {
            for q in p + 1..4 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..4 {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..4 {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vkp, vkq) = (row[p], row[q]);
                    row[p] = c * vkp - s * vkq;
                    row[q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let best = (0..4).max_by(|&i, &j| a[i][i].total_cmp(&a[j][j])).unwrap_or(0);
    [v[0][best], v[1][best], v[2][best], v[3][best]]
}

/// Asse di un moto rigido: rotazione di `angle` gradi attorno alla retta per `point` con
/// direzione `dir` (mano destra), più `slide` lungo l'asse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screw {
    pub point: DVec3,
    pub dir: DVec3,
    pub angle: f64,
    pub slide: f64,
}

/// Asse del moto `m`; `None` se non c'è rotazione (meno di 0,01°: solo uno spostamento).
pub fn screw_axis(m: DMat4) -> Option<Screw> {
    let r = DMat3::from_mat4(m);
    let t = m.w_axis.truncate();
    let v = DVec3::new(r.y_axis.z - r.z_axis.y, r.z_axis.x - r.x_axis.z, r.x_axis.y - r.y_axis.x);
    let cos = ((r.x_axis.x + r.y_axis.y + r.z_axis.z) - 1.0) / 2.0;
    let angle = (v.length() / 2.0).atan2(cos);
    if angle.to_degrees() < 0.01 {
        return None;
    }
    let dir = if v.length() > 1e-6 {
        v.normalize()
    } else {
        // Circa 180°: l'asse è la colonna più lunga di R + I
        let p = r + DMat3::IDENTITY;
        [p.x_axis, p.y_axis, p.z_axis].into_iter().max_by(|a, b| a.length().total_cmp(&b.length()))?.normalize()
    };
    let slide = dir.dot(t);
    let perp = t - dir * slide;
    let point = (perp + dir.cross(t) / (angle / 2.0).tan()) / 2.0;
    Some(Screw { point, dir, angle: angle.to_degrees(), slide })
}

/// Risultato del confronto tra due posizioni.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    /// Punto dell'asse (il più vicino al centro dei pezzi mossi) e direzione
    pub point: Vec3,
    pub dir: Vec3,
    /// Angolo dalla prima alla seconda posizione (gradi, mano destra attorno a `dir`)
    pub angle: f32,
    /// Scorrimento lungo l'asse (mm): circa 0 per una cerniera
    pub slide: f32,
    /// Scarto medio dei vertici dopo la rotazione trovata (mm)
    pub rms: f32,
    /// Pezzi che si muovono insieme (usati per l'asse), fermi, con un altro moto, non accoppiati
    pub moving: usize,
    pub fixed: usize,
    pub other: usize,
    pub unmatched: usize,
    /// Mesh della prima posizione che ruotano (le altre, es. le parti fisse delle cerniere, no)
    pub moving_meshes: Vec<u32>,
}

/// Vertici di una mesh campionati a intervalli regolari, in f64.
fn samples(scene: &SceneGraph, mesh: u32) -> Vec<DVec3> {
    let v = &scene.shapes_data[mesh as usize].world_vertices;
    let step = v.len().div_ceil(SAMPLES).max(1);
    v.iter().step_by(step).map(|p| Vec3::from(*p).as_dvec3()).collect()
}

/// Coppie di mesh corrispondenti tra le due posizioni: per ordine se il numero di pezzi è lo
/// stesso, altrimenti per nome; servono anche lo stesso numero di vertici.
fn pair_meshes(scene: &SceneGraph, a: &[u32], b: &[u32]) -> (Vec<(u32, u32)>, usize) {
    let count = |m: u32| scene.shapes_data[m as usize].world_vertices.len();
    let mut pairs = Vec::new();
    if a.len() == b.len() {
        pairs = a.iter().zip(b).filter(|(x, y)| count(**x) == count(**y) && count(**x) >= 3).map(|(x, y)| (*x, *y)).collect();
    }
    if pairs.len() * 2 < a.len().min(b.len()) {
        let name = |m: u32| scene.mesh_to_node.get(m as usize).map(|&n| scene.display_name(n)).unwrap_or_default();
        let mut used = vec![false; b.len()];
        pairs.clear();
        for &x in a {
            let found = b.iter().enumerate().find(|(i, &y)| !used[*i] && count(y) == count(x) && count(x) >= 3 && name(y) == name(x));
            if let Some((i, &y)) = found {
                used[i] = true;
                pairs.push((x, y));
            }
        }
    }
    let unmatched = a.len().max(b.len()) - pairs.len();
    (pairs, unmatched)
}

impl SceneGraph {
    /// Asse di rotazione che porta i pezzi di `first` (prima posizione) su quelli di `second`.
    pub fn motion_between(&self, first: &[u32], second: &[u32]) -> Result<Motion, String> {
        let (a, b) = (self.meshes_of(first), self.meshes_of(second));
        if a.is_empty() || b.is_empty() {
            return Err("Scegli le due posizioni".to_string());
        }
        if a.iter().any(|m| b.contains(m)) {
            return Err("Le due posizioni devono essere oggetti diversi".to_string());
        }
        let (pairs, unmatched) = pair_meshes(self, &a, &b);
        if pairs.is_empty() {
            return Err("Nessun pezzo corrisponde tra le due posizioni (servono la stessa matematica e gli stessi pezzi)".to_string());
        }
        let size = pairs
            .iter()
            .fold(super::Aabb::empty(), |mut bb, &(m, _)| {
                bb.extend_aabb(&self.mesh_bboxes[m as usize]);
                bb
            })
            .diagonal()
            .max(1.0) as f64;
        // Tolleranza: 0,05 mm su una scena in mm, in proporzione alla grandezza
        let tol = (size * 2e-5).max(0.05_f64.min(size * 1e-4));

        let sampled: Vec<(Vec<DVec3>, Vec<DVec3>)> = pairs.iter().map(|&(x, y)| (samples(self, x), samples(self, y))).collect();
        let mut fixed = 0;
        // Gruppi di pezzi con lo stesso moto: (moto, indici delle coppie, vertici)
        let mut groups: Vec<(DMat4, Vec<usize>, usize)> = Vec::new();
        let mut deformed = 0;
        for (i, (pa, pb)) in sampled.iter().enumerate() {
            let still = pa.iter().zip(pb).all(|(p, q)| p.distance(*q) <= tol);
            if still {
                fixed += 1;
                continue;
            }
            let fits = |m: &DMat4| (pa.iter().zip(pb).map(|(p, q)| (m.transform_point3(*p) - *q).length_squared()).sum::<f64>() / pa.len() as f64).sqrt();
            if let Some(g) = groups.iter_mut().find(|g| fits(&g.0) <= tol * 4.0) {
                g.1.push(i);
                g.2 += pa.len();
                continue;
            }
            match fit_rigid(pa, pb) {
                Some((m, rms)) if rms <= tol * 4.0 => groups.push((m, vec![i], pa.len())),
                _ => deformed += 1,
            }
        }
        let Some(best) = groups.iter().enumerate().max_by_key(|(_, g)| g.2).map(|(i, _)| i) else {
            return Err(if fixed > 0 && deformed == 0 {
                "I pezzi sono nella stessa posizione nei due oggetti".to_string()
            } else {
                "I pezzi non si muovono come un corpo rigido tra le due posizioni".to_string()
            });
        };
        let other = groups.len() - 1 + deformed;
        let members = &groups[best].1;
        // Adattamento finale su tutti i pezzi del gruppo
        let (pa, pb): (Vec<DVec3>, Vec<DVec3>) = members
            .iter()
            .flat_map(|&i| sampled[i].0.iter().copied().zip(sampled[i].1.iter().copied()))
            .unzip();
        let (m, rms) = fit_rigid(&pa, &pb).ok_or("Troppo pochi vertici")?;
        let screw = screw_axis(m).ok_or("I pezzi si spostano senza ruotare: non c'è un asse di rotazione")?;
        // Punto dell'asse vicino ai pezzi mossi
        let centre = pa.iter().copied().sum::<DVec3>() / pa.len() as f64;
        let point = screw.point + screw.dir * screw.dir.dot(centre - screw.point);
        Ok(Motion {
            point: point.as_vec3(),
            dir: screw.dir.as_vec3(),
            angle: screw.angle as f32,
            slide: screw.slide as f32,
            rms: rms as f32,
            moving: members.len(),
            fixed,
            other,
            unmatched,
            moving_meshes: members.iter().map(|&i| pairs[i].0).collect(),
        })
    }
}

/// Matrice della rotazione trovata (senza lo scorrimento), per controlli.
#[allow(dead_code)]
pub fn rotation_of(m: &Motion) -> Mat4 {
    super::placement::about_axis(m.point, m.dir, m.angle)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn cloud() -> Vec<DVec3> {
        // Punti di uno «sportello» qualsiasi, non complanari
        (0..60).map(|i| {
            let f = i as f64;
            DVec3::new(1200.0 + (f * 7.3).sin() * 300.0, -400.0 + (f * 3.1).cos() * 250.0, 900.0 + (f * 1.7).sin() * 40.0 + f)
        }).collect()
    }

    /// Rotazione nota attorno a un asse noto: asse, angolo e scorrimento nullo ritrovati, anche con
    /// un po' di rumore sui punti; la sola traslazione non dà un asse.
    #[test]
    fn known_rotation_is_recovered() {
        let point = DVec3::new(1000.0, -500.0, 950.0);
        let dir = DVec3::new(0.2, 1.0, 0.1).normalize();
        let m = DMat4::from_translation(point) * DMat4::from_axis_angle(dir, 34f64.to_radians()) * DMat4::from_translation(-point);
        let a = cloud();
        let b: Vec<DVec3> = a.iter().map(|p| m.transform_point3(*p)).collect();
        let (fit, rms) = fit_rigid(&a, &b).unwrap();
        assert!(rms < 1e-6, "{rms}");
        let s = screw_axis(fit).unwrap();
        assert!((s.angle - 34.0).abs() < 1e-6, "{s:?}");
        assert!((s.dir - dir).length() < 1e-6, "{s:?}");
        assert!(s.slide.abs() < 1e-6);
        // Il punto trovato sta sull'asse
        assert!((s.point - point).cross(dir).length() < 1e-6, "{s:?}");

        // Rumore di ±0,1 mm: asse e angolo restano buoni
        let noisy: Vec<DVec3> = b.iter().enumerate().map(|(i, p)| *p + DVec3::splat(((i * 37 % 11) as f64 - 5.0) * 0.02)).collect();
        let (fit, rms) = fit_rigid(&a, &noisy).unwrap();
        assert!(rms < 0.15, "{rms}");
        let s = screw_axis(fit).unwrap();
        assert!((s.angle - 34.0).abs() < 0.05 && (s.dir - dir).length() < 1e-3, "{s:?}");

        // 180°: asse ancora trovato
        let half = DMat4::from_translation(point) * DMat4::from_axis_angle(dir, std::f64::consts::PI) * DMat4::from_translation(-point);
        let b: Vec<DVec3> = a.iter().map(|p| half.transform_point3(*p)).collect();
        let s = screw_axis(fit_rigid(&a, &b).unwrap().0).unwrap();
        assert!((s.angle - 180.0).abs() < 1e-4 && s.dir.dot(dir).abs() > 0.999999, "{s:?}");
        assert!((s.point - point).cross(dir).length() < 1e-4, "{s:?}");

        let shift = DMat4::from_translation(DVec3::new(10.0, 0.0, 0.0));
        let b: Vec<DVec3> = a.iter().map(|p| shift.transform_point3(*p)).collect();
        assert!(screw_axis(fit_rigid(&a, &b).unwrap().0).is_none());
    }

    pub(crate) fn scene_with(second: Mat4) -> SceneGraph {
        // Stessa «matematica» due volte: una cerniera fissa e uno sportello; nella seconda copia lo
        // sportello è ruotato con `second`
        let wrl = |m: Mat4| {
            let pts = |v: &[Vec3]| v.iter().map(|p| format!("{} {} {}", p.x, p.y, p.z)).collect::<Vec<_>>().join(", ");
            let fixed = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 5.0), Vec3::new(3.0, 2.0, 9.0)];
            let door: Vec<Vec3> = cloud().iter().map(|p| m.transform_point3(p.as_vec3())).collect();
            format!(
                "#VRML V2.0 utf8\nDEF Cerniera Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ {} ] }} coordIndex [ 0 1 2 -1 1 2 3 -1 ] }} }}\n\
                 DEF Sportello Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ {} ] }} coordIndex [ {} ] }} }}\n",
                pts(&fixed),
                pts(&door),
                (0..58).map(|i| format!("{} {} {} -1", i, i + 1, i + 2)).collect::<Vec<_>>().join(" ")
            )
        };
        let mut scene = crate::parser::parse_wrl(wrl(Mat4::IDENTITY).as_bytes());
        scene.into_file("chiuso.wrl".into(), None);
        let mut open = crate::parser::parse_wrl(wrl(second).as_bytes());
        open.into_file("aperto.wrl".into(), None);
        scene.append(open);
        scene
    }

    /// Due file della stessa matematica: la cerniera fissa è ferma, lo sportello ruotato. L'asse
    /// trovato, usato come cerniera sulla prima posizione, la porta sulla seconda.
    #[test]
    fn axis_between_two_files() {
        let point = Vec3::new(1000.0, -500.0, 950.0);
        let dir = Vec3::new(0.0, 0.3, 1.0).normalize();
        let scene = scene_with(super::super::placement::about_axis(point, dir, 85.0));
        let files: Vec<u32> = scene.open_files().map(|f| f.node).collect();
        let m = scene.motion_between(&[files[0]], &[files[1]]).unwrap();
        assert_eq!((m.moving, m.fixed, m.other, m.unmatched), (1, 1, 0, 0), "{m:?}");
        assert!((m.angle - 85.0).abs() < 0.01 && (m.dir - dir).length() < 1e-4, "{m:?}");
        assert!(m.slide.abs() < 0.01 && m.rms < 0.01, "{m:?}");
        assert!((m.point - point).cross(dir).length() < 0.05, "{m:?}");

        // Al contrario: angolo uguale, verso opposto
        let back = scene.motion_between(&[files[1]], &[files[0]]).unwrap();
        assert!((back.angle - 85.0).abs() < 0.01 && (back.dir + dir).length() < 1e-4, "{back:?}");

        assert!(scene.motion_between(&[files[0]], &[files[0]]).is_err());
        let same = scene_with(Mat4::IDENTITY);
        let files: Vec<u32> = same.open_files().map(|f| f.node).collect();
        let e = same.motion_between(&[files[0]], &[files[1]]).unwrap_err();
        assert!(e.contains("stessa posizione"), "{e}");
    }

    /// Prova sulle matematiche vere (non nel repo): GESTORE_PROVA_ASSE = coppie di WRL
    /// «chiuso|aperto» separate da ';'.
    #[test]
    #[ignore]
    fn axis_from_real_files() {
        let Ok(list) = std::env::var("GESTORE_PROVA_ASSE") else { return };
        for pair in list.split(';') {
            let (a, b) = pair.split_once('|').expect("chiuso|aperto");
            let mut scene = crate::parser::parse_wrl(&std::fs::read(a).unwrap());
            scene.into_file(a.into(), None);
            let mut other = crate::parser::parse_wrl(&std::fs::read(b).unwrap());
            other.into_file(b.into(), None);
            scene.append(other);
            let files: Vec<u32> = scene.open_files().map(|f| f.node).collect();
            let t = std::time::Instant::now();
            let m = scene.motion_between(&[files[0]], &[files[1]]);
            println!("{a} -> {b}: {:?} in {:?}", m, t.elapsed());
            let m = m.unwrap();
            // Verifica: ruotando i pezzi della prima posizione si arriva sulla seconda
            let rot = rotation_of(&m);
            let (ma, mb) = (scene.meshes_of(&[files[0]]), scene.meshes_of(&[files[1]]));
            let mut worst = 0.0f32;
            let mut moved = 0;
            for (x, y) in ma.iter().zip(&mb) {
                let (va, vb) = (&scene.shapes_data[*x as usize].world_vertices, &scene.shapes_data[*y as usize].world_vertices);
                if va.len() != vb.len() || va.iter().zip(vb).all(|(p, q)| Vec3::from(*p).distance(Vec3::from(*q)) < 0.05) {
                    continue;
                }
                let e = va.iter().zip(vb).map(|(p, q)| rot.transform_point3(Vec3::from(*p)).distance(Vec3::from(*q))).fold(0.0, f32::max);
                if e < 1.0 {
                    moved += 1;
                    worst = worst.max(e);
                }
            }
            println!("  pezzi portati sulla seconda posizione: {moved}, scarto massimo {worst:.4} mm");
            assert!(moved >= m.moving);
        }
    }
}
