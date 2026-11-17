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
    let mut n = 0;
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let [x, y, z] = f.cell([x, FLAT_TOP_Y, z]);
            n += usize::from(is_tuft(fx.server.world().get_voxel(x, y, z)));
        }
    }
    n
}

/// Whether `s` is tall grass at any growth stage.
fn is_tuft(s: dwell_blocks::StateId) -> bool {
    s != M.air && dwell_blocks::block_of(s).name == "dwell:tall_grass"
}

/// The growth stage of the tuft at face-local `p` (None: no tuft).
fn stage_at(fx: &mut Fixture, f: FaceFrame, p: [i32; 3]) -> Option<usize> {
    let [x, y, z] = f.cell(p);
    let s = fx.server.world().get_voxel(x, y, z);
    dwell_worldgen::grass::Rule::build(0).stage_of(f.state(s))
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

/// Runs the grass step until every loaded chunk is grown up to today (a step takes at most 2).
fn grow_all(fx: &mut Fixture) {
    for _ in 0..2048 {
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

dwell_world::both_faces!(
    fn the_server_grows_sprouts_taller_and_an_eaten_tuft_goes_back_to_a_sprout(f: FaceFrame) {
        // docs/ECOLOGY.md §10: stages advance with the days the server grows.
        let rule = dwell_worldgen::grass::Rule::build(0);
        let full = rule.stage_of(rule.block()).unwrap();
        let mut fx = scene(f, config(f), 0);
        let row: Vec<[i32; 3]> = (8..24).map(|x| [x, FLAT_TOP_Y, 12]).collect();
        for &p in &row {
            let [x, y, z] = f.cell(p);
            fx.server.world().set_voxel(x, y, z, f.state(rule.sprout()));
        }
        for _ in 0..20 {
            fx.server.step();
        }
        grow_all(&mut fx);
        let stages: Vec<usize> = row
            .iter()
            .map(|&p| stage_at(&mut fx, f, p).unwrap())
            .collect();
        assert!(stages.iter().all(|&s| s > 0), "all grew: {stages:?}");
        assert!(
            stages.iter().filter(|&&s| s == full).count() >= row.len() / 2,
            "most reached full height in 20 days: {stages:?}"
        );
        // Eaten (a grazer's bite): a sprout over grass, in a world that grows grass.
        let p = row[0];
        assert_eq!(fx.server.eaten_to(f.cell(p)), f.state(rule.sprout()));
        fx.server.set_grass_enabled(false);
        assert_eq!(
            fx.server.eaten_to(f.cell(p)),
            M.air,
            "no growth: gone, as before"
        );
    }
);

dwell_world::both_faces!(
    /// Up to 2 m (WORK_QUEUE item 58 g): over the days the server grows, a lush stand grows cell
    /// by cell to four ½ m cells, its lower cells stalks; a bite at its foot leaves a sprout and
    /// nothing standing over it.
    fn the_server_grows_a_lush_stand_to_two_metres_and_a_bite_takes_it(f: FaceFrame) {
        let rule = dwell_worldgen::grass::Rule::build(0);
        let mut fx = scene(f, config(f), 0);
        let foot = [10, FLAT_TOP_Y, 10];
        let [x, y, z] = f.cell(foot);
        fx.server.world().set_voxel(x, y, z, f.state(rule.block()));
        for _ in 0..12 {
            fx.server.step();
        }
        grow_all(&mut fx);
        let at = |fx: &mut Fixture, k: i32| {
            let [x, y, z] = f.cell([foot[0], foot[1] + k, foot[2]]);
            f.state(fx.server.world().get_voxel(x, y, z))
        };
        for k in 0..3 {
            assert_eq!(at(&mut fx, k), rule.stalk(), "cell {k} a stalk");
        }
        assert_eq!(at(&mut fx, 3), rule.block(), "the tip full, 2 m up");
        assert_eq!(at(&mut fx, 4), M.air, "no more than four cells");
        let bite = fx.server.eaten_stand(f.cell(foot));
        assert_eq!(bite.len(), 4);
        assert_eq!(bite[0], (f.cell(foot), f.state(rule.sprout())));
        assert!(
            bite[1..].iter().all(|&(_, s)| s == M.air),
            "the stand over it eaten too"
        );
    }
);

dwell_world::both_faces!(
    /// BUGS.md #194 (owner, 2026-11-16: "It's a periodic stutter. But quite visible and stalls the
    /// player movement."): the grass step grew up to 64 chunks in one tick every 5 s, a spike in
    /// the server's tick and the clients' re-meshing. Now a tick grows at most a couple, every
    /// tick, so a hundred chunks behind are spread over many ticks, and all of them still grow.
    fn the_grass_step_spreads_its_chunks_over_the_ticks(f: FaceFrame) {
        let mut fx = scene(f, config(f), 0);
        for dx in -5..5 {
            for dz in -5..5 {
                let [x, y, z] = f.cell([dx * 32 + 4, FLAT_TOP_Y, dz * 32 + 4]);
                fx.server.world().get_or_create(chunk_of(x, y, z));
            }
        }
        // A world day on (a day a tick here): every chunk is behind.
        for _ in 0..3 {
            fx.server.step();
            for dx in -5..5 {
                for dz in -5..5 {
                    let [x, y, z] = f.cell([dx * 32 + 4, FLAT_TOP_Y, dz * 32 + 4]);
                    fx.server.world().get_or_create(chunk_of(x, y, z));
                }
            }
        }
        let before = fx.server.grass().days.clone();
        fx.server.grow_grass_now();
        let n = fx
            .server
            .grass()
            .days
            .iter()
            .filter(|(c, d)| before.get(*c) != Some(*d))
            .count();
        assert!(
            (1..=2).contains(&n),
            "one step grows a couple of chunks: {n}"
        );
        for _ in 0..120 {
            fx.server.grow_grass_now();
        }
        let today = fx.server.world_days().floor() as u32;
        let done = fx
            .server
            .grass()
            .days
            .values()
            .filter(|d| **d == today)
            .count();
        assert!(done >= 100, "every chunk grows in turn: {done}");
    }
);

dwell_world::both_faces!(
    /// Priority 0 (BUGS.md #194): the tick's grass step never regenerates a chunk. A chunk grows a
    /// quarter a tick; an edit between its quarters (its own first quarter's grass, a player's)
    /// left it edited without a pristine copy, and the later quarters regenerated it (3–12 ms
    /// natively a quarter). An unedited chunk's own cells are kept as its generated ones when it is
    /// picked.
    fn the_tick_never_regenerates_a_chunk_to_grow_grass(f: FaceFrame) {
        // No edit to start with (the scene's would wait for its pristine copy, never sent here).
        // The meadow with a generated tuft in each chunk (local column 8, 8): grass to spread from.
        let base = generator(f);
        let tall = f.state(dwell_blocks::require("dwell:tall_grass"));
        let tufted: dwell_world::ChunkGenerator =
            Arc::new(move |c: ChunkCoord, chunk: &mut dwell_world::Chunk| {
                base(c, chunk);
                for ly in 0..CHUNK_SIZE {
                    if f.row(c.y * CHUNK_SIZE + ly) == FLAT_TOP_Y {
                        chunk.voxels_mut()[local_index(8, ly, 8)] = tall;
                    }
                }
            });
        let mut fx = Fixture::new(ServerConfig {
            generator_override: Some(tufted),
            // A world day every 300 ticks.
            growth_speed: 240.0,
            // The browser's: pristine copies come from the jobs worker (here never), not inline.
            jobs_remote: true,
            ..config(f)
        });
        fx.server.set_grass_envelope(Some(lush()));
        let mut floors = Vec::new();
        for dx in -2..2 {
            for dz in -2..2 {
                let [x, y, z] = f.cell([dx * 32 + 40, FLAT_TOP_Y, dz * 32 + 40]);
                floors.push(chunk_of(x, y, z));
                fx.server.world().get_or_create(chunk_of(x, y, z));
            }
        }
        // Until a floor chunk (its meadow the spots) is part way through: then an edit in it.
        let mut edited = false;
        for _ in 0..700 {
            // Kept loaded (no player here holds them).
            for &c in &floors {
                fx.server.world().get_or_create(c);
            }
            fx.server.step();
            if !edited
                && let Some((c, 1)) = fx.server.grass_in_hand()
                && floors.contains(&c)
            {
                let o = [c.x * CHUNK_SIZE, c.y * CHUNK_SIZE, c.z * CHUNK_SIZE];
                let stone = f.state(M.stone);
                fx.server
                    .world()
                    .set_voxel(o[0] + 30, o[1] + 30, o[2] + 30, stone);
                edited = true;
            }
        }
        assert!(edited, "a chunk part way through");
        assert_eq!(
            fx.server.grass_regenerated(),
            0,
            "chunks regenerated on the tick"
        );
    }
);
