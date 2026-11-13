//! The Realistic world's geography in numbers (WORK_QUEUE item 15, the realistic batch part A):
//! for each seed's face A, the longest lake (km), the plates (kinds and each continental plate's
//! interior, m), the land's reach from the centre and the outer coast's radius spread over angle.
//!
//! `cargo run --release -p dwell-worldgen --example realistic_geo -- 1 7 42 2741893632`

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plates::{PlateKind, PlateLayout};
use dwell_worldgen::terrain::FIXTURE_WORLDS;
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    let seeds: Vec<u64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("a seed"))
        .collect();
    let h = FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:realistic")
        .expect("realistic")
        .1;
    for s in seeds {
        let seed = face_seed(s, Face::A);
        let layout = PlateLayout::generate(&dwell_worldgen::plates::PLATES, seed);
        let m = WorldMap::for_height(seed, h);
        // Lakes: each one's longest extent (the farthest two cells' distance, by bounding box).
        let mut boxes = vec![(f64::MAX, f64::MAX, f64::MIN, f64::MIN, 0usize); m.lakes.len()];
        for c in 0..m.height.len() {
            let l = m.lake[c] as usize;
            if l == 0 || l > m.lakes.len() {
                continue;
            }
            let (x, z) = m.centre(c);
            let b = &mut boxes[l - 1];
            *b = (b.0.min(x), b.1.min(z), b.2.max(x), b.3.max(z), b.4 + 1);
        }
        let longest = boxes
            .iter()
            .filter(|b| b.4 > 0)
            .map(|b| (b.2 - b.0).hypot(b.3 - b.1) / 1000.0)
            .fold(0.0, f64::max);
        let big = boxes.iter().filter(|b| b.4 >= 20).count();
        if std::env::var("GEO_LAKES").is_ok() {
            let t = dwell_worldgen::plate_tectonics::PlateTectonics::new(
                layout.clone(),
                &dwell_worldgen::plate_tectonics::UPLIFT,
            );
            for (i, b) in boxes.iter().enumerate() {
                let len = (b.2 - b.0).hypot(b.3 - b.1) / 1000.0;
                if len < 15.0 {
                    continue;
                }
                let (cx, cz) = (0.5 * (b.0 + b.2), 0.5 * (b.1 + b.3));
                let plate = layout.plate_at(cx, cz);
                let near: Vec<String> = t
                    .lines_of(plate, cx, cz)
                    .map(|(l, d)| format!("{:?}{:?} {:.1}km", l.kind, l.pair.plates, d / 1000.0))
                    .collect();
                println!(
                    "  lake {i}: {len:.1} km at ({:.1}, {:.1}) km, level {:.0} m, {} cells; plate {plate} {:?}; {near:?}",
                    cx / 1000.0,
                    cz / 1000.0,
                    m.lakes[i].level,
                    b.4,
                    layout.plates[plate].kind
                );
            }
        }
        // Land: share, and each angle's outermost land radius.
        let (mut land, mut inside) = (0usize, 0usize);
        let r_world = m.world_radius() as f64;
        for c in 0..m.height.len() {
            let (x, z) = m.centre(c);
            if x.hypot(z) > r_world {
                continue;
            }
            inside += 1;
            land += usize::from(m.height[c] > 0.0);
        }
        let mut outer = Vec::new();
        for a in 0..144 {
            let th = f64::from(a) * std::f64::consts::TAU / 144.0;
            let mut r = 0.0;
            let mut k = 0.0;
            while k < r_world {
                if m.height_at(k * th.cos(), k * th.sin()).0 > 0.0 {
                    r = k;
                }
                k += 200.0;
            }
            outer.push(r);
        }
        let far = outer.iter().cloned().fold(0.0, f64::max);
        // The spread of the outer coast where land reaches the outer band (≥ 0.6 of the radius).
        let band: Vec<f64> = outer
            .iter()
            .cloned()
            .filter(|&r| r > 0.6 * r_world)
            .collect();
        let spread = if band.len() > 1 {
            let mean = band.iter().sum::<f64>() / band.len() as f64;
            (band.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / band.len() as f64).sqrt()
        } else {
            0.0
        };
        let kinds: String = layout
            .plates
            .iter()
            .map(|p| match p.kind {
                PlateKind::Continental => 'C',
                PlateKind::Oceanic => 'o',
            })
            .collect();
        let interiors: Vec<String> = layout
            .interiors()
            .iter()
            .zip(&layout.plates)
            .map(|(d, p)| {
                format!(
                    "{}{:.1}",
                    if p.kind == PlateKind::Continental {
                        "C"
                    } else {
                        "o"
                    },
                    d / 1000.0
                )
            })
            .collect();
        println!(
            "seed {s}: plates {kinds} interiors(km) [{}]; land {:.1} %; longest lake {longest:.1} km ({big} lakes ≥ 20 cells, {} in all); land reaches {:.1} km; outer coast spread {:.2} km over {} of 144 angles",
            interiors.join(" "),
            100.0 * land as f64 / inside as f64,
            m.lakes.len(),
            far / 1000.0,
            spread / 1000.0,
            band.len()
        );
    }
}
