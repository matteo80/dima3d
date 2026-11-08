pub mod camera;
pub mod mesh_buffer;

use wgpu;
use wgpu::util::DeviceExt;

/// Layout uniforme per la camera 3D (allineato a 16 byte per WGSL).
#[repr(C)]
#[derive(Copy, Clone, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    /// Direzione verso la luce principale (mondo): in alto a destra, davanti alla camera
    pub key_light: [f32; 4],
    /// Direzione verso la luce di schiarita (mondo): a sinistra, poco sotto
    pub fill_light: [f32; 4],
    /// "Su" dello schermo (mondo), per l'ambiente a emisfero
    pub view_up: [f32; 4],
    /// Piano di sezione (vedi `Section::plane`); (0, 0, 0, 1) = nessun taglio
    pub clip_plane: [f32; 4],
    /// Sfondo sfumato, colori lineari in alto e in basso (zero se lo sfondo è uniforme)
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
}

/// Colori (sRGB) dello sfondo sfumato attorno al colore scelto: verso il bianco in alto,
/// più scuro in basso.
pub fn gradient_colors(background: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    (background.map(|c| c + (1.0 - c) * 0.30), background.map(|c| c * 0.55))
}

/// Piano di sezione perpendicolare a un asse: si tiene la parte con coordinata ≤ `position`
/// (con `flip` quella ≥).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    /// 0 = X, 1 = Y, 2 = Z
    pub axis: usize,
    pub position: f32,
    pub flip: bool,
}

impl Section {
    /// Piano (n, d): un punto p è tagliato se n·p > d.
    pub fn plane(&self) -> [f32; 4] {
        let sign = if self.flip { -1.0 } else { 1.0 };
        let mut p = [0.0; 4];
        p[self.axis.min(2)] = sign;
        p[3] = sign * self.position;
        p
    }

    /// Vero se il punto resta visibile.
    pub fn keeps(&self, p: glam::Vec3) -> bool {
        let [x, y, z, d] = self.plane();
        glam::Vec3::new(x, y, z).dot(p) <= d
    }
}

impl CameraUniform {
    /// Uniform della camera con le luci solidali alla vista.
    pub fn from_camera(camera: &camera::ArcballCamera) -> Self {
        let world = |v: glam::Vec3| {
            let d = (camera.orientation * v.normalize()).normalize();
            [d.x, d.y, d.z, 0.0]
        };
        let eye = camera.eye();
        Self {
            view_proj: camera.view_proj().to_cols_array_2d(),
            camera_pos: [eye.x, eye.y, eye.z, 1.0],
            // Spazio camera: X destra, Y su, Z verso chi guarda
            key_light: world(glam::Vec3::new(0.25, 0.45, 0.85)),
            fill_light: world(glam::Vec3::new(-0.65, -0.15, 0.75)),
            view_up: world(glam::Vec3::Y),
            clip_plane: [0.0, 0.0, 0.0, 1.0],
            bg_top: [0.0; 4],
            bg_bottom: [0.0; 4],
        }
    }
}

/// Colore base dei componenti senza materiale (alluminio satinato), sRGB.
pub const DEFAULT_MESH_COLOR: [f32; 4] = [0.88, 0.90, 0.93, 1.0];
/// Colore di evidenziazione della selezione (verde, come FreeCAD), sRGB. Il contorno usa lo
/// stesso colore, scritto in valori lineari in `shaders/outline.wgsl`.
pub const COLOR_SELECTED: [f32; 4] = [0.20, 0.85, 0.25, 1.0];
/// Colore di sfondo predefinito della viewport, sRGB.
pub const DEFAULT_BACKGROUND: [f32; 3] = [0.38, 0.40, 0.44];
/// Campioni per pixel dell'antialiasing (MSAA). 4 è garantito da ogni scheda video
/// supportata da wgpu, sia per il colore sRGB sia per il depth Depth32Float.
pub const MSAA_SAMPLES: u32 = 4;
/// Formato della maschera della selezione.
const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// Quota del colore di selezione mescolata al colore del pezzo selezionato (il resto lo fa il contorno).
const SELECTION_TINT: f32 = 0.45;

/// Conversione sRGB → lineare (la texture di destinazione è sRGB).
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn rgba_to_linear(c: [f32; 4]) -> [f32; 4] {
    [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3]]
}

