//! Tall peaks (owner, 2026-11-10: "at least 5000m tall elevations like mount rainier"): the
//! fixture world `test:tall_peak` (`shared/worldgen/worlds/tall_peak.json`, by data alone) raises
//! one 4800 m stratovolcano on a convergent boundary, its cone hardened against the erosion
//! (`tectonics.coneHardness`) so it keeps its relief through the 100 m map. The chunks, the LOD
//! and the chunk keys reach its summit. Since 0.52.40 it is shaped like Rainier by data: radial
//! ridges, an offset summit, a shoulder and flank cones, an ice-domed crater, and a snow line from
//! temperature.

use std::sync::Arc;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::terrain::{FIXTURE_WORLDS, HeightField, TerrainGenerator, Water};
use dwell_worldgen::worldmap::WorldMap;

fn fixture() -> HeightField {
    FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:tall_peak")
        .expect("the tall peak fixture world")
        .1
}

fn map(f: FaceFrame, h: HeightField) -> WorldMap {
    WorldMap::for_height(face_seed(7, f.face), h)
}

/// The highest map ground within `r` m of (x, z), and where.
fn summit(m: &WorldMap, x: f64, z: f64, r: f64) -> (f64, f64, f64) {
    let mut best = (f64::NEG_INFINITY, x, z);
    let n = (r / 50.0) as i32;
    for i in -n..=n {
        for j in -n..=n {
            let (px, pz) = (x + f64::from(i) * 50.0, z + f64::from(j) * 50.0);
            let g = m.height_at(px, pz).0;
            if g > best.0 {
                best = (g, px, pz);
            }
        }
    }
    best
}

dwell_world::both_faces!(
    /// The fixture's cone keeps its height through the erosion: its summit (the crater's rim)
    /// stands at least 4400 m above the sea on the map, with a crater below the rim, and the
    /// chunks generate ground up there.
    fn a_fixture_stratovolcano_stands_over_4400_m(f: FaceFrame) {
        let h = fixture();
        let t = h.tectonics;
        let v = t.volcanoes()[0];
        let m = map(f, h);
        let (top, sx, sz) = summit(&m, v.x, v.z, 1500.0);
        assert!(top >= 4400.0, "the summit stands {top:.0} m above the sea");
        let centre = m.height_at(v.x, v.z).0;
        assert!(
            centre < top - 30.0,
            "a crater: centre {centre:.0}, rim {top:.0}"
        );
        let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, h)));
        let col = g.column_at(sx as i32, sz as i32);
        let above = f64::from(col.height) - SEA_LEVEL_M;
        assert!(
            above >= 4300.0,
            "the chunks' ground at the summit: {above:.0} m"
        );
    }
);

/// The summit's column (face-local x, z, and its top row).
fn summit_column(f: FaceFrame) -> (TerrainGenerator, i64, i64, i64) {
    let h = fixture();
    let v = h.tectonics.volcanoes()[0];
    let m = map(f, h);
    let (_, sx, sz) = summit(&m, v.x, v.z, 1500.0);
    let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(m));
    let top = (f64::from(g.column_at(sx as i32, sz as i32).height) / CELL_H).floor() as i64 - 1;
    (g, sx as i64, sz as i64, top)
}

dwell_world::both_faces!(
    /// The summit's chunk, ~9000 rows up, is inside the build volume and generates ground: the
    /// chunk keys reach it (no row or key cap below 5000 m).
    fn the_summit_chunk_is_in_bounds_and_holds_ground(f: FaceFrame) {
        let (g, x, z, top) = summit_column(f);
        assert!(top as f64 * CELL_H >= 4300.0, "the summit's row {top}");
        let c = ChunkCoord {
            x: x.div_euclid(i64::from(CHUNK_SIZE)) as i32,
            y: top.div_euclid(i64::from(CHUNK_SIZE)) as i32,
            z: z.div_euclid(i64::from(CHUNK_SIZE)) as i32,
        };
        assert!(BUILD_VOLUME.chunk_may_hold(c), "{c:?} in the build volume");
        assert!(!g.is_air_chunk(c), "{c:?} is not skipped as air");
        let mut chunk = Chunk::default();
        g.generate(c, &mut chunk);
        let s = CHUNK_SIZE;
        let solid = (0..s * s * s).any(|i| chunk.get(i % s, (i / s) % s, i / (s * s)) != M.air);
        assert!(solid, "{c:?} holds the summit's ground");
    }
);

