use std::collections::HashMap;
use rayon::prelude::*;
use crate::parser::{ContainerKind, PathSeg, ShapeInfo, lexer, delimiter_map, transform};
use crate::scene_graph::SceneGraph;

/// Forma memorizzata a livello locale (senza parent_matrix applicata) per supporto a DEF/USE.
#[derive(Clone, Debug)]
pub struct LocalShape {
    pub source_start: usize,
    pub source_end: usize,
    pub point_range_start: usize,
    pub point_range_end: usize,
    pub local_matrix: glam::Mat4,
    pub local_vertices: Vec<[f32; 3]>,
    pub local_normals: Vec<[f32; 3]>,
    pub split: Option<Box<crate::parser::SplitNormals>>,
    pub indices: Vec<u32>,
    pub face_count: u32,
    /// Nome DEF della Shape (se presente)
    pub name: Option<String>,
    /// Colore originale del Material
    pub color: Option<[f32; 4]>,
    /// Percorso dei contenitori relativo al nodo DEF che contiene il template
    pub path: Vec<PathSeg>,
}

/// Converte una ShapeInfo (calcolata con matrice relativa al DEF) in template locale:
/// `local_vertices`/`local_normals` sono già nello spazio del DEF, `local_matrix` è la
/// matrice che ce li ha portati.
fn to_local(s: ShapeInfo) -> LocalShape {
    LocalShape {
        source_start: s.source_start,
        source_end: s.source_end,
        point_range_start: s.point_range_start,
        point_range_end: s.point_range_end,
        local_matrix: s.matrix,
        local_vertices: s.world_vertices, // già trasformati dalla matrice locale del DEF
        local_normals: s.normals,
        split: s.split,
        indices: s.indices,
        face_count: s.face_count,
        name: s.name,
        color: s.color,
        path: s.path,
    }
}

/// Antepone un livello di gerarchia a tutte le Shape prodotte dentro un contenitore.
/// Il percorso viene quindi costruito bottom-up senza dover propagare uno stack
/// attraverso tutte le funzioni ricorsive.
fn prefix_path(results: &mut [ShapeInfo], seg: &PathSeg) {
    for s in results {
        s.path.insert(0, seg.clone());
    }
}

/// Definizione di un nodo Coordinate per supporto a `coord DEF Nome Coordinate` / `coord USE Nome`.
#[derive(Clone, Debug)]
pub struct CoordDef {
    pub point_range_start: usize,
    pub point_range_end: usize,
    pub points: Vec<[f32; 3]>,
}

/// Una definizione `DEF` di un nodo Shape o contenitore.
#[derive(Clone, Debug)]
pub struct ShapeDef {
    /// Posizione della `{` del nodo: distingue le definizioni con lo stesso nome
    pub pos: usize,
    pub shapes: Vec<LocalShape>,
    /// Transform/Group/...: un `USE` diventa un nodo "istanza" nell'albero
    pub container: bool,
    /// Matrice locale del contenitore (identità per Group e Shape): sistema dei figli di un `USE`
    pub matrix: glam::Mat4,
}

/// Registro delle definizioni DEF/USE all'interno di un file WRL. Lo stesso nome può essere
/// ridefinito: ogni voce elenca le definizioni in ordine di file.
#[derive(Default, Clone, Debug)]
pub struct DefRegistry {
    pub shapes: HashMap<String, Vec<ShapeDef>>,
    pub coords: HashMap<String, Vec<(usize, CoordDef)>>,
    /// `DEF Nome IndexedFaceSet`: posizioni delle graffe, la geometria si legge a ogni `USE`
    pub geometries: HashMap<String, Vec<(usize, usize)>>,
    /// Colori dei `DEF` Material / Appearance
    pub materials: crate::parser::material::MaterialRegistry,
    /// Nomi citati da almeno un `USE` nel file. Solo per questi serve un template:
    /// analizzare due volte ogni `DEF` raddoppierebbe il costo a ogni livello di annidamento
    /// (i CAD annidano decine di `DEF Transform`).
    pub used: std::collections::HashSet<String>,
    /// `Switch` con contenuto ma nessuna scelta attiva: condiviso tra le copie dei job paralleli,
    /// le posizioni evitano di contare due volte uno `Switch` analizzato anche come template
    pub hidden_switches: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<usize>>>,
}

impl DefRegistry {
    fn has_shape_def(&self, name: &str, pos: usize) -> bool {
        self.shapes.get(name).is_some_and(|d| d.binary_search_by_key(&pos, |d| d.pos).is_ok())
    }

    fn add_shape_def(&mut self, name: &str, def: ShapeDef) {
        let defs = self.shapes.entry(name.to_string()).or_default();
        if let Err(at) = defs.binary_search_by_key(&def.pos, |d| d.pos) {
            defs.insert(at, def);
        }
    }

    fn has_coord_def(&self, name: &str, pos: usize) -> bool {
        self.coords.get(name).is_some_and(|d| d.binary_search_by_key(&pos, |d| d.0).is_ok())
    }

    fn add_coord_def(&mut self, name: &str, pos: usize, def: CoordDef) {
        let defs = self.coords.entry(name.to_string()).or_default();
        if let Err(at) = defs.binary_search_by_key(&pos, |d| d.0) {
            defs.insert(at, (pos, def));
        }
    }
}

