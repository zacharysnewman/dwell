//! The GPU marcher against the CPU reference, per pixel (docs/RAY_MARCHED_RENDERER.md §5
//! "Correctness under both"): small crafted scenes — chunks and LOD sections, both world faces,
//! near the origin and 4,000 km out — rendered offscreen at 64² from several camera poses, the id,
//! view depth and per-pixel debug records read back, and every ray marched again on the CPU with
//! the direction the GPU used. Ids must match exactly and depths within 1e-4 relative; the report
//! also counts how many rays agree on every field (steps, level, cell, face, `t` to the bit).
//!
//! Shared by the native test (`march/test.rs`, on the machine's GPU when it has one) and the
//! browser probe (`probeMarch`, headless SwiftShader in CI).

use dwell_blocks::{StateId, require};
use dwell_march::layout::GRID_OFFSET;
use dwell_march::{BuildOptions, NodeBuild, Outcome, Ray, build_node};
use dwell_world::MIRROR_SUM;

use super::stats::{DEBUG_WORDS, DebugRay, HIST_WORDS};
use super::{MarchFrame, Marcher, wgsl};
use crate::render::camera::{aim, vertical_fov};

pub const SIZE: u32 = 64;
/// How far a hit's `t` may be from the reference's, in ULPs (see `Report::t_inexact`).
pub const T_ULPS: u32 = 8;

/// What one comparison found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub views: usize,
    pub pixels: usize,
    pub hits: usize,
    /// Pixels whose id differs from the reference's.
    pub id_mismatches: usize,
    /// The largest relative difference of view depth where both hit.
    pub max_depth_error: f64,
    /// Pixels where any field of the march differs (outcome, steps, level, cell, face, state;
    /// `t` beyond `T_ULPS`).
    pub record_mismatches: usize,
    /// Hits whose `t` differs from the reference's at all, and the most ULPs it differs by: a
    /// GPU's `1 / d` need not be correctly rounded (WGSL allows 2.5 ULP; Metal's is not), which
    /// moves `t` by an ULP or so but never the cell, the face or the step count.
    pub t_inexact: usize,
    pub t_ulps_max: u32,
    /// The largest difference between the GPU's ray direction and the CPU's for the same pixel.
    pub max_dir_error: f32,
    /// Levels hit, as a bit set (the views reach LOD sections).
    pub levels_hit: u32,
    /// One line per view: its name and counts.
    pub lines: Vec<String>,
    /// The first few mismatches, described.
    pub examples: Vec<String>,
}

impl Report {
    /// The bar: ids exact, depth within 1e-4 relative, directions as the CPU makes them.
    pub fn passes(&self) -> bool {
        self.id_mismatches == 0
            && self.record_mismatches == 0
            && self.max_depth_error <= 1e-4
            && self.max_dir_error < 1e-5
            && self.hits > self.pixels / 4
    }
}

fn hash(x: i64, z: i64, seed: i64) -> i64 {
    let mut h = (x.wrapping_mul(0x27d4_eb2f) ^ z.wrapping_mul(0x1656_67b1) ^ seed) as u64;
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    (h & 0xffff) as i64
}

/// Terrain height (cells of the level) at a column, rolling with steps and pillars.
fn height(x: i64, z: i64) -> i64 {
    let base = 18 + ((x / 7 + z / 5).rem_euclid(9)) - ((x / 11 - z / 13).rem_euclid(7));
    let pillar = if hash(x / 4, z / 4, 3) % 23 == 0 {
        9
    } else {
        0
    };
    base + pillar
}

struct Ids {
    stone: StateId,
    grass: StateId,
    dirt: StateId,
    sand: StateId,
    water: StateId,
    ladder: StateId,
}

fn ids() -> Ids {
    Ids {
        stone: require("dwell:stone"),
        grass: require("dwell:grass"),
        dirt: require("dwell:dirt"),
        sand: require("dwell:sand"),
        water: require("dwell:water"),
        ladder: require("dwell:ladder"),
    }
}

/// A face-local column's state at face-local height `y` (cells of its level).
fn column_state(ids: &Ids, x: i64, y: i64, z: i64) -> StateId {
    let h = height(x, z);
    if y < h {
        // Caves.
        if y > 4 && y < h - 3 && hash(x / 3 + y, z / 3 - y, 7) % 5 == 0 {
            return 0;
        }
        if y == h - 1 {
            if h < 15 { ids.sand } else { ids.grass }
        } else if y > h - 4 {
            ids.dirt
        } else {
            ids.stone
        }
    } else if y < 15 {
        ids.water
    } else if y == h && hash(x, z, 11) % 31 == 0 {
        ids.ladder
    } else {
        0
    }
}

