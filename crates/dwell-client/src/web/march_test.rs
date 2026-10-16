//! The marcher's dev page (`?marchtest=1`, local builds only; Phase 25 A3b): a generated world
//! drawn by the ray marcher alone, without the game (the game draws it with `?renderer=march` or
//! `compare`). The job workers generate the chunks around the spawn and the LOD sections of every
//! level out to a few hundred kilometres, then build their bricks (the meshing jobs with the
//! `BRICKS` payload only); the main thread places them in the marcher's GPU pool as they arrive. The camera is set from JS
//! (`window.__march`, client/src/debug/marchTest.ts).

use std::collections::{HashMap, VecDeque};

use js_sys::Function;
use wasm_bindgen::prelude::*;

use dwell_march::NodeBuild;
use dwell_world::lod::{self, LodCoord, LodKind};
use dwell_world::{ChunkCoord, GENERATOR_TERRAIN};

use super::jobs::WebJobs;
use crate::jobs::{Job, Output};
use crate::pool::Jobs;
use crate::render::Renderer;
use crate::render::march::Marcher;
use crate::render::seam::Payloads;
use crate::render::timing::{self, GpuTimer};
use crate::telemetry;

/// What a job's result is for.
#[derive(Clone, Copy, Debug)]
enum For {
    Chunk(ChunkCoord),
    ChunkBricks(ChunkCoord),
    Bounds(i32, i32, i32),
    Section(LodCoord),
    SectionBricks(LodCoord),
}

