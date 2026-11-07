# Hotfire

*Build it. Light it. Land it.* A rocket-engine sandbox: build a liquid-fuel engine cell by cell (Powder Toy style), plumb it, light it on a start cartridge, helium or TEA-TEB, and fly it.

(The cargo package, and so the exe, is still called `rocket_sandbox`.)

```
cargo run --release                                  # the game
cargo run --release -- --selftest 20 shots/run       # headless: run 20 s, print gauges, save PNGs
cargo run --release -- --selftest 20 --prop 3 --pf --pc 25   # hypergolic, helium-fed, 25 bar
cargo test                                           # plumbing, turbopump, valve, nozzle, save/load checks
```
Other self-test flags: `--nbskirt [--nbexp R --nblen N]` (niobium skirt on the exit), `--vac`, `--hethruster`, `--coldgas --brushhole --patchlate` (patch a punctured bottle), `--prop N` (0 LH2, 1 CH4, 2 RP-1, 3 MMH/N2O4), `--axi`, `--pf`, `--pc BAR`, `--gg`, `--nojacket`.

Simulation space: 640 x 768 cells (6.4 m x 7.7 m at 1 cm cells); presets are centred near the top so the
plume has room. About 3500 gas steps per second on an RTX 3050 laptop GPU.

## Portable build

Hotfire is one file: the shaders are compiled into the exe, so there is nothing to install next to it.

```
cargo build --release
```

The result is `target/release/rocket_sandbox.exe` (about 16 MB). Copy it anywhere, to a USB stick or another PC, and
run it. To make a ready-to-share folder:

```
mkdir hotfire
copy target\release\rocket_sandbox.exe hotfire\Hotfire.exe
elease
ocket_sandbox.exe hotfire\Hotfire.exe
elease
ocket_sandbox.exe hotfire\Hotfire.exe
```

- **What the target PC needs:** Windows 10 or 11 and a GPU with a Vulkan or DirectX 12 driver (any recent one; the
  gas solver runs on it, so a very old or software-only adapter will be slow). The Visual C++ runtime is already on
  almost every Windows PC; to not depend on it at all, build with `set RUSTFLAGS=-C target-feature=+crt-static`
  first.
- **Where it saves:** designs go to a `designs` folder and stamps to a `stamps` folder, both made in the folder the
  game is started from. For a truly portable copy, start it from its own folder (or a shortcut with *Start in* set to
  that folder) and your designs travel with it. Copy the `designs` folder across to move your builds.
- **Faster start-up on another machine:** the first launch compiles the shaders for that GPU, which takes a second
  or two; there is nothing to configure.
- **Linux or macOS:** the same `cargo build --release` works (it uses wgpu), and gives `target/release/rocket_sandbox`.
  Not tested here; only Windows has been run.

