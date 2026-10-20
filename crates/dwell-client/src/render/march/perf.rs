//! The marcher's GPU cost on this machine, natively (Phase 25 A4): generated terrain around a
//! spawn (chunks and LOD sections, as the game streams them), marched at 1280 × 720 from the
//! bench's views, the compute pass timed with timestamp queries. A profiling aid, not a check:
//! `cargo test --release -p dwell-client march_perf -- --ignored --nocapture`, with
//! `DWELL_MARCHDBG=<bits>` to switch parts of the per-step work off (`wgsl::DBG_*`) and
//! `DWELL_PERF_SEED`, `DWELL_PERF_SIZE=WxH` to change the scene and size.
//!
//! Naga's Metal output is not Tint's (Chrome's), so the absolute numbers differ from the bench's;
//! the bench (`npm run bench -- --compare`) is the measurement, this the quick loop.

use dwell_march::{NodeBuild, build_chunk, build_section, chunk_node};
use dwell_world::lod::{self, LodCoord, LodKind};
use dwell_world::{Chunk, ChunkCoord, GENERATOR_TERRAIN, M};
use dwell_worldgen::{
    air_test_for, generator_for, lod_bounds_for, lod_generator_for, spawn_point_for,
};

use super::test::{block_on, real_device};
use super::{MarchFrame, Marcher};
use crate::render::camera::{aim, vertical_fov};

const CHUNK_RADIUS: i32 = 8;
const CHUNK_ROWS: i32 = 3;
const SECTION_RADIUS: i32 = 6;
const TOP_LEVEL: i32 = 12;

fn fill(m: &mut Marcher, seed: u64, eye: [f64; 3]) {
    let version = GENERATOR_TERRAIN;
    let table = m.gpu.scene.table().clone();
    let gen_lod = lod_generator_for(version, seed);
    let bounds = lod_bounds_for(version, seed);
    let mut cells = Vec::new();
    for level in 1..=TOP_LEVEL {
        let size = lod::section_size(level);
        let at = |v: f64, o: i64| ((v - o as f64) / size as f64).floor() as i32;
        let (ci, ck) = (at(eye[0], lod::ORIGIN_X), at(eye[2], lod::ORIGIN_Z));
        for k in ck - SECTION_RADIUS..=ck + SECTION_RADIUS {
            for i in ci - SECTION_RADIUS..=ci + SECTION_RADIUS {
                let b = bounds(level, i, k);
                for j in lod::first_row(level)..=lod::last_row(level) {
                    let c = LodCoord::new(level, i, j, k);
                    if !lod::in_world(c) {
                        continue;
                    }
                    let n = [i, j, k];
                    let build = match lod::kind_from_bounds(c, &b) {
                        LodKind::Empty => NodeBuild::Air,
                        LodKind::Buried => NodeBuild::Solid(M.stone),
                        LodKind::Content => {
                            gen_lod(c, &mut cells);
                            build_section(&cells, &table)
                        }
                    };
                    m.gpu.put(level as u32, n, &build);
                }
            }
        }
    }
    let generate = generator_for(version, seed);
    let is_air = air_test_for(version, seed);
    let centre = dwell_world::chunk_of(eye[0] as i32, eye[1] as i32, eye[2] as i32);
    for cz in -CHUNK_RADIUS..=CHUNK_RADIUS {
        for cy in -CHUNK_ROWS..=CHUNK_ROWS {
            for cx in -CHUNK_RADIUS..=CHUNK_RADIUS {
                let c = ChunkCoord::new(centre.x + cx, centre.y + cy, centre.z + cz);
                if is_air(c) {
                    m.gpu.put(0, chunk_node(c), &NodeBuild::Air);
                    continue;
                }
                let mut chunk = Chunk::default();
                generate(c, &mut chunk);
                m.gpu
                    .put(0, chunk_node(c), &build_chunk(&chunk.voxels()[..], &table));
            }
        }
    }
}

fn frame_for(eye: [f64; 3], yaw: f64, pitch: f64, size: (u32, u32)) -> MarchFrame {
    let b = aim(yaw, pitch, 1.0, 0.0);
    let aspect = f64::from(size.0) / f64::from(size.1);
    let tan_y = (vertical_fov(aspect).to_radians() / 2.0).tan();
    let f3 = |v: [f64; 3]| v.map(|x| x as f32);
    MarchFrame {
        uniforms: vec![0.0; crate::render::FRAME_FLOATS],
        eye,
        forward: f3(b.forward()),
        right: f3(b.right),
        up: f3(b.up),
        tan: [(tan_y * aspect) as f32, tan_y as f32],
    }
}

