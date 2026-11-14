//! Active and dormant volcanoes (WORK_QUEUE item 15; owner, 2026-11-13: "Also active volcanoes
//! should've dumped some noisy lava stone in the water around them. And if they're dormant, they
//! should slowly gain dirt and grass, and trees."; plates.json `uplift.volcanism`). On seeds 1, 7,
//! 42, 274005646 and 2741893632, both faces, over the Realistic world's placed volcanoes (arc cones
//! and hotspot islands, satellites left out):
//!
//! - the apron: in the shallows within 40 m of an active volcano's shore, at least [`APRON`] of the
//!   sea floor is lava rock (basalt or obsidian), and round a dormant one at most [`NO_APRON`];
//! - the cover by age: on an active volcano's gentle ground at least [`BARE`] is bare lava rock;
//!   the dormant ones' covered share (not bare rock) rises with age: the old ones' (age ≥ 0.6)
//!   mean at least [`OLD_COVER`] and [`GAIN`] over the young ones' (age ≤ 0.3);
//! - the trees: next to none on an active volcano (at most [`ACTIVE_TREES`] of the old dormant
//!   ones' density, where a neighbouring cone's ground overlaps), the old dormant ones' at least
//!   [`TREE_GAIN`] × the young ones';
//! - no circular boundary: the outermost bare rock along 48 rays round an active volcano varies,
//!   its radii's coefficient of variation at least [`EDGE_CV`].
//! - no black blotches across an old dormant cone's green lowland (0.66, the map check, 2026-11-14):
//!   its bare rock keeps to the shore and the vent (`volcanism.bareShore`, `bareVent`).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 5] = [1, 7, 42, 274_005_646, 2_741_893_632];
const APRON: f64 = 0.6;
const NO_APRON: f64 = 0.2;
const BARE: f64 = 0.75;
const OLD_COVER: f64 = 0.6;
const GAIN: f64 = 0.3;
const TREE_GAIN: f64 = 2.0;
const ACTIVE_TREES: f64 = 0.02;
const EDGE_CV: f64 = 0.05;

