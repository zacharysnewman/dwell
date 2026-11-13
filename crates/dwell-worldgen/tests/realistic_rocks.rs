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
use dwell_worldgen::geology::{GEOLOGY, Part, PlateSetting};
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

/// The stacks a point may take: each stack (its own and its `mix`) of the winning rule at every
/// shift of its boundary distances the mixing's wandering edges reach (`plateProvinces.mixing`
/// `edgeShift`, WORK_QUEUE item 15).
fn allowed(t: &PlateTectonics, p: [f64; 2]) -> Vec<&'static str> {
    let pp = &GEOLOGY.plate_provinces;
    let e = 1.3 * pp.mixing.edge_shift;
    let mut out = Vec::new();
    let mut shift = -e;
    loop {
        let (s, _) = pp.settings_shifted(t, p[0], p[1], shift);
        if let Some(r) = pp.rule(s) {
            out.push(r.stack);
            out.extend(r.mix.iter().map(|m| m.stack));
        }
        if shift >= e {
            break;
        }
        shift = (shift + 100.0).min(e);
    }
    out
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
                    let ok = allowed(&t, p);
                    assert!(
                        ok.contains(&at(p)),
                        "seed {world}: {} at {p:?}: {} not of {ok:?}",
                        s.name(),
                        at(p)
                    );
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
                        let ok = allowed(&t, [xf, zf]);
                        assert!(
                            ok.contains(&at([xf, zf])),
                            "seed {world}: basin at {x}, {z}"
                        );
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
    /// WORK_QUEUE item 15 (part B): the build's rules end with a `default` rule, so every column of
    /// a Realistic world takes one of the rules' stacks (its own or its `mix`) and the 12 km noise's
    /// provinces, one stack over hundreds of km², never pick; the interior shows several stacks.
    fn every_column_takes_a_rules_stack(f: FaceFrame) {
        let pp = &GEOLOGY.plate_provinces;
        assert!(
            pp.rules.iter().any(|r| r.setting == PlateSetting::Default),
            "the build's rules cover the rest"
        );
        let seed = face_seed(SEEDS[0], f.face);
        let g = generator(seed);
        let t = PlateTectonics::for_seed(seed);
        let mut seen: Vec<&str> = Vec::new();
        for i in 0..400i64 {
            let (x, z) = (i * 107 - 21_000, (i * 7919) % 42_000 - 21_000);
            let s = g.province_at(x, z);
            let ok = allowed(&t, [x as f64, z as f64]);
            assert!(ok.contains(&s), "{s} at {x}, {z} is a rule's ({ok:?})");
            if !seen.contains(&s) {
                seen.push(s);
            }
        }
        assert!(seen.len() >= 5, "the provinces met: {seen:?}");
    }
);

dwell_world::both_faces!(
    /// BUGS.md #134 (owner, 2026-11-13: "We need a red brown sand for the red desert areas"): the
    /// red desert's floors — the sandstone's flat treads (the basins' red beds, the badlands, the
    /// mesas, their tops too) where the ground is arid (rocks.json `arid`, the biomes' climate
    /// values) — are red sand; elsewhere, forests included, they stay red sandstone (sandstone's
    /// `capBy`). Its walls are red sandstone everywhere. Before 0.60 every tread was red sandstone
    /// and on seed 42 the nearest red sand lay 23 km from its mesas.
    fn the_red_desert_floors_are_red_sand(f: FaceFrame) {
        let sand = ["dwell:red_sand", "dwell:red_sand_ramp"];
        let stone = ["dwell:red_sandstone", "dwell:red_sandstone_ramp"];
        let forest =
            |b: &str| b.contains("forest") || b.contains("boreal") || b.contains("old_growth");
        let (mut arid, mut humid, mut forests) = (0, 0, 0);
        for world in SEEDS {
            let g = generator(face_seed(world, f.face));
            let r = g.world().radius() as i32;
            let mut wrong = Vec::new();
            let mut sampled = 0usize;
            for x in (-r..=r).step_by(250) {
                for z in (-r..=r).step_by(250) {
                    let col = g.column_at(x, z);
                    // A floor: a rock's ground (a wall is the rock band), flat to its neighbours.
                    if col.outside
                        || col.wet()
                        || !g.is_rock_palette(&col)
                        || col.band != Band::Ground
                    {
                        continue;
                    }
                    let (rock, part) = g.rock_at_surface(i64::from(x), i64::from(z));
                    if rock.name != "dwell:sandstone" || part != Part::Tread {
                        continue;
                    }
                    let Some(y) = g.ground_y(x, z) else { continue };
                    let flat = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .all(|(dx, dz)| g.ground_y(x + dx, z + dz) == Some(y));
                    let top = dwell_blocks::info(g.state_at(x, y, z)).name;
                    let block = top.split('[').next().unwrap_or(top);
                    if !flat || !(sand.contains(&block) || stone.contains(&block)) {
                        continue;
                    }
                    let dry = g.arid_at(i64::from(x), i64::from(z)).expect("a geography");
                    sampled += 1;
                    let biome = g.biomes()
                        [usize::from(g.biome_mix(i64::from(x), i64::from(z)).chosen)]
                    .name;
                    if dry {
                        arid += 1;
                    } else {
                        humid += 1;
                    }
                    let in_forest = forest(biome);
                    forests += usize::from(in_forest);
                    let want_sand = dry && !in_forest;
                    if sand.contains(&block) != want_sand {
                        wrong.push((x, z, block, dry, biome));
                    }
                }
            }
            // A floor on the arid rule's very edge may read its climate a little off the column's
            // (the climate's district noise, climate.json `variety`): at most one in 200.
            assert!(
                wrong.len() * 200 <= sampled.max(200),
                "seed {world}: {} floors wrong (x, z, top, arid, biome): {:?}",
                wrong.len(),
                &wrong[..wrong.len().min(8)]
            );
        }
        println!(
            "{:?}: arid {arid}, humid {humid}, forest {forests} floors",
            f.face
        );
        assert!(
            arid >= 50 && humid >= 50 && forests >= 10,
            "arid {arid}, humid {humid}, forest {forests} floors sampled"
        );
    }
);
