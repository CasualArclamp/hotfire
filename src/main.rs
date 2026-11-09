mod design;
mod powerhead;
mod undo;
mod stand;
mod prefabs;
mod gpu;
mod presets;
mod world;

use design::{Clip, Contour, NozzleSpec};
use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use gpu::{FieldCallback, FrameInput, GpuSim, RenderParams, SimParams};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use world::{c, gas_p, PumpRef, Readback, Shaft, World, H, NO_NET, PROPS, W};

const MODES: [&str; 7] = ["Plain", "Pressure", "Velocity", "Mach", "Temperature", "Density", "Flame"];
const TOOL_VALVE: u8 = 255;
const TOOL_NOZZLE: u8 = 254;
const TOOL_INSPECT: u8 = 253;
const TOOL_FIRE: u8 = 252;
const TOOL_FUEL: u8 = 251;
const TOOL_GAS: u8 = 250;
const TOOL_FILL: u8 = 249;
const TOOL_GAUGE: u8 = 248;
const TOOL_REG: u8 = 247;
const GAUGE_R: f32 = 34.0; // dial radius on screen, points
const DESIGN_DIR: &str = "designs";
const STAMP_DIR: &str = "stamps";

struct NozzleOpts {
    expansion: f32,
    length: f32, // throat to exit, cells
    contour: Contour,
    liner: i32,
    jacket: i32,
    shell: i32,
    liner_mat: u8,
    at_throat: bool, // the clicked width is the engine's throat (used for injector sizing)
}

/// A force in newtons as a number and the unit that suits its size: MN for the big engines, kN for the
/// small ones, N for a cold-gas thruster.
fn force_text(n: f32) -> (String, &'static str) {
    let a = n.abs();
    if a >= 1.0e6 {
        (format!("{:.2}", n / 1e6), "MN")
    } else if a >= 1.0e5 {
        (format!("{:.0}", n / 1e3), "kN")
    } else if a >= 1.0e3 {
        (format!("{:.1}", n / 1e3), "kN")
    } else {
        (format!("{:.0}", n), "N")
    }
}

/// Where the nozzle maker's two clicks put the throat: centre x, row, half-width (cells).
#[derive(Clone, Copy)]
struct NozzlePlace {
    cx: f32,
    y: i32,
    hw: f32,
}

/// Shift-drag line or Ctrl-drag rectangle in progress (cell coordinates).
#[derive(Clone, Copy)]
struct ShapeDraw {
    sx: i32,
    sy: i32,
    ex: i32,
    ey: i32,
    rect: bool,
    erase: bool,
}

/// What a drag box is being drawn for (Powder Toy's Ctrl+C, Ctrl+X and S).
#[derive(Clone, Copy, PartialEq)]
enum SelectFor {
    Copy,
    Cut,
    Stamp,
}

/// Picture of a copied piece: its parts in their colours, layer pipes fainter, empty cells see-through.
fn clip_image(clip: &Clip) -> egui::ColorImage {
    let mut rgba = vec![0u8; clip.w * clip.h * 4];
    for k in 0..clip.w * clip.h {
        let (t, a) = if clip.cells[k] != c::EMPTY {
            (clip.cells[k], 255)
        } else if clip.back[k] == c::PIPE || clip.front[k] == c::PIPE {
            (c::PIPE, 130)
        } else {
            continue;
        };
        let [r, g, b] = c::rgb(t);
        rgba[4 * k..4 * k + 4].copy_from_slice(&[r, g, b, a]);
    }
    egui::ColorImage::from_rgba_unmultiplied([clip.w, clip.h], &rgba)
}

/// A round pressure gauge placed on the screen: the dial sits at `at`, it reads the pressure at cell `probe`.
struct PressureGauge {
    at: egui::Vec2,
    probe: usize,
    range_bar: f32,
}

struct App {
    world: World,
    rb: Readback,
    view_c: egui::Vec2, // view centre in cells
    view_reset: u8,     // a view button was pressed: 1 frames the engine (the top rows), 2 the whole grid
    scale: f32,         // points per cell
    mode: u32,
    show_heat: bool,
    tool: u8, // cell type to paint, or a TOOL_* code
    brush: i32,
    brush_acc: f32,
    shape: Option<ShapeDraw>,
    flood_held: bool, // a Ctrl+Shift flood fill was done on this press: no painting until the button comes up
    reset: bool,
    last: Instant,
    last_cell: Option<(i32, i32)>,
    status: String,
    flash: Option<(String, Instant)>, // a short note in the status bar (eyedropper, flood fill)
    replace: bool,          // replace mode (;): painting only changes what is already there
    labels: Vec<(f32, f32, f32, f32, String, bool)>, // names drawn on the field: centre x, y, width, height (cells), text, is a tank
    labels_age: u32,
    space_clean: bool,      // space went down and has not been used to pan yet: letting go pauses
    hover_info: String,     // what the field shows in its top-left corner for the cell under the mouse
    hover_danger: f32,      // ... and how near the part under it is to melting, 0..1
    noz: NozzleOpts,
    noz_a: Option<egui::Vec2>,        // first throat edge clicked
    noz_place: Option<NozzlePlace>, // both edges clicked: outline shown, sliders live
    target_pc_bar: f32,
    design_name: String,
    msg: String,
    selected_shaft: Option<usize>,
    layer: u8, // 0 middle (engine plane), 1 back, 2 front
    bottle_stroke: Option<usize>,       // a helium bottle cell drawn during the current drag
    bottle_prompt: Option<(usize, f32)>, // ask for the new bottle's pressure: (cell, psi)
    reg_prompt: Option<(usize, f32)>,    // set a regulator's pressure: (cell, bar)
    tank_prompt: Option<usize>,          // choose what a tank holds (a cell of it)
    gauges: Vec<PressureGauge>,
    gauge_start: Option<egui::Vec2>, // dial placed, waiting for the click that sets where it measures
    before_fire: Option<World>,
    gpu_in: std::sync::Arc<Mutex<Option<FrameInput>>>, // the frame waiting for the GPU      // the engine as it was just before it was first fired (Reset simulation)
    t0: Instant,
    step_acc: f32,   // fractional gas steps carried to the next frame
    steps_now: u32,  // gas steps to run this frame
    minimized: bool, // window minimised: everything pauses
    stroking: bool,         // a paint or erase stroke is under way
    hist: std::collections::VecDeque<[f32; 4]>, // last ~10 s for the engine panel's traces: chamber Pa, thrust N, fuel kg/s, ox kg/s
    show_engine: bool,      // the engine panel on the right
    step_budget: f32,       // most gas steps a frame may run: shrinks when frames run long, so the picture stays smooth
    sim_speed: f32,         // gas time against real time, smoothed (1 = real time)
    last_rebuild: Instant,  // the plumbing networks were last rebuilt (at most every 0.3 s mid-stroke)
    select: Option<SelectFor>,             // waiting for a drag box to copy, cut or stamp
    sel_drag: Option<(i32, i32, i32, i32)>, // the box being dragged: start and end cell
    clip: Option<Clip>,                    // the clipboard
    clip_tex: Option<egui::TextureHandle>, // ... and its picture for the paste preview
    pasting: bool,                         // the clipboard follows the mouse; a click puts it down
    stamps_open: bool,
    stamps: Vec<(String, Clip, egui::TextureHandle)>, // the stamp browser's stamps: file name, piece, picture
    prefabs_open: bool,
    ph_open: bool,
    stand: stand::TestStand,
    history: undo::History,
    undo_tick: u32,
    ph_in: Option<powerhead::PhInput>,
    ph_cache: Option<(powerhead::PhInput, powerhead::PhSize, Clip, Vec<powerhead::Port>, egui::TextureHandle)>, // sized and drawn for these inputs
    prefabs: Vec<(prefabs::Prefab, egui::TextureHandle)>, // the prefab parts, with pictures (made when first opened)
}

/// Supersonic Mach number for an area ratio A/A* (isentropic, gamma).
fn area_mach(eps: f32, g: f32) -> f32 {
    let f = |m: f32| (1.0 / m) * ((2.0 / (g + 1.0)) * (1.0 + 0.5 * (g - 1.0) * m * m)).powf((g + 1.0) / (2.0 * (g - 1.0)));
    let (mut lo, mut hi) = (1.0f32, 12.0f32);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < eps {
            lo = mid
        } else {
            hi = mid
        }
    }
    0.5 * (lo + hi)
}

impl App {
    fn new() -> Self {
        App {
            world: World::new(),
            rb: Readback::default(),
            view_c: egui::vec2(W as f32 / 2.0, world::VIEW_ROWS as f32 / 2.0),
            view_reset: 0,
            scale: 0.0,
            mode: 6,
            show_heat: true,
            tool: c::STEEL,
            brush: 1,
            brush_acc: 0.0,
            shape: None,
            flood_held: false,
            reset: true,
            last: Instant::now(),
            last_cell: None,
            status: String::new(),
            flash: None,
            replace: false,
            labels: vec![],
            labels_age: 0,
            space_clean: false,
            hover_info: String::new(),
            hover_danger: 0.0,
            noz: NozzleOpts { expansion: 11.0, length: 150.0, contour: Contour::Bell, liner: 2, jacket: 2, shell: 2, liner_mat: c::COPPER, at_throat: true },
            noz_a: None,
            noz_place: None,
            target_pc_bar: 100.0,
            design_name: "my_engine".into(),
            msg: String::new(),
            selected_shaft: None,
            layer: 0,
            bottle_stroke: None,
            bottle_prompt: None,
            reg_prompt: None,
            tank_prompt: None,
            gauges: vec![],
            gauge_start: None,
            before_fire: None,
            gpu_in: Default::default(),
            t0: Instant::now(),
            step_acc: 0.0,
            steps_now: 0,
            minimized: false,
            stroking: false,
            hist: std::collections::VecDeque::new(),
            show_engine: true,
            step_budget: 120.0,
            sim_speed: 1.0,
            last_rebuild: Instant::now(),
            select: None,
            sel_drag: None,
            clip: None,
            clip_tex: None,
            pasting: false,
            stamps_open: false,
            stamps: vec![],
            prefabs_open: false,
            ph_open: false,
            stand: stand::TestStand::default(),
            history: undo::History::default(),
            undo_tick: 0,
            ph_in: None,
            ph_cache: None,
            prefabs: vec![],
        }
    }

    /// Put a piece on the clipboard (and its picture for the preview).
    fn set_clip(&mut self, ctx: &egui::Context, clip: Clip) {
        self.clip_tex = Some(ctx.load_texture("clipboard", clip_image(&clip), egui::TextureOptions::NEAREST));
        self.clip = Some(clip);
    }

    /// Turn or mirror the clipboard while pasting.
    fn turn_clip(&mut self, ctx: &egui::Context, how: u8) {
        if let Some(cl) = &self.clip {
            let t = match how {
                0 => cl.rotated(),
                1 => cl.flipped(),
                _ => cl.flipped_v(),
            };
            self.set_clip(ctx, t);
        }
    }

    /// Start a copy, cut or stamp: the next drag on the field picks the box.
    fn start_select(&mut self, how: SelectFor) {
        self.select = Some(how);
        self.sel_drag = None;
        self.pasting = false;
    }

    /// Start pasting the clipboard (if there is anything on it).
    fn start_paste(&mut self) {
        if self.clip.is_some() {
            self.pasting = true;
            self.select = None;
            self.sel_drag = None;
        }
    }

    /// The drag box is finished: copy it, cut it or save it as a stamp.
    fn finish_select(&mut self, ctx: &egui::Context, how: SelectFor, (x0, y0, x1, y1): (i32, i32, i32, i32)) {
        let clip = self.world.copy_region(x0, y0, x1, y1);
        let (w, h) = (clip.w, clip.h);
        match how {
            SelectFor::Copy | SelectFor::Cut => {
                // the system clipboard gets it too, so Ctrl+V works after a restart or in another window
                ctx.copy_text(clip.to_text());
                self.set_clip(ctx, clip);
                if how == SelectFor::Cut {
                    if self.world.idle() {
                        self.forget_snapshot();
                    }
                    self.world.erase_region(x0, y0, x1, y1);
                }
                self.msg = format!("{} {w} x {h} cells: Ctrl+V to paste", if how == SelectFor::Cut { "cut" } else { "copied" });
            }
            SelectFor::Stamp => {
                if clip.is_empty() {
                    self.msg = "nothing to stamp there".into();
                } else {
                    let _ = std::fs::create_dir_all(STAMP_DIR);
                    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                    let path = format!("{STAMP_DIR}/stamp_{t}.stamp");
                    self.msg = match std::fs::write(&path, clip.to_text()) {
                        Ok(()) => format!("saved {path} ({w} x {h}): K opens the stamps"),
                        Err(e) => format!("stamp failed: {e}"),
                    };
                    if self.stamps_open {
                        self.load_stamps(ctx);
                    }
                }
            }
        }
        self.select = None;
        self.sel_drag = None;
    }

