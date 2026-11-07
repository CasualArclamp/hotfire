//! wgpu side: compute solver, read-back and the field renderer.

use crate::world::{Readback, Settings, Src, H, N, W};
use bytemuck::{Pod, Zeroable};
use eframe::wgpu;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const MAX_SRC: usize = 4096;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SimParams {
    pub w: u32,
    pub h: u32,
    pub nsrc: u32,
    pub q_heat: f32, // heat released per kg of combustible gas, J/kg
    pub dx: f32,
    pub dt: f32,
    pub depth: f32,
    pub gamma: f32,
    pub rgas: f32,
    pub accel: f32,
    pub h0: f32,
    pub amb_rho: f32,
    pub amb_t: f32,
    pub rho_min: f32,
    pub axi: u32,
    pub axis_x: f32,
    pub amb_k: f32, // quiet air anywhere relaxes to the ambient density at this fraction a base step (while the altitude changes)
    pub amb_v: f32, // the wind past the vehicle, m/s down the screen (flying up: air rushes down past the engine)
    pub _pad: [f32; 2],
}

impl SimParams {
    pub fn from_settings(st: &Settings) -> Self {
        let p = st.p();
        SimParams {
            w: W as u32,
            h: H as u32,
            nsrc: 0,
            q_heat: p.rgas / (p.gamma - 1.0) * (p.t_peak - 300.0),
            dx: st.dx,
            dt: st.dt(),
            depth: st.depth,
            gamma: p.gamma,
            rgas: p.rgas,
            accel: st.heat_accel,
            h0: st.h0,
            amb_rho: st.amb_p / (p.rgas * 300.0),
            amb_t: 300.0,
            rho_min: 1e-5,
            axi: st.axi as u32,
            axis_x: W as f32 / 2.0,
            amb_k: 0.0,
            amb_v: 0.0,
            _pad: [0.0; 2],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RenderParams {
    pub w: u32,
    pub h: u32,
    pub mode: u32,
    pub show_heat: u32,
    pub cx: f32,
    pub cy: f32,
    pub scale: f32,
    pub gamma: f32,
    pub rw: f32,
    pub rh: f32,
    pub rgas: f32,
    pub particles: u32,
    pub time: f32,
    pub tint: [f32; 3],
    pub soot: f32,
    pub liq_f: [f32; 3],
    pub liq_o: [f32; 3],
    pub edit_layer: u32, // 1 while the player edits the back layer (ghost it on top)
    pub amb_rho: f32,    // ambient gas density: only gas above this is drawn as speckles
    pub _q: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PartParams {
    w: u32,
    h: u32,
    count: u32,
    frame: u32,
    dtc: f32,
    gamma: f32,
    rgas: f32,
    dx: f32,
}

pub const PARTICLES: usize = 40000;

pub struct FrameInput {
    pub reset: Option<Vec<u32>>,
    pub mat: Option<Vec<u32>>,
    pub dirty: Vec<(usize, bool)>,
    pub params: SimParams,
    pub net_t: [f32; 256],
    pub srcs: Vec<Src>,
    pub steps: u32,
    pub render: RenderParams,
    pub particles: bool,
    pub phase: Option<Vec<f32>>,
    pub layers: Option<Vec<u32>>,
    pub anim: Vec<f32>,
    /// gas painted by the player: (cell, state, combustible density)
    pub paint: Vec<(usize, [f32; 4], f32)>,
    /// a simulation frame (at most at the frame-rate cap): work out the flame glow and read the field
    /// back. Redraws in between (panning, a fast mouse) only repaint the view.
    pub sim: bool,
}

pub struct GpuSim {
    adv_pipe: wgpu::ComputePipeline,
    adv_bg: [wgpu::BindGroup; 2],
    _parts: wgpu::Buffer, // kept alive for the particle bind groups
    pp: wgpu::Buffer,
    pdraw_pipe: wgpu::RenderPipeline,
    pdraw_bg: wgpu::BindGroup,
    frame_no: u32,
    step_pipe: wgpu::ComputePipeline,
    src_pipe: wgpu::ComputePipeline,
    sim_bg: [wgpu::BindGroup; 2],
    render_pipe: wgpu::RenderPipeline,
    render_bg: [wgpu::BindGroup; 2],
    glow_bg: [wgpu::BindGroup; 2],
    emit_pipe: wgpu::ComputePipeline,
    gather_pipe: wgpu::ComputePipeline,
    _glow_bufs: [wgpu::Buffer; 2],
    state: [wgpu::Buffer; 2],
    mat: wgpu::Buffer,
    net_t: wgpu::Buffer,
    flux: wgpu::Buffer,
    spec: [wgpu::Buffer; 2],
    last_params: Option<SimParams>,
    frames_since_readback: u32,
    no_sleep: bool, // ROCKET_NO_SLEEP: keep every tile awake (for comparing)
    readback_every: u32,
    src: wgpu::Buffer,
    params: wgpu::Buffer,
    rparams: wgpu::Buffer,
    phase: wgpu::Buffer,
    layers: wgpu::Buffer,
    anim: wgpu::Buffer,
    stage_state: wgpu::Buffer,
    stage_flux: wgpu::Buffer,
    mapped_state: Arc<AtomicBool>,
    mapped_flux: Arc<AtomicBool>,
    pending: bool,
    pending_time: f32,
    acc_time: f32,
    cur: usize,
    pub total_steps: u64,
}

fn entry(binding: u32, ty: wgpu::BufferBindingType, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn buf(device: &wgpu::Device, size: usize, usage: wgpu::BufferUsages, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage, mapped_at_creation: false })
}

/// 8x8-cell tiles for the sleeping-tile optimisation.
const N_TILES: usize = ((W + 7) / 8) * ((H + 7) / 8);

pub fn init_state(mat: &[u32], p: &SimParams) -> Vec<[f32; 4]> {
    mat.iter()
        .map(|&m| {
            if m & 255 == 0 {
                [p.amb_rho, 0.0, 0.0, p.amb_rho * p.rgas / (p.gamma - 1.0) * p.amb_t]
            } else {
                [0.0, 0.0, 0.0, 300.0]
            }
        })
        .collect()
}

impl GpuSim {
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> GpuSim {
        use wgpu::BufferBindingType as B;
        use wgpu::BufferUsages as U;
        let ro = B::Storage { read_only: true };
        let rw = B::Storage { read_only: false };
        let cs = wgpu::ShaderStages::COMPUTE;

        let sim_src = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sim"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("sim.wgsl"))),
        });
        let sim_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sim bgl"),
            entries: &[
                entry(0, B::Uniform, cs),
                entry(1, ro, cs),
                entry(2, rw, cs),
                entry(3, ro, cs),
                entry(4, ro, cs),
                entry(5, rw, cs),
                entry(6, ro, cs),
                entry(7, ro, cs),
                entry(8, rw, cs),
            ],
        });
        let sim_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&sim_bgl)],
            immediate_size: 0,
        });
        let mk = |name: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&sim_pl),
                module: &sim_src,
                entry_point: Some(name),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let step_pipe = mk("step");
        let src_pipe = mk("sources");

        let sbytes = N * 16;
        let st = U::STORAGE | U::COPY_DST | U::COPY_SRC;
        let state = [buf(device, sbytes, st, "state a"), buf(device, sbytes, st, "state b")];
        let mat = buf(device, N * 4, U::STORAGE | U::COPY_DST, "mat");
        let net_t = buf(device, 256 * 4, U::STORAGE | U::COPY_DST, "net_t");
        let flux = buf(device, N * 4, st, "flux");
        // combustible gas per cell, then per tile: awake flag, step rate it wants, when it last ran; then a step counter
        let spec_len = (N + 3 * N_TILES + 1) * 4;
        let spec = [buf(device, spec_len, U::STORAGE | U::COPY_DST | U::COPY_SRC, "spec a"), buf(device, spec_len, U::STORAGE | U::COPY_DST | U::COPY_SRC, "spec b")];
        let src = buf(device, MAX_SRC * 32, U::STORAGE | U::COPY_DST, "src");
        let params = buf(device, std::mem::size_of::<SimParams>(), U::UNIFORM | U::COPY_DST, "params");
        let rparams = buf(device, std::mem::size_of::<RenderParams>(), U::UNIFORM | U::COPY_DST, "rparams");
        let phase = buf(device, 3 * N * 4, U::STORAGE | U::COPY_DST, "phase");
        let layers = buf(device, N * 4, U::STORAGE | U::COPY_DST, "layers");
        let anim = buf(device, crate::world::ANIM_LEN * 4, U::STORAGE | U::COPY_DST, "anim");
        let stage_state = buf(device, sbytes, U::MAP_READ | U::COPY_DST, "stage state");
        // flame-view glow: per-cell light, then per-cell glow gathered from it (see glow.wgsl)
        let em = buf(device, N * 16, U::STORAGE, "emission");
        let glow = buf(device, N * 16, U::STORAGE, "glow");
        let rsrc = include_str!("render.wgsl");
        let glow_src = format!("{}{}", &rsrc[..rsrc.find("@group").expect("render.wgsl starts with struct R")], include_str!("glow.wgsl"));
        let glow_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("glow"), source: wgpu::ShaderSource::Wgsl(glow_src.into()) });
        let glow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glow bgl"),
            entries: &[entry(0, B::Uniform, cs), entry(1, ro, cs), entry(2, ro, cs), entry(3, rw, cs), entry(4, rw, cs)],
        });
        let glow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&glow_bgl)], immediate_size: 0 });
        let glow_pipe = |name: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&glow_pl),
                module: &glow_mod,
                entry_point: Some(name),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (emit_pipe, gather_pipe) = (glow_pipe("emit"), glow_pipe("gather"));
        let stage_flux = buf(device, N * 4, U::MAP_READ | U::COPY_DST, "stage flux");

        let bg = |c: usize, n: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &sim_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: state[c].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: state[n].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: mat.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: net_t.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: flux.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: spec[c].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 8, resource: spec[n].as_entire_binding() },
                ],
            })
        };
        let sim_bg = [bg(0, 1), bg(1, 0)];

        // renderer
        let rshader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("render.wgsl"))),
        });
        let fsv = wgpu::ShaderStages::FRAGMENT;
        let rbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render bgl"),
            entries: &[entry(0, B::Uniform, fsv), entry(1, ro, fsv), entry(2, ro, fsv), entry(3, ro, fsv), entry(4, ro, fsv), entry(5, ro, fsv), entry(6, ro, fsv), entry(7, ro, fsv)],
        });
        let rpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&rbgl)],
            immediate_size: 0,
        });
        let render_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render"),
            layout: Some(&rpl),
            vertex: wgpu::VertexState {
                module: &rshader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &rshader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let rbg = |c: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &rbgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: rparams.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: state[c].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: mat.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: phase.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: anim.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: spec[c].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: layers.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: glow.as_entire_binding() },
                ],
            })
        };
        let render_bg = [rbg(0), rbg(1)];
        let gbg = |c: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &glow_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: rparams.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: state[c].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: mat.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: em.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: glow.as_entire_binding() },
                ],
            })
        };
        let glow_bg = [gbg(0), gbg(1)];

        // tracer particles
        let parts = buf(device, PARTICLES * 32, U::STORAGE | U::COPY_DST, "particles");
        let pp = buf(device, std::mem::size_of::<PartParams>(), U::UNIFORM | U::COPY_DST, "pp");
        let adv_src = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("advect"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("advect.wgsl"))),
        });
        let adv_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("adv bgl"),
            entries: &[entry(0, B::Uniform, cs), entry(1, ro, cs), entry(2, ro, cs), entry(3, rw, cs)],
        });
        let adv_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&adv_bgl)], immediate_size: 0 });
        let adv_pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("advect"),
            layout: Some(&adv_pl),
            module: &adv_src,
            entry_point: Some("advect"),
            compilation_options: Default::default(),
            cache: None,
        });
        let abg = |c: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &adv_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: pp.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: state[c].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: mat.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: parts.as_entire_binding() },
                ],
            })
        };
        let adv_bg = [abg(0), abg(1)];
        let pd_src = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pdraw"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("pdraw.wgsl"))),
        });
        let vs = wgpu::ShaderStages::VERTEX;
        let pd_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pdraw bgl"),
            entries: &[entry(0, B::Uniform, vs), entry(1, ro, vs)],
        });
        let pd_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&pd_bgl)], immediate_size: 0 });
        let pdraw_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pdraw"),
            layout: Some(&pd_pl),
            vertex: wgpu::VertexState { module: &pd_src, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &pd_src,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pdraw_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pd_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: rparams.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: parts.as_entire_binding() },
            ],
        });

        GpuSim {
            adv_pipe,
            adv_bg,
            _parts: parts,
            pp,
            pdraw_pipe,
            pdraw_bg,
            frame_no: 0,
            step_pipe,
            src_pipe,
            sim_bg,
            render_pipe,
            render_bg,
            glow_bg,
            emit_pipe,
            gather_pipe,
            _glow_bufs: [em, glow],
            state,
            mat,
            net_t,
            flux,
            spec,
            src,
            params,
            rparams,
            phase,
            layers,
            anim,
            stage_state,
            stage_flux,
            mapped_state: Arc::new(AtomicBool::new(false)),
            mapped_flux: Arc::new(AtomicBool::new(false)),
            pending: false,
            pending_time: 0.0,
            acc_time: 0.0,
            cur: 0,
            total_steps: 0,
            last_params: None,
            frames_since_readback: 0,
            no_sleep: std::env::var("ROCKET_NO_SLEEP").is_ok(),
            readback_every: std::env::var("ROCKET_READBACK_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(2),
        }
    }

    /// Wake every tile (after a reset or a settings change).
    fn wake_all(&self, queue: &wgpu::Queue) {
        let ones = vec![1.0f32; N_TILES];
        for b in &self.spec {
            queue.write_buffer(b, (N * 4) as u64, bytemuck::cast_slice(&ones));
        }
    }

    /// Advance the simulation and queue a read-back. Submits its own command buffer.
    pub fn frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mut inp: FrameInput) {
        if let Some(mat) = &inp.reset {
            queue.write_buffer(&self.mat, 0, bytemuck::cast_slice(mat));
            let init = init_state(mat, &inp.params);
            queue.write_buffer(&self.state[0], 0, bytemuck::cast_slice(&init));
            queue.write_buffer(&self.state[1], 0, bytemuck::cast_slice(&init));
            queue.write_buffer(&self.flux, 0, &vec![0u8; N * 4]);
            for b in &self.spec {
                queue.write_buffer(b, 0, &vec![0u8; N * 4]);
            }
            self.cur = 0;
            self.acc_time = 0.0;
            self.wake_all(queue);
        } else if let Some(mat) = &inp.mat {
            queue.write_buffer(&self.mat, 0, bytemuck::cast_slice(mat));
        }
        for &(i, gas) in &inp.dirty {
            let s: [f32; 4] = if gas {
                [inp.params.amb_rho, 0.0, 0.0, inp.params.amb_rho * inp.params.rgas / (inp.params.gamma - 1.0) * inp.params.amb_t]
            } else {
                [0.0, 0.0, 0.0, 300.0]
            };
            for b in &self.state {
                queue.write_buffer(b, (i * 16) as u64, bytemuck::bytes_of(&s));
            }
            for b in &self.spec {
                queue.write_buffer(b, (i * 4) as u64, bytemuck::bytes_of(&0.0f32));
            }
        }
        // painted gas: a fast drag stamps the brush over the same cells many times, so keep the last
        // value per cell and upload runs of neighbouring cells in one write each
        if !inp.paint.is_empty() {
            // gas painted into a cell that this same update turned into a wall is dropped
            let walled: std::collections::HashSet<usize> = inp.dirty.iter().filter(|d| !d.1).map(|d| d.0).collect();
            let mut last = std::collections::HashMap::with_capacity(inp.paint.len());
            for &(i, st, phi) in &inp.paint {
                if !walled.contains(&i) {
                    last.insert(i, (st, phi));
                }
            }
            let mut cells: Vec<(usize, ([f32; 4], f32))> = last.into_iter().collect();
            cells.sort_unstable_by_key(|c| c.0);
            let mut k = 0;
            while k < cells.len() {
                let mut e = k + 1;
                while e < cells.len() && cells[e].0 == cells[e - 1].0 + 1 {
                    e += 1;
                }
                let run_s: Vec<[f32; 4]> = cells[k..e].iter().map(|c| c.1 .0).collect();
                let run_p: Vec<f32> = cells[k..e].iter().map(|c| c.1 .1).collect();
                let i0 = cells[k].0;
                for b in &self.state {
                    queue.write_buffer(b, (i0 * 16) as u64, bytemuck::cast_slice(&run_s));
                }
                for b in &self.spec {
                    queue.write_buffer(b, (i0 * 4) as u64, bytemuck::cast_slice(&run_p));
                }
                k = e;
            }
        }
        // cells the game changed wake their tiles; a settings change wakes everything
        if self.last_params.map_or(true, |lp| bytemuck::bytes_of(&lp) != bytemuck::bytes_of(&inp.params)) || self.no_sleep {
            self.wake_all(queue);
            self.last_params = Some(inp.params);
        } else if !inp.dirty.is_empty() || !inp.paint.is_empty() {
            let tw = (W + 7) / 8;
            let mut tiles: Vec<usize> = inp.dirty.iter().map(|d| d.0).chain(inp.paint.iter().map(|p| p.0)).map(|i| (i / W / 8) * tw + (i % W) / 8).collect();
            tiles.sort_unstable();
            tiles.dedup();
            for t in tiles {
                for b in &self.spec {
                    queue.write_buffer(b, ((N + t) * 4) as u64, bytemuck::bytes_of(&1.0f32));
                }
            }
        }
        inp.srcs.truncate(MAX_SRC);
        inp.params.nsrc = inp.srcs.len() as u32;
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&inp.params));
        queue.write_buffer(&self.net_t, 0, bytemuck::cast_slice(&inp.net_t));
        if !inp.srcs.is_empty() {
            queue.write_buffer(&self.src, 0, bytemuck::cast_slice(&inp.srcs));
        }
        queue.write_buffer(&self.rparams, 0, bytemuck::bytes_of(&inp.render));
        if let Some(l) = &inp.layers {
            queue.write_buffer(&self.layers, 0, bytemuck::cast_slice(l));
        }
        if let Some(ph) = &inp.phase {
            queue.write_buffer(&self.phase, 0, bytemuck::cast_slice(ph));
        }
        if inp.anim.len() == crate::world::ANIM_LEN {
            queue.write_buffer(&self.anim, 0, bytemuck::cast_slice(&inp.anim));
        }

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sim") });
        if inp.steps > 0 {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            let gx = (W as u32 + 7) / 8;
            let gy = (H as u32 + 7) / 8;
            for _ in 0..inp.steps {
                pass.set_bind_group(0, &self.sim_bg[self.cur], &[]);
                pass.set_pipeline(&self.step_pipe);
                pass.dispatch_workgroups(gx, gy, 1);
                if inp.params.nsrc > 0 {
                    pass.set_pipeline(&self.src_pipe);
                    pass.dispatch_workgroups((inp.params.nsrc + 63) / 64, 1, 1);
                }
                self.cur ^= 1;
            }
            drop(pass);
            self.acc_time += inp.steps as f32 * inp.params.dt;
            self.total_steps += inp.steps as u64;
        }
        if inp.steps > 0 && inp.particles {
            self.frame_no = self.frame_no.wrapping_add(1);
            let pp = PartParams {
                w: W as u32,
                h: H as u32,
                count: PARTICLES as u32,
                frame: self.frame_no,
                dtc: inp.steps as f32 * inp.params.dt / inp.params.dx,
                gamma: inp.params.gamma,
                rgas: inp.params.rgas,
                dx: inp.params.dx,
            };
            queue.write_buffer(&self.pp, 0, bytemuck::bytes_of(&pp));
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(&self.adv_pipe);
            pass.set_bind_group(0, &self.adv_bg[self.cur], &[]);
            pass.dispatch_workgroups((PARTICLES as u32 + 63) / 64, 1, 1);
        }
        // flame view: work out each cell's light and glow once (the pixel shader blends them)
        if inp.render.mode == 6 && inp.sim {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("glow"), timestamp_writes: None });
            pass.set_bind_group(0, &self.glow_bg[self.cur], &[]);
            pass.set_pipeline(&self.emit_pipe);
            pass.dispatch_workgroups((N as u32 + 63) / 64, 1, 1);
            pass.set_pipeline(&self.gather_pipe);
            pass.dispatch_workgroups((N as u32 + 63) / 64, 1, 1);
        }
        // read the field back every frame: the injector flows feed back on the chamber pressure it carries,
        // and a reading two frames old makes some engines (the hydrolox gas generator) oscillate
        self.frames_since_readback += 1;
        let start_readback = inp.sim && !self.pending && self.frames_since_readback >= self.readback_every;
        if start_readback {
            self.frames_since_readback = 0;
        }
        if start_readback {
            enc.copy_buffer_to_buffer(&self.state[self.cur], 0, &self.stage_state, 0, (N * 16) as u64);
            enc.copy_buffer_to_buffer(&self.flux, 0, &self.stage_flux, 0, (N * 4) as u64);
            enc.clear_buffer(&self.flux, 0, None);
        }
        queue.submit(Some(enc.finish()));
        if start_readback {
            self.pending = true;
            self.pending_time = self.acc_time;
            self.acc_time = 0.0;
            let a = self.mapped_state.clone();
            self.stage_state.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                if r.is_ok() {
                    a.store(true, Ordering::SeqCst);
                }
            });
            let b = self.mapped_flux.clone();
            self.stage_flux.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                if r.is_ok() {
                    b.store(true, Ordering::SeqCst);
                }
            });
        }
    }

    /// Debugging: how many 8x8 tiles were awake on the last step (blocks until the GPU is done).
    pub fn count_awake(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> (usize, usize, [usize; 4]) {
        let tiles = N_TILES;
        let buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (2 * tiles * 4) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut e = device.create_command_encoder(&Default::default());
        e.copy_buffer_to_buffer(&self.spec[self.cur], (N * 4) as u64, &buf, 0, (2 * tiles * 4) as u64);
        queue.submit(Some(e.finish()));
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let v = buf.slice(..).get_mapped_range().expect("mapped");
        let f: &[f32] = bytemuck::cast_slice(&v);
        let mut rates = [0usize; 4];
        for &r in &f[tiles..] {
            rates[match r as u32 { 0 | 1 => 0, 2 => 1, 4 => 2, _ => 3 }] += 1;
        }
        (f[..tiles].iter().filter(|&&x| x != 0.0).count(), tiles, rates)
    }

    /// Returns the latest completed read-back, if one finished since the last call.
    pub fn take_readback(&mut self, device: &wgpu::Device) -> Option<Readback> {
        let mut rb = Readback::default();
        self.take_readback_into(device, &mut rb).then_some(rb)
    }

    /// The same, copied into `rb`'s buffers (no fresh 10 MB of memory every frame). True if it did.
    pub fn take_readback_into(&mut self, device: &wgpu::Device, rb: &mut Readback) -> bool {
        let _ = device.poll(wgpu::PollType::Poll);
        if !(self.pending && self.mapped_state.load(Ordering::SeqCst) && self.mapped_flux.load(Ordering::SeqCst)) {
            return false;
        }
        {
            let v = self.stage_state.slice(..).get_mapped_range().expect("mapped");
            let src = bytemuck::cast_slice::<u8, [f32; 4]>(&v);
            rb.state.resize(src.len(), [0.0; 4]);
            rb.state.copy_from_slice(src);
        }
        {
            let v = self.stage_flux.slice(..).get_mapped_range().expect("mapped");
            let src = bytemuck::cast_slice::<u8, f32>(&v);
            rb.flux.resize(src.len(), 0.0);
            rb.flux.copy_from_slice(src);
        }
        self.stage_state.unmap();
        self.stage_flux.unmap();
        self.mapped_state.store(false, Ordering::SeqCst);
        self.mapped_flux.store(false, Ordering::SeqCst);
        self.pending = false;
        rb.sim_time = self.pending_time;
        rb.valid = true;
        true
    }

    pub fn paint(&self, pass: &mut wgpu::RenderPass<'static>) {
        pass.set_pipeline(&self.render_pipe);
        pass.set_bind_group(0, &self.render_bg[self.cur], &[]);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.pdraw_pipe);
        pass.set_bind_group(0, &self.pdraw_bg, &[]);
        pass.draw(0..6, 0..PARTICLES as u32);
    }
}

