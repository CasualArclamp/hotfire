//! Design tools on top of the world: nozzle maker, injector auto-sizing, save/load.

use crate::world::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Contour {
    Bell,
    Conical,
}

#[derive(Clone, Copy)]
pub struct NozzleSpec {
    pub cx: f32,
    pub y_throat: i32,
    pub y_exit: i32,
    pub throat_hw: f32, // half width (slab) or radius (round) of the throat, in cells
    pub expansion: f32, // exit area / throat area
    pub contour: Contour,
    pub liner: i32,
    pub jacket: i32,
    pub shell: i32,
    pub axi: bool,
    pub liner_mat: u8, // copper (regen), niobium (radiation cooled), ceramic (ablative) or steel
}

impl NozzleSpec {
    pub fn exit_hw(&self) -> f32 {
        self.throat_hw * if self.axi { self.expansion.max(1.0).sqrt() } else { self.expansion.max(1.0) }
    }
    /// Gas half width at row y (cells), measured from the axis.
    pub fn hw(&self, y: f32) -> f32 {
        let len = (self.y_exit - self.y_throat).max(1) as f32;
        let t = ((y - self.y_throat as f32) / len).clamp(0.0, 1.0);
        let (h0, h1) = (self.throat_hw, self.exit_hw());
        match self.contour {
            Contour::Bell => h0 + (h1 - h0) * (1.0 - (1.0 - t).powf(2.2)),
            Contour::Conical => h0 + (h1 - h0) * t,
        }
    }
    pub fn thickness(&self) -> f32 {
        (self.liner + self.jacket + self.shell) as f32
    }
}

/// Run-length encode a grid: "count*type" tokens, 16 per line.
fn rle(cells: &[u8]) -> String {
    let mut o = String::new();
    let mut run = 0usize;
    let mut cur = cells[0];
    let mut toks = 0;
    for &v in cells.iter().chain(std::iter::once(&255u8)) {
        if v == cur {
            run += 1;
        } else {
            o += &format!("{run}*{cur} ");
            toks += 1;
            if toks % 16 == 0 {
                o += "\n";
            }
            cur = v;
            run = 1;
        }
    }
    o
}

impl World {
    /// Stamp a nozzle: gas channel, liner (copper, niobium, ...), fuel jacket and steel shell, symmetric about `cx`.
    pub fn build_nozzle(&mut self, n: &NozzleSpec) {
        let (y0, y1) = (n.y_throat.min(n.y_exit), n.y_throat.max(n.y_exit));
        let m = n.thickness();
        for y in y0.max(0)..=y1.min(H as i32 - 1) {
            let h = n.hw(y as f32 + 0.5);
            let reach = (h + m + 1.0).ceil() as i32;
            for dx in -reach..=reach {
                let x = (n.cx + dx as f32).floor() as i32;
                if x < 0 || x >= W as i32 {
                    continue;
                }
                let d = (x as f32 + 0.5 - n.cx).abs();
                let v = if d < h {
                    c::EMPTY
                } else if d < h + n.liner as f32 {
                    n.liner_mat
                } else if d < h + (n.liner + n.jacket) as f32 {
                    c::PIPE
                } else if d < h + m {
                    c::STEEL
                } else {
                    continue;
                };
                self.set_cell(Self::idx(x as usize, y as usize), v);
            }
        }
        // cap the coolant jacket's open ends so it doesn't spill
        for y in y0.max(0)..=y1.min(H as i32 - 1) {
            for x in 0..W {
                let i = Self::idx(x, y as usize);
                if self.cells[i] == c::PIPE && (y == y0 || y == y1) {
                    self.wall_around(i);
                }
            }
        }
    }

    /// What a cell holds on a layer: 0 the engine plane, 1 the back pipe layer, 2 the front.
    pub fn layer_cell(&self, layer: u8, i: usize) -> u8 {
        match layer {
            1 => self.back[i],
            2 => self.front[i],
            _ => self.cells[i],
        }
    }

