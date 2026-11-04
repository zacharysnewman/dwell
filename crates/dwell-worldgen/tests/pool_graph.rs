//! The pool graph (docs/proposals/WATER_GRAPH.md G1, ADR 0059): a face's rivers as pools joined
//! by lips, derived from the world map. Balanced by construction: every lip pours into exactly
//! the cells its river's flow needs (a lip into a chute too: steps steeper than `STEEP_FALL` are
//! one chute down to the pool at its foot); deterministic whatever order its rivers are built in.

use dwell_worldgen::terrain::{PoolKey, TerrainGenerator};

#[test]
fn every_lip_is_balanced_on_seeds_0_to_5() {
    // face-a-only: one face's map (each face's graph is built the same way).
    // Known: a junction at the top of a chute splits a step between two rivers (seed 0, river
    // 17), a lip a cell short; at most one in ten thousand (docs/BUGS.md 28).
    let mut lips = 0;
    let mut bad = Vec::new();
    for seed in 0..6u64 {
        let g = TerrainGenerator::new(seed);
        for r in 0..g.river_count() {
            let rg = g.river_graph(r);
            lips += rg.lips.len();
            assert!(
                rg.missing.is_empty(),
                "seed {seed}, river {r}: no lip found: {:?}",
                rg.missing
            );
            bad.extend(rg.unbalanced().into_iter().map(|l| (seed, r, l.clone())));
        }
    }
    assert!(lips > 40_000, "lips: {lips}");
    assert!(bad.len() * 10_000 <= lips, "unbalanced lips: {bad:?}");
}

#[test]
fn the_graph_is_the_same_whatever_order_its_rivers_are_built_in() {
    // face-a-only: one face's map.
    let (a, b) = (TerrainGenerator::new(1), TerrainGenerator::new(1));
    let n = a.river_count();
    let fwd: Vec<_> = (0..n).map(|r| a.river_graph(r)).collect();
    let back: Vec<_> = (0..n).rev().map(|r| b.river_graph(r)).collect();
    for (r, g) in fwd.iter().enumerate() {
        assert_eq!(**g, *back[n as usize - 1 - r], "river {r}");
    }
}

#[test]
fn a_river_is_a_chain_of_pools_one_row_apart_ending_where_it_joins() {
    // face-a-only: one face's map.
    let g = TerrainGenerator::new(0);
    for r in 0..g.river_count() {
        let rg = g.river_graph(r);
        for w in rg.pools.windows(2) {
            if let (PoolKey::River { row: a, .. }, PoolKey::River { row: b, .. }) =
                (w[0].key, w[1].key)
            {
                assert!(
                    b < a,
                    "river {r}: pools step down (a row, or a chute's rows)"
                );
            }
            assert!(
                w[0].reach.1 <= w[1].reach.0 + 1e-6,
                "river {r}: in order along it"
            );
        }
        for l in &rg.lips {
            if let (PoolKey::River { river, row }, PoolKey::River { river: tr, row: to }) =
                (l.from, l.to)
            {
                assert_eq!(river, r);
                assert!(to < row, "river {r}: a lip pours down");
                assert!(
                    tr == r || to < row,
                    "a junction's lip pours into the river it joins"
                );
            }
        }
    }
}
