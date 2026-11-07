struct R {
    w: u32, h: u32, mode: u32, show_heat: u32,
    cx: f32, cy: f32, scale: f32, gamma: f32,
    rw: f32, rh: f32, rgas: f32, particles: u32,
    time: f32, tint_r: f32, tint_g: f32, tint_b: f32,
    soot: f32, lf_r: f32, lf_g: f32, lf_b: f32,
    lo_r: f32, lo_g: f32, lo_b: f32, edit_layer: u32,
    amb_rho: f32, _q0: f32, _q1: f32, _q2: f32,
}
@group(0) @binding(0) var<uniform> r: R;
@group(0) @binding(1) var<storage, read> st: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> mat: array<u32>;
@group(0) @binding(3) var<storage, read> phase: array<f32>;  // per cell: distance from the tank along its line
@group(0) @binding(5) var<storage, read> spec: array<f32>;   // combustible gas density
@group(0) @binding(6) var<storage, read> layers: array<u32>;
@group(0) @binding(7) var<storage, read> glow: array<vec4<f32>>; // flame-view glow per cell (glow.wgsl) // back pipe type|net, front pipe type|net (8 bits each)
@group(0) @binding(4) var<storage, read> anim: array<f32>;   // [256] line flow offsets, then 64 x (x0, y0, w, h, angle, horiz, stages, 0)

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: VOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn cmap(t0: f32) -> vec3<f32> {
    let t = clamp(t0, 0.0, 1.0);
    var c = mix(vec3<f32>(0.0, 0.0, 0.04), vec3<f32>(0.40, 0.05, 0.50), smoothstep(0.0, 0.3, t));
    c = mix(c, vec3<f32>(0.95, 0.30, 0.10), smoothstep(0.3, 0.6, t));
    c = mix(c, vec3<f32>(1.0, 0.85, 0.20), smoothstep(0.6, 0.85, t));
    c = mix(c, vec3<f32>(1.0, 1.0, 0.95), smoothstep(0.85, 1.0, t));
    return c;
}

// rough incandescence ramp: dull red -> orange -> yellow -> white
fn blackbody(T: f32) -> vec3<f32> {
    let t = clamp((T - 700.0) / 3300.0, 0.0, 1.0);
    return vec3<f32>(smoothstep(0.0, 0.35, t), 0.92 * smoothstep(0.12, 0.72, t), 0.85 * smoothstep(0.45, 1.0, t));
}

fn hash3(x: u32, y: u32, z: u32) -> f32 {
    var v = x * 73856093u ^ y * 19349663u ^ z * 83492791u;
    v = v ^ (v >> 16u);
    v = v * 0x7feb352du;
    v = v ^ (v >> 15u);
    return f32(v & 0xffffu) / 65535.0;
}

// smooth value noise and a few octaves of it, for the drifting vapour in tank ullage
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let ix = u32(i32(i.x) + 100000);
    let iy = u32(i32(i.y) + 100000);
    let a = hash3(ix, iy, 3u);
    let b = hash3(ix + 1u, iy, 3u);
    let c = hash3(ix, iy + 1u, 3u);
    let d = hash3(ix + 1u, iy + 1u, 3u);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return 0.55 * vnoise(p) + 0.3 * vnoise(p * 2.1 + vec2<f32>(5.2, 1.3)) + 0.15 * vnoise(p * 4.3 + vec2<f32>(1.7, 9.2));
}

fn base_color(t: u32) -> vec3<f32> {
    switch t {
        case 1u: { return vec3<f32>(0.47, 0.49, 0.53); }
        case 2u: { return vec3<f32>(0.77, 0.47, 0.27); }
        case 3u: { return vec3<f32>(0.84, 0.81, 0.70); }
        case 4u: { return vec3<f32>(0.35, 0.59, 0.94); }
        case 5u: { return vec3<f32>(0.94, 0.67, 0.27); }
        case 6u: { return vec3<f32>(0.78, 0.51, 0.16); }
        case 7u: { return vec3<f32>(0.24, 0.39, 0.75); }
        case 8u: { return vec3<f32>(0.43, 0.35, 0.82); }
        case 9u: { return vec3<f32>(0.78, 0.35, 0.59); }
        case 10u, 12u: { return vec3<f32>(0.27, 0.78, 0.35); }
        case 11u, 13u: { return vec3<f32>(0.86, 0.24, 0.24); }
        case 14u: { return vec3<f32>(0.59, 0.82, 1.0); }
        case 15u: { return vec3<f32>(1.0, 0.82, 0.59); }
        case 16u: { return vec3<f32>(0.94, 0.90, 0.31); }
        case 17u: { return vec3<f32>(0.88, 0.88, 0.92); }
        case 18u: { return vec3<f32>(0.90, 0.55, 0.24); }
        case 20u: { return vec3<f32>(0.90, 0.88, 0.55); }
        case 21u: { return vec3<f32>(0.59, 0.65, 0.78); }
        case 22u: { return vec3<f32>(0.43, 0.78, 0.39); }
        case 23u: { return vec3<f32>(1.0, 0.47, 0.12); }
        case 24u: { return vec3<f32>(0.59, 0.58, 0.67); }
        case 25u: { return vec3<f32>(0.92, 0.78, 0.47); }
        case 26u: { return vec3<f32>(0.82, 0.35, 0.27); }
        case 27u: { return vec3<f32>(0.94, 0.67, 0.24); }
        case 19u: { return vec3<f32>(0.35, 0.35, 0.40); }
        case 28u: { return vec3<f32>(0.27, 0.59, 0.78); }
        case 31u: { return vec3<f32>(0.75, 0.43, 0.31); }
        case 32u: { return vec3<f32>(0.78, 0.67, 0.35); }
        case 33u: { return vec3<f32>(0.35, 0.78, 0.47); }
        case 29u: { return vec3<f32>(0.30, 0.55, 0.28); }
        case 30u: { return vec3<f32>(0.82, 0.47, 0.20); }
        default: { return vec3<f32>(0.1, 0.1, 0.12); }
    }
}

fn gas_rho(i: u32) -> f32 {
    if ((mat[i] & 255u) != 0u) { return -1.0; }
    return max(st[i].x, 1e-6);
}

// Flame view, styled on Combuster-like games: colour follows temperature (white-yellow when hottest,
// through orange and red to dark as the exhaust cools), soft blobs carried along with the flow,
// and a bloom (see `bloom`) that lets hot gas glow over walls and past the plume's edges.
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

