//! The Realistic world's scale over many seeds (WORK_QUEUE item 15, real-world scale; the realism
//! audit, 2026-11-14): per face, the plates and boundaries inside the disc, the land share, the
//! faces that are (nearly) all ocean, and the volcanoes' spacing; at a world size (`--size N`, the
//! disc's radius × N; 1 the default 50 km).
//!
//! `cargo run --release -p dwell-worldgen --example realistic_scale -- --seeds 50 --size 1`

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plate_tectonics::{PlateTectonics, UPLIFT};
use dwell_worldgen::plates::{PLATES, PlateLayout};

fn arg(args: &[String], name: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seeds: u64 = arg(&args, "--seeds", "50").parse().unwrap();
    let size: f64 = arg(&args, "--size", "1").parse().unwrap();
    // `--list`: one line a face (to pick test seeds with the features a test needs).
    let list = args.iter().any(|a| a == "--list");
    let reach = PLATES.reach * size;
    let (mut plates, mut bounds, mut land, mut ocean, mut faces) = (0.0, 0.0, 0.0, 0, 0);
    let mut spacing = Vec::new();
    let mut per_face = Vec::new();
    for s in 0..seeds {
        for face in [Face::A, Face::B] {
            let seed = face_seed(s, face);
            let layout = PlateLayout::generate_sized(&PLATES, seed, reach);
            let t = PlateTectonics::new(layout.clone(), &UPLIFT);
            // A grid over the disc: the plates seen, the land share.
            let n = 80;
            let mut seen = std::collections::BTreeSet::new();
            let (mut l, mut all, mut top) = (0, 0, 0.0f64);
            for j in 0..n {
                for i in 0..n {
                    let x = -reach + (i as f64 + 0.5) * 2.0 * reach / n as f64;
                    let z = -reach + (j as f64 + 0.5) * 2.0 * reach / n as f64;
                    if x.hypot(z) > reach - PLATES.rim.edge_sea {
                        continue;
                    }
                    all += 1;
                    seen.insert(layout.plate_at(x, z));
                    l += usize::from(layout.is_land(x, z) || t.height(x, z) > 0.0);
                    top = top.max(t.height(x, z));
                }
            }
            let share = l as f64 / all.max(1) as f64;
            let b = layout
                .boundaries
                .iter()
                .filter(|b| b.points.iter().any(|p| p[0].hypot(p[1]) < reach))
                .count();
            plates += seen.len() as f64;
            bounds += b as f64;
            land += share;
            ocean += usize::from(share < 0.05);
            faces += 1;
            per_face.push((seen.len(), b));
            let vs: Vec<[f64; 2]> = t.volcanoes[..t.mains]
                .iter()
                // The placed cones and islands, not the satellite islets round them.
                .filter(|v| v.x.hypot(v.z) < reach)
                .map(|v| [v.x, v.z])
                .collect();
            if list {
                println!(
                    "seed {s} {face:?}: plates {}, boundaries {b}, land {:.0} %, uplift max {top:.0} m, volcanoes {}, band {}",
                    seen.len(),
                    100.0 * share,
                    vs.len(),
                    dwell_worldgen::climate::Climate::for_seed(seed).band().name
                );
            }
            for (k, a) in vs.iter().enumerate() {
                let d = vs
                    .iter()
                    .enumerate()
                    .filter(|(m, _)| *m != k)
                    .map(|(_, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
                    .fold(f64::INFINITY, f64::min);
                if d.is_finite() {
                    spacing.push(d);
                }
            }
        }
    }
    let f = f64::from(faces as u32);
    spacing.sort_by(f64::total_cmp);
    let med = spacing.get(spacing.len() / 2).copied().unwrap_or(f64::NAN);
    let max_plates = per_face.iter().map(|p| p.0).max().unwrap_or(0);
    let one = per_face.iter().filter(|p| p.0 == 1).count();
    println!(
        "size {size}× (radius {:.0} km), {faces} faces: plates per face {:.2} (max {max_plates}, {one} with one), boundaries {:.2}, land {:.0} %, faces under 5 % land {ocean} ({:.0} %), volcanoes {} (median nearest {:.1} km)",
        reach / 1000.0,
        plates / f,
        bounds / f,
        100.0 * land / f,
        100.0 * ocean as f64 / f,
        spacing.len(),
        med / 1000.0
    );
}
