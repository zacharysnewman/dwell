//! What the generated hillsides are made of on the half-height grid (HALF_HEIGHT_GRID.md §4):
//! the share of flat tops, ½ m ramps, 45° ramps and vertical faces by the ground's slope, the
//! 1 m cliffs, the terraces and how far the drawn ground sits from the continuous height field.
//! Owner, 2026-10-20: option A of the slopes investigation (no 1 m step snap, the Sobel facing).

use dwell_world::ramps::RampFacing;
use dwell_world::{CELL_H, SEA_LEVEL_M, rows_for_m};
use dwell_worldgen::TerrainGenerator;
use dwell_worldgen::ramps::{Around, Top, top_of};

/// Window side (columns) and margin (the rule reads one column round, the edges one more).
const W: i32 = 48;
const MARGIN: i32 = 2;

struct Window {
    n: i32,
    /// The continuous height (m) and the column's rows, margin included.
    h: Vec<f32>,
    rows: Vec<i32>,
}

impl Window {
    fn new(g: &TerrainGenerator, x0: i32, z0: i32) -> Self {
        let n = W + 2 * MARGIN;
        let (mut h, mut rows) = (vec![], vec![]);
        for j in -MARGIN..W + MARGIN {
            for i in -MARGIN..W + MARGIN {
                h.push(g.column_at(x0 + i, z0 + j).height);
                rows.push(g.ground_y(x0 + i, z0 + j).expect("inside") + 1);
            }
        }
        Self { n, h, rows }
    }

    fn idx(&self, i: i32, j: i32) -> usize {
        ((j + MARGIN) * self.n + i + MARGIN) as usize
    }

    fn h(&self, i: i32, j: i32) -> f32 {
        self.h[self.idx(i, j)]
    }

    /// The column's rows and its top by the generation rule.
    fn top(&self, i: i32, j: i32) -> (i32, Top) {
        let around: Around = std::array::from_fn(|r| {
            std::array::from_fn(|c| self.rows[self.idx(i + c as i32 - 1, j + r as i32 - 1)])
        });
        (around[1][1], top_of(&around, rows_for_m(1.0)))
    }

    /// The continuous gradient (m per m).
    fn gradient(&self, i: i32, j: i32) -> (f32, f32) {
        (
            (self.h(i + 1, j) - self.h(i - 1, j)) * 0.5,
            (self.h(i, j + 1) - self.h(i, j - 1)) * 0.5,
        )
    }
}

/// The drawn surface's height (m) at (fx, fz) in [0, 1]² over a column.
fn surface((rows, top): (i32, Top), fx: f32, fz: f32) -> f32 {
    let t = rows as f32 * CELL_H as f32;
    let (facing, drop) = match top {
        Top::Flat => return t,
        Top::Gentle(f) => (f, CELL_H as f32),
        Top::Steep(f) => (f, 2.0 * CELL_H as f32),
    };
    let along = match facing {
        RampFacing::East => fx,
        RampFacing::West => 1.0 - fx,
        RampFacing::South => fz,
        RampFacing::North => 1.0 - fz,
    };
    t - drop * along
}

/// The vertical face between a column and its neighbour east (`east`) or south: its area (m²,
/// sampled at 9 points along the edge) and its tallest gap (m).
fn face(me: (i32, Top), nb: (i32, Top), east: bool) -> (f32, f32) {
    let (mut area, mut tallest) = (0.0f32, 0.0f32);
    for k in 0..9 {
        let t = (k as f32 + 0.5) / 9.0;
        let gap = if east {
            surface(me, 1.0, t) - surface(nb, 0.0, t)
        } else {
            surface(me, t, 1.0) - surface(nb, t, 0.0)
        }
        .abs();
        area += gap / 9.0;
        tallest = tallest.max(gap);
    }
    (area, tallest)
}

#[derive(Default, Debug)]
struct Tally {
    cols: f64,
    /// Edges with a face over 3/4 m (a 1 m cliff).
    cliffs: f64,
    /// On 10–25° ground: flat, ½ m ramp, 45° ramp and vertical area (m²).
    moderate: [f64; 4],
    /// Flat tops on ground of at least 10°, and those whose downhill edge is a 1 m face.
    flat_on_slopes: f64,
    terraces: f64,
    /// Sum of squares of the drawn centre minus the continuous height.
    err2: f64,
}

