struct R {
    w: u32, h: u32, mode: u32, show_heat: u32,
    cx: f32, cy: f32, scale: f32, gamma: f32,
    rw: f32, rh: f32, rgas: f32, particles: u32,
}
@group(0) @binding(0) var<uniform> r: R;
@group(0) @binding(1) var<storage, read> parts: array<vec4<f32>>;

struct VO { @builtin(position) pos: vec4<f32>, @location(0) col: vec4<f32> }

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
    var o: VO;
    let P = parts[2u * ii];
    let V = parts[2u * ii + 1u];
    if (P.z <= 0.0 || r.particles == 0u || V.z < 1.0) {
        o.pos = vec4<f32>(2.0, 2.0, 0.0, 1.0);
        o.col = vec4<f32>(0.0);
        return o;
    }
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -1.0), vec2<f32>(0.5, -1.0), vec2<f32>(-0.5, 1.0),
        vec2<f32>(-0.5, 1.0), vec2<f32>(0.5, -1.0), vec2<f32>(0.5, 1.0));
    let q = corners[vi];
    let d = vec2<f32>(V.x, V.y) / V.z;
    let perp = vec2<f32>(-d.y, d.x);
    let len = 2.0 + 16.0 * clamp(V.z / 5000.0, 0.0, 1.0);
    let cen = (vec2<f32>(P.x, P.y) - vec2<f32>(r.cx, r.cy)) * r.scale + 0.5 * vec2<f32>(r.rw, r.rh);
    let px = cen + d * q.y * len * 0.5 + perp * q.x * 1.8;
    o.pos = vec4<f32>(px.x / r.rw * 2.0 - 1.0, 1.0 - px.y / r.rh * 2.0, 0.0, 1.0);
    let s = clamp(V.z / 5000.0, 0.0, 1.0);
    o.col = vec4<f32>(mix(vec3<f32>(0.55, 0.8, 1.0), vec3<f32>(1.0, 1.0, 0.9), s), 0.6);
    return o;
}

@fragment
fn fs(in: VO) -> @location(0) vec4<f32> {
    return in.col;
}
