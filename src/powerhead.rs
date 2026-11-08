//! Powerhead maker: sizes the turbopumps, turbines and gas generator for a propellant pair, a chamber
//! pressure and a throat, and draws them as one piece that goes down like a paste.
//!
//! The sizing uses the same pump and turbine models the simulation runs: head per stage is
//! PSI * rho * u^2, a pump's best flow is PHI_BEP * eye area * u, and a turbine works best near its
//! optimum blade-to-gas speed ratio.

use crate::design::Clip;
use crate::prefabs::{stub, Board};
use crate::world::{c, t_flame, turbine_eff, turbine_opt, Settings, PHI_BEP, PROPS, PSI, U_TIP_MAX};
use std::f32::consts::PI;

/// The gas pressure at the fuel and the oxidiser injectors, against the chamber's average (measured on the standard chambers: the
/// fuel enters down the side, the oxidiser at the head end where the gas stagnates).
pub const LOCAL_F: f32 = 0.9;
pub const LOCAL_O: f32 = 1.25;

/// Design tip speed: well under the rated limit, so the governor has room above it.
const U_DESIGN: f32 = 0.62 * U_TIP_MAX;
/// Turbine drive gas as the simulation delivers it: spouting velocity against sqrt(R T) of the gas.
const C0_FACTOR: f32 = 1.08;
/// Share of the generator gas enthalpy the turbines actually turn into shaft power (ducting, leaving
/// losses, the gas that bypasses the blades).
const GG_YIELD: f32 = 0.10;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Layout {
    Auto,
    Single,
    Twin,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PhInput {
    pub prop: usize,
    pub pc: f32, // chamber pressure, Pa
    pub throat_cells: f32,
    pub layout: Layout,
    pub t_gg: f32, // turbine drive gas temperature, K
    pub dx: f32,
    pub depth: f32,
    pub axi: bool,
    pub tank_p_f: f32,
    pub tank_p_o: f32,
}

impl PhInput {
    pub fn from_settings(st: &Settings, pc: f32) -> PhInput {
        PhInput {
            prop: st.prop,
            pc,
            throat_cells: st.throat_cells,
            layout: Layout::Auto,
            t_gg: 1000.0,
            dx: st.dx,
            depth: st.depth,
            axi: st.axi,
            tank_p_f: st.tank_p_f,
            tank_p_o: st.tank_p_o,
        }
    }
}

pub struct PumpSize {
    pub prop: u8,
    pub cells: i32, // impeller diameter, cells
    pub len: i32,   // block length along the shaft, cells
    pub stages: i32,
    pub tip: f32,
    pub head: f32,
    pub mdot: f32,
    pub flow_ratio: f32, // design flow over the impeller's best-efficiency flow
    pub eff: f32,
    pub power: f32,
}

pub struct ShaftSize {
    pub pumps: Vec<PumpSize>,
    pub omega: f32,
    pub turb_cells: i32,
    pub turb_stages: i32,
    pub u_c0: f32,
    pub eff_t: f32,
    pub power: f32,
    pub speed_frac: f32, // design speed over rated speed
}

pub struct PhSize {
    pub input: PhInput,
    pub mf: f32,
    pub mo: f32,
    pub p_dis_f: f32,
    pub p_dis_o: f32,
    pub shafts: Vec<ShaftSize>,
    pub turb_len: i32,
    pub gg_of: f32,
    pub gg_mdot: f32,
    pub gg_area_f: f32, // cm^2
    pub gg_area_o: f32,
    pub gg_p: f32,
    pub feed_p_max: f32,
    pub cartridge_w: f32,
    pub notes: Vec<String>,
}

fn eta_peak(d: f32) -> f32 {
    (0.86 - 0.1 * (0.15 / d.max(0.01)).sqrt()).clamp(0.45, 0.84)
}

/// Block length along the shaft that the simulation reads back as `stages` stages.
fn pump_len(stages: i32, d_cells: i32) -> i32 {
    let mut len = ((stages as f32 * 0.6 * d_cells as f32).round() as i32).max(2);
    while ((len as f32 / (0.6 * d_cells as f32)).round().max(1.0) as i32) < stages {
        len += 1;
    }
    while len > 2 && ((len as f32 / (0.6 * d_cells as f32)).round().max(1.0) as i32) > stages {
        len -= 1;
    }
    len
}

/// How a pump of this size does at this speed and flow.
fn pump_eval(prop: u8, rho: f32, cells: i32, stages: i32, omega: f32, m: f32, dx: f32) -> PumpSize {
    let d = cells as f32 * dx;
    let u = omega * d * 0.5;
    let a = 0.25 * PI * d * d;
    let bep = rho * PHI_BEP * a * u;
    let r = m / bep.max(1e-6);
    let eta = eta_peak(d);
    let eff = (eta * (1.0 - 0.55 * (r - 1.0).powi(2))).clamp(0.1, eta);
    let ca = m / (rho * a);
    let head = (stages as f32 * rho * (PSI * u * u - 0.1 * PSI / (PHI_BEP * PHI_BEP) * ca * ca)).max(0.0);
    let friction = stages as f32 * 0.003 * rho * omega.powi(3) * (0.5 * d).powi(5);
    PumpSize { prop, cells, len: pump_len(stages, cells), stages, tip: u, head, mdot: m, flow_ratio: r, eff, power: m * head / (rho * eff) + friction }
}

/// A pump with a shaft to itself: the fewest stages that keep the tip under the design speed, the
/// impeller sized so the design flow is its best-efficiency flow. Returns the pump and its speed.
fn lone_pump(prop: u8, rho: f32, h: f32, m: f32, dx: f32) -> (PumpSize, f32) {
    let mut n = 1;
    let mut u;
    loop {
        u = (h / (n as f32 * 0.9 * PSI * rho)).sqrt();
        if u <= U_DESIGN || n >= 5 {
            break;
        }
        n += 1;
    }
    let d = (4.0 * m / (PI * rho * PHI_BEP * u)).sqrt();
    let cells = ((d / dx).round() as i32).max(4);
    let omega = 2.0 * u / (cells as f32 * dx);
    (pump_eval(prop, rho, cells, n, omega, m, dx), omega)
}

/// A pump that has to share a shaft turning at `omega`: for each stage count, the impeller that makes
/// the head at the design flow (head falls off above the best-efficiency flow, so it is solved for, not
/// read off at the best point); the stage count that comes closest to its best flow wins.
fn shaft_pump(prop: u8, rho: f32, h: f32, m: f32, omega: f32, dx: f32) -> PumpSize {
    let mut best: Option<PumpSize> = None;
    for n in 1..=4 {
        // head rises with diameter at a fixed speed and flow: bisect for it
        let (mut lo, mut hi) = (0.005f32, 2.0f32);
        for _ in 0..40 {
            let d = 0.5 * (lo + hi);
            let u = omega * d * 0.5;
            let a = 0.25 * PI * d * d;
            let ca = m / (rho * a);
            let head = n as f32 * rho * (PSI * u * u - 0.1 * PSI / (PHI_BEP * PHI_BEP) * ca * ca);
            if head < h {
                lo = d;
            } else {
                hi = d;
            }
        }
        let cells = ((hi / dx).ceil() as i32).max(3);
        let p = pump_eval(prop, rho, cells, n, omega, m, dx);
        if best.as_ref().map_or(true, |b| p.flow_ratio.ln().abs() < b.flow_ratio.ln().abs()) {
            best = Some(p);
        }
    }
    best.unwrap()
}

pub fn size(i: &PhInput) -> PhSize {
    let p = &PROPS[i.prop.min(PROPS.len() - 1)];
    let dx = i.dx;
    // propellant flow for this chamber pressure and throat at the best mixture ratio
    let a_t = if i.axi { PI * (i.throat_cells * 0.5 * dx).powi(2) } else { i.throat_cells * dx * i.depth };
    let g = p.gamma;
    let big_gamma = g.sqrt() * (2.0 / (g + 1.0)).powf((g + 1.0) / (2.0 * (g - 1.0)));
    let cstar = (p.rgas * t_flame(p, p.of_opt)).sqrt() / big_gamma;
    let mdot = i.pc * a_t / cstar;
    let mf = mdot / (1.0 + p.of_opt);
    let mo = mdot - mf;
    // pump discharge: the gas pressure at the injectors plus a stiff 25% across them, and for the fuel another
    // 10% through the cooling jacket and lines
    let p_dis_f = (LOCAL_F + 0.35) * i.pc;
    let p_dis_o = (LOCAL_O + 0.30) * i.pc;
    let h_f = (p_dis_f - i.tank_p_f).max(0.2 * i.pc);
    let h_o = (p_dis_o - i.tank_p_o).max(0.2 * i.pc);
    let mut notes = vec![];

    let twin = match i.layout {
        Layout::Single => false,
        Layout::Twin => true,
        Layout::Auto => p.rho_o / p.rho_f > 3.0,
    };
    // pumps, shaft by shaft (fuel first)
    let mut groups: Vec<(Vec<PumpSize>, f32)> = vec![];
    let (fp, w_f) = lone_pump(0, p.rho_f, h_f, mf, dx);
    if twin {
        let (op, w_o) = lone_pump(1, p.rho_o, h_o, mo, dx);
        groups.push((vec![fp], w_f));
        groups.push((vec![op], w_o));
        if i.layout == Layout::Auto {
            notes.push(format!(
                "The fuel is {:.0}x lighter than the oxidiser, so it needs {:.1}x the tip speed for the same pressure: two shafts let each pump spin at its own speed (as on the J-2 and RS-25).",
                p.rho_o / p.rho_f,
                (p.rho_o / p.rho_f).sqrt()
            ));
        }
    } else {
        // one speed for both: between what each pump would like on its own
        let (_, w_o) = lone_pump(1, p.rho_o, h_o, mo, dx);
        let w = (w_f * w_o).sqrt();
        let fp = shaft_pump(0, p.rho_f, h_f, mf, w, dx);
        let op = shaft_pump(1, p.rho_o, h_o, mo, w, dx);
        if [&fp, &op].iter().any(|q| q.flow_ratio > 1.6 || q.flow_ratio < 0.6) {
            notes.push(format!(
                "On one shaft the pumps run at {:.1}x and {:.1}x their best flow ({:.0}% and {:.0}% efficient). Two shafts would suit these propellants better.",
                fp.flow_ratio,
                op.flow_ratio,
                fp.eff * 100.0,
                op.eff * 100.0
            ));
        }
        groups.push((vec![fp, op], w));
    }

    // turbines: the blade speed that suits the drive gas, within the shaft's rating
    let c0 = C0_FACTOR * (p.rgas * i.t_gg).sqrt();
    let (opt, _) = turbine_opt(2.0);
    let tcells: Vec<i32> = groups
        .iter()
        .map(|(pumps, omega)| {
            let pump_max = pumps.iter().map(|q| q.cells).max().unwrap_or(4);
            let d_ideal = 2.0 * opt * c0 / omega;
            let d_cap = 2.0 * 0.75 * U_TIP_MAX / omega; // keep design speed under 75% of rated
            ((d_ideal.min(d_cap) / dx).round() as i32).max(6).max((0.8 * pump_max as f32).round() as i32)
        })
        .collect();
    let mut shafts = vec![];
    // one gas channel through all the turbines: its width is every wheel's length along the shaft
    let turb_len = tcells.iter().map(|&d| (0.8 * d as f32).round() as i32).max().unwrap_or(6).max(4);
    let mut p_total = 0.0;
    for ((pumps, omega), &cells) in groups.into_iter().zip(&tcells) {
        let stages = ((turb_len as f32 / (0.4 * cells as f32)).round() as i32).clamp(1, 3);
        let d_max = pumps.iter().map(|q| q.cells).max().unwrap_or(0).max(cells) as f32 * dx;
        let u_c0 = omega * cells as f32 * dx * 0.5 / c0;
        let power = 1.1 * pumps.iter().map(|q| q.power).sum::<f32>();
        p_total += power;
        shafts.push(ShaftSize {
            omega,
            turb_cells: cells,
            turb_stages: stages,
            u_c0,
            eff_t: turbine_eff(u_c0, stages as f32),
            power,
            speed_frac: omega * 0.5 * d_max / U_TIP_MAX,
            pumps,
        });
    }

    // gas generator: fuel-rich enough to come out at the drive temperature, big enough for the power
    // with a margin the governor trims off
    let t_gg = i.t_gg.clamp(850.0, p.t_peak - 100.0);
    let gg_of = p.of_opt * (-0.7 * (2.0 * ((p.t_peak - 800.0) / (t_gg - 800.0)).ln()).sqrt()).exp();
    let cp = p.gamma * p.rgas / (p.gamma - 1.0);
    let gg_mdot = 1.3 * p_total / (GG_YIELD * cp * t_gg);
    let gg_p = 0.5 * p_dis_f.min(p_dis_o);
    let mf_gg = gg_mdot / (1.0 + gg_of);
    let mo_gg = gg_mdot - mf_gg;
    let area = |m: f32, rho: f32, dp: f32| m / (0.7 * (2.0 * rho * dp.max(1e5)).sqrt()) * 1e4;
    let gg_area_f = area(mf_gg, p.rho_f, p_dis_f - gg_p);
    let gg_area_o = area(mo_gg, p.rho_o, p_dis_o - gg_p);
    if shafts.iter().any(|s| s.speed_frac > 0.8) {
        notes.push("A rotor runs close to its tip-speed rating: a lower chamber pressure or a bigger throat gives more margin.".into());
    }
    PhSize {
        input: *i,
        mf,
        mo,
        p_dis_f,
        p_dis_o,
        shafts,
        turb_len,
        gg_of,
        gg_mdot,
        gg_area_f,
        gg_area_o,
        gg_p,
        feed_p_max: 1.05 * p_dis_f.max(p_dis_o),
        cartridge_w: (2.0 * p_total).clamp(5.0e5, 2.0e8),
        notes,
    }
}

/// Where the piece's pipe ends are, for the legend and for joining it up: (name, open cell x, y).
pub struct Port {
    pub name: &'static str,
    pub x: i32,
    pub y: i32,
}

/// Draw the powerhead. Pumps on the left, each on a horizontal shaft into a turbine in one vertical gas
/// channel on the right; the gas generator sits on top of the channel and its exhaust leaves at the
/// bottom. Both propellants come in on the top edge and leave on the bottom edge through a main valve
/// and a throttle valve, so tanks above and a chamber below join up without crossing. The generator taps
/// both pump discharges through the back pipe layer.
pub fn build(s: &PhSize) -> (Clip, Vec<Port>) {
    let twin = s.shafts.len() == 2;
    let lt = s.turb_len;
    let fp = &s.shafts[0].pumps[0];
    let op = if twin { &s.shafts[1].pumps[0] } else { &s.shafts[0].pumps[1] };
    let dt0 = s.shafts[0].turb_cells;
    let dt1 = if twin { s.shafts[1].turb_cells } else { 0 };
    let gap = 6; // shaft between rotors
    let xfv = 2; // fuel outlet line down the left side
    let xov = 8; // two shafts: oxidiser inlet line down to the lower pump
    let xf = if twin { 18 } else { 12 }; // fuel pump left end
    let xo = if twin { xf } else { xf + fp.len + gap };
    let end0 = if twin { xf + fp.len } else { xo + op.len };
    let end1 = if twin { xo + op.len } else { 0 };
    let xt = end0.max(end1) + gap;
    let yg = 3; // generator head rows yg, yg + 1
    let lo = |d: i32| d / 2; // rows above the shaft line
    let hi = |d: i32| d - d / 2; // ... and from it down
    let mut cy0 = (yg + 2 + 6 + lo(dt0)).max(10 + lo(fp.cells));
    if !twin {
        cy0 = cy0.max(10 + lo(op.cells));
    }
    let cy1 = if twin { (cy0 + hi(dt0) + 3 + lo(dt1)).max(cy0 + hi(fp.cells) + 4 + lo(op.cells)) } else { cy0 };
    let cyo = if twin { cy1 } else { cy0 };
    let top_f = cy0 - lo(fp.cells);
    let top_o = cyo - lo(op.cells);
    let ob = top_o + op.cells - 1; // oxidiser pump bottom row
    let tb = if twin { cy1 - lo(dt1) + dt1 - 1 } else { cy0 - lo(dt0) + dt0 - 1 };
    let fb = top_f + fp.cells - 1;
    let h = (ob + 15).max(tb + 8).max(fb + 16);
    let w = xt + lt + 6;
    let mut b = Board::new(w as usize, h as usize);

    // gas channel: cap, generator head, walls, open passage down to the bottom edge
    b.rect(xt - 1, yg - 1, xt + lt, yg - 1, c::STEEL);
    b.rect(xt, yg, xt + lt - 1, yg + 1, c::GG);
    b.rect(xt - 1, yg + 2, xt - 1, h - 1, c::STEEL);
    b.rect(xt + lt, yg + 2, xt + lt, h - 1, c::STEEL);
    for y in yg + 2..h {
        for x in xt..xt + lt {
            b.open.insert((x, y));
        }
    }
    // turbines in the channel, pumps and shafts on their rows
    let rows: Vec<(i32, i32, Vec<(i32, &PumpSize)>)> = if twin {
        vec![(cy0, dt0, vec![(xf, fp)]), (cy1, dt1, vec![(xo, op)])]
    } else {
        vec![(cy0, dt0, vec![(xf, fp), (xo, op)])]
    };
    for (cy, dt, pumps) in &rows {
        b.rect(xt, cy - lo(*dt), xt + lt - 1, cy - lo(*dt) + dt - 1, c::TURBINE);
        let mut x_end = 0;
        for &(x0, q) in pumps {
            if x_end > 0 {
                b.rect(x_end, cy - 1, x0 - 1, *cy, c::SHAFT);
            }
            b.rect(x0, cy - lo(q.cells), x0 + q.len - 1, cy - lo(q.cells) + q.cells - 1, c::PUMP);
            x_end = x0 + q.len;
        }
        b.rect(x_end, cy - 1, xt - 1, *cy, c::SHAFT);
    }
    let mut ports = vec![];
    // fuel: in through the top of the pump; out of its left end, past the generator tap, down the left
    // side through the main valve and throttle valve
    stub(&mut b, xf, top_f - 1, 0, -1, top_f - 1);
    ports.push(Port { name: "fuel in", x: xf, y: 0 });
    let xj = xf - 3; // generator tap
    if twin {
        // the fuel line passes behind the oxidiser inlet line through the back layer
        b.rect(xov + 3, cy0 - 1, xf - 1, cy0, c::PIPE);
        b.rect(xov + 3, cy0 - 1, xov + 4, cy0, c::JOINT);
        b.back_rect(xov - 3, cy0 - 1, xov + 4, cy0);
        b.rect(xov - 3, cy0 - 1, xov - 2, cy0, c::JOINT);
        b.rect(xfv, cy0 - 1, xov - 4, cy0, c::PIPE);
    } else {
        b.rect(xfv, cy0 - 1, xf - 1, cy0, c::PIPE);
    }
    stub(&mut b, xfv, cy0 + 1, 0, 1, h - 2 - cy0);
    b.rect(xfv, h - 12, xfv + 1, h - 11, c::VALVE_X);
    b.rect(xfv, h - 7, xfv + 1, h - 7, c::THROTTLE);
    ports.push(Port { name: "fuel out (to the cooling jacket)", x: xfv, y: h - 1 });
    b.rect(xj, cy0 - 1, xj + 1, cy0, c::JOINT);
    b.back_rect(xj, yg, xj + 1, cy0);
    b.back_rect(xj, yg, xt - 1, yg + 1);
    b.rect(xt - 1, yg, xt - 1, yg + 1, c::JOINT);
    // oxidiser: in from the top (straight down to its pump, or down the left to the lower shaft), out of
    // the bottom through the generator tap and the valves
    let xom = xo + (op.len - 2).max(0) / 2;
    if twin {
        b.rect(xov, 1, xov + 1, cy1, c::PIPE);
        b.rect(xov, cy1 - 1, xo - 1, cy1, c::PIPE);
        b.open.insert((xov, 0));
        b.open.insert((xov + 1, 0));
        ports.push(Port { name: "oxidiser in", x: xov, y: 0 });
    } else {
        stub(&mut b, xom, top_o - 1, 0, -1, top_o - 1);
        ports.push(Port { name: "oxidiser in", x: xom, y: 0 });
    }
    stub(&mut b, xom, ob + 1, 0, 1, h - 2 - ob);
    b.rect(xom, ob + 2, xom + 1, ob + 3, c::JOINT);
    b.rect(xom, ob + 6, xom + 1, ob + 7, c::VALVE_X);
    b.rect(xom, ob + 10, xom + 1, ob + 10, c::THROTTLE);
    ports.push(Port { name: "oxidiser out (to the injectors)", x: xom, y: h - 1 });
    let xb = xt + lt + 2;
    b.back_rect(xom, ob + 2, xb + 1, ob + 3);
    b.back_rect(xb, yg, xb + 1, ob + 3);
    b.back_rect(xt + lt, yg, xb + 1, yg + 1);
    b.rect(xt + lt, yg, xt + lt, yg + 1, c::JOINT);
    ports.push(Port { name: "turbine exhaust", x: xt, y: h - 1 });
    (b.clip(), ports)
}

/// Put the sizing into the engine settings: generator orifices and pressure, the pump governor and the
/// start cartridge.
pub fn apply(s: &PhSize, st: &mut Settings) {
    st.gg_area_f = s.gg_area_f;
    st.gg_area_o = s.gg_area_o;
    st.gg_p = s.gg_p;
    st.feed_p_max = s.feed_p_max;
    st.cartridge_w = s.cartridge_w;
    // the throttle valves trim the mixture to the best ratio, whatever the lines lose
    st.mix_target = PROPS[s.input.prop.min(PROPS.len() - 1)].of_opt;
    st.throttle = 1.0;
}

/// A whole engine around a generated powerhead (for tests): tanks above it, a regen-cooled chamber
/// below, everything piped up and the injectors sized.
pub fn demo_engine(w: &mut crate::world::World, i: &PhInput) -> PhSize {
    use crate::world::World;
    w.clear();
    w.st.prop = i.prop;
    w.st.throat_cells = i.throat_cells;
    w.st.gse_tanks = true;
    let s = size(&PhInput { dx: w.st.dx, depth: w.st.depth, axi: w.st.axi, tank_p_f: w.st.tank_p_f, tank_p_o: w.st.tank_p_o, ..*i });
    let (ph, ports) = build(&s);
    let port = |name: &str| ports.iter().find(|p| p.name.starts_with(name)).map(|p| (p.x, p.y)).unwrap();
    let th = i.throat_cells * 0.5;
    let ch = crate::prefabs::chamber(th * 2.3, th * 4.3, th, th * 6.0, th * 16.0);
    // the chamber's ports: oxidiser stub on top, fuel stub on the left
    let cget = |x: usize, y: usize| ch.cells[y * ch.w + x];
    let xm = (0..ch.w).find(|&x| cget(x, 1) == c::PIPE).unwrap() as i32;
    let yf = (0..ch.h).find(|&y| cget(1, y) == c::PIPE).unwrap() as i32;
    let (px, py) = (230, 110);
    w.paste(&ph, px, py);
    let (fox, foy) = port("oxidiser out");
    let cx = px + fox - xm;
    let cy = py + ph.h as i32 + 3;
    w.paste(&ch, cx, cy);
    let pipe = |w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32| {
        for y in y0.min(y1)..=y0.max(y1) {
            for x in x0.min(x1)..=x0.max(x1) {
                w.paint_plumbing(World::idx(x as usize, y as usize), c::PIPE);
            }
        }
    };
    // oxidiser: straight down into the chamber's top
    pipe(w, px + fox, py + foy, px + fox + 1, cy);
    // fuel: down, left past the bell, down and into the jacket stub
    let (ffx, ffy) = port("fuel out");
    let r1 = py + ffy + 1;
    let x1 = cx - 6;
    pipe(w, px + ffx, py + ffy, px + ffx + 1, r1 + 1);
    pipe(w, x1, r1, px + ffx + 1, r1 + 1);
    pipe(w, x1, r1, x1 + 1, cy + yf + 1);
    pipe(w, x1, cy + yf, cx, cy + yf + 1);
    // tanks above the two inlets, narrow enough not to overlap
    let (fix, _) = port("fuel in");
    let (oix, _) = port("oxidiser in");
    let tw = (((oix - fix).abs() - 6) / 2).clamp(2, 10);
    for (x, kind, top) in [(px + fix, c::TANK_F, py - 70), (px + oix, c::TANK_O, py - 70)] {
        w.rect(x + 1 - tw, top, x + tw, py - 9, kind);
        for yy in top..=py - 9 {
            for xx in x + 1 - tw..=x + tw {
                w.wall_around(World::idx(xx as usize, yy as usize));
            }
        }
        pipe(w, x, py - 8, x + 1, py);
    }
    w.nets_dirty = true;
    w.rebuild_nets();
    w.auto_size_injectors(i.pc);
    apply(&s, &mut w.st);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[test]
    fn powerheads_size_and_plumb_for_every_propellant() {
        for prop in 0..PROPS.len() {
            for layout in [Layout::Auto, Layout::Single, Layout::Twin] {
                let mut w = World::new();
                let mut i = PhInput::from_settings(&w.st, 60.0e5);
                i.prop = prop;
                i.layout = layout;
                let s = demo_engine(&mut w, &i);
                let what = format!("{} {:?}", PROPS[prop].name, layout);
                // sized sensibly: every pump makes its head near its best flow, no rotor near its limit
                for sh in &s.shafts {
                    assert!(sh.speed_frac < 0.8, "{what}: {:.2} of rated", sh.speed_frac);
                    for q in &sh.pumps {
                        let need = if q.prop == 0 { s.p_dis_f - i.tank_p_f } else { s.p_dis_o - i.tank_p_o };
                        assert!(q.head > 0.9 * need, "{what}: pump {} makes {:.0} of {:.0} bar", q.prop, q.head / 1e5, need / 1e5);
                    }
                }
                assert!(s.gg_area_f > 0.0 && s.gg_area_o > 0.0 && s.gg_of < PROPS[prop].of_opt, "{what}: generator runs fuel-rich");
                // and drawn so it all joins up: nothing mixed, the right rotors, a generator fed by both
                assert!(w.mixed.is_none(), "{what}: fuel and oxidiser meet");
                assert_eq!(w.shafts.len(), s.shafts.len(), "{what}");
                for (ws, ss) in w.shafts.iter().zip(&s.shafts) {
                    assert_eq!(ws.pumps.len(), ss.pumps.len(), "{what}");
                    assert!(ws.turb_cells > 0, "{what}");
                }
                assert_eq!(w.gg.len(), 1, "{what}: gas generator fed from both pumps");
                assert!(w.st.inj_area_f > 0.0 && w.st.inj_area_o > 0.0);
            }
        }
    }
}