    /// Flood-fill area: every cell joined (side by side) to `start` holding the same as it on that layer.
    pub fn flood_region(&self, layer: u8, start: usize) -> Vec<usize> {
        let target = self.layer_cell(layer, start);
        let mut seen = vec![false; N];
        let mut region = vec![];
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            region.push(i);
            let (x, y) = (i % W, i / W);
            for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                if nx < W && ny < H {
                    let j = World::idx(nx, ny);
                    if !seen[j] && self.layer_cell(layer, j) == target {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        region
    }

    /// The propellant networks as they will be with every valve open: main-layer cells of each, joined
    /// across the pipe layers at joints (a fuel and an oxidiser tank never merge directly).
    pub fn open_valve_networks(&self) -> Vec<Vec<usize>> {
        let cells = &self.cells;
        let conducts = |l: usize, i: usize| match l {
            0 => c::plumbing(cells[i]) || cells[i] == c::VALVE_X,
            1 => self.back[i] == c::PIPE,
            _ => self.front[i] == c::PIPE,
        };
        let mut seen = vec![false; 3 * N];
        let mut out = vec![];
        for s0 in 0..N {
            if seen[s0] || !conducts(0, s0) {
                continue;
            }
            let mut comp = vec![];
            let mut stack = vec![s0];
            seen[s0] = true;
            while let Some(nd) = stack.pop() {
                let (l, i) = (nd / N, nd % N);
                if l == 0 {
                    comp.push(i);
                }
                let (x, y) = (i % W, i / W);
                let mut next = vec![];
                for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                    if nx < W && ny < H {
                        let j = World::idx(nx, ny);
                        let tank_pair = l == 0 && matches!((cells[i], cells[j]), (c::TANK_F, c::TANK_O) | (c::TANK_O, c::TANK_F));
                        if conducts(l, j) && !tank_pair {
                            next.push(l * N + j);
                        }
                    }
                }
                if cells[i] == c::JOINT {
                    for l2 in 0..3 {
                        if l2 != l && conducts(l2, i) {
                            next.push(l2 * N + i);
                        }
                    }
                }
                for n2 in next {
                    if !seen[n2] {
                        seen[n2] = true;
                        stack.push(n2);
                    }
                }
            }
            out.push(comp);
        }
        out
    }

    /// Size injector orifices so the current engine would hold `pc` (Pa) at the optimum mixture ratio.
    pub fn auto_size_injectors(&mut self, pc: f32) {
        if self.nets_dirty {
            self.rebuild_nets();
        }
        let st = self.st;
        let p = st.p();
        let a_t = if st.axi {
            std::f32::consts::PI * (st.throat_cells * 0.5 * st.dx).powi(2)
        } else {
            st.throat_cells * st.dx * st.depth
        };
        let g = p.gamma;
        let big_gamma = g.sqrt() * (2.0 / (g + 1.0)).powf((g + 1.0) / (2.0 * (g - 1.0)));
        let cstar = (p.rgas * t_flame(p, p.of_opt)).sqrt() / big_gamma;
        let mdot = pc * a_t / cstar;
        let mf = mdot / (1.0 + p.of_opt);
        let mo = mdot - mf;
        // which tank feeds each injector once the valves are open (pipes hold whatever they reach)
        // which tank feeds each injector once the valves are open, following back- and front-layer pipes
        // through their joints
        let (mut nf, mut no) = (0.0f32, 0.0f32);
        for comp in self.open_valve_networks() {
            let has_f = comp.iter().any(|&i| self.cells[i] == c::TANK_F);
            let has_o = comp.iter().any(|&i| self.cells[i] == c::TANK_O);
            if has_f == has_o {
                continue; // no tank, or both: nothing sensible to size for
            }
            let w: f32 = comp.iter().filter(|&&i| self.cells[i] == c::INJ).map(|&i| st.inj_w(i % W)).sum();
            if has_f {
                nf += w;
            } else {
                no += w;
            }
        }
        // injectors take a stiff 25% pressure drop; the turbopumps then settle at whatever speed gives that head
        let dp = 0.25 * pc;
        // the governor holds pump discharge at the design feed pressure, not the hardware limit
        self.st.feed_p_max = 1.05 * (pc + dp);
        // The pumps do not make the same pressure on both sides: each shaft turns until its highest discharge reaches the
        // governor's limit, and the other pump then makes whatever its own curve gives at that speed. So the pressure left over
        // for each set of injectors, at the design flows, is read off the real pump curves, and the orifices are sized for that
        // (not for a flat 25%), which is what makes the sized pumps deliver the design mixture ratio.
        // (The gas pressure the injectors push against is not the chamber's average: at the oxidiser injectors, at the head end,
        // it reads higher and at the fuel injectors, down the side, lower; measured on the standard chambers. The pumps the
        // powerhead sizes make head for that, and the governor is set for it as far as the pumps can reach.)
        use crate::powerhead::{LOCAL_F, LOCAL_O};
        {
            let mut need = 0.0f32;
            let mut cap = f32::MAX;
            for (prop, rho, tank, loc, extra) in [(0u8, p.rho_f, st.tank_p_f, LOCAL_F, 0.35), (1, p.rho_o, st.tank_p_o, LOCAL_O, 0.30)] {
                let heads: Vec<f32> = self
                    .shafts
                    .iter()
                    .flat_map(|s| s.pumps.iter().map(move |q| (s.omega_rated, q)))
                    .filter(|(_, q)| q.prop == prop)
                    .map(|(w, q)| q.head_at(w, q.bep_flow(w, rho), rho))
                    .collect();
                if !heads.is_empty() {
                    need = need.max((loc + extra) * pc);
                    cap = cap.min(tank + heads.iter().sum::<f32>());
                }
            }
            if need > 0.0 {
                self.st.feed_p_max = self.st.feed_p_max.max((1.05 * need).min(0.97 * cap));
            }
        }
        let (mut dp_f, mut dp_o) = (dp, dp);
        let (tank_f, tank_o) = (st.tank_p_f, st.tank_p_o);
        for sh in &self.shafts {
            if sh.pumps.is_empty() || sh.omega_rated <= 0.0 {
                continue;
            }
            let head_of = |w: f32| {
                let mut h = [0.0f32; 2];
                let mut has = [false; 2];
                for q in &sh.pumps {
                    let (m, rho) = if q.prop == 0 { (mf, p.rho_f) } else { (mo, p.rho_o) };
                    h[q.prop as usize & 1] += q.head_at(w, m, rho).max(0.0);
                    has[q.prop as usize & 1] = true;
                }
                let d = [if has[0] { tank_f + h[0] } else { 0.0 }, if has[1] { tank_o + h[1] } else { 0.0 }];
                (d, d[0].max(d[1]))
            };
            let (mut lo, mut hi) = (0.05 * sh.omega_rated, 1.2 * sh.omega_rated);
            if head_of(hi).1 < self.st.feed_p_max {
                lo = hi; // it cannot get there (reported above): at the top speed
            } else {
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if head_of(mid).1 < self.st.feed_p_max {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
            }
            let (d, _) = head_of(lo);
            if sh.pumps.iter().any(|q| q.prop == 0) {
                dp_f = (d[0] - LOCAL_F * pc).max(0.1 * pc);
            }
            if sh.pumps.iter().any(|q| q.prop == 1) {
                dp_o = (d[1] - LOCAL_O * pc).max(0.1 * pc);
            }
        }
        for (prop, rho, tank) in [(0u8, p.rho_f, st.tank_p_f), (1, p.rho_o, st.tank_p_o)] {
            let pumped = self.shafts.iter().any(|s| s.pumps.iter().any(|q| q.prop == prop));
            if !pumped {
                // no pump on this side: pressure-fed, the tank itself has to supply the feed pressure
                if prop == 0 {
                    self.st.tank_p_f = pc + dp;
                } else {
                    self.st.tank_p_o = pc + dp;
                }
                continue;
            }
            {
                let best: f32 = self
                    .shafts
                    .iter()
                    .flat_map(|s| s.pumps.iter().map(move |q| (s.omega_rated, q)))
                    .filter(|(_, q)| q.prop == prop)
                    .map(|(w, q)| q.head_at(w, q.bep_flow(w, rho), rho))
                    .sum();
                if best + tank < pc + dp {
                    let name = if prop == 0 { "fuel" } else { "oxidiser" };
                    self.event(format!("{name} pumps reach only {:.0} bar at rated speed; need {:.0} bar (bigger impeller or more stages)", (best + tank) / 1e5, (pc + dp) / 1e5));
                }
            }
        }
        if nf > 0.0 {
            self.st.inj_area_f = (mf / nf / (0.7 * (2.0 * p.rho_f * dp_f).sqrt()) * 1e4).clamp(0.02, 50.0);
        }
        if no > 0.0 {
            self.st.inj_area_o = (mo / no / (0.7 * (2.0 * p.rho_o * dp_o).sqrt()) * 1e4).clamp(0.02, 50.0);
        }
    }

    pub fn to_text(&self) -> String {
        let s = &self.st;
        let mut o = String::from("ROCKET2\n");
        for (k, v) in [
            ("prop", s.prop as f32),
            ("dx", s.dx),
            ("axi", s.axi as u8 as f32),
            ("depth", s.depth),
            ("throat_cells", s.throat_cells),
            ("tank_p_f", s.tank_p_f),
            ("tank_p_o", s.tank_p_o),
            ("dp_stage", s.dp_stage),
            ("turb_mult", s.turb_mult),
            ("inj_area_f", s.inj_area_f),
            ("inj_area_f2", s.inj_area_f2),
            ("inj_area_o", s.inj_area_o),
            ("gg_area_f", s.gg_area_f),
            ("gg_area_o", s.gg_area_o),
            ("gg_p", s.gg_p),
            ("feed_p_max", s.feed_p_max),
            ("heat_accel", s.heat_accel),
            ("h0", s.h0),
            ("throttle", s.throttle),
            ("mix_target", s.mix_target),
            ("gg_t_target", s.gg_t_target),
            ("cartridge_w", s.cartridge_w),
        ] {
            o += &format!("{k} {v}\n");
        }
        o += "cells\n";
        o += &rle(&self.cells);
        o += "\nback\n";
        o += &rle(&self.back);
        o += "\nfront\n";
        o += &rle(&self.front);
        o += "\n";
        // regulators with their own set pressure: cell index and pressure (Pa)
        let regs: Vec<String> = (0..N).filter(|&i| self.cells[i] == c::REG && self.reg_p[i] > 0.0).map(|i| format!("{i} {}", self.reg_p[i])).collect();
        if !regs.is_empty() {
            o += "regs\n";
            o += &regs.join("\n");
            o += "\n";
        }
        if self.fluid.iter().any(|&f| f > 0) {
            o += "fluid\n";
            o += &rle(&self.fluid);
            o += "\n";
        }
        o
    }

    pub fn from_text(&mut self, text: &str) -> Result<(), String> {
        let mut lines = text.lines();
        if lines.next() != Some("ROCKET2") {
            return Err("not a ROCKET2 design".into());
        }
        let mut st = self.st;
        // sections: settings, then "cells", optionally "back" and "front" pipe layers
        let mut grids: [Vec<u8>; 4] = [Vec::with_capacity(N), vec![], vec![], vec![]];
        let mut section: Option<usize> = None;
        let mut regs: Vec<(usize, f32)> = vec![];
        let (mut old_pf, mut old_he) = (false, 0.0f32);
        for line in lines {
            match line {
                "regs" => {
                    section = Some(3);
                    continue;
                }
                "fluid" => {
                    section = Some(4);
                    continue;
                }
                "cells" => {
                    section = Some(0);
                    continue;
                }
                "back" => {
                    section = Some(1);
                    continue;
                }
                "front" => {
                    section = Some(2);
                    continue;
                }
                _ => {}
            }
            if section == Some(3) {
                let mut it = line.split_whitespace();
                if let (Some(Ok(i)), Some(Ok(p))) = (it.next().map(str::parse::<usize>), it.next().map(str::parse::<f32>)) {
                    regs.push((i, p));
                }
                continue;
            }
            if let Some(k) = section {
                let k = if k == 4 { 3 } else { k };
                for tok in line.split_whitespace() {
                    let (n, v) = tok.split_once('*').ok_or("bad token")?;
                    let n: usize = n.parse().map_err(|_| "bad run length")?;
                    let v: u8 = v.parse().map_err(|_| "bad cell type")?;
                    grids[k].extend(std::iter::repeat(v).take(n));
                }
                continue;
            }
            let Some((k, v)) = line.split_once(' ') else { continue };
            let Ok(f) = v.parse::<f32>() else { continue };
            match k {
                "prop" => st.prop = (f as usize).min(PROPS.len() - 1),
                // older saves: a pressure-fed engine held both tanks at the regulator setting
                "pressure_fed" => old_pf = f > 0.5,
                "he_setpoint" => old_he = f,
                "dx" => st.dx = f,
                "axi" => st.axi = f > 0.5,
                "depth" => st.depth = f,
                "throat_cells" => st.throat_cells = f,
                "tank_p_f" => st.tank_p_f = f,
                "tank_p_o" => st.tank_p_o = f,
                "dp_stage" => st.dp_stage = f,
                "turb_mult" => st.turb_mult = f,
                "inj_area_f" => st.inj_area_f = f,
                "inj_area_f2" => st.inj_area_f2 = f,
                "inj_area_o" => st.inj_area_o = f,
                "gg_area_f" => st.gg_area_f = f,
                "gg_area_o" => st.gg_area_o = f,
                "cartridge_w" => st.cartridge_w = f,
                "gg_p" => st.gg_p = f,
                "feed_p_max" => st.feed_p_max = f,
                "heat_accel" => st.heat_accel = f,
                "h0" => st.h0 = f,
                "throttle" => st.throttle = f.clamp(0.0, 1.0),
                "mix_target" => st.mix_target = f.clamp(0.0, 20.0),
                "gg_t_target" => st.gg_t_target = f.clamp(0.0, 1300.0),
                _ => {}
            }
        }
        if old_pf && old_he > 0.0 {
            st.tank_p_f = old_he;
            st.tank_p_o = old_he;
        }
        let [mut cells, mut back, mut front, mut fluid] = grids;
        // designs saved when the grid was shorter (the same width, fewer rows) load with the new rows below left empty
        for g in [&mut cells, &mut back, &mut front, &mut fluid] {
            if !g.is_empty() && g.len() < N && g.len() % W == 0 {
                g.resize(N, 0);
            }
        }
        self.fluid = if fluid.len() == N { fluid } else { vec![0; N] };
        for g in [&mut cells, &mut back, &mut front] {
            for v in g.iter_mut() {
                *v = c::canon(*v);
            }
        }
        if cells.len() != N {
            return Err(format!("expected {N} cells, found {}", cells.len()));
        }
        if cells.iter().any(|&v| v >= c::COUNT) {
            return Err("unknown cell type".into());
        }
        let layer = |g: Vec<u8>| -> Result<Vec<u8>, String> {
            if g.is_empty() {
                return Ok(vec![0; N]);
            }
            if g.len() != N || g.iter().any(|&v| !matches!(v, c::EMPTY | c::PIPE)) {
                return Err("bad pipe layer".into());
            }
            Ok(g)
        };
        self.back = layer(back)?;
        self.front = layer(front)?;
        self.cells = cells;
        self.st = st;
        self.reg_p = vec![0.0; N];
        for (i, p) in regs {
            if i < N {
                self.reg_p[i] = p;
            }
        }
        self.dirty.clear();
        self.nets_dirty = true;
        // (the network table is rebuilt on the next update: until then no cell may point into the old one, or a panel that
        // looks a cell's network up between the load and the rebuild reads past the end)
        self.nets.clear();
        self.net_of = vec![NO_NET; N];
        self.layer_net = [vec![NO_NET; N], vec![NO_NET; N]];
        self.valve_dests.clear();
        Ok(())
    }
}

/// A copied piece of the build, like Powder Toy's clipboard and stamps: parts, back / front layer pipes and
/// regulator settings in a w x h box. Empty cells are left alone when it is pasted.
#[derive(Clone)]
pub struct Clip {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<u8>,
    pub back: Vec<u8>,
    pub front: Vec<u8>,
    pub reg_p: Vec<f32>,
    pub fluid: Vec<u8>, // tank contents
}

impl Clip {
    /// The same box rearranged: cell (x, y) of the new w x h box comes from cell `from(x, y)` of this one.
    fn remap(&self, w: usize, h: usize, from: impl Fn(usize, usize) -> usize) -> Clip {
        let idx: Vec<usize> = (0..w * h).map(|k| from(k % w, k / w)).collect();
        Clip {
            w,
            h,
            cells: idx.iter().map(|&j| self.cells[j]).collect(),
            back: idx.iter().map(|&j| self.back[j]).collect(),
            front: idx.iter().map(|&j| self.front[j]).collect(),
            reg_p: idx.iter().map(|&j| self.reg_p[j]).collect(),
            fluid: idx.iter().map(|&j| self.fluid[j]).collect(),
        }
    }

    /// A quarter turn clockwise.
    pub fn rotated(&self) -> Clip {
        let (w, h) = (self.w, self.h);
        self.remap(h, w, |x, y| (h - 1 - x) * w + y)
    }

    /// Mirrored left to right.
    pub fn flipped(&self) -> Clip {
        let w = self.w;
        self.remap(w, self.h, |x, y| y * w + (w - 1 - x))
    }

    /// Mirrored top to bottom.
    pub fn flipped_v(&self) -> Clip {
        let (w, h) = (self.w, self.h);
        self.remap(w, h, |x, y| (h - 1 - y) * w + x)
    }

    pub fn to_text(&self) -> String {
        let mut o = format!("STAMP1\nsize {} {}\ncells\n", self.w, self.h);
        o += &rle(&self.cells);
        o += "\nback\n";
        o += &rle(&self.back);
        o += "\nfront\n";
        o += &rle(&self.front);
        o += "\n";
        let regs: Vec<String> = (0..self.cells.len()).filter(|&i| self.reg_p[i] > 0.0).map(|i| format!("{i} {}", self.reg_p[i])).collect();
        if !regs.is_empty() {
            o += "regs\n";
            o += &regs.join("\n");
            o += "\n";
        }
        if self.fluid.iter().any(|&f| f > 0) {
            o += "fluid\n";
            o += &rle(&self.fluid);
            o += "\n";
        }
        o
    }

    pub fn from_text(text: &str) -> Result<Clip, String> {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some("STAMP1") {
            return Err("not a stamp".into());
        }
        let (mut w, mut h) = (0usize, 0usize);
        let mut grids: [Vec<u8>; 5] = [vec![], vec![], vec![], vec![], vec![]];
        let mut regs: Vec<(usize, f32)> = vec![];
        let mut section: Option<usize> = None;
        for line in lines {
            match line.trim() {
                "cells" => section = Some(0),
                "back" => section = Some(1),
                "front" => section = Some(2),
                "regs" => section = Some(3),
                "fluid" => section = Some(4),
                l if l.starts_with("size ") => {
                    let v: Vec<usize> = l[5..].split_whitespace().filter_map(|t| t.parse().ok()).collect();
                    if v.len() == 2 {
                        (w, h) = (v[0], v[1]);
                    }
                }
                l => match section {
                    Some(3) => {
                        let mut it = l.split_whitespace();
                        if let (Some(Ok(i)), Some(Ok(p))) = (it.next().map(str::parse::<usize>), it.next().map(str::parse::<f32>)) {
                            regs.push((i, p));
                        }
                    }
                    Some(k) => {
                        for tok in l.split_whitespace() {
                            let (n, v) = tok.split_once('*').ok_or("bad token")?;
                            let n: usize = n.parse().map_err(|_| "bad run length")?;
                            let v: u8 = v.parse().map_err(|_| "bad cell type")?;
                            if grids[k].len() + n > W * H {
                                return Err("stamp too big".into());
                            }
                            grids[k].extend(std::iter::repeat(c::canon(v)).take(n));
                        }
                    }
                    None => {}
                },
            }
        }
        let n = w * h;
        if n == 0 || w > W || h > H || grids[0].len() != n {
            return Err("stamp size does not match its cells".into());
        }
        if grids[0].iter().any(|&v| v >= c::COUNT) {
            return Err("unknown cell type".into());
        }
        let [cells, back, front, _, fluid] = grids;
        let fluid = if fluid.len() == n { fluid } else { vec![0; n] };
        let layer = |g: Vec<u8>| if g.len() == n && g.iter().all(|&v| matches!(v, c::EMPTY | c::PIPE)) { g } else { vec![0; n] };
        let mut reg_p = vec![0.0; n];
        for (i, p) in regs {
            if i < n {
                reg_p[i] = p;
            }
        }
        Ok(Clip { w, h, cells, back: layer(back), front: layer(front), reg_p, fluid })
    }

    /// Nothing in it worth pasting.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().chain(&self.back).chain(&self.front).all(|&v| v == c::EMPTY)
    }
}

impl World {
    /// Copy the box between two corners (inclusive, either order), clipped to the world.
    pub fn copy_region(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Clip {
        let (xa, xb) = (x0.min(x1).clamp(0, W as i32 - 1) as usize, x0.max(x1).clamp(0, W as i32 - 1) as usize);
        let (ya, yb) = (y0.min(y1).clamp(0, H as i32 - 1) as usize, y0.max(y1).clamp(0, H as i32 - 1) as usize);
        let (w, h) = (xb - xa + 1, yb - ya + 1);
        let idx: Vec<usize> = (0..w * h).map(|k| World::idx(xa + k % w, ya + k / w)).collect();
        Clip {
            w,
            h,
            cells: idx.iter().map(|&i| self.cells[i]).collect(),
            back: idx.iter().map(|&i| self.back[i]).collect(),
            front: idx.iter().map(|&i| self.front[i]).collect(),
            reg_p: idx.iter().map(|&i| if self.cells[i] == c::REG { self.reg_p[i] } else { 0.0 }).collect(),
            // tanks keep what they hold (one not chosen yet holds what the engine burns now)
            fluid: idx
                .iter()
                .map(|&i| if matches!(self.cells[i], c::TANK_F | c::TANK_O) { self.tank_kind(i) as u8 + 1 } else { 0 })
                .collect(),
        }
    }

    /// Erase everything in a box (as the eraser does): parts, layer pipes and any painted gas.
    pub fn erase_region(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        for y in y0.min(y1).max(0)..=y0.max(y1).min(H as i32 - 1) {
            for x in x0.min(x1).max(0)..=x0.max(x1).min(W as i32 - 1) {
                let i = World::idx(x as usize, y as usize);
                if self.cells[i] != c::EMPTY {
                    self.paint_gas(i, 3);
                }
                self.reg_p[i] = 0.0;
                self.set_layer_cell(1, i, c::EMPTY);
                self.set_layer_cell(2, i, c::EMPTY);
            }
        }
    }

    /// Put a copied piece in with its top-left corner at (x0, y0). Its empty cells leave what is there alone;
    /// whatever falls outside the world is dropped.
    pub fn paste(&mut self, clip: &Clip, x0: i32, y0: i32) {
        for k in 0..clip.w * clip.h {
            let (x, y) = (x0 + (k % clip.w) as i32, y0 + (k / clip.w) as i32);
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                continue;
            }
            let i = World::idx(x as usize, y as usize);
            let v = clip.cells[k];
            if v != c::EMPTY {
                self.set_cell(i, v);
                self.reg_p[i] = if v == c::REG { clip.reg_p[k] } else { 0.0 };
                self.fluid[i] = clip.fluid[k];
            }
            if clip.back[k] == c::PIPE {
                self.set_layer_cell(1, i, c::PIPE);
            }
            if clip.front[k] == c::PIPE {
                self.set_layer_cell(2, i, c::PIPE);
            }
        }
        self.nets_dirty = true;
        self.mat_stale = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aerospike_preset_feeds_both_slots() {
        let mut w = World::new();
        w.load_preset(10);
        // (the main valves start shut, which splits each network at the valve: open them and let the plumbing settle)
        w.command_all(0, true);
        w.command_all(1, true);
        for _ in 0..180 {
            w.update(&Readback::default(), 1.0 / 60.0);
        }
        // a fuel and an oxidiser network, each with injectors in both slot chambers, either side of the spike
        let nets: Vec<_> = w.nets.iter().filter(|n| n.prop <= 1 && !n.injectors.is_empty()).collect();
        assert_eq!(nets.len(), 2, "expected one fuel and one oxidiser network");
        let cx = W / 2;
        for n in &nets {
            let xs: Vec<usize> = n.injectors.iter().map(|&(_, g)| g % W).collect();
            assert!(*xs.iter().min().unwrap() + 15 < cx && *xs.iter().max().unwrap() > cx + 15, "injectors on only one side");
            assert!(n.tank_cells > 0, "network not connected to a tank");
        }
        // the spike is solid down the middle, and the stabiliser finds its tip (there is no bell for the plume code to find)
        let top = crate::presets::PRESET_OY;
        // a vertical stack on the axis: helium bottle, fuel tank, oxidiser tank, then the spike
        assert_eq!(w.cells[World::idx(cx, top + 15)], c::HE_TANK);
        assert_eq!(w.cells[World::idx(cx, top + 60)], c::TANK_F);
        assert_eq!(w.cells[World::idx(cx, top + 130)], c::TANK_O);
        assert_ne!(w.cells[World::idx(cx, top + 210)], c::EMPTY, "the spike is solid down the middle");
        let (tip, half) = w.find_stab_geom().expect("a stabiliser anchor");
        assert!(tip > (top + 340) as f32 && half > 20.0, "tip {tip}, half width {half}");
    }

    #[test]
    fn gas_cloud_reports_its_mixture_ratio() {
        // a gas generator's cloud reports the generator's O/F, the chamber's the engine's
        let mut w = World::new();
        w.load_preset(6);
        w.start_sequence();
        for _ in 0..900 {
            w.update(&Readback::default(), 1.0 / 60.0);
        }
        // (no GPU here: set the flows the simulation would give)
        let (fi, oi, _, _) = w.gg[0];
        w.nets[fi].gg_draw = 2.0;
        w.nets[oi].gg_draw = 3.0;
        w.ignited = true;
        w.g.of = 2.3;
        let gg_cell = w.gg_faces[0][0].1;
        let (r, who) = w.gas_of(gg_cell).expect("the generator is burning");
        assert_eq!(who, "gas generator");
        assert!((r - 1.5).abs() < 1e-4, "{r}");
        let inj = w.nets.iter().flat_map(|n| n.injectors.iter()).next().unwrap().1;
        let (r2, who2) = w.gas_of(inj).expect("the chamber is burning");
        assert_eq!(who2, "main chamber");
        assert!((r2 - w.g.of).abs() < 1e-4);
    }

    #[test]
    fn loading_over_another_engine_is_safe() {
        // a design loaded over a running engine: the panels look at valves and cells before the networks are rebuilt
        let mut w = World::new();
        w.load_preset(6);
        for _ in 0..10 {
            w.update(&Readback::default(), 1.0 / 60.0);
        }
        let t = w.to_text();
        let mut other = World::new();
        other.load_preset(0);
        for _ in 0..10 {
            other.update(&Readback::default(), 1.0 / 60.0);
        }
        other.from_text(&t).unwrap();
        for k in 0..other.valves.len() {
            let _ = other.valve_label(k);
        }
        for i in (0..crate::world::N).step_by(101) {
            let _ = other.gas_of(i);
            let _ = other.net_around(i);
        }
    }

    #[test]
    fn alternating_injector_feeds_both_propellants() {
        // the hypergolic pressure-fed engine with its injector row swapped for one alternating injector: every other cell
        // sprays fuel, the ones between oxidiser
        let mut w = World::new();
        w.load_preset(3);
        let row: Vec<usize> = (0..N).filter(|&i| w.cells[i] == c::INJ).collect();
        assert!(row.len() > 10);
        let y = row[0] / W;
        let (x0, x1) = (row.iter().map(|&i| i % W).min().unwrap(), row.iter().map(|&i| i % W).max().unwrap());
        // (one unbroken line, so one injector touching both manifolds)
        for x in x0..=x1 {
            w.set_cell(World::idx(x, y), c::AINJ);
        }
        for p in [0, 1] {
            w.command_all(p, true);
        }
        for _ in 0..60 {
            w.update(&Readback::default(), 1.0 / 60.0);
        }
        let (mut nf, mut no) = (0, 0);
        for n in &w.nets {
            let k = n.injectors.iter().filter(|&&(i, _)| w.cells[i] == c::AINJ).count();
            match n.prop {
                0 => nf += k,
                1 => no += k,
                _ => {}
            }
        }
        let xs: Vec<(usize, usize)> = (0..N).filter(|&i| w.cells[i] == c::AINJ).map(|i| (i % W, i / W)).collect();
        assert!(nf >= 4 && no >= 4 && nf.abs_diff(no) <= 1, "fuel {nf}, oxidiser {no}, cells {xs:?}");
    }

    #[test]
    fn valves_say_where_they_go() {
        // the gas-generator aerospike: two main valves feed the chamber, two feed the generator
        let mut w = World::new();
        w.load_preset(12);
        for _ in 0..10 {
            w.update(&Readback::default(), 1.0 / 60.0);
        }
        let labels: Vec<String> = (0..w.valves.len()).map(|k| w.valve_label(k)).collect();
        // and a Merlin-style engine: the generator's valves and the main ones
        let mut m = World::new();
        m.load_preset(6);
        for _ in 0..10 {
            m.update(&Readback::default(), 1.0 / 60.0);
        }
        let ml: Vec<String> = (0..m.valves.len()).map(|v| m.valve_label(v)).collect();
        assert_eq!(ml.iter().filter(|l| l.contains("to gas generator")).count(), 2, "{ml:?}");
        assert_eq!(ml.iter().filter(|l| l.contains("to main chamber")).count(), 2, "{ml:?}");
        let n_gg = labels.iter().filter(|l| l.contains("to gas generator")).count();
        let n_ch = labels.iter().filter(|l| l.contains("to main chamber")).count();
        assert_eq!((n_gg, n_ch), (2, 2), "{labels:?}");
    }

    #[test]
    fn pumped_aerospikes_are_plumbed() {
        // the electric and the gas-generator aerospikes: both feeds through the pumps on one shaft inside the spike, and (gas
        // generator) a generator that the turbine's drive gas runs from, with its valves opened by the start sequence
        for k in [11usize, 12] {
            let mut w = World::new();
            w.load_preset(k);
            w.start_sequence();
            for _ in 0..400 {
                w.update(&Readback::default(), 1.0 / 60.0);
            }
            let nets: Vec<_> = w.nets.iter().filter(|n| n.prop <= 1 && !n.injectors.is_empty()).collect();
            assert_eq!(nets.len(), 2, "preset {k}: one fuel and one oxidiser network");
            for n in &nets {
                assert!(n.tank_cells > 0 && n.p_tank > 0.0, "preset {k}: a network not connected to its tank");
            }
            assert_eq!(w.shafts.len(), 1, "preset {k}: one shaft carrying both pumps");
            assert_eq!(w.gg.len(), if k == 12 { 1 } else { 0 }, "preset {k}: generators");
            assert!(w.valves.iter().all(|v| v.prop <= 1 && v.cmd), "preset {k}: every valve labelled and opened by the start");
            if k == 12 {
                // the spike is cut off, with the exhaust duct opening in its base
                let (tip, _) = w.find_stab_geom().expect("a stabiliser anchor");
                assert!(tip > (crate::presets::PRESET_OY + 320) as f32, "tip {tip}");
            }
        }
    }

    #[test]
    fn nozzle_area_mach_and_separation() {
        use crate::world::{area_ratio, mach_from_area, p_over_pt, SEPARATION_P};
        // area ratio and Mach are inverses of each other, and pressure falls as the bell widens
        for gam in [1.15f32, 1.2, 1.4] {
            for m in [1.5f32, 2.5, 3.5, 5.0] {
                let back = mach_from_area(area_ratio(m, gam), gam);
                assert!((back - m).abs() < 1e-3, "gamma {gam}: Mach {m} came back as {back}");
            }
            assert!(p_over_pt(3.0, gam) < p_over_pt(2.0, gam));
        }
        // a bell of area ratio 16 on a 100 bar chamber ends at a few tenths of a bar (gamma 1.2): below 0.4 of sea-level air,
        // so the flow would separate there, and well above 0.4 of the air at 10 km
        let pe = 100.0e5 * p_over_pt(mach_from_area(16.0, 1.2), 1.2);
        assert!(pe > 0.2e5 && pe < 0.8e5, "exit pressure {pe}");
        assert!(pe > SEPARATION_P * 26_500.0, "no separation at 10 km");
    }

    #[test]
    fn save_load_roundtrip() {
        let mut a = World::new();
        a.st.prop = 2;
        a.st.tank_p_o = 7.5e5;
        a.st.inj_area_f = 1.25;
        let text = a.to_text();
        let mut b = World::new();
        b.clear();
        b.from_text(&text).unwrap();
        assert_eq!(a.cells, b.cells);
        assert_eq!(b.st.prop, 2);
        assert_eq!(b.st.tank_p_o, 7.5e5);
        // an old save from when pressure feeding was a switch: its regulator setting becomes the tanks'
        let old = text.replacen("ROCKET2
", "ROCKET2
pressure_fed 1
he_setpoint 2500000
", 1);
        b.from_text(&old).unwrap();
        assert_eq!((b.st.tank_p_f, b.st.tank_p_o), (2.5e6, 2.5e6));
        assert!((b.st.inj_area_f - 1.25).abs() < 1e-6);
        assert!(b.from_text("garbage").is_err());
    }

    #[test]
    fn nozzle_maker_expansion_ratio() {
        let mut w = World::new();
        w.clear();
        let spec = NozzleSpec { cx: 128.0, y_throat: 100, y_exit: 250, throat_hw: 8.0, expansion: 9.0, contour: Contour::Bell, liner: 2, jacket: 2, shell: 2, axi: false, liner_mat: c::COPPER };
        w.build_nozzle(&spec);
        let gas_width = |y: usize| {
            let (mut a, mut b) = (128usize, 128usize);
            while a > 0 && w.cells[World::idx(a - 1, y)] == c::EMPTY {
                a -= 1;
            }
            while b < W - 1 && w.cells[World::idx(b, y)] == c::EMPTY {
                b += 1;
            }
            (b - a) as f32
        };
        // gas channel is 2*half-width wide: throat 16 cells, exit 16 * expansion (slab)
        assert!((gas_width(100) - 16.0).abs() <= 1.0, "throat {}", gas_width(100));
        assert!((gas_width(250) - 16.0 * 9.0).abs() <= 3.0, "exit {}", gas_width(250));
        // round engine: expansion is an area ratio, so the radius grows as sqrt
        let mut r = spec;
        r.axi = true;
        assert!((r.exit_hw() - 8.0 * 3.0).abs() < 1e-4);
        // wall layers present next to the channel
        assert!((0..W).any(|x| w.cells[World::idx(x, 200)] == c::COPPER));
        assert!((0..W).any(|x| w.cells[World::idx(x, 200)] == c::PIPE));
        assert!((0..W).any(|x| w.cells[World::idx(x, 200)] == c::STEEL));
    }

    #[test]
    fn preset_networks_are_connected() {
        let mut w = World::new();
        w.start_sequence();
        run(&mut w, 4.0); // priming: the fuel has to run all the way round the jacket first
        assert!(w.fuel_primed(), "fuel reaches every injector");
        w.rebuild_nets();
        let f: Vec<_> = w.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0).collect();
        assert_eq!(f.len(), 1, "fuel tank/pump/jacket/injectors must be one network");
        assert!(f[0].injectors.len() >= 30);
        let o: Vec<_> = w.nets.iter().filter(|n| n.prop == 1 && n.tank_cells > 0).collect();
        assert_eq!(o.len(), 1);
        assert!(o[0].injectors.len() >= 8);
    }

    fn run(w: &mut World, secs: f32) {
        run_pc(w, secs, 0.0);
    }

    /// Step the plumbing without the gas solver; `pc` stands in for the chamber back-pressure.
    fn run_pc(w: &mut World, secs: f32, pc: f32) {
        let rb = Readback::default();
        for _ in 0..(secs / 0.016) as usize {
            if pc > 0.0 {
                for n in &mut w.nets {
                    n.pc_f = pc;
                }
            }
            w.update(&rb, 0.016);
        }
    }

    fn open_all(w: &mut World) {
        w.rebuild_nets();
        w.command_all(0, true);
        w.command_all(1, true);
    }

    #[test]
    fn preset_has_two_turbopumps() {
        let mut w = World::new();
        w.rebuild_nets();
        assert_eq!(w.shafts.len(), 2);
        for s in &w.shafts {
            assert!(s.turb_cells > 0 && s.pumps.len() == 1, "each shaft: one turbine + one pump");
        }
        assert!(w.valves.iter().all(|v| !v.cmd && v.pos == 0.0), "preset valves start closed");
    }

    #[test]
    fn cryogenic_needs_spark_and_flames_out_without_flow() {
        let mut w = World::new();
        run(&mut w, 0.5);
        assert!(w.g.mdot_f < 1e-3, "closed valves: no flow");
        open_all(&mut w);
        run(&mut w, 0.3);
        assert!(w.g.mdot_f < 1e-3, "no flow until the fuel has run through the jacket to the injectors");
        run(&mut w, 3.0);
        assert!(w.g.mdot_f > 0.1 && !w.ignited, "cold flow without a spark");
        w.spark_t = 2.5;
        run(&mut w, 0.5);
        assert!(w.ignited && w.g.t_flame > 2000.0);
        w.command_all(1, false);
        run(&mut w, 1.5);
        assert!(!w.ignited, "flameout after closing the oxidiser valve");
    }

    #[test]
    fn valves_stroke_over_time() {
        let mut w = World::new();
        w.st.valve_stroke = 1.0;
        open_all(&mut w);
        run(&mut w, 0.5);
        let p = w.valves[0].pos;
        assert!(p > 0.35 && p < 0.65, "half way after half the stroke time: {p}");
    }

    #[test]
    fn hypergolic_lights_on_contact() {
        let mut w = World::new();
        w.st.prop = 3;
        open_all(&mut w);
        run(&mut w, 3.5);
        assert!(w.ignited, "hypergolics ignite without a spark");
    }

    #[test]
    fn gas_generator_exhaust_into_open_air_drives_nothing() {
        let mut w = World::new();
        let mut e = crate::presets::presets().into_iter().nth(4).unwrap();
        e.twin_shaft = false; // the classic layout: generator not ducted to the turbines
        e.stage = false; // (drawn the old way, tanks beside the engine: the generator stands in the open there)
        e.inj = Some((0.95, 2.2));
        w.build_engine(&e);
        w.st.boil_speed = 0.0; // this plumbing-only harness has no real chamber and sits on a knife edge
        w.st.amb_p = 100.0; // ... so it runs in vacuum (the GPU self-test covers the sea-level start)
        w.start_sequence();
        run_pc(&mut w, 4.0, 1.1e7);
        // the generator burns and the start cartridge spins the pumps ...
        assert!(w.gg_power > 1.0e6, "gg power {}", w.gg_power);
        assert!(w.g.t_gg < 1300.0);
        let rpm = w.g.rpm;
        assert!(rpm > 10000.0, "rpm {rpm}");
        // ... but its exhaust goes into open air, not through a turbine, so once the cartridge is spent
        // nothing keeps the pumps turning (the GPU self-tests run the ducted gas-generator presets)
        run_pc(&mut w, 6.0, 1.1e7);
        assert!(w.g.rpm < 0.7 * rpm, "rpm {} after {}", w.g.rpm, rpm);
    }

    #[test]
    fn helium_drains_with_use() {
        let mut w = World::new();
        w.load_preset(3); // pressure-fed
        w.st.tank_p_f = 3.0e6;
        w.st.tank_p_o = 3.0e6;
        w.start_sequence();
        run(&mut w, 20.0);
        assert!(w.he_frac < 1.0 && w.he_frac > 0.0, "he {}", w.he_frac);
        assert!(w.g.p_reg > 1.0e6);
    }

    #[test]
    fn low_tank_pressure_cavitates_the_pump() {
        let mut w = World::new();
        w.st.tank_p_o = 1.05e5; // barely above LOX vapour pressure
        w.st.boil_speed = 0.0; // no boil-off help
        w.start_sequence();
        run(&mut w, 5.0); // purge, priming, then the pumps spin up
        let ox = w.shafts.iter().flat_map(|s| s.pumps.iter()).find(|p| p.prop == 1).unwrap();
        assert!(ox.cav < 0.5, "cav {}", ox.cav);
        assert!(w.g.cavitating);
    }

    #[test]
    fn no_governor_and_no_load_overspeeds() {
        let mut w = World::new();
        w.st.governor = false;
        // empty tanks: the pumps run dry (a primed pump against a shut valve would still churn its liquid
        // and hold the rotor back), so the start cartridge spins them up with almost nothing to drive
        w.rebuild_nets();
        for n in &mut w.nets {
            n.mass_frac = 0.0;
        }
        w.cartridge_t = 30.0;
        run(&mut w, 8.0);
        assert!(w.events.iter().any(|e| e.contains("overspeed")), "{:?}", w.events);
        assert!(w.shafts.iter().all(|s| s.omega == 0.0) || w.cells.iter().filter(|&&c| c == c::TURBINE).count() < 50);
    }

    #[test]
    fn slamming_a_valve_shut_causes_water_hammer() {
        let mut slow = World::new();
        slow.load_preset(3);
        (slow.st.tank_p_f, slow.st.tank_p_o) = (3.0e7, 3.0e7);
        slow.st.valve_stroke = 0.5;
        slow.start_sequence();
        run(&mut slow, 4.0);
        slow.shutdown();
        run(&mut slow, 1.5);
        assert!(!slow.events.iter().any(|e| e.contains("water hammer")), "{:?}", slow.events);

        let mut fast = World::new();
        fast.load_preset(3);
        (fast.st.tank_p_f, fast.st.tank_p_o) = (3.0e7, 3.0e7);
        fast.st.valve_stroke = 0.005;
        fast.start_sequence();
        run(&mut fast, 4.0);
        // slammed by hand (the Shutdown button closes everything slowly now)
        fast.command_all(0, false);
        fast.command_all(1, false);
        run(&mut fast, 1.5);
        assert!(fast.events.iter().any(|e| e.contains("water hammer")), "{:?}", fast.events);
    }

    #[test]
    fn pipe_layers_join_only_at_joints() {
        let mut w = World::new();
        w.clear();
        w.rect(10, 10, 20, 20, c::TANK_F);
        w.rect(21, 15, 29, 15, c::PIPE); // middle segment A
        w.rect(30, 15, 30, 15, c::JOINT);
        w.rect(31, 10, 33, 20, c::STEEL); // a wall in the engine plane
        w.rect(34, 15, 34, 15, c::JOINT);
        w.rect(35, 15, 40, 15, c::PIPE); // middle segment B
        for x in 30..=34 {
            w.set_layer_cell(1, World::idx(x, 15), c::PIPE); // back pipe behind the wall
        }
        w.rect(50, 30, 60, 40, c::TANK_O);
        w.rect(50, 15, 55, 15, c::PIPE);
        w.rect(55, 16, 55, 29, c::PIPE); // an oxidiser pipe ...
        for x in 50..=55 {
            w.set_layer_cell(2, World::idx(x, 15), c::PIPE); // ... with a front pipe crossing it (no joint)
        }
        w.rebuild_nets();
        let a = w.net_of[World::idx(25, 15)];
        let b = w.net_of[World::idx(38, 15)];
        assert!(a != NO_NET && a == b, "the back pipe joins A and B through the joints");
        assert_eq!(w.nets[a as usize].prop, 0, "it holds fuel because it reaches the fuel tank");
        assert_eq!(w.cells[World::idx(32, 15)], c::STEEL, "the wall is untouched");
        assert!(w.phase[World::idx(38, 15)] < DRY, "B is reachable from the tank");
        let ox = w.net_of[World::idx(52, 15)];
        assert_eq!(w.nets[ox as usize].prop, 1, "the plain pipe on the oxidiser tank holds oxidiser");
        assert!(w.layer_net[1][World::idx(52, 15)] != ox, "without a joint the front pipe stays separate");

        let text = w.to_text();
        let mut v = World::new();
        v.from_text(&text).unwrap();
        assert_eq!(v.back, w.back);
        assert_eq!(v.front, w.front);
        assert_eq!(v.cells, w.cells);
    }

    #[test]
    fn old_saves_with_typed_pipes_still_load() {
        let mut w = World::new();
        w.cells[0] = 5; // the retired "oxidiser line" id, as an old save would contain it
        let text = w.to_text();
        let mut v = World::new();
        v.from_text(&text).unwrap();
        assert_eq!(v.cells[0], c::PIPE, "old oxidiser-line id 5 loads as a plain pipe");
    }

    #[test]
    fn presets_are_sealed_and_breaches_leak() {
        for k in 0..4 {
            let mut w = World::new();
            w.load_preset(k);
            w.rebuild_nets();
            let b: usize = w.nets.iter().map(|n| n.breaches.len()).sum();
            assert_eq!(b + w.he_breaches.len(), 0, "preset {k} must be sealed");
        }
        let (ox, oy) = (crate::presets::PRESET_OX, crate::presets::PRESET_OY);
        let mut w = World::new();
        w.load_preset(3);
        // knock a hole in the wall above the oxidiser tank (the lower one of the stack) and beside the helium bottle
        let hole = (20..200).map(|y| World::idx(120 + ox, y + oy)).find(|&i| w.cells[i] == c::STEEL && w.cells[i + W] == c::TANK_O).expect("the oxidiser tank's top wall");
        assert_eq!(w.cells[hole], c::STEEL);
        w.set_cell(hole, c::EMPTY);
        let he_hole = World::idx(109 + ox, 12 + oy);
        assert_eq!(w.cells[he_hole], c::STEEL);
        w.set_cell(he_hole, c::EMPTY);
        let before = w.nets.iter().filter(|n| n.prop == 1).map(|n| n.mass_frac).fold(0.0, f32::max);
        run(&mut w, 2.0);
        let ox_net = w.nets.iter().find(|n| n.prop == 1 && n.tank_cells > 0).unwrap();
        assert!(ox_net.leak > 1.0, "oxidiser tank leaks: {}", ox_net.leak);
        assert!(ox_net.mass_frac < before, "and drains");
        assert!(w.he_frac < 0.99, "helium bottle vents (about 60 kg/s through one cell): {}", w.he_frac);
        assert!(w.events.iter().any(|e| e.contains("leak")), "{:?}", w.events);
        assert!(w.srcs.iter().any(|s| s.cell as usize == hole), "spill enters the gas at the hole");
    }

    #[test]
    fn helium_bottle_with_a_hole_is_a_cold_gas_thruster() {
        let mut w = World::new();
        w.clear();
        w.rect(100, 100, 130, 130, c::HE_TANK);
        w.auto_wall_all();
        w.rebuild_nets();
        assert!(w.he_breaches.is_empty(), "sealed bottle");
        // knock out one wall block on the bottom: gas jets down, thrust pushes up
        w.set_cell(World::idx(115, 131), c::EMPTY);
        let before = w.he_frac;
        run(&mut w, 0.5);
        assert!(w.g.leak > 10.0, "venting {}", w.g.leak);
        assert!(w.g.leak_thrust > 5.0e4, "upward thrust {}", w.g.leak_thrust);
        assert!(w.g.leak_side.abs() < 1.0, "no side force from a bottom hole");
        assert!(w.he_frac < before, "the bottle blows down");
        // a second hole on the right side adds a push to the left
        w.set_cell(World::idx(131, 115), c::EMPTY);
        run(&mut w, 0.1);
        assert!(w.g.leak_side < -1.0e4, "side hole pushes left: {}", w.g.leak_side);
    }

    #[test]
    fn each_helium_bottle_is_its_own_and_new_ones_are_full() {
        let mut w = World::new();
        w.clear();
        w.st.he_fill = 2000.0 * crate::world::PA_PER_PSI;
        w.rect(100, 100, 120, 120, c::HE_TANK);
        w.auto_wall_all();
        w.rebuild_nets();
        assert!((w.he_bottles[0].p - 2000.0 * crate::world::PA_PER_PSI).abs() < 1.0, "filled to the set psi");
        // empty it through a wide hole
        for x in 100..=120 {
            w.set_cell(World::idx(x, 121), c::EMPTY);
        }
        run(&mut w, 20.0);
        let a = w.he_of[World::idx(110, 110)] as usize;
        assert!(w.he_bottles[a].p < 0.05 * w.he_bottles[a].p0, "first bottle drained: {}", w.he_bottles[a].p);
        // a new bottle somewhere else is full, the old one stays empty
        w.rect(200, 100, 220, 120, c::HE_TANK);
        w.rebuild_nets();
        let b = w.he_of[World::idx(210, 110)] as usize;
        let a = w.he_of[World::idx(110, 110)] as usize;
        assert!((w.he_bottles[b].p - w.he_bottles[b].p0).abs() < 1.0, "new bottle is full");
        assert!(w.he_bottles[a].p < 0.05 * w.he_bottles[a].p0, "old bottle still empty");
        // drawing onto the empty bottle makes it a freshly filled one
        w.rect(121, 100, 121, 110, c::HE_TANK);
        w.rebuild_nets();
        let a = w.he_of[World::idx(110, 110)] as usize;
        assert!((w.he_bottles[a].p - w.he_bottles[a].p0).abs() < 1.0, "redrawn bottle is full again");
    }

    #[test]
    fn sealed_cryogenic_tank_pressurises_itself() {
        let mut w = World::new();
        w.clear();
        w.st.prop = 0; // LH2 / LOX
        w.st.boil_speed = 5.0;
        w.rect(100, 100, 140, 140, c::TANK_O); // a lone LOX tank, no helium, no outlet
        w.auto_wall_all();
        w.rebuild_nets();
        run(&mut w, 1.0);
        let p1 = w.nets.iter().find(|n| n.tank_cells > 0).unwrap().p_tank;
        run(&mut w, 5.0);
        let n = w.nets.iter().find(|n| n.tank_cells > 0).unwrap();
        assert!(n.boil > 0.0, "LOX boils");
        assert!(n.p_tank > p1, "pressure climbs: {} -> {}", p1, n.p_tank);
        // with the vent valve it stops a little above the working pressure
        run(&mut w, 60.0);
        let n = w.nets.iter().find(|n| n.tank_cells > 0).unwrap();
        assert!(n.p_tank <= 1.3 * w.st.tank_p_o + 0.5e5 + 1.0, "vent holds it: {}", n.p_tank);

        // without relief the tank eventually splits and vents
        w.st.relief = false;
        run(&mut w, 120.0);
        assert!(w.events.iter().any(|e| e.contains("ruptured")), "{:?}", w.events);
        let n = w.nets.iter().find(|n| n.tank_cells > 0).unwrap();
        assert!(!n.breaches.is_empty(), "the split wall leaves a hole");
    }

    #[test]
    fn storable_propellant_does_not_boil() {
        let mut w = World::new();
        w.clear();
        w.st.prop = 3; // MMH / N2O4
        w.rect(100, 100, 140, 140, c::TANK_F);
        w.auto_wall_all();
        w.rebuild_nets();
        run(&mut w, 5.0);
        let n = w.nets.iter().find(|n| n.tank_cells > 0).unwrap();
        assert_eq!(n.boil, 0.0);
    }

    #[test]
    fn gse_and_source_blocks() {
        let mut w = World::new();
        w.load_preset(3);
        w.st.gse_tanks = true;
        w.st.gse_he = true;
        w.start_sequence();
        run(&mut w, 5.0);
        assert!(w.nets.iter().filter(|n| n.tank_cells > 0).all(|n| n.mass_frac == 1.0), "GSE keeps tanks full");
        assert!(w.he_bottles.iter().all(|b| b.p == b.p0), "GSE keeps bottles full");

        let mut w = World::new();
        w.clear();
        w.rect(100, 100, 100, 100, c::SRC_FIRE);
        w.rebuild_nets();
        assert_eq!(w.sources.len(), 4, "a lone source cell pushes out of all four sides");
        run(&mut w, 0.1);
        assert!(w.srcs.iter().filter(|s| s.drho > 0.0).count() >= 4, "it keeps feeding the gas around it");
    }

    #[test]
    fn snapshot_before_firing_restores_everything() {
        let mut w = World::new();
        w.rebuild_nets();
        assert!(w.idle());
        let before = w.clone(); // what Reset simulation keeps
        let cells = w.cells.clone();
        w.start_sequence();
        assert!(!w.idle());
        run(&mut w, 6.0);
        let tank = w.cells.iter().position(|&t| t == c::TANK_O).unwrap();
        w.set_cell(tank - W, c::EMPTY); // punch a hole above the oxidiser tank
        run(&mut w, 1.0);
        assert!(w.nets.iter().any(|n| n.tank_cells > 0 && n.mass_frac < 1.0), "propellant was used");

        let mut w = before.clone();
        assert!(w.idle() && w.cells == cells, "same parts, nothing open");
        w.rebuild_nets();
        assert!(w.nets.iter().filter(|n| n.tank_cells > 0).all(|n| n.mass_frac == 1.0), "tanks full again");
        assert!(w.shafts.iter().all(|s| s.omega == 0.0), "pumps stopped");
        assert!(w.valves.iter().all(|v| v.pos == 0.0));
    }

    #[test]
    fn helium_bottle_feeds_a_valved_thruster() {
        // bottle, pipe down through its wall, a closed valve, an injector at the bottom (painted like the UI does)
        let mut w = World::new();
        w.clear();
        for y in 100..=120 {
            for x in 100..=130 {
                w.paint_plumbing(World::idx(x, y), c::HE_TANK);
            }
        }
        for y in 121..=140 {
            w.paint_plumbing(World::idx(115, y), c::PIPE);
        }
        w.paint_plumbing(World::idx(115, 136), c::VALVE_X);
        w.set_cell(World::idx(115, 141), c::INJ);
        w.rebuild_nets();
        assert!(w.he_breaches.is_empty(), "sealed: the pipe and walls close the bottle");
        assert_eq!(w.valves.len(), 1);
        assert!(w.valves[0].label().starts_with("Helium"), "valve knows it carries helium: {}", w.valves[0].label());
        let p0 = w.he_bottles[0].p;
        run(&mut w, 0.5);
        assert_eq!(w.he_bottles[0].p, p0, "closed valve holds the gas");
        assert!(w.g.leak < 1e-6);
        w.command_valve(0, true);
        run(&mut w, 1.0);
        assert!(w.g.leak > 1.0, "gas flows out of the injector: {}", w.g.leak);
        assert!(w.g.leak_thrust > 1.0e3, "and pushes up: {}", w.g.leak_thrust);
        assert!(w.he_bottles[0].p < p0, "the bottle blows down");
        w.command_valve(0, false);
        run(&mut w, 1.0);
        let p1 = w.he_bottles[0].p;
        run(&mut w, 0.5);
        assert_eq!(w.he_bottles[0].p, p1, "closing the valve stops it");
    }

    #[test]
    fn helium_must_be_piped_to_a_tank_through_a_regulator() {
        // the preset pipes its helium bank to both tanks through regulators
        let mut w = World::new();
        w.load_preset(3);
        (w.st.tank_p_f, w.st.tank_p_o) = (3.0e6, 3.0e6);
        w.st.boil_speed = 0.0;
        w.rebuild_nets();
        assert!(w.nets.iter().filter(|n| n.tank_cells > 0).all(|n| n.reg_bottle != NO_NET), "both tanks regulated");
        w.start_sequence();
        run(&mut w, 6.0);
        let ox = w.nets.iter().find(|n| n.prop == 1 && n.tank_cells > 0).unwrap();
        let p_regulated = ox.p_tank;
        assert!(p_regulated > 2.9e6, "held at the set point: {}", p_regulated);

        // take the regulators out: the tanks only have their own ullage gas and blow down
        let mut w = World::new();
        w.load_preset(3);
        (w.st.tank_p_f, w.st.tank_p_o) = (3.0e6, 3.0e6);
        w.st.boil_speed = 0.0;
        for i in 0..crate::world::N {
            if w.cells[i] == c::REG {
                w.set_cell(i, c::STEEL);
            }
        }
        w.rebuild_nets();
        assert!(w.nets.iter().all(|n| n.reg_bottle == NO_NET));
        let he0 = w.he_bottles[0].p;
        w.start_sequence();
        run(&mut w, 10.0);
        let ox = w.nets.iter().find(|n| n.prop == 1 && n.tank_cells > 0).unwrap();
        assert!(ox.p_tank < 0.98 * p_regulated, "no helium top-up, it blows down: {} vs {}", ox.p_tank, p_regulated);
        assert_eq!(w.he_bottles[0].p, he0, "the bottle isn't used");
    }


    #[test]
    fn each_regulator_can_hold_its_own_pressure() {
        let mut w = World::new();
        w.st.boil_speed = 0.0;
        w.rebuild_nets();
        // the oxidiser tank's regulator gets its own 12 bar; the fuel tank's stays on the shared setting
        let ox_reg = (0..crate::world::N)
            .find(|&i| {
                w.cells[i] == c::REG && {
                    let (x, y) = (i % W, i / W);
                    [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)].iter().any(|&(nx, ny)| w.cells[World::idx(nx, ny)] == c::TANK_O)
                }
            })
            .expect("oxidiser regulator");
        w.set_regulator(ox_reg, 12.0e5);
        run(&mut w, 1.0);
        let ox = w.nets.iter().find(|n| n.prop == 1 && n.tank_cells > 0).unwrap();
        let fu = w.nets.iter().find(|n| n.prop == 0 && n.tank_cells > 0).unwrap();
        assert!((ox.p_tank - 12.0e5).abs() < 0.3e5, "oxidiser tank held at its regulator's 12 bar: {}", ox.p_tank);
        assert!((fu.p_tank - w.st.tank_p_f).abs() < 0.3e5, "fuel tank on the shared setting: {}", fu.p_tank);
        // saved with the design
        let mut v = World::new();
        v.from_text(&w.to_text()).unwrap();
        assert_eq!(v.reg_p[ox_reg], 12.0e5);
        // and back to the shared setting
        w.set_regulator(ox_reg, 0.0);
        w.rebuild_nets();
        assert!(w.nets.iter().all(|n| n.reg_set == 0.0));
    }

