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
//! gruppo = 812 Lato sinistro             (gruppo creato in Dima 3D: numero fisso e nome)
//! figli = 4 7 812 9                     (contenuto di un gruppo cambiato, in ordine)
//! nascosti = 3-10 15                    (nodi, con il loro numero fisso: la posizione
//!                                        all'apertura, contando dalla radice del file)
//! eliminati = 22
//! colore = 12 1 0.5 0 1
//! nome = 14 Nuovo nome
//! posizione = 7 m0 … m15                (mesh del file, matrice per colonne)
//!
//! [copia]                               (pezzi incollati: si rifà la copia, poi le modifiche)
//! di = 0                                (modello o copia da cui viene, in ordine nel progetto)
//! nodi = 3 8                            (nodi copiati, numerati dalla radice di quel modello)
//! saltati = 5                           (rami già eliminati al momento della copia)
//! titolo = Copia di Staffa
//! nascosti = …                          (come per i file, numerati dalla radice della copia)
//!
//! [cabina]                              (cabina generata: si ricostruisce, poi le modifiche)
//! misure = 20000 5700 …                 (vedi `CabinParams::to_text`)
//! ```

use std::path::{Component, Path, PathBuf};

use glam::{Mat4, Quat, Vec3};

use crate::clusterer::ClusterConfig;
use crate::scene_graph::copy::CopyOf;
use crate::scene_graph::SceneGraph;
use crate::settings::{StepLoad, StepMesh};

