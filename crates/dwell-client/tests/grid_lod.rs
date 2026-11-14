//! A grid's own LOD tree (`dwell_client::grid_lod`, BUGS.md #156): on the moon and the sun, both
//! faces, home's `LodSystem` over the grid's disc with its sections from the grid's generator
//! (`Job::GridLod`, run here inline by the job runner) selects sections of several levels outward
//! to the horizon, coarser with distance; the drawn sections neither leave gaps nor overlap where
//! levels meet; their tops take the colour of the block the chunks show there (the block looks'
//! LOD colour, not the far bake's albedo the rings had); and the cost is bounded.

use std::collections::HashMap;

use dwell_client::grid_lod::{GridJobs, GridLod, GridView, Host};
use dwell_client::jobs::{Job, Output, Runner};
use dwell_lod::fade::Fade;
use dwell_lod::{
    ChunkReadiness, GeneratedSection, LodCamera, LodMeshJob, LodOptions, LodView, StandInBox,
};
use dwell_mesh::SectionMeshes;
use dwell_world::FaceFrame;
use dwell_world::lod::LodCoord;
use dwell_worldgen::grid_lod::GridColumns;

/// Jobs queued by the LOD, run after its update.
#[derive(Default)]
struct Queue {
    bounds: Vec<(u8, i32, i32, i32)>,
    sections: Vec<(u8, LodCoord, u64)>,
    meshes: Vec<(LodMeshJob, u64)>,
    run: usize,
}

impl GridJobs for Queue {
    fn bounds(&mut self, grid: u8, _: u64, level: i32, i: i32, k: i32) {
        self.bounds.push((grid, level, i, k));
    }
    fn section(&mut self, grid: u8, _: u64, c: LodCoord, token: u64) {
        self.sections.push((grid, c, token));
    }
    fn mesh(&mut self, _: u64, job: LodMeshJob, token: u64) {
        self.meshes.push((job, token));
    }
}

/// The renderer: each section held (origin, cell, meshes) and the last frame's shown ids.
#[derive(Default)]
struct Recorder {
    held: HashMap<i64, ([f64; 3], f64, SectionMeshes)>,
    shown: Vec<i64>,
}

impl LodView<SectionMeshes> for Recorder {
    fn set_lod_section(&mut self, id: i64, o: [f64; 3], cell: f64, m: Option<SectionMeshes>) {
        match m {
            Some(m) => {
                self.held.insert(id, (o, cell, m));
            }
            None => {
                self.held.remove(&id);
            }
        }
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8, Fade)]) {
        self.shown = visible.iter().map(|v| v.0).collect();
    }
    fn show_lod_stand_ins(&mut self, _: &[StandInBox]) {}
}

/// No chunk is streamed: level 1 is the finest the LOD draws.
struct NoChunks;
impl ChunkReadiness for NoChunks {
    fn chunk_drawable(&self, _: dwell_world::ChunkCoord) -> bool {
        false
    }
}

/// A drawn section: its box (lo, hi), level, cell size (m) and meshes.
type Drawn<'a> = ([f64; 3], [f64; 3], i64, f64, &'a SectionMeshes);

struct Run {
    lod: GridLod,
    view: Recorder,
    runner: Runner,
    queue: Queue,
}

const SEED: u64 = 7;
/// The High preset's Detail memory (shared/settings/graphics-presets.json): the budget that bounds
/// a grid's LOD as it does home's.
const BUDGET: usize = 192 << 20;

fn options() -> LodOptions {
    LodOptions::new(3.0, BUDGET, 128, 128)
}

impl Run {
    fn new(grid: u8) -> Self {
        Self {
            lod: GridLod::for_grid(grid, 1, options(), Some(192.0)).expect("the grid's LOD"),
            view: Recorder::default(),
            runner: Runner::world(0, SEED),
            queue: Queue::default(),
        }
    }

