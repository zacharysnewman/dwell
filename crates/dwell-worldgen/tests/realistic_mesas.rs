//! Relief on the mesas (WORK_QUEUE item 15; owner, 2026-11-13: "I also feel like mesas are too
//! flat on the tops and the bottoms, real mesas have height variety"). rocks.json `mesaRelief`:
//! the erosion's standing tops undulate, tilt and carry knobs; its floors slope to their drainage
//! line with mounds, talus fans and washes; the walls between stay walls. On seeds 1, 7, 42 and the
//! owner's 2741893632, both
//! faces, against the same world with `mesaRelief` off (0.58–0.61's flat tops and floors):
//!
//! - a top's interior: the height spread (rms about its mean, over 96 m patches the flat relief leaves flat) rises by
//!   [`TOP_GAIN`];
//! - a floor's: by [`FLOOR_GAIN`];
//! - at the walls, the share of columns that drop 2 m or more to a neighbour is unchanged within
//!   [`WALL_TOLERANCE`].

use std::sync::Arc;

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::geology::MesaRelief;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
/// The brief's seeds and the owner's (its face B is a third red rock mesa: most of the mesas).
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
/// The tops' and floors' rms gain (m) over the flat 0.61 relief, [min, max].
const TOP_GAIN: [f64; 2] = [0.25, 8.0];
const FLOOR_GAIN: [f64; 2] = [0.3, 6.0];
/// The walls' steep share may move by at most this share of itself.
const WALL_TOLERANCE: f64 = 0.15;
const PATCH: i32 = 96;
const STEP: i32 = 6;
/// A patch the flat relief leaves flatter than this (m rms): a tread's or a floor's interior, not
/// a bed's step.
const FLAT: f64 = 1.0;

fn generator(seed: u64, map: &Arc<WorldMap>, flat: bool) -> TerrainGenerator {
    let mut e = world_config(NAME).unwrap().expect("the Realistic world");
    if flat {
        e.geology.mesa_relief = MesaRelief::NONE;
    }
    TerrainGenerator::with_map(seed, map.clone())
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

/// The rms (m) of `h` about its mean.
fn rms(h: &[f64]) -> f64 {
    let m = h.iter().sum::<f64>() / h.len() as f64;
    (h.iter().map(|v| (v - m).powi(2)).sum::<f64>() / h.len() as f64).sqrt()
}

#[derive(Default, Debug, Clone, Copy)]
struct Census {
    /// (rms sum, patches) for tops and floors, flat then relief.
    top: [(f64, usize); 2],
    floor: [(f64, usize); 2],
    /// (steep, sampled) wall columns, flat then relief.
    wall: [(usize, usize); 2],
}

fn census(world: u64, f: FaceFrame) -> Census {
    let seed = face_seed(world, f.face);
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let wall_m = e.geology.erosion.wall;
    let talus = e.geology.erosion.talus_width;
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    let g = [generator(seed, &map, true), generator(seed, &map, false)];
    let mut c = Census::default();
    for pz in (-44_000..=44_000).step_by(250) {
        for px in (-44_000..=44_000).step_by(250) {
            let Some((s, m0, _)) = g[1].erosion_at(px.into(), pz.into()) else {
                continue;
            };
            if s < 0.9 {
                continue;
            }
            // A patch of one kind: every sample a top's interior, or a floor's.
            let kind = if m0 >= wall_m + 60.0 {
                0
            } else if m0 <= -(talus + 60.0) {
                1
            } else {
                continue;
            };
            let mut pts = Vec::new();
            for dz in (0..PATCH).step_by(STEP as usize) {
                for dx in (0..PATCH).step_by(STEP as usize) {
                    let (x, z) = (px + dx, pz + dz);
                    let Some((_, m, _)) = g[1].erosion_at(x.into(), z.into()) else {
                        continue;
                    };
                    let ok = if kind == 0 {
                        m >= wall_m + 20.0
                    } else {
                        m <= -(talus + 20.0)
                    };
                    let col = g[1].column_at(x, z);
                    if ok && !col.outside && col.water == dwell_worldgen::terrain::Water::None {
                        pts.push((x, z));
                    }
                }
            }
            if pts.len() < 160 {
                continue;
            }
            let spread: Vec<f64> = (0..2)
                .map(|k| {
                    let h: Vec<f64> = pts
                        .iter()
                        .map(|&(x, z)| f64::from(g[k].column_at(x, z).height))
                        .collect();
                    rms(&h)
                })
                .collect();
            if spread[0] >= FLAT {
                continue;
            }
            for (k, &r) in spread.iter().enumerate() {
                let slot = if kind == 0 {
                    &mut c.top[k]
                } else {
                    &mut c.floor[k]
                };
                slot.0 += r;
                slot.1 += 1;
            }
        }
    }
    // The walls: columns in the wall's band, steep where one drops 2 m or more to a neighbour.
    for z in (-44_000..=44_000).step_by(500) {
        for x0 in (-44_000..=44_000).step_by(500) {
            for i in 0..40 {
                let x = x0 + i;
                let Some((s, m, _)) = g[1].erosion_at(x.into(), z.into()) else {
                    break;
                };
                if s < 0.9 || !(0.0..wall_m).contains(&m) {
                    continue;
                }
                for (gk, wall) in g.iter().zip(c.wall.iter_mut()) {
                    let h = gk.column_at(x, z).height;
                    let steep = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|&(dx, dz)| h - gk.column_at(x + dx, z + dz).height >= 2.0);
                    wall.0 += usize::from(steep);
                    wall.1 += 1;
                }
            }
        }
    }
    c
}

