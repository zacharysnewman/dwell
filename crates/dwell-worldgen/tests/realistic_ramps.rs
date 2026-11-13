//! Notched ramps only at mesa edges (WORK_QUEUE item 15; owner, 2026-11-13: "Now it seems like it's
//! only slopes, what happened?" … "I'd like the old rule to apply except for mesa edges"). 0.59 set
//! rocks.json `shaping.notched` on everywhere the rock shapes the ground, so the stored ramps also
//! topped every diagonal contour and rolling Realistic ground became nearly all slopes. Now it is
//! `mesaEdges`: a column takes notched ramps only where it or a neighbour shows a rock wall (an
//! erosion mesa's scarp, a hard bed's wall), the pre-0.59 straight-edge rule elsewhere. On seeds 1,
//! 7, 42, both faces, over transects of the land:
//!
//! - off the walls' edges the ramps are exactly the old rule's (`none`);
//! - at the walls' edges some columns take notched ramps the old rule leaves flat;
//! - the share of ramped columns off the edges is the old rule's, well under 0.59's (`all`).

use std::sync::Arc;

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::geology::Notched;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 3] = [1, 7, 42];

/// The Realistic generator over `map` with notched ramps by `notched` (None: the build's data).
fn generator(seed: u64, map: &Arc<WorldMap>, notched: Option<Notched>) -> TerrainGenerator {
    let mut e = world_config(NAME).unwrap().expect("the Realistic world");
    if let Some(n) = notched {
        e.geology.notched = n;
    }
    TerrainGenerator::with_map(seed, map.clone())
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

/// Per rule (none, the build's: mesaEdges, all): (ramped columns off the walls' edges, of them sampled) and
/// (columns at an edge where mesaEdges ramps but none does not, edge columns sampled).
#[derive(Default, Debug)]
struct Census {
    off: [usize; 3],
    off_n: usize,
    edge_notched: usize,
    edge_n: usize,
    off_differ: usize,
}

fn census(world: u64, f: FaceFrame) -> Census {
    let seed = face_seed(world, f.face);
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    // The old rule, the build's data (rocks.json `shaping.notched`), and 0.59's.
    let g = [Some(Notched::None), None, Some(Notched::All)].map(|n| generator(seed, &map, n));
    let mut c = Census::default();
    for z in (-44_000..=44_000).step_by(2000) {
        for x in (-44_000..=44_000).step_by(2000) {
            for i in 0..24 {
                let (cx, cz) = (x + i, z);
                let col = g[1].column_at(cx, cz);
                if col.outside || col.height < 2.0 {
                    continue;
                }
                let edge =
                    (-1..=1).any(|dz| (-1..=1).any(|dx| g[1].column_at(cx + dx, cz + dz).wall));
                let ramps = [0, 1, 2].map(|k| g[k].ramp_pieces_at(cx, cz));
                if edge {
                    c.edge_n += 1;
                    c.edge_notched += usize::from(ramps[1] > 0 && ramps[0] == 0);
                } else {
                    c.off_n += 1;
                    for k in 0..3 {
                        c.off[k] += usize::from(ramps[k] > 0);
                    }
                    c.off_differ += usize::from(ramps[1] != ramps[0]);
                }
            }
        }
    }
    c
}

both_faces!(
    fn notched_ramps_stand_only_at_mesa_edges(f: FaceFrame) {
        assert_eq!(
            world_config(NAME).unwrap().unwrap().geology.notched,
            Notched::MesaEdges,
            "rocks.json shaping.notched"
        );
        let (mut notched, mut off, mut off_all, mut off_n) = (0, 0, 0, 0);
        for world in SEEDS {
            let c = census(world, f);
            let share = |k: usize| c.off[k] as f64 / c.off_n.max(1) as f64;
            eprintln!(
                "seed {world} {:?}: off the edges {} columns, ramped: old rule {:.1} %, mesaEdges {:.1} %, 0.59 (all) {:.1} %; at the edges {} columns, {} notched",
                f.face,
                c.off_n,
                100.0 * share(0),
                100.0 * share(1),
                100.0 * share(2),
                c.edge_n,
                c.edge_notched
            );
            assert!(
                c.off_n > 5000,
                "seed {world}: enough land sampled ({})",
                c.off_n
            );
            // Off the walls' edges: the old rule exactly.
            assert_eq!(
                c.off_differ, 0,
                "seed {world}: notched ramps off the mesa edges"
            );
            notched += c.edge_notched;
            (off, off_all, off_n) = (off + c.off[1], off_all + c.off[2], off_n + c.off_n);
        }
        // At the edges they are notched (the old rule leaves those columns flat).
        assert!(
            notched > 0,
            "{:?}: no notched ramp at any mesa edge",
            f.face
        );
        // Rolling ground: well under 0.59's share of ramps.
        assert!(
            (off as f64) < 0.8 * off_all as f64,
            "{:?}: ramped {off} of {off_n} off the edges, 0.59's {off_all}",
            f.face
        );
    }
);