/// Renderer 3D nativo wgpu integrato con egui/eframe via offscreen render target.
pub struct WgpuRenderer {
    pub camera: camera::ArcballCamera,
    pub pipeline: wgpu::RenderPipeline,
    /// Pipeline per le mesh trasparenti (blending, senza scrittura depth)
    pipeline_transparent: wgpu::RenderPipeline,
    /// Sagoma delle mesh selezionate nella maschera (vedi `mask_view`)
    mask_pipeline: wgpu::RenderPipeline,
    /// Contorno della selezione: legge la maschera e colora il bordo esterno della sagoma
    outline_pipeline: wgpu::RenderPipeline,
    outline_layout: wgpu::BindGroupLayout,
    outline_bind_group: Option<wgpu::BindGroup>,
    /// Maschera della selezione (1 campione, un canale), grande come la viewport
    mask_view: Option<wgpu::TextureView>,
    /// Chiamate di disegno delle mesh selezionate e visibili
    selected_runs: Vec<mesh_buffer::DrawRun>,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    /// Buffer GPU condivisi tra le Shape
    chunks: Vec<mesh_buffer::GeometryChunk>,
    /// Posizione di ogni mesh nei buffer condivisi
    slots: Vec<mesh_buffer::MeshSlot>,
    /// Chiamate di disegno delle mesh opache (ricalcolate a ogni sincronizzazione)
    opaque_runs: Vec<mesh_buffer::DrawRun>,
    /// Flag di visibilità per ogni mesh (allineato a slots)
    pub visibility: Vec<bool>,
    /// Flag di selezione per ogni mesh (allineato a slots)
    pub selected: Vec<bool>,
    /// Colore originale (sRGB) di ogni mesh, dal Material del file
    original_colors: Vec<Option<[f32; 4]>>,
    /// Colore utente ereditato di ogni mesh (buffer riusato)
    user_colors: Vec<Option<[f32; 4]>>,
    /// Mesh con alpha < 1 (disegnate nel passaggio trasparente)
    transparent: Vec<bool>,
    /// Centro del bounding box di ogni mesh (ordinamento delle trasparenze)
    centers: Vec<glam::Vec3>,
    /// Colore per le mesh senza materiale (sRGB)
    pub default_color: [f32; 4],
    /// Colore di sfondo (sRGB)
    pub background: [f32; 3],
    /// Piano di sezione attivo
    pub section: Option<Section>,
    /// Sfondo sfumato attorno a `background`
    pub background_gradient: bool,
    background_pipeline: wgpu::RenderPipeline,
    /// Storage buffer (gruppo 1) con il colore lineare di ogni mesh
    color_layout: wgpu::BindGroupLayout,
    color_buffer: wgpu::Buffer,
    color_bind_group: wgpu::BindGroup,
    /// (revisione scena, nodo selezionato, colore default) dell'ultima sincronizzazione
    synced_state: Option<(u64, Vec<u32>, [u32; 4])>,
    /// Colori lineari da caricare sulla GPU al prossimo frame
    pending_colors: Option<Vec<[f32; 4]>>,
    /// Il contenuto della scena è cambiato: il prossimo frame va ridisegnato
    frame_dirty: bool,
    /// Camera e sfondo dell'ultimo disegno: se non cambiano, l'immagine resta valida
    last_drawn: Option<(CameraUniform, [f32; 3])>,
    /// Texture offscreen a colori per il rendering della viewport (immagine finale, 1 campione)
    color_texture: Option<wgpu::Texture>,
    color_view: Option<wgpu::TextureView>,
    /// Texture multicampione in cui si disegna; a fine passaggio viene risolta in `color_view`
    msaa_view: Option<wgpu::TextureView>,
    /// Texture depth/stencil per Z-buffer hardware
    _depth_texture: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,
    /// ID della texture registrata in egui
    pub egui_texture_id: Option<egui::TextureId>,
    /// Dimensioni correnti del viewport (larghezza, altezza) in pixel fisici
    pub texture_size: (u32, u32),
}

