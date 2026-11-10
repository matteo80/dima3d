//! Scostamento tra due copie della stessa geometria, ricavato dai vertici.
//!
//! Il calcolo «Tra due oggetti» confronta i sistemi di coordinate degli oggetti, cioè i
//! `Transform` del file. Se la posizione è già scritta nelle coordinate dei vertici (un export
//! che applica i `Transform`, due file caricati separatamente, un pezzo spostato e poi
//! esportato) i due sistemi coincidono e lo scostamento risulta 0. Quando i due oggetti sono la
//! stessa geometria (stesse mesh, stessi vertici nello stesso ordine) lo spostamento rigido che
//! porta l'uno sull'altro si ricava dai vertici stessi, con il metodo dei quaternioni di Horn
//! (B. K. P. Horn, «Closed-form solution of absolute orientation using unit quaternions», 1987).

use glam::{DMat4, DQuat, DVec3, Mat4};

use super::SceneGraph;

/// Coppie di punti usate al massimo: oltre si prende un vertice ogni tanto (il risultato non
/// cambia, il tempo sì).
const MAX_PAIRS: usize = 400_000;

/// Spostamento rigido trovato dai vertici.
#[derive(Debug, Clone, Copy)]
pub struct RigidFit {
    /// Porta i vertici del riferimento su quelli della destinazione (coordinate mondo)
    pub matrix: Mat4,
    /// Distanza massima tra un vertice spostato e il suo corrispondente
    pub max_error: f64,
    /// Coppie di vertici usate
    pub pairs: usize,
}

/// Rotazione e traslazione che portano `from[i]` su `to[i]` con il minimo errore quadratico.
pub fn rigid_fit(from: &[DVec3], to: &[DVec3]) -> Option<(DQuat, DVec3)> {
    if from.len() != to.len() || from.len() < 3 {
        return None;
    }
    let n = from.len() as f64;
    let ca = from.iter().copied().sum::<DVec3>() / n;
    let cb = to.iter().copied().sum::<DVec3>() / n;
    // Matrice di correlazione: s[i][j] = Σ a_i · b_j (punti centrati)
    let mut s = [[0.0f64; 3]; 3];
    for (a, b) in from.iter().zip(to) {
        let (a, b) = ((*a - ca).to_array(), (*b - cb).to_array());
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += a[i] * b[j];
            }
        }
    }
    let [[sxx, sxy, sxz], [syx, syy, syz], [szx, szy, szz]] = s;
    let m = [
        [sxx + syy + szz, syz - szy, szx - sxz, sxy - syx],
        [syz - szy, sxx - syy - szz, sxy + syx, szx + sxz],
        [szx - sxz, sxy + syx, -sxx + syy - szz, syz + szy],
        [sxy - syx, szx + sxz, syz + szy, -sxx - syy + szz],
    ];
    let [w, x, y, z] = largest_eigenvector(m)?;
    let rotation = DQuat::from_xyzw(x, y, z, w).normalize();
    if !rotation.is_finite() {
        return None;
    }
    Some((rotation, cb - rotation * ca))
}