dwell_world::both_faces!(
    /// The far terrain holds the peak: at the LOD levels drawn 20–60 km out (cells 16–128 m), the
    /// section around the summit's top is content with solid cells, and its column bounds reach
    /// within two cells of the summit (the peak is not clipped off the horizon).
    fn the_lod_sections_hold_the_summit(f: FaceFrame) {
        use dwell_world::lod::*;
        let (g, x, z, top) = summit_column(f);
        for level in 4..=7 {
            let size = section_size(level);
            let c = LodCoord::new(
                level,
                (x - ORIGIN_X).div_euclid(size) as i32,
                (top - ORIGIN_Y).div_euclid(size) as i32,
                (z - ORIGIN_Z).div_euclid(size) as i32,
            );
            let mut cells = LodCells::new();
            let (kind, bounds) = g.generate_lod_at(c, section_origin(c).y, &mut cells, None);
            assert_ne!(kind, LodKind::Empty, "level {level}: {c:?}");
            assert!(
                cells.iter().any(|&m| m != M.air),
                "level {level}: solid cells"
            );
            let cell = cell_size(level) as f64;
            assert!(
                bounds.hi >= top as f64 - 2.0 * cell,
                "level {level}: the bounds reach {:.0}, the summit row {top}",
                bounds.hi
            );
        }
    }
);

// Rainier (0.52.40): the fixture's cone shaped by data (`ridgeCount`, `summitOffset`, the
// shoulder, the vents, `iceAbove`) and a snow line from temperature (`snowBelowC`).

/// The fixture's summit: the cone's offset apex (face-local x, z).
fn apex() -> (f64, f64) {
    let t = fixture().tectonics;
    t.summit(&t.volcanoes()[0])
}

/// The fixture with the Rainier shape off (the plain cone).
fn plain() -> HeightField {
    let mut h = fixture();
    let t = &mut h.tectonics;
    t.ridge_depth = 0.0;
    t.summit_offset = 0.0;
    t.shoulder_height = 0.0;
    t.vent_height = 0.0;
    h
}

dwell_world::both_faces!(
    /// Radial ridges: round a circle 3 km from the summit the map's ground rises and falls by
    /// more than 150 m (glacier-cut valleys between ridges), with at least 8 crests; the plain
    /// cone varies by less than half that.
    fn the_flanks_carry_radial_ridges(f: FaceFrame) {
        let ring = |h: HeightField| {
            let (ax, az) = h.tectonics.summit(&h.tectonics.volcanoes()[0]);
            let m = map(f, h);
            (0..360)
                .map(|k| {
                    let a = f64::from(k).to_radians();
                    m.height_at(ax + 3000.0 * a.cos(), az + 3000.0 * a.sin()).0
                })
                .collect::<Vec<f64>>()
        };
        let span = |v: &[f64]| {
            v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min)
        };
        let shaped = ring(fixture());
        let crests = (0..360)
            .filter(|&k| {
                let (p, c, n) = (shaped[(k + 359) % 360], shaped[k], shaped[(k + 1) % 360]);
                c > p && c >= n
            })
            .count();
        assert!(
            span(&shaped) > 150.0,
            "the ring spans {:.0} m",
            span(&shaped)
        );
        assert!(crests >= 8, "{crests} crests round the ring");
        let p = ring(plain());
        assert!(
            span(&p) < span(&shaped) * 0.5,
            "plain {:.0} m, ridged {:.0} m",
            span(&p),
            span(&shaped)
        );
    }
);

dwell_world::both_faces!(
    /// An asymmetric summit: the highest ground is the offset apex, not the cone's geometric
    /// centre (≥ 500 m from it).
    fn the_summit_is_not_the_cones_centre(f: FaceFrame) {
        let v = fixture().tectonics.volcanoes()[0];
        let m = map(f, fixture());
        let (top, sx, sz) = summit(&m, v.x, v.z, 2000.0);
        let off = ((sx - v.x).powi(2) + (sz - v.z).powi(2)).sqrt();
        assert!(off >= 500.0, "summit {top:.0} m, {off:.0} m off the centre");
    }
);

