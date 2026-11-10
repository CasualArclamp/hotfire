//! Parametric engine builder and the preset engines.

use crate::world::*;
use std::f32::consts::PI;

/// Presets are laid out on a 256 x 384 frame and then moved here: centred, 40 rows from the top.
pub const PRESET_OX: usize = (W - 256) / 2;
pub const PRESET_OY: usize = 40;

#[derive(Clone)]
pub struct TpSpec {
    pub pump_d: i32, // impeller diameter, cells
    pub stages: i32,
    pub turb_d: i32, // turbine wheel diameter, cells
    pub turb_l: i32, // turbine length along the shaft, cells
}

#[derive(Clone)]
pub struct EngineSpec {
    pub name: &'static str,
    pub blurb: &'static str,
    pub prop: usize,
    pub gg: bool,    // gas generator drives the turbines (otherwise regen heat does)
    pub ch_hw: f32,
    pub conv_start: f32,
    pub throat_y: f32,
    pub throat_hw: f32,
    pub exit_y: f32,
    pub exit_hw: f32,
    pub conical: bool,
    pub liner_mat: u8,
    pub liner: f32,
    pub jacket: f32,
    pub shell: f32,
    pub fuel_tp: Option<TpSpec>,
    pub ox_tp: Option<TpSpec>,
    pub ox_tank_bottom: i32,
    pub fuel_tank: (i32, i32, i32, i32),
    pub he: (i32, i32, i32, i32),
    pub inj: Option<(f32, f32)>, // fixed injector areas, otherwise auto-sized for target_pc
    pub target_pc: f32,
    pub turb_mult: f32,
    pub cartridge_w: f32,
    pub gg_area: (f32, f32),
    pub twin_shaft: bool, // two-shaft powerhead, drive gas through the fuel then the oxidiser turbine (J-2 / RS-25 style)
    pub single_shaft: bool, // one turbopump powerhead: oxidiser pump, turbine and fuel pump on one shaft, gas generator on the turbine
    pub gg_dump: u8, // gas generator exhaust: 0 overboard, 1 into the nozzle extension, 2 staged combustion (into the chamber)
    pub gg_size: (i32, i32), // gas generator combustion cavity: width, height in cells (1 cm each)
    pub tank_p: (f32, f32),
    pub ox_trim: f32, // mixture-ratio trim on the auto-sized oxidiser injectors
    pub dx: f32,      // metres per cell (the layout is drawn in cells: a bigger cell is a bigger engine)
    pub depth: f32,   // depth of the 2D slab, m (with the cell width, sets the throat area)
    pub feed_p_max: f32, // turbine bypass holds pump discharge under this (with fixed injector areas)
    pub ox_head: f32,    // one-shaft powerhead: oxidiser impeller sized for this times the fuel pump's head
    pub electric: bool,  // pumps driven by an electric motor (the 'turbine' block) with a battery pack beside it
    pub tri: bool,       // a second fuel on its own tank, pressure-fed through a regulator, into its own injector manifold
    pub tri_p: f32,      // ... held at this pressure, Pa
    pub tri_inj: f32,    // ... and its injector area, cm2 a cell
    pub burner: bool,    // the gas generator is a burner block (a machine drawn as a cutaway) instead of an injector plate
    pub plug: bool,      // a linear aerospike (plug nozzle): two slot chambers either side of a central spike, instead of a bell
    pub stage: bool,     // laid out like a stage: both tanks and the helium bottle stacked on the centre line above the engine
    pub ox_top: bool,    // ... with the oxidiser tank the upper one (kerosene stages; hydrogen stages carry the fuel on top)
    pub mix: f32,        // mixture ratio the throttle valves hold (0: the layout's own rule)
    pub t_gg: f32,       // turbine drive gas temperature the powerhead maker sizes the generator for, K
    pub tapoff: bool,    // tap-off cycle: no generator, the turbine runs on hot gas ducted from the main chamber
    pub twin_noz: f32,   // one chamber, two nozzles: each nozzle's centre line this many cells either side of the engine's (0: one nozzle)
}

/// Where a layout wants its propellants delivered: the columns (two cells wide) of its fuel and oxidiser inlet pipes at
/// the top row of the engine's part of the frame, and a column for a helium line if it has a tank of its own to pressurise.
#[derive(Default)]
pub struct Feeds {
    pub fuel: i32,
    pub ox: i32,
    pub he_tap: Option<i32>,
    pub top: i32, // the inlet pipes start on this row, and nothing of the engine is above it under the tank stack
    pub outlets: Vec<Rect>, // gas outlets to put on the back layer (over the back pipes already listed)
    pub reopen: Vec<Rect>,  // gas cells to open again once the plumbing has been walled in (the intake of a gas line)
}

type Rect = (i32, i32, i32, i32);


pub fn presets() -> Vec<EngineSpec> {
    let hydrolox = EngineSpec {
        stage: true,
        ox_top: false,
        mix: 0.0,
        t_gg: 1000.0,
        tapoff: false,
        twin_noz: 0.0,
        name: "Hydrolox expander",
        blurb: "LH2/LOX, regen jacket heat drives two turbopumps (RL10 / RS-25 style), big vacuum bell",
        prop: 0,
        gg: false,
        ch_hw: 22.0,
        conv_start: 100.0,
        throat_y: 140.0,
        throat_hw: 8.0,
        exit_y: 330.0,
        exit_hw: 90.0,
        conical: false,
        liner_mat: c::COPPER,
        liner: 2.0,
        jacket: 2.0,
        shell: 2.0,
        fuel_tp: Some(TpSpec { pump_d: 24, stages: 2, turb_d: 24, turb_l: 20 }), // (the powerhead maker sizes these)
        ox_tp: Some(TpSpec { pump_d: 22, stages: 1, turb_d: 26, turb_l: 3 }),
        ox_tank_bottom: 25,
        fuel_tank: (6, 150, 30, 180),
        he: (170, 8, 200, 24),
        inj: None,
        target_pc: 6.5e6, // (what the jacket's heat carries it to)
        turb_mult: 14.0,
        cartridge_w: 2.0e7,
        gg_area: (0.5, 1.5),
        gg_size: (16, 30),
        gg_dump: 0,
        single_shaft: false,
        twin_shaft: false,
        tank_p: (4.0e5, 4.5e5),
        ox_trim: 1.0,
        dx: DX0,
        depth: 0.36,
        feed_p_max: 2.6e7,
        ox_head: 1.2,
        electric: false,
        tri: false,
        tri_p: 0.0,
        tri_inj: 0.0,
        burner: false,
        plug: false,
    };
    let hydrolox_gg = EngineSpec {
        name: "J-2",
        blurb: "Saturn V upper stages (Rocketdyne J-2): LH2/LOX gas generator, two separate turbopumps with the drive gas going through the fuel turbine and then the oxidiser turbine, then overboard. Real: 53 bar, mixture 5.5, 486 kN at sea level, 1033 kN in vacuum. A vacuum engine: try it with Vacuum.",
        gg: true,
        cartridge_w: 3.0e7,
        gg_area: (3.1, 0.61), // gas generator injector areas, cm2
        gg_size: (12, 26), // combustion cavity, cm (J-2 unit, about 16 x 34 cm outside)
        single_shaft: false, // hydrogen and oxygen pumps can't share a shaft speed (J-2 / RS-25 use two)
        twin_shaft: true,
        inj: None, // powerhead and injectors sized for the real engine's chamber pressure
        target_pc: 5.3e6,
        burner: true,
        plug: false,
        ..hydrolox_clone()
    };
    let kerolox = EngineSpec {
        stage: true,
        ox_top: true,
        mix: 2.36,
        t_gg: 900.0,
        tapoff: false,
        twin_noz: 0.0,
        name: "Merlin 1D",
        blurb: "Falcon 9 first stage (SpaceX Merlin 1D): RP-1/LOX gas generator, one turbopump powerhead (oxidiser pump, turbine, fuel pump on one shaft), generator exhaust dumped overboard, regen-cooled. Real: 97 bar, mixture 2.36, 845 kN at sea level, Isp 282 s; throttles to about 57%.",
        prop: 2,
        gg: true,
        ch_hw: 16.0,
        conv_start: 90.0,
        throat_y: 115.0,
        throat_hw: 7.0,
        exit_y: 230.0,
        exit_hw: 42.0,
        conical: false,
        liner_mat: c::COPPER,
        liner: 2.0,
        jacket: 2.0,
        shell: 2.0,
        fuel_tp: Some(TpSpec { pump_d: 14, stages: 1, turb_d: 24, turb_l: 4 }),
        ox_tp: Some(TpSpec { pump_d: 19, stages: 1, turb_d: 30, turb_l: 3 }),
        ox_tank_bottom: 25,
        fuel_tank: (6, 140, 30, 185),
        he: (170, 8, 200, 24),
        inj: None,
        target_pc: 9.7e6,
        turb_mult: 14.0,
        cartridge_w: 3.0e7,
        gg_area: (2.3, 0.75), // (the powerhead maker sets these)
        gg_size: (8, 22), // Merlin class unit
        gg_dump: 0,
        single_shaft: true,
        twin_shaft: false,
        tank_p: (4.5e5, 4.5e5),
        ox_trim: 1.0,
        dx: DX0,
        depth: 0.36,
        feed_p_max: 2.6e7,
        ox_head: 1.4, // a bigger oxidiser impeller: Merlin burns oxidiser-heavy (2.36)
        electric: false,
        tri: false,
        tri_p: 0.0,
        tri_inj: 0.0,
        burner: true,
        plug: false,
    };
    let hypergolic = EngineSpec {
        stage: true,
        ox_top: false,
        mix: 0.0,
        t_gg: 1000.0,
        tapoff: false,
        twin_noz: 0.0,
        name: "Hypergolic pressure-fed",
        blurb: "MMH/N2O4 (AJ10 / OMS style): helium-pressurised tanks, no pumps, ablative ceramic chamber, long nozzle",
        prop: 3,
        gg: false,
        ch_hw: 14.0,
        conv_start: 85.0,
        throat_y: 105.0,
        throat_hw: 5.0,
        exit_y: 300.0,
        exit_hw: 70.0,
        conical: false,
        liner_mat: c::CERAMIC,
        liner: 3.0,
        jacket: 0.0,
        shell: 2.0,
        fuel_tp: None,
        ox_tp: None,
        ox_tank_bottom: 45,
        fuel_tank: (6, 110, 40, 200),
        he: (165, 8, 205, 40),
        inj: None,
        target_pc: 1.2e6,
        turb_mult: 12.0, // cooling jacket heat scale (2D slab to round chamber)
        cartridge_w: 0.0,
        gg_area: (0.5, 1.5),
        gg_size: (16, 30),
        gg_dump: 0,
        single_shaft: false,
        twin_shaft: false,
        tank_p: (4.0e5, 4.5e5),
        ox_trim: 1.0,
        dx: DX0,
        depth: 0.36,
        feed_p_max: 2.6e7,
        ox_head: 1.2,
        electric: false,
        tri: false,
        tri_p: 0.0,
        tri_inj: 0.0,
        burner: false,
        plug: false,
    };
    let dump_to_nozzle = EngineSpec {
        name: "Hydrolox GG, exhaust into nozzle",
        blurb: "Open cycle, F-1 style: the gas generator's exhaust is ducted down beside the bell into a slot at the top of a radiation-cooled niobium nozzle extension, lining its wall",
        gg: true,
        cartridge_w: 4.0e7,
        gg_area: (5.8, 1.14),
        gg_size: (12, 26),
        gg_dump: 1,
        single_shaft: false,
        twin_shaft: true,
        inj: None,
        target_pc: 1.0e7,
        burner: true,
        plug: false,
        ..hydrolox_clone()
    };
    let staged = EngineSpec {
        name: "Hydrolox staged combustion",
        blurb: "Closed cycle, RS-25 style: a fuel-rich preburner drives the turbines and its exhaust goes through a hot-gas injector into the main chamber, where its leftover fuel burns",
        gg: true,
        cartridge_w: 4.0e7,
        gg_size: (12, 20),
        gg_dump: 2,
        gg_area: (12.0, 2.3), // preburner injector areas, cm2
        single_shaft: false,
        twin_shaft: true,
        inj: Some((0.95, 2.8)), // oxidiser injectors trimmed for the two-shaft powerhead's pump balance
        feed_p_max: 1.7e7, // the turbine bypass holds pump discharge here, so the chamber pressure can't creep up
        burner: true,
        plug: false,
        ..hydrolox_clone()
    };
    let f1 = EngineSpec {
        name: "F-1",
        blurb: "Saturn V first stage (Rocketdyne F-1), the biggest single-chamber liquid engine flown: RP-1/LOX gas generator and one huge turbopump on one shaft, drawn at 2.5 cm a cell (2.5x the Merlin). Real: 70 bar, mixture 2.27, 6.77 MN at sea level, Isp 263 s, 41 MW turbine at 5500 rpm.",
        target_pc: 7.0e6,
        fuel_tp: Some(TpSpec { pump_d: 20, stages: 1, turb_d: 30, turb_l: 4 }),
        ox_tp: Some(TpSpec { pump_d: 19, stages: 1, turb_d: 30, turb_l: 3 }),
        cartridge_w: 2.0e8,
        gg_area: (24.0, 7.9),
        inj: None,
        mix: 2.27,
        feed_p_max: 9.2e6,
        ox_head: 1.2,
        electric: false,
        tri: false,
        tri_p: 0.0,
        tri_inj: 0.0,
        burner: true,
        plug: false,
        tank_p: (4.5e5, 6.0e5),
        dx: 0.025,
        depth: 1.8,
        ..kerolox.clone()
    };
    let rl10 = EngineSpec {
        name: "RL10",
        blurb: "Centaur / Delta upper stage (Aerojet Rocketdyne RL10), the classic expander: no gas generator or preburner, the hydrogen warmed in the cooling jacket is piped through the turbines and drives the pumps. Small: a 8 cm deep engine. Real: 42 bar, mixture 5.5, 99 kN and Isp 451 s in vacuum (try Vacuum). The real one drives its oxidiser pump from the same turbine through a gearbox; here each pump has its own turbine on the jacket's hydrogen line.",
        fuel_tp: Some(TpSpec { pump_d: 10, stages: 2, turb_d: 14, turb_l: 11 }), // two-stage turbine
        ox_tp: Some(TpSpec { pump_d: 8, stages: 1, turb_d: 16, turb_l: 3 }), // (the wheel reaches up from the injector head past its inlet pipe)
        inj: Some((0.065, 0.45)), // injector areas, cm2 a cell
        target_pc: 4.2e6,
        cartridge_w: 1.0e6, // an expander needs no start cartridge: a small spin-up only
        depth: 0.08,
        feed_p_max: 7.0e6,
        ..hydrolox_clone()
    };
    let rutherford = EngineSpec {
        name: "Rutherford",
        blurb: "Rocket Lab Electron (Rutherford): RP-1/LOX with electric turbopumps and no gas generator at all. A brushless electric motor spins both pumps from a lithium battery pack (the pack is bolted to the motor). Real: about 25 kN and Isp 311 s at sea level, two 50 hp motors, a pack that gives over 1 MW for nine engines. A small engine: bigger ones would need more battery than they could carry.",
        prop: 2,
        gg: false,
        ch_hw: 8.0,
        conv_start: 100.0,
        throat_y: 125.0,
        throat_hw: 2.5,
        exit_y: 215.0,
        exit_hw: 18.0,
        fuel_tp: Some(TpSpec { pump_d: 6, stages: 1, turb_d: 14, turb_l: 41 }), // turb_* is the motor here
        ox_tp: Some(TpSpec { pump_d: 8, stages: 1, turb_d: 6, turb_l: 3 }),
        inj: None,
        target_pc: 8.6e6,
        turb_mult: 8.0,
        cartridge_w: 0.0,
        tank_p: (4.5e5, 4.5e5),
        dx: 0.005,
        depth: 0.08,
        feed_p_max: 1.4e7,
        ox_head: 1.0,
        single_shaft: true,
        electric: true,
        tri: false,
        tri_p: 0.0,
        tri_inj: 0.0,
        burner: false,
        plug: false,
        ..hydrolox_clone()
    };
    let tripropellant = EngineSpec {
        name: "Tripropellant",
        blurb: "LOX burning two fuels at once, methane and liquid hydrogen (the idea behind the Russian RD-701 and some Glushko studies, which paired kerosene with hydrogen): methane carries the bulk and cools the chamber, a little hydrogen lightens the exhaust and lifts the specific impulse. Electric pumps for the oxygen and the methane; the hydrogen is pressure-fed at 95 bar from its own helium-regulated tank into its own injector manifold. Methane and hydrogen are separate lines: fill each tank with the Fill tool.",
        prop: 1,
        gg: false,
        ch_hw: 14.0,
        conv_start: 100.0,
        throat_y: 125.0,
        throat_hw: 3.0,
        exit_y: 215.0,
        exit_hw: 20.0,
        fuel_tp: Some(TpSpec { pump_d: 9, stages: 3, turb_d: 14, turb_l: 41 }), // turb_* is the motor
        ox_tp: Some(TpSpec { pump_d: 9, stages: 1, turb_d: 6, turb_l: 3 }),
        inj: Some((0.14, 1.0)),
        target_pc: 6.0e6,
        turb_mult: 8.0,
        cartridge_w: 0.0,
        tank_p: (4.5e5, 4.5e5),
        dx: 0.005,
        depth: 0.08,
        feed_p_max: 1.2e7,
        ox_head: 1.0,
        single_shaft: true,
        electric: true,
        tri: true,
        tri_p: 9.5e6,
        tri_inj: 0.09,
        burner: false,
        plug: false,
        ..hydrolox_clone()
    };
    // A linear aerospike (as on the X-33's XRS-2200): the exhaust leaves two throat slots either side of a central spike and
    // expands against the spike's ramp, with only the open air for an outer wall, so the nozzle follows the altitude. Pressure-fed
    // hypergolic, like the OMS-style engine, so it needs no pumps or igniter.
    let aerospike = EngineSpec {
        name: "Aerospike",
        stage: false, // (its own layout is a stack already)
        blurb: "Linear aerospike (plug nozzle, XRS-2200 style): MMH/N2O4 pressure-fed, two slot chambers either side of a central spike; the exhaust expands against the spike's ramp, so it adjusts to the altitude",
        plug: true,
        conv_start: 85.0,
        throat_y: 105.0,
        throat_hw: 5.0,
        exit_y: 300.0,
        ..hypergolic.clone()
    };
    // The same aerospike with electric turbopumps inside the spike, the engine's tail: an oxidiser pump, a motor and a fuel pump on
    // one shaft down the middle, and a battery pack beside the motor, all in the steel of the spike. The tanks are low pressure,
    // the pumps raise the feed to the chamber's, so it runs at a higher chamber pressure than the pressure-fed one.
    let aerospike_pumped = EngineSpec {
        name: "Aerospike, pump-fed",
        blurb: "The linear aerospike with electric pumps inside the spike (the engine's tail): oxidiser pump, motor and fuel pump on one shaft down the middle, a battery pack beside the motor. Low-pressure tanks, a higher chamber pressure than the pressure-fed one",
        electric: true,
        fuel_tp: Some(TpSpec { pump_d: 6, stages: 1, turb_d: 10, turb_l: 6 }), // turb_* is the motor here
        ox_tp: Some(TpSpec { pump_d: 6, stages: 1, turb_d: 10, turb_l: 6 }),
        target_pc: 2.0e6,
        feed_p_max: 3.2e6,
        tank_p: (4.5e5, 4.5e5),
        ..aerospike.clone()
    };
    // And with a gas generator: the turbopumps and the generator are inside the spike, and the generator's exhaust, after the
    // turbine, runs down a duct through the spike and out of its truncated base, where it fills the wake behind the spike (base
    // bleed, as on the XRS-2200). Pressure-fed hypergolic, so the generator lights on contact, and it starts on tank head.
    let aerospike_gg = EngineSpec {
        name: "Aerospike, gas generator",
        blurb: "The linear aerospike with a gas-generator turbopump inside the spike: oxidiser pump, turbine and fuel pump on one shaft, the generator bolted to the turbine, and its exhaust ducted down through the spike and out of the truncated base (base bleed, as on the XRS-2200). Starts on tank head",
        gg: true,
        fuel_tp: Some(TpSpec { pump_d: 6, stages: 1, turb_d: 10, turb_l: 14 }),
        ox_tp: Some(TpSpec { pump_d: 6, stages: 1, turb_d: 10, turb_l: 14 }),
        target_pc: 1.5e6,
        feed_p_max: 2.6e6,
        tank_p: (4.5e5, 4.5e5),
        gg_area: (1.2, 0.23), // the generator runs very fuel-rich (O/F about 0.5, near 1000 K)
        ox_trim: 0.9,
        ..aerospike.clone()
    };
    // The expander cycle as the textbook diagram draws it: one turbopump, the fuel pump, the turbine and the oxidiser pump on a
    // single shaft, the turbine driven by the fuel the regen jacket has warmed, which then goes on to the injectors.
    let hydrolox_exp1 = EngineSpec {
        name: "Hydrolox expander, one turbopump",
        blurb: "The closed expander cycle as in the textbook diagram: fuel pump, turbine and oxidiser pump on one shaft, the turbine driven by the hydrogen the regen jacket has heated, which then goes to the injectors",
        single_shaft: true,
        twin_shaft: false,
        ox_head: 3.0,
        turb_mult: 30.0,
        fuel_tp: Some(TpSpec { pump_d: 24, stages: 2, turb_d: 24, turb_l: 20 }),
        ox_tp: Some(TpSpec { pump_d: 22, stages: 1, turb_d: 24, turb_l: 20 }),
        ..hydrolox.clone()
    };
    // The tap-off cycle (J-2S, Blue Origin BE-3): no gas generator at all. Hot gas is bled from the main chamber, at the wall
    // just under the injector face, down a ceramic-lined duct and through a nozzle onto the turbine, then overboard.
    let tapoff = EngineSpec {
        name: "Tap-off cycle",
        blurb: "Combustion tap-off (Rocketdyne J-2S, Blue Origin BE-3 style), here on RP-1/LOX: no gas generator or preburner. Hot gas is tapped from the main chamber just under the injector face, down a ceramic-lined duct and through a nozzle onto the turbine of a one-shaft turbopump, then dumped overboard. A start cartridge spins the pumps up; once the chamber is lit it drives its own pumps",
        gg: false,
        tapoff: true,
        burner: false,
        target_pc: 6.0e6,
        mix: 2.3,
        cartridge_w: 1.5e7,
        ..kerolox.clone()
    };
    // One chamber, two nozzles: a wide combustion chamber whose floor is split by a cooled wedge into two throats, each with
    // its own bell. One turbopump and one injector head feed both.
    let twin_nozzle = EngineSpec {
        name: "Twin-nozzle engine",
        blurb: "One combustion chamber, two nozzles: a wide chamber whose floor is split by a regen-cooled wedge into two throats, each with its own bell, on RP-1/LOX. One gas-generator turbopump from the powerhead maker and one injector head (fuel and oxidiser from alternate injectors right across the face) feed both. Two small bells in place of one big one make a shorter engine for the same thrust",
        twin_noz: 35.0,
        ch_hw: 37.0,
        conv_start: 68.0, // (a long, gentle convergence: any steeper and the cooling jacket's cells no longer touch row to row)
        throat_y: 122.0,
        throat_hw: 6.0,
        exit_y: 225.0,
        exit_hw: 26.0,
        target_pc: 6.0e6,
        mix: 2.36,
        ..kerolox.clone()
    };
    vec![hydrolox, hydrolox_gg, kerolox, hypergolic, dump_to_nozzle, staged, f1, rl10, rutherford, tripropellant, aerospike, aerospike_pumped, aerospike_gg, hydrolox_exp1, tapoff, twin_nozzle]
}

