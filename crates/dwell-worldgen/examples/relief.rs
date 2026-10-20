//! The terrain's relief over seeds and both faces (the elevation check of 2026-10-19):
//! `cargo run --release -p dwell-worldgen --example relief -- [seeds…]` samples land columns on a
//! 397 m grid over the island and prints, per face, the land's height above sea level (min, mean,
//! standard deviation, 99th percentile, max), the mean ground slope (degrees, from the height
//! field over 1 m), the share of the shaped surface steeper than the slide's 44° (the adaptive top
//! blocks, sampled at four points each), the share of columns too steep for tall grass (a top
//! more than one block from a neighbour's) and the share of the island (inside its coast) under
//! water. Then, per face over the first three seeds, how connected the walkable ground is: of the
//! dry land in a 1,025 m square around the spawn, the share reachable from the spawn without climbing
//! a slide face or a cliff (a move may go down any drop or slide, and up one block only between two
//! blocks that don't slide; water is swum at its surface), and the share from which the spawn can be
//! reached again too (both ways); and whether the spawn's block slides.

use std::collections::VecDeque;

use dwell_blocks::{ShapeInfo, adaptive, shape};
use dwell_world::{Face, M, SEA_LEVEL};
use dwell_worldgen::{BifacialTerrain, TerrainGenerator};

const HALF: i32 = 30_000;
const STEP: usize = 397;
/// tan 44°: the slide's threshold (`slide.min_angle`).
const SLIDE_TAN: f32 = 0.965_689;

/// The walkability square reaches this far either side of the spawn (m).
const WALK_HALF: i32 = 512;

/// Whether a shaped top block has a face steeper than the slide's threshold (four sample points).
fn slides(shaped: &ShapeInfo) -> [bool; 4] {
    let hi = |fx: f32, fz: f32| shape::solid_span_at(shaped, fx, fz).hi;
    [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)].map(|(fx, fz)| {
        let gx = (hi(fx + 0.05, fz) - hi(fx - 0.05, fz)) / 0.1;
        let gz = (hi(fx, fz + 0.05) - hi(fx, fz - 0.05)) / 0.1;
        gx.hypot(gz) > SLIDE_TAN
    })
}

/// (reachable from the spawn, reachable both ways, the spawn's block slides) for one generator.
fn walk(g: &TerrainGenerator) -> (f64, f64, bool) {
    let sp = g.spawn_point();
    let (sx, sz) = (sp[0].floor() as i32, sp[2].floor() as i32);
    let n = (2 * WALK_HALF + 1) as usize;
    let p = n + 2;
    let mut top = vec![i32::MIN / 2; p * p];
    for j in 0..p {
        for i in 0..p {
            let (x, z) = (sx - WALK_HALF - 1 + i as i32, sz - WALK_HALF - 1 + j as i32);
            if let Some(t) = g.ground_y(x, z) {
                top[j * p + i] = t;
            }
        }
    }
    // Per cell of the square: the height walked or swum at, and whether it slides.
    let mut height = vec![0i32; n * n];
    let mut slide = vec![false; n * n];
    let mut land = vec![false; n * n];
    for j in 0..n {
        for i in 0..n {
            let t = top[(j + 1) * p + i + 1];
            let k = j * n + i;
            land[k] = t >= SEA_LEVEL;
            height[k] = t.max(SEA_LEVEL - 1);
            if !land[k] {
                continue;
            }
            let get = |ox: i32, oy: i32, oz: i32| {
                let c = top[(j as i32 + 1 + oz) as usize * p + (i as i32 + 1 + ox) as usize];
                let y = t + oy;
                if y <= c {
                    M.stone
                } else if y < SEA_LEVEL {
                    M.water
                } else {
                    M.air
                }
            };
            slide[k] = slides(adaptive::shape_at(get, 1)).iter().any(|&s| s);
        }
    }
    let can = |a: usize, b: usize| {
        let (ha, hb) = (height[a], height[b]);
        hb <= ha || (hb == ha + 1 && !slide[a] && !slide[b])
    };
    let start = WALK_HALF as usize * n + WALK_HALF as usize;
    let search = |forward: bool| {
        let mut seen = vec![false; n * n];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(a) = queue.pop_front() {
            let (i, j) = (a % n, a / n);
            let near = [
                (i > 0).then(|| a - 1),
                (i + 1 < n).then(|| a + 1),
                (j > 0).then(|| a - n),
                (j + 1 < n).then(|| a + n),
            ];
            for b in near.into_iter().flatten() {
                if !seen[b] && (if forward { can(a, b) } else { can(b, a) }) {
                    seen[b] = true;
                    queue.push_back(b);
                }
            }
        }
        seen
    };
    let (there, back) = (search(true), search(false));
    let dry = land.iter().filter(|&&l| l).count() as f64;
    let reach = (0..n * n).filter(|&k| land[k] && there[k]).count() as f64;
    let both = (0..n * n)
        .filter(|&k| land[k] && there[k] && back[k])
        .count() as f64;
    (reach / dry, both / dry, slide[start])
}

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
    for slide in slides(adaptive::shape_at(&mut get, 1)) {
        s.slide_samples += 1;
        s.slides += u64::from(slide);
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
    println!("face seed reachable_% both_ways_% spawn_slides");
    for face in [Face::A, Face::B] {
        for &seed in seeds.iter().take(3) {
            let terrain = BifacialTerrain::new(seed);
            let (reach, both, spawn_slides) = walk(terrain.local(face));
            println!(
                "{face:?} {seed} {:.1} {:.1} {spawn_slides}",
                100.0 * reach,
                100.0 * both
            );
        }
    }
}
