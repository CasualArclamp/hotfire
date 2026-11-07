//! Ready-made parts for the prefab menu. Each one comes out as a clipboard piece, so it goes down like a
//! paste (and turns or mirrors with R / Shift+R). Pipe ends are left open so a line drawn up to them
//! joins; an unconnected one leaks, as a real open port would.

use crate::design::Clip;
use crate::world::{c, PROP_HE};
use std::collections::HashSet;
use std::f32::consts::PI;

/// A small drawing board for a part: engine plane and back pipe layer.
pub(crate) struct Board {
    pub w: usize,
    pub h: usize,
    cells: Vec<u8>,
    back: Vec<u8>,
    pub open: HashSet<(i32, i32)>, // pipe ends (and gas passages) to leave open after walling
}

impl Board {
    pub fn new(w: usize, h: usize) -> Board {
        Board { w, h, cells: vec![0; w * h], back: vec![0; w * h], open: HashSet::new() }
    }
    pub fn set(&mut self, x: i32, y: i32, v: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.cells[y as usize * self.w + x as usize] = v;
        }
    }
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h { self.cells[y as usize * self.w + x as usize] } else { 255 }
    }
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: u8) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, v);
            }
        }
    }
    pub fn back_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
                    self.back[y as usize * self.w + x as usize] = c::PIPE;
                }
            }
        }
    }
    /// Give containers and plumbing their one-cell steel wall (as painting them does), except at the
    /// open pipe ends.
    fn wall(&mut self) {
        let mut add = vec![];
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if !c::auto_walls(self.get(x, y)) {
                    continue;
                }
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if self.get(x + dx, y + dy) == c::EMPTY && !self.open.contains(&(x + dx, y + dy)) {
                            add.push((x + dx, y + dy));
                        }
                    }
                }
            }
        }
        for (x, y) in add {
            self.set(x, y, c::STEEL);
        }
    }
    pub fn clip(mut self) -> Clip {
        self.wall();
        let n = self.w * self.h;
        Clip { w: self.w, h: self.h, cells: self.cells, back: self.back, front: vec![0; n], reg_p: vec![0.0; n], fluid: vec![0; n] }
    }
}

/// A short open-ended pipe stub, 2 cells wide, from (x, y) going `len` cells in direction (dx, dy).
pub(crate) fn stub(b: &mut Board, x: i32, y: i32, dx: i32, dy: i32, len: i32) {
    for k in 0..len {
        let (px, py) = (x + dx * k, y + dy * k);
        b.set(px, py, c::PIPE);
        b.set(px + dy.abs(), py + dx.abs(), c::PIPE);
    }
    // the cells just past the end stay open
    let (ex, ey) = (x + dx * len, y + dy * len);
    b.open.insert((ex, ey));
    b.open.insert((ex + dy.abs(), ey + dx.abs()));
}

