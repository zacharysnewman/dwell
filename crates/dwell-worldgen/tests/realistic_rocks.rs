//! The Realistic world's rocks from plate history (docs/proposals/REALISTIC_WORLD.md R5,
//! `shared/worldgen/rocks.json` `plateProvinces`): for seed fixtures on both faces, the arcs' cones
//! sit on the arc's volcanic stack, the hotspot islands on the shield's, the collision boundaries on
//! the core's, the overriding plate's strip by a trench on the accreted shelf and the plates' quiet
//! interiors on the basin's sandstone, as the generator's rock provinces read them; every rule's
//! stack shows today's blocks (§7 q5). The default and starter worlds never read the rules
//! (`tests/golden.rs`, `tests/starter_world.rs`).

use std::sync::Arc;

use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::geology::{GEOLOGY, PlateSetting};
use dwell_worldgen::plate_tectonics::PlateTectonics;
use dwell_worldgen::plates::{BoundaryKind, PLATES};
use dwell_worldgen::terrain::{Band, TerrainGenerator};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 3] = [1, 7, 42];

fn generator(seed: u64) -> TerrainGenerator {
    let e = world_config(NAME)
        .unwrap()
        .expect("the Realistic world's edit");
    let map = WorldMap::for_height(seed, e.height.expect("its height field"))
        .with_worldgen(NAME)
        .unwrap();
    TerrainGenerator::with_map(seed, Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

fn rule(s: PlateSetting) -> &'static str {
    GEOLOGY
        .plate_provinces
        .rules
        .iter()
        .find(|r| r.setting == s)
        .unwrap_or_else(|| panic!("a rule for {}", s.name()))
        .stack
}

/// Points `off` m from each segment's middle of a polyline, on `plate`'s side (either side for
/// None), inside the disc's land reach.
fn beside(t: &PlateTectonics, pl: &[[f64; 2]], plate: Option<usize>, off: f64) -> Vec<[f64; 2]> {
    let keep = PLATES.reach - PLATES.sea_margin;
    let mut out = Vec::new();
    for w in pl.windows(2) {
        let (p, q) = (w[0], w[1]);
        let len = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2))
            .sqrt()
            .max(1e-9);
        let n = [-(q[1] - p[1]) / len, (q[0] - p[0]) / len];
        let mid = [0.5 * (p[0] + q[0]), 0.5 * (p[1] + q[1])];
        for s in [1.0, -1.0] {
            let c = [mid[0] + s * off * n[0], mid[1] + s * off * n[1]];
            let on = plate.is_none_or(|i| t.layout.plate_at(c[0], c[1]) == i);
            if on && c[0] * c[0] + c[1] * c[1] < keep * keep {
                out.push(c);
                break;
            }
        }
    }
    out
}

dwell_world::both_faces!(
    /// The arcs' cones on the arc's stack, the hotspot islands on the shield's, the collisions on
    /// the core's, the accreted strips on the shelf's and the interiors on the basin's.
    fn each_plate_setting_lays_its_stack(f: FaceFrame) {
        let pp = &GEOLOGY.plate_provinces;
        let mut counts = [0usize; 5];
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let g = generator(seed);
            let t = PlateTectonics::for_seed(seed);
            let at = |p: [f64; 2]| g.province_at(p[0] as i64, p[1] as i64);
            for v in &t.volcanoes {
                let (want, k) = if v.chain == 0 {
                    (rule(PlateSetting::Arc), 0)
                } else if pp.settings(&t, v.x, v.z).has(PlateSetting::Arc) {
                    // A hotspot island under an arc's cone takes the arc's (the first rule).
                    continue;
                } else {
                    (rule(PlateSetting::Hotspot), 1)
                };
                assert_eq!(
                    at([v.x, v.z]),
                    want,
                    "seed {world}: cone at {}, {}",
                    v.x,
                    v.z
                );
                counts[k] += 1;
            }
            // A point of a setting takes its rule's stack where that rule wins (the first rule
            // whose setting holds: an arc's cone or another boundary's setting may come first).
            let mut check = |p: [f64; 2], s: PlateSetting, k: usize| {
                let winner = pp.rule(pp.settings(&t, p[0], p[1])).map(|r| r.setting);
                assert!(
                    pp.settings(&t, p[0], p[1]).has(s),
                    "seed {world}: {} holds at {p:?}",
                    s.name()
                );
                if winner == Some(s) {
                    assert_eq!(at(p), rule(s), "seed {world}: {} at {p:?}", s.name());
                    counts[k] += 1;
                }
            };
            for line in &t.lines {
                for pl in &line.polylines {
                    match (line.kind, line.over) {
                        (BoundaryKind::Convergent, None) => {
                            for p in beside(&t, pl, None, 0.5 * pp.core_width) {
                                check(p, PlateSetting::Core, 2);
                            }
                        }
                        (BoundaryKind::Convergent, Some(over)) => {
                            for p in beside(&t, pl, Some(over), 0.5 * pp.accreted_width) {
                                check(p, PlateSetting::Accreted, 3);
                            }
                        }
                        _ => {}
                    }
                }
            }
            // The interiors: every 1.5 km where no boundary of the plate is near.
            let r = (PLATES.reach - PLATES.sea_margin) as i64;
            for z in (-r..=r).step_by(1500) {
                for x in (-r..=r).step_by(1500) {
                    let (xf, zf) = (x as f64, z as f64);
                    let s = pp.settings(&t, xf, zf);
                    let higher = [
                        PlateSetting::Arc,
                        PlateSetting::Hotspot,
                        PlateSetting::Accreted,
                        PlateSetting::Core,
                        PlateSetting::Rift,
                    ];
                    if s.has(PlateSetting::Basin) && !higher.iter().any(|&h| s.has(h)) {
                        assert_eq!(at([xf, zf]), rule(PlateSetting::Basin), "seed {world}");
                        counts[4] += 1;
                    }
                }
            }
        }
        // Each setting is met somewhere in the seeds.
        let names = ["arc", "hotspot", "collision core", "accreted", "basin"];
        for (n, c) in names.iter().zip(counts) {
            assert!(c > 0, "no {n} province met in seeds {SEEDS:?}: {counts:?}");
        }
    }
);