    /// Read the stamps folder for the browser, newest first.
    fn load_stamps(&mut self, ctx: &egui::Context) {
        let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(STAMP_DIR)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("stamp"))
                    .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        self.stamps = files
            .into_iter()
            .filter_map(|(_, p)| {
                let clip = Clip::from_text(&std::fs::read_to_string(&p).ok()?).ok()?;
                let name = p.file_stem()?.to_string_lossy().to_string();
                let tex = ctx.load_texture(format!("stamp {name}"), clip_image(&clip), egui::TextureOptions::NEAREST);
                Some((name, clip, tex))
            })
            .collect();
    }

    /// Powder Toy keys: Ctrl+C / Ctrl+X / Ctrl+V, S stamp, K stamps, R rotate (Shift: mirror, Ctrl+Shift: upside
    /// down), Esc cancel.
    fn clipboard_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (z, y, shift, cmd) = ctx.input(|i| (i.key_pressed(egui::Key::Z), i.key_pressed(egui::Key::Y), i.modifiers.shift, i.modifiers.command));
        if cmd && z && !shift {
            self.do_undo();
        } else if cmd && ((z && shift) || y) {
            self.do_redo();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::T) && !i.modifiers.any()) {
            self.stand.open = !self.stand.open;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Semicolon)) {
            self.replace = !self.replace;
            let m = if self.replace { "replace mode ON: painting only changes parts that are already there (; to turn off)" } else { "replace mode off" };
            self.flash = Some((m.to_string(), Instant::now()));
        }
        let (copy, cut, paste, s, k, pk, r, esc, mods) = ctx.input(|i| {
            let mut copy = false;
            let mut cut = false;
            let mut paste: Option<Option<String>> = None;
            for e in &i.events {
                match e {
                    egui::Event::Copy => copy = true,
                    egui::Event::Cut => cut = true,
                    egui::Event::Paste(t) => paste = Some(Some(t.clone())),
                    egui::Event::PasteImage(_) => paste = Some(None),
                    _ => {}
                }
            }
            let plain = !i.modifiers.ctrl && !i.modifiers.command && !i.modifiers.alt;
            (
                copy,
                cut,
                paste,
                plain && i.key_pressed(egui::Key::S),
                plain && i.key_pressed(egui::Key::K),
                plain && i.key_pressed(egui::Key::P),
                i.key_pressed(egui::Key::R),
                i.key_pressed(egui::Key::Escape),
                i.modifiers,
            )
        });
        if copy {
            self.start_select(SelectFor::Copy);
        }
        if cut {
            self.start_select(SelectFor::Cut);
        }
        if s {
            self.start_select(SelectFor::Stamp);
        }
        if let Some(text) = paste {
            // a stamp copied to the system clipboard (by this or another window) wins over the old clipboard
            if let Some(cl) = text.and_then(|t| Clip::from_text(&t).ok()) {
                self.set_clip(ctx, cl);
            }
            self.start_paste();
        }
        if pk {
            self.prefabs_open = !self.prefabs_open;
        }
        if k {
            self.stamps_open = !self.stamps_open;
            if self.stamps_open {
                self.load_stamps(ctx);
            }
        }
        if r && self.pasting {
            self.turn_clip(ctx, if (mods.ctrl || mods.command) && mods.shift { 2 } else if mods.shift { 1 } else { 0 });
        }
        if esc {
            self.select = None;
            self.sel_drag = None;
            self.pasting = false;
        }
    }

    fn all_mat(&self) -> Vec<u32> {
        (0..world::N).map(|i| self.world.gpu_mat(i)).collect()
    }

    /// About to fire (start, igniter, a valve opening): remember the engine as it is now, once,
    /// so Reset simulation can put everything back.
    fn before_firing(&mut self) {
        if self.before_fire.is_none() {
            self.before_fire = Some(self.world.clone());
        }
    }

    /// Back to the moment before the engine was first fired: same build, full tanks and bottles,
    /// cold walls and still gas. Settings changed since are kept.
    fn reset_simulation(&mut self) {
        if let Some(snap) = &self.before_fire {
            let st = self.world.st;
            self.world = snap.clone();
            self.world.st = st;
            self.world.mat_stale = true;
            self.world.nets_dirty = true;
            self.rb = Readback::default();
            self.reset = true;
            self.selected_shaft = None;
        }
    }

    /// A new design or an edit made while nothing is running: the next firing takes a fresh snapshot.
    fn forget_snapshot(&mut self) {
        self.before_fire = None;
    }

    /// Take back the last finished edit.
    fn do_undo(&mut self) {
        if self.history.undo(&mut self.world) {
            self.after_history("undo");
        }
    }

    fn do_redo(&mut self) {
        if self.history.redo(&mut self.world) {
            self.after_history("redo");
        }
    }

    fn after_history(&mut self, what: &str) {
        if self.world.idle() {
            self.forget_snapshot();
        }
        self.flash = Some((format!("{what}: {} to undo, {} to redo", self.history.undo_steps(), self.history.redo_steps()), Instant::now()));
    }

    /// Put a cell type, or one of the free-gas tools, into a cell.
    fn apply(&mut self, i: usize, v: u8) {
        // replace mode: only change what's already there (a part on the layer being painted), never fill empty space
        if self.replace && v != c::EMPTY && !matches!(v, TOOL_GAS | TOOL_FUEL | TOOL_FIRE) {
            let there = if self.layer != 0 && v == c::PIPE { self.world.layer_cell(self.layer, i) } else { self.world.cells[i] };
            if there == c::EMPTY {
                return;
            }
        }
        if self.world.idle() {
            self.forget_snapshot();
        }
        match v {
            TOOL_GAS => self.world.paint_gas(i, 0),
            TOOL_FUEL => self.world.paint_gas(i, 1),
            TOOL_FIRE => self.world.paint_gas(i, 2),
            // erasing on the back/front layer takes away the layer pipe there, or if there isn't one, whatever
            // is in the engine plane (other parts always go in the engine plane, so they must come off too)
            c::EMPTY if self.layer != 0 => {
                let layer_cell = if self.layer == 1 { self.world.back[i] } else { self.world.front[i] };
                if layer_cell != c::EMPTY {
                    self.world.set_layer_cell(self.layer, i, c::EMPTY);
                } else {
                    self.world.paint_gas(i, 3);
                }
            }
            v if self.layer != 0 && v == c::PIPE => self.world.set_layer_cell(self.layer, i, v),
            c::EMPTY => self.world.paint_gas(i, 3), // erase walls and any painted gas, fuel or fire
            v if c::auto_walls(v) => {
                if v == c::HE_TANK {
                    self.bottle_stroke = Some(i);
                }
                self.world.paint_plumbing(i, v)
            }
            _ => self.world.set_cell(i, v),
        }
    }

    /// Flood fill (Ctrl+Shift+click, as in Powder Toy): everything joined to the clicked cell that is the
    /// same as it (on the layer being painted) becomes `v`, painted cell by cell as the brush would.
    /// Returns how many cells it filled.
    fn flood_fill(&mut self, cx: i32, cy: i32, v: u8) -> usize {
        if cx < 0 || cy < 0 || cx >= W as i32 || cy >= H as i32 {
            return 0;
        }
        // layer pipes only go on the back/front layers; everything else is in the engine plane
        let layer = if self.layer != 0 && (v == c::PIPE || v == c::EMPTY) { self.layer } else { 0 };
        let start = World::idx(cx as usize, cy as usize);
        let target = self.world.layer_cell(layer, start);
        let gas_tool = matches!(v, TOOL_GAS | TOOL_FUEL | TOOL_FIRE);
        if (!gas_tool && target == v) || (gas_tool && target != c::EMPTY) || (v == c::EMPTY && target == c::EMPTY) {
            return 0;
        }
        let region = self.world.flood_region(layer, start);
        for &i in &region {
            self.apply(i, v);
        }
        region.len()
    }

    /// Square brush: (2r+1) x (2r+1) cells centred on (cx, cy).
    fn paint_box(&mut self, cx: i32, cy: i32, v: u8) {
        let r = self.brush;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
                    self.apply(World::idx(x as usize, y as usize), v);
                }
            }
        }
    }

    /// The powerhead maker: pick propellants, chamber pressure, throat and layout; it sizes the pumps,
    /// turbines and gas generator, shows the numbers and a picture, and puts the part down like a paste.
    fn powerhead_window(&mut self, ctx: &egui::Context) {
        let st = self.world.st;
        let mut inp = self.ph_in.unwrap_or_else(|| powerhead::PhInput::from_settings(&st, self.target_pc_bar * 1e5));
        // the engine's scale and tank pressures always come from the current settings
        inp.dx = st.dx;
        inp.depth = st.depth;
        inp.axi = st.axi;
        inp.tank_p_f = st.tank_p_f;
        inp.tank_p_o = st.tank_p_o;
        inp.steps = st.steps_per_frame;
        let mut open = true;
        let mut place = false;
        let mut size_inj = false;
        egui::Window::new("Powerhead maker").open(&mut open).default_width(480.0).show(ctx, |ui| {
            ui.label(egui::RichText::new("Sizes turbopumps, turbines and a gas generator for your engine and draws them as one part. The generator's gas goes through a reducer and a nozzle into each turbine wheel. Both propellants come in on the top edge and leave on the bottom edge through their main and throttle valves; the turbine exhaust leaves at the bottom edge too.").small());
            egui::ComboBox::from_label("propellants").selected_text(PROPS[inp.prop].name).show_ui(ui, |ui| {
                for (k, p) in PROPS.iter().enumerate() {
                    ui.selectable_value(&mut inp.prop, k, p.name);
                }
            });
            if inp.prop != st.prop {
                ui.label(egui::RichText::new(format!("Your tanks hold {}: fill them to match (Fill tool) or the sizing won't fit.", PROPS[st.prop].name)).small().color(egui::Color32::from_rgb(230, 190, 90)));
            }
            let mut pc = inp.pc / 1e5;
            ui.add(egui::Slider::new(&mut pc, 10.0..=200.0).text("chamber pressure (bar)"));
            inp.pc = pc * 1e5;
            let throat_cm = inp.throat_cells * st.dx * 100.0;
            ui.add(egui::Slider::new(&mut inp.throat_cells, 4.0..=40.0).step_by(1.0).text(format!("throat width (cells, {throat_cm:.0} cm)")));
            ui.horizontal(|ui| {
                ui.label("layout");
                ui.radio_value(&mut inp.layout, powerhead::Layout::Auto, "Auto");
                ui.radio_value(&mut inp.layout, powerhead::Layout::Single, "One shaft");
                ui.radio_value(&mut inp.layout, powerhead::Layout::Twin, "Two shafts");
            });
            ui.add(egui::Slider::new(&mut inp.t_gg, 850.0..=1300.0).text("turbine gas temperature (K)"))
                .on_hover_text("Hotter gas needs less of it (less propellant spent in the gas generator) but is harder on the turbine");
            if self.ph_cache.as_ref().map_or(true, |c| c.0 != inp) {
                let s = powerhead::size(&inp);
                let (clip, ports) = powerhead::build(&s);
                let tex = ctx.load_texture("powerhead", clip_image(&clip), egui::TextureOptions::NEAREST);
                self.ph_cache = Some((inp, s, clip, ports, tex));
            }
            let (_, s, clip, ports, tex) = self.ph_cache.as_ref().unwrap();
            ui.separator();
            ui.label(format!("Propellant flow {:.1} kg/s fuel + {:.1} kg/s oxidiser; pumps deliver {:.0} / {:.0} bar", s.mf, s.mo, s.p_dis_f / 1e5, s.p_dis_o / 1e5));
            for (k, sh) in s.shafts.iter().enumerate() {
                ui.label(egui::RichText::new(format!(
                    "Shaft {}: {:.0} rpm ({:.0}% of rated), turbine {:.0} cm x {} stage{}, {:.2} MW",
                    k + 1,
                    sh.omega * 60.0 / std::f32::consts::TAU,
                    sh.speed_frac * 100.0,
                    sh.turb_cells as f32 * st.dx * 100.0,
                    sh.turb_stages,
                    if sh.turb_stages == 1 { "" } else { "s" },
                    sh.power / 1e6
                )).strong());
                ui.label(format!(
                    "    drive gas: {}-cell nozzle, {:.0} bar down to {:.1} bar across the wheel, u/c0 {:.2} ({:.0}% efficient)",
                    sh.nozzle,
                    sh.p_in / 1e5,
                    sh.p_out / 1e5,
                    sh.u_c0,
                    sh.eff_t * 100.0
                ));
                for q in &sh.pumps {
                    ui.label(format!(
                        "    {} pump: {:.0} cm x {} stage{}, tip {:.0} m/s, {:.0} bar at {:.1} kg/s, {:.0}% efficient",
                        if q.prop == 0 { "fuel" } else { "oxidiser" },
                        q.cells as f32 * st.dx * 100.0,
                        q.stages,
                        if q.stages == 1 { "" } else { "s" },
                        q.tip,
                        q.head / 1e5,
                        q.mdot,
                        q.eff * 100.0
                    ));
                }
            }
            ui.label(format!(
                "Gas generator: O/F {:.2}, {:.1} kg/s ({:.1}% of the propellant), orifices {:.2} / {:.2} cm2 at {:.0} bar. Governor {:.0} bar, start cartridge {:.1} MW",
                s.gg_of, s.gg_mdot, 100.0 * s.gg_mdot / (s.mf + s.mo), s.gg_area_f, s.gg_area_o, s.gg_p / 1e5, s.feed_p_max / 1e5, s.cartridge_w / 1e6
            ));
            for n in &s.notes {
                ui.label(egui::RichText::new(n).small().color(egui::Color32::from_rgb(230, 190, 90)));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let f = (220.0 / clip.w.max(clip.h) as f32).min(3.0);
                let size = egui::vec2(clip.w as f32 * f, clip.h as f32 * f);
                ui.add(egui::Image::new((tex.id(), size)).bg_fill(egui::Color32::from_gray(22)));
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(format!("{} x {} cm", clip.w, clip.h)).small().weak());
                    for p in ports {
                        ui.label(egui::RichText::new(format!("{}: {}", p.name, if p.y == 0 { "top edge" } else { "bottom edge" })).small());
                    }
                    ui.add_space(6.0);
                    place = ui
                        .button(egui::RichText::new("Place powerhead").strong())
                        .on_hover_text("Click on the field to put it down (R turns it, Shift+R mirrors it). Also sets the gas generator orifices and its temperature control, the pump governor, start cartridge and mixture control.")
                        .clicked();
                    size_inj = ui.button("Size injectors for it").on_hover_text("Once it's piped up to the tanks and chamber: size the injectors for this chamber pressure").clicked();
                });
            });
        });
        self.ph_in = Some(inp);
        if let Some((inp, s, clip, _, _)) = &self.ph_cache {
            let pc = inp.pc;
            if place {
                powerhead::apply(s, &mut self.world.st);
                let clip = clip.clone();
                self.target_pc_bar = pc / 1e5;
                self.set_clip(ctx, clip);
                self.start_paste();
            } else if size_inj {
                // injector sizing resets the governor to its own guess: the powerhead's settings go back on top
                self.world.nets_dirty = true;
                self.world.auto_size_injectors(pc);
                if let Some((_, s, _, _, _)) = &self.ph_cache {
                    powerhead::apply(s, &mut self.world.st);
                }
                self.target_pc_bar = pc / 1e5;
                self.reset = true;
            }
        }
        self.ph_open = open;
    }

    fn noz_spec(&self, pl: NozzlePlace) -> NozzleSpec {
        NozzleSpec {
            cx: pl.cx,
            y_throat: pl.y,
            y_exit: pl.y + self.noz.length.round().max(4.0) as i32,
            throat_hw: pl.hw,
            expansion: self.noz.expansion,
            contour: self.noz.contour,
            liner: self.noz.liner,
            jacket: self.noz.jacket,
            shell: self.noz.shell,
            axi: self.world.st.axi,
            liner_mat: self.noz.liner_mat,
        }
    }

    fn build_placed_nozzle(&mut self) {
        let Some(pl) = self.noz_place else { return };
        let spec = self.noz_spec(pl);
        self.world.build_nozzle(&spec);
        if self.noz.at_throat {
            self.world.st.throat_cells = 2.0 * spec.throat_hw;
        }
        self.msg = format!(
            "built {} nozzle, expansion {:.1}{}",
            c::name(spec.liner_mat).split(' ').next().unwrap_or("").to_lowercase(),
            spec.expansion,
            if self.noz.at_throat { "; re-run Auto-size injectors if the throat changed" } else { "" }
        );
        self.noz_place = None;
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Hotfire");

            egui::CollapsingHeader::new("Engine").default_open(true).show(ui, |ui| {
                ui.label(egui::RichText::new("Start, shutdown, igniter and throttle are in the engine panel on the right.").small().weak());
                ui.horizontal(|ui| {
                    if ui.button(if self.world.st.paused { "Resume" } else { "Pause" }).clicked() {
                        self.world.st.paused = !self.world.st.paused;
                    }
                    if ui
                        .add_enabled(self.before_fire.is_some(), egui::Button::new("Reset simulation"))
                        .on_hover_text("Put the engine back exactly as it was before it was first fired: same parts, full tanks and bottles, cold walls")
                        .on_disabled_hover_text("Fire the engine first (Start sequence, igniter or a valve)")
                        .clicked()
                    {
                        self.reset_simulation();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Reset gas/heat").on_hover_text("Still air, cold walls, full tanks, bottles and batteries; the build stays as it is").clicked() {
                        self.reset = true;
                        for s in &mut self.world.shafts {
                            s.omega = 0.0;
                        }
                        self.world.refill_helium();
                        self.world.recharge();
                        for n in &mut self.world.nets {
                            n.mass_frac = 1.0;
                        }
                    }
                    if ui.button("Clear all").clicked() {
                        self.world.clear();
                        self.forget_snapshot();
                        self.reset = true;
                    }
                });
            });

            egui::CollapsingHeader::new("Preset engines").default_open(true).show(ui, |ui| {
                ui.label("Hover a preset for details.");
                ui.horizontal_wrapped(|ui| {
                    for (k, e) in presets::presets().iter().enumerate() {
                        if ui.button(e.name).on_hover_text(e.blurb).clicked() {
                            self.world.load_preset(k);
                            self.forget_snapshot();
                            self.selected_shaft = None;
                            self.reset = true;
                        }
                    }
                });
            });

            egui::CollapsingHeader::new("View").default_open(true).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, m) in MODES.iter().enumerate() {
                        ui.selectable_value(&mut self.mode, i as u32, *m);
                    }
                });
                ui.checkbox(&mut self.show_heat, "Glow hot walls");
                ui.checkbox(&mut self.show_engine, "Engine panel (right)");
                ui.checkbox(&mut self.world.st.show_particles, "Flow particles");
                ui.checkbox(&mut self.world.stable_jet, "Stable exhaust").on_hover_text("A planar (2D) jet flaps from side to side, so the raw exhaust swings about slowly. This damps the sideways swinging of the fast gas far below the nozzle: the shock cells close to the engine are left alone. Off: the simulation exactly as the solver has it.");
                ui.checkbox(&mut self.world.fx_plume, "Stylised exhaust plume").on_hover_text("Draw the main nozzle's exhaust as a plume shader driven by the exit speed, Mach number, flame temperature, air pressure and wind, and the start purge as a white cloud (the gas simulation underneath is unchanged). Off: the full simulation, the gas exactly as the solver has it: no plume shader, no purge cloud, no recolouring in the bell. A far plume needs a lot of gas steps to look right, so on a fast GPU raise \"gas steps per 1/60 s\" (Simulation) and the plume fills in.");
                if !self.world.fx_plume {
                    ui.label(egui::RichText::new("Full simulation: the raw gas, no plume shader").small().weak());
                } else {
                    egui::ComboBox::from_label("Mach diamonds").selected_text(world::PLUME_STYLES[(self.world.fx_style as usize).min(6)]).show_ui(ui, |ui| {
                        for (k, name) in world::PLUME_STYLES.iter().enumerate() {
                            ui.selectable_value(&mut self.world.fx_style, k as u32, *name);
                        }
                    });
                }
                ui.checkbox(&mut self.world.st.failures, "Walls can melt / burst");
                ui.horizontal(|ui| {
                    if ui.button("Engine view").on_hover_text("Frame the top of the grid, where engines are built. The grid goes on 15 m below it: scroll down with Space-drag or the middle mouse button.").clicked() {
                        self.view_reset = 1;
                    }
                    if ui.button("Whole grid").on_hover_text("Zoom out to the whole 6.4 m x 15.4 m grid, to watch a long plume.").clicked() {
                        self.view_reset = 2;
                    }
                });
            });

            egui::CollapsingHeader::new("Brush").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.history.can_undo(), egui::Button::new("Undo")).on_hover_text(format!("Ctrl+Z: take back the last edit ({} steps)", self.history.undo_steps())).clicked() {
                        self.do_undo();
                    }
                    if ui.add_enabled(self.history.can_redo(), egui::Button::new("Redo")).on_hover_text(format!("Ctrl+Y or Ctrl+Shift+Z: put it back ({} steps)", self.history.redo_steps())).clicked() {
                        self.do_redo();
                    }
                });
                ui.checkbox(&mut self.replace, "Replace mode (;)").on_hover_text("Painting only changes parts that are already there (swap a material for another), and never fills empty space");
                ui.label("Left paints, right-click erases. Space: pause. Wheel: brush size. Ctrl/Alt+wheel or +/-: zoom. Middle or Space-drag: pan. Middle-click: eyedropper (pick up what's under the cursor). Shift-drag: line. Ctrl-drag: rectangle. Ctrl+Shift+click: flood fill (right-click: flood erase).");
                ui.add(egui::Slider::new(&mut self.brush, 0..=40).text("brush radius"));
                ui.horizontal(|ui| {
                    ui.label("Pipe layer:");
                    ui.selectable_value(&mut self.layer, 1, "Back").on_hover_text("Pipes behind the engine: they pass behind walls and join the engine plane only at pipe joints");
                    ui.selectable_value(&mut self.layer, 0, "Middle").on_hover_text("The engine plane: everything that touches the gas");
                    ui.selectable_value(&mut self.layer, 2, "Front").on_hover_text("Pipes in front of the engine");
                });
                if self.layer != 0 {
                    ui.label("Back/front layers hold pipes only; they link to the engine plane at Pipe joints. Other parts still go in the middle, and erasing here removes a layer pipe or, where there is none, what is in the middle.");
                }
            });

            if ui
                .add(egui::Button::new(egui::RichText::new("Prefab parts...").strong()).min_size(egui::vec2(ui.available_width(), 24.0)))
                .on_hover_text("Ready-made combustion chambers, turbopumps, gas generators, tanks and valves: pick one and click to put it down (R turns it, Shift+R mirrors it). P opens this too.")
                .clicked()
            {
                self.prefabs_open = !self.prefabs_open;
            }
            if ui
                .add(egui::Button::new(egui::RichText::new("Powerhead maker...").strong()).min_size(egui::vec2(ui.available_width(), 24.0)))
                .on_hover_text("Turbopumps, turbines and gas generator sized for your propellants, chamber pressure and throat, drawn as one part to put down")
                .clicked()
            {
                self.ph_open = !self.ph_open;
            }
            if ui
                .add(egui::Button::new(egui::RichText::new("Test stand...").strong()).min_size(egui::vec2(ui.available_width(), 24.0)))
                .on_hover_text("Records each run (from Start) and draws it as charts you can compare with a kept reference run: chamber pressure, thrust, mixture, wall heat, shaft speed and more, with the numbers (steady pressure, impulse, propellant used). T opens this too.")
                .clicked()
            {
                self.stand.open = !self.stand.open;
            }
            // building parts, grouped by what they're for
            let part = |ui: &mut egui::Ui, tool: &mut u8, t: u8| {
                let [r, g, b] = c::rgb(t);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
                    if ui.selectable_label(*tool == t, c::name(t)).clicked() {
                        *tool = t;
                    }
                });
            };
            let groups: [(&str, &[u8]); 5] = [
                ("Structure", &[c::STEEL, c::COPPER, c::CERAMIC, c::NIOBIUM]),
                ("Plumbing", &[c::PIPE, c::VALVE, c::THROTTLE, c::JOINT, c::REG, c::INJ, c::AINJ, c::HGI]),
                ("Tanks and bottles", &[c::TANK_F, c::TANK_O, c::HE_TANK]),
                ("Turbopumps and gas generators", &[c::PUMP, c::TURBINE, c::SHAFT, c::GG, c::BURNER, c::CARTRIDGE, c::TEATEB]),
                ("Electric pumps", &[c::MOTOR, c::BATTERY, c::WIRE]),
            ];
            for (title, parts) in groups {
                egui::CollapsingHeader::new(title).default_open(true).show(ui, |ui| {
                    for &t in parts {
                        part(ui, &mut self.tool, t);
                    }
                });
            }
            egui::CollapsingHeader::new("Gas, fuel and fire").default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (t, name, col, tip) in [
                        (TOOL_GAS, "Gas", egui::Color32::from_rgb(150, 160, 190), "Paint inert 1 bar gas"),
                        (TOOL_FUEL, "Fuel vapour", egui::Color32::from_rgb(130, 210, 110), "Paint burnable fuel vapour"),
                        (
                            TOOL_FIRE,
                            "Fire",
                            egui::Color32::from_rgb(255, 140, 40),
                            "Paint 2600 K fire. It lights fuel vapour, and painted into a combustion chamber or gas generator with propellant flowing it lights it like an igniter.",
                        ),
                    ] {
                        if ui.selectable_label(self.tool == t, egui::RichText::new(name).color(col)).on_hover_text(tip).clicked() {
                            self.tool = t;
                        }
                    }
                });
                for t in [c::SRC_GAS, c::SRC_FUEL, c::SRC_FIRE] {
                    part(ui, &mut self.tool, t);
                }
            });
            egui::CollapsingHeader::new("Tools").default_open(true).show(ui, |ui| {
                for (t, name, tip) in [
                    (TOOL_NOZZLE, "Nozzle maker (click both throat edges)", "Click the two edges of a throat (or of a nozzle exit to extend), tune the outline with the sliders, then Build"),
                    (TOOL_GAUGE, "Pressure gauge (click: dial, click: where it measures)", "First click places a round gauge, the second click is the point it measures. Right-click a dial to remove it."),
                    (TOOL_REG, "Set regulator pressure (click a regulator)", "Click a Pressure regulator to set the pressure it holds its tank at"),
                    (TOOL_FILL, "Fill (click a bottle, tank, regulator or battery)", "Helium bottle: choose its pressure. Fuel / oxidiser tank: choose what it holds, or top it up. Pressure regulator: set the pressure it holds its tank at. Battery: recharge it."),
                    (TOOL_INSPECT, "Inspect turbopump (click)", "Opens the pump map of the turbopump you click"),
                    (TOOL_VALVE, "Toggle valve (click)", "Opens or closes the valve you click"),
                ] {
                    if ui.selectable_label(self.tool == t, name).on_hover_text(tip).clicked() {
                        self.tool = t;
                    }
                }
            });
            egui::CollapsingHeader::new("Copy, paste and stamps").default_open(true).show(ui, |ui| {
                ui.label("Copy, Cut and Stamp: then drag a box on the engine. Paste: the piece follows the mouse, click to put it down (hold Shift to keep pasting).");
                let ctx = ui.ctx().clone();
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.select == Some(SelectFor::Copy), "Copy").on_hover_text("Ctrl+C").clicked() {
                        self.start_select(SelectFor::Copy);
                    }
                    if ui.selectable_label(self.select == Some(SelectFor::Cut), "Cut").on_hover_text("Ctrl+X").clicked() {
                        self.start_select(SelectFor::Cut);
                    }
                    if ui.add_enabled(self.clip.is_some(), egui::Button::new("Paste").selected(self.pasting)).on_hover_text("Ctrl+V").clicked() {
                        self.start_paste();
                    }
                    if ui.selectable_label(self.select == Some(SelectFor::Stamp), "Stamp").on_hover_text("S: save a piece to the stamps folder").clicked() {
                        self.start_select(SelectFor::Stamp);
                    }
                    if ui.selectable_label(self.stamps_open, "Stamps...").on_hover_text("K: browse saved stamps").clicked() {
                        self.stamps_open = !self.stamps_open;
                        if self.stamps_open {
                            self.load_stamps(&ctx);
                        }
                    }
                });
                if self.clip.is_some() {
                    ui.horizontal(|ui| {
                        if ui.button("Rotate").on_hover_text("R").clicked() {
                            self.turn_clip(&ctx, 0);
                        }
                        if ui.button("Mirror").on_hover_text("Shift+R").clicked() {
                            self.turn_clip(&ctx, 1);
                        }
                        if ui.button("Upside down").on_hover_text("Ctrl+Shift+R").clicked() {
                            self.turn_clip(&ctx, 2);
                        }
                        if let Some(cl) = &self.clip {
                            ui.label(format!("clipboard {} x {}", cl.w, cl.h));
                        }
                    });
                }
            });

            if self.tool == TOOL_NOZZLE {
                ui.separator();
                ui.strong("Nozzle maker");
                let axi = self.world.st.axi;
                let g = self.world.st.p().gamma;
                let dx = self.world.st.dx;
                ui.label(match (self.noz_a, self.noz_place) {
                    (None, None) => "Click one edge of the throat (or of the nozzle exit you want to extend).",
                    (Some(_), _) => "Now click the other edge.",
                    (None, Some(_)) => "Adjust with the sliders (the outline follows), then Build. Right-click cancels.",
                });
                ui.horizontal(|ui| {
                    if ui.button("Regen copper bell").on_hover_text("Copper liner, fuel cooling jacket, steel shell").clicked() {
                        self.noz.liner_mat = c::COPPER;
                        (self.noz.liner, self.noz.jacket, self.noz.shell) = (2, 2, 2);
                        self.noz.at_throat = true;
                    }
                    if ui
                        .button("Niobium skirt")
                        .on_hover_text("Thin radiation-cooled niobium extension (Merlin Vacuum / RL10 style): no jacket, glows white-hot. Click the edges of an existing nozzle exit to extend it.")
                        .clicked()
                    {
                        self.noz.liner_mat = c::NIOBIUM;
                        (self.noz.liner, self.noz.jacket, self.noz.shell) = (1, 0, 0);
                        self.noz.at_throat = false;
                    }
                });
                if let Some(pl) = &mut self.noz_place {
                    ui.add(egui::Slider::new(&mut pl.hw, 2.0..=120.0).text(if axi { "start radius (cells)" } else { "start half-width (cells)" }));
                }
                ui.add(egui::Slider::new(&mut self.noz.expansion, 1.0..=80.0).logarithmic(true).text("expansion ratio (exit / start area)"));
                ui.add(egui::Slider::new(&mut self.noz.length, 5.0..=600.0).logarithmic(true).text("length (cells)"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.noz.contour, Contour::Bell, "Bell");
                    ui.selectable_value(&mut self.noz.contour, Contour::Conical, "Conical");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Liner:");
                    for (m, name) in [(c::COPPER, "Copper"), (c::NIOBIUM, "Niobium"), (c::CERAMIC, "Ceramic"), (c::STEEL, "Steel")] {
                        ui.selectable_value(&mut self.noz.liner_mat, m, name);
                    }
                });
                ui.add(egui::Slider::new(&mut self.noz.liner, 1..=6).text("liner thickness"));
                ui.add(egui::Slider::new(&mut self.noz.jacket, 0..=4).text("fuel jacket"));
                ui.add(egui::Slider::new(&mut self.noz.shell, 0..=6).text("steel shell"));
                ui.checkbox(&mut self.noz.at_throat, "This is the throat (sets injector sizing)");
                let m = area_mach(self.noz.expansion.max(1.0001), g);
                let pr = (1.0 + 0.5 * (g - 1.0) * m * m).powf(-g / (g - 1.0));
                if let Some(pl) = self.noz_place {
                    let spec = self.noz_spec(pl);
                    ui.label(format!(
                        "exit {} {:.2} m, length {:.2} m",
                        if axi { "diameter" } else { "width" },
                        2.0 * spec.exit_hw() * dx,
                        (spec.y_exit - spec.y_throat) as f32 * dx
                    ));
                }
                if self.noz.at_throat {
                    ui.label(format!("ideal exit Mach {:.2}   Pe/Pc {:.5}", m, pr));
                }
                if self.noz.liner_mat == c::NIOBIUM && self.noz.jacket > 0 {
                    ui.label("(niobium skirts normally have no jacket: they cool by glowing)");
                }
                let placed = self.noz_place.is_some();
                ui.horizontal(|ui| {
                    if ui.add_enabled(placed, egui::Button::new("Build")).clicked() {
                        self.build_placed_nozzle();
                    }
                    if ui.add_enabled(placed || self.noz_a.is_some(), egui::Button::new("Cancel")).clicked() {
                        self.noz_place = None;
                        self.noz_a = None;
                    }
                });
            }
            ui.separator();

            egui::CollapsingHeader::new("Valves").default_open(true).show(ui, |ui| {
                ui.add(egui::Slider::new(&mut self.world.st.valve_stroke, 0.005..=3.0).logarithmic(true).text("valve stroke (s)"));
                ui.horizontal(|ui| {
                    if ui.button("Open all").clicked() {
                        self.before_firing();
                        for p in [0, 1, world::PROP_HE] {
                            self.world.command_all(p, true);
                        }
                    }
                    if ui.button("Close all").clicked() {
                        for p in [0, 1, world::PROP_HE] {
                            self.world.command_all(p, false);
                        }
                    }
                });
                let mut cmds = vec![];
                for (k, v) in self.world.valves.iter().enumerate() {
                    let vlabel = self.world.valve_label(k);
                    ui.horizontal(|ui| {
                        let label = if v.cmd { "Close" } else { "Open" };
                        if ui.button(label).clicked() {
                            cmds.push((k, !v.cmd));
                        }
                        let state = if v.pos >= 1.0 { "open" } else if v.pos <= 0.0 { "closed" } else if v.cmd { "opening" } else { "closing" };
                        ui.add(egui::ProgressBar::new(v.pos).desired_width(70.0).text(state));
                        ui.label(vlabel);
                    });
                }
                for (k, open) in cmds {
                    if open {
                        self.before_firing();
                    }
                    self.world.command_valve(k, open);
                }
                if self.world.valves.is_empty() {
                    ui.label("No valves. Paint a valve cell into a line to add one.");
                }
            });
            egui::CollapsingHeader::new("Turbopumps").default_open(true).show(ui, |ui| {
                ui.checkbox(&mut self.world.st.governor, "Governor: limits tip speed and pump discharge pressure");
                let mut bar = self.world.st.feed_p_max / 1e5;
                if ui.add(egui::Slider::new(&mut bar, 30.0..=400.0).text("max pump discharge (bar)")).changed() {
                    self.world.st.feed_p_max = bar * 1e5;
                }
                let mut pick = None;
                for (k, s) in self.world.shafts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button("Pump map").clicked() {
                            pick = Some(k);
                        }
                        let cav = s.pumps.iter().any(|p| p.cav < 0.95);
                        ui.label(format!("#{} {:.0} rpm  turbine {:.1} MW  pumps {:.1} MW{}", k + 1, s.rpm(), s.p_turb / 1e6, s.p_load / 1e6, if cav { "  CAVITATING" } else { "" }));
                    });
                }
                if pick.is_some() {
                    self.selected_shaft = pick;
                }
                if self.world.shafts.is_empty() {
                    ui.label("No turbopumps. Pump cells + turbine cells (+ shaft cells) that touch form one.");
                }
            });
            ui.collapsing("Propellant and feed", |ui| {
                let mut changed = false;
                ui.label(format!("Burning: {} (from what the tanks hold; click a tank with the Fill tool to change it)", self.world.prop_name()));
                if ui.checkbox(&mut self.world.st.axi, "Round engine (axisymmetric about centre line)").changed() {
                    changed = true;
                }
                ui.add(egui::Slider::new(&mut self.target_pc_bar, 10.0..=250.0).text("target Pc (bar)"));
                if ui.button("Auto-size injectors for target Pc").clicked() {
                    changed = true;
                }
                if changed {
                    self.world.nets_dirty = true;
                    self.world.auto_size_injectors(self.target_pc_bar * 1e5);
                    self.world.refill_helium();
                    self.reset = true;
                }
                let s = &mut self.world.st;
                ui.checkbox(&mut s.gse_tanks, "GSE: keep propellant tanks full").on_hover_text("As if the tanks were plumbed to ground support equipment");
                ui.checkbox(&mut s.gse_he, "GSE: keep helium bottles at their fill pressure");
                ui.checkbox(&mut s.gse_batt, "GSE: keep batteries charged").on_hover_text("As if the batteries were on a charger (a motor can then run as long as the pack can deliver the power)");
                let mut src_bar = s.src_p / 1e5;
                if ui.add(egui::Slider::new(&mut src_bar, 0.1..=100.0).logarithmic(true).text("source block pressure (bar)")).changed() {
                    s.src_p = src_bar * 1e5;
                }
                let mut psi = s.he_fill / world::PA_PER_PSI;
                if ui
                    .add(egui::Slider::new(&mut psi, 100.0..=10000.0).logarithmic(true).text("new helium bottle fill (psi)"))
                    .on_hover_text("Pressure a helium bottle is filled to when you draw it (a real COPV is ~4500 psi)")
                    .changed()
                {
                    s.he_fill = psi * world::PA_PER_PSI;
                }
                ui.label("Pumps on a line add their head; with no pump the tank pressure alone feeds it (pressure-fed). Regulators with their own setting override these.");
                ui.add(egui::Slider::new(&mut s.tank_p_f, 1.0e5..=3.0e7).logarithmic(true).text("fuel tank Pa"));
                ui.add(egui::Slider::new(&mut s.tank_p_o, 1.0e5..=3.0e7).logarithmic(true).text("ox tank Pa"));
                ui.add(egui::Slider::new(&mut s.turb_mult, 0.1..=30.0).logarithmic(true).text("cooling jacket heat scale"))
                    .on_hover_text("The flat 2D chamber has much less wall than a round one: this scales up the heat the coolant picks up, so the fuel leaves the jacket as warm as in a real engine. That warmth is what drives an expander's turbines.");
                ui.add(egui::Slider::new(&mut s.inj_area_f, 0.02..=50.0).logarithmic(true).text("fuel inj cm2"));
                ui.add(egui::Slider::new(&mut s.inj_area_f2, 0.005..=50.0).logarithmic(true).text("second fuel inj cm2")).on_hover_text("Tri-propellant engines: the injector size for the fuel that is not the main one (the one with fewer tank cells)");
                ui.add(egui::Slider::new(&mut s.inj_area_o, 0.02..=50.0).logarithmic(true).text("ox inj cm2"));
                ui.add(egui::Slider::new(&mut s.gg_area_f, 0.02..=20.0).logarithmic(true).text("gas-gen fuel cm2"));
                ui.add(egui::Slider::new(&mut s.gg_area_o, 0.02..=50.0).logarithmic(true).text("gas-gen ox cm2"));
                ui.add(egui::Slider::new(&mut s.gg_p, 5.0e5..=2.0e7).logarithmic(true).text("gas-gen Pa"));
            });
            ui.collapsing("Altitude", |ui| {
                let w = &mut self.world;
                let mut alt = if w.alt_on { w.altitude } else { world::altitude_of_p(w.st.amb_p) };
                if ui.add(egui::Slider::new(&mut alt, 0.0..=60.0).text("altitude (km)")).on_hover_text("Sets the air pressure round the engine from the standard atmosphere: thrust and specific impulse rise as it thins, and a nozzle sized for vacuum over-expands near the ground").changed() {
                    w.alt_on = true;
                    w.altitude = alt;
                    w.st.amb_p = world::atmosphere_p(alt).max(world::MIN_AMBIENT_P);
                }
                ui.horizontal(|ui| {
                    for (name, km) in [("Sea level", 0.0f32), ("5 km", 5.0), ("10 km", 10.0), ("30 km", 30.0), ("Vacuum", 50.0)] {
                        if ui.button(name).clicked() {
                            w.alt_on = true;
                            w.altitude = km;
                            w.climbing = false;
                            w.st.amb_p = world::atmosphere_p(km).max(world::MIN_AMBIENT_P);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(if w.climbing { "Stop climbing" } else { "Climb" }).on_hover_text("The altitude rises by itself: watch the engine come up out of the air").clicked() {
                        w.climbing = !w.climbing;
                        w.alt_on = true;
                    }
                    ui.add(egui::Slider::new(&mut w.climb_rate, 0.05..=5.0).logarithmic(true).text("km per second"));
                });
                ui.label(egui::RichText::new(format!("ambient {:.4} bar{}. Above about 48 km the gas solver treats it as vacuum (it can't hold less than 100 Pa). The test stand records altitude and ambient pressure.", w.st.amb_p / 1e5, if w.alt_on { "" } else { " (set by hand under Simulation)" })).small().weak());
            });
            ui.collapsing("Flight", |ui| {
                let w = &mut self.world;
                ui.label(egui::RichText::new("The vehicle flies on the engine's thrust against its weight and the drag of the air, so the altitude follows. Air rushes past the engine: down the screen going up, up into the nozzle coming down.").small().weak());
                ui.horizontal(|ui| {
                    if ui.button("Launch").on_hover_text("On the pad at sea level: start the engine and it lifts off once the thrust beats the weight").clicked() {
                        w.flight = true;
                        w.climbing = false;
                        w.alt_on = true;
                        w.altitude = 0.0;
                        w.vel = 0.0;
                        w.flight_msg.clear();
                        w.veh_mass = w.dry_mass + 6000.0;
                    }
                    if ui.button("Land a Falcon 9").on_hover_text("Falling tail-first from 6 km at 250 m/s, engine off. Start the engine at the right moment and slow to a stop at 0 km").clicked() {
                        w.flight = true;
                        w.climbing = false;
                        w.alt_on = true;
                        w.altitude = 6.0;
                        w.vel = -250.0;
                        w.flight_msg.clear();
                        w.veh_mass = w.dry_mass + 6000.0;
                    }
                    if w.flight && ui.button("Stop flight").clicked() {
                        w.flight = false;
                        w.vel = 0.0;
                    }
                });
                ui.add(egui::Slider::new(&mut w.dry_mass, 1000.0..=200000.0).logarithmic(true).text("dry mass (kg)")).on_hover_text("A Falcon 9 first stage is about 22 t dry. The Merlin preset gives 767 kN: enough to hover a stage of up to 78 t");
                if w.flight {
                    let weight = w.veh_mass * 9.81;
                    ui.label(format!("{:.0} m   {:+.0} m/s   mass {:.1} t   thrust / weight {:.2}", w.altitude * 1000.0, w.vel, w.veh_mass / 1000.0, w.thrust_f / weight.max(1.0)));
                }
                if !w.flight_msg.is_empty() {
                    ui.label(egui::RichText::new(w.flight_msg.clone()).strong());
                }
            });
            ui.collapsing("Simulation", |ui| {
                let s = &mut self.world.st;
                ui.add(egui::Slider::new(&mut s.cartridge_w, 1.0e5..=2.0e8).logarithmic(true).text("start cartridge W"));
                let mut hold = s.gg_t_target > 0.0;
                ui.horizontal(|ui| {
                    ui.checkbox(&mut hold, "Hold gas generator at").on_hover_text("Temperature control: the gas generator's oxidiser valve closes down whenever its gas runs hotter than this, so a lean spell (one pump up to speed before the other, a throttle change) cannot cook the turbine. It can only close the valve, so the oxidiser orifice has to be big enough to reach the temperature in the first place. The powerhead maker sets it up.");
                    if hold {
                        if s.gg_t_target <= 0.0 {
                            s.gg_t_target = 1000.0;
                        }
                        ui.add(egui::Slider::new(&mut s.gg_t_target, 850.0..=1300.0).suffix(" K"));
                    } else {
                        s.gg_t_target = 0.0;
                    }
                });
                ui.checkbox(&mut s.dead_start, "Tank-head (dead-head) start").on_hover_text("No starter at all, like the RS-25: propellant runs through on tank pressure, the preburners / gas generator light and their gas spins the pumps up while the main injectors are held back, then the main chamber opens up as the pumps come up to speed. Needs a gas generator or preburner.");
                ui.checkbox(&mut s.he_spin, "Helium spin start").on_hover_text("Spin the turbopumps up with cold helium from the bottle expanding through the turbines, instead of a solid-propellant start cartridge. The bottle empties as it does (it needs a helium bottle in the build; the cartridge's power setting is the most it gives, at least 10 MW).");
                ui.add(egui::Slider::new(&mut s.steps_per_frame, 0..=600).text("gas steps per 1/60 s"))
                    .on_hover_text("How fast the gas runs. More is faster but costs GPU time in proportion.");
                ui.add(egui::Slider::new(&mut s.fps_cap, 20.0..=240.0).text("frame rate cap"))
                    .on_hover_text("Redraws per second at most. The simulation speed doesn't depend on it; lower saves GPU power.");
                ui.add(egui::Slider::new(&mut s.boil_speed, 0.0..=500.0).logarithmic(true).text("cryogen boil-off speed-up"));
                ui.add(egui::Slider::new(&mut s.fill_speed, 0.5..=50.0).logarithmic(true).text("line fill speed (m/s)"))
                    .on_hover_text("Fastest the liquid runs down empty pipes. Real lines fill in a fraction of a second; slower lets you watch it.");
                ui.checkbox(&mut s.relief, "Tank vent valves (off: boil-off pressure builds until the tank splits)");
                ui.add(egui::Slider::new(&mut s.dx, 0.005..=0.04).text("cell size (m)"));
                ui.add(egui::Slider::new(&mut s.heat_accel, 1.0..=20000.0).logarithmic(true).text("wall heat speed-up"));
                ui.add(egui::Slider::new(&mut s.h0, 0.1..=30.0).logarithmic(true).text("film coefficient"));
                ui.add(egui::Slider::new(&mut s.depth, 0.05..=2.0).text("slab depth (m)"));
                let mut amb_bar = s.amb_p / 1e5;
                if ui.add(egui::Slider::new(&mut amb_bar, 1e-5..=1.01325).logarithmic(true).text("ambient pressure (bar)")).changed() {
                    s.amb_p = amb_bar * 1e5;
                    self.world.alt_on = false;
                    self.world.climbing = false;
                }
                ui.horizontal(|ui| {
                    if ui.button("Sea level (1 bar)").clicked() {
                        s.amb_p = 101325.0;
                        self.world.alt_on = false;
                        self.world.climbing = false;
                    }
                    if ui.button("Vacuum").clicked() {
                        s.amb_p = 100.0;
                        self.world.alt_on = false;
                        self.world.climbing = false;
                    }
                });
                ui.add(egui::Slider::new(&mut s.lumped_rate, 0.1..=10.0).logarithmic(true).text("plumbing speed"));
            });
            ui.collapsing("Designs", |ui| {
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.design_name);
                    if ui.button("Save").clicked() {
                        let _ = std::fs::create_dir_all(DESIGN_DIR);
                        let path = format!("{DESIGN_DIR}/{}.rkt", self.design_name.trim());
                        self.msg = match std::fs::write(&path, self.world.to_text()) {
                            Ok(()) => format!("saved {path}"),
                            Err(e) => format!("save failed: {e}"),
                        };
                    }
                });
                let mut files: Vec<String> = std::fs::read_dir(DESIGN_DIR)
                    .map(|d| {
                        d.filter_map(|e| e.ok())
                            .filter_map(|e| {
                                let p = e.path();
                                (p.extension().and_then(|x| x.to_str()) == Some("rkt")).then(|| p.file_stem().unwrap().to_string_lossy().to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                files.sort();
                for f in files {
                    if ui.button(format!("Load {f}")).clicked() {
                        let path = format!("{DESIGN_DIR}/{f}.rkt");
                        self.msg = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| self.world.from_text(&t)) {
                            Ok(()) => {
                                self.reset = true;
                                self.before_fire = None;
                                self.design_name = f.clone();
                                format!("loaded {path}")
                            }
                            Err(e) => format!("load failed: {e}"),
                        };
                    }
                }
                if !self.msg.is_empty() {
                    ui.label(&self.msg);
                }
            });
        });
    }

    /// Ask for a regulator's pressure, starting from what it holds now: its own setting, or the shared
    /// tank-pressure setting for the tank it feeds.
    fn open_reg_prompt(&mut self, i: usize) {
        let w = &self.world;
        let own = w.reg_p[i];
        // the regulator unit this cell belongs to (a few touching cells)
        let mut unit = vec![i];
        let mut k = 0;
        while k < unit.len() && unit.len() < 400 {
            let (x, y) = (unit[k] % W, unit[k] / W);
            for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                if nx < W && ny < H {
                    let j = World::idx(nx, ny);
                    if w.cells[j] == c::REG && !unit.contains(&j) {
                        unit.push(j);
                    }
                }
            }
            k += 1;
        }
        let ox = w.regs.iter().any(|&(cell, p, _)| p == 1 && unit.contains(&cell));
        let shared = if ox { w.st.tank_p_o } else { w.st.tank_p_f };
        self.reg_prompt = Some((i, if own > 0.0 { own } else { shared } / 1e5));
    }

    /// The engine panel on the right: what the engine is doing, at a glance.
    fn engine_panel(&mut self, ui: &mut egui::Ui) {
        let g = self.world.g;
        let w = &self.world;
        let pr_blend = w.blended_prop(); // the burn, blended when two fuels are being fed
        let pr = &pr_blend;
        let (good, warn, bad, dim) = (
            egui::Color32::from_rgb(110, 210, 130),
            egui::Color32::from_rgb(240, 190, 80),
            egui::Color32::from_rgb(240, 100, 90),
            egui::Color32::from_gray(140),
        );
        let mut pick = None;
        let mut new_thr = None;
        let mut new_mix: Option<f32> = None;
        let mut act: Option<u8> = None; // 0 start, 1 shutdown, 2 igniter
        let mut set_reg: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            // status line
            let (state, col) = if g.leak > 0.0 {
                ("LEAKING", bad)
            } else if g.ignited {
                ("FIRING", good)
            } else if g.mdot_f + g.mdot_o > 0.5 {
                ("FLOWING, NOT LIT", warn)
            } else {
                ("SAFE", dim)
            };
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new(state).color(col).strong());
                ui.label(egui::RichText::new(pr.name).color(dim));
            });
            // engine controls
            ui.horizontal(|ui| {
                let big = |t: &str, c: egui::Color32| egui::Button::new(egui::RichText::new(t).strong().color(egui::Color32::WHITE)).fill(c).min_size(egui::vec2(0.0, 26.0));
                if ui.add(big("START", egui::Color32::from_rgb(40, 120, 60))).on_hover_text("Start sequence: prime the fuel lines, then oxidiser, turbopump start cartridge and igniter").clicked() {
                    act = Some(0);
                }
                if ui.add(big("SHUTDOWN", egui::Color32::from_rgb(140, 45, 40))).on_hover_text("Close the fuel valves, then the oxidiser").clicked() {
                    act = Some(1);
                }
                let (label, fill) = if w.igniter { ("IGNITER ON", egui::Color32::from_rgb(190, 120, 30)) } else { ("IGNITER OFF", egui::Color32::from_gray(60)) };
                if ui
                    .add(big(label, fill))
                    .on_hover_text("Lights the chamber whenever fuel and oxidiser are both flowing (hypergolics don't need it). Painting Fire into a chamber works too.")
                    .clicked()
                {
                    act = Some(2);
                }
            });
            if !g.ignited && !g.no_flame.is_empty() {
                ui.colored_label(warn, g.no_flame);
            }
            if let Some((i, at_inj)) = w.mixed {
                let (x, y) = (i % W, i / W);
                ui.colored_label(
                    bad,
                    if at_inj {
                        format!("Fuel and oxidiser feed the same injectors near ({x}, {y}). Each propellant needs its own manifold: put a wall in the manifold row between them (the prefab chambers show how), so fuel and oxidiser injectors alternate or sit side by side but never share pipe.")
                    } else {
                        format!("Fuel and oxidiser lines touch near ({x}, {y}), joining them into one blocked network. Keep them apart with a wall, or cross on the Back/Front layer.")
                    },
                );
            }
            ui.add_space(4.0);

            // the three numbers that matter, big
            let tile = |ui: &mut egui::Ui, label: &str, value: String, unit: &str| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(label).small().color(dim));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.label(egui::RichText::new(value).size(22.0).strong());
                        ui.label(egui::RichText::new(unit).color(dim));
                    });
                });
            };
            ui.columns(3, |c| {
                tile(&mut c[0], "CHAMBER", format!("{:.0}", g.chamber_p / 1e5), "bar");
                let (tv, tu) = force_text(g.thrust);
                tile(&mut c[1], "THRUST", tv, tu);
                tile(&mut c[2], "ISP", format!("{:.0}", g.isp), "s");
            });
            if w.alt_on {
                ui.label(egui::RichText::new(format!("altitude {:.1} km{}   ambient {:.3} bar", w.altitude, if w.climbing { " (climbing)" } else { "" }, w.st.amb_p / 1e5)).small().color(dim));
            }

            // throttle: sets every throttle valve's opening
            let n_thr = w.nets.iter().filter(|n| n.throttle && !n.injectors.is_empty()).count();
            let mut thr = w.st.throttle * 100.0;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("THROTTLE").small().color(dim));
                let r = ui.add(egui::Slider::new(&mut thr, 0.0..=100.0).suffix(" %").step_by(1.0));
                if r.changed() {
                    new_thr = Some(thr / 100.0);
                }
            });
            // mixture-ratio control: trims the throttle valves to hold an O/F
            let n_f = w.nets.iter().any(|n| n.throttle && n.prop == 0);
            let n_o = w.nets.iter().any(|n| n.throttle && n.prop == 1);
            let mut mix_on = w.st.mix_target > 0.0;
            let mut target = ((if mix_on { w.st.mix_target } else { pr.of_opt }) / 0.05).round() * 0.05;
            ui.horizontal(|ui| {
                let c = ui.checkbox(&mut mix_on, egui::RichText::new("MIXTURE O/F").small().color(dim)).on_hover_text("Mixture-ratio control: trims the throttle valves to hold this oxidiser-to-fuel ratio (like the J-2's propellant utilisation valve). Needs a throttle valve in both the fuel and the oxidiser line.");
                // centred on stoichiometric: left of the middle fuel-rich (where engines run), right oxidiser-rich
                let s = ui.add_enabled(mix_on, egui::Slider::new(&mut target, 0.2 * pr.of_stoich..=1.8 * pr.of_stoich).fixed_decimals(2));
                if c.changed() || s.changed() {
                    new_mix = Some(if mix_on { (target / 0.05).round() * 0.05 } else { 0.0 });
                }
            });
            if mix_on {
                let note = if !(n_f && n_o) {
                    "needs a throttle valve in both lines".to_string()
                } else {
                    let side = if target < 0.97 * pr.of_stoich { "fuel-rich" } else if target > 1.03 * pr.of_stoich { "oxidiser-rich: hot, eats walls" } else { "stoichiometric" };
                    let limit = if w.mix_k < 0.1 || w.mix_k > 10.0 { "
Out of range: a valve is closed right down and the pumps still cannot deliver this ratio (resize the pumps or injectors)" } else { "" };
                    format!("{side}. Middle = stoichiometric {:.2}, best Isp about {:.1}. Valves: fuel {:.0}%, oxidiser {:.0}%{limit}", pr.of_stoich, pr.of_opt, w.thr_valves[0] * 100.0, w.thr_valves[1] * 100.0)
                };
                ui.label(egui::RichText::new(note).small().color(if n_f && n_o { dim } else { warn }));
            }
            let at = w.throttle_pos;
            if n_thr == 0 {
                ui.label(egui::RichText::new("No throttle valves: put a Throttle valve (Plumbing) in the fuel and oxidiser lines to the injectors.").small().color(warn));
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{} throttle valve{} at {:.0}%{}",
                        n_thr,
                        if n_thr == 1 { "" } else { "s" },
                        at * 100.0,
                        if n_thr == 1 { "  (only one line: the mixture ratio will shift)" } else { "" }
                    ))
                    .small()
                    .color(dim),
                );
            }
            // traces over the last ~10 s
            let trace = |ui: &mut egui::Ui, k: usize, col: egui::Color32, label: &str| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 3.0, egui::Color32::from_gray(18));
                let max = self.hist.iter().map(|h| h[k]).fold(1.0f32, f32::max) * 1.1;
                let n = self.hist.len().max(2);
                let pts: Vec<egui::Pos2> = self
                    .hist
                    .iter()
                    .enumerate()
                    .map(|(i, h)| egui::pos2(rect.left() + rect.width() * i as f32 / (n - 1) as f32, rect.bottom() - 3.0 - (rect.height() - 6.0) * (h[k].max(0.0) / max)))
                    .collect();
                if pts.len() > 1 {
                    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, col)));
                }
                p.text(rect.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(10.0), dim);
            };
            trace(ui, 0, egui::Color32::from_rgb(255, 160, 80), "chamber pressure, last 10 s");
            let (tv, tu) = force_text(g.thrust);
            trace(ui, 1, egui::Color32::from_rgb(120, 190, 255), &format!("thrust, last 10 s: {tv} {tu}"));
            ui.add_space(6.0);

            // propellant
            egui::CollapsingHeader::new(egui::RichText::new("Propellant").strong()).default_open(true).show(ui, |ui| {
                let total = g.mdot_f + g.mdot_o;
                ui.label(format!("flow {:.1} kg/s   fuel {:.1} + oxidiser {:.1}", total, g.mdot_f, g.mdot_o));
                // mixture ratio against the best one for this pair
                let of_col = if g.of <= 0.0 { dim } else if (g.of / pr.of_opt - 1.0).abs() < 0.25 { good } else { warn };
                ui.horizontal(|ui| {
                    ui.label("mixture O/F");
                    ui.label(egui::RichText::new(format!("{:.2}", (g.of / 0.05).round() * 0.05)).color(of_col).strong());
                    ui.label(egui::RichText::new(format!("(best {:.1})", pr.of_opt)).color(dim));
                });
                let tank_p = |prop: u8| w.nets.iter().filter(|n| n.prop == prop && n.tank_cells > 0).map(|n| n.p_tank).fold(0.0, f32::max);
                let bar = |ui: &mut egui::Ui, label: &str, frac: f32, text: String, col: egui::Color32| {
                    ui.horizontal(|ui| {
                        ui.add_sized([70.0, 14.0], egui::Label::new(label));
                        ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).text(text).fill(col).desired_width(ui.available_width()));
                    });
                };
                let liq = |c: [f32; 3]| egui::Color32::from_rgb((c[0] * 110.0) as u8, (c[1] * 110.0) as u8, (c[2] * 110.0) as u8);
                bar(ui, "fuel tank", g.tank_f, format!("{:.0}%  {:.1} bar", g.tank_f * 100.0, tank_p(0) / 1e5), liq(pr.liq_f));
                bar(ui, "ox tank", g.tank_o, format!("{:.0}%  {:.1} bar", g.tank_o * 100.0, tank_p(1) / 1e5), liq(pr.liq_o));
                if !w.he_bottles.is_empty() {
                    bar(ui, "helium", w.he_frac, format!("{:.0}%  regulating {:.0} bar", w.he_frac * 100.0, g.p_reg / 1e5), egui::Color32::from_rgb(120, 120, 150));
                }
                for b in w.he_bottles.iter().filter(|b| b.tea) {
                    bar(ui, "TEA-TEB", b.p / b.p0.max(1.0), format!("{:.0}% left", 100.0 * b.p / b.p0.max(1.0)), egui::Color32::from_rgb(60, 140, 80));
                }
                let line_frac = g.p_line_max / world::LINE_BURST;
                bar(
                    ui,
                    "feed lines",
                    line_frac,
                    format!("{:.0} bar (burst {:.0})", g.p_line_max / 1e5, world::LINE_BURST / 1e5),
                    if line_frac > 0.85 { egui::Color32::from_rgb(170, 60, 50) } else { egui::Color32::from_rgb(70, 90, 110) },
                );
            });

            // pressure regulators: each one's setting, with a button to change it
            if !w.regs.is_empty() {
                egui::CollapsingHeader::new(egui::RichText::new("Pressure regulators").strong()).default_open(true).show(ui, |ui| {
                    for &(cell, prop, piped) in &w.regs {
                        let what = match prop {
                            0 => "fuel tank",
                            1 => "oxidiser tank",
                            _ => "no tank",
                        };
                        let own = w.reg_p[cell];
                        let shared = if prop == 1 { w.st.tank_p_o } else { w.st.tank_p_f };
                        ui.horizontal(|ui| {
                            ui.label(format!("{what}:"));
                            let txt = if own > 0.0 { format!("{:.1} bar", own / 1e5) } else { format!("{:.1} bar (shared)", shared / 1e5) };
                            ui.label(egui::RichText::new(txt).strong());
                            if ui.small_button("Set...").on_hover_text("Change the pressure this regulator holds its tank at").clicked() {
                                set_reg = Some(cell);
                            }
                        });
                        if !piped || prop == 255 {
                            ui.label(egui::RichText::new(if !piped { "  not piped to a helium bottle" } else { "  not connected to a tank" }).small().color(warn));
                        }
                    }
                    ui.label(egui::RichText::new("Or use the Set regulator pressure tool (Tools) and click one.").small().color(dim));
                });
            }

            // turbopumps
            if !w.shafts.is_empty() {
                egui::CollapsingHeader::new(egui::RichText::new("Turbopumps").strong()).default_open(true).show(ui, |ui| {
                    for (k, s) in w.shafts.iter().enumerate() {
                        let name = match (s.pumps.iter().any(|p| p.prop == 0), s.pumps.iter().any(|p| p.prop == 1)) {
                            (true, true) => "Fuel + oxidiser",
                            (true, false) => "Fuel",
                            (false, true) => "Oxidiser",
                            _ => "Turbine",
                        };
                        let x = s.x();
                        let col = if x > 1.05 { bad } else if s.pumps.iter().any(|p| p.cav < 0.95 && p.wet) { warn } else { good };
                        ui.horizontal(|ui| {
                            if ui.link(egui::RichText::new(format!("{name} pump")).strong()).on_hover_text("Open its pump map").clicked() {
                                pick = Some(k);
                            }
                            ui.label(egui::RichText::new(format!("{:.0} rpm", s.rpm())).color(col));
                        });
                        ui.add(
                            egui::ProgressBar::new((x / world::OVERSPEED).clamp(0.0, 1.0))
                                .text(format!("{:.0}% of rated speed", x * 100.0))
                                .fill(if x > 1.05 { egui::Color32::from_rgb(170, 60, 50) } else { egui::Color32::from_rgb(60, 110, 90) }),
                        );
                        let head: f32 = s.pumps.iter().map(|p| p.head).fold(0.0, f32::max);
                        if s.motor_cells > 0 {
                            // electric: motor power and the battery it runs from
                            ui.label(
                                egui::RichText::new(format!("motor {:.0} kW   pumps {:.0} kW   head {:.0} bar", s.p_motor / 1e3, s.p_load / 1e3, head / 1e5))
                                    .small()
                                    .color(dim),
                            );
                            match w.circuits.get(s.circuit as usize) {
                                Some(cc) if cc.cap > 0.0 => {
                                    let frac = cc.energy / cc.cap;
                                    let secs = if cc.p_draw > 1.0 { format!(", {:.0} s left", cc.energy / cc.p_draw) } else { String::new() };
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .text(format!("battery {:.0}% of {:.1} MJ ({} cells){secs}", frac * 100.0, cc.cap / 1e6, cc.batt_cells))
                                            .fill(if frac < 0.15 { egui::Color32::from_rgb(170, 60, 50) } else { egui::Color32::from_rgb(70, 140, 70) }),
                                    );
                                    if cc.p_draw > 0.98 * cc.p_max && cc.p_max > 0.0 {
                                        ui.colored_label(warn, format!("battery at its {:.0} kW limit: add battery cells", cc.p_max / 1e3));
                                    }
                                }
                                _ => {
                                    ui.colored_label(warn, "motor not wired to a battery");
                                }
                            }
                        } else {
                        ui.label(
                            egui::RichText::new(format!(
                                "turbine {:.1} MW ({:.0}% eff)   pumps {:.1} MW   head {:.0} bar",
                                s.p_turb / 1e6,
                                s.eff_t * 100.0,
                                s.p_load / 1e6,
                                head / 1e5
                            ))
                            .small()
                            .color(dim),
                        );
                        }
                        for p in &s.pumps {
                            if !p.wet && s.omega > 50.0 {
                                ui.colored_label(warn, format!("{} pump running dry", if p.prop == 0 { "fuel" } else { "oxidiser" }));
                            } else if p.cav < 0.95 {
                                ui.colored_label(warn, format!("{} pump cavitating: {:.0}% head", if p.prop == 0 { "fuel" } else { "oxidiser" }, p.cav * 100.0));
                            }
                        }
                        ui.add_space(3.0);
                    }
                    if g.p_gg > 0.0 || g.t_gg > 0.0 {
                        ui.label(format!("gas generator burning at {:.0} K", g.t_gg));
                    }
                });
            }

            // heat
            egui::CollapsingHeader::new(egui::RichText::new("Heat").strong()).default_open(true).show(ui, |ui| {
                ui.label(format!("flame {:.0} K", g.t_flame));
                // the bar is how close the wall that is nearest its own melting point is to it (ceramic and niobium take far more
                // than steel or copper, so a fixed scale would call a safe ceramic wall nearly molten)
                let (hot_t, melt) = if g.hot_melt > 0.0 { (g.hot_t, g.hot_melt) } else { (g.max_wall_t, 1700.0) };
                let wall_frac = hot_t / melt;
                let mat = match g.hot_mat {
                    world::c::COPPER => "copper",
                    world::c::CERAMIC => "ceramic",
                    world::c::NIOBIUM => "niobium",
                    _ => "steel",
                };
                ui.horizontal(|ui| {
                    ui.add_sized([70.0, 14.0], egui::Label::new("nearest melt"));
                    ui.add(
                        egui::ProgressBar::new(wall_frac.clamp(0.0, 1.0))
                            .text(if g.hot_melt > 0.0 { format!("{:.0} K {mat}, melts {:.0}", hot_t, melt) } else { format!("{:.0} K", hot_t) })
                            .fill(if wall_frac > 0.85 { egui::Color32::from_rgb(190, 70, 40) } else { egui::Color32::from_rgb(140, 90, 50) }),
                    );
                });
                // what the mixture does to the walls
                if g.wall_scale > 1.03 {
                    ui.colored_label(if g.wall_scale > 1.5 { bad } else { warn }, format!("oxygen-rich: no cooling film, walls take {:.0}% more heat", (g.wall_scale - 1.0) * 100.0));
                } else if g.wall_scale < 0.97 && g.wall_scale > 0.0 {
                    ui.label(egui::RichText::new(format!("fuel-rich: a cooling film cuts wall heating by {:.0}%", (1.0 - g.wall_scale) * 100.0)).color(good));
                }
                if g.t_cool > 0.0 {
                    let lim = if g.t_cool_max < 9000.0 { format!(" (limit {:.0} K)", g.t_cool_max) } else { String::new() };
                    ui.label(format!("regen jacket takes {:.1} MW, coolant leaves at {:.0} K{lim}", g.q_regen / 1e6, g.t_cool));
                }
            });

            if g.leak > 0.0 {
                ui.colored_label(bad, format!("Leaking {:.1} kg/s: jets push {:.1} kN up, {:.1} kN sideways", g.leak, g.leak_thrust / 1e3, g.leak_side / 1e3));
            }

            // what happened, newest first
            egui::CollapsingHeader::new(egui::RichText::new("Events").strong()).default_open(true).show(ui, |ui| {
                if w.events.is_empty() {
                    ui.label(egui::RichText::new("nothing yet").color(dim));
                }
                for (k, e) in w.events.iter().rev().enumerate() {
                    let serious = ["burst", "melted", "overspeed", "ruptured", "leak", "hammer"].iter().any(|x| e.contains(x));
                    let col = if serious { bad } else if e.contains("cavitat") || e.contains("too hot") { warn } else { egui::Color32::from_gray(200) };
                    ui.label(egui::RichText::new(e).color(if k == 0 { col } else { col.gamma_multiply(0.6) }));
                }
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("gas time {:.2} ms per read-back", self.rb.sim_time * 1000.0)).small().color(dim));
        });
        if pick.is_some() {
            self.selected_shaft = pick;
        }
        if let Some(t) = new_thr {
            self.world.st.throttle = t;
        }
        if let Some(m) = new_mix {
            self.world.st.mix_target = m;
        }
        if let Some(i) = set_reg {
            self.open_reg_prompt(i);
        }
        match act {
            Some(0) => {
                self.before_firing();
                self.world.start_sequence();
            }
            Some(1) => self.world.shutdown(),
            Some(2) => {
                if !self.world.igniter {
                    self.before_firing();
                }
                self.world.igniter = !self.world.igniter;
            }
            _ => {}
        }
    }

    /// Pressure at a cell for a gauge: gas pressure in open space, the line / tank / bottle pressure in plumbing.
    fn probe_pressure(&self, i: usize) -> (Option<f32>, &'static str) {
        let w = &self.world;
        let t = w.cells[i];
        if t == c::EMPTY {
            if self.rb.valid {
                return (Some(gas_p(self.rb.state[i], w.st.p().gamma)), "gas");
            }
            return (None, "gas");
        }
        if t == c::HE_TANK && w.he_of[i] != NO_NET {
            return (Some(w.he_bottles[w.he_of[i] as usize].p), "helium");
        }
        if w.net_of[i] != NO_NET {
            let n = &w.nets[w.net_of[i] as usize];
            if t == c::TANK_F || t == c::TANK_O {
                return (Some(n.p_tank), "tank");
            }
            let p = if n.pumps_ok() { n.p_line.max(n.p_up) } else { 0.0 };
            return (Some(p), "line");
        }
        (None, "solid")
    }

    /// The hover readout for a cell: lines of text, and how near to melting the part is (0..1; 0 for gas).
    fn hover_text(&self, cx: i32, cy: i32) -> (String, f32) {
        if cx < 0 || cy < 0 || cx >= W as i32 || cy >= H as i32 || !self.rb.valid {
            return (String::new(), 0.0);
        }
        let i = World::idx(cx as usize, cy as usize);
        let t = self.world.cells[i];
        let s = self.rb.state[i];
        let p = self.world.blended_prop();
        let mut danger = 0.0;
        let text = if t == c::EMPTY {
            let rho = s[0].max(1e-6);
            let (u, v) = (s[1] / rho, s[2] / rho);
            let sp = (u * u + v * v).sqrt();
            let pr = gas_p(s, p.gamma);
            let temp = pr / (rho * p.rgas);
            let mach = sp / (p.gamma * p.rgas * temp.max(1.0)).sqrt();
            let of = if temp > 900.0 { self.world.gas_of(i).map(|(r, who)| format!("\nO/F {r:.2} ({who})")).unwrap_or_default() } else { String::new() };
            format!("({cx},{cy}) gas\nP {:.2} bar   T {:.0} K\nv {:.0} m/s   M {:.2}   rho {:.3}{of}", pr / 1e5, temp, sp, mach, rho)
        } else if (t == c::TANK_F || t == c::TANK_O) && self.world.net_of[i] != world::NO_NET {
            let n = &self.world.nets[self.world.net_of[i] as usize];
            format!(
                "({cx},{cy}) {}: {}\n{:.0}% full   {:.2} bar ({:.0} psi){}",
                c::name(t),
                self.world.fluid_name(self.world.net_of[i] as usize, true),
                n.mass_frac * 100.0,
                n.p_tank / 1e5,
                n.p_tank / world::PA_PER_PSI,
                if n.boil > 0.0 { format!("\nboiling off {:.2} kg/s", n.boil) } else { String::new() }
            )
        } else if t == c::HE_TANK && self.world.he_of[i] != world::NO_NET {
            let b = &self.world.he_bottles[self.world.he_of[i] as usize];
            format!("({cx},{cy}) {} bottle\n{:.0} psi ({:.0} bar) of {:.0} psi filled", if b.tea { "TEA-TEB" } else { "helium" }, b.p / world::PA_PER_PSI, b.p / 1e5, b.p0 / world::PA_PER_PSI)
        } else if t == c::REG {
            let own = self.world.reg_p[i];
            if own > 0.0 {
                format!("({cx},{cy}) pressure regulator\nset to {:.1} bar ({:.0} psi). Fill tool: change it", own / 1e5, own / world::PA_PER_PSI)
            } else {
                format!("({cx},{cy}) pressure regulator\non the shared setting (Propellant and feed). Fill tool: give it its own")
            }
        } else if t == c::BURNER {
            format!("({cx},{cy}) {}\n{}", c::name(t), self.world.burner_info(i).unwrap_or_else(|| "not connected to both a fuel and an oxidiser line".to_string()))
        } else if t == c::BATTERY || t == c::WIRE || t == c::MOTOR {
            let mut out = format!("({cx},{cy}) {}", c::name(t));
            if let Some(cc) = self.world.circuits.get(self.world.circuit_of[i] as usize) {
                if cc.cap > 0.0 {
                    out += &format!("\nbattery {:.0}% of {:.1} MJ", 100.0 * cc.energy / cc.cap, cc.cap / 1e6);
                }
            }
            out
        } else {
            format!("({cx},{cy}) {}\nT {:.0} K", c::name(t), s[3])
        };
        // plumbing says what is in it
        let text = match ((c::plumbing(t) || t == c::VALVE_X) && !matches!(t, c::TANK_F | c::TANK_O), self.world.net_around(i)) {
            (true, Some(n)) => format!("{text}\ncarries {}", self.world.fluid_name(n, true)),
            (true, None) if t != c::INJ && t != c::JOINT => format!("{text}\nnot connected to a tank"),
            _ => text,
        };
        // a part that can melt: its melting point and how far it has got
        if t != c::EMPTY && (!c::is_line(t) || !self.world.wet_cell(i)) && s[0] == 0.0 {
            let tm = c::t_melt(t);
            let frac = ((s[3] - 300.0) / (tm - 300.0)).clamp(0.0, 1.5);
            danger = frac;
            let left = (tm - s[3]).max(0.0);
            let mut out = format!("{text}\nmelts at {:.0} K ({:.0} C)", tm, tm - 273.15);
            if t == c::NIOBIUM {
                out += "\n(radiation-cooled: it glows white-hot instead of melting)";
            }
            out += &if s[3] >= tm {
                "\nMELTING NOW".to_string()
            } else {
                format!("\n{:.0}% of the way to melting, {:.0} K to go", frac * 100.0, left)
            };
            return (out, danger);
        }
        if c::is_line(t) && self.world.wet_cell(i) {
            return (format!("{text}\ncooled by what flows through it (it doesn't melt)"), 0.0);
        }
        if (t == c::PIPE || t == c::INJ) && !self.world.wet_cell(i) {
            return (format!("{text}\nempty: just a hole in the wall, no cooling"), danger);
        }
        (text, danger)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Space pauses (a tap; holding it and dragging pans instead). Taken before any widget sees it, so a
        // button that still has focus doesn't press itself
        if !ui.ctx().egui_wants_keyboard_input() {
            let (down, up) = ui.ctx().input(|i| (i.key_pressed(egui::Key::Space), i.key_released(egui::Key::Space)));
            if down {
                self.space_clean = true;
            }
            if up && self.space_clean {
                self.space_clean = false;
                self.world.st.paused = !self.world.st.paused;
                let m = if self.world.st.paused { "paused (space to resume)" } else { "running" };
                self.flash = Some((m.to_string(), Instant::now()));
            }
            if down || up {
                ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space));
            }
        }
        // a finished edit is an undo step (not while a button is down: a stroke is one step)
        let held = ui.ctx().input(|i| i.pointer.any_down());
        // (the design is only hashed when an edit has flagged itself, and every half second as a safety net)
        self.undo_tick = self.undo_tick.wrapping_add(1);
        let flagged = self.world.nets_dirty || self.world.mat_stale || !self.world.dirty.is_empty();
        if !held && (flagged || self.undo_tick % 30 == 0) {
            self.history.commit_if_changed(&self.world);
        }
        let now = Instant::now();
        // simulate at most at the frame-rate cap; the extra redraws egui makes while the view is panned or
        // the mouse moves fast only repaint the view (no plumbing update, glow pass or field read-back)
        let since = (now - self.last).as_secs_f32();
        let sim = since >= 0.9 / self.world.st.fps_cap.max(1.0);
        let dt = since.min(0.1);
        if sim {
            self.last = now;
        }
        let prof = std::env::var("ROCKET_PROFILE").is_ok();
        let rs = frame.wgpu_render_state().expect("wgpu").clone();

        {
            let mut r = rs.renderer.write();
            if let Some(g) = r.callback_resources.get_mut::<GpuSim>() {
                g.take_readback_into(&rs.device, &mut self.rb);
            }
        }
        // gas steps are rationed per second of real time, so a fast display (or extra redraws while the
        // mouse moves) doesn't multiply the GPU work; a minimised window pauses everything
        self.minimized = ui.ctx().input(|i| i.viewport().minimized).unwrap_or(false);
        let running = !self.world.st.paused && !self.minimized && sim;
        self.steps_now = 0;
        if running {
            // keep the frame rate: when frames run long (the graphics card can't do all the gas steps and
            // draw at the frame-rate cap) run fewer steps a frame, so the gas runs a little slower than real
            // time instead of the picture stuttering and catching up in jumps
            let spf = self.world.st.steps_per_frame as f32;
            let target = 1.0 / self.world.st.fps_cap.max(1.0);
            if since > 1.35 * target {
                self.step_budget *= 0.97;
            } else if since < 1.2 * target {
                self.step_budget *= 1.015;
            }
            let want = spf * dt * 60.0;
            self.step_budget = self.step_budget.clamp((0.5 * spf).max(1.0), (2.0 * spf).max(1.0));
            self.step_acc = (self.step_acc + want).min(self.step_budget);
            self.steps_now = self.step_acc.floor() as u32;
            self.step_acc -= self.steps_now as f32;
            if want > 0.0 {
                self.sim_speed += (self.steps_now as f32 / want - self.sim_speed) * 0.05;
            }
            let tu = Instant::now();
            // rebuilding the plumbing networks takes a while on a big build: mid-stroke, only now and then
            self.world.hold_rebuild = self.stroking && self.last_rebuild.elapsed().as_secs_f32() < 0.3;
            let rebuild = self.world.nets_dirty && !self.world.hold_rebuild;
            if rebuild {
                self.last_rebuild = Instant::now();
            }
            self.world.frame_steps = self.steps_now;
            let cells_before = self.world.dirty.len();
            self.world.update(&self.rb, dt);
            self.stand.record(&self.world);
            if !held && self.world.dirty.len() > cells_before {
                // (a wall the engine melted is the new baseline, not something to undo)
                self.history.accept_sim_changes(&self.world);
            }
            let g = &self.world.g;
            self.hist.push_back([g.chamber_p, g.thrust, g.mdot_f, g.mdot_o]);
            while self.hist.len() > 600 {
                self.hist.pop_front();
            }
            if prof {
                let g = &self.world.g;
                println!(
                    "frame dt {:.1} ms  update {:.2} ms{}  steps {}  Pc {:.0} bar thrust {} rpm {:.0} line {:.0} bar ev {:?}",
                    dt * 1e3,
                    tu.elapsed().as_secs_f32() * 1e3,
                    if rebuild { " REBUILD" } else { "" },
                    self.steps_now,
                    g.chamber_p / 1e5,
                    { let (v, u) = force_text(g.thrust); format!("{v} {u}") },
                    g.rpm,
                    g.p_line_max / 1e5,
                    self.world.events.last()
                );
            }
        }

        egui::Panel::left("side").exact_size(340.0).show(ui, |ui| self.side_panel(ui));
        if self.show_engine {
            egui::Panel::right("engine").default_size(310.0).min_size(260.0).show(ui, |ui| self.engine_panel(ui));
        }
        if let Some(k) = self.selected_shaft {
            if k < self.world.shafts.len() {
                let mut open = true;
                let world = &self.world;
                egui::Window::new(format!("Turbopump {}", k + 1)).open(&mut open).default_width(330.0).show(ui.ctx(), |ui| turbopump_ui(ui, world, &world.shafts[k]));
                if !open {
                    self.selected_shaft = None;
                }
            } else {
                self.selected_shaft = None;
            }
        }
        if let Some((cell, mut psi)) = self.bottle_prompt {
            let mut done = None;
            egui::Window::new("New helium bottle").collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                ui.label("Fill it to what pressure?");
                ui.add(egui::Slider::new(&mut psi, 100.0..=10000.0).logarithmic(true).suffix(" psi"));
                ui.label(format!("= {:.0} bar   (a real COPV is about 4500 psi)", psi * world::PA_PER_PSI / 1e5));
                ui.horizontal(|ui| {
                    if ui.button("Fill").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Leave empty").clicked() {
                        done = Some(false);
                    }
                });
            });
            match done {
                Some(fill) => {
                    let p = if fill { psi * world::PA_PER_PSI } else { 0.0 };
                    self.world.set_bottle_pressure(cell, p);
                    if fill {
                        self.world.st.he_fill = p;
                    } else {
                        self.world.set_bottle_pressure(cell, 0.0);
                    }
                    self.bottle_prompt = None;
                }
                None => self.bottle_prompt = Some((cell, psi)),
            }
        }
        if let Some((cell, mut bar)) = self.reg_prompt {
            let mut done = None;
            egui::Window::new("Pressure regulator").collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                ui.label("Hold the tank it feeds at:");
                ui.add(egui::Slider::new(&mut bar, 1.0..=400.0).logarithmic(true).suffix(" bar"));
                ui.label(format!("= {:.0} psi. It can only deliver up to about 95% of its helium bottle's pressure.", bar * 1e5 / world::PA_PER_PSI));
                ui.horizontal(|ui| {
                    if ui.button("Set").clicked() {
                        done = Some(bar * 1e5);
                    }
                    if ui.button("Use the shared setting").on_hover_text("Follow the tank-pressure sliders in Propellant and feed").clicked() {
                        done = Some(0.0);
                    }
                    if ui.button("Cancel").clicked() {
                        done = Some(-1.0);
                    }
                });
            });
            match done {
                Some(p) => {
                    if p >= 0.0 {
                        self.world.set_regulator(cell, p);
                    }
                    self.reg_prompt = None;
                }
                None => self.reg_prompt = Some((cell, bar)),
            }
        }
        let ctx = ui.ctx().clone();
        self.clipboard_keys(&ctx);
        if self.prefabs_open {
            if self.prefabs.is_empty() {
                self.prefabs = prefabs::all().into_iter().map(|p| {
                    let tex = ctx.load_texture(format!("prefab {}", p.name), clip_image(&p.clip), egui::TextureOptions::NEAREST);
                    (p, tex)
                }).collect();
            }
            let mut open = true;
            let mut pick = None;
            egui::Window::new("Prefab parts").open(&mut open).default_width(460.0).default_height(520.0).show(&ctx, |ui| {
                ui.label(egui::RichText::new("Click a part, then click on the field to put it down. R turns it, Shift+R mirrors it, right-click or Esc cancels. Pipe ends are open: draw a line up to them to connect.").small());
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut group = "";
                    for (k, (pf, tex)) in self.prefabs.iter().enumerate() {
                        if pf.group != group {
                            group = pf.group;
                            ui.add_space(4.0);
                            ui.strong(group);
                        }
                        ui.horizontal(|ui| {
                            let f = 70.0 / pf.clip.w.max(pf.clip.h) as f32;
                            let size = egui::vec2(pf.clip.w as f32 * f, pf.clip.h as f32 * f);
                            let img = egui::Image::new((tex.id(), size)).bg_fill(egui::Color32::from_gray(22));
                            let clicked = ui.add(egui::Button::image(img)).clicked();
                            ui.vertical(|ui| {
                                let name = ui.add(egui::Label::new(egui::RichText::new(pf.name).strong()).sense(egui::Sense::click())).clicked();
                                ui.label(egui::RichText::new(pf.blurb).small());
                                ui.label(egui::RichText::new(format!("{} x {} cm", pf.clip.w, pf.clip.h)).small().weak());
                                if clicked || name {
                                    pick = Some(k);
                                }
                            });
                        });
                    }
                });
            });
            if let Some(k) = pick {
                let cl = self.prefabs[k].0.clip.clone();
                self.set_clip(&ctx, cl);
                self.start_paste();
                open = false;
            }
            self.prefabs_open = open;
        }
        if self.ph_open {
            self.powerhead_window(&ctx);
        }
        if self.stand.open {
            self.stand.show(&ctx);
        }
        if self.stamps_open {
            let mut open = true;
            let mut pick = None;
            let mut delete = None;
            egui::Window::new("Stamps").open(&mut open).default_width(420.0).default_height(460.0).show(&ctx, |ui| {
                if self.stamps.is_empty() {
                    ui.label("No stamps yet. Press S (or Stamp) and drag a box around part of an engine to save one.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (k, (name, cl, tex)) in self.stamps.iter().enumerate() {
                            ui.vertical(|ui| {
                                let f = 110.0 / cl.w.max(cl.h) as f32;
                                let size = egui::vec2(cl.w as f32 * f, cl.h as f32 * f);
                                let img = egui::Image::new((tex.id(), size)).bg_fill(egui::Color32::from_gray(22));
                                if ui.add(egui::Button::image(img)).on_hover_text(format!("{name}: {} x {} cells. Click to paste.", cl.w, cl.h)).clicked() {
                                    pick = Some(k);
                                }
                                ui.horizontal(|ui| {
                                    ui.small(format!("{} x {}", cl.w, cl.h));
                                    if ui.small_button("Delete").clicked() {
                                        delete = Some(k);
                                    }
                                });
                            });
                        }
                    });
                });
            });
            if let Some(k) = pick {
                let cl = self.stamps[k].1.clone();
                self.set_clip(&ctx, cl);
                self.start_paste();
                open = false;
            }
            if let Some(k) = delete {
                let _ = std::fs::remove_file(format!("{STAMP_DIR}/{}.stamp", self.stamps[k].0));
                let _ = self.stamps.remove(k);
            }
            self.stamps_open = open;
        }
        if let Some(cell) = self.tank_prompt {
            let mut done = false;
            let fuel = self.world.cells[cell] == c::TANK_F;
            let cur = self.world.tank_kind(cell);
            egui::Window::new(if fuel { "Fuel tank" } else { "Oxidiser tank" }).collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                ui.label("Fill it with (drains it and fills it full of the new one):");
                let names: &[&str] = if fuel { &world::FUELS } else { &world::OXIDISERS };
                for (k, name) in names.iter().enumerate() {
                    if ui.selectable_label(k == cur, *name).clicked() && k != cur {
                        if self.world.idle() {
                            self.forget_snapshot();
                        }
                        self.world.set_tank_fluid(cell, k as u8);
                        self.reset = true;
                        done = true;
                    }
                }
                ui.label(format!("The engine burns what its tanks hold together: now {}.", self.world.prop_name()));
                ui.horizontal(|ui| {
                    if ui.button("Top up").clicked() {
                        if self.world.refill_tank(cell) {
                            self.msg = "tank refilled".into();
                        }
                        done = true;
                    }
                    if ui.button("Close").clicked() {
                        done = true;
                    }
                });
            });
            if done || !matches!(self.world.cells[cell], c::TANK_F | c::TANK_O) {
                self.tank_prompt = None;
            }
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if !self.world.st.paused && self.sim_speed < 0.95 {
                    ui.label(egui::RichText::new(format!("gas at {:.0}% speed", self.sim_speed * 100.0)).color(egui::Color32::from_rgb(230, 180, 80)))
                        .on_hover_text("The graphics card can't run every gas step and keep the frame rate, so the gas runs a bit slower than real time. Lower 'gas steps per 1/60 s' or the frame-rate cap under Simulation to trade one for the other.");
                }
                if let Some((m, at)) = &self.flash {
                    if at.elapsed().as_secs_f32() < 2.0 {
                        ui.label(egui::RichText::new(m).color(egui::Color32::from_rgb(120, 200, 255)));
                    }
                }
                if self.replace {
                    ui.label(egui::RichText::new("REPLACE mode (;)").color(egui::Color32::from_rgb(255, 190, 90)));
                }
                ui.label(self.status.clone());
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            // the GPU field goes in this slot so cursor, previews and outlines are painted over it
            let field_slot = painter.add(egui::Shape::Noop);
            let rect = resp.rect;
            if self.scale == 0.0 {
                self.scale = rect.height() / world::VIEW_ROWS as f32;
            }
            match std::mem::take(&mut self.view_reset) {
                1 => {
                    self.scale = rect.height() / world::VIEW_ROWS as f32;
                    self.view_c = egui::vec2(W as f32 / 2.0, world::VIEW_ROWS as f32 / 2.0);
                }
                2 => {
                    self.scale = rect.height() / H as f32;
                    self.view_c = egui::vec2(W as f32 / 2.0, H as f32 / 2.0);
                }
                _ => {}
            }
            let ppp = ui.ctx().pixels_per_point();
            let to_cell = |p: egui::Pos2, view_c: egui::Vec2, scale: f32| -> egui::Vec2 { view_c + (p - rect.center()) / scale };
            let to_screen = |c: egui::Vec2, view_c: egui::Vec2, scale: f32| -> egui::Pos2 { rect.center() + (c - view_c) * scale };

            if resp.hovered() {
                // wheel resizes the brush (like Powder Toy); Ctrl/Alt + wheel or +/- zooms
                let (scroll, zoom, alt) = ui.input(|i| (i.smooth_scroll_delta.y + i.smooth_scroll_delta.x, i.zoom_delta(), i.modifiers.alt));
                let (zin, zout) = ui.input(|i| (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals), i.key_pressed(egui::Key::Minus)));
                let mut zoom_by = if zoom != 1.0 {
                    zoom
                } else if alt && scroll != 0.0 {
                    1.0 + scroll * 0.002
                } else {
                    1.0
                };
                if zin {
                    zoom_by *= 1.25;
                }
                if zout {
                    zoom_by /= 1.25;
                }
                if zoom_by != 1.0 {
                    let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                    let before = to_cell(p, self.view_c, self.scale);
                    self.scale = (self.scale * zoom_by).clamp(0.5, 60.0);
                    let after = to_cell(p, self.view_c, self.scale);
                    self.view_c += before - after;
                } else if scroll != 0.0 {
                    self.brush_acc += scroll / 30.0;
                    let steps = self.brush_acc.trunc();
                    if steps != 0.0 {
                        self.brush = (self.brush + steps as i32).clamp(0, 40);
                        self.brush_acc -= steps;
                    }
                }
            }
            let space = ui.input(|i| i.key_down(egui::Key::Space));
            if resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Primary) && space) {
                self.view_c -= resp.drag_delta() / self.scale;
                self.space_clean = false; // it was used to pan, so letting go doesn't pause
            }

            let mut status = String::new();
            let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
            if hover.is_none() {
                self.hover_info.clear();
            }

            // eyedropper (as in Powder Toy): a middle click (not a drag, which pans) picks up what's under the
            // cursor as the tool
            if resp.clicked_by(egui::PointerButton::Middle) {
                if let Some(p) = hover {
                    let cc = to_cell(p, self.view_c, self.scale);
                    let (cx, cy) = (cc.x.floor() as i32, cc.y.floor() as i32);
                    if cx >= 0 && cy >= 0 && cx < W as i32 && cy < H as i32 {
                        let i = World::idx(cx as usize, cy as usize);
                        // the layer being worked on first (its pipes), then the engine plane
                        let on_layer = self.layer != 0 && self.world.layer_cell(self.layer, i) != c::EMPTY;
                        let t = if on_layer { c::PIPE } else { self.world.cells[i] };
                        // a shut valve is drawn with the valve tool
                        let t = if t == c::VALVE_X { c::VALVE } else { t };
                        if t == c::EMPTY {
                            self.flash = Some(("eyedropper: nothing here to pick up".into(), Instant::now()));
                        } else {
                            self.tool = t;
                            self.pasting = false;
                            self.select = None;
                            self.flash = Some((format!("eyedropper: {}", c::name(t)), Instant::now()));
                        }
                    }
                }
            }

            // nozzle maker: click both edges of the throat, then tune it with the sliders and Build
            if self.tool == TOOL_NOZZLE && !space {
                if let Some(p) = hover {
                    let cc = to_cell(p, self.view_c, self.scale);
                    if resp.clicked() {
                        match self.noz_a.take() {
                            None => {
                                self.noz_a = Some(cc);
                                self.noz_place = None;
                            }
                            Some(a) => {
                                let axi = self.world.st.axi;
                                // centre snaps to a cell edge or middle so the nozzle is symmetric
                                let cx = if axi { W as f32 / 2.0 } else { ((a.x + cc.x) * 0.5 * 2.0).round() * 0.5 };
                                let hw = if axi { 0.5 * ((a.x - cx).abs() + (cc.x - cx).abs()) } else { 0.5 * (cc.x - a.x).abs() };
                                self.noz_place = Some(NozzlePlace { cx, y: (0.5 * (a.y + cc.y)).floor() as i32, hw: hw.round().max(2.0) });
                            }
                        }
                    }
                    if resp.secondary_clicked() {
                        self.noz_a = None;
                        self.noz_place = None;
                    }
                    if let Some(a) = self.noz_a {
                        let sa = to_screen(a, self.view_c, self.scale);
                        let col = egui::Color32::from_rgb(120, 220, 255);
                        painter.line_segment([sa, p], egui::Stroke::new(1.5, col));
                        painter.circle_filled(sa, 3.5, col);
                        let w = (cc.x - a.x).abs();
                        status = format!("throat width {:.0} cells ({:.2} m): click the other edge", w, w * self.world.st.dx);
                    }
                }
                if let Some(pl) = self.noz_place {
                    let spec = self.noz_spec(pl);
                    let g = self.world.st.p().gamma;
                    let m = area_mach(spec.expansion.max(1.0001), g);
                    status = format!(
                        "nozzle: expansion {:.1}   exit {:.2} m {}   length {:.2} m   ideal exit Mach {:.2}   (Build in the side panel)",
                        spec.expansion,
                        2.0 * spec.exit_hw() * self.world.st.dx,
                        if spec.axi { "diameter" } else { "width" },
                        (spec.y_exit - spec.y_throat) as f32 * self.world.st.dx,
                        m
                    );
                    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(120, 220, 255));
                    let outer = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 200, 120));
                    let mut pl: [Vec<egui::Pos2>; 4] = [vec![], vec![], vec![], vec![]];
                    let mut y = spec.y_throat;
                    while y <= spec.y_exit {
                        let h = spec.hw(y as f32 + 0.5);
                        let o = h + spec.thickness();
                        let yy = y as f32;
                        pl[0].push(to_screen(egui::vec2(spec.cx - h, yy), self.view_c, self.scale));
                        pl[1].push(to_screen(egui::vec2(spec.cx + h, yy), self.view_c, self.scale));
                        pl[2].push(to_screen(egui::vec2(spec.cx - o, yy), self.view_c, self.scale));
                        pl[3].push(to_screen(egui::vec2(spec.cx + o, yy), self.view_c, self.scale));
                        y += 2;
                    }
                    for (i, pts) in pl.into_iter().enumerate() {
                        painter.add(egui::Shape::line(pts, if i < 2 { stroke } else { outer }));
                    }
                    // the two clicked throat edges and the exit plane
                    for s in [-1.0f32, 1.0] {
                        let t = to_screen(egui::vec2(spec.cx + s * spec.throat_hw, spec.y_throat as f32), self.view_c, self.scale);
                        painter.circle_filled(t, 3.5, egui::Color32::from_rgb(120, 220, 255));
                    }
                    let (e0, e1) = (
                        to_screen(egui::vec2(spec.cx - spec.exit_hw(), spec.y_exit as f32 + 1.0), self.view_c, self.scale),
                        to_screen(egui::vec2(spec.cx + spec.exit_hw(), spec.y_exit as f32 + 1.0), self.view_c, self.scale),
                    );
                    painter.line_segment([e0, e1], egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(120, 220, 255, 120)));
                }
            } else {
                self.noz_a = None;
                self.noz_place = None;
            }

            // painting
            let (prim, sec, pressed) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down(), i.pointer.any_pressed()));
            let mods = ui.input(|i| i.modifiers);
            let mut stroking = false;
            let busy = self.select.is_some() || self.pasting; // copying, cutting, stamping or pasting: no painting
            let paint_tool = !busy && self.tool != TOOL_VALVE && self.tool != TOOL_NOZZLE && self.tool != TOOL_INSPECT && self.tool != TOOL_FILL && self.tool != TOOL_GAUGE && self.tool != TOOL_REG;
            let erase = !busy && sec && self.tool != TOOL_GAUGE; // right-click with the gauge tool removes dials instead
            let cell_box = |x0: f32, y0: f32, x1: f32, y1: f32, view_c: egui::Vec2, scale: f32| {
                egui::Rect::from_two_pos(to_screen(egui::vec2(x0, y0), view_c, scale), to_screen(egui::vec2(x1, y1), view_c, scale))
            };
            // copy / cut / stamp: drag a box
            if let Some(how) = self.select {
                let verb = match how {
                    SelectFor::Copy => "copy",
                    SelectFor::Cut => "cut",
                    SelectFor::Stamp => "stamp",
                };
                if let Some(p) = hover {
                    let cc = to_cell(p, self.view_c, self.scale);
                    let (cx, cy) = (cc.x.floor() as i32, cc.y.floor() as i32);
                    if pressed && prim && !space && self.sel_drag.is_none() {
                        self.sel_drag = Some((cx, cy, cx, cy));
                    }
                    if let Some(d) = &mut self.sel_drag {
                        if prim {
                            (d.2, d.3) = (cx, cy);
                        }
                    }
                }
                match self.sel_drag {
                    Some(d @ (x0, y0, x1, y1)) => {
                        let b = cell_box(x0.min(x1) as f32, y0.min(y1) as f32, x0.max(x1) as f32 + 1.0, y0.max(y1) as f32 + 1.0, self.view_c, self.scale);
                        painter.rect_filled(b, 0.0, egui::Color32::from_rgba_unmultiplied(120, 200, 255, 25));
                        painter.rect_stroke(b, 0.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Outside);
                        status = format!("{verb} {} x {} cells", (x1 - x0).abs() + 1, (y1 - y0).abs() + 1);
                        if !prim {
                            self.finish_select(ui.ctx(), how, d);
                        }
                    }
                    None => status = format!("drag a box around what to {verb} (right-click or Esc: cancel)"),
                }
                if resp.secondary_clicked() {
                    self.select = None;
                    self.sel_drag = None;
                }
            }
            // paste: the clipboard follows the mouse, centred on it
            if self.pasting {
                if let (Some(cl), Some(tex), Some(p)) = (&self.clip, &self.clip_tex, hover) {
                    let cc = to_cell(p, self.view_c, self.scale);
                    let (x0, y0) = (cc.x.floor() as i32 - cl.w as i32 / 2, cc.y.floor() as i32 - cl.h as i32 / 2);
                    let b = cell_box(x0 as f32, y0 as f32, (x0 + cl.w as i32) as f32, (y0 + cl.h as i32) as f32, self.view_c, self.scale);
                    painter.image(tex.id(), b, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::from_white_alpha(190));
                    painter.rect_stroke(b, 0.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 230, 120)), egui::StrokeKind::Outside);
                    status = format!("paste {} x {}: click to put it down (Shift: keep pasting). R rotate, Shift+R mirror, Ctrl+Shift+R upside down. Right-click or Esc: cancel", cl.w, cl.h);
                    if resp.clicked() && !space {
                        let cl = cl.clone();
                        if self.world.idle() {
                            self.forget_snapshot();
                        }
                        self.world.paste(&cl, x0, y0);
                        self.pasting = mods.shift;
                    }
                }
                if resp.secondary_clicked() {
                    self.pasting = false;
                }
            }
            if let Some(p) = hover {
                let cc = to_cell(p, self.view_c, self.scale);
                let (cx, cy) = (cc.x.floor() as i32, cc.y.floor() as i32);
                (self.hover_info, self.hover_danger) = self.hover_text(cx, cy);
                let ctrl_shift = mods.shift && (mods.ctrl || mods.command);
                if !prim && !sec {
                    self.flood_held = false;
                }
                if ctrl_shift && self.shape.is_none() && paint_tool {
                    status = "Ctrl+Shift+click: flood fill (right-click: flood erase)".into();
                }
                if self.shape.is_none() && pressed && !space && ctrl_shift && ((prim && paint_tool) || erase) {
                    // flood fill: the joined area of whatever was clicked, once per press
                    let v = if erase { c::EMPTY } else { self.tool };
                    let n = self.flood_fill(cx, cy, v);
                    self.flash = Some((if n > 0 { format!("flood fill: {n} cells") } else { "flood fill: nothing to fill there".into() }, Instant::now()));
                    self.flood_held = true;
                    self.last_cell = None;
                } else if self.shape.is_none() && pressed && !space && (mods.shift || mods.ctrl || mods.command) && ((prim && paint_tool) || erase) {
                    self.shape = Some(ShapeDraw { sx: cx, sy: cy, ex: cx, ey: cy, rect: mods.ctrl || mods.command, erase: sec });
                }
                if self.flood_held {
                    // nothing more until the button comes up
                } else if let Some(sh) = &mut self.shape {
                    sh.ex = cx;
                    sh.ey = cy;
                    self.last_cell = None;
                } else if (prim && !space && paint_tool) || erase {
                    stroking = true;
                    let v = if erase { c::EMPTY } else { self.tool };
                    let (x0, y0) = self.last_cell.unwrap_or((cx, cy));
                    let n = (cx - x0).abs().max((cy - y0).abs()).max(1);
                    for k in 0..=n {
                        self.paint_box(x0 + (cx - x0) * k / n, y0 + (cy - y0) * k / n, v);
                    }
                    self.last_cell = Some((cx, cy));
                } else {
                    self.last_cell = None;
                }
                if self.tool == TOOL_GAUGE && cx >= 0 && cy >= 0 && cx < W as i32 && cy < H as i32 {
                    if resp.clicked() {
                        match self.gauge_start.take() {
                            None => self.gauge_start = Some(cc),
                            Some(at) => self.gauges.push(PressureGauge { at, probe: World::idx(cx as usize, cy as usize), range_bar: 1.0 }),
                        }
                    }
                    if resp.secondary_clicked() {
                        let near = self.gauges.iter().position(|g| (to_screen(g.at, self.view_c, self.scale) - p).length() < GAUGE_R);
                        match near {
                            Some(k) => {
                                self.gauges.remove(k);
                            }
                            None => self.gauge_start = None,
                        }
                    }
                    if let Some(at) = self.gauge_start {
                        // preview: where the dial goes and the line to the point it will measure
                        let a = to_screen(at, self.view_c, self.scale);
                        painter.line_segment([a, p], egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
                        painter.circle_stroke(a, GAUGE_R, egui::Stroke::new(1.5, egui::Color32::from_white_alpha(140)));
                        status = "click where the gauge should measure".into();
                    }
                }
                if self.tool == TOOL_REG && resp.clicked() {
                    // the regulator under the click, or the nearest one within a few cells (they're small)
                    let mut best: Option<(i32, usize)> = None;
                    for dy in -3..=3 {
                        for dx in -3..=3 {
                            let (x, y) = (cx + dx, cy + dy);
                            if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
                                let i = World::idx(x as usize, y as usize);
                                let d = dx * dx + dy * dy;
                                if self.world.cells[i] == c::REG && best.map_or(true, |b| d < b.0) {
                                    best = Some((d, i));
                                }
                            }
                        }
                    }
                    match best {
                        Some((_, i)) => self.open_reg_prompt(i),
                        None => self.msg = "no pressure regulator there".into(),
                    }
                }
                if self.tool == TOOL_REG {
                    status = "click a pressure regulator to set its pressure".into();
                }
                if self.tool == TOOL_FILL && resp.clicked() && cx >= 0 && cy >= 0 && cx < W as i32 && cy < H as i32 {
                    let i = World::idx(cx as usize, cy as usize);
                    match self.world.cells[i] {
                        c::HE_TANK => self.bottle_prompt = Some((i, self.world.st.he_fill / world::PA_PER_PSI)),
                        c::REG => self.open_reg_prompt(i),
                        c::TANK_F | c::TANK_O => self.tank_prompt = Some(i),
                        c::BATTERY | c::MOTOR | c::WIRE => {
                            if self.world.nets_dirty {
                                self.world.rebuild_nets();
                            }
                            let ok = self.world.recharge_at(i);
                            self.flash = Some((if ok { "battery recharged".to_string() } else { "no battery wired to that".to_string() }, Instant::now()));
                        }
                        _ => {}
                    }
                }
                if self.tool == TOOL_INSPECT && resp.clicked() && cx >= 0 && cy >= 0 && cx < W as i32 && cy < H as i32 {
                    let k = self.world.shaft_of[World::idx(cx as usize, cy as usize)];
                    if k != NO_NET {
                        self.selected_shaft = Some(k as usize);
                    }
                }
                if self.tool == TOOL_VALVE && resp.clicked() {
                    // toggle each valve group near the click exactly once
                    if self.world.nets_dirty {
                        self.world.rebuild_nets();
                    }
                    let mut groups = vec![];
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (x, y) = (cx + dx, cy + dy);
                            if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
                                let g = self.world.valve_of[World::idx(x as usize, y as usize)];
                                if g != NO_NET && !groups.contains(&g) {
                                    groups.push(g);
                                }
                            }
                        }
                    }
                    for g in groups {
                        let open = !self.world.valves[g as usize].cmd;
                        if open {
                            self.before_firing();
                        }
                        self.world.command_valve(g as usize, open);
                    }
                }
                if paint_tool || erase {
                    // box cursor showing exactly which cells the brush covers
                    let r = self.brush as f32;
                    let b = cell_box(cx as f32 - r, cy as f32 - r, cx as f32 + r + 1.0, cy as f32 + r + 1.0, self.view_c, self.scale);
                    painter.rect_stroke(b, 0.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
                    if self.shape.is_none() {
                        status = format!("brush {}x{}   {}", 2 * self.brush + 1, 2 * self.brush + 1, status);
                    }
                }
            } else {
                self.last_cell = None;
            }
            self.stroking = stroking;
            // Shift-line / Ctrl-rectangle: preview while held, draw on release
            if let Some(sh) = self.shape {
                let r = self.brush as f32;
                let col = if sh.erase { egui::Color32::from_rgb(255, 90, 90) } else { egui::Color32::from_rgb(255, 230, 120) };
                if prim || sec {
                    if sh.rect {
                        let b = cell_box(sh.sx.min(sh.ex) as f32, sh.sy.min(sh.ey) as f32, sh.sx.max(sh.ex) as f32 + 1.0, sh.sy.max(sh.ey) as f32 + 1.0, self.view_c, self.scale);
                        painter.rect_stroke(b, 0.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Outside);
                        status = format!("rectangle {} x {} cells", (sh.ex - sh.sx).abs() + 1, (sh.ey - sh.sy).abs() + 1);
                    } else {
                        let a = to_screen(egui::vec2(sh.sx as f32 + 0.5, sh.sy as f32 + 0.5), self.view_c, self.scale);
                        let e = to_screen(egui::vec2(sh.ex as f32 + 0.5, sh.ey as f32 + 0.5), self.view_c, self.scale);
                        painter.line_segment([a, e], egui::Stroke::new((2.0 * r + 1.0) * self.scale, col.gamma_multiply(0.35)));
                        painter.line_segment([a, e], egui::Stroke::new(1.0, col));
                        for (x, y) in [(sh.sx, sh.sy), (sh.ex, sh.ey)] {
                            let b = cell_box(x as f32 - r, y as f32 - r, x as f32 + r + 1.0, y as f32 + r + 1.0, self.view_c, self.scale);
                            painter.rect_stroke(b, 0.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Outside);
                        }
                        status = format!("line {} cells long", (sh.ex - sh.sx).abs().max((sh.ey - sh.sy).abs()) + 1);
                    }
                } else {
                    let v = if sh.erase { c::EMPTY } else { self.tool };
                    if sh.rect {
                        for y in sh.sy.min(sh.ey).max(0)..=sh.sy.max(sh.ey).min(H as i32 - 1) {
                            for x in sh.sx.min(sh.ex).max(0)..=sh.sx.max(sh.ex).min(W as i32 - 1) {
                                self.apply(World::idx(x as usize, y as usize), v);
                            }
                        }
                    } else {
                        let n = (sh.ex - sh.sx).abs().max((sh.ey - sh.sy).abs()).max(1);
                        for k in 0..=n {
                            self.paint_box(sh.sx + (sh.ex - sh.sx) * k / n, sh.sy + (sh.ey - sh.sy) * k / n, v);
                        }
                    }
                    self.shape = None;
                }
            }
            self.status = status;
            // each tank and valve says what it carries (when it is big enough on screen to read)
            if self.labels_age % 30 == 0 || self.world.nets_dirty {
                let mut l = vec![];
                for n in self.world.nets.iter().filter(|n| n.prop <= 1 && n.tank_cells > 0) {
                    let ni = self.world.net_of[n.tank_top] as usize;
                    let cells = self.world.flood_region(0, n.tank_top);
                    let (mut x0, mut x1, mut y0, mut y1) = (W, 0, H, 0);
                    for &i in &cells {
                        x0 = x0.min(i % W);
                        x1 = x1.max(i % W);
                        y0 = y0.min(i / W);
                        y1 = y1.max(i / W);
                    }
                    if ni < self.world.nets.len() {
                        l.push(((x0 + x1) as f32 * 0.5 + 0.5, (y0 + y1) as f32 * 0.5 + 0.5, (x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32, self.world.fluid_name(ni, false).to_string(), true));
                    }
                }
                for v in &self.world.valves {
                    if let Some(n) = self.world.net_around(v.cells[0]) {
                        let (mut x0, mut x1, mut y0, mut y1) = (W, 0, H, 0);
                        for &i in &v.cells {
                            x0 = x0.min(i % W);
                            x1 = x1.max(i % W);
                            y0 = y0.min(i / W);
                            y1 = y1.max(i / W);
                        }
                        l.push(((x0 + x1) as f32 * 0.5 + 0.5, y0 as f32 - 1.0, (x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32, self.world.fluid_name(n, false).to_string(), false));
                    }
                }
                self.labels = l;
            }
            self.labels_age = self.labels_age.wrapping_add(1);
            for (lx, ly, lw, lh, text, tank) in &self.labels {
                let at = to_screen(egui::vec2(*lx, *ly), self.view_c, self.scale);
                if !rect.expand(40.0).contains(at) {
                    continue;
                }
                let font = if *tank {
                    let w_px = lw * self.scale;
                    if w_px < 26.0 || lh * self.scale < 12.0 {
                        continue;
                    }
                    (w_px / (text.len() as f32 * 0.9)).clamp(9.0, 22.0)
                } else {
                    if self.scale < 4.0 {
                        continue;
                    }
                    9.0
                };
                let id = egui::FontId::proportional(font);
                painter.text(at + egui::vec2(1.0, 1.0), egui::Align2::CENTER_CENTER, text, id.clone(), egui::Color32::from_black_alpha(190));
                painter.text(at, egui::Align2::CENTER_CENTER, text, id, egui::Color32::from_white_alpha(if *tank { 230 } else { 200 }));
            }
            // what's under the mouse, in the top-left corner of the field, and how near it is to melting
            if !self.hover_info.is_empty() && hover.is_some() {
                let col = if self.hover_danger > 0.85 {
                    egui::Color32::from_rgb(255, 110, 90)
                } else if self.hover_danger > 0.6 {
                    egui::Color32::from_rgb(255, 190, 90)
                } else {
                    egui::Color32::from_gray(225)
                };
                let galley = painter.layout(self.hover_info.clone(), egui::FontId::monospace(12.0), col, 520.0);
                let pos = rect.left_top() + egui::vec2(10.0, 8.0);
                painter.rect_filled(egui::Rect::from_min_size(pos - egui::vec2(6.0, 4.0), galley.size() + egui::vec2(12.0, 8.0)), 4.0, egui::Color32::from_black_alpha(175));
                painter.galley(pos, galley, col);
            }

            // the gauges themselves: dial, needle, reading, and a line to what they measure
            for k in 0..self.gauges.len() {
                let (p_pa, what) = self.probe_pressure(self.gauges[k].probe);
                let g = &mut self.gauges[k];
                let bar = p_pa.map_or(0.0, |v| v / 1e5);
                // dial range steps up (1, 2, 5, 10, 20, 50 ...) when the reading passes 90 %
                while bar > 0.9 * g.range_bar && g.range_bar < 1.0e5 {
                    let r = g.range_bar;
                    let m = r / 10f32.powf(r.log10().floor());
                    g.range_bar = if m < 1.5 { r * 2.0 } else if m < 3.0 { r * 2.5 } else { r * 2.0 };
                }
                let c0 = to_screen(g.at, self.view_c, self.scale);
                let pc = to_screen(egui::vec2((g.probe % W) as f32 + 0.5, (g.probe / W) as f32 + 0.5), self.view_c, self.scale);
                painter.line_segment([c0, pc], egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 200, 120)));
                painter.circle_filled(pc, 3.0, egui::Color32::from_rgb(230, 230, 120));
                painter.circle(c0, GAUGE_R, egui::Color32::from_rgb(235, 235, 228), egui::Stroke::new(3.0, egui::Color32::from_rgb(70, 72, 80)));
                // 270 degree scale, ten ticks
                let ang = |f: f32| std::f32::consts::PI * (0.75 + 1.5 * f);
                for t in 0..=10 {
                    let a = ang(t as f32 / 10.0);
                    let d = egui::vec2(a.cos(), a.sin());
                    let inner = if t % 5 == 0 { GAUGE_R - 10.0 } else { GAUGE_R - 6.0 };
                    painter.line_segment([c0 + d * inner, c0 + d * (GAUGE_R - 2.0)], egui::Stroke::new(1.2, egui::Color32::from_gray(40)));
                }
                // red zone over 80 %
                let red: Vec<egui::Pos2> = (0..=12).map(|s| ang(0.8 + 0.2 * s as f32 / 12.0)).map(|a| c0 + egui::vec2(a.cos(), a.sin()) * (GAUGE_R - 4.0)).collect();
                painter.add(egui::Shape::line(red, egui::Stroke::new(3.0, egui::Color32::from_rgb(200, 60, 50))));
                let f = (bar / g.range_bar).clamp(0.0, 1.0);
                let a = ang(f);
                painter.line_segment([c0, c0 + egui::vec2(a.cos(), a.sin()) * (GAUGE_R - 8.0)], egui::Stroke::new(2.5, egui::Color32::from_rgb(200, 30, 30)));
                painter.circle_filled(c0, 3.5, egui::Color32::from_gray(30));
                let font = egui::FontId::proportional(10.0);
                let reading = match p_pa {
                    Some(v) => format!("{:.1} bar\n{:.0} psi", v / 1e5, v / world::PA_PER_PSI),
                    None => "--".to_string(),
                };
                painter.text(c0 + egui::vec2(0.0, 13.0), egui::Align2::CENTER_CENTER, reading, font.clone(), egui::Color32::from_gray(20));
                painter.text(c0 + egui::vec2(0.0, -12.0), egui::Align2::CENTER_CENTER, format!("0-{} bar", g.range_bar), egui::FontId::proportional(8.0), egui::Color32::from_gray(90));
                painter.text(c0 + egui::vec2(0.0, GAUGE_R + 8.0), egui::Align2::CENTER_CENTER, what, font, egui::Color32::from_gray(200));
            }

            // a helium bottle was just drawn: once the mouse is released, ask what to fill it to
            if !prim && !sec && self.shape.is_none() {
                if let Some(cell) = self.bottle_stroke.take() {
                    self.bottle_prompt = Some((cell, self.world.st.he_fill / world::PA_PER_PSI));
                }
            }

            // build the frame for the GPU
            let mat_full = if self.reset || self.world.mat_stale || !self.world.dirty.is_empty() { Some(self.all_mat()) } else { None };
            let send_layers = mat_full.is_some();
            let dirty: Vec<(usize, bool)> = self.world.dirty.drain(..).map(|i| (i, self.world.cells[i] == c::EMPTY)).collect();
            let st = self.world.st;
            let pr_blend = self.world.blended_prop();
            let pr = &pr_blend;
            let params = self.world.sim_params(); // gas constants of the burn, and the mixture's effect on wall heating
            let mut input = FrameInput {
                reset: if self.reset { mat_full.clone() } else { None },
                mat: if self.reset { None } else { mat_full },
                dirty,
                params,
                net_t: self.world.net_temps(),
                srcs: self.world.srcs.clone(),
                steps: self.steps_now,
                particles: st.show_particles,
                phase: if self.reset || self.world.phase_dirty { Some(self.world.phase.clone()) } else { None },
                layers: if send_layers { Some(self.world.gpu_layers()) } else { None },
                anim: self.world.anim(),
                paint: paint_states(&mut self.world, &self.rb),
                sim,
                render: RenderParams {
                    w: W as u32,
                    h: H as u32,
                    mode: self.mode,
                    show_heat: self.show_heat as u32,
                    cx: self.view_c.x,
                    cy: self.view_c.y,
                    scale: self.scale * ppp,
                    gamma: pr.gamma,
                    rw: rect.width() * ppp,
                    rh: rect.height() * ppp,
                    rgas: pr.rgas,
                    particles: st.show_particles as u32,
                    time: self.t0.elapsed().as_secs_f32() % 1000.0,
                    tint: pr.tint,
                    soot: pr.soot,
                    liq_f: pr.liq_f, liq_o: pr.liq_o, edit_layer: (self.layer == 1) as u32, amb_rho: st.amb_p / (pr.rgas * 300.0), _q: [0.0; 3],
                },
            };
            self.world.mat_stale = false;
            self.world.phase_dirty = false;
            if self.reset {
                self.reset = false;
                self.rb = Readback::default();
            }
            {
                let mut slot = self.gpu_in.lock().unwrap();
                if let Some(old) = slot.take() {
                    input.absorb(old, || self.all_mat());
                }
                *slot = Some(input);
            }
            painter.set(field_slot, egui_wgpu::Callback::new_paint_callback(rect, FieldCallback { input: self.gpu_in.clone() }));

        });
        // redraw at the frame-rate cap (slowly while minimised) instead of as fast as the display allows
        // counted from the start of this frame, so the frame's own work doesn't stretch the period
        let period = if self.minimized { 0.25 } else { 1.0 / self.world.st.fps_cap.max(1.0) };
        let left = (period - self.last.elapsed().as_secs_f32()).max(0.0);
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f32(left));
    }
}

