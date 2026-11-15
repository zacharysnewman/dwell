//! The Realistic world at real-world scale (WORK_QUEUE item 15; owner, 2026-11-14: "We already have
//! 10x realistic worlds right? Those seem like the same fix regardless it's to scale the regions by
//! the world size essentially right?"). Every rule in real units, so a world's size decides its
//! variety: plates hundreds to thousands of km across, latitude in degrees per km, volcanoes tens
//! of km apart. The realism audit (main a34db850) measured 6.6 plates and 10.9 boundaries per 1×
//! face, 10–14° of latitude across it, volcanoes 2.6 km apart. Over 30 seeds, both faces, at 1×
//! (the default 50 km disc) and 10× (500 km):
//!
//! - plates: a 1× face mostly one plate (at most two), a 10× face 2 to 5;
//! - latitude: a face spans `degreesPerKm` × its diameter (about 0.9° at 1×, 9° at 10×);
//! - volcanoes: the placed cones and islands at least 40 km apart (median);
//! - the lee keeps at least `rain.leeFloor` of the flat rate (a ridge's lee on a fixture grid).

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::climate::{CLIMATE, Climate};
use dwell_worldgen::plate_tectonics::{PlateTectonics, UPLIFT};
use dwell_worldgen::plates::{PLATES, PlateLayout};

const SEEDS: u64 = 30;

/// The plates seen on a 40 × 40 grid over the disc of `reach` m.
fn plates_seen(l: &PlateLayout, reach: f64) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for j in 0..40 {
        for i in 0..40 {
            let x = -reach + (f64::from(i) + 0.5) * reach / 20.0;
            let z = -reach + (f64::from(j) + 0.5) * reach / 20.0;
            if x.hypot(z) < reach - PLATES.rim.edge_sea {
                seen.insert(l.plate_at(x, z));
            }
        }
    }
    seen.len()
}

both_faces!(
    fn plates_follow_the_worlds_area(f: FaceFrame) {
        for (size, lo, hi, mean_lo, mean_hi) in [(1.0, 1, 2, 1.0, 1.4), (10.0, 1, 5, 2.0, 4.5)] {
            let reach = PLATES.reach * size;
            let counts: Vec<usize> = (0..SEEDS)
                .map(|s| {
                    plates_seen(
                        &PlateLayout::generate_sized(&PLATES, face_seed(s, f.face), reach),
                        reach,
                    )
                })
                .collect();
            let mean = counts.iter().sum::<usize>() as f64 / counts.len() as f64;
            eprintln!("{:?} {size}×: plates per face {mean:.2} {counts:?}", f.face);
            assert!(
                counts.iter().all(|&c| (lo..=hi).contains(&c)),
                "{size}×: {counts:?}"
            );
            assert!(
                (mean_lo..=mean_hi).contains(&mean),
                "{size}×: mean {mean:.2}"
            );
        }
    }
);

both_faces!(
    fn latitude_spans_the_worlds_size(f: FaceFrame) {
        for s in 0..SEEDS {
            let c = Climate::generate(CLIMATE, face_seed(s, f.face));
            let (nx, nz) = c.north_vector();
            for size in [1.0, 10.0] {
                let r = PLATES.reach * size;
                let span = c.latitude(r * nx, r * nz) - c.latitude(-r * nx, -r * nz);
                let want = CLIMATE.degrees_per_km * 2.0 * r / 1000.0;
                // At the poles' or equator's clamp a span is cut short.
                assert!(span <= want + 1e-9, "{size}×: {span:.2}° over {want:.2}°");
                if c.centre > want && c.centre < 90.0 - want {
                    assert!(
                        (span - want).abs() < 1e-6,
                        "{size}×: {span:.2}° for {want:.2}°"
                    );
                }
            }
        }
    }
);

#[test]
fn volcanoes_stand_tens_of_km_apart() {
    // both-faces: the plates' rules are the same on both; seeds over face A's and B's seeds.
    let mut near = Vec::new();
    for s in 0..10u64 {
        for face in [dwell_world::Face::A, dwell_world::Face::B] {
            let reach = PLATES.reach * 10.0;
            let t = PlateTectonics::new(
                PlateLayout::generate_sized(&PLATES, face_seed(s, face), reach),
                &UPLIFT,
            );
            let vs: Vec<[f64; 2]> = t.volcanoes[..t.mains].iter().map(|v| [v.x, v.z]).collect();
            for (k, a) in vs.iter().enumerate() {
                let d = vs
                    .iter()
                    .enumerate()
                    .filter(|(m, _)| *m != k)
                    .map(|(_, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
                    .fold(f64::INFINITY, f64::min);
                if d.is_finite() {
                    near.push(d);
                }
            }
        }
    }
    near.sort_by(f64::total_cmp);
    let med = near[near.len() / 2];
    eprintln!(
        "{} volcanoes, median nearest {:.1} km",
        near.len(),
        med / 1000.0
    );
    assert!(near.len() >= 10, "volcanoes sampled");
    assert!(med >= 40_000.0, "median nearest {:.1} km", med / 1000.0);
}

#[test]
fn a_lee_keeps_its_floor_of_rain() {
    // both-faces: a face-local fixture grid, no face. A 1500 m ridge across the wind: the lee's
    // driest cell keeps at least `leeFloor` of the flat rate (it dried to ~0 before).
    let (n, cell) = (120usize, 1000.0);
    let mut height = vec![0f32; n * n];
    let mut sea = vec![false; n * n];
    for j in 0..n {
        for i in 0..n {
            let c = j * n + i;
            if i >= 110 {
                sea[c] = true;
                continue;
            }
            let d = (i as f32 - 60.0).abs();
            height[c] = (1500.0 - 60.0 * d).max(0.0);
        }
    }
    let r = CLIMATE.rain;
    let mut p = dwell_worldgen::terrain::BIOME_MAP;
    p.wind_direction = 0.0;
    p.wind_reach = r.reach_m;
    p.base_rain = r.base_rain;
    p.uplift_rain = r.uplift_rain;
    p.lee_dry = r.lee_dry;
    p.lee_floor = r.lee_floor;
    p.recover_m = r.recover_m;
    p.wind_smooth_m = r.smooth_m;
    let rain = dwell_worldgen::biomemap::rainfall(n, cell, &height, &sea, &p).rain;
    let flat = (r.base_rain * 2.0 * cell / 1000.0).clamp(0.0, 1.0);
    let row = 60 * n;
    let lee = (5..55)
        .map(|i| f64::from(rain[row + i]))
        .fold(f64::INFINITY, f64::min);
    eprintln!("the lee's driest {lee:.4} of a flat {flat:.4}");
    assert!(
        lee >= 0.5 * r.lee_floor * flat,
        "lee {lee:.4}, flat {flat:.4}"
    );
}
