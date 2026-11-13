//! The Realistic world's uplift from plate boundaries (docs/proposals/REALISTIC_WORLD.md R2,
//! `shared/worldgen/worlds/realistic.json`: `tectonics.plates` 1). For seed fixtures on both faces,
//! on the world map the chunks follow: ranges stand along convergent boundaries above the plain map
//! (the same plates with every landform at 0), rifts below it, the arcs' cones on the overriding
//! side, hotspot chains older along the plate's motion; the plates decide land and sea; the map
//! builds in about the default's time.

use std::time::Instant;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plate_tectonics::{PlateTectonics, PlateUplift, Shape, Side, UPLIFT};
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
    let pairs: Vec<_> = u
        .pairs
        .iter()
        .map(|p| {
            let mut p = *p;
            let forms: Vec<_> = p
                .landforms
                .iter()
                .map(|f| {
                    let mut f = *f;
                    f.height = 0.0;
                    f
                })
                .collect();
            p.landforms = Box::leak(forms.into_boxed_slice());
            p
        })
        .collect();
    u.pairs = Box::leak(pairs.into_boxed_slice());
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
            if on && t.layout.land_edge(c[0], c[1]) > 3000.0 {
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
        // The disc's outer band is sea.
        let edge = PLATES.reach - 0.5 * PLATES.rim.edge_sea;
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
                // The pair's main landform: a range on the overriding side, a collision astride,
                // a rift (two continents opening); the rest are low.
                let main = line.pair.landforms.iter().find(|f| {
                    f.shape == Shape::Rift
                        || (f.height >= 300.0 && matches!(f.side, Side::Over | Side::Astride))
                });
                let Some(main) = main else { continue };
                for pl in &line.polylines {
                    let (pts, want) = match main.side {
                        Side::Over => (beside(&t, pl, line.over, main.distance), main.height),
                        _ => (beside(&t, pl, None, main.distance), main.height),
                    };
                    // A rift is measured on land: at sea the map's floor (`FLOOR`) holds it.
                    let pts: Vec<_> = pts
                        .into_iter()
                        .filter(|p| want > 0.0 || t.layout.coast_distance(p[0], p[1]) > 2500.0)
                        // A rift where it is cut (mid-segment, not under a junction's range).
                        .filter(|p| {
                            want > 0.0 || t.landform_exact(p[0], p[1]) < 0.3 * want * line.scale
                        })
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
                    // The along-line noise takes up to `along.height` off a range; a rift's axis
                    // steps up to half its step-over off the line.
                    let share = if want > 0.0 {
                        0.4 * (1.0 - UPLIFT.along.height)
                    } else {
                        0.1
                    };
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
            let (mut lifts, mut cones) = (0.0, 0.0);
            for v in t.volcanoes.iter().filter(|v| v.chain == 0) {
                // On the overriding plate of a subducting boundary beside it.
                let here = t.layout.plate_at(v.x, v.z);
                let beside_one = t.lines.iter().any(|l| {
                    l.over == Some(here)
                        && l.polylines.iter().flatten().any(|p| {
                            (p[0] - v.x).powi(2) + (p[1] - v.z).powi(2)
                                <= (UPLIFT.arc.distance.max(4000.0) + UPLIFT.link).powi(2)
                        })
                });
                assert!(
                    beside_one,
                    "seed {s}: an arc cone off its overriding plate: {v:?}"
                );
                let lift = ground(&m, v.x, v.z) - ground(&plain, v.x, v.z);
                assert!(lift > 0.0, "seed {s}: the cone stands {lift:.0} m");
                // An island arc's cones are small islands in the wet sea air, eroded hard (as a
                // young hotspot island, below); a continental arc's stand on its range's crest.
                if t.layout.plates[here].kind == PlateKind::Continental {
                    lifts += lift;
                    cones += v.height;
                }
                arcs += 1;
            }
            // Together the cones stand (one may sit in a wet range's erosion or a junction).
            assert!(
                lifts >= 0.3 * cones,
                "seed {s}: the cones stand {lifts:.0} of {cones:.0} m"
            );
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
        // The landform's lift, with the climate's rain (R3) held off: a young island in the wind's
        // wet air erodes faster (face B seed 0's keeps 90 of its 420 m), which is the climate's
        // doing, not the chain's.
        let mut h = realistic();
        h.tectonics.climate = 0;
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
                    assert!(
                        lift > 0.3 * v.height,
                        "seed {s}: {lift:.0} m of {:.0} m",
                        v.height
                    );
                }
                chains += 1;
            }
        }
        assert!(chains >= 2, "{chains} chains");
    }
);