fn hydrolox_clone() -> EngineSpec {
    let mut v = presets_base();
    v.fuel_tp = Some(TpSpec { pump_d: 24, stages: 2, turb_d: 24, turb_l: 12 });
    v.ox_tp = Some(TpSpec { pump_d: 22, stages: 1, turb_d: 26, turb_l: 3 });
    v
}

fn presets_base() -> EngineSpec {
    EngineSpec {
        stage: true,
        ox_top: false,
        mix: 0.0,
        t_gg: 1000.0,
        tapoff: false,
        twin_noz: 0.0,
        name: "",
        blurb: "",
        prop: 0,
        gg: false,
        ch_hw: 22.0,
        conv_start: 100.0,
        throat_y: 140.0,
        throat_hw: 8.0,
        exit_y: 330.0,
        exit_hw: 90.0,
        conical: false,
        liner_mat: c::COPPER,
        liner: 2.0,
        jacket: 2.0,
        shell: 2.0,
        fuel_tp: None,
        ox_tp: None,
        ox_tank_bottom: 25,
        fuel_tank: (6, 150, 30, 180),
        he: (170, 8, 200, 24),
        inj: Some((0.95, 4.5)),
        target_pc: 1.4e7,
        turb_mult: 14.0,
        cartridge_w: 2.0e7,
        gg_area: (0.5, 1.5),
        gg_size: (16, 30),
        gg_dump: 0,
        single_shaft: false,
        twin_shaft: false,
        tank_p: (4.0e5, 4.5e5),
        ox_trim: 1.0,
        dx: DX0,
        depth: 0.36,
        feed_p_max: 2.6e7,
        ox_head: 1.2,
        electric: false,
        tri: false,
        tri_p: 0.0,
        tri_inj: 0.0,
        burner: false,
        plug: false,
    }
}

