//! World persistence through the server (ARCHITECTURE.md §6.4, Phase 3e): saving and reloading an
//! edited world, evicting saved chunks, bans and allow-lists, and the LOD cache across restarts.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use common::*;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_storage::{SaveBatch, WORLD_EXTENSION, WorldDb, WorldStore};
use dwell_world::lod::{LodCoord, MAX_LEVEL};
use dwell_world::{
    Chunk, ChunkCoord, FLAT_TOP_Y, GENERATOR_FLAT, M, chunk_hash, generate_flat_chunk,
};

/// The chunk row holding the flat world's surface row (`FLAT_TOP_Y`): 2 on ½ m cells.
const GC: i32 = dwell_world::FLAT_TOP_Y / dwell_world::CHUNK_SIZE;

/// The flat world's surface: its top voxel is G − 1.
const G: i32 = FLAT_TOP_Y;

/// A fresh scratch directory under the system temp dir, removed (with its world files) on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dwell-persistence-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
    fn world(&self, name: &str) -> PathBuf {
        self.0.join(format!("{name}{WORLD_EXTENSION}"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A store over one world file that saves inline (no I/O thread), reopened by each call.
fn open_store(path: &Path) -> WorldStore {
    WorldStore::new(WorldDb::open(path).expect("opens"), false)
}

fn place(cell: [i32; 3], face: u8, m: u16) -> Message {
    Message::BlockEditRequest(BlockEditRequest {
        action: BlockEditAction::Place,
        cell,
        face,
        material: m,
        rows: 1,
        down: false,
    })
}

#[test]
fn a_worlds_settings_are_saved_with_it_and_kept_over_a_new_servers_defaults() {
    // DAY_NIGHT.md §2, WATER_FLOW.md §4.1: a new world takes the server's water speed (and this
    // build's clock) and keeps them; a server opening it later with other defaults uses the
    // world's, and sends them at join.
    let scratch = Scratch::new("settings");
    let path = scratch.world("settings");
    {
        let store = WorldStore::open_file(&path).expect("opens");
        let mut f = Fixture::new(ServerConfig {
            store: Some(store),
            water_speed: 3.0,
            ..flat()
        });
        assert_eq!(f.server.world_settings().water_speed, 3.0);
        f.server.save_now();
        f.server.flush_store();
    }
    let store = WorldStore::open_file(&path).expect("reopens");
    let mut f = Fixture::new(ServerConfig {
        store: Some(store),
        water_speed: 1.0,
        ..flat()
    });
    let s = f.server.world_settings();
    assert_eq!(s.water_speed, 3.0, "the world's own");
    assert_eq!(
        s.clock,
        dwell_sim::server::default_settings(1.0).clock,
        "the clock it was made with"
    );
    f.join(1, &Client::new(1)); // checks the settings sent are these
}

#[test]
fn an_edited_world_survives_a_server_restart() {
    let scratch = Scratch::new("server");
    let path = scratch.world("server");
    {
        let store = WorldStore::open_file(&path).expect("opens");
        let mut f = Fixture::new(ServerConfig {
            world_seed: 77,
            store: Some(store),
            ..flat()
        });
        f.join(1, &Client::new(1));
        f.server.step();
        f.send(1, place([-1, G - 1, 2], 2, M.sand));
        f.server.step();
        assert_eq!(f.server.world().get_voxel(-1, G, 2), M.sand);
        f.move_to(1, 3.5, f64::from(G), 4.5);
        f.server.step();
        assert_eq!(f.server.save_stats().dirty, 1);
        f.server.save_now();
        f.server.flush_store();
        f.server.step();
        assert_eq!(f.server.save_stats().dirty, 0);
        assert_eq!(f.server.save_stats().saved_chunks, 1);
        assert!(
            f.server.save_stats().saves >= 2,
            "{:?}",
            f.server.save_stats()
        ); // the new world's meta, then this one
    }

    // A new server on the same file: the launch options' seed is ignored for the saved world's.
    let store = WorldStore::open_file(&path).expect("reopens");
    let mut f = Fixture::new(ServerConfig {
        world_seed: 1,
        store: Some(store),
        ..flat()
    });
    assert_eq!(f.server.world().get_voxel(-1, G, 2), M.sand);
    assert_eq!(
        f.server
            .world()
            .find(ChunkCoord::new(-1, GC, 0))
            .unwrap()
            .revision(),
        1
    );
    assert!(f.server.save_stats().loaded >= 1);
    // The returning player continues where it was.
    let w = f.join(1, &Client::new(1));
    assert_eq!(w.world_seed, 77);
    let h = f.server.player_handle_of(w.player_id).unwrap();
    let p = {
        let (_, physics, players) = f.server.sim();
        players.position(physics, h)
    };
    assert!((p.x - 3.5).abs() < 1e-4, "{p:?}");
    assert!((p.z - 4.5).abs() < 1e-4, "{p:?}");
    // The edited chunk streams Explicit (in generated mode), its neighbours Generated.
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(1, Message::WorldgenCheck(chunk_hash(&v)));
    let chunks = f.chunks(1, 10, None);
    let edited = chunks
        .iter()
        .find(|m| m.coord == [-1, GC, 0])
        .expect("the edited chunk streamed");
    assert_eq!(edited.form, ChunkForm::Explicit);
    assert_eq!(edited.revision, 1);
}

#[test]
fn saved_chunks_far_from_players_are_evicted_and_read_back_when_needed() {
    let scratch = Scratch::new("evict");
    let path = scratch.world("evict");
    let mut f = Fixture::new(ServerConfig {
        store: Some(open_store(&path)),
        ..flat()
    }); // saves inline
    f.join(1, &Client::new(1));
    f.server.step();
    f.send(1, place([-1, G - 1, 2], 2, M.sand));
    f.server.step();
    f.server.save_now();
    // Walk far away: the saved chunk is evicted, not lost.
    f.move_to(1, 500.5, dwell_world::m_of_row(G), 0.5);
    for _ in 0..130 {
        f.server.step();
    }
    assert!(f.server.world().find(ChunkCoord::new(-1, GC, 0)).is_none());
    f.move_to(1, 0.5, dwell_world::m_of_row(G), 0.5);
    for _ in 0..3 {
        f.server.step();
    }
    assert!(f.server.world().find(ChunkCoord::new(-1, GC, 0)).is_some());
    assert_eq!(f.server.world().get_voxel(-1, G, 2), M.sand);
}

#[test]
fn banned_players_are_refused_and_an_allow_list_admits_only_its_keys() {
    let mut f = Fixture::new(ServerConfig {
        banned: vec![Client::new(2).public_key],
        allow_list: Some(vec![Client::new(1).public_key, Client::new(2).public_key]),
        ..flat()
    });
    let banned = Client::new(2);
    f.server
        .on_connected(2, TransportKind::WebTransport, f.binding);
    f.send(
        2,
        Message::ClientHello(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_version: "test".into(),
            public_key: banned.public_key,
            display_name: "Banned".into(),
        }),
    );
    let Message::Challenge(nonce) = f.take(2).remove(0).0 else {
        panic!("expected a challenge")
    };
    let signature = banned.sign(&auth_transcript(&nonce, &f.binding, &banned.public_key));
    f.send(2, Message::ClientAuth(signature));
    let reply = f.take(2);
    assert!(!reply.is_empty());
    let Message::Reject(r) = &reply[0].0 else {
        panic!("expected Reject, got {:?}", reply[0].0)
    };
    assert_eq!(r.reason, RejectReason::Banned);
    assert_eq!(f.server.joined_players(), 0);
    f.join(1, &Client::new(1));
    assert_eq!(f.server.joined_players(), 1);
}

#[test]
fn lod_sections_survive_a_restart_and_are_rebuilt_when_stale_or_missing() {
    let scratch = Scratch::new("lod");
    let path = scratch.world("lod");
    let mut saved: HashMap<LodCoord, u32> = HashMap::new();
    {
        let mut f = Fixture::new(ServerConfig {
            store: Some(open_store(&path)),
            ..flat()
        });
        f.join(1, &Client::new(1));
        f.server.step();
        f.send(1, place([-1, G - 1, 2], 2, M.sand));
        f.server.step();
        f.server.drain_lod();
        assert_eq!(f.server.lod().sections().len(), MAX_LEVEL as usize);
        for (c, s) in f.server.lod().sections() {
            saved.insert(*c, s.revision);
        }
        f.server.save_now();
    }
    {
        // Restored as saved: nothing to recompute, the same revisions.
        let f = Fixture::new(ServerConfig {
            store: Some(open_store(&path)),
            ..flat()
        });
        assert_eq!(f.server.lod().pending(), 0);
        assert_eq!(f.server.lod().sections().len(), saved.len());
        for (c, s) in f.server.lod().sections() {
            assert_eq!(saved.get(c), Some(&s.revision), "{c:?}");
        }
    }
    // A world saved before the LOD cache existed (no rows): derived from its chunks.
    // (C++: `DELETE FROM lod_sections` over SQLite; here a save that clears the table.)
    {
        let db = WorldDb::open(&path).expect("opens");
        db.save(&SaveBatch {
            clear_lod: true,
            ..SaveBatch::default()
        })
        .expect("clears the LOD rows");
        assert!(db.lod_sections(GENERATOR_FLAT).0.is_empty());
    }
    {
        let mut f = Fixture::new(ServerConfig {
            store: Some(open_store(&path)),
            ..flat()
        });
        assert_eq!(f.server.lod().pending(), 1); // the edited chunk's level-1 section
        f.server.drain_lod();
        assert_eq!(f.server.lod().sections().len(), saved.len());
        f.server.save_now();
    }
    // Rows of another generator version: dropped and rebuilt from the chunks.
    // (C++: `UPDATE lod_sections SET generator_version = 99`. Rows take the generator version of
    // the batch's meta, so the rows are rewritten under meta version 99, then the meta restored.)
    {
        let db = WorldDb::open(&path).expect("opens");
        let meta = db.load_meta().expect("meta");
        let (rows, stale) = db.lod_sections(GENERATOR_FLAT);
        assert!(!stale);
        assert_eq!(rows.len(), saved.len());
        let stale_meta = dwell_storage::WorldMeta {
            generator_version: 99,
            ..meta.clone()
        };
        db.save(&SaveBatch {
            meta: Some(stale_meta),
            lod_sections: rows,
            clear_lod: true,
            ..SaveBatch::default()
        })
        .expect("rewrites the LOD rows");
        db.save(&SaveBatch {
            meta: Some(meta),
            ..SaveBatch::default()
        })
        .expect("restores the meta");
        let (rows, stale) = db.lod_sections(GENERATOR_FLAT);
        assert!(rows.is_empty() && stale);
    }
    {
        let mut f = Fixture::new(ServerConfig {
            store: Some(open_store(&path)),
            ..flat()
        });
        assert!(f.server.lod().sections().is_empty());
        assert_eq!(f.server.lod().pending(), 1);
        f.server.drain_lod();
        f.server.save_now();
        let (rows, stale) = f
            .server
            .config()
            .store
            .as_ref()
            .expect("store")
            .db()
            .lod_sections(GENERATOR_FLAT);
        assert_eq!(rows.len(), saved.len());
        assert!(!stale);
    }
}

/// A world holding a block that is not the build's (a mod's, ADR 0038) is served with it by the
/// next server on that file, even one that never registered it: clients learn it by sync.
#[test]
fn a_worlds_blocks_beyond_the_build_are_in_its_servers_registry() {
    let scratch = Scratch::new("mod");
    let path = scratch.world("mod");
    let moss = {
        let store = WorldStore::open_file(&path).expect("opens");
        let mut f = Fixture::new(ServerConfig {
            store: Some(store),
            ..flat()
        });
        let mut row = *dwell_blocks::info(M.stone);
        row.name = dwell_blocks::intern("mod:moss");
        let moss = f
            .server
            .register_block(dwell_blocks::NewBlock {
                name: "mod:moss".into(),
                properties: Vec::new(),
                states: vec![row],
            })
            .expect("registers");
        f.join(1, &Client::new(1));
        f.server.step();
        f.send(1, place([-1, G - 1, 2], 2, moss));
        f.server.step();
        assert_eq!(f.server.world().get_voxel(-1, G, 2), moss);
        f.move_to(1, 3.5, f64::from(G), 4.5);
        f.server.step();
        f.server.save_now();
        f.server.flush_store();
        moss
    };
    let store = WorldStore::open_file(&path).expect("reopens");
    let mut f = Fixture::new(ServerConfig {
        store: Some(store),
        ..flat()
    });
    assert_eq!(f.server.world().get_voxel(-1, G, 2), moss);
    let (payload, _) = f.server.registry();
    let p = dwell_blocks::sync::Payload::decode(&payload).expect("decodes");
    assert!(p.blocks.iter().any(|b| b.name == "mod:moss"));
}

#[test]
fn a_world_opened_at_a_spawn_puts_a_returning_player_there() {
    // A local world opened by a spawn link (`?spawn=portal`, `b`) on a file that is already there:
    // the link's spawn wins over the saved one, and the returning player starts at it, not where
    // it left the world (owner, 2026-10-27).
    // face-a-only: the spawn is a point the launch gives; which face it is on is the worldgen's.
    let scratch = Scratch::new("spawn-on-open");
    let path = scratch.world("spawn-on-open");
    {
        let mut f = Fixture::new(ServerConfig {
            store: Some(open_store(&path)),
            ..flat()
        });
        f.join(1, &Client::new(1));
        f.server.step();
        f.move_to(1, 3.5, f64::from(G) * dwell_world::CELL_H, 4.5);
        f.server.step();
        f.server.save_now();
        f.server.flush_store();
    }
    let feet = |f: &mut Fixture, id| {
        let h = f.server.player_handle_of(id).unwrap();
        let (_, physics, players) = f.server.sim();
        players.position(physics, h)
    };
    let at = [10.5, f64::from(G) * dwell_world::CELL_H, -6.5];
    let mut f = Fixture::new(ServerConfig {
        store: Some(open_store(&path)),
        spawn: Some(at),
        join_at_spawn: true,
        ..flat()
    });
    let w = f.join(1, &Client::new(1));
    let p = feet(&mut f, w.player_id);
    let [dx, dz] = dwell_sim::server::spawn_spread(w.player_id);
    assert!(
        (p.x - (at[0] + dx)).abs() < 1e-4 && (p.z - (at[2] + dz)).abs() < 1e-4,
        "at the spawn: {p:?}"
    );
    // Without the link, the saved place again.
    drop(f);
    let mut f = Fixture::new(ServerConfig {
        store: Some(open_store(&path)),
        ..flat()
    });
    let w = f.join(1, &Client::new(1));
    let p = feet(&mut f, w.player_id);
    assert!(
        (p.x - 3.5).abs() < 1e-4 && (p.z - 4.5).abs() < 1e-4,
        "{p:?}"
    );
}

#[test]
fn the_waters_basins_survive_a_server_restart() {
    // face-a-only: the server's flat world is face A's; the basins' saves on both faces are tested
    // in `water::tests`.
    // docs/WATER_FLOW.md §8: a lake an edit drained, saved, comes back as a basin at its level.
    let scratch = Scratch::new("water");
    let path = scratch.world("water");
    let stone = dwell_blocks::require("dwell:stone");
    let level = {
        let store = WorldStore::open_file(&path).expect("opens");
        let mut f = Fixture::new(ServerConfig {
            store: Some(store),
            ..flat()
        });
        f.join(1, &Client::new(1));
        f.server.step();
        // A walled lake four rows deep (x 2–6, z 3–8) in one chunk, its near wall at z 2.
        let w = f.server.world();
        for x in 1..=7 {
            for z in 2..=9 {
                for y in G..G + 4 {
                    let wall = x == 1 || x == 7 || z == 2 || z == 9;
                    w.set_voxel(x, y, z, if wall { stone } else { M.water });
                }
            }
        }
        f.send(
            1,
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Break,
                cell: [4, G + 3, 2],
                face: 5,
                material: 0,
                rows: 1,
                down: false,
            }),
        );
        for _ in 0..240 {
            f.server.step();
        }
        let lake = f.server.water().basin_at([4, G, 5]).expect("the lake");
        let level = lake.world_level();
        f.server.save_now();
        f.server.flush_store();
        f.server.step();
        level
    };
    let store = WorldStore::open_file(&path).expect("reopens");
    let f = Fixture::new(ServerConfig {
        store: Some(store),
        ..flat()
    });
    let lake = f
        .server
        .water()
        .basin_at([4, G, 5])
        .expect("the lake is a basin again");
    assert!(
        (lake.world_level() - level).abs() < 1e-9,
        "at {} (saved at {level})",
        lake.world_level()
    );
}

