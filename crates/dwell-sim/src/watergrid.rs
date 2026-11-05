//! A prototype (docs/proposals/WATER_MODEL_EVALUATION.md, candidate C): water as a depth on each
//! ground column, moving between neighbours across their shared faces. Not wired into the game: it
//! exists to measure cost and stability and to be judged in the Water Grid Lab (`docs/prototypes/`).
//!
//! The local-inertial shallow-water form (Bates, Horritt & Fewtrell 2010, LISFLOOD-FP): each face
//! carries a discharge per metre of width `q` (m²/s), accelerated by the slope of the water surface
//! across it and slowed by bed friction (Manning), through the depth of water that stands over the
//! higher of its two beds. So flow needs depth: water over a dam's top stands as deep as it must to
//! pass what comes, rather than a film carrying a river. A column never sends more than it holds;
//! water is conserved; springs hold their column at a depth (an infinite source), sinks take all
//! that reaches them.

/// The model's constants.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Gravity (m/s²).
    pub g: f32,
    /// Column width (m).
    pub cell: f32,
    /// Manning's roughness (s/m^⅓): 0.03 is a natural channel.
    pub manning: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            g: 9.81,
            cell: 1.0,
            manning: 0.03,
        }
    }
}

/// A grid of columns: ground height (m), water depth (m), and the discharge per metre of width
/// across each column's +x and +z faces (m²/s, positive toward +x / +z).
#[derive(Clone, Debug)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub ground: Vec<f32>,
    pub depth: Vec<f32>,
    pub qx: Vec<f32>,
    pub qz: Vec<f32>,
    /// Springs (column, the depth it is held at: an infinite source) and sinks (columns whose
    /// water is taken).
    pub sources: Vec<(usize, f32)>,
    pub sinks: Vec<usize>,
    pub params: Params,
    /// m³ brought in and taken out so far.
    pub brought: f64,
    pub taken: f64,
    /// Scratch, kept between steps.
    next_x: Vec<f32>,
    next_z: Vec<f32>,
    scale: Vec<f32>,
}

/// ∛x for x > 0 to about 1e-4 relative: a bit estimate and two Newton steps (libm's cbrt was
/// most of a step's time; friction needs no more).
fn fast_cbrt(x: f32) -> f32 {
    let mut y = f32::from_bits(x.to_bits() / 3 + 709_921_077);
    y = (2.0 * y + x / (y * y)) / 3.0;
    (2.0 * y + x / (y * y)) / 3.0
}

/// The least depth over a face that carries flow (m).
const DRY: f32 = 1e-4;

