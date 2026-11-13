//! Mesa country's colours (WORK_QUEUE item 15; owner, 2026-11-13: "it just looks like a mess with
//! all the different colors" … "Real monument valley uses a blend of red, brown and tan, with brown
//! leading to tan, always smooth color changes" … "Mostly reddish dirt though"). rocks.json
//! `blends` `dwell:mesa_country`: on the red desert beds (the red rock mesa's own stack) every
//! ground and under block of the family takes the blend's tone, red dirt ↔ red sand ↔ tan sand in
//! order by a low-frequency noise and the slope, dithered; the walls are the darker red sandstone.
//! On seeds 1, 7, 42 and the owner's 2741893632, both faces, over the red rock mesa's columns:
//!
//! - the surface blocks are the blend's and the wall rock's (at most [`EXTRA`] others in 100);
//! - plain sand only as the blend's tan patches, at most its share and a little;
//! - neighbouring blended columns are at most one tone apart;
//! - the floors are mostly red dirt.

use std::sync::Arc;

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::geology::GEOLOGY;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
/// Surface blocks outside the family allowed, per 100 mesa columns (a boulder's rock, a river's).
const EXTRA: f64 = 1.0;

fn base(state: u16) -> &'static str {
    let n = dwell_blocks::info(state).name;
    let n = n.split('[').next().unwrap_or(n);
    n.strip_suffix("_ramp").unwrap_or(n)
}

#[derive(Default, Debug)]
struct Tally {
    columns: usize,
    blocks: std::collections::BTreeMap<&'static str, usize>,
    sand_unblended: usize,
    sand: usize,
    jumps: usize,
    pairs: usize,
    floors: usize,
    floor_dirt: usize,
}

fn tally(world: u64, f: FaceFrame) -> Tally {
    let seed = face_seed(world, f.face);
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let talus = e.geology.erosion.talus_width;
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    let g = TerrainGenerator::with_map(seed, map)
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    let mesa = g
        .biomes()
        .iter()
        .position(|b| b.name == "preset:red_rock_mesa")
        .expect("the red rock mesa");
    let mut t = Tally::default();
    for z in (-46_000..=46_000).step_by(500) {
        for x0 in (-46_000..=46_000).step_by(500) {
            let mut last: Option<usize> = None;
            for i in 0..32 {
                let x = x0 + i;
                let col = g.column_at(x, z);
                if col.outside
                    || col.water != Water::None
                    || usize::from(g.biome_mix(x.into(), z.into()).chosen) != mesa
                {
                    last = None;
                    continue;
                }
                let Some(y) = g.ground_y(x, z) else { continue };
                let top = base(g.state_at(x, y, z));
                t.columns += 1;
                *t.blocks.entry(top).or_default() += 1;
                let tone = g.blend_tone_at(x, z);
                if top == "dwell:sand" {
                    t.sand += 1;
                    t.sand_unblended += usize::from(tone.is_none());
                }
                if let Some((k, _)) = tone {
                    if let Some(l) = last {
                        t.pairs += 1;
                        t.jumps += usize::from(k.abs_diff(l) > 1);
                    }
                    last = Some(k);
                } else {
                    last = None;
                }
                if let Some((s, m, _)) = g.erosion_at(x.into(), z.into())
                    && s > 0.9
                    && m < -(talus + 10.0)
                {
                    t.floors += 1;
                    t.floor_dirt += usize::from(top == "dwell:red_dirt");
                }
            }
        }
    }
    t
}

both_faces!(
    fn mesa_country_is_one_blended_red_palette(f: FaceFrame) {
        let blend = GEOLOGY
            .blends
            .iter()
            .find(|b| b.name == "dwell:mesa_country")
            .expect("rocks.json blends dwell:mesa_country");
        let mut family: Vec<&str> = blend.blocks.to_vec();
        family.push("dwell:red_sandstone");
        let sand_share = blend.shares[blend
            .blocks
            .iter()
            .position(|&b| b == "dwell:sand")
            .unwrap()];
        let mut seen = 0;
        for world in SEEDS {
            let t = tally(world, f);
            eprintln!(
                "seed {world} {:?}: {} mesa columns {:?}; sand {} ({} unblended); tone jumps {} of {}; floors {} red dirt of {}",
                f.face,
                t.columns,
                t.blocks,
                t.sand,
                t.sand_unblended,
                t.jumps,
                t.pairs,
                t.floor_dirt,
                t.floors
            );
            if t.columns < 500 {
                continue;
            }
            seen += 1;
            let others: usize = t
                .blocks
                .iter()
                .filter(|(b, _)| !family.contains(b))
                .map(|(_, n)| n)
                .sum();
            assert!(
                (others as f64) <= EXTRA / 100.0 * t.columns as f64,
                "seed {world}: {others} of {} surface columns outside the family: {:?}",
                t.columns,
                t.blocks
            );
            assert!(
                t.sand_unblended * 1000 <= t.columns,
                "seed {world}: plain sand off the blend on {} columns",
                t.sand_unblended
            );
            assert!(
                (t.sand as f64) <= (sand_share + 0.06) * t.columns as f64,
                "seed {world}: tan sand on {} of {}",
                t.sand,
                t.columns
            );
            assert_eq!(t.jumps, 0, "seed {world}: neighbours two tones apart");
            if t.floors >= 200 {
                assert!(
                    t.floor_dirt * 2 > t.floors,
                    "seed {world}: red dirt on {} of {} floors",
                    t.floor_dirt,
                    t.floors
                );
            }
        }
        assert!(seen >= 1, "{:?}: mesa country on some seed", f.face);
    }
);
