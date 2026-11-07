//! Test stand: a recorded run of the engine, with the charts and the numbers that come out of it.
//!
//! A run starts when the engine is started (or lit by hand) and is recorded ten times a second of simulated time
//! until the engine has been quiet for a while. The last run can be kept as a reference and drawn behind the
//! next one, so two designs (or two tunings of one) can be compared by eye and by the numbers.

use crate::world::World;
use eframe::egui;

pub const NCH: usize = 18;
const SAMPLE_DT: f32 = 0.1;
const MAX_SAMPLES: usize = 6000;
const QUIET_STOP: f32 = 8.0;

pub struct Channel {
    pub name: &'static str,
    pub unit: &'static str,
    pub color: [u8; 3],
    pub on: bool,
}

pub const CHANNELS: [Channel; NCH] = [
    Channel { name: "Chamber pressure", unit: "bar", color: [255, 160, 80], on: true },
    Channel { name: "Thrust", unit: "kN", color: [120, 190, 255], on: true },
    Channel { name: "Specific impulse", unit: "s", color: [200, 160, 255], on: false },
    Channel { name: "Mixture O/F", unit: "", color: [120, 230, 160], on: true },
    Channel { name: "Fuel flow", unit: "kg/s", color: [255, 210, 110], on: false },
    Channel { name: "Oxidiser flow", unit: "kg/s", color: [130, 170, 255], on: false },
    Channel { name: "Hottest wall", unit: "K", color: [255, 110, 90], on: true },
    Channel { name: "Feed line pressure", unit: "bar", color: [210, 210, 120], on: false },
    Channel { name: "Fastest shaft", unit: "% rated", color: [150, 220, 220], on: true },
    Channel { name: "Throttle", unit: "%", color: [240, 170, 60], on: false },
    Channel { name: "Fuel tank", unit: "%", color: [230, 150, 70], on: false },
    Channel { name: "Oxidiser tank", unit: "%", color: [90, 140, 230], on: false },
    Channel { name: "Battery", unit: "%", color: [120, 200, 100], on: false },
    Channel { name: "Generator gas", unit: "K", color: [255, 140, 140], on: false },
    Channel { name: "Coolant leaving jacket", unit: "K", color: [140, 200, 255], on: false },
    Channel { name: "Wall heating from mixture", unit: "x", color: [200, 120, 200], on: false },
    Channel { name: "Altitude", unit: "km", color: [170, 200, 255], on: false },
    Channel { name: "Ambient pressure", unit: "bar", color: [150, 150, 190], on: false },
];

#[derive(Clone)]
pub struct Sample {
    pub t: f32,
    pub v: [f32; NCH],
}

/// The numbers out of a run.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Metrics {
    pub duration: f32,
    pub burn_time: f32,
    pub peak_pc: f32,
    pub peak_pc_t: f32,
    pub steady_pc: f32,
    pub steady_thrust: f32,
    pub steady_of: f32,
    pub steady_isp: f32,
    pub rise_t: f32, // when the chamber reaches 90% of its steady pressure
    pub peak_thrust: f32,
    pub peak_wall: f32,
    pub impulse: f32,    // kN s
    pub fuel_used: f32,  // kg
    pub ox_used: f32,    // kg
    pub mean_isp: f32,   // from impulse and propellant used
    pub min_fuel_tank: f32,
    pub min_ox_tank: f32,
    pub min_battery: f32,
}

pub struct TestStand {
    pub open: bool,
    pub run: Vec<Sample>,
    pub reference: Vec<Sample>,
    serial: u32,
    t0: f32,
    next_t: f32,
    last_run_t: f32,
    quiet: f32,
    pub recording: bool,
    pub show: [bool; NCH],
    window_s: f32, // 0: the whole run
    pinned: Option<f32>,
    pub run_number: u32,
}

impl Default for TestStand {
    fn default() -> Self {
        TestStand {
            open: false,
            run: vec![],
            reference: vec![],
            serial: 0,
            t0: 0.0,
            next_t: 0.0,
            last_run_t: 0.0,
            quiet: 0.0,
            recording: false,
            show: std::array::from_fn(|i| CHANNELS[i].on),
            window_s: 0.0,
            pinned: None,
            run_number: 0,
        }
    }
}