/// Nomi citati da un `USE` e geometrie con `DEF` in tutto il file (commenti e stringhe esclusi).
fn scan_use_and_geometry_defs(
    data: &[u8],
    brace_map: &HashMap<usize, usize>,
) -> (std::collections::HashSet<String>, HashMap<String, Vec<(usize, usize)>>) {
    let mut used = std::collections::HashSet::new();
    let mut geometries: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'#' => {
                while i < data.len() && data[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < data.len() && data[i] != b'"' {
                    i += if data[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'U' if data[i..].starts_with(b"USE")
                && (i == 0 || data[i - 1].is_ascii_whitespace() || b"{}[],".contains(&data[i - 1]))
                && data.get(i + 3).is_some_and(|c| c.is_ascii_whitespace()) =>
            {
                match lexer::scan_word(data, i + 3) {
                    Some((name, after)) => {
                        used.insert(name.to_string());
                        i = after;
                    }
                    None => i += 3,
                }
            }
            b'D' if data[i..].starts_with(b"DEF")
                && (i == 0 || data[i - 1].is_ascii_whitespace() || b"{}[],".contains(&data[i - 1]))
                && data.get(i + 3).is_some_and(|c| c.is_ascii_whitespace()) =>
            {
                let Some((name, after_name)) = lexer::scan_word(data, i + 3) else {
                    i += 3;
                    continue;
                };
                if let Some(("IndexedFaceSet", after_type)) = lexer::scan_word(data, after_name) {
                    let open = lexer::skip_whitespace_comments(data, after_type);
                    if let Some(&close) = brace_map.get(&open) {
                        geometries.entry(name.to_string()).or_default().push((open, close));
                    }
                }
                i = after_name;
            }
            _ => i += 1,
        }
    }
    (used, geometries)
}

/// Blocco top-level identificato nel file WRL.
#[derive(Clone, Debug)]
struct TopLevelBlock {
    start: usize,
    end: usize,
    is_def: bool,
}

/// Entry point sincrono standard del parser WRL.
#[allow(dead_code)]
pub fn parse_wrl(data: &[u8]) -> SceneGraph {
    let mut scene = parse_wrl_with_progress(data, |_, _| {});
    if scene.source_data.is_empty() {
        scene.source_data = data.into();
    }
    scene
}

/// Entry point con callback di progresso per reporting alla UI.
///
/// `source_data` resta vuoto se il testo da conservare per l'export è `data`, così il chiamante
/// può tenerlo mappato in memoria. Un file VRML 1.0 è invece convertito: `source_data` contiene
/// il testo VRML 2.0, a cui si riferiscono le posizioni della scena.
pub fn parse_wrl_with_progress<F>(data: &[u8], progress_fn: F) -> SceneGraph
where
    F: FnMut(f32, String),
{
    parse_wrl_with_config(data, crate::clusterer::ClusterConfig::default(), progress_fn)
}

/// Come `parse_wrl_with_progress`, con i parametri del raggruppamento automatico scelti
/// dall'utente.
pub fn parse_wrl_with_config<F>(data: &[u8], cluster_cfg: crate::clusterer::ClusterConfig, mut progress_fn: F) -> SceneGraph
where
    F: FnMut(f32, String),
{
    let converted = if crate::parser::vrml1::is_vrml1(data) {
        progress_fn(0.0, "Conversione da VRML 1.0...".to_string());
        crate::parser::vrml1::convert(data)
    } else if crate::parser::iv::is_inventor(data) {
        // Il testo Inventor è quasi VRML 1.0; il binario viene prima tradotto in testo
        progress_fn(0.0, "Lettura Open Inventor...".to_string());
        let (text, mut warnings) = crate::parser::iv::ascii(data);
        let mut converted = crate::parser::vrml1::convert(&text);
        warnings.append(&mut converted.warnings);
        converted.warnings = warnings;
        converted
    } else if crate::parser::hsf::is_hsf(data) {
        progress_fn(0.0, "Lettura HSF...".to_string());
        let (shapes, warnings) = crate::parser::hsf::shapes(data);
        let mut scene = scene_from_shapes(shapes, cluster_cfg, &mut progress_fn);
        scene.load_warnings.extend(warnings);
        // L'HSF dichiara le unità e il lettore le porta in millimetri
        scene.units_mm = true;
        // Come per lo STL: i pezzi sono `rewritten`, l'export non legge il file
        scene.source_data = b"#VRML V2.0 utf8
".to_vec().into();
        return scene;
    } else if crate::parser::stl::is_stl(data) {
        // Nessun testo intermedio: i pezzi si costruiscono direttamente dai triangoli
        progress_fn(0.0, "Lettura STL...".to_string());
        let (shapes, warnings) = crate::parser::stl::shapes(data);
        let mut scene = scene_from_shapes(shapes, cluster_cfg, &mut progress_fn);
        scene.load_warnings.extend(warnings);
        // Testo minimo al posto del file: i pezzi sono `rewritten`, l'export non lo legge
        scene.source_data = b"#VRML V2.0 utf8\n".to_vec().into();
        return scene;
    } else {
        return parse_vrml2(data, cluster_cfg, progress_fn);
    };
    let mut scene = parse_vrml2(&converted.text, cluster_cfg, progress_fn);
    scene.load_warnings.extend(converted.warnings);
    scene.source_data = converted.text.into();
    scene
}

fn parse_vrml2<F>(data: &[u8], cluster_cfg: crate::clusterer::ClusterConfig, mut progress_fn: F) -> SceneGraph
where
    F: FnMut(f32, String),
{
    tracing::info!("Inizio parsing WRL ({} bytes)", data.len());
    progress_fn(0.05, "Scansione delimitatori single-pass O(n)...".to_string());

    let (brace_map, bracket_map) = delimiter_map::build_delimiter_maps(data);
    tracing::debug!(
        "Delimiter maps: {} braces, {} brackets",
        brace_map.len(),
        bracket_map.len()
    );

    progress_fn(0.20, "Identificazione blocchi top-level...".to_string());
    let blocks = scan_top_level_blocks(data, &brace_map);
    tracing::info!("Blocchi top-level trovati: {}", blocks.len());

    let mut defs = DefRegistry::default();
    defs.materials = crate::parser::material::prescan_materials(data, &brace_map);
    (defs.used, defs.geometries) = scan_use_and_geometry_defs(data, &brace_map);
    let hidden_switches = defs.hidden_switches.clone();

    // 1. Prima passata: pre-scansione globale di tutte le definizioni DEF
    // (coordinate, shape, transform) per popolare il registro prima del parsing parallelo
    progress_fn(0.30, "Elaborazione globale definizioni DEF...".to_string());
    pre_scan_all_defs(data, &brace_map, &bracket_map, &mut defs);

    // 2. Seconda passata: parsing dei blocchi con Rayon (parallelismo data-parallel)
    progress_fn(0.40, format!("Parsing parallelo con Rayon su {} blocchi...", blocks.len()));

    let shapes: Vec<ShapeInfo> = if blocks.len() > 1 {
        // Parallelizza sui blocchi top-level. `map_init` clona il registro DEF una
        // volta per job rayon (non una volta per blocco: con migliaia di blocchi
        // top-level la clonazione per-blocco dominava il tempo di parsing).
        let per_block: Vec<Vec<ShapeInfo>> = blocks
            .par_iter()
            .map_init(
                || defs.clone(),
                |local_defs, b| {
                    let mut local_res = Vec::new();
                    extract_shapes_recursive(
                        data,
                        b.start,
                        b.end,
                        glam::Mat4::IDENTITY,
                        &brace_map,
                        &bracket_map,
                        local_defs,
                        &mut local_res,
                    );
                    local_res
                },
            )
            .collect();
        per_block.into_iter().flatten().collect()
    } else {
        // Parsing sequenziale per file con un solo blocco contenitore
        let mut all_shapes = Vec::new();
        extract_shapes_recursive(
            data,
            0,
            data.len(),
            glam::Mat4::IDENTITY,
            &brace_map,
            &bracket_map,
            &mut defs,
            &mut all_shapes,
        );
        all_shapes
    };

    tracing::info!("Shape estratte: {}", shapes.len());

    let mut scene = scene_from_shapes(shapes, cluster_cfg, &mut progress_fn);
    let hidden = hidden_switches.lock().map_or(0, |h| h.len());
    match hidden {
        0 => {}
        1 => scene.load_warnings.push(
            "1 nodo Switch spento nel file: il suo contenuto non è caricato né esportato".to_string(),
        ),
        n => scene.load_warnings.push(format!(
            "{n} nodi Switch spenti nel file: il loro contenuto non è caricato né esportato"
        )),
    }
    tracing::info!(
        "Grafo scena: {} nodi, {} radici, {} macro-componenti",
        scene.nodes.len(),
        scene.roots.len(),
        scene.macro_components.len()
    );

    progress_fn(1.0, "Parsing completato con successo.".to_string());

    scene
}

/// Albero dei pezzi: macro-oggetti dai percorsi Group/Transform/USE e raggruppamento
/// automatico dei contenitori con molte Shape dirette.
pub(crate) fn scene_from_shapes<F>(shapes: Vec<ShapeInfo>, cluster_cfg: crate::clusterer::ClusterConfig, progress_fn: &mut F) -> SceneGraph
where
    F: FnMut(f32, String),
{
    progress_fn(
        0.85,
        format!("Ricostruzione gerarchia e raggruppamento di {} shape...", shapes.len()),
    );
    let tree = crate::scene_graph::builder::build_tree(
        shapes.iter().map(|s| (s.path.as_slice(), s.name.as_deref())),
        |ids: &[u32]| crate::clusterer::cluster_subset(&shapes, ids, &cluster_cfg),
    );
    progress_fn(0.95, "Costruzione grafo scena...".to_string());
    SceneGraph::from_built(tree, shapes)
}

/// Registra il template di un `DEF` contenitore riusato: `matrix` è la matrice locale del nodo.
#[allow(clippy::too_many_arguments)]
fn register_container(
    data: &[u8],
    name: &str,
    open: usize,
    close: usize,
    matrix: glam::Mat4,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
    defs: &mut DefRegistry,
) {
    let mut local_shapes = Vec::new();
    find_children_and_recurse(
        data,
        open + 1,
        close,
        matrix,
        brace_map,
        bracket_map,
        defs,
        &mut local_shapes,
    );
    let shapes = local_shapes.into_iter().map(to_local).collect();
    defs.add_shape_def(name, ShapeDef { pos: open, shapes, container: true, matrix });
}

/// Esegue una veloce scansione lineare di tutti i token DEF nel file WRL,
/// popolando DefRegistry (sia per Coordinate che per nodi Shape/Transform/Group).
fn pre_scan_all_defs(
    data: &[u8],
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
    defs: &mut DefRegistry,
) {
    let mut pos = 0;
    let end = data.len();

    while pos < end {
        pos = lexer::skip_whitespace_comments(data, pos);
        if pos >= end {
            break;
        }

        let Some((word, next_pos)) = lexer::scan_word(data, pos) else {
            break;
        };

        if word == "DEF" {
            pos = lexer::skip_whitespace_comments(data, next_pos);
            let Some((def_name, after_name)) = lexer::scan_word(data, pos) else {
                pos = next_pos;
                continue;
            };

            pos = lexer::skip_whitespace_comments(data, after_name);
            let Some((node_type, after_type)) = lexer::scan_word(data, pos) else {
                pos = after_name;
                continue;
            };

            // Un DEF mai riusato non serve qui: la scansione prosegue al suo interno
            // per trovare gli eventuali DEF riusati annidati. Prosegue anche dentro i contenitori
            // riusati, perché un DEF può stare in un ramo non attivo di LOD/Switch: `has_*_def`
            // evita di rianalizzare ciò che è già stato registrato dal contenitore.
            let used = defs.used.contains(def_name);
            let b_pos = lexer::skip_whitespace_comments(data, after_type);
            let block = (data.get(b_pos) == Some(&b'{'))
                .then(|| brace_map.get(&b_pos).copied())
                .flatten();
            match (node_type, block) {
                ("Coordinate", Some(close)) if used => {
                    if !defs.has_coord_def(def_name, b_pos) {
                        if let Some((p_start, p_end, verts)) =
                            parse_coordinate_points(data, b_pos + 1, close, 0, bracket_map)
                        {
                            defs.add_coord_def(
                                def_name,
                                b_pos,
                                CoordDef {
                                    point_range_start: p_start,
                                    point_range_end: p_end,
                                    points: verts,
                                },
                            );
                        }
                    }
                    pos = close + 1;
                    continue;
                }
                ("Shape", Some(close)) if used => {
                    if !defs.has_shape_def(def_name, b_pos) {
                        // Una Shape senza geometria leggibile resta una definizione vuota:
                        // i `USE` successivi non devono prendere quella precedente con lo stesso nome
                        let local = extract_shape_info(
                            data,
                            b_pos,
                            close + 1,
                            glam::Mat4::IDENTITY,
                            brace_map,
                            bracket_map,
                            defs,
                        )
                        .map(|(_, mut local)| {
                            local.name = Some(def_name.to_string());
                            local
                        });
                        defs.add_shape_def(
                            def_name,
                            ShapeDef {
                                pos: b_pos,
                                shapes: local.into_iter().collect(),
                                container: false,
                                matrix: glam::Mat4::IDENTITY,
                            },
                        );
                    }
                    pos = close + 1;
                    continue;
                }
                ("Transform", Some(close)) if used && !defs.has_shape_def(def_name, b_pos) => {
                    let local_m = transform::build_transform_matrix(
                        data,
                        b_pos + 1,
                        close,
                        brace_map,
                        bracket_map,
                    );
                    register_container(
                        data, def_name, b_pos, close, local_m, brace_map, bracket_map, defs,
                    );
                }
                ("Group" | "Anchor" | "Collision" | "LOD" | "Switch" | "Billboard", Some(close))
                    if used && !defs.has_shape_def(def_name, b_pos) =>
                {
                    register_container(
                        data,
                        def_name,
                        b_pos,
                        close,
                        glam::Mat4::IDENTITY,
                        brace_map,
                        bracket_map,
                        defs,
                    );
                }
                _ => {}
            }
            pos = after_type;
        } else if matches!(
            word,
            "point" | "vector" | "color" | "coordIndex" | "normalIndex" | "colorIndex" | "texCoordIndex"
        ) {
            // I grandi vettori numerici non contengono DEF: si saltano in un colpo solo
            let open = lexer::skip_whitespace_comments(data, next_pos);
            pos = match (data.get(open), bracket_map.get(&open)) {
                (Some(b'['), Some(&close)) => close + 1,
                _ => next_pos,
            };
        } else {
            pos = next_pos;
        }
    }
}

/// Scansiona i blocchi di primo livello del file WRL sfruttando la `brace_map`.
fn scan_top_level_blocks(
    data: &[u8],
    brace_map: &HashMap<usize, usize>,
) -> Vec<TopLevelBlock> {
    let mut blocks = Vec::new();
    let mut pos = 0;
    let end = data.len();

    while pos < end {
        pos = lexer::skip_whitespace_comments(data, pos);
        if pos >= end {
            break;
        }

        let start_pos = pos;
        let Some((word, next_pos)) = lexer::scan_word(data, pos) else {
            break;
        };

        let mut is_def = false;
        let mut cur_word = word;
        let mut cur_pos = next_pos;

        if cur_word == "DEF" {
            is_def = true;
            // Salta DEF e il nome
            cur_pos = lexer::skip_whitespace_comments(data, cur_pos);
            if let Some((_, after_name)) = lexer::scan_word(data, cur_pos) {
                cur_pos = lexer::skip_whitespace_comments(data, after_name);
                if let Some((node_type, after_type)) = lexer::scan_word(data, cur_pos) {
                    cur_word = node_type;
                    cur_pos = after_type;
                }
            }
        }

        match cur_word {
            "Transform" | "Group" | "Anchor" | "Collision" | "LOD" | "Switch" | "Billboard" | "Shape" => {
                cur_pos = lexer::skip_whitespace_comments(data, cur_pos);
                if cur_pos < end && data[cur_pos] == b'{' {
                    if let Some(&close_brace) = brace_map.get(&cur_pos) {
                        blocks.push(TopLevelBlock {
                            start: start_pos,
                            end: close_brace + 1,
                            is_def,
                        });
                        pos = close_brace + 1;
                        continue;
                    }
                }
                pos = cur_pos;
            }
            "USE" => {
                // Nodo USE a livello top-level
                cur_pos = lexer::skip_whitespace_comments(data, cur_pos);
                if let Some((_, after_name)) = lexer::scan_word(data, cur_pos) {
                    blocks.push(TopLevelBlock {
                        start: start_pos,
                        end: after_name,
                        is_def: false,
                    });
                    pos = after_name;
                } else {
                    pos = next_pos;
                }
            }
            _ => {
                // Altro elemento: salta se ha parentesi graffa
                cur_pos = lexer::skip_whitespace_comments(data, cur_pos);
                if cur_pos < end && data[cur_pos] == b'{' {
                    if let Some(&close_brace) = brace_map.get(&cur_pos) {
                        pos = close_brace + 1;
                        continue;
                    }
                }
                pos = next_pos;
            }
        }
    }

    blocks
}

/// Traversal ricorsivo completo del grafo di scena WRL con gestione di DEF e USE.
fn extract_shapes_recursive(
    data: &[u8],
    start: usize,
    end: usize,
    parent_matrix: glam::Mat4,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
    defs: &mut DefRegistry,
    results: &mut Vec<ShapeInfo>,
) {
    let mut pos = start;

    while pos < end {
        pos = lexer::skip_whitespace_comments(data, pos);
        if pos >= end {
            break;
        }

        let Some((word, next_pos)) = lexer::scan_word(data, pos) else {
            break;
        };

        if word == "DEF" {
            // DEF <nome> <NodoTipo> { ... }
            pos = next_pos;
            pos = lexer::skip_whitespace_comments(data, pos);

            let Some((def_name, after_name)) = lexer::scan_word(data, pos) else {
                break;
            };

            pos = lexer::skip_whitespace_comments(data, after_name);
            let Some((node_type, after_type)) = lexer::scan_word(data, pos) else {
                break;
            };

            match node_type {
                "Transform" => {
                    pos = lexer::skip_whitespace_comments(data, after_type);
                    if pos < end && data[pos] == b'{' {
                        let block_open = pos;
                        if let Some(&block_close) = brace_map.get(&block_open) {
                            let local_matrix = transform::build_transform_matrix(
                                data,
                                block_open + 1,
                                block_close,
                                brace_map,
                                bracket_map,
                            );
                            let world_matrix = parent_matrix * local_matrix;

                            // Forme con world_matrix per results
                            let prev_len = results.len();
                            find_children_and_recurse(
                                data,
                                block_open + 1,
                                block_close,
                                world_matrix,
                                brace_map,
                                bracket_map,
                                defs,
                                results,
                            );
                            prefix_path(
                                &mut results[prev_len..],
                                &PathSeg {
                                    key: block_open,
                                    name: Some(def_name.to_string()),
                                    kind: ContainerKind::Transform,
                                    frame: world_matrix,
                                },
                            );

                            // Registra le forme per USE con matrice relativa a questo Transform
                            if defs.used.contains(def_name) && !defs.has_shape_def(def_name, block_open) {
                                register_container(
                                    data,
                                    def_name,
                                    block_open,
                                    block_close,
                                    local_matrix,
                                    brace_map,
                                    bracket_map,
                                    defs,
                                );
                            }
                            pos = block_close + 1;
                        } else {
                            pos = after_type;
                        }
                    } else {
                        pos = after_type;
                    }
                }
                "Group" | "Anchor" | "Collision" | "LOD" | "Switch" | "Billboard" => {
                    pos = lexer::skip_whitespace_comments(data, after_type);
                    if pos < end && data[pos] == b'{' {
                        let block_open = pos;
                        if let Some(&block_close) = brace_map.get(&block_open) {
                            let prev_len = results.len();
                            find_children_and_recurse(
                                data,
                                block_open + 1,
                                block_close,
                                parent_matrix,
                                brace_map,
                                bracket_map,
                                defs,
                                results,
                            );
                            prefix_path(
                                &mut results[prev_len..],
                                &PathSeg {
                                    key: block_open,
                                    name: Some(def_name.to_string()),
                                    kind: ContainerKind::Group,
                                    frame: parent_matrix,
                                },
                            );

                            if defs.used.contains(def_name) && !defs.has_shape_def(def_name, block_open) {
                                register_container(
                                    data,
                                    def_name,
                                    block_open,
                                    block_close,
                                    glam::Mat4::IDENTITY,
                                    brace_map,
                                    bracket_map,
                                    defs,
                                );
                            }
                            pos = block_close + 1;
                        } else {
                            pos = after_type;
                        }
                    } else {
                        pos = after_type;
                    }
                }
                "Shape" => {
                    pos = lexer::skip_whitespace_comments(data, after_type);
                    if pos < end && data[pos] == b'{' {
                        let shape_start = pos;
                        if let Some(&shape_close) = brace_map.get(&shape_start) {
                            if let Some((mut world_shape, mut local_shape)) = extract_shape_info(
                                data,
                                shape_start,
                                shape_close + 1,
                                parent_matrix,
                                brace_map,
                                bracket_map,
                                defs,
                            ) {
                                world_shape.name = Some(def_name.to_string());
                                local_shape.name = Some(def_name.to_string());
                                results.push(world_shape);
                                if defs.used.contains(def_name) {
                                    defs.add_shape_def(
                                        def_name,
                                        ShapeDef {
                                            pos: shape_start,
                                            shapes: vec![local_shape],
                                            container: false,
                                            matrix: glam::Mat4::IDENTITY,
                                        },
                                    );
                                }
                            }
                            pos = shape_close + 1;
                        } else {
                            pos = after_type;
                        }
                    } else {
                        pos = after_type;
                    }
                }
                _ => {
                    pos = after_type;
                }
            }
            continue;
        }

        if word == "USE" {
            // USE <nome>: istanzia la geometria duplicando i vertici con la matrice corrente
            pos = next_pos;
            pos = lexer::skip_whitespace_comments(data, pos);

            if let Some((name, after_name)) = lexer::scan_word(data, pos) {
                let def = defs
                    .shapes
                    .get(name)
                    .and_then(|d| crate::parser::latest_before(d, pos, |d| d.pos));
                if let Some(def) = def {
                    // Un USE di un contenitore diventa un nodo "istanza" nell'albero;
                    // un USE di una singola Shape resta una foglia con il nome DEF.
                    let use_seg = def.container.then(|| PathSeg {
                        key: pos,
                        name: Some(name.to_string()),
                        kind: ContainerKind::Use,
                        frame: parent_matrix * def.matrix,
                    });
                    for t in &def.shapes {
                        let mut path = Vec::with_capacity(t.path.len() + 1);
                        if let Some(seg) = &use_seg {
                            path.push(seg.clone());
                        }
                        // I sistemi dei contenitori del template sono relativi a dove è definito
                        path.extend(t.path.iter().map(|s| PathSeg { frame: parent_matrix * s.frame, ..s.clone() }));
                        // Vertici e normali del template hanno già applicato `local_matrix`:
                        // resta solo la matrice corrente. `combined_matrix` è quella che porta
                        // dalle coordinate scritte nel file al mondo.
                        let combined_matrix = parent_matrix * t.local_matrix;
                        let normal_mat = glam::Mat3::from_mat4(parent_matrix).inverse().transpose();
                        let world_vertices = t
                            .local_vertices
                            .iter()
                            .map(|&[x, y, z]| {
                                let p = parent_matrix.transform_point3(glam::Vec3::new(x, y, z));
                                [p.x, p.y, p.z]
                            })
                            .collect();
                        let normals = t
                            .local_normals
                            .iter()
                            .map(|&[nx, ny, nz]| {
                                let n = (normal_mat * glam::Vec3::new(nx, ny, nz)).normalize_or_zero();
                                if n.length_squared() > 1e-6 {
                                    [n.x, n.y, n.z]
                                } else {
                                    [0.0, 1.0, 0.0]
                                }
                            })
                            .collect();

                        results.push(ShapeInfo {
                            source_start: t.source_start,
                            source_end: t.source_end,
                            point_range_start: t.point_range_start,
                            point_range_end: t.point_range_end,
                            matrix: combined_matrix,
                            world_vertices,
                            normals,
                            split: t.split.clone(),
                            indices: t.indices.clone(),
                            face_count: t.face_count,
                            name: t.name.clone(),
                            color: t.color,
                            path,
                            rewritten: false,
                        });
                    }
                } else {
                    tracing::warn!("USE di nodo non trovato nel registro DEF: '{}'", name);
                }
                pos = after_name;
            } else {
                pos = next_pos;
            }
            continue;
        }

        match word {
            "Transform" => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);

                if pos < end && data[pos] == b'{' {
                    let block_open = pos;
                    if let Some(&block_close) = brace_map.get(&block_open) {
                        let local_matrix = transform::build_transform_matrix(
                            data,
                            block_open + 1,
                            block_close,
                            brace_map,
                            bracket_map,
                        );
                        let world_matrix = parent_matrix * local_matrix;

                        let prev_len = results.len();
                        find_children_and_recurse(
                            data,
                            block_open + 1,
                            block_close,
                            world_matrix,
                            brace_map,
                            bracket_map,
                            defs,
                            results,
                        );
                        prefix_path(
                            &mut results[prev_len..],
                            &PathSeg { key: block_open, name: None, kind: ContainerKind::Transform, frame: world_matrix },
                        );
                        pos = block_close + 1;
                    } else {
                        pos = next_pos;
                    }
                } else {
                    pos = next_pos;
                }
            }
            "Group" | "Anchor" | "Collision" | "LOD" | "Switch" | "Billboard" => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);

                if pos < end && data[pos] == b'{' {
                    let block_open = pos;
                    if let Some(&block_close) = brace_map.get(&block_open) {
                        let prev_len = results.len();
                        find_children_and_recurse(
                            data,
                            block_open + 1,
                            block_close,
                            parent_matrix,
                            brace_map,
                            bracket_map,
                            defs,
                            results,
                        );
                        prefix_path(
                            &mut results[prev_len..],
                            &PathSeg { key: block_open, name: None, kind: ContainerKind::Group, frame: parent_matrix },
                        );
                        pos = block_close + 1;
                    } else {
                        pos = next_pos;
                    }
                } else {
                    pos = next_pos;
                }
            }
            "Shape" => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);

                if pos < end && data[pos] == b'{' {
                    let shape_start = pos;
                    if let Some(&shape_close) = brace_map.get(&shape_start) {
                        if let Some((world_shape, _)) = extract_shape_info(
                            data,
                            shape_start,
                            shape_close + 1,
                            parent_matrix,
                            brace_map,
                            bracket_map,
                            defs,
                        ) {
                            results.push(world_shape);
                        }
                        pos = shape_close + 1;
                    } else {
                        pos = next_pos;
                    }
                } else {
                    pos = next_pos;
                }
            }
            _ => {
                pos = next_pos;
            }
        }
    }
}