    #[test]
    fn copy_rotate_stamp_and_paste() {
        let mut w = World::new();
        w.load_preset(3);
        let reg = (0..N).find(|&i| w.cells[i] == c::REG);
        if let Some(i) = reg {
            w.set_regulator(i, 12.0e5);
        }
        let (x0, y0, x1, y1) = (100, 50, 539, 700);
        let clip = w.copy_region(x1, y1, x0, y0); // corners in either order
        assert_eq!((clip.w, clip.h), (440, 651));
        assert!(!clip.is_empty());
        // four quarter turns and two flips each way give it back unchanged
        let back = clip.rotated().rotated().rotated().rotated();
        assert_eq!(back.cells, clip.cells);
        assert_eq!(clip.rotated().w, clip.h);
        assert_eq!(clip.flipped().flipped().cells, clip.cells);
        assert_eq!(clip.flipped_v().flipped_v().back, clip.back);
        // a quarter turn moves the top-left corner to the top-right
        let r = clip.rotated();
        assert_eq!(r.cells[r.w - 1], clip.cells[0]);
        // a stamp file round trip keeps everything, regulator settings included
        let s = Clip::from_text(&clip.to_text()).unwrap();
        assert_eq!((s.w, s.h), (clip.w, clip.h));
        assert_eq!(s.cells, clip.cells);
        assert_eq!(s.back, clip.back);
        assert_eq!(s.front, clip.front);
        assert_eq!(s.reg_p, clip.reg_p);
        assert!(Clip::from_text("ROCKET2\n").is_err());
        // cut (copy then erase) and paste back where it was: the same build
        let before = w.cells.clone();
        let (bb, ff) = (w.back.clone(), w.front.clone());
        w.erase_region(x0, y0, x1, y1);
        assert!(w.cells[World::idx(300, 300)] == c::EMPTY || before[World::idx(300, 300)] == c::EMPTY);
        w.paste(&s, x0, y0);
        assert_eq!(w.cells, before);
        assert_eq!(w.back, bb);
        assert_eq!(w.front, ff);
        if let Some(i) = reg {
            assert_eq!(w.reg_p[i], 12.0e5);
        }
        // pasting off the edge is clipped, not a crash
        w.paste(&s, W as i32 - 10, H as i32 - 10);
        w.paste(&s, -1000, -1000);
    }