impl Tally {
    fn add(&mut self, w: &Window) {
        for j in 0..W {
            for i in 0..W {
                if w.h(i, j) < SEA_LEVEL_M as f32 + 2.0 {
                    continue;
                }
                self.cols += 1.0;
                let me = w.top(i, j);
                let (gx, gz) = w.gradient(i, j);
                let degrees = gx.hypot(gz).atan().to_degrees();
                let class = match me.1 {
                    Top::Flat => 0,
                    Top::Gentle(_) => 1,
                    Top::Steep(_) => 2,
                };
                let mut vertical = 0.0;
                for (di, dj) in [(1, 0), (0, 1)] {
                    let (area, tallest) = face(me, w.top(i + di, j + dj), di == 1);
                    vertical += area;
                    self.cliffs += f64::from(u8::from(tallest > 0.75));
                }
                if (10.0..25.0).contains(&degrees) {
                    // A ramp's top is longer than the column is wide.
                    self.moderate[class] += [1.0, 1.25f64.sqrt(), 2f64.sqrt()][class];
                    self.moderate[3] += f64::from(vertical);
                }
                if degrees >= 10.0 && class == 0 {
                    // The downhill neighbour along the fall line's axis.
                    let (di, dj) = if gx.abs() >= gz.abs() {
                        (-gx.signum() as i32, 0)
                    } else {
                        (0, -gz.signum() as i32)
                    };
                    let nb = w.top(i + di, j + dj);
                    let tallest = if di + dj > 0 {
                        face(me, nb, di == 1).1
                    } else {
                        face(nb, me, di == -1).1
                    };
                    self.flat_on_slopes += 1.0;
                    self.terraces += f64::from(u8::from(tallest > 0.75));
                }
                let e = surface(me, 0.5, 0.5) - w.h(i, j);
                self.err2 += f64::from(e * e);
            }
        }
    }
}

#[test]
fn hillsides_are_v2_steps_and_ramps() {
    // Owner, 2026-10-20: of the slopes 2×2 (HALF_HEIGHT_GRID.md §4) the owner chose V2, the old
    // ramp rule (both sides and both back diagonals not lower) with the 1 m step snap off, over
    // option A (the Sobel facing). V2 measured 24.5 cliff edges ≥ 1 m per 100 land columns
    // (option A 1.0, generator 1018 36.8), 10–25° ground 47 % flat, 31 % ½ m ramp and 23 %
    // vertical (A: 33 / 56 / 11 %), 19.8 % of flat runs on ≥ 10° ending in a 1 m face (A 0 %) and
    // the drawn ground 0.25 m RMS from the continuous field (A 0.15 m). Windows of 48 × 48 land
    // columns on a 1.5 km grid, inland, over five seeds. Generator 21 (ADR 0049, the world map's
    // terrain with ~2,400 m ranges): 43.5 cliff edges per 100, 48 % flat, 26 % ½ m ramp, 26 %
    // vertical, 39.4 % terraces, 0.27 m RMS: the same rule, the bands moved to the new terrain.
    let mut t = Tally::default();
    for seed in [1u64, 2, 3, 7, 42] {
        let g = TerrainGenerator::new(seed);
        let mut windows = 0;
        'grid: for gz in -12..=12 {
            for gx in -12..=12 {
                if windows == 6 {
                    break 'grid;
                }
                let (x, z) = (gx * 1500, gz * 1500);
                let c = g.column_at(x, z);
                if c.outside || g.coast_at(x, z) < 3000.0 || c.height < SEA_LEVEL_M as f32 + 4.0 {
                    continue;
                }
                let sx = (g.column_at(x + 8, z).height - c.height) / 8.0;
                let sz = (g.column_at(x, z + 8).height - c.height) / 8.0;
                if sx.hypot(sz) > 0.2 {
                    t.add(&Window::new(&g, x - W / 2, z - W / 2));
                    windows += 1;
                }
            }
        }
        assert_eq!(windows, 6, "seed {seed}");
    }
    let per_100 = 100.0 * t.cliffs / t.cols;
    let m: f64 = t.moderate.iter().sum();
    let [flat, gentle, steep, vertical] = t.moderate.map(|a| 100.0 * a / m);
    let terraces = 100.0 * t.terraces / t.flat_on_slopes;
    let rms = (t.err2 / t.cols).sqrt();
    let at = format!(
        "{:.0} land columns: cliff edges per 100 {per_100:.1}; 10–25°: flat {flat:.0} %, ½ m ramp \
         {gentle:.0} %, 45° {steep:.0} %, vertical {vertical:.0} %; flat tops on ≥ 10° ending in a \
         1 m face {terraces:.1} %; RMS {rms:.3} m",
        t.cols
    );
    println!("{at}");
    assert!(t.cols > 50_000.0, "{at}");
    assert!((38.0..50.0).contains(&per_100), "{at}");
    assert!((40.0..55.0).contains(&flat), "{at}");
    assert!((20.0..32.0).contains(&gentle), "{at}");
    assert!((18.0..28.0).contains(&vertical), "{at}");
    assert!((33.0..46.0).contains(&terraces), "{at}");
    assert!((0.2..0.3).contains(&rms), "{at}");
}
