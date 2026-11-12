//! The Realistic world's uplift from plate boundaries (docs/proposals/REALISTIC_WORLD.md R2,
//! `shared/worldgen/worlds/realistic.json`: `tectonics.plates` 1). For seed fixtures on both faces,
//! on the world map the chunks follow: ranges stand along convergent boundaries above the plain map
//! (the same plates with every landform at 0), rifts below it, the arcs' cones on the overriding
//! side, hotspot chains older along the plate's motion; the plates decide land and sea; the map
//! builds in about the default's time.

use std::time::Instant;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plate_tectonics::{PlateTectonics, PlateUplift, UPLIFT};
use dwell_worldgen::plates::{BoundaryKind, PLATES, PlateKind, PlateLayout};
use dwell_worldgen::terrain::{FIXTURE_WORLDS, HeightField};
use dwell_worldgen::worldmap::WorldMap;

const SEEDS: [u64; 3] = [1, 7, 42];

fn realistic() -> HeightField {
    FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:realistic")
        .expect("worlds/realistic.json")
        .1
}

/// The uplift with every landform, cone and island at 0: the plain map of the same plates.
fn flat() -> PlateUplift {
    let mut u = UPLIFT;
    for l in [
        &mut u.range,
        &mut u.trench,
        &mut u.collision,
        &mut u.rift,
        &mut u.transform,
    ] {
        l.height = 0.0;
    }
    u.arc.cone_height = 0.0;
    u.hotspot.height = 0.0;
    u
}

fn ground(m: &WorldMap, x: f64, z: f64) -> f64 {
    m.height_at(x, z).0
}

/// Points `off` m from a polyline's vertices, on `plate`'s side (or on the line at 0), kept well
/// inside the disc's land reach.
fn beside(t: &PlateTectonics, pl: &[[f64; 2]], plate: Option<usize>, off: f64) -> Vec<[f64; 2]> {
    let keep = PLATES.reach - PLATES.sea_margin - 3000.0;
    let mut out = Vec::new();
    for w in pl.windows(2) {
        let (p, q) = (w[0], w[1]);
        let len = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2))
            .sqrt()
            .max(1e-9);
        let n = [-(q[1] - p[1]) / len, (q[0] - p[0]) / len];
        let mid = [0.5 * (p[0] + q[0]), 0.5 * (p[1] + q[1])];
        for s in [1.0, -1.0] {
            let c = [mid[0] + s * off * n[0], mid[1] + s * off * n[1]];
            let on = plate.is_none_or(|i| t.layout.plate_at(c[0], c[1]) == i);
            if on && c[0] * c[0] + c[1] * c[1] < keep * keep {
                out.push(c);
                break;
            }
        }
    }
    out
}

dwell_world::both_faces!(
    /// The world file turns the plates on; the plates' coast is the map's (land on continental
    /// plates, sea on oceanic ones and at the edge).
    fn the_plates_decide_land_and_sea(f: FaceFrame) {
        let h = realistic();
        assert!(h.tectonics.from_plates() && h.tectonics.enabled());
        let seed = face_seed(SEEDS[0], f.face);
        let full = WorldMap::for_height(seed, h);
        let layout = PlateLayout::generate(&PLATES, seed);
        let t = PlateTectonics::new(layout.clone(), &flat());
        // The plates alone (every landform at 0): the arcs' and ranges' islands come on top.
        let m = WorldMap::for_plates(seed, h, &t);
        let (mut agree, mut n, mut full_agree) = (0, 0, 0);
        for k in 0..400 {
            let (x, z) = (
                -21000.0 + 2100.0 * f64::from(k % 20),
                -21000.0 + 2100.0 * f64::from(k / 20),
            );
            // Away from the coast (the erosion and the shelf move it a little).
            if layout.coast_distance(x, z).abs() < 2500.0 {
                continue;
            }
            n += 1;
            agree += usize::from((ground(&m, x, z) > 0.0) == layout.is_land(x, z));
            full_agree += usize::from((ground(&full, x, z) > 0.0) == layout.is_land(x, z));
        }
        assert!(n > 100);
        assert!(
            agree * 100 >= 97 * n,
            "{agree} of {n} agree with the plates"
        );
        // With the landforms: still mostly the plates' (islands, rifts' bays and trenches aside).
        assert!(
            full_agree * 100 >= 75 * n,
            "{full_agree} of {n} agree with the plates"
        );
        // The disc's edge is sea.
        let edge = PLATES.reach + 2000.0;
        assert!(ground(&m, edge, 0.0) <= 0.0 && ground(&m, 0.0, -edge) <= 0.0);
    }
);