/// GPU states for the gas painted this frame: 1 bar inert gas, 1 bar fuel vapour, or 2600 K fire.
fn paint_states(w: &mut World, rb: &Readback) -> Vec<(usize, [f32; 4], f32)> {
    let pr = w.st.p();
    w.gas_paint
        .drain(..)
        .map(|(i, kind)| {
            let t = if kind == 2 { 2600.0 } else { 300.0 };
            // fire takes over the gas that's there at its pressure (so it lights a full, pressurised
            // chamber instead of being crushed); kind 3 = erased: back to the ambient gas
            let local_p = if rb.valid && rb.state[i][0] > 0.0 { gas_p(rb.state[i], pr.gamma) } else { 0.0 };
            let p = match kind {
                3 => w.st.amb_p,
                2 => local_p.max(1.0e5),
                _ => 1.0e5,
            };
            let rho = p / (pr.rgas * t);
            let e = rho * pr.rgas / (pr.gamma - 1.0) * t;
            (i, [rho, 0.0, 0.0, e], if kind == 1 { rho } else { 0.0 })
        })
        .collect()
}

/// Animation clock for `shoot` (f32 bits); `SHOT_TIME` in the environment overrides it. The GIF capture steps it per frame.
static SHOT_TIME: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

fn shoot(device: &wgpu::Device, queue: &wgpu::Queue, gpu: &mut GpuSim, w: &mut World, mode: u32, path: &str, view: (f32, f32, f32)) {
    let (tw, th) = (1024u32, 1000u32);
    let mat: Vec<u32> = (0..world::N).map(|i| w.gpu_mat(i)).collect();
    let st = w.st;
    let pr = st.p();
    let inp = FrameInput {
        reset: None,
        mat: Some(mat),
        dirty: vec![],
        params: SimParams::from_settings(&st),
        net_t: w.net_temps(),
        srcs: vec![],
        steps: 0,
        particles: true,
        phase: Some(w.phase.clone()),
        layers: Some(w.gpu_layers()),
        anim: w.anim(),
        paint: vec![],
        sim: true,
        render: RenderParams {
            w: W as u32,
            h: H as u32,
            mode,
            show_heat: 1,
            cx: view.0,
            cy: view.1,
            scale: view.2,
            gamma: pr.gamma,
            rw: tw as f32,
            rh: th as f32,
            rgas: pr.rgas,
            particles: (mode != 6) as u32,
            time: std::env::var("SHOT_TIME").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| f32::from_bits(SHOT_TIME.load(Ordering::Relaxed))),
            tint: pr.tint,
            soot: pr.soot,
            liq_f: pr.liq_f, liq_o: pr.liq_o, edit_layer: 0, amb_rho: w.st.amb_p / (pr.rgas * 300.0), _q: [0.0; 3],
        },
    };
    gpu.frame(device, queue, inp);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (tw * th * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        gpu.paint(&mut pass);
    }
    if let Some(reps) = std::env::var("ROCKET_RENDER_BENCH").ok().and_then(|v| v.parse::<u32>().ok()) {
        // time the field render on its own: the same pass, many times
        queue.submit(Some(enc.finish()));
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let t0 = Instant::now();
        let mut e2 = device.create_command_encoder(&Default::default());
        for _ in 0..reps {
            let mut pass = e2
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            gpu.paint(&mut pass);
        }
        queue.submit(Some(e2.finish()));
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        println!("render mode {mode}: {:.2} ms per frame at {tw}x{th}", t0.elapsed().as_secs_f32() * 1000.0 / reps as f32);
        enc = device.create_command_encoder(&Default::default());
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: Default::default(), aspect: Default::default() },
        wgpu::TexelCopyBufferInfo { buffer: &out, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tw * 4), rows_per_image: Some(th) } },
        wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
    );
    queue.submit(Some(enc.finish()));
    out.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    let data = out.slice(..).get_mapped_range().expect("range").to_vec();
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), tw, th);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&data).unwrap();
    println!("wrote {path}");
}