/// The readings of the engine right now, in the channels' units (NaN: this engine has none).
pub fn readings(w: &World) -> [f32; NCH] {
    let g = &w.g;
    let rpm = w.shafts.iter().map(|s| s.x() * 100.0).fold(0.0f32, f32::max);
    let batt = {
        let b: Vec<f32> = w.circuits.iter().filter(|c| c.cap > 0.0).map(|c| 100.0 * c.energy / c.cap).collect();
        if b.is_empty() { f32::NAN } else { b.iter().copied().fold(100.0, f32::min) }
    };
    let gen_t = if g.t_gg > 0.0 { g.t_gg } else { f32::NAN };
    let cool = if g.t_cool > 0.0 { g.t_cool } else { f32::NAN };
    [
        g.chamber_p / 1e5,
        g.thrust / 1e3,
        g.isp,
        g.of,
        g.mdot_f,
        g.mdot_o,
        g.max_wall_t,
        g.p_line_max / 1e5,
        rpm,
        w.throttle_pos * 100.0,
        g.tank_f * 100.0,
        g.tank_o * 100.0,
        batt,
        gen_t,
        cool,
        g.wall_scale,
        if w.alt_on { w.altitude } else { crate::world::altitude_of_p(w.st.amb_p) },
        w.st.amb_p / 1e5,
    ]
}

impl TestStand {
    fn begin(&mut self, w: &World) {
        self.run.clear();
        self.serial = w.run_serial;
        self.t0 = w.run_t;
        self.next_t = w.run_t;
        self.last_run_t = w.run_t;
        self.quiet = 0.0;
        self.recording = true;
        self.pinned = None;
        self.run_number += 1;
    }

    /// Call after every world update: starts a run when the engine is started (or lit by hand), records, and
    /// stops once the engine has been quiet for a while.
    pub fn record(&mut self, w: &World) {
        let busy = w.ignited || w.g.mdot_f + w.g.mdot_o > 0.01 || w.priming >= 0.0 || w.purge_t > 0.0 || !w.seq.is_empty();
        if w.run_serial != self.serial && w.run_serial != 0 {
            self.begin(w);
        } else if !self.recording && busy && w.run_t > self.last_run_t {
            // lit by hand, or started again after the recording stopped
            self.begin(w);
        }
        if !self.recording {
            return;
        }
        let dt = (w.run_t - self.last_run_t).max(0.0);
        self.last_run_t = w.run_t;
        if busy {
            self.quiet = 0.0;
        } else {
            self.quiet += dt;
        }
        if w.run_t >= self.next_t && self.run.len() < MAX_SAMPLES {
            self.run.push(Sample { t: w.run_t - self.t0, v: readings(w) });
            self.next_t = w.run_t + SAMPLE_DT;
        }
        if self.quiet > QUIET_STOP && w.run_t - self.t0 > 5.0 {
            self.recording = false;
        }
    }

    pub fn keep_as_reference(&mut self) {
        self.reference = self.run.clone();
    }

    pub fn csv(&self) -> String {
        let mut o = String::from("t_s");
        for ch in &CHANNELS {
            o += &format!(",{} ({})", ch.name, ch.unit);
        }
        o.push('\n');
        for s in &self.run {
            o += &format!("{:.2}", s.t);
            for v in s.v {
                if v.is_nan() {
                    o += ",";
                } else {
                    o += &format!(",{v:.3}");
                }
            }
            o.push('\n');
        }
        o
    }
}

/// The value of channel `ch` at time `t` (the sample at or just before it).
pub fn value_at(samples: &[Sample], t: f32, ch: usize) -> Option<f32> {
    if samples.is_empty() || t < samples[0].t - SAMPLE_DT {
        return None;
    }
    let k = samples.partition_point(|s| s.t <= t).saturating_sub(1);
    let v = samples[k].v[ch];
    (!v.is_nan()).then_some(v)
}

