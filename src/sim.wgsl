// 2D compressible Euler solver (Rusanov fluxes) + wall heat exchange + wall conduction.
// Cell state: gas  -> (rho, rho*u, rho*v, E)   solid -> (0, 0, 0, T)

struct P {
    w: u32, h: u32, nsrc: u32, q_heat: f32,
    dx: f32, dt: f32, depth: f32, gamma: f32,
    rgas: f32, accel: f32, h0: f32, amb_rho: f32,
    amb_t: f32, rho_min: f32, axi: u32, axis_x: f32,
    amb_k: f32, amb_v: f32, stab_y: f32, stab_k: f32,
    stab_r: f32, _d: f32, _e: f32, _f: f32,
}
struct Src { cell: u32, drho: f32, dmx: f32, dmy: f32, de: f32, p0: f32, p1: f32, p2: f32 }

@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> cur: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> nxt: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> mat: array<u32>;
@group(0) @binding(4) var<storage, read> net_t: array<f32>;
@group(0) @binding(5) var<storage, read_write> flux: array<f32>;
@group(0) @binding(6) var<storage, read> src: array<Src>;
// combustible (unburnt) gas density, kg/m^3: painted fuel vapour or cold-flowed propellant
@group(0) @binding(7) var<storage, read> spec_cur: array<f32>;
@group(0) @binding(8) var<storage, read_write> spec_nxt: array<f32>;
// Sleeping tiles: one flag per 8x8 tile, set when anything in the tile changed this step. A tile of
// still gas whose neighbourhood didn't change either is skipped (both state buffers already agree).
// The flags live after the last cell in the spec buffers, which ping-pong the same way.
fn act_cur(t: u32) -> bool { return spec_cur[p.w * p.h + t] != 0.0; }
fn set_act(t: u32, on: bool) { spec_nxt[p.w * p.h + t] = select(0.0, 1.0, on); }

// Per-tile step rates (local time stepping). Gas only needs a time step as short as the fastest wave in it
// allows: calm air far from the engine can take 2, 4 or 8 of the global steps at once. Each 8x8 tile keeps
// the rate it wants (from the fastest wave in it), when it last ran and, once for all, a step counter, in
// the spec buffers after the sleeping flags. A tile runs at the least of its own rate and twice its
// neighbours' (so rates change gently), and only when that much time has passed; in between it just
// carries its state over. Tiles with walls or machinery in them always run every step.
var<private> dtk: f32;
fn n_tiles() -> u32 { return ((p.w + 7u) / 8u) * ((p.h + 7u) / 8u); }
fn rate_cur(t: u32) -> f32 { return max(spec_cur[p.w * p.h + n_tiles() + t], 1.0); }
fn last_cur(t: u32) -> f32 { return spec_cur[p.w * p.h + 2u * n_tiles() + t]; } // the step count its state is up to
fn clock_cur() -> f32 { return spec_cur[p.w * p.h + 3u * n_tiles()]; }
const CLOCK_WRAP: f32 = 1048576.0;
const CFL_TILE: f32 = 0.4;

var<workgroup> wg_wake: atomic<u32>;
var<workgroup> wg_solid: atomic<u32>;
var<workgroup> wg_nbr: atomic<u32>;   // least rate any neighbouring tile wants
var<workgroup> wg_speed: atomic<u32>; // fastest wave in the tile (f32 bits; positive floats sort as u32)
var<workgroup> wg_mode: u32;          // 0 asleep, 1 run, 2 carry over
var<workgroup> wg_k: f32;             // global steps this update covers
var<workgroup> wg_copy: u32;          // carrying over: the other buffer still holds the state before the last update
var<workgroup> wg_chg: atomic<u32>;
var<workgroup> wg_run: u32;

fn tile_of(i: u32) -> u32 {
    return ((i / p.w) / 8u) * ((p.w + 7u) / 8u) + (i % p.w) / 8u;
}

const HC: f32 = 60000.0;     // coolant film coefficient, W/m^2K
const VMAX: f32 = 6000.0;
const CMAX: f32 = 4000.0;
const TMIN: f32 = 10.0;
const SPONGE: i32 = 24; // absorbing border width, cells
const STILL_AIR_K: f32 = 0.003; // quiet air thinned by the jet's pull relaxes to ambient at this fraction a base step

