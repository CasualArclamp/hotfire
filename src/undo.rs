//! Undo and redo for the design: every finished edit (a brush stroke, a line or box, a paste, a flood fill, a
//! preset or design loaded, Clear all, a prefab put down, a tank refilled with another fluid...) is one step.
//!
//! The editor never says "I edited"; instead the design (the parts on all three pipe layers, the tank fluids,
//! the regulator settings and the settings that belong to the drawing, like the injector sizes) is hashed, and a
//! change that has finished (no mouse button down) pushes the state it came from onto the undo stack. Changes
//! the simulation makes itself (a wall melting) are taken as the new baseline without a step, so undo never
//! "repairs" damage and a firing engine isn't full of steps.

use crate::world::{Settings, World, N};

const MAX_STEPS: usize = 120;
const MAX_BYTES: usize = 160 * 1024 * 1024;

/// A design, run-length encoded (it is mostly empty space).
#[derive(Clone)]
pub struct Snap {
    cells: Vec<(u32, u8)>,
    back: Vec<(u32, u8)>,
    front: Vec<(u32, u8)>,
    fluid: Vec<(u32, u8)>,
    reg: Vec<(u32, f32)>,
    st: Settings,
}

fn rle(a: &[u8]) -> Vec<(u32, u8)> {
    let mut out: Vec<(u32, u8)> = vec![];
    for &v in a {
        match out.last_mut() {
            Some((n, x)) if *x == v => *n += 1,
            _ => out.push((1, v)),
        }
    }
    out
}