/// Fine del nodo che inizia in `pos` (`Tipo { ... }`, `DEF Nome Tipo { ... }`, `USE Nome`).
fn single_node_end(
    data: &[u8],
    pos: usize,
    end: usize,
    brace_map: &HashMap<usize, usize>,
) -> Option<usize> {
    let (word, after) = lexer::scan_word(data, pos)?;
    match word {
        "USE" => lexer::scan_word(data, after).map(|(_, e)| e),
        "DEF" => {
            let (_, after_name) = lexer::scan_word(data, after)?;
            single_node_end(data, after_name, end, brace_map)
        }
        _ => {
            let open = lexer::skip_whitespace_comments(data, after);
            if open < end && data[open] == b'{' {
                brace_map.get(&open).map(|&close| close + 1)
            } else {
                None
            }
        }
    }
}

/// Nodi del campo con valore in `pos` (`[ A B ]`, oppure un solo nodo senza parentesi),
/// come intervalli di byte, e posizione dopo il campo.
fn field_nodes(
    data: &[u8],
    pos: usize,
    end: usize,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
) -> Option<(Vec<(usize, usize)>, usize)> {
    let pos = lexer::skip_whitespace_comments(data, pos);
    if data.get(pos) != Some(&b'[') {
        let node_end = single_node_end(data, pos, end, brace_map)?;
        return Some((vec![(pos, node_end)], node_end));
    }
    let &close = bracket_map.get(&pos)?;
    let mut nodes = Vec::new();
    let mut p = pos + 1;
    loop {
        p = lexer::skip_whitespace_comments(data, p);
        if p >= close {
            break;
        }
        if let Some(node_end) = single_node_end(data, p, close, brace_map) {
            nodes.push((p, node_end));
            p = node_end;
        } else {
            // Virgole e altri token fuori posto: si saltano
            p = lexer::scan_word(data, p)?.1;
        }
    }
    Some((nodes, close + 1))
}

/// Valore di `whichChoice` di uno `Switch`, cercato tra i campi di primo livello del nodo.
fn which_choice(
    data: &[u8],
    start: usize,
    end: usize,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
) -> i32 {
    let mut pos = start;
    while let Some((word, next)) = lexer::scan_word(data, pos) {
        if next > end {
            break;
        }
        if word == "whichChoice" {
            return lexer::scan_word(data, next)
                .and_then(|(value, _)| value.parse().ok())
                .unwrap_or(-1);
        }
        let open = lexer::skip_whitespace_comments(data, pos);
        pos = match data[open] {
            b'{' => brace_map.get(&open).map_or(next, |&c| c + 1),
            b'[' => bracket_map.get(&open).map_or(next, |&c| c + 1),
            _ => next,
        };
    }
    -1
}