/// Fills `nx × ny × nz` nodes of `level` from grid cell `origin` (that level's cells) with the
/// terrain; on face B the columns hang below the midplane (face-local height `MIRROR_SUM − y`).
fn terrain(m: &mut Marcher, level: u32, origin: [i64; 3], dims: [i64; 3], face_b: bool) {
    let ids = ids();
    let opts = if level == 0 {
        BuildOptions::CHUNK
    } else {
        BuildOptions::SECTION
    };
    let table = m.gpu.scene.table().clone();
    for nz in 0..dims[2] {
        for ny in 0..dims[1] {
            for nx in 0..dims[0] {
                let n0 = [
                    origin[0] + 32 * nx,
                    origin[1] + 32 * ny,
                    origin[2] + 32 * nz,
                ];
                let build = build_node(
                    |x, y, z| {
                        let g = [
                            n0[0] + i64::from(x),
                            n0[1] + i64::from(y),
                            n0[2] + i64::from(z),
                        ];
                        // World metres of the cell's bottom, then face-local height in cells.
                        let wy = (g[1] << level) - GRID_OFFSET;
                        let local = if face_b {
                            (i64::from(MIRROR_SUM) - wy) >> level
                        } else {
                            wy >> level
                        };
                        column_state(&ids, g[0], local, g[2])
                    },
                    &table,
                    opts,
                );
                let n = n0.map(|v| (v >> 5) as i32);
                let placed = m.gpu.put(level, n, &build);
                debug_assert!(placed || matches!(build, NodeBuild::Air));
            }
        }
    }
}