dwell_world::both_faces!(
    /// For seed fixtures: ranges along convergent boundaries stand above the plain map (on the
    /// overriding side of a subducting one, astride a collision), rifts below it, transforms
    /// barely move it, and the arcs' cones stand on the overriding plate.
    fn boundaries_raise_ranges_and_cut_rifts_by_their_kind(f: FaceFrame) {
        let h = realistic();
        let (mut ranges, mut rifts, mut arcs) = (0, 0, 0);
        // The fixtures, and the face's first seed with a rift inside a continent (found from the
        // layout alone).
        let rift = (0..200u64)
            .find(|&s| {
                let t = PlateTectonics::for_seed(face_seed(s, f.face));
                t.lines.iter().any(|l| {
                    let [a, b] = t.layout.boundaries[l.boundary].plates;
                    l.kind == BoundaryKind::Divergent
                        && [a, b]
                            .iter()
                            .all(|&i| t.layout.plates[i].kind == PlateKind::Continental)
                })
            })
            .expect("a continental rift among 200 seeds");
        for s in SEEDS.into_iter().chain([rift]) {
            let seed = face_seed(s, f.face);
            let t = PlateTectonics::for_seed(seed);
            let plain_t = PlateTectonics::new(t.layout.clone(), &flat());
            let m = WorldMap::for_plates(seed, h, &t);
            let plain = WorldMap::for_plates(seed, h, &plain_t);
            for line in &t.lines {
                for pl in &line.polylines {
                    let (pts, want) = match (line.kind, line.over) {
                        (BoundaryKind::Convergent, Some(over)) => (
                            beside(&t, pl, Some(over), UPLIFT.range.distance),
                            UPLIFT.range.height,
                        ),
                        (BoundaryKind::Convergent, None) => (
                            beside(&t, pl, None, UPLIFT.collision.distance),
                            UPLIFT.collision.height,
                        ),
                        (BoundaryKind::Divergent, _) => (
                            beside(&t, pl, None, UPLIFT.rift.distance),
                            -UPLIFT.rift.height,
                        ),
                        (BoundaryKind::Transform, _) => continue,
                    };
                    // A rift is measured on land: at sea the map's floor (`FLOOR`) holds it.
                    let pts: Vec<_> = pts
                        .into_iter()
                        .filter(|p| want > 0.0 || t.layout.coast_distance(p[0], p[1]) > 2500.0)
                        .collect();
                    if pts.len() < 4 {
                        continue;
                    }
                    let mean = pts
                        .iter()
                        .map(|p| ground(&m, p[0], p[1]) - ground(&plain, p[0], p[1]))
                        .sum::<f64>()
                        / pts.len() as f64;
                    // A rift's floor partly fills (its lakes and the erosion's deposits).
                    let share = if want > 0.0 { 0.4 } else { 0.2 };
                    let need = share * want.abs() * line.scale;
                    if want > 0.0 {
                        assert!(mean > need, "seed {s}: range {mean:.0} m, need {need:.0}");
                        ranges += 1;
                    } else {
                        assert!(mean < -need, "seed {s}: rift {mean:.0} m, need -{need:.0}");
                        rifts += 1;
                    }
                }
            }
            for v in t.volcanoes.iter().filter(|v| v.chain == 0) {
                let over = t.lines.iter().find_map(|l| {
                    let near = l.polylines.iter().flatten().any(|p| {
                        (p[0] - v.x).powi(2) + (p[1] - v.z).powi(2)
                            <= (UPLIFT.arc.distance + UPLIFT.link).powi(2)
                    });
                    l.over.filter(|_| near)
                });
                let over = over.expect("an arc cone beside a subducting boundary");
                assert_eq!(t.layout.plate_at(v.x, v.z), over, "seed {s}: {v:?}");
                let lift = ground(&m, v.x, v.z) - ground(&plain, v.x, v.z);
                assert!(
                    lift > 0.3 * v.height,
                    "seed {s}: the cone stands {lift:.0} m"
                );
                arcs += 1;
            }
        }
        assert!(ranges >= 2, "{ranges} ranges checked");
        assert!(rifts >= 1, "{rifts} rifts checked");
        assert!(arcs >= 1, "{arcs} arc cones checked");
    }
);

dwell_world::both_faces!(
    /// Hotspot chains: each island stands above the plain map, the older ones farther along the
    /// plate's motion and lower.
    fn hotspot_chains_age_along_the_plates_motion(f: FaceFrame) {
        let h = realistic();
        let mut chains = 0;
        for s in 0..12u64 {
            let seed = face_seed(s, f.face);
            let t = PlateTectonics::for_seed(seed);
            let n = UPLIFT.hotspot.islands as usize;
            let islands: Vec<_> = t.volcanoes.iter().filter(|v| v.chain > 0).collect();
            for (hs, chain) in t.layout.hotspots.iter().zip(islands.chunks(n)) {
                if hs.trail == [0.0, 0.0] || chain.len() < 2 {
                    continue;
                }
                let m = t.layout.plates[hs.plate].motion;
                let along = |x: f64, z: f64| x * m[0] + z * m[1];
                for w in chain.windows(2) {
                    assert!(along(w[1].x, w[1].z) > along(w[0].x, w[0].z), "seed {s}");
                    assert!(w[1].height < w[0].height);
                }
                if chains == 0 {
                    // One map is enough to see the youngest island stand on the ground.
                    let map = WorldMap::for_plates(seed, h, &t);
                    let plain = WorldMap::for_plates(
                        seed,
                        h,
                        &PlateTectonics::new(t.layout.clone(), &flat()),
                    );
                    let v = chain[0];
                    let lift = ground(&map, v.x, v.z) - ground(&plain, v.x, v.z);
                    assert!(lift > 0.3 * v.height, "seed {s}: {lift:.0} m");
                }
                chains += 1;
            }
        }
        assert!(chains >= 2, "{chains} chains");
    }
);

/// The Realistic map builds in at most 1.3× the default's time (best of three each).
#[test]
#[cfg_attr(debug_assertions, ignore = "timed: release only")]
fn the_realistic_map_builds_in_about_the_defaults_time() {
    // face-a-only: a timing; the faces' maps are built the same way.
    let seed = face_seed(7, Face::A);
    let h = realistic();
    let best = |f: &dyn Fn()| {
        (0..3)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64()
            })
            .fold(f64::INFINITY, f64::min)
    };
    let default = best(&|| drop(WorldMap::compute(seed)));
    let real = best(&|| drop(WorldMap::for_height(seed, h)));
    eprintln!("default {default:.3} s, realistic {real:.3} s");
    assert!(real <= 1.3 * default, "{real:.3} s vs {default:.3} s");
}
