//! Powerhead maker: sizes the turbopumps, turbines and gas generator for a propellant pair, a chamber
//! pressure and a throat, and draws them as one piece that goes down like a paste.
//!
//! The sizing uses the same pump and turbine models the simulation runs: head per stage is
//! PSI * rho * u^2, a pump's best flow is PHI_BEP * eye area * u, and a turbine works best near its
//! optimum blade-to-gas speed ratio.
//!
//! The drive gas is what keeps the turbines small: the generator's gas is held at pressure by a narrow nozzle into
//! each wheel (the generator's chamber narrows to it through a reducer), so a little gas at a high pressure ratio does
//! the work, and the wheels are sized to their pumps, not to the gas.

use crate::design::Clip;
use crate::prefabs::{stub, Board};
use crate::world::{c, t_flame, turbine_eff, Settings, PHI_BEP, PROPS, PSI, U_TIP_MAX};
use std::f32::consts::PI;

/// The gas pressure at the fuel and the oxidiser injectors, against the chamber's average (measured on the standard chambers: the
/// fuel enters down the side, the oxidiser at the head end where the gas stagnates).
pub const LOCAL_F: f32 = 0.9;
pub const LOCAL_O: f32 = 1.25;

/// Design tip speed: well under the rated limit, so the governor has room above it.
const U_DESIGN: f32 = 0.62 * U_TIP_MAX;
/// A turbine wheel is at most this much wider than the biggest pump on its shaft.
const TURB_OVER_PUMP: f32 = 1.2;
/// Flow coefficient of a turbine nozzle, as the simulation meters the gas through a wheel.
const K_NOZ: f32 = 0.6;
/// The generator's chamber pressure over the pressure in the nozzle at the wheel (measured on generated powerheads).
const NOZ_LOSS: f32 = 1.18;
/// Pressure under the last wheel when it exhausts into the open.
const P_EXH: f32 = 1.5e5;
/// The generator runs at no more than this share of the lower pump discharge pressure (its injectors need the rest).
const GG_P_SHARE: f32 = 0.65;
/// Where the generator's temperature control holds its oxidiser valve at the design point.
const GG_TRIM_DESIGN: f32 = 0.85;
/// Tap-off: the tapped gas against the chamber's flame temperature and pressure (it is taken at the wall by the injector face).
const TAP_T_FRAC: f32 = 0.85;
const TAP_P_FRAC: f32 = 0.9;
/// Turbine power over what the pumps take at the design point: the governor's bypass trims the surplus off.
const POWER_MARGIN: f32 = 1.25;

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
    pub steps: u32, // gas steps per frame (sets how fast a wheel can take gas in, see `expand`)
    pub p_exh: f32, // pressure the last wheel exhausts into, Pa (open air under the part, or a duct that holds it higher)
    pub tapoff: bool, // no generator: the turbine runs on hot gas tapped from the main chamber (one shaft)
    pub expander: bool, // no generator: the fuel the cooling jacket has warmed runs through the turbines on its way to the injectors
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
            steps: st.steps_per_frame,
            p_exh: P_EXH,
            tapoff: false,
            expander: false,
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
    pub turb_cells: i32, // wheel diameter, cells
    pub turb_len: i32,   // block length along the shaft, cells
    pub turb_stages: i32,
    pub nozzle: i32, // width of the nozzle into the wheel, cells
    pub p_in: f32,   // gas pressure at the nozzle, Pa
    pub p_out: f32,  // ... and behind the wheel
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
    pub gg_of: f32,
    pub gg_t: f32, // drive gas temperature the generator is sized for, K
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

/// Block length along the shaft that the simulation reads back as `stages` stages, each `per` diameters long.
fn block_len(stages: i32, d_cells: i32, per: f32) -> i32 {
    let read = |len: i32| (len as f32 / (per * d_cells as f32)).round().max(1.0) as i32;
    let mut len = ((stages as f32 * per * d_cells as f32).round() as i32).max(2);
    while read(len) < stages {
        len += 1;
    }
    while len > 2 && read(len) > stages {
        len -= 1;
    }
    len
}
fn pump_len(stages: i32, d_cells: i32) -> i32 {
    block_len(stages, d_cells, 0.6)
}

/// Turbine stages for this blade-speed ratio: the fewest that come within 12 points of the best efficiency (every
/// stage makes the wheel 0.4 diameters longer, and a compact wheel is worth a little more drive gas).
fn pick_stages(u_c0: f32) -> i32 {
    let eff = |n: i32| turbine_eff(u_c0, n as f32);
    let best = (1..=3).map(eff).fold(0.0, f32::max);
    (1..=3).find(|&n| eff(n) >= best - 0.12).unwrap_or(3)
}

