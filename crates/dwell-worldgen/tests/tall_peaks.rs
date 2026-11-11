//! Tall peaks (owner, 2026-11-10: "at least 5000m tall elevations like mount rainier"): the
//! fixture world `test:tall_peak` (`shared/worldgen/worlds/tall_peak.json`, by data alone) raises
//! one 4800 m stratovolcano on a convergent boundary, its cone hardened against the erosion
//! (`tectonics.coneHardness`) so it keeps its relief through the 100 m map, with its crater. The
//! chunks, the LOD and the chunk keys reach its summit.

use std::sync::Arc;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::terrain::{FIXTURE_WORLDS, HeightField, TerrainGenerator};
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
        let solid =
            (0..s * s * s).any(|i| chunk.get(i % s, (i / s) % s, i / (s * s)) != M.air);
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