/// All'interno di un blocco Transform/Group, cerca il campo `children`
/// (`level` per i LOD, `choice` per gli Switch) e ricorre sulle shape al suo interno.
#[allow(clippy::too_many_arguments)]
fn find_children_and_recurse(
    data: &[u8],
    block_start: usize,
    block_end: usize,
    matrix: glam::Mat4,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
    defs: &mut DefRegistry,
    results: &mut Vec<ShapeInfo>,
) {
    let mut pos = block_start;

    while pos < block_end {
        pos = lexer::skip_whitespace_comments(data, pos);
        if pos >= block_end {
            break;
        }

        let Some((word, next_pos)) = lexer::scan_word(data, pos) else {
            break;
        };

        if word == "level" || word == "choice" {
            let Some((nodes, after)) =
                field_nodes(data, next_pos, block_end, brace_map, bracket_map)
            else {
                pos = next_pos;
                continue;
            };
            if word == "level" {
                // LOD: il primo livello è il più dettagliato; quelli dopo sono versioni
                // semplificate dello stesso pezzo. Se è vuoto vale il successivo.
                for &(start, end) in &nodes {
                    let before = results.len();
                    extract_shapes_recursive(
                        data, start, end, matrix, brace_map, bracket_map, defs, results,
                    );
                    if results.len() > before {
                        break;
                    }
                }
            } else {
                // Switch: solo la scelta attiva è visibile (`whichChoice`, -1 se assente: nessuna)
                let chosen = usize::try_from(which_choice(data, block_start, block_end, brace_map, bracket_map))
                    .ok()
                    .and_then(|i| nodes.get(i));
                match chosen {
                    Some(&(start, end)) => extract_shapes_recursive(
                        data, start, end, matrix, brace_map, bracket_map, defs, results,
                    ),
                    None if !nodes.is_empty() => {
                        if let Ok(mut hidden) = defs.hidden_switches.lock() {
                            hidden.insert(block_start);
                        }
                    }
                    None => {}
                }
            }
            pos = after;
        } else if word == "children" {
            pos = next_pos;
            pos = lexer::skip_whitespace_comments(data, pos);

            if pos < block_end {
                if data[pos] == b'[' {
                    if let Some(&bracket_close) = bracket_map.get(&pos) {
                        extract_shapes_recursive(
                            data,
                            pos + 1,
                            bracket_close,
                            matrix,
                            brace_map,
                            bracket_map,
                            defs,
                            results,
                        );
                        pos = bracket_close + 1;
                    } else {
                        pos = next_pos;
                    }
                } else if data[pos] == b'{' {
                    if let Some(&brace_close) = brace_map.get(&pos) {
                        extract_shapes_recursive(
                            data,
                            pos + 1,
                            brace_close,
                            matrix,
                            brace_map,
                            bracket_map,
                            defs,
                            results,
                        );
                        pos = brace_close + 1;
                    } else {
                        pos = next_pos;
                    }
                } else if let Some(node_end) = single_node_end(data, pos, block_end, brace_map) {
                    // `children Group { ... }`: un solo figlio, senza parentesi quadre
                    extract_shapes_recursive(
                        data,
                        pos,
                        node_end,
                        matrix,
                        brace_map,
                        bracket_map,
                        defs,
                        results,
                    );
                    pos = node_end;
                } else {
                    pos = next_pos;
                }
            }
        } else if data[pos] == b'{' {
            if let Some(&close) = brace_map.get(&pos) {
                pos = close + 1;
            } else {
                pos = next_pos;
            }
        } else if data[pos] == b'[' {
            if let Some(&close) = bracket_map.get(&pos) {
                pos = close + 1;
            } else {
                pos = next_pos;
            }
        } else {
            pos = next_pos;
        }
    }
}

/// Estrae le informazioni di una Shape (con IndexedFaceSet), restituendo sia
/// la forma con coordinate world-space, sia il template locale.
fn extract_shape_info(
    data: &[u8],
    shape_start: usize,
    shape_end: usize,
    matrix: glam::Mat4,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
    defs: &mut DefRegistry,
) -> Option<(ShapeInfo, LocalShape)> {
    let mut pos = shape_start + 1;

    while pos < shape_end {
        pos = lexer::skip_whitespace_comments(data, pos);
        if pos >= shape_end {
            break;
        }

        let (word, next_pos) = lexer::scan_word(data, pos)?;

        if word == "geometry" {
            pos = next_pos;
            pos = lexer::skip_whitespace_comments(data, pos);

            let (first, after_first) = lexer::scan_word(data, pos)?;
            let (ifs_open, ifs_close) = if first == "USE" {
                // geometry USE Nome: la geometria è altrove nel file
                let (name, _) = lexer::scan_word(data, after_first)?;
                let found = defs
                    .geometries
                    .get(name)
                    .and_then(|g| crate::parser::latest_before(g, pos, |g| g.0))
                    .copied();
                if found.is_none() {
                    tracing::warn!("geometry USE non trovato: {}", name);
                }
                found?
            } else {
                let (geo_type, after_type) = if first == "DEF" {
                    let (_, after_name) = lexer::scan_word(data, after_first)?;
                    lexer::scan_word(data, after_name)?
                } else {
                    (first, after_first)
                };
                if geo_type != "IndexedFaceSet" {
                    return None;
                }
                let open = lexer::skip_whitespace_comments(data, after_type);
                if open >= shape_end || data[open] != b'{' {
                    return None;
                }
                (open, *brace_map.get(&open)?)
            };

            let (mut world, mut local) = extract_indexed_face_set(
                data,
                shape_start,
                shape_end,
                ifs_open + 1,
                ifs_close,
                matrix,
                brace_map,
                bracket_map,
                defs,
            )?;
            // `shape_start` è la `{` della Shape, `shape_end` la posizione dopo la `}`
            let color = crate::parser::material::shape_color(
                data,
                shape_start,
                shape_end.saturating_sub(1),
                brace_map,
                &defs.materials,
            );
            world.color = color;
            local.color = color;
            return Some((world, local));
        } else if data[pos] == b'{' {
            if let Some(&close) = brace_map.get(&pos) {
                pos = close + 1;
            } else {
                pos = next_pos;
            }
        } else {
            pos = next_pos;
        }
    }

    None
}

/// Estrae punti e indici da un blocco `IndexedFaceSet { ... }`.
fn extract_indexed_face_set(
    data: &[u8],
    shape_start: usize,
    shape_end: usize,
    ifs_start: usize,
    ifs_end: usize,
    matrix: glam::Mat4,
    brace_map: &HashMap<usize, usize>,
    bracket_map: &HashMap<usize, usize>,
    defs: &mut DefRegistry,
) -> Option<(ShapeInfo, LocalShape)> {
    let mut local_vertices: Vec<[f32; 3]> = Vec::new();
    let mut raw_normals: Vec<[f32; 3]> = Vec::new();
    let mut raw_indices: Vec<i32> = Vec::new();
    let mut raw_normal_index: Vec<i32> = Vec::new();
    let mut normal_per_vertex = true;
    let mut point_range_start: usize = 0;
    let mut point_range_end: usize = 0;

    let mut pos = ifs_start;

    while pos < ifs_end {
        pos = lexer::skip_whitespace_comments(data, pos);
        if pos >= ifs_end {
            break;
        }

        let Some((word, next_pos)) = lexer::scan_word(data, pos) else {
            break;
        };

        match word {
            "coord" => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);

                let Some((first_token, after_first)) = lexer::scan_word(data, pos) else {
                    pos = next_pos;
                    continue;
                };

                if first_token == "USE" {
                    // coord USE MyCoords
                    let name_pos = lexer::skip_whitespace_comments(data, after_first);
                    if let Some((coord_name, after_name)) = lexer::scan_word(data, name_pos) {
                        let cdef = defs
                            .coords
                            .get(coord_name)
                            .and_then(|d| crate::parser::latest_before(d, name_pos, |d| d.0))
                            .map(|d| &d.1);
                        if let Some(cdef) = cdef {
                            local_vertices = cdef.points.clone();
                            point_range_start = cdef.point_range_start;
                            point_range_end = cdef.point_range_end;
                        } else {
                            tracing::warn!("coord USE non trovato: {}", coord_name);
                        }
                        pos = after_name;
                    } else {
                        pos = after_first;
                    }
                } else if first_token == "DEF" {
                    // coord DEF MyCoords Coordinate { point [ ... ] }
                    let name_pos = lexer::skip_whitespace_comments(data, after_first);
                    if let Some((coord_name, after_name)) = lexer::scan_word(data, name_pos) {
                        let type_pos = lexer::skip_whitespace_comments(data, after_name);
                        if let Some((coord_type, after_type)) = lexer::scan_word(data, type_pos) {
                            if coord_type == "Coordinate" {
                                let brace_pos = lexer::skip_whitespace_comments(data, after_type);
                                if brace_pos < ifs_end && data[brace_pos] == b'{' {
                                    if let Some(&coord_close) = brace_map.get(&brace_pos) {
                                        if let Some((p_start, p_end, verts)) = parse_coordinate_points(
                                            data,
                                            brace_pos + 1,
                                            coord_close,
                                            shape_start,
                                            bracket_map,
                                        ) {
                                            if defs.used.contains(coord_name)
                                                && !defs.has_coord_def(coord_name, brace_pos)
                                            {
                                                defs.add_coord_def(
                                                    coord_name,
                                                    brace_pos,
                                                    CoordDef {
                                                        point_range_start: p_start,
                                                        point_range_end: p_end,
                                                        points: verts.clone(),
                                                    },
                                                );
                                            }
                                            point_range_start = p_start;
                                            point_range_end = p_end;
                                            local_vertices = verts;
                                        }
                                        pos = coord_close + 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    pos = after_first;
                } else if first_token == "Coordinate" {
                    // coord Coordinate { point [ ... ] }
                    let brace_pos = lexer::skip_whitespace_comments(data, after_first);
                    if brace_pos < ifs_end && data[brace_pos] == b'{' {
                        if let Some(&coord_close) = brace_map.get(&brace_pos) {
                            if let Some((p_start, p_end, verts)) = parse_coordinate_points(
                                data,
                                brace_pos + 1,
                                coord_close,
                                shape_start,
                                bracket_map,
                            ) {
                                point_range_start = p_start;
                                point_range_end = p_end;
                                local_vertices = verts;
                            }
                            pos = coord_close + 1;
                            continue;
                        }
                    }
                    pos = after_first;
                } else {
                    pos = after_first;
                }
            }
            "normal" => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);
                // `normal DEF Nome Normal { ... }`: il nome non serve
                let mut normal_type = lexer::scan_word(data, pos);
                if let Some(("DEF", after_def)) = normal_type {
                    normal_type = lexer::scan_word(data, after_def)
                        .and_then(|(_, after_name)| lexer::scan_word(data, after_name));
                }
                if let Some((n_tok, after_n)) = normal_type {
                    if n_tok == "Normal" {
                        let brace_pos = lexer::skip_whitespace_comments(data, after_n);
                        if brace_pos < ifs_end && data[brace_pos] == b'{' {
                            if let Some(&n_close) = brace_map.get(&brace_pos) {
                                let mut inner = brace_pos + 1;
                                while inner < n_close {
                                    inner = lexer::skip_whitespace_comments(data, inner);
                                    if inner >= n_close {
                                        break;
                                    }
                                    if let Some((w, np)) = lexer::scan_word(data, inner) {
                                        if w == "vector" {
                                            let b_pos = lexer::skip_whitespace_comments(data, np);
                                            if b_pos < n_close && data[b_pos] == b'[' {
                                                if let Some(&b_close) = bracket_map.get(&b_pos) {
                                                    let nums = lexer::scan_numbers_f32(data, b_pos + 1, b_close);
                                                    raw_normals = nums
                                                        .chunks_exact(3)
                                                        .map(|c| [c[0], c[1], c[2]])
                                                        .collect();
                                                }
                                            }
                                            break;
                                        }
                                        inner = np;
                                    } else {
                                        break;
                                    }
                                }
                                pos = n_close + 1;
                                continue;
                            }
                        }
                    }
                }
                pos = next_pos;
            }
            "coordIndex" => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);
                if pos < ifs_end && data[pos] == b'[' {
                    let bracket_open = pos;
                    if let Some(&bracket_close) = bracket_map.get(&bracket_open) {
                        raw_indices = lexer::scan_numbers_i32(data, bracket_open + 1, bracket_close);
                        pos = bracket_close + 1;
                        continue;
                    }
                }
                pos = next_pos;
            }
            "normalIndex" => {
                pos = lexer::skip_whitespace_comments(data, next_pos);
                if pos < ifs_end && data[pos] == b'[' {
                    if let Some(&close) = bracket_map.get(&pos) {
                        raw_normal_index = lexer::scan_numbers_i32(data, pos + 1, close);
                        pos = close + 1;
                        continue;
                    }
                }
                pos = next_pos;
            }
            "normalPerVertex" => {
                pos = next_pos;
                if let Some((value, after)) = lexer::scan_word(data, pos) {
                    normal_per_vertex = value != "FALSE";
                    pos = after;
                }
            }
            _ => {
                pos = next_pos;
                pos = lexer::skip_whitespace_comments(data, pos);
                if pos < ifs_end && data[pos] == b'{' {
                    if let Some(&close) = brace_map.get(&pos) {
                        pos = close + 1;
                    }
                } else if pos < ifs_end && data[pos] == b'[' {
                    if let Some(&close) = bracket_map.get(&pos) {
                        pos = close + 1;
                    }
                }
            }
        }
    }

    if local_vertices.is_empty() {
        return None;
    }

    let (indices, face_count, corners) = triangulate_with_corners(&raw_indices, local_vertices.len());

    let file_normals = FileNormals {
        normals: &raw_normals,
        index: &raw_normal_index,
        per_vertex: normal_per_vertex,
    };
    let (local_normals, split) = match file_normals.assign(&indices, corners.as_deref(), local_vertices.len()) {
        Some(assigned) => assigned,
        None => (crate::renderer::mesh_buffer::compute_normals(&local_vertices, &indices), None),
    };

    let normal_mat = glam::Mat3::from_mat4(matrix).inverse().transpose();
    let world_normals: Vec<[f32; 3]> = local_normals
        .iter()
        .map(|&[nx, ny, nz]| {
            let n = (normal_mat * glam::Vec3::new(nx, ny, nz)).normalize_or_zero();
            if n.length_squared() > 1e-6 {
                [n.x, n.y, n.z]
            } else {
                [0.0, 1.0, 0.0]
            }
        })
        .collect();

    let world_vertices: Vec<[f32; 3]> = local_vertices
        .iter()
        .map(|&[x, y, z]| {
            let p = matrix.transform_point3(glam::Vec3::new(x, y, z));
            [p.x, p.y, p.z]
        })
        .collect();

    let world_shape = ShapeInfo {
        source_start: shape_start,
        source_end: shape_end,
        point_range_start,
        point_range_end,
        matrix,
        world_vertices,
        normals: world_normals,
        split: split.clone(),
        indices: indices.clone(),
        face_count,
        name: None,
        color: None,
        path: Vec::new(),
        rewritten: false,
    };

    let local_shape = LocalShape {
        source_start: shape_start,
        source_end: shape_end,
        point_range_start,
        point_range_end,
        local_matrix: glam::Mat4::IDENTITY,
        local_vertices,
        local_normals,
        split,
        indices,
        face_count,
        name: None,
        color: None,
        path: Vec::new(),
    };

    Some((world_shape, local_shape))
}

