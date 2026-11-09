//! Progetto Dima 3D (`.dima3d`): un file di testo con l'elenco dei modelli aperti e le modifiche
//! fatte nella scena. La geometria non c'è: all'apertura si rileggono i modelli originali, con le
//! stesse impostazioni di raggruppamento, e si riapplicano le modifiche.
//!
//! ```text
//! # Progetto Dima 3D
//! versione = 1
//! raggruppamento = 0.5 12
//! camera = tx ty tz distanza qx qy qz qw
//!
//! [file]
//! percorso = modelli\scocca.wrl        (relativo alla cartella del progetto, se si può)
//! assoluto = D:\lavoro\modelli\scocca.wrl
//! dimensione = 123456
//! modificato = 1731150000               (secondi Unix)
//! step = mesh 0.1 30 0                  (solo per gli STEP: vista | mesh deviazione angolo relativa)
//! scala = 1000
//! nascosti = 3-10 15                    (nodi, numerati dalla radice del file)
//! eliminati = 22
//! colore = 12 1 0.5 0 1
//! nome = 14 Nuovo nome
//! posizione = 7 m0 … m15                (mesh del file, matrice per colonne)
//! ```

use std::path::{Component, Path, PathBuf};

use glam::{Mat4, Quat, Vec3};

use crate::clusterer::ClusterConfig;
use crate::scene_graph::SceneGraph;
use crate::settings::{StepLoad, StepMesh};

pub const EXTENSION: &str = "dima3d";
const VERSION: u32 = 1;

/// Vista salvata: centro, distanza e orientamento della camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    pub target: Vec3,
    pub distance: f32,
    pub orientation: Quat,
}

/// Un modello del progetto e le sue modifiche (nodi e mesh numerati dall'inizio del file).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileEntry {
    /// Percorso usato per aprirlo (assoluto)
    pub path: PathBuf,
    /// Percorso relativo alla cartella del progetto, se c'era
    pub relative: Option<PathBuf>,
    pub size: u64,
    pub modified: u64,
    pub step: Option<StepLoad>,
    pub scale: f32,
    pub hidden: Vec<u32>,
    pub deleted: Vec<u32>,
    pub colors: Vec<(u32, [f32; 4])>,
    pub names: Vec<(u32, Option<String>)>,
    pub placements: Vec<(u32, Mat4)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFile {
    pub cluster: ClusterConfig,
    pub camera: Option<CameraView>,
    pub files: Vec<FileEntry>,
}

/// Dimensione e data di modifica di un file (0 se non si leggono).
pub fn file_stamp(path: &Path) -> (u64, u64) {
    let Ok(meta) = std::fs::metadata(path) else { return (0, 0) };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    (meta.len(), modified)
}

/// `path` relativo alla cartella `base` (anche con `..`); `None` su dischi diversi.
pub fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    let norm = |p: &Path| -> Vec<String> {
        p.components()
            .filter(|c| !matches!(c, Component::CurDir))
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let (p, b) = (norm(path), norm(base));
    // Stesso disco (o stessa radice)
    if p.first() != b.first() || p.is_empty() {
        return None;
    }
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let originals: Vec<_> = path.components().filter(|c| !matches!(c, Component::CurDir)).collect();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in &originals[common..] {
        out.push(c.as_os_str());
    }
    Some(out)
}

/// Elenco di numeri come intervalli: `3-10 15 20-22`.
fn ranges(list: &[u32]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        let start = list[i];
        let mut end = start;
        while i + 1 < list.len() && list[i + 1] == end + 1 {
            i += 1;
            end = list[i];
        }
        out.push(if start == end { start.to_string() } else { format!("{start}-{end}") });
        i += 1;
    }
    out.join(" ")
}

fn parse_ranges(text: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for part in text.split_whitespace() {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
                if b < a || b - a > 50_000_000 {
                    return None;
                }
                out.extend(a..=b);
            }
            None => out.push(part.parse().ok()?),
        }
    }
    Some(out)
}

fn floats<const N: usize>(text: &str) -> Option<[f32; N]> {
    let values: Vec<f32> = text.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
    values.try_into().ok()
}