    #[test]
    fn start_lamps_follow_the_sequence() {
        let mut w = World::new();
        w.load_preset(2);
        w.rebuild_nets();
        let lamp = |w: &World, name: &str| w.start_lamps().into_iter().find(|l| l.0 == name).map(|l| l.1);
        // at rest: every lamp dark, and the Merlin's starter is its helium spin
        assert!(w.start_lamps().iter().all(|l| l.1 == 0), "{:?}", w.start_lamps());
        assert!(lamp(&w, "HELIUM SPIN").is_some() && lamp(&w, "CARTRIDGE").is_none());
        w.start_sequence();
        assert_eq!(lamp(&w, "PURGE"), Some(1));
        assert_eq!(lamp(&w, "PRIME"), Some(0));
        // purge over: priming, then the ignition part (oxidiser, igniter) with the purge and prime lamps green
        let mut seen_prime = false;
        for _ in 0..400 {
            run(&mut w, 0.016);
            if lamp(&w, "PRIME") == Some(1) {
                seen_prime = true;
                assert_eq!(lamp(&w, "PURGE"), Some(2));
                assert_eq!(lamp(&w, "OXIDISER"), Some(0));
            }
        }
        assert!(seen_prime);
        assert_eq!(lamp(&w, "PRIME"), Some(2), "{:?}", w.start_lamps());
        assert_ne!(lamp(&w, "OXIDISER"), Some(0), "{:?}", w.start_lamps());
        // lit: the flame lamp green, and the main-stage lamp ramping or done
        assert!(w.ignited);
        assert_eq!(lamp(&w, "LIT"), Some(2));
        assert_eq!(lamp(&w, "IGNITER"), Some(2));
        assert_ne!(lamp(&w, "MAIN STAGE"), Some(0), "{:?}", w.start_lamps());
        // a pressure-fed engine has no starter or generator lamp
        w.load_preset(3);
        w.rebuild_nets();
        let names: Vec<&str> = w.start_lamps().iter().map(|l| l.0).collect();
        assert_eq!(names, ["PURGE", "PRIME", "OXIDISER", "IGNITER", "LIT", "MAIN STAGE"]);
    }