/// Headless run: `--selftest SECS [PNG_PREFIX] [--prop N] [--axi] [--pf] [--pc BAR]`
fn selftest(seconds: f32, shot: Option<String>, args: &[String]) {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
    println!("adapter: {:?}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
    let mut gpu = GpuSim::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let mut w = World::new();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|p| args.get(p + 1)).and_then(|v| v.parse::<f32>().ok());
    let mut retune = false;
    let (ox, oy) = (presets::PRESET_OX, presets::PRESET_OY);
    if args.iter().any(|a| a == "--atm") {
        w.st.amb_p = 101325.0;
    }
    if let Some(n) = arg("--steps") {
        w.st.steps_per_frame = n as u32;
    }
    if args.iter().any(|a| a == "--idle") {
        w.clear(); // nothing at all: still air everywhere
    }
    if let Some(k) = arg("--plumestyle") {
        w.fx_style = k as u32;
    }
    if args.iter().any(|a| a == "--nostab") {
        w.stable_jet = false;
    }
    if args.iter().any(|a| a == "--fullsim") {
        w.fx_plume = false; // the gas simulation as it is, no stylised plume
    }
    if args.iter().any(|a| a == "--vac") {
        w.st.amb_p = 100.0;
    }
    if args.iter().any(|a| a == "--gg") {
        w.preset_gg();
    }
    if let Some(path) = args.iter().position(|a| a == "--design").and_then(|p| args.get(p + 1)) {
        let t = std::fs::read_to_string(path).expect("read design");
        w.from_text(&t).expect("load design");
        if let Some(f) = arg("--injf") {
            w.st.inj_area_f = f;
        }
        if let Some(o) = arg("--injo") {
            w.st.inj_area_o = o;
        }
        println!("loaded {path}: injectors fuel {:.2} ox {:.2} cm2/cell", w.st.inj_area_f, w.st.inj_area_o);
    }
    if let Some(k) = args.iter().position(|a| a == "--ph") {
        // a whole engine around a generated powerhead: --ph PROP PC_BAR LAYOUT(0 auto, 1 one shaft, 2 two) [THROAT]
        let num = |o: usize, d: f32| args.get(k + o).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let layout = match num(3, 0.0) as i32 { 1 => powerhead::Layout::Single, 2 => powerhead::Layout::Twin, _ => powerhead::Layout::Auto };
        if let Some(v) = arg("--dx") {
            w.st.dx = v;
        }
        if let Some(v) = arg("--depth") {
            w.st.depth = v;
        }
        let mut inp = powerhead::PhInput::from_settings(&w.st, num(2, 60.0) * 1e5);
        inp.prop = num(1, 0.0) as usize;
        inp.layout = layout;
        inp.throat_cells = num(4, 14.0);
        let s = powerhead::demo_engine(&mut w, &inp);
        // --phpng FILE: the generated part on its own, as the maker's window shows it, 6 pixels a cell
        if let Some(path) = args.iter().position(|a| a == "--phpng").and_then(|p| args.get(p + 1)) {
            let img = clip_image(&powerhead::build(&s).0);
            let (iw, ih, z) = (img.size[0], img.size[1], 6usize);
            let mut data = vec![22u8; iw * z * ih * z * 3];
            for y in 0..ih * z {
                for x in 0..iw * z {
                    let px = img.pixels[(y / z) * iw + x / z];
                    let a = px.a() as u32;
                    for (k, v) in [px.r(), px.g(), px.b()].into_iter().enumerate() {
                        data[(y * iw * z + x) * 3 + k] = ((v as u32 * a + 22 * (255 - a)) / 255) as u8;
                    }
                }
            }
            let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).unwrap()), (iw * z) as u32, (ih * z) as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&data).unwrap();
            println!("wrote {path} ({iw} x {ih} cells)");
        }
        for (k, sh) in s.shafts.iter().enumerate() {
            let pumps: Vec<String> = sh.pumps.iter().map(|q| format!("{} {}cm x{} tip {:.0} head {:.0}bar flow x{:.2} eff {:.0}% {:.2}MW", if q.prop == 0 { "F" } else { "O" }, q.cells, q.stages, q.tip, q.head / 1e5, q.flow_ratio, q.eff * 100.0, q.power / 1e6)).collect();
            println!("ph shaft {k}: {:.0} rpm ({:.0}% rated) turbine {}x{} cells x{} nozzle {} cells {:.1} -> {:.1} bar u/c0 {:.2} eff {:.0}% {:.2}MW  {:?}", sh.omega * 60.0 / (2.0 * std::f32::consts::PI), sh.speed_frac * 100.0, sh.turb_cells, sh.turb_len, sh.turb_stages, sh.nozzle, sh.p_in / 1e5, sh.p_out / 1e5, sh.u_c0, sh.eff_t * 100.0, sh.power / 1e6, pumps);
        }
        println!("ph: {} at {:.0} bar: {:.1}+{:.1} kg/s, discharge {:.0}/{:.0} bar, gg O/F {:.2} {:.1} kg/s areas {:.2}/{:.2} cm2 p {:.0} bar, governor {:.0} bar, cartridge {:.1} MW, inj {:.2}/{:.2}  {:?}", PROPS[inp.prop].name, inp.pc / 1e5, s.mf, s.mo, s.p_dis_f / 1e5, s.p_dis_o / 1e5, s.gg_of, s.gg_mdot, s.gg_area_f, s.gg_area_o, s.gg_p / 1e5, s.feed_p_max / 1e5, s.cartridge_w / 1e6, w.st.inj_area_f, w.st.inj_area_o, s.notes);
    }
    if let Some(k) = arg("--preset") {
        w.load_preset(k as usize);
        if let (Some(f), Some(o)) = (arg("--injf"), arg("--injo")) {
            w.st.inj_area_f = f;
            w.st.inj_area_o = o;
        }
        if let (Some(f), Some(o)) = (arg("--ggf"), arg("--ggo")) {
            w.st.gg_area_f = f;
            w.st.gg_area_o = o;
        }
        if let Some(v) = arg("--injf2") {
            w.st.inj_area_f2 = v;
        }
        println!("preset {}: injectors fuel {:.2} ox {:.2} cm2/cell, gas generator {:.2} / {:.2} cm2, dx {} depth {}", k, w.st.inj_area_f, w.st.inj_area_o, w.st.gg_area_f, w.st.gg_area_o, w.st.dx, w.st.depth);
        if args.iter().any(|a| a == "--nbskirt") {
            // bolt a radiation-cooled niobium skirt onto the preset's nozzle exit
            let e = &presets::presets()[(k as usize).min(3)];
            let spec = design::NozzleSpec {
                cx: 128.0 + ox as f32,
                y_throat: e.exit_y as i32 + 1 + oy as i32,
                y_exit: e.exit_y as i32 + 1 + oy as i32 + arg("--nblen").unwrap_or(90.0) as i32,
                throat_hw: e.exit_hw,
                expansion: arg("--nbexp").unwrap_or(1.8),
                contour: design::Contour::Bell,
                liner: 1,
                jacket: 0,
                shell: 0,
                axi: false,
                liner_mat: c::NIOBIUM,
            };
            w.build_nozzle(&spec);
            w.dirty.clear();
            println!("niobium skirt: {} cells", w.cells.iter().filter(|&&t| t == c::NIOBIUM).count());
        }
    }
    if args.iter().any(|a| a == "--nojacket") {
        // lower bell without coolant passages: the jacket cells become plain steel
        for y in (200 + oy)..=(330 + oy) {
            for x in 0..W {
                let i = World::idx(x, y);
                if w.cells[i] == c::PIPE {
                    w.cells[i] = c::STEEL;
                }
            }
        }
        println!("lower bell jacket (y>=200) removed");
    }
    if let Some(p) = arg("--prop") {
        w.st.prop = p as usize;
        retune = true;
    }
    if args.iter().any(|a| a == "--axi") {
        w.st.axi = true;
        retune = true;
    }
    if retune {
        w.auto_size_injectors(arg("--pc").unwrap_or(100.0) * 1e5);
        println!("auto-sized: fuel {:.2} cm2, ox {:.2} cm2, tanks {:.0} / {:.0} bar", w.st.inj_area_f, w.st.inj_area_o, w.st.tank_p_f / 1e5, w.st.tank_p_o / 1e5);
    }
    if args.iter().any(|a| a == "--boiltank") {
        // a half-full LOX tank boiling off, plus a fuel-vapour source beside a fire source
        w.clear();
        w.st.prop = 0;
        w.rect(250, 200, 330, 290, c::TANK_O);
        w.auto_wall_all();
        w.rect(360, 240, 366, 246, c::SRC_FUEL);
        w.rect(375, 243, 376, 244, c::SRC_FIRE);
        w.rebuild_nets();
        for n in &mut w.nets {
            n.mass_frac = 0.55;
        }
    }
    if args.iter().any(|a| a == "--coldgas") {
        // a lone helium bottle (built before the first frame so the initial upload includes it)
        w.clear();
        w.rect(300, 200, 340, 260, c::HE_TANK);
        w.auto_wall_all();
        w.nets_dirty = true;
    }
    let mut rb = Readback::default();
    let t0 = Instant::now();
    // --electric: make it electric-pump fed (as on Rutherford): turbines become motors, gas generators go,
    // and batteries fill the open space next to the motors (out to 12 cells)
    if args.iter().any(|a| a == "--electric") {
        let mut q = std::collections::VecDeque::new();
        let mut dist = vec![u32::MAX; W * H];
        for i in 0..W * H {
            match w.cells[i] {
                c::TURBINE => {
                    w.cells[i] = c::MOTOR;
                    dist[i] = 0;
                    q.push_back(i);
                }
                c::GG => w.cells[i] = c::STEEL,
                _ => {}
            }
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % W, i / W);
            for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                if nx < W && ny < H {
                    let j = World::idx(nx, ny);
                    if dist[j] == u32::MAX && w.cells[j] == c::EMPTY && dist[i] < 12 {
                        dist[j] = dist[i] + 1;
                        w.cells[j] = c::BATTERY;
                        q.push_back(j);
                    }
                }
            }
        }
        w.nets_dirty = true;
        w.rebuild_nets();
        let cap: f32 = w.circuits.iter().map(|cc| cc.cap).sum();
        let pmax: f32 = w.circuits.iter().map(|cc| cc.p_max).sum();
        println!("electric: {} motor(s), batteries {:.1} MJ, {:.0} kW", w.shafts.iter().filter(|s| s.motor_cells > 0).count(), cap / 1e6, pmax / 1e3);
    }
    // overrides after the engine is set up: start cartridge power (W), cooling jacket heat scale
    if args.iter().any(|a| a == "--noheline") {
        // take the helium line's injector out again and use the lumped helium start instead
        w.rebuild_nets();
        for g in 0..w.valves.len() {
            if w.valves[g].prop == world::PROP_HE {
                for k in 0..w.valves[g].cells.len() {
                    let i = w.valves[g].cells[k];
                    for j in [i + 1, i - 1, i + W, i - W] {
                        if w.cells[j] == c::INJ {
                            w.set_cell(j, c::STEEL);
                        }
                    }
                }
            }
        }
        w.st.he_spin = true;
    }
    if args.iter().any(|a| a == "--hespin") {
        w.st.he_spin = true;
    }
    if args.iter().any(|a| a == "--deadstart") {
        w.st.dead_start = true;
    }
    if let Some(v) = arg("--cart") {
        w.st.cartridge_w = v;
    }
    if let Some(v) = arg("--heat") {
        w.st.turb_mult = v;
    }
    // --alt KM: fly at this altitude (standard atmosphere); --climb KM_PER_S: climb from there
    if let Some(h) = arg("--alt") {
        w.alt_on = true;
        w.altitude = h;
        w.st.amb_p = world::atmosphere_p(h).max(world::MIN_AMBIENT_P);
        w.amb_prev = w.st.amb_p;
    }
    if let Some(r) = arg("--climb") {
        w.climbing = true;
        w.climb_rate = r;
    }
    // --flyalt KM [--flyvel M/S] [--dry KG]: the vehicle flies from here (up is positive); --burnat SECS delays the start
    if let Some(h) = arg("--flyalt") {
        w.flight = true;
        w.alt_on = true;
        w.altitude = h;
        w.vel = arg("--flyvel").unwrap_or(0.0);
        w.dry_mass = arg("--dry").unwrap_or(22000.0);
        w.veh_mass = w.dry_mass + 3000.0;
        w.st.amb_p = world::atmosphere_p(h).max(world::MIN_AMBIENT_P);
        w.amb_prev = w.st.amb_p;
    }
    // --burnerdemo: a burner prefab between a fuel tank and an oxidiser tank, to look at its cutaway
    if args.iter().any(|a| a == "--burnerdemo") {
        w.clear();
        w.st.gse_tanks = true;
        w.st.tank_p_f = 3.0e6;
        w.st.tank_p_o = 3.0e6;
        w.st.amb_p = 100.0;
        (w.st.gg_area_f, w.st.gg_area_o) = (6.0, 0.5); // fuel-rich, as a preburner runs
        let clip = prefabs::all().into_iter().find(|p| p.name == "Gas burner").unwrap().clip;
        let (px, py) = (300i32, 250i32);
        w.paste(&clip, px, py);
        let top = (0..clip.w * clip.h).find(|&k| clip.cells[k] == c::PIPE).unwrap();
        let bot = (0..clip.w * clip.h).rev().find(|&k| clip.cells[k] == c::PIPE).unwrap();
        let (tx, ty) = (px + (top % clip.w) as i32, py + (top / clip.w) as i32);
        let (bx, by) = (px + (bot % clip.w) as i32, py + (bot / clip.w) as i32);
        for (x0, y0, x1, y1, k) in [(tx - 5, ty - 14, tx + 6, ty - 1, c::TANK_F), (bx - 5, by + 1, bx + 6, by + 14, c::TANK_O)] {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    w.paint_plumbing(World::idx(x as usize, y as usize), k);
                }
            }
        }
        w.rebuild_nets();
        println!("burnerdemo: {} generators ({} burner blocks)", w.gg.len(), w.gg_lumped.iter().filter(|&&b| b).count());
    }
    // --burner: every gas generator injector plate in the engine becomes a burner block (it burns by its own
    // equations, not in the gas simulation)
    if args.iter().any(|a| a == "--burner") {
        w.rebuild_nets();
        println!("burner: before: {} generators", w.gg.len());
        let mut n = 0;
        for i in 0..world::N {
            if w.cells[i] == c::GG {
                w.set_cell(i, c::BURNER);
                n += 1;
            }
        }
        w.rebuild_nets();
        println!("burner: {n} injector cells became burner cells; {} generators, {} of them burner blocks", w.gg.len(), w.gg_lumped.iter().filter(|&&b| b).count());
    }
    // --hotchamber [--wetlines]: a fire source in the chamber of a preset with nothing flowing, the lines
    // empty (or, with --wetlines, filled), to see what an empty regen jacket does for the walls
    if args.iter().any(|a| a == "--hotchamber") {
        if args.iter().any(|a| a == "--wetlines") {
            // the fuel valves open: the fuel runs down the lines and round the jacket
            w.command_all(0, true);
        }
        for y in 105..=110 {
            for x in 316..=324 {
                w.set_cell(World::idx(x, y), c::SRC_FIRE);
            }
        }
    }
    // --save FILE: write the engine as set up (preset, design or generated) for inspection
    if let Some(path) = args.iter().position(|a| a == "--save").and_then(|p| args.get(p + 1)) {
        std::fs::write(path, w.to_text()).unwrap();
    }
    // --cellpng FILE: the engine as built (cells, with back-layer pipes dimmed blue and front-layer pipes bright), cropped
    // to what is drawn, 3 pixels a cell
    if let Some(path) = args.iter().position(|a| a == "--cellpng").and_then(|p| args.get(p + 1)) {
        let used: Vec<usize> = (0..world::N).filter(|&i| w.cells[i] != c::EMPTY || w.back[i] != 0 || w.front[i] != 0).collect();
        let (x0, x1) = (used.iter().map(|&i| i % W).min().unwrap_or(0).saturating_sub(4), (used.iter().map(|&i| i % W).max().unwrap_or(0) + 4).min(W - 1));
        let (y0, y1) = (used.iter().map(|&i| i / W).min().unwrap_or(0).saturating_sub(4), (used.iter().map(|&i| i / W).max().unwrap_or(0) + 4).min(world::H - 1));
        let (iw, ih, z) = (x1 - x0 + 1, y1 - y0 + 1, 3usize);
        let mut data = vec![22u8; iw * z * ih * z * 3];
        for y in 0..ih * z {
            for x in 0..iw * z {
                let i = World::idx(x0 + x / z, y0 + y / z);
                let mut px = if w.cells[i] != c::EMPTY { c::rgb(w.cells[i]) } else { [22, 22, 22] };
                if w.back[i] != 0 && w.cells[i] != c::JOINT {
                    px = [(px[0] as u32 * 2 / 3 + 20) as u8, (px[1] as u32 * 2 / 3 + 40) as u8, (px[2] as u32 * 2 / 3 + 80) as u8];
                }
                if w.front[i] != 0 && w.cells[i] != c::JOINT && (x / z + y / z) % 2 == 0 {
                    px = [235, 235, 255];
                }
                data[(y * iw * z + x) * 3..(y * iw * z + x) * 3 + 3].copy_from_slice(&px);
            }
        }
        let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).unwrap()), (iw * z) as u32, (ih * z) as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
        println!("wrote {path} ({iw} x {ih} cells from {x0},{y0})");
    }
    // --probe X Y: the cell types in a 9 x 11 patch round a cell (for chasing a failure the event log names)
    if let Some(k) = args.iter().position(|a| a == "--probe") {
        let (px, py): (usize, usize) = (args[k + 1].parse().unwrap(), args[k + 2].parse().unwrap());
        for y in py - 6..=py + 4 {
            let row: Vec<String> = (px - 4..=px + 4).map(|x| format!("{:>3}", w.cells[World::idx(x, y)])).collect();
            println!("probe y={y}: {}", row.join(" "));
        }
    }
    let mut frame = 0u32;
    let mut stand = stand::TestStand::default();
    let mut first = true;
    let mut last_print = 0.0;
    let (mut t_upd, mut t_gpu, mut t_rb) = (0.0f32, 0.0f32, 0.0f32);
    let mut wander: Vec<[f32; 3]> = vec![]; // --wander: the jet's lateral position at three depths, every 10 frames
    while t0.elapsed().as_secs_f32() < seconds {
        let manual = args.iter().any(|a| a == "--manual");
        let burn_at = arg("--burnat").map_or(3, |t| (t * 60.0) as u32);
        if w.flight && frame % 60 == 0 {
            println!("flight t={}: {:.0} m {:+.0} m/s mass {:.0} kg thrust {:.0} kN  {}", frame / 60, w.altitude * 1000.0, w.vel, w.veh_mass, w.thrust_f / 1e3, w.flight_msg);
        }
        if frame == burn_at && !args.iter().any(|a| a == "--firelight" || a == "--hotchamber") {
            if manual {
                // what a player does by hand: open the valves, then press Ignite a bit later
                w.command_all(0, true);
                w.command_all(1, true);
            } else {
                w.start_sequence();
            }
        }
        if args.iter().any(|a| a == "--fuelblob") && frame == 5 {
            // a cloud of fuel vapour beside the engine with a spark of fire at one edge
            for y in (230 + oy)..(270 + oy) {
                for x in (215 + ox)..(252 + ox) {
                    w.paint_gas(World::idx(x, y), 1);
                }
            }
            for y in (248 + oy)..(252 + oy) {
                for x in (211 + ox)..(215 + ox) {
                    w.paint_gas(World::idx(x, y), 2);
                }
            }
        }
        if args.iter().any(|a| a == "--layerdemo") && frame == 1 {
            // reroute part of the fuel line through the back layer, behind a wall in the engine plane
            for y in (40 + oy)..=(41 + oy) {
                for x in (60 + ox)..=(80 + ox) {
                    w.set_cell(World::idx(x, y), c::EMPTY);
                }
                for x in (58 + ox)..=(82 + ox) {
                    w.set_layer_cell(1, World::idx(x, y), c::PIPE);
                }
            }
            w.rect((68 + ox) as i32, (28 + oy) as i32, (71 + ox) as i32, (54 + oy) as i32, c::STEEL);
            for y in (30 + oy)..=(50 + oy) {
                w.set_layer_cell(2, World::idx(75 + ox, y), c::PIPE);
                w.set_layer_cell(2, World::idx(76 + ox, y), c::PIPE);
            }
        }
        if args.iter().any(|a| a == "--breach") && frame == 120 {
            // punch holes: above the oxidiser tank and in the side of the fuel tank
            for x in (115 + ox)..=(118 + ox) {
                w.set_cell(World::idx(x, 7 + oy), c::EMPTY);
            }
            for y in (130 + oy)..=(133 + oy) {
                w.set_cell(World::idx(5 + ox, y), c::EMPTY);
            }
        }
        if args.iter().any(|a| a == "--coldgas") {
            // a lone helium bottle; knock one block out of its bottom wall and it becomes a cold-gas thruster
            if frame == 20 && args.iter().any(|a| a == "--bighole") {
                for x in 302..=338 {
                    w.set_cell(World::idx(x, 261), c::EMPTY);
                }
            }
            if frame == 20 {
                w.set_cell(World::idx(320, 261), c::EMPTY);
                w.set_cell(World::idx(321, 261), c::EMPTY);
            }
            if frame == 20 && args.iter().any(|a| a == "--brushhole") {
                // a careless right-click with a 21x21 brush across the bottle's bottom edge
                for y in 250..=270 {
                    for x in 310..=330 {
                        w.paint_gas(World::idx(x, y), 3);
                    }
                }
            }
            if frame == 120 && args.iter().any(|a| a == "--brushhole") && !args.iter().any(|a| a == "--patchlate") {
                for y in 250..=270 {
                    for x in 310..=330 {
                        w.set_cell(World::idx(x, y), c::STEEL);
                    }
                }
                println!("patched with a 21x21 steel block; bottle at {:.0} bar", w.he_bottles.first().map_or(0.0, |b| b.p / 1e5));
            }
            if frame > 120 && frame % 30 == 0 && args.iter().any(|a| a == "--brushhole") {
                let steel = (310..=330).flat_map(|x| (250..=270).map(move |y| (x, y))).filter(|&(x, y)| w.cells[World::idx(x, y)] == c::STEEL).count();
                println!("frame {frame}: patch steel {steel}/441, leak {:.1} kg/s", w.g.leak);
            }
            if frame == 120 && args.iter().any(|a| a == "--patch") {
                // plug the hole again with steel, the way a player would paint over it
                let (x0, x1, y1) = if args.iter().any(|a| a == "--thin") { (320, 321, 261) } else { (318, 323, 263) };
                for x in x0..=x1 {
                    for y in 261..=y1 {
                        w.set_cell(World::idx(x, y), c::STEEL);
                    }
                }
                println!("patched; bottle at {:.0} bar", w.he_bottles.first().map_or(0.0, |b| b.p / 1e5));
            }
            if frame > 120 && frame % 30 == 0 && args.iter().any(|a| a == "--patch") {
                let steel = (318..=323).flat_map(|x| (261..=263).map(move |y| (x, y))).filter(|&(x, y)| w.cells[World::idx(x, y)] == c::STEEL).count();
                println!("frame {frame}: steel cells in patch area {steel}, leak {:.1} kg/s", w.g.leak);
            }
        }
        if args.iter().any(|a| a == "--clearfire") {
            // Clear all mid-run, then paint a blob of fire into the empty space
            if frame == 240 {
                w.clear();
                first = true;
            }
            if frame >= 250 && frame < 310 {
                // like a fast drag: 200 overlapping 21x21 brush stamps every frame for a second
                let t0 = std::time::Instant::now();
                for k in 0..200usize {
                    let (cx, cy) = (150 + k + (frame - 250) as usize, 300 + k / 2);
                    for y in cy - 10..=cy + 10 {
                        for x in cx - 10..=cx + 10 {
                            w.paint_gas(World::idx(x, y), 2);
                        }
                    }
                }
                if frame == 250 {
                    println!("painting 200 stamps took {:.2} ms", t0.elapsed().as_secs_f32() * 1000.0);
                }
            }
        }
        if args.iter().any(|a| a == "--hethruster") {
            // a helium bottle feeding a valved injector (cold-gas thruster), valve opened after a moment
            if frame == 1 {
                w.clear();
                for y in 300..=330 {
                    for x in 300..=340 {
                        w.paint_plumbing(World::idx(x, y), c::HE_TANK);
                    }
                }
                for y in 331..=350 {
                    for x in 319..=321 {
                        w.paint_plumbing(World::idx(x, y), c::PIPE);
                    }
                }
                for x in 319..=321 {
                    w.paint_plumbing(World::idx(x, 345), c::VALVE_X);
                    w.set_cell(World::idx(x, 351), c::INJ);
                }
                first = true;
            }
            if frame == 30 {
                w.command_all(world::PROP_HE, true);
                println!("valve: {}", w.valves.first().map(|v| v.label()).unwrap_or_default());
            }
        }
        if args.iter().any(|a| a == "--firelight") {
            // no spark and no igniter: open the valves by hand, let it cold-flow, then paint fire into the
            // chamber just below the injectors
            if frame == 4 {
                w.command_all(0, true);
                w.command_all(1, true);
            }
            if frame == 150 {
                let draw: f32 = w.nets.iter().map(|n| n.gg_draw).sum();
                let pgg = w.gg_faces.first().filter(|f| !f.is_empty() && rb.valid).map_or(0.0, |f| gas_p(rb.state[f[0].1], w.st.p().gamma));
                println!("before the fire: ignited {}, gas generator flow {:.1} kg/s, its chamber {:.2} bar", w.ignited, draw, pgg / 1e5);
                for y in (63 + oy)..(69 + oy) {
                    for x in (118 + ox)..(138 + ox) {
                        w.paint_gas(World::idx(x, y), 2);
                    }
                }
            }
            if frame == 260 {
                println!("after the fire: ignited {}", w.ignited);
            }
        }
        if args.iter().any(|a| a == "--gaspipe") {
            // a closed steel box pressurised by a gas source, vented through a plain pipe to open air
            if frame == 1 {
                w.clear();
                w.rect(300, 300, 340, 340, c::STEEL);
                w.rect(302, 302, 338, 338, c::EMPTY);
                w.rect(318, 318, 321, 321, c::SRC_GAS);
                if !args.iter().any(|a| a == "--nopipe") {
                    for x in 330..=372 {
                        w.paint_plumbing(World::idx(x, 320), c::PIPE);
                        w.paint_plumbing(World::idx(x, 321), c::PIPE);
                    }
                    // open both ends, as a player would by erasing the caps
                    for y in 320..=321 {
                        w.set_cell(World::idx(329, y), c::EMPTY);
                        w.set_cell(World::idx(373, y), c::EMPTY);
                    }
                }
                w.st.src_p = 2.0e6;
                first = true;
            }
            if frame % 40 == 0 && frame > 0 && rb.valid {
                let pb = gas_p(rb.state[World::idx(310, 310)], w.st.p().gamma);
                let jet = rb.state[World::idx(376, 320)];
                println!("frame {frame}: box {:.2} bar, gas lines {}, outside the pipe end {:.2} bar {:.0} m/s", pb / 1e5, w.gas_pipes.len(), gas_p(jet, w.st.p().gamma) / 1e5, jet[1] / jet[0].max(1e-6));
            }
        }
        if args.iter().any(|a| a == "--turbfire") {
            // a closed steel box with fire burning in it, its right wall a turbine wheel (with a pump on the
            // shaft) opening to open air: the gas pushing through has to spin it, no gas generator involved
            if frame == 1 {
                w.clear();
                w.rect(300, 300, 340, 340, c::STEEL);
                w.rect(302, 302, 338, 338, c::EMPTY);
                w.rect(318, 318, 321, 321, c::SRC_FIRE);
                w.rect(339, 310, 346, 330, c::TURBINE);
                w.rect(347, 318, 360, 322, c::SHAFT);
                w.rect(361, 312, 372, 328, c::PUMP);
                w.st.src_p = 2.0e6;
                first = true;
            }
            if frame % 40 == 0 && frame > 0 && rb.valid {
                let pb = gas_p(rb.state[World::idx(310, 310)], w.st.p().gamma);
                let s = &w.shafts[0];
                println!("frame {frame}: box {:.2} bar, turbine {:.0} rpm, gas power {:.2} MW, c0 {:.0} m/s", pb / 1e5, s.rpm(), s.p_gas / 1e6, s.c0_gas);
            }
        }
        if let Some(t) = arg("--place") {
            // place one steel cell far from everything at this many seconds in, and log the chamber around it
            let f0 = (t * 60.0) as u32;
            if frame == f0 {
                w.set_cell(World::idx(8, 8), c::STEEL);
            }
            if frame + 20 >= f0 && frame < f0 + 90 && frame % 5 == 0 {
                println!("frame {frame}: Pc {:.2} bar thrust {:.0} kN mdot {:.1}", w.g.chamber_p / 1e5, w.g.thrust / 1e3, w.g.mdot_f + w.g.mdot_o);
            }
        }
        if let Some(t) = arg("--stop") {
            // press Shutdown at this many seconds in
            if frame == (t * 60.0) as u32 {
                w.shutdown();
                println!("SHUTDOWN at frame {frame}");
            }
        }
        if let Some(m) = arg("--mix") {
            // mixture-ratio control on from the start
            if frame == 1 {
                w.st.mix_target = m;
            }
        }
        if let Some(t) = arg("--thr") {
            // throttle back once the engine has settled
            if frame == 480 {
                w.st.throttle = t;
            }
        }
        if args.iter().any(|a| a == "--plumesteel") {
            // a steel block dropped into the running exhaust (~3000 K) has to melt
            if frame == 400 {
                for y in (oy as i32 + 150)..=(oy as i32 + 154) { for x in (ox as i32 + 125)..=(ox as i32 + 130) { w.set_cell(World::idx(x as usize, y as usize), c::STEEL); } } // like painting
            }
            if frame == 401 || frame % 200 == 0 {
                let blk: Vec<[f32; 4]> = (150..=154).flat_map(|y| (125..=130).map(move |x| (x, y))).map(|(x, y)| rb.state.get(World::idx(x + ox, y + oy)).copied().unwrap_or_default()).collect();
                let gas_t = rb.state.get(World::idx(128 + ox, 148 + oy)).map_or(0.0, |s| gas_p(*s, w.st.p().gamma) / (s[0].max(1e-6) * w.st.p().rgas));
                println!("frame {frame}: steel cells {}  block rho max {:.3} T max {:.0}  gas above {:.0} K", w.cells.iter().filter(|&&t| t == c::STEEL).count(), blk.iter().map(|s| s[0]).fold(0.0, f32::max), blk.iter().map(|s| s[3]).fold(0.0, f32::max), gas_t);
            }
        }
        if args.iter().any(|a| a == "--clearsteel") {
            // Clear all whose gas reset gets lost, then a Ctrl-drag steel rectangle over where the engine was
            if frame == 240 {
                w.clear();
                if !args.iter().any(|a| a == "--lostreset") {
                    first = true;
                }
            }
            if frame == 260 {
                for y in (oy as i32 + 100)..=(oy as i32 + 300) { for x in (ox as i32 + 60)..=(ox as i32 + 200) { w.set_cell(World::idx(x as usize, y as usize), c::STEEL); } } // like painting
                println!("steel cells after drawing: {}", w.cells.iter().filter(|&&t| t == c::STEEL).count());
            }
            if frame == 400 {
                println!("steel cells later: {}", w.cells.iter().filter(|&&t| t == c::STEEL).count());
            }
        }
        if manual && frame == 200 {
            w.spark_t = 2.5;
        }
        let tq = Instant::now();
        w.update(&rb, 1.0 / 60.0);
        t_upd += tq.elapsed().as_secs_f32();
        stand.record(&w);
        if frame == 120 && args.iter().any(|a| a == "--patchlate") {
            // like the app: the brush paints after this frame's update, so the leak jet is already aimed here
            for y in 250..=270 {
                for x in 310..=330 {
                    w.set_cell(World::idx(x, y), c::STEEL);
                }
            }
            println!("patched after the update; bottle at {:.0} bar", w.he_bottles.first().map_or(0.0, |b| b.p / 1e5));
        }
        if frame > 120 && frame % 60 == 0 && args.iter().any(|a| a == "--patchlate") {
            let steel = (310..=330).flat_map(|x| (250..=270).map(move |y| (x, y))).filter(|&(x, y)| w.cells[World::idx(x, y)] == c::STEEL).count();
            println!("frame {frame}: patch steel {steel}/441");
        }
        let st = w.st;
        let mat: Vec<u32> = (0..world::N).map(|i| w.gpu_mat(i)).collect();
        let reset = first;
        first = false;
        let dirty: Vec<(usize, bool)> = w.dirty.drain(..).map(|i| (i, w.cells[i] == c::EMPTY)).collect();
        let do_mat = reset || w.mat_stale || !dirty.is_empty();
        w.mat_stale = false;
        w.phase_dirty = false;
        let pr = st.p();
        let inp = FrameInput {
            reset: if reset { Some(mat.clone()) } else { None },
            mat: if do_mat && !reset { Some(mat) } else { None },
            dirty,
            params: w.sim_params(),
            net_t: w.net_temps(),
            srcs: w.srcs.clone(),
            steps: st.steps_per_frame,
            particles: true,
            phase: if reset || w.phase_dirty { Some(w.phase.clone()) } else { None },
            layers: if do_mat { Some(w.gpu_layers()) } else { None },
            anim: w.anim(),
            paint: paint_states(&mut w, &rb),
            sim: true,
            render: RenderParams { w: W as u32, h: H as u32, mode: 0, show_heat: 1, cx: 0.0, cy: 0.0, scale: 1.0, gamma: pr.gamma, rw: 100.0, rh: 100.0, rgas: pr.rgas, particles: 1, time: 0.0, tint: pr.tint, soot: pr.soot, liq_f: pr.liq_f, liq_o: pr.liq_o, edit_layer: 0, amb_rho: w.st.amb_p / (pr.rgas * 300.0), _q: [0.0; 3]},
        };
        let tq = Instant::now();
        gpu.frame(&device, &queue, inp);
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        t_gpu += tq.elapsed().as_secs_f32();
        let tq = Instant::now();
        if let Some(r) = gpu.take_readback(&device) {
            rb = r;
        }
        t_rb += tq.elapsed().as_secs_f32();
        frame += 1;
        // --wander: where the jet is sideways, as the momentum-weighted mean x of the gas moving down fast, at 1.5, 4 and 8 exit
        // diameters below the exit (how far the exhaust swings about in the simulation)
        if args.iter().any(|a| a == "--wander") && frame % 10 == 0 && frame > 480 && rb.valid && w.plume[0] > 0.5 {
            let (ye, de) = (w.plume[2] as usize, 2.0 * w.plume[3]);
            let mut row = [f32::NAN; 3];
            for (k, mult) in [1.5f32, 4.0, 8.0].iter().enumerate() {
                let y = ye + (mult * de) as usize;
                if y + 1 >= H {
                    continue;
                }
                let (mut sx, mut sm) = (0.0f64, 0.0f64);
                for x in 0..W {
                    let s = rb.state[World::idx(x, y)];
                    let vy = s[2] / s[0].max(1e-6);
                    if vy > 150.0 {
                        let m = (s[0] * vy) as f64;
                        sx += x as f64 * m;
                        sm += m;
                    }
                }
                if sm > 0.0 {
                    row[k] = (sx / sm) as f32;
                }
            }
            wander.push(row);
        }
        // --gif PREFIX [--gifstart F] [--gifevery N] [--gifn COUNT] [--gifmode M]: a flame-view PNG every N frames from frame F
        // (for the README's animation; ffmpeg turns the numbered files into a GIF). Stops once COUNT are saved.
        if let Some(gp) = args.iter().position(|a| a == "--gif").and_then(|p| args.get(p + 1)) {
            let (g0, ge, gn) = (arg("--gifstart").unwrap_or(0.0) as u32, (arg("--gifevery").unwrap_or(4.0) as u32).max(1), arg("--gifn").unwrap_or(60.0) as u32);
            if frame >= g0 && (frame - g0) % ge == 0 {
                let k = (frame - g0) / ge;
                if k >= gn {
                    break;
                }
                SHOT_TIME.store((frame as f32 / 60.0 % 1000.0).to_bits(), Ordering::Relaxed);
                let view = match args.iter().position(|a| a == "--view") {
                    Some(p) => (
                        args[p + 1].parse().unwrap_or(128.0) + presets::PRESET_OX as f32,
                        args[p + 2].parse().unwrap_or(192.0) + presets::PRESET_OY as f32,
                        args[p + 3].parse().unwrap_or(2.6),
                    ),
                    None => (W as f32 / 2.0, world::VIEW_ROWS as f32 / 2.0, 1000.0 / world::VIEW_ROWS as f32),
                };
                shoot(&device, &queue, &mut gpu, &mut w, arg("--gifmode").unwrap_or(6.0) as u32, &format!("{gp}_{k:04}.png"), view);
            }
        }
        let el = t0.elapsed().as_secs_f32();
        if el - last_print > arg("--every").unwrap_or(1.0) {
            last_print = el;
            let g = w.g;
            let bell_t = (200 * W..330 * W).filter(|&i| w.cells[i] != c::EMPTY && !c::is_line(w.cells[i])).map(|i| rb.state.get(i).map_or(0.0, |s| s[3])).fold(0.0f32, f32::max);
            let thr_t = (140 * W..150 * W).filter(|&i| w.cells[i] != c::EMPTY && !c::is_line(w.cells[i])).map(|i| rb.state.get(i).map_or(0.0, |s| s[3])).fold(0.0f32, f32::max);
            let nb_t = (0..world::N).filter(|&i| w.cells[i] == c::NIOBIUM).filter_map(|i| rb.state.get(i)).filter(|s| s[0] == 0.0).map(|s| s[3]).fold(0.0f32, f32::max);
            println!("   wall peak: throat {:.0} K, lower bell {:.0} K, niobium {:.0} K, regen heat {:.1} MW, wet pipe cells {}", thr_t, bell_t, nb_t, g.q_regen / 1e6, (0..world::N).filter(|&i| w.cells[i] == c::PIPE && w.wet_cell(i)).count());
            for (k, s) in w.shafts.iter().enumerate() {
                let pumps: Vec<String> = s.pumps.iter().map(|p| format!("{}:{:.0}cm x{} tip{:.0} head{:.0}bar eff{:.0}% cav{:.2}{}", if p.prop == 0 { "F" } else { "O" }, p.d * 100.0, p.stages, p.tip(s.omega), p.head / 1e5, p.eff * 100.0, p.cav, if p.wet { "" } else { " DRY" })).collect();
                println!("   regen{:?} tp{} {:.0}rpm ({:.0}% rated) J={:.2} turb {:.0}cm x{} u/c0={:.2} eta={:.2} P={:.1}/{:.1}MW {:?}", s.regen_nets, k + 1, s.rpm(), s.x() * 100.0, s.j, s.d_turb * 100.0, s.turb_stages, s.u_c0, s.eff_t, s.p_turb / 1e6, s.p_load / 1e6, pumps);
                if s.gas_driven && rb.valid {
                    let gamma = w.st.p().gamma;
                    let avg = |v: &mut dyn Iterator<Item = usize>| {
                        let (mut sum, mut n) = (0.0f32, 0);
                        for j in v {
                            sum += gas_p(rb.state[j], gamma);
                            n += 1;
                        }
                        (sum / n.max(1) as f32 / 1e5, n)
                    };
                    let (p_in, n_in) = avg(&mut s.turb_in.iter().copied());
                    let (p_out, n_out) = avg(&mut s.turb_faces.iter().copied().filter(|j| !s.turb_in.contains(j)));
                    println!("   drive gas tp{}: in {:.1} bar ({} cells) out {:.1} bar ({} cells), gas power {:.2} MW, c0 {:.0} m/s", k + 1, p_in, n_in, p_out, n_out, s.p_gas / 1e6, s.c0_gas);
                }
                if s.motor_cells > 0 {
                    if let Some(cc) = w.circuits.get(s.circuit as usize) {
                        println!("   motor {:.0} kW, battery {:.0}% ({:.0} kW of {:.0})", s.p_motor / 1e3, 100.0 * cc.energy / cc.cap.max(1.0), cc.p_draw / 1e3, cc.p_max / 1e3);
                    }
                }
            }
            for (k, f) in w.gg_faces.iter().enumerate() {
                if rb.valid && !f.is_empty() {
                    let p = f.iter().map(|x| gas_p(rb.state[x.1], w.st.p().gamma)).sum::<f32>() / f.len() as f32;
                    let draw: f32 = w.nets.iter().map(|n| n.gg_draw).sum();
                    println!("   generator {}: chamber {:.1} bar (set {:.0}), all generators draw {:.2} kg/s, gas {:.0} K (hold {:.0}), oxidiser valve {:.0}%", k + 1, p / 1e5, w.st.gg_p / 1e5, draw, w.g.t_gg, w.st.gg_t_target, w.gg_trim * 100.0);
                }
            }
            for n in w.nets.iter().filter(|n| !n.injectors.is_empty()) {
                println!("   net p{} tank{:.1} head{:.0} up{:.0} pc{:.0} m{:.1} line{:.0} valves{:?} inj{} breaches{} leak{:.2}", n.prop, n.p_tank / 1e5, n.head / 1e5, n.p_up / 1e5, n.pc_f / 1e5, n.m_inj, n.p_line / 1e5, n.valves.iter().map(|&v| w.valves[v].pos).collect::<Vec<_>>(), n.injectors.len(), n.breaches.len(), n.leak);
            }
            // --probet X Y: wall temperatures along a row (16 cells from X), with the flow and wetness of any injector there
            if let Some(k) = args.iter().position(|a| a == "--probet") {
                let (px, py): (usize, usize) = (args[k + 1].parse().unwrap(), args[k + 2].parse().unwrap());
                let row: Vec<String> = (px..px + 16).map(|x| format!("{}:{:.0}", w.cells[World::idx(x, py)], rb.state.get(World::idx(x, py)).map_or(0.0, |s| s[3]))).collect();
                println!("   probet y={py}: {}", row.join(" "));
            }
            let nan = rb.state.iter().filter(|s| !s[0].is_finite() || !s[3].is_finite()).count();
            println!(
                "t={:.1}s fr={} ign={} Tcool={:.0} Pc={:.1}bar thrust={:.3}MN mdot={:.1}+{:.1} OF={:.2} rpm={:.0} he={:.0}% gg={:.1}MW/{:.0}K pumps={:.1}MW Twall={:.0}K Tflame={:.0} Isp={:.0} alt={:.1}km amb={:.3}bar nan={} mix={:.2}/{:.2} valves={:.2}/{:.2} ev={:?}",
                el, frame, g.ignited as u8, g.t_cool, g.chamber_p / 1e5, g.thrust / 1e6, g.mdot_f, g.mdot_o, g.of, g.rpm, g.he_frac * 100.0, g.p_gg / 1e6, g.t_gg, g.p_pump / 1e6, g.max_wall_t, g.t_flame, g.isp, if w.alt_on { w.altitude } else { 0.0 }, w.st.amb_p / 1e5, nan, g.of, w.st.mix_target, w.thr_valves[0], w.thr_valves[1], w.events.last()
            );
        }
    }
    println!("events: {:?}", w.events);
    if args.iter().any(|a| a == "--stand") {
        let m = stand::metrics(&stand.run);
        println!("stand: {} samples, {:?}", stand.run.len(), m);
    }
    println!("start: he_spin {} dead_start {} cartridge cells {} (per shaft {:?}), setting {:.1} MW", w.st.he_spin, w.st.dead_start, w.cells.iter().filter(|&&t| t == c::CARTRIDGE).count(), w.shafts.iter().map(|s| s.cart_cells).collect::<Vec<_>>(), w.st.cartridge_w / 1e6);
    println!("generators: {} ({} burner blocks)", w.gg.len(), w.gg_lumped.iter().filter(|&&b| b).count());
    println!("jet: {:.1} kg/s, thrust {:.1} kN up, {:.1} kN sideways, helium left {:.0}%", w.g.leak, w.g.leak_thrust / 1e3, w.g.leak_side / 1e3, w.he_frac * 100.0);
    if std::env::var("ROCKET_PROFILE").is_ok() {
        let (a, t, r) = gpu.count_awake(&device, &queue);
        println!("tiles awake: {a} of {t}; tiles wanting step rate 1/2/4/8: {r:?}");
        println!("per frame: world update {:.2} ms, gpu submit+wait {:.2} ms, read-back {:.2} ms ({} frames)", 1e3 * t_upd / frame.max(1) as f32, 1e3 * t_gpu / frame.max(1) as f32, 1e3 * t_rb / frame.max(1) as f32, frame);
    }
    println!("gas steps: {} total, {:.0} per second ({} cells)", gpu.total_steps, gpu.total_steps as f32 / t0.elapsed().as_secs_f32(), world::N);
    if args.iter().any(|a| a == "--flowmap") && rb.valid {
        // coarse map of the flow: an arrow per 20 x 20 cells (average velocity), '#' solid, '.' still
        for gy in 0..H / 20 {
            let mut line = String::new();
            for gx in 0..W / 20 {
                let (mut mx, mut my, mut m, mut solid) = (0.0f32, 0.0f32, 0.0f32, 0usize);
                for y in gy * 20..gy * 20 + 20 {
                    for x in gx * 20..gx * 20 + 20 {
                        let i = World::idx(x, y);
                        if w.cells[i] != c::EMPTY {
                            solid += 1;
                            continue;
                        }
                        let st = rb.state[i];
                        mx += st[1];
                        my += st[2];
                        m += st[0];
                    }
                }
                let (u, v) = (mx / m.max(1e-6), my / m.max(1e-6));
                let sp = (u * u + v * v).sqrt();
                let ch = if solid > 200 {
                    '#'
                } else if sp < 15.0 {
                    '.'
                } else {
                    let a = v.atan2(u); // screen: y down
                    ['→', '↘', '↓', '↙', '←', '↖', '↑', '↗'][(((a / (std::f32::consts::PI / 4.0)).round() as i32).rem_euclid(8)) as usize]
                };
                line.push(ch);
            }
            println!("{line}");
        }
    }
    if let Some(k) = args.iter().position(|a| a == "--probe") {
        // print the gas at frame-coordinate points: --probe x,y;x,y;...
        let pr = w.st.p();
        for pt in args.get(k + 1).map(|s| s.as_str()).unwrap_or("").split(';').filter(|s| !s.is_empty()) {
            let v: Vec<usize> = pt.split(',').filter_map(|t| t.trim().parse().ok()).collect();
            if v.len() == 2 && rb.valid {
                let i = World::idx(v[0] + presets::PRESET_OX, v[1] + presets::PRESET_OY);
                let s = rb.state[i];
                let p = gas_p(s, pr.gamma);
                if w.cells[i] != c::EMPTY && s[0] == 0.0 {
                    println!("probe ({},{}) type {} wall T {:.0} K", v[0], v[1], w.cells[i], s[3]);
                } else {
                    println!("probe ({},{}) type {} rho {:.3} T {:.0} K P {:.2} bar  u {:.0} v {:.0} m/s", v[0], v[1], w.cells[i], s[0], p / (s[0].max(1e-6) * pr.rgas), p / 1e5, s[1] / s[0].max(1e-6), s[2] / s[0].max(1e-6));
                }
            }
        }
    }
    if args.iter().any(|a| a == "--fuelblob") && rb.valid {
        let pr = w.st.p();
        let (ox, oy) = (presets::PRESET_OX, presets::PRESET_OY);
        for (x, y) in [(212 + ox, 250 + oy), (215 + ox, 250 + oy), (217 + ox, 250 + oy), (230 + ox, 250 + oy)] {
            let s = rb.state[World::idx(x, y)];
            println!("cell ({x},{y}) type {} rho {:.4} T {:.0} K P {:.2} bar", w.cells[World::idx(x, y)], s[0], gas_p(s, pr.gamma) / (s[0].max(1e-6) * pr.rgas), gas_p(s, pr.gamma) / 1e5);
        }
    }
    if args.iter().any(|a| a == "--wander") {
        for (k, name) in ["1.5 D", "4 D", "8 D"].iter().enumerate() {
            let v: Vec<f32> = wander.iter().map(|r| r[k]).filter(|x| x.is_finite()).collect();
            if v.len() > 3 {
                let mean = v.iter().sum::<f32>() / v.len() as f32;
                let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
                let (lo, hi) = (v.iter().cloned().fold(f32::MAX, f32::min), v.iter().cloned().fold(f32::MIN, f32::max));
                println!("wander at {name} below the exit: mean x {mean:.1}, std dev {sd:.1} cells, range {lo:.1} .. {hi:.1} ({} samples)", v.len());
            }
        }
    }
    if args.iter().any(|a| a == "--plumeinfo") {
        let p = w.plume;
        println!("plume: on {} exit x {:.0} y {:.0} half-width {:.1} cells, exit speed {:.0} m/s, Pe/Pa {:.2}, Mach {:.2}, flame {:.0} K, intensity {:.2}, running at {:.2}, jet half-width {:.1}, separation {:.2}", p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[14], p[15], p[16]);
    }
    if let Some(prefix) = shot {
        for (m, name) in [(6u32, "flame"), (1, "pressure"), (3, "mach"), (4, "temp")] {
            let view = match args.iter().position(|a| a == "--view") {
                Some(p) => (
                    args[p + 1].parse().unwrap_or(128.0) + presets::PRESET_OX as f32,
                    args[p + 2].parse().unwrap_or(192.0) + presets::PRESET_OY as f32,
                    args[p + 3].parse().unwrap_or(2.6),
                ),
                None => (W as f32 / 2.0, world::VIEW_ROWS as f32 / 2.0, 1000.0 / world::VIEW_ROWS as f32),
            };
            shoot(&device, &queue, &mut gpu, &mut w, m, &format!("{prefix}_{name}.png"), view);
        }
    }
}