both_faces!(
    fn mesa_tops_and_floors_have_relief_and_walls_stay_walls(f: FaceFrame) {
        let mut all = Census::default();
        for world in SEEDS {
            let c = census(world, f);
            let mean = |(s, n): (f64, usize)| s / n.max(1) as f64;
            eprintln!(
                "seed {world} {:?}: tops rms {:.2} -> {:.2} m ({} patches), floors {:.2} -> {:.2} m ({}), walls steep {}/{} -> {}/{}",
                f.face,
                mean(c.top[0]),
                mean(c.top[1]),
                c.top[1].1,
                mean(c.floor[0]),
                mean(c.floor[1]),
                c.floor[1].1,
                c.wall[0].0,
                c.wall[0].1,
                c.wall[1].0,
                c.wall[1].1
            );
            for k in 0..2 {
                all.top[k] = (all.top[k].0 + c.top[k].0, all.top[k].1 + c.top[k].1);
                all.floor[k] = (all.floor[k].0 + c.floor[k].0, all.floor[k].1 + c.floor[k].1);
                all.wall[k] = (all.wall[k].0 + c.wall[k].0, all.wall[k].1 + c.wall[k].1);
            }
        }
        let mean = |(s, n): (f64, usize)| s / n.max(1) as f64;
        assert!(
            all.top[1].1 >= 2,
            "{:?}: mesa tops sampled {}",
            f.face,
            all.top[1].1
        );
        assert!(
            all.floor[1].1 >= 5,
            "{:?}: floors sampled {}",
            f.face,
            all.floor[1].1
        );
        let gain = mean(all.top[1]) - mean(all.top[0]);
        assert!(
            (TOP_GAIN[0]..=TOP_GAIN[1]).contains(&gain),
            "{:?}: tops' rms gain {gain:.2} m",
            f.face
        );
        let gain = mean(all.floor[1]) - mean(all.floor[0]);
        assert!(
            (FLOOR_GAIN[0]..=FLOOR_GAIN[1]).contains(&gain),
            "{:?}: floors' rms gain {gain:.2} m",
            f.face
        );
        let share = |(a, n): (usize, usize)| a as f64 / n.max(1) as f64;
        let (was, is) = (share(all.wall[0]), share(all.wall[1]));
        assert!(
            all.wall[1].1 > 150,
            "{:?}: wall columns {}",
            f.face,
            all.wall[1].1
        );
        assert!(
            (is - was).abs() <= WALL_TOLERANCE * was,
            "{:?}: walls steep {was:.3} -> {is:.3}",
            f.face
        );
    }
);