#[test]
#[ignore = "a profiling aid: run with --ignored --nocapture on a machine with a GPU"]
fn march_perf() {
    let Some((device, queue, name)) = real_device() else {
        eprintln!("SKIPPED: no GPU adapter");
        return;
    };
    if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        eprintln!("SKIPPED: no timestamp queries on {name}");
        return;
    }
    let env = |k: &str| std::env::var(k).ok();
    let seed: u64 = env("DWELL_PERF_SEED")
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    let dbg: u32 = env("DWELL_MARCHDBG")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if let Some(path) = env("DWELL_PERF_DUMP") {
        std::fs::write(&path, super::wgsl::source()).expect("dump");
        std::fs::write(format!("{path}.beam"), super::wgsl::beam_source()).expect("dump");
    }
    // `name=bits` or `name=@file.wgsl` or `name=@file.wgsl:bits`, separated by `;`.
    let variants: Vec<(String, Option<String>, u32)> = env("DWELL_PERF_VARIANTS")
        .map(|v| {
            v.split(';')
                .filter(|s| !s.is_empty())
                .map(|item| {
                    let (name, spec) = item.split_once('=').expect("name=spec");
                    let (file, bits) = match spec.strip_prefix('@') {
                        Some(rest) => match rest.rsplit_once(':') {
                            Some((f, b)) => (Some(f.to_string()), b.parse().expect("bits")),
                            None => (Some(rest.to_string()), 0),
                        },
                        None => (None, spec.parse().expect("bits")),
                    };
                    (name.to_string(), file, bits)
                })
                .collect()
        })
        .unwrap_or_else(|| vec![("base".into(), None, dbg)]);
    let size: (u32, u32) = env("DWELL_PERF_SIZE")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1280, 720));
    let spawn = spawn_point_for(GENERATOR_TERRAIN, seed);
    let eye = [spawn[0], spawn[1] + 1.6, spawn[2]];
    let mut m = Marcher::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
    );
    m.settings.histogram_every = 1;
    m.settings.debug = dbg;
    let t0 = std::time::Instant::now();
    fill(&mut m, seed, eye);
    eprintln!(
        "{name}: seed {seed}, {} nodes, filled in {:.1} s, {}×{}, dbg {dbg:#x}",
        m.gpu.scene.live_nodes(),
        t0.elapsed().as_secs_f64(),
        size.0,
        size.1
    );
    {
        use dwell_march::layout::{COLUMN_WORDS, GRID_OFFSET, Slot, column_span};
        let sc = &m.gpu.scene;
        for l in 0..dwell_march::layout::LEVEL_COUNT as u32 {
            let Some(info) = sc.level_info(l) else {
                continue;
            };
            let (mut top, mut bot) = (i64::MIN, i64::MAX);
            for z in 0..info.dims[2] as i32 {
                for y in 0..info.dims[1] as i32 {
                    for x in 0..info.dims[0] as i32 {
                        let n = [info.origin[0] + x, info.origin[1] + y, info.origin[2] + z];
                        let (lo, hi) = match sc.slot(l, n) {
                            Slot::AllSolid(_) => (0i64, 32i64),
                            Slot::Node(i) => {
                                let c = sc.node_columns()[i as usize];
                                if c == dwell_march::scene::COLUMNS_NONE {
                                    (0, 32)
                                } else {
                                    let (a, b) = column_span(
                                        &sc.column_words()[c as usize..c as usize + COLUMN_WORDS],
                                        5,
                                        0,
                                        0,
                                    );
                                    if a >= b {
                                        continue;
                                    }
                                    (i64::from(a), i64::from(b))
                                }
                            }
                            _ => continue,
                        };
                        top = top.max(((i64::from(n[1]) * 32 + hi) << l) - GRID_OFFSET);
                        bot = bot.min(((i64::from(n[1]) * 32 + lo) << l) - GRID_OFFSET);
                    }
                }
            }
            eprintln!("  level {l}: occupied y {bot}..{top} m (eye {:.0})", eye[1]);
        }
    }
    let targets = m.targets(size);
    let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 3,
    });
    // The beam prepass's end, too (index 2).
    m.beam_end_query = Some(2);
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 24,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let period = f64::from(queue.get_timestamp_period());
    let high = [eye[0], eye[1] + 540.0, eye[2]];
    let views: [(&str, [f64; 3], f64, f64); 5] = [
        ("spawn (pitch 0)", eye, 90.0, 0.0),
        ("horizon (−3°)", eye, 90.0, -3.0),
        ("down (−80°)", eye, 90.0, -80.0),
        ("high (540 m, −20°)", high, 90.0, -20.0),
        ("high side", high, 0.0, -20.0),
    ];
    // Rounds of every variant in turn: a shared, busy GPU drifts, so compare within a round.
    let moving: f64 = env("DWELL_PERF_MOVE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let rounds: usize = env("DWELL_PERF_ROUNDS")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let variants: Vec<_> = (0..rounds).flat_map(|_| variants.clone()).collect();
    for (variant, file, bits) in &variants {
        m.set_source(&match file {
            Some(f) => std::fs::read_to_string(f).expect("variant source"),
            None => super::wgsl::source(),
        });
        m.settings.debug = *bits;
        // A beam module of the variant's own: `<file>.beam` beside it, if there is one.
        let beam = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(format!("{f}.beam")).ok());
        m.set_beam_source(&beam.unwrap_or_else(super::wgsl::beam_source));
        m.shade = env("DWELL_PERF_NOSHADE").is_none() && !variant.starts_with("noshade");
        m.settings.beam = env("DWELL_PERF_NOBEAM").is_none() && !variant.contains("nobeam");
        m.settings.temporal = !variant.contains("notemporal");
        // `…still…`: the temporal start with the eye still only (A4b).
        m.settings.moving = !variant.contains("still");
        // The harness draws one view again and again: kept frames only when asked (`…reuse…`).
        m.settings.reuse = variant.contains("reuse");
        m.settings.columns = !variant.contains("nocolumns");
        m.settings.distances = !variant.contains("nodistances");
        // `beamonly…`: the beam prepass alone.
        m.march_pass = !variant.starts_with("beamonly");
        m.shade &= m.march_pass;
        // The workgroup a variant's source declares.
        m.workgroup = match file {
            Some(f) => {
                let src = std::fs::read_to_string(f).expect("variant source");
                let at = src.find("@workgroup_size(").expect("a workgroup size") + 16;
                let dims: Vec<u32> = src[at..at + src[at..].find(')').expect(")")]
                    .split(',')
                    .map(|d| d.trim().parse().expect("a size"))
                    .collect();
                [dims[0], dims.get(1).copied().unwrap_or(1)]
            }
            None => [super::wgsl::WORKGROUP; 2],
        };
        eprintln!("variant {variant} (dbg {bits:#x})");
        for (view, at, yaw, pitch) in views {
            let mut frame = frame_for(at, yaw, pitch, size);
            let mut ms = Vec::new();
            let mut beam_ms = Vec::new();
            for i in 0..25 {
                // `DWELL_PERF_MOVE`: the eye moves this far to the right each frame.
                let along = moving * f64::from(i);
                frame = frame_for(
                    std::array::from_fn(|k| at[k] + along * f64::from(frame.right[k])),
                    yaw,
                    pitch,
                    size,
                );
                // The last frame fills the histogram (its atomics are not part of the timing).
                m.settings.histogram_every = u32::from(i == 24);
                let mut e = device.create_command_encoder(&Default::default());
                m.draw(
                    &mut e,
                    &frame,
                    &targets,
                    Some(wgpu::ComputePassTimestampWrites {
                        query_set: &queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                );
                e.resolve_query_set(&queries, 0..3, &resolve, 0);
                queue.submit([e.finish()]);
                m.submitted();
                let words = block_on(
                    &device,
                    super::check::read_buffer(&device, &queue, &resolve, 0, 24),
                )
                .expect("timestamps");
                let t = |i: usize| u64::from(words[2 * i]) | (u64::from(words[2 * i + 1]) << 32);
                if (4..24).contains(&i) {
                    ms.push((t(1).saturating_sub(t(0))) as f64 * period / 1e6);
                    if m.settings.beam && m.march_pass {
                        beam_ms.push((t(2).saturating_sub(t(0))) as f64 * period / 1e6);
                    }
                }
                m.collect();
            }
            ms.sort_by(f64::total_cmp);
            beam_ms.sort_by(f64::total_cmp);
            if env("DWELL_PERF_CHECK").is_some() {
                check_starts(&device, &queue, &mut m, (at, yaw, pitch), &targets, size, moving);
            }
            if let Some(dir) = env("DWELL_PERF_IMAGES") {
                steps_image(
                    &device,
                    &queue,
                    &mut m,
                    &frame,
                    &targets,
                    size,
                    &format!(
                        "{dir}/{variant}-{}.pgm",
                        view.split(' ').next().unwrap_or("v")
                    ),
                );
            }
            let s = m.last_steps;
            eprintln!(
                "  {view:<20} march p50 {:6.2} ms  min {:6.2}  beam {}  steps p50/p95/p99 {}",
                ms[ms.len() / 2],
                ms[0],
                beam_ms
                    .first()
                    .map_or("–".into(), |b| format!("{:.2}/{:.2}", b, beam_ms[beam_ms.len() / 2])),
                s.map_or("–".into(), |s| format!(
                    "{}/{}/{} mean {:.1}",
                    s.median, s.p95, s.p99, s.mean
                )),
            );
        }
    }
}

/// A greyscale image of steps per ray (white = 128 or more), with misses tinted darker.
fn steps_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    m: &mut Marcher,
    frame: &MarchFrame,
    targets: &super::MarchTargets,
    size: (u32, u32),
    path: &str,
) {
    use super::stats::{DEBUG_WORDS, DebugRay, HIST_WORDS};
    let pixels = (size.0 * size.1) as usize;
    m.set_debug(pixels);
    let mut e = device.create_command_encoder(&Default::default());
    m.draw(&mut e, frame, targets, None);
    queue.submit([e.finish()]);
    let words = block_on(
        device,
        super::check::read_buffer(
            device,
            queue,
            m.stats_buffer_ref(),
            HIST_WORDS as u64 * 4,
            (pixels * DEBUG_WORDS) as u64 * 4,
        ),
    )
    .expect("debug records");
    let mut out = format!("P5 {} {} 255\n", size.0, size.1).into_bytes();
    let mut warp_max = 0u64;
    for i in 0..pixels {
        let r = DebugRay::from_words(&words[i * DEBUG_WORDS..(i + 1) * DEBUG_WORDS]);
        out.push((r.steps.min(127) * 2) as u8);
        warp_max += u64::from(r.steps);
    }
    // Steps if every 8 × 4 block of pixels ran as long as its longest ray (a SIMD group).
    let mut groups = 0u64;
    for by in (0..size.1).step_by(4) {
        for bx in (0..size.0).step_by(8) {
            let mut mx = 0;
            for y in by..(by + 4).min(size.1) {
                for x in bx..(bx + 8).min(size.0) {
                    let i = (y * size.0 + x) as usize;
                    mx = mx.max(words[i * DEBUG_WORDS + 1]);
                }
            }
            groups += u64::from(mx) * 32;
        }
    }
    eprintln!(
        "    steps: mean {:.1}, as SIMD groups (8×4, longest ray) {:.1}",
        warp_max as f64 / pixels as f64,
        groups as f64 / pixels as f64
    );
    std::fs::write(path, out).expect("image");
    m.set_debug(0);
}

