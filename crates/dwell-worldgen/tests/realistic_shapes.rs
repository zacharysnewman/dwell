//! The Realistic world's map shapes (WORK_QUEUE item 15, the map check of 2026-11-14 and the owner's
//! "our tectonic plates are too straight, our continents don't have natural curves inner and
//! outer, with noise variation"; "the volcano islands are the exact same size, perfectly round, and
//! evenly spaced in a line"). On seeds 1, 7, 42, 274005646 and 2741893632, both faces, on the
//! world map (100 m cells; the measures in `support/map_shapes.rs`):
//!
//! - no thin land strip: nothing the opening by a 500 m disc leaves (land under ~1 km wide) runs
//!   longer than [`STRIP_KM`] (an oceanic transform's ridge made an 11 km strip on seed 7);
//! - coasts not straight: at most [`COAST_STRAIGHT`] of the coast's 2 km windows hold a straight
//!   run (30 m RMS off a line);
//! - plate boundaries not straight: at most [`BOUNDARY_STRAIGHT`] of their 2 km windows and
//!   [`BOUNDARY_STRAIGHT_5`] of their 5 km windows (75 m) straight (8 to 14 % of the 5 km before);
//! - islands (land under 60 km²) varied, pooled over the seeds: their radii's coefficient of
//!   variation at least [`ISLAND_CV`], their nearest neighbours' distances' at least
//!   [`SPACING_CV`] (no even spacing), and their outlines' irregularity (outline² / 4π area over a
//!   rasterized disc's) on average at least [`IRREGULAR`] (no round islands).

#[path = "support/map_shapes.rs"]
mod map_shapes;

use dwell_world::both_faces;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plates::{PLATES, PlateLayout};
use dwell_worldgen::terrain::FIXTURE_WORLDS;
use dwell_worldgen::worldmap::WorldMap;
use map_shapes::{Mask, cv};

const SEEDS: [u64; 5] = [1, 7, 42, 274_005_646, 2_741_893_632];
const STRIP_KM: f64 = 5.0;
const COAST_STRAIGHT: f64 = 0.02;
const BOUNDARY_STRAIGHT: f64 = 0.04;
const BOUNDARY_STRAIGHT_5: f64 = 0.03;
const ISLAND_CV: f64 = 0.4;
const SPACING_CV: f64 = 0.3;
const IRREGULAR: f64 = 1.2;

struct Measured {
    strip: f64,
    coast: f64,
    boundary: f64,
    boundary5: f64,
    radii: Vec<f64>,
    spacing: Vec<f64>,
    irregularity: Vec<f64>,
}

fn measure(world: u64, face: dwell_world::Face) -> Measured {
    let h = FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:realistic")
        .expect("the Realistic world")
        .1;
    let seed = face_seed(world, face);
    let m = WorldMap::for_height(seed, h);
    let land = Mask {
        n: m.n,
        cell: m.cell,
        on: (0..m.height.len())
            .map(|c| !m.is_sea(c) && m.height[c].is_finite())
            .collect(),
    };
    let strip = land
        .components(&land.thin(500.0))
        .iter()
        .map(|c| land.extent(c))
        .fold(0.0, f64::max);
    let coast = land.straight_share(&land.outline(), 1000.0, 30.0).0;
    // The plate boundaries inside the rim's band, one cell wide.
    let layout = PlateLayout::generate(&PLATES, seed);
    let inside = m.world_radius() as f64 - PLATES.rim.edge_sea;
    let plate: Vec<usize> = (0..m.height.len())
        .map(|c| {
            let (x, z) = m.centre(c);
            layout.plate_at(x, z)
        })
        .collect();
    let edge: Vec<bool> = (0..plate.len())
        .map(|c| {
            let (x, z) = m.centre(c);
            x.hypot(z) < inside
                && ((c % m.n + 1 < m.n && plate[c + 1] != plate[c])
                    || (c + m.n < plate.len() && plate[c + m.n] != plate[c]))
        })
        .collect();
    let boundary = land.straight_share(&edge, 1000.0, 30.0).0;
    let boundary5 = land.straight_share(&edge, 2500.0, 75.0).0;
    let islands: Vec<_> = land
        .components(&land.on)
        .iter()
        .filter(|c| c.len() >= 12 && (c.len() as f64) * m.cell * m.cell < 60e6)
        .map(|c| land.shape(c))
        .collect();
    let spacing = islands
        .iter()
        .map(|a| {
            islands
                .iter()
                .filter(|b| !std::ptr::eq(a, *b))
                .map(|b| (a.x - b.x).hypot(a.z - b.z))
                .fold(f64::MAX, f64::min)
        })
        .filter(|d| *d < 20_000.0)
        .collect();
    Measured {
        strip,
        coast,
        boundary,
        boundary5,
        radii: islands.iter().map(|i| i.radius).collect(),
        spacing,
        irregularity: islands.iter().map(|i| i.irregularity).collect(),
    }
}

both_faces!(
    fn realistic_maps_have_natural_coasts_boundaries_and_islands(f: FaceFrame) {
        let (mut radii, mut spacing, mut irregular) = (Vec::new(), Vec::new(), Vec::new());
        let mut report = Vec::new();
        for world in SEEDS {
            let m = measure(world, f.face);
            report.push(format!(
                "seed {world}: strip {:.1} km, straight coast {:.2} %, boundary {:.2} % / {:.2} %",
                m.strip / 1000.0,
                100.0 * m.coast,
                100.0 * m.boundary,
                100.0 * m.boundary5
            ));
            assert!(
                m.strip <= STRIP_KM * 1000.0,
                "seed {world}: a land strip under 1 km wide runs {:.1} km",
                m.strip / 1000.0
            );
            assert!(
                m.coast <= COAST_STRAIGHT,
                "seed {world}: {}",
                report.last().unwrap()
            );
            assert!(
                m.boundary <= BOUNDARY_STRAIGHT && m.boundary5 <= BOUNDARY_STRAIGHT_5,
                "seed {world}: {}",
                report.last().unwrap()
            );
            radii.extend(m.radii);
            spacing.extend(m.spacing);
            irregular.extend(m.irregularity);
        }
        let mean = irregular.iter().sum::<f64>() / irregular.len().max(1) as f64;
        let line = format!(
            "{} islands: radius cv {:.2}, spacing cv {:.2} ({}), irregularity {:.2}",
            radii.len(),
            cv(&radii),
            cv(&spacing),
            spacing.len(),
            mean
        );
        eprintln!("{}\n{line}", report.join("\n"));
        assert!(radii.len() >= 5, "{line}");
        assert!(cv(&radii) >= ISLAND_CV, "{line}");
        assert!(spacing.len() >= 4 && cv(&spacing) >= SPACING_CV, "{line}");
        assert!(mean >= IRREGULAR, "{line}");
    }
);
