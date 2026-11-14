//! Steps by geology in the Realistic world (docs/proposals/REALISTIC_WORLD.md R5, WORK_QUEUE item 15;
//! owner, 2026-11-12: "I like stepped terrain but it should be system driven not across the whole
//! realistic world"). `shared/worldgen/worlds/realistic.json` turns the global `cliffs` volume off
//! (`"cliffs": 0`) and the rock shaping on (`geology.shaping.strength`), so the ground steps where its
//! rock's jointing says: `bedded` stacks into walls and treads every `bed` m, `vertical` rock into
//! columns, `massive` granite not at all. On both faces of a seed fixture, the share of stepped
//! columns (a wall of at least `WALL` m to a neighbour) on massive ground is about none and on
//! bedded ground over a bar. The default world keeps its cliffs (`tests/golden.rs`).

use std::sync::Arc;

use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::geology::Jointing;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEED: u64 = 42;
/// A drop to a neighbouring column at least this tall (m) is a wall: the column is stepped.
const WALL: f32 = 2.0;

/// The Realistic world's generator, its rock shaping `strength` of the way (None: realistic.json's).
fn generator(seed: u64, strength: Option<f64>) -> TerrainGenerator {
    let mut e = world_config(NAME)
        .unwrap()
        .expect("the Realistic world's edit");
    if let Some(s) = strength {
        e.geology.strength = s;
    }
    let map = WorldMap::for_height(seed, e.height.expect("its height field"))
        .with_worldgen(NAME)
        .unwrap();
    let def = dwell_worldgen::volume::terrain_volume().expect("the build's terrain volume");
    TerrainGenerator::with_map(seed, Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
        .with_volume(def)
}

/// Stepped and sampled dry land columns by the surface rock's jointing (bedded, vertical,
/// massive), over transects every `step` m across the face's land, and how many columns the
/// global volume reshaped.
fn census(g: &TerrainGenerator, step: usize) -> ([(usize, usize); 3], usize) {
    let mut out = [(0usize, 0usize); 3];
    let mut reshaped = 0;
    for z in (-28_000..=28_000).step_by(step) {
        for x in (-28_000..=28_000).step_by(step) {
            let first = g.column_at(x, z);
            if first.outside || first.height < 2.0 {
                continue;
            }
            // A 24 m transect along x: each column against its +x and +z neighbours.
            for i in 0..24 {
                let (cx, cz) = (x + i, z);
                let c = g.column_at(cx, cz);
                if c.outside || c.height < 2.0 || c.water != dwell_worldgen::terrain::Water::None {
                    continue;
                }
                if (c.height - c.base).abs() > 0.01 {
                    reshaped += 1;
                }
                let k = match g.rock_at_surface(i64::from(cx), i64::from(cz)).0.jointing {
                    Jointing::Bedded => 0,
                    Jointing::Vertical => 1,
                    Jointing::Massive => 2,
                };
                let wall = [(1, 0), (0, 1)].iter().any(|&(dx, dz)| {
                    let n = g.column_at(cx + dx, cz + dz);
                    !n.outside && (n.height - c.height).abs() >= WALL
                });
                out[k].1 += 1;
                if wall {
                    out[k].0 += 1;
                }
            }
        }
    }
    (out, reshaped)
}

fn share((stepped, n): (usize, usize)) -> f64 {
    stepped as f64 / n.max(1) as f64
}

dwell_world::both_faces!(
    /// Steps on bedded stacks and vertical rock, none on massive granite, and no global cliffs.
    fn realistic_steps_follow_the_rock(f: FaceFrame) {
        let seed = face_seed(SEED, f.face);
        let (shaped, reshaped) = census(&generator(seed, None), 1000);
        // The same land unshaped (strength 0): the steps are the rock's, not the height field's.
        let (plain, _) = census(&generator(seed, Some(0.0)), 1000);
        let [bedded, vertical, massive] = shaped;
        let [p_bedded, p_vertical, p_massive] = plain;
        eprintln!(
            "face {:?}: stepped / sampled (unshaped stepped): bedded {bedded:?} ({}), vertical \
             {vertical:?} ({}), massive {massive:?} ({}); reshaped by the volume {reshaped}",
            f.face, p_bedded.0, p_vertical.0, p_massive.0,
        );
        // The global cliffs volume reshapes nothing in a Realistic world (realistic.json `cliffs` 0).
        assert_eq!(reshaped, 0, "the cliffs volume is off in Realistic worlds");
        assert!(
            bedded.1 > 2000 && vertical.1 > 1000 && massive.1 > 2000,
            "enough of each rock's land sampled: {shaped:?}"
        );
        // Massive granite: about none, as unshaped (the few are the hill's own steepest ground and
        // the walls of a bedded layer or stack beside it).
        // Under 1.5 % (1 % before the plate boundaries' true crossings, BUGS.md #164, 0.64, moved
        // the provinces' edges: face B 133 of 13128, 1.01 %).
        assert!(
            share(massive) < 0.015,
            "massive granite rises unstepped: {massive:?}"
        );
        assert!(
            share(massive) - share(p_massive) < 0.005,
            "the shaping steps no massive ground: {massive:?} against {p_massive:?}"
        );
        // Bedded stacks step into walls and treads, vertical rock into columns.
        assert!(
            share(bedded) > share(p_bedded) + 0.025,
            "bedded stacks step: {bedded:?} against {p_bedded:?} unshaped"
        );
        assert!(
            vertical.0 > 2 * p_vertical.0 + 5,
            "vertical rock stands in columns: {vertical:?} against {p_vertical:?} unshaped"
        );
    }
);

/// Bedded-slope census (owner, 2026-11-13: Realistic's bedded slopes read as a field of single-cell
/// ½ m stair bumps): over dry bedded columns along transects, the ½ m edges (a 4-neighbour exactly
/// one row lower) and how many of them are bare (the column's top flat, no ramp over the edge),
/// and the ledges (a drop to a neighbour of at least `ledge` m).
fn bedded_steps(g: &TerrainGenerator, step: usize, ledge: f64) -> (usize, usize, usize, usize) {
    let (mut columns, mut edges, mut bare, mut ledges) = (0, 0, 0, 0);
    for z in (-28_000..=28_000).step_by(step) {
        for x in (-28_000..=28_000).step_by(step) {
            let first = g.column_at(x, z);
            if first.outside || first.height < 2.0 {
                continue;
            }
            for i in 0..24 {
                let (cx, cz) = (x + i, z);
                let c = g.column_at(cx, cz);
                if c.outside || c.height < 2.0 || c.water != dwell_worldgen::terrain::Water::None {
                    continue;
                }
                if g.rock_at_surface(i64::from(cx), i64::from(cz)).0.jointing != Jointing::Bedded {
                    continue;
                }
                let Some(t) = g.ground_y(cx, cz) else {
                    continue;
                };
                columns += 1;
                let flat = g
                    .surface_y_at(cx, cz)
                    .is_some_and(|s| (s - f64::from(t + 1) * 0.5).abs() < 1e-6);
                let mut half = false;
                let mut tall = false;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let Some(n) = g.ground_y(cx + dx, cz + dz) else {
                        continue;
                    };
                    half |= n == t - 1;
                    tall |= f64::from(t - n) * 0.5 >= ledge;
                }
                if half {
                    edges += 1;
                    if flat {
                        bare += 1;
                    }
                }
                if tall {
                    ledges += 1;
                }
            }
        }
    }
    (columns, edges, bare, ledges)
}