/// The numbers out of a run: peaks, the steady part (the last five seconds before the pressure falls away), how fast it came up, the impulse and what it burned.
pub fn metrics(s: &[Sample]) -> Metrics {
    let mut m = Metrics::default();
    if s.len() < 2 {
        return m;
    }
    const G0: f32 = 9.80665;
    m.duration = s.last().unwrap().t - s[0].t;
    let (mut pk, mut pk_t) = (0.0f32, 0.0f32);
    for x in s {
        if x.v[0] > pk {
            pk = x.v[0];
            pk_t = x.t;
        }
        m.peak_thrust = m.peak_thrust.max(x.v[1]);
        m.peak_wall = m.peak_wall.max(x.v[6]);
    }
    (m.peak_pc, m.peak_pc_t) = (pk, pk_t);
    m.min_fuel_tank = s.iter().map(|x| x.v[10]).fold(f32::MAX, f32::min);
    m.min_ox_tank = s.iter().map(|x| x.v[11]).fold(f32::MAX, f32::min);
    m.min_battery = s.iter().map(|x| x.v[12]).filter(|v| !v.is_nan()).fold(f32::NAN, f32::min);
    for w in s.windows(2) {
        let dt = w[1].t - w[0].t;
        let (a, b) = (&w[0].v, &w[1].v);
        m.impulse += 0.5 * (a[1] + b[1]) * dt;
        m.fuel_used += 0.5 * (a[4] + b[4]) * dt;
        m.ox_used += 0.5 * (a[5] + b[5]) * dt;
        if b[1] > 0.1 * m.peak_thrust.max(1e-6) {
            m.burn_time += dt;
        }
    }
    let mass = m.fuel_used + m.ox_used;
    if mass > 1.0 {
        m.mean_isp = m.impulse * 1e3 / (mass * G0);
    }
    // the steady part: the last five seconds before the chamber pressure falls away (below 90% of its typical
    // value), where typical is the middle value over the later part of the burn (so a start overshoot, or the
    // shutdown ramp, is not counted)
    let burn: Vec<usize> = (0..s.len()).filter(|&k| s[k].v[0] > 0.2 * pk).collect();
    if let (Some(&i0), Some(&i1)) = (burn.first(), burn.last()) {
        let from = i0 + ((i1 - i0) as f32 * 0.4) as usize;
        let mut later: Vec<f32> = s[from..=i1].iter().map(|x| x.v[0]).collect();
        later.sort_by(|a, b| a.total_cmp(b));
        let typical = later[later.len() / 2];
        if let Some(end) = s.iter().rposition(|x| x.v[0] >= 0.9 * typical) {
            let t_end = s[end].t;
            let part: Vec<&Sample> = s.iter().filter(|x| x.t > t_end - 5.0 && x.t <= t_end).collect();
            let mean = |ch: usize| part.iter().map(|x| x.v[ch]).sum::<f32>() / part.len().max(1) as f32;
            (m.steady_pc, m.steady_thrust, m.steady_of, m.steady_isp) = (mean(0), mean(1), mean(3), mean(2));
            m.rise_t = s.iter().find(|x| x.v[0] >= 0.9 * m.steady_pc).map_or(0.0, |x| x.t - s[0].t);
        }
    }
    m
}

// ---- the window

impl TestStand {
    pub fn show(&mut self, ctx: &egui::Context) {
        let mut open = self.open;
        egui::Window::new("Test stand").open(&mut open).default_width(660.0).default_height(820.0).show(ctx, |ui| {
            self.contents(ui, ctx);
        });
        self.open = open;
    }