/// One wheel of the drive-gas path at a given gas flow.
#[derive(Clone, Copy)]
struct Stage {
    p_in: f32,
    p_out: f32,
    u_c0: f32,
    stages: i32,
    power: f32,
}

/// The drive gas followed through the wheels in series (`wheels`: blade speed and shaft load of each; `noz`: the width
/// of the nozzle into each, cells of `cell_area` m^2): every nozzle passes the whole flow, so working back from the
/// exhaust gives the pressure ahead of each, and with that what each wheel makes of the gas.
/// A wheel takes its gas from the nozzle cells against it, by the orifice law and never more than the gas in those
/// cells each frame: in effect the gas goes in no faster than `v_in`, and at these pressures that is what sets the
/// pressure in the nozzle.
fn expand(m: f32, t_gg: f32, noz: &[i32], wheels: &[(f32, f32)], rgas: f32, gamma: f32, cell_area: f32, v_in: f32, p_exh: f32) -> Vec<Stage> {
    let n = wheels.len();
    let cp = gamma * rgas / (gamma - 1.0);
    let k = (gamma - 1.0) / gamma;
    let mut t = vec![t_gg; n];
    let mut out = vec![];
    for _ in 0..6 {
        let mut p = vec![p_exh; n + 1];
        for j in (0..n).rev() {
            // m = K A sqrt(2 rho (p_in - p_out)) with rho = p_in / (R T), solved for p_in
            let a = noz[j] as f32 * cell_area;
            let q = m / (K_NOZ * a);
            let p_orifice = 0.5 * (p[j + 1] + (p[j + 1] * p[j + 1] + 2.0 * rgas * t[j] * q * q).sqrt());
            // m = rho A v_in
            p[j] = p_orifice.max(m * rgas * t[j] / (a * v_in));
        }
        out = (0..n)
            .map(|j| {
                let dh = cp * t[j] * (1.0 - (p[j + 1] / p[j]).powf(k));
                let u_c0 = wheels[j].0 / (2.0 * dh).sqrt().max(1.0);
                let stages = pick_stages(u_c0);
                let eff = turbine_eff(u_c0, stages as f32);
                Stage { p_in: p[j], p_out: p[j + 1], u_c0, stages, power: eff * m * dh }
            })
            .collect();
        // the gas reaches the next wheel cooler by the work the one before took out of it
        for j in 1..n {
            t[j] = (t[j - 1] - wheels[j - 1].1 / (m * cp)).max(0.3 * t[j - 1]);
        }
    }
    out
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
    // (an expander's fuel also has to get through the turbines: about 1.3 in pressure for each stage, two on the fuel
    // shaft and one on the oxidiser's, or two on a shared shaft)
    let exp_stages: &[i32] = if !i.expander { &[] } else if i.layout == Layout::Single || (i.layout == Layout::Auto && p.rho_o / p.rho_f <= 3.0) { &[2] } else { &[2, 1] };
    let p_dis_f = (LOCAL_F + 0.35) * i.pc * 1.3f32.powi(exp_stages.iter().sum());
    let p_dis_o = (LOCAL_O + 0.30) * i.pc;
    let h_f = (p_dis_f - i.tank_p_f).max(0.2 * i.pc);
    let h_o = (p_dis_o - i.tank_p_o).max(0.2 * i.pc);
    let mut notes = vec![];

    let twin = !i.tapoff
        && match i.layout {
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
        // one speed for both: between what each pump would like on its own, nearer the one that takes more of the power
        // (a hydrogen pump made to turn at an oxygen pump's speed is big, many-staged and wasteful)
        let (op_alone, w_o) = lone_pump(1, p.rho_o, h_o, mo, dx);
        let share = fp.power / (fp.power + op_alone.power).max(1.0);
        let w = w_f.powf(share) * w_o.powf(1.0 - share);
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

    // turbine wheels: about the size of the pumps they drive (a little wider, for blade speed), within the tip-speed rating
    let tcells: Vec<i32> = groups
        .iter()
        .map(|(pumps, omega)| {
            let pump_max = pumps.iter().map(|q| q.cells).max().unwrap_or(6) as f32;
            let d_cap = 2.0 * 0.75 * U_TIP_MAX / omega / dx; // keep design speed under 75% of rated
            (((TURB_OVER_PUMP * pump_max).min(d_cap)).round() as i32).max(6)
        })
        .collect();
    // the drive gas: one generator, through a nozzle into the first wheel and (two shafts) on through a second nozzle
    // into the other. The nozzles are what hold the pressure up in front of each wheel: the narrowest that keep the
    // generator under its share of the pump pressure, at the least gas flow that gives every shaft its power.
    let t_gg = i.t_gg.clamp(850.0, p.t_peak - 100.0);
    let loads: Vec<f32> = groups.iter().map(|(pumps, _)| 1.1 * pumps.iter().map(|q| q.power).sum::<f32>()).collect();
    let wheels: Vec<(f32, f32)> = groups.iter().zip(&tcells).zip(&loads).map(|(((_, omega), &d), &load)| (omega * d as f32 * dx * 0.5, load)).collect();
    // (a round engine: the passages are rings about the centre line; the maker cannot know where the part will be put
    // down, and takes it to stand about 60 cells out)
    let cell_area = dx * if i.axi { std::f32::consts::TAU * 60.0 * dx } else { i.depth };
    let p_cap = GG_P_SHARE * p_dis_f.min(p_dis_o) / NOZ_LOSS;
    let v_in = crate::world::DX0 / (crate::world::DT0 * i.steps.max(1) as f32);
    // (a nozzle can be as wide as a three-stage wheel is long)
    let n_max: Vec<i32> = tcells.iter().map(|&d| block_len(3, d, 0.4)).collect();
    let solve = |noz: &[i32]| -> Option<(f32, Vec<Stage>)> {
        let ok = |m: f32| expand(m, t_gg, noz, &wheels, p.rgas, g, cell_area, v_in, i.p_exh).iter().zip(&loads).all(|(st, &load)| st.power >= POWER_MARGIN * load);
        let m_hi = 0.4 * mdot;
        if !ok(m_hi) {
            return None;
        }
        let (mut lo, mut hi) = (0.0f32, m_hi);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if ok(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Some((hi, expand(hi, t_gg, noz, &wheels, p.rgas, g, cell_area, v_in, i.p_exh)))
    };
    let mut cands: Vec<Vec<i32>> = vec![];
    for a in 1..=n_max[0] {
        if wheels.len() == 1 {
            cands.push(vec![a]);
        } else {
            for b in 1..=n_max[1] {
                cands.push(vec![a, b]);
            }
        }
    }
    let mut best: Option<(f32, Vec<i32>, Vec<Stage>)> = None;
    let mut over: Option<(f32, Vec<i32>, Vec<Stage>)> = None; // if nothing fits under the cap: the lowest pressure found
    if i.expander {
        // expander: no gas at all. Each wheel has the fuel line through it and takes what the warm fuel gives across its
        // stages; what chamber pressure that reaches is the cooling jacket's doing, not the sizing's
        let st: Vec<Stage> = wheels
            .iter()
            .zip(exp_stages)
            .zip(&loads)
            .map(|((wh, &n), &load)| {
                let dh = p.cp_f * 150.0 * (1.0 - 1.3f32.powf(-0.29 * n as f32));
                Stage { p_in: p_dis_f, p_out: p_dis_f / 1.3f32.powi(n), u_c0: wh.0 / (2.0 * dh).sqrt().max(1.0), stages: n, power: POWER_MARGIN * load }
            })
            .collect();
        best = Some((0.0, vec![2; st.len()], st));
    }
    if i.tapoff {
        // tap-off: the gas is the chamber's own, at the chamber's pressure and near its temperature, so the nozzle sets
        // only how much of it the wheel gets (a wheel with no generator ducted to it takes gas at half the speed): the
        // narrowest that gives the shaft its power
        let (t_tap, p_tap) = (TAP_T_FRAC * t_flame(p, p.of_opt), TAP_P_FRAC * i.pc);
        let cp = g * p.rgas / (g - 1.0);
        let dh = cp * t_tap * (1.0 - (i.p_exh / p_tap).powf((g - 1.0) / g));
        let u_c0 = wheels[0].0 / (2.0 * dh).sqrt().max(1.0);
        let st_n = pick_stages(u_c0);
        let eff = turbine_eff(u_c0, st_n as f32);
        for n in 1..=n_max[0] {
            let m = n as f32 * p_tap / (p.rgas * t_tap) * cell_area * 0.5 * v_in;
            let st = Stage { p_in: p_tap, p_out: i.p_exh, u_c0, stages: st_n, power: eff * m * dh };
            best = Some((m, vec![n], vec![st]));
            if st.power >= POWER_MARGIN * loads[0] {
                break;
            }
        }
    }
    for noz in if i.tapoff || i.expander { vec![] } else { cands } {
        let Some((m, st)) = solve(&noz) else { continue };
        if st[0].p_in <= p_cap {
            if best.as_ref().map_or(true, |b| m < 0.995 * b.0) {
                best = Some((m, noz, st));
            }
        } else if over.as_ref().map_or(true, |b| st[0].p_in < b.2[0].p_in) {
            over = Some((m, noz, st));
        }
    }
    let (gg_mdot, noz, stages) = best.or(over).unwrap_or_else(|| {
        let noz = n_max.clone();
        let m = 0.4 * mdot;
        let st = expand(m, t_gg, &noz, &wheels, p.rgas, g, cell_area, v_in, i.p_exh);
        (m, noz, st)
    });
    if stages.iter().zip(&loads).any(|(st, &load)| st.power < 1.1 * load) {
        notes.push("The turbines cannot make the pumps' power from this gas: a hotter generator or a lower chamber pressure would.".into());
    }
    let mut shafts = vec![];
    let mut p_total = 0.0;
    for ((((pumps, omega), &cells), st), (&power, &nozzle)) in groups.into_iter().zip(&tcells).zip(&stages).zip(loads.iter().zip(&noz)) {
        let d_max = pumps.iter().map(|q| q.cells).max().unwrap_or(0).max(cells) as f32 * dx;
        p_total += power;
        // a wheel at least as long as its nozzle is wide
        let n_st = (st.stages..=3).find(|&n| block_len(n, cells, 0.4) >= nozzle).unwrap_or(3);
        shafts.push(ShaftSize {
            omega,
            turb_cells: cells,
            turb_len: block_len(n_st, cells, 0.4),
            turb_stages: n_st,
            nozzle,
            p_in: st.p_in,
            p_out: st.p_out,
            u_c0: st.u_c0,
            eff_t: turbine_eff(st.u_c0, n_st as f32),
            power,
            speed_frac: omega * 0.5 * d_max / U_TIP_MAX,
            pumps,
        });
    }

    // gas generator: fuel-rich enough to come out at the drive temperature, its injectors sized against the pressure
    // the first nozzle holds it at
    let gg_of = p.of_opt * (-0.7 * (2.0 * ((p.t_peak - 800.0) / (t_gg - 800.0)).ln()).sqrt()).exp();
    let gg_p = NOZ_LOSS * stages[0].p_in;
    if !i.tapoff && !i.expander && gg_p > 0.8 * p_dis_f.min(p_dis_o) {
        notes.push(format!(
            "The generator would have to run at {:.0} bar, too close to what the pumps deliver for its injectors to work: this engine wants a lower chamber pressure or a hotter generator.",
            gg_p / 1e5
        ));
    }
    if !i.tapoff && !i.expander && gg_p < 8.0e5 {
        notes.push(format!(
            "This engine needs so little drive gas that even a one-cell nozzle only holds the generator at {:.0} bar: a higher chamber pressure or a bigger throat makes better use of it.",
            gg_p / 1e5
        ));
    }
    let mf_gg = gg_mdot / (1.0 + gg_of);
    let mo_gg = gg_mdot - mf_gg;
    let area = |m: f32, rho: f32, dp: f32| m / (0.7 * (2.0 * rho * dp.max(1e5)).sqrt()) * 1e4;
    let gg_area_f = area(mf_gg, p.rho_f, p_dis_f - gg_p);
    // (the oxidiser orifice a size over: the temperature control holds its valve at about 85% and has room to open)
    let gg_area_o = area(mo_gg, p.rho_o, p_dis_o - gg_p) / GG_TRIM_DESIGN;
    // (tap-off: there is no generator to feed or hold)
    let (gg_area_f, gg_area_o, t_gg) = if i.tapoff || i.expander { (0.0, 0.0, 0.0) } else { (gg_area_f, gg_area_o, t_gg) };
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
        gg_of,
        gg_t: t_gg,
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

/// Steel wherever nothing has been drawn yet (and no passage is to stay open).
fn steel(b: &mut Board, x0: i32, y0: i32, x1: i32, y1: i32) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            if b.get(x, y) == c::EMPTY && !b.open.contains(&(x, y)) {
                b.set(x, y, c::STEEL);
            }
        }
    }
}