/// The main window: size, title and the icon (assets/icon.png, built into the exe).
fn window_viewport() -> egui::ViewportBuilder {
    let mut vp = egui::ViewportBuilder::default().with_inner_size([1500.0, 950.0]).with_title("Hotfire");
    let mut dec = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/icon.png").as_slice()));
    dec.set_transformations(png::Transformations::EXPAND);
    if let Ok(mut rd) = dec.read_info() {
        let mut buf = vec![0u8; rd.output_buffer_size().unwrap_or(0)];
        if let Ok(info) = rd.next_frame(&mut buf) {
            if info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight {
                buf.truncate(info.buffer_size());
                vp = vp.with_icon(egui::IconData { rgba: buf, width: info.width, height: info.height });
            }
        }
    }
    vp
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(p) = args.iter().position(|a| a == "--selftest") {
        let secs = args.get(p + 1).and_then(|s| s.parse().ok()).unwrap_or(20.0);
        let shot = args.get(p + 2).filter(|s| !s.starts_with("--")).cloned();
        selftest(secs, shot, &args);
        return Ok(());
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: window_viewport(),
        ..Default::default()
    };
    eframe::run_native(
        "Hotfire",
        options,
        Box::new(|cc| {
            let rs = cc.wgpu_render_state.as_ref().expect("wgpu render state");
            let sim = GpuSim::new(&rs.device, rs.target_format);
            rs.renderer.write().callback_resources.insert(sim);
            let mut app = App::new();
            // test overrides: gas steps per 1/60 s and the starting view
            if let Some(n) = std::env::var("ROCKET_STEPS").ok().and_then(|v| v.parse().ok()) {
                app.world.st.steps_per_frame = n;
            }
            if let Some(m) = std::env::var("ROCKET_MODE").ok().and_then(|v| v.parse().ok()) {
                app.mode = m;
            }
            if let Some(k) = std::env::var("ROCKET_PRESET").ok().and_then(|v| v.parse::<usize>().ok()) {
                app.world.load_preset(k);
            }
            if std::env::var("ROCKET_DEMO_GAUGE").is_ok() {
                // screenshot aid: two gauges on the default engine, already firing
                let (ox, oy) = (presets::PRESET_OX as f32, presets::PRESET_OY as f32);
                app.gauges.push(PressureGauge { at: egui::vec2(60.0 + ox, 70.0 + oy), probe: World::idx(128 + presets::PRESET_OX, 80 + presets::PRESET_OY), range_bar: 1.0 });
                app.gauges.push(PressureGauge { at: egui::vec2(200.0 + ox, 60.0 + oy), probe: World::idx(127 + presets::PRESET_OX, 15 + presets::PRESET_OY), range_bar: 1.0 });
                app.world.start_sequence();
            }
            if let Some(k) = std::env::var("ROCKET_PLUMESTYLE").ok().and_then(|v| v.parse::<u32>().ok()) {
                app.world.fx_style = k;
            }
            if std::env::var("ROCKET_WHOLEGRID").is_ok() {
                app.view_reset = 2; // for screenshots: start zoomed out to the whole grid
            }
            if std::env::var("ROCKET_FULLSIM").is_ok() {
                app.world.fx_plume = false; // for screenshots: the full simulation, no plume shader
            }
            if std::env::var("ROCKET_AUTOSTART").is_ok() {
                app.world.start_sequence();
            }
            if std::env::var("ROCKET_STAND").is_ok() {
                app.stand.open = true; // for screenshots
            }
            if let Some(r) = std::env::var("ROCKET_CLIMB").ok().and_then(|v| v.parse::<f32>().ok()) {
                app.world.climbing = true; // for screenshots
                app.world.climb_rate = r;
                app.world.alt_on = true;
                app.stand.show[16] = true;
                app.stand.show[1] = true;
            }
            Ok(Box::new(app))
        }),
    )
}