/// The Realistic map builds in at most 1.6× the default's time (best of three each). 1.3× until
/// the realistic batch (0.61): its plates run out to the rim (two to three times the land to
/// erode), its relief reads turned, warped noise and its erosion draws each step's routing
/// (part B: about +0.4 s each on seed 7 after the warp and coast were shared between the height
/// and the swells and the routing's interior cells unchecked), so the map is ~1.45× the default's.
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
    assert!(real <= 1.6 * default, "{real:.3} s vs {default:.3} s");
}

/// The seeds the realistic batch measured on (WORK_QUEUE item 15; 2741893632 the owner's).
const BATCH: [u64; 4] = [1, 7, 42, 2_741_893_632];

dwell_world::both_faces!(
    /// No sliver plates (realistic batch A): over 20 seeds, every continental plate has room
    /// beyond its boundaries inside the land area, and no oceanic plate between two continents is
    /// a strait narrower than twice the oceanic interior.
    fn no_plate_is_all_boundary_and_no_strait_is_a_sliver(f: FaceFrame) {
        for s in (0..18).chain([7, 2_741_893_632]) {
            let l = PlateLayout::for_face(s, f.face);
            let inner = l.interiors();
            for (i, p) in l.plates.iter().enumerate() {
                let need = match p.kind {
                    PlateKind::Continental => PLATES.continental.interior,
                    PlateKind::Oceanic => PLATES.oceanic.interior,
                };
                assert!(
                    inner[i] >= need,
                    "seed {s}: plate {i} {:?} interior {:.1} km",
                    p.kind,
                    inner[i] / 1000.0
                );
            }
            // A strait: an ocean meeting two continents is at least 2 × its interior wide.
            for (i, p) in l.plates.iter().enumerate() {
                if p.kind != PlateKind::Oceanic {
                    continue;
                }
                let shores = l
                    .boundaries
                    .iter()
                    .filter(|b| b.plates.contains(&i))
                    .filter(|b| {
                        let o = if b.plates[0] == i {
                            b.plates[1]
                        } else {
                            b.plates[0]
                        };
                        l.plates[o].kind == PlateKind::Continental
                    })
                    .count();
                if shores >= 2 {
                    assert!(
                        2.0 * inner[i] >= 10_000.0,
                        "seed {s}: strait {:.1} km",
                        2.0 * inner[i] / 1000.0
                    );
                }
            }
        }
    }
);

/// Each lake's extent (km, the bounding box's diagonal) and its cells, by lake.
fn lakes(m: &WorldMap) -> Vec<(f64, [f64; 4], usize)> {
    let mut boxes = vec![([f64::MAX, f64::MAX, f64::MIN, f64::MIN], 0usize); m.lakes.len()];
    for c in 0..m.height.len() {
        let l = usize::from(m.lake[c]);
        if l == 0 || l > m.lakes.len() {
            continue;
        }
        let (x, z) = m.centre(c);
        let (b, n) = &mut boxes[l - 1];
        *b = [b[0].min(x), b[1].min(z), b[2].max(x), b[3].max(z)];
        *n += 1;
    }
    boxes
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(b, n)| ((b[2] - b[0]).hypot(b[3] - b[1]) / 1000.0, b, n))
        .collect()
}