// light given off by one cell of gas (nothing for solids)
fn emission(j: u32) -> vec3<f32> {
    if ((mat[j] & 255u) != 0u) { return vec3<f32>(0.0); }
    let U = st[j];
    let rho = max(U.x, 1e-6);
    let e = max(U.w / rho - 0.5 * (U.y * U.y + U.z * U.z) / (rho * rho), 1.0);
    let T = e * (r.gamma - 1.0) / r.rgas;
    return flame_color(T) * (1.0 - exp(-20.0 * rho));
}

// flame-view glow at this point: the per-cell glow (worked out in glow.wgsl) blended between the four
// nearest cells
fn glow_at(cf: vec2<f32>) -> vec3<f32> {
    let g = cf - vec2<f32>(0.5);
    let x0 = clamp(i32(floor(g.x)), 0, i32(r.w) - 1);
    let y0 = clamp(i32(floor(g.y)), 0, i32(r.h) - 1);
    let x1 = min(x0 + 1, i32(r.w) - 1);
    let y1 = min(y0 + 1, i32(r.h) - 1);
    let f = clamp(g - floor(g), vec2<f32>(0.0), vec2<f32>(1.0));
    let a = mix(glow[u32(y0) * r.w + u32(x0)].xyz, glow[u32(y0) * r.w + u32(x1)].xyz, f.x);
    let b = mix(glow[u32(y1) * r.w + u32(x0)].xyz, glow[u32(y1) * r.w + u32(x1)].xyz, f.x);
    return mix(a, b, f.y);
}

// soft glow gathered from hot gas around this point (two rings of samples); per-pixel original of glow_at
fn bloom(cf: vec2<f32>) -> vec3<f32> {
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
                acc = acc + emission(u32(q.y) * r.w + u32(q.x)) * wgt;
            }
        }
    }
    return acc;
}

fn flame(i: u32, ix: u32, iy: u32, rho: f32, T: f32, vel: vec2<f32>, cf: vec2<f32>) -> vec3<f32> {
    var em = flame_color(T) * (1.0 - exp(-20.0 * rho));
    // actively burning gas (hot, with unburnt propellant still in it) glows as a bright flame front
    let burning_now = clamp(spec[i] / max(rho, 1e-6), 0.0, 1.0) * smoothstep(850.0, 1100.0, T);
    // (its colour follows the propellant too: a sooty flame front is yellow, a clean one its own tint)
    let front = mix(vec3<f32>(r.tint_r, r.tint_g, r.tint_b) * 0.8 + vec3<f32>(0.15), vec3<f32>(1.0, 0.78, 0.35), 0.2 + 0.8 * clamp(r.soot, 0.0, 1.0));
    em = em + front * smoothstep(0.0, 0.003, burning_now) * 0.9;
    // soft blobs carried along by the gas (faster gas, faster blobs)
    let sp = length(vel);
    var dir = vec2<f32>(0.0, 1.0);
    if (sp > 1.0) { dir = vel / sp; }
    // slow or swirling gas still streams along the exhaust: tongues, not drifting blobs
    dir = normalize(mix(vec2<f32>(0.0, 1.0), dir, clamp(sp / 600.0, 0.0, 1.0)) + vec2<f32>(0.0001));
    let along = dot(cf, dir);
    let across = dot(cf, vec2<f32>(-dir.y, dir.x));
    // the flame texture races along the flow: fast gas crosses the screen in about a tenth of a second
    let tt = r.time - 60.0 * floor(r.time / 60.0);
    let drift = tt * (2500.0 + 5500.0 * clamp(sp / 3000.0, 0.0, 1.5));
    // long licking tongues (stretched along the flow) with finer flicker on top
    let blob = fbm(vec2<f32>(across * 0.35, (along - drift) * 0.025));
    let fine = fbm(vec2<f32>(across * 0.9 + 7.0, (along - drift * 1.4) * 0.07));
    let tongues = 0.65 * blob + 0.35 * fine;
    em = em * (0.2 + 1.2 * tongues);
    // shock diamonds still light up where the gas compresses
    if (iy > 0u && iy + 1u < r.h) {
        let up = gas_rho(i - r.w);
        let dn = gas_rho(i + r.w);
        if (up > 0.0 && dn > 0.0) {
            let comp = clamp((dn - up) / rho * 1.2, 0.0, 1.0);
            em = em + vec3<f32>(1.0, 0.85, 0.5) * comp * smoothstep(900.0, 2000.0, T) * 0.4;
        }
    }
    // gas that isn't burning is smoke, drawn like the flame: soft wisps carried along with the flow.
    // Dense cold gas or warm exhaust (a dumped gas generator) shows; a sooty propellant (kerosene)
    // makes it thick, dark and billowing, a clean one (hydrogen) thin and wispy; unburnt fuel is tan.
    let excess = max(rho - 1.5 * r.amb_rho - 0.002, 0.0);
    let dens = 1.0 - exp(-25.0 * excess);
    let warm = smoothstep(380.0, 800.0, T);
    let amount = max(dens, warm);
    let soot = clamp(r.soot, 0.0, 1.0);
    let unb = clamp(spec[i] / rho, 0.0, 1.0);
    var tint = mix(vec3<f32>(0.62, 0.64, 0.7), vec3<f32>(0.33, 0.29, 0.25), soot);
    tint = mix(tint, vec3<f32>(0.78, 0.5, 0.22), unb);
    // (smoke drifts slowly: at the flame's speed its fine texture would alias into snow)
    let drift_s = r.time * (40.0 + 220.0 * clamp(sp / 3000.0, 0.0, 1.5));
    let wisp = fbm(vec2<f32>(across * mix(0.32, 0.16, soot), (along - drift_s) * mix(0.22, 0.1, soot)));
    let body = mix(0.35, 0.95, soot); // how solid the smoke is
    let alpha = clamp(amount * (body + mix(1.1, 0.6, soot) * (wisp - 0.5)), 0.0, 1.0);
    let cold = tint * alpha * 0.8;
    let burning = smoothstep(550.0, 1000.0, T * (0.85 + 0.3 * blob)); // (the big slow noise only: the fine one speckles gas that is only warm) // ragged edge: flame licks out past the hot core. Gas from ~550 K glows (dull red at gas-generator temperatures)
    return mix(cold, em, burning);
}

const A_TANK: u32 = 768u;    // must match world::ANIM_TANK
const A_FRONT: u32 = 1024u;  // must match world::ANIM_FRONT
const OUTLINE: vec3<f32> = vec3<f32>(0.46, 0.49, 0.56);
const EMPTY_PIPE: vec3<f32> = vec3<f32>(0.07, 0.075, 0.09);