    #[test]
    fn dead_headed_primed_pumps_churn_instead_of_running_away() {
        let mut w = World::new();
        w.st.governor = false;
        run(&mut w, 1.0); // (the feed lines fill from the tank stack down to the pumps)
        w.cartridge_t = 30.0; // start cartridge with every valve shut: the pumps are full but nothing flows
        run(&mut w, 8.0);
        assert!(!w.events.iter().any(|e| e.contains("overspeed")), "{:?}", w.events);
        for s in &w.shafts {
            assert!(s.pumps.iter().all(|p| p.wet), "pumps should be primed up to the shut valves");
            assert!(s.x() < 1.3 && s.omega > 0.0, "speed {}", s.x());
            assert!(s.p_load > 0.1e6, "a dead-headed pump still takes power: {}", s.p_load);
        }
    }

    #[test]
    fn turbine_stages_and_pump_losses() {
        // the best blade-speed ratio falls with more stages, and the best efficiency rises a little
        for n in [1.0f32, 2.0, 3.0] {
            let (opt, peak) = turbine_opt(n);
            assert!((turbine_eff(opt, n) - peak).abs() < 1e-4);
            assert!(turbine_eff(0.7 * opt, n) < peak && turbine_eff(1.3 * opt, n) < peak);
        }
        assert!(turbine_opt(2.0).0 < turbine_opt(1.0).0 && turbine_opt(2.0).1 > turbine_opt(1.0).1);
        // torque is highest at standstill and falls to zero at twice the best blade speed
        let (opt, _) = turbine_opt(1.0);
        let (r, c0, p) = (0.15, 1500.0, 1.0e7);
        let w_free = 2.0 * opt * c0 / r;
        assert!(turbine_torque(p, c0, r, 0.0, 1.0) > turbine_torque(p, c0, r, 0.5 * w_free, 1.0));
        assert!(turbine_torque(p, c0, r, w_free, 1.0).abs() < 1.0);
        // pump: shut-off power is a sizeable part of design power, the overall efficiency peaks near the
        // design flow, and needed suction pressure rises with flow
        let pump = PumpRef { net: 0, prop: 1, d: 0.22, stages: 1.0, head: 0.0, eff: 0.0, cav: 1.0, npsp_avail: 0.0, npsp_req: 0.0, power: 0.0, cells: vec![], phase: 0.0, wet: true };
        let (w, rho) = (2000.0, 1140.0);
        let mb = pump.bep_flow(w, rho);
        let p_design = pump.shaft_power(w, mb, rho, pump.head_at(w, mb, rho), true, 1.0);
        let p_shut = pump.shaft_power(w, 0.0, rho, pump.head_at(w, 0.0, rho), true, 1.0);
        assert!(p_shut > 0.25 * p_design && p_shut < 0.6 * p_design, "shut-off {p_shut} design {p_design}");
        assert!(pump.efficiency(w, mb, rho) > pump.efficiency(w, 0.5 * mb, rho));
        assert!(pump.efficiency(w, mb, rho) > pump.efficiency(w, 1.6 * mb, rho));
        assert!(pump.npsp_needed(w, 1.5 * mb, rho) > pump.npsp_needed(w, mb, rho));
        assert_eq!(PumpRef::cav_factor(2.0e5, 1.0e5), 1.0);
        assert!(PumpRef::cav_factor(0.5e5, 1.0e5) < 0.3 && PumpRef::cav_factor(0.2e5, 1.0e5) == 0.0);
        // a big impeller is more efficient than a small one
        let small = PumpRef { d: 0.05, ..pump.clone() };
        assert!(small.eta_peak() < pump.eta_peak());
    }