fn unrle(r: &[(u32, u8)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(N);
    for &(n, v) in r {
        out.extend(std::iter::repeat(v).take(n as usize));
    }
    out
}

impl Snap {
    pub fn encode(w: &World) -> Snap {
        Snap {
            cells: rle(&w.cells),
            back: rle(&w.back),
            front: rle(&w.front),
            fluid: rle(&w.fluid),
            reg: w.reg_p.iter().enumerate().filter(|(_, p)| **p != 0.0).map(|(i, p)| (i as u32, *p)).collect(),
            st: w.st,
        }
    }

    fn bytes(&self) -> usize {
        (self.cells.len() + self.back.len() + self.front.len() + self.fluid.len()) * 8 + self.reg.len() * 8 + std::mem::size_of::<Settings>()
    }

    /// Put this design on the world: only the cells that differ are touched (so the gas solver is told about
    /// exactly those), and the settings that belong to the drawing come back while the ones that are about how
    /// the game runs (pause, speed, ambient pressure, failures...) stay as they are.
    pub fn apply(&self, w: &mut World) {
        let (cells, back, front, fluid) = (unrle(&self.cells), unrle(&self.back), unrle(&self.front), unrle(&self.fluid));
        for i in 0..N {
            if w.cells[i] != cells[i] {
                w.set_cell(i, cells[i]);
            }
        }
        for (layer, new) in [(1u8, &back), (2u8, &front)] {
            for i in 0..N {
                let have = if layer == 1 { w.back[i] } else { w.front[i] };
                if have != new[i] {
                    w.set_layer_cell(layer, i, new[i]);
                }
            }
        }
        w.fluid.copy_from_slice(&fluid);
        w.reg_p.iter_mut().for_each(|p| *p = 0.0);
        for &(i, p) in &self.reg {
            w.reg_p[i as usize] = p;
        }
        let keep = w.st;
        let mut st = self.st;
        st.paused = keep.paused;
        st.failures = keep.failures;
        st.relief = keep.relief;
        st.show_particles = keep.show_particles;
        st.steps_per_frame = keep.steps_per_frame;
        st.fps_cap = keep.fps_cap;
        st.heat_accel = keep.heat_accel;
        st.h0 = keep.h0;
        st.lumped_rate = keep.lumped_rate;
        st.amb_p = keep.amb_p;
        st.gse_tanks = keep.gse_tanks;
        st.gse_he = keep.gse_he;
        st.gse_batt = keep.gse_batt;
        st.boil_speed = keep.boil_speed;
        st.fill_speed = keep.fill_speed;
        st.he_fill = keep.he_fill;
        st.src_p = keep.src_p;
        st.valve_stroke = keep.valve_stroke;
        st.governor = keep.governor;
        st.throttle = keep.throttle;
        w.st = st;
        w.nets_dirty = true;
        w.mat_stale = true;
    }
}

/// A fast hash of the design (not of how the engine is running): the parts, the pipe layers, the tank fluids and
/// the regulator settings.
pub fn design_hash(w: &World) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut mix = |v: u64| {
        h = (h ^ v).wrapping_mul(K);
        h ^= h >> 29;
    };
    for a in [&w.cells, &w.back, &w.front, &w.fluid] {
        let mut chunks = a.chunks_exact(8);
        for c in &mut chunks {
            mix(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        for &b in chunks.remainder() {
            mix(b as u64);
        }
        mix(a.len() as u64);
    }
    for (i, &p) in w.reg_p.iter().enumerate() {
        if p != 0.0 {
            mix(i as u64);
            mix(p.to_bits() as u64);
        }
    }
    h
}

pub struct History {
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    cur: Option<Snap>, // the design as of the last step
    cur_hash: u64,
}

impl Default for History {
    fn default() -> Self {
        History { undo: vec![], redo: vec![], cur: None, cur_hash: 0 }
    }
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_steps(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_steps(&self) -> usize {
        self.redo.len()
    }

    /// Take the world's design as the baseline (the first frame, and after something replaces the whole world).
    pub fn sync(&mut self, w: &World) {
        self.cur = Some(Snap::encode(w));
        self.cur_hash = design_hash(w);
    }

    /// A finished change since the last step makes a new step. Call each frame while no mouse button is down.
    pub fn commit_if_changed(&mut self, w: &World) -> bool {
        let h = design_hash(w);
        let Some(prev) = &self.cur else {
            self.sync(w);
            return false;
        };
        if h == self.cur_hash {
            return false;
        }
        self.undo.push(prev.clone());
        self.cur = Some(Snap::encode(w));
        self.cur_hash = h;
        self.redo.clear();
        self.trim();
        true
    }

    /// The simulation changed the design itself (a wall melted or burst): that is the new baseline, not a step.
    pub fn accept_sim_changes(&mut self, w: &World) {
        let h = design_hash(w);
        if h != self.cur_hash {
            self.cur = Some(Snap::encode(w));
            self.cur_hash = h;
        }
    }

    fn trim(&mut self) {
        while self.undo.len() > MAX_STEPS || self.undo.iter().map(Snap::bytes).sum::<usize>() > MAX_BYTES {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self, w: &mut World) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        if let Some(now) = self.cur.take() {
            self.redo.push(now);
        }
        prev.apply(w);
        self.cur_hash = design_hash(w);
        self.cur = Some(prev);
        true
    }

    pub fn redo(&mut self, w: &mut World) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        if let Some(now) = self.cur.take() {
            self.undo.push(now);
        }
        next.apply(w);
        self.cur_hash = design_hash(w);
        self.cur = Some(next);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::c;

    fn paint(w: &mut World, x0: usize, x1: usize, y: usize, v: u8) {
        for x in x0..=x1 {
            w.set_cell(World::idx(x, y), v);
        }
    }

    #[test]
    fn edits_undo_and_redo() {
        let mut w = World::new();
        w.clear();
        let mut h = History::default();
        h.sync(&w);
        assert!(!h.commit_if_changed(&w) && !h.can_undo());
        // three separate edits
        paint(&mut w, 100, 110, 100, c::STEEL);
        assert!(h.commit_if_changed(&w));
        paint(&mut w, 100, 110, 101, c::COPPER);
        w.set_layer_cell(1, World::idx(120, 120), c::PIPE);
        assert!(h.commit_if_changed(&w), "a cell edit and a layer pipe in one frame are one step");
        w.reg_p[World::idx(130, 130)] = 5.0e6;
        assert!(h.commit_if_changed(&w));
        assert_eq!(h.undo_steps(), 3);
        // undo them in turn
        assert!(h.undo(&mut w));
        assert_eq!(w.reg_p[World::idx(130, 130)], 0.0);
        assert!(h.undo(&mut w));
        assert_eq!(w.cells[World::idx(105, 101)], c::EMPTY);
        assert_eq!(w.back[World::idx(120, 120)], c::EMPTY);
        assert_eq!(w.cells[World::idx(105, 100)], c::STEEL);
        assert!(w.dirty.contains(&World::idx(105, 101)), "the gas solver is told which cells changed");
        assert!(h.undo(&mut w));
        assert_eq!(w.cells[World::idx(105, 100)], c::EMPTY);
        assert!(!h.undo(&mut w), "nothing more to undo");
        // and redo them
        assert!(h.redo(&mut w) && h.redo(&mut w));
        assert_eq!(w.cells[World::idx(105, 101)], c::COPPER);
        assert_eq!(w.back[World::idx(120, 120)], c::PIPE);
        assert!(h.redo(&mut w));
        assert_eq!(w.reg_p[World::idx(130, 130)], 5.0e6);
        assert!(!h.redo(&mut w));
        // undoing then drawing something new drops the redo steps
        assert!(h.undo(&mut w));
        paint(&mut w, 140, 145, 140, c::STEEL);
        assert!(h.commit_if_changed(&w));
        assert!(!h.can_redo());
        // a change that is undone and redone isn't taken as a new edit
        let steps = h.undo_steps();
        assert!(h.undo(&mut w) && h.redo(&mut w));
        assert!(!h.commit_if_changed(&w));
        assert_eq!(h.undo_steps(), steps);
    }

    #[test]
    fn clear_all_and_preset_loads_can_be_undone() {
        let mut w = World::new();
        w.load_preset(2);
        let mut h = History::default();
        h.sync(&w);
        let before = (w.cells.clone(), w.back.clone(), w.fluid.clone());
        let (dx, prop, inj) = (w.st.dx, w.st.prop, w.st.inj_area_f);
        w.clear();
        assert!(h.commit_if_changed(&w));
        assert!(w.cells.iter().all(|&v| v == c::EMPTY));
        // another engine at another scale, then back through both
        w.load_preset(8);
        assert!(h.commit_if_changed(&w));
        assert!(w.st.dx != dx);
        assert!(h.undo(&mut w) && h.undo(&mut w));
        assert_eq!(w.cells, before.0);
        assert_eq!(w.back, before.1);
        assert_eq!(w.fluid, before.2);
        assert_eq!((w.st.dx, w.st.prop, w.st.inj_area_f), (dx, prop, inj), "the settings that belong to the drawing come back");
        // the way the game is running does not
        w.st.paused = true;
        w.st.amb_p = 123.0;
        assert!(h.redo(&mut w));
        assert!(w.st.paused && w.st.amb_p == 123.0);
        // the engine still works after being rebuilt from a snapshot
        assert!(h.undo(&mut w) && h.undo(&mut w) == false);
        w.st.paused = false;
        w.rebuild_nets();
        assert!(!w.shafts.is_empty());
    }

    #[test]
    fn damage_from_the_simulation_is_not_a_step() {
        let mut w = World::new();
        w.clear();
        paint(&mut w, 100, 130, 100, c::STEEL);
        let mut h = History::default();
        h.sync(&w);
        // a wall melts while the engine runs: the new baseline, no step
        w.set_cell(World::idx(110, 100), c::EMPTY);
        h.accept_sim_changes(&w);
        assert!(!h.commit_if_changed(&w) && !h.can_undo());
        // then the player draws: undoing that puts the melted wall back as it was (still melted)
        paint(&mut w, 100, 130, 105, c::COPPER);
        assert!(h.commit_if_changed(&w));
        assert!(h.undo(&mut w));
        assert_eq!(w.cells[World::idx(110, 100)], c::EMPTY, "the melted wall is not repaired by undo");
        assert_eq!(w.cells[World::idx(110, 105)], c::EMPTY);
    }

    #[test]
    fn checking_for_changes_every_frame_is_cheap() {
        let mut w = World::new();
        w.load_preset(5);
        let t = std::time::Instant::now();
        let mut x = 0u64;
        for _ in 0..200 {
            x ^= design_hash(&w);
        }
        let hash_ms = t.elapsed().as_secs_f32() * 1e3 / 200.0;
        let t = std::time::Instant::now();
        let snap = Snap::encode(&w);
        let enc_ms = t.elapsed().as_secs_f32() * 1e3;
        let t = std::time::Instant::now();
        snap.apply(&mut w);
        let apply_ms = t.elapsed().as_secs_f32() * 1e3;
        eprintln!("design hash {hash_ms:.3} ms, encode {enc_ms:.2} ms, apply {apply_ms:.2} ms ({x})");
        // twice a frame at 60 frames a second: well under a tenth of the frame
        assert!(hash_ms < 3.0, "hashing the design takes {hash_ms} ms");
    }

    #[test]
    fn history_is_bounded() {
        let mut w = World::new();
        w.clear();
        let mut h = History::default();
        h.sync(&w);
        for k in 0..(MAX_STEPS + 40) {
            w.set_cell(World::idx(100 + k % 50, 100 + k / 50), c::STEEL);
            assert!(h.commit_if_changed(&w));
        }
        assert_eq!(h.undo_steps(), MAX_STEPS);
        // a detailed engine encodes small
        let mut e = World::new();
        e.load_preset(5);
        let snap = Snap::encode(&e);
        assert!(snap.bytes() < 400_000, "{} bytes", snap.bytes());
    }
}
