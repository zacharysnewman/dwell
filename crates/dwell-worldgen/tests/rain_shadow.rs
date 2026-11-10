//! The rain shadow (systemic terrain T1, `biomemap::rainfall`): a prevailing wind carries moisture
//! in from the sea and drops it where the ground rises, so the lee of high ground is dry.

use dwell_world::*;
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::biomemap::{BiomeField, rainfall};
use dwell_worldgen::terrain::{BIOME_MAP, BiomeMap};

/// The tests' wind: a strong one blowing from +x, the wetness all rain.
fn strong_wind() -> BiomeMap {
    BiomeMap {
        wind_direction: 0.0,
        wind_reach: 1500.0,
        base_rain: 0.15,
        uplift_rain: 1.5,
        lee_dry: 1.0,
        rain_share: 1.0,
        ..BIOME_MAP
    }
}

#[test]
fn a_ridge_is_wet_on_its_windward_side_and_dry_in_its_lee() {
    // both-faces: a face-local grid of its own, no face; each face's map is the next test's.
    // The sea at x ≥ 100 cells (upwind, the wind blowing from +x), then land rising at 15 % to a
    // 600 m ridge at x = 60 and falling away behind it.
    let (n, cell) = (120usize, 100.0);
    let mut height = vec![0f32; n * n];
    let mut sea = vec![false; n * n];
    for j in 0..n {
        for i in 0..n {
            let c = j * n + i;
            if i >= 100 {
                sea[c] = true;
                height[c] = -20.0;
            } else {
                let d = (i as f64 - 60.0).abs();
                height[c] = (600.0 - 15.0 * d).max(20.0) as f32;
            }
        }
    }
    let p = strong_wind();
    let r = rainfall(n, cell, &height, &sea, &p);
    let row = 60 * n;
    let at = |i: usize| f64::from(r.rain[row + i]);
    let windward = (62..90).map(at).sum::<f64>() / 28.0;
    let lee = (30..58).map(at).sum::<f64>() / 28.0;
    assert!(
        windward > 4.0 * lee,
        "the windward slope wetter than the lee: {windward} vs {lee}"
    );
    // Bounded; the air's moisture only falls over land, and by what rained (× the step's share).
    for c in 0..n * n {
        assert!((0.0..=1.0).contains(&r.rain[c]), "rain at {c}");
        assert!((0.0..=1.0).contains(&r.moisture[c]), "moisture at {c}");
    }
    let dry = (2.0 * cell / p.wind_reach).min(1.0);
    for i in 0..98 {
        let (up, here) = (row + i + 2, row + i);
        let lost = f64::from(r.moisture[up]) - f64::from(r.moisture[here]);
        assert!(lost >= -1e-6, "moisture only falls inland at {i}");
        assert!(
            (lost - f64::from(r.rain[here]) * dry).abs() < 1e-5,
            "the air loses what rained at {i}: {lost} vs {}",
            r.rain[here]
        );
    }
    // The sea refills the air.
    assert_eq!(r.moisture[row + 100], 1.0);
}

#[test]
fn flat_land_dries_steadily_inland() {
    // both-faces: a face-local grid of its own, no face.
    // Flat land: the rain falls off steadily inland from the coast, never rising.
    let (n, cell) = (64usize, 100.0);
    let height = vec![10f32; n * n];
    let sea: Vec<bool> = (0..n * n).map(|c| c % n >= 60).collect();
    let r = rainfall(n, cell, &height, &sea, &strong_wind());
    let row = 30 * n;
    for i in 0..57 {
        assert!(r.rain[row + i] <= r.rain[row + i + 1] + 1e-7, "{i}");
    }
}

dwell_world::both_faces!(
    /// The rain shadow on a real face's map (seed 7): with a strong wind, slopes facing it (the
    /// ground rising along the wind) take more rain than slopes in its lee, on each face's own map;
    /// rainfall is bounded; with `rainShare` 0 (the default world's) the wetness is the rivers'
    /// alone, whatever the wind.
    fn the_rain_shadow_falls_behind_high_ground(f: FaceFrame) {
        let t = BifacialTerrain::height_field(7);
        let map = t.local(f.face).map();
        let world_seed = face_seed(7, f.face);
        let wind = strong_wind();
        let field = BiomeField::with_params(map, world_seed, wind);
        let (mut up, mut down) = ((0.0, 0u32), (0.0, 0u32));
        let r = i64::from(WORLD_RADIUS) * 9 / 10;
        for x in (-r..=r).step_by(300) {
            for z in (-r..=r).step_by(300) {
                let h = |x: i64| map.height[map.cell_at(x as f64, z as f64)];
                let c = map.cell_at(x as f64, z as f64);
                if map.is_sea(c)
                    || map.is_sea(map.cell_at((x + 200) as f64, z as f64))
                    || map.is_sea(map.cell_at((x - 200) as f64, z as f64))
                {
                    continue;
                }
                let rain = field.rainfall(x as f64, z as f64);
                assert!((0.0..=1.0).contains(&rain), "bounded at ({x}, {z}): {rain}");
                // The wind blows from +x toward -x: ground rising toward -x (downwind) faces it.
                let grade = f64::from(h(x + 200) - h(x - 200)) / 400.0;
                if grade < -0.05 {
                    up = (up.0 + rain, up.1 + 1);
                } else if grade > 0.05 {
                    down = (down.0 + rain, down.1 + 1);
                }
            }
        }
        assert!(up.1 > 20 && down.1 > 20, "slopes both ways: {up:?} {down:?}");
        let (wet, dry) = (up.0 / f64::from(up.1), down.0 / f64::from(down.1));
        assert!(wet > 2.0 * dry, "windward {wet} vs lee {dry} ({:?})", f.face);
        // Neutral: rainShare 0 is the rivers' wetness, whatever the wind.
        let plain = BiomeField::with_params(map, world_seed, BIOME_MAP);
        let windy = BiomeField::with_params(
            map,
            world_seed,
            BiomeMap {
                rain_share: 0.0,
                ..wind
            },
        );
        let mut differs = 0;
        for (x, z) in [(0, 0), (3000, -2000), (-8000, 5000), (6000, 6000)] {
            assert_eq!(plain.geography(x, z, 100.0), windy.geography(x, z, 100.0));
            differs += usize::from(plain.geography(x, z, 100.0) != field.geography(x, z, 100.0));
        }
        assert!(differs > 0, "rainShare 1 changes the wetness");
    }
);