    #[test]
    fn tanks_decide_the_propellants() {
        let mut w = World::new(); // hydrolox expander: LH2 / LOX
        w.rebuild_nets();
        assert_eq!(w.st.prop, 0);
        let fuel = (0..N).find(|&i| w.cells[i] == c::TANK_F).unwrap();
        let ox = (0..N).find(|&i| w.cells[i] == c::TANK_O).unwrap();
        w.set_tank_fluid(fuel, 2); // RP-1
        assert_eq!(PROPS[w.st.prop].name, "RP-1 / LOX");
        w.set_tank_fluid(ox, 1); // N2O4
        assert_eq!(PROPS[w.st.prop].name, "RP-1 / N2O4");
        assert!(!w.st.p().hypergolic);
        w.set_tank_fluid(fuel, 3); // MMH: hypergolic with N2O4
        assert!(w.st.p().hypergolic);
        // what the tanks hold is saved with the design ...
        let text = w.to_text();
        let mut b = World::new();
        b.from_text(&text).unwrap();
        b.rebuild_nets();
        assert_eq!(b.st.prop, w.st.prop);
        assert_eq!(b.tank_kind(fuel), 3);
        // ... and goes with a tank that is copied and pasted into an empty world
        let (x, y) = ((fuel % crate::world::W) as i32, (fuel / crate::world::W) as i32);
        let clip = w.copy_region(x - 2, y - 2, x + 2, y + 2);
        let mut e = World::new();
        e.clear();
        e.paste(&clip, 100, 100);
        e.rebuild_nets();
        assert_eq!(e.tank_kind(World::idx(102, 102)), 3);
        // every fuel goes with every oxidiser
        for f in 0..4 {
            for o in 0..2 {
                assert_eq!(pair_kinds(pair_index(f, o)), (f, o));
            }
        }
    }

