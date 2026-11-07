struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // Direzioni verso le luci e "su" dello schermo, in coordinate mondo. Le luci seguono la
    // camera (come nei visualizzatori CAD): il lato del pezzo che si guarda è sempre illuminato.
    key_light: vec4<f32>,
    fill_light: vec4<f32>,
    view_up: vec4<f32>,
    // Piano di sezione: si scarta ciò che ha dot(p, xyz) > w. Spento: (0, 0, 0, 1).
    clip_plane: vec4<f32>,
    // Sfondo sfumato (lineare): colore in alto e in basso
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
};

fn clipped(p: vec3<f32>) -> bool {
    return dot(p, camera.clip_plane.xyz) > camera.clip_plane.w;
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

// Colore (lineare, alpha = opacità) di ogni mesh, indicizzato dall'attributo mesh_id:
// una draw call può coprire molte mesh dei buffer condivisi.
@group(1) @binding(0)
var<storage, read> mesh_colors: array<vec4<f32>>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) mesh_id: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.world_pos = in.position;
    out.world_normal = in.normal;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.color = mesh_colors[in.mesh_id];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (clipped(in.world_pos)) {
        discard;
    }
    let v = normalize(camera.camera_pos.xyz - in.world_pos);

    // Normale rivolta verso chi guarda, qualunque sia l'orientamento delle facce nel file
    // (i CAD esportano spesso superfici con verso misto). Riflessa sul piano di vista invece
    // che invertita: resta continua sui contorni, dove la normale passa da davanti a dietro.
    var n = normalize(in.world_normal);
    n = normalize(n - 2.0 * min(dot(n, v), 0.0) * v + v * 1e-4);

    // Ambiente a emisfero: più chiaro verso l'alto dello schermo, più scuro verso il basso.
    // Dà la forma anche alle zone che le luci non colpiscono direttamente.
    let sky = vec3<f32>(0.30, 0.31, 0.34);
    let ground = vec3<f32>(0.13, 0.13, 0.14);
    let ambient = mix(ground, sky, 0.5 + 0.5 * dot(n, camera.view_up.xyz));

    // Luce principale (in alto a destra, davanti) e luce di schiarita (a sinistra, più debole)
    let key = camera.key_light.xyz;
    let diffuse = 0.66 * max(dot(n, key), 0.0) + 0.30 * max(dot(n, camera.fill_light.xyz), 0.0);

    // Riflesso della luce principale, contenuto per non bruciare le superfici chiare
    let spec = pow(max(dot(n, normalize(key + v)), 0.0), 48.0) * 0.20;

    // Leggero scurimento delle superfici viste di taglio: rende leggibili le curvature
    let facing = 0.82 + 0.18 * dot(n, v);

    let lit = in.color.rgb * (ambient + vec3<f32>(diffuse)) * facing + vec3<f32>(spec);
    return vec4<f32>(min(lit, vec3<f32>(1.0)), in.color.a);
}

// Maschera della selezione: le mesh selezionate scrivono 1 in una texture a un canale,
// senza depth (sagoma intera, anche dietro ad altri pezzi). Il contorno nasce da qui.
struct MaskOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
};

@vertex
fn vs_mask(in: VertexInput) -> MaskOutput {
    var out: MaskOutput;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.world_pos = in.position;
    return out;
}

@fragment
fn fs_mask(in: MaskOutput) -> @location(0) vec4<f32> {
    if (clipped(in.world_pos)) {
        discard; // la parte tagliata non ha contorno
    }
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
// Sfondo sfumato: triangolo a tutto schermo disegnato per primo, sotto ai pezzi
struct BackgroundOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BackgroundOutput {
    var out: BackgroundOutput;
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.t = y * 0.5 + 0.5; // 0 in basso, 1 in alto
    return out;
}

@fragment
fn fs_background(in: BackgroundOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(mix(camera.bg_bottom.rgb, camera.bg_top.rgb, clamp(in.t, 0.0, 1.0)), 1.0);
}