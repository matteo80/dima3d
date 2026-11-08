use std::ops::Range;

/// Dimensione massima di un buffer condiviso (limite predefinito di wgpu per i dispositivi).
const MAX_CHUNK_BYTES: u64 = 256 * 1024 * 1024;

/// Vertice GPU con posizione, normale e indice della mesh (allineato a 32 byte).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Indice della mesh: seleziona il colore nello storage buffer
    pub mesh_id: u32,
    pub _pad: u32,
}

impl Vertex {
    pub const DESC: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            // Position
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            // Normal
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            // Indice della mesh
            wgpu::VertexAttribute {
                offset: 2 * std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                shader_location: 2,
                format: wgpu::VertexFormat::Uint32,
            },
        ],
    };
}

/// Dove si trova una mesh dentro i buffer condivisi. `index_count` = 0: niente da disegnare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshSlot {
    pub chunk: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub first_index: u32,
    pub index_count: u32,
}

/// Dimensioni di un blocco di buffer condivisi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChunkSize {
    pub vertices: u32,
    pub indices: u32,
}

/// Una chiamata di disegno: un intervallo continuo di indici di un blocco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawRun {
    pub chunk: u32,
    pub indices: Range<u32>,
}

/// Un blocco di geometria sulla GPU.
pub struct GeometryChunk {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
}

/// Distribuisce le mesh, date come (vertici, indici), in blocchi da al massimo `max_bytes`.
/// Una mesh che da sola supera il limite non può stare in nessun buffer: resta non disegnata.
pub fn plan_slots(sizes: &[(usize, usize)], max_bytes: u64) -> (Vec<MeshSlot>, Vec<ChunkSize>) {
    let vertex_bytes = std::mem::size_of::<Vertex>() as u64;
    let index_bytes = std::mem::size_of::<u32>() as u64;
    let mut slots = Vec::with_capacity(sizes.len());
    let mut chunks: Vec<ChunkSize> = Vec::new();

    for (mesh, &(vertices, indices)) in sizes.iter().enumerate() {
        let (v, i) = (vertices as u64, indices as u64);
        if v == 0 || i == 0 {
            slots.push(MeshSlot::default());
            continue;
        }
        if v * vertex_bytes > max_bytes || i * index_bytes > max_bytes {
            tracing::warn!("Mesh {mesh} troppo grande per la GPU ({v} vertici, {i} indici): non disegnata");
            slots.push(MeshSlot::default());
            continue;
        }
        let fits = chunks.last().is_some_and(|c| {
            (c.vertices as u64 + v) * vertex_bytes <= max_bytes
                && (c.indices as u64 + i) * index_bytes <= max_bytes
        });
        if !fits {
            chunks.push(ChunkSize::default());
        }
        let chunk = chunks.len() - 1;
        let c = &mut chunks[chunk];
        slots.push(MeshSlot {
            chunk: chunk as u32,
            first_vertex: c.vertices,
            vertex_count: vertices as u32,
            first_index: c.indices,
            index_count: indices as u32,
        });
        c.vertices += vertices as u32;
        c.indices += indices as u32;
    }
    (slots, chunks)
}

/// Scrive una mesh nelle sue porzioni di buffer condiviso. Gli indici diventano assoluti nel blocco.
/// Un triangolo con indici fuori dalla mesh diventa degenere: altrimenti userebbe i vertici di
/// un'altra mesh dello stesso blocco.
pub fn write_mesh(
    shape: &crate::parser::ShapeInfo,
    mesh_id: u32,
    first_vertex: u32,
    vertices: &mut [Vertex],
    indices: &mut [u32],
) {
    let positions = &shape.world_vertices;
    let count = shape.render_vertex_count();
    let computed;
    let normals = if shape.normals.len() == count {
        &shape.normals
    } else {
        // Se le normali non corrispondono, calcolale al volo (senza copie degli spigoli)
        computed = compute_normals(positions, &shape.indices);
        &computed
    };
    let render_indices = if normals.len() == count { shape.render_indices() } else { &shape.indices };
    for (r, (out, &normal)) in vertices.iter_mut().zip(normals).enumerate() {
        let position = shape.render_position(r).unwrap_or_default();
        *out = Vertex { position, normal, mesh_id, _pad: 0 };
    }

    let count = normals.len() as u32;
    for (out, tri) in indices.chunks_exact_mut(3).zip(render_indices.chunks_exact(3)) {
        if tri.iter().all(|&i| i < count) {
            for (o, &i) in out.iter_mut().zip(tri) {
                *o = first_vertex + i;
            }
        } else {
            out.fill(first_vertex);
        }
    }
    // Eventuali indici oltre l'ultimo triangolo completo restano degeneri
    let complete = shape.indices.len() / 3 * 3;
    indices[complete..].fill(first_vertex);
}

