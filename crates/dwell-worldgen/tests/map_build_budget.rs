//! A bigger world's map costs about today's (BUGS.md #91: a 10× world sat for minutes on "Shaping
//! the world map"): its grid has today's cell count, so no step may grow with the radius. Pinned
//! by op counts per cell, not wall-clock: the uplift's range-spine segment tests per land cell and
//! the channel index's entries per segment, each at 10× within a small factor of 1×'s (measured
//! with seed 3: 205 and 572 segment tests a land cell at 1× and 10×, ~1 000 at 10× since #77, ~21 000 before; 10 and
//! 91 entries a segment, ~6 000 before). And the
//! build's progress (the loading screen's "Shaping the world map — rivers (5/6)") runs through
//! every step in order, each to its end.

use std::cell::RefCell;
use std::rc::Rc;

use dwell_protocol::world_size::WorldSize;
use dwell_world::both_faces;
use dwell_worldgen::hydrology::uplift;
use dwell_worldgen::progress::{self, MapStep};
use dwell_worldgen::worldmap::{SEA, WorldMap};

struct Built {
    map: WorldMap,
    /// Segment tests per land cell.
    tests_per_cell: f64,
    /// Index entries per channel segment.
    entries_per_seg: f64,
    heard: Vec<(MapStep, f64)>,
}

fn build(seed: u64, size: WorldSize) -> Built {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let h = heard.clone();
    let before = uplift::segment_tests();
    let map = progress::with_sink(
        move |s, f| h.borrow_mut().push((s, f)),
        || WorldMap::compute_sized(seed, size),
    );
    let tests = uplift::segment_tests() - before;
    let land = map.flags.iter().filter(|&&f| f & SEA == 0).count();
    let entries = map.channels.index_entries();
    let segs = map.channels.segs.len().max(1);
    let heard = heard.borrow().clone();
    Built {
        tests_per_cell: tests as f64 / land.max(1) as f64,
        entries_per_seg: entries as f64 / segs as f64,
        map,
        heard,
    }
}

both_faces!(
    /// A 10× face's map does at most a few times 1×'s work per cell (it did ~100× the uplift's
    /// and ~250× the channel index's before BUGS.md #91's fix: 44 s natively against 2 s).
    fn a_10x_map_costs_about_a_1x_map_per_cell(f: FaceFrame) {
        let seed = dwell_worldgen::bifacial::face_seed(3, f.face);
        let one = build(seed, WorldSize::DEFAULT);
        let ten = build(seed, WorldSize::named("10x").expect("a named size"));
        assert_eq!(ten.map.n, one.map.n, "today's cell count");
        // Since BUGS.md #77's fix a 10× island's ranges rise across it, not in a ring by its
        // coast, so more of its land lies near a spine: ~1 000 tests a cell (~5× 1×'s; the
        // whole 10× map still builds in ~2 s natively), against ~21 000 before #91.
        assert!(
            ten.tests_per_cell <= 8.0 * one.tests_per_cell.max(1.0),
            "uplift: {:.0} segment tests a land cell at 10×, {:.0} at 1×",
            ten.tests_per_cell,
            one.tests_per_cell
        );
        // The index's squares follow the cell, so a segment is listed in about as many as at 1×;
        // a 10× map's rivers are wider (their catchments 100× the area), so a few times more
        // (9× here; 600× before the fix: a 12 GB index natively, past a browser's memory).
        assert!(
            ten.entries_per_seg <= 16.0 * one.entries_per_seg.max(1.0),
            "channels: {:.0} index entries a segment at 10×, {:.0} at 1×",
            ten.entries_per_seg,
            one.entries_per_seg
        );
        for b in [&one, &ten] {
            progress_runs_in_order(&b.heard);
        }
    }
);

/// Every step reported, in [`MapStep::ALL`]'s order, each one's fraction never falling and
/// reaching 1.
fn progress_runs_in_order(heard: &[(MapStep, f64)]) {
    let mut at: Option<(MapStep, f64)> = None;
    let mut ended = Vec::new();
    for &(s, f) in heard {
        if let Some((prev, pf)) = at {
            assert!(s.index() >= prev.index(), "{s:?} after {prev:?}");
            if s == prev {
                assert!(f >= pf, "{s:?} fell from {pf} to {f}");
            }
        }
        if f >= 1.0 && !ended.contains(&s) {
            ended.push(s);
        }
        at = Some((s, f));
    }
    assert_eq!(ended, MapStep::ALL.to_vec(), "every step reached its end");
}
