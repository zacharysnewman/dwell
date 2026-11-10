//! Tectonics at realistic scale (systemic terrain T5, `shared/worldgen/terrain.json` `tectonics`):
//! neutral by default; a fixture convergent boundary (by data alone) raises a range along its line
//! with a volcanic arc of cratered cones behind it on volcanic rock, and a fixture hotspot chain
//! rises as islands from the sea, lower down the chain. On the world map, which the chunks follow.

use std::sync::Arc;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::tectonics::{CONVERGENT, Tectonics};
use dwell_worldgen::terrain::{HEIGHT, HeightField, TerrainGenerator};
use dwell_worldgen::worldmap::WorldMap;

/// A fixture boundary and chain by data alone: a convergent line across the island, three arc
/// volcanoes behind it, a four-island hotspot chain offshore.
fn fixture() -> Tectonics {
    Tectonics {
        kind: CONVERGENT,
        angle: 30.0,
        offset: -6000.0,
        width: 3500.0,
        range_height: 260.0,
        trench_depth: 40.0,
        arc_count: 3,
        arc_distance: 7000.0,
        arc_spacing: 9000.0,
        cone_height: 700.0,
        cone_radius: 3500.0,
        crater_radius: 400.0,
        crater_depth: 80.0,
        hotspot_count: 4,
        hotspot_x: 31000.0,
        hotspot_z: -9000.0,
        hotspot_heading: 60.0,
        hotspot_spacing: 5500.0,
        hotspot_height: 380.0,
        hotspot_radius: 2600.0,
        hotspot_decay: 0.6,
        hotspot_widen: 0.12,
        ..HEIGHT.tectonics
    }
}

fn map(f: FaceFrame, t: Tectonics) -> WorldMap {
    let h = HeightField {
        tectonics: t,
        ..HEIGHT
    };
    WorldMap::for_height(face_seed(7, f.face), h)
}

fn ground(m: &WorldMap, x: f64, z: f64) -> f64 {
    m.height_at(x, z).0
}

#[test]
fn the_default_tectonics_are_off() {
    // face-a-only: the build's data, the same for both faces.
    assert!(
        !HEIGHT.tectonics.enabled(),
        "the default world has no plates"
    );
    assert!(HEIGHT.tectonics.volcanoes().is_empty() || HEIGHT.tectonics.kind == 0);
}

dwell_world::both_faces!(
    /// With no plates the world map is the plain one (the goldens pin the chunks).
    fn no_plates_leave_the_map_unchanged(f: FaceFrame) {
        let off = Tectonics {
            kind: 0,
            hotspot_count: 0,
            ..fixture()
        };
        let a = map(f, off);
        let b = WorldMap::compute(face_seed(7, f.face));
        assert_eq!(a.height, b.height);
    }
);

dwell_world::both_faces!(
    /// A convergent boundary raises a ridge along its line, cones with craters along the arc on
    /// volcanic rock, the old core's granite far behind, and a hotspot chain of islands above the
    /// sea, lower down the chain.
    fn a_convergent_boundary_and_a_hotspot_chain_shape_the_island(f: FaceFrame) {
        let t = fixture();
        let m = map(f, t);
        let plain = WorldMap::compute(face_seed(7, f.face));
        // The ridge: along the line at the range's crest (one width behind it), the ground stands
        // well above the plain map's, and above the trench side's.
        let a = t.angle.to_radians();
        let at = |along: f64, across: f64| {
            let across = across + t.offset;
            (
                along * a.cos() - across * a.sin(),
                along * a.sin() + across * a.cos(),
            )
        };
        let (mut lift, mut over, mut n) = (0.0, 0.0, 0);
        for k in -8..=8 {
            let along = f64::from(k) * 1000.0;
            let (x, z) = at(along, t.width);
            if ground(&plain, x, z) <= 5.0 {
                continue;
            }
            let (tx, tz) = at(along, -1.5 * t.width);
            lift += ground(&m, x, z) - ground(&plain, x, z);
            over += ground(&m, x, z) - ground(&m, tx, tz);
            n += 1;
        }
        assert!(n >= 8, "the line crosses the island: {n} land samples");
        let (lift, over) = (lift / f64::from(n), over / f64::from(n));
        assert!(lift > 0.5 * t.range_height, "the range rises {lift:.0} m");
        assert!(
            over > 0.5 * t.range_height,
            "above its surroundings by {over:.0} m"
        );

        let volcanoes = t.volcanoes();
        let arc: Vec<_> = volcanoes.iter().filter(|v| v.chain == 0).collect();
        assert_eq!(arc.len(), 3);
        let g = TerrainGenerator::with_map(face_seed(7, f.face), Arc::new(map(f, t)));
        for v in &arc {
            let ring = |r: f64| {
                (0..12)
                    .map(|k| {
                        let b = f64::from(k) * std::f64::consts::PI / 6.0;
                        ground(&m, v.x + r * b.cos(), v.z + r * b.sin())
                    })
                    .sum::<f64>()
                    / 12.0
            };
            let (centre, rim, flank) = (
                ground(&m, v.x, v.z),
                ring(v.crater_radius * 1.25),
                ring(0.6 * v.radius),
            );
            let around = ring(1.5 * v.radius);
            assert!(rim > flank + 50.0, "a cone: rim {rim:.0}, flank {flank:.0}");
            assert!(
                rim > around + 100.0,
                "a local high: rim {rim:.0}, around {around:.0}"
            );
            assert!(
                centre < rim - 15.0,
                "a crater: centre {centre:.0}, rim {rim:.0}"
            );
            assert_eq!(g.province_at(v.x as i64, v.z as i64), "dwell:basalt_flows");
        }
        let (cx, cz) = at(0.0, t.core_distance + 4000.0);
        assert_eq!(g.province_at(cx as i64, cz as i64), "dwell:granite_core");

        let chain: Vec<_> = volcanoes.iter().filter(|v| v.chain > 0).collect();
        let peaks: Vec<f64> = chain
            .iter()
            .map(|v| {
                let mut best = f64::NEG_INFINITY;
                for i in -4..=4 {
                    for j in -4..=4 {
                        let (dx, dz) = (f64::from(i) * 150.0, f64::from(j) * 150.0);
                        best = best.max(ground(&m, v.x + dx, v.z + dz));
                    }
                }
                best
            })
            .collect();
        for (k, p) in peaks.iter().enumerate() {
            assert!(*p > 20.0, "island {k} stands above the sea: {p:.0} m");
            let (x, z) = (chain[k].x, chain[k].z);
            assert!(ground(&plain, x, z) <= 0.0, "island {k} is offshore");
        }
        for w in peaks.windows(2) {
            assert!(w[1] < w[0], "lower down the chain: {peaks:?}");
        }
    }
);