    /// Frames until nothing is left to do round `camera` (at most `frames`).
    fn settle(&mut self, camera: &LodCamera, frames: usize) {
        let grid = self.lod.grid();
        for frame in 0..frames {
            let now = frame as f64 * 16.0;
            {
                let mut host = Host {
                    grid,
                    epoch: 1,
                    jobs: &mut self.queue,
                    view: GridView(&mut self.view),
                    chunks: &NoChunks,
                };
                self.lod.lod.update(camera, now, &mut host);
            }
            let (b, s, m) = (
                std::mem::take(&mut self.queue.bounds),
                std::mem::take(&mut self.queue.sections),
                std::mem::take(&mut self.queue.meshes),
            );
            if b.is_empty() && s.is_empty() && m.is_empty() && frame > 4 {
                return;
            }
            self.queue.run += b.len() + s.len() + m.len();
            for (g, level, i, k) in b {
                let out = self.runner.run(Job::GridLodBounds {
                    grid: g,
                    level,
                    i,
                    k,
                });
                let Output::Bounds(bounds) = out else {
                    panic!("bounds: {out:?}")
                };
                self.lod.lod.on_lod_bounds(level, i, k, Some(bounds));
            }
            for (g, c, token) in s {
                let Output::Lod {
                    cells,
                    surface,
                    surface_b,
                    tints,
                } = self.runner.run(Job::GridLod { grid: g, coord: c })
                else {
                    panic!("a section")
                };
                self.lod.lod.on_generated(
                    token,
                    Some(GeneratedSection {
                        cells,
                        surface,
                        surface_b,
                        tints,
                    }),
                );
            }
            for (job, token) in m {
                let out = self.runner.run(Job::MeshSection {
                    cells: job.cells,
                    surface: job.surface,
                    water_drop: job.water_drop,
                    mirror: job.mirror,
                    tints: job.tints,
                    trees: false,
                });
                let Output::SectionMesh(mesh) = out else {
                    panic!("a mesh")
                };
                self.lod
                    .lod
                    .on_meshed(token, *mesh, &mut GridView(&mut self.view));
            }
        }
        panic!("the LOD never settled in {frames} frames");
    }

    /// The drawn sections: (box lo, box hi, level, cell, meshes).
    fn drawn(&self) -> Vec<Drawn<'_>> {
        self.view
            .shown
            .iter()
            .map(|id| {
                let (o, cell, m) = self.view.held.get(id).expect("a shown section is held");
                let s = 32.0 * cell;
                let hi = [o[0] + s, o[1] + s * dwell_world::CELL_H, o[2] + s];
                (*o, hi, cell.log2().round() as i64, *cell, m)
            })
            .collect()
    }
}

fn camera(eye: [f64; 3], yaw_deg: f64, pitch_deg: f64) -> LodCamera {
    LodCamera {
        position: eye,
        yaw_deg,
        pitch_deg,
        fov_y_deg: 70.0,
        aspect: 16.0 / 9.0,
        height_px: 1080.0,
        magnification: 1.0,
        detail_magnification: 1.0,
    }
}

fn grid_index(name: &str) -> u8 {
    dwell_worldgen::grids::grids()
        .iter()
        .position(|g| g.name == name)
        .unwrap() as u8
}

/// The block the chunks show on top at face-local (x, z): the lava where it stands over the
/// ground, else the ground's.
fn chunk_top(src: &dyn GridColumns, f: FaceFrame, x: f64, z: f64) -> Option<u16> {
    let c = src.lod_column(f.face, x, z, 1.0)?;
    Some(if c.fluid_row().is_some() {
        c.fluid
    } else {
        c.ground
    })
}