dwell_world::both_faces!(
    /// Segmented rifts (realistic batch A; the owner's 45 km canal on seed 2741893632): no lake
    /// on a rift is longer than a segment and a half, and a continental rift holds a chain of
    /// lakes, not one.
    fn rifts_hold_chains_of_lakes_not_canals(f: FaceFrame) {
        let h = realistic();
        let longest =
            (UPLIFT.rift.segment_length[1] + 0.5 * UPLIFT.rift.segment_length[0]) / 1000.0;
        let mut chains = 0;
        // The batch's seeds and the face's first seeds with a continental rift on land.
        let rifts: Vec<u64> = (0..60u64)
            .filter(|&s| {
                let t = PlateTectonics::for_seed(face_seed(s, f.face));
                t.lines.iter().any(|l| {
                    l.has_rift()
                        && l.polylines
                            .iter()
                            .flatten()
                            .filter(|p| t.layout.coast_distance(p[0], p[1]) > 3000.0)
                            .count()
                            >= 20
                })
            })
            .take(2)
            .collect();
        assert!(!rifts.is_empty(), "a continental rift among 60 seeds");
        for s in BATCH
            .into_iter()
            .take(1)
            .chain([2_741_893_632])
            .chain(rifts.iter().copied())
        {
            let seed = face_seed(s, f.face);
            let t = PlateTectonics::for_seed(seed);
            let m = WorldMap::for_plates(seed, h, &t);
            let rift_lines: Vec<_> = t.lines.iter().filter(|l| l.has_rift()).collect();
            let on_rift = |b: &[f64; 4]| {
                let c = [0.5 * (b[0] + b[2]), 0.5 * (b[1] + b[3])];
                rift_lines.iter().any(|l| {
                    l.polylines
                        .iter()
                        .flatten()
                        .any(|p| (p[0] - c[0]).hypot(p[1] - c[1]) < 4000.0)
                })
            };
            let mut along = 0;
            for (len, b, n) in lakes(&m) {
                if n < 4 || !on_rift(&b) {
                    continue;
                }
                along += 1;
                assert!(
                    len <= longest,
                    "seed {s}: a rift lake {len:.1} km long (≤ {longest:.1})"
                );
            }
            if rifts.contains(&s) && along >= 2 {
                chains += 1;
            }
        }
        assert!(chains >= 1, "a rift with a chain of lakes");
    }
);

dwell_world::both_faces!(
    /// The rim (realistic batch A; owner: "are we hardcoding that there's an ocean edge dropoff
    /// always around?"): the outer band is always sea, where land reaches the outer part of the
    /// disc its coast is not a circle, and on some seeds land comes within a few km of the edge.
    fn the_rim_is_no_circle_and_land_reaches_near_the_edge(f: FaceFrame) {
        let h = realistic();
        let world = PLATES.reach;
        let band = world - PLATES.rim.edge_sea;
        let mut nearest_edge = f64::INFINITY;
        for s in BATCH {
            let seed = face_seed(s, f.face);
            let m = WorldMap::for_height(seed, h);
            let mut outer = Vec::new();
            for a in 0..180 {
                let th = f64::from(a) * std::f64::consts::TAU / 180.0;
                let (c, sn) = (th.cos(), th.sin());
                // The band is sea all round.
                for r in [band + 200.0, 0.5 * (band + world), world - 300.0] {
                    assert!(
                        ground(&m, r * c, r * sn) <= 0.0,
                        "seed {s}: land at {r:.0} m, {a}°"
                    );
                }
                let mut far = 0.0;
                let mut r = 0.6 * world;
                while r < band {
                    if ground(&m, r * c, r * sn) > 0.0 {
                        far = r;
                    }
                    r += 200.0;
                }
                if far > 0.0 {
                    outer.push(far);
                }
            }
            if let Some(&f) = outer.iter().max_by(|a, b| a.total_cmp(b)) {
                nearest_edge = nearest_edge.min(world - f);
            }
            if outer.len() >= 10 {
                let mean = outer.iter().sum::<f64>() / outer.len() as f64;
                let sd = (outer.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                    / outer.len() as f64)
                    .sqrt();
                assert!(
                    sd > 1000.0,
                    "seed {s}: the outer coast is a circle (σ {sd:.0} m)"
                );
            }
        }
        assert!(
            nearest_edge < 6000.0,
            "land within 6 km of the edge on some seed ({nearest_edge:.0} m)"
        );
    }
);