impl ProjectFile {
    /// Stato della scena da salvare.
    pub fn capture(scene: &SceneGraph, cluster: ClusterConfig, camera: Option<CameraView>) -> ProjectFile {
        let starts: Vec<u32> = scene.sources.iter().map(|s| s.node).collect();
        let mut files = Vec::new();
        for (i, source) in scene.sources.iter().enumerate() {
            let Some(path) = &source.path else { continue };
            if scene.info.get(source.node as usize).is_none_or(|n| n.deleted) {
                continue;
            }
            let first = source.node;
            let end = starts.get(i + 1).copied().unwrap_or(scene.nodes.len() as u32);
            let (size, modified) = file_stamp(path);
            let mut entry = FileEntry {
                path: path.clone(),
                relative: None,
                size,
                modified,
                step: source.step,
                scale: source.scale,
                ..Default::default()
            };
            for n in first..end {
                let (node, info) = (&scene.nodes[n as usize], &scene.info[n as usize]);
                let rel = n - first;
                if info.deleted {
                    // Solo la cima di ogni ramo eliminato: il resto si elimina con lei
                    if info.parent == crate::scene_graph::NO_PARENT || !scene.info[info.parent as usize].deleted {
                        entry.deleted.push(rel);
                    }
                    continue;
                }
                if !node.is_visible() {
                    entry.hidden.push(rel);
                }
                if let Some(c) = info.color {
                    entry.colors.push((rel, c));
                }
                if info.renamed {
                    entry.names.push((rel, node.name().map(str::to_string)));
                }
            }
            for (mesh, m) in scene.moved_placements() {
                if source.meshes.contains(&mesh) {
                    entry.placements.push((mesh - source.meshes.start, m));
                }
            }
            files.push(entry);
        }
        ProjectFile { cluster, camera, files }
    }

    /// Testo del progetto salvato in `project` (per i percorsi relativi).
    pub fn to_text(&self, project: &Path) -> String {
        let base = project.parent().unwrap_or(Path::new(""));
        let mut out = format!(
            "# Progetto Dima 3D\nversione = {VERSION}\nraggruppamento = {} {}\n",
            self.cluster.tolerance, self.cluster.min_matching_vertices
        );
        if let Some(c) = self.camera {
            let (t, q) = (c.target, c.orientation);
            out.push_str(&format!("camera = {} {} {} {} {} {} {} {}\n", t.x, t.y, t.z, c.distance, q.x, q.y, q.z, q.w));
        }
        for f in &self.files {
            out.push_str("\n[file]\n");
            if let Some(rel) = relative_to(&f.path, base) {
                out.push_str(&format!("percorso = {}\n", rel.display()));
            }
            out.push_str(&format!("assoluto = {}\n", f.path.display()));
            out.push_str(&format!("dimensione = {}\nmodificato = {}\n", f.size, f.modified));
            match f.step {
                Some(StepLoad::View) => out.push_str("step = vista\n"),
                Some(StepLoad::Mesh(m)) => out.push_str(&format!("step = mesh {} {} {}\n", m.deviation, m.angle, u8::from(m.relative))),
                None => {}
            }
            if f.scale != 1.0 {
                out.push_str(&format!("scala = {}\n", f.scale));
            }
            if !f.hidden.is_empty() {
                out.push_str(&format!("nascosti = {}\n", ranges(&f.hidden)));
            }
            if !f.deleted.is_empty() {
                out.push_str(&format!("eliminati = {}\n", ranges(&f.deleted)));
            }
            for (n, [r, g, b, a]) in &f.colors {
                out.push_str(&format!("colore = {n} {r} {g} {b} {a}\n"));
            }
            for (n, name) in &f.names {
                out.push_str(&format!("nome = {n} {}\n", name.as_deref().unwrap_or("").replace(['\r', '\n'], " ")));
            }
            for (mesh, m) in &f.placements {
                let values: Vec<String> = m.to_cols_array().iter().map(f32::to_string).collect();
                out.push_str(&format!("posizione = {mesh} {}\n", values.join(" ")));
            }
        }
        out
    }