## Controls
- **Brush**: left paints, right-click erases (walls, pipes and any painted gas, fuel or fire; on the Back/Front
  layer only that layer's pipes). The white box shows exactly which cells the brush covers.
  Mouse wheel changes brush size; Ctrl+wheel, Alt+wheel or `+` / `-` zoom; middle drag or Space+drag pans.
- **Test stand** (button *Test stand...* under the prefab buttons, or **T**): records every run and shows it as
  charts and numbers, so engines and tunings can be compared instead of eyeballed.
  - **Runs:** a run starts when you press Start (or light the engine by hand) and is recorded ten times a
    second of simulated time (so a slow or paused game doesn't spoil it) until the engine has been quiet for
    8 s. Starting again begins a new run.
  - **Numbers:** steady chamber pressure, thrust, mixture and specific impulse (the last five seconds before
    the pressure falls away, so a start overshoot and the shutdown ramp aren't counted), time to 90% pressure,
    peak pressure and thrust, hottest wall, burn time, total impulse, mean Isp over the run (from the impulse
    and the propellant used), fuel and oxidiser used, and the battery left on an electric engine.
  - **Charts:** pick any of 16 channels (chamber pressure, thrust, Isp, mixture, flows, hottest wall, feed
    pressure, shaft speed, throttle, tank levels, battery, generator gas, coolant temperature, wall-heating
    scale). They share one time axis; point at a chart for the values at that time on every chart, or click to pin
    the cursor. Show the whole run, the last 30 s or the last 10 s.
  - **Comparing:** *Keep this run as the reference* draws it grey behind the next run, and the numbers show the
    change from it (hover for the reference value). *Copy run as CSV* puts every channel on the clipboard for a
    spreadsheet. Headless: `--stand` prints the numbers at the end of a `--selftest` run.
- **Gas burner** (palette: *Turbopumps and gas generators*, and *Gas burner* / *Small gas burner* in the prefab
  menu): a machine block like a turbine or pump, not cells of gas, so it doesn't depend on the gas simulation
  lighting or mixing anything.
  - **Ports:** touch it with a fuel line and an oxidiser line; every open (empty) cell touching the block is its
    outlet, so wall it in except where the hot gas should leave (the prefab has a casing and a tapered neck, ready
    to butt against a turbine or duct).
  - **How it works:** it draws fuel and oxidiser by the same orifice sizes as a gas generator (the *gas-gen*
    sliders), lights itself the moment both flow, and hands the gas on already burnt, at the flame temperature of
    its mixture (fuel-rich ones run around 800-1000 K, which a turbine can take), out of its open faces. A Merlin
    or J-2 with the generator swapped for a burner block runs the same as before.
  - **It's drawn as a cutaway, not a black box:** manifold galleries (fuel and oxidiser, on the side each line
    really comes in, in the colours of the two liquids), an injector plate with alternating fuel and oxidiser
    orifices, dashed jets bending in towards the middle, a flame that fills the chamber, a converging nozzle with a
    blazing throat, and a casing that glows. The flame is the colour of the propellant (hydrogen violet-white,
    methane blue, kerosene orange, as the engine exhaust is) and gets whiter as it gets hotter. **The hot gas
    visibly leaves the block as a jet** straight out of its outlet face, filling the funnel or duct it opens into
    and fading with distance, in the same colours. It goes dark when nothing flows, and it is drawn the right way
    round whether or not the engine is running.
  - **Every engine with a gas generator or preburner uses a burner block** (J-2, Merlin, F-1, the hydrolox
    GG-into-nozzle engine and the staged-combustion engine): the injector head and its chamber are one block, the
    funnel or duct below it is its outlet, so you can watch the flame in them. They run as before (J-2 about 52
    bar, Merlin 98 bar, F-1 61 bar, staged about 114 bar).
  - Hover it to see the fuel and oxidiser flow, the mixture and the gas temperature. Headless test: `--burner`
    turns every generator plate in an engine into a burner block, and `--burnerdemo` puts one between two tanks.
- **Tanks and valves say what they carry.** Each tank is labelled on the field with its fluid (LH2, CH4, RP-1,
  MMH, LOX, N2O4) once it is big enough on screen to read, and each valve gets a small label (when zoomed in).
  The hover readout says "Fuel tank: LH2 (liquid hydrogen)" and, for pipes, valves, pumps and the like,
  "carries LH2 (liquid hydrogen)" (or "not connected to a tank"). The valve list in the panel names them too,
  e.g. "Fuel valve (CH4) at (x, y)". The fluids come from what each tank holds (Fill tool).
- **Stylised exhaust plume** (left panel, *View*, on by default): the main nozzle's exhaust is drawn by a plume shader
  from the gas at the nozzle exit (speed, Mach number, pressure against the air's), the flame temperature, the
  ambient pressure and the wind: it bursts wide in thin air, pinches in low down, has shock diamonds and a Mach disk,
  streams fast, and is squashed by air rushing up into the nozzle. The gas simulation underneath is unchanged.
- **Starters**: three ways to spin the pumps up. A *Start cartridge* part (Turbopumps and gas generators) touching a
  shaft, or a wall's thickness from it, gives that shaft 250 kW per cell for the start (builds with none use the
  *start cartridge W* setting). *Helium spin start* (Simulation) blows helium from the bottle through the turbines
  instead, and empties it as it goes. *Tank-head (dead-head) start* has no starter at all, like the RS-25: the
  preburner or gas generator lights on tank pressure and spins the pumps up while the main injectors are held back,
  then the main chamber opens as they come up to speed. It works for the Merlin; the J-2 and the staged engine
  still idle at a few bar, because their burners push into a main chamber that has already pressurised (the real
  RS-25 runs as an expander for the first second or so, then lights its preburners on a tight valve schedule). The
  J-2 preset starts the way the real one did, on a gas start tank (the helium spin start), and so does the staged
  engine if you switch it on. Performance: the gas solver is GPU bound, about 8,400 steps a second; the default is
  100 steps a frame so 60 frames a second holds, and the state read-back runs every second frame.
- **Helium start line**: the J-2 and the staged engine have a real one: a pipe from the helium bottle, on the back pipe
  layer so it passes behind the rest of the engine, a pipe joint at each end, and a closed valve and an injector in
  the steel beside the turbine's inlet passage. The start sequence opens the valve (events say so), cold helium blows
  through the turbine and the bottle drains (you can watch the helium bar fall), and the valve shuts again once the
  pumps are up. Any build with a valve between a helium pipe and an injector that opens into a turbine passage works
  the same way, and then the lumped *Helium spin start* setting and start cartridge are not used.
- **TEA-TEB** (*TEA-TEB bottle*, in the Turbopumps and gas generators group): a green bottle holding the pyrophoric
  igniter (triethylaluminium / triethylborane), piped like the helium start: a pipe (the back layer is the way past
  the rest of the engine), a closed valve, and an injector into the drive-gas passage. The start sequence opens
  the valve for 1.6 s; what it sprays lights on contact in a green flash, and that is the engine's igniter (it
  has no other spark), so an empty bottle lights nothing, and the igniter button still does. The bottle's bar
  in the engine panel shows what is left; it drains with every start, and *Reset gas/heat* refills it. The
  Merlin carries one.
- **Flight** (left panel, *Flight*): the vehicle flies on the engine's thrust against its weight and the drag of the
  air (10 m2 of drag area, a mass you set that burns off with the propellant used), so the altitude follows. *Launch*
  starts on the pad; *Land a Falcon 9* drops the stage tail-first from 6 km at 250 m/s with the engine off: start it
  at the right moment, throttle, and touch down slower than 3 m/s (8 m/s is a hard landing, more a crash). The air
  rushes past the engine as a wind in the gas solver (down the screen going up, up into the nozzle coming down), and
  shows as streaks in the plain and flame views. Climbing (above) makes the wind too.
- **Altitude** (left panel, *Altitude*): the air pressure round the engine from the 1976 standard atmosphere, by
  slider or quick buttons (Sea level, 5 km, 10 km, 30 km, Vacuum). **Climb** raises the altitude by itself at the
  chosen rate, so you can watch an engine come up out of the air. The nozzle physics is the gas solver's own, so
  thrust and specific impulse rise as the air thins (the Merlin: 767 kN and 303 s at sea level, 803 kN and 317 s at
  10 km, 816 kN and 323 s in vacuum; the J-2: 425 kN to 505 kN; the RL10: 70 kN to 86 kN), a bell sized for vacuum over-expands near the ground, and thrust is the real
  pressure force on the walls, ambient air included. While the altitude changes the quiet air in the whole world
  relaxes to the new density (hot or fast gas is left alone), not only the air at the edges. The gas solver can't hold
  less than 100 Pa, so everything above about 48 km is vacuum. The test stand records *Altitude* and *Ambient
  pressure* channels, so thrust against altitude is a chart. The older ambient-pressure slider still works; touching it
  hands control back from the altitude. Headless: `--alt KM` and `--climb KM_PER_S`.
- **Undo and redo:** **Ctrl+Z** takes back the last edit, **Ctrl+Y** or **Ctrl+Shift+Z** puts it back (also the
  Undo and Redo buttons under the brush). Every finished edit is one step: a brush stroke (until you let go), a
  line, a box, a flood fill, a paste or prefab, a preset or design loaded, Clear all, a tank given a new fluid,
  a regulator set. 120 steps are kept (and at most about 160 MB); a new edit after an undo drops the redo steps.
  What comes back is the drawing (all three pipe layers, tank fluids, regulator settings, and the settings that
  belong to the drawing such as injector sizes and the cell scale); how the game is running (pause, speed,
  ambient pressure, failures) is left alone. A wall the engine melts or bursts is not an edit, and undo does not
  repair it.
- **Space** pauses and resumes (a quick tap; holding it and dragging still pans).
- **`;` toggles replace mode** (also a checkbox under the field help text; the status bar says REPLACE while it
  is on). Painting then only changes parts that are already there, swapping one material for another, and
  never fills empty space; it works with the brush, lines, rectangles and flood fill.
- **Hover readout, top-left of the field:** point at anything to see what it is. Gas shows pressure,
  temperature, speed, Mach number and density. A part shows its temperature and, for anything that can melt,
  its **melting point** and **how far it has got** ("68% of the way to melting, 430 K to go"; orange above 60%,
  red above 85%, "MELTING NOW" past it). Pipes say they are cooled by what flows through them. Batteries show
  their charge.
- **Middle-click** is the eyedropper, as in Powder Toy: it picks up whatever is under the cursor (a wall
  material, pipe, valve, tank, pump...) as the current tool. On the back or front layer it picks up that
  layer's pipe.
- **Shift + drag** draws a line from where you pressed, **Ctrl + drag** a filled rectangle (preview while held,
  drawn on release; right button erases instead).
- **Ctrl + Shift + click** flood-fills, as in Powder Toy: everything joined to the clicked cell that is the same
  as it becomes the current tool (a pipe, a wall material, a tank, or gas, fuel or fire filling open space).
  Right-click with Ctrl + Shift flood-erases. On the back or front layer it fills or clears a layer pipe run.
- **Copy, paste and stamps** (as in Powder Toy; also buttons under *Copy, paste and stamps*):
  - **Ctrl+C** / **Ctrl+X**, then drag a box: copies (or cuts) every part, back/front layer pipe and
    regulator setting inside it. Gas isn't copied. The copy also goes onto the system clipboard as text,
    so it pastes into another window or after a restart.
  - **Ctrl+V**: the piece follows the mouse; click to put it down (hold Shift to keep pasting copies).
    **R** rotates a quarter turn, **Shift+R** mirrors, **Ctrl+Shift+R** turns it upside down. Its empty
    cells leave what's underneath alone.
  - **S**, then drag a box: saves a stamp to `stamps/` (`.stamp` text files). **K** opens the stamp
    browser: click a picture to paste it, or Delete.
  - Right-click or **Esc** cancels.
- **Prefab parts** (button above the parts list, or **P**): ready-made pieces that go down like a paste.
  Pick one, click on the field to put it down; **R** turns it, **Shift+R** mirrors it.
  - **Combustion chambers** (small, medium, large vacuum bell): regen-cooled. Oxidiser comes in through the
    stub on top; fuel comes in at the bottom left, through a back-layer ring up both jackets to the injectors.
  - **Turbopumps** (hydrogen, oxygen, small): pump inlet on the end, outlet on top, shaft and turbine wheel.
  - **Gas generators** (normal and small): fuel from the left, oxidiser from the right, outlet at the bottom.
  - **Tanks and pressurant**: fuel tank, oxidiser tank, and a helium bottle with its regulator.
  - **Plumbing**: a length of feed line with a main valve and a throttle valve.
  - Pipe ends are left open, so a line drawn up to them joins. An unconnected one leaks, like a real open port.
- **Powerhead maker** (button under Prefab parts): turbopumps, turbines and a gas generator sized for your
  engine, drawn as one part.
  - Choose the propellants, chamber pressure, throat width, layout (one shaft or two) and turbine gas
    temperature. It works out the propellant flow, the pressure each pump must deliver, and from the same pump
    and turbine models the simulation runs: impeller sizes and stages, shaft speeds, turbine wheels, and the
    gas generator's mixture and orifices.
  - **Auto** gives liquid hydrogen its own shaft. It is 16 times lighter than LOX, so its pump needs 4 times
    the tip speed for the same pressure. Denser fuels share one shaft with the oxidiser pump.
  - Both propellants come in on the top edge and leave on the bottom edge through a main valve and a throttle
    valve, so tanks above and a chamber below join up without crossing. The turbine exhaust leaves at the
    bottom of its gas channel. The generator taps both pump outlets through the back pipe layer.
  - **Place powerhead** puts it on the cursor and sets the gas generator orifices, the pump governor, the start
    cartridge and the mixture control. Once it's piped up, **Size injectors for it** sizes the injectors for
    the chosen chamber pressure.
  - The start cartridge power is now saved with designs.
- - **Engine panel (right)**: what the engine is doing at a glance. It hides with *View > Engine panel*.
  - Firing / not lit / leaking at the top, with why it isn't lit.
  - **START**, **SHUTDOWN** and **IGNITER** buttons (moved here from the left menu, which keeps Pause, the
    resets and Clear all).
  - **Pressure regulators**: each regulator, which tank it feeds and the pressure it holds, with a **Set...**
    button. The **Set regulator pressure** tool (Tools) does the same by clicking a regulator, and picks the
    nearest one within a few cells.
  - Big chamber pressure, thrust and Isp, and 10-second traces of chamber pressure and thrust.
  - **Throttle** slider.
  - Propellant: flow, mixture ratio against the best one for the pair, tank and helium levels, feed-line
    pressure against burst.
  - One row per turbopump: speed against rated, turbine and pump power, cavitation or running dry. Click
    its name for the pump map.
  - Flame and hottest wall, and the event log, newest first.
- **Mixture O/F control** (engine panel, under the throttle): tick it and pick an oxidiser-to-fuel ratio.
  It works like the J-2's propellant utilisation valve: a controller trims the throttle valves to hold that
  mixture. For more oxidiser it closes the fuel valve down a little; for less, the oxidiser valve.
  - The slider is centred on the propellants' **stoichiometric** ratio (the exact amount of oxidiser to burn
    all the fuel; e.g. 7.94 for LH2/LOX, 3.4 for RP-1/LOX). Left of the middle is fuel-rich, where real
    engines run for the best Isp; right is oxidiser-rich, hot and hard on the walls.
  - It shows how far each valve is open. It needs a throttle valve in both lines, and can't go past what the
    pumps can push.