#[wasm_bindgen]
pub struct MarchTest {
    _instance: wgpu::Instance,
    _adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    marcher: Marcher,
    timer: Option<GpuTimer>,
    targets: Option<crate::render::march::MarchTargets>,
    jobs: WebJobs,
    queued: [VecDeque<(Job, For)>; 2],
    running: HashMap<u32, For>,
    in_flight: [usize; 2],
    next: u32,
    spawn: [f64; 3],
    chunks: usize,
    sections: usize,
}

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
impl MarchTest {
    /// Opens the page's marcher on `canvas`, with job workers made by `make_worker`, and starts
    /// generating world `seed` around its spawn: chunks `chunk_radius` around it (and 3 rows up
    /// and down), sections `section_radius` around it on levels 1..=`top_level`.
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        make_worker: Function,
        seed: u64,
        workers: usize,
        chunk_radius: i32,
        section_radius: i32,
        top_level: i32,
    ) -> Result<MarchTest, JsError> {
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(err)?;
        let (adapter, device, queue) = crate::request_device(&instance, Some(&surface))
            .await
            .map_err(|e| JsError::new(&e))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            web_sys::console::error_1(&format!("Dwell march: WebGPU error: {e}").into());
        }));
        let mut renderer = Renderer::with_samples(device.clone(), queue.clone(), format, 1);
        renderer.resize(width, height, 1.0);
        let marcher = Marcher::new(device.clone(), queue.clone(), format);
        let timer = GpuTimer::new(&device, &queue);
        let jobs = WebJobs::new(
            &make_worker,
            workers.max(1),
            workers.max(1),
            (GENERATOR_TERRAIN, seed),
        )
        .map_err(|e| JsError::new(&format!("{e:?}")))?;
        let spawn = dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, seed);
        let mut t = MarchTest {
            _instance: instance,
            _adapter: adapter,
            surface,
            config,
            renderer,
            marcher,
            timer,
            targets: None,
            jobs,
            queued: Default::default(),
            running: HashMap::new(),
            in_flight: [0; 2],
            next: 1,
            spawn,
            chunks: 0,
            sections: 0,
        };
        // Chunks: nearest first.
        let centre = dwell_world::chunk_of(spawn[0] as i32, spawn[1] as i32, spawn[2] as i32);
        let mut cs = Vec::new();
        for dz in -chunk_radius..=chunk_radius {
            for dy in -3..=3 {
                for dx in -chunk_radius..=chunk_radius {
                    cs.push(ChunkCoord::new(centre.x + dx, centre.y + dy, centre.z + dz));
                }
            }
        }
        cs.sort_by_key(|c| {
            (c.x - centre.x).pow(2) + (4 * (c.y - centre.y)).pow(2) + (c.z - centre.z).pow(2)
        });
        for c in cs {
            t.queue(Job::Generate { coord: c }, For::Chunk(c));
        }
        // Sections: the bounds of each column first.
        for level in 1..=top_level.min(lod::MAX_LEVEL) {
            let size = lod::section_size(level) as f64;
            let at = |v: f64, o: i64| ((v - o as f64) / size).floor() as i32;
            let (ci, ck) = (at(spawn[0], lod::ORIGIN_X), at(spawn[2], lod::ORIGIN_Z));
            for k in ck - section_radius..=ck + section_radius {
                for i in ci - section_radius..=ci + section_radius {
                    t.queue(Job::LodBounds { level, i, k }, For::Bounds(level, i, k));
                }
            }
        }
        Ok(t)
    }

    fn queue(&mut self, job: Job, purpose: For) {
        self.queued[usize::from(job.is_mesh())].push_back((job, purpose));
    }

    fn pump(&mut self) {
        for kind in 0..2 {
            while self.in_flight[kind] < self.jobs.capacity(kind == 1) {
                let Some((job, purpose)) = self.queued[kind].pop_front() else {
                    break;
                };
                let id = self.next;
                self.next += 1;
                self.running.insert(id, purpose);
                self.in_flight[kind] += 1;
                self.jobs.submit(id, job);
            }
        }
    }

    fn place(&mut self, purpose: For, out: Output) {
        match (purpose, out) {
            (For::Chunk(c), Output::Chunk { voxels, .. }) => {
                if voxels.iter().all(|&v| v == 0) {
                    self.marcher.set_chunk(c, Some(&NodeBuild::Air));
                } else {
                    // The bricks take the interior: the apron stays air.
                    let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
                    for z in 0..32 {
                        for y in 0..32 {
                            for x in 0..32 {
                                padded[dwell_mesh::padded_index(x, y, z)] =
                                    voxels[dwell_world::local_index(x, y, z)];
                            }
                        }
                    }
                    let job = Job::MeshChunk {
                        padded,
                        mirror: false,
                        payloads: Payloads::BRICKS,
                    };
                    self.queue(job, For::ChunkBricks(c));
                }
            }
            (For::ChunkBricks(c), Output::ChunkMesh(t)) => {
                let b = t.march.map_or(NodeBuild::Air, |b| b.0);
                self.marcher.set_chunk(c, Some(&b));
                self.chunks += 1;
            }
            (For::Bounds(level, i, k), Output::Bounds(b)) => {
                for j in lod::first_row(level)..=lod::last_row(level) {
                    let c = LodCoord::new(level, i, j, k);
                    if !lod::in_world(c) {
                        continue;
                    }
                    match lod::kind_from_bounds(c, &b) {
                        LodKind::Empty => self.marcher.set_section(c, Some(&NodeBuild::Air)),
                        LodKind::Buried => self
                            .marcher
                            .set_section(c, Some(&NodeBuild::Solid(dwell_world::M.stone))),
                        LodKind::Content => self.queue(Job::Lod { coord: c }, For::Section(c)),
                    }
                }
            }
            (For::Section(c), Output::Lod { cells, .. }) => {
                let job = Job::MeshSection {
                    cells,
                    surface: None,
                    water_drop: 0.0,
                    mirror: false,
                    payloads: Payloads::BRICKS,
                };
                self.queue(job, For::SectionBricks(c));
            }
            (For::SectionBricks(c), Output::SectionMesh(t)) => {
                let b = t.march.map_or(NodeBuild::Air, |b| b.0);
                self.marcher.set_section(c, Some(&b));
                self.sections += 1;
            }
            (p, Output::Failed(e)) => {
                web_sys::console::warn_1(&format!("march test: {p:?} failed: {e}").into());
            }
            (p, _) => web_sys::console::warn_1(&format!("march test: {p:?}: wrong output").into()),
        }
    }

    /// The spawn point (world metres): `[x, y, z]`.
    pub fn spawn(&self) -> Vec<f64> {
        self.spawn.to_vec()
    }

    /// Jobs not done yet (queued and running).
    pub fn pending(&self) -> usize {
        self.queued.iter().map(VecDeque::len).sum::<usize>() + self.running.len()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) != (self.config.width, self.config.height) {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(self.renderer.device(), &self.config);
            self.targets = None;
        }
        self.renderer.resize(w, h, 1.0);
    }

    /// Places the camera (world metres; yaw 0 = +Z, degrees).
    #[wasm_bindgen(js_name = setCamera)]
    pub fn set_camera(&mut self, x: f64, y: f64, z: f64, yaw: f64, pitch: f64) {
        let face = if y >= f64::from(dwell_world::MIDPLANE_Y) {
            1.0
        } else {
            -1.0
        };
        self.renderer.set_camera([x, y, z], yaw, pitch, face, 0.0);
    }

    /// Sets the march's step cap, LOD pixel error and skipping switches.
    pub fn configure(&mut self, step_cap: u32, lod_pixels: f32, columns: bool, distances: bool) {
        let s = &mut self.marcher.settings;
        s.step_cap = step_cap;
        s.lod_pixels = lod_pixels;
        s.columns = columns;
        s.distances = distances;
    }

    /// One frame: collect finished jobs, start more, march and present.
    pub fn frame(&mut self, now_ms: f64) {
        telemetry::with(|t| t.begin_frame(now_ms));
        let t0 = telemetry::now_ms();
        for (id, out) in self.jobs.poll() {
            if let Some(p) = self.running.remove(&id) {
                let kind = usize::from(matches!(p, For::ChunkBricks(_) | For::SectionBricks(_)));
                self.in_flight[kind] = self.in_flight[kind].saturating_sub(1);
                self.place(p, out);
            }
        }
        self.pump();
        self.marcher.collect();
        if let Some(timer) = &mut self.timer {
            for (name, ms) in timer.collect() {
                telemetry::sample(name, ms);
            }
        }
        let frame_tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };
        // Only a frame that will be submitted takes a readback buffer (an early return above
        // would hold it forever).
        if let Some(timer) = &mut self.timer {
            timer.begin();
        }
        let q_march = self.timer.as_mut().and_then(|t| t.pass(timing::MARCH));
        let q_composite = self.timer.as_mut().and_then(|t| t.pass(timing::COMPOSITE));
        let size = (self.config.width, self.config.height);
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            self.targets = Some(self.marcher.targets(size));
        }
        let targets = self.targets.as_ref().expect("targets");
        let frame = self.renderer.march_frame();
        let device = self.renderer.device().clone();
        let mut encoder = device.create_command_encoder(&Default::default());
        let t1 = telemetry::now_ms();
        self.marcher.draw(
            &mut encoder,
            &frame,
            targets,
            self.timer
                .as_ref()
                .and_then(|t| t.writes(q_march))
                .map(|w| wgpu::ComputePassTimestampWrites {
                    query_set: w.query_set,
                    beginning_of_pass_write_index: w.beginning_of_pass_write_index,
                    end_of_pass_write_index: w.end_of_pass_write_index,
                }),
        );
        let view = frame_tex.texture.create_view(&Default::default());
        self.marcher.composite(
            &mut encoder,
            targets,
            &view,
            self.timer.as_ref().and_then(|t| t.writes(q_composite)),
        );
        if let Some(timer) = &self.timer {
            timer.resolve(&mut encoder);
        }
        self.renderer.queue().submit([encoder.finish()]);
        self.renderer.queue().present(frame_tex);
        self.marcher.submitted();
        if let Some(timer) = &mut self.timer {
            timer.submitted();
        }
        telemetry::add("draw.march", telemetry::now_ms() - t1);
        telemetry::add("frame.cpu", telemetry::now_ms() - t0);
    }

    /// What the page shows and the screenshots record: progress, memory and the GPU's numbers,
    /// as JSON.
    pub fn stats(&self) -> String {
        let s = &self.marcher;
        let steps = s.last_steps.map_or("null".to_string(), |st| {
            format!(
                "{{\"rays\":{},\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{},\"mean\":{:.2},\"capped\":{:.5},\"hits\":{:.4}}}",
                st.rays, st.median, st.p95, st.p99, st.max, st.mean, st.capped_share, st.hit_share
            )
        });
        let gpu = telemetry::with(|t| {
            t.sample_stats()
                .into_iter()
                .filter(|(n, _)| n.starts_with("gpu."))
                .map(|(n, st)| {
                    format!(
                        "\"{n}\":{{\"n\":{},\"p50\":{:.3},\"p95\":{:.3},\"mean\":{:.3}}}",
                        st.n, st.p50, st.p95, st.mean
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        });
        format!(
            "{{\"pending\":{},\"chunks\":{},\"sections\":{},\"nodes\":{},\"poolMiB\":{:.2},\"gpuMiB\":{:.2},\"refused\":{},\"steps\":{steps},\"gpu\":{{{gpu}}}}}",
            self.pending(),
            self.chunks,
            self.sections,
            s.gpu.scene.live_nodes(),
            s.gpu.scene.pool_live_words() as f64 * 4.0 / 1048576.0,
            s.gpu_bytes() as f64 / 1048576.0,
            s.gpu.refused,
        )
    }

    /// Forgets the telemetry so far.
    #[wasm_bindgen(js_name = telemetryReset)]
    pub fn telemetry_reset(&self) {
        telemetry::with(telemetry::Telemetry::reset);
    }
}
