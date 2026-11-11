//! World files (§6.4): everything round-trips, saves are atomic, chunks mean their strings, and the
//! LOD cache is only a cache.

use std::path::PathBuf;

use dwell_blocks::{STATES, require};
use dwell_storage::*;
use dwell_world::ChunkCoord;
use dwell_world::lod::LodCoord;
use redb::{Database, TableDefinition};

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const BLOCK_STATES: TableDefinition<u32, &str> = TableDefinition::new("block_states");

fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dwell-storage-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&dir);
    dir
}

fn chunk_of(m: u16) -> Vec<u16> {
    let mut v = vec![require("dwell:air"); dwell_protocol::CHUNK_VOLUME];
    for (i, x) in v.iter_mut().enumerate().take(5000) {
        *x = if i % 7 == 0 { require("dwell:dirt") } else { m };
    }
    v
}

fn meta() -> WorldMeta {
    WorldMeta {
        world_seed: 0xDEAD_BEEF_0123,
        generator_version: 8,
        spawn: Some([1.5, 2.0, -3.25]),
        world_tick: 1234,
        created_at: 1_700_000_000,
        saved_at: 0,
        app_version_created: "0.9.0".into(),
        app_version_last: "0.9.1".into(),
        settings: None,
    }
}

#[test]
fn a_new_world_is_at_the_current_format_and_has_no_meta() {
    let db = WorldDb::in_memory().expect("opens");
    assert_eq!(db.format_version(), FORMAT_VERSION);
    assert_eq!(db.load_meta(), None);
    assert!(db.chunk_index().is_empty());
}

