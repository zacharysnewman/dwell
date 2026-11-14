//! No circular cliff round the Realistic world's cones (WORK_QUEUE item 15, the map check of
//! 2026-11-14: "cones are smooth discs with circular cliff rims"). The erosion's cap on a cone
//! (its `shield`, plates.json `uplift.arc.hardness`) held the cone whole out to its radius and let
//! the ground beyond erode, so a ring-shaped step stood at the foot; the cap now fades out toward
//! the foot. On seeds 1, 7, 42, 274005646 and 2741893632, both faces, for every arc cone mostly on
//! land, each ray's slope break at the radius, (a − b) / (|a| + |b| + 20 m) of its fall just
//! inside (a, 0.85 to 0.95 of the radius) and across it (b, 0.95 to 1.05): +1 is all the fall
//! inside and none outside, a cliff's edge. The 48 rays' median, less the same measure's mean at
//! 0.6, 0.7, 1.3 and 1.4 of the radius (the ground's own relief), is at most [`RIM_BREAK`]. Before
//! the fix every cone read 0.60 to 0.96; after, −0.31 to 0.26.

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plate_tectonics::PlateTectonics;
use dwell_worldgen::terrain::FIXTURE_WORLDS;
use dwell_worldgen::worldmap::WorldMap;

const SEEDS: [u64; 5] = [1, 7, 42, 274_005_646, 2_741_893_632];
const RIM_BREAK: f64 = 0.4;

both_faces!(
    fn no_circular_cliff_rings_the_cones(f: FaceFrame) {
        let h = FIXTURE_WORLDS
            .iter()
            .find(|(n, _)| *n == "test:realistic")
            .expect("the Realistic world")
            .1;
        let mut seen = 0;
        let mut report = Vec::new();
        for world in SEEDS {
            let seed = face_seed(world, f.face);
            let t = PlateTectonics::for_seed(seed);
            let map = WorldMap::for_height(seed, h);
            for v in t
                .volcanoes
                .iter()
                .filter(|v| v.chain == 0 && v.crater_radius > 0.0)
            {
                let at = |r: f64, th: f64| {
                    map.height_at(v.x + r * v.radius * th.cos(), v.z + r * v.radius * th.sin())
                        .0
                };
                // Each ray's fall across `r` (from 0.95 r to 1.05 r) over its fall just inside
                // (0.85 r to 0.95 r), the rays' median; `None` for a ring mostly at sea.
                let excess = |r: f64| {
                    let mut e: Vec<f64> = (0..48)
                        .filter_map(|a| {
                            let th = f64::from(a) * std::f64::consts::TAU / 48.0;
                            let (inner, rim, outer) =
                                (at(r - 0.15, th), at(r - 0.05, th), at(r + 0.05, th));
                            // The break: +1 all the fall inside, none outside (a cliff's edge).
                            (outer > 2.0).then(|| {
                                let (a, b) = (inner - rim, rim - outer);
                                (a - b) / (a.abs() + b.abs() + 20.0)
                            })
                        })
                        .collect();
                    (e.len() >= 24).then(|| {
                        e.sort_by(f64::total_cmp);
                        e[e.len() / 2]
                    })
                };
                // Against the same measure at radii away from the foot (the ground's own relief).
                let Some(foot) = excess(1.0) else { continue };
                let controls: Vec<f64> = [0.6, 0.7, 1.3, 1.4]
                    .into_iter()
                    .filter_map(excess)
                    .collect();
                if controls.len() < 3 {
                    continue;
                }
                let control = controls.iter().sum::<f64>() / controls.len() as f64;
                let median = foot - control;
                seen += 1;
                report.push(format!(
                    "seed {world} ({:.0}, {:.0}): {median:.2}",
                    v.x, v.z
                ));
                assert!(
                    median <= RIM_BREAK,
                    "seed {world}: the cone at ({:.0}, {:.0}) r {:.0} breaks {median:.2} at \
                     its foot: steep inside, flat beyond (a circular cliff)",
                    v.x,
                    v.z,
                    v.radius
                );
            }
        }
        eprintln!("{}", report.join("\n"));
        assert!(seen >= 4, "{seen} cones on land");
    }
);