dwell_world::both_faces!(
    /// The crater holds ice, not a lake: no column within 800 m of the summit (all far above the
    /// snow line) has water; the plain crater is a hollow there and the ice dome stands over it.
    fn the_crater_holds_no_water_above_the_snow_line(f: FaceFrame) {
        let (ax, az) = apex();
        let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, fixture())));
        for i in -16..=16 {
            for j in -16..=16 {
                let (x, z) = (ax as i32 + i * 50, az as i32 + j * 50);
                let col = g.column_at(x, z);
                assert!(f64::from(col.height) - SEA_LEVEL_M > 2500.0);
                assert_eq!(col.water, Water::None, "water at ({x}, {z})");
            }
        }
        let mut hollow = fixture();
        hollow.tectonics.ice_above = 100_000.0;
        let t = hollow.tectonics;
        let v = t.volcanoes()[0];
        let rim = t.shaped_cone(&v, ax + 400.0, az);
        let centre = t.shaped_cone(&v, ax, az);
        assert!(
            centre < rim - 100.0,
            "the open crater: {centre:.0} under {rim:.0}"
        );
        let iced = fixture().tectonics.shaped_cone(&v, ax, az);
        assert!(iced > rim, "the ice dome {iced:.0} over the rim {rim:.0}");
    }
);

/// Columns round the cone: (m above the sea, slope, its top block's name).
fn cover_samples(f: FaceFrame) -> Vec<(f64, f64, &'static str)> {
    let v = fixture().tectonics.volcanoes()[0];
    let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, fixture())));
    let mut out = Vec::new();
    for k in 0..48 {
        let a = f64::from(k) * 7.5f64.to_radians();
        for r in (600..11_000).step_by(400) {
            let (x, z) = (
                (v.x + f64::from(r) * a.cos()) as i32,
                (v.z + f64::from(r) * a.sin()) as i32,
            );
            let c = g.column_at(x, z);
            if c.water != Water::None {
                continue;
            }
            // The landform's slope as the snow line reads it (the map's with the cone's
            // column-level detail, Rainier detail).
            let slope = g.cover_slope_at(i64::from(x), i64::from(z));
            let top = dwell_blocks::block_of(g.top_state(&c)).name;
            out.push((f64::from(c.height) - SEA_LEVEL_M, slope, top));
        }
    }
    out
}

dwell_world::both_faces!(
    /// The snow line from temperature: gentle ground above 2300 m (well below 0 °C) is snow,
    /// ground below 1600 m is not, and faces steeper than `snowMaxSlope` stay bare rock.
    fn snow_lies_above_the_line_and_not_on_steep_faces(f: FaceFrame) {
        let max = fixture().tectonics.snow_max_slope;
        // The snow line's cover: the snow, or the glacier's ice where colder still.
        let s: Vec<(f64, f64, bool)> = cover_samples(f)
            .into_iter()
            .map(|(h, sl, top)| (h, sl, top == "dwell:snow" || top == "dwell:glacier_ice"))
            .collect();
        let high: Vec<_> = s.iter().filter(|c| c.0 > 2300.0 && c.1 < max).collect();
        let low: Vec<_> = s.iter().filter(|c| c.0 < 1600.0).collect();
        let steep: Vec<_> = s.iter().filter(|c| c.0 > 2300.0 && c.1 > max).collect();
        assert!(
            high.len() > 20 && low.len() > 20,
            "{} high, {} low",
            high.len(),
            low.len()
        );
        let snowy = high.iter().filter(|c| c.2).count();
        assert!(
            snowy * 10 >= high.len() * 9,
            "snow on {snowy} of {} high",
            high.len()
        );
        assert!(low.iter().all(|c| !c.2), "no snow below the line");
        assert!(
            steep.iter().all(|c| !c.2),
            "steep faces bare ({} of them)",
            steep.len()
        );
    }
);

