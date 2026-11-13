//! The Realistic world's spawn (BUGS.md #132; docs/SHRINES.md §4): on seeds 1, 7 and 42 each face's
//! spawn resolves within a time budget, at a shrine (the one nearest the spawn search's point,
//! within `spawnWithin`), as every spawn is (owner, 2026-11-12). And the spawn shrine's cache key
//! is cheap: it hashed the whole biome set's `Debug` text, its whole-map grids included (134 ms a
//! face natively on Realistic, again for every generator that resolves the spawn's shrine).

use std::time::{Duration, Instant};

use dwell_world::Face;
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::worldgen_map_bytes;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 3] = [1, 7, 42];

/// The whole spawn's budget a face (the search, the shrine's siting and its stand point). Natively
/// in release it takes 0.1–0.25 s; the budget leaves room for a busy CI runner.
const SPAWN_BUDGET: Duration = Duration::from_secs(2);

/// Installs the Realistic world's maps of every seed here (once per process).
fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for seed in SEEDS {
            for f in [Face::A, Face::B] {
                let bytes =
                    worldgen_map_bytes(face_seed(seed, f), NAME).expect("the Realistic map");
                dwell_worldgen::worldmap::install(&bytes).expect("installs");
            }
        }
    });
}

dwell_world::both_faces!(
    /// Each seed's spawn on face `f` resolves within the budget and stands at the shrine nearest
    /// the spawn search's point.
    fn the_realistic_spawn_resolves_in_budget_at_a_shrine(f: FaceFrame) {
        install();
        for seed in SEEDS {
            let g = BifacialTerrain::new(seed);
            let t = g.local(f.face);
            assert_eq!(
                t.map().worldgen_text(),
                NAME,
                "seed {seed}: the Realistic map is installed"
            );
            let start = Instant::now();
            let spawn = match f.face {
                Face::A => g.spawn_point(),
                Face::B => g.spawn_point_b(),
            };
            let took = start.elapsed();
            assert!(
                took < SPAWN_BUDGET,
                "seed {seed}: the spawn took {took:?} (budget {SPAWN_BUDGET:?})"
            );
            let sh = t.shrines().expect("the Realistic world has shrines");
            let [px, _, pz] = t.spawn_search_point();
            let s = sh.spawn_shrine(t);
            let d = (f64::from(s.x) + 0.5 - px).hypot(f64::from(s.z) + 0.5 - pz);
            assert!(
                d <= f64::from(sh.rules.spawn_within) + 1.0,
                "seed {seed}: the spawn's shrine is {d:.0} m from the search's point"
            );
            let (near, feet) = g
                .nearest_shrine(f.face, px, pz)
                .expect("the spawn's shrine");
            assert_eq!(near, s, "seed {seed}");
            assert_eq!(spawn, feet, "seed {seed}: the spawn stands at its shrine");
        }
    }
);

/// The spawn shrine's cache key costs little on the Realistic world: it names the generator's
/// config, not its derived whole-map grids. Red before the fix: 134 ms a face natively.
// face-a-only: the key is per face but built by the same code; face A's biome set is the larger.
#[test]
fn the_spawn_keys_cost_is_small_on_realistic() {
    install();
    let g = BifacialTerrain::new(42);
    let t = g.local(Face::A);
    // The best of three (a wall-clock bar on a shared runner).
    let best = (0..3)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(t.spawn_key());
            start.elapsed()
        })
        .min()
        .expect("three runs");
    assert!(
        best < Duration::from_millis(20),
        "the spawn key took {best:?}"
    );
}