/// Pump map window: geometry, head-flow curves at several speeds, the live operating point, turbine and NPSH data.
fn turbopump_ui(ui: &mut egui::Ui, w: &World, s: &Shaft) {
    let rpm_of = |omega: f32| omega * 60.0 / std::f32::consts::TAU;
    ui.label(format!(
        "{:.0} rpm ({:.0}% of rated {:.0} rpm)   pumps {:.1} MW",
        s.rpm(),
        s.x() * 100.0,
        rpm_of(s.omega_rated),
        s.p_load / 1e6
    ));
    let (opt, peak) = world::turbine_opt(s.turb_stages);
    ui.label(format!(
        "turbine: {:.0} cm wheel, {} stage{}, blade speed {:.0} m/s, u/c0 {:.2} (best {:.2}) -> {:.0}% eff, {:.1} MW",
        s.d_turb * 100.0,
        s.turb_stages,
        if s.turb_stages > 1.0 { "s" } else { "" },
        s.omega * 0.5 * s.d_turb,
        s.u_c0,
        opt,
        s.eff_t * 100.0,
        s.p_turb / 1e6
    ));
    // turbine efficiency against blade-speed ratio for 1, 2 and 3 stages, with where it is running now
    {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(320.0), 90.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 4.0, egui::Color32::from_gray(16));
        let inner = rect.shrink2(egui::vec2(28.0, 12.0));
        let to = |a: f32, e: f32| egui::pos2(inner.left() + (a / 1.0).min(1.0) * inner.width(), inner.bottom() - (e / 0.9).clamp(0.0, 1.0) * inner.height());
        p.line_segment([inner.left_bottom(), inner.right_bottom()], egui::Stroke::new(1.0, egui::Color32::GRAY));
        for n in [1.0f32, 2.0, 3.0] {
            let pts: Vec<egui::Pos2> = (0..=50).map(|k| k as f32 / 50.0).map(|a| to(a, world::turbine_eff(a, n))).collect();
            let col = if n == s.turb_stages { egui::Color32::from_rgb(255, 170, 80) } else { egui::Color32::from_gray(70) };
            p.add(egui::Shape::line(pts, egui::Stroke::new(if n == s.turb_stages { 2.0 } else { 1.0 }, col)));
            let (o, e) = world::turbine_opt(n);
            p.text(to(o, e) + egui::vec2(0.0, -4.0), egui::Align2::CENTER_BOTTOM, format!("{n:.0}"), egui::FontId::proportional(9.0), egui::Color32::GRAY);
        }
        p.circle_filled(to(s.u_c0.min(1.0), s.eff_t), 4.5, egui::Color32::from_rgb(255, 90, 60));
        p.text(inner.right_bottom() + egui::vec2(0.0, 1.0), egui::Align2::RIGHT_TOP, "blade speed / gas spouting speed (u/c0)", egui::FontId::proportional(9.0), egui::Color32::GRAY);
        p.text(inner.left_top(), egui::Align2::LEFT_TOP, format!("turbine efficiency (peak {:.0}%), curves for 1-3 stages", peak * 100.0), egui::FontId::proportional(9.0), egui::Color32::GRAY);
    }
    ui.label(format!(
        "rotor inertia {:.2} kg m2; rated = {:.0} m/s tip speed; bursts at {:.0} rpm{}. Longer turbine blocks have more stages (one per 0.4 diameters, up to 3): slower blades suit more stages.",
        s.j,
        world::U_TIP_MAX,
        rpm_of(world::OVERSPEED * s.omega_rated),
        if w.st.governor { "  (governor on)" } else { "  (GOVERNOR OFF)" }
    ));
    // end-on impeller like the inset of the reference picture
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::hover());
    let p = ui.painter_at(r);
    let c0 = r.center();
    p.circle(c0, 30.0, egui::Color32::from_rgb(40, 50, 70), egui::Stroke::new(1.5, egui::Color32::from_rgb(140, 160, 200)));
    for k in 0..7 {
        let a0 = s.theta * 6.0 + k as f32 * std::f32::consts::TAU / 7.0;
        let pts: Vec<egui::Pos2> = (0..6)
            .map(|j| {
                let t = j as f32 / 5.0;
                let rad = 6.0 + 22.0 * t;
                let a = a0 + 0.9 * t;
                c0 + egui::vec2(a.cos(), a.sin()) * rad
            })
            .collect();
        p.add(egui::Shape::line(pts, egui::Stroke::new(2.0, egui::Color32::from_rgb(180, 200, 255))));
    }
    p.circle_filled(c0, 5.0, egui::Color32::from_gray(200));
    for pump in &s.pumps {
        ui.separator();
        pump_map(ui, w, s, pump);
    }
    if s.pumps.is_empty() {
        ui.label("This rotor has no pump cells.");
    }
}