impl FrameInput {
    /// `old` was never handed to the GPU (egui threw that pass away, e.g. when the side panel changed
    /// size): fold its one-off parts (reset, material, painted gas, edited cells) into this frame.
    pub fn absorb(&mut self, old: FrameInput, full_mat: impl Fn() -> Vec<u32>) {
        if old.reset.is_some() && self.reset.is_none() {
            self.reset = Some(self.mat.take().unwrap_or_else(&full_mat));
        } else if old.mat.is_some() && self.reset.is_none() && self.mat.is_none() {
            self.mat = Some(full_mat());
        }
        let mut dirty = old.dirty;
        dirty.append(&mut self.dirty);
        self.dirty = dirty;
        let mut paint = old.paint;
        paint.append(&mut self.paint);
        self.paint = paint;
        if self.phase.is_none() {
            self.phase = old.phase;
        }
        if self.layers.is_none() {
            self.layers = old.layers;
        }
        self.sim |= old.sim;
    }
}

/// egui paint callback: all the heavy lifting happens in `GpuSim::frame`, called in `prepare`.
/// The input slot is shared with the app, so a frame whose callback never runs is merged into the next.
pub struct FieldCallback {
    pub input: std::sync::Arc<Mutex<Option<FrameInput>>>,
}

impl eframe::egui_wgpu::CallbackTrait for FieldCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &eframe::egui_wgpu::ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let (Some(g), Some(inp)) = (res.get_mut::<GpuSim>(), self.input.lock().unwrap().take()) {
            g.frame(device, queue, inp);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &eframe::egui_wgpu::CallbackResources,
    ) {
        if let Some(g) = res.get::<GpuSim>() {
            g.paint(pass);
        }
    }
}