dwell_world::both_faces!(
    /// The glacier (0.52.45, `iceBelowC`): the summit's gentle ground (above 4200 m, well below
    /// -10 °C) is mostly glacier ice, the snow lies below it (2300-3000 m is snow, no ice), and
    /// neither lies below the snow line (1600 m); the crater's dome is ice.
    fn glacier_ice_caps_the_summit_over_the_snow(f: FaceFrame) {
        let max = fixture().tectonics.snow_max_slope;
        let s = cover_samples(f);
        let summit: Vec<_> = s.iter().filter(|c| c.0 > 4200.0 && c.1 < max).collect();
        let mid: Vec<_> = s
            .iter()
            .filter(|c| c.0 > 2300.0 && c.0 < 3000.0 && c.1 < max)
            .collect();
        assert!(
            summit.len() > 5 && mid.len() > 20,
            "{} summit, {} mid",
            summit.len(),
            mid.len()
        );
        let iced = summit.iter().filter(|c| c.2 == "dwell:glacier_ice").count();
        assert!(
            iced * 10 >= summit.len() * 8,
            "ice on {iced} of {} at the summit",
            summit.len()
        );
        let low_ice: Vec<_> = mid
            .iter()
            .filter(|c| c.2 == "dwell:glacier_ice")
            .map(|c| c.0 as i32)
            .collect();
        assert!(
            low_ice.is_empty(),
            "ice mid-slope at {low_ice:?} of {}",
            mid.len()
        );
        let snowy = mid.iter().filter(|c| c.2 == "dwell:snow").count();
        assert!(
            snowy * 10 >= mid.len() * 9,
            "snow on {snowy} of {} mid-slope",
            mid.len()
        );
        assert!(
            s.iter()
                .filter(|c| c.0 < 1600.0)
                .all(|c| c.2 != "dwell:snow" && c.2 != "dwell:glacier_ice"),
            "no snow or ice below the line"
        );
        // The crater's dome: the summit column itself.
        let (ax, az) = apex();
        let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, fixture())));
        let c = g.column_at(ax as i32, az as i32);
        assert_eq!(
            dwell_blocks::block_of(g.top_state(&c)).name,
            "dwell:glacier_ice"
        );
    }
);

// Rainier detail: the cone's column-level detail (`cleaverDepth`, `rimNotches`,
// `cragCount`, `cirqueCount`), added where the chunks generate on top of the 100 m map's cone.

/// The fixture's generator on face `f`.
fn generator(f: FaceFrame, h: HeightField) -> TerrainGenerator {
    TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, h)))
}

/// The fixture with the column-level detail off.
fn undetailed() -> HeightField {
    let mut h = fixture();
    let t = &mut h.tectonics;
    t.cleaver_depth = 0.0;
    t.rim_notch_depth = 0.0;
    t.crag_height = 0.0;
    t.cirque_depth = 0.0;
    h
}

/// The columns' ground (m above the sea) round a circle `r` m from (x, z), one a degree.
fn column_ring(g: &TerrainGenerator, (x, z): (f64, f64), r: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|k| {
            let a = (k as f64 * 360.0 / n as f64).to_radians();
            let c = g.column_at((x + r * a.cos()) as i32, (z + r * a.sin()) as i32);
            f64::from(c.height) - SEA_LEVEL_M
        })
        .collect()
}

fn span(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min)
}

/// The local maxima round a ring standing `by` m over the lower of the ring's lows within `w`
/// samples either side.
fn peaks_round(v: &[f64], w: usize, by: f64) -> usize {
    let n = v.len();
    (0..n)
        .filter(|&k| {
            let near = (1..=w).flat_map(|d| [v[(k + d) % n], v[(k + n - d) % n]]);
            let mut low = f64::MAX;
            let mut top = true;
            for x in near {
                top &= v[k] >= x;
                low = low.min(x);
            }
            top && v[k] - low >= by
        })
        .count()
}

dwell_world::both_faces!(
    /// Cleavers at the column level: round a circle 3 km from the summit the chunks' ground
    /// rises and falls by more than 400 m with at least 10 sharp crests, ≥ 100 m more relief
    /// than the map's cone holds there (measured on columns, not the map), and its steepest step
    /// between columns 1° apart (~52 m) is over 0.6 (a crest's cusp, not a rounded swell).
    fn the_cleavers_survive_at_the_column_level(f: FaceFrame) {
        let (ax, az) = apex();
        let g = generator(f, fixture());
        let ring = column_ring(&g, (ax, az), 3000.0, 360);
        let m = map(f, fixture());
        let map_ring: Vec<f64> = (0..360)
            .map(|k| {
                let a = f64::from(k).to_radians();
                m.height_at(ax + 3000.0 * a.cos(), az + 3000.0 * a.sin()).0
            })
            .collect();
        let plain = column_ring(&generator(f, undetailed()), (ax, az), 3000.0, 360);
        let (s, sm, sp) = (span(&ring), span(&map_ring), span(&plain));
        assert!(s > 400.0, "the columns' ring spans {s:.0} m");
        assert!(s >= sm + 100.0, "columns {s:.0} m, the map {sm:.0} m");
        assert!(s >= sp + 100.0, "columns {s:.0} m, undetailed {sp:.0} m");
        let crests = peaks_round(&ring, 6, 40.0);
        assert!(crests >= 10, "{crests} crests round the ring");
        let step = 3000.0 * 1f64.to_radians();
        let steepest = (0..360)
            .map(|k| (ring[(k + 1) % 360] - ring[k]).abs() / step)
            .fold(0.0, f64::max);
        assert!(steepest > 0.6, "the steepest step {steepest:.2}");
    }
);