/// Triangola una sequenza di indici VRML coordIndex (separati da -1) con fan triangulation.
pub fn triangulate_coord_index(raw_indices: &[i32], num_vertices: usize) -> (Vec<u32>, u32) {
    let (triangles, faces, _) = triangulate_with_corners(raw_indices, num_vertices);
    (triangles, faces)
}

/// Angolo di un triangolo nel file: posizione in `coordIndex` e numero della faccia
/// (i poligoni contano in ordine di file, anche quelli scartati perché incompleti).
type Corner = (u32, u32);

/// Come [`triangulate_coord_index`], con l'origine di ogni angolo dei triangoli, che serve a
/// leggere `normalIndex`. Gli angoli mancano (`None`) se `coordIndex` non dà nessuna faccia e i
/// triangoli sono i vertici presi in ordine.
fn triangulate_with_corners(raw_indices: &[i32], num_vertices: usize) -> (Vec<u32>, u32, Option<Vec<Corner>>) {
    let mut triangulated = Vec::new();
    let mut corners: Vec<Corner> = Vec::new();
    // (vertice, posizione) dei vertici validi del poligono corrente
    let mut current: Vec<(u32, u32)> = Vec::new();
    let mut face_count = 0;
    let mut face = 0u32;
    let mut face_open = false;

    let mut close_face = |current: &mut Vec<(u32, u32)>, face: u32, triangulated: &mut Vec<u32>, corners: &mut Vec<Corner>| {
        if current.len() >= 3 {
            face_count += 1;
            for i in 1..(current.len() - 1) {
                for &(v, pos) in [current[0], current[i], current[i + 1]].iter() {
                    triangulated.push(v);
                    corners.push((pos, face));
                }
            }
        }
        current.clear();
    };

    for (pos, &idx) in raw_indices.iter().enumerate() {
        if idx == -1 {
            close_face(&mut current, face, &mut triangulated, &mut corners);
            face += u32::from(face_open);
            face_open = false;
        } else {
            face_open = true;
            if idx >= 0 && (idx as usize) < num_vertices {
                current.push((idx as u32, pos as u32));
            }
        }
    }
    close_face(&mut current, face, &mut triangulated, &mut corners);

    if triangulated.is_empty() && num_vertices >= 3 {
        face_count = (num_vertices / 3) as u32;
        for i in 0..(face_count as usize) {
            triangulated.push((i * 3) as u32);
            triangulated.push((i * 3 + 1) as u32);
            triangulated.push((i * 3 + 2) as u32);
        }
        return (triangulated, face_count, None);
    }

    (triangulated, face_count, Some(corners))
}

/// Normali scritte nel file per un `IndexedFaceSet`.
struct FileNormals<'a> {
    normals: &'a [[f32; 3]],
    /// `normalIndex`: vuoto se assente
    index: &'a [i32],
    per_vertex: bool,
}

impl FileNormals<'_> {
    /// Indice in `normals` di ogni angolo dei triangoli, secondo la specifica VRML 2.0:
    /// * per vertice, con `normalIndex`: parallelo a `coordIndex`;
    /// * per vertice, senza: le normali seguono i vertici (`coordIndex`);
    /// * per faccia, con `normalIndex`: un indice per faccia; senza: una normale per faccia.
    /// `None` se mancano normali o un indice non è valido: si ricalcolano dalla geometria.
    fn corner_ids(&self, indices: &[u32], corners: Option<&[Corner]>) -> Option<Vec<u32>> {
        if self.normals.is_empty() {
            return None;
        }
        // `normalIndex [ -1 ]` vale come assente
        let index = if self.index.iter().all(|&i| i == -1) { &[][..] } else { self.index };
        let count = self.normals.len();
        let valid = |i: i64| usize::try_from(i).ok().filter(|&i| i < count).map(|i| i as u32);
        let at = |list: &[i32], k: u32| list.get(k as usize).and_then(|&i| valid(i.into()));
        match (corners, self.per_vertex, index.is_empty()) {
            (_, true, true) => indices.iter().map(|&v| valid(v.into())).collect(),
            // Senza `coordIndex` non c'è corrispondenza con `normalIndex` né con le facce
            (None, _, _) => None,
            (Some(c), true, false) => c.iter().map(|&(pos, _)| at(index, pos)).collect(),
            (Some(c), false, false) => c.iter().map(|&(_, face)| at(index, face)).collect(),
            (Some(c), false, true) => c.iter().map(|&(_, face)| valid(face.into())).collect(),
        }
    }

    /// Normali per vertice di resa e, se un vertice ha normali diverse sulle sue facce, le copie
    /// da aggiungere. Il primo uso di un vertice tiene il vertice originale.
    fn assign(
        &self,
        indices: &[u32],
        corners: Option<&[Corner]>,
        vertices: usize,
    ) -> Option<(Vec<[f32; 3]>, Option<Box<crate::parser::SplitNormals>>)> {
        let ids = self.corner_ids(indices, corners)?;
        let bits = |n: [f32; 3]| n.map(f32::to_bits);
        const NONE: u32 = u32::MAX;
        // Vertici non usati da nessun triangolo: non si disegnano, la normale non conta
        let mut normals = vec![[0.0f32, 0.0, 1.0]; vertices];
        let mut seen = vec![false; vertices];
        let mut extra: Vec<u32> = Vec::new();
        // Catena delle copie di ogni vertice di resa: un vertice ha di solito poche normali
        // diverse, scorrerle costa meno di una tabella hash
        let mut next = vec![NONE; vertices];
        let mut render: Option<Vec<u32>> = None;

        for (k, (&v, &id)) in indices.iter().zip(&ids).enumerate() {
            let n = self.normals[id as usize];
            let vi = v as usize;
            let r = if !seen[vi] {
                seen[vi] = true;
                normals[vi] = n;
                v
            } else {
                let mut r = v;
                loop {
                    if bits(normals[r as usize]) == bits(n) {
                        break r;
                    }
                    if next[r as usize] == NONE {
                        let copy = normals.len() as u32;
                        next[r as usize] = copy;
                        next.push(NONE);
                        extra.push(v);
                        normals.push(n);
                        break copy;
                    }
                    r = next[r as usize];
                }
            };
            match &mut render {
                Some(list) => list.push(r),
                None if r != v => {
                    let mut list = Vec::with_capacity(indices.len());
                    list.extend_from_slice(&indices[..k]);
                    list.push(r);
                    render = Some(list);
                }
                None => {}
            }
        }
        let split = render.map(|indices| Box::new(crate::parser::SplitNormals { extra, indices }));
        Some((normals, split))
    }
}