/// The Realistic generator with its 0.59 bedded shaping undone (strict ramps, no ledges, flat
/// treads): the 0.58 look, for the comparison.
fn generator_058(seed: u64) -> TerrainGenerator {
    let mut e = world_config(NAME)
        .unwrap()
        .expect("the Realistic world's edit");
    e.geology.notched = false;
    e.geology.ledge = 0.0;
    e.geology.tread_relief = 0.0;
    let map = WorldMap::for_height(seed, e.height.expect("its height field"))
        .with_worldgen(NAME)
        .unwrap();
    let def = dwell_worldgen::volume::terrain_volume().expect("the build's terrain volume");
    TerrainGenerator::with_map(seed, Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
        .with_volume(def)
}

dwell_world::both_faces!(
    /// Bedded slopes ramp, broken by ledges (owner, 2026-11-13): few ½ m edges on bedded ground are
    /// bare stairs (a flat top over a ½ m drop), and ledges at least the data's height stand.
    fn bedded_slopes_ramp_between_ledges(f: FaceFrame) {
        let seed = face_seed(SEED, f.face);
        let g = generator(seed, None);
        let ledge = g.geology().ledge;
        assert!(ledge >= 1.0, "rocks.json shaping.ledge: {ledge}");
        let (columns, edges, bare, ledges) = bedded_steps(&g, 1000, ledge);
        let old = bedded_steps(&generator_058(seed), 1000, ledge);
        let share = |e: usize, b: usize| b as f64 / e.max(1) as f64;
        eprintln!(
            "face {:?}: bedded columns {columns}, ½ m edges {edges}, bare {bare} ({:.3}), ledges ≥ {ledge} m {ledges}; 0.58: {old:?} ({:.3})",
            f.face,
            share(edges, bare),
            share(old.1, old.2),
        );
        assert!(columns > 2000, "enough bedded ground sampled: {columns}");
        // Under an eighth (a tenth before every ramp was backed, BUGS.md #163, 0.64: a ramp that
        // would back onto open air is a full block, its ½ m edge bare; face B 463 of 4530).
        assert!(
            share(edges, bare) < 0.125,
            "bedded slopes ramp, not a field of ½ m stairs: {bare} of {edges} ½ m edges bare"
        );
        assert!(
            share(old.1, old.2) > 0.4,
            "the 0.58 shaping stairs (the measure sees it): {old:?}"
        );
        assert!(
            ledges as f64 > 1.15 * old.3 as f64,
            "more ledges of at least {ledge} m than 0.58's stairs: {ledges} against {}",
            old.3
        );
        assert!(
            ledges * 25 > columns,
            "ledges of at least {ledge} m break the bedded ground: {ledges} of {columns}"
        );
    }
);

/// A top-down dump of bedded ground for the PR (not a check): `DWELL_DUMP=<dir>` writes, for face A,
/// a 160 m square on the bedded slope with the most ½ m edges among the samples, each column's
/// ground row and surface (m) under 0.58's shaping and 0.59's, as `x z ground surface` lines.
#[test]
#[ignore = "writes a dump for the PR; run with DWELL_DUMP=<dir> -- --ignored"]
fn dump_a_bedded_slope() {
    let Ok(dir) = std::env::var("DWELL_DUMP") else {
        return;
    };
    let seed = face_seed(SEED, dwell_world::Face::A);
    let new = generator(seed, None);
    let old = generator_058(seed);
    // The sample with the most ½ m edges on bedded ground over a 24 m patch.
    let mut best = (0usize, 0, 0);
    for z in (-28_000..=28_000).step_by(1000) {
        for x in (-28_000..=28_000).step_by(1000) {
            let c = new.column_at(x, z);
            if c.outside || c.height < 2.0 || c.water != dwell_worldgen::terrain::Water::None {
                continue;
            }
            if new.rock_at_surface(i64::from(x), i64::from(z)).0.jointing != Jointing::Bedded {
                continue;
            }
            let mut n = 0;
            for dz in (0..24).step_by(3) {
                for dx in (0..24).step_by(3) {
                    let (Some(a), Some(b)) = (
                        old.ground_y(x + dx, z + dz),
                        old.ground_y(x + dx + 1, z + dz),
                    ) else {
                        continue;
                    };
                    n += usize::from((a - b).abs() == 1);
                }
            }
            if n > best.0 {
                best = (n, x, z);
            }
        }
    }
    let (_, x0, z0) = best;
    for (name, g) in [("0.58", &old), ("0.59", &new)] {
        let mut out = String::new();
        for z in z0 - 80..z0 + 80 {
            for x in x0 - 80..x0 + 80 {
                let gy = g.ground_y(x, z).unwrap_or(0);
                let sy = g.surface_y_at(x, z).unwrap_or(0.0);
                out.push_str(&format!("{x} {z} {gy} {sy}\n"));
            }
        }
        std::fs::write(format!("{dir}/bedded-{name}.txt"), out).unwrap();
    }
    eprintln!("dumped around ({x0}, {z0})");
}

dwell_world::both_faces!(
    /// Red-brown desert sand (owner, 2026-11-13: "We need a red brown sand for the red desert
    /// areas"): the red rock mesa's floors are its floor band's ground by data (biomes/presets
    /// red_rock_mesa.json, now `dwell:red_sand`), red sand on the surface and never plain sand.
    fn the_red_desert_floor_is_red_sand_by_data(f: FaceFrame) {
        let (mut tops, mut red, mut plain) = (0, 0, 0);
        // Seed 2 too: since the biomes read the rain blurred (BUGS.md #164, 0.64), face B of the
        // other three holds only 17 mesa samples.
        for seed in [1, 2, 7, SEED] {
            let seed = face_seed(seed, f.face);
            let g = generator(seed, None);
            let mesa = g
                .biomes()
                .iter()
                .position(|b| b.name == "preset:red_rock_mesa")
                .expect("the Realistic world has the red rock mesa");
            let floor = g.biomes()[mesa].bands[0].ground;
            assert_eq!(floor, "dwell:red_sand", "the preset's floor band (data)");
            for z in (-28_000..=28_000).step_by(250) {
                for x in (-28_000..=28_000).step_by(250) {
                    let c = g.column_at(x, z);
                    if c.outside
                        || c.height < 2.0
                        || c.water != dwell_worldgen::terrain::Water::None
                    {
                        continue;
                    }
                    if usize::from(g.biome_mix(i64::from(x), i64::from(z)).chosen) != mesa {
                        continue;
                    }
                    let Some(y) = g.ground_y(x, z) else { continue };
                    tops += 1;
                    let name = dwell_blocks::block_of(g.state_at(x, y, z)).name;
                    red += usize::from(name == floor);
                    plain += usize::from(name == "dwell:sand");
                }
            }
        }
        eprintln!(
            "face {:?}: red rock mesa tops {tops}, red sand {red}, plain sand {plain}",
            f.face
        );
        assert!(tops > 20, "enough of the mesa sampled: {tops}");
        assert!(red * 5 > tops, "red sand floors the mesa: {red} of {tops}");
        assert_eq!(plain, 0, "no plain sand on the red desert's surface");
    }
);
