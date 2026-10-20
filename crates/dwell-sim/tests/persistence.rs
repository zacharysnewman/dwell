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
    })
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
            .find(ChunkCoord::new(-1, 1, 0))
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
        .find(|m| m.coord == [-1, 1, 0])
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
    f.move_to(1, 500.5, f64::from(G), 0.5);
    for _ in 0..130 {
        f.server.step();
    }
    assert!(f.server.world().find(ChunkCoord::new(-1, 1, 0)).is_none());
    f.move_to(1, 0.5, f64::from(G), 0.5);
    for _ in 0..3 {
        f.server.step();
    }
    assert!(f.server.world().find(ChunkCoord::new(-1, 1, 0)).is_some());
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