#[test]
fn everything_round_trips_through_a_file() {
    let path = temp_path("round-trip");
    let key = [9u8; 32];
    {
        let db = WorldDb::open(&path).expect("creates");
        let batch = SaveBatch {
            meta: Some(meta()),
            chunks: vec![SavedChunk {
                coord: ChunkCoord::new(1, -2, 3),
                revision: 7,
                voxels: chunk_of(require("dwell:sand")),
            }],
            players: vec![PlayerRecord {
                key,
                display_name: "Zack".into(),
                feet: [10.0, 20.0, 30.0],
                health: 80,
                first_seen: 100,
                last_seen: 200,
            }],
            lod_sections: vec![
                SavedLodSection {
                    coord: LodCoord::new(1, 2, 3, 4),
                    revision: 5,
                    dirty: false,
                    encoded: vec![1, 2, 3],
                },
                SavedLodSection {
                    coord: LodCoord::new(2, 1, 1, 1),
                    revision: 6,
                    dirty: true,
                    encoded: vec![],
                },
            ],
            clear_lod: false,
            bodies: None,
            water: None,
            inventories: vec![(key, vec![1, 2, 3])],
            vitals: vec![(key, vec![1, 0, 0, 200, 66, 0, 0, 160, 64, 0])],
            loose_items: Some(vec![9, 8]),
            cuts: Some(vec![1, 0, 0, 0, 0]),
            ground_gone: Some(vec![1, 1, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
            damage: Some(vec![1, 0, 0, 0, 0]),
            tree_growth: Some(vec![1, 0, 0, 0, 0]),
        };
        db.save(&batch).expect("saves");
        db.set_setting("motd", "hello").expect("setting");
        db.set_permission(&PermissionEntry {
            key,
            kind: PermissionKind::Op,
            reason: "owner".into(),
            by: None,
            at: 5,
        })
        .expect("permission");
        db.set_permission(&PermissionEntry {
            key: [1; 32],
            kind: PermissionKind::Ban,
            reason: String::new(),
            by: Some(key),
            at: 6,
        })
        .expect("permission");
    }
    let db = WorldDb::open(&path).expect("reopens");
    let mut m = db.load_meta().expect("meta");
    assert_ne!(m.saved_at, 0);
    m.saved_at = 0;
    assert_eq!(m, meta());
    assert_eq!(db.chunk_index(), vec![(ChunkCoord::new(1, -2, 3), 7)]);
    let c = db.load_chunk(ChunkCoord::new(1, -2, 3)).expect("chunk");
    assert_eq!(c.voxels, chunk_of(require("dwell:sand")));
    assert_eq!(db.load_chunk(ChunkCoord::new(0, 0, 0)), None);
    let p = db.load_player(&key).expect("player");
    assert_eq!(
        (p.feet, p.health, p.first_seen, p.last_seen),
        ([10.0, 20.0, 30.0], 80, 100, 200)
    );
    assert_eq!(db.setting("motd").as_deref(), Some("hello"));
    // Inventories and loose items (ADR 0066): their own tables, records as the server gave them.
    assert_eq!(db.load_inventory(&key), Some(vec![1, 2, 3]));
    assert_eq!(
        db.load_vitals(&key),
        Some(vec![1, 0, 0, 200, 66, 0, 0, 160, 64, 0])
    );
    assert_eq!(db.load_inventory(&[1; 32]), None);
    assert_eq!(db.loose_items(), Some(vec![9, 8]));
    assert_eq!(db.cuts(), Some(vec![1, 0, 0, 0, 0]));
    assert_eq!(db.damage(), Some(vec![1, 0, 0, 0, 0]));
    assert_eq!(db.tree_growth(), Some(vec![1, 0, 0, 0, 0]));
    assert_eq!(
        db.ground_gone(),
        Some(vec![1, 1, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0])
    );
    let mut perms = db.permissions();
    perms.sort_by_key(|p| p.at);
    assert_eq!(perms.len(), 2);
    assert_eq!(
        (perms[0].kind, perms[1].by),
        (PermissionKind::Op, Some(key))
    );
    db.remove_permission(&key, PermissionKind::Op)
        .expect("removes");
    assert_eq!(db.permissions().len(), 1);
    let (lod, stale) = db.lod_sections(8);
    assert!(!stale);
    assert_eq!(lod.len(), 2);
    let (lod, stale) = db.lod_sections(9);
    assert!(stale && lod.is_empty());
    drop(db);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn saving_again_keeps_the_creation_and_first_seen() {
    let db = WorldDb::in_memory().expect("opens");
    let key = [3u8; 32];
    let player = |first, last| PlayerRecord {
        key,
        display_name: "A".into(),
        feet: [0.0; 3],
        health: 1,
        first_seen: first,
        last_seen: last,
    };
    db.save(&SaveBatch {
        meta: Some(meta()),
        players: vec![player(100, 100)],
        ..Default::default()
    })
    .expect("save");
    let mut later = meta();
    later.created_at = 5;
    later.app_version_created = "9.9.9".into();
    later.app_version_last = "1.0.0".into();
    db.save(&SaveBatch {
        meta: Some(later),
        players: vec![player(999, 300)],
        ..Default::default()
    })
    .expect("save");
    let m = db.load_meta().expect("meta");
    assert_eq!(
        (
            m.created_at,
            m.app_version_created.as_str(),
            m.app_version_last.as_str()
        ),
        (1_700_000_000, "0.9.0", "1.0.0")
    );
    let p = db.load_player(&key).expect("player");
    assert_eq!((p.first_seen, p.last_seen), (100, 300));
}

#[test]
fn a_failed_save_leaves_the_previous_one() {
    let mut db = WorldDb::in_memory().expect("opens");
    let first = SaveBatch {
        meta: Some(meta()),
        chunks: vec![SavedChunk {
            coord: ChunkCoord::new(0, 0, 0),
            revision: 1,
            voxels: chunk_of(require("dwell:stone")),
        }],
        ..Default::default()
    };
    db.save(&first).expect("first save");
    std::sync::Arc::get_mut(&mut db)
        .expect("unshared")
        .before_commit = Some(Box::new(|| Err("crash".into())));
    let mut second = first.clone();
    second.chunks[0].revision = 2;
    second.chunks[0].voxels = chunk_of(require("dwell:sand"));
    second.chunks.push(SavedChunk {
        coord: ChunkCoord::new(5, 5, 5),
        revision: 1,
        voxels: chunk_of(require("dwell:grass")),
    });
    assert_eq!(db.save(&second), Err("crash".into()));
    assert_eq!(db.chunk_index(), vec![(ChunkCoord::new(0, 0, 0), 1)]);
    assert_eq!(
        db.load_chunk(ChunkCoord::new(0, 0, 0))
            .expect("chunk")
            .voxels,
        chunk_of(require("dwell:stone"))
    );
    // The aborted save's new block states were not recorded either.
    assert!(!db.block_states().iter().any(|s| s == "dwell:grass"));
    assert_eq!(WorldDb::integrity_check(&mut db), Ok(true));
}

#[test]
fn saves_commit_on_the_store_thread() {
    let mut store = WorldStore::new(WorldDb::in_memory().expect("opens"), true);
    assert!(store.threaded());
    let ids: Vec<u64> = (0..5)
        .map(|i| {
            store.save(SaveBatch {
                chunks: vec![SavedChunk {
                    coord: ChunkCoord::new(i, 0, 0),
                    revision: 1,
                    voxels: chunk_of(require("dwell:sand")),
                }],
                ..Default::default()
            })
        })
        .collect();
    store.flush();
    let done = store.take_completed();
    assert_eq!(done.iter().map(|r| r.id).collect::<Vec<_>>(), ids);
    assert!(done.iter().all(|r| r.ok));
    assert_eq!(store.db().chunk_index().len(), 5);
}

#[test]
fn an_inline_save_is_encoded_a_piece_a_step_and_committed_on_a_step_of_its_own() {
    // The browser's store (no thread, §6.4): nothing is written until the whole batch is encoded
    // and committed; a store dropped mid-save (a tab closed) leaves the last complete save, and
    // flush commits what is left at once.
    let db = WorldDb::in_memory().expect("opens");
    let mut store = WorldStore::new(db.clone(), false);
    let chunk = |x, m| SavedChunk {
        coord: ChunkCoord::new(x, 0, 0),
        revision: 1,
        voxels: chunk_of(require(m)),
    };
    let first = store.save(SaveBatch {
        chunks: vec![chunk(0, "dwell:stone")],
        ..Default::default()
    });
    store.flush();
    assert_eq!(store.take_completed().first().map(|r| r.id), Some(first));
    let second = store.save(SaveBatch {
        chunks: (1..4).map(|x| chunk(x, "dwell:sand")).collect(),
        water: Some(vec![7; 100]),
        ..Default::default()
    });
    assert!(store.pending());
    // One piece a step while there is no time, then the commit alone.
    let mut steps = 0;
    while store.step(|| false) {
        steps += 1;
        assert!(store.take_completed().is_empty(), "committed mid-encoding");
        assert_eq!(db.chunk_index().len(), 1, "written before the commit");
        assert!(steps < 10);
    }
    assert_eq!(steps, 4, "three chunks and the water, then the commit");
    let done = store.take_completed();
    assert_eq!(
        done.iter().map(|r| (r.id, r.ok)).collect::<Vec<_>>(),
        [(second, true)]
    );
    assert_eq!(db.chunk_index().len(), 4);
    assert_eq!(db.water(), Some(vec![7; 100]));
    // Dropped mid-save: a store's drop commits what is queued (closing a world)…
    store.save(SaveBatch {
        chunks: vec![chunk(9, "dwell:grass")],
        ..Default::default()
    });
    assert!(store.step(|| false));
    drop(store);
    assert_eq!(db.chunk_index().len(), 5);
    // …and a save prepared but never committed (a crash) leaves the last complete one.
    let mut prepared = db.prepare(SaveBatch {
        chunks: vec![chunk(10, "dwell:dirt")],
        ..Default::default()
    });
    while prepared.encode_step() {}
    drop(prepared);
    assert_eq!(db.chunk_index().len(), 5);
}

#[test]
fn a_chunk_means_its_strings_not_the_runtime_ids() {
    let path = temp_path("strings");
    {
        let db = WorldDb::open(&path).expect("creates");
        let voxels = chunk_of(require("dwell:sand"));
        db.save(&SaveBatch {
            chunks: vec![SavedChunk {
                coord: ChunkCoord::new(0, 0, 0),
                revision: 1,
                voxels,
            }],
            ..Default::default()
        })
        .expect("save");
        // World state ids are assigned in order of first appearance, not runtime ids.
        assert_eq!(
            db.block_states(),
            vec![
                "dwell:dirt[flooded=false]",
                "dwell:sand[flooded=false]",
                "dwell:air"
            ]
        );
    }
    {
        // Another build that numbered states differently wrote this file: swap two strings.
        let raw = Database::create(&path).expect("raw");
        let txn = raw.begin_write().expect("write");
        {
            let mut t = txn.open_table(BLOCK_STATES).expect("table");
            t.insert(0, "dwell:sand[flooded=false]").expect("insert");
            t.insert(1, "dwell:dirt[flooded=false]").expect("insert");
        }
        txn.commit().expect("commit");
    }
    let db = WorldDb::open(&path).expect("reopens");
    let c = db.load_chunk(ChunkCoord::new(0, 0, 0)).expect("chunk");
    assert_eq!(c.voxels[0], require("dwell:sand")); // was dirt: the file says id 0 is sand now
    assert_eq!(c.voxels[1], require("dwell:dirt"));
    drop(db);
    {
        // A state this build does not know (a mod's block, ADR 0038).
        let raw = Database::create(&path).expect("raw");
        let txn = raw.begin_write().expect("write");
        txn.open_table(BLOCK_STATES)
            .expect("table")
            .insert(0, "mod:unobtainium[lit=true]")
            .expect("insert");
        txn.commit().expect("commit");
    }
    // It loads as a placeholder under that exact string (a solid, indestructible cube), and
    // saving writes the string back unchanged.
    let db = WorldDb::open(&path).expect("reopens");
    let mut c = db
        .load_chunk(ChunkCoord::new(0, 0, 0))
        .expect("kept, not regenerated");
    let p = c.voxels[0];
    let row = dwell_blocks::info(p);
    assert_eq!(row.name, "mod:unobtainium[lit=true]");
    assert!(row.solid && row.indestructible && !row.placeable);
    assert_eq!(db.placeholders(), vec![p]);
    assert_eq!(c.voxels[1], require("dwell:dirt"));
    c.voxels[1] = require("dwell:stone");
    c.revision = 2;
    db.save(&SaveBatch {
        chunks: vec![c],
        ..Default::default()
    })
    .expect("save");
    drop(db);
    let db = WorldDb::open(&path).expect("reopens");
    assert!(
        db.block_states()
            .contains(&"mod:unobtainium[lit=true]".to_string())
    );
    let c = db.load_chunk(ChunkCoord::new(0, 0, 0)).expect("chunk");
    assert_eq!(
        dwell_blocks::info(c.voxels[0]).name,
        "mod:unobtainium[lit=true]"
    );
    assert_eq!(c.voxels[0], p, "the same placeholder, not a second one");
    assert_eq!(c.voxels[1], require("dwell:stone"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_state_with_an_unknown_property_loads_as_its_base_and_saves_back_unchanged() {
    // ADR 0063: a newer build's (or a mod's) property on a known block stays on the block.
    let path = temp_path("alias");
    let text = "dwell:leaves[flooded=true,mymod:wet=7]";
    {
        let db = WorldDb::open(&path).expect("creates");
        db.save(&SaveBatch {
            chunks: vec![SavedChunk {
                coord: ChunkCoord::new(0, 0, 0),
                revision: 1,
                voxels: chunk_of(require("dwell:sand")),
            }],
            ..Default::default()
        })
        .expect("save");
    }
    {
        // A newer build wrote id 0 (the dirt) with a property this build doesn't have.
        let raw = Database::create(&path).expect("raw");
        let txn = raw.begin_write().expect("write");
        txn.open_table(BLOCK_STATES)
            .expect("table")
            .insert(0, text)
            .expect("insert");
        txn.commit().expect("commit");
    }
    let db = WorldDb::open(&path).expect("reopens");
    let mut c = db.load_chunk(ChunkCoord::new(0, 0, 0)).expect("kept");
    let a = c.voxels[0];
    let base = require("dwell:leaves[flooded=true]");
    assert_eq!(
        dwell_blocks::alias_base(a),
        Some(base),
        "an alias of the leaves"
    );
    let row = dwell_blocks::info(a);
    assert_eq!(row.name, text);
    assert_eq!(
        (
            row.solid,
            row.passable,
            row.flooded,
            row.render,
            row.shape_index
        ),
        {
            let b = dwell_blocks::info(base);
            (b.solid, b.passable, b.flooded, b.render, b.shape_index)
        },
        "it behaves as dwell:leaves"
    );
    c.voxels[2] = a;
    c.revision = 2;
    db.save(&SaveBatch {
        chunks: vec![c],
        ..Default::default()
    })
    .expect("save");
    drop(db);
    let db = WorldDb::open(&path).expect("reopens");
    assert!(db.block_states().contains(&text.to_string()));
    let c = db.load_chunk(ChunkCoord::new(0, 0, 0)).expect("chunk");
    assert_eq!(
        dwell_blocks::info(c.voxels[0]).name,
        text,
        "saved back unchanged"
    );
    assert_eq!(c.voxels[2], a);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn files_from_a_newer_build_are_refused() {
    let path = temp_path("newer");
    drop(WorldDb::open(&path).expect("creates"));
    {
        let raw = Database::create(&path).expect("raw");
        let txn = raw.begin_write().expect("write");
        txn.open_table(META)
            .expect("table")
            .insert(
                "format_version",
                (FORMAT_VERSION + 1).to_le_bytes().as_slice(),
            )
            .expect("insert");
        txn.commit().expect("commit");
    }
    let e = WorldDb::open(&path).expect_err("refused");
    assert!(e.contains("newer than this build"), "{e}");
    std::fs::write(&path, b"not a database at all").expect("write");
    assert!(WorldDb::open(&path).is_err());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_lod_cache_is_dropped_when_the_registry_changes() {
    let path = temp_path("lod-cache");
    {
        let db = WorldDb::open(&path).expect("creates");
        db.save(&SaveBatch {
            meta: Some(meta()),
            lod_sections: vec![SavedLodSection {
                coord: LodCoord::new(1, 0, 0, 0),
                revision: 1,
                dirty: false,
                encoded: vec![7],
            }],
            ..Default::default()
        })
        .expect("save");
    }
    assert_eq!(
        WorldDb::open(&path)
            .expect("reopens")
            .lod_sections(8)
            .0
            .len(),
        1
    );
    {
        let raw = Database::create(&path).expect("raw");
        let txn = raw.begin_write().expect("write");
        txn.open_table(META)
            .expect("table")
            .insert("registry_hash", 1u64.to_le_bytes().as_slice())
            .expect("insert");
        txn.commit().expect("commit");
    }
    let db = WorldDb::open(&path).expect("reopens");
    assert!(db.lod_sections(8).0.is_empty());
    assert!(db.load_meta().is_some(), "only the cache goes");
    let _ = std::fs::remove_file(&path);
    let _ = STATES.len();
}

#[test]
fn peeking_a_world_files_meta_writes_nothing() {
    let path = temp_path("untouched");
    {
        let db = WorldDb::open(&path).unwrap();
        db.save(&SaveBatch {
            meta: Some(meta()),
            ..SaveBatch::default()
        })
        .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    // The version lock reads the meta first, read-only (RELEASES.md §6)…
    let peeked = WorldDb::peek_meta(&path).unwrap().expect("meta");
    assert_eq!(peeked.world_seed, meta().world_seed);
    // (Opening for writing always rewrites redb's header: open and closed flags.)
    assert!(
        std::fs::read(&path).unwrap() == before,
        "peek_meta modified the file"
    );
    assert!(WorldDb::peek_meta(&temp_path("missing")).unwrap().is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn moving_bodies_are_saved_with_the_world_and_replaced_on_each_save() {
    // docs/BUILT_BODIES.md §7 (owner, 2026-11-02: built objects "saved with the world"): a body's
    // kind, pose, velocities and cells come back as saved, its states by name; a save with bodies
    // replaces those saved before, one without leaves them.
    let db = WorldDb::in_memory().expect("db");
    let log = require("dwell:log[axis=x]");
    let leaves = require("dwell:leaves");
    let raft = SavedBody {
        id: 7,
        kind: 1,
        settles: 0,
        position: [12.5, 40.25, -3.5],
        rotation: [0.0, 0.38268343236508984, 0.0, 0.9238795325112867],
        velocity: [0.5, 0.0, -0.25],
        angular: [0.0, 0.1, 0.0],
        cells: vec![([0, 0, 0], log), ([1, 0, 0], log), ([0, 1, 0], leaves)],
    };
    let tree = SavedBody {
        id: 9,
        cells: vec![([0, 0, 0], log)],
        ..SavedBody::default()
    };
    db.save(&SaveBatch {
        bodies: Some(vec![raft.clone(), tree.clone()]),
        ..SaveBatch::default()
    })
    .expect("saved");
    assert_eq!(db.bodies(), vec![raft.clone(), tree]);
    // A save without bodies leaves them; one with replaces them.
    db.save(&SaveBatch::default()).expect("saved");
    assert_eq!(db.bodies().len(), 2);
    db.save(&SaveBatch {
        bodies: Some(vec![raft.clone()]),
        ..SaveBatch::default()
    })
    .expect("saved");
    assert_eq!(db.bodies(), vec![raft]);
}

#[test]
fn the_waters_record_is_saved_with_the_world_and_replaced_on_each_save() {
    // WATER_SURFACE_GRID.md §2b: the water's record comes back as saved; a save without it leaves it,
    // one with replaces it; a world from before it has none.
    let db = WorldDb::in_memory().expect("db");
    assert_eq!(db.water(), None);
    let a: Vec<u8> = (0..5000u32).map(|i| (i % 7) as u8).collect();
    db.save(&SaveBatch {
        water: Some(a.clone()),
        ..SaveBatch::default()
    })
    .expect("saved");
    assert_eq!(db.water().as_ref(), Some(&a));
    db.save(&SaveBatch::default()).expect("saved");
    assert_eq!(db.water().as_ref(), Some(&a));
    db.save(&SaveBatch {
        water: Some(vec![1, 2, 3]),
        ..SaveBatch::default()
    })
    .expect("saved");
    assert_eq!(db.water(), Some(vec![1, 2, 3]));
}