    #[test]
    fn prefab_parts_work_when_placed() {
        let parts = crate::prefabs::all();
        assert!(parts.len() >= 10);
        for pf in &parts {
            let mut w = World::new();
            w.clear();
            w.paste(&pf.clip, 100, 100);
            w.rebuild_nets();
            assert!(w.cells.iter().any(|&v| v != c::EMPTY), "{} is empty", pf.name);
            match pf.group {
                "Combustion chambers" => {
                    // the injectors split into an oxidiser network (from the top stub) and one fuel network
                    // (from the bottom-left stub, up both jackets through the back-layer ring)
                    let inj: Vec<usize> = (0..N).filter(|&i| w.cells[i] == c::INJ).collect();
                    assert!(inj.len() > 10, "{}", pf.name);
                    // the injectors themselves (touching injectors join, so this catches an unsplit row)
                    let mut nets: Vec<u16> = inj.iter().map(|&i| w.net_of[i]).collect();
                    nets.sort();
                    nets.dedup();
                    assert_eq!(nets.len(), 2, "{}: injector networks {:?}", pf.name, nets);
                }
                "Turbopumps" => {
                    assert_eq!(w.shafts.len(), 1, "{}", pf.name);
                    assert_eq!(w.shafts[0].pumps.len(), 1, "{}", pf.name);
                    assert!(w.shafts[0].turb_cells > 0, "{}", pf.name);
                }
                "Gas generators" => assert!(w.cells.iter().any(|&v| v == c::GG || v == c::BURNER)),
                "Electric pumps" => {
                    // one shaft: a pump and a motor, wired to a charged battery
                    assert_eq!(w.shafts.len(), 1, "{}", pf.name);
                    let s = &w.shafts[0];
                    assert!(s.motor_cells > 0 && s.pumps.len() == 1, "{}", pf.name);
                    let cc = &w.circuits[s.circuit as usize];
                    assert!(cc.cap > 1.0e6 && cc.energy == cc.cap && cc.p_max > 1.0e5, "{}", pf.name);
                }
                "Tanks and pressurant" => assert!(w.nets.iter().any(|n| n.tank_cells > 0) || !w.he_bottles.is_empty(), "{}", pf.name),
                _ => {}
            }
        }
    }

    #[test]
    fn shutdown_is_soft_and_ends_fuel_rich() {
        let mut w = World::new();
        open_all(&mut w);
        run(&mut w, 4.0);
        w.shutdown();
        let pos = |w: &World, prop: u8| w.valves.iter().filter(|v| v.prop == prop).map(|v| v.pos).fold(0.0f32, f32::max);
        // the presets have throttle valves: first it throttles down, valves still open
        run(&mut w, 0.6);
        assert!(w.throttle_pos < 0.9, "throttling down first: {}", w.throttle_pos);
        assert_eq!(pos(&w, 1), 1.0);
        // then the oxidiser closes, slowly, while the fuel is still open
        run(&mut w, 1.2);
        let ox = pos(&w, 1);
        assert!(ox > 0.0 && ox < 1.0, "oxidiser valve part closed: {ox}");
        assert_eq!(pos(&w, 0), 1.0, "fuel still open");
        // and in the end everything is shut, with no water hammer on the way
        run(&mut w, 4.0);
        assert_eq!(pos(&w, 0), 0.0);
        assert_eq!(pos(&w, 1), 0.0);
        assert!(!w.events.iter().any(|e| e.contains("hammer") || e.contains("burst")), "{:?}", w.events);
        // and it starts again normally afterwards (purging first)
        w.start_sequence();
        run(&mut w, 0.1);
        assert!(w.purge_t > 0.0);
    }

    #[test]
    fn flood_region_stays_inside_walls() {
        let mut w = World::new();
        w.clear();
        // a 10 x 6 steel box with a 1-cell wall: the inside is 8 x 4 = 32 empty cells
        w.rect(100, 100, 109, 105, c::STEEL);
        w.rect(101, 101, 108, 104, c::EMPTY);
        let inside = w.flood_region(0, World::idx(104, 102));
        assert_eq!(inside.len(), 32);
        let wall = w.flood_region(0, World::idx(100, 100));
        assert_eq!(wall.len(), 10 * 6 - 32);
        // back layer: a pipe run is one region, the engine plane under it doesn't matter
        w.set_layer_cell(1, World::idx(50, 50), c::PIPE);
        w.set_layer_cell(1, World::idx(51, 50), c::PIPE);
        assert_eq!(w.flood_region(1, World::idx(50, 50)).len(), 2);
    }

    #[test]
    fn mixture_changes_how_hard_the_walls_are_heated() {
        use crate::world::wall_heat_scale as f;
        assert_eq!(f(2.4, 2.4), 1.0);
        // fuel-rich: a cooling film, at most 30% off
        assert!(f(1.2, 2.4) < 0.9 && f(0.1, 2.4) >= 0.7);
        // oxygen-rich: far hotter walls, the more so the richer
        assert!(f(3.4, 2.4) > 2.5 && f(4.8, 2.4) > 5.4 && f(20.0, 2.4) <= 5.5);
        assert!(f(3.0, 2.4) < f(3.6, 2.4) && f(1.8, 2.4) < f(2.4, 2.4));
    }

    #[test]
    fn batteries_recharge() {
        let mut w = World::new();
        w.load_preset(8);
        w.rebuild_nets();
        let cap = w.circuits[0].cap;
        w.start_sequence();
        run_pc(&mut w, 8.0, 6.0e6);
        assert!(w.circuits[0].energy < 0.99 * cap, "drained");
        // clicking one of its cells charges that pack; Reset gas/heat charges them all
        let cell = w.cells.iter().position(|&v| v == c::BATTERY).unwrap();
        assert!(w.recharge_at(cell));
        assert_eq!(w.circuits[0].energy, cap);
        assert!(!w.recharge_at(0), "nothing wired there");
        w.circuits[0].energy = 0.0;
        w.recharge();
        assert_eq!(w.circuits[0].energy, cap);
        // with the ground-support option on it never runs down
        w.st.gse_batt = true;
        run_pc(&mut w, 4.0, 6.0e6);
        assert!(w.circuits[0].energy > 0.999 * cap);
    }