fn pump_map(ui: &mut egui::Ui, w: &World, s: &Shaft, pump: &PumpRef) {
    let st = &w.st;
    let net = &w.nets[pump.net];
    let rho = if pump.prop == 0 { st.p().rho_f } else { st.p().rho_o };
    let wr = s.omega_rated;
    ui.strong(format!(
        "{} pump: {:.0} cm impeller, {:.0} stage{}, tip {:.0} m/s now",
        match pump.prop {
            0 => "Fuel",
            1 => "Oxidiser",
            _ => "Unconnected",
        },
        pump.d * 100.0,
        pump.stages,
        if pump.stages > 1.0 { "s" } else { "" },
        pump.tip(s.omega)
    ));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(320.0), 170.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, egui::Color32::from_gray(16));
    let qmax = pump.bep_flow(wr, rho) * 2.5;
    let hmax = pump.head_at(wr, 0.0, rho) * 1.15;
    let inner = rect.shrink2(egui::vec2(28.0, 14.0));
    let to = |m: f32, h: f32| egui::pos2(inner.left() + (m / qmax).min(1.0) * inner.width(), inner.bottom() - (h / hmax).clamp(0.0, 1.0) * inner.height());
    p.line_segment([inner.left_bottom(), inner.right_bottom()], egui::Stroke::new(1.0, egui::Color32::GRAY));
    p.line_segment([inner.left_bottom(), inner.left_top()], egui::Stroke::new(1.0, egui::Color32::GRAY));
    let curve = |omega: f32, cav: f32| -> Vec<egui::Pos2> {
        (0..=40)
            .filter_map(|k| {
                let m = qmax * k as f32 / 40.0;
                let h = pump.head_at(omega, m, rho) * cav;
                (h >= 0.0).then(|| to(m, h))
            })
            .collect()
    };
    for f in [0.6f32, 0.8, 1.0] {
        let pts = curve(f * wr, 1.0);
        if let Some(&start) = pts.first() {
            p.text(start + egui::vec2(-26.0, 0.0), egui::Align2::LEFT_CENTER, format!("{:.0}%", f * 100.0), egui::FontId::proportional(10.0), egui::Color32::GRAY);
        }
        p.add(egui::Shape::line(pts, egui::Stroke::new(1.0, egui::Color32::from_gray(if f == 1.0 { 120 } else { 70 }))));
    }
    // best-efficiency line
    let bep: Vec<egui::Pos2> = (1..=10).map(|k| {
        let om = wr * k as f32 / 10.0;
        let m = pump.bep_flow(om, rho);
        to(m, pump.head_at(om, m, rho))
    }).collect();
    p.add(egui::Shape::dashed_line(&bep, egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 160, 90)), 4.0, 3.0));
    // overall efficiency at the current speed (orange, 0-100% up the same height)
    if s.omega > 10.0 {
        let eff: Vec<egui::Pos2> = (1..=40)
            .map(|k| {
                let m = qmax * k as f32 / 40.0;
                to(m, pump.efficiency(s.omega, m, rho) * hmax)
            })
            .collect();
        p.add(egui::Shape::dashed_line(&eff, egui::Stroke::new(1.0, egui::Color32::from_rgb(230, 150, 60)), 3.0, 3.0));
    }
    let col = if pump.cav < 0.95 { egui::Color32::YELLOW } else { egui::Color32::from_rgb(110, 200, 255) };
    p.add(egui::Shape::line(curve(s.omega, pump.cav), egui::Stroke::new(2.0, col)));
    let op = to(net.mdot, pump.head.max(0.0));
    p.circle_filled(op, 5.0, egui::Color32::from_rgb(255, 90, 60));
    p.text(inner.right_bottom() + egui::vec2(0.0, 2.0), egui::Align2::RIGHT_TOP, format!("flow kg/s (max {:.0})", qmax), egui::FontId::proportional(10.0), egui::Color32::GRAY);
    p.text(inner.left_top(), egui::Align2::LEFT_TOP, format!("head (max {:.0} bar)   green: best efficiency   orange: efficiency", hmax / 1e5), egui::FontId::proportional(10.0), egui::Color32::GRAY);
    ui.label(format!(
        "flow {:.1} kg/s (best {:.1})  head {:.0} bar  eff {:.0}%  power {:.1} MW",
        net.mdot,
        pump.bep_flow(s.omega, rho),
        pump.head / 1e5,
        pump.eff * 100.0,
        pump.power / 1e6
    ));
    let m = net.mdot;
    let rec = pump.recirculation(s.omega, m, rho) * (0.3 + 0.7 * pump.cav);
    let disc = pump.disc_friction(s.omega, if pump.wet { rho } else { 0.01 * rho });
    ui.label(format!(
        "power: {:.2} MW into the liquid, {:.2} MW recirculation (below design flow), {:.2} MW disc friction{}",
        (pump.power - rec - disc).max(0.0) / 1e6,
        rec / 1e6,
        disc / 1e6,
        if pump.wet { "" } else { "  (DRY: no liquid has reached it)" }
    ));
    ui.label(format!(
        "suction: tank {:.1} bar, NPSP available {:.1} bar / required {:.1} bar (rises with flow and speed), head kept {:.0}%{}",
        net.p_tank / 1e5,
        pump.npsp_avail / 1e5,
        pump.npsp_req / 1e5,
        pump.cav * 100.0,
        if pump.cav < 0.95 { "  -> CAVITATING" } else { "" }
    ));
    ui.label(format!("discharge {:.0} bar, line peak {:.0} bar", net.p_up / 1e5, net.p_line / 1e5));
}

#[cfg(test)]
mod tests {
    use super::force_text;

    #[test]
    fn thrust_shows_in_the_unit_that_fits() {
        assert_eq!(force_text(5.8e6), ("5.80".to_string(), "MN"));
        assert_eq!(force_text(2.4e5), ("240".to_string(), "kN"));
        assert_eq!(force_text(2.4e4), ("24.0".to_string(), "kN"));
        assert_eq!(force_text(850.0), ("850".to_string(), "N"));
        assert_eq!(force_text(-3.2e4), ("-32.0".to_string(), "kN"));
    }
}
