//! A named size's island relief, rivers and lakes, measured (BUGS.md #77, BIG_MAP.md §5): one
//! face's world map computed at the size, then its land's elevation range, the share of the land
//! above 200 m and above 1 500 m, the number of ranges (connected land above 1 000 m, at least
//! 20 km² each) and of peaks per 1 000 km² (summits above 1 000 m, highest within 5 km), the widest river and channel, and the lakes' share of the land.
//!
//! `cargo run --release -p dwell-worldgen --example big_terrain -- [size…]` (default 1x 4x 10x).

use dwell_protocol::world_size::WorldSize;
use dwell_world::Face;
use dwell_worldgen::worldmap::{WorldMap, relief::Relief};

fn main() {
    let names: Vec<String> = std::env::args().skip(1).collect();
    let names = if names.is_empty() {
        vec!["1x".into(), "4x".into(), "10x".into()]
    } else {
        names
    };
    println!(
        "| size | cell | land km² | max m | >200 m | >1500 m | ranges | peaks/1000 km² | river max m | channel max m (at m³/s) | lake share | lakes |"
    );
    for name in names {
        let size = WorldSize::named(&name).expect("a named size");
        let seed = dwell_worldgen::bifacial::face_seed(0, Face::A);
        let m = WorldMap::compute_sized(seed, size);
        let r = Relief::of(&m);
        println!(
            "| {name} | {} m | {:.0} | {:.0} | {:.1} % | {:.1} % | {} | {:.2} | {:.0} | {:.0} ({:.0}) | {:.2} % | {} |",
            m.cell,
            r.land_km2,
            r.max_height,
            100.0 * r.above_200,
            100.0 * r.above_1500,
            r.ranges,
            1000.0 * r.peaks as f64 / r.land_km2,
            r.river_width,
            r.channel_width,
            r.channel_q,
            100.0 * r.lake_share,
            r.lakes,
        );
    }
}
