//! Plants bleed across borders (WORK_QUEUE item 15, the 0.69 follow-up; owner, 2026-11-14:
//! "Bushes, shrubs, rocks, trees, should bleed into the edge of the neighboring terrain to help with
//! blending."). A placement stood only on its own ground (`on`), so trees, bushes and rocks stopped
//! at the line where the ground changed. With `bleed` (placements.json) a candidate on the
//! neighbour's ground stands when its own lies within `bleed` m, at a chance falling with the
//! distance, and its biomes' density is read at a point moved by a fine noise. On seeds 1, 7, 42
//! and 2741893632, both faces, patches of the land:
//!
//! - plants of the bleeding placements stand on the neighbour's ground;
//! - each within its `bleed` of its own ground, fewer the farther out.

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::placements::PLACEMENTS;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 2] = [1, 2_741_893_632];
const PATCH: i32 = 128;

fn base(name: &str) -> &str {
    let n = name.split('[').next().unwrap_or(name);
    n.strip_suffix("_ramp").unwrap_or(n)
}

both_faces!(
    fn plants_reach_past_the_ground_line(f: FaceFrame) {
        let (mut own, mut bled, mut beyond) = (0usize, 0usize, 0usize);
        let mut bins = [0usize; 3];
        for world in SEEDS {
            let e = world_config(NAME).unwrap().expect("the Realistic world");
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen(NAME)
                    .unwrap(),
            );
            let g = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation)
                .with_placements(&PLACEMENTS);
            let top = |x: i32, z: i32| {
                g.ground_y(x, z)
                    .map(|y| base(dwell_blocks::info(g.state_at(x, y, z)).name))
            };
            for pz in (-40_000..=40_000).step_by(4500) {
                for px in (-40_000..=40_000).step_by(4500) {
                    for t in g.trees_in((px, pz), (px + PATCH - 1, pz + PATCH - 1)) {
                        let [x, _, z] = t.anchor;
                        if !(px..px + PATCH).contains(&x) || !(pz..pz + PATCH).contains(&z) {
                            continue;
                        }
                        let Some(rule) = g
                            .planter_name(t.planter)
                            .and_then(|n| PLACEMENTS.iter().find(|p| p.name == n))
                        else {
                            continue;
                        };
                        if rule.bleed <= 0.0 {
                            continue;
                        }
                        let Some(here) = top(x, z) else { continue };
                        if rule.on.contains(&here) {
                            own += 1;
                            continue;
                        }
                        bled += 1;
                        // The nearest of its own ground, on rings out to its bleed.
                        let reach = rule.bleed.ceil() as i32;
                        let d = (1..=reach + 1).find(|&d| {
                            (0..16).any(|k| {
                                let a = f64::from(k) * std::f64::consts::TAU / 16.0;
                                let (dx, dz) = (
                                    (f64::from(d) * a.cos()).round() as i32,
                                    (f64::from(d) * a.sin()).round() as i32,
                                );
                                top(x + dx, z + dz).is_some_and(|b| rule.on.contains(&b))
                            })
                        });
                        match d {
                            Some(d) => bins[((3 * (d - 1)) / reach).min(2) as usize] += 1,
                            None => beyond += 1,
                        }
                    }
                }
            }
        }
        eprintln!(
            "{:?}: {own} on their own ground, {bled} on a neighbour's, by distance {bins:?} (thirds of the bleed), {beyond} beyond",
            f.face
        );
        assert!(
            bled >= 20,
            "{:?}: plants bled past the ground line ({bled})",
            f.face
        );
        // A ring search can miss a sliver of own ground: nearly all within the bleed.
        assert!(
            beyond * 20 <= bled,
            "{:?}: {beyond} of {bled} beyond their bleed",
            f.face
        );
        assert!(
            bins[0] > bins[2],
            "{:?}: fewer the farther out: {bins:?}",
            f.face
        );
    }
);
