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
            vec!["dwell:dirt", "dwell:sand", "dwell:air"]
        );
    }
    {
        // Another build that numbered states differently wrote this file: swap two strings.
        let raw = Database::create(&path).expect("raw");
        let txn = raw.begin_write().expect("write");
        {
            let mut t = txn.open_table(BLOCK_STATES).expect("table");
            t.insert(0, "dwell:sand").expect("insert");
            t.insert(1, "dwell:dirt").expect("insert");
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