#[test]
fn a_world_from_an_older_line_opens_on_this_builds_terrain() {
    // Owner (2026-11-06): older-line worlds open in newer builds after a warning. A build knows
    // only its own terrain generator: a world saved under an older terrain generator number (21
    // here) generates its unexplored land with this build's terrain, not flat ground (generator_for
    // fell back to flat for any number it didn't know). Flat, sandbox and the retired playground
    // keep their own.
    for (saved, opened) in [
        (21, dwell_world::GENERATOR_TERRAIN),
        (
            dwell_world::GENERATOR_TERRAIN,
            dwell_world::GENERATOR_TERRAIN,
        ),
        (GENERATOR_FLAT, GENERATOR_FLAT),
        (
            dwell_world::GENERATOR_SANDBOX,
            dwell_world::GENERATOR_SANDBOX,
        ),
        (1, 1),
    ] {
        let scratch = Scratch::new("older-line");
        let path = scratch.world("older-line");
        {
            let db = WorldDb::open(&path).expect("opens");
            db.save(&SaveBatch {
                meta: Some(dwell_storage::WorldMeta {
                    world_seed: 5,
                    generator_version: saved,
                    world_tick: 0,
                    created_at: 0,
                    saved_at: 0,
                    spawn: Some([0.5, 40.0, 0.5]),
                    app_version_created: "0.40.0".into(),
                    app_version_last: "0.40.0".into(),
                    settings: None,
                }),
                ..SaveBatch::default()
            })
            .expect("saves");
        }
        let f = Fixture::new(ServerConfig {
            store: Some(open_store(&path)),
            ..flat()
        });
        assert_eq!(
            f.server.config().generator_version,
            opened,
            "saved under {saved}"
        );
    }
}
