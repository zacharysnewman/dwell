//! The autosave's cost on the server's tick (ARCHITECTURE.md §6.4): in the browser the world file
//! has no I/O thread, so a save is encoded a little each tick and committed on a tick of its own.

mod common;

use common::*;
use dwell_sim::server::{ServerConfig, TICK_PHASES};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{CHUNK_VOLUME, ChunkCoord, M};

/// The "save" phase's index in `Server::tick_phases`.
fn save_phase() -> usize {
    TICK_PHASES
        .iter()
        .position(|&n| n == "save")
        .expect("a save phase")
}

/// `n` chunks round the spawn written outside the edit path, varied enough that encoding them is
/// real work (runs of four materials), and marked dirty; each with what it holds.
fn dirty_chunks(f: &mut Fixture, n: i32) -> Vec<(ChunkCoord, Vec<u16>)> {
    let materials = [M.stone, M.dirt, M.sand, M.grass];
    let mut coords = Vec::new();
    for k in 0..n {
        let c = ChunkCoord {
            x: k % 16 - 8,
            y: k / 256,
            z: (k / 16) % 16 - 8,
        };
        let chunk = f.server.world().get_or_create(c);
        let voxels = chunk.voxels_mut();
        let mut seed = (k as u32).wrapping_mul(2_654_435_761) | 1;
        let mut i = 0;
        while i < CHUNK_VOLUME {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let run = 1 + (seed % 7) as usize;
            let m = materials[(seed >> 8) as usize % materials.len()];
            for v in voxels.iter_mut().skip(i).take(run) {
                *v = m;
            }
            i += run;
        }
        coords.push((c, voxels.to_vec()));
        f.server.mark_dirty(c);
    }
    coords
}

#[test]
fn a_save_of_many_dirty_chunks_costs_no_tick_more_than_a_few_ms() {
    // face-a-only: the save path has no up; the chunks are written straight to the world.
    // As the browser's server saves (an inline store, no I/O thread): 300 dirty chunks were one
    // tick of encoding and committing (red: 106 ms in a dev build); now ≤ 2 ms of encoding a
    // tick, then the commit on a tick of its own.
    let db = WorldDb::in_memory().expect("opens");
    let mut f = Fixture::new(ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        autosave_seconds: 1,
        ..flat()
    });
    f.join(1, &Client::new(1));
    f.server.step();
    f.take_all();
    let coords = dirty_chunks(&mut f, 300);
    assert_eq!(f.server.save_stats().dirty, 300);
    let saves = f.server.save_stats().saves;
    f.server.tick_phases();
    let (mut worst, mut ticks) = (0.0f64, 0);
    while f.server.save_stats().saves == saves {
        f.server.step();
        f.take_all();
        worst = worst.max(f.server.tick_phases()[save_phase()].1);
        ticks += 1;
        assert!(ticks < 5 * 60, "the save never completed");
    }
    println!("the save's worst tick: {worst:.2} ms");
    assert!(
        worst < 15.0,
        "a save tick took {worst:.1} ms of the server's tick"
    );
    // And it is all there.
    assert_eq!(f.server.save_stats().dirty, 0);
    for (c, voxels) in coords {
        assert_eq!(db.load_chunk(c).expect("saved").voxels, voxels, "{c:?}");
    }
}
