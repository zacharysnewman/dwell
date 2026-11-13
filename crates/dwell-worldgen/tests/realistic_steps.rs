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
        assert!(
            share(massive) < 0.01,
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