/// Regen-cooled combustion chamber and bell nozzle, pointing down. Oxidiser comes in through the stub on
/// top into the middle of the injector manifold; fuel comes in through the stub at the bottom left, into
/// the cooling jacket on both sides (a back-layer ring passes behind the bell), up past the throat and into
/// the outer injector manifolds.
pub(crate) fn chamber(ch_hw: f32, ch_len: f32, throat_hw: f32, exit_hw: f32, noz_len: f32) -> Clip {
    let (l1, l2, l3) = (2.0f32, 4.0f32, 6.0f32); // copper liner, fuel jacket, steel shell
    let conv = (ch_hw - throat_hw) * 1.6;
    let w = (2.0 * (exit_hw + l3) + 8.0).ceil() as usize;
    let top = 6; // injector row
    let ey = top + 1 + (ch_len + conv + noz_len) as i32;
    let h = (ey + 4) as usize;
    let cx = w as f32 / 2.0;
    let mut b = Board::new(w, h);
    let hw = |y: f32| -> f32 {
        let yy = y - (top + 1) as f32;
        if yy < ch_len {
            ch_hw
        } else if yy < ch_len + conv {
            let t = (yy - ch_len) / conv;
            throat_hw + (ch_hw - throat_hw) * (0.5 + 0.5 * (PI * t).cos())
        } else {
            let t = ((yy - ch_len - conv) / noz_len).clamp(0.0, 1.0);
            throat_hw + (exit_hw - throat_hw) * (1.0 - (1.0 - t).powf(2.2))
        }
    };
    for y in (top - 1)..=ey {
        let hh = hw(y as f32 + 0.5);
        for x in 0..w as i32 {
            let d = (x as f32 + 0.5 - cx).abs();
            let v = if d < hh {
                c::EMPTY
            } else if d < hh + l1 {
                c::COPPER
            } else if d < hh + l2 {
                c::PIPE
            } else if d < hh + l3 {
                c::STEEL
            } else {
                continue;
            };
            b.set(x, y, v);
        }
    }
    // injector head: steel cap, manifolds (oxidiser in the middle, fuel either side joined to the jackets),
    // injector elements across the chamber, split under the manifold walls (touching injectors would join
    // the fuel and oxidiser)
    let span = ch_hw + l3;
    let ox_r = (ch_hw * 0.3).max(3.0);
    for x in 0..w as i32 {
        let d = (x as f32 + 0.5 - cx).abs();
        if d < span {
            let divider = d >= ox_r && d < ox_r + 1.0;
            b.set(x, top - 3, c::STEEL);
            b.set(x, top - 2, c::STEEL);
            b.set(x, top - 1, if d < ox_r { c::PIPE } else if divider { c::STEEL } else if d < ch_hw + l2 { c::PIPE } else { c::STEEL });
            if d < ch_hw {
                b.set(x, top, if divider { c::STEEL } else { c::INJ });
            }
        }
    }
    // oxidiser inlet on top
    let xm = cx.floor() as i32 - 1;
    stub(&mut b, xm, top - 2, 0, -1, top - 2);
    // fuel inlet: into the left jacket just above the exit, and a back-layer ring to the right jacket
    let yf = ey - 3;
    let hf = hw(yf as f32 + 0.5);
    let jl = (cx - hf - l1 - 1.0).floor() as i32; // left jacket column
    let jr = (cx + hf + l1).floor() as i32; // right jacket column
    b.rect(jl - 1, yf, jl, yf + 1, c::JOINT);
    b.rect(jr, yf, jr + 1, yf + 1, c::JOINT);
    b.back_rect(jl - 1, yf, jr + 1, yf + 1);
    stub(&mut b, jl - 2, yf, -1, 0, jl - 2);
    b.clip()
}

/// Turbopump lying on its side: pump (inlet on the end, outlet on top), shaft, turbine wheel.
fn turbopump(pump_d: i32, stages: i32, turb_d: i32, turb_l: i32) -> Clip {
    let pl = ((0.6 * (pump_d * stages) as f32).round() as i32).max(3);
    let w = (8 + pl + 6 + turb_l + 4) as usize;
    let h = (turb_d.max(pump_d) + 12) as usize;
    let mut b = Board::new(w, h);
    let cy = h as i32 / 2 + 2;
    let px0 = 7;
    b.rect(px0, cy - pump_d / 2, px0 + pl - 1, cy - pump_d / 2 + pump_d - 1, c::PUMP);
    b.rect(px0 + pl, cy - 2, px0 + pl + 5, cy + 1, c::SHAFT);
    let tx0 = px0 + pl + 6;
    b.rect(tx0, cy - turb_d / 2, tx0 + turb_l - 1, cy - turb_d / 2 + turb_d - 1, c::TURBINE);
    // inlet into the end of the pump, outlet from its top
    stub(&mut b, px0 - 1, cy - 1, -1, 0, px0 - 1);
    stub(&mut b, px0 + pl / 2 - 1, cy - pump_d / 2 - 1, 0, -1, cy - pump_d / 2 - 1);
    b.clip()
}