    #[test]
    fn tripropellant_burns_two_fuels_at_once() {
        // blending: all one fuel is that pair; half and half sits between the two
        let ch4 = crate::world::blend_pairs(0, &[0.0, 1.0, 0.0, 0.0]);
        let h2 = crate::world::blend_pairs(0, &[1.0, 0.0, 0.0, 0.0]);
        let mix = crate::world::blend_pairs(0, &[0.5, 0.5, 0.0, 0.0]);
        assert_eq!(ch4.name, "CH4 / LOX");
        assert!(mix.of_opt > ch4.of_opt && mix.of_opt < h2.of_opt, "{}", mix.of_opt);
        assert!(mix.rgas > ch4.rgas && mix.rgas < h2.rgas);
        // the preset: methane and hydrogen each on their own tank, burning together
        let mut w = World::new();
        w.load_preset(9);
        w.rebuild_nets();
        assert!(w.tripropellant());
        assert!(w.prop_name().contains("tri-propellant") && w.prop_name().contains("LH2") && w.prop_name().contains("CH4"));
        let fuels: Vec<u8> = w.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0).map(|n| n.fkind).collect();
        assert_eq!(fuels.len(), 2);
        assert!(fuels.contains(&0) && fuels.contains(&1), "{fuels:?}");
        assert!(w.mixed.is_none());
        // the hydrogen tank is held at its own, higher pressure by its own regulator
        let h = w.nets.iter().find(|n| n.prop == 0 && n.tank_cells > 0 && n.fkind == 0).unwrap();
        assert!(h.reg_bottle != NO_NET && h.reg_set > 9.0e6, "{} {}", h.reg_bottle, h.reg_set);
        let b = w.blended_prop();
        assert!(b.of_opt > 3.5 && b.of_opt < 6.0);
        // running: both fuels flow, the burn is richer in hydrogen than the pair alone and nothing breaks
        w.start_sequence();
        run_pc(&mut w, 14.0, 2.0e6);
        let flows: Vec<f32> = w.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0).map(|n| n.m_inj).collect();
        assert!(flows.iter().all(|&m| m > 0.05), "{flows:?}");
        assert!(!w.events.iter().any(|e| e.contains("burst") || e.contains("hammer") || e.contains("overspeed")), "{:?}", w.events);
    }

    #[test]
    fn tanks_and_valves_name_what_they_carry() {
        let mut w = World::new();
        w.load_preset(9); // methane and hydrogen fuel tanks, liquid oxygen
        w.rebuild_nets();
        let mut names: Vec<(&str, bool)> = w.nets.iter().enumerate().filter(|(_, n)| n.prop <= 1 && n.tank_cells > 0).map(|(k, _)| (w.fluid_name(k, false), w.fluid_name(k, true).contains('('))).collect();
        names.sort();
        assert_eq!(names.iter().map(|n| n.0).collect::<Vec<_>>(), vec!["CH4", "LH2", "LOX"]);
        assert!(names.iter().all(|n| n.1), "long names carry the description");
        // every valve says what it carries (closed ones sit between two lines: it looks at the neighbours)
        assert!(!w.valves.is_empty());
        for k in 0..w.valves.len() {
            let l = w.valve_label(k);
            assert!(l.contains("CH4") || l.contains("LH2") || l.contains("LOX"), "{l}");
        }
        // and a pump or pipe on a line knows it too
        let pump = w.cells.iter().position(|&v| v == c::PUMP).unwrap();
        assert!(w.net_around(pump).is_some());
        // a preset on another pair names its own
        w.load_preset(2);
        w.rebuild_nets();
        let k = w.nets.iter().position(|n| n.prop == 0 && n.tank_cells > 0).unwrap();
        assert_eq!(w.fluid_name(k, false), "RP-1");
    }

    #[test]
    fn empty_pipes_are_holes_not_coolant() {
        let mut w = World::new();
        w.load_preset(2);
        w.rebuild_nets();
        // the cell furthest down a fuel line from its tank
        let far = (0..crate::world::N).filter(|&i| w.cells[i] == c::PIPE && w.net_of[i] != NO_NET && w.nets[w.net_of[i] as usize].prop == 0).max_by(|&a, &b| w.phase[a].total_cmp(&w.phase[b])).unwrap();
        let dry_bit = |w: &World, i: usize| (w.gpu_mat(i) >> 24) & 1;
        // before anything has flowed the line is empty: a hole, flagged dry for the heat solver
        assert!(!w.wet_cell(far));
        assert_eq!(dry_bit(&w, far), 1);
        // priming fills the line, and the shader is told (the materials are re-sent as it fills)
        w.start_sequence();
        let mut resent = 0;
        for _ in 0..(8.0 / 0.016) as usize {
            w.mat_stale = false;
            run_pc(&mut w, 0.016, 0.0);
            if w.mat_stale {
                resent += 1;
            }
        }
        assert!(resent > 0, "wetness changes reach the shader");
        assert!(w.wet_cell(far) && dry_bit(&w, far) == 0, "the line has filled: {} vs front {}", w.phase[far], w.nets[w.net_of[far] as usize].front);
        // an empty tank leaves its line dry again
        for n in &mut w.nets {
            n.mass_frac = 0.0;
        }
        assert!(!w.wet_cell(far) && dry_bit(&w, far) == 1);
        // a line joined to no tank never holds liquid
        let mut w2 = World::new();
        w2.clear();
        w2.rect(100, 100, 120, 101, c::PIPE);
        w2.rebuild_nets();
        assert!(!w2.wet_cell(World::idx(110, 100)));
    }

    #[test]
    fn two_shaft_engines_hold_their_mixture() {
        // J-2, hydrolox GG into the nozzle, and staged combustion run each pump on its own shaft: left alone the
        // oxidiser side can run away from the fuel side (O/F 14 and more on a slow GPU), so the mixture control
        // is on from the start, a little fuel-rich of the best mixture
        for k in [1usize, 4, 5] {
            let mut w = World::new();
            w.load_preset(k);
            let of_opt = w.st.p().of_opt;
            assert!((w.st.mix_target - 0.92 * of_opt).abs() < 1e-4, "preset {k}: {}", w.st.mix_target);
        }
        let mut w = World::new();
        w.load_preset(5);
        assert!(w.st.feed_p_max <= 1.8e7, "the pump bypass caps the staged engine's pressure: {}", w.st.feed_p_max);
    }

    #[test]
    fn gas_burner_is_a_machine_block_with_ports() {
        let mut w = World::new();
        w.clear();
        w.st.gse_tanks = true;
        let clip = crate::prefabs::all().into_iter().find(|p| p.name == "Gas burner").unwrap().clip;
        w.paste(&clip, 300, 250);
        // fuel in on top, oxidiser in below: tanks butted against the open stub ends
        let top = (0..clip.w * clip.h).find(|&k| clip.cells[k] == c::PIPE).unwrap();
        let bot = (0..clip.w * clip.h).rev().find(|&k| clip.cells[k] == c::PIPE).unwrap();
        let (tx, ty) = (300 + (top % clip.w), 250 + top / clip.w);
        let (bx, by) = (300 + (bot % clip.w), 250 + bot / clip.w);
        for (x0, y0, x1, y1, k) in [(tx - 5, ty - 14, tx + 6, ty - 1, c::TANK_F), (bx - 5, by + 1, bx + 6, by + 14, c::TANK_O)] {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    w.paint_plumbing(World::idx(x, y), k);
                }
            }
        }
        w.rebuild_nets();
        // one generator, a burner block (burns by its own equations), fed by both lines, with an open outlet face
        assert_eq!(w.gg.len(), 1);
        assert!(w.gg_lumped[0]);
        let (fi, oi, _, _) = w.gg[0];
        assert_eq!((w.nets[fi].prop, w.nets[oi].prop), (0, 1));
        assert!(!w.gg_faces[0].is_empty(), "the neck is its outlet");
        assert_eq!(w.gg_dir[0], 1.0, "the neck is on the right");
        assert!(w.gg_fneg[0], "the fuel line comes in on the top");
        // it is a drawn block (for the cutaway), not a rotor
        let cell = w.cells.iter().position(|&v| v == c::BURNER).unwrap();
        assert!(w.block_of[cell] < 64);
        assert!(w.shafts.is_empty());
        assert!(w.burner_info(cell).unwrap().contains("not burning"));
        // propellant arrives and it burns: no ignition needed (the plumbing-only test has no gas to push back, so the
        // burner's set back-pressure is left low)
        w.st.gg_p = 1.0e5;
        run_pc(&mut w, 6.0, 0.0);
        let info = w.burner_info(cell).unwrap();
        assert!(info.contains("burning") && !info.contains("not burning"), "{info}");
        assert!(w.g.t_gg > 500.0, "gas temperature {}", w.g.t_gg);
        let a = w.anim();
        let o = 256 + 8 * w.block_of[cell] as usize;
        assert!(a[o + 7] > 0.3, "its picture shows it burning: {}", a[o + 7]);
    }

    #[test]
    fn gas_generator_engines_use_burner_blocks() {
        for k in [1usize, 2, 4, 5, 6] {
            let mut w = World::new();
            w.load_preset(k);
            assert!(w.cells.iter().any(|&v| v == c::BURNER), "preset {k} has a burner block");
            assert!(!w.cells.iter().any(|&v| v == c::GG), "and no injector-plate generator");
            // shut down (valves closed, so no generator is registered yet) its picture still faces the right way
            // (checked for the two-shaft engines: the outlet on the left, oxidiser in from above, fuel from below)
            w.rebuild_nets();
            assert!(w.gg.is_empty());
            let cell = w.cells.iter().position(|&v| v == c::BURNER).unwrap();
            let (dir, fuel_low) = w.burner_view[w.block_of[cell] as usize];
            if k == 5 {
                assert_eq!((dir, fuel_low), (-1.0, false), "preset {k}");
            } else {
                // a powerhead from the maker: the outlet below, onto the turbine, the fuel in on the left
                assert_eq!((dir, fuel_low), (1.0, true), "preset {k}");
                assert!(!w.blocks[w.block_of[cell] as usize].horizontal, "preset {k}: the burner points down");
            }
            // its feed valves open at the start: the burner is then joined to both lines
            open_all(&mut w);
            run(&mut w, 4.0);
            assert_eq!(w.gg.len(), 1, "preset {k}");
            assert!(w.gg_lumped[0], "preset {k}: a burner block");
            assert!(!w.gg_faces[0].is_empty(), "preset {k}: it has an outlet");
        }
        // the engines with no generator have none
        let mut w = World::new();
        w.load_preset(7);
        assert!(!w.cells.iter().any(|&v| v == c::BURNER || v == c::GG));
    }

    #[test]
    fn the_atmosphere_thins_with_altitude() {
        use crate::world::{altitude_of_p, atmosphere_p};
        // the 1976 standard atmosphere
        for (km, want) in [(0.0, 101325.0), (5.0, 54019.0), (11.0, 22632.0), (20.0, 5474.9), (32.0, 868.0), (47.0, 110.9), (51.0, 66.94), (71.0, 3.956), (84.852, 0.3734)] {
            let p = atmosphere_p(km);
            assert!((p / want - 1.0).abs() < 0.01, "{km} km: {p} Pa, expected {want}");
        }
        let mut last = f32::MAX;
        for k in 0..=1200 {
            let p = atmosphere_p(k as f32 * 0.1);
            assert!(p < last && p > 0.0, "pressure falls steadily with height");
            last = p;
        }
        // about 48 km is where the gas solver's floor is
        assert!(atmosphere_p(47.0) > 100.0 && atmosphere_p(48.5) < 100.0);
        // and the inverse
        for km in [0.0, 3.0, 11.0, 25.0, 40.0, 60.0] {
            assert!((altitude_of_p(atmosphere_p(km)) - km).abs() < 0.05, "{km}");
        }
    }

    #[test]
    fn the_vehicle_flies_and_lands_on_thrust_weight_and_drag() {
        let mut w = World::new();
        w.load_preset(2);
        let rb = Readback::default();
        w.flight = true;
        w.alt_on = true;
        w.veh_mass = 25000.0;
        w.dry_mass = 25000.0;
        // no thrust: it sits on the pad
        for _ in 0..60 {
            w.update(&rb, 0.016);
        }
        assert_eq!((w.altitude, w.vel), (0.0, 0.0));
        // more thrust than weight lifts it, and the air rushes down past the engine
        w.thrust_f = 400.0e3;
        for _ in 0..120 {
            w.thrust_f = 400.0e3;
            w.update(&rb, 0.016);
        }
        assert!(w.vel > 5.0 && w.altitude > 0.0, "{} {}", w.vel, w.altitude);
        assert!(w.wind() > 5.0);
        // falling fast with no thrust it hits the ground and says so
        w.altitude = 0.02;
        w.vel = -60.0;
        w.thrust_f = 0.0;
        for _ in 0..30 {
            w.thrust_f = 0.0;
            w.update(&rb, 0.016);
        }
        assert!(w.flight_msg.starts_with("Crashed"), "{}", w.flight_msg);
        assert_eq!(w.vel, 0.0);
    }

    #[test]
    fn a_cartridge_part_serves_the_shaft_it_touches() {
        let mut w = World::new();
        w.load_preset(2);
        w.rebuild_nets();
        assert!(w.shafts.iter().all(|s| s.cart_cells == 0));
        // a cartridge cell on an empty cell next to a turbine cell
        // (the wheel is cased in: the open space against it is its exhaust duct, below)
        let t = (0..N).find(|&i| w.cells[i] == c::TURBINE && w.cells[i + W] == c::EMPTY).expect("a turbine with open space beside it");
        w.set_cell(t + W, c::CARTRIDGE);
        w.rebuild_nets();
        assert_eq!(w.shafts.iter().map(|s| s.cart_cells).sum::<usize>(), 1);
    }

    #[test]
    fn the_j2_and_staged_engines_get_a_helium_start_line() {
        for k in [1usize, 5] {
            let mut w = World::new();
            w.load_preset(k);
            w.rebuild_nets();
            eprintln!("preset {k}: line {} he_spin {} events {:?}", w.helium_start_line(), w.st.he_spin, w.events);
            assert!(w.helium_start_line(), "preset {k} has no helium start line");
            assert!(!w.st.he_spin);
        }
    }

    #[test]
    fn a_tea_teb_line_lights_the_merlin_with_a_green_flash() {
        let mut w = World::new();
        w.load_preset(2);
        w.rebuild_nets();
        assert!(w.teateb_line(), "the Merlin carries a TEA-TEB bottle, valve and injector");
        assert!(w.he_bottles.iter().any(|b| b.tea));
        w.ignition_sequence_for_test();
        assert_eq!(w.spark_t, 0.0, "no igniter of its own: the TEA-TEB does it");
        assert!(w.seq.iter().any(|(_, a)| matches!(a, crate::world::SeqAction::TeaValves(true))));
    }

    #[test]
    fn altitude_sets_the_ambient_pressure_and_climbing_follows_it() {
        use crate::world::{atmosphere_p, MIN_AMBIENT_P};
        let mut w = World::new();
        w.load_preset(2);
        let rb = Readback::default();
        // by hand the ambient pressure is left alone
        w.update(&rb, 0.016);
        assert_eq!(w.st.amb_p, 101325.0);
        assert_eq!(w.amb_k, 0.0);
        // an altitude sets it, and the air in the world follows for a few seconds
        w.alt_on = true;
        w.altitude = 10.0;
        w.update(&rb, 0.016);
        assert!((w.st.amb_p / atmosphere_p(10.0) - 1.0).abs() < 1e-4);
        assert!(w.amb_k > 0.0, "the air is following the change");
        for _ in 0..(6.0 / 0.016) as usize {
            w.update(&rb, 0.016);
        }
        assert_eq!(w.amb_k, 0.0, "and settles");
        // climbing raises the altitude steadily, to the gas solver's floor
        w.altitude = 0.0;
        w.climbing = true;
        w.climb_rate = 2.0;
        for _ in 0..(10.0 / 0.016) as usize {
            w.update(&rb, 0.016);
        }
        assert!((w.altitude - 20.0).abs() < 1.0, "{} km after 10 s at 2 km/s", w.altitude);
        for _ in 0..(40.0 / 0.016) as usize {
            w.update(&rb, 0.016);
        }
        assert_eq!(w.st.amb_p, MIN_AMBIENT_P, "above about 48 km it is vacuum to the gas solver");
        assert!(w.altitude > 90.0);
        // the test stand records it
        let r = crate::stand::readings(&w);
        assert!(r[16] > 90.0 && r[17] < 0.01, "altitude {} km, {} bar", r[16], r[17]);
    }

    #[test]
    fn rutherford_runs_on_electric_pumps() {
        let mut w = World::new();
        w.load_preset(8);
        w.rebuild_nets();
        // one shaft, a motor not a turbine, a charged battery wired to it, and no gas generator at all
        assert_eq!(w.shafts.len(), 1);
        let s = &w.shafts[0];
        assert!(s.motor_cells > 100 && s.turb_cells == 0 && s.pumps.len() == 2);
        assert!(w.gg.is_empty());
        let cc = w.circuits[s.circuit as usize].clone();
        assert!(cc.cap > 5.0e6 && cc.energy == cc.cap && cc.p_max > 1.0e5, "{} J, {} W", cc.cap, cc.p_max);
        assert!(w.mixed.is_none());
        // starting spins the pumps up from the battery and nothing breaks
        w.start_sequence();
        run_pc(&mut w, 10.0, 6.0e6);
        let s = &w.shafts[0];
        assert!(s.rpm() > 20000.0, "{} rpm", s.rpm());
        let cc2 = &w.circuits[s.circuit as usize];
        assert!(cc2.energy < cc.energy, "the battery drains");
        assert!(!w.events.iter().any(|e| e.contains("burst") || e.contains("hammer") || e.contains("overspeed")), "{:?}", w.events);
    }

    #[test]
    fn start_purges_then_opens_the_valves_slowly() {
        for k in [0usize, 2, 5, 6] {
            let mut w = World::new();
            w.load_preset(k);
            w.start_sequence();
            let pos = |w: &World, prop: u8| w.valves.iter().filter(|v| v.prop == prop).map(|v| v.pos).fold(0.0f32, f32::max);
            // purge: inert gas through the injectors, every valve still shut
            run(&mut w, 1.0);
            assert!(w.purge_t > 0.0, "preset {k}");
            assert_eq!(pos(&w, 0), 0.0, "preset {k}");
            assert_eq!(w.shut_thr, 0.4, "throttle held down for the start");
            // then the fuel valves open, slowly
            run(&mut w, 1.0);
            let f = pos(&w, 0);
            assert!(f > 0.0 && f < 1.0, "preset {k}: fuel valve part open after half a second: {f}");
            // it lights, the throttle comes up, and nothing surged on the way
            run(&mut w, 10.0);
            assert!(w.ignited, "preset {k}: {:?}", w.events);
            assert_eq!(w.shut_thr, 1.0, "preset {k}");
            assert!(!w.events.iter().any(|e| e.contains("hammer") || e.contains("burst") || e.contains("overspeed")), "preset {k}: {:?}", w.events);
        }
    }
}