/// A gas passage cut through whatever is there.
fn carve(b: &mut Board, x0: i32, y0: i32, x1: i32, y1: i32) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            b.set(x, y, c::EMPTY);
            b.open.insert((x, y));
        }
    }
}

/// Draw the powerhead. Pumps on the left, each on a horizontal shaft into a turbine wheel in its casing on the right.
/// The drive gas runs down the right-hand side: the gas generator on top (injector head, combustion chamber), a
/// reducer narrowing to the nozzle into the top of the first wheel, out of the bottom of that wheel and (two shafts)
/// through a second reducer and nozzle into the wheel below, then out of the bottom edge. Both propellants come in on
/// the top edge and leave on the bottom edge through a main valve and a throttle valve, so tanks above and a chamber
/// below join up without crossing. The generator taps both pump discharges through the back pipe layer, with a valve
/// on each feed beside its head.
pub fn build(s: &PhSize) -> (Clip, Vec<Port>) {
    build_with(s, false, true)
}

/// The same, with the generator as a burner block (a machine drawn as a cutaway: head and chamber are the block, the
/// reducer is its outlet) instead of an injector plate over an open chamber; and, with `ox_valves` false, without the
/// main and throttle valves in the oxidiser outlet (for an engine that has them at its injector head, where the real
/// ones are: the oxidiser then has no long dry line to fill before it reaches the injectors).
pub fn build_with(s: &PhSize, burner: bool, ox_valves: bool) -> (Clip, Vec<Port>) {
    let twin = s.shafts.len() == 2;
    let fp = &s.shafts[0].pumps[0];
    let op = if twin { &s.shafts[1].pumps[0] } else { &s.shafts[0].pumps[1] };
    let (d0, l0, n0) = (s.shafts[0].turb_cells, s.shafts[0].turb_len, s.shafts[0].nozzle);
    let (d1, l1, n1) = if twin { (s.shafts[1].turb_cells, s.shafts[1].turb_len, s.shafts[1].nozzle) } else { (0, 0, 0) };
    let gap = 6; // shaft between rotors
    let xfv = 2; // fuel outlet line down the left side
    let xov = 8; // two shafts: oxidiser inlet line down to the lower pump
    let xf = if twin { 18 } else { 12 }; // fuel pump left end
    let xo = if twin { xf } else { xf + fp.len + gap };
    let end0 = if twin { xf + fp.len } else { xo + op.len };
    let end1 = if twin { xo + op.len } else { 0 };
    // generator chamber: a few cells wider than the nozzle it narrows to (and by an even number, to narrow evenly)
    let mut wc = (n0 + 6).max(8);
    if (wc - n0) % 2 != 0 {
        wc += 1;
    }
    // everything the gas goes through is centred on one column
    // (the generator's feed valves stick out five cells either side of its head)
    let xc = end0.max(end1) + gap + ((wc + 12).max(l0).max(l1) + 1) / 2;
    let xt0 = xc - l0 / 2;
    let xt1 = xc - l1 / 2;
    let (cx0, nx0) = (xc - wc / 2, xc - n0 / 2);
    let cx1 = cx0 + wc - 1;
    let lo = |d: i32| d / 2; // rows above the shaft line
    let hi = |d: i32| d - d / 2; // ... and from it down
    // down the gas path: head rows yg, yg + 1; chamber; reducer; a two-row nozzle; the first wheel
    let yg = 3;
    let nf = (wc - n0) / 2; // reducer rows: one cell in from each side per row
    let pump_top = if twin { lo(fp.cells) } else { lo(fp.cells).max(lo(op.cells)) };
    // (the chamber longer than it is wide, so a burner block reads as pointing down at the wheel)
    let cy0 = (yg + 2 + 6.max(wc - 1) + nf + 2 + lo(d0)).max(10 + pump_top);
    let ty0 = cy0 - lo(d0); // first wheel's top row
    let hc = ty0 - 2 - nf - (yg + 2); // chamber rows
    let by0 = ty0 + d0 - 1;
    // two shafts: a collector under the first wheel, a reducer (two cells in per row) to the second nozzle, the second wheel
    let nx1 = xc - n1 / 2;
    let nf2 = if twin { ((nx1 - xt0).abs().max((xt0 + l0 - nx1 - n1).abs()) + 1) / 2 } else { 0 };
    let cy1 = if twin { (by0 + 1 + 2 + nf2 + 2 + lo(d1)).max(cy0 + hi(fp.cells) + 4 + lo(op.cells)) } else { cy0 };
    let ty1 = cy1 - lo(d1);
    let coll = ty1 - 2 - nf2 - (by0 + 1); // collector rows
    let cyo = if twin { cy1 } else { cy0 };
    let top_f = cy0 - lo(fp.cells);
    let top_o = cyo - lo(op.cells);
    let ob = top_o + op.cells - 1; // oxidiser pump bottom row
    let fb = top_f + fp.cells - 1;
    let (xtl, ll, tb) = if twin { (xt1, l1, ty1 + d1 - 1) } else { (xt0, l0, by0) }; // the last wheel: left end, length, bottom row
    let h = (ob + 15).max(tb + 8).max(fb + 16);
    let xr = (cx1 + 7).max(xt0 + l0 + 1).max(if twin { xt1 + l1 + 1 } else { 0 }); // right edge of the casings
    let xb = xr + 2; // the generator's oxidiser tap comes up the right side on the back layer
    let w = xb + 3;
    let mut b = Board::new(w as usize, h as usize);

    // rotors: turbine wheels, pumps and the shafts between them
    let rows: Vec<(i32, i32, i32, i32, Vec<(i32, &PumpSize)>)> = if twin {
        vec![(cy0, d0, xt0, l0, vec![(xf, fp)]), (cy1, d1, xt1, l1, vec![(xo, op)])]
    } else {
        vec![(cy0, d0, xt0, l0, vec![(xf, fp), (xo, op)])]
    };
    for (cy, dt, xt, lt, pumps) in &rows {
        b.rect(*xt, cy - lo(*dt), xt + lt - 1, cy - lo(*dt) + dt - 1, c::TURBINE);
        let mut x_end = 0;
        for &(x0, q) in pumps {
            if x_end > 0 {
                b.rect(x_end, cy - 1, x0 - 1, *cy, c::SHAFT);
            }
            b.rect(x0, cy - lo(q.cells), x0 + q.len - 1, cy - lo(q.cells) + q.cells - 1, c::PUMP);
            x_end = x0 + q.len;
        }
        b.rect(x_end, cy - 1, xt - 1, *cy, c::SHAFT);
        // casing over the wheel's two ends: the gas gets in and out only through the nozzle above and the outlet below
        if s.input.expander {
            // (an expander's wheel has no gas round it, but it still sits in a casing: a cell of steel all round, with the
            // fuel line coming in through the top and out of the bottom)
            steel(&mut b, xt - 1, cy - lo(*dt) - 1, xt + lt, cy - lo(*dt) + dt);
        } else {
            steel(&mut b, xt - 2, cy - lo(*dt), xt - 1, cy - lo(*dt) + dt - 1);
            steel(&mut b, xt + lt, cy - lo(*dt), xt + lt + 1, cy - lo(*dt) + dt - 1);
        }
    }
    // gas generator: injector head, chamber, reducer and nozzle in a thick steel body on top of the first wheel
    let tapoff = s.input.tapoff;
    let expander = s.input.expander;
    if expander {
        // expander: no gas path. The warm fuel comes in at the top, runs through the first wheel, on down through the
        // second (two shafts), and leaves at the bottom for the injectors
        stub(&mut b, xc - 1, ty0 - 1, 0, -1, ty0 - 1);
        if twin {
            b.rect(xc - 1, by0 + 1, xc, ty1 - 1, c::PIPE);
        }
        stub(&mut b, xc - 1, tb + 1, 0, 1, h - 2 - tb);
    }
    if !tapoff && !expander {
        b.rect(cx0, yg, cx1, yg + 1, if burner { c::BURNER } else { c::GG });
        // its two feeds come up on the back layer and in through a valve each side of the head (the taps are upstream of
        // the main valves: without these the propellants would run through the generator as soon as the tanks are on)
        for (x_in, dir) in [(cx0 - 1, -1), (cx1 + 1, 1)] {
            b.rect(x_in.min(x_in + 3 * dir), yg, x_in.max(x_in + 3 * dir), yg + 1, c::PIPE);
            b.rect((x_in + dir).min(x_in + 2 * dir), yg, (x_in + dir).max(x_in + 2 * dir), yg + 1, c::VALVE_X);
            b.rect(x_in + 4 * dir, yg, x_in + 4 * dir, yg + 1, c::JOINT);
        }
        if burner {
            b.rect(cx0, yg + 2, cx1, yg + 1 + hc, c::BURNER);
        } else {
            carve(&mut b, cx0, yg + 2, cx1, yg + 1 + hc);
        }
        for k in 0..nf {
            carve(&mut b, cx0 + k + 1, yg + 2 + hc + k, cx1 - k - 1, yg + 2 + hc + k);
        }
    }
    // (tap-off: just the nozzle through the casing, open on top for the duct from the chamber)
    if !expander {
        carve(&mut b, nx0, ty0 - 2, nx0 + n0 - 1, ty0 - 1);
    }
    if !tapoff && !expander {
        steel(&mut b, cx0 - 5, yg - 2, cx1 + 5, ty0 - 1);
    }
    if !expander {
        steel(&mut b, xt0 - 2, ty0 - 2, xt0 + l0 + 1, ty0 - 1);
    }
    if twin && !expander {
        // between the wheels: collector, reducer, nozzle
        carve(&mut b, xt0, by0 + 1, xt0 + l0 - 1, by0 + coll);
        let toward = |from: i32, to: i32, step: i32| if to >= from { (from + step).min(to) } else { (from - step).max(to) };
        for r in 0..nf2 {
            let y = by0 + 1 + coll + r;
            carve(&mut b, toward(xt0, nx1, 2 * (r + 1)), y, toward(xt0 + l0 - 1, nx1 + n1 - 1, 2 * (r + 1)), y);
        }
        carve(&mut b, nx1, ty1 - 2, nx1 + n1 - 1, ty1 - 1);
        steel(&mut b, xt0.min(xt1) - 2, by0 + 1, (xt0 + l0).max(xt1 + l1) + 1, ty1 - 1);
    }
    // exhaust: straight down from the last wheel and out of the bottom edge
    if !expander {
        carve(&mut b, xtl, tb + 1, xtl + ll - 1, h - 1);
        steel(&mut b, xtl - 1, tb + 1, xtl - 1, h - 1);
        steel(&mut b, xtl + ll, tb + 1, xtl + ll, h - 1);
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
    if !tapoff && !expander {
        b.rect(xj, cy0 - 1, xj + 1, cy0, c::JOINT);
        b.back_rect(xj, yg, xj + 1, cy0);
        b.back_rect(xj, yg, cx0 - 5, yg + 1);
    }
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
    if !tapoff && !expander {
        b.rect(xom, ob + 2, xom + 1, ob + 3, c::JOINT);
    }
    if ox_valves {
        b.rect(xom, ob + 6, xom + 1, ob + 7, c::VALVE_X);
        b.rect(xom, ob + 10, xom + 1, ob + 10, c::THROTTLE);
    }
    ports.push(Port { name: "oxidiser out (to the injectors)", x: xom, y: h - 1 });
    if !tapoff && !expander {
        b.back_rect(xom, ob + 2, xb + 1, ob + 3);
        b.back_rect(xb, yg, xb + 1, ob + 3);
        b.back_rect(cx1 + 5, yg, xb + 1, yg + 1);
    }
    ports.push(Port { name: "turbine exhaust", x: xtl, y: h - 1 });
    if tapoff {
        ports.push(Port { name: "drive gas in", x: nx0, y: ty0 - 2 });
    }
    if expander {
        ports.push(Port { name: "warm fuel in", x: xc - 1, y: 0 });
        ports.push(Port { name: "fuel to the injectors", x: xc - 1, y: h - 1 });
    }
    (b.clip(), ports)
}

/// Put the sizing into the engine settings: generator orifices and pressure, the pump governor and the
/// start cartridge.
pub fn apply(s: &PhSize, st: &mut Settings) {
    if !s.input.expander && !s.input.tapoff {
        st.gg_area_f = s.gg_area_f;
        st.gg_area_o = s.gg_area_o;
        st.gg_p = s.gg_p;
        // the generator's oxidiser valve holds the gas at the temperature the turbines were sized for
        st.gg_t_target = s.gg_t;
    }
    // (with no generator, as in an expander or tap-off powerhead, the generator settings are left for any the player adds)
    st.feed_p_max = s.feed_p_max;
    st.cartridge_w = s.cartridge_w;
    // the throttle valves trim the mixture to the best ratio, whatever the lines lose
    st.mix_target = PROPS[s.input.prop.min(PROPS.len() - 1)].of_opt;
    st.throttle = 1.0;
}

/// The part's ports as it is drawn, for laying an engine out round it: the turbine's length and nozzle width.
pub fn last_wheel(s: &PhSize) -> (i32, i32) {
    s.shafts.last().map_or((8, 1), |sh| (sh.turb_len, sh.nozzle))
}

/// A whole engine around a generated powerhead (for tests): tanks above it, a regen-cooled chamber
/// below, everything piped up and the injectors sized.
pub fn demo_engine(w: &mut crate::world::World, i: &PhInput) -> PhSize {
    use crate::world::World;
    w.clear();
    w.st.prop = i.prop;
    w.st.throat_cells = i.throat_cells;
    w.st.gse_tanks = true;
    let s = size(&PhInput { dx: w.st.dx, depth: w.st.depth, axi: w.st.axi, tank_p_f: w.st.tank_p_f, tank_p_o: w.st.tank_p_o, steps: w.st.steps_per_frame, ..*i });
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
    fn every_setting_of_the_maker_sizes_and_draws() {
        let st = Settings::default();
        for prop in 0..PROPS.len() {
            for layout in [Layout::Single, Layout::Twin] {
                for pc in [10.0e5, 35.0e5, 100.0e5, 200.0e5] {
                    for throat in [4.0, 12.0, 24.0, 40.0] {
                        for t_gg in [850.0, 1300.0] {
                            let i = PhInput { prop, pc, throat_cells: throat, layout, t_gg, ..PhInput::from_settings(&st, pc) };
                            let s = size(&i);
                            let what = format!("{} {layout:?} {:.0} bar, throat {throat}, {t_gg} K", PROPS[prop].name, pc / 1e5);
                            assert!(s.gg_mdot.is_finite() && s.gg_mdot > 0.0 && s.gg_p.is_finite() && s.gg_area_f.is_finite() && s.gg_area_o.is_finite(), "{what}");
                            for sh in &s.shafts {
                                assert!(sh.nozzle >= 1 && sh.nozzle <= sh.turb_len, "{what}: nozzle {} cells into a wheel {} long", sh.nozzle, sh.turb_len);
                                assert!(sh.turb_cells >= 6 && sh.p_in > sh.p_out, "{what}");
                            }
                            let (clip, ports) = build(&s);
                            assert_eq!(ports.len(), 5, "{what}");
                            // drawn whole: generator head, a wheel for every shaft, and the exhaust open at the bottom edge
                            let count = |t: u8| clip.cells.iter().filter(|&&v| v == t).count() as i32;
                            assert!(count(c::GG) >= 16, "{what}");
                            assert_eq!(count(c::TURBINE), s.shafts.iter().map(|sh| sh.turb_cells * sh.turb_len).sum::<i32>(), "{what}");
                            let ex = ports.iter().find(|p| p.name == "turbine exhaust").unwrap();
                            assert_eq!(clip.cells[ex.y as usize * clip.w + ex.x as usize], c::EMPTY, "{what}");
                        }
                    }
                }
            }
        }
    }

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
                // as placed, every valve shut: nothing reaches the generator (its feeds tap the pumps upstream of the
                // main valves, so they have valves of their own), and the start sequence has four valves to open
                assert_eq!(w.gg.len(), 0, "{what}: propellant reaches the generator past shut valves");
                assert_eq!(w.valves.len(), 4, "{what}: two main valves and two generator valves");
                w.command_all(0, true);
                w.command_all(1, true);
                // (the plumbing stepped without the gas solver while the valves travel open)
                let rb = crate::world::Readback::default();
                for _ in 0..250 {
                    w.update(&rb, 0.016);
                }
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
                for ss in &s.shafts {
                    // the shaft as the simulation reads the drawing back
                    let ws = w.shafts.iter().find(|ws| ws.pumps.iter().any(|q| q.prop == ss.pumps[0].prop)).expect("its shaft");
                    assert_eq!(ws.pumps.len(), ss.pumps.len(), "{what}");
                    assert_eq!(ws.turb_stages as i32, ss.turb_stages, "{what}: turbine stages");
                    // the drive gas is ducted to the wheel and gets in only through the nozzle
                    assert!(ws.gas_driven, "{what}: generator ducted to the turbine");
                    assert_eq!(ws.turb_in.len() as i32, ss.nozzle, "{what}: nozzle cells");
                    assert!(ws.turb_faces.len() > ws.turb_in.len(), "{what}: the wheel has an outlet");
                    // a wheel in proportion to its pumps, with a real pressure drop across the whole gas path
                    let pump_max = ss.pumps.iter().map(|q| q.cells).max().unwrap();
                    assert!(ss.turb_cells as f32 <= 1.2 * pump_max as f32 + 1.0, "{what}: turbine {} cells against a {} cell pump", ss.turb_cells, pump_max);
                    assert!(ss.p_in > ss.p_out, "{what}");
                }
                assert!(s.gg_p > 8.0e5 && s.gg_p < 0.8 * s.p_dis_f.min(s.p_dis_o), "{what}: generator at {:.0} bar", s.gg_p / 1e5);
                assert!(s.gg_mdot < 0.08 * (s.mf + s.mo), "{what}: generator takes {:.1}% of the propellant", 100.0 * s.gg_mdot / (s.mf + s.mo));
                assert_eq!(w.gg.len(), 1, "{what}: gas generator fed from both pumps");
                assert!(w.st.inj_area_f > 0.0 && w.st.inj_area_o > 0.0);
            }
        }
    }
}
