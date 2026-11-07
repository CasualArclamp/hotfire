// Tracer particles riding the gas. Two vec4 per particle: (x, y, ttl, 0) and (vx, vy, speed, 0).
struct PP { w: u32, h: u32, count: u32, frame: u32, dtc: f32, gamma: f32, rgas: f32, dx: f32 }

@group(0) @binding(0) var<uniform> pp: PP;
@group(0) @binding(1) var<storage, read> st: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> mat: array<u32>;
@group(0) @binding(3) var<storage, read_write> parts: array<vec4<f32>>;

fn hash(x: u32) -> u32 {
    var v = x;
    v = v ^ (v >> 16u);
    v = v * 0x7feb352du;
    v = v ^ (v >> 15u);
    v = v * 0x846ca68bu;
    v = v ^ (v >> 16u);
    return v;
}
fn rnd(x: u32) -> f32 { return f32(hash(x) & 0xffffffu) / 16777216.0; }

fn pressure(U: vec4<f32>) -> f32 {
    let r = max(U.x, 1e-6);
    return (pp.gamma - 1.0) * (U.w - 0.5 * (U.y * U.y + U.z * U.z) / r);
}

@compute @workgroup_size(64)
fn advect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= pp.count) { return; }
    var P = parts[2u * i];
    var V = parts[2u * i + 1u];
    var alive = P.z > 0.0;
    if (alive) {
        let c = u32(P.y) * pp.w + u32(P.x);
        let U = st[c];
        let rho = max(U.x, 1e-6);
        let v = vec2<f32>(U.y, U.z) / rho;
        let nx = P.x + v.x * pp.dtc;
        let ny = P.y + v.y * pp.dtc;
        if (nx < 0.0 || ny < 0.0 || nx >= f32(pp.w) || ny >= f32(pp.h)) {
            alive = false;
        } else if ((mat[u32(ny) * pp.w + u32(nx)] & 255u) != 0u) {
            alive = false;
        } else {
            P.x = nx;
            P.y = ny;
            P.z = P.z - 1.0;
            V = vec4<f32>(v.x, v.y, length(v), 0.0);
        }
    }
    if (!alive) {
        P.z = 0.0;
        for (var k = 0u; k < 8u; k = k + 1u) {
            let s = i * 7919u + pp.frame * 104729u + k * 31u;
            let x = rnd(s) * f32(pp.w);
            let y = rnd(s ^ 0x9e3779b9u) * f32(pp.h);
            let c = u32(y) * pp.w + u32(x);
            if ((mat[c] & 255u) == 0u && pressure(st[c]) > 4000.0) {
                P = vec4<f32>(x, y, 80.0 + rnd(s + 17u) * 220.0, 0.0);
                V = vec4<f32>(0.0, 0.0, 0.0, 0.0);
                break;
            }
        }
    }
    parts[2u * i] = P;
    parts[2u * i + 1u] = V;
}
