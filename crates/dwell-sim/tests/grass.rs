//! Tall grass growing on the server (ADR 0075 T3c, docs/ECOLOGY.md §10): the grass step grows a
//! loaded chunk's missed world days (bounded), a player's block stands, the chunk's day and the
//! world's switch survive a save, and a world saved before grass growth has none.

mod common;

use std::sync::Arc;

use common::*;
use dwell_sim::server::ServerConfig;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{
    CHUNK_SIZE, ChunkCoord, FLAT_TOP_Y, FaceFrame, FlatLayers, M, MIDPLANE_Y, chunk_of,
    local_index, m_of_row,
};
use dwell_worldgen::noise::{hash2, unit};

/// The flat world's layers on face `f` (face B its mirror), so both faces have a meadow floor.
fn generator(f: FaceFrame) -> dwell_world::ChunkGenerator {
    let layers = FlatLayers::standard();
    Arc::new(move |c: ChunkCoord, chunk: &mut dwell_world::Chunk| {
        for ly in 0..CHUNK_SIZE {
            let y = f.row(c.y * CHUNK_SIZE + ly);
            if !(MIDPLANE_Y + 8..FLAT_TOP_Y).contains(&y) {
                continue;
            }
            let s = f.state(layers.at_depth(i64::from(FLAT_TOP_Y - 1 - y)));
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    chunk.voxels_mut()[local_index(lx, ly, lz)] = s;
                }
            }
        }
    })
}

fn config(f: FaceFrame) -> ServerConfig {
    ServerConfig {
        generator_override: Some(generator(f)),
        spawn: Some(f.point([0.5, m_of_row(FLAT_TOP_Y), 0.5])),
        // A world day a tick (the clock's 20-minute day).
        growth_speed: 72_000.0,
        ..flat()
    }
}

/// A lush climate everywhere: the per-column roll a hash, the share 1.
fn lush() -> dwell_sim::server::GrassEnvelope {
    Arc::new(|_, x, z| (unit(hash2(0x5EED, x, z)), 1.0))
}

/// The tall grass on the floor of the chunk holding face-local column (4, 4).
fn tufts(fx: &mut Fixture, f: FaceFrame) -> usize {
    let tall = f.state(dwell_blocks::require("dwell:tall_grass"));
    let mut n = 0;
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let [x, y, z] = f.cell([x, FLAT_TOP_Y, z]);
            n += usize::from(fx.server.world().get_voxel(x, y, z) == tall);
        }
    }
    n
}

/// A server on face `f` with one tuft at (4, 4) and a placed stone beside it, `ticks` ticks on.
fn scene(f: FaceFrame, cfg: ServerConfig, ticks: u32) -> Fixture {
    let mut fx = Fixture::new(cfg);
    fx.server.set_grass_envelope(Some(lush()));
    let tall = f.state(dwell_blocks::require("dwell:tall_grass"));
    let [x, y, z] = f.cell([4, FLAT_TOP_Y, 4]);
    fx.server.world().set_voxel(x, y, z, tall);
    let [x, y, z] = f.cell([5, FLAT_TOP_Y, 4]);
    fx.server.world().set_voxel(x, y, z, f.state(M.stone));
    for _ in 0..ticks {
        fx.server.step();
    }
    fx
}

/// Runs the grass step until every loaded chunk is grown up to today (a step takes at most 64).
fn grow_all(fx: &mut Fixture) {
    for _ in 0..64 {
        fx.server.grow_grass_now();
    }
}

dwell_world::both_faces!(
    fn the_server_grows_a_loaded_chunks_missed_days_and_a_players_block_stands(f: FaceFrame) {
        let mut fx = scene(f, config(f), 12);
        let before = tufts(&mut fx, f);
        grow_all(&mut fx);
        let after = tufts(&mut fx, f);
        assert!(after > before, "it spread: {before} → {after}");
        let [x, y, z] = f.cell([5, FLAT_TOP_Y, 4]);
        assert_eq!(fx.server.world().get_voxel(x, y, z), f.state(M.stone));
        let c = chunk_of(x, y, z);
        let today = fx.server.world_days().floor() as u32;
        assert_eq!(fx.server.grass().days.get(&c), Some(&today));
        // Grown up to today: nothing more until tomorrow.
        grow_all(&mut fx);
        assert_eq!(tufts(&mut fx, f), after);
    }
);

#[test]
fn the_grass_switch_and_days_survive_a_save_and_an_older_world_has_none() {
    // face-a-only: a byte format through the store.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f)
    };
    let days = {
        let mut fx = scene(f, cfg(), 12);
        assert!(fx.server.grass().enabled, "a new world grows grass");
        grow_all(&mut fx);
        fx.server.save_now();
        fx.server.flush_store();
        fx.server.grass().days.clone()
    };
    assert!(db.grass().is_some(), "saved");
    let fx = Fixture::new(cfg());
    assert!(fx.server.grass().enabled);
    assert_eq!(fx.server.grass().days, days, "the chunks' days kept");

    // A world saved before grass growth (meta, no table): none.
    let old = WorldDb::in_memory().expect("db");
    {
        let mut fx = Fixture::new(ServerConfig {
            store: Some(WorldStore::new(old.clone(), false)),
            grass: false,
            ..config(f)
        });
        fx.server.save_now();
        fx.server.flush_store();
    }
    assert!(old.load_meta().is_some());
    let fx = Fixture::new(ServerConfig {
        store: Some(WorldStore::new(old.clone(), false)),
        ..config(f)
    });
    assert!(
        !fx.server.grass().enabled,
        "an opted-out world keeps its choice"
    );
}

#[test]
fn the_saved_form_round_trips_and_another_version_is_unread() {
    // face-a-only: a byte format.
    use dwell_sim::server::Grass;
    let mut g = Grass {
        enabled: true,
        days: Default::default(),
    };
    g.days.insert(ChunkCoord { x: -3, y: 7, z: 2 }, 41);
    g.days.insert(ChunkCoord { x: 1, y: -2, z: 0 }, 9);
    assert_eq!(Grass::from_bytes(&g.to_bytes()), Some(g.clone()));
    let mut newer = g.to_bytes();
    newer[0] = 2;
    assert_eq!(Grass::from_bytes(&newer), None);
}