/// Electric turbopump (Rutherford style): pump, shaft, electric motor, and a battery pack wired to it.
/// Pump inlet on the end, outlet on top, as on the turbopumps.
fn electric_pump(pump_d: i32, stages: i32, motor_d: i32, motor_l: i32, batt_w: i32, batt_h: i32) -> Clip {
    let pl = ((0.6 * (pump_d * stages) as f32).round() as i32).max(3);
    let w = (8 + pl + 5 + motor_l + 3).max(8 + batt_w + 3) as usize;
    let h = (motor_d.max(pump_d) + 14 + batt_h) as usize;
    let mut b = Board::new(w, h);
    let cy = motor_d.max(pump_d) / 2 + 6;
    let px0 = 7;
    b.rect(px0, cy - pump_d / 2, px0 + pl - 1, cy - pump_d / 2 + pump_d - 1, c::PUMP);
    b.rect(px0 + pl, cy - 1, px0 + pl + 4, cy, c::SHAFT);
    let mx0 = px0 + pl + 5;
    b.rect(mx0, cy - motor_d / 2, mx0 + motor_l - 1, cy - motor_d / 2 + motor_d - 1, c::MOTOR);
    // wire down from the motor to the battery pack
    let mb = cy - motor_d / 2 + motor_d;
    let by0 = mb + 4;
    b.rect(mx0 + motor_l / 2, mb, mx0 + motor_l / 2, by0 - 1, c::WIRE);
    b.rect(mx0 + motor_l / 2 - batt_w + 2, by0, mx0 + motor_l / 2 + 1, by0 + batt_h - 1, c::BATTERY);
    stub(&mut b, px0 - 1, cy - 1, -1, 0, px0 - 1);
    stub(&mut b, px0 + pl / 2 - 1, cy - pump_d / 2 - 1, 0, -1, cy - pump_d / 2 - 1);
    b.clip()
}

/// Gas burner: a machine block (like a turbine) in a steel casing. Fuel comes in through the stub on top, oxidiser
/// through the stub below, and the hot gas leaves through the tapered neck on the right, ready to butt against a
/// turbine or a duct. The inside is drawn as a cutaway, with the jets and the flame in it.
fn gas_burner(len: i32, wid: i32) -> Clip {
    let duct = 8;
    let (x0, y0) = (6, 8);
    let w = (x0 + len + duct + 3) as usize;
    let h = (y0 + wid + 8 + 6) as usize;
    let mut b = Board::new(w, h);
    // casing round the block and the neck
    b.rect(x0 - 1, y0 - 1, x0 + len + duct - 1, y0 + wid, c::STEEL);
    b.rect(x0, y0, x0 + len - 1, y0 + wid - 1, c::BURNER);
    // the neck: open across the block's whole outlet face, then narrowing to a passage 4 cells tall
    b.rect(x0 + len, y0, x0 + len, y0 + wid - 1, c::EMPTY);
    let mid = y0 + wid / 2;
    b.rect(x0 + len + 1, mid - 2, x0 + len + duct - 1, mid + 1, c::EMPTY);
    for y in mid - 2..=mid + 1 {
        b.open.insert((x0 + len + duct, y));
    }
    // fuel in on top, oxidiser in below
    stub(&mut b, x0 + 3, y0 - 1, 0, -1, 6);
    stub(&mut b, x0 + 3, y0 + wid, 0, 1, 6);
    b.clip()
}

/// Gas generator: injector head on top fed from both sides, its own small steel combustion chamber
/// narrowing to an outlet at the bottom.
fn gas_generator(cw: i32, ch: i32) -> Clip {
    let w = (cw + 4 + 16) as usize;
    let h = (ch + cw / 2 + 8) as usize;
    let mut b = Board::new(w, h);
    let (xa, hy) = (8, 2);
    let xb = xa + cw + 3;
    b.rect(xa, hy - 1, xb, hy - 1, c::STEEL);
    b.rect(xa, hy, xb, hy + 1, c::GG);
    let top = hy + 2;
    b.rect(xa, top, xa + 1, top + ch - 1, c::STEEL);
    b.rect(xb - 1, top, xb, top + ch - 1, c::STEEL);
    let steps = (cw - 4) / 2;
    for k in 0..=steps + 1 {
        let inset = (k + 1).min(steps);
        let y = top + ch + k;
        b.rect(xa, y, xa + 1 + inset, y, c::STEEL);
        b.rect(xb - 1 - inset, y, xb, y, c::STEEL);
    }
    // fuel from the left, oxidiser from the right, straight into the head
    stub(&mut b, xa - 1, hy, -1, 0, xa - 1);
    stub(&mut b, xb + 1, hy, 1, 0, w as i32 - xb - 1);
    b.clip()
}

fn tank(kind: u8, tw: i32, th: i32) -> Clip {
    let mut b = Board::new((tw + 4) as usize, (th + 10) as usize);
    b.rect(2, 2, tw + 1, th + 1, kind);
    stub(&mut b, tw / 2, th + 2, 0, 1, 6);
    b.clip()
}