/// A view: a name, the eye (world metres), yaw and pitch (degrees), the face.
type View = (&'static str, [f64; 3], f64, f64, f64);

fn frame_for(eye: [f64; 3], yaw: f64, pitch: f64, face: f64) -> MarchFrame {
    let b = aim(yaw, pitch, face, 0.0);
    let tan_y = (vertical_fov(1.0).to_radians() / 2.0).tan();
    let f3 = |v: [f64; 3]| v.map(|x| x as f32);
    MarchFrame {
        uniforms: vec![0.0; crate::render::FRAME_FLOATS],
        eye,
        forward: f3(b.forward()),
        right: f3(b.right),
        up: f3(b.up),
        tan: [tan_y as f32, tan_y as f32],
    }
}

/// The scenes and their views: (scene builder, views).
type Scene = (&'static str, fn(&mut Marcher), Vec<View>);

fn grid(world: i64) -> i64 {
    world + GRID_OFFSET
}

fn scenes() -> Vec<Scene> {
    const FAR: i64 = 4_000_000;
    let near_chunks: fn(&mut Marcher) =
        |m| terrain(m, 0, [grid(0), grid(0), grid(0)], [3, 2, 3], false);
    let far_chunks: fn(&mut Marcher) =
        |m| terrain(m, 0, [grid(FAR), grid(0), grid(-96)], [3, 2, 3], false);
    let face_b: fn(&mut Marcher) = |m| {
        let top = i64::from(MIRROR_SUM) + 1 - 64;
        terrain(m, 0, [grid(0), grid(top), grid(0)], [3, 2, 3], true)
    };
    let sections: fn(&mut Marcher) = |m| {
        terrain(m, 0, [grid(0), grid(0), grid(0)], [2, 2, 2], false);
        for level in 1..=3u32 {
            // 4 × 2 × 4 sections around the origin at each level, in that level's cells.
            let at = |w: i64| (grid(w) >> level) & !31;
            let span = 64i64 << level;
            terrain(m, level, [at(-span), at(0), at(-span)], [4, 2, 4], false);
        }
    };
    let ladder_eye = [40.3, 44.7, 40.9];
    vec![
        (
            "chunks",
            near_chunks,
            vec![
                ("down", ladder_eye, 30.0, -70.0, 1.0),
                ("oblique", [10.2, 38.4, 12.7], 45.0, -25.0, 1.0),
                ("horizon", [5.5, 30.25, 48.0], 90.0, -4.0, 1.0),
                ("outside", [-40.0, 120.0, -35.0], 45.0, -40.0, 1.0),
                ("inside", [50.5, 8.5, 50.5], 10.0, 5.0, 1.0),
            ],
        ),
        (
            "far chunks",
            far_chunks,
            vec![
                ("down", [FAR as f64 + 40.3, 44.7, -55.1], 30.0, -70.0, 1.0),
                (
                    "oblique",
                    [FAR as f64 + 10.2, 38.4, -83.3],
                    45.0,
                    -25.0,
                    1.0,
                ),
            ],
        ),
        (
            "face B",
            face_b,
            vec![
                (
                    "down (world up)",
                    [40.3, f64::from(MIRROR_SUM) - 44.7, 40.9],
                    30.0,
                    -70.0,
                    -1.0,
                ),
                (
                    "oblique",
                    [10.2, f64::from(MIRROR_SUM) - 38.4, 12.7],
                    45.0,
                    -25.0,
                    -1.0,
                ),
            ],
        ),
        (
            "sections",
            sections,
            vec![
                ("low", [20.5, 40.0, 20.5], 60.0, -12.0, 1.0),
                ("high", [30.0, 700.0, 30.0], 200.0, -35.0, 1.0),
                ("horizon", [12.0, 45.0, 8.0], 120.0, -3.0, 1.0),
            ],
        ),
    ]
}

fn depth_of(hit: &dwell_march::Hit, ray: &Ray, forward: [f32; 3]) -> f64 {
    let p = hit.point(ray);
    (0..3)
        .map(|k| f64::from(p[k]) * f64::from(forward[k]))
        .sum()
}

/// Renders every view and compares it with the reference. `read` reads a texture or buffer back
/// (async: the browser maps asynchronously; native tests poll).
pub async fn run(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Report, String> {
    let mut report = Report::default();
    let pixels = (SIZE * SIZE) as usize;
    for (scene_name, build, views) in scenes() {
        let mut m = Marcher::new(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba8Unorm,
        );
        m.settings.histogram_every = 0;
        m.settings.max_t = 4000.0;
        m.set_debug(pixels);
        build(&mut m);
        let targets = m.targets((SIZE, SIZE));
        for (view, eye, yaw, pitch, face) in views {
            let frame = frame_for(eye, yaw, pitch, face);
            let mut encoder = device.create_command_encoder(&Default::default());
            m.draw(&mut encoder, &frame, &targets, None);
            queue.submit([encoder.finish()]);
            let ids = crate::read_back(device, queue, &targets.id.0).await?;
            let depths = crate::read_back(device, queue, &targets.depth.0).await?;
            let debug = read_buffer(
                device,
                queue,
                m.stats_buffer_ref(),
                HIST_WORDS as u64 * 4,
                (pixels * DEBUG_WORDS) as u64 * 4,
            )
            .await?;
            let word =
                |b: &[u8], i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
            let params = m.reference_params(&frame, (SIZE, SIZE));
            let origin = Ray::from_world(eye, frame.forward);
            let (mut hits, mut id_bad, mut rec_bad) = (0, 0, 0);
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let i = (y * SIZE + x) as usize;
                    let g = DebugRay::from_words(&debug[i * DEBUG_WORDS..(i + 1) * DEBUG_WORDS]);
                    let want_dir = frame.ray(x, y, (SIZE, SIZE));
                    for (a, b) in g.dir.iter().zip(want_dir) {
                        report.max_dir_error = report.max_dir_error.max((a - b).abs());
                    }
                    // The reference, with the GPU's own direction (not normalised again).
                    let ray = Ray {
                        cell: origin.cell,
                        frac: origin.frac,
                        dir: g.dir,
                    };
                    let r = m.gpu.scene.march(&ray, &params);
                    let (kind, want_id) = match r.outcome {
                        Outcome::Hit(h) => (1, wgsl::cell_id(h.level, h.cell, h.face)),
                        Outcome::Miss => (0, 0),
                        Outcome::Cap => (2, 0),
                    };
                    let got_id = word(&ids, i);
                    let mut same = g.kind == kind && g.steps == r.steps;
                    if let Outcome::Hit(h) = r.outcome {
                        hits += 1;
                        report.levels_hit |= 1 << h.level;
                        let ulps = g.t.to_bits().abs_diff(h.t.to_bits());
                        if ulps != 0 {
                            report.t_inexact += 1;
                            report.t_ulps_max = report.t_ulps_max.max(ulps);
                        }
                        same &= g.level == h.level
                            && g.cell == h.cell
                            && g.face == h.face.map_or(wgsl::FACE_NONE, |f| f as u32)
                            && g.t.to_bits().abs_diff(h.t.to_bits()) <= T_ULPS
                            && g.state == u32::from(h.state);
                        let d = f64::from(f32::from_bits(word(&depths, i)));
                        let want = depth_of(&h, &ray, frame.forward);
                        let err = (d - want).abs() / want.abs().max(1e-3);
                        report.max_depth_error = report.max_depth_error.max(err);
                    }
                    if got_id != want_id {
                        id_bad += 1;
                    }
                    if !same {
                        rec_bad += 1;
                        if report.examples.len() < 8 {
                            report.examples.push(format!(
                                "{scene_name}/{view} ({x},{y}): gpu {g:?} cpu {r:?}"
                            ));
                        }
                    }
                }
            }
            report.views += 1;
            report.pixels += pixels;
            report.hits += hits;
            report.id_mismatches += id_bad;
            report.record_mismatches += rec_bad;
            report.lines.push(format!(
                "{scene_name}/{view}: {hits} hits, {id_bad} ids differ, {rec_bad} records differ"
            ));
        }
    }
    Ok(report)
}

/// Copies `bytes` of a buffer from `offset` back to the CPU as words.
pub async fn read_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    offset: u64,
    bytes: u64,
) -> Result<Vec<u32>, String> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, offset, &readback, 0, bytes);
    queue.submit([encoder.finish()]);
    let (tx, rx) = futures_channel::oneshot::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.await
        .map_err(|_| "readback dropped".to_string())?
        .map_err(|e| format!("readback failed: {e}"))?;
    let words = bytemuck::cast_slice(
        &readback
            .get_mapped_range(..)
            .map_err(|e| format!("readback range: {e}"))?,
    )
    .to_vec();
    readback.destroy();
    Ok(words)
}