impl Grid {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            ground: vec![0.0; w * h],
            depth: vec![0.0; w * h],
            qx: vec![0.0; w * h],
            qz: vec![0.0; w * h],
            sources: Vec::new(),
            sinks: Vec::new(),
            params: Params::default(),
            brought: 0.0,
            taken: 0.0,
            next_x: vec![0.0; w * h],
            next_z: vec![0.0; w * h],
            scale: vec![1.0; w * h],
        }
    }

    pub fn at(&self, x: usize, z: usize) -> usize {
        z * self.w + x
    }

    /// The water stored (m³).
    pub fn stored(&self) -> f64 {
        let a = f64::from(self.params.cell * self.params.cell);
        self.depth.iter().map(|&d| f64::from(d) * a).sum()
    }

    /// The water surface of column `i` (m).
    pub fn surface(&self, i: usize) -> f32 {
        self.ground[i] + self.depth[i]
    }

    /// The longest stable step now (s): a CFL limit on the deepest water.
    pub fn max_step(&self) -> f32 {
        let deepest = self.depth.iter().copied().fold(0.01, f32::max);
        0.7 * self.params.cell / (self.params.g * deepest).sqrt()
    }

    /// The new discharge across the face from `a` to `b` (m²/s), from `q` last step.
    fn face(&self, a: usize, b: usize, q: f32, dt: f32) -> f32 {
        let p = self.params;
        let (ea, eb) = (self.surface(a), self.surface(b));
        let bed = self.ground[a].max(self.ground[b]);
        let h = ea.max(eb) - bed;
        if h <= DRY {
            return 0.0;
        }
        let slope = (eb - ea) / p.cell;
        // Friction: h^(7/3) as h²·∛h (powf is the step's dearest call).
        let friction = if q == 0.0 {
            1.0
        } else {
            1.0 + p.g * dt * p.manning * p.manning * q.abs() / (h * h * fast_cbrt(h))
        };
        let q = (q - p.g * h * dt * slope) / friction;
        // No faster than critical flow over the face (a weir passes no more).
        let limit = h * (p.g * h).sqrt();
        q.clamp(-limit, limit)
    }

    /// One step of `dt` seconds (at most `max_step()`).
    pub fn step(&mut self, dt: f32) {
        let w = self.w;
        let area = self.params.cell * self.params.cell;
        let width = self.params.cell;
        for k in 0..self.sources.len() {
            let (i, held) = self.sources[k];
            if self.depth[i] < held {
                self.brought += f64::from((held - self.depth[i]) * area);
                self.depth[i] = held;
            }
        }
        let mut qx = std::mem::take(&mut self.next_x);
        let mut qz = std::mem::take(&mut self.next_z);
        let mut scale = std::mem::take(&mut self.scale);
        // Rows then columns: x and z from the loops, not from a division per cell.
        let h = self.h;
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                qx[i] = if x + 1 < w && (self.depth[i] > 0.0 || self.depth[i + 1] > 0.0) {
                    self.face(i, i + 1, self.qx[i], dt)
                } else {
                    0.0
                };
                qz[i] = if z + 1 < h && (self.depth[i] > 0.0 || self.depth[i + w] > 0.0) {
                    self.face(i, i + w, self.qz[i], dt)
                } else {
                    0.0
                };
            }
        }
        // No column sends more than it holds: scale what leaves it.
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                let mut out = qx[i].max(0.0) + qz[i].max(0.0);
                if x > 0 {
                    out += (-qx[i - 1]).max(0.0);
                }
                if z > 0 {
                    out += (-qz[i - w]).max(0.0);
                }
                let out = out * width * dt;
                let have = self.depth[i] * area;
                scale[i] = if out > have { have / out } else { 1.0 };
            }
        }
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                if qx[i] > 0.0 {
                    qx[i] *= scale[i];
                } else if x + 1 < w {
                    qx[i] *= scale[i + 1];
                }
                if qz[i] > 0.0 {
                    qz[i] *= scale[i];
                } else if z + 1 < h {
                    qz[i] *= scale[i + w];
                }
            }
        }
        let k = width * dt / area;
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                let mut net = -qx[i] - qz[i];
                if x > 0 {
                    net += qx[i - 1];
                }
                if z > 0 {
                    net += qz[i - w];
                }
                self.depth[i] = (self.depth[i] + net * k).max(0.0);
            }
        }
        self.next_x = std::mem::replace(&mut self.qx, qx);
        self.next_z = std::mem::replace(&mut self.qz, qz);
        self.scale = scale;
        for k in 0..self.sinks.len() {
            let i = self.sinks[k];
            self.taken += f64::from(self.depth[i] * area);
            self.depth[i] = 0.0;
        }
    }

    /// The flow speed in column `i` (m/s): the mean discharge of its faces over its depth.
    pub fn speed(&self, i: usize) -> f32 {
        let d = self.depth[i].max(0.05);
        let left = if !i.is_multiple_of(self.w) {
            self.qx[i - 1]
        } else {
            0.0
        };
        let back = if i >= self.w {
            self.qz[i - self.w]
        } else {
            0.0
        };
        let vx = 0.5 * (self.qx[i] + left);
        let vz = 0.5 * (self.qz[i] + back);
        (vx * vx + vz * vz).sqrt() / d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valley along x (w × h), its floor falling `slope` m per m, a channel 3 columns wide in the
    /// middle 0.5 m below banks; springs held `q` m deep at its head, a sink at its foot.
    fn valley(w: usize, h: usize, slope: f32, q: f32) -> Grid {
        let mut g = Grid::new(w, h);
        let mid = h / 2;
        for z in 0..h {
            for x in 0..w {
                let fall = slope * x as f32;
                let off = (z as isize - mid as isize).unsigned_abs() as f32;
                let i = g.at(x, z);
                g.ground[i] = 10.0 - fall + if off <= 1.0 { 0.0 } else { 0.5 + 0.25 * off };
            }
        }
        // Springs across the channel's head, held `q` m deep.
        for z in mid - 1..=mid + 1 {
            let i = g.at(0, z);
            g.sources.push((i, q));
        }
        for z in 0..h {
            let i = g.at(w - 1, z);
            g.sinks.push(i);
        }
        g
    }

    #[test]
    fn fast_cbrt_is_close() {
        for x in [1e-4f32, 0.01, 0.3, 1.0, 2.5, 40.0] {
            let rel = (fast_cbrt(x) - x.cbrt()).abs() / x.cbrt();
            assert!(rel < 1e-3, "∛{x}: {} vs {}", fast_cbrt(x), x.cbrt());
        }
    }

    #[test]
    fn water_is_conserved() {
        let mut g = valley(64, 16, 0.02, 0.3);
        for _ in 0..2000 {
            g.step(0.05);
        }
        let balance = g.stored() + g.taken - g.brought;
        assert!(
            balance.abs() < 1e-3 * g.brought.max(1.0),
            "unbalanced by {balance} m³"
        );
    }

    #[test]
    fn a_river_runs_downhill_to_its_foot_and_settles() {
        let mut g = valley(64, 16, 0.02, 0.3);
        for _ in 0..20_000 {
            g.step(0.05);
        }
        // Settled: what reaches the foot each second is what the springs bring.
        let (brought, taken) = (g.brought, g.taken);
        for _ in 0..400 {
            g.step(0.05);
        }
        let (inn, out) = ((g.brought - brought) / 20.0, (g.taken - taken) / 20.0);
        assert!(inn > 0.05, "flowing: {inn} m³/s");
        assert!(
            (out - inn).abs() < 0.05 * inn,
            "reaches its foot at {out} m³/s of {inn}"
        );
        // In its channel, not over its banks.
        let bank = g.at(32, 8 + 3);
        assert!(g.depth[bank] < 0.01, "banks dry: {}", g.depth[bank]);
    }

    #[test]
    fn a_dam_across_the_channel_backs_the_river_up_and_pours_over_its_top() {
        // A dam across the channel at x = 32, 0.75 m over the bed (the banks beside it are 1 m):
        // the river rises behind it, stands over its top deep enough to see, and runs on below.
        let mut g = valley(64, 16, 0.02, 0.6);
        for z in 0..16 {
            let i = g.at(32, z);
            let crest = 10.0 - 0.02 * 32.0 + 0.75;
            g.ground[i] = g.ground[i].max(crest);
        }
        for _ in 0..40_000 {
            g.step(0.05);
        }
        let (up, dam) = (g.at(31, 8), g.at(32, 8));
        assert!(
            g.surface(up) > g.ground[dam] + 0.03,
            "risen over the dam: {} vs {}",
            g.surface(up),
            g.ground[dam]
        );
        assert!(
            g.depth[dam] > 0.02,
            "over the dam's top: {} m",
            g.depth[dam]
        );
        assert!(g.depth[g.at(34, 8)] > 0.0, "flowing on below the dam");
    }

    #[test]
    fn flow_over_a_crest_needs_depth() {
        // The water over a crest passing a steady flow stands at least critical depth, (q²/g)^⅓,
        // not a film carrying a river (the virtual-pipe model's 1 cm).
        let mut g = valley(64, 16, 0.02, 0.6);
        for z in 0..16 {
            let i = g.at(32, z);
            g.ground[i] = if (7..=9).contains(&z) {
                g.ground[i] + 0.4
            } else {
                10.0 - 0.02 * 32.0 + 3.0
            };
        }
        for _ in 0..40_000 {
            g.step(0.05);
        }
        let i = g.at(32, 8);
        let q = g.qx[i].abs();
        let critical = (q * q / g.params.g).cbrt();
        assert!(q > 0.01, "passing water: {q} m²/s");
        assert!(
            g.depth[i] >= 0.9 * critical,
            "{} m over the crest, critical {critical}",
            g.depth[i]
        );
    }

    /// Native cost: ns per column per step on a large grid, and the steps a second of water time
    /// takes. Run: `cargo test -p dwell-sim --release watergrid -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cost() {
        for side in [256usize, 1024, 2048] {
            let mut g = valley(side, side, 0.005, 0.5);
            // Water everywhere a little, so every column works.
            g.depth.iter_mut().for_each(|d| *d = 0.2);
            // Warm: the first steps touch the scratch buffers' pages.
            for _ in 0..3 {
                g.step(0.05);
            }
            let t = std::time::Instant::now();
            let steps = (64_000_000 / (side * side)).max(10);
            for _ in 0..steps {
                g.step(0.05);
            }
            let ns = t.elapsed().as_nanos() as f64 / (steps * side * side) as f64;
            println!("COST {side}x{side}: {ns:.1} ns per column per step");
        }
    }

    /// The longest stable step: a dam break (2 m against 0) for each `dt`; stable if no depth goes
    /// wild (over the starting 2 m) and water is conserved.
    #[test]
    #[ignore]
    fn stable_step() {
        for dt in [0.02f32, 0.05, 0.1, 0.15, 0.2, 0.3] {
            let mut g = Grid::new(64, 4);
            for z in 0..4 {
                for x in 0..32 {
                    let i = g.at(x, z);
                    g.depth[i] = 2.0;
                }
            }
            let start = g.stored();
            let mut worst = 0.0f32;
            for _ in 0..((60.0 / dt) as usize) {
                g.step(dt);
                worst = worst.max(g.depth.iter().copied().fold(0.0, f32::max));
            }
            let drift = (g.stored() - start).abs() / start;
            println!("STABLE dt {dt}: max depth {worst:.3} (start 2.0), mass drift {drift:.2e}");
        }
    }
}
