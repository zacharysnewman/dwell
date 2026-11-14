//! No straight strips, bands or lattices of terrain types from the air (BUGS.md #169; owner,
//! 2026-11-14, flying high over a Realistic world on 0.64.2: "Still seeing harsh bands of terrain
//! types, strips, lines, etc"). The owner's spot (seed 4065579781, face A, x 10472, z −3091) showed
//! a lattice of straight dirt strips a few hundred metres wide: the plate provinces (`rocks.json`
//! `plateProvinces`), each setting a band of one stack at a fixed width along straight plate
//! boundaries (`rift` → `dwell:rift_fill`'s loamy soil). 0.65 curved the boundaries, made the
//! settings' edges wander and mixed stacks inside them.
//!
//! The measure: windows of the surface's top blocks at 2 m over the land of every measured seed and
//! face (and the owner's spot); each boundary cell between two block types gets a line fitted to
//! the boundary cells within 50 m (`support/map_shapes.rs` `straight_share`), straight when they
//! lie within 2 m of it over at least 80 m. A lattice of strips is many such runs.

use std::sync::Arc;

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

#[path = "support/map_shapes.rs"]
mod map_shapes;
use map_shapes::Mask;

const NAME: &str = "test:realistic";
/// The seeds of the map checks, and the owner's world of the report.
const SEEDS: [u64; 6] = [1, 7, 42, 274_005_646, 2_741_893_632, 4_065_579_781];
/// The owner's spot (face A).
const OWNER: (u64, i32, i32) = (4_065_579_781, 10_472, -3_091);
/// Fine windows: `N` × `N` samples `STEP` m apart, a run straight within 2 m over 80 m.
const N: usize = 160;
const STEP: i32 = 2;
/// Coarse windows (the view from high up) of the rock provinces (what painted 0.64's strips):
/// `N` × `N` samples `COARSE` m apart, a run straight within 40 m over 1.3 km.
const COARSE: i32 = 64;
/// At most this share of the fine windows' boundary cells on a straight run (over all windows of a
/// face). Measured 2026-11-14 with the 0.66 surface changes: 3.3 % (A) / 0.8 % (B), nearly all of it
/// smooth, crisp rock-wall edges (stone or red sandstone against grass), not straight strips: the
/// soft edges (fingers over a few metres) are still to come (WORK_QUEUE item 15, 0b iii).
const STRAIGHT: f64 = 0.05;
/// At most this share of the coarse windows' boundary cells on a straight run.
const STRAIGHT_COARSE: f64 = 0.05;

/// The top block's name at (x, z) without its state and ramp suffix ("dwell:air" off the ground).
fn top(g: &TerrainGenerator, x: i32, z: i32) -> &'static str {
    let name = match g.ground_y(x, z) {
        Some(y) => dwell_blocks::info(g.state_at(x, y, z)).name,
        None => "dwell:air",
    };
    let base = name.split('[').next().unwrap_or(name);
    base.strip_suffix("_ramp").unwrap_or(base)
}

/// The share of a window's block-type boundary cells on straight runs, and their count: samples
/// `step` m apart, a run straight within `tolerance` m of a line over 1.6 × `radius` m.
pub fn window(
    g: &TerrainGenerator,
    cx: i32,
    cz: i32,
    step: i32,
    radius: f64,
    tolerance: f64,
) -> (f64, usize) {
    let names: Vec<&str> = (0..N * N)
        .map(|c| {
            let (i, j) = ((c % N) as i32, (c / N) as i32);
            top(
                g,
                cx + (i - N as i32 / 2) * step,
                cz + (j - N as i32 / 2) * step,
            )
        })
        .collect();
    let edge: Vec<bool> = (0..N * N)
        .map(|c| {
            (c % N + 1 < N && names[c + 1] != names[c])
                || (c + N < names.len() && names[c + N] != names[c])
        })
        .collect();
    let mask = Mask {
        n: N,
        cell: f64::from(step),
        on: vec![true; N * N],
    };
    mask.straight_share(&edge, radius, tolerance)
}

