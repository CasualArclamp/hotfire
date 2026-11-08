//! CPU side of the game: the cell grid the player edits, the plumbing / turbopump /
//! valve model, structural failure and the preset engines.
//! The gas and wall thermal fields themselves live on the GPU (see gpu.rs).

pub const W: usize = 640; // 6.4 m at 1 cm cells
pub const H: usize = 1536; // twice the original 768 rows: room below the engine for a long plume
/// How many rows the default camera shows (the top of the grid, where the engines are built).
pub const VIEW_ROWS: usize = 768;
pub const N: usize = W * H;
pub const DX0: f32 = 0.01; // default metres per cell
pub const DT0: f32 = 1.2e-6; // solver step at DX0; scales with cell size (constant CFL)
pub const OMEGA_D: f32 = 3665.0; // design shaft speed, rad/s (35 000 rpm)
pub const U_TIP_MAX: f32 = 550.0; // rated rotor tip speed (titanium impellers), m/s
pub const PSI: f32 = 0.55; // pump head coefficient: dp per stage = PSI * rho * u_tip^2
pub const PHI_BEP: f32 = 0.03; // flow coefficient at best efficiency: Q = PHI * eye area * u_tip
pub const NPSH_K: f32 = 0.006; // suction pressure needed (with inducer) = K * rho * u_tip^2
pub const RHO_ROTOR: f32 = 4500.0; // rotor material density, kg/m^3
pub const NO_NET: u16 = 0xFFFF;
/// Network contents: pipework fed by a helium bottle (and no propellant tank) carries helium gas.
pub const PROP_HE: u8 = 3;
pub const HE_CELL_VOL: f32 = 0.05; // m^3 of bottle per helium cell
pub const HE_FILL_P: f32 = 3.0e7; // default bottle fill pressure, Pa (about 4350 psi)
pub const PA_PER_PSI: f32 = 6894.76;
pub const HE_RT: f32 = 2077.0 * 300.0; // R*T for helium
pub const LINE_BURST: f32 = 4.5e7; // feed line burst pressure, Pa
pub const VALVE_AREA: f32 = 80.0e-4; // flow area of a fully open valve at 1 cm a cell and the standard depth, m^2
pub const OVERSPEED: f32 = 1.3;
// electric pump drive (Rutherford style), per cell at 1 cm cells and the standard depth (a 36 cm^3 cell)
pub const CART_W_CELL: f32 = 2.5e5; // start cartridge power per cell, W (a 120-cell canister gives 30 MW)
pub const MOTOR_W_CELL: f32 = 3600.0; // motor power: about 100 kW per litre (Rutherford's 50 hp motor is the size of a soda can)
pub const BATT_J_CELL: f32 = 60.0e3; // battery energy: about 460 Wh per litre, lithium cells
pub const BATT_W_CELL: f32 = 1000.0; // battery discharge power: a high-power pack
pub const MOTOR_EFF: f32 = 0.92; // motor and inverter
pub const PURGE_S: f32 = 1.5; // start: inert-gas purge before the fuel valves open, s
pub const PURGE_KG_S: f32 = 0.05; // purge gas per injector cell at 1 cm cells and the standard depth, kg/s // shaft disintegrates above this fraction of design speed
pub const MAX_BLOCKS: usize = 64;
pub const ANIM_TANK: usize = 256 + 8 * MAX_BLOCKS; // per-network tank liquid level (row)
pub const ANIM_FRONT: usize = ANIM_TANK + 256; // per-network liquid front (cells from the tank)
pub const ANIM_PROP: usize = ANIM_FRONT + 256; // per-network contents: 0 fuel, 1 oxidiser, 2 mixed, 255 none
pub const ANIM_HE: usize = ANIM_PROP + 256; // fill (pressure / fill pressure) of each helium bottle
pub const ANIM_BOIL: usize = ANIM_HE + MAX_BLOCKS; // per-network boiling intensity (0..1)
pub const ANIM_VAP: usize = ANIM_BOIL + 256; // per-network ullage vapour density for drawing (0..1)
pub const ANIM_BURN: usize = ANIM_VAP + 256; // the burner blocks: how many (up to 8), then their block numbers
pub const ANIM_AIR: usize = ANIM_BURN + 9; // the wind past the vehicle (m/s down the screen), ambient density against sea level
pub const ANIM_PLUME: usize = ANIM_AIR + 3; // the stylised exhaust plume: on, exit x, exit y, half width, exit speed, Pe/Pa, Mach, flame temperature, intensity, throat y, purge cloud, green TEA-TEB exhaust glow, chamber top y, diamond style, how hard the engine is running (0..1), jet half width after any flow separation, separation severity (0..1), 1 for a round (axisymmetric) engine
pub const ANIM_TEA: usize = ANIM_PLUME + 18; // the TEA-TEB flash (x, y, how bright), then 1 for each bottle that holds TEA-TEB
/// How hard the jet stabiliser damps the sideways velocity of the fast exhaust far below the nozzle, per second of simulated
/// time. A planar jet flaps from side to side; this keeps the near-nozzle shock structure and stills the swinging.
pub const JET_STABILISER: f32 = 4000.0;

/// Wall pressure over ambient below which the flow in an over-expanded bell separates (Summerfield criterion, about 0.35 to 0.4).
pub const SEPARATION_P: f32 = 0.4;

/// Area ratio A/A* of isentropic flow at Mach `m` for a gas of ratio of specific heats `gam`.
pub fn area_ratio(m: f32, gam: f32) -> f32 {
    let kg = 0.5 * (gam - 1.0);
    (1.0 / m) * ((1.0 + kg * m * m) / (1.0 + kg)).powf((gam + 1.0) / (2.0 * (gam - 1.0)))
}