- **Fuel and oxidiser lines that touch** (often one injector manifold fed from both sides) make one network
  that can't flow. The engine panel says so in red, pointing at the injectors if that's where they meet.
  Each propellant needs its own manifold, with a wall between them.
- **Throttle valve** (Plumbing) is a metering valve; the throttle slider sets every one. It's sized against the
  injectors so the flow follows the throttle at the same feed pressure, and it travels at the valve stroke speed.
  - With throttle valves fitted, pump-fed engines also throttle their turbines, as real ones do: the turbine
    bypass lowers the pumps' discharge pressure with the throttle (RL10), and gas generators and preburners
    get their oxidiser metered too (J-2, RS-25).
  - Put one in both the fuel and the oxidiser line, or the mixture ratio shifts. Every preset has a pair just
    after its main valves.
  - At 60% throttle the presets give about 52-60% thrust; staged combustion throttles less steeply (~80%).
- - **Start sequence** is soft, as real engines start:
  - It **purges** first: for 1.5 s inert gas blows in through every injector and clears any propellant vapour
    left in the chamber.
  - Then it **primes**: it opens the fuel valves and waits while the fuel runs down the lines (and round the
    cooling jacket) until it reaches every fuel injector.
  - Then it opens the oxidiser (gas-generator engines: 1.2 s later, once the generator has spun the pumps
    up) and fires the start cartridge and the igniter.
  - Every valve opens slowly, over at least 1.5 s, and the throttle valves hold the engine at 40% until it
    has lit, then bring it up to the throttle setting over about 2.5 s. Held down like that, the pump governor
    keeps the discharge pressure low too, so the lines don't take a pressure spike.
  - The governor counts every pump on a shaft, including a dry one whose valve hasn't opened yet: the moment
    liquid reaches it, it makes its full no-flow pressure, and a shaft spun up for the other pump would burst
    its line.
