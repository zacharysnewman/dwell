//! Tall grass in the starter world's meadow on the server (the visual pass, 2026-11-11): the
//! chunks the server holds round a player keep the generator's tufts (the report "tall grass nearly
//! absent in game" was the meadow spot standing in a gap between grass patches, not a loss). Its own test binary: the
//! starter's maps are installed process-wide.

mod common;

use std::sync::OnceLock;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_world::{CHUNK_SIZE, Face, FaceFrame, GENERATOR_TERRAIN};
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};

const SEED: u64 = 0;
const NAME: &str = "test:starter";
/// The tour's meadow spot (`client/bench/spots.json` `starter:meadow-grass`).
const MEADOW: [i32; 2] = [10100, 16300];

fn install() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        for face in [Face::A, Face::B] {
            let bytes =
                dwell_worldgen::config::worldgen_map_bytes(face_seed(SEED, face), NAME).unwrap();
            dwell_worldgen::worldmap::install(&bytes).unwrap();
        }
    });
}

#[test]
fn the_servers_meadow_chunks_keep_the_generated_tall_grass() {
    // face-a-only: the tour's meadow is face A's.
    install();
    let f = FaceFrame::A;
    let t = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).unwrap();
    let y = t.local(f.face).surface_y_at(MEADOW[0], MEADOW[1]).unwrap();
    let mut fx = Fixture::new(ServerConfig {
        generator_version: GENERATOR_TERRAIN,
        world_seed: SEED,
        spawn: Some(f.point([
            f64::from(MEADOW[0]) + 0.5,
            y + 1.0,
            f64::from(MEADOW[1]) + 0.5,
        ])),
        mode: "creative".into(),
        ..flat()
    });
    assert_eq!(fx.server.worldgen_text(), NAME);
    fx.join(1, &Client::new(1));
    let tall = dwell_blocks::require("dwell:tall_grass");
    let generator = dwell_worldgen::generator_for(GENERATOR_TERRAIN, SEED);
    let count = |fx: &mut Fixture| {
        let (mut now, mut generated) = (0, 0);
        let near: Vec<_> = fx
            .server
            .world()
            .chunks()
            .filter(|(c, _)| {
                (c.x * CHUNK_SIZE - MEADOW[0]).abs() <= 48
                    && (c.z * CHUNK_SIZE - MEADOW[1]).abs() <= 48
            })
            .map(|(c, k)| (*c, k.voxels().iter().filter(|&&m| m == tall).count()))
            .collect();
        for (c, n) in near {
            let mut g = dwell_world::Chunk::default();
            generator(c, &mut g);
            generated += g.voxels().iter().filter(|&&m| m == tall).count();
            now += n;
        }
        (now, generated)
    };
    for s in [1u32, 10] {
        for _ in 0..s * SIM_HZ {
            fx.server.step();
        }
        let _ = fx.take_all();
    }
    let (now, generated) = count(&mut fx);
    assert!(
        generated > 100,
        "the meadow generates tall grass: {generated}"
    );
    assert!(
        now * 10 >= generated * 9,
        "the server's chunks hold {now} tall grass cells, the generator's {generated}"
    );
}
