//! No ramp backs onto open air (BUGS.md #163; owner, 2026-11-13: "I don't mind slopes with edges
//! exposed but I do mind slopes that back up against open air"). Every stored ramp's high side
//! meets the column behind it standing at least the ramp's top along their shared edge
//! (`ramps::top_backed_by`), in the default world and the Realistic world, on seeds 1, 7, 42 and
//! the owner's 274005646, both faces. (A ramp's triangular sides may still show.)

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::world_config;
use dwell_worldgen::ramps::{Top, edge_height};
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const SEEDS: [u64; 4] = [1, 7, 42, 274_005_646];

/// (ramps seen, ramps whose back meets lower ground) over transects of the face's land.
fn census(g: &TerrainGenerator, reach: i32) -> (usize, usize) {
    let top = |x: i32, z: i32| {
        if g.ramp_pieces_at(x, z) > 0 {
            g.ramp_top_at(x, z)
        } else {
            Top::Flat
        }
    };
    let height = |x: i32, z: i32| g.ground_y(x, z).map_or(i32::MIN / 2, |y| y + 1);
    let (mut ramps, mut bad) = (0, 0);
    for z in (-reach..=reach).step_by(4000) {
        for x0 in (-reach..=reach).step_by(4000) {
            for dz in 0..16 {
                for i in 0..16 {
                    let (x, z) = (x0 + i, z + dz);
                    let f = match top(x, z) {
                        Top::Flat => continue,
                        Top::Gentle(f) | Top::Steep(f) => f,
                    };
                    ramps += 1;
                    let (sx, sz) = f.step();
                    let (bx, bz) = (x - sx, z - sz);
                    if edge_height(height(bx, bz), top(bx, bz), f) < height(x, z) {
                        bad += 1;
                    }
                }
            }
        }
    }
    (ramps, bad)
}

both_faces!(
    fn no_ramp_backs_onto_open_air(f: FaceFrame) {
        let e = world_config("test:realistic")
            .unwrap()
            .expect("the Realistic world");
        let (mut ramps, mut bad) = (0, 0);
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let default = BifacialTerrain::height_field(world).local(f.face).clone();
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen("test:realistic")
                    .unwrap(),
            );
            let realistic = TerrainGenerator::with_map(seed, map)
                .with_biomes_and_geology(&e.biomes, e.map, e.geology)
                .with_vegetation(e.vegetation);
            for (name, g, reach) in [
                ("default", &default, 20_000),
                ("realistic", &realistic, 44_000),
            ] {
                let (r, b) = census(g, reach);
                eprintln!(
                    "seed {world} {:?} {name}: {r} ramps, {b} backed onto air",
                    f.face
                );
                ramps += r;
                bad += b;
            }
        }
        assert!(ramps > 1000, "{:?}: ramps sampled {ramps}", f.face);
        assert_eq!(
            bad, 0,
            "{:?}: {bad} of {ramps} ramps back onto open air",
            f.face
        );
    }
);