    fn contents(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dim = egui::Color32::from_gray(150);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Keep this run as the reference").on_hover_text("Draws it grey behind the next run, and puts its numbers beside the new ones").clicked() {
                self.keep_as_reference();
            }
            if ui.add_enabled(!self.reference.is_empty(), egui::Button::new("Clear reference")).clicked() {
                self.reference.clear();
            }
            if ui.add_enabled(!self.run.is_empty(), egui::Button::new("Copy run as CSV")).on_hover_text("Puts every channel of this run on the clipboard, for a spreadsheet").clicked() {
                ctx.copy_text(self.csv());
            }
            ui.label("show:");
            ui.selectable_value(&mut self.window_s, 0.0, "whole run");
            ui.selectable_value(&mut self.window_s, 30.0, "last 30 s");
            ui.selectable_value(&mut self.window_s, 10.0, "last 10 s");
        });
        let status = if self.run.is_empty() {
            "No run yet: press Start on the engine panel and the run is recorded here.".to_string()
        } else {
            format!(
                "Run {}: {:.1} s{}{}",
                self.run_number,
                self.run.last().map_or(0.0, |s| s.t),
                if self.recording { "  (recording)" } else { "  (finished)" },
                if self.reference.is_empty() { String::new() } else { "   grey: the reference run".to_string() }
            )
        };
        ui.label(egui::RichText::new(status).small().color(dim));
        ui.separator();

        // the numbers
        let m = metrics(&self.run);
        let r = (!self.reference.is_empty()).then(|| metrics(&self.reference));
        egui::CollapsingHeader::new(egui::RichText::new("Numbers").strong()).default_open(true).show(ui, |ui| {
            let rr = r.as_ref();
            let mut rows: Vec<(&str, f32, Option<f32>, &str, usize)> = vec![
                ("Steady chamber pressure", m.steady_pc, rr.map(|r| r.steady_pc), "bar", 1),
                ("Steady thrust", m.steady_thrust, rr.map(|r| r.steady_thrust), "kN", 1),
                ("Steady mixture O/F", m.steady_of, rr.map(|r| r.steady_of), "", 2),
                ("Steady specific impulse", m.steady_isp, rr.map(|r| r.steady_isp), "s", 0),
                ("Time to 90% pressure", m.rise_t, rr.map(|r| r.rise_t), "s", 1),
                ("Peak chamber pressure", m.peak_pc, rr.map(|r| r.peak_pc), "bar", 1),
                ("Peak thrust", m.peak_thrust, rr.map(|r| r.peak_thrust), "kN", 1),
                ("Hottest wall", m.peak_wall, rr.map(|r| r.peak_wall), "K", 0),
                ("Burn time", m.burn_time, rr.map(|r| r.burn_time), "s", 1),
                ("Total impulse", m.impulse, rr.map(|r| r.impulse), "kN s", 0),
                ("Mean Isp over the run", m.mean_isp, rr.map(|r| r.mean_isp), "s", 0),
                ("Fuel used", m.fuel_used, rr.map(|r| r.fuel_used), "kg", 1),
                ("Oxidiser used", m.ox_used, rr.map(|r| r.ox_used), "kg", 1),
            ];
            if !m.min_battery.is_nan() {
                rows.push(("Battery left", m.min_battery, rr.map(|r| r.min_battery), "%", 0));
            }
            // two columns of (name, value, difference from the reference)
            egui::Grid::new("stand numbers").num_columns(6).spacing([12.0, 2.0]).show(ui, |ui| {
                let half = (rows.len() + 1) / 2;
                for k in 0..half {
                    for idx in [k, k + half] {
                        match rows.get(idx) {
                            Some(&(name, v, rv, unit, d)) => {
                                ui.label(egui::RichText::new(name).small());
                                ui.label(egui::RichText::new(format!("{v:.d$} {unit}")).strong());
                                match rv {
                                    Some(rv) => {
                                        let diff = v - rv;
                                        let col = if diff.abs() < 1e-6 { dim } else { egui::Color32::from_rgb(190, 190, 120) };
                                        ui.label(egui::RichText::new(format!("{diff:+.d$}")).small().color(col)).on_hover_text(format!("the reference run: {rv:.d$} {unit}"));
                                    }
                                    None => {
                                        ui.label("");
                                    }
                                }
                            }
                            None => {
                                ui.label("");
                                ui.label("");
                                ui.label("");
                            }
                        }
                    }
                    ui.end_row();
                }
            });
            if r.is_some() {
                ui.label(egui::RichText::new("The small number beside each is the change from the reference run (hover it for the reference value).").small().color(dim));
            }
        });
        ui.separator();

        // which channels to draw
        ui.horizontal_wrapped(|ui| {
            for (i, ch) in CHANNELS.iter().enumerate() {
                let col = egui::Color32::from_rgb(ch.color[0], ch.color[1], ch.color[2]);
                ui.checkbox(&mut self.show[i], egui::RichText::new(ch.name).color(col).small());
            }
        });
        ui.separator();

        // the charts, sharing one time axis and one cursor
        let (t_lo, t_hi) = match self.run.last() {
            Some(l) => {
                let hi = l.t.max(1.0);
                (if self.window_s > 0.0 { (hi - self.window_s).max(0.0) } else { 0.0 }, hi)
            }
            None => (0.0, 1.0),
        };
        let mut hover_t: Option<f32> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for i in 0..NCH {
                if !self.show[i] {
                    continue;
                }
                if let Some(t) = self.chart(ui, i, t_lo, t_hi) {
                    hover_t = Some(t);
                }
            }
        });
        if let Some(t) = hover_t {
            ctx.request_repaint();
            let _ = t;
        }
    }

    /// One strip chart. Returns the time under the pointer if it is over this chart.
    fn chart(&mut self, ui: &mut egui::Ui, i: usize, t_lo: f32, t_hi: f32) -> Option<f32> {
        let ch = &CHANNELS[i];
        let col = egui::Color32::from_rgb(ch.color[0], ch.color[1], ch.color[2]);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 74.0), egui::Sense::click());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, egui::Color32::from_gray(18));
        let vis = |s: &&Sample| s.t >= t_lo - SAMPLE_DT && s.t <= t_hi + SAMPLE_DT;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for s in self.run.iter().chain(self.reference.iter()).filter(vis) {
            let v = s.v[i];
            if !v.is_nan() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if lo > hi {
            p.text(rect.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, format!("{} ({}): none on this engine", ch.name, ch.unit), egui::FontId::proportional(11.0), egui::Color32::from_gray(110));
            return None;
        }
        if lo > 0.0 && lo < 0.6 * hi {
            lo = 0.0;
        }
        if hi - lo < 1e-6 {
            hi = lo + 1.0;
        }
        let span = hi - lo;
        let (hi, lo) = (hi + 0.08 * span, if lo < 0.0 { lo - 0.08 * span } else { lo });
        let plot = rect.shrink2(egui::vec2(6.0, 14.0));
        let x_of = |t: f32| plot.left() + plot.width() * ((t - t_lo) / (t_hi - t_lo).max(1e-3)).clamp(0.0, 1.0);
        let y_of = |v: f32| plot.bottom() - plot.height() * ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        for f in [0.25, 0.5, 0.75] {
            let y = plot.bottom() - plot.height() * f;
            p.line_segment([egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)], egui::Stroke::new(0.5, egui::Color32::from_gray(40)));
        }
        let line = |samples: &[Sample], stroke: egui::Stroke| {
            let mut pts: Vec<egui::Pos2> = vec![];
            for s in samples.iter().filter(vis) {
                let v = s.v[i];
                if v.is_nan() {
                    if pts.len() > 1 {
                        p.add(egui::Shape::line(std::mem::take(&mut pts), stroke));
                    }
                    pts.clear();
                } else {
                    pts.push(egui::pos2(x_of(s.t), y_of(v)));
                }
            }
            if pts.len() > 1 {
                p.add(egui::Shape::line(pts, stroke));
            }
        };
        line(&self.reference, egui::Stroke::new(1.2, egui::Color32::from_gray(105)));
        line(&self.run, egui::Stroke::new(1.6, col));
        // the cursor: under the pointer, or pinned by a click
        let hover = ui.input(|inp| inp.pointer.hover_pos()).filter(|h| rect.contains(*h));
        let mut hover_t = None;
        if let Some(h) = hover {
            hover_t = Some(t_lo + (t_hi - t_lo) * ((h.x - plot.left()) / plot.width()).clamp(0.0, 1.0));
        }
        if resp.clicked() {
            self.pinned = if self.pinned.is_some() { None } else { hover_t };
        }
        let cursor_t = hover_t.or(self.pinned);
        p.text(rect.left_top() + egui::vec2(6.0, 2.0), egui::Align2::LEFT_TOP, format!("{} ({})", ch.name, ch.unit), egui::FontId::proportional(11.0), col);
        p.text(rect.right_top() + egui::vec2(-6.0, 2.0), egui::Align2::RIGHT_TOP, format!("{hi:.1}"), egui::FontId::proportional(9.0), egui::Color32::from_gray(120));
        p.text(rect.right_bottom() + egui::vec2(-6.0, -1.0), egui::Align2::RIGHT_BOTTOM, format!("{lo:.1}"), egui::FontId::proportional(9.0), egui::Color32::from_gray(120));
        if let Some(t) = cursor_t {
            let x = x_of(t);
            p.line_segment([egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
            let mut txt = format!("t {t:.1} s");
            if let Some(v) = value_at(&self.run, t, i) {
                txt += &format!("   {v:.2}");
            }
            if let Some(v) = value_at(&self.reference, t, i) {
                txt += &format!("   (reference {v:.2})");
            }
            p.text(rect.center_top() + egui::vec2(0.0, 2.0), egui::Align2::CENTER_TOP, txt, egui::FontId::proportional(11.0), egui::Color32::from_gray(230));
        }
        hover_t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Readback;

    fn sample(t: f32, pc: f32, thrust: f32, of: f32) -> Sample {
        let mut v = [0.0; NCH];
        v[0] = pc;
        v[1] = thrust;
        v[2] = if pc > 0.0 { 300.0 } else { 0.0 };
        v[3] = of;
        v[4] = 10.0;
        v[5] = 30.0;
        v[6] = 600.0 + pc;
        v[10] = 100.0 - t;
        v[11] = 100.0 - 2.0 * t;
        v[12] = f32::NAN;
        Sample { t, v }
    }

    #[test]
    fn metrics_come_out_of_a_run() {
        // a 20 s run: pressure rises to 100 bar over 4 s, holds, then the engine shuts down at 16 s
        let run: Vec<Sample> = (0..200)
            .map(|k| {
                let t = k as f32 * 0.1;
                let pc = if t < 4.0 { 25.0 * t } else if t < 16.0 { 100.0 } else { (100.0 - 50.0 * (t - 16.0)).max(0.0) };
                sample(t, pc, pc * 10.0, 5.5)
            })
            .collect();
        let m = metrics(&run);
        assert!((m.peak_pc - 100.0).abs() < 1.0, "{}", m.peak_pc);
        assert!((m.steady_pc - 100.0).abs() < 1.0, "{}", m.steady_pc);
        assert!((m.steady_of - 5.5).abs() < 0.01);
        assert!(m.rise_t > 3.4 && m.rise_t < 3.8, "90% of 100 bar at 3.6 s: {}", m.rise_t);
        // the burn is from the thrust passing 10% of its peak to it falling away again: about 0.4 s to 17.8 s
        assert!(m.burn_time > 16.5 && m.burn_time < 18.0, "{}", m.burn_time);
        // impulse: 1000 kN for 12 s, plus the ramp up (2000) and the ramp down (1000)
        assert!(m.impulse > 14_000.0 && m.impulse < 16_000.0, "{}", m.impulse);
        assert!((m.fuel_used - 10.0 * m.duration).abs() < 1.0);
        // mean Isp from impulse and propellant: impulse / (mass g)
        let want = m.impulse * 1e3 / ((m.fuel_used + m.ox_used) * 9.80665);
        assert!((m.mean_isp - want).abs() < 0.5);
        assert!(m.min_battery.is_nan(), "no battery on this engine");
        assert!(m.min_fuel_tank < 85.0);
        assert!(metrics(&[]) == Metrics::default());
        // values at a time, with the channel missing where the engine has none
        assert_eq!(value_at(&run, 10.0, 0), Some(100.0));
        assert_eq!(value_at(&run, 10.0, 12), None);
        assert_eq!(value_at(&run, -5.0, 0), None);
        // the CSV has a header and a row a sample
        let mut st = TestStand::default();
        st.run = run;
        let csv = st.csv();
        assert!(csv.starts_with("t_s,Chamber pressure (bar),Thrust (kN)"));
        assert_eq!(csv.lines().count(), 201);
    }

    #[test]
    fn a_run_is_recorded_from_start_to_quiet() {
        let mut w = World::new();
        w.load_preset(2);
        w.rebuild_nets();
        let mut st = TestStand::default();
        let rb = Readback::default();
        // nothing recorded before the engine is started
        for _ in 0..30 {
            w.update(&rb, 0.016);
            st.record(&w);
        }
        assert!(st.run.is_empty() && !st.recording);
        w.start_sequence();
        for _ in 0..(10.0 / 0.016) as usize {
            w.update(&rb, 0.016);
            st.record(&w);
        }
        assert!(st.recording && st.run_number == 1);
        // ten samples a second of simulated time
        assert!(st.run.len() > 85 && st.run.len() < 115, "{}", st.run.len());
        assert!(st.run[0].t < 0.2 && st.run.windows(2).all(|p| p[1].t > p[0].t));
        // shut it down and wait: the recording stops once the engine has been quiet for a while
        w.shutdown();
        for _ in 0..(30.0 / 0.016) as usize {
            w.update(&rb, 0.016);
            st.record(&w);
        }
        assert!(!st.recording, "recording stops after the engine goes quiet");
        let n = st.run.len();
        for _ in 0..100 {
            w.update(&rb, 0.016);
            st.record(&w);
        }
        assert_eq!(st.run.len(), n, "nothing more is recorded");
        // keeping it as the reference, and a new start begins a fresh run
        st.keep_as_reference();
        assert_eq!(st.reference.len(), n);
        w.start_sequence();
        w.update(&rb, 0.016);
        st.record(&w);
        assert!(st.recording && st.run_number == 2 && st.run.len() <= 2, "{}", st.run.len());
        assert_eq!(st.reference.len(), n, "the reference is kept");
    }
}