/// (vertici, indici) di ogni mesh sulla GPU, comprese le copie dei vertici sugli spigoli vivi.
pub fn render_sizes(shapes: &[crate::parser::ShapeInfo]) -> Vec<(usize, usize)> {
    shapes.iter().map(|s| (s.render_vertex_count(), s.indices.len())).collect()
}

/// Crea i buffer GPU condivisi per le mesh date, scrivendoli direttamente nella memoria mappata.
/// `first_mesh` è il mesh_id della prima (le mesh di un file aggiunto seguono quelle già caricate).
pub fn build_chunks(
    shapes: &[crate::parser::ShapeInfo],
    first_mesh: u32,
    device: &wgpu::Device,
) -> (Vec<GeometryChunk>, Vec<MeshSlot>) {
    use rayon::prelude::*;

    let sizes = render_sizes(shapes);
    let max_bytes = device.limits().max_buffer_size.min(MAX_CHUNK_BYTES);
    let (slots, sizes) = plan_slots(&sizes, max_bytes);

    let mut chunks = Vec::with_capacity(sizes.len());
    for (c, size) in sizes.iter().enumerate() {
        let create = |label: &str, bytes: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: true,
            })
        };
        let vertex_buffer = create(
            "mesh_vertices",
            size.vertices as u64 * std::mem::size_of::<Vertex>() as u64,
            // COPY_DST: i vertici dei pezzi spostati si riscrivono in posto
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let index_buffer = create(
            "mesh_indices",
            size.indices as u64 * std::mem::size_of::<u32>() as u64,
            wgpu::BufferUsages::INDEX,
        );
        {
            let mut vertex_view = vertex_buffer.slice(..).get_mapped_range_mut();
            let mut index_view = index_buffer.slice(..).get_mapped_range_mut();
            let mut free_vertices: &mut [Vertex] = bytemuck::cast_slice_mut(&mut vertex_view);
            let mut free_indices: &mut [u32] = bytemuck::cast_slice_mut(&mut index_view);

            // Le mesh di un blocco sono in ordine: a ognuna tocca una porzione separata
            let mut jobs = Vec::new();
            for (mesh, slot) in slots.iter().enumerate() {
                if slot.index_count == 0 || slot.chunk as usize != c {
                    continue;
                }
                let (v, rest_v) =
                    std::mem::take(&mut free_vertices).split_at_mut(slot.vertex_count as usize);
                let (i, rest_i) =
                    std::mem::take(&mut free_indices).split_at_mut(slot.index_count as usize);
                free_vertices = rest_v;
                free_indices = rest_i;
                jobs.push((mesh, slot.first_vertex, v, i));
            }
            jobs.into_par_iter().for_each(|(mesh, first_vertex, v, i)| {
                write_mesh(&shapes[mesh], first_mesh + mesh as u32, first_vertex, v, i);
            });
        }
        vertex_buffer.unmap();
        index_buffer.unmap();
        chunks.push(GeometryChunk { vertex_buffer, index_buffer });
    }
    (chunks, slots)
}

/// Chiamate di disegno per le mesh opache visibili: mesh adiacenti nello stesso blocco
/// diventano una sola chiamata, quindi il numero dipende da quante parti sono nascoste.
pub fn opaque_runs(slots: &[MeshSlot], visible: &[bool], transparent: &[bool]) -> Vec<DrawRun> {
    let mut runs: Vec<DrawRun> = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let drawn = slot.index_count > 0
            && visible.get(i).copied().unwrap_or(false)
            && !transparent.get(i).copied().unwrap_or(false);
        if !drawn {
            continue;
        }
        let range = slot.first_index..slot.first_index + slot.index_count;
        match runs.last_mut() {
            Some(run) if run.chunk == slot.chunk && run.indices.end == range.start => {
                run.indices.end = range.end;
            }
            _ => runs.push(DrawRun { chunk: slot.chunk, indices: range }),
        }
    }
    runs
}

