//! Big worlds keep today's island, bigger (BUGS.md #77, docs/proposals/BIG_MAP.md §5): a 4× and a
//! 10× world's map, on each face, keeps the 1× map's mountains (its highest land, its share above
//! 1 500 m, its summits per area), its rivers as wide as their discharge makes them (√Q past
//! `bigWorlds.lipsLinearTo`, not kilometres of lip cells) and no speckle of lakes. Measured with
//! `worldmap::relief` in metres, so the coarse cell doesn't change what is counted.

use dwell_protocol::world_size::WorldSize;
use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::worldmap::{WorldMap, relief::Relief};

fn relief(seed: u64, f: dwell_world::Face, size: &str) -> Relief {
    let size = WorldSize::named(size).expect("a named size");
    Relief::of(&WorldMap::compute_sized(face_seed(seed, f), size))
}

/// A big map's relief against the 1× map's (same seed and face).
fn keeps_today_s_island(one: &Relief, big: &Relief, size: &str) {
    assert!(
        big.max_height >= 0.8 * one.max_height,
        "{size}: the highest land {:.0} m vs 1×'s {:.0} m",
        big.max_height,
        one.max_height
    );
    assert!(
        big.above_1500 >= 0.5 * one.above_1500,
        "{size}: {:.1} % of the land above 1 500 m vs 1×'s {:.1} %",
        100.0 * big.above_1500,
        100.0 * one.above_1500
    );
    let density = |r: &Relief| 1000.0 * f64::from(r.peaks) / r.land_km2;
    assert!(
        density(big) >= 0.5 * density(one),
        "{size}: {:.2} peaks per 1 000 km² vs 1×'s {:.2}",
        density(big),
        density(one)
    );
    // Hydraulic geometry: width ∝ √Q. 1× is ~24 √Q; before the fix 10× was kilometres wide.
    assert!(
        big.channel_width <= 60.0 * big.channel_q.max(1.0).sqrt() && big.channel_width <= 1500.0,
        "{size}: the widest channel {:.0} m at {:.0} m³/s",
        big.channel_width,
        big.channel_q
    );
    assert!(
        big.lake_share <= 0.03,
        "{size}: lakes cover {:.1} % of the land ({} lakes)",
        100.0 * big.lake_share,
        big.lakes
    );
}

both_faces!(
    /// A 4× world's island keeps the 1× island's mountains, rivers and lakes (BUGS.md #77).
    fn a_4x_world_keeps_its_mountains_and_rivers(f: FaceFrame) {
        let one = relief(0, f.face, "1x");
        keeps_today_s_island(&one, &relief(0, f.face, "4x"), "4x");
    }
);

both_faces!(
    /// A 10× world's island keeps the 1× island's mountains, rivers and lakes (BUGS.md #77: before,
    /// it was flat, with kilometre-wide rivers and a tenth of it lake).
    fn a_10x_world_keeps_its_mountains_and_rivers(f: FaceFrame) {
        let one = relief(0, f.face, "1x");
        keeps_today_s_island(&one, &relief(0, f.face, "10x"), "10x");
    }
);

#[test]
fn at_1x_the_lips_follow_the_discharge_one_for_one() {
    // both-faces: no face (the lip rule is a flow law).
    let b = dwell_worldgen::hydrology::BIG_WORLDS;
    let per_cell = 0.4;
    // Every lip count a 1× river reaches (≤ 20 m³/s) is unchanged.
    for cells in 0..=(b.lips_linear_to / per_cell) as u32 {
        assert_eq!(b.lips(cells, per_cell), cells);
    }
    // Past it, √Q: four times the flow, twice the lips beyond the linear part's end.
    let l = (b.lips_linear_to / per_cell) as u32;
    assert_eq!(b.lips(4 * l, per_cell), 2 * l);
}