dwell_world::both_faces!(
    /// A broken summit: round the crater's rim the ground dips through its notches (at least
    /// `rimNotches` of them, ≥ ½ `rimNotchDepth` deep) and the crags stand over it; the highest
    /// column within 1.5 km is a crag ≥ 200 m off the crater's centre, over the ice dome; without
    /// the detail the rim is smooth by comparison.
    fn the_summit_is_a_broken_rim_with_crags(f: FaceFrame) {
        let h = fixture();
        let t = h.tectonics;
        let (ax, az) = apex();
        let g = generator(f, h);
        let rim = t.crater_radius;
        let ring = column_ring(&g, (ax, az), rim, 360);
        let neg: Vec<f64> = ring.iter().map(|x| -x).collect();
        let notches = peaks_round(&neg, 8, 0.5 * t.rim_notch_depth);
        assert!(
            notches as u32 >= t.rim_notches,
            "{notches} notches round the rim"
        );
        let plain = column_ring(&generator(f, undetailed()), (ax, az), rim, 360);
        assert!(
            span(&ring) > span(&plain) + t.rim_notch_depth,
            "the rim spans {:.0} m, undetailed {:.0} m",
            span(&ring),
            span(&plain)
        );
        let peak = &g.peak_details()[0];
        assert_eq!(peak.crags.len() as u32, t.crag_count);
        let mut top = (f64::MIN, 0.0, 0.0);
        for i in -60..=60 {
            for j in -60..=60 {
                let (x, z) = (ax + f64::from(i) * 25.0, az + f64::from(j) * 25.0);
                let c = f64::from(g.column_at(x as i32, z as i32).height) - SEA_LEVEL_M;
                if c > top.0 {
                    top = (c, x, z);
                }
            }
        }
        let off = ((top.1 - ax).powi(2) + (top.2 - az).powi(2)).sqrt();
        let dome = f64::from(g.column_at(ax as i32, az as i32).height) - SEA_LEVEL_M;
        assert!(
            off >= 200.0,
            "the highest column {:.0} m, {off:.0} m off",
            top.0
        );
        assert!(
            top.0 > dome + 50.0,
            "the highest {:.0} m, the dome {dome:.0} m",
            top.0
        );
        for &(cx, cz, ch) in &peak.crags {
            let c = f64::from(g.column_at(cx as i32, cz as i32).height) - SEA_LEVEL_M;
            let around = column_ring(&g, (cx, cz), 2.0 * t.crag_radius, 36);
            let low = around.iter().copied().fold(f64::MAX, f64::min);
            assert!(
                c > low + 0.5 * ch,
                "a crag {c:.0} m over {low:.0} m round it"
            );
        }
    }
);