// effective slab depth of a cell: constant, or 2*pi*r for a round (axisymmetric) engine
fn depth_eff(x: i32) -> f32 {
    if (p.axi == 1u) { return 6.2831853 * abs(f32(x) + 0.5 - p.axis_x) * p.dx; }
    return p.depth;
}

fn ctype(i: u32) -> u32 { return mat[i] & 255u; }
fn netid(i: u32) -> u32 { return (mat[i] >> 8u) & 255u; }
// propellant-cooled cells: pipes (and their kin), and injectors, whose face the propellant flowing through
// keeps cool (the walls between injector elements lose their heat into them)
fn is_sink(t: u32) -> bool { return t == 4u || t == 5u || t == 14u || t == 34u; }
// ... but only while liquid is in them: bit 24 of the material word marks a dry one (the liquid hasn't got
// there, or the tank is empty), which is just a cavity in the wall that conducts a little and holds its own heat
fn sinkc(i: u32) -> bool { return is_sink(ctype(i)) && ((mat[i] >> 24u) & 1u) == 0u; }

const NIOBIUM: u32 = 24u;
const SIGMA_EPS: f32 = 4.8e-8;  // Stefan-Boltzmann x emissivity of a silicide-coated niobium skirt (~0.85)

fn kcond(t: u32) -> f32 {
    if (is_sink(t)) { return 3.0; } // a dry pipe or injector: an empty passage
    if (t == 2u) { return 380.0; }
    if (t == 3u) { return 3.0; }
    if (t == NIOBIUM) { return 50.0; }
    return 30.0;
}
fn rcap(t: u32) -> f32 {
    if (is_sink(t)) { return 3.0e5; }
    if (t == 2u) { return 3.4e6; }
    if (t == 3u) { return 2.7e6; }
    if (t == NIOBIUM) { return 2.3e6; }
    return 4.0e6;
}

fn pressure(U: vec4<f32>) -> f32 {
    let r = max(U.x, p.rho_min);
    let ke = 0.5 * (U.y * U.y + U.z * U.z) / r;
    return max((p.gamma - 1.0) * (U.w - ke), r * p.rgas * TMIN);
}

fn gas_temp(U: vec4<f32>) -> f32 {
    let r = max(U.x, p.rho_min);
    let e = U.w / r - 0.5 * (U.y * U.y + U.z * U.z) / (r * r);
    return max(e * (p.gamma - 1.0) / p.rgas, TMIN);
}

// A cell state with its velocity, pressure and sound speed worked out once (each state takes part in
// two faces, and each face needs the flux and the wave speed of both sides).
struct Prim { U: vec4<f32>, u: f32, v: f32, pr: f32, c: f32 }

fn prim(U: vec4<f32>) -> Prim {
    let r = max(U.x, p.rho_min);
    let pr = pressure(U);
    return Prim(U, U.y / r, U.z / r, pr, sqrt(p.gamma * pr / r));
}

fn flux_p(a: Prim, horiz: bool) -> vec4<f32> {
    if (horiz) {
        return vec4<f32>(a.U.y, a.U.y * a.u + a.pr, a.U.z * a.u, (a.U.w + a.pr) * a.u);
    }
    return vec4<f32>(a.U.z, a.U.y * a.v, a.U.z * a.v + a.pr, (a.U.w + a.pr) * a.v);
}

fn speed_p(a: Prim, horiz: bool) -> f32 {
    return min(abs(select(a.v, a.u, horiz)), VMAX) + min(a.c, CMAX);
}

fn rusanov_p(a: Prim, b: Prim, horiz: bool) -> vec4<f32> {
    let s = max(speed_p(a, horiz), speed_p(b, horiz));
    return 0.5 * (flux_p(a, horiz) + flux_p(b, horiz)) - 0.5 * s * (b.U - a.U);
}

// State of the neighbour as seen from gas cell (x,y) holding U. Walls mirror the normal momentum.
fn nbr(x: i32, y: i32, U: vec4<f32>, dx: i32, dy: i32) -> vec4<f32> {
    let nx = x + dx;
    let ny = y + dy;
    if (nx < 0 || ny < 0 || nx >= i32(p.w) || ny >= i32(p.h)) { return U; }
    let j = u32(ny) * p.w + u32(nx);
    if (ctype(j) == 0u) { return cur[j]; }
    var G = U;
    if (dx != 0) { G.y = -U.y; } else { G.z = -U.z; }
    return G;
}