- **Shutdown** is soft, as real engines shut down:
  - It switches the igniter off and throttles down to 40% over a second (if there are throttle valves).
  - Then it closes the oxidiser, including the gas generator's (which takes the turbines' power away).
  - About 1.8 s later it closes the fuel, so the engine ends fuel-rich with fuel still cooling the walls.
  - Every valve closes slowly, over at least 1.5 s, so there's no water hammer. Presets start with
  every valve closed.
- **Reset simulation** puts everything back to just before the engine was first fired: the same parts (holes
  you punched while it ran are repaired), full tanks and bottles, stopped pumps, closed valves, cold walls and
  still gas. The snapshot is taken the first time you press Start, switch the igniter on or open a valve; it is
  dropped when you load a preset or design, clear, or edit while nothing is running, so the next firing takes
  a fresh one. Settings you changed since are kept.
- **Side panel**, top to bottom: Engine (start, shutdown, igniter, pause, reset simulation, reset gas/heat,
  clear all), Preset engines, View, Brush, then the parts in groups: Structure (steel, copper, ceramic,
  niobium), Plumbing (pipe, valve, joint, regulator, injector, hot-gas injector), Tanks and bottles,
  Turbopumps and gas generators (pump, turbine, shaft, gas generator), Gas, fuel and fire (paint tools and
  source blocks), Tools (nozzle maker, pressure gauge, fill, inspect turbopump, toggle valve), Copy, paste and stamps. Each section
  folds.
- **Thrust reading**: thrust is the gas pressure on the walls from gas that's open to the outside world
  (chamber, nozzle, plume, vents); sealed vessels like a preburner and its ducts push on all their own walls
  and are left out. The gauge reads it averaged over about half a second, like a test-stand load cell.
- **Hot-gas injectors into a chamber** feed the gas through the whole injector plate (as a real hot-gas
  manifold does), straight in like the propellants, so it doesn't push the flame to one side.
- **Gas generators start from tank pressure**: their propellants push against the pressure actually in
  the generator's chamber (about 1 bar cold), up to the pressure their injectors are sized for, so opening a
  preburner's valves by hand with the pumps still lets tank pressure feed it.
- **Painted fire takes the local pressure**: it replaces the gas that's there at that gas's pressure, so it
  lights a chamber that's already full of pressurised propellant instead of being crushed.
- **Fire works as a makeshift igniter**: fire painted at a combustion chamber's injectors, or into a gas
  generator, lights it when propellant is flowing (any gas above 1200 K there does). Self-test:
  `--firelight` (valves opened by hand, no spark, then fire painted into the chamber).
- **Igniter: ON/OFF** stays armed: the chamber lights whenever fuel and oxidiser are both flowing. If there is
  no flame the gauges say why (valves shut, one propellant missing, igniter off). Turbines have full torque at
  standstill, so an expander lit by hand bootstraps itself up to full power.
- **Gas / Fuel vapour / Fire** paint free gas like Powder Toy: inert 1 bar gas, combustible vapour (green
  haze), or 2600 K fire. They only fill open space: painting over walls, pipes or parts leaves them intact
  (erase with right-click). Fuel burns where it is hot or touching a flame and spreads cell to cell. Propellant
  that cold-flows into a chamber without a flame stays combustible, so lighting it late gives a hard start.
- **Pipe layer: Back / Middle / Front** (2.5D). Erasing on Back or Front removes the layer pipe there, or
  whatever is in the engine plane where there is none. Back and front layers hold pipes that pass behind or in front
  of the engine plane without touching gas or walls. They join the engine only at **Pipe joint** cells (drawn
  as a ring), so pipes can cross freely. Back pipes show through gas and are hidden behind walls.
- **Valves** panel: an Open/Close button per valve (adjacent valve cells form one valve), Open all / Close all,
  and the valve stroke time. The **Toggle valve** tool does the same by clicking a valve.
- **Turbopumps** panel / **Inspect turbopump** tool: opens a live pump map (head vs flow at 60/80/100% and the
  current speed, operating point, efficiency, NPSH margin, turbine power).
- **Nozzle maker**: click one edge of the throat, then the other. An outline of the nozzle appears and
  follows the side-panel sliders (start width, expansion ratio, length, bell or conical, liner material
  and thickness, fuel jacket, steel shell); press **Build** (right-click or Cancel to start over).
  **Regen copper bell** and **Niobium skirt** set the usual layups. To extend an engine, click the two edges
  of its nozzle exit instead of the throat and untick "This is the throat".
- **Niobium** (C-103) is a radiation-cooled material: no coolant, it glows white-hot and radiates its heat
  away (melts at 2600 K). A niobium skirt on the kerolox engine in vacuum settles near 1100 K and lifts its
  Isp from 309 s to 322 s. In the regen presets the fuel enters the jacket just above the exit, so the exit
  is free for a skirt.
- **Ambient pressure** starts at **1 bar (sea level)**; the Settings panel has a slider and **Sea level** /
  **Vacuum** buttons. Big vacuum bells (the hydrolox presets, the hypergolic engine) are over-expanded at sea
  level and lose Isp, just like the real ones; switch to Vacuum to see them at their best.
- Views: **Flame** (default), Plain, Pressure, Velocity, Mach, Temperature, Density; optional tracer particles.

## Pipes and containers
- There is one **Pipe** part (plus generic **Valve**, **Pump**, **Injector**). A pipe holds whatever its
  network reaches: connect it to a fuel tank and it carries fuel, to an oxidiser tank and it carries
  oxidiser. A cooling jacket is just a pipe running through a copper wall. If fuel and oxidiser tanks end up
  in the same pipework it is blocked and you get a warning. Only tanks say what they hold.
- **Propellant has to travel through the pipes.** Liquid fills the lines from the tank outwards (you see the
  front move); a pump runs dry until liquid reaches it, an injector only feeds the chamber once liquid gets to
  it, a gas generator needs both feeds wet, and a hole only spills where there is liquid behind it. The front
  moves at the speed the feed pressure drives it, capped by **line fill speed** (default 10 m/s, so you can
  watch it; real lines fill in a fraction of a second). Lines stay full after a shutdown.
- In the regen-cooled presets the fuel goes **through the nozzle first**: down the outside to the bottom of
  the bell, into the cooling jacket through the shell just above the exit on both sides (a back-layer ring
  passes behind the bell to the far side), up the bell past the throat and the chamber, and only then into
  the injector manifolds.
- **Gas generator**: the Gas generator cells are its injector head. Each block of touching GG cells is one
  unit, fed by any fuel line and any oxidiser line it touches (its flow is set by the GG injector areas, not
  by how many cells it has). It burns into whatever open space the head faces: build a chamber of walls
  around that with an outlet and you can watch the fuel-rich combustion inside (about 870-900 K, dull red)
  and its dump plume. The gas-generator presets have a real-sized one (about 16 x 34 cm hydrolox, 12 x 30 cm
  kerolox outside) on the open right side of the fuel loop, so its exhaust falls away into open air:
  fuel from the loop, oxidiser from the tank through a back-layer line, a steel chamber narrowing to a
  dump outlet. Its turbine power still comes from its flow.