dwell_world::both_faces!(
    /// Cirques: each is a hollow on the upper flank, below the summit (its floor lower than every
    /// column round it from 0.1 to 0.3 of its radius: a local minimum, with a lip downhill ≥ 20 m over
    /// it), under a steep
    /// headwall on the summit side (a rise steeper than 1 between the floor and the bowl's back),
    /// and above the ice line its gentle floor is glacier ice.
    fn cirques_are_hollows_under_steep_headwalls(f: FaceFrame) {
        let h = fixture();
        let t = h.tectonics;
        let g = generator(f, h);
        let peak = g.peak_details()[0].clone();
        assert_eq!(peak.cirques.len() as u32, t.cirque_count);
        let top = f64::from(g.column_at(peak.apex.0 as i32, peak.apex.1 as i32).height);
        for &(cx, cz, th, _) in &peak.cirques {
            let floor_col = g.column_at(cx as i32, cz as i32);
            let floor = f64::from(floor_col.height) - SEA_LEVEL_M;
            assert!(
                floor < top - SEA_LEVEL_M - 500.0 && floor > 2500.0,
                "a cirque on the upper flank at {floor:.0} m"
            );
            for k in 2..=6 {
                let r = 0.05 * f64::from(k) * t.cirque_radius;
                let round = column_ring(&g, (cx, cz), r, 48);
                let low = round.iter().copied().fold(f64::MAX, f64::min);
                assert!(
                    low > floor + 1.0,
                    "a hollow: floor {floor:.0} m, its low {low:.0} m at {r:.0} m"
                );
            }
            // The lip downhill: the ground rises over the floor before it falls away.
            let lip = (1..60)
                .map(|i| {
                    let d = f64::from(i) * 10.0;
                    let (x, z) = (cx + d * th.cos(), cz + d * th.sin());
                    f64::from(g.column_at(x as i32, z as i32).height) - SEA_LEVEL_M - floor
                })
                .fold(f64::MIN, f64::max);
            assert!(lip > 20.0, "the lip stands {lip:.0} m over the floor");
            // The headwall: up toward the summit, the steepest rise between columns 10 m apart.
            let (ux, uz) = (-th.cos(), -th.sin());
            let steepest = (0..80)
                .map(|i| {
                    let d = f64::from(i) * 10.0;
                    let at = |d: f64| {
                        f64::from(
                            g.column_at((cx + d * ux) as i32, (cz + d * uz) as i32)
                                .height,
                        )
                    };
                    (at(d + 10.0) - at(d)) / 10.0
                })
                .fold(f64::MIN, f64::max);
            assert!(steepest > 1.0, "the headwall's steepest rise {steepest:.2}");
            assert_eq!(
                dwell_blocks::block_of(g.top_state(&floor_col)).name,
                "dwell:glacier_ice",
                "the cirque's floor at {floor:.0} m"
            );
        }
    }
);

// face-a-only: the registry has no face.
/// The ice blocks load by name (the creative catalogue's, shared/items/items.json).
#[test]
fn the_ice_blocks_load_by_name() {
    for name in ["dwell:snow", "dwell:glacier_ice"] {
        assert!(dwell_blocks::find_block(name).is_some(), "{name} missing");
        assert!(dwell_blocks::parse_state(name).is_ok(), "{name} state");
    }
}

// face-a-only: the data's parsing and ranges, no face.
/// The Rainier detail's keys: off in the build's data (no detail placed), read from a
/// world's knobs by name, and refused out of range.
#[test]
fn the_rainier_detail_keys_parse_and_are_checked() {
    use dwell_worldgen::config::height_from_json;
    use dwell_worldgen::terrain::HEIGHT;
    let t = HEIGHT.tectonics;
    assert!(
        !t.detailed(),
        "the default world has no column-level detail"
    );
    assert!(t.peak_details(|_, _| 0.0).is_empty());
    let on = serde_json::json!({ "tectonics": {
        "kind": 1, "arcCount": 1, "cleaverDepth": 120, "cleaverCount": 20, "rimNotches": 3,
        "rimNotchDepth": 40, "cragCount": 2, "cragHeight": 90, "cragRadius": 50,
        "cirqueCount": 3, "cirqueDepth": 200, "cirqueRadius": 500, "cirqueDistance": 1000
    }});
    let h = height_from_json(&on).expect("the detail's keys");
    let k = h.tectonics;
    assert_eq!(
        (k.cleaver_count, k.rim_notches, k.crag_count, k.cirque_count),
        (20, 3, 2, 3)
    );
    assert_eq!(
        (
            k.cleaver_depth,
            k.rim_notch_depth,
            k.crag_height,
            k.crag_radius
        ),
        (120.0, 40.0, 90.0, 50.0)
    );
    assert_eq!(
        (k.cirque_depth, k.cirque_radius, k.cirque_distance),
        (200.0, 500.0, 1000.0)
    );
    assert!(k.detailed());
    let p = k.peak_details(|_, _| 1000.0);
    assert_eq!((p.len(), p[0].cirques.len(), p[0].crags.len()), (1, 3, 2));
    for bad in [
        serde_json::json!({ "tectonics": { "cirqueRadius": 0 } }),
        serde_json::json!({ "tectonics": { "cragRadius": 0 } }),
        serde_json::json!({ "tectonics": { "rimNotches": 17 } }),
        serde_json::json!({ "tectonics": { "cleaverDepth": -5 } }),
    ] {
        assert!(height_from_json(&bad).is_err(), "{bad} refused");
    }
}
