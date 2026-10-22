//! The LOD colour map's raster (ARCHITECTURE.md §6.6 "Colour map"): a fixture river, narrower than
//! any ring's texel, stays continuous on every ring.

use std::sync::Arc;

use dwell_client::lod_map::{Rect, raster};
use dwell_look::lod_map::LOD_MAP;
use dwell_worldgen::TerrainGenerator;
use dwell_worldgen::worldmap::{
    self, MapRiver, MapVertex, Mouth, NO_LAKE, RiverEnd, SEA, WorldMap,
};

/// A plain sloping from 40 m down to the sea at +x, and one 3 m river running diagonally across it.
fn fixture(seed: u64) -> WorldMap {
    let n = 64usize;
    let cell = 100.0;
    let x0 = -((n - 1) as f64) * cell / 2.0;
    let mut height = vec![0f32; n * n];
    let mut flags = vec![0u8; n * n];
    for j in 0..n {
        for i in 0..n {
            let x = x0 + i as f64 * cell;
            let h = (20.0 - x / 100.0) as f32;
            let c = j * n + i;
            height[c] = h.max(-10.0);
            flags[c] = if h <= 0.0 { SEA | worldmap::NO_FLOW } else { 0 };
        }
    }
    let path = (0..=20)
        .map(|k| {
            let x = -1000.0 + 100.0 * k as f32;
            MapVertex {
                x,
                z: river_z(f64::from(x)) as f32,
                surface: (20.0 - x / 100.0 - 0.5).max(0.0),
                width: 3.0,
                depth: 1.0,
                discharge: 2.0,
            }
        })
        .collect();
    WorldMap::assemble(
        seed,
        n,
        cell,
        x0,
        height,
        flags,
        vec![NO_LAKE; n * n],
        Vec::new(),
        vec![MapRiver {
            from_lake: NO_LAKE,
            end: RiverEnd::Sea,
            mouth: Mouth::None,
            path,
        }],
    )
}

fn river_z(x: f64) -> f64 {
    -500.0 + 0.6 * (x + 1000.0)
}

/// Blue over red and green: the water's colour (`dwell:water`), not grass, sand or rock.
fn watery(px: &[u8]) -> bool {
    px[2] as i32 > px[0] as i32 + 30 && px[2] > px[1]
}

#[test]
fn a_river_narrower_than_a_texel_is_continuous_on_every_ring() {
    // face-a-only: a fixture map of one face; the raster is face-local (the rings' placing is
    // tested on both faces in lod_map.rs).
    let g = TerrainGenerator::with_map(3, Arc::new(fixture(3)));
    for ring in LOD_MAP.rings {
        let t = ring.texel_m;
        let span = |a: f64, b: f64| ((a / t as f64).floor() as i64, (b / t as f64).ceil() as i64);
        let (x0, x1) = span(-400.0, 400.0);
        let (z0, z1) = span(
            river_z(-400.0) - 2.0 * t as f64,
            river_z(400.0) + 2.0 * t as f64,
        );
        let rect = Rect { x0, z0, x1, z1 };
        let px = raster(&g, &LOD_MAP, t, &rect);
        let w = rect.width() as usize;
        let mut last: Option<i64> = None;
        for gx in x0..x1 {
            let rows: Vec<i64> = (z0..z1)
                .filter(|gz| {
                    let at = ((gz - z0) as usize * w + (gx - x0) as usize) * 4;
                    watery(&px[at..at + 4])
                })
                .collect();
            assert!(!rows.is_empty(), "ring {t} m: column {gx} has no river");
            // Each column's river touches the last's (8-connected): no gap, no jump.
            let (lo, hi) = (rows[0], rows[rows.len() - 1]);
            if let Some(prev) = last {
                assert!(
                    lo <= prev + 1 && hi >= prev - 1,
                    "ring {t} m: a gap at column {gx}"
                );
            }
            last = Some((lo + hi) / 2);
            // Never wider than the minimum width and the anti-aliasing allow.
            assert!(
                hi - lo <= 4,
                "ring {t} m: the river is {} texels wide at {gx}",
                hi - lo + 1
            );
        }
    }
}