/// `DWELL_PERF_CHECK`: the starts on generated terrain. The view is drawn after the same view
/// turned by a couple of degrees (the temporal start then applies where it can), every pixel's
/// debug record read back, and every ray marched on the CPU from the eye: a start must never pass
/// that march's hit. Counts too the rays whose outcome from the start differs from the eye's (a
/// LOD switch falling elsewhere).
fn check_starts(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    m: &mut Marcher,
    (at, yaw, pitch): ([f64; 3], f64, f64),
    targets: &super::MarchTargets,
    size: (u32, u32),
    moving: f64,
) {
    use super::stats::{DEBUG_WORDS, DebugRay, HIST_WORDS};
    use dwell_march::{Outcome, Ray};
    let pixels = (size.0 * size.1) as usize;
    m.set_debug(pixels);
    // With `moving`, the frame before comes from an eye that far to the left.
    let fwd = frame_for(at, yaw, pitch, size).right;
    for (y, p, back) in [
        (yaw + 2.0, pitch + 1.0, 1.0),
        (yaw + 2.0, pitch + 1.0, 1.0),
        (yaw, pitch, 0.0),
    ] {
        let eye: [f64; 3] = std::array::from_fn(|k| at[k] - back * moving * f64::from(fwd[k]));
        let frame = frame_for(eye, y, p, size);
        let mut e = device.create_command_encoder(&Default::default());
        m.draw(&mut e, &frame, targets, None);
        queue.submit([e.finish()]);
    }
    let frame = frame_for(at, yaw, pitch, size);
    let words = block_on(
        device,
        super::check::read_buffer(
            device,
            queue,
            m.stats_buffer_ref(),
            HIST_WORDS as u64 * 4,
            (pixels * DEBUG_WORDS) as u64 * 4,
        ),
    )
    .expect("debug records");
    let params = m.reference_params(&frame, size);
    let origin = Ray::from_world(at, frame.forward);
    let (mut past, mut changed, mut temporal, mut differ) = (0, 0, 0, 0);
    for i in 0..pixels {
        let g = DebugRay::from_words(&words[i * DEBUG_WORDS..(i + 1) * DEBUG_WORDS]);
        let ray = Ray {
            cell: origin.cell,
            frac: origin.frac,
            dir: g.dir,
        };
        let full = m.gpu.scene.march(&ray, &params);
        let from = m.gpu.scene.march_from(&ray, &params, g.start);
        temporal += usize::from(g.temporal);
        if let Outcome::Hit(h) = full.outcome
            && g.start > h.t
        {
            past += 1;
        }
        if full.outcome != from.outcome {
            changed += 1;
        }
        if from.steps != g.steps {
            differ += 1;
        }
    }
    eprintln!(
        "    starts: {pixels} rays, {temporal} temporal, {past} past a hit, {changed} changed \
         outcome, {differ} records differ from the GPU's"
    );
    m.set_debug(0);
}