/// [`window`] over the rock provinces' stacks (`province_at`) in place of the top blocks.
pub fn province_window(g: &TerrainGenerator, cx: i32, cz: i32, step: i32) -> (f64, usize) {
    let names: Vec<&str> = (0..N * N)
        .map(|c| {
            let (i, j) = ((c % N) as i32, (c / N) as i32);
            let (x, z) = (
                cx + (i - N as i32 / 2) * step,
                cz + (j - N as i32 / 2) * step,
            );
            if g.column_at(x, z).water == dwell_worldgen::terrain::Water::Sea {
                "sea"
            } else {
                g.province_at(x.into(), z.into())
            }
        })
        .collect();
    let edge: Vec<bool> = (0..N * N)
        .map(|c| {
            (c % N + 1 < N && names[c + 1] != names[c])
                || (c + N < names.len() && names[c + N] != names[c])
        })
        .collect();
    let mask = Mask {
        n: N,
        cell: f64::from(step),
        on: vec![true; N * N],
    };
    mask.straight_share(&edge, 800.0, 40.0)
}

fn generator(seed: u64) -> (TerrainGenerator, Arc<WorldMap>) {
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    let g = TerrainGenerator::with_map(seed, map.clone())
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    (g, map)
}

/// Window centres on a face's land: three land cells spread through the map's land, and the
/// owner's spot on its face.
fn centres(world: u64, f: FaceFrame, map: &WorldMap) -> Vec<(i32, i32)> {
    let land: Vec<usize> = (0..map.height.len())
        .filter(|&c| {
            !map.is_sea(c)
                && (f64::from(map.height[c]) - dwell_world::SEA_LEVEL_M) > 20.0
                && map.centre(c).0.hypot(map.centre(c).1) < 40_000.0
        })
        .collect();
    let mut out: Vec<(i32, i32)> = (1..=3)
        .filter_map(|k| land.get(land.len() * k / 4))
        .map(|&c| {
            let (x, z) = map.centre(c);
            (x as i32, z as i32)
        })
        .collect();
    if world == OWNER.0 && f.face == dwell_world::Face::A {
        out.push((OWNER.1, OWNER.2));
    }
    out
}

both_faces!(
    fn terrain_types_meet_along_irregular_edges(f: FaceFrame) {
        let (mut straight, mut all) = (0.0, 0usize);
        let (mut coarse, mut coarse_all) = (0.0, 0usize);
        for world in SEEDS {
            let (g, map) = generator(face_seed(world, f.face));
            // From high up: one coarse window over the land's middle (the owner's spot on its face).
            let middle = centres(world, f, &map)
                .last()
                .copied()
                .filter(|_| world == OWNER.0 && f.face == dwell_world::Face::A)
                .or_else(|| centres(world, f, &map).get(1).copied());
            if let Some((x, z)) = middle {
                let (share, n) = province_window(&g, x, z, COARSE);
                eprintln!(
                    "seed {world} {:?} coarse ({x}, {z}): {n} boundary cells, {:.2} % straight",
                    f.face,
                    100.0 * share
                );
                coarse += share * n as f64;
                coarse_all += n;
            }
            for (x, z) in centres(world, f, &map) {
                let (share, n) = window(&g, x, z, STEP, 50.0, 2.0);
                eprintln!(
                    "seed {world} {:?} ({x}, {z}): {n} boundary cells, {:.2} % straight",
                    f.face,
                    100.0 * share
                );
                straight += share * n as f64;
                all += n;
            }
        }
        let coarse = coarse / coarse_all.max(1) as f64;
        eprintln!(
            "{:?}: coarse {:.2} % of {coarse_all} boundary cells straight",
            f.face,
            100.0 * coarse
        );
        assert!(
            coarse <= STRAIGHT_COARSE,
            "{:?}: {:.2} % of the coarse boundary cells on straight runs (bar {:.0} %)",
            f.face,
            100.0 * coarse,
            100.0 * STRAIGHT_COARSE
        );
        let share = straight / all.max(1) as f64;
        eprintln!(
            "{:?}: {:.2} % of {all} boundary cells straight",
            f.face,
            100.0 * share
        );
        assert!(all > 1000, "enough boundaries sampled ({all})");
        assert!(
            share <= STRAIGHT,
            "{:?}: {:.2} % of the terrain types' boundary cells on straight runs (bar {:.0} %)",
            f.face,
            100.0 * share,
            100.0 * STRAIGHT
        );
    }
);
