// Flame-view glow, worked out once per cell per frame instead of per screen pixel.
// (The render settings struct `R` is prepended from render.wgsl when this is compiled.)

@group(0) @binding(0) var<uniform> r: R;
@group(0) @binding(1) var<storage, read> st: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> mat: array<u32>;
@group(0) @binding(3) var<storage, read_write> em: array<vec4<f32>>;   // light given off by each cell
@group(0) @binding(4) var<storage, read_write> glow: array<vec4<f32>>; // glow gathered around each cell

// must match flame_color in render.wgsl
fn flame_color(T: f32) -> vec3<f32> {
    let t = clamp((T - 600.0) / 2700.0, 0.0, 1.0);
    // glowing soot (kerosene): an incandescent ramp, dull red -> orange -> yellow -> white
    var c = mix(vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.45, 0.03, 0.0), smoothstep(0.0, 0.15, t));
    c = mix(c, vec3<f32>(1.0, 0.22, 0.02), smoothstep(0.15, 0.4, t));
    c = mix(c, vec3<f32>(1.0, 0.55, 0.05), smoothstep(0.4, 0.6, t));
    c = mix(c, vec3<f32>(1.0, 0.9, 0.2), smoothstep(0.6, 0.82, t));
    c = mix(c, vec3<f32>(1.0, 1.0, 0.85), smoothstep(0.82, 1.0, t));
    // a clean flame (hydrogen, methane, hypergolics) has little soot to glow: it shows the faint colour of
    // its hot gases instead (hydrogen pale violet, methane blue, hypergolics orange-pink), the hottest
    // core whitening a little
    let tint = vec3<f32>(r.tint_r, r.tint_g, r.tint_b);
    let clean = (tint * (0.12 + 0.6 * smoothstep(0.1, 0.9, t)) + vec3<f32>(0.22) * smoothstep(0.8, 1.0, t)) * smoothstep(0.0, 0.25, t);
    let lum = 0.15 + 0.85 * clamp(r.soot, 0.0, 1.0);
    return mix(clean, c, lum);
}

@compute @workgroup_size(64)
fn emit(@builtin(global_invocation_id) gid: vec3<u32>) {
    let j = gid.x;
    if (j >= r.w * r.h) { return; }
    if ((mat[j] & 255u) != 0u) {
        em[j] = vec4<f32>(0.0);
        return;
    }
    let U = st[j];
    let rho = max(U.x, 1e-6);
    let e = max(U.w / rho - 0.5 * (U.y * U.y + U.z * U.z) / (rho * rho), 1.0);
    let T = e * (r.gamma - 1.0) / r.rgas;
    em[j] = vec4<f32>(flame_color(T) * (1.0 - exp(-20.0 * rho)), 0.0);
}

// soft glow from hot gas around each cell (two rings of samples, as the old per-pixel bloom)
@compute @workgroup_size(64)
fn gather(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= r.w * r.h) { return; }
    let cf = vec2<f32>(f32(i % r.w) + 0.5, f32(i / r.w) + 0.5);
    var acc = vec3<f32>(0.0);
    for (var k = 0; k < 8; k = k + 1) {
        let a = f32(k) * 0.7853982 + 0.39;
        let d = vec2<f32>(cos(a), sin(a));
        for (var ring = 0; ring < 2; ring = ring + 1) {
            var rad = 2.5;
            var wgt = 0.09;
            if (ring == 1) {
                rad = 6.0;
                wgt = 0.045;
            }
            let q = cf + d * rad;
            if (q.x >= 0.0 && q.y >= 0.0 && q.x < f32(r.w) && q.y < f32(r.h)) {
                acc = acc + em[u32(q.y) * r.w + u32(q.x)].xyz * wgt;
            }
        }
    }
    glow[i] = vec4<f32>(acc, 0.0);
}