impl WgpuRenderer {
    /// Crea il renderer compilando gli shader WGSL e configurando la pipeline wgpu.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        // Carica lo shader WGSL con Blinn-Phong shading e two-sided lighting
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mesh.wgsl").into()),
        });

        // Buffer per CameraUniform
        let initial_camera = CameraUniform::from_camera(&camera::ArcballCamera::new());

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_uniform_buffer"),
            contents: bytemuck::bytes_of(&initial_camera),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("camera_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // Gruppo 1: colori per-mesh (storage buffer indicizzato da instance_index)
        let color_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh_colors_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let (color_buffer, color_bind_group) = Self::make_color_buffer(device, &color_layout, 1);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh_pipeline_layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &color_layout],
            push_constant_ranges: &[],
        });

        let make_pipeline = |label: &str, transparent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[mesh_buffer::Vertex::DESC],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: if transparent { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None },
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // Disabilitato per mostrare facce con qualsiasi orientamento
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: !transparent,
                depth_compare: wgpu::CompareFunction::Greater, // profondità invertita (vedi camera)
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: MSAA_SAMPLES, ..Default::default() },
            multiview: None,
            cache: None,
            })
        };
        let pipeline = make_pipeline("mesh_render_pipeline", false);
        let pipeline_transparent = make_pipeline("mesh_render_pipeline_transparent", true);

        // Maschera della selezione: solo posizioni, nessun depth, un campione
        let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("selection_mask_layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });
        let mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("selection_mask_pipeline"),
            layout: Some(&mask_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mask"),
                buffers: &[mesh_buffer::Vertex::DESC],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_mask"),
                targets: &[Some(MASK_FORMAT.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Sfondo sfumato: triangolo a tutto schermo disegnato per primo, senza depth
        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background_pipeline"),
            layout: Some(&mask_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_background"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_background"),
                targets: &[Some(target_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: MSAA_SAMPLES, ..Default::default() },
            multiview: None,
            cache: None,
        });

        // Contorno: triangolo a tutto schermo nel passaggio principale, sopra a tutto
        let outline_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("outline_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/outline.wgsl").into()),
        });
        let outline_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let outline_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline_pipeline_layout"),
            bind_group_layouts: &[&outline_layout],
            push_constant_ranges: &[],
        });
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline_pipeline"),
            layout: Some(&outline_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &outline_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &outline_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: MSAA_SAMPLES, ..Default::default() },
            multiview: None,
            cache: None,
        });

        Self {
            camera: camera::ArcballCamera::new(),
            pipeline,
            pipeline_transparent,
            mask_pipeline,
            outline_pipeline,
            outline_layout,
            outline_bind_group: None,
            mask_view: None,
            selected_runs: Vec::new(),
            camera_buffer,
            camera_bind_group,
            chunks: Vec::new(),
            slots: Vec::new(),
            opaque_runs: Vec::new(),
            visibility: Vec::new(),
            selected: Vec::new(),
            original_colors: Vec::new(),
            user_colors: Vec::new(),
            transparent: Vec::new(),
            centers: Vec::new(),
            default_color: DEFAULT_MESH_COLOR,
            background: DEFAULT_BACKGROUND,
            section: None,
            background_gradient: false,
            background_pipeline,
            color_layout,
            color_buffer,
            color_bind_group,
            synced_state: None,
            pending_colors: None,
            frame_dirty: true,
            last_drawn: None,
            color_texture: None,
            color_view: None,
            msaa_view: None,
            _depth_texture: None,
            depth_view: None,
            egui_texture_id: None,
            texture_size: (0, 0),
        }
    }

    /// Crea lo storage buffer dei colori per `count` mesh e il relativo bind group.
    fn make_color_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        count: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh_colors"),
            size: (count.max(1) * std::mem::size_of::<[f32; 4]>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh_colors_bind_group"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        (buffer, bind_group)
    }

    /// Libera tutte le mesh (chiusura del progetto).
    pub fn clear_meshes(&mut self) {
        self.chunks.clear();
        self.slots.clear();
        self.opaque_runs.clear();
        self.selected_runs.clear();
        self.frame_dirty = true;
        self.visibility.clear();
        self.selected.clear();
        self.original_colors.clear();
        self.user_colors.clear();
        self.transparent.clear();
        self.centers.clear();
        self.pending_colors = None;
        self.synced_state = None;
        self.camera = camera::ArcballCamera::new();
    }

    /// Carica le mesh dal SceneGraph sulla GPU.
    /// Ritorna quante mesh non vengono disegnate perché troppo grandi per un buffer.
    pub fn upload_meshes(
        &mut self,
        shapes: &[crate::parser::ShapeInfo],
        device: &wgpu::Device,
    ) -> usize {
        self.chunks.clear();
        self.slots.clear();
        self.visibility.clear();
        self.selected.clear();
        self.original_colors.clear();
        self.transparent.clear();
        self.centers.clear();
        self.append_meshes(shapes, device)
    }

    /// Aggiunge sulla GPU le mesh oltre quelle già caricate (`shapes` è l'elenco completo della
    /// scena): i buffer esistenti non vengono riscritti. Ritorna quante delle nuove mesh non
    /// vengono disegnate perché troppo grandi per un buffer.
    pub fn append_meshes(
        &mut self,
        shapes: &[crate::parser::ShapeInfo],
        device: &wgpu::Device,
    ) -> usize {
        self.opaque_runs.clear();
        self.selected_runs.clear();
        self.synced_state = None;
        self.frame_dirty = true;

        let first = self.slots.len().min(shapes.len());
        let new_shapes = &shapes[first..];
        let (chunks, mut slots) = mesh_buffer::build_chunks(new_shapes, first as u32, device);
        let undrawn = mesh_buffer::undrawn_meshes(&mesh_buffer::render_sizes(new_shapes), &slots);
        let chunk_shift = self.chunks.len() as u32;
        for s in &mut slots {
            s.chunk += chunk_shift;
        }
        self.chunks.extend(chunks);
        self.slots.extend(slots);

        for shape in new_shapes {
            self.visibility.push(true);
            self.selected.push(false);
            self.original_colors.push(shape.color);
            self.transparent.push(false);
            let mut bb = crate::scene_graph::Aabb::empty();
            for &v in &shape.world_vertices {
                bb.extend_point(glam::Vec3::from(v));
            }
            self.centers.push(if bb.is_valid() { bb.center() } else { glam::Vec3::ZERO });
        }

        let (buffer, bind_group) = Self::make_color_buffer(device, &self.color_layout, self.slots.len());
        self.color_buffer = buffer;
        self.color_bind_group = bind_group;

        tracing::info!(
            "Caricate {} mesh su GPU wgpu ({} nuove) in {} blocchi (vertici nuovi: {})",
            self.slots.len(),
            new_shapes.len(),
            self.chunks.len(),
            new_shapes.iter().map(|s| s.render_vertex_count()).sum::<usize>()
        );
        undrawn
    }

    /// Allinea i flag di visibilità/selezione delle mesh GPU allo stato del SceneGraph.
    /// Lavoro O(nodi) eseguito solo quando cambia la revisione della scena o la selezione.
    pub fn sync_with_scene(&mut self, scene: &crate::scene_graph::SceneGraph, selection: &[u32]) {
        let color_bits = self.default_color.map(f32::to_bits);
        let unchanged = self.synced_state.as_ref().is_some_and(|(revision, synced, bits)| {
            *revision == scene.revision && synced == selection && *bits == color_bits
        });
        if unchanged && self.visibility.len() == self.slots.len() {
            return;
        }
        let state = (scene.revision, selection.to_vec(), color_bits);
        scene.mesh_visibility_into(&mut self.visibility);
        self.visibility.resize(self.slots.len(), false);

        self.selected.clear();
        self.selected.resize(self.slots.len(), false);
        for &sel in selection {
            let selected = &mut self.selected;
            scene.for_each_subtree_mesh(sel, |m| {
                if let Some(flag) = selected.get_mut(m as usize) {
                    *flag = true;
                }
            });
        }

        // Colori: selezione > colore utente ereditato > Material del file > default
        scene.mesh_user_colors_into(&mut self.user_colors);
        let n = self.slots.len();
        let mut linear = Vec::with_capacity(n);
        self.transparent.resize(n, false);
        for i in 0..n {
            let rgba = self
                .user_colors
                .get(i)
                .copied()
                .flatten()
                .or_else(|| self.original_colors.get(i).copied().flatten())
                .unwrap_or(self.default_color);
            self.transparent[i] = rgba[3] < 0.999;
            // Selezione: colore del pezzo tinto di verde (resta riconoscibile), più il contorno
            let rgba = if self.selected[i] {
                let mix = |a: f32, b: f32| a + (b - a) * SELECTION_TINT;
                [
                    mix(rgba[0], COLOR_SELECTED[0]),
                    mix(rgba[1], COLOR_SELECTED[1]),
                    mix(rgba[2], COLOR_SELECTED[2]),
                    rgba[3].max(0.6),
                ]
            } else {
                rgba
            };
            linear.push(rgba_to_linear(rgba));
        }
        self.pending_colors = Some(linear);
        self.opaque_runs = mesh_buffer::opaque_runs(&self.slots, &self.visibility, &self.transparent);
        let shown_selected: Vec<bool> =
            (0..n).map(|i| self.selected[i] && self.visibility.get(i).copied().unwrap_or(false)).collect();
        self.selected_runs = mesh_buffer::opaque_runs(&self.slots, &shown_selected, &[]);
        self.frame_dirty = true;
        self.synced_state = Some(state);
    }

    /// Esegue il rendering 3D sulla texture offscreen e restituisce l'ID texture egui per la visualizzazione.
    pub fn render_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        egui_renderer: &mut eframe::egui_wgpu::Renderer,
        width: u32,
        height: u32,
    ) -> Option<egui::TextureId> {
        let width = width.max(1);
        let height = height.max(1);

        // Se le dimensioni sono cambiate o la texture non esiste, rialloca
        if self.texture_size != (width, height) || self.color_texture.is_none() {
            let color_desc = wgpu::TextureDescriptor {
                label: Some("viewport_color_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            };

            let color_texture = device.create_texture(&color_desc);
            let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Antialiasing: si disegna su MSAA_SAMPLES campioni per pixel, poi la scheda video
            // ne fa la media nella texture finale. I campioni non servono dopo la media.
            let msaa_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport_msaa_texture"),
                sample_count: MSAA_SAMPLES,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                ..color_desc
            });
            let msaa_view = msaa_texture.create_view(&wgpu::TextureViewDescriptor::default());

            let depth_desc = wgpu::TextureDescriptor {
                label: Some("viewport_depth_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: MSAA_SAMPLES,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            };

            let depth_texture = device.create_texture(&depth_desc);
            let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

            let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("selection_mask_texture"),
                format: MASK_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                ..color_desc
            });
            let mask_view = mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.outline_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("outline_bind_group"),
                layout: &self.outline_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&mask_view),
                }],
            }));
            self.mask_view = Some(mask_view);

            // Registra o aggiorna la texture nativa per egui
            let texture_id = if let Some(old_id) = self.egui_texture_id {
                egui_renderer.update_egui_texture_from_wgpu_texture(
                    device,
                    &color_view,
                    wgpu::FilterMode::Linear,
                    old_id,
                );
                old_id
            } else {
                egui_renderer.register_native_texture(
                    device,
                    &color_view,
                    wgpu::FilterMode::Linear,
                )
            };

            self.color_texture = Some(color_texture);
            self.color_view = Some(color_view);
            self.msaa_view = Some(msaa_view);
            self._depth_texture = Some(depth_texture);
            self.depth_view = Some(depth_view);
            self.egui_texture_id = Some(texture_id);
            self.texture_size = (width, height);
            self.frame_dirty = true;
        }

        // Aggiorna aspect ratio della telecamera
        self.camera.aspect = width as f32 / height as f32;

        // Scrivi l'uniform della camera
        let eye = self.camera.eye();
        let mut uniform = CameraUniform::from_camera(&self.camera);
        if let Some(s) = &self.section {
            uniform.clip_plane = s.plane();
        }
        if self.background_gradient {
            let (top, bottom) = gradient_colors(self.background);
            let linear = |c: [f32; 3]| [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), 1.0];
            uniform.bg_top = linear(top);
            uniform.bg_bottom = linear(bottom);
        }

        // Nulla è cambiato (es. mouse sui pannelli): l'immagine precedente è ancora valida
        let drawn = (uniform, self.background);
        if !self.frame_dirty && self.last_drawn == Some(drawn) {
            return self.egui_texture_id;
        }
        self.frame_dirty = false;
        self.last_drawn = Some(drawn);

        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));

        // Colori per-mesh aggiornati (solo dopo una modifica)
        if let Some(colors) = self.pending_colors.take() {
            if !colors.is_empty() {
                queue.write_buffer(&self.color_buffer, 0, bytemuck::cast_slice(&colors));
            }
        }

        // Avvia il RenderPass
        let Some(color_view) = &self.color_view else {
            return None;
        };
        let Some(depth_view) = &self.depth_view else {
            return None;
        };
        let Some(msaa_view) = &self.msaa_view else {
            return None;
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("viewport_render_encoder"),
        });

        // Sagoma delle mesh selezionate, prima del disegno principale che la usa per il contorno
        let outline = match (&self.mask_view, &self.outline_bind_group) {
            (Some(mask_view), Some(bind_group)) if !self.selected_runs.is_empty() => {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("selection_mask_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: mask_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.mask_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                for run in &self.selected_runs {
                    let c = &self.chunks[run.chunk as usize];
                    pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                    pass.set_index_buffer(c.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(run.indices.clone(), 0, 0..1);
                }
                Some(bind_group)
            }
            _ => None,
        };

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa_view,
                    resolve_target: Some(color_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: srgb_to_linear(self.background[0]) as f64,
                            g: srgb_to_linear(self.background[1]) as f64,
                            b: srgb_to_linear(self.background[2]) as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard, // conta solo la media in color_view
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if self.background_gradient {
                render_pass.set_pipeline(&self.background_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_bind_group(1, &self.color_bind_group, &[]);

            // I buffer si cambiano solo passando da un blocco all'altro
            let chunks = &self.chunks;
            let mut bound = None;
            let mut draw = |pass: &mut wgpu::RenderPass, chunk: u32, indices: std::ops::Range<u32>| {
                if bound != Some(chunk) {
                    let c = &chunks[chunk as usize];
                    pass.set_vertex_buffer(0, c.vertex_buffer.slice(..));
                    pass.set_index_buffer(c.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    bound = Some(chunk);
                }
                pass.draw_indexed(indices, 0, 0..1);
            };

            // Passaggio 1: mesh opache, raggruppate. Il colore arriva dal mesh_id dei vertici.
            for run in &self.opaque_runs {
                draw(&mut render_pass, run.chunk, run.indices.clone());
            }

            // Passaggio 2: mesh trasparenti, una a una dalla più lontana alla più vicina
            let mut transparent_list: Vec<(f32, usize)> = (0..self.slots.len())
                .filter(|&i| {
                    self.slots[i].index_count > 0
                        && self.visibility.get(i).copied().unwrap_or(false)
                        && self.transparent.get(i).copied().unwrap_or(false)
                })
                .map(|i| (self.centers.get(i).map_or(0.0, |c| c.distance_squared(eye)), i))
                .collect();
            if !transparent_list.is_empty() {
                transparent_list.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
                render_pass.set_pipeline(&self.pipeline_transparent);
                for (_, i) in transparent_list {
                    let slot = &self.slots[i];
                    draw(&mut render_pass, slot.chunk, slot.first_index..slot.first_index + slot.index_count);
                }
            }

            // Passaggio 3: contorno della selezione, sopra a tutto (visibile anche se coperta)
            if let Some(bind_group) = outline {
                render_pass.set_pipeline(&self.outline_pipeline);
                render_pass.set_bind_group(0, bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));

        self.egui_texture_id
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn section_keeps_one_side_and_flips() {
        let s = super::Section { axis: 1, position: 2.0, flip: false };
        assert_eq!(s.plane(), [0.0, 1.0, 0.0, 2.0]);
        assert!(s.keeps(glam::Vec3::new(9.0, 1.0, 9.0)) && !s.keeps(glam::Vec3::new(0.0, 3.0, 0.0)));
        let f = super::Section { flip: true, ..s };
        assert!(!f.keeps(glam::Vec3::new(9.0, 1.0, 9.0)) && f.keeps(glam::Vec3::new(0.0, 3.0, 0.0)));
        // Sul piano il punto resta
        assert!(s.keeps(glam::Vec3::new(0.0, 2.0, 0.0)) && f.keeps(glam::Vec3::new(0.0, 2.0, 0.0)));
    }

    /// Il colore del contorno scritto nello shader è COLOR_SELECTED convertito in lineare.
    #[test]
    fn outline_color_matches_selection_color() {
        let src = include_str!("shaders/outline.wgsl");
        let line = src.lines().find(|l| l.starts_with("const COLOR")).unwrap();
        let inside = &line[line.rfind('(').unwrap() + 1..line.rfind(')').unwrap()];
        let shader: Vec<f32> = inside.split(',').map(|x| x.trim().parse().unwrap()).collect();
        for (s, c) in shader.iter().zip(super::COLOR_SELECTED) {
            assert!((s - super::srgb_to_linear(c)).abs() < 1e-3, "{shader:?}");
        }
    }
}

/// I test che usano la scheda video non girano mai insieme: due istanze Vulkan vive in parallelo
/// chiudono il processo di test (accesso non valido nel driver). Ogni test che crea un
/// `wgpu::Instance` prende il lucchetto per primo, così viene rilasciato dopo il device.
#[cfg(test)]
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod bench {
    use super::*;

    /// Legge dalla GPU i pixel RGBA dell'ultimo frame.
    fn read_pixels(r: &WgpuRenderer, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let (w, h) = r.texture_size;
        let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            r.color_texture.as_ref().unwrap().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::Maintain::Wait);
        let data = buffer.slice(..).get_mapped_range();
        data.chunks(row as usize).flat_map(|l| l[..(w * 4) as usize].to_vec()).collect()
    }

    /// Salva o confronta l'immagine di un file reale, per verificare che il disegno non cambi:
    /// con `GESTORE_RENDER_DUMP=percorso` scrive l'immagine se non esiste, altrimenti la confronta.
    fn dump_or_compare(r: &WgpuRenderer, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Ok(path) = std::env::var("GESTORE_RENDER_DUMP") else { return };
        let pixels = read_pixels(r, device, queue);
        match std::fs::read(&path) {
            Err(_) => {
                std::fs::write(&path, &pixels).unwrap();
                println!("Immagine salvata: {path}");
            }
            Ok(old) => {
                let different = pixels.chunks(4).zip(old.chunks(4)).filter(|(a, b)| a != b).count();
                let worst = pixels.iter().zip(&old).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
                println!(
                    "Confronto: {different}/{} pixel diversi, differenza massima {worst}",
                    pixels.len() / 4
                );
            }
        }
    }

    /// Tempi di caricamento e disegno sulla scheda video vera, su un file reale:
    /// `GESTORE_WRL_FILE=percorso cargo test --release tempo_rendering -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tempo_rendering_file_reale() {
        let _gpu = gpu_lock();
        // Più file separati da "|" vengono uniti nella stessa scena, come con "Aggiungi"
        let paths = std::env::var("GESTORE_WRL_FILE").expect("impostare GESTORE_WRL_FILE");
        let mut scene = crate::scene_graph::SceneGraph::new();
        for (i, path) in paths.split('|').enumerate() {
            let mut one = crate::parser::parse_wrl(&std::fs::read(path).unwrap());
            one.into_file(path.to_string(), None);
            if i == 0 {
                scene = one;
            } else {
                scene.append(one);
                // GESTORE_CONVERTI: applica le conversioni di unità proposte, come "Converti tutti"
                let added = scene.sources.len() - 1;
                if let Some(f) = scene.suggest_unit_scale(added).filter(|_| std::env::var_os("GESTORE_CONVERTI").is_some()) {
                    println!("Convertito {path} × {f}");
                    scene.scale_file(added, f);
                }
            }
        }
        // GESTORE_NORMALI_RICALCOLATE: normali lisce dai vertici, ignorando quelle del file
        // (come prima della lettura di `normalIndex`), per confrontare le immagini
        if std::env::var_os("GESTORE_NORMALI_RICALCOLATE").is_some() {
            for s in &mut scene.shapes_data {
                s.split = None;
                s.normals = mesh_buffer::compute_normals(&s.world_vertices, &s.indices);
            }
        }

        // GESTORE_RIDUCI=n: riduce la scena a n triangoli prima del disegno
        if let Some(n) = std::env::var("GESTORE_RIDUCI").ok().and_then(|v| v.parse().ok()) {
            let refs: Vec<&crate::parser::ShapeInfo> = scene.shapes_data.iter().collect();
            let t = std::time::Instant::now();
            let params = crate::reduce::Params { target: crate::reduce::Target::Triangles(n), lock_border: false };
            let out = crate::reduce::reduce_many(&refs, params, &std::sync::atomic::AtomicU32::new(0));
            println!("Ridotto a {} triangoli, errore {}, in {:?}", out.triangles_after, out.max_error, t.elapsed());
            for (i, s) in out.shapes.into_iter().enumerate() {
                if let Some(s) = s {
                    scene.shapes_data[i] = s;
                }
            }
        }

        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("nessuna scheda video");
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor::default(), None),
        )
        .unwrap();
        println!("Scheda: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
        let mut r = WgpuRenderer::new(&device, format);

        let t = std::time::Instant::now();
        r.upload_meshes(&scene.shapes_data, &device);
        let _ = device.poll(wgpu::Maintain::Wait);
        println!("{} shape, caricamento su GPU: {:?}", scene.shapes_data.len(), t.elapsed());

        let bbox = scene.compute_bounding_box();
        println!("Ingombro: {:?} → {:?}, diagonale {}", bbox.min, bbox.max, bbox.diagonal());
        for (i, s) in scene.sources.iter().enumerate() {
            let b = scene.file_bounding_box(i);
            println!("  file {}: {:?} → {:?} (scala {})", s.name, b.min, b.max, s.scale);
        }
        for (axis, name) in ["X", "Y", "Z"].iter().enumerate() {
            let mut centers: Vec<f32> = scene
                .mesh_bboxes
                .iter()
                .filter(|b| b.is_valid())
                .map(|b| b.center()[axis])
                .collect();
            centers.sort_by(f32::total_cmp);
            let q = |f: f32| centers[((centers.len() - 1) as f32 * f) as usize];
            println!(
                "Centri dei pezzi {name}: min {} | 5% {} | mediana {} | 95% {} | max {}",
                q(0.0), q(0.05), q(0.5), q(0.95), q(1.0)
            );
        }
        let far: Vec<usize> = (0..scene.mesh_bboxes.len())
            .filter(|&m| scene.mesh_bboxes[m].is_valid() && scene.mesh_bboxes[m].center().length() > 100.0)
            .collect();
        let far_tris: usize = far.iter().map(|&m| scene.shapes_data[m].indices.len() / 3).sum();
        println!("Pezzi oltre 100 unità dall'origine: {} ({far_tris} triangoli)", far.len());
        for &m in far.iter().take(12) {
            let b = &scene.mesh_bboxes[m];
            let name = scene.mesh_to_node.get(m).map(|&n| scene.display_name(n)).unwrap_or_default();
            println!("  {name}: {:?} → {:?}", b.min, b.max);
        }
        let (main_bbox, far_parts) = scene.main_visible_bounding_box();
        println!(
            "Gruppo principale: {:?} → {:?}, pezzi lontani esclusi: {far_parts}",
            main_bbox.min, main_bbox.max
        );
        r.camera.fit_to_bbox(&main_bbox);
        r.camera.scene_bounds = Some(bbox);

        // GESTORE_WRL_ISOLA: disegna e inquadra solo il primo pezzo con questo testo nel percorso
        if let Ok(wanted) = std::env::var("GESTORE_WRL_ISOLA") {
            let mesh = scene
                .shapes_data
                .iter()
                .position(|s| s.path.iter().any(|p| p.name.as_deref().is_some_and(|n| n.contains(&wanted))))
                .expect("nessun pezzo con questo nome");
            scene.isolate(scene.mesh_to_node[mesh]);
            r.camera.fit_to_bbox(&scene.mesh_bboxes[mesh]);
        }
        // GESTORE_SFUMATO: sfondo sfumato
        r.background_gradient = std::env::var_os("GESTORE_SFUMATO").is_some();
        // GESTORE_TRASPARENTE: tutta la scena semitrasparente (caso peggiore delle trasparenze)
        if std::env::var_os("GESTORE_TRASPARENTE").is_some() {
            for &r in &scene.roots.clone() {
                scene.set_color(r, Some([0.8, 0.8, 0.85, 0.5]));
            }
        }
        // GESTORE_WRL_SELEZIONA: seleziona il primo pezzo con questo testo nel percorso
        let selection: Vec<u32> = std::env::var("GESTORE_WRL_SELEZIONA")
            .ok()
            .and_then(|wanted| {
                scene.shapes_data.iter().position(|s| {
                    s.path.iter().any(|p| p.name.as_deref().is_some_and(|n| n.contains(&wanted)))
                })
            })
            .map(|mesh| vec![scene.mesh_to_node[mesh]])
            .unwrap_or_default();
        r.sync_with_scene(&scene, &selection);

        // GESTORE_VISTA: nome di una vista standard (Fronte, Retro, Destra, ...), altrimenti isometrica
        let view = std::env::var("GESTORE_VISTA").ok().and_then(|v| {
            camera::StandardView::ALL.iter().find(|x| x.1.eq_ignore_ascii_case(&v)).map(|x| x.0)
        });
        r.camera.set_standard_view(view.unwrap_or(camera::StandardView::Iso), camera::Axes::Vehicle);
        // GESTORE_CAMERA="x,y,z,distanza[,orbita_x,orbita_y]": punto inquadrato, distanza e rotazione
        // (per ingrandire un dettaglio, es. i riflessi di un pannello)
        if let Ok(v) = std::env::var("GESTORE_CAMERA") {
            let v: Vec<f32> = v.split(',').map(|x| x.trim().parse().unwrap()).collect();
            r.camera.target = glam::Vec3::new(v[0], v[1], v[2]);
            r.camera.distance = v[3];
            if v.len() >= 6 {
                r.camera.orbit(v[4], v[5]);
            }
        }
        r.render_frame(&device, &queue, &mut egui_renderer, 1600, 900);
        dump_or_compare(&r, &device, &queue);

        // Effetto dei piani di taglio sulla precisione: stessa vista con la scena ristretta
        // al gruppo principale (piani molto più stretti). Differenze = imprecisione del depth.
        let wide = read_pixels(&r, &device, &queue);
        r.camera.scene_bounds = Some(main_bbox);
        r.render_frame(&device, &queue, &mut egui_renderer, 1600, 900);
        let tight = read_pixels(&r, &device, &queue);
        println!(
            "Piani larghi {:?} / stretti: pixel diversi {}",
            { r.camera.scene_bounds = Some(bbox); r.camera.clip_planes() },
            wide.chunks(4).zip(tight.chunks(4)).filter(|(a, b)| a != b).count()
        );

        const FRAMES: u32 = 60;
        let (mut cpu, mut total) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        for _ in 0..FRAMES {
            r.camera.orbit(0.01, 0.005); // la vista cambia: ogni frame va ridisegnato davvero
            let t = std::time::Instant::now();
            r.render_frame(&device, &queue, &mut egui_renderer, 1600, 900);
            cpu += t.elapsed();
            let _ = device.poll(wgpu::Maintain::Wait);
            total += t.elapsed();
        }
        println!(
            "Frame 1600x900: preparazione CPU {:?}, totale con GPU {:?} ({:.0} fps)",
            cpu / FRAMES,
            total / FRAMES,
            FRAMES as f64 / total.as_secs_f64()
        );
        let t = std::time::Instant::now();
        r.render_frame(&device, &queue, &mut egui_renderer, 1600, 900);
        println!(
            "Chiamate di disegno opache: {}, blocchi: {}, frame senza modifiche: {:?}",
            r.opaque_runs.len(),
            r.chunks.len(),
            t.elapsed()
        );
    }

    /// Un file aggiunto viene disegnato con i suoi colori, senza toccare le mesh già caricate.
    #[test]
    fn added_file_is_drawn_on_the_gpu() {
        let _gpu = gpu_lock();
        let instance = wgpu::Instance::default();
        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            return;
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let red = br#"#VRML V2.0 utf8
Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
  geometry IndexedFaceSet { coord Coordinate { point [ -3 -1 0, -1 -1 0, -2 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;
        let blue = br#"#VRML V2.0 utf8
Shape { appearance Appearance { material Material { diffuseColor 0 0 1 } }
  geometry IndexedFaceSet { coord Coordinate { point [ 1 -1 0, 3 -1 0, 2 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
        let mut r = WgpuRenderer::new(&device, format);
        let mut scene = crate::parser::parse_wrl(red);
        scene.into_file("rosso.wrl".into(), None);
        r.upload_meshes(&scene.shapes_data, &device);
        let first_chunks = r.chunks.len();
        let mut other = crate::parser::parse_wrl(blue);
        other.into_file("blu.wrl".into(), None);
        scene.append(other);
        assert_eq!(r.append_meshes(&scene.shapes_data, &device), 0);
        assert_eq!(r.slots.len(), 2);
        assert_eq!(r.chunks.len(), first_chunks + 1, "i buffer esistenti restano");

        let bbox = scene.compute_bounding_box();
        r.camera.fit_to_bbox(&bbox);
        r.camera.scene_bounds = Some(bbox);
        r.camera.orientation = glam::Quat::IDENTITY;
        r.sync_with_scene(&scene, &[]);
        const SIZE: u32 = 256;
        r.render_frame(&device, &queue, &mut egui_renderer, SIZE, SIZE);
        let pixels = read_pixels(&r, &device, &queue);
        let at = |p: glam::Vec3| {
            let n = r.camera.view_proj().project_point3(p);
            let x = ((n.x + 1.0) * 0.5 * SIZE as f32) as usize;
            let y = ((1.0 - n.y) * 0.5 * SIZE as f32) as usize;
            let o = (y * SIZE as usize + x) * 4;
            [pixels[o], pixels[o + 1], pixels[o + 2]]
        };
        let (left, right) = (at(glam::Vec3::new(-2.0, -0.3, 0.0)), at(glam::Vec3::new(2.0, -0.3, 0.0)));
        assert!(left[0] > 120 && left[2] < 60, "rosso {left:?}");
        assert!(right[2] > 120 && right[0] < 60, "blu {right:?}");

        // Sezione a X = 0: resta il rosso (X < 0); invertita, resta il blu
        let background = {
            let p = pixels.chunks(4).next().unwrap();
            [p[0], p[1], p[2]]
        };
        for (flip, keep_red) in [(false, true), (true, false)] {
            r.section = Some(Section { axis: 0, position: 0.0, flip });
            r.render_frame(&device, &queue, &mut egui_renderer, SIZE, SIZE);
            let pixels = read_pixels(&r, &device, &queue);
            let at = |p: glam::Vec3| {
                let n = r.camera.view_proj().project_point3(p);
                let x = ((n.x + 1.0) * 0.5 * SIZE as f32) as usize;
                let y = ((1.0 - n.y) * 0.5 * SIZE as f32) as usize;
                let o = (y * SIZE as usize + x) * 4;
                [pixels[o], pixels[o + 1], pixels[o + 2]]
            };
            let (left, right) = (at(glam::Vec3::new(-2.0, -0.3, 0.0)), at(glam::Vec3::new(2.0, -0.3, 0.0)));
            assert_eq!(left == background, !keep_red, "rosso {left:?} con flip {flip}");
            assert_eq!(right == background, keep_red, "blu {right:?} con flip {flip}");
        }
    }

    /// Disegno vero su scheda video con una scena piccola: colori, parti nascoste, selezione,
    /// trasparenza, chiusura. Senza scheda video (macchine di test) non verifica nulla.
    #[test]
    fn scene_is_drawn_correctly_on_the_gpu() {
        let _gpu = gpu_lock();
        let instance = wgpu::Instance::default();
        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            return;
        };
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor::default(), None),
        )
        .unwrap();

        // Rosso a sinistra, blu a destra, verde semitrasparente davanti al blu
        let wrl = br#"#VRML V2.0 utf8
Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } }
  geometry IndexedFaceSet { coord Coordinate { point [ -3 -1 0, -1 -1 0, -2 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
Shape { appearance Appearance { material Material { diffuseColor 0 0 1 } }
  geometry IndexedFaceSet { coord Coordinate { point [ 1 -1 0, 3 -1 0, 2 1 0 ] } coordIndex [ 0, 1, 2, -1 ] } }
Shape { appearance Appearance { material Material { diffuseColor 0 1 0 transparency 0.5 } }
  geometry IndexedFaceSet { coord Coordinate { point [ 1 -1 1, 3 -1 1, 2 1 1 ] } coordIndex [ 0, 1, 2, -1 ] } }
"#;
        let mut scene = crate::parser::parse_wrl(wrl);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
        let mut r = WgpuRenderer::new(&device, format);
        r.upload_meshes(&scene.shapes_data, &device);
        let bbox = scene.compute_bounding_box();
        r.camera.fit_to_bbox(&bbox);
        r.camera.scene_bounds = Some(bbox);
        r.camera.orientation = glam::Quat::IDENTITY;
        const SIZE: u32 = 256;

        let mut frame = |r: &mut WgpuRenderer, scene: &crate::scene_graph::SceneGraph, sel: Option<u32>| {
            r.sync_with_scene(scene, sel.as_slice());
            r.render_frame(&device, &queue, &mut egui_renderer, SIZE, SIZE);
            let pixels = read_pixels(r, &device, &queue);
            // `dy` pixel sotto il punto proiettato
            let at = |r: &WgpuRenderer, p: glam::Vec3, dy: usize| {
                let n = r.camera.view_proj().project_point3(p);
                let x = ((n.x + 1.0) * 0.5 * SIZE as f32) as usize;
                let y = ((1.0 - n.y) * 0.5 * SIZE as f32) as usize + dy;
                let o = (y * SIZE as usize + x) * 4;
                [pixels[o], pixels[o + 1], pixels[o + 2]]
            };
            (
                at(r, glam::Vec3::new(-2.0, -0.3, 0.0), 0),
                at(r, glam::Vec3::new(2.0, -0.3, 1.0), 0),
                at(r, glam::Vec3::new(0.0, -0.3, 0.0), 0),
                // Appena sotto il lato inferiore del triangolo rosso, fuori dalla sagoma
                at(r, glam::Vec3::new(-2.0, -1.0, 0.0), 1),
            )
        };
        let is_red = |c: [u8; 3]| c[0] > 120 && c[1] < 60 && c[2] < 60;

        // Tutto visibile; 2 opache adiacenti = 1 sola chiamata
        let (left, right, gap, under_red) = frame(&mut r, &scene, None);
        assert_eq!(r.opaque_runs.len(), 1);
        assert!(is_red(left), "sinistra {left:?}");
        assert!(right[1] > 60 && right[2] > 60 && right[0] < 60, "verde sul blu {right:?}");
        let background = gap;
        assert_ne!(left, background);
        assert_eq!(under_red, background, "senza selezione nessun contorno");

        // Nascosto il rosso: si vede lo sfondo
        let red_node = scene.mesh_to_node[0];
        scene.set_subtree_visible(red_node, false);
        let (left, right_after, _, _) = frame(&mut r, &scene, None);
        assert_eq!(left, background);
        assert_eq!(right_after, right);

        // Rimostrato e selezionato: rosso tinto di verde, contorno verde
        // appena fuori dalla sagoma
        scene.set_subtree_visible(red_node, true);
        let (unselected, _, _, _) = frame(&mut r, &scene, None);
        let (left, _, _, under_red) = frame(&mut r, &scene, Some(red_node));
        assert!(left[0] > 100 && left[1] > unselected[1] + 30 && left[2] < 90, "selezione {left:?}");
        assert!(under_red[0] < 120 && under_red[1] > 180 && under_red[2] < 140, "contorno {under_red:?}");

        // Sfondo sfumato: in alto più chiaro, in basso più scuro; i pezzi non cambiano
        r.background_gradient = true;
        let (left_flat, _, _, _) = frame(&mut r, &scene, None);
        let pixels = read_pixels(&r, &device, &queue);
        let px = |x: usize, y: usize| {
            let o = (y * SIZE as usize + x) * 4;
            pixels[o] as u32 + pixels[o + 1] as u32 + pixels[o + 2] as u32
        };
        let (top, bottom) = (px(2, 2), px(2, SIZE as usize - 3));
        assert!(top > bottom + 100, "alto {top}, basso {bottom}");
        assert!(is_red(left_flat), "il pezzo resta rosso {left_flat:?}");
        r.background_gradient = false;

        // Chiusura: resta solo lo sfondo (anche con la camera ferma)
        let camera = (r.camera.target, r.camera.distance, r.camera.orientation);
        r.clear_meshes();
        (r.camera.target, r.camera.distance, r.camera.orientation) = camera;
        r.camera.scene_bounds = Some(bbox);
        r.render_frame(&device, &queue, &mut egui_renderer, SIZE, SIZE);
        let pixels = read_pixels(&r, &device, &queue);
        assert!(pixels.chunks(4).all(|p| p[..3] == background), "dopo la chiusura solo sfondo");
    }
}