/// Autovettore dell'autovalore maggiore di una matrice simmetrica 4×4 (metodo di Jacobi).
fn largest_eigenvector(mut a: [[f64; 4]; 4]) -> Option<[f64; 4]> {
    let mut v = [[0.0f64; 4]; 4];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..100 {
        let off: f64 = (0..4).flat_map(|i| (0..4).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        let scale: f64 = (0..4).map(|i| a[i][i] * a[i][i]).sum::<f64>().max(1e-300);
        if off <= scale * 1e-30 {
            break;
        }
        for p in 0..4 {
            for q in p + 1..4 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum().max(0.0).mul_add(2.0, -1.0) / (theta.abs() + (theta * theta + 1.0).sqrt());
                let (c, s) = (1.0 / (t * t + 1.0).sqrt(), t / (t * t + 1.0).sqrt());
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
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    let best = (0..4).max_by(|&i, &j| a[i][i].total_cmp(&a[j][j]))?;
    let vector = [v[0][best], v[1][best], v[2][best], v[3][best]];
    vector.iter().all(|x| x.is_finite()).then_some(vector)
}

impl SceneGraph {
    /// Spostamento rigido che porta la geometria di `reference` su quella di `destination`, se
    /// sono la stessa geometria: stesse mesh, con lo stesso numero di vertici, e vertici che
    /// dopo lo spostamento coincidono. Altrimenti il motivo.
    pub fn geometric_offset(&self, reference: u32, destination: u32) -> Result<RigidFit, String> {
        let (ma, mb) = (self.meshes_of(&[reference]), self.meshes_of(&[destination]));
        if ma.is_empty() || mb.is_empty() {
            return Err("uno dei due oggetti non ha pezzi".to_string());
        }
        if ma.iter().any(|m| mb.contains(m)) {
            return Err("un oggetto contiene l'altro".to_string());
        }
        if ma.len() != mb.len() {
            return Err(format!("numero di pezzi diverso ({} e {})", ma.len(), mb.len()));
        }
        let mut total = 0usize;
        for (&a, &b) in ma.iter().zip(&mb) {
            let (va, vb) = (&self.shapes_data[a as usize].world_vertices, &self.shapes_data[b as usize].world_vertices);
            if va.len() != vb.len() {
                return Err("i pezzi hanno un numero di vertici diverso".to_string());
            }
            total += va.len();
        }
        let step = total.div_ceil(MAX_PAIRS).max(1);
        let point = |v: &[f32; 3]| DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
        let (mut from, mut to) = (Vec::new(), Vec::new());
        let mut k = 0usize;
        for (&a, &b) in ma.iter().zip(&mb) {
            let (va, vb) = (&self.shapes_data[a as usize].world_vertices, &self.shapes_data[b as usize].world_vertices);
            for (pa, pb) in va.iter().zip(vb) {
                if k % step == 0 {
                    from.push(point(pa));
                    to.push(point(pb));
                }
                k += 1;
            }
        }
        let (rotation, translation) = rigid_fit(&from, &to).ok_or("troppi pochi vertici")?;
        // Controllo su tutti i vertici: devono coincidere, a meno della precisione dei float
        let mut max_error = 0.0f64;
        let mut size = 0.0f64;
        for (&a, &b) in ma.iter().zip(&mb) {
            let (va, vb) = (&self.shapes_data[a as usize].world_vertices, &self.shapes_data[b as usize].world_vertices);
            for (pa, pb) in va.iter().zip(vb) {
                let (pa, pb) = (point(pa), point(pb));
                max_error = max_error.max((rotation * pa + translation - pb).length());
                size = size.max(pa.abs().max_element()).max(pb.abs().max_element());
            }
        }
        let bbox = self.subtree_bounding_box(reference);
        let diagonal = if bbox.is_valid() { bbox.diagonal() as f64 } else { 0.0 };
        // f32 ha circa 7 cifre: tolleranza sulle coordinate più grandi e sull'ingombro
        let tolerance = (size * 2e-6).max(diagonal * 1e-5);
        if max_error > tolerance {
            return Err(format!("la geometria non è la stessa (scarto fino a {max_error:.3e})"));
        }
        let matrix = DMat4::from_rotation_translation(rotation, translation);
        Ok(RigidFit { matrix: matrix.as_mat4(), max_error, pairs: from.len() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> Vec<DVec3> {
        // Punti sparsi e non complanari, a coordinate grandi come quelle di una scocca in mm
        (0..500)
            .map(|i| {
                let f = i as f64;
                DVec3::new(4000.0 + (f * 1.7).sin() * 300.0, (f * 0.37).cos() * 800.0, -100.0 + (f * 2.3).sin() * 120.0)
            })
            .collect()
    }

    /// Stessa rotazione (q e -q sono la stessa); il prodotto scalare è più preciso dell'angolo.
    fn same(a: DQuat, b: DQuat) -> bool {
        a.dot(b).abs() > 1.0 - 1e-12
    }

    #[test]
    fn rotation_and_translation_are_recovered() {
        let rotation = DQuat::from_euler(glam::EulerRot::XYZ, 0.1, 0.209_439, -0.3);
        let translation = DVec3::new(215.0, -3.0, 1665.0);
        let from = cloud();
        let to: Vec<DVec3> = from.iter().map(|p| rotation * *p + translation).collect();
        let (r, t) = rigid_fit(&from, &to).unwrap();
        assert!(same(r, rotation), "{r:?} invece di {rotation:?}");
        assert!((t - translation).length() < 1e-6, "{t:?}");
        // Nessuno spostamento
        let (r, t) = rigid_fit(&from, &from).unwrap();
        assert!(same(r, DQuat::IDENTITY) && t.length() < 1e-9);
        // Mezzo giro (caso difficile per i metodi con gli angoli)
        let half = DQuat::from_rotation_z(std::f64::consts::PI);
        let to: Vec<DVec3> = from.iter().map(|p| half * *p).collect();
        let (r, _) = rigid_fit(&from, &to).unwrap();
        assert!(same(r, half), "{r:?}");
    }

    /// Stesso pezzo caricato due volte, il secondo spostato: lo scostamento viene dai vertici.
    #[test]
    fn offset_of_two_copies_comes_from_the_vertices() {
        use crate::scene_graph::node::multi_file_tests::{file, FILE_B};
        let mut scene = file(FILE_B, "a.wrl");
        scene.append(file(FILE_B, "b.wrl"));
        let (a, b) = (scene.sources[0].node, scene.sources[1].node);
        let moved = Mat4::from_translation(glam::Vec3::new(215.0, 0.0, 1665.0)) * Mat4::from_rotation_y(0.209_439);
        let meshes = scene.meshes_of(&[b]);
        scene.transform_meshes(&meshes, moved);
        let fit = scene.geometric_offset(a, b).unwrap();
        assert!(fit.matrix.abs_diff_eq(moved, 1e-3), "{:?}", fit.matrix);

        // Pezzi diversi: niente scostamento dalla geometria
        let telaio = file(crate::scene_graph::node::multi_file_tests::FILE_A, "c.wrl");
        let mut other = file(FILE_B, "a.wrl");
        other.append(telaio);
        let only_one = other.sources[1].node;
        let shapes = other.meshes_of(&[only_one]);
        let first = other.sources[0].node;
        assert_eq!(shapes.len(), 2);
        assert!(other.geometric_offset(first, only_one).is_err());
        assert!(scene.geometric_offset(a, a).is_err());
    }

    /// Scostamento tra due oggetti di un file reale, dai Transform e dai vertici:
    /// `GESTORE_PROVA_SCOSTAMENTO=file GESTORE_RIF=nome GESTORE_DEST=nome cargo test --release scostamento_file_reale -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scostamento_file_reale() {
        use crate::scene_graph::placement::Pose;
        let path = std::env::var("GESTORE_PROVA_SCOSTAMENTO").expect("file");
        let (rif, dest) = (std::env::var("GESTORE_RIF").unwrap(), std::env::var("GESTORE_DEST").unwrap());
        let mut scene = crate::parser::parse_wrl(&std::fs::read(&path).unwrap());
        scene.into_file("prova".into(), None);
        // Come all'apertura nell'applicazione: coordinate da metri → millimetri
        if !scene.units_mm && scene.file_in_meters(0) {
            scene.scale_file(0, 1000.0);
            println!("Convertito da metri a millimetri");
        }
        let find = |name: &str| (0..scene.nodes.len() as u32).find(|&n| scene.display_name(n) == name).unwrap_or_else(|| panic!("{name} non trovato"));
        let (a, b) = (find(&rif), find(&dest));
        let frame = Pose::from_matrix(scene.object_frame(a));
        let by_transform = frame.offset_to(&Pose::from_matrix(scene.object_frame(b)));
        println!("Dai Transform:  {}", by_transform.to_text());
        let t = std::time::Instant::now();
        match scene.geometric_offset(a, b) {
            Ok(fit) => {
                let m = frame.matrix();
                let pose = Pose::from_matrix(m.inverse() * fit.matrix * m);
                println!("Dai vertici:    {}  ({} coppie, scarto massimo {:.2e}, {:?})", pose.to_text(), fit.pairs, fit.max_error, t.elapsed());
            }
            Err(e) => println!("Dai vertici: {e}"),
        }
        // Export di Dima 3D e riapertura: i Transform degli oggetti restano
        for options in [
            crate::exporter::ExportOptions { meters: true, ..Default::default() },
            crate::exporter::ExportOptions { compact: false, normals: true, meters: true },
        ] {
            let out = std::env::temp_dir().join("dima3d_scostamento_export.wrl");
            crate::exporter::export_with(&scene, &crate::exporter::writer::Scope::Visible, &out, options).unwrap();
            let mut again = crate::parser::parse_wrl(&std::fs::read(&out).unwrap());
            let size = std::fs::metadata(&out).map_or(0, |m| m.len());
            // GESTORE_EXPORT_OUT: conserva l'export compatto (es. per provarlo con lo script)
            match std::env::var("GESTORE_EXPORT_OUT") {
                Ok(keep) if options.compact => std::fs::rename(&out, keep).unwrap(),
                _ => {
                    let _ = std::fs::remove_file(&out);
                }
            }
            again.into_file("export".into(), None);
            if again.file_in_meters(0) {
                again.scale_file(0, 1000.0);
            }
            let find = |name: &str| (0..again.nodes.len() as u32).find(|&n| again.display_name(n) == name).unwrap();
            let (a2, b2) = (find(&rif), find(&dest));
            let pose = Pose::from_matrix(again.object_frame(a2)).offset_to(&Pose::from_matrix(again.object_frame(b2)));
            println!("Dopo l'export (compatto {}, {size} byte), dai Transform: {}", options.compact, pose.to_text());
            let worst = scene.shapes_data.iter().zip(&again.shapes_data)
                .flat_map(|(p, q)| p.world_vertices.iter().zip(&q.world_vertices))
                .map(|(x, y)| glam::Vec3::from(*x).distance(glam::Vec3::from(*y)))
                .fold(0.0f32, f32::max);
            println!("  vertici: scarto massimo {worst:.2e}");
        }
    }
}