// convective heat transfer coefficient from a gas cell state
fn film_h(U: vec4<f32>) -> f32 {
    let r = max(U.x, p.rho_min);
    let sp = length(vec2<f32>(U.y, U.z)) / r;
    let T = gas_temp(U);
    let c = sqrt(p.gamma * p.rgas * T);
    return p.h0 * pow(max(r * (sp + 0.1 * c), 1e-6), 0.8);
}

fn wall_temp(j: u32) -> f32 {
    if (sinkc(j)) { return net_t[netid(j)]; }
    return cur[j].w;
}

// combustible mass fraction of a neighbour (walls and the domain edge reuse our own)
fn ynb(x: i32, y: i32, dx: i32, dy: i32, yself: f32) -> f32 {
    let nx = x + dx;
    let ny = y + dy;
    if (nx < 0 || ny < 0 || nx >= i32(p.w) || ny >= i32(p.h)) { return yself; }
    let j = u32(ny) * p.w + u32(nx);
    if (ctype(j) != 0u) { return yself; }
    return clamp(spec_cur[j] / max(cur[j].x, p.rho_min), 0.0, 1.0);
}

fn gas_update(x: i32, y: i32, i: u32) {
    let U = cur[i];
    let P0 = prim(U);
    let fl = rusanov_p(prim(nbr(x, y, U, -1, 0)), P0, true);
    let fr = rusanov_p(P0, prim(nbr(x, y, U, 1, 0)), true);
    let gu = rusanov_p(prim(nbr(x, y, U, 0, -1)), P0, false);
    let gd = rusanov_p(P0, prim(nbr(x, y, U, 0, 1)), false);
    var N = U - (dtk / p.dx) * (fr - fl + gd - gu);
    if (p.axi == 1u) {
        // geometric source terms of the Euler equations about the vertical axis
        let r = (f32(x) + 0.5 - p.axis_x) * p.dx;
        let rho = max(U.x, p.rho_min);
        let u = U.y / rho;
        let v = U.z / rho;
        let pr = pressure(U);
        N = N - (dtk / r) * vec4<f32>(rho * u, rho * u * u, rho * u * v, (U.w + pr) * u);
    }

    // heat exchange with adjacent solids
    let Tg = gas_temp(U);
    let h = film_h(U);
    for (var k = 0; k < 4; k = k + 1) {
        var dx = 0; var dy = 0;
        if (k == 0) { dx = 1; } else if (k == 1) { dx = -1; } else if (k == 2) { dy = 1; } else { dy = -1; }
        let nx = x + dx; let ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= i32(p.w) || ny >= i32(p.h)) { continue; }
        let j = u32(ny) * p.w + u32(nx);
        if (ctype(j) != 0u) {
            N.w = N.w - h * (Tg - wall_temp(j)) * dtk / p.dx;
        }
    }

    // the atmosphere follows the altitude: quiet air at ambient temperature anywhere in the world relaxes to the
    // ambient density while the altitude is changing (the plume, shocks and hot gas are left alone)
    //
    // And at any time, quiet air that is at or below the ambient pressure is topped up, slowly. A jet drags the air beside it
    // away with it; in the open that air is replaced from all round, but in this flat slice of the world it can only come in
    // past the jet from the edges, and for a wide exhaust it cannot come fast enough: the air beside the jet thins, the jet
    // balloons out into it, air rushes back, and the whole exhaust breathes in and out. (Gas shut in a vessel above the
    // ambient pressure is left alone.)
    {
        let rq = max(N.x, p.rho_min);
        let uq = N.y / rq;
        let vq = N.z / rq;
        let Tq = gas_temp(N);
        // (the wind past the vehicle is quiet air too: it is pulled towards the wind speed)
        let dvq = vq - p.amb_v;
        let thinned = rq * Tq < 1.3 * p.amb_rho * p.amb_t;
        let rate = max(p.amb_k, select(0.0, STILL_AIR_K, thinned));
        if (rate > 0.0 && uq * uq + dvq * dvq < 1600.0 && Tq > 180.0 && Tq < 420.0) {
            let kq = 1.0 - pow(1.0 - rate, dtk / p.dt);
            let raq = p.amb_rho;
            let vt = vq + (p.amb_v - vq) * 0.5;
            let ambq = vec4<f32>(raq, raq * uq, raq * vt, raq * p.rgas / (p.gamma - 1.0) * p.amb_t + 0.5 * raq * (uq * uq + vt * vt));
            N = mix(N, ambq, kq);
        }
    }

    // the wind carries thin, slow hot gas too (what the plume has left behind and its ragged edges), so
    // it streams past the vehicle instead of hanging in the air; the fast plume and the dense chamber gas
    // are left alone
    if (p.amb_k > 0.0 && abs(p.amb_v) > 1.0) {
        let rw = max(N.x, p.rho_min);
        let uw = N.y / rw;
        let vw = N.z / rw;
        let rel = length(vec2<f32>(uw, vw - p.amb_v));
        let thin = 1.0 - smoothstep(2.0 * p.amb_rho, 6.0 * p.amb_rho, rw);
        let wgt = thin * (1.0 - smoothstep(60.0, 180.0, rel));
        let kd = (1.0 - pow(1.0 - 0.02, dtk / p.dt)) * wgt;
        N.y = N.y - kd * N.y;
        N.z = N.z + kd * (rw * p.amb_v - N.z);
    }

    // absorbing border: within SPONGE cells of the edge of the world the gas relaxes to still ambient air,
    // so exhaust flows off-screen instead of reflecting back and filling the space
    var sponge = 0.0;
    let edge = min(min(x, i32(p.w) - 1 - x), min(y, i32(p.h) - 1 - y));
    if (edge < SPONGE) {
        let f = 1.0 - f32(edge) / f32(SPONGE);
        sponge = 1.0 - pow(1.0 - 0.03 * f * f, dtk / p.dt); // the same relaxation per unit time at any step rate
        let r0 = max(N.x, p.rho_min);
        let u = N.y / r0;
        let v = N.z / r0;
        let ra = p.amb_rho;
        // The edges act like still open air. Only the bottom, where the exhaust leaves, lets gas keep going,
        // and only outwards: air coming back up through it would rise up the sides and swirl the smoke round
        // in a loop. At the top the air settles still; at the sides drifting up or down settles out, while
        // gas can still flow in or out sideways. So what the plume drags along comes from above and beside
        // it, and goes down with it, as in open air.
        var uu = u;
        var vv = v;
        let damp = max(1.0 - 0.3 * f * f * dtk / p.dt, 0.0);
        let vt = p.amb_v;
        if (i32(p.h) - 1 - y == edge) {
            if (v < vt) { vv = vt + (v - vt) * damp; }
            else if (vt < 0.0 && gas_temp(N) < 450.0) { vv = v + (vt - v) * (1.0 - damp); } // wind coming up from below
        } else {
            vv = vt + (v - vt) * damp;
            if (y == edge) { uu = u * damp; }
        }
        // slow the gas itself (the kinetic energy it loses goes with it), then relax density and temperature
        // to ambient
        N.w = N.w - 0.5 * r0 * (u * u + v * v - uu * uu - vv * vv);
        N.y = r0 * uu;
        N.z = r0 * vv;
        let amb = vec4<f32>(ra, ra * uu, ra * vv, ra * p.rgas / (p.gamma - 1.0) * p.amb_t + 0.5 * ra * (uu * uu + vv * vv));
        N = mix(N, amb, sponge);
    }

    // jet stabiliser: a planar jet flaps from side to side (the sinuous mode, which a round jet has far less of), swinging the
    // whole exhaust about. Far enough below the nozzle, the sideways velocity of the fast gas moving down is damped, which
    // kills that mode; the lost kinetic energy stays in the gas as heat, as with a viscosity.
    if (p.stab_k > 0.0 && f32(y) > p.stab_y) {
        let rs = max(N.x, p.rho_min);
        let vs = N.z / rs;
        let jet = smoothstep(250.0, 700.0, vs) * smoothstep(0.0, max(p.stab_r, 1.0), f32(y) - p.stab_y);
        N.y = N.y * exp(-p.stab_k * dtk * jet);
    }

    // positivity / speed limits
    N.x = max(N.x, p.rho_min);
    let sp = length(vec2<f32>(N.y, N.z)) / N.x;
    if (sp > VMAX) { N.y = N.y * VMAX / sp; N.z = N.z * VMAX / sp; }
    let ke = 0.5 * (N.y * N.y + N.z * N.z) / N.x;
    let emin = ke + N.x * p.rgas / (p.gamma - 1.0) * TMIN;
    N.w = max(N.w, emin);
    // combustible gas rides the same face mass fluxes (upwind) and burns wherever the gas is hot
    let yc = clamp(spec_cur[i] / max(U.x, p.rho_min), 0.0, 1.0);
    let f_l = fl.x * select(yc, ynb(x, y, -1, 0, yc), fl.x > 0.0);
    let f_r = fr.x * select(ynb(x, y, 1, 0, yc), yc, fr.x > 0.0);
    let g_u = gu.x * select(yc, ynb(x, y, 0, -1, yc), gu.x > 0.0);
    let g_d = gd.x * select(ynb(x, y, 0, 1, yc), yc, gd.x > 0.0);
    var phi = clamp(spec_cur[i] - (dtk / p.dx) * (f_r - f_l + g_d - g_u), 0.0, N.x);
    // ignition: hot enough here, or touching a flame (fire spreads by contact)
    let t_here = gas_temp(N);
    var hot = t_here > 900.0;
    var t_nb = 0.0; // hottest flame next door
    for (var k = 0; k < 4 && phi > 0.0; k = k + 1) {
        var ddx = 0; var ddy = 0;
        if (k == 0) { ddx = 1; } else if (k == 1) { ddx = -1; } else if (k == 2) { ddy = 1; } else { ddy = -1; }
        let nx = x + ddx; let ny = y + ddy;
        if (nx < 0 || ny < 0 || nx >= i32(p.w) || ny >= i32(p.h)) { continue; }
        let j = u32(ny) * p.w + u32(nx);
        if (ctype(j) == 0u) {
            let tj = gas_temp(cur[j]);
            if (tj > 1200.0) { hot = true; t_nb = max(t_nb, tj); }
        }
        // a gas generator's injector plate anchors its flame: what it sprays lights right at the plate
        if (ctype(j) == 16u) { hot = true; t_nb = max(t_nb, 2350.0); }
    }
    if (phi > 0.0 && hot) {
        // burn rate rises steeply with temperature and with how much of the gas is burnable (as real
        // chemistry does): a dilute mix just above ignition, like a gas generator's jets, burns out over
        // several cells; a cloud of fuel vapour, or a ~3000 K chamber, burns in a few dozen steps
        let t_eff = max(t_here, 0.6 * t_nb);
        let rate_t = pow(clamp((t_eff - 850.0) / 1500.0, 0.005, 1.0), 1.5);
        let rate = mix(rate_t, 1.0, smoothstep(0.1, 0.5, phi / max(N.x, p.rho_min)));
        let burn = phi * min(1.0, 1.0e5 * dtk * rate);
        phi = phi - burn;
        N.w = N.w + burn * p.q_heat;
    }
    spec_nxt[i] = phi * (1.0 - sponge);
    nxt[i] = N;
}