impl World {
    /// Gas-generator cycle as one turbopump powerhead (like the textbook diagram, or Merlin): oxidiser pump,
    /// turbine and fuel pump on one vertical shaft right of the chamber, the gas generator bolted to the
    /// turbine and fed from both pump outlets through control valves, its exhaust driving the turbine and
    /// the turbine exhaust ducted down and dumped. Returns the back-layer pipe runs to add after the shift.
    fn single_shaft_layout(&mut self, e: &EngineSpec, hw: &dyn Fn(f32) -> f32, feeds: &mut Feeds) -> Vec<(i32, i32, i32, i32)> {
        let cx = 128.0f32;
        let mut back = vec![];
        let fill_empty = |w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32, v: u8| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H && w.cells[World::idx(x as usize, y as usize)] == c::EMPTY {
                        w.cells[World::idx(x as usize, y as usize)] = v;
                    }
                }
            }
        };
        let (ftp, otp) = (e.fuel_tp.as_ref().expect("fuel turbopump"), e.ox_tp.as_ref().expect("oxidiser turbopump"));
        let len = |tp: &TpSpec| ((0.6 * (tp.pump_d * tp.stages) as f32).round() as i32).max(3);

        // the shaft: oxidiser pump on top, turbine, fuel pump below
        let xc = 185;
        // one shaft speed for both pumps: size the oxidiser impeller so its head (stages x density x tip
        // speed^2) comes out `ox_head` times the fuel pump's; the turbine wheel (the largest) sets the speed limit
        let pr = self.st.p();
        let (d_f, l_f) = (ftp.pump_d, len(ftp));
        let r_o = 0.5 * d_f as f32 * (e.ox_head * pr.rho_f * ftp.stages as f32 / (pr.rho_o * otp.stages as f32)).sqrt();
        let d_o = ((2.0 * r_o).round() as i32).max(6);
        let l_o = ((0.6 * (d_o * otp.stages) as f32).round() as i32).max(3);
        // an electric motor is long and thin (its power goes with its volume, its speed with its diameter)
        let (d_t, l_t) = (ftp.turb_d.max(otp.turb_d).max(d_f).max(d_o), if e.electric || !e.gg { ftp.turb_l } else { 12 });
        let drive = if e.electric { c::MOTOR } else { c::TURBINE };
        let (y_o, y_t) = (30, 30 + l_o);
        let (y_f, y_fe) = (y_t + l_t, y_t + l_t + l_f - 1);
        let (ox0, ox1) = (xc - d_o / 2, xc - d_o / 2 + d_o - 1);
        let (tx0, tx1) = (xc - d_t / 2, xc - d_t / 2 + d_t - 1);
        let (fx0, fx1) = (xc - d_f / 2, xc - d_f / 2 + d_f - 1);
        self.rect(ox0, y_o, ox1, y_t - 1, c::PUMP);
        self.rect(tx0, y_t, tx1, y_f - 1, drive);
        self.rect(fx0, y_f, fx1, y_fe, c::PUMP);
        // turbine casing: gas only gets in and out through the windows cut below
        fill_empty(self, tx0 - 1, y_t - 1, tx1 + 1, y_t - 1, c::STEEL);
        fill_empty(self, tx0 - 1, y_f, tx1 + 1, y_f, c::STEEL);
        fill_empty(self, tx0 - 1, y_t, tx0 - 1, y_f - 1, c::STEEL);

        // oxidiser: tank, inlet to the top of the pump, outlet from its side to the main valve and manifold
        if e.stage {
            // (from the tank stack above: straight down onto the pump)
            self.rect(xc, 0, xc + 1, y_o - 1, c::PIPE);
            feeds.ox = xc;
        } else {
            let ob = e.ox_tank_bottom;
            self.rect(108, 8, 147, ob, c::TANK_O);
            self.rect(148, 20, xc + 1, 21, c::PIPE);
            self.rect(xc, 22, xc + 1, y_o - 1, c::PIPE);
        }
        let yo_m = y_o + l_o / 2 - 1; // oxidiser outlet rows yo_m, yo_m + 1 (within the pump's length)
        self.rect(127, yo_m, ox0 - 1, yo_m + 1, c::PIPE);
        self.rect(127, yo_m, 128, 56, c::PIPE);
        self.rect(127, 57, 128, 57, c::VALVE_X);
        self.rect(127, 58, 128, 59, c::PIPE);
        self.rect(127, 58, 128, 58, c::THROTTLE);

        // gas generator bolted to the turbine's right face: injector head on the far right, combustion
        // chamber, then narrowing to a 2-cell nozzle into the turbine
        let (cy0, cy1) = (y_t, y_t + l_t - 1);
        let (gc0, gc1) = (tx1 + 12, tx1 + 27); // combustion cavity columns
        let hx = gc1 + 1; // head columns hx, hx + 1
        let yfl = y_f + 3; // main fuel line rows
        if e.electric {
            // battery pack against the motor's right face (touching cells are wired together)
            self.rect(tx1 + 1, y_t, tx1 + 59, y_f - 1, c::BATTERY);
        } else if !e.gg {
            // Closed expander cycle, as in the textbook diagram: fuel pump, turbine and oxidiser pump on one shaft. The fuel goes
            // pump -> main valve -> up the regen jacket -> (both jacket tops joined by a back-layer line over the head) the
            // turbine's left face -> out of its right face -> back layer over to the left injector manifold, and a back-layer
            // line behind the oxidiser manifold feeds the right one.
            fill_empty(self, tx1 + 1, y_t, tx1 + 1, y_f - 1, c::STEEL);
            // the jacket tops empty into stubs, not into the injector manifolds
            self.rect(100, 60, 103, 60, c::STEEL);
            self.rect(152, 60, 155, 60, c::STEEL);
            self.rect(100, 61, 101, 61, c::PIPE);
            self.rect(154, 61, 155, 61, c::PIPE);
            self.rect(98, 50, 99, 61, c::PIPE);
            self.rect(156, 50, 157, 61, c::PIPE);
            self.rect(98, 50, 99, 51, c::JOINT);
            self.rect(156, 50, 157, 51, c::JOINT);
            back.push((98, 50, 157, 51));
            // collector -> the turbine's left face
            let yc = y_t + 4;
            self.rect(158, 50, 160, 51, c::PIPE);
            self.rect(160, yc.min(50), 161, yc.max(51), c::PIPE);
            self.rect(160, yc, tx0 - 1, yc + 1, c::PIPE);
            // the turbine's right face -> a back-layer line over to the left injector manifold, and across behind the oxidiser one
            let yr = y_t + 3;
            self.rect(tx1 + 1, yr, tx1 + 2, yr + 1, c::PIPE);
            self.rect(tx1 + 3, yr, tx1 + 4, yr + 1, c::JOINT);
            self.rect(117, yr, 118, yr + 1, c::JOINT);
            back.push((117, yr, tx1 + 4, yr + 1));
            self.rect(117, yr + 2, 118, 59, c::PIPE);
            self.rect(108, 60, 109, 60, c::JOINT);
            self.rect(146, 60, 147, 60, c::JOINT);
            back.push((108, 60, 147, 60));
        } else {
        fill_empty(self, tx1 + 1, cy0 - 2, hx + 2, cy1 + 2, c::STEEL);
        self.rect(gc0, cy0, gc1, cy1, c::EMPTY);
        let (dr0, dr1) = (cy0 + 5, cy0 + 6); // nozzle rows into the turbine
        for k in 0..=6 {
            let inset = (k + 1).min(5);
            self.rect(gc0 - 1 - k, cy0 + inset, gc0 - 1 - k, cy1 - inset, c::EMPTY);
        }
        self.rect(tx1 + 1, dr0, gc0 - 1, dr1, c::EMPTY);
        self.rect(hx, cy0, hx + 1, cy1, if e.burner { c::BURNER } else { c::GG });
        if e.burner {
            // the whole chamber is the burner block (the funnel cells at its far end are its outlet)
            self.rect(gc0, cy0, gc1, cy1, c::BURNER);
        }
        // control valves: oxidiser from the pump's side, fuel teed off the main fuel line
        self.rect(ox1 + 1, yo_m, hx + 1, yo_m + 1, c::PIPE);
        self.rect(hx - 6, yo_m, hx - 5, yo_m + 1, c::VALVE_X);
        self.rect(hx, yo_m, hx + 1, cy0 - 1, c::PIPE);
        self.rect(hx, cy1 + 1, hx + 1, yfl - 1, c::PIPE);
        self.rect(hx, cy1 + 2, hx + 1, cy1 + 2, c::VALVE_X);

        // turbine exhaust: out of the left face, down a duct beside the chamber, dumped below
        let (ex0, ex1) = (tx0 - 8, tx0 - 1);
        fill_empty(self, ex0 - 2, y_t, ex1, 80, c::STEEL);
        self.rect(ex0, y_t + 2, ex1, y_t + 9, c::EMPTY);
        self.rect(ex0, y_t + 2, ex1 - 2, 82, c::EMPTY);
        }

        // fuel: tank below the powerhead, into the bottom of the fuel pump, out of its side down the far
        // right to the bottom of the bell, into the jacket on both sides (back-layer ring to the left)
        let (tk0, tk1, tky0, tky1) = (200, 236, 90, 135);
        self.rect(xc, y_fe + 1, xc + 1, 87, c::PIPE);
        if e.stage {
            // (from the tank stack: down the far right, clear of the powerhead, and back under it to the pump)
            self.rect(xc, 86, 263, 87, c::PIPE);
            self.rect(262, 0, 263, 87, c::PIPE);
            feeds.fuel = 262;
        } else {
            self.rect(tk0, tky0, tk1, tky1, c::TANK_F);
            self.rect(xc, 86, 218, 87, c::PIPE);
            self.rect(217, 86, 218, tky0 - 1, c::PIPE);
        }
        let col = 250;
        let ey = e.exit_y as i32;
        let yf = ey - 3;
        // main line: pump -> generator tee -> main valve -> joint, then on the back layer across and down
        // to the bottom of the bell, so it doesn't box in the turbine exhaust
        let jx = (hx + 6).max(230); // clear of the helium line's back-layer run down at x 226
        self.rect(fx1 + 1, yfl, jx + 1, yfl + 1, c::PIPE);
        self.rect(hx + 3, yfl, hx + 4, yfl + 1, c::VALVE_X);
        self.rect(hx + 5, yfl, hx + 5, yfl + 1, c::THROTTLE);
        self.rect(jx, yfl, jx + 1, yfl + 1, c::JOINT);
        back.push((jx, yfl, col + 1, yfl + 1));
        back.push((col, yfl, col + 1, yf + 1));
        let h = hw(yf as f32 + 0.5);
        let tx = (cx - (h + e.liner + e.jacket * 0.5)).floor() as i32; // outer jacket column on the left
        let mt = 255 - tx;
        self.rect(mt, yf, mt + 3, yf + 1, c::PIPE);
        self.rect(mt + 2, yf, mt + 3, yf + 1, c::JOINT);
        let mc = (tx - 8).max(2); // the left feed just clears the bell (it used to run out to the edge of the layout)
        self.rect(mc, yf, tx, yf + 1, c::PIPE);
        self.rect(mc, yf, mc + 1, yf + 1, c::JOINT);
        back.push((mc, yf, col + 1, yf + 1));

        // helium bottle top right, piped to both tanks through regulators (a stage carries it in the tank stack)
        let (h0, h1, h2, h3) = (245, 8, 275, 24);
        let rx = 226;
        if !e.stage {
            self.rect(h0, h1, h2, h3, c::HE_TANK);
            self.rect(149, h1 + 3, h0 - 1, h1 + 4, c::PIPE);
            self.rect(148, h1 + 3, 148, h1 + 4, c::REG);
            let hb = (h0 + h2) / 2;
            self.rect(hb, h1 - 1, hb + 1, h1 - 1, c::JOINT);
            self.rect(rx, tky0 - 2, rx + 1, tky0 - 2, c::JOINT);
            self.rect(rx, tky0 - 1, rx + 1, tky0 - 1, c::REG);
            back.extend([(hb, 0, hb + 1, h1 - 1), (rx, 0, hb + 1, 1), (rx, 0, rx + 1, tky0 - 2)]);
        }
        if e.tri {
            // the second fuel (liquid hydrogen): a small tank left of the oxidiser tank held at high pressure by
            // its own helium regulator (it takes so little flow that it needs no pump), down a line with a main
            // valve and a throttle valve into its own injector manifold (x 118..121 in the head)
            let (lx0, lx1, ly0, ly1) = (82, 104, 8, 25);
            self.rect(lx0, ly0, lx1, ly1, c::TANK_F);
            let lr = 90;
            self.rect(lr, ly0 - 2, lr + 1, ly0 - 2, c::JOINT);
            self.rect(lr, ly0 - 1, lr + 1, ly0 - 1, c::REG);
            back.push((lr, 0, lr + 1, ly0 - 2));
            if e.stage {
                feeds.he_tap = Some(lr);
            } else {
                back.push((lr, 0, rx + 1, 1));
            }
            self.rect(lx1 - 1, ly1 + 1, lx1, 47, c::PIPE);
            self.rect(lx1 - 1, 46, 120, 47, c::PIPE);
            self.rect(119, 46, 120, 59, c::PIPE);
            self.rect(119, 52, 120, 52, c::VALVE_X);
            self.rect(119, 55, 120, 55, c::THROTTLE);
        }
        back
    }

    /// Two-shaft powerhead (J-2 / RS-25 style): the fuel turbopump and the oxidiser turbopump side by side
    /// right of the chamber, the gas generator (or preburner) bolted to the fuel turbine. Drive gas goes
    /// generator -> fuel turbine -> duct -> oxidiser turbine -> out of the bottom of the oxidiser turbine,
    /// then: dumped overboard (gg_dump 0), into a slot above a niobium nozzle extension (1), or through a
    /// hot-gas injector into the main chamber (2, closed cycle).
    fn twin_shaft_layout(&mut self, e: &EngineSpec, hw: &dyn Fn(f32) -> f32, feeds: &mut Feeds) -> Vec<(i32, i32, i32, i32)> {
        let cx = 128.0f32;
        let mut back = vec![];
        let fill_empty = |w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32, v: u8| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H && w.cells[World::idx(x as usize, y as usize)] == c::EMPTY {
                        w.cells[World::idx(x as usize, y as usize)] = v;
                    }
                }
            }
        };
        let (ftp, otp) = (e.fuel_tp.as_ref().expect("fuel turbopump"), e.ox_tp.as_ref().expect("oxidiser turbopump"));
        let len = |tp: &TpSpec| ((0.6 * (tp.pump_d * tp.stages) as f32).round() as i32).max(3);
        let (y_t, l_t) = (43, 12); // both turbine wheels: rows y_t .. y_t + l_t - 1
        let yb = y_t + l_t; // first row below the turbines

        // oxidiser turbopump (left): pump on top, turbine below
        let xb = 180;
        let (d_o, l_o) = (otp.pump_d, len(otp));
        let d_tb = otp.turb_d;
        let (bo0, bo1) = (xb - d_o / 2, xb - d_o / 2 + d_o - 1);
        let (bt0, bt1) = (xb - d_tb / 2, xb - d_tb / 2 + d_tb - 1);
        let y_o = y_t - l_o;
        self.rect(bo0, y_o, bo1, y_t - 1, c::PUMP);
        self.rect(bt0, y_t, bt1, yb - 1, c::TURBINE);
        // fuel turbopump (right): turbine on top, pump below
        let xa = 228;
        let (d_f, l_f) = (ftp.pump_d, len(ftp));
        let d_ta = ftp.turb_d;
        let (at0, at1) = (xa - d_ta / 2, xa - d_ta / 2 + d_ta - 1);
        let (af0, af1) = (xa - d_f / 2, xa - d_f / 2 + d_f - 1);
        self.rect(at0, y_t, at1, yb - 1, c::TURBINE);
        self.rect(af0, yb, af1, yb + l_f - 1, c::PUMP);
        // casings: drive gas only goes in and out where the windows are cut below
        fill_empty(self, bt0 - 1, y_t - 1, at1 + 1, y_t - 1, c::STEEL);
        fill_empty(self, bt0 - 1, yb, at1 + 1, yb, c::STEEL);
        fill_empty(self, bt0 - 1, y_t, bt0 - 1, yb - 1, c::STEEL);
        fill_empty(self, bt1 + 1, y_t, at0 - 1, yb - 1, c::STEEL);

        // drive gas: generator -> 2-cell nozzle -> fuel turbine (most of the expansion); fuel turbine's left
        // face -> a wide duct -> oxidiser turbine's right face (the rest); out of the oxidiser turbine's bottom
        let (gc0, gc1) = (at1 + 12, at1 + 27);
        let hx = gc1 + 1;
        fill_empty(self, at1 + 1, y_t - 4, hx + 4, yb + 2, c::STEEL); // 4-cell walls: preburners run at hundreds of bar
        self.rect(gc0, y_t, gc1, yb - 1, c::EMPTY);
        for k in 0..=6 {
            let inset = (k + 1).min(5);
            self.rect(gc0 - 1 - k, y_t + inset, gc0 - 1 - k, yb - 1 - inset, c::EMPTY);
        }
        // nozzle into the fuel turbine: 2 cells, 1 for a staged-combustion preburner (it has to run at a
        // few times the chamber pressure, since everything after it ends up in the chamber)
        let nz1 = if e.gg_dump == 2 { y_t + 5 } else { y_t + 6 };
        self.rect(at1 + 1, y_t + 5, gc0 - 1, nz1, c::EMPTY);
        self.rect(hx, y_t, hx + 1, yb - 1, if e.burner { c::BURNER } else { c::GG });
        if e.burner {
            // the whole chamber is the burner block (the funnel cells at its far end are its outlet)
            self.rect(gc0, y_t, gc1, yb - 1, c::BURNER);
        }
        let mid = (bt1 + at0) / 2;
        self.rect(mid, y_t + 2, at0 - 1, yb - 3, c::EMPTY);
        // into the oxidiser turbine: a wide passage when it exhausts straight overboard; a nozzle when what
        // comes after it holds its outlet pressure up (3 cells for the closed cycle's chamber, 5 for the long
        // duct into the nozzle), since that's where its share of the expansion comes from
        if e.gg_dump == 2 {
            self.rect(bt1 + 1, y_t + 4, mid, y_t + 6, c::EMPTY);
        } else if e.gg_dump == 1 {
            self.rect(bt1 + 1, y_t + 3, mid, y_t + 7, c::EMPTY);
        } else {
            self.rect(bt1 + 1, y_t + 2, mid, yb - 3, c::EMPTY);
        }
        let (w0, w1) = (xb - 6, xb + 1); // the oxidiser turbine's outlet window in its casing, below it
        self.rect(w0, yb, w1, yb, c::EMPTY);

        // oxidiser: tank, inlet to the top of its pump, outlet from the pump's side to the main valve,
        // and a branch from the other side through a control valve to the generator's head
        if e.stage {
            self.rect(xb, 0, xb + 1, y_o - 1, c::PIPE);
            feeds.ox = xb;
        } else {
            let ob = e.ox_tank_bottom;
            self.rect(108, 8, 147, ob, c::TANK_O);
            self.rect(148, 20, xb + 1, 21, c::PIPE);
            self.rect(xb, 22, xb + 1, y_o - 1, c::PIPE);
        }
        let yo_m = y_o + l_o / 2 - 1;
        self.rect(127, yo_m, bo0 - 1, yo_m + 1, c::PIPE);
        self.rect(127, yo_m, 128, 56, c::PIPE);
        self.rect(127, 57, 128, 57, c::VALVE_X);
        self.rect(127, 58, 128, 59, c::PIPE);
        self.rect(127, 58, 128, 58, c::THROTTLE);
        self.rect(bo1 + 1, yo_m, hx + 1, yo_m + 1, c::PIPE);
        self.rect(hx - 6, yo_m, hx - 5, yo_m + 1, c::VALVE_X);
        self.rect(hx, yo_m, hx + 1, y_t - 1, c::PIPE);

        // fuel: tank under its pump, outlet from the pump's side: generator tee with its control valve,
        // main valve, joint, then on the back layer across and down to the bottom of the bell
        let (tk0, tk1, tky0, tky1) = (205, 251, 92, 135);
        if e.stage {
            // (from the tank stack: down the far right, clear of the preburner, and back under it to the pump)
            self.rect(xa, yb + l_f, xa + 1, 87, c::PIPE);
            self.rect(xa, 86, hx + 21, 87, c::PIPE);
            self.rect(hx + 20, 0, hx + 21, 87, c::PIPE);
            feeds.fuel = hx + 20;
        } else {
            self.rect(tk0, tky0, tk1, tky1, c::TANK_F);
            self.rect(xa, yb + l_f, xa + 1, tky0 - 1, c::PIPE);
        }
        let yfl = yb + 3;
        self.rect(hx, yb, hx + 1, yfl - 1, c::PIPE);
        self.rect(hx, yb + 1, hx + 1, yb + 1, c::VALVE_X);
        let jx = hx + 6;
        self.rect(af1 + 1, yfl, jx + 1, yfl + 1, c::PIPE);
        self.rect(hx + 3, yfl, hx + 4, yfl + 1, c::VALVE_X);
        self.rect(hx + 5, yfl, hx + 5, yfl + 1, c::THROTTLE);
        self.rect(jx, yfl, jx + 1, yfl + 1, c::JOINT);
        let col = jx + 8;
        let ey = e.exit_y as i32;
        let yf = ey - 3;
        let h = hw(yf as f32 + 0.5);
        let tx = (cx - (h + e.liner + e.jacket * 0.5)).floor() as i32;
        let mt = 255 - tx;
        self.rect(mt, yf, mt + 3, yf + 1, c::PIPE);
        self.rect(mt + 2, yf, mt + 3, yf + 1, c::JOINT);
        let lj = (tx - 8).max(2); // just clear of the bell
        self.rect(lj, yf, tx, yf + 1, c::PIPE);
        self.rect(lj, yf, lj + 1, yf + 1, c::JOINT);
        back.extend([(jx, yfl, col + 1, yfl + 1), (col, yfl, col + 1, yf + 1), (lj, yf, col + 1, yf + 1)]);

        // helium: bottle top right, piped to both tanks through regulators (a stage carries it in the tank stack)
        if !e.stage {
            let (h0, h1, h2, h3) = (hx + 14, 8, hx + 44, 24);
            self.rect(h0, h1, h2, h3, c::HE_TANK);
            self.rect(149, h1 + 3, h0 - 1, h1 + 4, c::PIPE);
            self.rect(148, h1 + 3, 148, h1 + 4, c::REG);
            let hb = (h0 + h2) / 2;
            let rx = tk0 + 4;
            self.rect(hb, h1 - 1, hb + 1, h1 - 1, c::JOINT);
            self.rect(rx, tky0 - 2, rx + 1, tky0 - 2, c::JOINT);
            self.rect(rx, tky0 - 1, rx + 1, tky0 - 1, c::REG);
            back.extend([(hb, 0, hb + 1, h1 - 1), (rx, 0, hb + 1, 1), (rx, 0, rx + 1, tky0 - 2)]);
        }

        // where the oxidiser turbine's exhaust goes
        match e.gg_dump {
            2 => {
                // closed cycle: along above the injector head to a hot-gas injector into the chamber
                // a roomy hot-gas manifold (8 rows) beside and below the turbine, so the duct itself doesn't
                // throttle the flow
                fill_empty(self, 136, yb - 9, w1 + 3, 59, c::STEEL);
                self.rect(140, yb - 5, bt0 - 2, 57, c::EMPTY);
                self.rect(bt0 - 1, yb, w1, 57, c::EMPTY);
                self.rect(140, 58, 147, 61, c::HGI);
                // the manifold cells boxed in beside the hot-gas injector (x 134..139) would be cut off from the fuel,
                // and an empty pipe is only a hole the preburner's exhaust burns through: tie them to the fuel on the
                // far side of the injector through the back pipe layer (joints at 137 and 148), and make their
                // injector row (which would never flow) steel
                for px in 134..140 {
                    self.cells[World::idx(px as usize, 61)] = c::STEEL;
                }
                self.cells[World::idx(137, 60)] = c::JOINT;
                self.cells[World::idx(148, 60)] = c::JOINT;
            }
            1 => {
                // open cycle into the nozzle: down, across below the fuel tank, down beside the bell into
                // a slot between the regen bell's exit and a niobium extension
                let (dy, dx0) = (tky1 + 6, (cx + e.exit_hw) as i32 + 27);
                let legs = [(w0, yb, w1, dy + 7), (w0, dy, dx0 + 7, dy + 7), (dx0, dy, dx0 + 7, ey + 6)];
                for &(x0, y0, x1, y1) in &legs {
                    fill_empty(self, x0 - 2, y0, x1 + 2, y1 + 2, c::STEEL);
                }
                let gx = (cx + e.exit_hw) as i32;
                let gl = (cx - e.exit_hw) as i32 - 1;
                self.rect(gl - 7, ey + 1, gl, ey + 6, c::STEEL);
                self.rect(gx, ey + 1, dx0 + 9, ey + 1, c::STEEL);
                fill_empty(self, gx + 1, ey, dx0 + 9, ey, c::STEEL);
                let ext = crate::design::NozzleSpec {
                    cx,
                    y_throat: ey + 7,
                    y_exit: ey + 97,
                    throat_hw: e.exit_hw,
                    expansion: 1.4,
                    contour: crate::design::Contour::Bell,
                    liner: 1,
                    jacket: 0,
                    shell: 0,
                    axi: false,
                    liner_mat: c::NIOBIUM,
                };
                self.build_nozzle(&ext);
                fill_empty(self, gx + 1, ey + 7, dx0 + 9, ey + 8, c::STEEL);
                for &(x0, y0, x1, y1) in &legs {
                    self.rect(x0, y0, x1, y1, c::EMPTY);
                }
                self.rect(gx, ey + 2, dx0 + 7, ey + 6, c::EMPTY);
            }
            _ => {
                // open cycle: a duct straight down, dumped overboard
                fill_empty(self, w0 - 2, yb, w1 + 2, 92, c::STEEL);
                self.rect(w0, yb, w1, 96, c::EMPTY);
            }
        }
        back
    }

    /// Open-cycle F-1 style dump: a steel duct from the right-hand gas generator's outlet, along and down
    /// beside the bell, into a slot between the regen bell's exit and a niobium nozzle extension.
    fn build_dump_duct(&mut self, e: &EngineSpec) {
        let (cw, ch) = e.gg_size;
        let (xa, top) = (158, 32);
        let steps = (cw - 4) / 2;
        let ox0 = xa + 2 + steps; // outlet's left column
        let y_out = top + ch + steps + 1; // last row of the outlet
        let ey = e.exit_y as i32;
        let gx = (128.0 + e.exit_hw) as i32; // first column right of the exit's gas
        let (dx0, dy) = (gx + 27, y_out + 7); // duct's down-leg column and cross-over row
        // gas channels, 4 cells wide
        let legs = [(ox0, y_out - 1, ox0 + 3, dy + 3), // starts inside the outlet so its walls can't seal it
                    (ox0, dy, dx0 + 3, dy + 3), (dx0, dy, dx0 + 3, ey + 4)];
        let wall_if_empty = |w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H && w.cells[World::idx(x as usize, y as usize)] == c::EMPTY {
                        w.cells[World::idx(x as usize, y as usize)] = c::STEEL;
                    }
                }
            }
        };
        for &(x0, y0, x1, y1) in &legs {
            wall_if_empty(self, x0 - 2, y0 - 2, x1 + 2, y1 + 2);
        }
        // the slot under the bell's exit: closed on the left, open into the duct on the right; the regen
        // bell's jacket ends are capped
        let gl = (128.0 - e.exit_hw) as i32 - 1; // last liner column on the left
        self.rect(gl - 7, ey + 1, gl, ey + 4, c::STEEL);
        self.rect(gx, ey + 1, dx0 + 5, ey + 1, c::STEEL);
        wall_if_empty(self, gx + 1, ey, dx0 + 5, ey);
        // niobium extension below the slot
        let ext = crate::design::NozzleSpec {
            cx: 128.0,
            y_throat: ey + 5,
            y_exit: ey + 95,
            throat_hw: e.exit_hw,
            expansion: 1.4,
            contour: crate::design::Contour::Bell,
            liner: 1,
            jacket: 0,
            shell: 0,
            axi: false,
            liner_mat: c::NIOBIUM,
        };
        self.build_nozzle(&ext);
        wall_if_empty(self, gx + 1, ey + 5, dx0 + 5, ey + 6);
        // carve the gas paths last, re-opening the generator's outlet the duct walls closed over
        for k in 0..=steps + 1 {
            let inset = (k + 1).min(steps);
            self.rect(xa + 2 + inset, top + ch + k, xa + cw + 1 - inset, top + ch + k, c::EMPTY);
        }
        for &(x0, y0, x1, y1) in &legs {
            self.rect(x0, y0, x1, y1, c::EMPTY);
        }
        self.rect(gx, ey + 2, dx0 + 3, ey + 4, c::EMPTY);
    }

    /// An expander-cycle engine: turbopumps, a cooling jacket, and nothing else to drive them.
    fn is_expander(e: &EngineSpec) -> bool {
        !e.gg && !e.electric && !e.tapoff && !e.plug && e.fuel_tp.is_some() && e.ox_tp.is_some() && e.jacket > 0.0
    }

    /// Tank rows of a stage: (upper tank, lower tank). Liquid hydrogen is so light that its tank is much the bigger.
    fn stage_tank_rows(e: &EngineSpec) -> (i32, i32) {
        let pr = &PROPS[e.prop.min(PROPS.len() - 1)];
        let fuel = if pr.rho_f < 200.0 { 78 } else { 40 };
        let ox = 46;
        if e.ox_top { (ox, fuel) } else { (fuel, ox) }
    }

    /// How many rows the tank stack takes above the engine.
    fn stage_stack_height(&self, e: &EngineSpec) -> i32 {
        let (hu, hl) = Self::stage_tank_rows(e);
        // bottle to row 17, upper tank from row 24, eight rows between the tanks, sixteen rows of feed lines under the lower one
        24 + hu + 8 + hl + 16
    }

    /// The tank stack of a stage, in the rows above the engine: the helium bottle on top, then the two propellant tanks
    /// on the centre line, each pressurised through its own regulator. The lower tank feeds straight down and along to
    /// its inlet; the upper tank's line goes behind the lower tank on the back pipe layer.
    fn stage_stack(&mut self, e: &EngineSpec, feeds: &Feeds, back: &mut Vec<Rect>) {
        let (hu, hl) = Self::stage_tank_rows(e);
        let dy = self.stage_stack_height(e);
        let (upper, lower) = if e.ox_top { (c::TANK_O, c::TANK_F) } else { (c::TANK_F, c::TANK_O) };
        let (x_u, x_l) = if e.ox_top { (feeds.ox, feeds.fuel) } else { (feeds.fuel, feeds.ox) };
        let (yu0, yu1) = (24, 24 + hu - 1);
        let (yl0, yl1) = (yu1 + 9, yu1 + 9 + hl - 1);
        self.rect(110, 4, 145, 17, c::HE_TANK);
        self.rect(98, yu0, 157, yu1, upper);
        self.rect(98, yl0, 157, yl1, lower);
        // helium to the upper tank: straight down from the bottle through a regulator (one row, so that a cell of it touches
        // both the pipe and the tank)
        self.rect(127, 18, 128, yu0 - 2, c::PIPE);
        self.rect(127, yu0 - 1, 128, yu0 - 1, c::REG);
        // helium to the lower tank: off the bottle's right end, down the right of the stack on the back layer, and through a
        // regulator onto the tank's side
        let ry = yl0 + 10;
        self.rect(146, 9, 147, 10, c::JOINT);
        self.rect(159, ry, 160, ry + 1, c::JOINT);
        self.rect(158, ry, 158, ry + 1, c::REG);
        back.push((146, 9, 160, 10));
        back.push((159, 9, 160, ry + 1));
        // a tank of the engine's own (the tripropellant's hydrogen): a helium line down the left of the stack to it
        if let Some(x) = feeds.he_tap {
            self.rect(108, 9, 109, 10, c::JOINT);
            back.push((x, 9, 109, 10));
            back.push((x, 9, x + 1, dy + 8));
        }
        // lower tank: down from the middle of its bottom, along and down to its inlet
        let (band_b, band_a, yj) = (yl1 + 3, yl1 + 7, yl1 + 11);
        self.rect(127, yl1 + 1, 128, band_a + 1, c::PIPE);
        self.rect(x_l.min(127), band_a, x_l.max(127) + 1, band_a + 1, c::PIPE);
        self.rect(x_l, band_a, x_l + 1, dy - 1, c::PIPE);
        // upper tank: a joint under it, behind the lower tank and along on the back layer, and through a joint to its inlet
        self.rect(138, yu1 + 1, 139, yu1 + 2, c::PIPE);
        self.rect(138, yu1 + 3, 139, yu1 + 4, c::JOINT);
        back.push((138, yu1 + 3, 139, band_b + 1));
        back.push((x_u.min(138), band_b, x_u.max(138) + 1, band_b + 1));
        back.push((x_u, band_b, x_u + 1, yj + 1));
        self.rect(x_u, yj, x_u + 1, yj + 1, c::JOINT);
        self.rect(x_u, yj + 2, x_u + 1, dy - 1, c::PIPE);
    }

    /// A gas-generator engine around a powerhead from the powerhead maker: the part stands right of the chamber, clear of
    /// the bell, with its two inlets brought up to the top of the engine's rows. The oxidiser leaves the part's bottom and
    /// comes round on the front pipe layer to a joint beside the chamber, then over the injector head into the oxidiser
    /// manifold. The fuel leaves beside it and goes down behind the bell on the back layer to the ring at the bottom of
    /// the cooling jacket. The turbine exhaust is dumped overboard under the part, or (gg_dump 1) ducted down beside the
    /// bell into a slot above a niobium nozzle extension.
    fn maker_layout(&mut self, e: &EngineSpec, hw: &dyn Fn(f32) -> f32, feeds: &mut Feeds, front: &mut Vec<Rect>) -> (Vec<Rect>, crate::powerhead::PhSize) {
        use crate::powerhead::{build_with, size, Layout, PhInput};
        let cx = 128.0f32;
        let mut back: Vec<Rect> = vec![];
        let fill_empty = |w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32, v: u8| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H && w.cells[World::idx(x as usize, y as usize)] == c::EMPTY {
                        w.cells[World::idx(x as usize, y as usize)] = v;
                    }
                }
            }
        };
        let inp = PhInput {
            prop: e.prop,
            pc: e.target_pc,
            throat_cells: 2.0 * e.throat_hw * if e.twin_noz > 0.0 { 2.0 } else { 1.0 },
            layout: if e.twin_shaft || (Self::is_expander(e) && !e.single_shaft) { Layout::Twin } else { Layout::Single },
            t_gg: e.t_gg,
            dx: e.dx,
            depth: e.depth,
            axi: false,
            tank_p_f: e.tank_p.0,
            tank_p_o: e.tank_p.1,
            steps: self.st.steps_per_frame,
            p_exh: 1.5e5,
            tapoff: e.tapoff,
            expander: Self::is_expander(e),
        };
        let expander = inp.expander;
        // (exhaust into the nozzle: the last wheel exhausts against its collector, a few bar)
        let s = size(&PhInput { p_exh: if e.gg_dump == 1 { 3.0e5 } else { 1.5e5 }, ..inp });
        let (clip, ports) = build_with(&s, e.burner, false);
        let (pw, ph) = (clip.w as i32, clip.h as i32);
        let (ll, n_noz) = crate::powerhead::last_wheel(&s);
        // a tap-off or expander part goes down mirrored: its turbines next to the chamber, where their gas or warm fuel comes from
        let mir = e.tapoff || expander;
        let port_y = |name: &str| ports.iter().find(|p| p.name.starts_with(name)).map(|p| p.y).unwrap();
        let port = |name: &str| {
            let x = ports.iter().find(|p| p.name.starts_with(name)).map(|p| p.x).unwrap();
            // (ports are two cells wide, the exhaust as wide as the wheel is long, the gas inlet as wide as the nozzle)
            let wd = if name.starts_with("turbine") { ll } else if name.starts_with("drive") { n_noz } else { 2 };
            if mir { pw - x - wd } else { x }
        };
        let l3 = e.liner + e.jacket + e.shell;
        // where it stands: beside the chamber, its top under the tank stack's feed lines, its left clear of the chamber wall
        // (and of the bell, as far down as its lines go), and its exhaust clear of the bell's rim
        let py = if e.tapoff { 70 - port_y("drive gas in") } else { 44 };
        feeds.top = 40;
        let off = e.twin_noz; // (two nozzles: everything outboard is that much further from the centre line)
        let xj = (cx + off + e.ch_hw + l3) as i32 + 2; // the oxidiser's joint column beside the chamber
        let mut px = xj + 6;
        for y in 58..py + ph + 8 {
            px = px.max((cx + off + hw(y as f32 + 0.5) + l3).ceil() as i32 + 6);
        }
        let ex = port("turbine exhaust");
        let gx = (cx + off + e.exit_hw) as i32; // first column right of the exit's gas
        if !expander {
            px = px.max(if e.gg_dump == 1 { gx + 22 - ex } else { gx + l3 as i32 + 8 - ex });
        }
        for k in 0..clip.w * clip.h {
            let kx = (k % clip.w) as i32;
            let (x, y) = (px + if mir { pw - 1 - kx } else { kx }, py + (k / clip.w) as i32);
            if clip.cells[k] != c::EMPTY {
                self.cells[World::idx(x as usize, y as usize)] = clip.cells[k];
            }
            if clip.back[k] == c::PIPE {
                back.push((x, y, x, y));
            }
        }
        if e.tapoff {
            // the tap: out through the chamber's right wall three rows under the injector face, along a niobium duct
            // and down through the casing's nozzle onto the wheel
            let cw = (cx + e.ch_hw) as i32; // first wall column right of the chamber's gas
            let (n0, n1) = (px + port("drive gas in"), px + port("drive gas in") + n_noz - 1);
            // (the duct is niobium, two cells thick: it runs white-hot in gas straight from the chamber and radiates the heat
            // away, where steel would melt and a ceramic lining crack under the pressure)
            self.rect(cw, 62, n1 + 2, 68, c::NIOBIUM);
            self.rect(n0 - 2, 67, n1 + 2, 69, c::NIOBIUM);
            self.rect(cw, 64, n1, 66, c::EMPTY);
            self.rect(n0, 67, n1, 69, c::EMPTY);
            // (and so are the turbine's casing and its exhaust duct: the gas is still far hotter than steel stands when it leaves)
            for y in py..py + ph {
                for x in px + ex - 2..=px + ex + ll + 1 {
                    let i = World::idx(x as usize, y as usize);
                    if self.cells[i] == c::STEEL {
                        self.cells[i] = c::NIOBIUM;
                    }
                }
            }
            // the duct cuts the cooling jacket on this side off from its injector manifold: a line behind it joins them again
            let (j0, j1) = (cw + e.liner as i32, cw + (e.liner + e.jacket) as i32 - 1);
            self.rect(j0, 69, j1, 70, c::JOINT);
            self.cells[World::idx(155, 60)] = c::JOINT;
            back.push((j0, 69, 158, 70));
            back.push((157, 60, 158, 70));
            back.push((155, 60, 158, 60));
        }
        if expander {
            // Expander plumbing. Each side of the cooling jacket is cut off from its injector manifold and ends in a joint;
            // a collector on the back layer joins the two and brings the warm fuel over to the top of the first wheel. From
            // the bottom of the last wheel it comes back beside the chamber into the right-hand manifold, and behind the
            // oxidiser manifold to the left-hand one.
            let cw = (cx + e.ch_hw) as i32; // first wall column right of the chamber's gas
            let (r0, r1) = (cw + e.liner as i32, cw + (e.liner + e.jacket) as i32 - 1);
            let (l0, l1) = (255 - r1, 255 - r0);
            for (a, b2) in [(l0, l1), (r0, r1)] {
                self.rect(a, 61, b2, 61, c::STEEL);
                self.rect(a, 62, b2, 63, c::JOINT);
                back.push((a, py, b2, 63));
            }
            let xw = px + port("warm fuel in");
            self.rect(xw, py, xw + 1, py + 1, c::JOINT);
            back.push((l0, py, xw + 1, py + 1));
            let xr = px + port("fuel to the injectors");
            let yb = py + ph - 1;
            let xret = xj + 4;
            self.rect(xr, yb, xr + 1, yb + 2, c::PIPE);
            self.rect(xret, yb + 1, xr + 1, yb + 2, c::PIPE);
            self.rect(xret, 60, xret + 1, yb + 2, c::PIPE);
            self.rect(cw + l3 as i32, 60, xret + 1, 60, c::PIPE);
            self.cells[World::idx(108, 60)] = c::JOINT;
            self.cells[World::idx(147, 60)] = c::JOINT;
            back.push((108, 60, 147, 60));
        }
        // inlets: straight up to the tank stack
        for (name, to) in [("fuel in", &mut feeds.fuel), ("oxidiser in", &mut feeds.ox)] {
            let x = px + port(name);
            self.rect(x, feeds.top, x + 1, py, c::PIPE);
            *to = x;
        }
        // oxidiser: on from the part's bottom to a joint, round in front of the fuel line to a joint beside the chamber, and
        // over the injector head into the manifold
        let xo = px + port("oxidiser out");
        let yb = py + ph - 1; // the part's bottom row (its outlets end open on it)
        self.rect(xo, yb, xo + 1, yb + 3, c::PIPE);
        self.rect(xo, yb + 4, xo + 1, yb + 5, c::JOINT);
        front.push((xj, yb + 4, xo + 1, yb + 5));
        front.push((xj, 54, xj + 1, yb + 5));
        self.rect(xj, 54, xj + 1, 55, c::JOINT);
        self.rect(127, 54, xj - 1, 55, c::PIPE);
        self.rect(127, 56, 128, 59, c::PIPE);
        // (the main oxidiser valve and its throttle sit on the injector head, as on the real engines)
        self.rect(127, 57, 128, 57, c::VALVE_X);
        self.rect(127, 58, 128, 58, c::THROTTLE);
        // fuel: a joint on the part's outlet, down behind the bell to the ring that feeds the jacket on both sides
        let xf = px + port("fuel out");
        self.rect(xf, yb, xf + 1, yb + 1, c::JOINT);
        let ey = e.exit_y as i32;
        let yf = ey - 3;
        let h = off + hw(yf as f32 + 0.5);
        let tx = (cx - (h + e.liner + e.jacket * 0.5)).floor() as i32; // outer jacket column on the left
        if off > 0.0 {
            // (two nozzles: the jackets on the two sides of the wedge are joined across the gap between the bells, and tied
            // into the ring through a joint there)
            let (i0, i1) = (2 * (128 - off as i32) - 1 - tx, tx + 2 * off as i32);
            self.rect(i0, yf, i1, yf + 1, c::PIPE);
            self.rect(127, yf, 128, yf + 1, c::JOINT);
        }
        let mt = 255 - tx;
        self.rect(mt, yf, mt + 3, yf + 1, c::PIPE);
        self.rect(mt + 2, yf, mt + 3, yf + 1, c::JOINT);
        let lj = (tx - 8).max(2);
        self.rect(lj, yf, tx, yf + 1, c::PIPE);
        self.rect(lj, yf, lj + 1, yf + 1, c::JOINT);
        // (round the outside of the bell's rim, not across behind the exhaust)
        let col = gx + l3 as i32 + 3;
        back.push((xf, yb, xf + 1, yb + 9));
        back.push((xf.min(col), yb + 8, xf.max(col) + 1, yb + 9));
        back.push((col, yb + 8, col + 1, yf + 1));
        back.push((lj, yf, col + 1, yf + 1));
        if e.gg_dump == 1 {
            // exhaust into the nozzle: a plate of pipe joints closes the duct under the last wheel and takes its gas into a
            // line on the back layer, down beside the bell and across behind the top of a radiation-cooled niobium skirt. Along
            // the inside of the skirt's wall on both sides the line is gas outlets: the exhaust comes out into the stream there,
            // moving with it, and lines the wall (nothing sticks into the jet, so nothing backs the turbines up)
            let e0 = px + ex;
            // (the collector: the duct carried on down ten rows, its sides and bottom all joints, so the line has plenty of
            // cells to draw the gas from and holds no pressure against the wheel)
            self.rect(e0 - 1, yb - 4, e0 - 1, yb + 11, c::JOINT);
            self.rect(e0 + ll, yb - 4, e0 + ll, yb + 11, c::JOINT);
            self.rect(e0 - 1, yb + 11, e0 + ll, yb + 11, c::JOINT);
            fill_empty(self, e0 - 2, yb - 5, e0 - 1, yb + 12, c::STEEL);
            fill_empty(self, e0 + ll, yb - 5, e0 + ll + 1, yb + 12, c::STEEL);
            fill_empty(self, e0 - 2, yb + 11, e0 + ll + 1, yb + 12, c::STEEL);
            for y in yb + 1..=yb + 10 {
                for x in e0..e0 + ll {
                    self.cells[World::idx(x as usize, y as usize)] = c::EMPTY;
                }
            }
            feeds.reopen.push((e0, yb - 4, e0 + ll - 1, yb + 10));
            let ext = crate::design::NozzleSpec {
                cx,
                y_throat: ey + 1,
                y_exit: ey + 91,
                throat_hw: e.exit_hw,
                // (a skirt carries on from a bell that has already turned the flow nearly straight: a shallow cone, about
                // 8 degrees, not a second bell flaring out from the joint)
                expansion: 1.0 + 0.14 * 90.0 / e.exit_hw,
                contour: crate::design::Contour::Conical,
                liner: 1,
                jacket: 0,
                shell: 0,
                axi: false,
                liner_mat: c::NIOBIUM,
            };
            self.build_nozzle(&ext);
            let (wl, wr) = (128 - e.exit_hw as i32, 127 + e.exit_hw as i32); // the skirt's first and last gas columns at its top
            back.push((e0, yb + 11, e0 + 1, ey + 4));
            back.push((wl, ey + 3, e0 + 1, ey + 4));
            feeds.outlets.push((wl, ey + 3, wl + 9, ey + 4));
            feeds.outlets.push((wr - 9, ey + 3, wr, ey + 4));
            back.push((wl, ey + 5, wl + 1, ey + 16));
            back.push((wr - 1, ey + 5, wr, ey + 16));
            feeds.outlets.push((wl, ey + 5, wl + 1, ey + 16));
            feeds.outlets.push((wr - 1, ey + 5, wr, ey + 16));
        }
        (back, s)
    }

    pub fn load_preset(&mut self, k: usize) {
        let all = presets();
        let e = &all[k.min(all.len() - 1)];
        self.build_engine(e);
        // how each engine gets its pumps turning: the J-2 and the staged engine on a gas start tank (cold gas blown
        // down through the turbines), the Merlin on helium as well, the rest on a start cartridge, a part beside
        // the turbopump sized for the power it has to give
        self.st.he_spin = matches!(k, 1 | 2 | 5); // (the Merlin's turbopump is spun up on helium too)
        if self.st.he_spin && self.add_helium_start() {
            self.st.he_spin = false; // a real helium line does it
        }
        self.st.dead_start = k == 12; // (the gas-generator aerospike starts on tank head)
        if k == 2 {
            // the Merlin lights its generator and chamber on TEA-TEB (three starts: a landing burn needs the third)
            self.add_teateb(18);
        }
        if !self.st.he_spin && !self.st.dead_start && !self.helium_start_line() && e.cartridge_w > 0.0 && !e.electric && !self.shafts_none() {
            let escale = (e.dx / DX0).powi(2) * (e.depth / 0.36);
            let cells = (e.cartridge_w / (crate::world::CART_W_CELL * escale)).ceil() as usize;
            if !self.add_cartridges(cells.max(4)) {
                self.event("(no free place beside the turbopump for a start cartridge: the start cartridge setting is used)".into());
            }
        }
    }

    /// A helium start line: a pipe from a helium bottle, through a (closed) valve, into the turbine's inlet passage.
    /// It runs on the back pipe layer, behind the rest of the engine, and comes through to the middle layer in two
    /// pipe joints: one at the bottle, and one in the steel beside the turbine's gas passage, where a valve and an
    /// injector open into it. At the start the valve opens and the cold helium goes through the turbine like any
    /// gas. Returns false when no route could be found.
    pub fn add_helium_start(&mut self) -> bool {
        self.add_gas_line(false)
    }

    /// The same kind of line from a TEA-TEB bottle into the main chamber, next to its first injector.
    pub fn add_gas_line(&mut self, tea: bool) -> bool {
        use std::collections::VecDeque;
        self.rebuild_nets();
        // the generator (or preburner): where the drive gas for the turbine comes from
        let Some(gg0) = (0..N).find(|&i| matches!(self.cells[i], c::GG | c::BURNER)) else { return false };
        if self.he_bottles.iter().all(|b| b.tea != tea) {
            return false;
        }
        let near = |a: usize, b: usize| ((a % W) as i32 - (b % W) as i32).abs() + ((a / W) as i32 - (b / W) as i32).abs();
        let Some(shaft) = self.shafts.iter().filter(|s| s.turb_cells > 0).min_by_key(|s| s.turb_faces.iter().map(|&j| near(j, gg0)).min().unwrap_or(i32::MAX)) else { return false };
        // the open air outside the engine
        let mut outside = vec![false; N];
        let mut q = VecDeque::new();
        for x in 0..W {
            for y in [0, H - 1] {
                let i = World::idx(x, y);
                if self.cells[i] == c::EMPTY && !outside[i] {
                    outside[i] = true;
                    q.push_back(i);
                }
            }
        }
        for y in 0..H {
            for x in [0, W - 1] {
                let i = World::idx(x, y);
                if self.cells[i] == c::EMPTY && !outside[i] {
                    outside[i] = true;
                    q.push_back(i);
                }
            }
        }
        let nb4 = |i: usize| -> Vec<usize> {
            let (x, y) = (i % W, i / W);
            [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))].into_iter().filter(|&(nx, ny)| nx < W && ny < H).map(|(nx, ny)| World::idx(nx, ny)).collect()
        };
        while let Some(i) = q.pop_front() {
            for j in nb4(i) {
                if self.cells[j] == c::EMPTY && !outside[j] {
                    outside[j] = true;
                    q.push_back(j);
                }
            }
        }
        // the gas passage next to the turbine's inlet: its gas cell nearest the generator, and the passage cells round it
        // (both lines end in the drive-gas passage at the turbine's inlet: the Merlin lights its generator with TEA-TEB)
        let seed = shaft.turb_faces.iter().filter(|&&j| !outside[j]).min_by_key(|&&j| near(j, gg0)).copied();
        let Some(g0) = seed else { return false };
        let mut passage = vec![g0];
        let mut seen = std::collections::HashSet::from([g0]);
        let mut k = 0;
        while k < passage.len() && passage.len() < 40 {
            let i = passage[k];
            k += 1;
            for j in nb4(i) {
                if self.cells[j] == c::EMPTY && !outside[j] && seen.insert(j) {
                    passage.push(j);
                }
            }
        }
        let plumb = |w: &World, i: usize| c::plumbing(w.cells[i]) || matches!(w.cells[i], c::REG | c::HGI | c::HE_TANK);
        // the middle-layer chain in the steel beside the passage: joint, valve, injector in a straight line
        let dirs: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let at = |x: i32, y: i32| -> Option<usize> { (x >= 1 && y >= 1 && (x as usize) < W - 1 && (y as usize) < H - 1).then(|| World::idx(x as usize, y as usize)) };
        let mut chains: Vec<(usize, usize, usize)> = vec![]; // (joint, valve, injector)
        for &gi in &passage {
            let (gx, gy) = ((gi % W) as i32, (gi / W) as i32);
            for (dx, dy) in dirs {
                // the injector cell is in the steel one step from the passage, the valve and joint continue away from it
                let cells: Vec<Option<usize>> = (1..=3).map(|n| at(gx + dx * n, gy + dy * n)).collect();
                let (Some(ci), Some(bi), Some(ai)) = (cells[0], cells[1], cells[2]) else { continue };
                if ![ci, bi, ai].iter().all(|&i| matches!(self.cells[i], c::STEEL | c::COPPER | c::CERAMIC | c::NIOBIUM)) {
                    continue;
                }
                let chain = [ci, bi, ai];
                // nothing else may touch the chain: no plumbing, no open space but the passage cell
                let clean = chain.iter().all(|&i| nb4(i).iter().all(|&m| chain.contains(&m) || m == gi || (!plumb(self, m) && self.cells[m] != c::EMPTY)));
                if clean {
                    chains.push((ai, bi, ci));
                }
            }
        }
        if chains.is_empty() {
            return false;
        }
        // the bottle end: a steel cell touching a bottle with nothing else of the plumbing round it
        let mut starts: Vec<usize> = vec![];
        let bottle_cell = |w: &World, i: usize| w.cells[i] == c::HE_TANK && (w.fluid[i] == crate::world::TEA_FLUID) == tea;
        for bc in (0..N).filter(|&i| bottle_cell(self, i)) {
            for j in nb4(bc) {
                if self.cells[j] == c::STEEL && nb4(j).iter().all(|&m| (self.cells[m] != c::EMPTY || outside[m]) && (!plumb(self, m) || self.cells[m] == c::HE_TANK)) && !starts.contains(&j) {
                    starts.push(j);
                }
            }
        }
        if starts.is_empty() {
            return false;
        }
        // the route on the back layer, from the bottle's joint to the chain's joint: anywhere free, keeping a cell clear of other back pipes
        let back_free = |w: &World, i: usize| w.back[i] == c::EMPTY && w.cells[i] != c::JOINT && nb4(i).iter().all(|&m| w.back[m] == c::EMPTY);
        chains.sort_by_key(|&(a, _, _)| starts.iter().map(|&s0| near(s0, a)).min().unwrap_or(0));
        for &(ja, vb, inj) in chains.iter().take(12) {
            let mut order = starts.clone();
            order.sort_by_key(|&s0| near(s0, ja));
            for &s0 in order.iter().take(6) {
                let mut prev = vec![usize::MAX; N];
                let mut q = VecDeque::from([s0]);
                prev[s0] = s0;
                while let Some(i) = q.pop_front() {
                    if i == ja {
                        break;
                    }
                    for j in nb4(i) {
                        if prev[j] == usize::MAX && (j == ja || back_free(self, j)) && j != vb && j != inj {
                            prev[j] = i;
                            q.push_back(j);
                        }
                    }
                }
                if prev[ja] == usize::MAX {
                    continue;
                }
                let mut path = vec![ja];
                while *path.last().unwrap() != s0 {
                    path.push(prev[*path.last().unwrap()]);
                }
                for &i in &path {
                    self.set_layer_cell(1, i, c::PIPE);
                }
                // (thicken the wall where the joint would face open air)
                for m in nb4(s0) {
                    if self.cells[m] == c::EMPTY {
                        self.set_cell(m, c::STEEL);
                    }
                }
                self.set_cell(s0, c::JOINT);
                self.set_cell(ja, c::JOINT);
                self.set_cell(vb, c::VALVE_X);
                self.set_cell(inj, c::INJ);
                self.rebuild_nets();
                return if tea { self.teateb_line() } else { self.helium_start_line() };
            }
        }
        false
    }

    /// A TEA-TEB bottle in free space beside the engine, and its line (back layer, a valve, an injector into the chamber).
    pub fn add_teateb(&mut self, cells: usize) -> bool {
        use std::collections::VecDeque;
        self.rebuild_nets();
        let mut outside = vec![false; N];
        let mut q = VecDeque::new();
        for x in 0..W {
            for y in [0, H - 1] {
                let i = World::idx(x, y);
                if self.cells[i] == c::EMPTY && !outside[i] {
                    outside[i] = true;
                    q.push_back(i);
                }
            }
        }
        for y in 0..H {
            for x in [0, W - 1] {
                let i = World::idx(x, y);
                if self.cells[i] == c::EMPTY && !outside[i] {
                    outside[i] = true;
                    q.push_back(i);
                }
            }
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % W, i / W);
            for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                if nx < W && ny < H {
                    let j = World::idx(nx, ny);
                    if self.cells[j] == c::EMPTY && !outside[j] {
                        outside[j] = true;
                        q.push_back(j);
                    }
                }
            }
        }
        // (the biggest injector set: the main chamber's, whatever its valves are doing)
        let Some(inj) = self.nets.iter().filter(|n| n.prop != crate::world::PROP_HE).max_by_key(|n| n.injectors.len()).and_then(|n| n.injectors.first().map(|&(i, _)| i)) else { return false };
        let (ix, iy) = ((inj % W) as i32, (inj / W) as i32);
        let h0 = ((cells as f32 / 2.0).sqrt().round() as i32).max(2);
        let w0 = ((cells as f32 / h0 as f32).ceil() as i32).max(2);
        // free space with a margin for the wall, as near the injector head as it goes
        let mut best: Option<(i32, i32, i32, i32, i32)> = None;
        for (w, h) in [(w0, h0), (h0, w0)] {
            for ry in (iy - 60).max(3)..=(iy + 60) {
                for rx in (ix - 90).max(3)..=(ix + 90) {
                    if rx + w + 2 >= W as i32 || ry + h + 2 >= H as i32 {
                        continue;
                    }
                    let free = (ry - 2..ry + h + 2).all(|yy| (rx - 2..rx + w + 2).all(|xx| outside[World::idx(xx as usize, yy as usize)]));
                    if !free {
                        continue;
                    }
                    // really outside the engine: a clear run along a row of it to the edge of the world (the nozzle's inside is open air too)
                    let row = ry + h / 2;
                    let clear_left = (0..rx).all(|xx| self.cells[World::idx(xx as usize, row as usize)] == c::EMPTY);
                    let clear_right = ((rx + w)..W as i32).all(|xx| self.cells[World::idx(xx as usize, row as usize)] == c::EMPTY);
                    if !(clear_left || clear_right) {
                        continue;
                    }
                    let d = (rx - ix).abs() + (ry - iy).abs();
                    if best.map_or(true, |b| d < b.4) {
                        best = Some((rx, ry, w, h, d));
                    }
                }
            }
        }
        let Some((rx, ry, w, h, _)) = best else { return false };
        for yy in ry..ry + h {
            for xx in rx..rx + w {
                let i = World::idx(xx as usize, yy as usize);
                self.set_cell(i, c::HE_TANK);
                self.fluid[i] = crate::world::TEA_FLUID;
            }
        }
        for yy in (ry - 1)..=(ry + h) {
            for xx in (rx - 1)..=(rx + w) {
                let i = World::idx(xx as usize, yy as usize);
                if self.cells[i] == c::EMPTY {
                    self.set_cell(i, c::STEEL);
                }
            }
        }
        self.rebuild_nets();
        self.add_gas_line(true)
    }

    fn shafts_none(&mut self) -> bool {
        self.rebuild_nets();
        !self.shafts.iter().any(|s| s.turb_cells > 0)
    }

    /// Put start cartridge canisters beside the turbopumps: `total` cells shared between the turbine shafts, each a
    /// rectangle of free outside space that faces the shaft (within a wall's thickness) and doesn't touch any of
    /// the engine's gas passages.
    pub fn add_cartridges(&mut self, total: usize) -> bool {
        use std::collections::VecDeque;
        self.rebuild_nets();
        // the open air outside the engine: empty cells joined to the edge of the world
        let mut outside = vec![false; N];
        let mut q = VecDeque::new();
        for x in 0..W {
            for y in [0, H - 1] {
                let i = World::idx(x, y);
                if self.cells[i] == c::EMPTY && !outside[i] {
                    outside[i] = true;
                    q.push_back(i);
                }
            }
        }
        for y in 0..H {
            for x in [0, W - 1] {
                let i = World::idx(x, y);
                if self.cells[i] == c::EMPTY && !outside[i] {
                    outside[i] = true;
                    q.push_back(i);
                }
            }
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % W, i / W);
            for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                if nx < W && ny < H {
                    let j = World::idx(nx, ny);
                    if self.cells[j] == c::EMPTY && !outside[j] {
                        outside[j] = true;
                        q.push_back(j);
                    }
                }
            }
        }
        let shafts: Vec<Vec<usize>> = self.shafts.iter().filter(|s| s.turb_cells > 0).map(|s| s.cells.clone()).collect();
        if shafts.is_empty() {
            return false;
        }
        let per = (total / shafts.len()).max(4);
        let mut rotor = vec![false; N];
        for sh in &shafts {
            for &i in sh {
                rotor[i] = true;
            }
        }
        let mut placed_all = true;
        for sh in &shafts {
            let (x0, x1) = (sh.iter().map(|&i| i % W).min().unwrap() as i32, sh.iter().map(|&i| i % W).max().unwrap() as i32);
            let (y0, y1) = (sh.iter().map(|&i| i / W).min().unwrap() as i32, sh.iter().map(|&i| i / W).max().unwrap() as i32);
            let h0 = ((per as f32 / 2.0).sqrt().round() as i32).max(2);
            let w0 = ((per as f32 / h0 as f32).ceil() as i32).max(2);
            let mut best: Option<(i32, i32, i32, i32, usize)> = None; // x, y, w, h, cells facing the shaft
            for (w, h) in [(w0, h0), (h0, w0)] {
                for ry in (y0 - h - 3)..=(y1 + 3) {
                    for rx in (x0 - w - 3)..=(x1 + 3) {
                        if rx < 1 || ry < 1 || rx + w >= W as i32 - 1 || ry + h >= H as i32 - 1 {
                            continue;
                        }
                        let mut ok = true;
                        let mut facing = 0usize;
                        'rect: for yy in ry..ry + h {
                            for xx in rx..rx + w {
                                let i = World::idx(xx as usize, yy as usize);
                                if !outside[i] {
                                    ok = false;
                                    break 'rect;
                                }
                                // a rotor cell straight out from here, at most a wall's thickness away
                                'dirs: for d in 1..=2i32 {
                                    for (ox, oy) in [(d, 0), (-d, 0), (0, d), (0, -d)] {
                                        let (nx, ny) = (xx + ox, yy + oy);
                                        if nx >= 0 && ny >= 0 && (nx as usize) < W && (ny as usize) < H && rotor[World::idx(nx as usize, ny as usize)] {
                                            facing += 1;
                                            break 'dirs;
                                        }
                                    }
                                }
                            }
                        }
                        if !ok || facing == 0 {
                            continue;
                        }
                        // keep clear of the engine's gas passages
                        for yy in (ry - 1)..=(ry + h) {
                            for xx in (rx - 1)..=(rx + w) {
                                if (xx >= rx && xx < rx + w) && (yy >= ry && yy < ry + h) {
                                    continue;
                                }
                                if xx < 0 || yy < 0 || xx >= W as i32 || yy >= H as i32 {
                                    continue;
                                }
                                let i = World::idx(xx as usize, yy as usize);
                                if self.cells[i] == c::EMPTY && !outside[i] {
                                    ok = false;
                                }
                            }
                        }
                        if ok && best.map_or(true, |b| facing > b.4) {
                            best = Some((rx, ry, w, h, facing));
                        }
                    }
                }
            }
            match best {
                Some((rx, ry, w, h, _)) => {
                    for yy in ry..ry + h {
                        for xx in rx..rx + w {
                            self.set_cell(World::idx(xx as usize, yy as usize), c::CARTRIDGE);
                        }
                    }
                }
                None => placed_all = false,
            }
        }
        self.rebuild_nets();
        placed_all
    }

    /// Draw a complete engine: chamber + nozzle walls, injector head, plumbing, tanks and turbopumps.
    /// Every valve starts closed.
    /// The linear aerospike's layout, in the preset frame (256 wide, the axis at x = 128), as a vertical stack like a real stage:
    /// the engine at the bottom (a central spike, a slot chamber either side of it with the throat between the spike's shoulder
    /// and a short cowl lip, the spike's ramp falling away below the throat to a point), and above it, inside a steel airframe
    /// that carries on up from the chamber's outer wall, the fuel tank and then the oxidiser tank on the axis, with the helium
    /// bottle on top. The oxidiser manifolds are the inner halves of the slots (against the spike), joined by a pipe over the top
    /// of the spike; the fuel manifolds are the outer halves, fed down two pipes either side of the oxidiser tank from a pipe
    /// under the fuel tank. Pressure-fed, the tanks feed straight down; pump-fed, electric pumps, a motor and a battery pack sit in
    /// the spike and both feeds run on the back layer to them. Returns the back-layer pipe runs and the regulators that need a
    /// setting of their own, as (x, y, pressure) cells.
    fn aerospike_layout(&mut self, e: &EngineSpec) -> (Vec<(i32, i32, i32, i32)>, Vec<(i32, i32, f32)>) {
        let dy: i32 = 120; // the engine sits this far down the frame, below the tanks
        let cx = 128.0f32;
        let (s0, cw, gap) = (24.0f32, 10.0f32, e.throat_hw); // spike half-width, slot width, throat gap of each slot
        let (conv, ty) = (e.conv_start + dy as f32, e.throat_y + dy as f32);
        // (a convex ramp, steep at the throat and flattening towards the axis, as on a real plug; 140 cells long and brought to
        // a point, which tried out as well as any longer or truncated spike in the simulation)
        // (with a gas generator the spike is longer and cut off where it is 7 cells either side of the axis, to leave the exhaust duct
        // an opening in the base)
        let (q, ramp_l) = if e.gg { (2.0f32, 240.0f32) } else { (2.0f32, 140.0f32) };
        let y_end = if e.gg { ty + ramp_l * (1.0 - (7.0f32 / s0).sqrt()) } else { ty + ramp_l };
        // the spike's surface: straight through the chamber, then the ramp curving in towards the axis
        let spike = |y: f32| -> f32 {
            if y <= ty {
                s0
            } else {
                s0 * (1.0 - ((y - ty) / ramp_l).min(1.0)).powf(q)
            }
        };
        // the gas side of the outer wall: the slot, converging to the throat gap, then the cowl lip
        let outer = |y: f32| -> f32 {
            if y < conv {
                s0 + cw
            } else if y < ty {
                let t = (y - conv) / (ty - conv);
                (s0 + gap) + (cw - gap) * (0.5 + 0.5 * (PI * t).cos())
            } else {
                s0 + gap
            }
        };
        let lip_end = ty + 8.0;
        let (liner, shell) = (e.liner, e.shell);
        let y_cap = 58 + dy; // the injector head: cap rows y_cap, y_cap + 1, manifold row y_cap + 2, injector row y_cap + 3
        for y in y_cap as usize..=y_end as usize {
            let yf = y as f32 + 0.5;
            let si = spike(yf);
            for x in 0..W {
                let d = (x as f32 + 0.5 - cx).abs();
                let v = if d < si {
                    if d < si - liner { c::STEEL } else { e.liner_mat }
                } else if yf < lip_end && d >= outer(yf) {
                    // the outer wall, thinning to a point at the lip
                    let th = (liner + shell) * (1.0 - 0.75 * ((yf - ty).max(0.0) / (lip_end - ty)));
                    let dd = d - outer(yf);
                    if dd < liner.min(th) {
                        e.liner_mat
                    } else if dd < th {
                        c::STEEL
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };
                self.cells[World::idx(x, y)] = v;
            }
        }
        // injector head: cap over both slots, then a manifold row and an injector row under it
        self.rect(89, y_cap, 166, y_cap + 1, c::STEEL);
        for x in 94..=161usize {
            let d = (x as f32 + 0.5 - cx).abs();
            if d <= s0 || d >= s0 + cw {
                continue; // (over the spike, or outside the slot)
            }
            let (m, inj) = if d < s0 + 5.0 {
                (c::PIPE, c::INJ) // oxidiser, against the spike
            } else if d < s0 + 6.0 {
                (c::STEEL, c::STEEL) // divider
            } else {
                (c::PIPE, c::INJ) // fuel, outside
            };
            self.cells[World::idx(x, (y_cap + 2) as usize)] = m;
            self.cells[World::idx(x, (y_cap + 3) as usize)] = inj;
        }
        let y_man = y_cap + 2;
        self.rect(99, y_man, 156, y_man, c::PIPE); // the oxidiser line over the top of the spike
        // the airframe: steel skin carrying on up from the chamber's outer wall, and a cap on top
        let y_top = 4;
        self.rect(89, y_top, 90, y_cap - 1, c::STEEL);
        self.rect(165, y_top, 166, y_cap - 1, c::STEEL);
        self.rect(89, y_top, 166, y_top + 1, c::STEEL);
        // the stack: helium bottle on top, fuel tank, oxidiser tank, and a common wall between each
        let he_y = (y_top + 3, y_top + 21);
        self.rect(108, he_y.0, 147, he_y.1, c::HE_TANK);
        let (fuel_y, ox_y) = ((29, 84), (100, 168));
        self.rect(98, fuel_y.0, 157, fuel_y.1, c::TANK_F);
        self.rect(100, ox_y.0, 155, ox_y.1, c::TANK_O);
        let mut back: Vec<(i32, i32, i32, i32)> = vec![];
        let mut regs: Vec<(i32, i32, f32)> = vec![];
        // helium to the fuel tank: straight down from the bottle through a regulator onto the tank's top (the regulator is one
        // row, so that one cell touches both the helium pipe and the tank, which is what makes it a regulator)
        self.rect(127, he_y.1 + 1, 128, fuel_y.0 - 2, c::PIPE);
        self.rect(127, fuel_y.0 - 1, 128, fuel_y.0 - 1, c::REG);
        regs.push((127, fuel_y.0 - 1, e.tank_p.0));
        regs.push((128, fuel_y.0 - 1, e.tank_p.0));
        // helium to the oxidiser tank: off the bottle's right end, behind everything on the back layer down the right of the
        // stack, and through a regulator onto the tank's right face
        let ry = ox_y.0 + 10;
        self.rect(148, he_y.0 + 6, 149, he_y.0 + 7, c::JOINT);
        self.rect(157, ry, 158, ry + 1, c::JOINT);
        self.rect(156, ry, 156, ry + 1, c::REG);
        regs.push((156, ry, e.tank_p.1));
        regs.push((156, ry + 1, e.tank_p.1));
        back.push((148, he_y.0 + 6, 158, he_y.0 + 7));
        back.push((157, he_y.0 + 6, 158, ry + 1));
        // fuel side pipes, down either side of the oxidiser tank into the outer manifolds, joined by a pipe under the fuel tank
        let y_loop = 95;
        self.rect(94, y_loop, 95, y_cap + 2, c::PIPE);
        self.rect(160, y_loop, 161, y_cap + 2, c::PIPE);
        self.rect(94, y_loop, 161, y_loop + 1, c::PIPE);
        if !e.electric && !e.gg {
            // pressure-fed: the fuel straight down from the middle of the tank, through a valve and a throttle, to the pipe under it
            self.rect(127, fuel_y.1 + 1, 128, fuel_y.1 + 2, c::PIPE);
            self.rect(127, fuel_y.1 + 3, 128, fuel_y.1 + 3, c::VALVE_X);
            self.rect(127, fuel_y.1 + 4, 128, fuel_y.1 + 4, c::THROTTLE);
            self.rect(127, fuel_y.1 + 5, 128, y_loop - 1, c::PIPE);
            // the oxidiser straight down from the middle of its tank into the line over the spike
            self.rect(127, ox_y.1 + 1, 128, y_cap - 3, c::PIPE);
            self.rect(127, y_cap - 2, 128, y_cap - 2, c::VALVE_X);
            self.rect(127, y_cap - 1, 128, y_cap + 1, c::PIPE);
            self.rect(127, y_cap - 1, 128, y_cap - 1, c::THROTTLE);
        } else if e.gg {
            // Gas-generator turbopumps down the middle of the spike, one shaft: oxidiser pump on top, a shaft cell, the fuel pump,
            // another, and the turbine at the bottom. The turbine's exhaust leaves its bottom face straight down the axis in a duct
            // that narrows with the spike and opens out of the truncated base, the shortest, straightest way out (little back
            // pressure on the wheel). The generator is bolted to the turbine's right face. A pipe touching any face of a pump joins
            // its network, so the pumps need no open end; the shaft cells keep the two pumps from merging into one block.
            let xc = 128;
            let (d_o, l_o, d_f, l_f, d_t, l_t) = (6, 4, 6, 4, 10, 14);
            let y_o = y_cap + 8;
            let y_s1 = y_o + l_o; // shaft cell row between the pumps
            let y_f = y_s1 + 1;
            let y_s2 = y_f + l_f; // shaft cell row between the fuel pump and the turbine
            let y_t = y_s2 + 1;
            let (ox0, ox1) = (xc - d_o / 2, xc - d_o / 2 + d_o - 1);
            let (tx0, tx1) = (xc - d_t / 2, xc - d_t / 2 + d_t - 1);
            let (pf0, pf1) = (xc - d_f / 2, xc - d_f / 2 + d_f - 1);
            self.rect(ox0, y_o, ox1, y_s1 - 1, c::PUMP);
            self.rect(xc - 1, y_s1, xc, y_s1, c::SHAFT);
            self.rect(pf0, y_f, pf1, y_f + l_f - 1, c::PUMP);
            self.rect(xc - 1, y_s2, xc, y_s2, c::SHAFT);
            self.rect(tx0, y_t, tx1, y_t + l_t - 1, c::TURBINE);
            let (cy0, cy1) = (y_t, y_t + l_t - 1);
            // the generator: a head on the right, a cavity, and a funnel narrowing to a 4-row opening onto the turbine
            let hx = 145;
            let (gc0, gc1) = (137, 144);
            self.rect(gc0, cy0, gc1, cy1, c::EMPTY);
            for k in 0..4 {
                let inset = (2 * k + 2).min(5);
                self.rect(gc0 - 1 - k, cy0 + inset, gc0 - 1 - k, cy1 - inset, c::EMPTY);
            }
            self.rect(hx, cy0, hx + 1, cy1, c::GG);
            // generator feeds. Oxidiser: out of the oxidiser pump's right side, along, and down a one-cell pipe at the far right of
            // the spike past the head to a row under it, through a valve. Fuel: out of the fuel pump's right side, along to the
            // head, and down through a valve onto its top. (The two never touch: the oxidiser pipe stays a cell clear of the head.)
            let yo_m = y_o + l_o / 2 - 1;
            self.rect(ox1 + 1, yo_m, 148, yo_m + 1, c::PIPE);
            self.rect(hx - 6, yo_m, hx - 5, yo_m + 1, c::VALVE_X);
            self.rect(148, yo_m + 2, 148, cy1 + 1, c::PIPE);
            self.rect(hx, cy1 + 1, 148, cy1 + 1, c::PIPE);
            let yf_g = y_f + 1; // the generator's fuel row
            self.rect(pf1 + 1, yf_g, hx + 1, yf_g + 1, c::PIPE);
            self.rect(hx, yf_g + 2, hx + 1, yf_g + 2, c::VALVE_X);
            self.rect(hx, yf_g + 3, hx + 1, cy0 - 1, c::PIPE);
            // the exhaust: straight down the axis from the turbine's bottom face, as wide as the spike allows (half-width 5, less
            // near the base where the spike is thin) and out of the base
            for y in (y_t + l_t)..=(y_end as i32) {
                let hd = (spike(y as f32 + 0.5) - liner - 2.0).clamp(3.0, 5.0) as i32;
                self.rect(xc - hd, y, xc + hd - 1, y, c::EMPTY);
            }
            // oxidiser main outlet: out of the pump's left side and up to the line over the top of the spike, through the main valve
            // and the throttle (downstream of the pump, as on the Merlin, so the pump and the generator's branch are connected to
            // the tank from the start and the start sequence can open the generator's valve)
            self.rect(117, yo_m, ox0 - 1, yo_m + 1, c::PIPE);
            self.rect(117, y_man + 1, 118, yo_m + 1, c::PIPE);
            self.rect(117, y_man + 4, 118, y_man + 5, c::VALVE_X);
            self.rect(117, y_man + 3, 118, y_man + 3, c::THROTTLE);
            // oxidiser inlet: straight from the middle of the tank, a joint, behind the line over the spike to a joint on the pump's top
            self.rect(127, ox_y.1 + 1, 128, ox_y.1 + 5, c::PIPE);
            self.rect(127, ox_y.1 + 6, 128, ox_y.1 + 7, c::JOINT);
            self.rect(127, y_o - 2, 128, y_o - 1, c::JOINT);
            back.push((127, ox_y.1 + 6, 128, y_o - 1));
            // fuel inlet: straight from the right of the tank's bottom, a joint, behind the stack down the right, and a joint against
            // the fuel pump's right face just above the generator's fuel row
            let y_fi = y_f - 1; // inlet joint rows y_fi, y_fi + 1
            self.rect(135, fuel_y.1 + 1, 136, fuel_y.1 + 5, c::PIPE);
            self.rect(135, fuel_y.1 + 6, 136, fuel_y.1 + 7, c::JOINT);
            self.rect(pf1 + 1, y_fi, pf1 + 2, y_fi + 1, c::JOINT);
            back.push((135, fuel_y.1 + 6, 136, y_fi + 1));
            back.push((pf1 + 1, y_fi, 136, y_fi + 1));
            // fuel main outlet: out of the pump's left side, through the main valve and the throttle, a joint, then behind the engine
            // to a joint in the left side pipe
            let yf_m = y_f + l_f / 2 - 1;
            self.rect(118, yf_m, pf0 - 1, yf_m + 1, c::PIPE);
            self.rect(121, yf_m, 122, yf_m + 1, c::VALVE_X);
            self.rect(120, yf_m, 120, yf_m + 1, c::THROTTLE);
            self.rect(118, yf_m, 119, yf_m + 1, c::JOINT);
            let yj = y_loop + 40;
            self.rect(94, yj, 95, yj + 1, c::JOINT);
            back.push((94, yf_m, 119, yf_m + 1));
            back.push((94, yj, 95, yf_m + 1));
        } else {
            // Electric pumps down the middle of the spike, one shaft: oxidiser pump on top, the motor, the fuel pump below, and a
            // battery pack against the motor's right face (touching cells are wired together). Both feeds have to cross a slot
            // chamber to get in, so they run on the back layer, behind everything, and come through in joints.
            let xc = 128;
            let (d_o, l_o, d_f, l_f, d_m, l_m) = (6, 4, 6, 4, 10, 6);
            let (y_o, y_m) = (y_cap + 8, y_cap + 8 + l_o);
            let y_f = y_m + l_m;
            let (ox0, ox1) = (xc - d_o / 2, xc - d_o / 2 + d_o - 1);
            let (mx0, mx1) = (xc - d_m / 2, xc - d_m / 2 + d_m - 1);
            let (pf0, pf1) = (xc - d_f / 2, xc - d_f / 2 + d_f - 1);
            self.rect(ox0, y_o, ox1, y_m - 1, c::PUMP);
            self.rect(mx0, y_m, mx1, y_f - 1, c::MOTOR);
            self.rect(pf0, y_f, pf1, y_f + l_f - 1, c::PUMP);
            self.rect(mx1 + 1, y_o, mx1 + 14, y_f + l_f + 3, c::BATTERY);
            // oxidiser: from the middle of its tank, a main valve and a throttle, a joint, behind the line over the spike to a
            // joint on the pump's top face; out of the pump's left side and up to the line over the top of the spike
            self.rect(127, ox_y.1 + 1, 128, ox_y.1 + 2, c::PIPE);
            self.rect(127, ox_y.1 + 3, 128, ox_y.1 + 3, c::VALVE_X);
            self.rect(127, ox_y.1 + 4, 128, ox_y.1 + 4, c::THROTTLE);
            self.rect(127, ox_y.1 + 5, 128, ox_y.1 + 5, c::PIPE);
            self.rect(127, ox_y.1 + 6, 128, ox_y.1 + 7, c::JOINT);
            self.rect(127, y_o - 2, 128, y_o - 1, c::JOINT);
            back.push((127, ox_y.1 + 6, 128, y_o - 1));
            let yo_m = y_o + l_o / 2 - 1;
            self.rect(117, yo_m, ox0 - 1, yo_m + 1, c::PIPE);
            self.rect(117, y_man + 1, 118, yo_m + 1, c::PIPE);
            // fuel: from the left of the tank's bottom, a valve and a throttle, a joint, behind the stack on the back layer down to
            // the pump's bottom face; out of the pump's left side, a joint, and behind the engine to a joint in the left side pipe
            let y_fi = y_f + l_f; // fuel inlet rows y_fi, y_fi + 1
            self.rect(121, fuel_y.1 + 1, 122, fuel_y.1 + 2, c::PIPE);
            self.rect(121, fuel_y.1 + 3, 122, fuel_y.1 + 3, c::VALVE_X);
            self.rect(121, fuel_y.1 + 4, 122, fuel_y.1 + 4, c::THROTTLE);
            self.rect(121, fuel_y.1 + 5, 122, fuel_y.1 + 5, c::PIPE);
            self.rect(121, fuel_y.1 + 6, 122, fuel_y.1 + 7, c::JOINT);
            self.rect(127, y_fi, 128, y_fi + 1, c::JOINT);
            back.push((121, fuel_y.1 + 6, 122, y_fi + 1));
            back.push((121, y_fi, 128, y_fi + 1));
            let yf_m = y_f + l_f / 2 - 1;
            self.rect(115, yf_m, pf0 - 1, yf_m + 1, c::PIPE);
            self.rect(115, yf_m, 116, yf_m + 1, c::JOINT);
            let yj = y_loop + 40;
            self.rect(94, yj, 95, yj + 1, c::JOINT);
            back.push((94, yf_m, 116, yf_m + 1));
            back.push((94, yj, 95, yf_m + 1));
        }
        (back, if e.electric || e.gg { regs } else { vec![] })
    }

    pub fn build_engine(&mut self, e: &EngineSpec) {
        self.clear();
        let drive = if e.electric { c::MOTOR } else { c::TURBINE };
        {
            let s = &mut self.st;
            s.prop = e.prop;
            s.turb_mult = e.turb_mult;
            s.cartridge_w = e.cartridge_w;
            // expanders hold their mixture with the throttle valves: with no gas generator to set the pace,
            // the oxidiser side would otherwise run away from the fuel side
            s.mix_target = if e.tri {
                crate::world::blend_pairs(0, &[0.1, 0.9, 0.0, 0.0]).of_opt
            } else if !e.gg && e.fuel_tp.is_some() {
                crate::world::PROPS[e.prop].of_opt
            } else if e.twin_shaft {
                // two shafts, each pump free to run away from the other: the mixture control holds the ratio (the J-2's
                // propellant utilisation valve), a little fuel-rich of the best mixture
                crate::world::PROPS[e.prop].of_opt * 0.92
            } else {
                0.0
            };
            s.gg_area_f = e.gg_area.0;
            s.gg_area_o = e.gg_area.1;
            // (a powerhead from the maker sets these for itself further down; nothing of the last engine's is left behind)
            s.gg_p = Settings::default().gg_p;
            s.gg_t_target = 0.0;
            s.tank_p_f = e.tank_p.0;
            s.tank_p_o = e.tank_p.1;
            s.throat_cells = 2.0 * e.throat_hw * if e.twin_noz > 0.0 { 2.0 } else { 1.0 };
            s.dx = e.dx;
            s.depth = e.depth;
            s.axi = false;
        }
        let cx = 128.0f32;
        let hw = |y: f32| -> f32 {
            if y < e.conv_start {
                e.ch_hw
            } else if y < e.throat_y {
                let t = (y - e.conv_start) / (e.throat_y - e.conv_start);
                e.throat_hw + (e.ch_hw - e.throat_hw) * (0.5 + 0.5 * (PI * t).cos())
            } else {
                let t = ((y - e.throat_y) / (e.exit_y - e.throat_y)).clamp(0.0, 1.0);
                let f = if e.conical { t } else { 1.0 - (1.0 - t).powf(2.2) };
                e.throat_hw + (e.exit_hw - e.throat_hw) * f
            }
        };
        let (l1, l2, l3) = (e.liner, e.liner + e.jacket, e.liner + e.jacket + e.shell);
        let mut plug_regs: Vec<(i32, i32, f32)> = vec![];
        let plug_back = if e.plug {
            let (b, r) = self.aerospike_layout(e);
            plug_regs = r;
            Some(b)
        } else {
            None
        };
        if !e.plug {
        // (two nozzles: the same contour about each nozzle's own centre line. Where the two overlap, at the top, they are
        // one chamber; below, the wall between them is the wedge that splits the flow, lined and jacketed like the rest)
        let off = e.twin_noz;
        for y in 58..=e.exit_y as usize {
            let h = hw(y as f32 + 0.5);
            for x in 0..W {
                let d = ((x as f32 + 0.5 - cx).abs() - off).abs();
                let v = if d < h {
                    c::EMPTY
                } else if d < h + l1 {
                    e.liner_mat
                } else if d < h + l2 {
                    c::PIPE
                } else if d < h + l3 {
                    c::STEEL
                } else {
                    continue;
                };
                self.cells[World::idx(x, y)] = v;
            }
        }
        // injector head: cap, oxidiser manifold in the middle, fuel manifolds either side
        let ext = if off > 0.0 { (off + e.ch_hw - 22.0) as i32 } else { 0 }; // (a twin-nozzle chamber is that much wider each side)
        self.rect(100 - ext, 58, 155 + ext, 59, c::STEEL);
        for x in (100 - ext) as usize..=(155 + ext) as usize {
            let d = (x as f32 + 0.5 - cx).abs();
            let left = (x as f32 + 0.5) < cx;
            if off > 0.0 {
                // one wide chamber: the oxidiser manifold in the middle, fuel either side as usual, and under them a row
                // of alternating injectors right across the face, so fuel and oxidiser meet everywhere and not just in
                // the middle of a chamber this wide
                self.cells[World::idx(x, 60)] = if d < 5.0 || d >= 6.0 { c::PIPE } else { c::STEEL };
                if d < off + e.ch_hw {
                    self.cells[World::idx(x, 61)] = c::AINJ;
                }
                continue;
            }
            let (m, inj) = if d < 5.0 {
                (c::PIPE, c::INJ)
            } else if d < 6.0 {
                (c::STEEL, c::STEEL)
            } else if e.tri && left && d < 10.0 {
                (c::PIPE, c::INJ) // the second fuel's manifold (x 118..121)
            } else if e.tri && left && d < 11.0 {
                (c::STEEL, c::STEEL)
            } else {
                (c::PIPE, c::INJ)
            };
            self.cells[World::idx(x, 60)] = m;
            if d < e.ch_hw {
                self.cells[World::idx(x, 61)] = inj;
            }
        }
        }
        let mut feeds = Feeds::default();
        let mut front: Vec<Rect> = vec![];
        let mut maker: Option<crate::powerhead::PhSize> = None;
        let stage = e.stage && !e.plug;
        let back: Vec<(i32, i32, i32, i32)> = if let Some(b) = plug_back {
            b
        } else if stage && (((e.gg && e.gg_dump != 2) || e.tapoff) && (e.single_shaft || e.twin_shaft) || Self::is_expander(e)) {
            let (b, s) = self.maker_layout(e, &hw, &mut feeds, &mut front);
            maker = Some(s);
            b
        } else if e.single_shaft {
            self.single_shaft_layout(e, &hw, &mut feeds)
        } else if e.twin_shaft {
            self.twin_shaft_layout(e, &hw, &mut feeds)
        } else {
            let mut stage_back: Vec<Rect> = vec![];
            // fuel loop over the top joins the two fuel manifolds
            self.rect(98, 3, 99, 61, c::PIPE);
            self.rect(156, 3, 157, 61, c::PIPE);
            self.rect(98, 3, 157, 4, c::PIPE);

            // oxidiser: tank, optional turbopump, main valve, into the manifold
            let ob = e.ox_tank_bottom;
            let mut y_top = ob + 1;
            if stage {
                // (from the tank stack above: behind the fuel loop on the back layer, between two joints)
                self.rect(127, 0, 128, 1, c::JOINT);
                self.rect(127, 6, 128, 7, c::JOINT);
                stage_back.push((127, 0, 128, 7));
                feeds.ox = 127;
                y_top = 8;
            } else {
                self.rect(108, 8, 147, ob, c::TANK_O);
            }
            if let Some(tp) = &e.ox_tp {
                let d = tp.pump_d;
                let w = ((0.6 * (d * tp.stages) as f32).round() as i32).max(3);
                let (py0, py1) = (46 - d / 2, 46 - d / 2 + d - 1);
                let px0 = 128 - w / 2;
                self.rect(127, y_top, 128, py0 - 1, c::PIPE);
                self.rect(px0, py0, px0 + w - 1, py1, c::PUMP);
                // (a stage's wheel stands on the injector head beside its pump; the old layout hung it from the tank)
                let ty0 = if stage { (58 - tp.turb_d).max(y_top) } else { y_top };
                self.rect(px0 - tp.turb_l, ty0, px0 - 1, (ty0 + tp.turb_d - 1).min(57), drive);
                y_top = py1 + 1;
            }
            self.rect(127, y_top, 128, 56, c::PIPE);
            self.rect(127, 57, 128, 57, c::VALVE_X);
            self.rect(127, 58, 128, 59, c::PIPE);
            self.rect(127, 58, 128, 58, c::THROTTLE);
            let mut gg_back = None;
            let mut gg_back2 = None;
            if e.gg && e.gg_dump == 2 {
                // staged combustion: preburner under the oxidiser tank, inside the fuel loop. Its head touches
                // the tank (oxidiser) and the loop pipe (fuel); the chamber narrows to a short duct down onto
                // the injector head, where a hot-gas injector lets the exhaust through into the main chamber
                let (cw, ch) = e.gg_size;
                let (xa, top) = (138, 28);
                let xb = xa + cw + 3;
                self.rect(148, 25, 155, 25, c::STEEL);
                self.rect(137, 26, 137, 27, c::STEEL);
                self.rect(xa, 26, 155, 27, c::GG);
                self.rect(xa, top, xa + 1, top + ch - 1, c::STEEL);
                self.rect(xb - 1, top, 155, top + ch - 1, c::STEEL);
                let steps = (cw - 4) / 2;
                for k in 0..=steps + 1 {
                    let inset = (k + 1).min(steps);
                    let y = top + ch + k;
                    self.rect(xa, y, xa + 1 + inset, y, c::STEEL);
                    self.rect(xb - 1 - inset, y, 155, y, c::STEEL);
                }
                let (ox0, dy0) = (xa + 2 + steps, top + ch + steps + 2); // outlet's left column, first duct row
                self.rect(xa - 2, dy0, ox0 + 5, 57, c::STEEL);
                self.rect(xa, dy0, ox0 + 3, 57, c::EMPTY); // one block: no thin wall corners to give way
                self.rect(xa, 58, xa + 7, 61, c::HGI);
                // the manifold cells boxed in beside the hot-gas injector are cut off from the fuel: wall them up (an
                // empty pipe is only a hole, and the preburner's exhaust would burn through it)
                let mut px = xa - 1;
                while px > 133 && self.cells[World::idx(px as usize, 60)] == c::PIPE {
                    self.cells[World::idx(px as usize, 60)] = c::STEEL;
                    self.cells[World::idx(px as usize, 61)] = c::STEEL;
                    px -= 1;
                }
            } else if e.gg {
                // gas generator on the open right side of the fuel loop, below the helium bottle: an injector
                // head (the GG cells) fed with fuel from the loop pipe it touches and oxidiser from the tank
                // through a line behind the loop (back layer), burning into its own steel chamber, which
                // narrows to an outlet at the bottom; the exhaust falls away past the engine into open air
                let (cw, ch) = e.gg_size; // combustion cavity, cells
                let xa = 158; // left edge: against the loop pipe
                let xb = xa + cw + 3; // two-cell walls either side of the cavity
                let hy = 30; // injector head rows hy, hy + 1
                self.rect(xa, hy - 1, xb, hy - 1, c::STEEL);
                self.rect(xa, hy, xb, hy + 1, if e.burner { c::BURNER } else { c::GG });
                let top = hy + 2;
                self.rect(xa, top, xa + 1, top + ch - 1, c::STEEL);
                self.rect(xb - 1, top, xb, top + ch - 1, c::STEEL);
                if e.burner {
                    // the whole chamber is the burner block (the converging outlet below it is where its gas leaves)
                    self.rect(xa + 2, top, xb - 2, top + ch - 1, c::BURNER);
                }
                // converging outlet
                let ow = 4;
                let steps = (cw - ow) / 2;
                for k in 0..=steps + 1 {
                    let inset = (k + 1).min(steps);
                    let y = top + ch + k;
                    self.rect(xa, y, xa + 1 + inset, y, c::STEEL);
                    self.rect(xb - 1 - inset, y, xb, y, c::STEEL);
                }
                // oxidiser line: tank side -> joint -> back layer, right and down -> joint at the head's end
                let oy = 20;
                self.rect(148, oy, 151, oy + 1, c::PIPE);
                self.rect(150, oy, 151, oy + 1, c::JOINT);
                self.rect(xb + 1, hy, xb + 2, hy + 1, c::JOINT);
                gg_back = Some((150, oy, xb + 2, oy + 1));
                gg_back2 = Some((xb + 1, oy, xb + 2, hy + 1));
            }
            let (h0, h1, h2, h3) = e.he;
            // helium is piped to each tank and enters through a pressure regulator
            let (fx0, fy0, fx1, _) = e.fuel_tank;
            let mut back: Vec<(i32, i32, i32, i32)> = vec![];
            let mut he_back: Vec<Rect> = vec![];
            if !stage {
                self.rect(h0, h1, h2, h3, c::HE_TANK);
                // oxidiser: across from the bottle to the side of the tank, passing behind the fuel loop
                // on the back layer (joints at both ends)
                self.rect(159, h1 + 3, h0 - 1, h1 + 4, c::PIPE);
                self.rect(159, h1 + 3, 160, h1 + 4, c::JOINT);
                self.rect(149, h1 + 3, 150, h1 + 4, c::JOINT);
                back.push((149, h1 + 3, 160, h1 + 4));
                self.rect(148, h1 + 3, 148, h1 + 4, c::REG);
                // fuel: up from the bottle, along the top, down the left side and onto the top of the tank, all
                // on the back layer (joints at both ends) so it doesn't wall in the space beside the engine
                let hx = (h0 + h2) / 2;
                let rx = (fx0 + fx1) / 2;
                self.rect(hx, h1 - 1, hx + 1, h1 - 1, c::JOINT);
                self.rect(rx - 1, fy0 - 2, rx, fy0 - 2, c::JOINT);
                self.rect(rx - 1, fy0 - 1, rx, fy0 - 1, c::REG);
                he_back.extend([(hx, 0, hx + 1, h1 - 1), (2, 0, hx + 1, 1), (2, 0, 3, fy0 - 2), (2, fy0 - 3, rx, fy0 - 2)]);
            }

            // fuel: tank, optional turbopump, riser with the main valve, then to the jacket or the loop
            let (fx0, fy0, fx1, fy1) = e.fuel_tank;
            if !stage {
                self.rect(fx0, fy0, fx1, fy1, c::TANK_F);
            }
            let (sx, sy, sb);
            if let Some(tp) = &e.fuel_tp {
                let d = tp.pump_d;
                let w = ((0.6 * (d * tp.stages) as f32).round() as i32).max(3);
                let px0 = fx1 + 10;
                let py0 = 161 - d / 2;
                if stage {
                    // (from the tank stack: down the far left to the pump)
                    self.rect(20, 0, 21, 161, c::PIPE);
                    self.rect(20, 160, px0 - 1, 161, c::PIPE);
                    feeds.fuel = 20;
                } else {
                    self.rect(fx1 + 1, 160, px0 - 1, 161, c::PIPE);
                }
                self.rect(px0, py0, px0 + w - 1, py0 + d - 1, c::PUMP);
                let ty0 = 161 - tp.turb_d / 2;
                self.rect(px0 + w, ty0, px0 + w + tp.turb_l - 1, ty0 + tp.turb_d - 1, drive);
                sx = px0 + w / 2 - 1;
                sy = py0 - 1;
                sb = py0 + d;
            } else {
                if stage {
                    feeds.fuel = 52;
                    if e.jacket > 0.0 {
                        self.rect(52, 0, 53, 161, c::PIPE);
                    }
                } else {
                    self.rect(fx1 + 1, 160, 53, 161, c::PIPE);
                }
                sx = 52;
                sy = 159;
                sb = 162;
            }
            let mut ring = None;
            let mut exp_back: Vec<(i32, i32, i32, i32)> = vec![];
            if e.jacket > 0.0 {
                // regen engines: the fuel runs down to the bottom of the bell first, enters the cooling jacket
                // through the shell just above the exit on both sides (a back-layer ring passes behind the
                // bell to the far side), climbs the jacket past the throat and the chamber, and only then
                // reaches the injector manifolds. The exit itself stays clear, so a niobium skirt can bolt on.
                let ey = e.exit_y as i32;
                let yf = ey - 3; // feed rows yf, yf + 1
                let h = hw(yf as f32 + 0.5);
                let tx = (cx - (h + e.liner + e.jacket * 0.5)).floor() as i32; // outer jacket column on the left
                let x_out = (cx - e.exit_hw - l3).floor() as i32;
                let col = if sx + 1 < x_out - 3 { sx } else { (x_out - 6).max(8) }; // go round the outside of a wide bell, close to it
                let yh = sb.max(fy1 + 4);
                self.rect(sx, sb, sx + 1, yh + 1, c::PIPE);
                self.rect(col.min(sx), yh, col.max(sx) + 1, yh + 1, c::PIPE);
                self.rect(col, yh, col + 1, yf + 1, c::PIPE);
                self.rect(col, yh + 4, col + 1, yh + 5, c::VALVE_X);
                self.rect(col, yh + 7, col + 1, yh + 7, c::THROTTLE);
                self.rect(col, yf, tx, yf + 1, c::PIPE);
                self.rect(col, yf, col + 1, yf + 1, c::JOINT);
                // mirror image on the right: x -> 255 - x
                let mt = 255 - tx;
                let mc = (mt + 8).min(255 - col - 1); // the right feed stops just clear of the bell too
                self.rect(mt, yf, mc + 1, yf + 1, c::PIPE);
                self.rect(mc, yf, mc + 1, yf + 1, c::JOINT);
                ring = Some((col, mc + 1, yf));
                if !e.gg && !e.electric {
                    if let (Some(ftp), Some(otp)) = (&e.fuel_tp, &e.ox_tp) {
                        // expander, in series as on the RL10: the jacket's warm hydrogen goes through the fuel
                        // turbine, then the oxidiser turbine, and only then to the injectors.
                        // 1. Both jacket tops empty into the loop over the top of the engine, which no longer
                        //    joins the injector manifolds: it collects the hot hydrogen.
                        self.rect(100, 60, 103, 60, c::STEEL);
                        self.rect(152, 60, 155, 60, c::STEEL);
                        self.rect(100, 61, 101, 61, c::PIPE);
                        self.rect(154, 61, 155, 61, c::PIPE);
                        // 2. Collector -> fuel turbine: off the left side of the loop, down onto the wheel
                        let fw = ((0.6 * (ftp.pump_d * ftp.stages) as f32).round() as i32).max(3);
                        let tx0 = fx1 + 10 + fw;
                        let tx1 = tx0 + ftp.turb_l - 1;
                        let ty0 = 161 - ftp.turb_d / 2;
                        let xi = tx1 - 2; // inlet column (2 wide)
                        self.rect(xi, 50, 97, 51, c::PIPE);
                        self.rect(xi, 50, xi + 1, ty0 - 1, c::PIPE);
                        // 3. Fuel turbine -> oxidiser turbine: up from the wheel, behind the collector (back
                        //    layer, between two joints) and across to the oxidiser turbine's side
                        let xo = tx0 + 1; // outlet column
                        let ow = ((0.6 * (otp.pump_d * otp.stages) as f32).round() as i32).max(3);
                        let ot_x0 = 128 - ow / 2 - otp.turb_l;
                        self.rect(xo, 44, xo + 1, ty0 - 1, c::PIPE);
                        self.rect(xo, 44, 95, 45, c::PIPE);
                        self.rect(94, 44, 95, 45, c::JOINT);
                        self.rect(102, 44, ot_x0 - 1, 45, c::PIPE);
                        self.rect(102, 44, 103, 45, c::JOINT);
                        exp_back.push((94, 44, 103, 45));
                        // 4. Oxidiser turbine -> injectors: down its left side through the injector cap into the
                        //    left fuel manifold; a back-layer pipe behind the oxidiser manifold feeds the right one
                        self.rect(ot_x0 - 2, 54, ot_x0 - 1, 59, c::PIPE);
                        self.rect(108, 60, 109, 60, c::JOINT);
                        self.rect(146, 60, 147, 60, c::JOINT);
                        exp_back.push((108, 60, 147, 60));
                    }
                }
            } else if stage {
                // pressure-fed from the stack: straight down through the main valve and the throttle, and across into the loop
                self.rect(sx, 0, sx + 1, 41, c::PIPE);
                self.rect(sx, 20, sx + 1, 21, c::VALVE_X);
                self.rect(sx, 24, sx + 1, 24, c::THROTTLE);
                self.rect(sx, 40, 97, 41, c::PIPE);
            } else {
                let ty = 42;
                self.rect(sx, ty, sx + 1, sy, c::PIPE);
                self.rect(sx, sy - 3, sx + 1, sy - 2, c::VALVE_X);
                self.rect(sx, sy - 5, sx + 1, sy - 5, c::THROTTLE);
                self.rect(sx, ty - 2, 97, ty - 1, c::PIPE);
            }

            if e.gg && e.gg_dump == 1 {
                self.build_dump_duct(e);
            }
            if let Some((x0, x1, y0)) = ring {
                back.push((x0, y0, x1, y0 + 1));
            }
            back.extend(exp_back);
            back.extend(gg_back);
            back.extend(gg_back2);
            back.extend(he_back);
            back.extend(stage_back);
            back
        };
        let mut back = back;
        if e.gg && e.gg_dump == 2 && e.twin_shaft {
            back.push((137, 60, 148, 60));
        }
        // a stage: everything so far is the engine; move it down the frame and put the tank stack above it
        let dy = if stage { self.stage_stack_height(e) - feeds.top } else { 0 };
        if stage {
            for y in (0..H).rev() {
                for x in 0..W {
                    self.cells[World::idx(x, y)] = if y >= dy as usize { self.cells[World::idx(x, y - dy as usize)] } else { c::EMPTY };
                }
            }
            for r in back.iter_mut().chain(front.iter_mut()).chain(feeds.outlets.iter_mut()).chain(feeds.reopen.iter_mut()) {
                r.1 += dy;
                r.3 += dy;
            }
            self.stage_stack(e, &feeds, &mut back);
        }
        // move the drawing from the layout frame to the middle of the simulation space
        for y in (0..H).rev() {
            for x in (0..W).rev() {
                let v = if x >= PRESET_OX && y >= PRESET_OY { self.cells[World::idx(x - PRESET_OX, y - PRESET_OY)] } else { c::EMPTY };
                self.cells[World::idx(x, y)] = v;
            }
        }
        for (layer, runs) in [(1u8, &back), (2u8, &front)] {
            for &(x0, y0, x1, y1) in runs {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        self.set_layer_cell(layer, World::idx(x as usize + PRESET_OX, y as usize + PRESET_OY), c::PIPE);
                    }
                }
            }
        }
        for &(x0, y0, x1, y1) in &feeds.outlets {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.set_layer_cell(1, World::idx(x as usize + PRESET_OX, y as usize + PRESET_OY), c::GASOUT);
                }
            }
        }
        if e.tri {
            let oy = PRESET_OY + dy as usize;
            for y in 8..=25 {
                for x in 82..=104 {
                    let i = World::idx(x + PRESET_OX, y + oy);
                    self.fluid[i] = 1; // liquid hydrogen (FUELS index 0, plus one)
                }
            }
            for x in 90..=91 {
                self.reg_p[World::idx(x + PRESET_OX, 7 + oy)] = e.tri_p;
            }
        }
        if e.plug && (e.electric || e.gg) {
            // low-pressure tanks (the pumps raise the feed): hold each at its own setting with its regulator, and run a little
            // fuel-rich so the walls get a cooler film (the chamber is uncooled, ceramic-lined)
            for &(x, y, p) in &plug_regs {
                self.reg_p[World::idx(x as usize + PRESET_OX, y as usize + PRESET_OY)] = p;
            }
            self.st.mix_target = 1.8; // (held with the throttle valves in both main lines)
        }
        self.auto_wall_all();
        for &(x0, y0, x1, y1) in &feeds.reopen {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.cells[World::idx(x as usize + PRESET_OX, y as usize + PRESET_OY)] = c::EMPTY;
                }
            }
        }
        self.dirty.clear();
        self.nets_dirty = true;
        match e.inj {
            Some((f, o)) => {
                self.st.inj_area_f = f;
                self.st.inj_area_o = o;
                if e.tri {
                    self.st.inj_area_f2 = e.tri_inj;
                }
                self.st.feed_p_max = e.feed_p_max;
                self.rebuild_nets();
            }
            None => {
                self.auto_size_injectors(e.target_pc);
                self.st.inj_area_o *= e.ox_trim;
            }
        }
        if let Some(s) = &maker {
            // the powerhead maker's settings: generator orifices, pressure and temperature hold, governor, mixture control
            let (cart, mix, gg_p) = (self.st.cartridge_w, self.st.mix_target, self.st.gg_p);
            crate::powerhead::apply(s, &mut self.st);
            // (an expander's light rotors want no more start cartridge than the maker reckons; and with no generator there is
            // no generator pressure to set)
            self.st.cartridge_w = if s.input.expander { cart.min(s.cartridge_w) } else { cart.max(s.cartridge_w) };
            if s.input.expander || s.input.tapoff {
                self.st.gg_p = gg_p;
            }
            if mix > 0.0 {
                self.st.mix_target = mix;
            }
        }
        if e.mix > 0.0 {
            self.st.mix_target = e.mix;
        }
    }
}
