//! Chisel in the game (Phase 56 B7, WORK_QUEUE item 34): the host retunes the world's worldgen
//! config and the chunks within a radius generate again under it, the player's edits kept on top;
//! the config is saved with the world and installed again when it opens; both faces.

use std::sync::Arc;

use dwell_sim::server::chisel::WORLDGEN_SETTING;
use dwell_sim::server::{Server, ServerConfig};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{Chunk, ChunkCoord, FaceFrame, GENERATOR_TERRAIN, chunk_of_pos};
use serde_json::Value;

const SEED: u64 = 0x0C15_E1B7;

fn biome(name: &str) -> Value {
    let p = format!(
        "{}/../../shared/worldgen/biomes/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The default biome with its meadow's ground sand: a biome slider's edit (the ground itself kept).
fn sand_meadow() -> String {
    let mut b = biome("default");
    b["bands"][0]["ground"] = "dwell:sand".into();
    serde_json::json!({ "biomes": [b] }).to_string()
}

fn server(db: &Arc<WorldDb>) -> Server {
    Server::new(
        ServerConfig {
            world_seed: SEED,
            generator_version: GENERATOR_TERRAIN,
            store: Some(WorldStore::new(db.clone(), false)),
            ..ServerConfig::default()
        },
        Box::new(dwell_sim::entropy::SeededEntropy(1)),
    )
}

fn generated(c: ChunkCoord) -> Chunk {
    let g = dwell_worldgen::generator_for(GENERATOR_TERRAIN, SEED);
    let mut ch = Chunk::default();
    g(c, &mut ch);
    ch
}

fn count(ch: &Chunk, state: dwell_blocks::StateId) -> usize {
    ch.voxels().iter().filter(|&&v| v == state).count()
}

#[test]
fn a_retune_regenerates_nearby_chunks_keeps_edits_and_is_saved_with_the_world() {
    // both-faces: the retune is one world's (both faces' maps at once), so one test walks both
    // faces in turn over the process-wide map cache rather than two tests racing on it.
    let sand = dwell_blocks::parse_state("dwell:sand").unwrap();
    let glass = dwell_blocks::parse_state("dwell:stone").unwrap();
    let text = sand_meadow();
    for f in [FaceFrame::A, FaceFrame::B] {
        // Each face's spawn, face-local (the faces are each other's mirror, with their own seeds).
        let spawn = f.point(if f == FaceFrame::A {
            dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, SEED)
        } else {
            dwell_worldgen::spawn_point_b_for(GENERATOR_TERRAIN, SEED)
        });
        // The build's ground to begin with (a re-wrap: nothing recomputed).
        dwell_worldgen::config::retune_worldgen(SEED, "").unwrap();
        let db = WorldDb::in_memory().unwrap();
        let mut s = server(&db);
        let ground = f.point([spawn[0], spawn[1] - 0.25, spawn[2]]);
        let surface = chunk_of_pos(ground);
        let up = chunk_of_pos(f.point([spawn[0], spawn[1] + 40.0, spawn[2]]));
        let far = ChunkCoord::new(surface.x + 6, surface.y, surface.z);
        let before = generated(surface);
        let sand_before = count(&before, sand);
        // A player's edit: a block in the air over the spawn, and one dug from the ground chunk.
        let cell = 5 + 32 * (7 + 32 * 9);
        {
            let ch = s.world().get_or_create(up);
            assert_eq!(
                ch.voxels()[cell],
                dwell_blocks::AIR,
                "face {f:?}: air to build in"
            );
            ch.voxels_mut()[cell] = glass;
            ch.bump_revision();
        }
        s.world().get_or_create(surface);
        // The host retunes, within 2 chunks.
        let r = s.retune_worldgen(&text, ground, 2).unwrap();
        assert!(r.chunks >= 2, "face {f:?}: {r:?}");
        assert_eq!(
            (r.edited, r.kept_cells),
            (1, 1),
            "face {f:?}: the edit kept"
        );
        assert_eq!(r.maps.len(), 2, "both faces' maps for the workers");
        let after = generated(surface);
        assert!(
            count(&after, sand) > sand_before,
            "face {f:?}: the new meadow's sand ({sand_before} before)"
        );
        assert_eq!(
            s.world().get_or_create(surface).voxels(),
            after.voxels(),
            "face {f:?}: the near chunk is the new config's"
        );
        assert_eq!(
            s.world().get_or_create(up).voxels()[cell],
            glass,
            "face {f:?}"
        );
        assert_eq!(s.worldgen_text(), text.trim());
        assert_eq!(db.setting(WORLDGEN_SETTING).as_deref(), Some(text.trim()));
        s.save_now();
        s.flush_store();
        drop(s);
        // Reopened in a fresh process: the build's maps installed, the world's config put back.
        dwell_worldgen::config::retune_worldgen(SEED, "").unwrap();
        assert_eq!(dwell_worldgen::config::installed_worldgen(SEED), "");
        let mut s = server(&db);
        assert_eq!(s.worldgen_text(), text.trim(), "face {f:?}: reopened");
        assert_eq!(
            dwell_worldgen::config::installed_worldgen(SEED),
            text.trim()
        );
        assert_eq!(
            s.world().get_or_create(up).voxels()[cell],
            glass,
            "face {f:?}: reopened"
        );
        assert_eq!(
            s.world().get_or_create(surface).voxels(),
            after.voxels(),
            "face {f:?}: reopened"
        );
        // A chunk outside the radius, never loaded: generated under the saved config.
        assert_eq!(
            s.world().get_or_create(far).voxels(),
            generated(far).voxels(),
            "face {f:?}: far"
        );
    }
    // An edit that moves the ground itself (the archipelago's world knobs) is a new world.
    let e = dwell_worldgen::config::retune_worldgen(SEED, "test:archipelago").unwrap_err();
    assert!(e.contains("ground itself"), "{e}");
    dwell_worldgen::config::retune_worldgen(SEED, "").unwrap();
}
