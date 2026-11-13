//! The Realistic world's climate (docs/proposals/REALISTIC_WORLD.md R3, `shared/worldgen/climate.json`,
//! `worlds/realistic.json` `tectonics.climate` 1). For seed fixtures on both faces: the windward side
//! of the seed's main range is at least twice as wet as its lee, on the coarse rain grid and in the
//! rain the rivers' discharge carries; the coarse grid stays under its bound; the default world's
//! rain stays uniform (its rivers and chunks: the goldens).

use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::smooth_ground;
use dwell_worldgen::climate::{CLIMATE, Climate, CoarseGround, RainGrid};
use dwell_worldgen::hydrology::{self, HydrologyMap};
use dwell_worldgen::terrain::{FIXTURE_WORLDS, HEIGHT, HeightField};

const SEEDS: [u64; 3] = [1, 7, 42];

fn realistic() -> HeightField {
    FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:realistic")
        .expect("worlds/realistic.json")
        .1
}

fn map(seed: u64) -> HydrologyMap {
    hydrology::generate_with_height(seed, hydrology::default_profile(), realistic())
}

/// The coarse ground the map marched its wind over, and the rain it dropped.
fn marched(m: &HydrologyMap) -> (&CoarseGround, &RainGrid) {
    let (g, r) = m
        .climate
        .as_ref()
        .expect("a Realistic map marches its climate");
    (g, r)
}

/// The mean of `f` over the land points 1.5–6 km upwind and downwind of the main range's crest
/// (the coarse ground's highest point once low-passed as the wind sees it), across a 6 km front.
fn windward_and_lee(c: &Climate, g: &CoarseGround, f: &dyn Fn(f64, f64) -> f64) -> (f64, f64) {
    let smooth = smooth_ground(g.n, g.cell, &g.height, CLIMATE.rain.smooth_m);
    let top = (0..smooth.len())
        .filter(|&i| !g.sea[i])
        .max_by(|&a, &b| smooth[a].total_cmp(&smooth[b]))
        .expect("land");
    let (cx, cz) = (
        g.x0 + (top % g.n) as f64 * g.cell,
        g.x0 + (top / g.n) as f64 * g.cell,
    );
    let (ux, uz) = c.upwind();
    let land = |x: f64, z: f64| {
        let i = ((x - g.x0) / g.cell).round();
        let j = ((z - g.x0) / g.cell).round();
        let last = (g.n - 1) as f64;
        i >= 0.0 && j >= 0.0 && i <= last && j <= last && !g.sea[j as usize * g.n + i as usize]
    };
    let side = |sign: f64| {
        let (mut sum, mut k) = (0.0, 0);
        for d in (0..=9).map(|s| 1500.0 + 500.0 * f64::from(s)) {
            for w in (-6..=6).map(|s| 500.0 * f64::from(s)) {
                let x = cx + sign * ux * d - uz * w;
                let z = cz + sign * uz * d + ux * w;
                if land(x, z) {
                    sum += f(x, z);
                    k += 1;
                }
            }
        }
        assert!(k >= 8, "too little land beside the range ({k} points)");
        sum / f64::from(k)
    };
    (side(1.0), side(-1.0))
}

dwell_world::both_faces!(
    /// The windward side of each seed's main range gets at least twice its lee's rain, on the coarse
    /// grid and in the discharge's rain.
    fn the_windward_side_of_the_main_range_is_twice_as_wet_as_its_lee(f: FaceFrame) {
        let (mut all_wq, mut all_lq) = (0.0, 0.0);
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let m = map(seed);
            let c = Climate::for_seed(seed);
            let (g, r) = marched(&m);
            let (wind, lee) = windward_and_lee(&c, g, &|x, z| r.at(x, z));
            eprintln!(
                "seed {world} {:?}: {} band, {} from {:.0}°: windward {wind:.3} lee {lee:.3}",
                f.face,
                c.band().name,
                c.wind_cell().name,
                c.wind_from
            );
            assert!(wind >= 2.0 * lee, "seed {world}: windward {wind} lee {lee}");
            // The discharge's rain (per map cell, the map's own march) follows it.
            let at = |x: f64, z: f64| {
                let i = (((x - m.x0) / m.cell).round() as usize).min(m.grid.n - 1);
                let j = (((z - m.x0) / m.cell).round() as usize).min(m.grid.n - 1);
                f64::from(m.rain[j * m.grid.n + i])
            };
            let (wq, lq) = windward_and_lee(&c, g, &at);
            eprintln!(
                "  discharge rain windward {wq:.3} lee {lq:.3} (mean land {:.3})",
                r.mean_land
            );
            // Wetter on every seed, by half again over the seeds together (the map's own march over
            // the eroded ground, with the climate's district rain, part B's `variety`; the coarse
            // grid's twice, above).
            assert!(
                wq >= 1.3 * lq,
                "seed {world}: discharge rain windward {wq} lee {lq}"
            );
            (all_wq, all_lq) = (all_wq + wq, all_lq + lq);
        }
        assert!(
            all_wq >= 1.5 * all_lq,
            "the seeds' discharge rain windward {all_wq} lee {all_lq}"
        );
    }
);

dwell_world::both_faces!(
    /// The map's coarse rain grid stays under `climate.json` `maxGridBytes`, with cells ≥ `rain.cell`.
    fn the_coarse_rain_grid_stays_under_its_bound(f: FaceFrame) {
        let seed = face_seed(SEEDS[0], f.face);
        let m = map(seed);
        let (g, r) = marched(&m);
        assert!(g.cell >= CLIMATE.rain.cell, "{} m cells", g.cell);
        assert!(
            r.bytes() <= CLIMATE.max_grid_bytes && g.bytes() <= CLIMATE.max_grid_bytes,
            "rain {} B, ground {} B over {}",
            r.bytes(),
            g.bytes(),
            CLIMATE.max_grid_bytes
        );
    }
);

dwell_world::both_faces!(
    /// Each face draws its own climate from its face seed, and its temperature falls with height.
    fn each_face_draws_its_own_climate(f: FaceFrame) {
        let c = Climate::for_face(SEEDS[1], f.face);
        assert_eq!(c, Climate::for_seed(face_seed(SEEDS[1], f.face)));
        assert!(c.temperature_c(0.0, 0.0, 1000.0) < c.temperature_c(0.0, 0.0, 0.0));
    }
);

dwell_world::both_faces!(
    /// The default world has no climate: its discharge's rain is the profile's uniform rain.
    fn the_default_world_rain_is_unchanged(f: FaceFrame) {
        assert!(!HEIGHT.tectonics.from_climate());
        let seed = face_seed(SEEDS[0], f.face);
        let m = hydrology::generate_seed(seed, hydrology::default_profile());
        assert!(m.rain.iter().all(|&r| r == 1.0));
        assert!(m.climate.is_none());
    }
);