const A_PROP: u32 = 1280u;   // must match world::ANIM_PROP
const A_HE: u32 = 1536u;     // must match world::ANIM_HE
const A_BOIL: u32 = 1600u;   // must match world::ANIM_BOIL
const A_VAP: u32 = 1856u;    // must match world::ANIM_VAP
const A_BURN: u32 = 2112u;   // must match world::ANIM_BURN
const A_AIR: u32 = 2121u;    // must match world::ANIM_AIR
const A_PLUME: u32 = 2124u;  // must match world::ANIM_PLUME
const A_TEA: u32 = 2136u;    // must match world::ANIM_TEA

// what a network holds: 0 fuel, 1 oxidiser, 2 both (mixed), 9 nothing
fn net_prop(net: u32) -> u32 {
    if (net >= 255u) { return 9u; }
    let p = u32(anim[A_PROP + net]);
    if (p > 3u) { return 9u; }
    return p;
}

// a tank's contents are fixed by its type
fn tank_prop(t: u32) -> u32 {
    if (t == 6u) { return 0u; }
    if (t == 7u) { return 1u; }
    return 9u;
}

fn liquid(pp: u32) -> vec3<f32> {
    if (pp == 0u) { return vec3<f32>(r.lf_r, r.lf_g, r.lf_b); }
    if (pp == 1u) { return vec3<f32>(r.lo_r, r.lo_g, r.lo_b); }
    if (pp == 2u) { return vec3<f32>(0.55, 0.45, 0.35); }
    if (pp == 3u) { return vec3<f32>(0.62, 0.66, 0.76); } // helium gas
    return EMPTY_PIPE;
}

// plumbing and machinery get a one-pixel wall drawn in the empty space around them
fn outline_src(t: u32) -> bool {
    return (t >= 4u && t <= 13u) || (t >= 16u && t <= 19u) || t == 28u || t == 29u || t == 31u;
}

fn outline_near(ix: u32, iy: u32) -> bool {
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let x = i32(ix) + dx;
            let y = i32(iy) + dy;
            if (x < 0 || y < 0 || x >= i32(r.w) || y >= i32(r.h)) { continue; }
            if (outline_src(mat[u32(y) * r.w + u32(x)] & 255u)) { return true; }
        }
    }
    return false;
}

fn layer_type(l: u32, j: u32) -> u32 { return (layers[j] >> (16u * (l - 1u))) & 255u; }
fn layer_netid(l: u32, j: u32) -> u32 { return (layers[j] >> (16u * (l - 1u) + 8u)) & 255u; }

// A pipe on the back (1) or front (2) layer: liquid or empty inside, walls where it ends,
// and a ring where it joins middle-layer plumbing of the same propellant.
fn layer_pipe(l: u32, i: u32, ix: u32, iy: u32, cf: vec2<f32>) -> vec4<f32> {
    let t = layer_type(l, i);
    if (t == 0u) { return vec4<f32>(0.0); }
    let net = layer_netid(l, i);
    let pp = net_prop(net);
    let ph = phase[l * r.w * r.h + i];
    var col = EMPTY_PIPE;
    if (net < 255u && ph <= anim[A_FRONT + net]) {
        let f = fract((ph - anim[net]) / 9.0);
        col = liquid(pp) * (0.78 + 0.3 * smoothstep(0.7, 1.0, f));
    }
    let fr = fract(cf);
    let wth = clamp(1.5 / r.scale, 0.12, 0.45);
    if ((ix == 0u || layer_type(l, i - 1u) == 0u) && fr.x < wth) { col = OUTLINE; }
    if ((ix + 1u >= r.w || layer_type(l, i + 1u) == 0u) && fr.x > 1.0 - wth) { col = OUTLINE; }
    if ((iy == 0u || layer_type(l, i - r.w) == 0u) && fr.y < wth) { col = OUTLINE; }
    if ((iy + 1u >= r.h || layer_type(l, i + r.w) == 0u) && fr.y > 1.0 - wth) { col = OUTLINE; }
    if ((mat[i] & 255u) == 20u) {
        let d = length(fr - vec2<f32>(0.5, 0.5));
        if (d > 0.16 && d < 0.32) { col = vec3<f32>(0.95, 0.95, 0.7); }
    }
    return vec4<f32>(col, 1.0);
}