pub const EXTENSION: &str = "dima3d";
/// 2: copie incollate, gruppi creati e pezzi spostati nell'albero. 3: cabina generata.
const VERSION: u32 = 3;

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
    /// Copia incollata: `source` è l'indice nel progetto del modello (o della copia) da cui viene;
    /// `path` resta vuoto
    pub copy: Option<CopyOf>,
    /// Nome della radice della copia al momento della copia
    pub title: String,
    /// Gruppi creati in Dima 3D: numero fisso e nome
    pub groups: Vec<(u32, String)>,
    /// Contenitori cambiati: numero fisso → numeri fissi dei figli, in ordine
    pub children: Vec<(u32, Vec<u32>)>,
    /// Cabina generata: le sue misure; `path` resta vuoto
    pub cabin: Option<crate::cabin::CabinParams>,
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
        // Modelli da salvare: quelli aperti e quelli da cui vengono le copie aperte (anche se
        // chiusi dopo la copia: servono per rifarla). Una copia si salva solo se si salva il suo.
        let alive = |i: usize| scene.info.get(scene.sources[i].node as usize).is_some_and(|n| !n.deleted);
        let mut needed: Vec<bool> = (0..scene.sources.len()).map(alive).collect();
        for i in (0..scene.sources.len()).rev() {
            if let (true, Some(c)) = (needed[i], &scene.sources[i].copy_of) {
                needed[c.source] = true;
            }
        }
        let mut entry_of: Vec<Option<usize>> = vec![None; scene.sources.len()];
        let mut files = Vec::new();
        for (i, source) in scene.sources.iter().enumerate() {
            if !needed[i] {
                continue;
            }
            let copy = match &source.copy_of {
                Some(c) => match entry_of.get(c.source).copied().flatten() {
                    Some(of) => Some(CopyOf { source: of, ..c.clone() }),
                    None => continue,
                },
                None => None,
            };
            let (path, size, modified) = match (&source.path, &copy, &source.cabin) {
                (Some(path), None, _) => {
                    let (size, modified) = file_stamp(path);
                    (path.clone(), size, modified)
                }
                (None, Some(_), _) | (None, None, Some(_)) => (PathBuf::new(), 0, 0),
                _ => continue,
            };
            entry_of[i] = Some(files.len());
            let mut entry = FileEntry {
                path,
                relative: None,
                size,
                modified,
                step: source.step,
                scale: source.scale,
                title: if copy.is_some() { source.name.clone() } else { String::new() },
                cabin: if copy.is_none() { source.cabin.clone() } else { None },
                copy,
                ..Default::default()
            };
            let mut nodes = scene.file_nodes(i);
            nodes.sort_by_key(|&n| scene.info[n as usize].origin);
            let origin = |n: u32| scene.info[n as usize].origin;
            for &n in &nodes {
                let (node, info) = (&scene.nodes[n as usize], &scene.info[n as usize]);
                let rel = info.origin;
                if info.deleted {
                    // Un gruppo creato e poi eliminato (o annullato) non serve a niente
                    if info.created {
                        continue;
                    }
                    // Solo la cima di ogni ramo eliminato: il resto si elimina con lei
                    if info.parent == crate::scene_graph::NO_PARENT || !scene.info[info.parent as usize].deleted {
                        entry.deleted.push(rel);
                    }
                    continue;
                }
                if info.created {
                    entry.groups.push((rel, node.name().unwrap_or("Gruppo").to_string()));
                }
                if info.restructured {
                    entry.children.push((rel, node.children().iter().map(|&c| origin(c)).collect()));
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
            if let Some(c) = &f.copy {
                out.push_str("\n[copia]\n");
                out.push_str(&format!("di = {}\nnodi = {}\n", c.source, ranges(&c.nodes)));
                if !c.skip.is_empty() {
                    out.push_str(&format!("saltati = {}\n", ranges(&c.skip)));
                }
                out.push_str(&format!("titolo = {}\n", f.title.replace(['\r', '\n'], " ")));
            } else if let Some(c) = &f.cabin {
                out.push_str("\n[cabina]\n");
                out.push_str(&format!("misure = {}\n", c.to_text()));
                if !c.label.text.trim().is_empty() {
                    out.push_str(&format!("scritta = {}\n", c.label.to_text()));
                }
            } else {
                out.push_str("\n[file]\n");
                if let Some(rel) = relative_to(&f.path, base) {
                    out.push_str(&format!("percorso = {}\n", rel.display()));
                }
                out.push_str(&format!("assoluto = {}\n", f.path.display()));
                out.push_str(&format!("dimensione = {}\nmodificato = {}\n", f.size, f.modified));
            }
            match f.step {
                Some(StepLoad::View) => out.push_str("step = vista\n"),
                Some(StepLoad::Mesh(m)) => out.push_str(&format!("step = mesh {} {} {}\n", m.deviation, m.angle, u8::from(m.relative))),
                None => {}
            }
            if f.scale != 1.0 {
                out.push_str(&format!("scala = {}\n", f.scale));
            }
            for (n, name) in &f.groups {
                out.push_str(&format!("gruppo = {n} {}\n", name.replace(['\r', '\n'], " ")));
            }
            for (n, list) in &f.children {
                let list: Vec<String> = list.iter().map(u32::to_string).collect();
                out.push_str(&format!("figli = {n} {}\n", list.join(" ")).replace("  \n", "\n").replace(" \n", "\n"));
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
            if line == "[copia]" {
                out.files.push(FileEntry { scale: 1.0, copy: Some(CopyOf::default()), ..Default::default() });
                continue;
            }
            if line == "[cabina]" {
                out.files.push(FileEntry { scale: 1.0, cabin: Some(Default::default()), ..Default::default() });
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
                (Some(f), "di") => {
                    let of: usize = value.parse().map_err(|_| bad(i))?;
                    f.copy.as_mut().ok_or_else(|| bad(i))?.source = of;
                }
                (Some(f), "nodi") => f.copy.as_mut().ok_or_else(|| bad(i))?.nodes = parse_ranges(value).ok_or_else(|| bad(i))?,
                (Some(f), "saltati") => f.copy.as_mut().ok_or_else(|| bad(i))?.skip = parse_ranges(value).ok_or_else(|| bad(i))?,
                (Some(f), "titolo") => f.title = value.to_string(),
                (Some(f), "misure") if f.cabin.is_some() => {
                    f.cabin = Some(crate::cabin::CabinParams::from_text(value).ok_or_else(|| bad(i))?);
                }
                (Some(f), "scritta") if f.cabin.is_some() => {
                    let label = crate::cabin::CabinLabel::from_text(value).ok_or_else(|| bad(i))?;
                    if let Some(c) = &mut f.cabin {
                        c.label = label;
                    }
                }
                (Some(f), "gruppo") => {
                    let (n, name) = value.split_once(' ').unwrap_or((value, ""));
                    f.groups.push((n.parse().map_err(|_| bad(i))?, name.trim().to_string()));
                }
                (Some(f), "figli") => {
                    let numbers: Vec<u32> = value.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|_| bad(i))?;
                    let (&parent, list) = numbers.split_first().ok_or_else(|| bad(i))?;
                    f.children.push((parent, list.to_vec()));
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
        for (index, f) in out.files.iter_mut().enumerate() {
            if let Some(c) = &f.copy {
                // Una copia viene da un elemento scritto prima nel progetto
                if c.source >= index || c.nodes.is_empty() {
                    return Err("Nel progetto c'è una copia senza provenienza valida".to_string());
                }
                continue;
            }
            if f.cabin.is_some() {
                continue;
            }
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
        let meshes = file.meshes.clone();
        // Numero fisso → nodo (la struttura salvata è già stata ricostruita)
        let file_nodes = scene.file_nodes(source);
        let map = scene.origin_map(source);
        let node = |rel: u32| map.get(&rel).copied();
        let mut skipped = 0;
        if entry.scale != 1.0 && file.scale > 0.0 {
            let factor = entry.scale / file.scale;
            if (factor - 1.0).abs() > 1e-6 {
                scene.scale_file(source, factor);
            }
        }
        let hidden: std::collections::HashSet<u32> = entry.hidden.iter().copied().collect();
        for &n in &file_nodes {
            let origin = scene.info[n as usize].origin;
            if let Some(x) = scene.nodes.get_mut(n as usize) {
                x.set_visible(!hidden.contains(&origin));
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
            copy: None,
            title: String::new(),
            groups: vec![(812, "Lato sinistro".into())],
            children: vec![(4, vec![7, 812, 9]), (812, vec![])],
            cabin: None,
        };
        let copy = FileEntry {
            scale: 1.0,
            hidden: vec![2],
            copy: Some(CopyOf { source: 1, nodes: vec![3, 4, 5, 9], skip: vec![6] }),
            title: "Copia di Staffa".into(),
            ..Default::default()
        };
        let cabin = FileEntry {
            scale: 1.0,
            hidden: vec![3],
            cabin: Some(crate::cabin::CabinParams { length: 9000.0, glass: false, ..Default::default() }),
            ..Default::default()
        };
        let p = ProjectFile {
            cluster: ClusterConfig { tolerance: 0.75, min_matching_vertices: 20 },
            camera: Some(CameraView { target: Vec3::new(1.0, 2.0, 3.0), distance: 4500.0, orientation: Quat::from_rotation_y(0.4) }),
            files: vec![file.clone(), FileEntry { path: PathBuf::from(r"E:\b.stl"), scale: 1.0, ..Default::default() }, copy.clone(), cabin.clone()],
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
        assert_eq!((&f.groups, &f.children), (&file.groups, &file.children));
        assert_eq!(back.files[1].path, PathBuf::from(r"E:\b.stl"));
        assert_eq!(back.files[3], cabin, "cabina: misure e modifiche");
        assert!(text.contains("[copia]\ndi = 1\nnodi = 3-5 9\nsaltati = 6\ntitolo = Copia di Staffa"), "{text}");
        assert_eq!(back.files[2], copy);
        // Una copia può venire solo da un elemento scritto prima
        let bad = text.replace("di = 1", "di = 2");
        assert!(ProjectFile::from_text(&bad, project).is_err());
        assert!(ProjectFile::from_text("ciao", project).is_err());
        assert!(ProjectFile::from_text("versione = 99", project).is_err());
    }
}