- **Turbines run on gas**: a turbine block lets gas through from its high-pressure side to its low side
  (an inlet and an outlet, like a pump). When a gas generator's exhaust is ducted to a turbine through
  enclosed gas, that turbine's power comes from the gas going through it: the isentropic expansion from
  its inlet to its outlet pressure times the blade efficiency (blade speed against spouting velocity). The
  gas leaves with that much less energy. Turbines aren't walled in automatically any more, so give them a
  casing with an inlet and an outlet.
  - **Any gas** going through a turbine drives it the same way, not just gas-generator exhaust. Fire burning
    in a closed box whose wall is a turbine wheel spins it up (self-test `--turbfire --idle`).
  - A generator whose exhaust only goes into open air drives nothing.
- **Real engines**: four presets are modelled on real engines. Each has its real propellants, cycle,
  turbopump layout, chamber pressure and mixture ratio; hover a preset for the real figures.

  | Preset | Here (sea level) | Real |
  |---|---|---|
  | **J-2** (Saturn V upper stages; LH2/LOX gas generator, two turbopumps, drive gas through the fuel turbine then the oxidiser turbine) | ~55 bar, O/F ~5.2, ~0.5 MN, pumps ~25,000 / ~9,000 rpm | 53 bar, O/F 5.5, 0.486 MN (1.03 MN vacuum), 27,000 / 8,600 rpm |
  | **Merlin 1D** (Falcon 9; RP-1/LOX gas generator, one-shaft powerhead) | 98 bar, O/F 2.15, 0.77 MN | 97 bar, O/F 2.36, 0.845 MN |
  | **F-1** (Saturn V first stage) | 61 bar, O/F 2.34, 5.8 MN, 5,630 rpm, 27 MW turbine | 70 bar, O/F 2.27, 6.77 MN, 5,500 rpm, 41 MW |

  - **RL10** (Centaur upper stage), the classic expander. The jacket's warm hydrogen (about 145 K) is piped
    through both turbines; it's an 8 cm deep engine. Here: 36 bar, O/F 6.1, 67 kN at sea level, **79 kN and
    Isp 473 s in vacuum**. Real: 42 bar, O/F 5.5, 99 kN, Isp 451 s. The real one drives its oxidiser pump
    through a gearbox from the same turbine; here each pump has its own turbine, the fuel's with two stages.
  - **Rutherford** (Rocket Lab Electron), the electric-pump engine: RP-1/LOX with no gas generator or turbine at
    all. One electric motor sits between the oxidiser pump and the fuel pump on a single shaft, with the
    battery pack bolted against its side, and the fuel still cools the chamber before it burns. It is drawn at
    **0.5 cm a cell** with a 8 cm slab. Here: about 64 bar, 24 kN and 6.5 kg/s at sea level, with the motor
    at its 115 kW rating and the battery giving 125 kW; the pack holds about 8 MJ, which is about 90 s of
    burn. Real: about 25 kN and Isp 311 s at sea level, two 50 hp (37 kW) motors, and a pack that gives over
    1 MW for nine engines. Here the Isp reads about 350-375 s, because the simulated nozzle is ideal. The pumps
    need more power than the real engine's (115 against about 75 kW) because the simulated pumps are less
    efficient at this tiny size. Real chamber pressure is not firmly known to me (a forum post says 1400 psi).
  - **Tripropellant** (LOX with methane and liquid hydrogen at once; the idea behind the Russian RD-701, which
    paired kerosene with hydrogen). It is the Rutherford powerhead, drawn at 0.5 cm a cell, with electric pumps
    for the oxygen and the methane, plus a small hydrogen tank to the left held at 95 bar by its own helium
    regulator (it takes so little flow that it needs no pump), down its own valve and throttle valve into its
    own injector manifold on the left of the head. Here: about 67 bar and 25 kN. The hydrogen is only about 10%
    of the fuel but lifts the specific impulse from 361 s to 370 s (and cools the walls from 630 K to 520 K); too
    much goes fuel-rich and loses it again.
  - The F-1 is drawn at **2.5 cm a cell** with a deeper slab (presets can set their own scale). Valves and
    feed-line friction scale with the cell size and depth, so a bigger drawing is bigger hardware.
  - In 2D the nozzles can't reach the real area ratios, so vacuum thrust in particular comes out lower than
    the real engines'.
  - The F-1 dumps its turbine exhaust overboard here; the real one fed it into the nozzle extension.
- - **Merlin 1D and F-1** use one turbopump **powerhead** (like the textbook gas-generator cycle, or
  Merlin): oxidiser pump, turbine and fuel pump on one shaft, the gas generator bolted to the turbine and
  fed from both pump outlets through control valves, its exhaust driving the turbine and the turbine
  exhaust ducted down and dumped (about 100 bar, 0.79 MN, 290 s at sea level).