// Side-view cutaway of a gas burner: manifold galleries, an injector plate whose orifices spray jets of fuel and
// oxidiser, a flame that fills the chamber, and the converging nozzle at the outlet. a runs from the injector end
// (0) to the outlet (1), p across (-1..1).
fn burner_cutaway(cf: vec2<f32>, k: u32) -> vec3<f32> {
    let x0 = anim[k];
    let y0 = anim[k + 1u];
    let bw = max(anim[k + 2u], 1.0);
    let bh = max(anim[k + 3u], 1.0);
    let dir = anim[k + 4u];
    let horiz = anim[k + 5u] > 0.5;
    var temp = anim[k + 6u];
    // (the fuel line comes in on the high side of the block when the temperature slot has 2 added to it)
    let fuel_high = temp > 1.5;
    if (fuel_high) { temp = temp - 2.0; }
    let inten = clamp(anim[k + 7u], 0.0, 1.0);
    var a = (cf.x - x0) / bw;
    var p = (cf.y - y0) / bh * 2.0 - 1.0;
    if (!horiz) {
        a = (cf.y - y0) / bh;
        p = (cf.x - x0) / bw * 2.0 - 1.0;
    }
    if (dir < 0.0) { a = 1.0 - a; }
    let ap = abs(p);
    let t = r.time;
    let metal = vec3<f32>(0.62, 0.64, 0.70);
    // the colours of what goes in: the fuel and oxidiser liquids, and the flame the pair burns with
    let fuel_c = vec3<f32>(r.lf_r, r.lf_g, r.lf_b);
    let ox_c = vec3<f32>(r.lo_r, r.lo_g, r.lo_b);
    let tint_c = vec3<f32>(r.tint_r, r.tint_g, r.tint_b);
    // chamber half width: straight, then converging to the throat
    var hw = 0.86;
    if (a > 0.6) { hw = mix(0.86, 0.36, smoothstep(0.6, 0.92, a)); }
    let flame_lo = mix(tint_c, fuel_c, 0.3) * 0.85;
    let flame_hi = mix(tint_c, vec3<f32>(1.0), 0.45);
    let hot = mix(flame_lo, flame_hi, clamp(temp * 1.15 - 0.15, 0.0, 1.0)) * (1.0 + 0.35 * temp);
    // casing
    if (ap > hw) {
        let wallv = 0.32 + 0.22 * (1.0 - clamp((ap - hw) / max(1.0 - hw, 0.05), 0.0, 1.0));
        var wc = metal * wallv;
        // the casing glows where the flame is hot, strongest at the throat
        wc = wc + hot * inten * 0.35 * smoothstep(0.35, 0.95, a) * (1.0 - clamp((ap - hw) / 0.25, 0.0, 1.0));
        return wc;
    }
    // manifold galleries behind the plate: fuel one side, oxidiser the other, liquid streaming in
    if (a < 0.085) {
        let flow = 0.78 + 0.22 * sin(a * 90.0 - t * 9.0 + p * 4.0);
        if (ap < 0.04) { return metal * 0.5; }
        if ((p > 0.0) == fuel_high) { return fuel_c * 0.5 * flow; }
        return ox_c * 0.5 * flow;
    }
    // the injector plate: orifices alternate fuel and oxidiser across it
    let cell_w = 0.24;
    let idx = floor((p + 0.84) / cell_w + 0.5);
    let off = (p + 0.84) / cell_w + 0.5 - idx;                // 0..1 across this orifice's slot
    let is_fuel = (i32(idx) % 2) == 0;
    if (a < 0.125) {
        let hole = smoothstep(0.28, 0.2, abs(off - 0.5)) * step(abs(idx - 3.0), 3.5);
        let c0 = select(ox_c, fuel_c, is_fuel);
        return mix(metal * 0.55, c0 * (0.8 + 0.4 * inten), hole);
    }
    // inside the chamber
    var col = vec3<f32>(0.045, 0.035, 0.03);
    // jets: thin dashed streaks from each orifice, bending in towards the centre line as they mix
    var jets = vec3<f32>(0.0);
    for (var n = 0; n < 7; n = n + 1) {
        let pi = -0.72 + 0.24 * f32(n);
        let fuel_j = (n % 2) == 0;
        let pc = pi * (1.0 - 0.6 * smoothstep(0.125, 0.6, a));
        let wj = 0.035 + 0.07 * clamp(a - 0.125, 0.0, 0.6);
        let line = smoothstep(wj, 0.0, abs(p - pc));
        let dash = 0.55 + 0.45 * sin((a - 0.125) * 70.0 - t * 24.0 + f32(n) * 1.9);
        let fade = 1.0 - smoothstep(0.35, 0.66, a);
        let c1 = select(ox_c, fuel_c, fuel_j);
        jets = jets + c1 * line * dash * fade * (0.5 + 1.0 * inten);
    }
    col = col + jets * inten;
    // flame: ragged, flowing towards the outlet, brightest and whitest in the middle and towards the throat
    let q = vec2<f32>(a * 7.0 - t * 4.5, p * 3.0 + t * 0.6);
    let n1 = fbm(q);
    let n2 = fbm(q * 2.1 + vec2<f32>(3.7, -t * 1.3));
    let rim = 1.0 - clamp(ap / hw, 0.0, 1.0);
    let body = smoothstep(0.16, 0.5, a) * (0.35 + 0.65 * rim);
    let tongues = 0.55 + 0.9 * n1 + 0.4 * n2;
    let core = clamp(rim * 1.3 + smoothstep(0.5, 0.95, a) * 0.7 - 0.25, 0.0, 1.0);
    let flame_c = mix(hot * tongues, mix(tint_c, vec3<f32>(1.0), 0.75), core * 0.55);
    // little flames anchored at each orifice
    var anchor = 0.0;
    for (var n = 0; n < 7; n = n + 1) {
        let pi = -0.72 + 0.24 * f32(n);
        let d = length(vec2<f32>((a - 0.14) * 3.0, p - pi * 0.97));
        anchor = anchor + smoothstep(0.17, 0.0, d) * (0.6 + 0.4 * sin(t * 30.0 + f32(n) * 2.3));
    }
    let alpha = clamp(body * inten * 1.1 + anchor * 0.5 * inten, 0.0, 1.0);
    col = mix(col, flame_c, alpha);
    // the converging end blazes
    col = col + mix(tint_c, vec3<f32>(1.0), 0.5) * inten * smoothstep(0.82, 1.0, a) * 0.55 * (0.7 + 0.5 * n1);
    return col;
}

// The exhaust plume of the main nozzle, drawn from what the gas does at its exit: the exit speed and Mach number,
// the exit pressure against the air's (an under-expanded plume bursts wide, an over-expanded one pinches in),
// the flame temperature and the air rushing past. Shock diamonds, a Mach disk and long fast tongues; the gas
// simulation's own picture is hidden where this one is drawn. rgb is added, a is how much of the old picture to hide.
// the colour of exhaust as it ages (0: hot at the throat, 1: far down the plume): white-hot, then the flame's own
// colour (orange for soot, pink for hydrogen), then deep red or violet as it cools and thins
fn exhaust_pal(u: f32) -> vec3<f32> {
    let soot = clamp(r.soot, 0.0, 1.0);
    let tint = vec3<f32>(r.tint_r, r.tint_g, r.tint_b);
    let hot = vec3<f32>(1.0, 0.95, 0.75);
    let mid = mix(vec3<f32>(1.0, 0.48, 0.82), vec3<f32>(1.0, 0.58, 0.14), soot);
    let far = mix(tint * vec3<f32>(0.9, 0.7, 1.0) + vec3<f32>(0.05, 0.0, 0.1), vec3<f32>(0.8, 0.2, 0.04), soot);
    var c = mix(hot, mid, smoothstep(0.0, 0.4, u));
    c = mix(c, far, smoothstep(0.35, 1.0, u));
    return c;
}