dwell_world::both_faces!(
    /// Away from every setting with a rule the noise's provinces still pick, as before R5.
    fn the_settings_without_a_rule_keep_the_noises_provinces(f: FaceFrame) {
        let pp = &GEOLOGY.plate_provinces;
        assert!(
            !pp.rules.iter().any(|r| r.setting == PlateSetting::Default),
            "the build's rules leave the rest to the noise"
        );
        let seed = face_seed(SEEDS[0], f.face);
        let g = generator(seed);
        let t = PlateTectonics::for_seed(seed);
        let ruled: Vec<&str> = pp.rules.iter().map(|r| r.stack).collect();
        let mut seen = 0;
        for i in 0..200i64 {
            let (x, z) = (i * 211 - 21_000, (i * 7919) % 42_000 - 21_000);
            if pp.rule(pp.settings(&t, x as f64, z as f64)).is_none() {
                let s = g.province_at(x, z);
                let k = GEOLOGY.stack(s).unwrap();
                assert!(GEOLOGY.stacks[k].weight > 0.0, "{s} is a noise stack");
                seen += usize::from(!ruled.contains(&s));
            }
        }
        assert!(seen > 0, "the noise's provinces are met");
    }
);

dwell_world::both_faces!(
    /// BUGS.md #133 (owner, 2026-11-13: "We need a red brown sand for the red desert areas"): the
    /// red desert's floors (the ground between the walls where a red rock shapes it: the
    /// sandstone's treads in the basins' red beds, the badlands and the mesas, the soft beds'
    /// talus) are red sand, its walls red sandstone (rocks.json sandstone `cap`). Before 0.59.2
    /// the sandstone's treads showed the sandstone, and on seed 42 the nearest red sand lay 23 km
    /// from its mesas.
    fn the_red_desert_floors_are_red_sand(f: FaceFrame) {
        let sand = ["dwell:red_sand", "dwell:red_sand_ramp"];
        let mut floors = 0;
        for world in SEEDS {
            let g = generator(face_seed(world, f.face));
            let r = g.world().radius() as i32;
            let mut wrong = Vec::new();
            for x in (-r..=r).step_by(250) {
                for z in (-r..=r).step_by(250) {
                    let col = g.column_at(x, z);
                    // A floor: a rock's ground (a wall is the rock band).
                    if col.outside
                        || col.wet()
                        || !g.is_rock_palette(&col)
                        || col.band != Band::Ground
                    {
                        continue;
                    }
                    let Some(y) = g.ground_y(x, z) else { continue };
                    let top = dwell_blocks::info(g.state_at(x, y, z)).name;
                    let block = top.split('[').next().unwrap_or(top);
                    if block.starts_with("dwell:red_") {
                        floors += 1;
                        if !sand.contains(&block) {
                            wrong.push((x, z, block));
                        }
                    }
                }
            }
            assert!(
                wrong.is_empty(),
                "seed {world}: {} floors not red sand: {:?}",
                wrong.len(),
                &wrong[..wrong.len().min(8)]
            );
        }
        assert!(floors >= 200, "only {floors} red desert floors sampled");
    }
);
