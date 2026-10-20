//! The terrain's relief over seeds and both faces (the elevation check of 2026-10-19):
//! `cargo run --release -p dwell-worldgen --example relief -- [seeds…]` samples land columns on a
//! 397 m grid over the island and prints, per face, the land's height above sea level (min, mean,
//! standard deviation, 99th percentile, max), the mean ground slope (degrees, from the height
//! field over 1 m), the share of the shaped surface steeper than the slide's 44° (the adaptive top
//! blocks, sampled at four points each), the share of columns too steep for tall grass (a top
//! more than one block from a neighbour's) and the share of the island (inside its coast) under
//! water.

use dwell_blocks::{adaptive, shape};
use dwell_world::{Face, SEA_LEVEL};
use dwell_worldgen::{BifacialTerrain, TerrainGenerator};

const HALF: i32 = 30_000;
const STEP: usize = 397;
/// tan 44°: the slide's threshold (`slide.min_angle`).
const SLIDE_TAN: f32 = 0.965_689;

#[derive(Default)]
struct Stats {
    heights: Vec<f32>,
    slope_deg: f64,
    slide_samples: u64,
    slides: u64,
    grass_steep: u64,
    island: u64,
    flooded: u64,
}

fn sample(g: &TerrainGenerator, x: i32, z: i32, s: &mut Stats) {
    let col = g.column_at(x, z);
    let h = col.height - SEA_LEVEL as f32;
    if col.outside {
        return;
    }
    if col.coast > 0.0 {
        s.island += 1;
        s.flooded += u64::from(h <= 0.0);
    }
    if h <= 0.0 {
        return;
    }
    s.heights.push(h);
    let dx = g.column_at(x + 1, z).height - g.column_at(x - 1, z).height;
    let dz = g.column_at(x, z + 1).height - g.column_at(x, z - 1).height;
    s.slope_deg += f64::from((dx * 0.5).hypot(dz * 0.5)).atan().to_degrees();
    let top = g.ground_y(x, z).expect("inside");
    let mut get = |ox: i32, oy: i32, oz: i32| g.state_at(x + ox, top + oy, z + oz);
    let shaped = adaptive::shape_at(&mut get, 1);
    let hi = |fx: f32, fz: f32| shape::solid_span_at(shaped, fx, fz).hi;
    for (fx, fz) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
        let gx = (hi(fx + 0.05, fz) - hi(fx - 0.05, fz)) / 0.1;
        let gz = (hi(fx, fz + 0.05) - hi(fx, fz - 0.05)) / 0.1;
        s.slide_samples += 1;
        s.slides += u64::from(gx.hypot(gz) > SLIDE_TAN);
    }
    let steep = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .any(|&(ox, oz)| (g.ground_y(x + ox, z + oz).expect("inside") - top).abs() > 1);
    s.grass_steep += u64::from(steep);
}

fn main() {
    let mut seeds: Vec<u64> = std::env::args()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if seeds.is_empty() {
        seeds = vec![0, 1, 2, 3, 42, 12345, 20_260_925];
    }
    println!(
        "face columns min_m mean_m stddev_m p99_m max_m mean_slope_deg slide_surface_% grass_too_steep_% island_under_water_%"
    );
    for face in [Face::A, Face::B] {
        let mut s = Stats::default();
        for &seed in &seeds {
            let terrain = BifacialTerrain::new(seed);
            let g = terrain.local(face);
            for z in (-HALF..=HALF).step_by(STEP) {
                for x in (-HALF..=HALF).step_by(STEP) {
                    sample(g, x, z, &mut s);
                }
            }
        }
        let n = s.heights.len() as f64;
        let mean = s.heights.iter().map(|&h| f64::from(h)).sum::<f64>() / n;
        let var = s
            .heights
            .iter()
            .map(|&h| (f64::from(h) - mean).powi(2))
            .sum::<f64>()
            / n;
        s.heights.sort_by(f32::total_cmp);
        let p99 = s.heights[((n * 0.99) as usize).min(s.heights.len() - 1)];
        println!(
            "{face:?} {} {:.1} {mean:.1} {:.1} {p99:.1} {:.1} {:.2} {:.2} {:.2} {:.2}",
            s.heights.len(),
            s.heights[0],
            var.sqrt(),
            s.heights[s.heights.len() - 1],
            s.slope_deg / n,
            100.0 * s.slides as f64 / s.slide_samples as f64,
            100.0 * s.grass_steep as f64 / n,
            100.0 * s.flooded as f64 / s.island as f64,
        );
    }
}