fn plume(cf: vec2<f32>) -> vec4<f32> {
    if (anim[A_PLUME] < 0.5) { return vec4<f32>(0.0); }
    let xc = anim[A_PLUME + 1u];
    let ye = anim[A_PLUME + 2u];
    let hw = max(anim[A_PLUME + 3u], 3.0);
    let ve = anim[A_PLUME + 4u];
    let ratio = clamp(anim[A_PLUME + 5u], 0.03, 300.0);
    let mach = clamp(anim[A_PLUME + 6u], 1.0, 6.0);
    let tfl = anim[A_PLUME + 7u];
    let inten = anim[A_PLUME + 8u];
    let s = cf.y - ye;
    if (s < -170.0 || inten < 0.03) { return vec4<f32>(0.0); }
    let sp = max(s, 0.0);
    let wv = anim[A_AIR];
    let fast = clamp(ve / 3000.0, 0.0, 1.5);
    let soot = clamp(r.soot, 0.0, 1.0);
    let tint = vec3<f32>(r.tint_r, r.tint_g, r.tint_b);
    // how far the hot core carries: far in thin air, squashed by air rushing up into the nozzle
    let head = 1.0 / (1.0 + max(-wv, 0.0) / 100.0);
    let Lc = 1500.0 * head * clamp(fast, 0.3, 1.5);
    // the outline: the first expansion (or pinch) after the exit, then a slow turbulent spread
    let lr = log(ratio);
    let th0 = clamp(0.30 * lr, -0.3, 0.95);
    let Lth = 6.0 * hw;
    var grow = th0 * Lth * (1.0 - exp(-sp / Lth));
    grow = max(grow, -0.4 * hw);
    var R = hw + grow + 0.06 * sp * (0.7 + 0.3 * clamp(lr * 0.3 + 1.0, 0.3, 2.0));
    // shock cells along the axis
    // (a well under-expanded plume has one long first cell and then fans out smoothly, it doesn't keep repeating)
    let under = clamp((ratio - 2.0) / 6.0, 0.0, 1.0);
    let Ls = hw * (2.4 + 0.7 * mach) * (1.0 + 2.5 * under);
    let ph = 6.2832 * sp / Ls;
    let cells_left = mix(4.0, 1.2, under);
    let amp = clamp(0.03 + 0.22 * abs(lr), 0.0, 0.5) * exp(-sp / (Ls * cells_left)) * (1.0 - 0.8 * under);
    R = max(R * (1.0 + amp * sin(ph)), 0.3 * hw);
    // turbulence: big slow eddies swing the plume sideways (more the further out), smaller ones tear its edge
    let tt = r.time - 60.0 * floor(r.time / 60.0);
    let drift = tt * (2500.0 + 6500.0 * fast);
    let w1 = fbm(vec2<f32>(3.0 + (cf.x - xc) * 0.03, (sp - drift * 0.6) * 0.012));
    let dx = (cf.x - xc) + (w1 - 0.5) * 2.0 * (4.0 + 0.1 * sp) * smoothstep(0.0, 80.0, sp);
    let e1 = fbm(vec2<f32>(dx * 0.1, (sp - drift) * 0.03));
    let e2 = fbm(vec2<f32>(dx * 0.27 + 9.0, (sp - drift * 1.3) * 0.08));
    let turb = 0.6 * e1 + 0.4 * e2;
    let q0 = abs(dx) / R;
    let q = q0 * (1.0 + (turb - 0.5) * (0.12 + 0.7 * smoothstep(0.0, 350.0, sp)));
    let dec = exp(-sp / Lc);
    let fade_in = smoothstep(-1.0, 4.0, s);
    let airf = clamp(anim[A_AIR + 1u] * 4.0, 0.25, 1.0);
    // a soft body, a bright core and a white-hot axis near the exit
    let body = smoothstep(1.25, 0.3, q);
    let core = exp(-pow(q * 1.7, 2.0));
    let axis = exp(-pow(q * 3.4, 2.0)) * exp(-sp / (Lc * 0.22));
    let speck = 0.7 + 0.6 * turb;
    // colour: orange body, yellow core, white axis; a clean flame (hydrogen) is a thin pale tint instead
    let up = 0.45 + 0.55 * smoothstep(0.0, 1.0, sp / (0.7 * Lc));
    let c_body = exhaust_pal(up);
    let c_core = exhaust_pal(max(up - 0.25, 0.0));
    let c_white = exhaust_pal(0.0);
    var col = c_body * body * 0.85 + c_core * core * 0.8 + c_white * axis * 1.0;
    col = col * speck * (0.3 + 0.7 * dec);
    // shock diamonds: bright beads on the axis, stronger the further from matched pressure
    let dia = pow(0.5 + 0.5 * cos(ph), 5.0) * clamp(0.25 + 0.6 * abs(lr), 0.0, 1.0) * exp(-sp / (Ls * mix(7.0, 1.0, under))) * (1.0 - 0.7 * under);
    let beam = exp(-pow(q * 2.4, 2.0));
    var md = 0.0;
    if (ratio > 3.0) {
        let dm = 0.67 * hw * sqrt(ratio);
        md = exp(-pow((sp - dm) / (0.15 * hw + 1.0), 2.0)) * smoothstep(0.6, 0.0, abs(dx) / (0.55 * R));
    }
    col = col + c_white * (dia * beam * 0.8 + md * 0.9) * (0.4 + 0.6 * dec);
    // a clean flame shows less of a body (the diamonds and the hot core are what you see)
    let see = mix(0.45, 1.0, soot);
    var rgb = col * see * fade_in * inten * airf;
    // light thrown off into the air round the exit and along the plume
    let halo = exp(-pow(dx / (2.2 * R + 10.0), 2.0)) * exp(-sp / (0.5 * Lc + 80.0)) * fade_in * inten * airf;
    rgb = rgb + mix(c_body, c_core, 0.4) * halo * 0.22 * see;
    // (the gas simulation's own picture is hidden in a wide area round the plume, up beside the nozzle too)
    var wide = smoothstep(12.0, 8.0, abs(dx) / R) * smoothstep(-170.0, -120.0, s);
    if (s < 0.0) { wide = wide * smoothstep(hw, hw * 1.1, abs(dx)); } // (not inside the bell)
    let mask = wide * smoothstep(0.03, 0.2, inten);
    return vec4<f32>(rgb, mask);
}

