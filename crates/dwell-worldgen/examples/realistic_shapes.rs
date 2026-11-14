//! A Realistic world's map shapes in numbers (WORK_QUEUE item 15's map checks, 2026-11-14): per seed
//! and face, the longest thin land strip, the straight shares of the coast and of the plate
//! boundaries, and the islands' sizes, spacing and irregularity (the measures of
//! `tests/realistic_shapes.rs`, `tests/support/map_shapes.rs`).
//!
//! `cargo run --release -p dwell-worldgen --example realistic_shapes -- 1 7 42 274005646 2741893632`

#[path = "../tests/support/map_shapes.rs"]
mod map_shapes;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plates::{PLATES, PlateLayout};
use dwell_worldgen::terrain::FIXTURE_WORLDS;
use dwell_worldgen::worldmap::WorldMap;
use map_shapes::{Mask, cv};

fn main() {
    let seeds: Vec<u64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("seed"))
        .collect();
    let h = FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:realistic")
        .expect("realistic")
        .1;
    for s in seeds {
        for face in [Face::A, Face::B] {
            let seed = face_seed(s, face);
            let m = WorldMap::for_height(seed, h);
            let land = Mask {
                n: m.n,
                cell: m.cell,
                on: (0..m.height.len())
                    .map(|c| !m.is_sea(c) && m.height[c].is_finite())
                    .collect(),
            };
            let thin = land.thin(500.0);
            let strip = land
                .components(&thin)
                .iter()
                .map(|c| land.extent(c))
                .fold(0.0, f64::max);
            let (coast, _) = land.straight_share(&land.outline(), 1000.0, 30.0);
            let layout = PlateLayout::generate(&PLATES, seed);
            let r = m.world_radius() as f64 - 2500.0;
            let plate: Vec<usize> = (0..m.height.len())
                .map(|c| {
                    let (x, z) = m.centre(c);
                    layout.plate_at(x, z)
                })
                .collect();
            let edge: Vec<bool> = (0..plate.len())
                .map(|c| {
                    let (x, z) = m.centre(c);
                    x.hypot(z) < r
                        && ((c % m.n + 1 < m.n && plate[c + 1] != plate[c])
                            || (c + m.n < plate.len() && plate[c + m.n] != plate[c]))
                })
                .collect();
            let (bound, _) = land.straight_share(&edge, 1000.0, 30.0);
            let long: Vec<String> = [(1500.0, 45.0), (2500.0, 75.0)]
                .iter()
                .map(|&(r, t)| {
                    format!(
                        "{:.0}m c{:.2}% b{:.2}%",
                        2.0 * r,
                        100.0 * land.straight_share(&land.outline(), r, t).0,
                        100.0 * land.straight_share(&edge, r, t).0
                    )
                })
                .collect();
            println!("  runs {}", long.join(", "));
            let islands: Vec<_> = land
                .components(&land.on)
                .iter()
                .filter(|c| c.len() >= 12 && (c.len() as f64) * m.cell * m.cell < 60e6)
                .map(|c| land.shape(c))
                .collect();
            let radii: Vec<f64> = islands.iter().map(|i| i.radius).collect();
            let near: Vec<f64> = islands
                .iter()
                .map(|a| {
                    islands
                        .iter()
                        .filter(|b| !std::ptr::eq(a, *b))
                        .map(|b| (a.x - b.x).hypot(a.z - b.z))
                        .fold(f64::MAX, f64::min)
                })
                .filter(|d| *d < 20_000.0)
                .collect();
            let irr: Vec<String> = islands
                .iter()
                .map(|i| format!("{:.2}", i.irregularity))
                .collect();
            println!(
                "seed {s} {face:?}: strip {:.1} km; straight coast {:.1} %, boundary {:.1} %; {} islands r̄ {:.0} m cv {:.2}, spacing cv {:.2} ({}), irregularity [{}]",
                strip / 1000.0,
                100.0 * coast,
                100.0 * bound,
                islands.len(),
                radii.iter().sum::<f64>() / radii.len().max(1) as f64,
                if radii.len() > 1 { cv(&radii) } else { 0.0 },
                if near.len() > 1 { cv(&near) } else { 0.0 },
                near.len(),
                irr.join(" ")
            );
        }
    }
}