/// Calcola le normali per smooth shading data la lista di vertici e indici triangolati.
/// Per ogni triangolo calcola la normale di faccia, poi la accumula sui vertici.
pub fn compute_normals(vertices: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let n = vertices.len();
    if n == 0 {
        return Vec::new();
    }

    let mut normals = vec![glam::Vec3::ZERO; n];

    // Accumula le normali di faccia
    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if i0 >= n || i1 >= n || i2 >= n {
            continue;
        }
        let v0 = glam::Vec3::from(vertices[i0]);
        let v1 = glam::Vec3::from(vertices[i1]);
        let v2 = glam::Vec3::from(vertices[i2]);

        let edge1 = v1 - v0;
        let edge2 = v2 - v0;
        let face_normal = edge1.cross(edge2);

        normals[i0] += face_normal;
        normals[i1] += face_normal;
        normals[i2] += face_normal;
    }

    // Normalizza. Nessuna soglia assoluta: la lunghezza dipende dall'unità di misura, e in un
    // file in metri i triangoli piccoli darebbero la normale di ripiego. Dividere prima per la
    // componente maggiore evita che il quadrato della lunghezza vada a zero.
    normals
        .into_iter()
        .map(|n| {
            let largest = n.abs().max_element();
            if largest > 0.0 && largest.is_finite() {
                (n / largest).normalize().to_array()
            } else {
                [0.0, 1.0, 0.0] // triangoli degeneri: normale verso l'alto
            }
        })
        .collect()
}

/// Mesh con geometria che nessun buffer può contenere e che quindi non vengono disegnate.
pub fn undrawn_meshes(sizes: &[(usize, usize)], slots: &[MeshSlot]) -> usize {
    sizes
        .iter()
        .zip(slots)
        .filter(|(&(vertices, indices), slot)| vertices > 0 && indices > 0 && slot.index_count == 0)
        .count()
}