/// The supersonic Mach number at area ratio `eps` (found by bisection).
pub fn mach_from_area(eps: f32, gam: f32) -> f32 {
    let (mut lo, mut hi) = (1.0f32, 15.0f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if area_ratio(mid, gam) < eps {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Static over stagnation pressure at Mach `m`.
pub fn p_over_pt(m: f32, gam: f32) -> f32 {
    (1.0 + 0.5 * (gam - 1.0) * m * m).powf(-gam / (gam - 1.0))
}

/// Names of the Mach-diamond styles the plume shader can draw (the index is `World::fx_style`).
pub const PLUME_STYLES: [&str; 7] = ["Classic beads", "Rhombus chain", "Shock lattice", "Crossed necks", "Nested photo diamonds", "Compression glow", "Raptor reference"];
pub const ANIM_NOZ: usize = ANIM_TEA + 3 + 64; // the gas passage of the main engine, a (left, right) pair of cells for each row from the injector row down
pub const NOZ_ROWS: usize = 420;
pub const ANIM_LEN: usize = ANIM_NOZ + 2 * NOZ_ROWS; // line flow phases, rotor blocks, tank levels, liquid fronts, contents, helium, burners
pub const DRY: f32 = 1.0e6; // phase of line cells no tank can reach

#[derive(Clone)]
pub struct Propellant {
    pub name: &'static str,
    pub rho_f: f32,
    pub rho_o: f32,
    pub cp_f: f32,
    pub cp_o: f32,
    pub t_in_f: f32,
    pub t_in_o: f32,
    pub p_vap_f: f32, // vapour pressure at tank temperature, Pa
    pub p_vap_o: f32,
    pub of_opt: f32,
    pub of_stoich: f32, // oxidiser to fuel by mass that burns it all exactly (stoichiometric)
    pub t_peak: f32,
    pub gamma: f32,
    pub rgas: f32,
    pub hypergolic: bool,
    pub t_cool_max: f32,  // coolant decomposition / coking limit, K
    pub tint: [f32; 3],   // characteristic flame colour
    pub liq_f: [f32; 3],  // liquid colours in pipes and tanks
    pub liq_o: [f32; 3],
    pub soot: f32,        // 0 = clean transparent flame, 1 = sooty, opaque
    pub boil_f: [f32; 3], // fuel: heat of vaporisation J/kg (0 = storable), vapour R, boiling point K
    pub boil_o: [f32; 3],
}

pub const PROPS: [Propellant; 8] = [
    Propellant { name: "LH2 / LOX", rho_f: 71.0, rho_o: 1141.0, cp_f: 14300.0, cp_o: 1700.0, t_in_f: 20.0, t_in_o: 90.0, p_vap_f: 1.0e5, p_vap_o: 1.0e5, of_opt: 6.0, of_stoich: 7.94, t_peak: 3650.0, gamma: 1.2, rgas: 600.0, hypergolic: false, t_cool_max: 99999.0, tint: [0.5, 0.45, 1.0], liq_f: [0.82, 0.9, 1.0], liq_o: [0.45, 0.66, 1.0], soot: 0.0, boil_f: [446.0e3, 4124.0, 20.0], boil_o: [213.0e3, 260.0, 90.0] },
    Propellant { name: "CH4 / LOX", rho_f: 422.0, rho_o: 1141.0, cp_f: 3500.0, cp_o: 1700.0, t_in_f: 112.0, t_in_o: 90.0, p_vap_f: 1.0e5, p_vap_o: 1.0e5, of_opt: 3.5, of_stoich: 3.99, t_peak: 3550.0, gamma: 1.2, rgas: 380.0, hypergolic: false, t_cool_max: 900.0, tint: [0.35, 0.55, 1.0], liq_f: [0.78, 0.82, 0.86], liq_o: [0.45, 0.66, 1.0], soot: 0.2, boil_f: [510.0e3, 518.0, 112.0], boil_o: [213.0e3, 260.0, 90.0] },
    Propellant { name: "RP-1 / LOX", rho_f: 810.0, rho_o: 1141.0, cp_f: 2000.0, cp_o: 1700.0, t_in_f: 290.0, t_in_o: 90.0, p_vap_f: 2.0e3, p_vap_o: 1.0e5, of_opt: 2.4, of_stoich: 3.4, t_peak: 3670.0, gamma: 1.2, rgas: 365.0, hypergolic: false, t_cool_max: 560.0, tint: [1.0, 0.62, 0.22], liq_f: [0.92, 0.72, 0.28], liq_o: [0.45, 0.66, 1.0], soot: 0.85, boil_f: [0.0, 0.0, 0.0], boil_o: [213.0e3, 260.0, 90.0] },
    Propellant { name: "MMH / N2O4 (hypergolic)", rho_f: 880.0, rho_o: 1440.0, cp_f: 2900.0, cp_o: 1550.0, t_in_f: 290.0, t_in_o: 290.0, p_vap_f: 5.0e3, p_vap_o: 1.0e5, of_opt: 2.2, of_stoich: 2.5, t_peak: 3400.0, gamma: 1.2, rgas: 345.0, hypergolic: true, t_cool_max: 530.0, tint: [1.0, 0.5, 0.35], liq_f: [0.88, 0.86, 0.62], liq_o: [0.78, 0.3, 0.14], soot: 0.35, boil_f: [0.0, 0.0, 0.0], boil_o: [0.0, 0.0, 0.0] },
    Propellant { name: "MMH / LOX", rho_f: 880.0, rho_o: 1141.0, cp_f: 2900.0, cp_o: 1700.0, t_in_f: 290.0, t_in_o: 90.0, p_vap_f: 5.0e3, p_vap_o: 1.0e5, of_opt: 1.6, of_stoich: 1.74, t_peak: 3500.0, gamma: 1.2, rgas: 360.0, hypergolic: false, t_cool_max: 530.0, tint: [1.0, 0.55, 0.3], liq_f: [0.88, 0.86, 0.62], liq_o: [0.45, 0.66, 1.0], soot: 0.3, boil_f: [0.0, 0.0, 0.0], boil_o: [213.0e3, 260.0, 90.0] },
    Propellant { name: "LH2 / N2O4", rho_f: 71.0, rho_o: 1440.0, cp_f: 14300.0, cp_o: 1550.0, t_in_f: 20.0, t_in_o: 290.0, p_vap_f: 1.0e5, p_vap_o: 1.0e5, of_opt: 4.5, of_stoich: 11.4, t_peak: 3200.0, gamma: 1.2, rgas: 470.0, hypergolic: false, t_cool_max: 99999.0, tint: [0.75, 0.5, 0.85], liq_f: [0.82, 0.9, 1.0], liq_o: [0.78, 0.3, 0.14], soot: 0.0, boil_f: [446.0e3, 4124.0, 20.0], boil_o: [0.0, 0.0, 0.0] },
    Propellant { name: "CH4 / N2O4", rho_f: 422.0, rho_o: 1440.0, cp_f: 3500.0, cp_o: 1550.0, t_in_f: 112.0, t_in_o: 290.0, p_vap_f: 1.0e5, p_vap_o: 1.0e5, of_opt: 3.5, of_stoich: 5.73, t_peak: 3250.0, gamma: 1.2, rgas: 340.0, hypergolic: false, t_cool_max: 900.0, tint: [0.65, 0.5, 0.8], liq_f: [0.78, 0.82, 0.86], liq_o: [0.78, 0.3, 0.14], soot: 0.15, boil_f: [510.0e3, 518.0, 112.0], boil_o: [0.0, 0.0, 0.0] },
    Propellant { name: "RP-1 / N2O4", rho_f: 810.0, rho_o: 1440.0, cp_f: 2000.0, cp_o: 1550.0, t_in_f: 290.0, t_in_o: 290.0, p_vap_f: 2.0e3, p_vap_o: 1.0e5, of_opt: 3.3, of_stoich: 4.89, t_peak: 3300.0, gamma: 1.2, rgas: 330.0, hypergolic: false, t_cool_max: 560.0, tint: [1.0, 0.6, 0.3], liq_f: [0.92, 0.72, 0.28], liq_o: [0.78, 0.3, 0.14], soot: 0.7, boil_f: [0.0, 0.0, 0.0], boil_o: [0.0, 0.0, 0.0] },
];

/// What a tank can hold. The engine burns whatever its fuel tanks and oxidiser tanks hold together.
pub const FUELS: [&str; 4] = ["LH2 (liquid hydrogen)", "CH4 (liquid methane)", "RP-1 (kerosene)", "MMH (hydrazine)"];
pub const OXIDISERS: [&str; 2] = ["LOX (liquid oxygen)", "N2O4 (nitrogen tetroxide)"];

/// The propellant pair (index into PROPS) for a fuel and an oxidiser.
pub fn pair_index(fuel: usize, ox: usize) -> usize {
    match (fuel.min(3), ox.min(1)) {
        (0, 0) => 0,
        (1, 0) => 1,
        (2, 0) => 2,
        (3, 1) => 3,
        (3, 0) => 4,
        (0, 1) => 5,
        (1, 1) => 6,
        _ => 7,
    }
}

/// The fuel and oxidiser of a propellant pair.
pub fn pair_kinds(prop: usize) -> (usize, usize) {
    (0..4).flat_map(|f| (0..2).map(move |o| (f, o))).find(|&(f, o)| pair_index(f, o) == prop).unwrap_or((0, 0))
}

/// A burn of several fuels in the same oxidiser (a tri-propellant engine): the pair's properties blended by the
/// share of the fuel mass each fuel makes up. `w` is indexed by fuel kind (see FUELS).
pub fn blend_pairs(ox: usize, w: &[f32; 4]) -> Propellant {
    let total: f32 = w.iter().sum();
    let pair = |f: usize| &PROPS[pair_index(f, ox)];
    let first = (0..4).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap_or(0);
    let mut p = pair(first).clone();
    if total <= 1e-6 || w[first] >= total * 0.9999 {
        return p;
    }
    let mix = |g: &dyn Fn(&Propellant) -> f32| (0..4).map(|f| w[f] / total * g(pair(f))).sum::<f32>();
    p.of_opt = mix(&|q| q.of_opt);
    p.of_stoich = mix(&|q| q.of_stoich);
    p.t_peak = mix(&|q| q.t_peak);
    p.gamma = mix(&|q| q.gamma);
    p.rgas = mix(&|q| q.rgas);
    p.soot = mix(&|q| q.soot);
    p.t_cool_max = (0..4).filter(|&f| w[f] > 0.0).map(|f| pair(f).t_cool_max).fold(f32::MAX, f32::min);
    for k in 0..3 {
        p.tint[k] = mix(&|q| q.tint[k]);
    }
    p
}

/// Pressure of the standard atmosphere (1976) at a height in km, Pa. The layers run to 84.85 km (where it is 0.373
/// Pa); above that it thins out exponentially.
pub fn atmosphere_p(h_km: f32) -> f32 {
    // (base height km, base pressure Pa, base temperature K, temperature change K/km)
    const LAYERS: [(f32, f32, f32, f32); 7] = [
        (0.0, 101325.0, 288.15, -6.5),
        (11.0, 22632.1, 216.65, 0.0),
        (20.0, 5474.89, 216.65, 1.0),
        (32.0, 868.019, 228.65, 2.8),
        (47.0, 110.906, 270.65, 0.0),
        (51.0, 66.9389, 270.65, -2.8),
        (71.0, 3.95642, 214.65, -2.0),
    ];
    let h = h_km.clamp(0.0, 200.0);
    if h >= 84.852 {
        return 0.3734 * (-(h - 84.852) / 6.2).exp();
    }
    let mut base = LAYERS[0];
    for l in LAYERS {
        if h >= l.0 {
            base = l;
        }
    }
    let (h0, p0, t0, lapse) = base;
    let dh = h - h0;
    if lapse.abs() < 1e-6 {
        p0 * (-34.1632 * dh / t0).exp()
    } else {
        p0 * ((t0 + lapse * dh) / t0).powf(-34.1632 / lapse)
    }
}

/// The height (km) where the standard atmosphere has this pressure.
pub fn altitude_of_p(p: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 200.0f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if atmosphere_p(mid) > p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The gas solver cannot hold less than this (Pa): about 48 km up, and as good as vacuum for an engine.
pub const MIN_AMBIENT_P: f32 = 100.0;

pub fn t_flame(p: &Propellant, of: f32) -> f32 {
    800.0 + (p.t_peak - 800.0) * (-0.5 * ((of.max(1e-3) / p.of_opt).ln() / 0.7).powi(2)).exp()
}

pub mod c {
    pub const EMPTY: u8 = 0;
    pub const STEEL: u8 = 1;
    pub const COPPER: u8 = 2;
    pub const CERAMIC: u8 = 3;
    /// Pipe interior: holds whatever its network is connected to (fuel, oxidiser, or nothing).
    pub const PIPE: u8 = 4;
    pub const TANK_F: u8 = 6;
    pub const TANK_O: u8 = 7;
    pub const PUMP: u8 = 8;
    pub const VALVE: u8 = 10;
    pub const VALVE_X: u8 = 11;
    pub const INJ: u8 = 14;
    pub const GG: u8 = 16;
    pub const HE_TANK: u8 = 17;
    pub const TURBINE: u8 = 18;
    pub const SHAFT: u8 = 19;
    /// Middle-layer pipe that also links to back / front layer pipes at the same spot.
    pub const JOINT: u8 = 20;
    /// Source blocks: keep pushing their substance into any open space they touch (like Powder Toy's clone).
    pub const SRC_GAS: u8 = 21;
    pub const SRC_FUEL: u8 = 22;
    pub const SRC_FIRE: u8 = 23;
    /// Niobium alloy (C-103): radiation-cooled nozzle extensions, glows white-hot instead of melting.
    pub const NIOBIUM: u8 = 24;
    /// Pressure regulator: helium line (or bottle) on one side, a propellant tank on the other.
    pub const REG: u8 = 25;
    /// Hot-gas injector: lets gas through from whichever side is at the higher pressure (orifice flow),
    /// e.g. a staged-combustion preburner's exhaust into the main chamber.
    pub const HGI: u8 = 26;
    /// Throttle valve: a metering valve in a feed line whose opening the throttle sets.
    pub const THROTTLE: u8 = 27;
    /// Electric motor: a rotor on a shaft (like a turbine) driven from the batteries it's wired to.
    pub const MOTOR: u8 = 28;
    /// Battery: stores the energy for electric motors (touching batteries, wire and motors form one circuit).
    pub const BATTERY: u8 = 29;
    /// Wire: joins batteries to motors.
    pub const WIRE: u8 = 30;
    /// Gas burner (preburner): a machine block like a turbine. Fuel and oxidiser lines touch it, its open face is the
    /// outlet. It burns by its own equations (no ignition, nothing for the gas simulation to light or mix) and hands
    /// the hot gas to whatever duct it opens into. Drawn as a cutaway with the injector jets and the flame inside.
    pub const BURNER: u8 = 31;
    /// Start cartridge: a solid-propellant canister. Touching (or within a wall of) a turbopump shaft it spins that
    /// shaft up at the start, its size setting the power. Builds with none use the start cartridge setting instead.
    pub const CARTRIDGE: u8 = 32;
    /// Palette alias: paints a helium-style bottle holding TEA-TEB, the pyrophoric igniter (triethylaluminium /
    /// triethylborane) that lights on contact with oxygen. Pipe it through a valve to an injector like the helium start.
    pub const TEATEB: u8 = 33;
    /// Alternating injector: touch a fuel line and an oxidiser line and it sprays fuel from every other cell and oxidiser from
    /// the cells between (checkerboard by position), into whatever open gas it faces: one part, no separate manifolds.
    pub const AINJ: u8 = 34;
    pub const COUNT: u8 = 35;

    /// Ids retired when plumbing stopped being fuel- or oxidiser-specific (old saves load as generic parts).
    pub fn canon(t: u8) -> u8 {
        match t {
            5 => PIPE,
            9 => PUMP,
            12 => VALVE,
            13 => VALVE_X,
            15 => INJ,
            x => x,
        }
    }
    pub fn name(c: u8) -> &'static str {
        match c {
            EMPTY => "Erase / gas",
            STEEL => "Steel wall",
            COPPER => "Copper liner",
            CERAMIC => "Ceramic insulator",
            PIPE => "Pipe (holds whatever it connects to)",
            TANK_F => "Fuel tank",
            TANK_O => "Oxidiser tank",
            PUMP => "Pump (across shaft = impeller dia, along = stages)",
            VALVE => "Valve (open)",
            VALVE_X => "Valve (closed)",
            INJ => "Injector (opens into the chamber)",
            GG => "Gas generator (touch a fuel pipe and an oxidiser pipe)",
            AINJ => "Alternating injector (touch a fuel pipe and an oxidiser pipe: fuel and oxidiser from alternate cells)",
            HE_TANK => "Helium bottle (pressurant)",
            TURBINE => "Turbine (diameter sets blade speed, area its gas share)",
            SHAFT => "Shaft (links turbine and pumps)",
            JOINT => "Pipe joint (links back / front layer pipes)",
            SRC_GAS => "Gas source (keeps making gas)",
            SRC_FUEL => "Fuel vapour source (keeps making fuel vapour)",
            SRC_FIRE => "Fire source (keeps making fire)",
            NIOBIUM => "Niobium (radiation-cooled nozzle skirt)",
            REG => "Pressure regulator (helium line -> tank)",
            HGI => "Hot-gas injector (gas flows through, high to low pressure)",
            THROTTLE => "Throttle valve (metering: the throttle sets its opening)",
            MOTOR => "Electric motor (on a shaft, like a turbine; wire it to a battery)",
            BATTERY => "Battery (powers electric motors it's wired to)",
            WIRE => "Wire (joins batteries and motors)",
            BURNER => "Gas burner (fuel and oxidiser lines in, hot gas out of its open face)",
            TEATEB => "TEA-TEB bottle (pipe it through a valve to an injector: it lights the engine on contact)",
            CARTRIDGE => "Start cartridge (touch a turbopump shaft: it spins the pumps up at the start; size sets the power)",
            _ => "?",
        }
    }
    /// Everything liquid can flow through. Closed valves do not conduct.
    pub fn plumbing(c: u8) -> bool {
        matches!(c, PIPE | TANK_F | TANK_O | PUMP | VALVE | INJ | JOINT | THROTTLE)
    }
    /// Two touching plumbing cells join one network (a fuel and an oxidiser tank never merge directly).
    pub fn joins(a: u8, b: u8) -> bool {
        plumbing(a) && plumbing(b) && !((a == TANK_F && b == TANK_O) || (a == TANK_O && b == TANK_F))
    }
    pub fn is_source(c: u8) -> bool {
        matches!(c, SRC_GAS | SRC_FUEL | SRC_FIRE)
    }
    pub fn is_valve(c: u8) -> bool {
        c == VALVE || c == VALVE_X
    }
    /// Containers and plumbing that get a real one-cell steel wall around them when painted.
    /// Injectors and gas generators are left open: they are meant to face gas.
    pub fn auto_walls(c: u8) -> bool {
        matches!(c, PIPE | TANK_F | TANK_O | PUMP | VALVE | VALVE_X | HE_TANK | SHAFT | JOINT | THROTTLE | CARTRIDGE)
    }
    /// Network cells that spill their contents when they touch open gas (everything but injectors).
    pub fn can_leak(c: u8) -> bool {
        matches!(c, PIPE | TANK_F | TANK_O | PUMP | VALVE | JOINT | THROTTLE)
    }
    /// Batteries, wire and motors that touch make one electrical circuit.
    pub fn electric(c: u8) -> bool {
        matches!(c, MOTOR | BATTERY | WIRE)
    }
    pub fn is_rotor(c: u8) -> bool {
        matches!(c, PUMP | TURBINE | SHAFT | MOTOR)
    }
    pub fn is_solid(c: u8) -> bool {
        c != EMPTY
    }
    pub fn is_line(c: u8) -> bool {
        c == PIPE
    }
    pub fn rgb(c: u8) -> [u8; 3] {
        match c {
            STEEL => [120, 124, 134],
            COPPER => [196, 120, 70],
            CERAMIC => [214, 206, 180],
            PIPE => [150, 175, 210],
            TANK_F => [200, 130, 40],  // fuel tanks are drawn warm, oxidiser (liquid oxygen) tanks blue, as the liquids are
            TANK_O => [60, 100, 190],
            PUMP => [120, 105, 205],
            VALVE => [70, 200, 90],
            VALVE_X => [220, 60, 60],
            INJ => [180, 215, 255],
            GG => [240, 230, 80],
            AINJ => [200, 190, 235],
            HE_TANK => [225, 225, 235],
            TURBINE => [230, 140, 60],
            SHAFT => [90, 90, 100],
            JOINT => [230, 225, 140],
            SRC_GAS => [150, 165, 200],
            SRC_FUEL => [110, 200, 100],
            SRC_FIRE => [255, 120, 30],
            NIOBIUM => [150, 148, 172],
            REG => [235, 200, 120],
            HGI => [210, 90, 70],
            THROTTLE => [240, 170, 60],
            MOTOR => [70, 150, 200],
            BATTERY => [90, 160, 80],
            WIRE => [210, 120, 50],
            BURNER => [190, 110, 80],
            CARTRIDGE => [200, 170, 90],
            TEATEB => [90, 200, 120],
            _ => [20, 20, 28],
        }
    }
    pub fn t_melt(c: u8) -> f32 {
        match c {
            COPPER => 1350.0,
            CERAMIC => 2500.0,
            NIOBIUM => 2600.0,
            _ => 1700.0,
        }
    }
    pub fn sigma0(c: u8) -> f32 {
        match c {
            COPPER => 3.0e8,
            CERAMIC => 1.5e8,
            _ => 1.0e9,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub prop: usize,
    pub governor: bool,
    pub feed_p_max: f32, // governor holds pump discharge pressure below this
    pub valve_stroke: f32, // seconds for a valve to travel fully open <-> closed
    pub he_fill: f32, // pressure a newly drawn helium bottle is filled to, Pa
    pub boil_speed: f32, // boil-off speed-up (real boil-off takes minutes)
    pub fill_speed: f32, // fastest the liquid front runs down empty lines, m/s
    pub gse_tanks: bool, // ground support equipment keeps propellant tanks full
    pub gse_he: bool,    // ... and helium bottles at their fill pressure
    pub gse_batt: bool,  // ... and batteries charged
    pub src_p: f32,      // pressure source blocks push their gas out at, Pa
    pub relief: bool,    // tank vent valves hold boil-off near working pressure (~1.3x)
    pub dx: f32,
    pub axi: bool,
    pub throat_cells: f32,
    pub gg_area_f: f32,
    pub gg_area_o: f32,
    pub gg_p: f32,
    pub show_particles: bool,
    pub steps_per_frame: u32, // gas steps per 1/60 s of real time (independent of the display's frame rate)
    pub throttle: f32,        // throttle setting, 0..1: the opening commanded of every throttle valve
    pub mix_target: f32,      // mixture-ratio control: the O/F the throttle valves trim towards (0: off)
    pub fps_cap: f32,         // redraws per second at most
    pub heat_accel: f32,
    pub h0: f32,
    pub depth: f32,
    pub amb_p: f32,
    pub tank_p_f: f32,
    pub tank_p_o: f32,
    pub dp_stage: f32,
    pub turb_mult: f32,
    pub inj_area_f: f32, // cm^2 per injector cell
    pub inj_area_f2: f32, // ... for the second fuel's injectors, in a tri-propellant engine
    pub inj_area_o: f32,
    pub cartridge_w: f32,
    pub dead_start: bool, // tank-head start: no starter at all, the pumps bootstrap on the preburner gas (like the RS-25)
    pub he_spin: bool, // spin the turbopumps up on helium from the bottle instead of a start cartridge
    pub lumped_rate: f32,
    pub failures: bool,
    pub paused: bool,
}
impl Settings {
    pub fn dt(&self) -> f32 {
        DT0 * self.dx / DX0
    }
    pub fn p(&self) -> &'static Propellant {
        &PROPS[self.prop.min(PROPS.len() - 1)]
    }
    /// Injector weight: in a round engine orifice area scales with the ring a cell sits on.
    pub fn inj_w(&self, x: usize) -> f32 {
        if self.axi { self.depth_at(x) / (std::f32::consts::TAU * 10.0 * self.dx) } else { 1.0 }
    }
    /// Effective slab depth (m) of a cell in column x: constant for a slab, 2*pi*r for a round engine.
    pub fn depth_at(&self, x: usize) -> f32 {
        if self.axi {
            std::f32::consts::TAU * ((x as f32 + 0.5 - W as f32 / 2.0).abs() * self.dx)
        } else {
            self.depth
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            prop: 0,
            governor: true,
            feed_p_max: 2.6e7,
            valve_stroke: 0.4,
            he_fill: HE_FILL_P,
            boil_speed: 10.0,
            fill_speed: 10.0,
            gse_tanks: false,
            gse_he: false,
            gse_batt: false,
            src_p: 3.0e5,
            relief: true,
            dx: DX0,
            axi: false,
            throat_cells: 16.0,
            gg_area_f: 0.5,
            gg_area_o: 1.5,
            gg_p: 4.0e6,
            show_particles: false,
            steps_per_frame: 100,
            throttle: 1.0,
            mix_target: 0.0,
            fps_cap: 60.0,
            heat_accel: 1500.0,
            h0: 3.0,
            depth: 0.36,
            amb_p: 101325.0, // sea level; the Vacuum button drops it to ~0
            tank_p_f: 4.0e5,
            tank_p_o: 4.5e5,
            dp_stage: 0.6e7,
            turb_mult: 12.0,
            inj_area_f: 0.95,
            inj_area_f2: 0.1,
            inj_area_o: 4.5,
            cartridge_w: 2.0e7,
            dead_start: false,
            he_spin: false,
            lumped_rate: 1.0,
            failures: true,
            paused: false,
        }
    }
}

#[derive(Clone)]
pub struct Net {
    pub fp: &'static Propellant, // the properties of what this line carries (its fuel's pair, or the oxidiser's)
    pub fkind: u8,               // fuel lines: which fuel (index into FUELS)
    pub prop: u8,
    pub temp: f32,
    pub tank_cells: usize,
    pub lines: Vec<usize>,
    pub injectors: Vec<(usize, usize)>, // (injector cell, gas cell it feeds)
    pub valves: Vec<usize>,             // valve groups inside this network
    pub inertance: f32,                 // L/A of the liquid column, 1/m
    pub mass_frac: f32,
    pub mdot: f32, // total drawn from the tank (injectors + gas generator)
    pub m_inj: f32,
    pub heat_w: f32,
    pub turb_dp: f32,    // pressure the expander turbines this fuel line runs through take out of it, Pa
    pub p_tank: f32,
    pub head: f32,
    pub p_up: f32,
    pub p_line: f32, // includes water-hammer surge
    pub pc_f: f32,
    pub gg_draw: f32,
    pub flow_off: f32, // animation phase of the liquid in the lines
    pub valve_prev: f32, // valve opening last step (water hammer needs a closing valve)
    pub breaches: Vec<(usize, usize)>, // (container cell, open gas cell it spills into)
    pub leak: f32,       // kg/s escaping through breaches
    pub ull_pv: f32,     // ullage gas content p*V (J): boil-off adds to it, draining expands the space
    pub boil: f32,       // kg/s boiling off
    pub tank_faces: usize, // tank cell faces touching the outside (heat leaks in here)
    pub tank_top: usize, // a tank cell on the top row (where an overpressured tank splits)
    pub tank_y0: f32,    // tank rows, for drawing the liquid level
    pub tank_y1: f32,
    pub max_phase: f32,  // farthest cell from the tank
    pub front: f32,      // how far the liquid has filled the lines (cells from the tank)
    pub bottle: u16,     // helium lines: the bottle feeding them
    pub reg_bottle: u16, // tank networks: the bottle piped to them through a regulator
    pub reg_set: f32,    // ... and that regulator's own set pressure, Pa (0: the shared setting)
    pub throttle: bool,  // a throttle valve meters this network's flow to the injectors
    pub h0: f32,         // pumps on this network: shut-off head, Pa (cavitation included)
    pub kq: f32,         // ... and how fast it falls with flow: head = h0 - kq m^2
}

impl Net {
    pub fn pumps_ok(&self) -> bool {
        self.tank_cells > 0 && self.mass_frac > 0.0 && self.prop <= 1
    }
}

/// One pump block. Its size sets the physics: the side across the shaft is the impeller
/// diameter, the length along the shaft holds the stages (each about 0.6 diameters long).
#[derive(Clone)]
pub struct PumpRef {
    pub net: usize,
    pub prop: u8,
    pub d: f32,      // impeller diameter, m
    pub stages: f32, // number of stages
    pub head: f32,
    pub eff: f32,
    pub cav: f32,
    pub npsp_avail: f32,
    pub npsp_req: f32,
    pub power: f32,
    pub cells: Vec<usize>,
    pub phase: f32, // distance from the tank to the pump (it runs dry until liquid gets here)
    pub wet: bool,  // primed: liquid has reached it
}

impl PumpRef {
    pub fn tip(&self, omega: f32) -> f32 {
        omega * self.d * 0.5
    }
    pub fn eye_area(&self) -> f32 {
        std::f32::consts::FRAC_PI_4 * self.d * self.d
    }
    /// Mass flow at best efficiency for this speed.
    pub fn bep_flow(&self, omega: f32, rho: f32) -> f32 {
        rho * PHI_BEP * self.eye_area() * self.tip(omega)
    }
    /// Pump curve: head = stages * rho * (PSI u^2 - c (Q/A)^2), 10% below shut-off at the best-efficiency flow.
    pub fn head_at(&self, omega: f32, m: f32, rho: f32) -> f32 {
        let u = self.tip(omega);
        let ca = m / (rho * self.eye_area());
        self.stages * rho * (PSI * u * u - 0.1 * PSI / (PHI_BEP * PHI_BEP) * ca * ca)
    }
    /// Shut-off head (no flow) and the quadratic fall-off: head = h0 - kq * m^2, so the feed-line solver can
    /// take the pump curve implicitly.
    pub fn curve(&self, omega: f32, rho: f32) -> (f32, f32) {
        let u = self.tip(omega);
        let a = self.eye_area();
        (self.stages * rho * PSI * u * u, self.stages * 0.1 * PSI / (PHI_BEP * PHI_BEP * rho * a * a))
    }
    /// Best hydraulic efficiency: small impellers lose relatively more to leakage and wall friction
    /// (about 67% for a 5 cm impeller, 76% at 25 cm, 80% at 50 cm).
    pub fn eta_peak(&self) -> f32 {
        (0.86 - 0.1 * (0.15 / self.d.max(0.01)).sqrt()).clamp(0.45, 0.84)
    }
    /// Hydraulic efficiency: best at the design flow, falling off either side (recirculation at low flow,
    /// shock losses at high flow).
    pub fn hyd_eff(&self, omega: f32, m: f32, rho: f32) -> f32 {
        let r = m / self.bep_flow(omega, rho).max(1e-3);
        (self.eta_peak() * (1.0 - 0.55 * (r - 1.0).powi(2))).clamp(0.1, self.eta_peak())
    }
    /// Disc friction: the impeller shrouds churning the liquid around them, there even at zero flow.
    /// `rho` is what fills the casing (vapour if the pump has lost its prime).
    pub fn disc_friction(&self, omega: f32, rho: f32) -> f32 {
        let r = 0.5 * self.d;
        self.stages * 0.003 * rho * omega.powi(3) * r.powi(5)
    }
    /// Power churned away below the design flow: liquid recirculating in the impeller eye and diffuser. A
    /// pump run against a closed valve still takes about a third of its design power this way (and heats
    /// the liquid it traps), which is what keeps a dead-headed pump from running away.
    pub fn recirculation(&self, omega: f32, m: f32, rho: f32) -> f32 {
        let m_b = self.bep_flow(omega, rho);
        let r = m / m_b.max(1e-3);
        let p_bep = m_b * self.head_at(omega, m_b, rho).max(0.0) / (rho * self.eta_peak());
        0.35 * p_bep * (1.0 - r).max(0.0).powi(2)
    }
    /// Shaft power the pump takes: hydraulic power over its efficiency, recirculation at low flow and disc
    /// friction. A cavitating impeller is partly full of vapour and churns less.
    pub fn shaft_power(&self, omega: f32, m: f32, rho: f32, head: f32, wet: bool, cav: f32) -> f32 {
        let hyd = if wet { m * head.max(0.0) / (rho * self.hyd_eff(omega, m, rho)) + self.recirculation(omega, m, rho) * (0.3 + 0.7 * cav) } else { 0.0 };
        hyd + self.disc_friction(omega, if wet { rho } else { 0.01 * rho })
    }
    /// Overall efficiency (what the pump map shows): useful power over shaft power.
    pub fn efficiency(&self, omega: f32, m: f32, rho: f32) -> f32 {
        let h = self.head_at(omega, m, rho).max(0.0);
        let p = self.shaft_power(omega, m, rho, h, true, 1.0);
        if p > 1.0 { m * h / rho / p } else { 0.0 }
    }
    /// Suction pressure the inducer needs above vapour pressure: rises with tip speed and steeply with flow.
    pub fn npsp_needed(&self, omega: f32, m: f32, rho: f32) -> f32 {
        let u = self.tip(omega);
        let r = m / self.bep_flow(omega, rho).max(1e-3);
        NPSH_K * rho * u * u * (0.6 + 0.4 * r * r)
    }
    /// Head left when the suction pressure is short: none lost with enough margin, then the head
    /// breaks down quickly as the vapour cavities fill the impeller passages.
    pub fn cav_factor(avail: f32, need: f32) -> f32 {
        if need <= 1.0 {
            return 1.0;
        }
        let x = ((avail / need - 0.3) / 0.7).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }
}

/// Batteries, wire and motors joined together.
#[derive(Clone)]
pub struct Circuit {
    pub batt_cells: usize,
    pub cap: f32,    // J
    pub energy: f32, // J left
    pub p_max: f32,  // most the batteries can deliver, W
    pub p_draw: f32, // what the motors are drawing, W
    pub motors: usize,
}

#[derive(Clone)]
pub struct Shaft {
    pub cells: Vec<usize>,
    pub turb_cells: usize,
    pub pumps: Vec<PumpRef>,
    pub d_turb: f32,       // turbine wheel diameter, m
    pub turb_stages: f32,  // turbine stages (pressure-compounded): more stages suit slower blades
    pub omega_rated: f32,  // speed at which the largest wheel reaches the tip-speed limit
    pub horizontal: bool,  // shaft axis direction on screen
    pub j: f32,
    pub omega: f32,
    pub theta: f32,
    pub p_turb: f32,
    pub p_load: f32,
    pub eff_t: f32,
    pub u_c0: f32,
    pub turb_faces: Vec<usize>, // open cells around its turbine wheels
    pub turb_in: Vec<usize>,    // ... of which the inlet: where the drive gas arrives (it only flows from these)
    pub gas_driven: bool,       // a gas generator's exhaust is ducted to the turbine: it runs on that gas
    pub p_gas: f32,             // isentropic power in the gas going through the turbine, W
    pub c0_gas: f32,            // its spouting velocity, m/s
    pub regen_nets: Vec<usize>, // fuel networks whose pipes run into its turbine: their regen heat drives it (expander)
    pub p_exp: f32,             // shaft power the expander gas gave it last step, W
    pub motor_cells: usize,     // electric motor on this shaft (cells; 0: none)
    pub cart_cells: usize,      // start cartridge cells touching it (0: none)
    pub d_motor: f32,           // motor rotor diameter, m
    pub circuit: u16,           // the battery circuit it's wired to (NO_NET: none)
    pub p_motor: f32,           // shaft power the motor is giving it, W
}

impl Shaft {
    pub fn x(&self) -> f32 {
        self.omega / self.omega_rated
    }
    pub fn rpm(&self) -> f32 {
        self.omega * 60.0 / std::f32::consts::TAU
    }
}

/// Turbine efficiency from blade-speed ratio u/c0 (c0 = spouting velocity of the drive gas):
/// zero at standstill, best (75%) around u/c0 = 0.45.
/// How the mixture changes the heat the walls take from the gas, against the engine's best mixture ratio.
/// Running fuel-rich the extra fuel hugs the wall as a cooling film (and the gas is lighter): up to 30% less.
/// Running oxygen-rich there is no film at all and the hot streaks and oxidising gas are brutal on the walls:
/// about 3x at stoichiometric (for kerosene), 5.5x at twice the best mixture: enough to melt copper. (1 at the
/// best mixture.)
pub fn wall_heat_scale(of: f32, of_opt: f32) -> f32 {
    let r = of / of_opt.max(1e-3);
    if r < 1.0 {
        1.0 - 0.35 * (1.0 - r)
    } else {
        1.0 + 4.5 * (r - 1.0).min(1.0)
    }
    .clamp(0.7, 5.5)
}

/// True if any of these open cells holds gas hot enough to light propellant (above 1200 K): a flame, or
/// fire painted there, works as an igniter.
pub fn flame_at(cells: impl IntoIterator<Item = usize>, rb: &Readback, pr: &Propellant) -> bool {
    rb.valid
        && cells.into_iter().any(|j| {
            let s = rb.state[j];
            s[0] > 0.0 && gas_p(s, pr.gamma) / (s[0] * pr.rgas) > 1200.0
        })
}

/// Gas going through a block (hot-gas injector, turbine) from its high-pressure faces to its low ones.
pub struct Through {
    pub hi: Vec<usize>,
    pub lo: Vec<usize>,
    pub m: f32,   // kg/s
    pub p_h: f32, // intake pressure
    pub p_l: f32, // outlet pressure
    pub h_h: f32, // intake specific enthalpy, J/kg
    pub v: f32,   // outlet jet speed
}

/// Orifice flow through a block between the open cells around it, from the last read-back field.
pub fn through_flow(faces: &[usize], rb: &Readback, st: &Settings, gamma: f32, cap: f32) -> Option<Through> {
    if faces.len() < 2 {
        return None;
    }
    let ps: Vec<f32> = faces.iter().map(|&j| gas_p(rb.state[j], gamma)).collect();
    let (pmax, pmin) = ps.iter().fold((f32::MIN, f32::MAX), |(a, b), &p| (a.max(p), b.min(p)));
    if pmax - pmin < 1.0e3 {
        return None;
    }
    let mid = 0.5 * (pmax + pmin);
    let hi: Vec<usize> = faces.iter().zip(&ps).filter(|(_, p)| **p > mid).map(|(&j, _)| j).collect();
    let lo: Vec<usize> = faces.iter().zip(&ps).filter(|(_, p)| **p <= mid).map(|(&j, _)| j).collect();
    flow_between(hi, lo, rb, st, gamma, cap)
}

/// Orifice flow from the open cells on one side of a block (`hi`, upstream) to those on the other (`lo`),
/// if the upstream side is at the higher pressure.
/// `cap`: the most of the intake cells' gas it may take over one frame (a fraction).
pub fn flow_between(hi: Vec<usize>, lo: Vec<usize>, rb: &Readback, st: &Settings, gamma: f32, cap: f32) -> Option<Through> {
    if hi.is_empty() || lo.is_empty() {
        return None;
    }
    let avg = |v: &[usize], f: &dyn Fn([f32; 4]) -> f32| v.iter().map(|&j| f(rb.state[j])).sum::<f32>() / v.len() as f32;
    let rho_h = avg(&hi, &|s| s[0]).max(1e-4);
    let p_h = avg(&hi, &|s| gas_p(s, gamma));
    let p_l = avg(&lo, &|s| gas_p(s, gamma));
    if p_h - p_l < 1.0e3 {
        return None;
    }
    let h_h = avg(&hi, &|s| (s[3] + gas_p(s, gamma)) / s[0].max(1e-4));
    // the narrower side is the throat (e.g. a turbine's nozzle)
    let side = |v: &[usize]| v.iter().map(|&j| st.dx * st.depth_at(j % W)).sum::<f32>();
    let area = side(&lo).min(side(&hi));
    let mut m = 0.6 * area * (2.0 * rho_h * (p_h - p_l).max(0.0)).sqrt();
    // never take more than the gas in the intake cells over a frame (the same sources apply on every gas
    // step of the frame; the cells are refilled from upstream meanwhile)
    let cell_m: f32 = hi.iter().map(|&j| rb.state[j][0] * st.dx * st.dx * st.depth_at(j % W)).sum();
    let steps = st.steps_per_frame.max(1) as f32;
    m = m.min(cap * cell_m / (st.dt() * steps));
    if m <= 0.0 {
        return None;
    }
    let v = (2.0 * (p_h - p_l).max(0.0) / rho_h).sqrt().min(400.0);
    Some(Through { hi, lo, m, p_h, p_l, h_h, v })
}

/// Take the gas out of the intake cells and put it into the outlet cells with enthalpy `h_out` and an
/// unburnt fuel fraction `comb`.
pub fn apply_through(map: &mut std::collections::HashMap<usize, Src>, t: &Through, h_out: f32, comb: f32, rb: &Readback, st: &Settings) {
    let gamma = st.p().gamma;
    for &j in &t.hi {
        let s0 = rb.state[j];
        let dm = t.m / t.hi.len() as f32 * st.dt() / (st.dx * st.dx * st.depth_at(j % W));
        let r0 = s0[0].max(1e-4);
        let s = map.entry(j).or_insert(Src { cell: j as u32, ..Default::default() });
        s.drho -= dm;
        s.dmx -= dm * s0[1] / r0;
        s.dmy -= dm * s0[2] / r0;
        s.de -= dm * (s0[3] + gas_p(s0, gamma)) / r0;
    }
    let cx = t.hi.iter().map(|&j| (j % W) as f32).sum::<f32>() / t.hi.len() as f32;
    let cy = t.hi.iter().map(|&j| (j / W) as f32).sum::<f32>() / t.hi.len() as f32;
    for &j in &t.lo {
        let dm = t.m / t.lo.len() as f32 * st.dt() / (st.dx * st.dx * st.depth_at(j % W));
        let (dx, dy) = ((j % W) as f32 - cx, (j / W) as f32 - cy);
        let n = (dx * dx + dy * dy).sqrt().max(1e-3);
        let s = map.entry(j).or_insert(Src { cell: j as u32, ..Default::default() });
        s.drho += dm;
        s.dmx += dm * t.v * dx / n;
        s.dmy += dm * t.v * dy / n;
        s.de += dm * (h_out + 0.5 * t.v * t.v);
        s.dphi += dm * comb;
    }
}

pub fn turbine_eff(u_c0: f32, stages: f32) -> f32 {
    let (opt, peak) = turbine_opt(stages);
    let a = (u_c0 / opt).clamp(0.0, 2.0);
    peak * (2.0 * a - a * a).max(0.0)
}

/// Best blade-speed ratio u/c0 and the efficiency there for a pressure-compounded turbine: the gas
/// expands a little in each stage, so each stage's jet is slower and the best blade speed falls as
/// 1/sqrt(stages); the extra stages also recover a little of the leaving loss.
pub fn turbine_opt(stages: f32) -> (f32, f32) {
    let n = stages.clamp(1.0, 3.0);
    (0.47 / n.sqrt(), 0.74 + 0.04 * (n - 1.0))
}

/// Torque on a turbine wheel of radius `r` turning at `omega` from drive gas carrying isentropic power
/// `p` with spouting velocity `c0`: highest at standstill and falling linearly with blade speed (power =
/// torque * speed reproduces turbine_eff), so drive gas alone can start a rotor.
pub fn turbine_torque(p: f32, c0: f32, r: f32, omega: f32, stages: f32) -> f32 {
    if r <= 0.0 || p <= 0.0 {
        return 0.0;
    }
    let (opt, peak) = turbine_opt(stages);
    let a = (omega * r / c0 / opt).min(2.0);
    p * peak * (2.0 - a) * r / (opt * c0)
}

/// A pump or turbine block as drawn: its bounding box, axis and stage count.
/// One helium bottle: touching helium cells, each bottle with its own pressure.
pub const TEA_FLUID: u8 = 200; // the fluid marker of a TEA-TEB bottle's cells
pub const TEA_FILL: f32 = 4.0e6; // what a TEA-TEB bottle starts at, Pa

#[derive(Clone)]
pub struct HeBottle {
    pub tea: bool, // holds TEA-TEB, not helium
    pub cells: usize,
    pub p: f32,  // current pressure, Pa
    pub p0: f32, // pressure it was filled to
}

impl HeBottle {
    pub fn volume(&self) -> f32 {
        self.cells as f32 * HE_CELL_VOL
    }
    pub fn mass(&self) -> f32 {
        self.p * self.volume() / HE_RT
    }
}

#[derive(Clone)]
pub struct Block {
    pub x0: f32,
    pub y0: f32,
    pub w: f32,
    pub h: f32,
    pub horizontal: bool,
    pub stages: f32,
    pub shaft: usize,
}

#[derive(Clone)]
pub struct ValveGroup {
    pub tea: bool, // a TEA-TEB line's valve
    pub cells: Vec<usize>,
    pub prop: u8,
    pub cmd: bool,
    pub pos: f32,
}

impl ValveGroup {
    /// (the world's valve list says what each carries: see World::valve_label)
    pub fn label(&self) -> String {
        let i = self.cells[0];
        let what = match self.prop {
            0 => "Fuel",
            1 => "Oxidiser",
            PROP_HE => "Helium",
            _ => "Unconnected",
        };
        format!("{what} valve at ({}, {})", i % W, i / W)
    }
}

#[derive(Clone, Copy)]
pub enum SeqAction {
    Valves(u8, bool),
    TeaValves(bool),
}

#[derive(Default, Clone, Copy)]
pub struct Gauges {
    pub wall_scale: f32, // how much the mixture scales the heat the walls take (1: best mixture)
    pub chamber_p: f32,
    pub thrust: f32,
    pub mdot_f: f32,
    pub mdot_o: f32,
    pub isp: f32,
    pub rpm: f32,
    pub of: f32,
    pub t_flame: f32,
    pub max_wall_t: f32,
    /// the wall cell closest to its own melting point: its temperature, the material and that material's melting point
    pub hot_t: f32,
    pub hot_mat: u8,
    pub hot_melt: f32,
    pub q_regen: f32,
    pub p_turb: f32,
    pub p_pump: f32,
    pub tank_f: f32,
    pub tank_o: f32,
    pub p_gg: f32,
    pub t_gg: f32,
    pub p_reg: f32,
    pub he_frac: f32,
    pub ignited: bool,
    pub t_cool: f32,
    pub t_cool_max: f32,
    pub cavitating: bool,
    pub no_flame: &'static str, // why the chamber is not lit
    pub leak: f32,              // kg/s escaping from breached containers
    pub leak_thrust: f32,       // upward thrust from jets out of holes, N
    pub leak_side: f32,         // sideways thrust from those jets (+ = right), N
    pub p_line_max: f32,
}

/// One aggregated mass/energy source for a gas cell, expressed as per-step increments.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default)]
pub struct Src {
    pub cell: u32,
    pub drho: f32,
    pub dmx: f32,
    pub dmy: f32,
    pub de: f32,
    pub dphi: f32, // combustible (unburnt) mass added
    pub _p: [f32; 2],
}

#[derive(Default)]
pub struct Readback {
    pub state: Vec<[f32; 4]>,
    pub flux: Vec<f32>,
    pub sim_time: f32,
    pub valid: bool,
}

pub fn gas_p(s: [f32; 4], gamma: f32) -> f32 {
    let r = s[0].max(1e-6);
    let ke = 0.5 * (s[1] * s[1] + s[2] * s[2]) / r;
    ((gamma - 1.0) * (s[3] - ke)).max(0.0)
}

#[derive(Clone)]
pub struct World {
    pub cells: Vec<u8>,
    pub net_of: Vec<u16>,
    pub back: Vec<u8>,  // pipe layer behind the engine plane
    pub front: Vec<u8>, // pipe layer in front of it
    pub layer_net: [Vec<u16>; 2],
    pub nets: Vec<Net>,
    pub shafts: Vec<Shaft>,
    pub circuits: Vec<Circuit>,
    pub wet_sig: u64,         // which stretches of line hold liquid, as a number: a change re-sends the materials
    pub wall_h_scale: f32,    // the mixture's effect on wall heating, as the gas solver uses it
    pub circuit_of: Vec<u16>,
    pub motors_on: bool, // electric motors running (the oxidiser valves are open or the engine is lit)
    pub shaft_of: Vec<u16>,
    pub blocks: Vec<Block>,
    pub block_of: Vec<u8>,
    pub valves: Vec<ValveGroup>,
    pub valve_dests: Vec<u8>, // where each valve leads: bit 0 the main chamber, bit 1 a gas generator (see compute_valve_dests)
    pub valve_of: Vec<u16>,
    pub phase: Vec<f32>,
    pub phase_dirty: bool,
    pub gg: Vec<(usize, usize, usize, usize)>, // (fuel net, ox net, fuel feed cell, ox feed cell) per gas generator cell
    pub hgi_faces: Vec<Vec<usize>>, // per hot-gas injector block: the open cells around it
    pub reg_p: Vec<f32>, // per regulator cell: its set pressure, Pa (0: follow the shared setting)
    pub mixed: Option<(usize, bool)>, // fuel and oxidiser lines joined: a cell where, and whether it's at injectors
    pub regs: Vec<(usize, u8, bool)>, // each regulator: a cell of it, the propellant of the tank it feeds (255: none), piped from a bottle
    pub fluid: Vec<u8>,  // per tank cell: what it holds, 1 + index into FUELS / OXIDISERS (0: not chosen yet)
    pub thrust_f: f32,          // thrust averaged over ~0.5 s, as a test-stand load cell reads it
    pub open_gas: Vec<bool>, // gas connected to the edge of the world (chamber, nozzle, plume, vents)
    pub gas_pipes: Vec<Vec<usize>>, // pipework with no tank or bottle that opens onto gas: the open cells at its ends
    pub hgi_down: Vec<Vec<usize>>,  // ... and those on its outlet side (opening into a big space): it only flows into them
    pub gg_comb: f32,
    pub burner_view: Vec<(f32, bool)>, // per drawn block (burners): the outlet end (+1 / -1 along the long axis) and whether the fuel line is on the low side; known from the layout, not from the generator list (which is empty while the feed valves are shut)
    pub gg_lumped: Vec<bool>, // per gas generator: a burner block (burns by its own equations) rather than an injector plate
    pub gg_cell0: Vec<usize>, // ... one of its cells (to find its drawn block)
    pub gg_dir: Vec<f32>,     // ... which end of its block the outlet is, along the block's long axis (+1 / -1)
    pub gg_fneg: Vec<bool>,   // ... whether its fuel line comes in on the low-coordinate side of the block's cross axis
    pub gg_vis: Vec<(f32, f32)>, // ... how hard it is burning (0..1, smoothed) and its gas temperature / 3500 K, for the picture
    pub gg_ducted: Vec<bool>, // per gas generator: its exhaust is ducted to a turbine               // unburnt fuel fraction of the gas generators' fuel-rich exhaust
    pub gg_faces: Vec<Vec<(usize, usize)>>, // per gas generator: (head cell, open cell it burns into)
    pub priming: f32, // seconds spent priming the fuel lines during a start (negative: not priming)
    pub dirty: Vec<usize>,
    pub nets_dirty: bool,
    pub hold_rebuild: bool, // mid paint stroke: leave the networks as they are for now (rebuilt when it ends)
    pub frame_steps: u32,   // gas steps the GPU will actually run this frame (0: the setting's worth)
    pub throttle_pos: f32,  // where the throttle valves actually are: they travel to the setting at the valve stroke speed
    pub mix_k: f32,         // mixture control: above 1 the fuel throttle valves close down, below 1 the oxidiser ones
    pub teateb_flash: f32,   // a TEA-TEB injection is burning: 1 at the start, dying away over a second or two
    pub teateb_at: usize,    // ... in this gas cell
    pub tea_age: f32,        // seconds since the TEA-TEB first reached an injector (the exhaust burns green for the first second or so)
    pub purge_vis: f32,      // the cold-gas purge cloud at the nozzle exit (0..1): rises while the start purge runs, thins away after it
    pub stab_geom: Option<(f32, f32)>, // where the jet stabiliser starts: the exit row and the half width of the exhaust (also found for an aerospike, which has no bell for the plume code to find)
    pub stable_jet: bool,   // damp the exhaust's sideways swinging far below the nozzle (the jet stabiliser, see sim.wgsl)
    pub pc_ref: f32,        // the chamber pressure the engine has been running at lately (a slowly decaying peak), to find how far it is throttled
    pub fx_style: u32,      // which Mach-diamond style the plume shader draws (see PLUME_STYLES)
    pub fx_plume: bool,     // draw the exhaust as a plume shader (off: just the gas simulation's own picture)
    pub noz: Vec<f32>,      // (left, right) gas cell of each row of the main engine's passage, from the injector row (see ANIM_NOZ)
    pub plume: [f32; 18],   // what the plume shader needs (see ANIM_PLUME)
    pub air_phase: f32,     // how far the air streaks have travelled (adds up the wind, so a change of speed never makes them jump)
    pub flight: bool,       // the vehicle flies: thrust, weight and drag move it, and the altitude follows
    pub vel: f32,           // vertical speed, m/s (up is positive)
    pub veh_mass: f32,      // vehicle mass now, kg (burns off with the propellant it uses)
    pub dry_mass: f32,      // ... down to this
    pub flight_msg: String, // how the last landing went
    pub alt_on: bool,       // the altitude sets the ambient pressure (otherwise the ambient pressure setting does)
    pub altitude: f32,      // km
    pub climbing: bool,     // the altitude rises by itself
    pub climb_rate: f32,    // km per second of simulated time
    pub amb_prev: f32,      // the ambient pressure a moment ago, to see it change
    pub amb_hot: f32,       // seconds left in which the air in the world is following a change
    pub amb_k: f32,         // the rate it follows at (to the gas solver)
    pub run_t: f32,         // simulated seconds since the engine was last started (the test stand's clock)
    pub run_serial: u32,    // counts the starts: a new number means a new run
    pub soft_t: f32,        // soft shutdown under way: seconds left in which valves close slowly
    pub shut_thr: f32,      // throttle ceiling: soft start and shutdown hold the engine down (1: no effect)
    pub purge_t: f32,       // start: seconds of inert-gas purge left before the fuel valves open
    pub start_t: f32,       // soft start under way (seconds left at most): valves open slowly
    pub thr_valves: [f32; 2], // how far the fuel and oxidiser throttle valves are open now
    pub mat_stale: bool,
    pub st: Settings,
    pub gg_power: f32, // isentropic gas power offered to the turbines
    pub ignited: bool,
    pub gg_lit: bool,
    pub gg_hot_t: f32,
    pub spark_t: f32,
    pub igniter: bool, // igniter switched on (stays armed until switched off)
    pub gas_paint: Vec<(usize, u8)>, // gas painted this frame: 0 inert gas, 1 fuel vapour, 2 fire
    pub lack_t: f32,
    pub he_frac: f32,
    pub he_cells: usize,
    pub he_bottles: Vec<HeBottle>,
    pub he_of: Vec<u16>,
    pub he_breaches: Vec<(usize, usize, usize)>, // (bottle cell, open gas cell, bottle)
    pub sources: Vec<(usize, usize, u8)>,        // (source cell, open gas cell, kind)
    pub leaking: bool,
    pub deadhead_t: f32,    // tank-head start: seconds left in which the main injectors are held back while the pumps spin up
    pub cartridge_t: f32,
    pub seq: Vec<(f32, SeqAction)>,
    pub seq_t: f32,
    pub g: Gauges,
    pub srcs: Vec<Src>,
    pub events: Vec<String>,
}

impl World {
    pub fn new() -> World {
        let mut w = World {
            cells: vec![0; N],
            net_of: vec![NO_NET; N],
            back: vec![0; N],
            front: vec![0; N],
            layer_net: [vec![NO_NET; N], vec![NO_NET; N]],
            nets: vec![],
            shafts: vec![],
            circuits: vec![],
            wall_h_scale: 1.0,
            wet_sig: 0,
            circuit_of: vec![NO_NET; N],
            motors_on: false,
            shaft_of: vec![NO_NET; N],
            blocks: vec![],
            block_of: vec![255; N],
            valves: vec![],
            valve_dests: vec![],
            valve_of: vec![NO_NET; N],
            phase: vec![0.0; N],
            phase_dirty: true,
            gg: vec![],
            priming: -1.0,
            gg_faces: vec![],
            hgi_faces: vec![],
            hgi_down: vec![],
            gas_pipes: vec![],
            open_gas: vec![true; N],
            thrust_f: 0.0,
            reg_p: vec![0.0; N],
            regs: vec![],
            mixed: None,
            fluid: vec![0; N],
            gg_comb: 0.0,
            gg_ducted: vec![],
            gg_lumped: vec![],
            burner_view: vec![],
            gg_cell0: vec![],
            gg_dir: vec![],
            gg_vis: vec![],
            gg_fneg: vec![],
            dirty: vec![],
            nets_dirty: true,
            hold_rebuild: false,
            frame_steps: 0,
            throttle_pos: 1.0,
            mix_k: 1.0,
            soft_t: 0.0,
            teateb_flash: 0.0,
            teateb_at: 0,
            tea_age: 99.0,
            purge_vis: 0.0,
            stab_geom: None,
            stable_jet: true,
            pc_ref: 0.0,
            fx_style: 6, // (the style matched to photographs: see PLUME_STYLES)
            fx_plume: true,
            noz: Vec::new(),
            plume: [0.0; 18],
            air_phase: 0.0,
            flight: false,
            vel: 0.0,
            veh_mass: 28000.0,
            dry_mass: 22000.0,
            flight_msg: String::new(),
            alt_on: false,
            altitude: 0.0,
            climbing: false,
            climb_rate: 0.5,
            amb_prev: 101325.0,
            amb_hot: 0.0,
            amb_k: 0.0,
            run_t: 0.0,
            run_serial: 0,
            shut_thr: 1.0,
            purge_t: 0.0,
            start_t: 0.0,
            thr_valves: [1.0, 1.0],
            mat_stale: true,
            st: Settings::default(),
            gg_power: 0.0,
            ignited: false,
            gg_lit: false,
            gg_hot_t: 0.0,
            spark_t: 0.0,
            igniter: false,
            gas_paint: vec![],
            lack_t: 0.0,
            he_frac: 1.0,
            he_cells: 0,
            he_bottles: vec![],
            he_of: vec![NO_NET; N],
            he_breaches: vec![],
            sources: vec![],
            leaking: false,
            deadhead_t: 0.0,
            cartridge_t: 0.0,
            seq: vec![],
            seq_t: 0.0,
            g: Gauges::default(),
            srcs: vec![],
            events: vec![],
        };
        w.preset();
        w
    }

    pub fn idx(x: usize, y: usize) -> usize {
        y * W + x
    }

    pub fn set_cell(&mut self, i: usize, v: u8) {
        if self.cells[i] != v {
            self.cells[i] = v;
            self.dirty.push(i);
            self.nets_dirty = true;
        }
    }

    /// Command a valve group open or closed; it then strokes over `valve_stroke` seconds.
    pub fn command_valve(&mut self, g: usize, open: bool) {
        if let Some(v) = self.valves.get_mut(g) {
            v.cmd = open;
        }
    }

    pub fn command_all(&mut self, prop: u8, open: bool) {
        for v in &mut self.valves {
            if v.prop == prop && !v.tea {
                v.cmd = open;
            }
        }
    }

    /// Start, as real engines do it: purge first (inert gas blown through the injectors clears out any
    /// propellant vapour left in the chamber), then open the fuel valves slowly and prime the lines (fuel
    /// has to run through the pipes, and the cooling jacket on regen engines, before it reaches the
    /// injectors). Once every fuel injector is wet the ignition part follows: oxidiser valve, igniter and
    /// turbopump start cartridge. Valves open slowly the whole way and the throttle valves hold the engine
    /// at 40% until it has lit, then bring it up, so nothing surges.
    pub fn start_sequence(&mut self) {
        self.run_t = 0.0;
        self.run_serial += 1;
        self.soft_t = 0.0;
        self.shut_thr = 0.4;
        if self.nets_dirty {
            self.rebuild_nets();
        }
        self.purge_t = PURGE_S;
        self.start_t = 30.0;
        self.seq = vec![(PURGE_S, SeqAction::Valves(0, true))];
        self.seq_t = 0.0;
        self.priming = -1.0;
        self.spark_t = 0.0;
        self.cartridge_t = 0.0;
        self.event("start: purging the injectors and chamber".into());
    }

    #[cfg(test)]
    pub fn ignition_sequence_for_test(&mut self) {
        self.ignition_sequence();
    }

    /// The lines are primed: oxidiser, igniter and start cartridge (gas-generator engines give the
    /// generator 1.2 s to spin the pumps up before the oxidiser valve lets the main chamber ignite).
    fn ignition_sequence(&mut self) {
        self.priming = -1.0;
        if self.gg.is_empty() {
            self.seq = vec![(0.0, SeqAction::Valves(1, true))];
            self.spark_t = 2.5;
        } else {
            self.seq = vec![(1.2, SeqAction::Valves(1, true))];
            self.spark_t = 3.0;
        }
        self.seq_t = 0.0;
        // gas-generator engines burn the cartridge longer: the generator only takes over once the
        // chamber pressure has built up and throttled the pump flow back
        self.cartridge_t = if self.gg.is_empty() { 3.0 } else { 5.0 };
        // a helium start line (a pipe from a bottle through a valve into the turbine's inlet) opens for the start
        // a TEA-TEB line is the igniter: its valve opens for a moment, and what it sprays lights on contact
        if self.teateb_line() {
            self.spark_t = 0.0;
            self.seq.push((0.0, SeqAction::TeaValves(true)));
            self.seq.push((1.6, SeqAction::TeaValves(false)));
            self.event("start: TEA-TEB valve open".into());
        }
        if self.helium_start_line() {
            self.seq.push((0.0, SeqAction::Valves(PROP_HE, true)));
            self.seq.push((self.cartridge_t.max(4.0) + 0.5, SeqAction::Valves(PROP_HE, false)));
            self.event("start: helium start valve open, helium spinning the turbine".into());
        }
        if self.st.dead_start {
            // no starter: propellant runs through on tank pressure, the preburners light and their gas spins the
            // pumps up against a held-back main chamber (a dead-headed pump), which opens up as the speed comes
            self.cartridge_t = 0.0;
            self.deadhead_t = 14.0;
            self.event("tank-head start: the preburners spin the pumps up, main injection held back".into());
        }
        self.event("fuel at the injectors: ignition".into());
    }

    /// The build has a TEA-TEB bottle with a valve ending in an injector: a physical igniter.
    pub fn teateb_line(&self) -> bool {
        self.valves.iter().any(|v| {
            v.tea
                && v.cells.iter().any(|&i| {
                    let (x, y) = (i % W, i / W);
                    [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))].into_iter().any(|(nx, ny)| nx < W && ny < H && self.cells[Self::idx(nx, ny)] == c::INJ)
                })
        })
    }

    /// The build has a helium line with a valve ending in an injector: a physical helium start.
    pub fn helium_start_line(&self) -> bool {
        self.valves.iter().any(|v| {
            v.prop == PROP_HE
                && !v.tea
                && v.cells.iter().any(|&i| {
                    let (x, y) = (i % W, i / W);
                    [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))].into_iter().any(|(nx, ny)| nx < W && ny < H && self.cells[Self::idx(nx, ny)] == c::INJ)
                })
        })
    }

    /// Nothing is running: no valve commanded open, no start sequence, igniter off, no flame.
    pub fn idle(&self) -> bool {
        !self.ignited && !self.igniter && self.seq.is_empty() && self.priming < 0.0 && self.valves.iter().all(|v| !v.cmd)
    }

    /// Every fuel injector fed from a tank has liquid at it.
    pub fn fuel_primed(&self) -> bool {
        let mut fed = self.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0 && !n.injectors.is_empty()).peekable();
        fed.peek().is_some() && fed.all(|n| n.injectors.iter().all(|&(i, _)| self.phase[i] <= n.front))
    }

    /// Shutdown: fuel valves first, oxidiser 200 ms later.
    /// Soft shutdown, as real engines do it: throttle down first (if there are throttle valves), then close
    /// the oxidiser (the gas generator's too, which takes the turbines' power away), then a moment later
    /// the fuel, so the engine ends fuel-rich and the fuel keeps cooling the walls; every valve closes
    /// slowly, so no water hammer.
    pub fn shutdown(&mut self) {
        let thr = self.nets.iter().any(|n| n.throttle);
        let t_ox = if thr { 1.0 } else { 0.0 };
        self.seq = vec![(t_ox, SeqAction::Valves(1, false)), (t_ox + 1.8, SeqAction::Valves(0, false))];
        self.soft_t = t_ox + 1.8 + 2.0;
        self.seq_t = 0.0;
        self.spark_t = 0.0;
        self.igniter = false;
        self.priming = -1.0;
        self.purge_t = 0.0;
        self.start_t = 0.0;
    }

    /// Paint free gas into a cell: 0 inert gas, 1 fuel vapour, 2 fire, 3 ambient (what erasing leaves
    /// behind). Gas, fuel and fire only fill open space and leave anything solid alone; erasing clears it.
    pub fn paint_gas(&mut self, i: usize, kind: u8) {
        if kind != 3 && self.cells[i] != c::EMPTY {
            return;
        }
        self.set_cell(i, c::EMPTY);
        self.gas_paint.push((i, kind));
    }

    /// Put every helium bottle back to the pressure it was filled to.
    /// Fill the bottle a cell belongs to (used by the "new helium bottle" prompt).
    pub fn set_bottle_pressure(&mut self, cell: usize, p: f32) {
        if self.nets_dirty {
            self.rebuild_nets();
        }
        let b = self.he_of[cell];
        if b != NO_NET {
            let b = &mut self.he_bottles[b as usize];
            b.p = p;
            b.p0 = p;
        }
    }

    /// Top up the propellant tank network a cell belongs to.
    pub fn refill_tank(&mut self, cell: usize) -> bool {
        if self.nets_dirty {
            self.rebuild_nets();
        }
        let n = self.net_of[cell];
        if n != NO_NET && self.nets[n as usize].tank_cells > 0 {
            self.nets[n as usize].mass_frac = 1.0;
            return true;
        }
        false
    }

    /// Charge every battery pack full again.
    pub fn recharge(&mut self) {
        for cc in &mut self.circuits {
            cc.energy = cc.cap;
        }
    }

    /// Charge the pack the battery (or motor, or wire) at this cell belongs to. False if it isn't part of one.
    pub fn recharge_at(&mut self, cell: usize) -> bool {
        match self.circuits.get_mut(self.circuit_of[cell] as usize) {
            Some(cc) if cc.cap > 0.0 => {
                cc.energy = cc.cap;
                true
            }
            _ => false,
        }
    }

    pub fn refill_helium(&mut self) {
        for b in &mut self.he_bottles {
            b.p = b.p0;
        }
        self.he_frac = 1.0;
    }

    pub fn event(&mut self, s: String) {
        self.events.push(s);
        if self.events.len() > 6 {
            self.events.remove(0);
        }
    }

    /// Set the pressure a regulator holds its tank at (every touching regulator cell); 0 returns it to the
    /// shared setting.
    pub fn set_regulator(&mut self, i: usize, p: f32) {
        if self.cells[i] != c::REG {
            return;
        }
        for comp in self.components(|j| self.cells[j] == c::REG, |_, _| true) {
            if comp.contains(&i) {
                for j in comp {
                    self.reg_p[j] = p;
                }
            }
        }
        self.nets_dirty = true;
    }

    pub fn clear(&mut self) {
        self.wall_h_scale = 1.0;
        for i in 0..N {
            self.cells[i] = 0;
            self.reg_p[i] = 0.0;
            self.fluid[i] = 0;
            self.back[i] = 0;
            self.front[i] = 0;
        }
        self.dirty.clear();
        self.nets_dirty = true;
        self.shafts.clear();
        self.shaft_of = vec![NO_NET; N];
        self.valves.clear();
        self.valve_of = vec![NO_NET; N];
        self.seq.clear();
        self.ignited = false;
    }

    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: u8) {
        for y in y0.max(0)..=y1.min(H as i32 - 1) {
            for x in x0.max(0)..=x1.min(W as i32 - 1) {
                self.cells[Self::idx(x as usize, y as usize)] = v;
            }
        }
    }

    /// Default engine: the hydrolox expander preset (see presets.rs).
    pub fn preset(&mut self) {
        self.load_preset(0);
    }

    /// Hydrolox gas-generator preset.
    pub fn preset_gg(&mut self) {
        self.load_preset(1);
    }

    pub(crate) fn components(&self, pred: impl Fn(usize) -> bool, same: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
        let mut seen = vec![false; N];
        let mut out = vec![];
        let mut stack = vec![];
        for s in 0..N {
            if seen[s] || !pred(s) {
                continue;
            }
            let mut comp = vec![];
            seen[s] = true;
            stack.push(s);
            while let Some(i) = stack.pop() {
                comp.push(i);
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                        continue;
                    }
                    let j = Self::idx(nx as usize, ny as usize);
                    if !seen[j] && pred(j) && same(i, j) {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            out.push(comp);
        }
        out
    }

    /// The propellant pair the engine burns: whatever most of the fuel-tank cells and most of the
    /// oxidiser-tank cells hold (tanks not given contents count as the current pair's).
    pub fn derive_propellants(&mut self) {
        let (f0, o0) = pair_kinds(self.st.prop);
        let (mut nf, mut no) = ([0usize; 4], [0usize; 2]);
        for i in 0..N {
            match self.cells[i] {
                c::TANK_F => nf[if self.fluid[i] > 0 { (self.fluid[i] as usize - 1).min(3) } else { f0 }] += 1,
                c::TANK_O => no[if self.fluid[i] > 0 { (self.fluid[i] as usize - 1).min(1) } else { o0 }] += 1,
                _ => {}
            }
        }
        let pick = |n: &[usize], cur: usize| if n.iter().all(|&k| k == 0) { cur } else { (0..n.len()).max_by_key(|&k| (n[k], k == cur)).unwrap() };
        let prop = pair_index(pick(&nf, f0), pick(&no, o0));
        if prop != self.st.prop {
            self.st.prop = prop;
            self.event(format!("propellants from the tanks: {}", PROPS[prop].name));
        }
    }

    /// Give a whole tank (the touching tank cells of that kind) new contents, full.
    pub fn set_tank_fluid(&mut self, cell: usize, kind: u8) {
        let t = self.cells[cell];
        if !matches!(t, c::TANK_F | c::TANK_O) {
            return;
        }
        for comp in self.components(|j| self.cells[j] == t, |_, _| true) {
            if comp.contains(&cell) {
                for j in comp {
                    self.fluid[j] = kind + 1;
                }
            }
        }
        self.nets_dirty = true;
        self.rebuild_nets();
        let n = self.net_of[cell];
        if n != NO_NET {
            self.nets[n as usize].mass_frac = 1.0;
            self.nets[n as usize].ull_pv = 0.0;
        }
    }

    /// What a tank cell holds (0-based index into FUELS or OXIDISERS).
    /// The burn right now: the primary pair, or, with fuel tanks of different fluids, the pairs blended by the
    /// share of the fuel flow each line is giving (by tank contents when nothing is flowing yet).
    pub fn blended_prop(&self) -> Propellant {
        let (_, o0) = pair_kinds(self.st.prop);
        let mut w = [0.0f32; 4];
        for n in self.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0) {
            w[(n.fkind as usize).min(3)] += n.m_inj.max(0.0);
        }
        if w.iter().sum::<f32>() < 1e-3 {
            for n in self.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0) {
                w[(n.fkind as usize).min(3)] += n.tank_cells as f32;
            }
        }
        if w.iter().sum::<f32>() < 1e-3 {
            return PROPS[self.st.prop.min(PROPS.len() - 1)].clone();
        }
        blend_pairs(o0, &w)
    }

    /// More than one fuel in the tanks: a tri-propellant engine.
    pub fn tripropellant(&self) -> bool {
        self.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0).map(|n| n.fkind).collect::<std::collections::HashSet<_>>().len() > 1
    }

    /// The propellants' name for display: the pair, or the fuels burnt together.
    pub fn prop_name(&self) -> String {
        if self.tripropellant() {
            let (_, o0) = pair_kinds(self.st.prop);
            let mut kinds: Vec<u8> = self.nets.iter().filter(|n| n.prop == 0 && n.tank_cells > 0).map(|n| n.fkind).collect();
            kinds.sort();
            kinds.dedup();
            let fuels: Vec<&str> = kinds.iter().map(|&k| FUELS[k as usize].split(' ').next().unwrap_or("?")).collect();
            format!("{} / {} (tri-propellant)", fuels.join(" + "), OXIDISERS[o0].split(' ').next().unwrap_or("?"))
        } else {
            PROPS[self.st.prop.min(PROPS.len() - 1)].name.to_string()
        }
    }

    /// GPU parameters with the burn's gas constants and the mixture's effect on wall heating folded in.
    pub fn sim_params(&self) -> crate::gpu::SimParams {
        let mut p = crate::gpu::SimParams::from_settings(&self.st);
        let pr = self.blended_prop();
        p.q_heat = pr.rgas / (pr.gamma - 1.0) * (pr.t_peak - 300.0);
        p.gamma = pr.gamma;
        p.rgas = pr.rgas;
        p.h0 *= self.wall_h_scale;
        p.amb_k = self.amb_k;
        p.amb_v = self.wind();
        // the jet stabiliser starts one exit diameter below the nozzle exit
        if let Some((ye, hw)) = self.stab_geom {
            p.stab_y = ye + 2.0 * hw;
            p.stab_r = 10.0 * hw; // fully on about six exit diameters down
            p.stab_k = if self.stable_jet { JET_STABILISER } else { 0.0 };
        }
        p
    }

    /// The propellant line a part belongs to: its own network, or (a closed valve splits one) the one beside it.
    pub fn net_around(&self, i: usize) -> Option<usize> {
        // (right after a load the network table can be older than the cells: look nets up safely)
        let ok = |j: usize| (self.net_of[j] != NO_NET).then_some(self.net_of[j] as usize).filter(|&n| self.nets.get(n).is_some_and(|n| n.prop <= 1));
        if let Some(n) = ok(i) {
            return Some(n);
        }
        let (x, y) = (i % W, i / W);
        [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))].into_iter().filter(|&(nx, ny)| nx < W && ny < H).find_map(|(nx, ny)| ok(Self::idx(nx, ny)))
    }

    /// What a burner block is doing, for the hover readout: fuel and oxidiser kg/s, mixture, gas temperature.
    pub fn burner_info(&self, cell: usize) -> Option<String> {
        let g = (0..self.gg_cell0.len()).find(|&g| self.gg_lumped[g] && self.block_of[self.gg_cell0[g]] == self.block_of[cell])?;
        let (fi, oi, _, _) = *self.gg.get(g)?;
        let (mf, mo) = (self.nets[fi].gg_draw, self.nets[oi].gg_draw);
        if mf + mo < 1e-3 {
            return Some("not burning: no fuel and oxidiser reaching it".to_string());
        }
        let pr = self.blended_prop();
        Some(format!("burning {:.2} kg/s fuel + {:.2} kg/s oxidiser, O/F {:.2}\nhot gas {:.0} K out of the open face", mf, mo, mo / mf.max(1e-6), t_flame(&pr, mo / mf.max(1e-6))))
    }

    /// The mixture ratio (oxidiser / fuel by mass) of the burning gas at a gas cell: a gas generator's own, if the cell is in or
    /// near its cloud (within 40 cells of where it burns into, and nearer to it than to the main chamber's injectors), otherwise
    /// the main chamber's (what the injectors are putting in). None while nothing is burning there.
    pub fn gas_of(&self, cell: usize) -> Option<(f32, &'static str)> {
        let (x, y) = ((cell % W) as f32, (cell / W) as f32);
        let near = |j: usize| ((j % W) as f32 - x).hypot((j / W) as f32 - y);
        let d_inj = self.nets.iter().flat_map(|n| n.injectors.iter()).map(|&(_, g)| near(g)).fold(f32::MAX, f32::min);
        let mut best: Option<(f32, usize)> = None;
        for (g, faces) in self.gg_faces.iter().enumerate() {
            let d = faces.iter().map(|&(_, o)| near(o)).fold(f32::MAX, f32::min);
            if d < 40.0 && d < d_inj && best.map_or(true, |b| d < b.0) {
                best = Some((d, g));
            }
        }
        if let Some((_, g)) = best {
            let (fi, oi, _, _) = *self.gg.get(g)?;
            let (mf, mo) = (self.nets[fi].gg_draw, self.nets[oi].gg_draw);
            return (mf + mo > 1e-3).then(|| (mo / mf.max(1e-6), "gas generator"));
        }
        (self.ignited && self.g.of > 0.0).then(|| (self.g.of, "main chamber"))
    }

    /// Where a valve sends its propellant: the main chamber (the injectors), a gas generator, or both (cached by rebuild_nets).
    pub fn valve_dest(&self, k: usize) -> Option<&'static str> {
        match self.valve_dests.get(k).copied().unwrap_or(0) {
            1 => Some("main chamber"),
            2 => Some("gas generator"),
            3 => Some("chamber and GG"),
            _ => None,
        }
    }

    /// Work out where each valve leads. Valves are treated as open (so the answer is the same whichever state they are in), the
    /// line is followed out of each side of the valve without crossing it, and the side without a tank is where it goes: the
    /// injectors there make it a main-chamber valve, a gas generator touching that line a generator valve.
    fn compute_valve_dests(&mut self) {
        let (cells, back, front) = (&self.cells, &self.back, &self.front);
        let lt = |l: usize, i: usize| -> u8 {
            match l {
                0 => cells[i],
                1 => back[i],
                _ => front[i],
            }
        };
        let line = |t: u8| c::plumbing(t) || c::is_valve(t);
        let mut out = vec![];
        for v in &self.valves {
            let mut sides: Vec<(bool, bool, bool)> = vec![]; // (tank, injectors, generator) for each side
            let mut done: std::collections::HashSet<usize> = std::collections::HashSet::new();
            for &vi in &v.cells {
                let (x, y) = (vi % W, vi / W);
                for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                    if nx >= W || ny >= H {
                        continue;
                    }
                    let s0 = Self::idx(nx, ny);
                    if done.contains(&s0) || v.cells.contains(&s0) || !line(cells[s0]) {
                        continue;
                    }
                    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
                    let mut stack = vec![s0];
                    seen.insert(s0);
                    let (mut tank, mut inj, mut gen_hd) = (false, false, false);
                    while let Some(nd) = stack.pop() {
                        let (l, i) = (nd / N, nd % N);
                        let t = lt(l, i);
                        tank |= matches!(t, c::TANK_F | c::TANK_O);
                        inj |= t == c::INJ;
                        let (x, y) = ((i % W) as i32, (i / W) as i32);
                        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let (nx, ny) = (x + dx, y + dy);
                            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                                continue;
                            }
                            let j = Self::idx(nx as usize, ny as usize);
                            if l == 0 && matches!(cells[j], c::GG | c::BURNER) && matches!(t, c::PIPE | c::JOINT) {
                                gen_hd = true;
                            }
                            // pipes into and out of the same turbine are one line (an expander's fuel runs through the wheel)
                            if l == 0 && cells[j] == c::TURBINE && matches!(t, c::PIPE | c::JOINT) {
                                let mut tseen = vec![j];
                                let mut ti = 0;
                                while ti < tseen.len() {
                                    let (tx, ty) = (tseen[ti] % W, tseen[ti] / W);
                                    ti += 1;
                                    for (ax, ay) in [(tx + 1, ty), (tx.wrapping_sub(1), ty), (tx, ty + 1), (tx, ty.wrapping_sub(1))] {
                                        if ax < W && ay < H {
                                            let a = Self::idx(ax, ay);
                                            if cells[a] == c::TURBINE && !tseen.contains(&a) {
                                                tseen.push(a);
                                            } else if matches!(cells[a], c::PIPE | c::JOINT) && seen.insert(a) {
                                                stack.push(a);
                                            }
                                        }
                                    }
                                }
                            }
                            let nn = l * N + j;
                            if !seen.contains(&nn) && !v.cells.contains(&j) && line(lt(l, j)) && line(t) && !((t == c::TANK_F && lt(l, j) == c::TANK_O) || (t == c::TANK_O && lt(l, j) == c::TANK_F)) {
                                seen.insert(nn);
                                stack.push(nn);
                            }
                        }
                        if t == c::JOINT && l == 0 {
                            for l2 in [1usize, 2] {
                                if line(lt(l2, i)) && seen.insert(l2 * N + i) {
                                    stack.push(l2 * N + i);
                                }
                            }
                        } else if l > 0 && cells[i] == c::JOINT && seen.insert(i) {
                            stack.push(i);
                        }
                    }
                    for &nd in &seen {
                        if nd < N {
                            done.insert(nd);
                        }
                    }
                    sides.push((tank, inj, gen_hd));
                }
            }
            // (the side with the tank is upstream; with no tank on either side, every side counts)
            let any_tank = sides.iter().any(|s| s.0);
            let (mut ch, mut gg) = (false, false);
            for s in sides.iter().filter(|s| !any_tank || !s.0) {
                ch |= s.1;
                gg |= s.2;
            }
            out.push(ch as u8 | (gg as u8) << 1);
        }
        self.valve_dests = out;
    }

    /// A valve's name for lists: what it carries, where it goes, and where it is.
    pub fn valve_label(&self, k: usize) -> String {
        let v = &self.valves[k];
        let i = v.cells[0];
        match self.net_around(i) {
            Some(n) => match self.valve_dest(k) {
                Some(d) => format!("{} valve ({}) to {} at ({}, {})", if v.prop == 0 { "Fuel" } else { "Oxidiser" }, self.fluid_name(n, false), d, i % W, i / W),
                None => format!("{} valve ({}) at ({}, {})", if v.prop == 0 { "Fuel" } else { "Oxidiser" }, self.fluid_name(n, false), i % W, i / W),
            },
            None => match self.valve_dest(k).filter(|_| v.prop != PROP_HE) {
                Some(d) => v.label().replacen(" valve", &format!(" valve to {d}"), 1),
                None => v.label(),
            },
        }
    }

    /// What a propellant line carries, short ("LH2", "LOX") or long ("LH2 (liquid hydrogen)").
    pub fn fluid_name(&self, net: usize, long: bool) -> &'static str {
        const SHORT_F: [&str; 4] = ["LH2", "CH4", "RP-1", "MMH"];
        const SHORT_O: [&str; 2] = ["LOX", "N2O4"];
        let n = &self.nets[net];
        if n.prop == 0 {
            let k = (n.fkind as usize).min(3);
            if long { FUELS[k] } else { SHORT_F[k] }
        } else {
            let k = pair_kinds(self.st.prop).1.min(1);
            if long { OXIDISERS[k] } else { SHORT_O[k] }
        }
    }

    pub fn tank_kind(&self, cell: usize) -> usize {
        let (f0, o0) = pair_kinds(self.st.prop);
        match (self.cells[cell], self.fluid[cell]) {
            (c::TANK_F, 0) => f0,
            (c::TANK_O, 0) => o0,
            (_, k) => k as usize - 1,
        }
    }

    pub fn rebuild_nets(&mut self) {
        self.derive_propellants();
        self.nets_dirty = false;
        self.mat_stale = true;
        self.phase_dirty = true;
        self.gg.clear();
        self.gg_faces.clear();
        let old_vis = std::mem::take(&mut self.gg_vis);
        let old_cell0 = std::mem::take(&mut self.gg_cell0);
        self.gg_lumped.clear();
        self.gg_dir.clear();
        self.gg_fneg.clear();
        self.hgi_faces.clear();
        self.hgi_down.clear();
        for comp in self.components(|i| self.cells[i] == c::HGI, |_, _| true) {
            let mut faces = vec![];
            for &i in &comp {
                let (x, y) = (i % W, i / W);
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H && self.cells[Self::idx(nx, ny)] == c::EMPTY && !faces.contains(&Self::idx(nx, ny)) {
                        faces.push(Self::idx(nx, ny));
                    }
                }
            }
            // the side that opens into a big space (the main chamber and the plume beyond) is the outlet:
            // a hot-gas injector is one-way, like a real one
            let mut down = vec![];
            for &f in &faces {
                let mut seen = std::collections::HashSet::new();
                let mut q = std::collections::VecDeque::from([f]);
                seen.insert(f);
                while let Some(j) = q.pop_front() {
                    if seen.len() > 6000 {
                        break;
                    }
                    let (x, y) = (j % W, j / W);
                    for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                        if nx < W && ny < H {
                            let n = Self::idx(nx, ny);
                            if self.cells[n] == c::EMPTY && seen.insert(n) {
                                q.push_back(n);
                            }
                        }
                    }
                }
                if seen.len() > 6000 {
                    down.push(f);
                }
            }
            self.hgi_faces.push(faces);
            self.hgi_down.push(down);
        }
        self.he_cells = self.cells.iter().filter(|&&v| v == c::HE_TANK).count();
        // helium bottles: each group of touching cells is one bottle with its own pressure;
        // drawing any new cells makes it a freshly filled bottle
        self.he_breaches.clear();
        self.sources.clear();
        for i in 0..N {
            if c::is_source(self.cells[i]) {
                let (x, y) = (i % W, i / W);
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H && self.cells[Self::idx(nx, ny)] == c::EMPTY {
                        self.sources.push((i, Self::idx(nx, ny), self.cells[i]));
                    }
                }
            }
        }
        let old_he_of = std::mem::replace(&mut self.he_of, vec![NO_NET; N]);
        let old_bottles = std::mem::take(&mut self.he_bottles);
        let cells_now = self.cells.clone();
        for comp in self.components(|i| cells_now[i] == c::HE_TANK, |_, _| true) {
            let fresh = comp.iter().any(|&i| old_he_of[i] == NO_NET);
            let old = comp.iter().map(|&i| old_he_of[i]).find(|&o| o != NO_NET).and_then(|o| old_bottles.get(o as usize));
            let tea = comp.iter().any(|&i| self.fluid[i] == TEA_FLUID);
            let (p, p0) = match (fresh, old) {
                (false, Some(b)) => (b.p, b.p0),
                _ if tea => (TEA_FILL, TEA_FILL),
                _ => (self.st.he_fill, self.st.he_fill),
            };
            let id = self.he_bottles.len() as u16;
            for &i in &comp {
                self.he_of[i] = id;
                let (x, y) = (i % W, i / W);
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H && cells_now[Self::idx(nx, ny)] == c::EMPTY {
                        self.he_breaches.push((i, Self::idx(nx, ny), id as usize));
                    }
                }
            }
            self.he_bottles.push(HeBottle { tea, cells: comp.len(), p, p0 });
        }
        let cells = self.cells.clone();

        // valve groups (keep their command and position)
        let old_valve_of = std::mem::replace(&mut self.valve_of, vec![NO_NET; N]);
        let old_valves = std::mem::take(&mut self.valves);
        for comp in self.components(|i| c::is_valve(cells[i]), |_, _| true) {
            let open_type = cells[comp[0]] == c::VALVE;
            let mut v = ValveGroup { tea: false, prop: 255, cmd: open_type, pos: if open_type { 1.0 } else { 0.0 }, cells: comp };
            if let Some(o) = v.cells.iter().map(|&i| old_valve_of[i]).find(|&o| o != NO_NET) {
                if let Some(ov) = old_valves.get(o as usize) {
                    v.cmd = ov.cmd;
                    v.pos = ov.pos;
                }
            }
            let id = self.valves.len() as u16;
            for &i in &v.cells {
                self.valve_of[i] = id;
            }
            self.valves.push(v);
        }

        // propellant networks, joined across the three layers: a back or front pipe connects to the
        // middle layer only where it sits on middle-layer plumbing of the same propellant
        let old_net_of = std::mem::replace(&mut self.net_of, vec![NO_NET; N]);
        let old_layer_net = std::mem::replace(&mut self.layer_net, [vec![NO_NET; N], vec![NO_NET; N]]);
        let old_nets = std::mem::take(&mut self.nets);
        let pr = self.st.p();
        let (back, front) = (self.back.clone(), self.front.clone());
        let lt = |l: usize, i: usize| -> u8 {
            match l {
                0 => cells[i],
                1 => back[i],
                _ => front[i],
            }
        };
        // pipes into and out of the same turbine are joined through it (an expander's warm fuel runs in
        // through one side of the wheel and out of the other on its way to the injectors). Only pipes and
        // joints count: a pump butted against a turbine stays its own network.
        let mut turb_link: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
        for comp in self.components(|i| cells[i] == c::TURBINE, |_, _| true) {
            let mut ports = vec![];
            for &i in &comp {
                let (x, y) = (i % W, i / W);
                for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                    if nx < W && ny < H {
                        let j = Self::idx(nx, ny);
                        if matches!(cells[j], c::PIPE | c::JOINT) && !ports.contains(&j) {
                            ports.push(j);
                        }
                    }
                }
            }
            for &a in &ports {
                turb_link.entry(a).or_default().extend(ports.iter().copied().filter(|&b| b != a));
            }
        }
        let mut seen = vec![false; 3 * N];
        let mut stack = vec![];
        let mut mixed_at: Option<usize> = None;
        self.mixed = None;
        for s in 0..3 * N {
            if !c::plumbing(lt(s / N, s % N)) || seen[s] {
                continue;
            }
            let mut comp = vec![];
            seen[s] = true;
            stack.push(s);
            while let Some(nd) = stack.pop() {
                comp.push(nd);
                let (l, i) = (nd / N, nd % N);
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                        continue;
                    }
                    let j = Self::idx(nx as usize, ny as usize);
                    if !seen[l * N + j] && c::joins(lt(l, i), lt(l, j)) {
                        seen[l * N + j] = true;
                        stack.push(l * N + j);
                    }
                }
                // layers meet only at pipe joints
                let others: &[usize] = if cells[i] != c::JOINT { &[] } else if l == 0 { &[1, 2] } else { &[0] };
                for &l2 in others {
                    if !seen[l2 * N + i] && c::plumbing(lt(l2, i)) {
                        seen[l2 * N + i] = true;
                        stack.push(l2 * N + i);
                    }
                }
                if l == 0 {
                    for &j in turb_link.get(&i).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if !seen[j] {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
            let has_f = comp.iter().any(|&nd| nd < N && cells[nd] == c::TANK_F);
            let has_o = comp.iter().any(|&nd| nd < N && cells[nd] == c::TANK_O);
            // 0 fuel, 1 oxidiser, 2 both (mixed: blocked), 255 not connected to any tank
            let p = match (has_f, has_o) {
                (true, false) => 0,
                (false, true) => 1,
                (true, true) => 2,
                _ => 255,
            };
            if p == 2 && mixed_at.is_none() {
                // say where: at the injectors if they're what joins the two (one manifold fed from both
                // sides), otherwise somewhere along the shared pipework
                let inj = comp.iter().copied().find(|&nd| nd < N && cells[nd] == c::INJ);
                let pipe = comp.iter().copied().find(|&nd| nd < N && !matches!(cells[nd], c::TANK_F | c::TANK_O));
                mixed_at = Some(inj.or(pipe).unwrap_or(comp[0]) % N);
                self.mixed = Some((mixed_at.unwrap(), inj.is_some()));
            }
            let id = self.nets.len().min(254) as u16;
            let mut net = Net {
                fp: pr,
                fkind: 0,
                prop: p,
                temp: if p == 0 { pr.t_in_f } else if p == 1 { pr.t_in_o } else { 300.0 },
                tank_cells: 0,
                lines: vec![],
                injectors: vec![],
                valves: vec![],
                inertance: 0.0,
                mass_frac: 1.0,
                mdot: 0.0,
                m_inj: 0.0,
                heat_w: 0.0,
                turb_dp: 0.0,
                p_tank: 0.0,
                head: 0.0,
                p_up: 0.0,
                p_line: 0.0,
                pc_f: 0.0,
                gg_draw: 0.0,
                flow_off: 0.0,
                valve_prev: 1.0,
                breaches: vec![],
                leak: 0.0,
                ull_pv: 0.0,
                boil: 0.0,
                tank_faces: 0,
                tank_top: 0,
                tank_y0: f32::MAX,
                tank_y1: 0.0,
                max_phase: 0.0,
                front: 0.0,
                bottle: NO_NET,
                reg_bottle: NO_NET,
                reg_set: 0.0,
                h0: 0.0,
                kq: 0.0,
                throttle: false,
            };
            let mut layer_lines = 0usize;
            for &nd in &comp {
                let (l, i) = (nd / N, nd % N);
                if l > 0 {
                    self.layer_net[l - 1][i] = id;
                    layer_lines += 1;
                    continue;
                }
                self.net_of[i] = id;
                match cells[i] {
                    c::TANK_F | c::TANK_O => {
                        net.tank_cells += 1;
                        if (i / W) as f32 <= net.tank_y0 {
                            net.tank_top = i;
                        }
                        let (x, y) = (i % W, i / W);
                        for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                            if nx >= W || ny >= H || cells[Self::idx(nx, ny)] != cells[i] {
                                net.tank_faces += 1;
                            }
                        }
                        net.tank_y0 = net.tank_y0.min((i / W) as f32);
                        net.tank_y1 = net.tank_y1.max((i / W) as f32);
                    }
                    c::PIPE => net.lines.push(i),
                    _ => {}
                }
                if c::can_leak(cells[i]) {
                    let (x, y) = (i % W, i / W);
                    for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                        if nx < W && ny < H && cells[Self::idx(nx, ny)] == c::EMPTY {
                            net.breaches.push((i, Self::idx(nx, ny)));
                        }
                    }
                }
                match cells[i] {
                    c::INJ => {
                        let (x, y) = (i % W, i / W);
                        let cand = [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)];
                        for (nx, ny) in cand {
                            if nx < W && ny < H && cells[Self::idx(nx, ny)] == c::EMPTY {
                                net.injectors.push((i, Self::idx(nx, ny)));
                                break;
                            }
                        }
                    }
                    c::VALVE => {
                        let g = self.valve_of[i] as usize;
                        if !net.valves.contains(&g) {
                            net.valves.push(g);
                        }
                    }
                    _ => {}
                }
            }
            net.inertance = 2.0 * (net.lines.len() + layer_lines) as f32 / self.st.depth.max(0.05) + 200.0;
            // pipework with no propellant tank but touching a helium bottle carries helium
            if p == 255 {
                let he_of = &self.he_of;
                let touching = comp.iter().filter(|&&nd| nd < N).find_map(|&i| {
                    let (x, y) = (i % W, i / W);
                    [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)]
                        .into_iter()
                        .filter(|&(nx, ny)| nx < W && ny < H)
                        .map(|(nx, ny)| he_of[Self::idx(nx, ny)])
                        .find(|&b| b != NO_NET)
                });
                if let Some(b) = touching {
                    net.prop = PROP_HE;
                    net.bottle = b;
                }
            }
            let old_of = |nd: usize| if nd < N { old_net_of[nd] } else { old_layer_net[nd / N - 1][nd % N] };
            let mut carried = false;
            if let Some(o) = comp.iter().filter(|&&nd| nd < N).map(|&nd| old_net_of[nd]).find(|&o| o != NO_NET) {
                if let Some(on) = old_nets.get(o as usize) {
                    carried = true;
                    net.temp = on.temp;
                    net.mass_frac = on.mass_frac;
                    net.ull_pv = on.ull_pv;
                    if comp.iter().any(|&nd| nd < N && matches!(cells[nd], c::TANK_F | c::TANK_O) && old_net_of[nd] == NO_NET) {
                        net.mass_frac = 1.0; // a freshly drawn tank is full
                    }
                    net.heat_w = on.heat_w;
                    net.pc_f = on.pc_f;
                    net.mdot = on.mdot;
                    net.m_inj = on.m_inj;
                    net.flow_off = on.flow_off;
                    net.valve_prev = on.valve_prev;
                    net.p_tank = on.p_tank;
                    net.head = on.head;
                    net.p_up = on.p_up;
                    net.p_line = on.p_line;
                    net.gg_draw = on.gg_draw;
                    net.turb_dp = on.turb_dp;
                    net.boil = on.boil;
                    net.leak = on.leak;
                }
            }
            // keep the liquid that was already in any of the merged lines
            let mut seen_old = vec![];
            for &nd in &comp {
                let o = old_of(nd);
                if o != NO_NET && !seen_old.contains(&o) {
                    seen_old.push(o);
                    if let Some(on) = old_nets.get(o as usize) {
                        net.front = net.front.max(on.front);
                    }
                }
            }
            // a fuel line carries its own fuel (two fuel tanks of different fluids make a tri-propellant engine)
            if net.prop == 0 && net.tank_cells > 0 {
                let (_, o0) = pair_kinds(self.st.prop);
                let mut count = [0usize; 4];
                for &nd in &comp {
                    if nd < N && cells[nd] == c::TANK_F {
                        count[self.tank_kind(nd).min(3)] += 1;
                    }
                }
                let fk = (0..4).max_by_key(|&k| count[k]).unwrap_or(0);
                net.fkind = fk as u8;
                net.fp = &PROPS[pair_index(fk, o0)];
                if !carried {
                    net.temp = net.fp.t_in_f;
                }
            }
            self.nets.push(net);
        }

        for i in 0..N {
            if cells[i] == c::THROTTLE && self.net_of[i] != NO_NET {
                self.nets[self.net_of[i] as usize].throttle = true;
            }
        }
        // pressure regulators: a helium line (or a bottle) on one side, a propellant tank's network on the other
        for i in 0..N {
            if cells[i] != c::REG {
                continue;
            }
            let (x, y) = (i % W, i / W);
            let (mut b, mut tank) = (NO_NET, NO_NET);
            for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                if nx >= W || ny >= H {
                    continue;
                }
                let j = Self::idx(nx, ny);
                if self.he_of[j] != NO_NET {
                    b = self.he_of[j];
                }
                let n = self.net_of[j];
                if n != NO_NET {
                    let net = &self.nets[n as usize];
                    if net.prop == PROP_HE {
                        b = net.bottle;
                    } else if net.tank_cells > 0 && net.prop <= 1 {
                        tank = n;
                    }
                }
            }
            if b != NO_NET && tank != NO_NET {
                self.nets[tank as usize].reg_bottle = b;
                self.nets[tank as usize].reg_set = self.nets[tank as usize].reg_set.max(self.reg_p[i]);
            }
        }
        // the regulators as units (touching regulator cells are one), for the engine panel
        self.regs.clear();
        {
            let mut seen = vec![false; N];
            for i in 0..N {
                if cells[i] != c::REG || seen[i] {
                    continue;
                }
                let (mut prop, mut piped) = (255u8, false);
                let mut stack = vec![i];
                seen[i] = true;
                while let Some(j) = stack.pop() {
                    let (x, y) = (j % W, j / W);
                    for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                        if nx >= W || ny >= H {
                            continue;
                        }
                        let k = Self::idx(nx, ny);
                        if cells[k] == c::REG && !seen[k] {
                            seen[k] = true;
                            stack.push(k);
                        }
                        if self.he_of[k] != NO_NET {
                            piped = true;
                        }
                        let n = self.net_of[k];
                        if n != NO_NET {
                            let net = &self.nets[n as usize];
                            if net.prop == PROP_HE && net.bottle != NO_NET {
                                piped = true;
                            } else if net.tank_cells > 0 && net.prop <= 1 {
                                prop = net.prop;
                            }
                        }
                    }
                }
                self.regs.push((i, prop, piped));
            }
        }

        // a valve belongs to whichever propellant flows through it
        for g in 0..self.valves.len() {
            let mut prop = 255u8;
            let mut tea = false;
            for &i in &self.valves[g].cells {
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                        continue;
                    }
                    let n = self.net_of[Self::idx(nx as usize, ny as usize)];
                    if n != NO_NET {
                        let np = self.nets[n as usize].prop;
                        if np <= 1 || (np == PROP_HE && prop == 255) {
                            prop = np;
                            let bn = self.nets[n as usize].bottle;
                            tea = np == PROP_HE && bn != NO_NET && self.he_bottles.get(bn as usize).map_or(false, |b| b.tea);
                        }
                    }
                }
            }
            self.valves[g].prop = prop;
            self.valves[g].tea = tea;
        }
        if let Some((i, at_inj)) = self.mixed {
            self.event(if at_inj {
                format!("fuel and oxidiser feed the same injectors near ({}, {}): split the manifold so each feeds its own (a wall between them)", i % W, i / W)
            } else {
                format!("fuel and oxidiser lines are joined near ({}, {}): they must not touch", i % W, i / W)
            });
        }

        // alternating injectors: each block of touching AINJ cells takes fuel from the fuel line it touches and oxidiser from the
        // oxidiser line, and sprays fuel from the cells with an even (x + y) and oxidiser from the odd ones, into the open gas
        // beside them, through the same per-cell injector flow as any other injector
        for comp in self.components(|i| cells[i] == c::AINJ, |_, _| true) {
            let (mut nf, mut no) = (None, None);
            for &i in &comp {
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                        continue;
                    }
                    let id = self.net_of[Self::idx(nx as usize, ny as usize)];
                    if id == NO_NET {
                        continue;
                    }
                    match self.nets[id as usize].prop {
                        0 => nf = nf.or(Some(id as usize)),
                        1 => no = no.or(Some(id as usize)),
                        _ => {}
                    }
                }
            }
            for &i in &comp {
                let (x, y) = (i % W, i / W);
                let Some(net) = (if (x + y) % 2 == 0 { nf } else { no }) else { continue };
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H && cells[Self::idx(nx, ny)] == c::EMPTY {
                        self.nets[net].injectors.push((i, Self::idx(nx, ny)));
                        break;
                    }
                }
            }
        }

        // gas generators: each block of touching GG cells is one generator, fed by whatever fuel and
        // oxidiser lines touch it anywhere
        for comp in self.components(|i| matches!(cells[i], c::GG | c::BURNER), |_, _| true) {
            let (mut nf, mut no) = (None, None);
            for &i in &comp {
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 {
                        continue;
                    }
                    let j = Self::idx(nx as usize, ny as usize);
                    let id = self.net_of[j];
                    if id == NO_NET {
                        continue;
                    }
                    match self.nets[id as usize].prop {
                        0 => nf = nf.or(Some((id as usize, j))),
                        1 => no = no.or(Some((id as usize, j))),
                        _ => {}
                    }
                }
            }
            if let (Some((f, cf)), Some((o, co))) = (nf, no) {
                self.gg.push((f, o, cf, co));
                // the open cells the head faces: its combustion chamber
                let mut faces = vec![];
                for &i in &comp {
                    let (x, y) = (i % W, i / W);
                    for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                        if nx < W && ny < H && cells[Self::idx(nx, ny)] == c::EMPTY {
                            faces.push((i, Self::idx(nx, ny)));
                        }
                    }
                }
                // a burner block: which end of its long axis the gas leaves from
                let lumped = comp.iter().any(|&i| cells[i] == c::BURNER);
                let (mut x0, mut x1, mut y0, mut y1) = (W, 0, H, 0);
                for &i in &comp {
                    x0 = x0.min(i % W);
                    x1 = x1.max(i % W);
                    y0 = y0.min(i / W);
                    y1 = y1.max(i / W);
                }
                let horizontal = x1 - x0 >= y1 - y0;
                let centre = if horizontal { (x0 + x1) as f32 * 0.5 } else { (y0 + y1) as f32 * 0.5 };
                let out_mean = if faces.is_empty() { centre } else { faces.iter().map(|f: &(usize, usize)| if horizontal { (f.1 % W) as f32 } else { (f.1 / W) as f32 }).sum::<f32>() / faces.len() as f32 };
                self.gg_dir.push(if out_mean >= centre { 1.0 } else { -1.0 });
                let cross_c = if horizontal { (y0 + y1) as f32 * 0.5 } else { (x0 + x1) as f32 * 0.5 };
                let fuel_at = if horizontal { (cf / W) as f32 } else { (cf % W) as f32 };
                self.gg_fneg.push(fuel_at <= cross_c);
                self.gg_lumped.push(lumped);
                self.gg_cell0.push(comp[0]);
                // how it was burning a moment ago (a rebuild shouldn't make the flame flash out and back)
                let was = old_cell0.iter().position(|&c0| comp.contains(&c0)).and_then(|p| old_vis.get(p)).copied().unwrap_or((0.0, 0.0));
                self.gg_vis.push(was);
                self.gg_faces.push(faces);
            }
        }

        // rotor blocks (impellers / turbine wheels) with their bounding boxes
        self.blocks.clear();
        self.block_of = vec![255; N];
        let mut bbox: Vec<(f32, f32, u8, Vec<usize>)> = vec![]; // (width, height, type, cells)
        for comp in self.components(|i| matches!(cells[i], c::PUMP | c::TURBINE | c::MOTOR | c::BURNER), |a, b| cells[a] == cells[b]) {
            let (mut x0, mut x1, mut y0, mut y1) = (W, 0, H, 0);
            for &i in &comp {
                x0 = x0.min(i % W);
                x1 = x1.max(i % W);
                y0 = y0.min(i / W);
                y1 = y1.max(i / W);
            }
            if bbox.len() < MAX_BLOCKS {
                let id = bbox.len() as u8;
                for &i in &comp {
                    self.block_of[i] = id;
                }
            }
            bbox.push(((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32, cells[comp[0]], comp));
        }

        // turbopump shafts: turbines, shaft cells and pumps that touch form one rotor
        let old_shaft_of = std::mem::replace(&mut self.shaft_of, vec![NO_NET; N]);
        let old_shafts = std::mem::take(&mut self.shafts);
        for comp in self.components(|i| c::is_rotor(cells[i]), |_, _| true) {
            // shaft axis: the line from the pump to the turbine; a lone block spins about its long side
            let centroid = |t: &dyn Fn(u8) -> bool| {
                let v: Vec<usize> = comp.iter().copied().filter(|&i| t(cells[i])).collect();
                (!v.is_empty()).then(|| {
                    let n = v.len() as f32;
                    (v.iter().map(|&i| (i % W) as f32).sum::<f32>() / n, v.iter().map(|&i| (i / W) as f32).sum::<f32>() / n)
                })
            };
            let pc = centroid(&|t| t == c::PUMP);
            let tc = centroid(&|t| t == c::TURBINE || t == c::MOTOR);
            let horizontal = match (pc, tc) {
                (Some(p), Some(t)) if (t.0 - p.0).abs().max((t.1 - p.1).abs()) > 1.0 => (t.0 - p.0).abs() >= (t.1 - p.1).abs(),
                // pumps either side of the turbine balance out on top of it: go by which way they sit from it
                (Some(_), Some(t)) => {
                    let pumps: Vec<usize> = comp.iter().copied().filter(|&i| cells[i] == c::PUMP).collect();
                    let n = pumps.len() as f32;
                    let ax = pumps.iter().map(|&i| ((i % W) as f32 - t.0).abs()).sum::<f32>() / n;
                    let ay = pumps.iter().map(|&i| ((i / W) as f32 - t.1).abs()).sum::<f32>() / n;
                    ax >= ay
                }
                _ => {
                    let xs = comp.iter().map(|&i| i % W);
                    let ys = comp.iter().map(|&i| i / W);
                    let w = xs.clone().max().unwrap() - xs.min().unwrap();
                    let h = ys.clone().max().unwrap() - ys.min().unwrap();
                    w >= h
                }
            };
            let mut s = Shaft {
                turb_cells: comp.iter().filter(|&&i| cells[i] == c::TURBINE).count(),
                pumps: vec![],
                d_turb: 0.0,
                turb_stages: 1.0,
                omega_rated: OMEGA_D,
                horizontal,
                j: 0.0,
                omega: 0.0,
                theta: 0.0,
                p_turb: 0.0,
                p_load: 0.0,
                eff_t: 0.0,
                u_c0: 0.0,
                turb_faces: vec![],
                turb_in: vec![],
                gas_driven: false,
                p_gas: 0.0,
                c0_gas: 1200.0,
                regen_nets: vec![],
                p_exp: 0.0,
                motor_cells: comp.iter().filter(|&&i| cells[i] == c::MOTOR).count(),
                cart_cells: 0,
                d_motor: 0.0,
                circuit: NO_NET,
                p_motor: 0.0,
                cells: vec![],
            };
            if let Some(o) = comp.iter().map(|&i| old_shaft_of[i]).find(|&o| o != NO_NET) {
                if let Some(os) = old_shafts.get(o as usize) {
                    s.omega = os.omega;
                    s.theta = os.theta;
                    s.p_turb = os.p_turb;
                    s.p_load = os.p_load;
                    s.eff_t = os.eff_t;
                    s.u_c0 = os.u_c0;
                    s.p_gas = os.p_gas;
                    s.c0_gas = os.c0_gas;
                    s.p_exp = os.p_exp;
                    s.p_motor = os.p_motor;
                }
            }
            let id = self.shafts.len() as u16;
            for &i in &comp {
                self.shaft_of[i] = id;
            }
            s.cells = comp;
            self.shafts.push(s);
        }
        // start cartridges serve the shaft they touch (a wall's thickness away counts): a canister is the cartridge
        // cells that touch each other, and all of it burns for the shaft any of it faces
        let mut seen = vec![false; N];
        for i0 in 0..N {
            if cells[i0] != c::CARTRIDGE || seen[i0] {
                continue;
            }
            let mut group = vec![i0];
            seen[i0] = true;
            let mut k = 0;
            let mut found = NO_NET;
            while k < group.len() {
                let i = group[k];
                k += 1;
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for (ox, oy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + ox, y + oy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < W && (ny as usize) < H {
                        let j = Self::idx(nx as usize, ny as usize);
                        if cells[j] == c::CARTRIDGE && !seen[j] {
                            seen[j] = true;
                            group.push(j);
                        }
                    }
                }
                if found == NO_NET {
                    'near: for d in 1..=2i32 {
                        for (ox, oy) in [(d, 0), (-d, 0), (0, d), (0, -d)] {
                            let (nx, ny) = (x + ox, y + oy);
                            if nx >= 0 && ny >= 0 && (nx as usize) < W && (ny as usize) < H {
                                let s = self.shaft_of[Self::idx(nx as usize, ny as usize)];
                                if s != NO_NET {
                                    found = s;
                                    break 'near;
                                }
                            }
                        }
                    }
                }
            }
            if found != NO_NET {
                self.shafts[found as usize].cart_cells += group.len();
            }
        }
        let dx = self.st.dx;
        for (k, (bw, bh, t, comp)) in bbox.iter().enumerate() {
            if *t == c::BURNER {
                // a burner is no rotor: a block to draw, along its longer side
                if k < MAX_BLOCKS {
                    let x0 = comp.iter().map(|&i| i % W).min().unwrap() as f32;
                    let y0 = comp.iter().map(|&i| i / W).min().unwrap() as f32;
                    self.blocks.push(Block { x0, y0, w: *bw, h: *bh, horizontal: *bw >= *bh, stages: 0.0, shaft: usize::MAX });
                }
                continue;
            }
            let si = self.shaft_of[comp[0]] as usize;
            let s = &mut self.shafts[si];
            let (len, dia) = if s.horizontal { (*bw, *bh) } else { (*bh, *bw) };
            let (d, l) = (dia * dx, len * dx);
            let r = 0.5 * d;
            // solid-ish cylinder (25% fill) for the rotor inertia
            let mass = RHO_ROTOR * std::f32::consts::PI * r * r * l * 0.25;
            s.j += 0.5 * mass * r * r;
            if *t == c::MOTOR {
                s.d_motor = s.d_motor.max(d);
            } else if *t == c::TURBINE {
                s.d_turb = s.d_turb.max(d);
                // each stage (nozzle ring plus rotor) takes about 0.4 diameters of the block's length
                s.turb_stages = s.turb_stages.max((len / (0.4 * dia)).round().clamp(1.0, 3.0));
            } else {
                // the same pump as before the edit keeps its running state (else it would restart dry)
                let old_pump = old_shafts.get(old_shaft_of[comp[0]] as usize).and_then(|os| os.pumps.iter().find(|p| p.cells.iter().any(|c| comp.contains(c))));
                let mut np = PumpRef {
                    net: self.net_of[comp[0]] as usize,
                    prop: self.nets.get(self.net_of[comp[0]] as usize).map_or(255, |n| n.prop),
                    d,
                    stages: (len / (0.6 * dia)).round().max(1.0),
                    head: 0.0,
                    eff: 0.7,
                    cav: 1.0,
                    npsp_avail: 0.0,
                    npsp_req: 0.0,
                    power: 0.0,
                    cells: comp.clone(),
                    phase: 0.0,
                    wet: false,
                };
                if let Some(o) = old_pump {
                    np.head = o.head;
                    np.eff = o.eff;
                    np.cav = o.cav;
                    np.npsp_avail = o.npsp_avail;
                    np.npsp_req = o.npsp_req;
                    np.power = o.power;
                    np.phase = o.phase;
                    np.wet = o.wet;
                }
                s.pumps.push(np);
            }
            if k < MAX_BLOCKS {
                let x0 = comp.iter().map(|&i| i % W).min().unwrap() as f32;
                let y0 = comp.iter().map(|&i| i / W).min().unwrap() as f32;
                let horizontal = self.shafts[si].horizontal;
                let stages = if *t == c::TURBINE { (len / (0.4 * dia)).round().clamp(1.0, 3.0) } else { (len / (0.6 * dia)).round().max(1.0) };
                self.blocks.push(Block { x0, y0, w: *bw, h: *bh, horizontal, stages, shaft: si });
            }
        }
        // where each burner block's outlet is and which side its fuel comes in on, from the layout alone (the feed
        // valves are shut at the start, which leaves the generator list empty); fuel and oxidiser lines are told
        // apart by which tank they would reach with every valve open
        self.burner_view = vec![(1.0, true); self.blocks.len()];
        if bbox.iter().any(|b| b.2 == c::BURNER) {
            let nets = self.open_valve_networks();
            let mut tank_of = vec![0u8; N]; // 1 fuel, 2 oxidiser
            for comp in &nets {
                let kind = if comp.iter().any(|&i| cells[i] == c::TANK_F) { 1 } else if comp.iter().any(|&i| cells[i] == c::TANK_O) { 2 } else { 0 };
                if kind != 0 {
                    for &i in comp {
                        tank_of[i] = kind;
                    }
                }
            }
            for (k, (bw, bh, t, comp)) in bbox.iter().enumerate() {
                if *t != c::BURNER || k >= self.burner_view.len() {
                    continue;
                }
                let horizontal = *bw >= *bh;
                let x0 = comp.iter().map(|&i| i % W).min().unwrap() as f32;
                let y0 = comp.iter().map(|&i| i / W).min().unwrap() as f32;
                let centre = if horizontal { x0 + (*bw - 1.0) * 0.5 } else { y0 + (*bh - 1.0) * 0.5 };
                let cross_c = if horizontal { y0 + (*bh - 1.0) * 0.5 } else { x0 + (*bw - 1.0) * 0.5 };
                let (mut out_sum, mut out_n) = (0.0f32, 0.0f32);
                let (mut fuel_sum, mut fuel_n) = (0.0f32, 0.0f32);
                for &i in comp {
                    let (x, y) = (i % W, i / W);
                    for (nx, ny) in [(x + 1, y), (x.wrapping_sub(1), y), (x, y + 1), (x, y.wrapping_sub(1))] {
                        if nx >= W || ny >= H {
                            continue;
                        }
                        let j = Self::idx(nx, ny);
                        if cells[j] == c::EMPTY {
                            out_sum += if horizontal { nx as f32 } else { ny as f32 };
                            out_n += 1.0;
                        } else if tank_of[j] == 1 {
                            fuel_sum += if horizontal { ny as f32 } else { nx as f32 };
                            fuel_n += 1.0;
                        }
                    }
                }
                let dir = if out_n > 0.0 && out_sum / out_n < centre { -1.0 } else { 1.0 };
                let fneg = fuel_n == 0.0 || fuel_sum / fuel_n <= cross_c;
                self.burner_view[k] = (dir, fneg);
            }
        }
        for s in &mut self.shafts {
            let r_max = s.pumps.iter().map(|p| 0.5 * p.d).fold(0.5 * s.d_turb.max(s.d_motor), f32::max);
            if r_max > 0.0 {
                s.omega_rated = U_TIP_MAX / r_max;
            }
            s.j = s.j.max(0.01);
        }

        // electrical circuits: batteries, wire and motors that touch. A circuit keeps its charge through a
        // rebuild (matched by any of its old battery cells); a new one starts full.
        let old_circuit_of = std::mem::replace(&mut self.circuit_of, vec![NO_NET; N]);
        let old_circuits = std::mem::take(&mut self.circuits);
        let scale = (self.st.dx / DX0).powi(2) * (self.st.depth / 0.36);
        for comp in self.components(|i| c::electric(cells[i]), |_, _| true) {
            let batt: Vec<usize> = comp.iter().copied().filter(|&i| cells[i] == c::BATTERY).collect();
            let cap = batt.len() as f32 * BATT_J_CELL * scale;
            let frac = batt
                .iter()
                .map(|&i| old_circuit_of[i])
                .find(|&o| o != NO_NET)
                .and_then(|o| old_circuits.get(o as usize))
                .map_or(1.0, |oc| if oc.cap > 0.0 { oc.energy / oc.cap } else { 1.0 });
            let id = self.circuits.len() as u16;
            for &i in &comp {
                self.circuit_of[i] = id;
            }
            self.circuits.push(Circuit { batt_cells: batt.len(), cap, energy: cap * frac, p_max: batt.len() as f32 * BATT_W_CELL * scale, p_draw: 0.0, motors: 0 });
        }
        for s in &mut self.shafts {
            if let Some(&i) = s.cells.iter().find(|&&i| cells[i] == c::MOTOR) {
                s.circuit = self.circuit_of[i];
                if let Some(cc) = self.circuits.get_mut(s.circuit as usize) {
                    cc.motors += s.motor_cells;
                }
            }
        }

        // flow phase: distance (in cells) from the tank along each network, through all three layers
        let node_net = |s: &World, nd: usize| if nd < N { s.net_of[nd] } else { s.layer_net[nd / N - 1][nd % N] };
        self.phase = vec![0.0; 3 * N];
        let mut dist = vec![u32::MAX; 3 * N];
        let mut q = std::collections::VecDeque::new();
        for i in 0..N {
            let he_line = self.net_of[i] != NO_NET && self.nets[self.net_of[i] as usize].prop == PROP_HE;
            if matches!(cells[i], c::TANK_F | c::TANK_O) || (he_line && self.nets[self.net_of[i] as usize].bottle != NO_NET && {
                let (x, y) = (i % W, i / W);
                [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)]
                    .into_iter()
                    .any(|(nx, ny)| nx < W && ny < H && cells[Self::idx(nx, ny)] == c::HE_TANK)
            }) {
                dist[i] = 0;
                q.push_back(i);
            }
        }
        while let Some(nd) = q.pop_front() {
            let (l, i) = (nd / N, nd % N);
            let me = node_net(self, nd);
            let (x, y) = (i % W, i / W);
            let mut next = [usize::MAX; 6];
            let mut k = 0;
            if x + 1 < W {
                next[k] = nd + 1;
                k += 1;
            }
            if x > 0 {
                next[k] = nd - 1;
                k += 1;
            }
            if y + 1 < H {
                next[k] = nd + W;
                k += 1;
            }
            if y > 0 {
                next[k] = nd - W;
                k += 1;
            }
            if cells[i] == c::JOINT {
                if l == 0 {
                    next[k] = N + i;
                    next[k + 1] = 2 * N + i;
                    k += 2;
                } else {
                    next[k] = i;
                    k += 1;
                }
            }
            let through: &[usize] = if l == 0 { turb_link.get(&i).map(|v| v.as_slice()).unwrap_or(&[]) } else { &[] };
            for &j in next[..k].iter().chain(through) {
                if dist[j] == u32::MAX && me != NO_NET && node_net(self, j) == me {
                    dist[j] = dist[nd] + 1;
                    q.push_back(j);
                }
            }
        }
        for nd in 0..3 * N {
            let n = node_net(self, nd);
            if dist[nd] != u32::MAX {
                self.phase[nd] = dist[nd] as f32;
                if n != NO_NET {
                    let net = &mut self.nets[n as usize];
                    net.max_phase = net.max_phase.max(dist[nd] as f32);
                }
            } else if n != NO_NET {
                self.phase[nd] = DRY;
            }
        }
        // an alternating injector's cell is wet when the liquid reaches the line it touches
        for n in 0..self.nets.len() {
            for k in 0..self.nets[n].injectors.len() {
                let i = self.nets[n].injectors[k].0;
                if self.cells[i] != c::AINJ {
                    continue;
                }
                let (x, y) = (i % W, i / W);
                let mut best = DRY;
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H {
                        let j = Self::idx(nx, ny);
                        if self.net_of[j] as usize == n {
                            best = best.min(self.phase[j] + 1.0);
                        }
                    }
                }
                self.phase[i] = best;
            }
        }
        for s in &mut self.shafts {
            for p in &mut s.pumps {
                p.phase = p.cells.iter().map(|&i| self.phase[i]).fold(DRY, f32::min);
            }
        }
        // turbine wheels drive on whatever gas flows through them; a gas generator whose exhaust can reach
        // a turbine through enclosed gas (a duct) drives it that way instead of by a calculated share
        let mut face_of = std::collections::HashMap::new();
        for (k, s) in self.shafts.iter_mut().enumerate() {
            let mut faces = vec![];
            for &i in &s.cells {
                if self.cells[i] != c::TURBINE {
                    continue;
                }
                let (x, y) = (i % W, i / W);
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H && self.cells[Self::idx(nx, ny)] == c::EMPTY && !faces.contains(&Self::idx(nx, ny)) {
                        faces.push(Self::idx(nx, ny));
                        face_of.insert(Self::idx(nx, ny), k);
                    }
                }
            }
            s.turb_faces = faces;
            s.turb_in.clear();
            s.gas_driven = false;
        }
        // expander drive: a fuel line that runs into a turbine block brings its regen-heated (gasified) fuel
        // through the wheel
        for s in &mut self.shafts {
            let mut nets = vec![];
            for &i in &s.cells {
                if self.cells[i] != c::TURBINE {
                    continue;
                }
                let (x, y) = (i % W, i / W);
                for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                    if nx < W && ny < H {
                        let j = Self::idx(nx, ny);
                        let n = self.net_of[j];
                        if n != NO_NET && c::is_line(self.cells[j]) && self.nets[n as usize].prop == 0 && !nets.contains(&(n as usize)) {
                            nets.push(n as usize);
                        }
                    }
                }
            }
            s.regen_nets = nets;
        }
        // gas that can reach the edge of the world; sealed vessels (preburners, ducts) are the rest
        {
            let mut open = vec![false; N];
            let mut stack: Vec<u32> = Vec::with_capacity(N / 4);
            let border = (0..W).flat_map(|x| [Self::idx(x, 0), Self::idx(x, H - 1)]).chain((0..H).flat_map(|y| [Self::idx(0, y), Self::idx(W - 1, y)]));
            for i in border {
                if self.cells[i] == c::EMPTY && !open[i] {
                    open[i] = true;
                    stack.push(i as u32);
                }
            }
            let cells = &self.cells;
            while let Some(i) = stack.pop() {
                let i = i as usize;
                let x = i % W;
                let mut visit = |j: usize| {
                    if cells[j] == c::EMPTY && !open[j] {
                        open[j] = true;
                        stack.push(j as u32);
                    }
                };
                if x + 1 < W {
                    visit(i + 1);
                }
                if x > 0 {
                    visit(i - 1);
                }
                if i + W < N {
                    visit(i + W);
                }
                if i >= W {
                    visit(i - W);
                }
            }
            self.open_gas = open;
        }
        // gas lines: pipework not connected to any tank or bottle that opens onto gas carries gas
        self.gas_pipes = self
            .nets
            .iter()
            .filter(|n| n.prop == 255 && n.breaches.len() >= 2)
            .map(|n| {
                let mut f: Vec<usize> = n.breaches.iter().map(|b| b.1).collect();
                f.sort_unstable();
                f.dedup();
                f
            })
            .collect();
        let mut pipe_of = std::collections::HashMap::new();
        for (k, faces) in self.gas_pipes.iter().enumerate() {
            for &j in faces {
                pipe_of.insert(j, k);
            }
        }
        // follow the drive gas: from each generator's chamber through every turbine it reaches (one
        // turbine's exhaust can feed the next). An enclosed region (a duct) carries it on; open air doesn't.
        self.gg_ducted = vec![false; self.gg_faces.len()];
        let shaft_faces: Vec<Vec<usize>> = self.shafts.iter().map(|s| s.turb_faces.clone()).collect();
        for g in 0..self.gg_faces.len() {
            let mut seen = std::collections::HashSet::new();
            let mut seeds: Vec<Vec<usize>> = vec![self.gg_faces[g].iter().map(|f| f.1).collect()];
            let mut first = true;
            while let Some(start) = seeds.pop() {
                let mut q: std::collections::VecDeque<usize> = start.into_iter().filter(|&j| seen.insert(j)).collect();
                let mut region = 0usize;
                let mut touched = vec![];
                let mut inlet_cells: Vec<(usize, usize)> = vec![];
                let mut pipes_hit: Vec<usize> = vec![];
                while let Some(j) = q.pop_front() {
                    region += 1;
                    if region > 6000 {
                        break; // open air: not a duct
                    }
                    if let Some(&k) = face_of.get(&j) {
                        if !touched.contains(&k) {
                            touched.push(k);
                        }
                        inlet_cells.push((k, j));
                    }
                    if let Some(&pk) = pipe_of.get(&j) {
                        if !pipes_hit.contains(&pk) {
                            pipes_hit.push(pk);
                        }
                    }
                    let (x, y) = (j % W, j / W);
                    for (nx, ny) in [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)] {
                        if nx < W && ny < H {
                            let n = Self::idx(nx, ny);
                            if self.cells[n] == c::EMPTY && seen.insert(n) {
                                q.push_back(n);
                            }
                        }
                    }
                }
                if region <= 6000 {
                    // gas carries on through any gas line this region opens into
                    for pk in pipes_hit {
                        seeds.push(self.gas_pipes[pk].clone());
                    }
                    for &(k, j) in &inlet_cells {
                        if !self.shafts[k].gas_driven && !self.shafts[k].turb_in.contains(&j) {
                            self.shafts[k].turb_in.push(j);
                        }
                    }
                    for k in touched {
                        if first {
                            self.gg_ducted[g] = true;
                        }
                        if !self.shafts[k].gas_driven {
                            self.shafts[k].gas_driven = true;
                            // its other faces lead on to whatever the turbine exhausts into
                            seeds.push(shaft_faces[k].clone());
                        }
                    }
                }
                first = false;
            }
        }
        if !self.gg_ducted.iter().any(|&d| d) {
            for s in &mut self.shafts {
                s.gas_driven = false;
            }
        }
        self.compute_valve_dests();
    }

    /// Pipes on the back (1) or front (2) layer: only fuel / oxidiser lines, or erase.
    /// Paint a part; containers and plumbing get a steel wall in any open cell around them.
    pub fn paint_plumbing(&mut self, i: usize, v: u8) {
        if v == c::TEATEB {
            // a TEA-TEB bottle is a helium-style bottle marked with its fluid
            self.set_cell(i, c::HE_TANK);
            self.fluid[i] = TEA_FLUID;
            self.wall_around(i);
            return;
        }
        if matches!(v, c::TANK_F | c::TANK_O) && self.cells[i] != v {
            let (x, y) = (i % W, i / W);
            self.fluid[i] = [(x, y + 1), (x, y.wrapping_sub(1)), (x.wrapping_sub(1), y), (x + 1, y)]
                .into_iter()
                .filter(|&(nx, ny)| nx < W && ny < H)
                .map(|(nx, ny)| Self::idx(nx, ny))
                .find(|&j| self.cells[j] == v)
                .map_or(0, |j| self.fluid[j]);
        }
        self.set_cell(i, v);
        if c::auto_walls(v) {
            self.wall_around(i);
        }
    }

    pub fn wall_around(&mut self, i: usize) {
        let (x, y) = ((i % W) as i32, (i / W) as i32);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < W as i32 && ny < H as i32 {
                    let j = Self::idx(nx as usize, ny as usize);
                    if self.cells[j] == c::EMPTY {
                        self.set_cell(j, c::STEEL);
                    }
                }
            }
        }
    }

    /// Give every container and pipe in the grid its wall (used when building presets).
    pub fn auto_wall_all(&mut self) {
        for i in 0..N {
            if c::auto_walls(self.cells[i]) {
                let (x, y) = ((i % W) as i32, (i / W) as i32);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < W as i32 && ny < H as i32 {
                            let j = Self::idx(nx as usize, ny as usize);
                            if self.cells[j] == c::EMPTY {
                                self.cells[j] = c::STEEL;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn set_layer_cell(&mut self, layer: u8, i: usize, v: u8) {
        if !matches!(v, c::EMPTY | c::PIPE) {
            return;
        }
        let arr = if layer == 1 { &mut self.back } else { &mut self.front };
        if arr[i] != v {
            arr[i] = v;
            self.nets_dirty = true;
            self.mat_stale = true;
        }
    }

    /// GPU word per cell for the back and front pipe layers: type and network id of each.
    pub fn gpu_layers(&self) -> Vec<u32> {
        (0..N)
            .map(|i| {
                let net = |n: u16| if n == NO_NET { 255u32 } else { n as u32 };
                self.back[i] as u32 | (net(self.layer_net[0][i]) << 8) | ((self.front[i] as u32) << 16) | (net(self.layer_net[1][i]) << 24)
            })
            .collect()
    }

    /// GPU-side material word: cell type, network id, rotor block id.
    /// Does liquid (or helium) fill this plumbing cell: its line is joined to a tank with something in it, and
    /// the liquid has run down the line as far as here (an empty pipe is just a hole).
    pub fn wet_cell(&self, i: usize) -> bool {
        if self.net_of[i] == NO_NET {
            return false;
        }
        let n = &self.nets[self.net_of[i] as usize];
        match n.prop {
            0 | 1 => n.tank_cells > 0 && n.mass_frac > 0.0 && self.phase[i] <= n.front,
            PROP_HE => true,
            _ => false,
        }
    }

    pub fn gpu_mat(&self, i: usize) -> u32 {
        // pipes and injectors with nothing in them don't cool the wall (bit 24: the shader treats them as a cavity)
        let dry = matches!(self.cells[i], c::PIPE | 5 | c::INJ) && !self.wet_cell(i);
        let n = if self.net_of[i] == NO_NET { 255 } else { self.net_of[i] as u32 };
        let b = if self.cells[i] == c::HE_TANK { self.he_of[i].min(255) as u32 } else { self.block_of[i] as u32 };
        self.cells[i] as u32 | (n << 8) | (b << 16) | ((dry as u32) << 24)
    }

    pub fn net_temps(&self) -> [f32; 256] {
        let mut t = [300.0; 256];
        let lim = self.st.p().t_cool_max;
        for (i, n) in self.nets.iter().enumerate().take(255) {
            t[i] = n.temp;
            if n.prop == 0 && n.temp > lim {
                // coking / decomposition fouls the passages: walls see a much hotter sink
                t[i] = n.temp + 4.0 * (n.temp - lim);
            }
        }
        t
    }

    /// Animation data for the renderer: per-network line flow phase, then
    /// (x0, y0, w, h, angle, horizontal, stages, 0) per rotor block, tank levels and liquid fronts.
    pub fn anim(&self) -> Vec<f32> {
        let mut a = vec![0.0f32; ANIM_LEN];
        for (i, n) in self.nets.iter().enumerate().take(255) {
            a[i] = n.flow_off;
        }
        for (k, b) in self.blocks.iter().enumerate() {
            let th = self.shafts.get(b.shaft).map_or(0.0, |s| s.theta);
            let o = 256 + 8 * k;
            if b.shaft == usize::MAX {
                // (x0, y0, w, h, outlet end +1 / -1, horizontal, gas temperature / 3500, how hard it burns)
                let g = (0..self.gg_cell0.len()).find(|&g| self.block_of[self.gg_cell0[g]] as usize == k);
                let (dir, fneg) = self.burner_view.get(k).copied().unwrap_or((1.0, true));
                let (inten, temp) = g.map_or((0.0, 0.0), |g| (self.gg_vis[g].0, self.gg_vis[g].1));
                let temp = temp + if fneg { 0.0 } else { 2.0 };
                a[o..o + 8].copy_from_slice(&[b.x0, b.y0, b.w, b.h, dir, b.horizontal as u8 as f32, temp, inten]);
                continue;
            }
            a[o..o + 8].copy_from_slice(&[b.x0, b.y0, b.w, b.h, th, b.horizontal as u8 as f32, b.stages, self.shafts.get(b.shaft).map_or(0.0, |s| s.x().clamp(0.0, 1.5))]);
        }
        let burners: Vec<usize> = self.blocks.iter().enumerate().filter(|(_, b)| b.shaft == usize::MAX).map(|(k, _)| k).take(8).collect();
        a[ANIM_AIR] = self.wind();
        a[ANIM_AIR + 1] = (self.st.amb_p / 101325.0).clamp(0.0, 1.0);
        a[ANIM_AIR + 2] = self.air_phase;
        a[ANIM_PLUME..ANIM_PLUME + 18].copy_from_slice(&self.plume);
        for (k, v) in self.noz.iter().enumerate().take(2 * NOZ_ROWS) {
            a[ANIM_NOZ + k] = *v;
        }
        a[ANIM_TEA] = (self.teateb_at % W) as f32;
        a[ANIM_TEA + 1] = (self.teateb_at / W) as f32;
        a[ANIM_TEA + 2] = self.teateb_flash;
        for (k, b) in self.he_bottles.iter().enumerate().take(64) {
            a[ANIM_TEA + 3 + k] = b.tea as u8 as f32;
        }
        if !self.fx_plume {
            a[ANIM_PLUME] = 0.0;
        }
        a[ANIM_BURN] = burners.len() as f32;
        for (q, k) in burners.iter().enumerate() {
            a[ANIM_BURN + 1 + q] = *k as f32;
        }
        for (i, n) in self.nets.iter().enumerate().take(255) {
            if n.tank_cells > 0 {
                let full = n.tank_y1 + 1.0 - n.tank_y0;
                a[ANIM_TANK + i] = n.tank_y1 + 1.0 - n.mass_frac * full;
            }
            a[ANIM_FRONT + i] = n.front;
            a[ANIM_PROP + i] = n.prop as f32;
            a[ANIM_BOIL + i] = if n.boil > 0.0 { 1.0 } else { 0.0 };
            a[ANIM_VAP + i] = (n.p_tank / 8.0e5).clamp(0.08, 1.0);
        }
        for (k, b) in self.he_bottles.iter().enumerate().take(MAX_BLOCKS) {
            a[ANIM_HE + k] = if b.p0 > 0.0 { b.p / b.p0 } else { 0.0 };
        }
        a
    }

    /// Where the main nozzle ends and what the gas is doing there, for the plume shader: walk down from a
    /// gas cell of the chamber while the gas is walled in on both sides; the row below the last one is the exit.
    fn find_plume(&self, rb: &Readback, noz: &mut Vec<f32>) -> [f32; 18] {
        let mut out = [0.0f32; 18];
        let Some(&(_, gc)) = self.nets.iter().filter(|n| n.prop != PROP_HE).max_by_key(|n| n.injectors.len()).and_then(|n| n.injectors.first()) else { return out };
        if let Some(g) = self.spike_geom() {
            return self.spike_plume(rb, g);
        }
        let (mut xc, y0) = (gc % W, gc / W);
        let mut last: Option<(usize, usize, usize)> = None;
        let mut rows = 0;
        let (mut throat_w, mut throat_y) = (usize::MAX, 0usize);
        for y in y0..H {
            if self.cells[Self::idx(xc, y)] != c::EMPTY {
                break;
            }
            let (mut l, mut r) = (xc, xc);
            while l > 0 && self.cells[Self::idx(l - 1, y)] == c::EMPTY {
                l -= 1;
            }
            while r + 1 < W && self.cells[Self::idx(r + 1, y)] == c::EMPTY {
                r += 1;
            }
            if l == 0 || r + 1 >= W || r - l > 400 {
                break;
            }
            last = Some((l, r, y));
            if r - l < throat_w {
                throat_w = r - l;
                throat_y = y;
            }
            rows += 1;
            if noz.len() < 2 * NOZ_ROWS {
                noz.push(l as f32);
                noz.push(r as f32);
            }
            xc = (l + r) / 2;
        }
        let Some((l, r, y)) = last else { return out };
        if rows < 15 || y + 1 >= H {
            return out;
        }
        let pr = self.blended_prop();
        let hw = (r - l + 1) as f32 * 0.5;
        let xcf = (l + r) as f32 * 0.5 + 0.5;
        // the stylised plume draws one round nozzle: if the injectors are not centred on the passage found (an aerospike's two
        // slots, either side of its spike), there is no such nozzle, and the simulation's own picture is left to show
        if let Some(n) = self.nets.iter().filter(|n| n.prop != PROP_HE).max_by_key(|n| n.injectors.len()) {
            let mean = n.injectors.iter().map(|&(_, g)| (g % W) as f32 + 0.5).sum::<f32>() / n.injectors.len().max(1) as f32;
            if (mean - xcf).abs() > 0.5 * hw + 3.0 {
                return out;
            }
        }
        let (mut speed, mut pe, mut mach) = (0.0, self.st.amb_p, 1.0);
        if rb.valid {
            let s = rb.state[Self::idx((l + r) / 2, y)];
            let rho = s[0].max(1e-6);
            pe = gas_p(s, pr.gamma);
            let t = pe / (rho * pr.rgas);
            speed = (s[1] * s[1] + s[2] * s[2]).sqrt() / rho;
            mach = speed / (pr.gamma * pr.rgas * t.max(50.0)).sqrt();
        }
        out[0] = 1.0;
        out[1] = xcf;
        out[2] = (y + 1) as f32;
        out[3] = hw;
        out[4] = speed;
        out[5] = pe / self.st.amb_p.max(1.0);
        out[6] = mach;
        out[7] = self.g.t_flame.max(1500.0);
        let burn = ((pe / 2.0e4).min(1.0)).sqrt() * ((speed - 200.0) / 1300.0).clamp(0.0, 1.0);
        out[8] = burn;
        out[9] = throat_y as f32;
        out[12] = y0 as f32; // the injector row: the top of the chamber
        // Flow separation. Where the isentropic wall pressure in the bell falls below about 0.4 of the air's (the Summerfield
        // criterion) the flow lets go of the wall, so the jet leaves the nozzle with the width, Mach number and pressure it had
        // at that row, narrower than the exit. The pressure at each row comes from the bell's own contour (area ratio: the width ratio in the planar
        // simulation, its square for a round engine) and the chamber pressure.
        out[15] = hw;
        out[17] = self.st.axi as u8 as f32;
        let (pt, pa) = (self.g.chamber_p, self.st.amb_p);
        let nrows = noz.len() / 2;
        if pt > 5.0e4 && pa > 1.0e3 && nrows >= 15 && throat_y >= y0 && throat_y - y0 < nrows {
            let gam = pr.gamma.clamp(1.05, 1.67);
            let k_t = throat_y - y0;
            let width = |k: usize| noz[2 * k + 1] - noz[2 * k] + 1.0;
            let wt = width(k_t).max(1.0);
            let row = |k: usize| -> (f32, f32) {
                let m = mach_from_area((width(k) / wt).powi(if self.st.axi { 2 } else { 1 }).max(1.0), gam);
                (pt * p_over_pt(m, gam), m)
            };
            let p_exit = row(nrows - 1).0;
            if p_exit < SEPARATION_P * pa {
                if let Some((k, (p, m))) = (k_t..nrows).map(|k| (k, row(k))).find(|(_, (p, _))| *p < SEPARATION_P * pa) {
                    out[15] = width(k) * 0.5;
                    out[5] = p / pa;
                    out[6] = m;
                    out[16] = ((SEPARATION_P - p_exit / pa) / 0.3).clamp(0.0, 1.0);
                }
            }
        }
        out
    }

    /// An aerospike's geometry, if the engine is one: injectors on both sides of a spike (the axis between them has solid cells
    /// under the injectors). (axis x, row of the top of the injectors, row of the base of the spike, half-span of the injectors)
    fn spike_geom(&self) -> Option<(f32, usize, usize, f32)> {
        let n = self.nets.iter().filter(|n| n.prop != PROP_HE).max_by_key(|n| n.injectors.len())?;
        let xs: Vec<usize> = n.injectors.iter().map(|&(_, g)| g % W).collect();
        let (xmin, xmax) = (*xs.iter().min()?, *xs.iter().max()?);
        if xmax - xmin < 24 {
            return None;
        }
        let y0 = n.injectors.iter().map(|&(_, g)| g / W).min()?;
        let xm = (xmin + xmax) / 2;
        // the spike: solid cells down the middle, below the injectors
        let solid = |y: usize| (xm.saturating_sub(8)..=xm + 8).any(|x| x < W && self.cells[Self::idx(x, y)] != c::EMPTY);
        if !(y0 + 5..y0 + 40).any(|y| y < H && solid(y)) {
            return None;
        }
        let (mut y, mut last, mut gap) = (y0, y0, 0);
        while y + 1 < H && gap < 10 {
            if solid(y) {
                last = y;
                gap = 0;
            } else {
                gap += 1;
            }
            y += 1;
        }
        if last - y0 < 15 || last + 20 >= H {
            return None;
        }
        Some((xm as f32 + 0.5, y0, last, (xmax - xmin) as f32 * 0.5 + 0.5))
    }

    /// The plume of an aerospike: a virtual nozzle whose exit is the base of the spike, as wide as the base and the two sheets of
    /// exhaust that run down its sides to it, with the speed and pressure of those sheets just below the base. Ambient-adapting, so
    /// its pressure ratio sits near 1 and the diamonds are weak.
    fn spike_plume(&self, rb: &Readback, (xm, y0, yb, _half): (f32, usize, usize, f32)) -> [f32; 18] {
        let mut out = [0.0f32; 18];
        let pr = self.blended_prop();
        // the base: the width of the solid cells (and the duct between them) a few rows above the end of the spike
        let row = yb.saturating_sub(2);
        let xi = xm as usize;
        let ext = |dir: i32| -> f32 {
            let (mut x, mut far) = (xi as i32, 0);
            for k in 0..50 {
                let xx = (xi as i32 + dir * k).clamp(0, W as i32 - 1);
                if self.cells[Self::idx(xx as usize, row)] != c::EMPTY {
                    far = k;
                }
                x = xx;
            }
            let _ = x;
            far as f32
        };
        let mut hw = (0.5 * (ext(-1) + ext(1)) + 7.0).clamp(8.0, 45.0);
        // the exhaust itself: the span of fast gas a few rows under the base (the sheets have not yet closed up behind the spike)
        if rb.valid {
            let yy = (yb + 9).min(H - 2);
            let (mut lo, mut hi) = (usize::MAX, 0usize);
            for x in xi.saturating_sub(80)..=(xi + 80).min(W - 2) {
                let st = rb.state[Self::idx(x, yy)];
                let rho = st[0].max(1e-6);
                if (st[1] * st[1] + st[2] * st[2]).sqrt() / rho > 300.0 {
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            if lo < hi {
                hw = hw.max(0.5 * (hi - lo) as f32 + 3.0).min(55.0);
            }
        }
        let ye = yb + 1;
        let (mut speed, mut pe, mut mach) = (0.0f32, self.st.amb_p, 2.0f32);
        if rb.valid {
            // the fastest gas just under the base, across the sheets
            // (taken along the ramp just above the base, where the sheets run at full speed: under the base the air rushing
            // past a climbing vehicle can stall the gas, and the exhaust's strength should not depend on that)
            let yy = yb.saturating_sub(4);
            let mut best = 0.0f32;
            for x in (xi.saturating_sub(hw as usize + 20))..=(xi + hw as usize + 20).min(W - 2) {
                let s = rb.state[Self::idx(x, yy)];
                let rho = s[0].max(1e-6);
                let v = (s[1] * s[1] + s[2] * s[2]).sqrt() / rho;
                if v > best {
                    best = v;
                    let p = gas_p(s, pr.gamma);
                    let t = p / (rho * pr.rgas);
                    (speed, pe, mach) = (v, p, v / (pr.gamma * pr.rgas * t.max(50.0)).sqrt());
                }
            }
        }
        out[0] = 1.0;
        out[1] = xm;
        out[2] = ye as f32;
        out[3] = hw;
        out[4] = speed;
        out[5] = (pe / self.st.amb_p.max(1.0)).clamp(0.8, 4.0);
        out[6] = mach.clamp(1.6, 4.0);
        out[7] = self.g.t_flame.max(1500.0);
        out[8] = ((pe / 2.0e4).min(1.0)).sqrt() * ((speed - 200.0) / 1300.0).clamp(0.0, 1.0);
        out[9] = ye as f32; // no throat or bell to tint: the ramp's own gas is drawn by the simulation
        out[12] = ye as f32;
        out[15] = hw;
        out[17] = 2.0 + self.st.axi as u8 as f32; // 2: an aerospike (the shader hides the simulation's gas below the base)
        let _ = y0;
        out
    }

    /// Where the jet stabiliser starts: below the exit of the bell the plume code found, or, for an engine with no bell to find (an
    /// aerospike), below the tip of the spike down the middle of its injectors, with the span of the injectors as the width.
    pub fn find_stab_geom(&self) -> Option<(f32, f32)> {
        if self.plume[0] > 0.5 && self.plume[17] < 1.5 {
            return Some((self.plume[2], self.plume[3]));
        }
        let n = self.nets.iter().filter(|n| n.prop != PROP_HE).max_by_key(|n| n.injectors.len())?;
        let xs: Vec<usize> = n.injectors.iter().map(|&(_, g)| g % W).collect();
        let (xmin, xmax) = (*xs.iter().min()?, *xs.iter().max()?);
        let y0 = n.injectors.iter().map(|&(_, g)| g / W).min()?;
        let xm = (xmin + xmax) / 2;
        // down the middle of the spike to its base: the lowest row with a solid cell within 8 cells of the axis, stopping at the
        // first run of empty rows (a duct through the spike, with solid cells either side, doesn't end it)
        let (mut y, mut last, mut gap) = (y0, y0, 0);
        while y + 1 < H && gap < 10 {
            if (xm.saturating_sub(8)..=xm + 8).any(|x| x < W && self.cells[Self::idx(x, y)] != c::EMPTY) {
                last = y;
                gap = 0;
            } else {
                gap += 1;
            }
            y += 1;
        }
        let y = last;
        if y - y0 < 15 || y + 1 >= H {
            return None;
        }
        Some((y as f32, (xmax - xmin) as f32 * 0.5 + 0.5))
    }

    /// The air speed past the vehicle, m/s down the screen: flying up the air rushes down past the engine,
    /// falling it rushes up into the nozzle.
    pub fn wind(&self) -> f32 {
        let v = if self.flight { self.vel } else if self.climbing { self.climb_rate * 1000.0 } else { 0.0 };
        v.clamp(-400.0, 400.0)
    }

    /// Fly the vehicle for `dt` seconds: thrust up, weight and drag down. It stands on the ground at 0 km.
    fn fly(&mut self, dt: f32) {
        const G: f32 = 9.81;
        const CDA: f32 = 10.0; // drag area of a 3.7 m stage, m2
        let m = self.veh_mass.max(1000.0);
        let h = self.altitude;
        let rho = atmosphere_p(h) / (287.0 * (288.15 - 6.5 * h).max(216.65));
        let drag = 0.5 * rho * self.vel * self.vel.abs() * CDA;
        let thrust = self.thrust_f.max(0.0);
        let a = (thrust - drag) / m - G;
        if h <= 0.0 && self.vel <= 0.0 && a <= 0.0 {
            self.vel = 0.0; // on the pad
            self.altitude = 0.0;
        } else {
            self.vel += a * dt;
            self.altitude += self.vel * dt / 1000.0;
            if self.altitude <= 0.0 {
                let v = -self.vel;
                self.altitude = 0.0;
                self.vel = 0.0;
                if v > 0.5 {
                    let msg = if v <= 3.0 {
                        format!("Landed: touchdown at {:.1} m/s, {:.0} kg of propellant left", v, (self.veh_mass - self.dry_mass).max(0.0))
                    } else if v <= 8.0 {
                        format!("Hard landing at {:.1} m/s: the legs would be damaged", v)
                    } else {
                        format!("Crashed at {:.0} m/s", v)
                    };
                    self.event(msg.clone());
                    self.flight_msg = msg;
                }
            }
        }
        // propellant burned off the vehicle
        let used = (self.g.mdot_f + self.g.mdot_o) * dt;
        self.veh_mass = (self.veh_mass - used).max(self.dry_mass);
    }

    /// Lumped plumbing, turbopump and failure model. `dt` is real frame time.
    pub fn update(&mut self, rb: &Readback, dt: f32) {
        if self.nets_dirty && !self.hold_rebuild {
            self.rebuild_nets();
        }
        // altitude: climbing raises it, and it sets the ambient pressure (down to what the gas solver can hold)
        let dt_sim = dt * self.st.lumped_rate;
        self.teateb_flash = (self.teateb_flash - dt_sim / 1.6).max(0.0);
        self.tea_age += dt_sim;
        self.air_phase = (self.air_phase + self.wind() / 250.0 * dt) % 2000.0;
        // the start purge shows as a white cloud of cold gas at the nozzle exit: quick to build, slow to clear
        let purge_to = if self.purge_t > 0.0 { 1.0 } else { 0.0 };
        let purge_rate = if purge_to > self.purge_vis { 1.0 / 0.4 } else { 1.0 / 1.8 };
        self.purge_vis += (purge_to - self.purge_vis).clamp(-purge_rate * dt_sim, purge_rate * dt_sim);
        let mut noz = std::mem::take(&mut self.noz);
        noz.clear();
        self.plume = self.find_plume(rb, &mut noz);
        self.noz = noz;
        self.plume[10] = self.purge_vis;
        self.plume[13] = self.fx_style as f32;
        self.stab_geom = self.find_stab_geom();
        // how hard the engine is running: chamber pressure against its recent peak (about the flow, and so the thrust)
        self.pc_ref = (self.pc_ref * (1.0 - dt_sim / 90.0)).max(self.g.chamber_p);
        self.plume[14] = if self.pc_ref > 2.0e5 { (self.g.chamber_p / self.pc_ref).clamp(0.0, 1.0) } else { 1.0 };
        self.plume[11] = ((2.4 - self.tea_age) / 1.3).clamp(0.0, 1.0); // green exhaust glow: full for the first 1.1 s (the plume takes about that long to build), gone by 2.4 s
        if self.flight {
            self.fly(dt_sim);
            self.alt_on = true;
        } else if self.climbing {
            self.altitude = (self.altitude + self.climb_rate * dt_sim).min(120.0);
            self.alt_on = true;
        }
        if self.alt_on {
            self.st.amb_p = atmosphere_p(self.altitude).max(MIN_AMBIENT_P);
        }
        // the air in the world follows a change in the ambient pressure for a few seconds after it
        if (self.st.amb_p - self.amb_prev).abs() > 2.0e-4 * self.amb_prev.max(1.0) {
            self.amb_hot = 4.0;
        }
        self.amb_prev = self.st.amb_p;
        self.amb_hot = (self.amb_hot - dt_sim).max(0.0);
        self.amb_k = if self.wind().abs() > 1.0 { 0.02 } else if self.amb_hot > 0.0 { 0.004 } else { 0.0 };
        let mut st = self.st;
        // gas moved through turbines, injectors and pipes is taken out of its cells on every gas step of the
        // frame: the caps that stop a cell being emptied have to count the steps that really run (a slow
        // frame runs more of them)
        if self.frame_steps > 0 {
            st.steps_per_frame = self.frame_steps;
        }
        let dtl = dt * st.lumped_rate;
        self.run_t += dtl;
        let blended = self.blended_prop();
        let pr = &blended;
        let pump_k = 1.0f32;
        let mut g = Gauges::default();

        // start / shutdown sequence
        if !self.seq.is_empty() {
            self.seq_t += dtl;
            let t = self.seq_t;
            let due: Vec<SeqAction> = self.seq.iter().filter(|(at, _)| *at <= t).map(|(_, a)| *a).collect();
            self.seq.retain(|(at, _)| *at > t);
            for a in due {
                match a {
                    SeqAction::Valves(p, open) => self.command_all(p, open),
                    SeqAction::TeaValves(open) => {
                        for v in &mut self.valves {
                            if v.tea {
                                v.cmd = open;
                            }
                        }
                    }
                }
            }
        }

        if self.purge_t > 0.0 {
            self.purge_t -= dtl;
            if self.purge_t <= 0.0 {
                self.priming = 0.0;
                self.event("start: purge done, priming, fuel is running down the lines".into());
            }
        }
        if self.priming >= 0.0 {
            self.priming += dtl;
            // no fuel network reaching injectors at all (odd builds): carry on after a short wait
            let none = !self.nets.iter().any(|n| n.prop == 0 && n.tank_cells > 0 && !n.injectors.is_empty());
            if self.fuel_primed() || self.priming > 20.0 || (none && self.priming > 0.5) {
                self.ignition_sequence();
            }
        }

        // valve travel; a fully closed valve splits the network, an opening one joins it
        // (soft start and shutdown move them slowly: at least 1.5 s from shut to open)
        let soft = self.soft_t > 0.0 || self.start_t > 0.0;
        let stroke = if soft { st.valve_stroke.max(1.5) } else { st.valve_stroke };
        let rate = dtl / stroke.max(0.005);
        if self.soft_t > 0.0 {
            self.soft_t -= dtl;
            self.shut_thr = (self.shut_thr - dtl * 0.6).max(0.4);
        } else if self.start_t > 0.0 {
            // held at 40% until the chamber has lit, then brought up to the throttle setting over ~2.5 s
            self.start_t -= dtl;
            if self.ignited && self.priming < 0.0 && self.purge_t <= 0.0 {
                self.shut_thr = (self.shut_thr + dtl * 0.25).min(1.0);
                if self.shut_thr >= 1.0 {
                    self.start_t = 0.0;
                }
            } else {
                self.shut_thr = 0.4;
            }
        } else {
            self.shut_thr = 1.0;
        }
        let mut retype: Vec<(usize, u8)> = vec![];
        for v in &mut self.valves {
            v.pos = if v.cmd { (v.pos + rate).min(1.0) } else { (v.pos - rate).max(0.0) };
            let closed_type = matches!(self.cells[v.cells[0]], c::VALVE_X);
            let (open_t, shut_t) = (c::VALVE, c::VALVE_X);
            if v.cmd && closed_type {
                retype.extend(v.cells.iter().map(|&i| (i, open_t)));
            } else if !v.cmd && v.pos <= 0.0 && !closed_type {
                retype.extend(v.cells.iter().map(|&i| (i, shut_t)));
            }
        }
        for (i, t) in retype {
            self.set_cell(i, t);
        }

        // wall heat picked up by each coolant network
        if rb.valid && rb.sim_time > 0.0 {
            for n in &mut self.nets {
                let q: f32 = n.lines.iter().map(|&i| rb.flux[i]).sum::<f32>() / rb.sim_time;
                n.heat_w += (q - n.heat_w) * 0.3;
            }
        }
        for n in &mut self.nets {
            let (tin, cp) = if n.prop == 0 { (n.fp.t_in_f, n.fp.cp_f) } else { (n.fp.t_in_o, n.fp.cp_o) };
            // the flat 2D chamber has much less wall than a round one: scale up the heat the coolant gets
            // (the hardware's own heat capacity, scaled the same way, keeps standing coolant from running away)
            let k = st.turb_mult.max(0.1);
            let target = (tin + k * n.heat_w.max(0.0) / (n.mdot * cp + 200.0 * k)).min(1500.0);
            n.temp += (target - n.temp) * (1.0 - (-dtl / 0.3).exp());
        }

        // tank pressure: helium-regulated while the bottles last, otherwise ullage blowdown
        let mut ruptured: Vec<(usize, f32)> = vec![];
        let mut p_reg = 0.0f32;
        for n in &mut self.nets {
            n.head = 0.0;
            n.boil = 0.0;
            if n.tank_cells == 0 || n.prop > 1 {
                n.p_tank = 0.0;
                continue;
            }
            if st.gse_tanks {
                n.mass_frac = 1.0; // ground support equipment keeps it topped up
            }
            // a regulator with its own setting holds that; otherwise the shared setting for the tank
            let set = if n.reg_set > 0.0 && n.reg_bottle != NO_NET {
                n.reg_set
            } else if n.prop == 0 {
                st.tank_p_f
            } else {
                st.tank_p_o
            };
            let rho = if n.prop == 0 { n.fp.rho_f } else { n.fp.rho_o };
            let [hvap, rvap, tsat] = if n.prop == 0 { n.fp.boil_f } else { n.fp.boil_o };
            // ullage: the space above the liquid (5% even when full)
            let v_tank = n.tank_cells as f32 * 100.0 / rho;
            let v_u = (v_tank * (1.05 - n.mass_frac)).max(0.05 * v_tank);
            if n.ull_pv <= 0.0 {
                n.ull_pv = set * v_u;
            }
            // cryogens boil from the heat leaking in through the tank walls
            if hvap > 0.0 && n.mass_frac > 0.0 {
                let q = n.tank_faces as f32 * 2000.0 * st.dx * st.depth * (300.0 - tsat).max(0.0);
                n.boil = q / hvap * st.boil_speed;
                n.mass_frac = (n.mass_frac - n.boil * dtl / (n.tank_cells as f32 * 100.0)).max(0.0);
                n.ull_pv += n.boil * rvap * tsat * dtl;
            }
            let mut p = n.ull_pv / v_u;
            // a regulator piped from a helium bottle tops the ullage up to the set pressure while the bottle lasts
            let reg = match self.he_bottles.get(n.reg_bottle as usize) {
                Some(b) if n.reg_bottle != NO_NET => set.min(0.95 * b.p),
                _ => 0.0,
            };
            p_reg = p_reg.max(reg);
            if p < reg {
                p = reg;
            }
            let relief = set.max(1.0e5) * 1.3 + 0.5e5; // vent valve a little above working pressure
            if st.relief && p > relief {
                p = relief; // relief valve vents the extra vapour
            }
            if p > 3.5 * set.max(1.0e5) + 3.0e5 && st.failures {
                ruptured.push((n.tank_top, p));
                p = 0.5 * p;
            }
            n.ull_pv = p * v_u;
            n.p_tank = p;
        }
        for (top, p) in ruptured {
            // the tank splits at its top: knock out the wall above it so the contents vent
            let (x, y) = (top % W, top / W);
            for dx in 0..3usize {
                let xx = (x + dx).saturating_sub(1);
                if y >= 1 && xx < W {
                    self.set_cell(Self::idx(xx, y - 1), c::EMPTY);
                }
            }
            self.event(format!("tank ruptured from boil-off pressure ({:.1} bar) at ({}, {})", p / 1e5, x, y));
        }
        g.p_reg = p_reg;

        // pumps: head from the pump curve, limited by cavitation (NPSH)
        let mut cav_any = false;
        for n in &mut self.nets {
            n.h0 = 0.0;
            n.kq = 0.0;
        }
        for s in &mut self.shafts {
            let w = s.omega;
            let mut load = 0.0;
            for p in &mut s.pumps {
                let n = &self.nets[p.net];
                let rho = if p.prop == 0 { n.fp.rho_f } else { n.fp.rho_o };
                let p_vap = if p.prop == 0 { n.fp.p_vap_f } else { n.fp.p_vap_o };
                let m = n.mdot;
                // suction: tank pressure less vapour pressure and the dynamic head of the liquid entering the eye
                let c_eye = m / (rho * p.eye_area());
                p.npsp_avail = n.p_tank - p_vap - 0.5 * rho * c_eye * c_eye;
                p.npsp_req = p.npsp_needed(w, m, rho);
                // vapour cavities take a few hundredths of a second to grow or collapse
                let cav = PumpRef::cav_factor(p.npsp_avail, p.npsp_req);
                p.cav += (cav - p.cav) * (1.0 - (-dtl / 0.05).exp());
                p.wet = n.front >= p.phase && n.pumps_ok();
                if p.cav < 0.95 && m > 1.0 && pump_k > 0.0 && p.wet {
                    cav_any = true;
                }
                let (h0, kq) = p.curve(w, rho);
                let k = if p.wet { pump_k * p.cav } else { 0.0 };
                let h = h0 - kq * m * m;
                p.head = k * h.max(-0.5 * h0);
                p.eff = if p.wet { p.efficiency(w, m, rho) * p.cav } else { 0.0 };
                p.power = p.shaft_power(w, m, rho, p.head, p.wet && pump_k > 0.0, p.cav);
                load += p.power;
                let n = &mut self.nets[p.net];
                n.h0 += k * h0;
                n.kq += k * kq;
                n.head += p.head;
            }
            s.p_load = load;
        }
        if cav_any && self.events.last().map_or(true, |e| !e.contains("cavitat")) {
            self.event("pump cavitating: raise tank pressure or lower pump speed".into());
        }
        g.cavitating = cav_any;

        // turbines: share the available drive power, efficiency depends on blade speed
        let q_reg: f32 = st.turb_mult * self.nets.iter().filter(|n| n.prop == 0).map(|n| n.heat_w.max(0.0)).sum::<f32>();
        // expander drive: the fuel warmed in the cooling jacket runs through the turbines its lines lead
        // into. Each takes the line's whole flow (turbines in series) and expands it across its pressure
        // ratio (about 1.3 a stage), so its power is that flow times the enthalpy the gas gives up, and no
        // more. The pressure drop that work costs (what the turbine actually took last step, up to its full
        // ratio) is lost to the injectors. Only a fuel that boils (hydrogen, methane)
        // can do this: a storable liquid like kerosene gives a turbine nothing.
        for n in &mut self.nets {
            n.turb_dp = 0.0;
        }
        let mut regen_of = vec![(0.0f32, 900.0f32); self.shafts.len()]; // (gas power, spouting velocity)
        let mut turb_dp: Vec<(usize, f32)> = vec![];
        for (k, s) in self.shafts.iter().enumerate() {
            let pr_t = 1.3f32.powf(s.turb_stages);
            let (mut p_sum, mut pc0) = (0.0f32, 0.0f32);
            for &n in &s.regen_nets {
                let net = &self.nets[n];
                // (gamma - 1) / gamma of this line's fuel; a storable liquid gives a turbine nothing
                let kappa = if net.fp.boil_f[1] > 0.0 { net.fp.boil_f[1] / net.fp.cp_f } else { 0.0 };
                if kappa <= 0.0 {
                    continue;
                }
                let frac = 1.0 - pr_t.powf(-kappa);
                let dh = net.fp.cp_f * net.temp * frac;
                let p = net.m_inj.max(0.0) * dh;
                p_sum += p;
                pc0 += p * (2.0 * dh).sqrt();
                // isentropic: work w = R T / kappa * (1 - PR^-kappa) per kg, so a pressure drop of about
                // p * kappa * w / (R T) for the work this line's gas actually gave the turbine
                let share = s.p_exp / s.regen_nets.len() as f32;
                let w_kg = share / net.m_inj.max(0.05);
                let dp = net.p_up.max(0.0) * kappa * w_kg / (net.fp.boil_f[1] * net.temp.max(20.0));
                turb_dp.push((n, dp.min(net.p_up.max(0.0) * (1.0 - 1.0 / pr_t))));
            }
            regen_of[k] = (p_sum, if p_sum > 1.0 { pc0 / p_sum } else { 900.0 });
        }
        for (n, dp) in turb_dp {
            self.nets[n].turb_dp += dp;
        }
        // the start cartridge blows through every turbine, sharing its gas by turbine size
        let self_turb_all: usize = self.shafts.iter().map(|s| s.turb_cells).sum();
        // start cartridge: a solid-propellant gas generator blowing hot gas through the turbines; it burns at
        // full rate after a short ignition rise and tails off over its last half second
        let cart_w = if st.dead_start || self.helium_start_line() { 0.0 } else if st.he_spin { st.cartridge_w.max(1.0e7) } else { st.cartridge_w };
        let mut cart_p = if self.cartridge_t > 0.0 { cart_w * (self.cartridge_t / 0.5).min(1.0) } else { 0.0 };
        // helium spin start: the same push, but cold helium from the bottle expanding through the turbines
        // (about 0.6 of the ideal work of the pressure ratio); the bottle empties as it goes
        if st.he_spin && cart_p > 0.0 && !self.helium_start_line() {
            let best = (0..self.he_bottles.len()).filter(|&b| self.he_bottles[b].p > 4.0e5).max_by(|&a, &b| self.he_bottles[a].p.total_cmp(&self.he_bottles[b].p));
            if let Some(bi) = best {
                let bot = &mut self.he_bottles[bi];
                let pr = (bot.p / 3.0e5).max(1.0);
                let w_spec = 0.6 * 5193.0 * 300.0 * (1.0 - pr.powf(-0.4));
                let mass = bot.mass().max(1e-3);
                let dm = (cart_p / w_spec * dtl).min(0.9 * mass);
                bot.p *= 1.0 - dm / mass;
                cart_p = dm / dtl * w_spec;
            } else {
                cart_p = 0.0;
            }
        }
        const C0_CART: f32 = 1500.0; // spouting velocity of cartridge gas, m/s
        if self.cartridge_t > 0.0 {
            self.cartridge_t -= dtl;
        }
        // cartridge parts in the build do the job themselves (each shaft gets what its own cartridges give)
        let phys_cart = !st.he_spin && !st.dead_start && !self.helium_start_line() && self.shafts.iter().any(|s| s.cart_cells > 0);
        let cart_ramp = if self.cartridge_t > 0.0 { (self.cartridge_t / 0.5).min(1.0) } else { 0.0 };
        let mut wrecked: Vec<usize> = vec![];
        // throttling: with throttle valves fitted, the turbine bypass also opens to bring the pumps' discharge
        // pressure down with the throttle (how the RL10 throttles; the metering valves trim the flow)
        let throttled = self.nets.iter().any(|n| n.throttle) && self.throttle_pos < 0.999;
        let feed_set = st.feed_p_max * if throttled { self.throttle_pos.clamp(0.1, 1.0) } else { 1.0 };
        // what each pump sees this frame: (mass flow, density, tank pressure, cavitation factor, primed)
        let pump_inputs: Vec<Vec<(f32, f32, f32, f32, bool)>> = self
            .shafts
            .iter()
            .map(|s| {
                s.pumps
                    .iter()
                    .map(|p| {
                        let n = &self.nets[p.net];
                        let rho = if p.prop == 0 { n.fp.rho_f } else { n.fp.rho_o };
                        (if p.wet { n.mdot } else { 0.0 }, rho, n.p_tank, p.cav, p.wet && pump_k > 0.0)
                    })
                    .collect()
            })
            .collect();
        // electric motors run while the oxidiser valves are open or the engine is lit (they spin the pumps
        // up as the oxidiser goes in at the start, and stop when it's shut off)
        self.motors_on = self.ignited || self.valves.iter().any(|v| v.prop == 1 && v.cmd);
        let motors_on = self.motors_on;
        let escale = (st.dx / DX0).powi(2) * (st.depth / 0.36);
        let mut circuit_draw = vec![0.0f32; self.circuits.len()];
        for (k, s) in self.shafts.iter_mut().enumerate() {
            let (p_regen, c0_regen) = regen_of[k];
            // the gas going through the wheel (worked out from last frame's field)
            let (p_gas, c0) = (s.p_gas, s.c0_gas.max(300.0));
            // light rotors against big pump loads are stiff: integrate the speed in substeps
            let nsub = 40;
            let h = dtl / nsub as f32;
            let (mut p_t, mut load, mut p_e, mut p_m) = (0.0, 0.0, 0.0f32, 0.0f32);
            // what its motor can give: its own rating, and its share of what its batteries can deliver
            let p_mot_avail = match self.circuits.get(s.circuit as usize) {
                Some(cc) if motors_on && s.motor_cells > 0 && cc.energy > 0.0 => {
                    let share = s.motor_cells as f32 / cc.motors.max(1) as f32;
                    (s.motor_cells as f32 * MOTOR_W_CELL * escale).min(cc.p_max * share * MOTOR_EFF)
                }
                _ => 0.0,
            };
            let cart_share = s.turb_cells as f32 / self_turb_all.max(1) as f32;
            for _ in 0..nsub {
                let w = s.omega;
                let x = w / s.omega_rated;
                let r_t = 0.5 * s.d_turb;
                // the ratio shown: against the drive gas carrying most of the power
                let pc = if phys_cart { s.cart_cells as f32 * CART_W_CELL * escale * cart_ramp } else { cart_p * cart_share };
                let c_mix = if p_gas + p_regen + pc > 1.0 { (p_gas * c0 + p_regen * c0_regen + pc * C0_CART) / (p_gas + p_regen + pc) } else { c0 };
                s.u_c0 = w * r_t / c_mix;
                s.eff_t = turbine_eff(s.u_c0, s.turb_stages);
                let tq_exp = turbine_torque(p_regen, c0_regen, r_t, w, s.turb_stages);
                // electric motor: full torque up to its base speed (a third of the shaft's rating), constant
                // power above it
                let w_base = 0.33 * s.omega_rated;
                let tq_mot = if p_mot_avail > 0.0 { p_mot_avail / w.max(w_base) } else { 0.0 };
                let mut tq = turbine_torque(p_gas, c0, r_t, w, s.turb_stages) + tq_exp + tq_mot + turbine_torque(cart_p * cart_share, C0_CART, r_t, w, s.turb_stages);
                let mut by = 1.0f32;
                load = 0.0;
                let mut feed = 0.0f32;
                for (p, &(m, rho, p_tank, cav, wet)) in s.pumps.iter().zip(&pump_inputs[k]) {
                    let hd = if wet { pump_k * p.head_at(w, m, rho).max(0.0) * cav } else { 0.0 };
                    load += p.shaft_power(w, m, rho, hd, wet, cav);
                    // the governor counts what every pump would make: a dry one (its valve still opening, say)
                    // jumps straight to its no-flow head the moment the liquid reaches it
                    feed = feed.max(p_tank + pump_k * p.head_at(w, if wet { m } else { 0.0 }, rho).max(0.0));
                }
                if st.governor || throttled {
                    // turbine bypass: hold tip speed under 104% of rated and pump discharge under the set pressure
                    let by_speed = if st.governor { ((1.04 - x) / 0.06).clamp(0.0, 1.0) } else { 1.0 };
                    let by_feed = ((1.03 - feed / feed_set) / 0.05).clamp(0.0, 1.0);
                    by = by_speed.min(by_feed);
                    tq *= by;
                }
                p_t = tq * w;
                p_e = tq_exp * by * w;
                p_m = tq_mot * by * w;
                // bearings and seals: a breakaway drag plus a part growing with speed (about 1% of the
                // power of a big turbopump at full speed); the larger the rotor, the larger its bearings
                // (small rotors have small bearings: below the size of a big turbopump's the drag falls off steeply)
                let jr = s.j / 0.15;
                let size = if jr >= 1.0 { jr.sqrt().min(4.0) } else { jr.powf(1.25).max(0.005) };
                let tq_bear = size * (if w > 1.0 { 5.0 } else { 0.0 } + 0.01 * w);
                let tq_net = tq - load / w.max(1.0) - tq_bear;
                s.omega = (w + tq_net * h / s.j).max(0.0);
            }
            s.p_turb = p_t;
            s.p_exp = p_e;
            s.p_motor = p_m;
            if let Some(cc) = circuit_draw.get_mut(s.circuit as usize) {
                *cc += p_m / MOTOR_EFF;
            }
            s.p_load = load;
            s.theta = (s.theta + s.omega * dt * 0.003) % std::f32::consts::TAU;
            if s.x() > OVERSPEED && st.failures {
                wrecked.push(k);
            }
            g.rpm = g.rpm.max(s.rpm());
            g.p_turb += p_t;
            g.p_pump += s.p_load;
        }
        // the batteries give up what the motors drew
        for (cc, draw) in self.circuits.iter_mut().zip(circuit_draw) {
            cc.p_draw = draw;
            cc.energy = (cc.energy - draw * dtl).max(0.0);
        }
        for k in wrecked {
            let cells = self.shafts[k].cells.clone();
            for i in cells {
                self.set_cell(i, c::EMPTY);
            }
            self.shafts[k].omega = 0.0;
            self.event(format!("turbopump {} overspeed: rotor disintegrated", k + 1));
        }
        g.q_regen = q_reg;

        for n in &mut self.nets {
            n.p_up = if n.pumps_ok() { n.p_tank + n.head } else { 0.0 };
            n.gg_draw = 0.0;
        }

        // gas generators: draw both propellants, burn fuel-rich, offer gas power to the turbines
        let mut gg_t = 0.0f32;
        let mut gg_flow = false;
        let mut gg_out: Vec<(usize, f32, f32, bool)> = vec![]; // (generator, fuel kg/s, oxidiser kg/s, lit)
        let mut gg_comb = 0.0f32; // (generator, kg/s, gas temperature) burnt into its chamber
        let mut gg_power = 0.0f32;
        for (k, &(fi, oi, cf, co)) in self.gg.iter().enumerate() {
            let upf = if self.phase[cf] <= self.nets[fi].front { self.nets[fi].p_up } else { 0.0 };
            let upo = if self.phase[co] <= self.nets[oi].front { self.nets[oi].p_up } else { 0.0 };
            // the propellants push against the pressure in the generator's chamber: about 1 bar when it's
            // cold, so tank pressure alone gets it going; once running, the set pressure (what its injectors
            // are sized for)
            let p_back = match self.gg_faces.get(k) {
                Some(f) if rb.valid && !f.is_empty() => (f.iter().map(|x| gas_p(rb.state[x.1], pr.gamma)).sum::<f32>() / f.len() as f32).min(st.gg_p),
                _ => st.gg_p,
            };
            let mf = 0.7 * st.gg_area_f * 1e-4 * (2.0 * self.nets[fi].fp.rho_f * (upf - p_back).max(0.0)).sqrt();
            let mut mo = 0.7 * st.gg_area_o * 1e-4 * (2.0 * self.nets[oi].fp.rho_o * (upo - p_back).max(0.0)).sqrt();
            // with throttle valves fitted the generator's oxidiser valve is metered too (how the RS-25 and J-2
            // throttle): less oxidiser, a cooler generator, less turbine power
            if self.nets.iter().any(|n| n.throttle) {
                mo *= self.throttle_pos.clamp(0.05, 1.0);
            }
            self.nets[fi].gg_draw += mf;
            self.nets[oi].gg_draw += mo;
            if mf + mo > 1e-3 {
                let lit_k = self.gg_lit || self.gg_lumped.get(k).copied().unwrap_or(false); // a burner lights itself
                gg_out.push((k, mf, mo, lit_k && mf > 1e-3 && mo > 1e-3));
                if lit_k && mf > 1e-3 {
                    // fuel-rich: the fuel the oxidiser couldn't burn is still in the exhaust
                    gg_comb = gg_comb.max((1.0 - (mo / mf) / pr.of_opt).clamp(0.0, 1.0) * mf / (mf + mo));
                }
            }
            // for the picture: how hard it is burning, and how hot
            if let Some(v) = self.gg_vis.get_mut(k) {
                let (inten, temp) = if mf > 1e-3 && mo > 1e-3 { (((mf + mo) / 3.0).sqrt().clamp(0.35, 1.0), (t_flame(pr, mo / mf) / 3500.0).clamp(0.0, 1.0)) } else { (0.0, v.1) };
                v.0 += (inten - v.0) * (1.0 - (-dtl / 0.25).exp());
                v.1 += (temp - v.1) * (1.0 - (-dtl / 0.25).exp());
            }
            if mf > 1e-3 && mo > 1e-3 {
                gg_flow = true;
                if self.gg_lit || self.gg_lumped.get(k).copied().unwrap_or(false) {
                    let t = t_flame(pr, mo / mf);
                    let cp = pr.gamma * pr.rgas / (pr.gamma - 1.0);
                    let ratio = (st.amb_p.max(1.0e4) / st.gg_p).powf((pr.gamma - 1.0) / pr.gamma);
                    if t <= 1300.0 && !self.gg_ducted.get(k).copied().unwrap_or(false) {
                        gg_power += (mf + mo) * cp * t * (1.0 - ratio);
                    }
                    gg_t = gg_t.max(t);
                }
            }
        }
        self.gg_power = gg_power;
        self.gg_comb = gg_comb;
        if !gg_flow {
            self.gg_lit = false;
        } else if pr.hypergolic
            || self.spark_t > 0.0
            || self.igniter
            || self.gg_faces.iter().any(|f| flame_at(f.iter().map(|x| x.1), rb, pr))
        {
            self.gg_lit = true;
        }
        self.gg_hot_t = if gg_t > 1300.0 { self.gg_hot_t + dtl } else { 0.0 };
        if self.gg_hot_t > 0.5 && self.events.last().map_or(true, |e| !e.contains("preburner")) {
            self.event("preburner too hot: turbine producing no power".to_string());
        }
        g.p_gg = gg_power * 0.6;
        g.t_gg = gg_t;

        // throttle valves travel towards the throttle setting at the valve stroke speed
        let thr_rate = dtl / st.valve_stroke.max(0.005);
        let thr_target = st.throttle.clamp(0.0, 1.0) * self.shut_thr;
        self.throttle_pos += (thr_target - self.throttle_pos).clamp(-thr_rate, thr_rate);
        // mixture-ratio control (like the J-2's propellant utilisation valve): while the engine burns, trim
        // the throttle valves to hold the target O/F. More oxidiser: close the fuel valve down a little; less:
        // the oxidiser valve. It reacts over about half a second, following the measured mixture
        let both_metered = self.nets.iter().any(|n| n.throttle && n.prop == 0) && self.nets.iter().any(|n| n.throttle && n.prop == 1);
        if st.mix_target > 0.0 && both_metered && self.ignited && self.g.of > 0.05 {
            let err = (st.mix_target / self.g.of).ln().clamp(-0.5, 0.5);
            self.mix_k = (self.mix_k * (err * dtl * 1.5).exp()).clamp(0.3, 3.0);
        } else if st.mix_target <= 0.0 {
            self.mix_k += (1.0 - self.mix_k) * (dtl * 2.0).min(1.0);
        }
        let thr = self.throttle_pos;
        let thr_of = [
            (thr / self.mix_k.max(1.0)).clamp(0.02, 0.999),
            (thr * self.mix_k.min(1.0)).clamp(0.02, 0.999),
        ];
        self.thr_valves = thr_of;
        // tank-head start: the main injectors are held back until the pumps are up to speed
        let dead = if self.deadhead_t > 0.0 {
            self.deadhead_t -= dtl;
            let x = self.shafts.iter().filter(|s| !s.pumps.is_empty()).map(|s| s.x()).fold(f32::MAX, f32::min);
            if x >= 0.7 || x == f32::MAX {
                self.deadhead_t = 0.0;
            }
            if x == f32::MAX { 1.0 } else { ((x - 0.2) / 0.5).clamp(0.12, 1.0) }
        } else {
            1.0
        };
        // liquid flow to the injectors: feed pressure minus injector/valve/line losses, with line inertia
        let mut mdot_f = 0.0;
        let mut mdot_o = 0.0;
        let (mut inj_f, mut inj_o) = (0.0f32, 0.0f32);
        let mut pc_sum = 0.0;
        let mut pc_n = 0.0;
        let mut he_vflow = 0.0f32;
        let mut hammer: Vec<(usize, f32)> = vec![];
        let mut inj_flows: Vec<(usize, usize, f32, f32)> = vec![]; // (inj, gas cell, mdot, temp)
        let mut leaks: Vec<(usize, usize, f32, f32, f32, bool, f32)> = vec![]; // (cell, gas cell, mdot, temp, jet speed, combustible, exit pressure)
        for (ni, n) in self.nets.iter_mut().enumerate() {
            let rho = if n.prop == 0 { n.fp.rho_f } else { n.fp.rho_o };
            // (a tri-propellant engine's second fuel has its own injector size)
            let second_fuel = n.prop == 0 && n.fkind as usize != pair_kinds(st.prop).0;
            let a_cell = if second_fuel { st.inj_area_f2 } else if n.prop == 0 { st.inj_area_f } else { st.inj_area_o } * 1e-4 * dead;
            if rb.valid && !n.injectors.is_empty() {
                let avg: f32 = n.injectors.iter().map(|&(_, gc)| gas_p(rb.state[gc], pr.gamma)).sum::<f32>() / n.injectors.len() as f32;
                n.pc_f += (avg - n.pc_f) * (1.0 - (-dtl / 0.08).exp());
            }
            let pc = n.pc_f.max(if rb.valid { 0.0 } else { st.amb_p });
            // an injector only flows once the liquid has run down the lines to it
            let weights: Vec<f32> = n.injectors.iter().map(|&(i, _)| 0.7 * a_cell * st.inj_w(i % W) * (n.front - self.phase[i] + 1.0).clamp(0.0, 1.0)).collect();
            let a_inj: f32 = weights.iter().sum();
            let a_valve = n.valves.iter().map(|&v| self.valves[v].pos).fold(f32::INFINITY, f32::min);
            let a_eff = if a_inj <= 0.0 {
                0.0
            } else if a_valve.is_finite() {
                // a valve drawn at a bigger scale (cell size, slab depth) is a bigger valve
                let av = a_valve * a_valve * VALVE_AREA * (st.dx / DX0).powi(2) * (st.depth / 0.36);
                if av <= 0.0 { 0.0 } else { 1.0 / (1.0 / (a_inj * a_inj) + 1.0 / (av * av)).sqrt() }
            } else {
                a_inj
            };
            // a throttle valve is a metering orifice sized against the injectors so that, at the same feed
            // pressure, the flow follows the throttle (half throttle: half the flow); in series with the rest
            let a_eff = if n.throttle && a_eff > 0.0 {
                let thr = thr_of[(n.prop == 1) as usize];
                let a_t = a_inj * thr / (1.0 - thr * thr).sqrt();
                1.0 / (1.0 / (a_eff * a_eff) + 1.0 / (a_t * a_t)).sqrt()
            } else {
                a_eff
            };
            // the pump curve goes into the step too (head = h0 - kq (m + gas generator draw)^2), so a steep pump
            // and a stiff feed line settle together instead of chasing each other frame to frame
            let m_old = n.m_inj;
            let g0 = n.gg_draw;
            let dp = if n.pumps_ok() { n.p_tank + n.h0 - n.kq * (2.0 * m_old * g0 + g0 * g0) } else { 0.0 } - pc - n.turb_dp;
            let m_new = if a_eff <= 0.0 {
                0.0
            } else {
                // implicit step of  I dm/dt = dp - c m^2  (unconditionally stable)
                // line friction: a bigger engine (cell size, slab depth) has bigger pipes
                let line_k = 20.0 / ((st.dx / DX0) * (st.depth / 0.36)).powi(2);
                let cc = 1.0 / (2.0 * rho * a_eff * a_eff) + line_k + n.kq;
                let a = cc * dtl / n.inertance;
                let b = m_old + dtl * dp / n.inertance;
                if b <= 0.0 { 0.0 } else { (-1.0 + (1.0 + 4.0 * a * b).sqrt()) / (2.0 * a) }
            };
            // water hammer: only a closing valve stops the column against the pump; a rising chamber
            // pressure just slows the flow without lifting the line above the feed pressure
            let a_now = if a_valve.is_finite() { a_valve } else { 1.0 };
            // (a valve being opened can look like it's closing when it joins two networks: it has to be
            // commanded shut)
            let closing = a_now < n.valve_prev - 1e-4 && n.valves.iter().any(|&v| !self.valves[v].cmd);
            n.valve_prev = a_now;
            let surge = if closing { n.inertance * ((m_old - m_new) / dtl.max(1e-4)).max(0.0) } else { 0.0 };
            if n.pumps_ok() && n.h0 > 0.0 {
                let mt = m_new + g0;
                n.head = (n.h0 - n.kq * mt * mt).max(-0.5 * n.h0);
                n.p_up = n.p_tank + n.head;
            }
            n.p_line = n.p_up + surge;
            if n.p_line > LINE_BURST && st.failures {
                hammer.push((ni, n.p_line));
            }
            g.p_line_max = g.p_line_max.max(n.p_line);
            n.m_inj = m_new;
            for (k, &(inj, gc)) in n.injectors.iter().enumerate() {
                if n.prop <= 1 {
                    pc_sum += pc; // (a helium start injector in the preburner passage isn't the chamber)
                    pc_n += 1.0;
                }
                inj_flows.push((inj, gc, m_new * weights[k] / a_inj.max(1e-9), n.temp));
            }
            n.mdot = m_new + n.gg_draw;
            // breached containers spill through every open face: orifice flow from the feed pressure
            n.leak = 0.0;
            let p_src = if n.pumps_ok() { n.p_up.max(n.p_tank) } else { 0.0 };
            if p_src > 0.0 {
                for &(cell, gc) in &n.breaches {
                    if self.phase[cell] > n.front {
                        continue; // the line is still dry here
                    }
                    let pg = if rb.valid { gas_p(rb.state[gc], pr.gamma) } else { st.amb_p };
                    let dp = (p_src - pg).max(0.0);
                    let m = 0.6 * st.dx * st.depth_at(gc % W) * (2.0 * rho * dp).sqrt();
                    if m > 0.0 {
                        n.leak += m;
                        leaks.push((cell, gc, m, n.temp, (2.0 * dp / rho).sqrt().min(250.0), n.prop == 0, pg));
                    }
                }
            }
            n.mdot += n.leak;
            n.flow_off = (n.flow_off + (2.0 + 6.0 * n.mdot.sqrt()) * dt) % 10000.0;
            // the liquid front runs down empty lines from the tank at the speed the feed pressure drives
            // it (capped by the fill-speed setting so it can be watched); nothing past it gets liquid
            if n.tank_cells > 0 && n.mass_frac > 0.0 && n.front < n.max_phase + 1.0 {
                let open = n.valves.iter().map(|&v| self.valves[v].pos).fold(1.0f32, f32::min);
                let dp = (n.p_up.max(n.p_tank) - st.amb_p).max(0.0);
                let v = (0.6 * (2.0 * dp / rho).sqrt()).min(st.fill_speed) * open;
                n.front = (n.front + v / st.dx * dtl).min(n.max_phase + 1.0);
            }
            he_vflow += n.mdot / rho;
            if n.tank_cells > 0 {
                n.mass_frac = (n.mass_frac - n.mdot * dtl / (n.tank_cells as f32 * 100.0)).max(0.0);
                if st.gse_tanks {
                    n.mass_frac = 1.0;
                }
            }
            if n.prop == 0 {
                mdot_f += n.mdot;
                inj_f += m_new;
            } else {
                mdot_o += n.mdot;
                inj_o += m_new;
            }
        }
        for (ni, p) in hammer {
            if let Some(&cell) = self.nets[ni].lines.first() {
                self.set_cell(cell, c::EMPTY);
                self.event(format!("water hammer: {:.0} bar surge burst a {} line", p / 1e5, if self.nets[ni].prop == 0 { "fuel" } else { "oxidiser" }));
            }
        }
        // a breached helium bottle vents (choked flow through each open face)
        let (gh, rh) = (1.66f32, 2077.0f32);
        let choke = (gh / (rh * 300.0)).sqrt() * (2.0 / (gh + 1.0)).powf((gh + 1.0) / (2.0 * (gh - 1.0)));
        // the hole is a sonic orifice: exit at the speed of sound of the cooled gas, at ~half bottle pressure
        let t_star = 300.0 * 2.0 / (gh + 1.0);
        let v_star = (gh * rh * t_star).sqrt();
        // helium lines: bottle -> pipes -> valves -> injectors (a valved cold-gas thruster); holes leak too
        {
            let mut out_of = vec![0.0f32; self.he_bottles.len()];
            let mut tea_flow: Option<usize> = None;
            for n in self.nets.iter_mut().filter(|n| n.prop == PROP_HE) {
                n.front = n.max_phase + 1.0; // gas fills its lines at once
                let bi = n.bottle as usize;
                let Some(b) = self.he_bottles.get(bi) else { continue };
                let open = n.valves.iter().map(|&v| self.valves[v].pos).fold(1.0f32, f32::min);
                if b.p <= st.amb_p || open <= 0.0 {
                    continue;
                }
                let p_star = b.p * (2.0 / (gh + 1.0)).powf(gh / (gh - 1.0));
                let tea_line = b.tea;
                for &(cell, gc) in n.injectors.iter().chain(n.breaches.iter()) {
                    if tea_line && n.injectors.contains(&(cell, gc)) {
                        tea_flow = Some(gc);
                    }
                    let m = open * 0.6 * st.dx * st.depth_at(gc % W) * b.p * choke;
                    out_of[bi] += m;
                    leaks.push((cell, gc, m, t_star, v_star, false, p_star));
                }
            }
            for (b, m) in self.he_bottles.iter_mut().zip(out_of) {
                b.p = (b.p - m * dtl * HE_RT / b.volume()).max(0.0);
            }
            if let Some(gc) = tea_flow {
                // pyrophoric: whatever it sprays into lights, in a green flash
                self.spark_t = self.spark_t.max(0.4);
                if self.teateb_flash <= 0.01 {
                    self.tea_age = 0.0; // a fresh injection, not the valve still being open
                }
                self.teateb_flash = 1.0;
                self.teateb_at = gc;
                self.gas_paint.push((gc, 2));
            }
        }
        if !self.he_breaches.is_empty() {
            let mut out = 0.0;
            let mut out_of = vec![0.0f32; self.he_bottles.len()];
            for &(cell, gc, bi) in &self.he_breaches {
                let p_b = self.he_bottles[bi].p;
                if p_b <= st.amb_p {
                    continue;
                }
                let p_star = p_b * (2.0 / (gh + 1.0)).powf(gh / (gh - 1.0));
                let m = 0.6 * st.dx * st.depth_at(gc % W) * p_b * choke;
                out += m;
                out_of[bi] += m;
                leaks.push((cell, gc, m, t_star, v_star, false, p_star));
            }
            for (b, m) in self.he_bottles.iter_mut().zip(out_of) {
                b.p = (b.p - m * dtl * HE_RT / b.volume()).max(0.0);
            }
            let _ = out;
        }
        // warn when a new breach appears (flow through a hole can pause while local pressure builds)
        let leaking = !self.he_breaches.is_empty() || self.nets.iter().any(|n| !n.breaches.is_empty() && n.tank_cells > 0);
        if leaking && !self.leaking && !leaks.is_empty() {
            let (cell, _, _, _, _, _, _) = leaks[0];
            self.event(format!("leak! {} breached at ({}, {})", c::name(self.cells[cell]).split(" (").next().unwrap_or("container"), cell % W, cell / W));
        }
        if !leaks.is_empty() || !leaking {
            self.leaking = leaking;
        }
        g.leak = leaks.iter().map(|l| l.2).sum();
        // every hole is a crude thruster: momentum of the jet plus the exit pressure over the hole area,
        // pushing opposite to the jet (screen y points down, thrust is reported upward)
        let (mut fx, mut fy) = (0.0f32, 0.0f32);
        for &(cell, gc, m, _, v, _, p_exit) in &leaks {
            let (dx, dy) = ((gc % W) as f32 - (cell % W) as f32, (gc / W) as f32 - (cell / W) as f32);
            let f = m * v + (p_exit - st.amb_p).max(0.0) * st.dx * st.depth_at(gc % W);
            fx -= f * dx;
            fy += f * dy;
        }
        g.leak_thrust = fy;
        g.leak_side = fx;
        // pressurant: each regulated tank draws helium from the bottle piped to it, filling the space
        // its draining liquid leaves behind at tank pressure
        let _ = he_vflow;
        for n in &self.nets {
            let rho = if n.prop == 0 { n.fp.rho_f } else { n.fp.rho_o };
            if let Some(b) = self.he_bottles.get_mut(n.reg_bottle as usize) {
                if n.reg_bottle != NO_NET && b.p > n.p_tank {
                    let dm = n.p_tank * (n.mdot / rho) / HE_RT * dtl;
                    b.p = (b.p - dm * HE_RT / b.volume()).max(0.0);
                }
            }
        }
        if st.gse_he {
            for b in &mut self.he_bottles {
                b.p = b.p0;
            }
        }
        if st.gse_batt {
            self.recharge();
        }
        let full: f32 = self.he_bottles.iter().filter(|b| !b.tea).map(|b| b.p0 * b.volume() / HE_RT).sum();
        self.he_frac = if full > 0.0 { self.he_bottles.iter().filter(|b| !b.tea).map(|b| b.mass()).sum::<f32>() / full } else { 0.0 };
        g.t_cool = self.nets.iter().filter(|n| n.prop == 0 && !n.injectors.is_empty()).map(|n| n.temp).fold(0.0, f32::max);
        g.t_cool_max = pr.t_cool_max;
        if g.t_cool > pr.t_cool_max && self.events.last().map_or(true, |e| !e.contains("coolant")) {
            self.event(format!("coolant above {:.0} K: decomposition/coking is fouling the regen jacket", pr.t_cool_max));
        }
        g.he_frac = self.he_frac;

        // ignition: cryogenic propellants need the igniter, hypergolics light on contact
        let of = if inj_f > 1e-3 { inj_o / inj_f } else { 0.0 };
        let both = inj_f > 1e-3 && inj_o > 1e-3;
        if pr.hypergolic {
            self.ignited = both;
        } else {
            if self.spark_t > 0.0 {
                self.spark_t -= dtl;
            }
            // fire at the injectors (painted, say) lights it as well as the spark or igniter does
            let fire = flame_at(self.nets.iter().flat_map(|n| n.injectors.iter().map(|x| x.1)), rb, pr);
            if both && (self.spark_t > 0.0 || self.igniter || fire) {
                self.ignited = true;
            }
            if both {
                self.lack_t = 0.0;
            } else {
                self.lack_t += dtl;
                if self.lack_t > 0.3 {
                    self.ignited = false;
                }
            }
        }
        g.ignited = self.ignited;
        g.no_flame = if self.ignited {
            ""
        } else if self.mixed.is_some() {
            "fuel and oxidiser lines are joined, so neither flows: see the warning below"
        } else if inj_f <= 1e-3 && inj_o <= 1e-3 {
            "no propellant reaching the injectors: open the valves"
        } else if inj_f <= 1e-3 {
            "no fuel flow: open the fuel valve"
        } else if inj_o <= 1e-3 {
            "no oxidiser flow: open the oxidiser valve"
        } else if pr.hypergolic {
            ""
        } else {
            "propellants flowing cold: switch the igniter on"
        };
        let tfl = if both && self.ignited { t_flame(pr, of) } else { 0.0 };

        // aggregate sources per gas cell
        let cv = pr.rgas / (pr.gamma - 1.0);
        let mut map: std::collections::HashMap<usize, Src> = std::collections::HashMap::new();
        for (inj, gc, m, temp) in inj_flows {
            if m <= 0.0 {
                continue;
            }
            let tinj = if tfl > 0.0 { tfl } else { temp };
            let dm = m * st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
            let (ix, iy) = ((inj % W) as f32, (inj / W) as f32);
            let (gx, gy) = ((gc % W) as f32, (gc / W) as f32);
            let v = 30.0;
            let (vx, vy) = ((gx - ix) * v, (gy - iy) * v);
            let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
            s.drho += dm;
            s.dmx += dm * vx;
            s.dmy += dm * vy;
            s.de += dm * (cv * tinj + 0.5 * v * v);
            if tfl <= 0.0 && both {
                // fuel and oxidiser mixing without a flame: it piles up and can burn later (hard start)
                s.dphi += dm;
            }
        }
        // start purge: inert gas at room temperature blown in through every propellant injector
        if self.purge_t > 0.0 {
            // (gas per cell volume, so a small cell gets proportionally less)
            let m_cell = PURGE_KG_S * (st.dx / DX0).powi(2) * (st.depth / 0.36);
            for n in &self.nets {
                if n.prop > 1 {
                    continue;
                }
                for &(inj, gc) in &n.injectors {
                    let dm = m_cell * st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
                    let (vx, vy) = (((gc % W) as f32 - (inj % W) as f32) * 30.0, ((gc / W) as f32 - (inj / W) as f32) * 30.0);
                    let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
                    s.drho += dm;
                    s.dmx += dm * vx;
                    s.dmy += dm * vy;
                    s.de += dm * (cv * 300.0 + 450.0);
                }
            }
        }
        // gas generators: the head is an injector plate whose cells alternate oxidiser and fuel elements.
        // Lit, each oxidiser element sprays its oxidiser with just enough fuel to burn, so it ignites at the
        // plate as a hot jet, and each fuel element sprays the rest of the fuel cold; the jets mix down the
        // chamber and arrive at the generator's fuel-rich flame temperature. Unlit, it's all cold spray.
        let cp = pr.gamma * cv;
        let q_burn = cv * (pr.t_peak - 300.0); // what the gas solver releases per kg it burns
        for &(k, mf, mo, lit) in &gg_out {
            let faces = &self.gg_faces[k];
            if faces.is_empty() {
                continue;
            }
            if self.gg_lumped.get(k).copied().unwrap_or(false) {
                // a burner burns inside its block: it hands the gas on already burnt, at its flame temperature,
                // out of every open face (no igniting or mixing for the gas solver to get wrong)
                let m = mf + mo;
                if m <= 0.0 || !lit {
                    continue;
                }
                let t = t_flame(pr, mo / mf.max(1e-6));
                let m1 = m / faces.len() as f32;
                for &(head, gc) in faces {
                    let k_cell = st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
                    let dm = m1 * k_cell;
                    let v = 120.0;
                    let (vx, vy) = (((gc % W) as f32 - (head % W) as f32) * v, ((gc / W) as f32 - (head / W) as f32) * v);
                    let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
                    s.drho += dm;
                    s.dmx += dm * vx;
                    s.dmy += dm * vy;
                    s.de += dm * (cp * t + 0.5 * v * v);
                }
                continue;
            }
            let ox_el = |h: usize| (h % W / 2 + h / W / 2) % 2 == 0; // 2-cell elements, alternating
            let mut ox: Vec<(usize, usize)> = faces.iter().copied().filter(|f| ox_el(f.0)).collect();
            let mut fu: Vec<(usize, usize)> = faces.iter().copied().filter(|f| !ox_el(f.0)).collect();
            if ox.is_empty() || fu.is_empty() {
                ox = faces.clone();
                fu = faces.clone();
            }
            // (faces, kg/s, temperature, kg/s of it that burns in the chamber)
            let streams: Vec<(&Vec<(usize, usize)>, f32, f32, f32)> = if lit {
                let m = mf + mo;
                let ms = (mo * (1.0 + 1.0 / pr.of_opt)).min(m);
                let me = (m - ms).max(0.0);
                let (t_s, t_f) = (950.0, pr.t_in_f.max(20.0));
                let burn = ((m * t_flame(pr, mo / mf) - ms * t_s - me * t_f) * cp / q_burn).clamp(0.0, ms);
                vec![(&ox, ms, t_s, burn), (&fu, me, t_f, 0.0)]
            } else {
                vec![(&ox, mo, pr.t_in_o.max(20.0), 0.0), (&fu, mf, pr.t_in_f.max(20.0), 0.0)]
            };
            for (fs, m, t, burn) in streams {
                if m <= 0.0 {
                    continue;
                }
                let (m1, b1) = (m / fs.len() as f32, burn / fs.len() as f32);
                for &(head, gc) in fs {
                    let k_cell = st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
                    let (dm, db) = (m1 * k_cell, b1 * k_cell);
                    let v = 120.0; // injector spray speed: the jets carry into the chamber before they mix
                    let (vx, vy) = (((gc % W) as f32 - (head % W) as f32) * v, ((gc / W) as f32 - (head / W) as f32) * v);
                    let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
                    s.drho += dm;
                    s.dmx += dm * vx;
                    s.dmy += dm * vy;
                    // flowing in through the plate it carries its enthalpy (cp T)
                    s.de += dm * (cp * t + 0.5 * v * v);
                    s.dphi += db;
                }
            }
        }
        // turbines with drive gas ducted to them: the gas goes through, giving the shaft its power, and
        // leaves with that much less energy
        if rb.valid {
            // the drive power follows the gas through the turbine smoothed over ~0.15 s (the field it comes
            // from is a frame old; two turbines in series otherwise chase each other)
            let blend = (dtl / 0.15).min(1.0);
            for s in &mut self.shafts {
                // ducted from a gas generator: one-way, inlet to outlet (if the pressure ever reverses the wheel
                // stalls instead). Any other turbine: whatever gas goes through it, from its high-pressure
                // side to its low one (fire lit in a box against a turbine spins it)
                let t = if s.gas_driven {
                    let outlet: Vec<usize> = s.turb_faces.iter().copied().filter(|j| !s.turb_in.contains(j)).collect();
                    flow_between(s.turb_in.clone(), outlet, rb, &st, pr.gamma, 1.0)
                } else {
                    // any other gas pushing through the wheel: only a real pressure difference, and at most
                    // half the gas in the cells it draws from per frame
                    through_flow(&s.turb_faces, rb, &st, pr.gamma, 0.5).filter(|t| t.p_h - t.p_l > 3.0e4)
                };
                let Some(t) = t else {
                    s.p_gas *= 1.0 - blend;
                    continue;
                };
                let k = (pr.gamma - 1.0) / pr.gamma;
                let dh_s = t.h_h * (1.0 - (t.p_l / t.p_h).max(1e-3).powf(k));
                s.p_gas += (t.m * dh_s - s.p_gas) * blend;
                s.c0_gas += ((2.0 * dh_s).sqrt() - s.c0_gas) * blend;
                // the gas leaves with the work the wheel took out of it: on a generator's duct all of the shaft's
                // turbine power, otherwise its efficiency's share of this gas's expansion (an expander or the
                // start cartridge may be driving the shaft too)
                let work = if s.gas_driven { s.p_turb / t.m.max(1e-6) } else { s.eff_t * dh_s };
                let h_out = (t.h_h - work).max(0.3 * t.h_h);
                apply_through(&mut map, &t, h_out, 0.0, rb, &st);
            }
        }
        // hot-gas injectors: gas goes through from the high-pressure side to the low-pressure side (orifice
        // flow), taking its enthalpy along; fuel-rich generator gas brings its unburnt fuel to burn there
        if rb.valid {
            for (faces, down) in self.hgi_faces.iter().zip(&self.hgi_down) {
                // one-way, from the duct side into the big space; with no clear outlet side, by pressure
                let t = if down.is_empty() || down.len() == faces.len() {
                    through_flow(faces, rb, &st, pr.gamma, 1.0)
                } else {
                    let up: Vec<usize> = faces.iter().copied().filter(|j| !down.contains(j)).collect();
                    flow_between(up, down.clone(), rb, &st, pr.gamma, 1.0)
                };
                if let Some(mut t) = t {
                    // into a chamber with injectors, the hot gas goes through the whole injector plate (as a
                    // real hot-gas manifold feeds every element), straight in like the propellants, so it
                    // doesn't jet in from one side and push the flame over
                    let plate: Vec<(usize, usize)> = if down.is_empty() { vec![] } else { self.nets.iter().flat_map(|n| n.injectors.iter().copied()).collect() };
                    if plate.is_empty() {
                        apply_through(&mut map, &t, t.h_h, self.gg_comb, rb, &st);
                        continue;
                    }
                    let lo = std::mem::take(&mut t.lo);
                    let _ = lo;
                    apply_through(&mut map, &t, t.h_h, 0.0, rb, &st); // takes the gas out of the duct
                    let m1 = t.m / plate.len() as f32;
                    for &(inj, gc) in &plate {
                        let dm = m1 * st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
                        let (dx, dy) = ((gc % W) as f32 - (inj % W) as f32, (gc / W) as f32 - (inj / W) as f32);
                        let v = t.v.min(150.0);
                        let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
                        s.drho += dm;
                        s.dmx += dm * v * dx;
                        s.dmy += dm * v * dy;
                        s.de += dm * (t.h_h + 0.5 * v * v);
                        s.dphi += dm * self.gg_comb;
                    }
                }
            }
        }
        // gas lines: gas goes along them from the higher-pressure end to the lower
        if rb.valid {
            for faces in &self.gas_pipes {
                if let Some(t) = through_flow(faces, rb, &st, pr.gamma, 0.5) {
                    apply_through(&mut map, &t, t.h_h, 0.0, rb, &st);
                }
            }
        }
        // source blocks push gas, fuel vapour or fire out at the set pressure into whatever is beside them
        for &(cell, gc, kind) in &self.sources {
            let t = if kind == c::SRC_FIRE { 2600.0 } else { 300.0 };
            let rho_g = st.src_p / (pr.rgas * t);
            let pg = if rb.valid { gas_p(rb.state[gc], pr.gamma) } else { st.amb_p };
            let dp = (st.src_p - pg).max(0.0);
            if dp <= 0.0 {
                continue;
            }
            let v = (2.0 * dp / rho_g).sqrt().min(800.0);
            let m = 0.6 * st.dx * st.depth_at(gc % W) * (2.0 * rho_g * dp).sqrt();
            let dm = m * st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
            let (ix, iy) = ((cell % W) as f32, (cell / W) as f32);
            let (gx, gy) = ((gc % W) as f32, (gc / W) as f32);
            let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
            s.drho += dm;
            s.dmx += dm * (gx - ix) * v;
            s.dmy += dm * (gy - iy) * v;
            s.de += dm * (cv * t + 0.5 * v * v);
            if kind == c::SRC_FUEL {
                s.dphi += dm;
            }
        }
        // spilled propellant flashes to cold vapour (fuel vapour can burn); helium vents as inert gas
        for (cell, gc, m, temp, v, fuel, _) in leaks {
            let dm = m * st.dt() / (st.dx * st.dx * st.depth_at(gc % W));
            let (ix, iy) = ((cell % W) as f32, (cell / W) as f32);
            let (gx, gy) = ((gc % W) as f32, (gc / W) as f32);
            let (vx, vy) = ((gx - ix) * v, (gy - iy) * v);
            let s = map.entry(gc).or_insert(Src { cell: gc as u32, ..Default::default() });
            s.drho += dm;
            s.dmx += dm * vx;
            s.dmy += dm * vy;
            s.de += dm * (cv * temp.max(60.0) + 0.5 * v * v);
            if fuel {
                s.dphi += dm;
            }
        }
        self.srcs = map.into_values().collect();

        g.chamber_p = if pc_n > 0.0 { pc_sum / pc_n } else { 0.0 };
        g.mdot_f = mdot_f;
        g.mdot_o = mdot_o;
        g.of = of;
        g.t_flame = tfl;
        // the mixture changes how hard the gas heats the walls (see wall_heat_scale)
        let heat_target = if self.ignited && of > 0.05 { wall_heat_scale(of, pr.of_opt) } else { 1.0 };
        self.wall_h_scale += (heat_target - self.wall_h_scale) * (1.0 - (-dtl / 0.5).exp());
        g.wall_scale = self.wall_h_scale;
        let (mut tf, mut tc, mut nf, mut no) = (0.0, 0.0, 0.0, 0.0);
        for n in &self.nets {
            if n.tank_cells > 0 {
                if n.prop == 0 {
                    tf += n.mass_frac;
                    nf += 1.0;
                } else {
                    tc += n.mass_frac;
                    no += 1.0;
                }
            }
        }
        g.tank_f = if nf > 0.0 { tf / nf } else { 0.0 };
        g.tank_o = if no > 0.0 { tc / no } else { 0.0 };

        // thrust and structural failure from the read-back field
        if rb.valid {
            let mut thrust = 0.0;
            let mut maxt = 0.0f32;
            let (mut hot_frac, mut hot_t, mut hot_mat) = (0.0f32, 0.0f32, 0u8);
            let mut broken: Vec<(usize, &'static str)> = vec![];
            for y in 1..H - 1 {
                for x in 1..W - 1 {
                    let i = Self::idx(x, y);
                    let t = self.cells[i];
                    // a wall just drawn into gas: the read-back is a frame or two behind and still holds
                    // gas there (density > 0, energy instead of a wall temperature), so skip it until then
                    if !c::is_solid(t) || rb.state[i][0] != 0.0 {
                        continue;
                    }
                    let face = st.dx * st.depth_at(x);
                    let tw = rb.state[i][3];
                    if !c::is_line(t) {
                        maxt = maxt.max(tw);
                        let fr = tw / c::t_melt(t);
                        if fr > hot_frac {
                            (hot_frac, hot_t, hot_mat) = (fr, tw, t);
                        }
                    }
                    let mut worst = 0.0f32;
                    let mut dir = (0i32, 0i32);
                    for (dx, dy) in [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)] {
                        let j = Self::idx((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                        // only gas open to the outside counts towards thrust: a sealed vessel's own pressure
                        // pushes on all its walls and cancels (its sloshing would only shake the reading)
                        if self.cells[j] == c::EMPTY && self.open_gas[j] {
                            let p = gas_p(rb.state[j], pr.gamma);
                            let gauge = p - st.amb_p;
                            // gas below the wall pushes it up (thrust), gas above pushes it down
                            if dy == 1 {
                                thrust += gauge * face;
                            } else if dy == -1 {
                                thrust -= gauge * face;
                            }
                            if p > worst {
                                worst = p;
                                dir = (-dx, -dy);
                            }
                        }
                    }
                    if !st.failures {
                        continue;
                    }
                    if (!c::is_line(t) || !self.wet_cell(i)) && tw > c::t_melt(t) {
                        broken.push((i, "melted"));
                    } else if worst > 3.0e5 {
                        let mut th = 0;
                        let (mut cx, mut cy) = (x as i32, y as i32);
                        while th < 8 {
                            if cx < 0 || cy < 0 || cx >= W as i32 || cy >= H as i32 || self.cells[Self::idx(cx as usize, cy as usize)] == c::EMPTY {
                                break;
                            }
                            th += 1;
                            cx += dir.0;
                            cy += dir.1;
                        }
                        let tm = c::t_melt(t);
                        let tt = if c::is_line(t) { 300.0 } else { tw };
                        let weak = (1.0 - ((tt - 300.0) / (tm - 300.0)).clamp(0.0, 1.0).powi(2)).max(0.02);
                        let stress = worst * 0.3 / (th.max(1) as f32 * st.dx);
                        if stress > c::sigma0(t) * weak {
                            broken.push((i, "burst"));
                        }
                    }
                }
            }
            // the flow is unsteady from frame to frame; the gauge reads it averaged over ~0.5 s
            self.thrust_f += (thrust + g.leak_thrust - self.thrust_f) * (dtl / 0.5).min(1.0);
            g.thrust = self.thrust_f;
            g.max_wall_t = maxt;
            (g.hot_t, g.hot_mat, g.hot_melt) = (hot_t, hot_mat, c::t_melt(hot_mat));
            if self.wall_h_scale > 1.5 && self.events.last().map_or(true, |e| !e.contains("oxygen-rich")) {
                self.event("oxygen-rich: no cooling film, the walls are taking far more heat".to_string());
            }
            let m = mdot_f + mdot_o;
            g.isp = if m > 1.0 { g.thrust / (m * 9.80665) } else { 0.0 };
            if !broken.is_empty() {
                let (i, why) = broken[0];
                let msg = format!("{} cell(s) failed ({}) near x={}, y={}", broken.len(), why, i % W, i / W);
                for (i, _) in broken {
                    self.set_cell(i, c::EMPTY);
                }
                self.event(msg);
            }
        }
        // which stretches of line hold liquid changes as it fills them or a tank runs out: the shader has to know
        // (a few cells at a time, so it isn't re-sent every frame)
        let sig = self.nets.iter().fold(0u64, |a, n| a.wrapping_mul(1_000_003).wrapping_add(((n.front * 0.25) as u64) * 2 + (n.mass_frac > 0.0) as u64));
        if sig != self.wet_sig {
            self.wet_sig = sig;
            self.mat_stale = true;
        }
        self.g = g;
    }
}