/// Helium bottle with a pressure regulator and a pipe stub to run to a tank.
fn helium(bw: i32, bh: i32) -> Clip {
    let mut b = Board::new((bw + 20) as usize, (bh + 4) as usize);
    b.rect(2, 2, bw + 1, bh + 1, c::HE_TANK);
    let y = bh / 2;
    b.rect(bw + 2, y, bw + 5, y + 1, c::PIPE);
    b.rect(bw + 6, y, bw + 6, y + 1, c::REG);
    stub(&mut b, bw + 7, y, 1, 0, 11);
    let _ = PROP_HE;
    b.clip()
}

/// A run of feed line with a (closed) main valve and a throttle valve in it.
fn valve_run() -> Clip {
    let mut b = Board::new(24, 6);
    b.rect(1, 2, 22, 3, c::PIPE);
    b.rect(9, 2, 10, 3, c::VALVE_X);
    b.rect(13, 2, 13, 3, c::THROTTLE);
    b.open.extend([(0, 2), (0, 3), (23, 2), (23, 3)]);
    b.clip()
}

pub struct Prefab {
    pub group: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub clip: Clip,
}

pub fn all() -> Vec<Prefab> {
    let p = |group, name, blurb, clip| Prefab { group, name, blurb, clip };
    vec![
        p("Combustion chambers", "Small chamber + bell", "Regen-cooled: oxidiser in on top, fuel in at the bottom left, up the jacket to the injectors. ~8 cm throat.", chamber(10.0, 22.0, 4.0, 28.0, 70.0)),
        p("Combustion chambers", "Medium chamber + bell", "Regen-cooled, ~14 cm throat (Merlin class).", chamber(16.0, 30.0, 7.0, 42.0, 110.0)),
        p("Combustion chambers", "Large chamber + vacuum bell", "Regen-cooled, ~16 cm throat with a big vacuum bell (RS-25 / J-2 class).", chamber(22.0, 40.0, 8.0, 80.0, 180.0)),
        p("Turbopumps", "Hydrogen turbopump", "24 cm two-stage pump, shaft, 24 cm turbine. Inlet on the end, outlet on top. Duct drive gas through the turbine.", turbopump(24, 2, 24, 8)),
        p("Turbopumps", "Oxygen turbopump", "22 cm single-stage pump, shaft, 32 cm turbine.", turbopump(22, 1, 32, 4)),
        p("Turbopumps", "Small turbopump", "12 cm pump and 18 cm turbine, for small engines.", turbopump(12, 1, 18, 4)),
        p("Electric pumps", "Electric turbopump", "Rutherford style: 10 cm pump, shaft, 12 cm electric motor (about 350 kW) wired to a 300-cell battery pack (18 MJ, 300 kW). The motor runs while the oxidiser valves are open.", electric_pump(10, 1, 12, 8, 20, 15)),
        p("Gas generators", "Gas burner", "A burner block (a machine like a turbine, not cells of gas): fuel in on top, oxidiser in below, hot gas out of the neck on the right. It burns by its own equations, lights itself, and is drawn as a cutaway with the jets and the flame in it. Butt the neck against a turbine or duct.", gas_burner(16, 10)),
        p("Gas generators", "Small gas burner", "A smaller burner block (Merlin class).", gas_burner(10, 6)),
        p("Gas generators", "Gas generator", "Injector head (fuel from the left, oxidiser from the right) burning into its own chamber with an outlet at the bottom: duct it to a turbine.", gas_generator(12, 26)),
        p("Gas generators", "Small gas generator", "A smaller unit (Merlin class).", gas_generator(8, 18)),
        p("Tanks and pressurant", "Fuel tank", "30 x 40 cm with an outlet at the bottom. Click it with the Fill tool to choose the fuel.", tank(c::TANK_F, 30, 40)),
        p("Tanks and pressurant", "Oxidiser tank", "30 x 40 cm with an outlet at the bottom.", tank(c::TANK_O, 30, 40)),
        p("Tanks and pressurant", "Helium bottle + regulator", "Pressurant bottle with a regulator; run the pipe to a tank.", helium(20, 14)),
        p("Plumbing", "Valve + throttle run", "A length of feed line with a main valve (closed) and a throttle valve.", valve_run()),
    ]
}
