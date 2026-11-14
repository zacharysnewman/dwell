//! The original straight-edge ramp rule everywhere in the Realistic world, mesa edges included
//! (WORK_QUEUE item 15, 0.66; owner, 2026-11-14: 'Im not sure I want to "fix" the step look yet.
//! Isn't that the original rule showing?' then "Let's put it back in for everything not excluding
//! mesa edges"). 0.59–0.61 notched every diagonal contour where the rock shapes the ground (`all`),
//! 0.62–0.65 only at a rock wall's edge (`mesaEdges`); now rocks.json `shaping.notched` is `none`,
//! the ramp backing (BUGS.md #163) kept. On seeds 1, 7, 42, both faces, over transects of the land:
//!
//! - every column's ramps are the straight-edge rule's (`none`), at the walls' edges too;
//! - 0.65's `mesaEdges` notched some columns at the edges (the red half: they differ there).

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

/// Per rule (none, the build's, 0.65's mesaEdges): (ramped columns off the walls' edges, of them sampled) and
/// (columns at an edge where mesaEdges ramps but none does not, edge columns sampled).
#[derive(Default, Debug)]
struct Census {
    off: [usize; 3],
    off_n: usize,
    edge_notched: usize,
    edge_n: usize,
    off_differ: usize,
    /// Edge columns where 0.65's mesaEdges differs from the straight-edge rule.
    edge_065: usize,
    /// Ramped edge columns per rule.
    edge_ramped: [usize; 3],
}

fn census(world: u64, f: FaceFrame) -> Census {
    let seed = face_seed(world, f.face);
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    // The old rule, the build's data (rocks.json `shaping.notched`), and 0.65's.
    let g = [Some(Notched::None), None, Some(Notched::MesaEdges)].map(|n| generator(seed, &map, n));
    let mut c = Census::default();
    for z in (-44_000..=44_000).step_by(2000) {
        for x in (-44_000..=44_000).step_by(2000) {
            for i in 0..24 {
                let (cx, cz) = (x + i, z);
                let col = g[1].column_at(cx, cz);
                if col.outside || col.height < 2.0 {
                    continue;
                }
                // Near a wall: within two columns (a column's backed check reads its neighbours'
                // rules, and a neighbour beside a wall notches).
                let edge =
                    (-2..=2).any(|dz| (-2..=2).any(|dx| g[1].column_at(cx + dx, cz + dz).wall));
                let ramps = [0, 1, 2].map(|k| g[k].ramp_pieces_at(cx, cz));
                if edge {
                    c.edge_n += 1;
                    c.edge_notched += usize::from(ramps[1] != ramps[0]);
                    c.edge_065 += usize::from(ramps[2] != ramps[0]);
                    for (n, r) in c.edge_ramped.iter_mut().zip(ramps) {
                        *n += usize::from(r > 0);
                    }
                } else {
                    c.off_n += 1;
                    for (n, r) in c.off.iter_mut().zip(ramps) {
                        *n += usize::from(r > 0);
                    }
                    c.off_differ += usize::from(ramps[1] != ramps[0]);
                }
            }
        }
    }
    c
}

both_faces!(
    fn straight_edge_ramps_everywhere_mesa_edges_included(f: FaceFrame) {
        assert_eq!(
            world_config(NAME).unwrap().unwrap().geology.notched,
            Notched::None,
            "rocks.json shaping.notched"
        );
        let mut was = 0;
        for world in SEEDS {
            let c = census(world, f);
            let share = |n: usize, of: usize| 100.0 * n as f64 / of.max(1) as f64;
            eprintln!(
                "seed {world} {:?}: off the edges {} columns, ramped: straight-edge rule {:.1} %, now {:.1} %, 0.65 {:.1} %; at the edges {} columns, ramped: straight-edge rule {:.1} %, now {:.1} %, 0.65 {:.1} % ({} notched in 0.65)",
                f.face,
                c.off_n,
                share(c.off[0], c.off_n),
                share(c.off[1], c.off_n),
                share(c.off[2], c.off_n),
                c.edge_n,
                share(c.edge_ramped[0], c.edge_n),
                share(c.edge_ramped[1], c.edge_n),
                share(c.edge_ramped[2], c.edge_n),
                c.edge_065
            );
            assert!(
                c.off_n > 5000,
                "seed {world}: enough land sampled ({})",
                c.off_n
            );
            assert_eq!(c.off_differ, 0, "seed {world}: notched ramps off the edges");
            assert_eq!(
                c.edge_notched, 0,
                "seed {world}: notched ramps at the mesa edges"
            );
            was += c.edge_065;
        }
        // 0.65's rule notched some edge columns: the census reaches the walls' edges.
        assert!(was > 0, "{:?}: no wall edge sampled", f.face);
    }
);
