//! The pool graph (docs/proposals/WATER_GRAPH.md G1, ADR 0059): a face's rivers as pools joined
//! by lips, derived from the world map. Balanced by construction: every lip between pools pours
//! into exactly the cells its river's flow needs (chutes, steps steeper than `STEEP_FALL`, are
//! G2's); deterministic whatever order its rivers are built in.

use dwell_worldgen::terrain::{PoolKey, STEEP_FALL, TerrainGenerator};

#[test]
fn every_lip_between_pools_is_balanced_on_seeds_0_to_5() {
    // face-a-only: one face's map (each face's graph is built the same way).
    let mut lips = 0;
    for seed in 0..6u64 {
        let g = TerrainGenerator::new(seed);
        for r in 0..g.river_count() {
            let rg = g.river_graph(r);
            lips += rg.lips.len();
            let bad = rg.unbalanced_pools();
            assert!(bad.is_empty(), "seed {seed}, river {r}: {bad:?}");
            for &(at, row, fall) in &rg.missing {
                assert!(
                    fall >= STEEP_FALL,
                    "seed {seed}, river {r}: no lip found at {at:.0} m (row {row}, fall {fall})"
                );
            }
        }
    }
    assert!(lips > 30_000, "lips: {lips}");
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
                assert_eq!(a - 1, b, "river {r}: pools step a row at a time");
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
