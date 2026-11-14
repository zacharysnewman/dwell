//! No straight seams between terrain types (BUGS.md #164; owner, 2026-11-13: "I'm seeing really
//! sharp edges between terrain types like this": a dead-straight, axis-aligned line many km long
//! between snow and forest). On seeds 1, 7, 42 and 2741893632, both faces, over the land every
//! [`STEP`] m: no biome edge runs straight along one axis for more than [`MAX_SEAM`] m, and the
//! biomes' rain changes by at most [`MAX_RAIN_STEP`] of itself between neighbours (the rain's march left a
//! hard step at every ridge's crest).

use std::sync::Arc;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::worldmap::WorldMap;

const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const STEP: i64 = 50;
const MAX_SEAM: i64 = 1500;
const MAX_RAIN_STEP: f64 = 0.15;

both_faces!(
    fn biome_edges_wander_and_the_rain_has_no_steps(f: FaceFrame) {
        let e = world_config("test:realistic")
            .unwrap()
            .expect("the Realistic world");
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let map = Arc::new(
                WorldMap::for_height(seed, e.height.expect("its height field"))
                    .with_worldgen("test:realistic")
                    .unwrap(),
            );
            let field = BiomeField::with_params(&map, seed, e.map);
            let r = 46_000 / STEP;
            let n = (2 * r + 1) as usize;
            let (mut biome, mut rain) = (vec![u8::MAX; n * n], vec![f64::NAN; n * n]);
            for j in 0..n {
                for i in 0..n {
                    let (x, z) = ((i as i64 - r) * STEP, (j as i64 - r) * STEP);
                    if map.is_sea(map.cell_at(x as f64, z as f64)) {
                        continue;
                    }
                    let h = map.height_at(x as f64, z as f64).0.max(0.0);
                    biome[j * n + i] = field.mix(&e.biomes, x, z, h).chosen;
                    rain[j * n + i] = field.geography(x, z, h).rain.unwrap_or(f64::NAN);
                }
            }
            // The longest run of one axis-aligned edge (the same two neighbours apart).
            let (mut seam, mut at) = (0usize, (0, 0));
            let mut step = 0.0f64;
            for (a, b, along) in [(1usize, n, 'v'), (n, 1, 'h')] {
                for line in 0..n - 1 {
                    let mut run = 0;
                    for t in 0..n {
                        let k = line * a + t * b;
                        let other = k + a;
                        let differs = biome[k] != u8::MAX
                            && biome[other] != u8::MAX
                            && biome[k] != biome[other];
                        run = if differs { run + 1 } else { 0 };
                        if run > seam {
                            seam = run;
                            at = (line, t);
                        }
                        // A share of the wetter side's rain (a step at 4000 mm matters less than one
                        // across a desert's edge).
                        let d = (rain[k] - rain[other]).abs() / rain[k].max(rain[other]).max(100.0);
                        if d.is_finite() {
                            step = step.max(d);
                        }
                    }
                }
                let _ = along;
            }
            let seam_m = seam as i64 * STEP;
            eprintln!(
                "seed {world} {:?}: longest straight biome edge {seam_m} m (near line {}, {}), largest rain step {:.1} % per {STEP} m",
                f.face,
                at.0,
                at.1,
                100.0 * step
            );
            assert!(
                seam_m <= MAX_SEAM,
                "seed {world}: a straight biome edge {seam_m} m long"
            );
            assert!(
                step <= MAX_RAIN_STEP,
                "seed {world}: the rain steps {:.1} % in {STEP} m",
                100.0 * step
            );
        }
    }
);
