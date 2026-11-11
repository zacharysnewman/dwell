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

/// Columns round the cone: (m above the sea, slope, whether its top is the snow line's cover).
fn snow_samples(f: FaceFrame) -> Vec<(f64, f64, bool)> {
    let v = fixture().tectonics.volcanoes()[0];
    let m = map(f, fixture());
    let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, fixture())));
    let snow = dwell_blocks::block_of(dwell_blocks::require("dwell:snowy_grass")).name;
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
            // The landform's slope (the map's, as the snow line reads it).
            let (_, e, n) = m.height_at(f64::from(x), f64::from(z));
            let top = dwell_blocks::block_of(g.top_state(&c)).name == snow;
            out.push((
                f64::from(c.height) - SEA_LEVEL_M,
                (e * e + n * n).sqrt(),
                top,
            ));
        }
    }
    out
}

dwell_world::both_faces!(
    /// The snow line from temperature: gentle ground above 2300 m (well below 0 °C) is snow,
    /// ground below 1600 m is not, and faces steeper than `snowMaxSlope` stay bare rock.
    fn snow_lies_above_the_line_and_not_on_steep_faces(f: FaceFrame) {
        let max = fixture().tectonics.snow_max_slope;
        let s = snow_samples(f);
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