fn generator(world: u64, face: dwell_world::Face) -> TerrainGenerator {
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let seed = face_seed(world, face);
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    TerrainGenerator::with_map(seed, map)
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

/// The top block's name at (x, z) (no state), and whether water stands there and how deep.
fn top(g: &TerrainGenerator, x: i32, z: i32) -> (&'static str, Option<f32>) {
    let c = g.column_at(x, z);
    let name = g.ground_y(x, z).map_or("dwell:air", |y| {
        dwell_blocks::info(g.state_at(x, y, z)).name
    });
    let name = name.split('[').next().unwrap_or(name);
    let wet = (c.water != Water::None).then_some(c.level - c.height);
    (name, wet)
}

fn lava(name: &str) -> bool {
    matches!(
        name,
        "dwell:basalt" | "dwell:obsidian" | "dwell:basalt_ramp"
    )
}

#[derive(Default)]
struct Tally {
    apron: [(usize, usize); 2],
    bare_active: (usize, usize),
    cover: Vec<(f64, f64)>,
    trees: Vec<(f64, f64)>,
    active_trees: f64,
    edge_cv: Vec<f64>,
    /// Old dormant cones' lowland (dry, over 20 m above the sea, beyond 0.3 of the radius):
    /// (bare lava columns, columns).
    blotches: (usize, usize),
}

both_faces!(
    fn volcanoes_show_their_state(f: FaceFrame) {
        let mut t = Tally::default();
        for world in SEEDS {
            let g = generator(world, f.face);
            let tect = g.plate_tectonics().expect("plates").clone();
            for (v, s) in tect.volcanoes.iter().zip(&tect.states) {
                if v.chain == 0 && v.crater_radius <= 0.0 || v.radius < 1000.0 {
                    continue; // a satellite, or too small to sample
                }
                // The apron: along 48 rays, the first wet column out from the cone, then 40 m on.
                let mut edge = Vec::new();
                for a in 0..48 {
                    let th = f64::from(a) * std::f64::consts::TAU / 48.0;
                    let at = |r: f64| ((v.x + r * th.cos()) as i32, (v.z + r * th.sin()) as i32);
                    let mut r = 0.2 * v.radius;
                    let mut outer_bare = None;
                    let mut shore = None;
                    while r < 1.6 * v.radius {
                        let (x, z) = at(r);
                        let (name, wet) = top(&g, x, z);
                        if wet.is_none() && lava(name) {
                            outer_bare = Some(r);
                        }
                        if wet.is_some() && shore.is_none() && r > 0.3 * v.radius {
                            shore = Some(r);
                            break;
                        }
                        r += 12.0;
                    }
                    if let Some(r) = outer_bare {
                        edge.push(r);
                    }
                    let Some(r0) = shore else { continue };
                    for k in 0..5 {
                        let (x, z) = at(r0 + f64::from(k) * 10.0);
                        if let (name, Some(depth)) = top(&g, x, z)
                            && depth <= 20.0
                        {
                            let i = usize::from(!s.active);
                            t.apron[i].0 += usize::from(lava(name));
                            t.apron[i].1 += 1;
                        }
                    }
                }
                if s.active && edge.len() >= 12 {
                    let k = edge.len() as f64;
                    let mean = edge.iter().sum::<f64>() / k;
                    let sd = (edge.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / k).sqrt();
                    t.edge_cv.push(sd / mean);
                }
                // The cover: gentle dry ground inside 0.6 of the radius.
                let (mut land, mut bare, mut trees) = (0usize, 0usize, 0.0);
                for j in -12..=12 {
                    for i in -12..=12 {
                        let (dx, dz) = (
                            f64::from(i) * 0.05 * v.radius,
                            f64::from(j) * 0.05 * v.radius,
                        );
                        if dx.hypot(dz) > 0.6 * v.radius {
                            continue;
                        }
                        let (x, z) = ((v.x + dx) as i32, (v.z + dz) as i32);
                        let (name, wet) = top(&g, x, z);
                        if wet.is_some() || name == "dwell:stone" || name == "dwell:snow" {
                            continue; // under water, steep rock or snow
                        }
                        land += 1;
                        bare += usize::from(lava(name));
                        let above = g.column_at(x, z).height - dwell_world::SEA_LEVEL_M as f32;
                        if !s.active
                            && s.age >= 0.6
                            && above > 20.0
                            && dx.hypot(dz) > 0.3 * v.radius
                        {
                            t.blotches.0 += usize::from(lava(name));
                            t.blotches.1 += 1;
                        }
                        trees += g.placement_density_at(x.into(), z.into()).unwrap_or(0.0);
                    }
                }
                if land < 30 {
                    continue;
                }
                let share = bare as f64 / land as f64;
                let density = trees / land as f64;
                if s.active {
                    t.bare_active.0 += bare;
                    t.bare_active.1 += land;
                    t.active_trees = t.active_trees.max(density);
                } else {
                    t.cover.push((s.age, 1.0 - share));
                    t.trees.push((s.age, density));
                }
            }
        }
        let share = |(a, b): (usize, usize)| a as f64 / b.max(1) as f64;
        let mean = |v: &[(f64, f64)], keep: &dyn Fn(f64) -> bool| {
            let k: Vec<f64> = v.iter().filter(|p| keep(p.0)).map(|p| p.1).collect();
            (k.iter().sum::<f64>() / k.len().max(1) as f64, k.len())
        };
        let (young, ny) = mean(&t.cover, &|a| a <= 0.3);
        let (old, no) = mean(&t.cover, &|a| a >= 0.6);
        let (young_trees, _) = mean(&t.trees, &|a| a <= 0.3);
        let (old_trees, _) = mean(&t.trees, &|a| a >= 0.6);
        let report = format!(
            "apron: active {:.2} of {}, dormant {:.2} of {}; active bare {:.2} of {}; cover young {young:.2} ({ny}), old {old:.2} ({no}); trees /ha young {young_trees:.2}, old {old_trees:.2}, active at most {:.3}; edge cv {:?}",
            share(t.apron[0]),
            t.apron[0].1,
            share(t.apron[1]),
            t.apron[1].1,
            share(t.bare_active),
            t.bare_active.1,
            t.active_trees,
            t.edge_cv
        );
        eprintln!(
            "{:?}: {report}; old cones' lowland bare {} of {}",
            f.face, t.blotches.0, t.blotches.1
        );
        // No black blotches across an old cone's green lowland (0.66, the map check: bare rock
        // only near the shore and the vent). 0.65: 6.4 % (B) / 8.8 % (A) of it bare lava; 0.66:
        // 0.7 % / 1.8 % (the rest not yet traced: a satellite cone's own state, an andesite wall).
        assert!(t.blotches.1 >= 30, "{report}: old lowland sampled");
        assert!(
            (t.blotches.0 as f64) <= 0.025 * t.blotches.1 as f64,
            "{:?}: {} of {} old lowland columns bare lava",
            f.face,
            t.blotches.0,
            t.blotches.1
        );
        assert!(t.apron[0].1 >= 40 && share(t.apron[0]) >= APRON, "{report}");
        assert!(
            t.apron[1].1 >= 40 && share(t.apron[1]) <= NO_APRON,
            "{report}"
        );
        assert!(
            t.bare_active.1 >= 30 && share(t.bare_active) >= BARE,
            "{report}"
        );
        assert!(ny >= 1 && no >= 1, "{report}");
        assert!(old >= OLD_COVER && old >= young + GAIN, "{report}");
        assert!(t.active_trees <= ACTIVE_TREES * old_trees, "{report}");
        assert!(
            old_trees >= TREE_GAIN * young_trees && old_trees > 0.0,
            "{report}"
        );
        assert!(
            !t.edge_cv.is_empty() && t.edge_cv.iter().all(|&c| c >= EDGE_CV),
            "{report}"
        );
    }
);