fn h11(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

// Streaks of air rushing past the vehicle: flying up they run down the screen, falling they run up. Fainter
// where the air is thin, quicker the faster it goes.
fn airstream(cf: vec2<f32>) -> f32 {
    let wv = anim[A_AIR];
    let dens = clamp(anim[A_AIR + 1u] * 3.0, 0.0, 1.0);
    if (abs(wv) < 3.0 || dens < 0.01) { return 0.0; }
    let sp = clamp(abs(wv) / 250.0, 0.15, 2.0);
    let ph_air = anim[A_AIR + 2u];
    let dir = select(-1.0, 1.0, wv > 0.0);
    let colw = 6.0;
    let cx = floor(cf.x / colw);
    let r0 = h11(cx);
    let r1 = h11(cx * 1.7 + 4.1);
    let r2 = h11(cx * 2.3 + 9.7);
    let xo = fract(cf.x / colw) * colw - (1.0 + 4.0 * r0);
    let width = max(0.35, 1.0 / r.scale);
    let line = smoothstep(width, 0.0, abs(xo));
    let period = 80.0 + 90.0 * r2;
    let y = cf.y - (60.0 + 90.0 * r1) * ph_air;
    let ph = fract(y / period + r1);
    let len = (14.0 + 30.0 * sp) / period;
    if (ph > len) { return 0.0; }
    let fade = select(1.0 - ph / len, ph / len, dir > 0.0);
    return line * fade * 0.5 * dens;
}

// The hot gas leaving a burner block: a jet that comes straight out of its outlet face, filling whatever
// passage it opens into and fading with distance, in the colours of the flame inside.
fn burner_jets(cf: vec2<f32>) -> vec3<f32> {
    let n = min(u32(anim[A_BURN]), 8u);
    var add = vec3<f32>(0.0);
    let tint_c = vec3<f32>(r.tint_r, r.tint_g, r.tint_b);
    let fuel_c = vec3<f32>(r.lf_r, r.lf_g, r.lf_b);
    for (var q = 0u; q < n; q = q + 1u) {
        let k = 256u + 8u * u32(anim[A_BURN + 1u + q]);
        let inten = clamp(anim[k + 7u], 0.0, 1.0);
        if (inten < 0.03) { continue; }
        let x0 = anim[k];
        let y0 = anim[k + 1u];
        let bw = max(anim[k + 2u], 1.0);
        let bh = max(anim[k + 3u], 1.0);
        let dir = anim[k + 4u];
        let horiz = anim[k + 5u] > 0.5;
        var temp = anim[k + 6u];
        if (temp > 1.5) { temp = temp - 2.0; }
        // s: cells beyond the outlet face, cr: cells off the block's centre line, half: the block's half width
        var s = 0.0;
        var cr = 0.0;
        var half = 1.0;
        if (horiz) {
            half = bh * 0.5;
            cr = cf.y - (y0 + half);
            if (dir > 0.0) { s = cf.x - (x0 + bw); } else { s = x0 - cf.x; }
        } else {
            half = bw * 0.5;
            cr = cf.x - (x0 + half);
            if (dir > 0.0) { s = cf.y - (y0 + bh); } else { s = y0 - cf.y; }
        }
        let reach = 10.0 + 1.6 * half;
        if (s < 0.0 || s > reach) { continue; }
        let width = half * 0.36 + 0.1 * s;
        let core = smoothstep(width, 0.0, abs(cr));
        let fade = pow(1.0 - s / reach, 1.2);
        let flick = 0.6 + 0.8 * fbm(vec2<f32>(s * 0.55 - r.time * 7.0, cr * 0.6 + r.time));
        let hot = mix(mix(tint_c, fuel_c, 0.3), mix(tint_c, vec3<f32>(1.0), 0.6), core);
        add = add + hot * core * fade * flick * inten * (1.0 + 0.4 * temp);
    }
    return add;
}

// Side-view cutaway of a pump or turbine block. a runs along the shaft (0..1), p across it (-1..1).
fn turbomachine(t: u32, cf: vec2<f32>, k: u32, pp: u32) -> vec3<f32> {
    let x0 = anim[k];
    let y0 = anim[k + 1u];
    let bw = max(anim[k + 2u], 1.0);
    let bh = max(anim[k + 3u], 1.0);
    let th = anim[k + 4u];
    let horiz = anim[k + 5u] > 0.5;
    let stages = max(anim[k + 6u], 1.0);
    var a = (cf.x - x0) / bw;
    var p = (cf.y - y0) / bh * 2.0 - 1.0;
    if (!horiz) {
        a = (cf.y - y0) / bh;
        p = (cf.x - x0) / bw * 2.0 - 1.0;
    }
    let ap = abs(p);
    let shade = 1.0 - 0.45 * ap * ap;               // round casing lighting
    let metal = vec3<f32>(0.62, 0.64, 0.70);
    // casing wall and outline
    if (ap > 0.9) {
        return metal * 0.45 * shade + vec3<f32>(0.08) * step(0.97, ap);
    }
    // shaft through the middle
    if (ap < 0.07) {
        return metal * (0.95 - 0.3 * ap / 0.07);
    }
    let s = fract(a * stages);
    if (t == 28u) {
        // electric motor: copper stator windings round the casing, a laminated rotor spinning inside
        if (ap > 0.55) {
            let turn = smoothstep(0.35, 0.5, fract(a * 9.0)) * smoothstep(0.95, 0.8, fract(a * 9.0));
            return mix(vec3<f32>(0.45, 0.25, 0.10), vec3<f32>(0.85, 0.50, 0.22), turn) * shade;
        }
        if (ap > 0.5) {
            return vec3<f32>(0.05);                    // the air gap
        }
        let lam = smoothstep(0.4, 0.6, fract(p * 3.0 + th * 6.0));
        return mix(vec3<f32>(0.30, 0.34, 0.40), vec3<f32>(0.55, 0.60, 0.68), lam) * shade;
    }
    if (t == 18u) {
        // turbine: hub disc with alternating rotor / stator blade rows, glowing hot
        let hot = vec3<f32>(1.0, 0.45, 0.12);
        if (ap < 0.38) {
            return metal * 0.7 * shade;
        }
        let row = floor(s * 2.0);                     // 0 = stator nozzles, 1 = rotor blades
        let gap = smoothstep(0.0, 0.08, s) * smoothstep(1.0, 0.92, s);
        var phase = p * 7.0 + a * 3.0;
        if (row > 0.5) { phase = p * 7.0 - a * 3.0 - th * 9.0; }
        let blade = smoothstep(0.55, 0.7, fract(phase));
        let gas = hot * (0.35 + 0.25 * (1.0 - ap));
        return mix(gas, metal * (0.9 + 0.2 * row), blade * gap) * shade;
    }
    // pump: inducer spiral at the inlet, then impeller stages with diffusers between them
    let fluid = liquid(pp) * 0.8;
    // the liquid itself: bright dashes of it racing along the passages and flung outwards off the impeller,
    // faster as the shaft speeds up (the shaft angle carries the phase, so a change of speed never makes it jump)
    let spd = clamp(anim[k + 7u], 0.0, 1.5);
    let wet_c = fluid * 1.5 + vec3<f32>(0.18);
    if (a < 0.07) {
        let helix = smoothstep(0.6, 0.75, fract(p * 2.5 + a * 25.0 - th * 4.0));
        let lane = floor(p * 8.0);
        let dash = smoothstep(0.55, 0.85, fract(a * 14.0 - th * 2.0 + h11(lane) * 3.0)) * smoothstep(0.45, 0.15, abs(fract(p * 8.0) - 0.5));
        return mix(mix(fluid * 0.8, metal, helix), wet_c, dash * clamp(spd * 2.0, 0.0, 1.0)) * shade;
    }
    if (s > 0.8 || s < 0.06) {
        // stationary diffuser / crossover passage between stages
        let vane = smoothstep(0.65, 0.8, fract(p * 5.0 + s * 4.0));
        let lane = floor(p * 6.0 + 20.0);
        let dash = smoothstep(0.5, 0.85, fract(s * 10.0 - th * 1.6 + h11(lane) * 2.0)) * smoothstep(0.4, 0.1, abs(fract(p * 6.0) - 0.5));
        return mix(mix(fluid * 0.55, metal * 0.55, vane), wet_c * 0.9, dash * clamp(spd * 2.0, 0.0, 1.0)) * shade;
    }
    let u = (s - 0.06) / 0.74;                        // 0 = impeller eye, 1 = impeller outlet
    let hub = 0.1 + 0.32 * u * u;                     // hub disc grows towards the outlet
    let shroud = 0.45 + 0.4 * u;                      // passage widens out to the tip
    if (ap < hub) {
        return metal * 0.8 * shade;
    }
    if (ap > shroud) {
        return fluid * 0.4 * shade + metal * 0.15;    // volute around the impeller
    }
    // moving impeller blades seen edge-on, curved backwards
    let blade = smoothstep(0.6, 0.72, fract(p * 4.0 - u * 1.5 - th * 7.0));
    var res = mix(fluid * (0.75 + 0.25 * u), metal * 1.05, blade);
    // liquid thrown outwards from the eye to the tip, in streaks that bend with the rotation, and spray off the rim
    let lane = floor(u * 14.0 + 3.0);
    let streak = smoothstep(0.5, 0.05, abs(fract(u * 14.0) - 0.5));
    let dash = smoothstep(0.2, 0.9, fract(ap * 2.6 - th * 1.8 + h11(lane) * 4.0 + 0.5 * sin(u * 9.0 + h11(lane) * 6.0)));
    res = mix(res, wet_c, streak * dash * 0.85 * clamp(spd * 2.0, 0.0, 1.0));
    let spray = smoothstep(0.82, 0.98, fract(ap * 3.0 - th * 2.4 + h11(floor(u * 30.0)) * 5.0)) * smoothstep(0.55, 0.8, ap / shroud);
    res = res + wet_c * spray * 0.5 * clamp(spd * 2.0, 0.0, 1.0);
    return res * shade;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let pix = in.uv * vec2<f32>(r.rw, r.rh);
    let cf = vec2<f32>(r.cx, r.cy) + (pix - 0.5 * vec2<f32>(r.rw, r.rh)) / r.scale;
    if (cf.x < 0.0 || cf.y < 0.0 || cf.x >= f32(r.w) || cf.y >= f32(r.h)) {
        return vec4<f32>(0.02, 0.02, 0.03, 1.0);
    }
    let ix = u32(cf.x);
    let iy = u32(cf.y);
    let i = iy * r.w + ix;
    let m = mat[i];
    let t = m & 255u;
    let U = st[i];
    var col: vec3<f32>;
    if (t == 0u) {
        let rho = max(U.x, 1e-6);
        let u = U.y / rho;
        let v = U.z / rho;
        let sp = sqrt(u * u + v * v);
        let e = max(U.w / rho - 0.5 * sp * sp, 1.0);
        let pr = (r.gamma - 1.0) * rho * e;
        let T = e * (r.gamma - 1.0) / r.rgas;
        let c = sqrt(r.gamma * pr / rho);
        var s = 0.0;
        switch r.mode {
            case 0u: { s = 0.45 * clamp((log2(max(pr, 1.0)) / log2(10.0) - 2.0) / 5.5, 0.0, 1.0); }
            case 1u: { s = clamp((log2(max(pr, 1.0)) / log2(10.0) - 2.0) / 5.5, 0.0, 1.0); }
            case 2u: { s = clamp(sp / 5500.0, 0.0, 1.0); }
            case 3u: { s = clamp(sp / c / 5.0, 0.0, 1.0); }
            case 4u: { s = clamp((T - 150.0) / 3700.0, 0.0, 1.0); }
            default: { s = clamp((log2(rho) / log2(10.0) + 3.0) / 5.0, 0.0, 1.0); }
        }
        if (r.mode == 6u) {
            col = flame(i, ix, iy, rho, T, vec2<f32>(u, v), cf);
        } else {
            col = cmap(s);
        }
        if (r.mode == 0u || r.mode == 6u) {
            // the wind past the vehicle (not over the plume, shocks and anything else that isn't moving with it)
            let wv = anim[A_AIR];
            let still = (1.0 - smoothstep(350.0, 600.0, T)) * (1.0 - smoothstep(150.0, 450.0, length(vec2<f32>(u, v - wv))));
            col = col + vec3<f32>(0.75, 0.85, 1.0) * airstream(cf) * still;
        }
        if (r.mode == 0u) {
            // unburnt fuel vapour shows as a green-white haze (the flame view draws it as speckles)
            let unb = clamp(spec[i] / rho, 0.0, 1.0);
            col = mix(col, vec3<f32>(0.5, 0.8, 0.45), unb * 0.7 * smoothstep(0.003, 0.5, rho));
        }
        // velocity ticks when zoomed in
        if (r.scale > 9.0 && (r.mode == 2u || r.mode == 3u) && sp > 30.0) {
            let f = fract(cf) - vec2<f32>(0.5, 0.5);
            let d = vec2<f32>(u, v) / sp;
            let along = dot(f, d);
            let perp = abs(f.x * d.y - f.y * d.x);
            let len = 0.12 + 0.3 * clamp(sp / 5500.0, 0.0, 1.0);
            if (abs(along) < len && perp < 1.2 / r.scale) { col = vec3<f32>(0.9, 0.95, 1.0); }
        }
    } else {
        col = base_color(t);
        if (t == 16u) {
            // gas generator head: alternating oxidiser and fuel injector elements
            let ox_el = ((ix / 2u + iy / 2u) % 2u) == 0u;
            col = mix(col, select(liquid(0u), liquid(1u), ox_el), 0.6);
        }
        // hollow plumbing: liquid inside where it has reached, empty otherwise
        let net = (m >> 8u) & 255u;
        if (t == 4u || t == 10u || t == 11u || t == 20u || t == 27u) {
            var wet = false;
            if (net < 255u) { wet = phase[i] <= anim[A_FRONT + net]; }
            if (wet) {
                let f = fract((phase[i] - anim[net]) / 9.0);
                col = liquid(net_prop(net)) * (0.78 + 0.3 * smoothstep(0.7, 1.0, f));
            } else {
                col = EMPTY_PIPE;
            }
            if (t == 11u) {
                col = vec3<f32>(0.8, 0.18, 0.14);                 // shut valve gate
            } else if (t == 10u) {
                col = mix(col, vec3<f32>(0.25, 0.85, 0.35), 0.35); // open valve
            } else if (t == 27u) {
                // throttle valve: amber body with a metering gate across it
                let gate = abs(fract(cf.x + cf.y) - 0.5) < 0.12;
                col = mix(col, vec3<f32>(0.95, 0.66, 0.2), select(0.35, 0.85, gate));
            } else if (t == 20u) {
                let d = length(fract(cf) - vec2<f32>(0.5, 0.5));
                if (d > 0.16 && d < 0.32) { col = vec3<f32>(0.95, 0.95, 0.7); } // joint ring
            }
        } else if (t == 17u) {
            // helium bottle: pale pressurised gas that thins out to an empty interior as it vents
            let bottle = (m >> 16u) & 255u;
            var f = 1.0;
            if (bottle < 64u) { f = clamp(anim[A_HE + bottle], 0.0, 1.0); }
            let n = hash3(ix / 2u, iy / 2u, u32(r.time * 6.0));
            var gas_c = vec3<f32>(0.82, 0.85, 0.94);
            if (bottle < 64u && anim[A_TEA + 3u + bottle] > 0.5) { gas_c = vec3<f32>(0.45, 0.92, 0.55); } // a TEA-TEB bottle is green
            col = mix(EMPTY_PIPE, gas_c * (0.92 + 0.08 * n), f);
        } else if (t == 6u || t == 7u) {
            // tank: liquid below its level, ullage gas above, sloshing a little
            var level = 1.0e9;
            if (net < 255u) { level = anim[A_TANK + net] + 0.4 * sin(cf.x * 0.35 + r.time * 1.6); }
            var vap = 0.1;
            var boil = 0.0;
            if (net < 255u) {
                vap = anim[A_VAP + net];
                boil = anim[A_BOIL + net];
            }
            let liq = liquid(tank_prop(t));
            if (cf.y >= level) {
                col = liq * (0.9 - 0.2 * clamp((cf.y - level) / 40.0, 0.0, 1.0));
                // bubbles rising just under the surface while it boils
                if (boil > 0.0 && cf.y - level < 6.0 && hash3(ix, iy + u32(r.time * 9.0), 5u) > 0.985) {
                    col = mix(col, vec3<f32>(0.95, 0.97, 1.0), 0.7);
                }
            } else {
                // ullage: boil-off vapour swirling and drifting up, denser as the pressure climbs
                let q = cf * 0.11 + vec2<f32>(r.time * 0.12, r.time * 0.35);
                let cloud = fbm(q + vec2<f32>(fbm(q * 0.7 + r.time * 0.05) * 1.5, 0.0));
                col = liq * (0.1 + 0.55 * vap) * (0.45 + 0.75 * cloud);
                // droplets flicking off the surface
                if (boil > 0.0 && level - cf.y < 3.0 && hash3(ix, iy, u32(r.time * 14.0)) > 0.993) {
                    col = vec3<f32>(0.9, 0.95, 1.0);
                }
            }
        }
        // turbopumps drawn as side-view cutaways along their real shaft axis
        let b = (m >> 16u) & 255u;
        if ((t == 8u || t == 18u || t == 28u) && b < 64u) {
            col = turbomachine(t, cf, 256u + 8u * b, net_prop(net));
        }
        if (t == 31u && b < 64u) {
            col = burner_cutaway(cf, 256u + 8u * b);
        }
        let T = U.w;
        if (r.show_heat != 0u) {
            var k = smoothstep(350.0, 1500.0, T);
            if (t == 4u || t == 5u) { k = 0.6 * smoothstep(40.0, 700.0, T); }
            col = mix(col, vec3<f32>(1.0, 0.35, 0.05), k);
            col = col + vec3<f32>(0.5, 0.35, 0.1) * smoothstep(1200.0, 1700.0, T);
            col = col + vec3<f32>(0.45, 0.45, 0.4) * smoothstep(1700.0, 2400.0, T); // white-hot (niobium skirts)
        }
    }
    // flame view: hot gas glows over walls and past the plume edges
    if (r.mode == 6u) {
        col = col + glow_at(cf);
    }
    // the hot gas a burner block sends out of its outlet
    if ((r.mode == 6u || r.mode == 0u) && (mat[i] & 255u) == 0u) {
        let pl = plume(cf);
        col = col * (1.0 - 0.95 * pl.a) + pl.rgb;
        // TEA-TEB burns green: a flash round the injector it sprays into
        let tea_k = anim[A_TEA + 2u];
        if (tea_k > 0.01) {
            let d = length(cf - vec2<f32>(anim[A_TEA], anim[A_TEA + 1u]));
            let flick = 0.7 + 0.5 * fbm(vec2<f32>(d * 0.4 - r.time * 14.0, r.time * 3.0));
            let g = exp(-pow(d / (7.0 + 8.0 * tea_k), 2.0)) * tea_k * flick;
            col = col + vec3<f32>(0.2, 1.0, 0.35) * g * 1.4 + vec3<f32>(0.7, 1.0, 0.8) * g * g * 0.8;
        }
        // inside the bell the hot gas is coloured by the same palette, from the throat to the exit
        if (anim[A_PLUME] > 0.5) {
            let yt = anim[A_PLUME + 9u];
            let ye = anim[A_PLUME + 2u];
            let Ui = st[i];
            let rho_i = max(Ui.x, 1e-6);
            let ei = max(Ui.w / rho_i - 0.5 * (Ui.y * Ui.y + Ui.z * Ui.z) / (rho_i * rho_i), 1.0);
            let Ti = ei * (r.gamma - 1.0) / r.rgas;
            if (cf.y > yt + 2.0 && cf.y < ye && Ti > 1300.0) {
                let ub = clamp((cf.y - yt) / max(ye - yt, 1.0), 0.0, 1.0);
                let lum = clamp(dot(col, vec3<f32>(0.35, 0.5, 0.15)) * 1.15, 0.0, 1.2);
                col = mix(col, exhaust_pal(0.05 + 0.4 * ub) * lum, 0.9);
            }
        }
        col = col + burner_jets(cf);
    }
    // pipes behind the engine plane show through gas; pipes in front cover everything
    let bk = layer_pipe(1u, i, ix, iy, cf);
    if (bk.a > 0.0) {
        if (t == 0u) {
            col = min(col + bk.rgb * 0.6, vec3<f32>(1.0));
        } else if (r.edit_layer == 1u) {
            col = mix(col, bk.rgb, 0.55);
        }
    }
    let fl = layer_pipe(2u, i, ix, iy, cf);
    if (fl.a > 0.0) {
        var a = 0.92;
        if (r.edit_layer == 1u) { a = 0.3; }
        col = mix(col, fl.rgb, a);
    }
    if (r.scale > 12.0) {
        let g = fract(cf);
        let e = min(min(g.x, 1.0 - g.x), min(g.y, 1.0 - g.y));
        col = col * (0.85 + 0.15 * smoothstep(0.0, 0.06, e));
    }
    return vec4<f32>(col, 1.0);
}