    /// Legge un progetto salvato in `project`. I percorsi relativi si risolvono dalla sua cartella
    /// se il file esiste lì, altrimenti resta l'assoluto.
    pub fn from_text(text: &str, project: &Path) -> Result<ProjectFile, String> {
        let base = project.parent().unwrap_or(Path::new(""));
        let mut out = ProjectFile { cluster: ClusterConfig::default(), camera: None, files: Vec::new() };
        let mut seen_header = false;
        let bad = |line: usize| format!("Riga {} del progetto non valida", line + 1);
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "[file]" {
                out.files.push(FileEntry { scale: 1.0, ..Default::default() });
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { return Err(bad(i)) };
            let (key, value) = (key.trim(), value.trim());
            if key == "versione" {
                let v: u32 = value.parse().map_err(|_| bad(i))?;
                if v > VERSION {
                    return Err(format!("Progetto salvato da una versione più nuova di Dima 3D (formato {v})"));
                }
                seen_header = true;
                continue;
            }
            match (out.files.last_mut(), key) {
                (None, "raggruppamento") => {
                    let mut it = value.split_whitespace();
                    let tolerance = it.next().and_then(|v| v.parse().ok()).ok_or_else(|| bad(i))?;
                    let min_matching_vertices = it.next().and_then(|v| v.parse().ok()).ok_or_else(|| bad(i))?;
                    out.cluster = ClusterConfig { tolerance, min_matching_vertices };
                }
                (None, "camera") => {
                    let [x, y, z, d, qx, qy, qz, qw] = floats::<8>(value).ok_or_else(|| bad(i))?;
                    out.camera = Some(CameraView {
                        target: Vec3::new(x, y, z),
                        distance: d,
                        orientation: Quat::from_xyzw(qx, qy, qz, qw).normalize(),
                    });
                }
                (Some(f), "percorso") => f.relative = Some(PathBuf::from(value)),
                (Some(f), "assoluto") => f.path = PathBuf::from(value),
                (Some(f), "dimensione") => f.size = value.parse().map_err(|_| bad(i))?,
                (Some(f), "modificato") => f.modified = value.parse().map_err(|_| bad(i))?,
                (Some(f), "step") => {
                    f.step = Some(if value == "vista" {
                        StepLoad::View
                    } else {
                        let rest = value.strip_prefix("mesh").ok_or_else(|| bad(i))?;
                        let [deviation, angle, relative] = floats::<3>(rest).ok_or_else(|| bad(i))?;
                        StepLoad::Mesh(StepMesh { deviation, angle, relative: relative != 0.0 })
                    })
                }
                (Some(f), "scala") => f.scale = value.parse().map_err(|_| bad(i))?,
                (Some(f), "nascosti") => f.hidden = parse_ranges(value).ok_or_else(|| bad(i))?,
                (Some(f), "eliminati") => f.deleted = parse_ranges(value).ok_or_else(|| bad(i))?,
                (Some(f), "colore") => {
                    let [n, r, g, b, a] = floats::<5>(value).ok_or_else(|| bad(i))?;
                    f.colors.push((n as u32, [r, g, b, a]));
                }
                (Some(f), "nome") => {
                    let (n, name) = value.split_once(' ').unwrap_or((value, ""));
                    let name = name.trim();
                    f.names.push((n.parse().map_err(|_| bad(i))?, (!name.is_empty()).then(|| name.to_string())));
                }
                (Some(f), "posizione") => {
                    let (n, rest) = value.split_once(' ').ok_or_else(|| bad(i))?;
                    let m = floats::<16>(rest).ok_or_else(|| bad(i))?;
                    f.placements.push((n.parse().map_err(|_| bad(i))?, Mat4::from_cols_array(&m)));
                }
                // Chiavi sconosciute (versioni future): si ignorano
                _ => {}
            }
        }
        if !seen_header {
            return Err("Non è un progetto di Dima 3D".to_string());
        }
        for f in &mut out.files {
            if f.path.as_os_str().is_empty() && f.relative.is_none() {
                return Err("Nel progetto c'è un file senza percorso".to_string());
            }
            if let Some(rel) = &f.relative {
                let candidate = base.join(rel);
                if candidate.exists() || f.path.as_os_str().is_empty() {
                    f.path = candidate;
                }
            }
        }
        Ok(out)
    }