fn solid_update(x: i32, y: i32, i: u32, t: u32) {
    var T = cur[i].w;
    let k = kcond(t);
    let cap = rcap(t) * p.dx * p.dx;       // per unit depth; accel shrinks effective capacity
    var dT = 0.0;
    for (var n = 0; n < 4; n = n + 1) {
        var dx = 0; var dy = 0;
        if (n == 0) { dx = 1; } else if (n == 1) { dx = -1; } else if (n == 2) { dy = 1; } else { dy = -1; }
        let nx = x + dx; let ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= i32(p.w) || ny >= i32(p.h)) { continue; }
        let j = u32(ny) * p.w + u32(nx);
        let tj = ctype(j);
        if (tj == 0u) {
            let h = film_h(cur[j]);
            dT = dT + h * (gas_temp(cur[j]) - T) * p.dx * p.dt * p.accel / cap;
            // a niobium skirt has no coolant: it radiates its heat away from every open face
            if (t == NIOBIUM) {
                let ta = p.amb_t;
                dT = dT - SIGMA_EPS * (T * T * T * T - ta * ta * ta * ta) * p.dx * p.dt * p.accel / cap;
            }
        } else if (sinkc(j)) {
            let gk = 1.0 / (1.0 / k + 1.0 / (HC * p.dx));
            dT = dT + gk * (net_t[netid(j)] - T) * p.dt * p.accel / cap;
        } else {
            let kj = kcond(tj);
            let kh = 2.0 * k * kj / (k + kj);
            dT = dT + kh * (cur[j].w - T) * p.dt * p.accel / cap;
        }
    }
    nxt[i] = vec4<f32>(0.0, 0.0, 0.0, T + dT);
    spec_nxt[i] = 0.0;
}