/// Estrae i punti e il range da un blocco interno `Coordinate { point [ ... ] }`.
fn parse_coordinate_points(
    data: &[u8],
    start: usize,
    end: usize,
    shape_start: usize,
    bracket_map: &HashMap<usize, usize>,
) -> Option<(usize, usize, Vec<[f32; 3]>)> {
    let mut inner = start;
    while inner < end {
        inner = lexer::skip_whitespace_comments(data, inner);
        if inner >= end {
            break;
        }
        if let Some((w2, np2)) = lexer::scan_word(data, inner) {
            if w2 == "point" {
                let b_pos = lexer::skip_whitespace_comments(data, np2);
                if b_pos < end && data[b_pos] == b'[' {
                    let bracket_open = b_pos;
                    if let Some(&bracket_close) = bracket_map.get(&bracket_open) {
                        // Con `geometry USE` i punti precedono la Shape: da qui `saturating_sub`
                        let point_range_start = (bracket_open + 1).saturating_sub(shape_start);
                        let point_range_end = bracket_close.saturating_sub(shape_start);

                        let nums = lexer::scan_numbers_f32(data, bracket_open + 1, bracket_close);
                        let local_vertices = nums
                            .chunks_exact(3)
                            .map(|c| [c[0], c[1], c[2]])
                            .collect();
                        return Some((point_range_start, point_range_end, local_vertices));
                    }
                }
                inner = np2;
            } else {
                inner = np2;
            }
        } else {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_shape() {
        let wrl = br#"#VRML V2.0 utf8
Shape {
    geometry IndexedFaceSet {
        coord Coordinate {
            point [ 0.0 0.0 0.0, 1.0 0.0 0.0, 0.0 1.0 0.0 ]
        }
        coordIndex [ 0, 1, 2, -1 ]
    }
}
"#;
        let scene = parse_wrl(wrl);
        assert_eq!(scene.all_shape_indices().len(), 1);
        assert_eq!(scene.roots.len(), 1);
    }

    #[test]
    fn test_shape_colors_are_parsed() {
        let wrl = br#"#VRML V2.0 utf8
DEF Pezzo Shape {
    appearance Appearance { material DEF Blu Material { diffuseColor 0 0 1 } }
    geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] }
}
Transform { translation 5 0 0 children [ USE Pezzo ] }
Shape {
    appearance Appearance { material USE Blu }
    geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] }
}
Shape {
    geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] }
}
"#;
        let scene = parse_wrl(wrl);
        let colors: Vec<_> = scene.shapes_data.iter().map(|s| s.color).collect();
        assert_eq!(colors.len(), 4);
        assert_eq!(colors.iter().filter(|c| **c == Some([0.0, 0.0, 1.0, 1.0])).count(), 3);
        assert_eq!(colors.iter().filter(|c| c.is_none()).count(), 1);
    }

    #[test]
    fn test_example_file_colors() {
        let scene = parse_wrl(include_bytes!("../../esempi/test_colori.wrl"));
        let colors: Vec<_> = scene.shapes_data.iter().map(|s| s.color).collect();
        assert_eq!(colors.len(), 7);
        let red = Some([0.85, 0.15, 0.1, 1.0]);
        let blue = Some([0.1, 0.35, 0.85, 1.0]);
        assert_eq!(colors.iter().filter(|c| **c == red).count(), 2);
        assert_eq!(colors.iter().filter(|c| **c == blue).count(), 2);
        assert!(colors.iter().any(|c| c.is_some_and(|c| (c[3] - 0.4).abs() < 1e-5)));
        assert!(colors.contains(&Some([0.95, 0.8, 0.1, 1.0])));
        assert_eq!(colors.iter().filter(|c| c.is_none()).count(), 1);
    }

    #[test]
    fn test_user_colors_inherit_and_rename() {
        let wrl = br#"#VRML V2.0 utf8
DEF Assieme Group { children [
    DEF A Group { children [
        Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
                geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }
    ] }
    DEF B Group { children [
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0 1 2 -1 ] } }
    ] }
] }
"#;
        let mut scene = parse_wrl(wrl);
        let root = scene.roots[0];
        let a = scene.nodes[root as usize].children()[0];
        let mut out = Vec::new();

        scene.set_color(root, Some([0.0, 1.0, 0.0, 1.0]));
        scene.mesh_user_colors_into(&mut out);
        assert!(out.iter().all(|c| *c == Some([0.0, 1.0, 0.0, 1.0])));

        // Colore sul figlio: vale solo per il suo sotto-albero
        scene.set_color(a, Some([0.0, 0.0, 1.0, 1.0]));
        scene.mesh_user_colors_into(&mut out);
        assert!(out.contains(&Some([0.0, 0.0, 1.0, 1.0])));
        assert!(out.contains(&Some([0.0, 1.0, 0.0, 1.0])));

        // Colore sul padre: sostituisce quelli dei figli
        scene.set_color(root, None);
        scene.mesh_user_colors_into(&mut out);
        assert!(out.iter().all(|c| c.is_none()));

        let rev = scene.revision;
        scene.rename(a, "  Staffa  ");
        assert_eq!(scene.display_name(a), "Staffa");
        assert!(scene.revision > rev);
        scene.rename(a, "");
        assert_ne!(scene.display_name(a), "Staffa");
    }

    #[test]
    fn test_parse_transform_nesting() {
        let wrl = br#"#VRML V2.0 utf8
Transform {
    translation 10 0 0
    children [
        Transform {
            translation 0 5 0
            children [
                Shape {
                    geometry IndexedFaceSet {
                        coord Coordinate {
                            point [ 1 1 1 ]
                        }
                    }
                }
            ]
        }
    ]
}
"#;
        let scene = parse_wrl(wrl);
        assert_eq!(scene.all_shape_indices().len(), 1);
    }

    #[test]
    fn test_parse_def_use_shape() {
        let wrl = br#"#VRML V2.0 utf8
DEF Cube Shape {
    geometry IndexedFaceSet {
        coord Coordinate {
            point [ 0 0 0, 1 0 0, 1 1 0 ]
        }
        coordIndex [ 0, 1, 2, -1 ]
    }
}
Transform {
    translation 5 0 0
    children [
        USE Cube
    ]
}
"#;
        let scene = parse_wrl(wrl);
        // Devono esserci 2 Shape: quella originale (Cube a x=0) e quella istanziata da USE (a x=5)
        assert_eq!(scene.all_shape_indices().len(), 2);
    }

    #[test]
    fn test_parse_coord_def_use() {
        let wrl = br#"#VRML V2.0 utf8
Shape {
    geometry IndexedFaceSet {
        coord DEF SharedPts Coordinate {
            point [ 10 20 30, 40 50 60 ]
        }
        coordIndex [ 0, 1, -1 ]
    }
}
Shape {
    geometry IndexedFaceSet {
        coord USE SharedPts
        coordIndex [ 1, 0, -1 ]
    }
}
"#;
        let scene = parse_wrl(wrl);
        assert_eq!(scene.all_shape_indices().len(), 2);
    }

    /// Regressione: con `children` senza parentesi quadre il contenuto veniva saltato
    /// (file da 542 MB aperto con 0 Shape).
    #[test]
    fn test_children_without_brackets() {
        let wrl = br#"#VRML V2.0 utf8
Collision {
  children
  Group {
    children [
      WorldInfo { title "prova" } ,
      Transform {
        translation 1 0 0
        children
        DEF Interno Transform {
          translation 0 2 0
          children
          DEF Pezzo Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
        }
      } ,
      Transform { translation 0 0 3 children USE Pezzo }
    ]
  }
}
"#;
        let scene = parse_wrl(wrl);
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].world_vertices[0], [1.0, 2.0, 0.0]);
        assert_eq!(s[1].world_vertices[0], [0.0, 0.0, 3.0]);
    }

    /// Regressione: i campi scritti dopo `children` venivano ignorati (file HOOPS con
    /// `scale 0.001` in fondo: pezzi in millimetri 1000 volte troppo grandi e fuori posto).
    #[test]
    fn test_transform_fields_after_children() {
        let wrl = br#"#VRML V2.0 utf8
Transform {
  children
  Transform {
    children
    DEF scale Transform {
      translation 7 7 7
      children [
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 1000 0 0, 2000 0 0, 1000 1000 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
      ]
      scale 2 2 2
    }
    translation 1 0 0
    rotation 0 0 1  0
    scale 0.001 0.001 0.001
  }
}
"#;
        let scene = parse_wrl(wrl);
        let v = scene.shapes_data[0].world_vertices[0];
        // 1000 → ×2 → +7 → ×0.001 → +1
        let expected = [3.007, 0.007, 0.007];
        assert!((0..3).all(|k| (v[k] - expected[k]).abs() < 1e-4), "{v:?}");
    }

    /// Forma di partenza in `x`, per riconoscere quali Shape sono state caricate.
    fn shape_at(x: i32) -> String {
        format!(
            "Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ {x} 0 0, {x} 1 0, {x} 0 1 ] }} coordIndex [ 0 1 2 -1 ] }} }}"
        )
    }

    fn loaded_x(wrl: &str) -> Vec<f32> {
        parse_wrl(wrl.as_bytes()).shapes_data.iter().map(|s| s.world_vertices[0][0]).collect()
    }

    /// Regressione: il contenuto dei LOD era ignorato (su Scocca_B mancava lo sportello carburante).
    /// Si carica il livello più dettagliato, una volta sola.
    #[test]
    fn test_lod_loads_first_level_only() {
        let (a, b) = (shape_at(1), shape_at(2));
        assert_eq!(loaded_x(&format!("LOD {{ range [ 10 ] level [ {a} {b} ] }}")), [1.0]);
        // Formato HOOPS: dentro Group/children senza parentesi quadre, range senza vettore
        assert_eq!(
            loaded_x(&format!(
                "DEF P Group {{ children LOD {{ center 0 0 0 range 293.25 level [ Group {{ children {a} }} , {b} ] }} }}"
            )),
            [1.0]
        );
        assert_eq!(loaded_x(&format!("LOD {{ level {a} }}")), [1.0]);
        // Primo livello vuoto: vale il successivo
        assert_eq!(loaded_x(&format!("LOD {{ level [ Group {{ }} {b} ] }}")), [2.0]);
        // Il livello è sotto il Transform del padre
        assert_eq!(
            loaded_x(&format!("Transform {{ translation 5 0 0 children LOD {{ level [ {a} ] }} }}")),
            [6.0]
        );
    }

    #[test]
    fn test_lod_inside_reused_def() {
        let wrl = format!(
            "DEF L LOD {{ level [ {} ] }} Transform {{ translation 10 0 0 children [ USE L ] }}",
            shape_at(1)
        );
        assert_eq!(loaded_x(&wrl), [1.0, 11.0]);
    }

    #[test]
    fn test_switch_loads_active_choice() {
        let (a, b) = (shape_at(1), shape_at(2));
        assert_eq!(loaded_x(&format!("Switch {{ whichChoice 1 choice [ {a} {b} ] }}")), [2.0]);
        // whichChoice può essere scritto dopo `choice`
        assert_eq!(loaded_x(&format!("Switch {{ choice [ {a} {b} ] whichChoice 0 }}")), [1.0]);

        for wrl in [
            format!("Switch {{ choice [ {a} {b} ] }}"),
            format!("Switch {{ whichChoice -1 choice [ {a} ] }}"),
            format!("Switch {{ whichChoice 7 choice [ {a} ] }}"),
        ] {
            let scene = parse_wrl(wrl.as_bytes());
            assert!(scene.shapes_data.is_empty(), "{wrl}");
            assert_eq!(scene.load_warnings.len(), 1, "{wrl}");
            assert!(scene.load_warnings[0].contains("Switch"));
        }
        // Uno Switch vuoto non nasconde niente: nessun avviso
        assert!(parse_wrl(b"Switch { choice [ ] }").load_warnings.is_empty());
        assert!(parse_wrl(format!("Switch {{ whichChoice 0 choice [ {a} ] }}").as_bytes())
            .load_warnings
            .is_empty());
    }

    /// Un DEF in un ramo non attivo resta valido per i USE successivi.
    #[test]
    fn test_def_in_inactive_branch_is_usable() {
        let wrl = format!(
            "DEF Esterno Transform {{ children [ Switch {{ choice [ DEF Nascosta {} ] }} ] }}\n\
             Transform {{ translation 10 0 0 children [ USE Nascosta USE Esterno ] }}",
            shape_at(1)
        );
        assert_eq!(loaded_x(&wrl), [11.0]);
    }

    #[test]
    fn test_geometry_def_and_use() {
        let wrl = format!(
            "Shape {{ geometry DEF G IndexedFaceSet {{ coord Coordinate {{ point [ 1 0 0, 2 0 0, 1 1 0, 2 1 0 ] }} coordIndex [ 0 1 3 2 -1 ] }} }}\n\
             Transform {{ translation 10 0 0 children [ Shape {{ appearance Appearance {{ material Material {{ diffuseColor 1 0 0 }} }} geometry USE G }} ] }}\n\
             Shape {{ geometry USE Assente }}"
        );
        let scene = parse_wrl(wrl.as_bytes());
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].world_vertices[0], [1.0, 0.0, 0.0]);
        assert_eq!(s[0].face_count, 1);
        assert_eq!(s[1].world_vertices[0], [11.0, 0.0, 0.0]);
        assert_eq!(s[1].indices, s[0].indices);
        // Il colore è quello della Shape che riusa la geometria
        assert_eq!(s[0].color, None);
        assert_eq!(s[1].color, Some([1.0, 0.0, 0.0, 1.0]));
    }

    /// Un `coord USE` dentro una geometria riusata e una geometria ridefinita.
    #[test]
    fn test_geometry_use_with_shared_coords_and_redefinition() {
        let wrl = "DEF Prima Shape { geometry IndexedFaceSet { coord DEF P Coordinate { point [ 1 0 0, 1 1 0, 1 0 1 ] } coordIndex [ 0 1 2 -1 ] } }\n\
             Shape { geometry DEF G IndexedFaceSet { coord USE P coordIndex [ 2 1 0 -1 ] } }\n\
             Shape { geometry USE G }\n\
             Shape { geometry DEF G IndexedFaceSet { coord Coordinate { point [ 5 0 0, 5 1 0, 5 0 1 ] } coordIndex [ 0 1 2 -1 ] } }\n\
             Shape { geometry USE G }";
        let scene = parse_wrl(wrl.as_bytes());
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 5);
        assert_eq!(s[2].world_vertices[0], [1.0, 0.0, 0.0]);
        assert_eq!(s[2].indices, vec![2, 1, 0]);
        assert_eq!(s[4].world_vertices[0], [5.0, 0.0, 0.0]);
    }

    /// Regressione: un nome ridefinito si risolveva sull'ultima definizione del file,
    /// anche per i `USE` scritti prima di essa.
    #[test]
    fn test_redefined_def_resolves_on_the_preceding_one() {
        let wrl = format!(
            "DEF P {a}\nTransform {{ translation 10 0 0 children [ USE P ] }}\n\
             DEF P {b}\nTransform {{ translation 20 0 0 children [ USE P ] }}\n\
             DEF P Group {{ children [ {c} ] }}\nTransform {{ translation 30 0 0 children [ USE P ] }}\n\
             Shape {{ geometry IndexedFaceSet {{ coord DEF C Coordinate {{ point [ 4 0 0, 4 1 0, 4 0 1 ] }} coordIndex [ 0 1 2 -1 ] }} }}\n\
             Shape {{ geometry IndexedFaceSet {{ coord USE C coordIndex [ 0 1 2 -1 ] }} }}\n\
             Shape {{ geometry IndexedFaceSet {{ coord DEF C Coordinate {{ point [ 5 0 0, 5 1 0, 5 0 1 ] }} coordIndex [ 0 1 2 -1 ] }} }}\n\
             Shape {{ geometry IndexedFaceSet {{ coord USE C coordIndex [ 0 1 2 -1 ] }} }}",
            a = shape_at(1),
            b = shape_at(2),
            c = shape_at(3),
        );
        assert_eq!(loaded_x(&wrl), [1.0, 11.0, 2.0, 22.0, 3.0, 33.0, 4.0, 4.0, 5.0, 5.0]);

        // Anche in un file con un solo blocco, dove i DEF si registrano durante la discesa
        let wrl = format!(
            "Group {{ children [ DEF P {a} Transform {{ translation 10 0 0 children [ USE P ] }} DEF P {b} USE P ] }}",
            a = shape_at(1),
            b = shape_at(2),
        );
        assert_eq!(loaded_x(&wrl), [1.0, 11.0, 2.0, 2.0]);
    }

    /// `DEF` riusati annidati in più livelli: il costo non deve raddoppiare a ogni livello.
    #[test]
    fn test_deep_used_def_nesting_is_not_exponential() {
        let depth = 40;
        let mut wrl = String::from("#VRML V2.0 utf8\n");
        for i in 0..depth {
            wrl.push_str(&format!("DEF L{i} Group {{ children [\n"));
        }
        wrl.push_str(&shape_at(1));
        wrl.push_str(&"] }\n".repeat(depth));
        wrl.push_str("Transform { translation 10 0 0 children [ USE L39 ] }\n");
        for i in 0..depth {
            wrl.push_str(&format!("# L{i}\nGroup {{ children [ USE L{i} ] }}\n"));
        }
        // La Shape originale, il primo USE e un USE per ogni livello
        assert_eq!(parse_wrl(wrl.as_bytes()).shapes_data.len(), 2 + depth);
    }

    /// Regressione: il costo raddoppiava a ogni livello di `DEF` annidati (qui sarebbero
    /// 2^40 passaggi). Il `DEF` riusato in fondo all'annidamento deve restare valido.
    #[test]
    fn test_deep_def_nesting_is_linear() {
        let depth = 40;
        let mut wrl = String::from("#VRML V2.0 utf8\n");
        for i in 0..depth {
            wrl.push_str(&format!("DEF Livello{i} Transform {{ translation 1 0 0 children [\n"));
        }
        wrl.push_str(
            "DEF Pezzo Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }\n",
        );
        wrl.push_str(&"] }\n".repeat(depth));
        wrl.push_str("Transform { translation 0 5 0 children [ USE Pezzo ] }\n");

        let scene = parse_wrl(wrl.as_bytes());
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].world_vertices[0], [40.0, 0.0, 0.0]);
        assert_eq!(s[1].world_vertices[0], [0.0, 5.0, 0.0]);
    }

    /// Regressione: il Transform del DEF veniva applicato due volte alle istanze USE.
    #[test]
    fn test_use_of_transform_applies_matrix_once() {
        let wrl = br#"#VRML V2.0 utf8
DEF Sub Transform {
    translation 1 0 0
    children [
        Shape { geometry IndexedFaceSet {
            coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] }
            coordIndex [ 0, 1, 2, -1 ]
            normal Normal { vector [ 0 0 1, 0 0 1, 0 0 1 ] }
        } }
    ]
}
Transform { translation 10 0 0 rotation 0 1 0 1.5707963 children [ USE Sub ] }
DEF Gruppo Group { children [
    Transform { translation 0 2 0 children [
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
    ] }
] }
Transform { translation 0 0 7 children [ USE Gruppo ] }
"#;
        let scene = parse_wrl(wrl);
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-5);
        let s = &scene.shapes_data;
        assert_eq!(s.len(), 4);
        assert!(close(s[0].world_vertices[0], [1.0, 0.0, 0.0]), "{:?}", s[0].world_vertices[0]);
        // Rotazione di 90° attorno a Y: (1,0,0) → (0,0,-1), poi traslazione di 10 in X
        assert!(close(s[1].world_vertices[0], [10.0, 0.0, -1.0]), "{:?}", s[1].world_vertices[0]);
        assert!(close(s[1].normals[0], [1.0, 0.0, 0.0]), "{:?}", s[1].normals[0]);
        assert!(close(s[3].world_vertices[0], [0.0, 2.0, 7.0]), "{:?}", s[3].world_vertices[0]);
        // La matrice della Shape porta sempre dalle coordinate del file al mondo
        let m = s[1].matrix;
        assert!(close(m.transform_point3(glam::Vec3::ZERO).into(), [10.0, 0.0, -1.0]));
    }

    #[test]
    fn test_parse_def_use_nested_transforms() {
        let wrl = br#"#VRML V2.0 utf8
DEF SubAssembly Transform {
    translation 1 0 0
    children [
        Shape {
            geometry IndexedFaceSet {
                coord Coordinate { point [ 0 0 0 ] }
                coordIndex [ 0, -1 ]
            }
        }
    ]
}
Transform {
    translation 10 0 0
    children [
        USE SubAssembly
    ]
}
"#;
        let scene = parse_wrl(wrl);
        assert_eq!(scene.all_shape_indices().len(), 2);
    }

    #[test]
    fn test_parallel_multi_block_parsing() {
        let mut wrl = Vec::from(&b"#VRML V2.0 utf8\n"[..]);
        for i in 0..15 {
            let block = format!(
                "Transform {{ translation {i} 0 0 children [ Shape {{ geometry IndexedFaceSet {{ coord Coordinate {{ point [ 0 0 0 ] }} coordIndex [ 0, -1 ] }} }} ] }}\n"
            );
            wrl.extend_from_slice(block.as_bytes());
        }
        let scene = parse_wrl(&wrl);
        assert_eq!(scene.all_shape_indices().len(), 15);
    }

    #[test]
    fn test_hierarchy_is_reconstructed() {
        let wrl = br#"#VRML V2.0 utf8
DEF Assieme Transform {
    children [
        DEF Staffa Transform {
            translation 1 0 0
            children [
                Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
                Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 1, 1 0 1, 0 1 1 ] } coordIndex [ 0, 1, 2, -1 ] } }
            ]
        }
        DEF Vite Shape { geometry IndexedFaceSet { coord Coordinate { point [ 5 5 5, 6 5 5, 5 6 5 ] } coordIndex [ 0, 1, 2, -1 ] } }
    ]
}
"#;
        let scene = parse_wrl(wrl);
        assert_eq!(scene.roots.len(), 1);
        let root = scene.roots[0];
        assert_eq!(scene.display_name(root), "Assieme");
        let kids = scene.nodes[root as usize].children().to_vec();
        assert_eq!(kids.len(), 2);
        assert_eq!(scene.display_name(kids[0]), "Staffa");
        assert_eq!(scene.nodes[kids[0] as usize].children().len(), 2);
        assert_eq!(scene.display_name(kids[1]), "Vite");
        assert_eq!(scene.info[root as usize].shape_count, 3);
        assert_eq!(scene.parent_of(kids[0]), Some(root));
    }

    #[test]
    fn test_use_of_container_creates_instance_nodes() {
        let wrl = br#"#VRML V2.0 utf8
DEF Ruota Group {
    children [
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
        Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 1, 1 0 1, 0 1 1 ] } coordIndex [ 0, 1, 2, -1 ] } }
    ]
}
Transform { translation 10 0 0 children [ USE Ruota ] }
"#;
        let scene = parse_wrl(wrl);
        assert_eq!(scene.all_shape_indices().len(), 4);
        // Radici: il DEF originale e l'istanza USE (il Transform anonimo viene fuso)
        assert_eq!(scene.roots.len(), 2);
        assert_eq!(scene.display_name(scene.roots[0]), "Ruota");
        assert_eq!(scene.display_name(scene.roots[1]), "Ruota");
        assert_eq!(scene.info[scene.roots[1] as usize].shape_count, 2);
    }

    #[test]
    fn test_isolate_hide_delete() {
        let wrl = br#"#VRML V2.0 utf8
DEF A Group { children [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
] }
DEF B Group { children [
    Shape { geometry IndexedFaceSet { coord Coordinate { point [ 9 9 9, 8 9 9, 9 8 9 ] } coordIndex [ 0, 1, 2, -1 ] } }
] }
"#;
        let mut scene = parse_wrl(wrl);
        let (a, b) = (scene.roots[0], scene.roots[1]);
        let mut vis = Vec::new();

        scene.isolate(a);
        scene.mesh_visibility_into(&mut vis);
        assert_eq!(vis, vec![true, false]);

        scene.show_all();
        scene.set_subtree_visible(a, false);
        scene.mesh_visibility_into(&mut vis);
        assert_eq!(vis, vec![false, true]);

        scene.show_all();
        scene.delete_node(b);
        scene.mesh_visibility_into(&mut vis);
        assert_eq!(vis, vec![true, false]);
        assert_eq!(scene.roots, vec![a]);
    }

    #[test]
    fn test_pick_hits_nearest_visible_mesh() {
        let wrl = br#"#VRML V2.0 utf8
Shape { geometry IndexedFaceSet { coord Coordinate { point [ -1 -1 0, 1 -1 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
Shape { geometry IndexedFaceSet { coord Coordinate { point [ -1 -1 5, 1 -1 5, 0 1 5 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;
        let scene = parse_wrl(wrl);
        let origin = glam::Vec3::new(0.0, 0.0, 10.0);
        let dir = glam::Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(scene.pick(origin, dir, &[true, true]), Some(1));
        assert_eq!(scene.pick(origin, dir, &[true, false]), Some(0));
        assert_eq!(scene.pick(origin, dir, &[false, false]), None);

        let (mesh, t) = scene.pick_hit(origin, dir, &[true, true]).unwrap();
        assert_eq!(mesh, 1);
        assert!((t - 5.0).abs() < 1e-5);
    }

    /// Elenca i pezzi più lontani dal centro del modello, con percorso e trasformazione:
    /// `GESTORE_WRL_FILE=percorso cargo test diagnosi_pezzi_lontani -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn diagnosi_pezzi_lontani() {
        let path = std::env::var("GESTORE_WRL_FILE").expect("impostare GESTORE_WRL_FILE");
        let scene = parse_wrl(&std::fs::read(&path).unwrap());
        let mut pieces: Vec<(usize, glam::Vec3, glam::Vec3)> = scene
            .shapes_data
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.world_vertices.is_empty())
            .map(|(i, s)| {
                let mut min = glam::Vec3::splat(f32::MAX);
                let mut max = glam::Vec3::splat(f32::MIN);
                for &v in &s.world_vertices {
                    min = min.min(v.into());
                    max = max.max(v.into());
                }
                (i, (min + max) * 0.5, max - min)
            })
            .collect();
        // GESTORE_WRL_NOME: elenca anche i pezzi con questo testo nel nome o nel percorso
        if let Ok(wanted) = std::env::var("GESTORE_WRL_NOME") {
            for &(i, center, size) in &pieces {
                let s = &scene.shapes_data[i];
                let names: Vec<&str> = s.path.iter().filter_map(|p| p.name.as_deref()).collect();
                let path = names.join(" / ");
                if path.contains(&wanted) || s.name.as_deref().is_some_and(|n| n.contains(&wanted)) {
                    println!(
                        "trovato: {} triangoli, centro {center:.1}, dim {size:.1}, colore {:?}: {path}",
                        s.indices.len() / 3,
                        s.color
                    );
                }
            }
        }
        let mut median = glam::Vec3::ZERO;
        for axis in 0..3 {
            let mut v: Vec<f32> = pieces.iter().map(|p| p.1[axis]).collect();
            v.sort_by(f32::total_cmp);
            median[axis] = v[v.len() / 2];
        }
        pieces.sort_by(|a, b| b.1.distance(median).total_cmp(&a.1.distance(median)));
        let mut dist: Vec<f32> = pieces.iter().map(|p| p.1.distance(median)).collect();
        dist.reverse();
        let at = |q: f32| dist[((dist.len() - 1) as f32 * q) as usize];
        println!("{} pezzi, centro mediano {median}", pieces.len());
        println!(
            "distanze: 50% {:.1}  90% {:.1}  99% {:.1}  max {:.1}",
            at(0.5), at(0.9), at(0.99), at(1.0)
        );
        for &(i, center, size) in pieces.iter().take(40) {
            let s = &scene.shapes_data[i];
            let (scale, rot, tr) = s.matrix.to_scale_rotation_translation();
            let names: Vec<&str> = s.path.iter().filter_map(|p| p.name.as_deref()).collect();
            println!(
                "dist {:9.1} centro {center:.1} dim {size:.1} T {tr:.1} S {scale:.3} R {rot:.3} byte {} {:?} {}",
                center.distance(median), s.source_start, s.name, names.join(" / ")
            );
        }
    }

    /// Tempo di picking su un file reale, con raggi puntati sui pezzi più pesanti:
    /// `GESTORE_WRL_FILE=percorso cargo test tempo_picking -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tempo_picking_file_reale() {
        let path = std::env::var("GESTORE_WRL_FILE").expect("impostare GESTORE_WRL_FILE");
        let scene = parse_wrl(&std::fs::read(&path).unwrap());
        let triangles: usize = scene.shapes_data.iter().map(|s| s.indices.len() / 3).sum();
        let visible = vec![true; scene.shapes_data.len()];
        let bbox = scene.compute_bounding_box();
        let far = bbox.diagonal() * 2.0;
        let mut by_size: Vec<_> = scene.shapes_data.iter().collect();
        by_size.sort_by_key(|s| std::cmp::Reverse(s.indices.len()));
        let mut worst = std::time::Duration::ZERO;
        let mut hits = 0;
        let mut rays = 0;
        for s in by_size.iter().take(10) {
            let Some(tri) = s.indices.chunks_exact(3).nth(s.indices.len() / 6) else { continue };
            let p = |i: usize| glam::Vec3::from(s.world_vertices[tri[i] as usize]);
            let aim = (p(0) + p(1) + p(2)) / 3.0;
            for dir in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z, glam::Vec3::ONE.normalize()] {
                let t = std::time::Instant::now();
                hits += usize::from(scene.pick_hit(aim - dir * far, dir, &visible).is_some());
                worst = worst.max(t.elapsed());
                rays += 1;
            }
        }
        println!("{triangles} triangoli: {hits}/{rays} raggi a segno, caso peggiore {worst:?}");
        assert!(hits > 0);
    }

    /// Due quadrati piegati a 90° lungo lo spigolo 1-2, con i campi delle normali in `fields`.
    fn folded(fields: &str) -> ShapeInfo {
        let wrl = format!(
            "#VRML V2.0 utf8\nShape {{ geometry IndexedFaceSet {{
                coord Coordinate {{ point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0, 1 0 -1, 1 1 -1 ] }}
                coordIndex [ 0 1 2 3 -1, 1 4 5 2 -1 ]
                {fields}
            }} }}"
        );
        parse_wrl(wrl.as_bytes()).shapes_data.remove(0)
    }

    const UP: [f32; 3] = [0.0, 0.0, 1.0];
    const SIDE: [f32; 3] = [1.0, 0.0, 0.0];

    /// Spigolo vivo con `normalIndex` (come nei file HOOPS): i vertici dello spigolo vengono
    /// sdoppiati solo per la resa, la geometria resta quella del file.
    #[test]
    fn normal_index_keeps_sharp_edges() {
        let s = folded(
            "normal Normal { vector [ 0 0 1, 1 0 0 ] }
             normalIndex [ 0 0 0 0 -1, 1 1 1 1 -1 ]",
        );
        assert_eq!(s.world_vertices.len(), 6);
        assert_eq!(s.indices, [0, 1, 2, 0, 2, 3, 1, 4, 5, 1, 5, 2]);
        let corners = s.corner_normals();
        assert!(corners[..6].iter().all(|&n| n == UP), "{corners:?}");
        assert!(corners[6..].iter().all(|&n| n == SIDE), "{corners:?}");
        // I vertici 1 e 2 stanno su entrambe le facce: una copia ciascuno
        let split = s.split.as_ref().expect("spigolo vivo");
        assert_eq!(split.extra, [1, 2]);
        assert_eq!(s.render_vertex_count(), 8);
        assert_eq!(s.render_position(6), Some([1.0, 0.0, 0.0]));
        assert_eq!(s.render_position(7), Some([1.0, 1.0, 0.0]));
        assert_eq!(s.render_position(8), None);
    }

    /// Normali diverse per indice ma uguali come vettore: nessuna copia.
    #[test]
    fn same_normals_need_no_copies() {
        let s = folded(
            "normal Normal { vector [ 0 0 1, 0 0 1, 0 0 1, 0 0 1, 0 0 1, 0 0 1, 0 0 1, 0 0 1 ] }
             normalIndex [ 0 1 2 3 -1, 4 5 6 7 -1 ]",
        );
        assert!(s.split.is_none());
        assert_eq!(s.normals.len(), 6);
        assert!(s.corner_normals().iter().all(|&n| n == UP));
    }

    /// Una normale per faccia, con e senza `normalIndex`. I poligoni incompleti contano
    /// comunque come facce.
    #[test]
    fn normals_per_face() {
        let with_index = folded(
            "normal Normal { vector [ 1 0 0, 0 0 1 ] } normalPerVertex FALSE normalIndex [ 1 0 ]",
        );
        let without = folded("normal Normal { vector [ 0 0 1, 1 0 0 ] } normalPerVertex FALSE");
        for s in [with_index, without] {
            let corners = s.corner_normals();
            assert_eq!(&corners[..6], [UP; 6]);
            assert_eq!(&corners[6..], [SIDE; 6]);
        }

        let wrl = br#"#VRML V2.0 utf8
Shape { geometry IndexedFaceSet {
    coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] }
    coordIndex [ 0 1 -1, 0 1 2 -1 ]
    normal Normal { vector [ 1 0 0, 0 0 1 ] }
    normalPerVertex FALSE
} }"#;
        let s = &parse_wrl(wrl).shapes_data[0];
        assert_eq!(s.corner_normals(), [UP; 3], "la faccia di due vertici è la numero 0");
    }

    /// Normali non utilizzabili: si ricalcolano dalla geometria, senza copie.
    #[test]
    fn invalid_normal_index_falls_back_to_computed_normals() {
        for fields in [
            "normal Normal { vector [ 0 0 1 ] } normalIndex [ 0 0 0 0 -1, 0 0 5 0 -1 ]",
            "normal Normal { vector [ 0 0 1 ] } normalIndex [ 0 0 -1 ]",
            "normal Normal { vector [ 0 0 1 ] }",
        ] {
            let s = folded(fields);
            assert!(s.split.is_none(), "{fields}");
            assert_eq!(s.normals.len(), 6, "{fields}");
        }
        // `normal DEF Nome Normal` si legge come `normal Normal`
        let s = folded("normal DEF N Normal { vector [ 0 0 1, 1 0 0 ] } normalIndex [ 0 0 0 0 -1, 1 1 1 1 -1 ]");
        assert_eq!(s.corner_normals()[6], SIDE);
    }

    /// Le istanze `USE` ruotano anche le normali delle copie.
    #[test]
    fn use_rotates_split_normals() {
        let wrl = br#"#VRML V2.0 utf8
DEF Piega Shape { geometry IndexedFaceSet {
    coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0, 1 0 -1, 1 1 -1 ] }
    coordIndex [ 0 1 2 3 -1, 1 4 5 2 -1 ]
    normal Normal { vector [ 0 0 1, 1 0 0 ] }
    normalIndex [ 0 0 0 0 -1, 1 1 1 1 -1 ]
} }
Transform { rotation 0 0 1 1.5707964 children [ USE Piega ] }"#;
        let scene = parse_wrl(wrl);
        let copy = &scene.shapes_data[1];
        assert_eq!(copy.split, scene.shapes_data[0].split);
        let side = copy.corner_normals()[6];
        assert!(side[0].abs() < 1e-5 && (side[1] - 1.0).abs() < 1e-5, "{side:?}");
        assert_eq!(copy.corner_normals()[0], UP);
    }
}