#[cfg(test)]
mod tests {
    /// Regressione: in un file in metri i triangoli di un decimo di millimetro prendevano
    /// la normale di ripiego (verso l'alto) per colpa di una soglia assoluta.
    #[test]
    fn normals_do_not_depend_on_the_unit_of_measure() {
        let vertices = [[0.0, 0.0, 0.0], [1e-4, 0.0, 0.0], [0.0, 0.0, -1e-4]];
        let normals = super::compute_normals(&vertices, &[0, 1, 2]);
        assert_eq!(normals[0], [0.0, 1.0, 0.0]);
        let vertices = [[0.0, 0.0, 0.0], [1e-4, 0.0, 0.0], [0.0, 1e-4, 0.0]];
        let normals = super::compute_normals(&vertices, &[0, 1, 2]);
        assert_eq!(normals[0], [0.0, 0.0, 1.0]);
        // Triangolo degenere: resta la normale di ripiego, senza NaN
        let vertices = [[1.0, 1.0, 1.0]; 3];
        assert_eq!(super::compute_normals(&vertices, &[0, 1, 2])[0], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn meshes_too_large_for_a_buffer_are_counted() {
        let sizes = [(3, 3), (100, 3), (0, 0), (3, 3)];
        let (slots, _) = super::plan_slots(&sizes, 320);
        assert_eq!(super::undrawn_meshes(&sizes, &slots), 1);
    }

    use super::*;

    #[test]
    fn plan_keeps_order_and_opens_new_chunks_at_the_limit() {
        // Limite: 10 vertici (320 byte) → 80 indici. Quarta mesh oltre il limite, quinta vuota.
        let sizes = [(4, 6), (6, 12), (3, 3), (11, 3), (0, 0), (1, 3)];
        let (slots, chunks) = plan_slots(&sizes, 320);
        assert_eq!(chunks, vec![
            ChunkSize { vertices: 10, indices: 18 },
            ChunkSize { vertices: 4, indices: 6 },
        ]);
        assert_eq!(slots[0], MeshSlot { chunk: 0, first_vertex: 0, vertex_count: 4, first_index: 0, index_count: 6 });
        assert_eq!(slots[1], MeshSlot { chunk: 0, first_vertex: 4, vertex_count: 6, first_index: 6, index_count: 12 });
        assert_eq!(slots[2], MeshSlot { chunk: 1, first_vertex: 0, vertex_count: 3, first_index: 0, index_count: 3 });
        assert_eq!(slots[3].index_count, 0);
        assert_eq!(slots[4].index_count, 0);
        assert_eq!(slots[5], MeshSlot { chunk: 1, first_vertex: 3, vertex_count: 1, first_index: 3, index_count: 3 });
    }

    #[test]
    fn plan_also_respects_the_index_limit() {
        let (slots, chunks) = plan_slots(&[(1, 60), (1, 60)], 320);
        assert_eq!(chunks.len(), 2);
        assert_eq!(slots[1].chunk, 1);
    }

    #[test]
    fn runs_merge_adjacent_meshes_only() {
        let sizes = [(3, 3), (3, 3), (3, 3), (3, 3), (3, 3)];
        let (slots, _) = plan_slots(&sizes, 1 << 20);
        let t = [false; 5];
        // Tutto visibile: una sola chiamata
        assert_eq!(opaque_runs(&slots, &[true; 5], &t), vec![DrawRun { chunk: 0, indices: 0..15 }]);
        // Una mesh nascosta spezza la chiamata in due
        assert_eq!(opaque_runs(&slots, &[true, true, false, true, true], &t), vec![
            DrawRun { chunk: 0, indices: 0..6 },
            DrawRun { chunk: 0, indices: 9..15 },
        ]);
        // Le trasparenti restano fuori
        assert_eq!(opaque_runs(&slots, &[true; 5], &[false, true, false, false, false]), vec![
            DrawRun { chunk: 0, indices: 0..3 },
            DrawRun { chunk: 0, indices: 6..15 },
        ]);
        assert!(opaque_runs(&slots, &[false; 5], &t).is_empty());
    }

    #[test]
    fn runs_do_not_cross_chunks() {
        // Limite di 3 vertici a blocco: ogni mesh sta in un blocco suo
        let (slots, chunks) = plan_slots(&[(3, 3), (3, 3)], 96);
        assert_eq!(chunks.len(), 2);
        assert_eq!(opaque_runs(&slots, &[true, true], &[false, false]).len(), 2);
    }

    #[test]
    fn write_mesh_makes_indices_absolute_and_contains_bad_ones() {
        let scene = crate::parser::parse_wrl(
            b"#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 0 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }\n",
        );
        let mut shape = scene.shapes_data[0].clone();
        shape.indices = vec![0, 1, 2, 2, 1, 99];
        let mut v = vec![Vertex { position: [9.0; 3], normal: [9.0; 3], mesh_id: 0, _pad: 0 }; 3];
        let mut i = vec![7; 6];
        write_mesh(&shape, 5, 100, &mut v, &mut i);
        assert_eq!(i, vec![100, 101, 102, 100, 100, 100]);
        assert!(v.iter().all(|x| x.mesh_id == 5));
        assert_eq!(v[1].position, [1.0, 0.0, 0.0]);

        // Normali mancanti: vengono calcolate
        shape.normals.clear();
        write_mesh(&shape, 5, 100, &mut v, &mut i);
        assert!((v[0].normal[2].abs() - 1.0).abs() < 1e-4);
    }

    /// Spigolo vivo: le copie dei vertici prendono la posizione dell'originale e la loro normale.
    #[test]
    fn write_mesh_adds_the_copies_of_sharp_edges() {
        let scene = crate::parser::parse_wrl(
            b"#VRML V2.0 utf8\nShape { geometry IndexedFaceSet {
                coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 1 0 -1 ] }
                coordIndex [ 0 1 2 -1, 1 3 2 -1 ]
                normal Normal { vector [ 0 0 1, 1 0 0 ] }
                normalIndex [ 0 0 0 -1, 1 1 1 -1 ] } }\n",
        );
        let shape = &scene.shapes_data[0];
        let sizes = render_sizes(&scene.shapes_data);
        assert_eq!(sizes, [(6, 6)], "4 vertici più le copie di 1 e 2");
        let mut v = vec![Vertex { position: [9.0; 3], normal: [9.0; 3], mesh_id: 0, _pad: 0 }; 6];
        let mut i = vec![0; 6];
        write_mesh(shape, 0, 10, &mut v, &mut i);
        assert_eq!(i, [10, 11, 12, 14, 13, 15]);
        assert_eq!((v[4].position, v[4].normal), ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        assert_eq!((v[5].position, v[5].normal), ([1.0, 1.0, 0.0], [1.0, 0.0, 0.0]));
        assert_eq!(v[1].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_compute_normals_flat_triangle() {
        let vertices = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let indices = vec![0, 1, 2];
        let normals = compute_normals(&vertices, &indices);
        assert_eq!(normals.len(), 3);
        // Il triangolo è nel piano XY con winding antiorario, la normale deve essere verso +Z [0, 0, 1]
        for n in normals {
            assert!((n[0] - 0.0).abs() < 1e-4);
            assert!((n[1] - 0.0).abs() < 1e-4);
            assert!((n[2] - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn test_triangulation_quad_fan() {
        let raw = vec![0, 1, 2, 3, -1];
        let (triangles, count) = crate::parser::shape_extractor::triangulate_coord_index(&raw, 4);
        assert_eq!(count, 1);
        // Quad diviso in due triangoli (fan): (0, 1, 2) e (0, 2, 3)
        assert_eq!(triangles, vec![0, 1, 2, 0, 2, 3]);
    }
}