fn linear(c: u32) -> [f64; 3] {
    let l = |v: u32| {
        let s = f64::from(v & 0xff) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [l(c >> 16), l(c >> 8), l(c)]
}

/// `name` from face-local (x, z), 3 m over its ground on face `f`, looking out two opposite ways.
fn grid_lod_checks(name: &str, src: &dyn GridColumns, f: FaceFrame, x: f64, z: f64) {
    let g = grid_index(name);
    let r = src.radius_m();
    let up = src.lod_column(f.face, x, z, 1.0).unwrap();
    let eye_y = up.ground_m.max(up.fluid_m) + 3.0;
    let eye = f.point([x, eye_y, z]);
    let mut matched = (0usize, 0usize);
    let mut albedo_matched = 0usize;
    for yaw in [0.0f64, 180.0] {
        let mut run = Run::new(g);
        // Level with the horizon: the face's pitch 0.
        run.settle(&camera(eye, yaw, f.pitch(-2.0)), 600);
        let drawn = run.drawn();
        let stats = run.lod.lod.debug_stats();
        let levels: std::collections::BTreeSet<i64> = drawn.iter().map(|d| d.2).collect();
        eprintln!(
            "{name} face {:?} yaw {yaw}: drawn per level {:?}, {} nodes, {} jobs, {} MB",
            f.face,
            stats.drawn,
            stats.nodes,
            run.queue.run,
            stats.cache_bytes >> 20
        );
        assert!(
            levels.len() >= 4,
            "{name} face {:?} yaw {yaw}: graded levels drawn {levels:?}",
            f.face
        );
        // Cost: within twice home's in this harness at the same settings (its spawn: 2,477 drawn,
        // 6,979 nodes; the budget's time-based coarsening, which bounds both in game, is not run
        // here), the other face culled away from the rim (`set_rim_view`: home's 16 km reach
        // spans the moon, which drew both faces: 5,334 drawn).
        assert!(
            drawn.len() <= 5000 && stats.nodes <= 14_000,
            "{name}: {} drawn, {} nodes, {} jobs",
            drawn.len(),
            stats.nodes,
            run.queue.run
        );
        // No two drawn sections overlap (levels hand over with no doubled ground).
        for (a, da) in drawn.iter().enumerate() {
            for db in &drawn[a + 1..] {
                let overlap = (0..3).all(|k| da.0[k] < db.1[k] - 1e-6 && db.0[k] < da.1[k] - 1e-6);
                assert!(
                    !overlap,
                    "{name}: sections overlap: {da:?} {db:?}",
                    da = (da.0, da.2),
                    db = (db.0, db.2)
                );
            }
        }
        // Along the view, every ground point to the horizon lies in a drawn section (no gaps), the
        // level never finer farther out, and the last ones near the rim.
        let (dx, dz) = (yaw.to_radians().sin(), yaw.to_radians().cos());
        let b = x * dx + z * dz;
        let rim = -b + (b * b - (x * x + z * z - r * r)).sqrt();
        let mut last_level = -1;
        let mut d = 40.0;
        let mut reached = 0.0;
        while d < rim - 2.0 {
            let (px, pz) = (x + dx * d, z + dz * d);
            let Some(col) = src.lod_column(f.face, px, pz, 1.0) else {
                break;
            };
            let top = f.point([px, col.ground_m.max(col.fluid_m), pz]);
            let covering: Vec<_> = drawn
                .iter()
                .filter(|s| {
                    // The ground lies within a section, or on its top or bottom face.
                    s.0[0] <= top[0]
                        && top[0] <= s.1[0]
                        && s.0[2] <= top[2]
                        && top[2] <= s.1[2]
                        && s.0[1] - 0.5 <= top[1]
                        && top[1] <= s.1[1] + 0.5
                })
                .collect();
            assert!(
                !covering.is_empty(),
                "{name} face {:?} yaw {yaw}: a gap {d:.0} m out (rim {rim:.0} m)",
                f.face
            );
            let level = covering.iter().map(|s| s.2).max().unwrap();
            assert!(
                level + 1 >= last_level,
                "{name}: finer farther out ({last_level} then {level} at {d:.0} m)"
            );
            last_level = last_level.max(level);
            reached = d;
            d *= 1.15;
        }
        assert!(
            reached > 0.8 * rim,
            "{name}: the drawn ground reaches {reached:.0} m of {rim:.0} m to the rim"
        );
        // The tops' colours: the LOD colour of the block the chunks show at that column.
        let tint = dwell_look::face_tint(1, 1);
        for s in &drawn {
            let m = &s.4.opaque;
            for t in m.indices.chunks(3).step_by(5) {
                let v = t[0] as usize;
                let n = [0, 1, 2].map(|k| f64::from(m.normals[v * 3 + k]));
                if f.vector(n)[1] < 0.95 {
                    continue; // a riser or a slope: its colour mixes the side bands
                }
                // The triangle's cell (in cells from the origin) and the column at its centre:
                // the one the section sampled for it.
                let mid = |k: usize| {
                    t.iter()
                        .map(|&i| f64::from(m.positions[i as usize * 3 + k]))
                        .sum::<f64>()
                        / 3.0
                };
                let wx = s.0[0] + (mid(0).floor() + 0.5) * s.3;
                let wz = s.0[2] + (mid(2).floor() + 0.5) * s.3;
                let Some(top) = chunk_top(src, f, wx, wz) else {
                    continue;
                };
                let got = [0, 1, 2].map(|k| f64::from(m.colors[v * 3 + k]) / tint[k]);
                let close = |want: [f64; 3]| {
                    (0..3).all(|k| (got[k] - want[k]).abs() <= 0.12 * want[k].max(0.02) + 0.01)
                };
                matched.1 += 1;
                matched.0 += usize::from(close(linear(dwell_mesh::lod_color(top, 0))));
                // What the rings wrote (BUGS.md #156): the far bake's albedo, the block's `color`,
                // in sRGB straight into the linear colour stream.
                let c = dwell_blocks::info(top).color;
                let ring = [c >> 16, c >> 8, c].map(|v| f64::from(v & 0xff) / 255.0);
                albedo_matched += usize::from(close(ring));
            }
        }
    }
    assert!(
        matched.1 > 200,
        "{name}: top vertices sampled ({})",
        matched.1
    );
    assert!(
        matched.0 * 100 >= matched.1 * 90,
        "{name} face {:?}: {} of {} tops in the chunks' block look",
        f.face,
        matched.0,
        matched.1
    );
    eprintln!(
        "{name} face {:?}: tops matching the block look {}/{}, the rings' colour {}/{}",
        f.face, matched.0, matched.1, albedo_matched, matched.1
    );
    assert!(
        albedo_matched * 2 < matched.1,
        "{name}: the rings' colours would match too ({albedo_matched} of {})",
        matched.1
    );
}

dwell_world::both_faces!(
    /// BUGS.md #156: on the moon its distance steps down in detail gradually to its horizon, in
    /// its chunks' colours.
    fn the_moon_s_lod_is_graded_gapless_and_in_its_blocks_colours(f: FaceFrame) {
        let t = dwell_worldgen::grids::CrateredTerrain::for_grid("dwell:moon", SEED).unwrap();
        let [x, _, z] = t.spawn_point(f.face);
        grid_lod_checks("dwell:moon", &t, f, x, z);
    }
);

dwell_world::both_faces!(
    /// The same on the sun (its lava and crust).
    fn the_sun_s_lod_is_graded_gapless_and_in_its_blocks_colours(f: FaceFrame) {
        let t = dwell_worldgen::volcanic::VolcanicTerrain::for_grid("dwell:sun", SEED).unwrap();
        let [x, _, z] = t.spawn_point(f.face);
        grid_lod_checks("dwell:sun", &t, f, x, z);
    }
);

/// A grid LOD's sections reach the renderer under the grid's ids, apart from home's.
// both-faces: ids only; no face enters.
#[test]
fn a_grid_s_section_ids_are_apart_from_home_s() {
    let home =
        dwell_lod::grid::lod_id(16, 0, 0, 0).max(dwell_lod::grid::lod_id(0, 65_535, 0, 65_535));
    assert!(home < dwell_client::grid_lod::ID_BIT);
    assert!(
        GridLod::for_grid(0, 1, options(), None).is_none(),
        "home has its own"
    );
}