    /// Riapplica a `scene` le modifiche di `entry`, il modello `source` della scena appena
    /// riaperto. Ritorna quante modifiche non si sono potute applicare (nodi o mesh inesistenti,
    /// es. file cambiato).
    pub fn apply_entry(entry: &FileEntry, scene: &mut SceneGraph, source: usize) -> usize {
        let Some(file) = scene.sources.get(source) else { return 0 };
        let first = file.node;
        let end = scene.sources.get(source + 1).map_or(scene.nodes.len() as u32, |s| s.node);
        let meshes = file.meshes.clone();
        let node = |rel: u32| first.checked_add(rel).filter(|&n| n < end);
        let mut skipped = 0;
        if entry.scale != 1.0 && file.scale > 0.0 {
            let factor = entry.scale / file.scale;
            if (factor - 1.0).abs() > 1e-6 {
                scene.scale_file(source, factor);
            }
        }
        let hidden: std::collections::HashSet<u32> = entry.hidden.iter().copied().collect();
        for n in first..end {
            if let Some(x) = scene.nodes.get_mut(n as usize) {
                x.set_visible(!hidden.contains(&(n - first)));
            }
        }
        skipped += entry.hidden.iter().filter(|&&n| node(n).is_none()).count();
        for &(rel, c) in &entry.colors {
            match node(rel) {
                Some(n) => scene.info[n as usize].color = Some(c),
                None => skipped += 1,
            }
        }
        for (rel, name) in &entry.names {
            match node(*rel) {
                Some(n) => scene.rename(n, name.as_deref().unwrap_or("")),
                None => skipped += 1,
            }
        }
        let mut placements = Vec::new();
        for &(rel, m) in &entry.placements {
            match meshes.start.checked_add(rel).filter(|x| meshes.contains(x)) {
                Some(mesh) => placements.push((mesh, m)),
                None => skipped += 1,
            }
        }
        let list: Vec<u32> = placements.iter().map(|(m, _)| *m).collect();
        scene.set_placements(&list, |mesh| placements.iter().find(|(m, _)| *m == mesh).map_or(Mat4::IDENTITY, |(_, p)| *p));
        // Le eliminazioni per ultime: tolgono i nodi dall'albero
        for &rel in &entry.deleted {
            match node(rel) {
                Some(n) => scene.delete_node(n),
                None => skipped += 1,
            }
        }
        scene.revision += 1;
        skipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_round_trip() {
        let list = vec![3, 4, 5, 6, 10, 12, 13];
        assert_eq!(ranges(&list), "3-6 10 12-13");
        assert_eq!(parse_ranges("3-6 10 12-13"), Some(list));
        assert_eq!(parse_ranges("5-2"), None);
        assert_eq!(parse_ranges("x"), None);
    }

    #[test]
    fn relative_paths() {
        let base = Path::new(r"D:\lavoro\progetti");
        assert_eq!(relative_to(Path::new(r"D:\lavoro\progetti\a.wrl"), base), Some(PathBuf::from("a.wrl")));
        assert_eq!(relative_to(Path::new(r"D:\lavoro\modelli\b.wrl"), base), Some(PathBuf::from(r"..\modelli\b.wrl")));
        if cfg!(windows) {
            assert_eq!(relative_to(Path::new(r"E:\b.wrl"), base), None);
        }
    }

    #[test]
    fn text_round_trip() {
        let project = Path::new(r"D:\lavoro\progetto.dima3d");
        let file = FileEntry {
            path: PathBuf::from(r"D:\lavoro\modelli\pezzo uno.wrl"),
            relative: None,
            size: 1234,
            modified: 1_700_000_000,
            step: Some(StepLoad::Mesh(StepMesh { deviation: 0.05, angle: 20.0, relative: true })),
            scale: 1000.0,
            hidden: vec![1, 2, 3, 9],
            deleted: vec![4],
            colors: vec![(2, [1.0, 0.5, 0.25, 1.0])],
            names: vec![(0, Some("Nome nuovo con spazi".into())), (5, None)],
            placements: vec![(3, Mat4::from_rotation_z(0.3) * Mat4::from_translation(Vec3::new(1.5, -2.0, 1e-7)))],
        };
        let p = ProjectFile {
            cluster: ClusterConfig { tolerance: 0.75, min_matching_vertices: 20 },
            camera: Some(CameraView { target: Vec3::new(1.0, 2.0, 3.0), distance: 4500.0, orientation: Quat::from_rotation_y(0.4) }),
            files: vec![file.clone(), FileEntry { path: PathBuf::from(r"E:\b.stl"), scale: 1.0, ..Default::default() }],
        };
        let text = p.to_text(project);
        assert!(text.contains(r"percorso = modelli\pezzo uno.wrl"), "{text}");
        let back = ProjectFile::from_text(&text, project).unwrap();
        assert_eq!(back.cluster, p.cluster);
        let c = back.camera.unwrap();
        assert!(c.orientation.abs_diff_eq(p.camera.unwrap().orientation, 1e-6) && c.distance == 4500.0);
        let f = &back.files[0];
        // Il file non esiste davvero: resta il percorso assoluto
        assert_eq!(f.path, file.path);
        assert_eq!(f.relative, Some(PathBuf::from(r"modelli\pezzo uno.wrl")));
        assert_eq!((f.size, f.modified, f.step, f.scale), (file.size, file.modified, file.step, file.scale));
        assert_eq!((&f.hidden, &f.deleted, &f.colors, &f.names), (&file.hidden, &file.deleted, &file.colors, &file.names));
        assert_eq!(f.placements, file.placements, "le matrici tornano identiche");
        assert_eq!(back.files[1].path, PathBuf::from(r"E:\b.stl"));
        assert!(ProjectFile::from_text("ciao", project).is_err());
        assert!(ProjectFile::from_text("versione = 99", project).is_err());
    }
}