fn sink_update(x: i32, y: i32, i: u32) {
    let dep = depth_eff(x);
    let Ts = net_t[netid(i)];
    var J = 0.0;   // joules absorbed this step (real, not accelerated)
    for (var n = 0; n < 4; n = n + 1) {
        var dx = 0; var dy = 0;
        if (n == 0) { dx = 1; } else if (n == 1) { dx = -1; } else if (n == 2) { dy = 1; } else { dy = -1; }
        let nx = x + dx; let ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= i32(p.w) || ny >= i32(p.h)) { continue; }
        let j = u32(ny) * p.w + u32(nx);
        let tj = ctype(j);
        if (tj == 0u) {
            J = J + film_h(cur[j]) * (gas_temp(cur[j]) - Ts) * p.dx * dep * p.dt;
        } else if (!sinkc(j)) {
            let k = kcond(tj);
            let gk = 1.0 / (1.0 / k + 1.0 / (HC * p.dx));
            J = J + gk * dep * (cur[j].w - Ts) * p.dt;
        }
    }
    flux[i] = flux[i] + J;
    nxt[i] = vec4<f32>(0.0, 0.0, 0.0, Ts);
    spec_nxt[i] = 0.0;
}

@compute @workgroup_size(8, 8)
fn step(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    let tw = (p.w + 7u) / 8u;
    let th = (p.h + 7u) / 8u;
    let tile = wid.y * tw + wid.x;
    let nt = tw * th;
    let base = p.w * p.h;
    let now = clock_cur();
    if (li == 0u) {
        atomicStore(&wg_wake, 0u);
        atomicStore(&wg_chg, 0u);
        atomicStore(&wg_solid, 0u);
        atomicStore(&wg_nbr, 8u);
        atomicStore(&wg_speed, 0u);
    }
    if (tile == 0u && li == 0u) { spec_nxt[base + 3u * nt] = (now + 1.0) % CLOCK_WRAP; }
    workgroupBarrier();
    let inside = gid.x < p.w && gid.y < p.h;
    let i = gid.y * p.w + gid.x;
    // walls, plumbing and machinery always run (conduction, coolant heat pick-up), every step
    if (inside && ctype(i) != 0u) {
        atomicStore(&wg_wake, 1u);
        atomicStore(&wg_solid, 1u);
    }
    // wake if this tile or one around it changed last step; and the rates the tiles around want
    if (li < 9u) {
        let nx = i32(wid.x) + i32(li % 3u) - 1;
        let ny = i32(wid.y) + i32(li / 3u) - 1;
        if (nx >= 0 && ny >= 0 && nx < i32(tw) && ny < i32(th)) {
            let nb = u32(ny) * tw + u32(nx);
            if (act_cur(nb)) { atomicStore(&wg_wake, 1u); }
            if (nb != tile) { atomicMin(&wg_nbr, u32(rate_cur(nb))); }
        }
    }
    workgroupBarrier();
    if (li == 0u) {
        var mode = 0u;
        var k = 1.0;
        if (atomicLoad(&wg_wake) != 0u) {
            var rate = min(rate_cur(tile), 2.0 * f32(atomicLoad(&wg_nbr)));
            if (atomicLoad(&wg_solid) != 0u) { rate = 1.0; }
            // steps its state is behind where this step brings everything
            let since = (now + 1.0 - last_cur(tile) + CLOCK_WRAP) % CLOCK_WRAP;
            if (since >= rate || (now + 1.0) % rate == 0.0) {
                mode = 1u;
                // catch up, but never more than its rate allows (a wave arriving early costs the calm
                // tile a little time rather than its stability)
                k = clamp(since, 1.0, rate);
            } else {
                mode = 2u;
            }
            // only the first step after an update has to copy: from then on both buffers agree (sources
            // only ever feed gas in next to walls and machinery, whose tiles run every step)
            wg_copy = select(0u, 1u, since < 1.5);
        }
        wg_mode = mode;
        wg_k = k;
    }
    let mode = workgroupUniformLoad(&wg_mode);
    if (mode == 0u) {
        // asleep: both state buffers already agree; it counts as up to date
        if (li == 0u) {
            set_act(tile, false);
            spec_nxt[base + nt + tile] = spec_cur[base + nt + tile];
            spec_nxt[base + 2u * nt + tile] = (now + 1.0) % CLOCK_WRAP;
        }
        return;
    }
    if (mode == 2u) {
        // not its turn: carry the state over to the other buffer unchanged
        if (inside && workgroupUniformLoad(&wg_copy) != 0u) {
            nxt[i] = cur[i];
            spec_nxt[i] = spec_cur[i];
        }
        if (li == 0u) {
            set_act(tile, act_cur(tile));
            spec_nxt[base + nt + tile] = spec_cur[base + nt + tile];
            spec_nxt[base + 2u * nt + tile] = last_cur(tile);
        }
        return;
    }
    dtk = p.dt * wg_k;
    if (inside) {
        let t = ctype(i);
        let x = i32(gid.x);
        let y = i32(gid.y);
        let U = cur[i];
        var changed = false;
        if (t == 0u) {
            gas_update(x, y, i);
            let N = nxt[i];
            let d = abs(N - U);
            changed = d.x > 1e-5 * U.x + 1e-7
                || d.w > 1e-5 * U.w + 1e-3
                || d.y + d.z > 1e-5 * (abs(U.y) + abs(U.z)) + 1e-4 * U.x
                || abs(spec_nxt[i] - spec_cur[i]) > 1e-6 * U.x + 1e-9;
            let P = prim(N);
            atomicMax(&wg_speed, bitcast<u32>(max(abs(P.u), abs(P.v)) + P.c));
        } else if (sinkc(i)) {
            sink_update(x, y, i);
            changed = abs(nxt[i].w - U.w) > 1e-4;
        } else {
            solid_update(x, y, i, t);
            changed = abs(nxt[i].w - U.w) > 1e-4;
        }
        if (changed) { atomicStore(&wg_chg, 1u); }
    }
    workgroupBarrier();
    if (li == 0u) {
        set_act(tile, atomicLoad(&wg_chg) != 0u);
        // the rate this tile wants next: as many global steps at once as its fastest wave allows
        let s = bitcast<f32>(atomicLoad(&wg_speed));
        var r = 1.0;
        if (atomicLoad(&wg_solid) == 0u) {
            loop {
                if (r >= 8.0 || s * 2.0 * r * p.dt / p.dx > CFL_TILE) { break; }
                r = r * 2.0;
            }
        }
        spec_nxt[base + nt + tile] = r;
        spec_nxt[base + 2u * nt + tile] = (now + 1.0) % CLOCK_WRAP;
    }
}

@compute @workgroup_size(64)
fn sources(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.nsrc) { return; }
    let s = src[gid.x];
    // the cell may have been walled over since the source was set up (e.g. patching a leak): a wall
    // stores a temperature, not gas, so it must not receive the gas
    if (ctype(s.cell) != 0u) { return; }
    var N = nxt[s.cell] + vec4<f32>(s.drho, s.dmx, s.dmy, s.de);
    // sources can also take gas away (vents, gas lines, turbines): never below a sane floor
    N.x = max(N.x, p.rho_min);
    let ke = 0.5 * (N.y * N.y + N.z * N.z) / N.x;
    N.w = max(N.w, ke + N.x * p.rgas / (p.gamma - 1.0) * TMIN);
    nxt[s.cell] = N;
    spec_nxt[s.cell] = clamp(spec_nxt[s.cell] + s.p0, 0.0, N.x);
    set_act(tile_of(s.cell), true); // gas pouring in keeps its tile awake
}
