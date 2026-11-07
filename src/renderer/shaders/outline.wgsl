// Contorno della selezione: un triangolo a tutto schermo legge la maschera delle mesh
// selezionate e colora i pixel fuori dalla sagoma entro RADIUS pixel dal bordo.

@group(0) @binding(0)
var mask: texture_2d<f32>;

// COLOR_SELECTED (verde 0.20, 0.85, 0.25 sRGB) in valori lineari
const COLOR: vec3<f32> = vec3<f32>(0.0331, 0.6920, 0.0509);

const RADIUS: i32 = 2;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // Triangolo che copre lo schermo: (-1,-1), (3,-1), (-1,3)
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(mask));
    let p = vec2<i32>(pos.xy);
    if (textureLoad(mask, p, 0).r > 0.5) {
        discard; // dentro la sagoma: resta la superficie del pezzo
    }
    // Copertura del bordo: distanza dal pixel selezionato più vicino, sfumata sull'ultimo pixel
    var best = 1e9;
    for (var dy = -RADIUS; dy <= RADIUS; dy++) {
        for (var dx = -RADIUS; dx <= RADIUS; dx++) {
            let q = clamp(p + vec2<i32>(dx, dy), vec2<i32>(0), size - 1);
            if (textureLoad(mask, q, 0).r > 0.5) {
                best = min(best, f32(dx * dx + dy * dy));
            }
        }
    }
    let alpha = clamp(f32(RADIUS) + 0.5 - sqrt(best), 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(COLOR, alpha);
}