- **J-2, exhaust-into-nozzle and staged combustion** use a **two-shaft powerhead**
  (J-2 / RS-25 style; hydrogen and oxygen pumps can't share one speed): the fuel turbopump and the oxidiser
  turbopump side by side, the gas generator (or preburner) bolted to the fuel turbine, and the drive gas
  going generator -> fuel turbine -> duct -> oxidiser turbine, then dumped overboard, ducted into a slot
  above a niobium nozzle extension, or (closed cycle) through a hot-gas manifold and injector into the main
  chamber. The nozzles in front of each turbine set how the expansion is shared between them. About 106 bar
  / 382 s, 115 bar / 387 s, and 130-140 bar for staged combustion (its preburner runs at about 650 bar,
  higher than a real RS-25's, to drive the series turbines against chamber pressure).
- **Gas generator / preburner injector plate**: the head's cells are injector elements, alternating
  oxidiser and fuel (2 cells each, drawn in the propellant colours). Lit, the oxidiser elements spray their
  oxidiser with just enough fuel to burn and light at the plate; the fuel elements spray the rest of the
  fuel cold; the two mix down the small chamber and arrive at the generator's fuel-rich temperature (about
  870-1070 K). In the Flame view, gas that is actively burning shows as a bright flame front.
- **Burning**: the burn rate rises with temperature and with how much of the gas is burnable, as in real
  chemistry. A dilute mix just above ignition (a gas generator's jets) burns out over several cells, so the
  jets show as separate flames from the injector plate; a cloud of fuel vapour, a fire or a ~3000 K chamber
  burns in a few dozen steps, so painted fire still spreads.
- **Smoke** is drawn like the flame: soft wisps carried along with the flow instead of speckles. Dense cold
  gas and warm exhaust (a dumped gas generator) show as smoke; a sooty propellant (kerosene) makes it thick,
  dark and billowing, a clean one (hydrogen) thin and wispy; unburnt fuel is tan.
- **Gas lines**: pipework that isn't connected to any tank or helium bottle and opens onto gas at its ends
  carries gas, from the higher-pressure end to the lower (a closed box's contents can go straight into a
  pipe; erase the cap a painted pipe gets to open its end). Valves work on them, and drive gas from a gas
  generator is followed through them to a turbine. Self-test: `--gaspipe` (a pressurised box venting through
  a pipe; `--nopipe` without it).
- **Turbines and hot-gas injectors are one-way**: gas only goes from a turbine's inlet (where the drive gas
  arrives) to its outlet, and from a hot-gas injector's duct side into the big space beyond; if the
  pressure reverses they stall instead of running backwards. Turbine drive power is smoothed over about
  0.15 s.
- **Two-shaft engines hold their mixture on their own** (J-2, hydrolox GG into the nozzle, staged combustion):
  each pump runs on its own shaft, and left alone the oxidiser side could run away from the fuel side (on a
  slower GPU the staged engine drifted to O/F 14, and in one game to 96, with the fuel pump unable to beat the
  chamber pressure). The mixture control is now on from the start, a little fuel-rich of the best mixture
  (O/F 5.5 for hydrogen), and holds at every gas step rate I tried. The staged engine's pump bypass is also
  set to 170 bar so its chamber pressure doesn't creep up over a long burn (about 110 bar, 0.95 MN).
- **The mixture changes how hard the walls are heated** (the flame itself still peaks at the best mixture and
  falls off either side, as real flames do):
  - **Fuel-rich** (below the best mixture): the extra fuel hugs the wall as a cooling film, so the walls take up
    to 30% less heat. The Heat panel says so.
  - **Oxygen-rich** (above it): no film at all, and the walls take far more heat: about 3x at stoichiometric
    (for kerosene) and up to 5.5x at twice the best mixture. Kerosene at O/F 4.5 holds its walls around
    1320 K (copper melts at 1350 K), cooks its coolant, bursts lines and loses chamber pressure within about a
    minute. Hydrogen at O/F 11 runs its walls near 1080 K. Real oxygen-rich engines need special alloys.
  - The scale follows the measured mixture over about half a second; the engine panel shows it and warns in red
    when it is over 2.5x, and an event is logged.
- **Tri-propellant engines (two fuels):** put fuel tanks with different fluids in the engine (Fill tool) and each
  line carries its own fuel (its own density, coolant properties, pumps and injectors). The burn is the pairs
  blended by the share of the fuel flow each line gives: the best mixture ratio, stoichiometric ratio, flame
  temperature, gas constant and flame colour all move between them as the shares change, and the engine panel
  names it ("LH2 + CH4 / LOX (tri-propellant)"). The second fuel has its own injector size (*second fuel inj
  cm2*; the fuel with fewer tank cells is the second). The tank colours were also swapped: oxidiser tanks are
  blue (liquid oxygen) and fuel tanks warm, matching the liquids.
- **An empty pipe is just a hole.** Pipes and injectors only cool a wall while liquid is in them. Where the
  liquid hasn't run yet (before a start the lines are empty, and during priming the front moves down them), where
  a line joins no tank, or once its tank has run dry, a pipe cell is a cavity: it conducts a little heat, holds a
  little heat of its own, cools nothing, and can melt. A regen jacket therefore only protects the chamber once the
  fuel has filled it. The hover readout says "empty: just a hole in the wall, no cooling". (Closing the preset
  staged-combustion engine's manifold pocket beside its hot-gas injector, which used to be an accidental
  permanent coolant sink, is tied to the fuel on the far side through the back layer.)
- **Thrust** shows in the unit that fits: MN above 1 MN, kN for the smaller engines (24.1 kN for Rutherford),
  and N for a cold-gas thruster. The thrust trace's label shows the current value too.
- **Electric pumps** (Rocket Lab Rutherford style): parts under *Electric pumps* in the palette, plus an
  *Electric turbopump* prefab.
  - **Electric motor:** sits on a shaft like a turbine (pump, shaft cells, motor). It gives full torque up to
    a third of the shaft's rated speed and constant power above that. Power is about 3.6 kW per cell (around
    100 kW a litre: Rutherford's 50 hp motor is the size of a soda can), scaled by cell size and depth.
  - **Battery:** stores about 60 kJ per cell and can deliver about 1 kW per cell. Batteries, **Wire** and
    motors that touch make one circuit, and a motor draws from its own circuit (motor and inverter about
    92% efficient).
  - **When it runs:** the motor runs while the oxidiser valves are open or the engine is lit, so Start spins
    the pumps up as the oxidiser goes in and Shutdown stops it. The pump governor and throttle control it
    like a turbine, which makes electric pumps easy to throttle.
  - **Recharging:** click a battery (or its motor or wire) with the **Fill** tool; *Reset gas/heat* recharges every
    pack along with the tanks; *Reset simulation* puts the whole engine back as it was; and the *GSE: keep
    batteries charged* option under Propellant and feed keeps them topped up (as if on a charger).
  - **Engine panel:** shows motor power, battery charge, seconds of burn left, and a warning when the battery
    pack is the limit.
  - **Why only small engines use them:** a 25 bar, 76 kN kerolox engine needs about 130 kW and drains a
    300-cell pack in about 100 s. A 100 bar booster engine needs megawatts, which no battery pack can carry,
    and that's why real electric-pump engines are small.
  - Headless test: `--electric` turns a set-up engine's turbines into motors and packs batteries next to them.
  - **Rutherford preset** (index 8): see the real-engines list.
- **Small rotors lose little to bearings.** Bearing drag was tuned on big turbopumps and ate most of a 100,000
  rpm motor's power. It now falls off steeply for rotors smaller than a big turbopump's, which also helps the RL10.
- **Expander drive comes from the plumbing**: a turbine that a fuel line runs into is driven by that fuel,
  warmed in the cooling jacket and expanding through it. There's no expander switch: any build where a
  jacket-heated fuel line touches a turbine is an expander.
  - **Pipes go through turbines:** pipes touching the same turbine are joined through it, so a line can run
    in one side of the wheel and out the other on to the injectors. Pumps butted against a turbine stay
    separate.
  - **The expander presets are plumbed in series, as on the RL10.** Both jacket tops empty into the loop over
    the top of the engine, which now only collects the hot hydrogen. It goes down to the fuel turbine, back up
    and behind the loop (back layer) into the oxidiser turbine, and out of that down into the fuel manifold. A
    back-layer pipe behind the oxidiser manifold feeds the other side.
  - It obeys energy conservation:
  - **Jacket heating:** the coolant warms by the heat the jacket picks up. The flat 2D chamber has much less
    wall than a round one, so that heat is scaled up (*cooling jacket heat scale*, about 12-14), and hydrogen
    leaves the jacket at 130-200 K, kerosene at about 440 K, as in real engines.
  - **Turbine power:** each turbine on the line takes its whole flow (turbines in series) and expands it
    across its pressure ratio, about 1.3 per turbine stage. Its power is that flow times the enthalpy the gas
    gives up, never more than the gas carries, and its efficiency still depends on blade speed.
  - **Pressure cost:** the pressure drop that work takes comes out of the fuel line before the injectors, so
    the fuel pump has to make it up.
  - **Only fuels that boil can do it:** hydrogen and methane expand as gas. Kerosene and MMH give a turbine
    nothing, as in reality.
  - **Expanders don't scale up:** the heat a chamber picks up grows with its wall area, but the pump power an
    engine needs grows with its flow. That's why real expanders are upper-stage engines (RL10, Vinci). The
    **Hydrolox expander** preset settles at about 72 bar and 650 kN. It used to reach 152 bar and 1.4 MN only
    because the old model handed the turbines 14 times the heat as shaft power.
  - Expander presets hold their mixture with the throttle valves (mixture control on): with no gas generator
    setting the pace, the oxidiser side would otherwise run away from the fuel side.
- **Exhaust colour follows the propellants**: sooty kerosene burns bright orange-yellow; clean flames show the
  colour of their hot gases instead and are fainter: hydrogen pale violet, methane blue, hypergolics
  orange-pink. Flame fronts follow too.
- **The edges act like open air**. The bottom is the exhaust's way out (outflow only). At the top the air
  settles still, and at the sides up/down drift settles out while gas can still flow in or out sideways. Air
  the plume drags along now comes in from the sides and goes down with it. Before, a big loop formed that
  carried warm smoke up past the engine in curling wisps.
- - **Absorbing border**: within 24 cells of the edge of the world, gas relaxes to ambient density and
  temperature while keeping its velocity, so exhaust flows off-screen instead of reflecting back and
  filling the space.
- **Hot-gas injector**: a block that lets gas through from whichever side is at the higher pressure
  (orifice flow), carrying its heat; fuel-rich generator gas brings its unburnt fuel along to burn on the
  far side. Use it to feed a preburner's exhaust into a main chamber.
- **Cycle presets**: *Hydrolox GG, exhaust into nozzle* (open cycle, F-1 style) ducts the generator exhaust
  down beside the bell into a slot above a niobium nozzle extension, lining its wall. *Hydrolox staged
  combustion* (closed cycle, RS-25 style) puts a preburner under the oxidiser tank whose exhaust goes through
  a hot-gas injector into the main chamber: about 123 bar and 434 s at sea level against 101 bar and 354 s
  for the open-cycle gas generator.
- **Tank pressurisation has to be plumbed**: a tank is only kept at pressure by helium if a helium line
  (or the bottle itself) reaches it through a **Pressure regulator** cell touching the tank. Without one the
  tank has only the gas already above its liquid and blows down as it drains (cryogens still boil off).
  Each regulated tank draws from the bottle piped to it. The presets pipe their helium bank to both tanks:
  the oxidiser line passes behind the fuel loop on the back layer, the fuel line runs over the top and
  down the left side onto the fuel tank, also on the back layer.
- **No mode switches: the build decides.**
  - **Pumped or pressure-fed**: a line with a pump gets its head on top of tank pressure; a line with no
    pump is fed by tank pressure alone. The tank-pressure sliders (or a regulator's own setting) set it.
    Auto-size injectors raises the tank pressure of an unpumped side to what the target chamber needs.
  - **Propellants come from the tanks**: click a tank with the **Fill** tool to choose what it holds.
    Fuel tanks hold LH2, CH4, RP-1 or MMH; oxidiser tanks hold LOX or N2O4. Choosing drains the tank and
    fills it with the new one. The engine burns what its fuel and oxidiser tanks hold together (any of
    the eight pairs; only MMH / N2O4 is hypergolic). With several tanks it goes by the most tank cells.
    A new tank painted onto an old one holds the same. Contents are saved with designs and stamps and
    go along with copy and paste.
  - Older saves made with the pressure-fed switch on load with both tanks at their regulator setting.
- **Each regulator can have its own pressure**: click a Pressure regulator with the **Fill** tool and set
  the pressure (in bar) it holds its tank at, or send it back to the shared setting (the tank-pressure
  sliders in Propellant and feed). Hovering over a regulator shows which it's on. A regulator can only
  deliver up to about 95% of its helium bottle's pressure. The settings are saved with the design.
- **Helium lines**: pipework that touches a helium bottle (and no propellant tank) carries helium, drawn
  grey-blue. Bottle -> pipe -> valve -> injector is a valved cold-gas thruster: the valve shows as a Helium
  valve, Open all / Close all include it, and the gas jets out of the injector at choked flow while the
  bottle blows down. A hole in a helium line vents the same way.
- Painting pipes, tanks, bottles, valves, pumps and turbines puts a **real one-cell steel wall** around them
  (presets have it too). Erase any wall, or the container itself, and the contents **leak out** into the
  simulation: propellant sprays out as cold vapour at a rate set by its pressure and the hole size (fuel
  vapour can catch fire), a helium bottle vents at choked (sonic) flow like a punctured COPV, and the tank
  level drops. Any hole is a crude thruster: a helium bottle with one wall block knocked out is a cold-gas
  thruster (the gauges show the jet's push while something is leaking). A melted chamber
  wall that reaches the cooling jacket leaks coolant into the chamber.
- **Pipe joint** links back/front layer pipes to the engine plane; layer pipes only connect at joints, so
  they can cross anything else.

- **Helium bottles** are separate containers: each keeps its own pressure. When you finish drawing one you are
  asked what to fill it to (psi); the **Fill** tool sets a bottle's pressure or tops up a tank later. Anything
  newly drawn is full.
- **Cryogens boil off.** LH2, LOX and methane soak up heat through their tank walls and boil; in a sealed tank
  the vapour raises the pressure by itself (autogenous pressurisation), draining the tank expands the ullage so
  pressure falls, helium tops it up when low. Vent valves hold it near working pressure; switch them off and
  boil-off pressure builds until the tank splits. Tanks show the liquid level with swirling vapour above it.
  RP-1, MMH and N2O4 are storable and do not boil. "Boil-off speed-up" sets how fast (real boil-off is slow).
- **GSE** checkboxes keep propellant tanks full and helium bottles at their fill pressure, as if plumbed to
  ground support equipment.
- **Source blocks** (Gas / Fuel vapour / Fire source), like Powder Toy's clone: they keep pushing their
  substance into any open space they touch, at the "source block pressure".

## What is simulated
- **Gas (GPU, `sim.wgsl`)**: 2D compressible Euler (Rusanov), reflective walls, gas->wall heat transfer, wall
  conduction, coolant passages and injectors (the propellant flowing through cools the injector face) as heat sinks; optional round-engine (axisymmetric) mode.
- **Turbopumps (`world.rs`) are sized like real hardware** (1 cell = 1 cm). Pump, turbine and shaft cells
  that touch form one rotor; its axis runs from the pump to the turbine.
  - A pump block's side **across** the shaft is the impeller diameter D, its length **along** the shaft holds
    the stages (one per ~0.6 D). Head per stage = 0.55 rho u_tip^2, best-efficiency flow = 0.03 x eye area x
    u_tip. So LH2 needs a fast, multi-stage pump and LOX a slower one, exactly like real engines.
  - **Pump losses**:
    - Hydraulic efficiency peaks at the design flow and depends on impeller size: about 67% for 5 cm,
      76% for 25 cm, 80% for 50 cm. It falls off either side of the design flow.
    - Below design flow the liquid recirculates in the eye and diffuser. A pump run against a shut valve
      still takes about a third of its design power, so a primed pump can't run away.
    - Disc friction (shrouds churning the liquid, ~rho w^3 r^5) is there even at zero flow.
    - A pump that has **lost its prime** (dry) has almost no load and overspeeds.
  - **Cavitation**:
    - The suction pressure needed (with inducer) is 0.006 rho u_tip^2 at design flow, and rises with
      flow. Available suction pressure is tank pressure minus vapour pressure, minus the dynamic head into
      the eye.
    - With too little margin the head **breaks down** steeply (nothing left below 30% of what's needed).
      Cavities take ~0.05 s to grow or collapse, which damps start-up surges.
  - **Pumps and feed lines are solved together**: the pump curve (head = h0 - k m^2) goes into the same
    implicit step as the line inertia and injector, so stiff pumps don't oscillate against their lines.
    Liquid fills the lines up to a shut valve, so pumps upstream of it are primed before the start.
  - Rated speed = 550 m/s tip speed on the largest wheel; above 130% the rotor **disintegrates**. Inertia comes
    from the rotor's actual size. Bearings and seals add a breakaway drag plus a part that grows with speed
    (~1% of a big turbopump's power).
  - **Turbines** are impulse turbines. Efficiency comes from the blade-speed ratio u/c0 (warm hydrogen
    ~900 m/s, gas-generator exhaust ~2000 m/s, start cartridge 1500 m/s). Torque is highest at standstill
    and zero at twice the best blade speed.
    - A turbine block's length sets its **stages**: one per 0.4 diameters, up to 3. A 1-stage wheel is
      best at u/c0 0.47 (74%), 2 stages at 0.33 (78%), 3 stages at 0.27 (82%). Slow, small wheels need
      more stages.
    - Non-ducted turbines share the drive gas by area. Ducted ones run on the gas actually going through them.
    - The **start cartridge** is a burst of solid-propellant gas through every turbine: it rises over the
      first moment and tails off over its last half second.
    - The Inspect turbopump window plots the efficiency curves with the operating point, and splits each
      pump's power into useful, recirculation and disc friction.
  - The governor (turbine bypass) holds tip speed under 104% and pump discharge under a set pressure.
  - Preset: fuel turbopump = 24 cm two-stage impeller + 24 cm turbine, ~43k rpm, ~200 bar;
    oxidiser turbopump = 22 cm single-stage impeller + 32 cm turbine disc, ~18k rpm, ~250 bar.
  - Drawn as side-view cutaways: inlet inducer, impeller stages, diffusers, shaft, hot turbine blade rows.
- **Valves and transients**: valves stroke open/closed over a set time; the liquid column in each line has
  inertia, so flow builds up and decays instead of jumping. Slamming a valve shut makes a **water-hammer**
  surge (`L/A * dm/dt`) that can burst a line above 450 bar. Tanks are helium-pressurised while the bottles
  last, then fall along an ullage blowdown curve.
- **Regen cooling**, **gas generator**, **helium pressure-fed**, eight propellant pairs, ignition (cryogenic needs a
  spark, hypergolics light on contact), melting and burst failures as before.
- **Flames**: the Flame view draws every propellant like a lighter flame: soft, translucent orange with a
  yellow-white core, a blue base where fuel is still burning, flicker, and edges that fade with gas density.
  Shock diamonds still light up; cold unburnt propellant shows as white mist.
- 2D slab or axisymmetric; combustion is a flame temperature looked up from O/F (no chemistry).
- Liquid in the lines is one lumped column per network (no pressure gradient along a pipe).
- Round mode shows a thin hot spike along the axis (axisymmetric-on-Cartesian-grid artefact).
- Wall heating is sped up by the "wall heat speed-up" slider because gas steps are microseconds long.
- Staged combustion is not modelled; the gas generator is an open (dump) cycle.

## Performance
- **Sleeping tiles**: the gas grid is split into 8x8-cell tiles. A tile of still gas whose neighbours
  didn't change either is skipped; walls and plumbing always run, and anything you paint, any gas source and
  any settings change wake the tiles they touch. Still air costs next to nothing.
- **Each tile steps at its own rate** (local time stepping, the time-wise version of video macroblocks).
  The global time step is sized for the fastest gas anywhere (exhaust at several km/s), but calm air only
  needs a step 4-8x as long.
  - Each 8x8 tile works out the fastest wave in it (flow speed plus speed of sound) and runs every 1st, 2nd,
    4th or 8th step with a 1-8x step to match.
  - It never goes faster than twice its slowest neighbour, so rates change gently towards the plume.
  - Tiles with walls or machinery always run every step (wall heat conduction needs it). In between its
    updates a tile only carries its state over.
  - Firing the expander: about a third of the tiles run every step, the rest at 4x or 8x. That's about 27%
    more gas steps per second at the default settings (35% at 400 steps a frame), the same engine numbers,
    and about 51 W of GPU power instead of about 70 W.
  - Debug: `ROCKET_PROFILE=1` with `--selftest` prints how many tiles are awake and at each rate.
  - It's done in time rather than space (coarser cells) so walls, pipes and nozzle contours stay at full
    resolution everywhere.
- **Gas steps are per second of real time** ("gas steps per 1/60 s", default 60): a fast display or extra
  redraws don't multiply the GPU work. **Frame rate cap** (default 60) limits redraws; a minimised window
  pauses everything.
- The Flame view's glow is worked out once per cell per frame (`glow.wgsl`) instead of per screen pixel.
- Measured on an RTX 3050 Laptop GPU with the expander preset: idle 97 % / 59 W -> 37 % / 17.5 W,
  firing 100 % / 71 W -> 61 % / 61 W.
- Test switches: `--steps N`, `--idle`, `ROCKET_RENDER_BENCH=N` (times the field render),
  `ROCKET_STEPS`, `ROCKET_MODE`, `ROCKET_READBACK_EVERY` (keep at 1: older chamber-pressure readings make
  the gas-generator engines oscillate).
