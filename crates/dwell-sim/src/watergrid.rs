//! A prototype (docs/proposals/WATER_MODEL_EVALUATION.md, candidate C): water as a depth on each
//! ground column, flowing to its four neighbours through "virtual pipes" (O'Brien & Hodgins 1995,
//! Mei, Decaudin & Hu 2007; Timberborn's model). Not wired into the game: it exists to measure
//! cost and stability and to be judged in the Water Grid Lab (`docs/prototypes/`).
//!
//! Each step: a pipe's flow accelerates by the difference of the two columns' water surfaces
//! (gravity), damped a little; a column never sends more than it holds; depths change by what
//! comes in less what goes out. Water is conserved exactly (to rounding); sources add, sinks take.

/// The model's constants.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Gravity (m/s²).
    pub g: f32,
    /// Column width (m): the pipes' length and the cells' side.
    pub cell: f32,
    /// A pipe's cross-section (m²) per its length: how freely water passes.
    pub pipe: f32,
    /// How much of a pipe's flow is kept each second (1 = none lost to friction).
    pub keep_per_s: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            g: 9.81,
            cell: 1.0,
            pipe: 1.0,
            keep_per_s: 0.5,
        }
    }
}

/// A grid of columns: ground height (m), water depth (m), the four outflows (m³/s: +x, −x, +z, −z).
#[derive(Clone, Debug)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub ground: Vec<f32>,
    pub depth: Vec<f32>,
    pub flow: Vec<[f32; 4]>,
    /// Where water comes in (column, m³/s) and goes out (column: everything that reaches it).
    pub sources: Vec<(usize, f32)>,
    pub sinks: Vec<usize>,
    pub params: Params,
    /// m³ brought in and taken out so far.
    pub brought: f64,
    pub taken: f64,
}

const DIRS: [(isize, isize); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

impl Grid {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            ground: vec![0.0; w * h],
            depth: vec![0.0; w * h],
            flow: vec![[0.0; 4]; w * h],
            sources: Vec::new(),
            sinks: Vec::new(),
            params: Params::default(),
            brought: 0.0,
            taken: 0.0,
        }
    }

    pub fn at(&self, x: usize, z: usize) -> usize {
        z * self.w + x
    }

    fn neighbour(&self, i: usize, d: usize) -> Option<usize> {
        let (x, z) = ((i % self.w) as isize, (i / self.w) as isize);
        let (nx, nz) = (x + DIRS[d].0, z + DIRS[d].1);
        (nx >= 0 && nz >= 0 && (nx as usize) < self.w && (nz as usize) < self.h)
            .then(|| nz as usize * self.w + nx as usize)
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

    /// One step of `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let p = self.params;
        let area = p.cell * p.cell;
        let keep = p.keep_per_s.powf(dt);
        let n = self.w * self.h;
        for &(i, q) in &self.sources {
            self.depth[i] += q * dt / area;
            self.brought += f64::from(q * dt);
        }
        // The pipes' flows.
        for i in 0..n {
            let hi = self.ground[i] + self.depth[i];
            let mut out = [0.0f32; 4];
            let mut total = 0.0;
            for (d, o) in out.iter_mut().enumerate() {
                if let Some(j) = self.neighbour(i, d) {
                    let dh = hi - (self.ground[j] + self.depth[j]);
                    let f = (self.flow[i][d] * keep + dt * p.pipe * p.g * dh / p.cell).max(0.0);
                    *o = f;
                    total += f;
                }
            }
            // No more out in a step than it holds.
            let have = self.depth[i] * area;
            if total * dt > have && total > 0.0 {
                let k = have / (total * dt);
                out.iter_mut().for_each(|f| *f *= k);
            }
            self.flow[i] = out;
        }
        // The depths.
        let mut next = self.depth.clone();
        for i in 0..n {
            let mut net = -self.flow[i].iter().sum::<f32>();
            for d in 0..4 {
                if let Some(j) = self.neighbour(i, d) {
                    net += self.flow[j][d ^ 1];
                }
            }
            next[i] = (self.depth[i] + net * dt / area).max(0.0);
        }
        self.depth = next;
        for &i in &self.sinks {
            self.taken += f64::from(self.depth[i] * area);
            self.depth[i] = 0.0;
        }
    }

    /// The flow speed through column `i` (m/s): its net flow over the cross-section its depth gives.
    pub fn speed(&self, i: usize) -> f32 {
        let d = self.depth[i].max(1e-3);
        let mut vx = 0.0;
        let mut vz = 0.0;
        if let Some(j) = self.neighbour(i, 1) {
            vx += self.flow[j][0];
        }
        vx -= self.flow[i][1];
        vx += self.flow[i][0];
        if let Some(j) = self.neighbour(i, 0) {
            vx -= self.flow[j][1];
        }
        if let Some(j) = self.neighbour(i, 3) {
            vz += self.flow[j][2];
        }
        vz -= self.flow[i][3];
        vz += self.flow[i][2];
        if let Some(j) = self.neighbour(i, 2) {
            vz -= self.flow[j][3];
        }
        (0.5 * (vx * vx + vz * vz).sqrt()) / (d * self.params.cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valley along x (w × h), its floor falling `slope` m per m, a channel 3 columns wide in the
    /// middle 0.5 m below banks; the river's source at its head, a sink at its foot.
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
        let head = g.at(0, mid);
        g.sources.push((head, q));
        for z in 0..h {
            let i = g.at(w - 1, z);
            g.sinks.push(i);
        }
        g
    }

    #[test]
    fn water_is_conserved() {
        let mut g = valley(64, 16, 0.02, 1.0);
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
        let mut g = valley(64, 16, 0.02, 1.0);
        for _ in 0..20_000 {
            g.step(0.05);
        }
        // Settled: what reaches the foot each second is what the source brings.
        let before = g.taken;
        for _ in 0..200 {
            g.step(0.05);
        }
        let rate = (g.taken - before) / 10.0;
        assert!(
            (rate - 1.0).abs() < 0.05,
            "reaches its foot at {rate} m³/s of 1"
        );
        // In its channel, not over its banks.
        let mid = 8;
        let bank = g.at(32, mid + 3);
        assert!(g.depth[bank] < 0.01, "banks dry: {}", g.depth[bank]);
    }

    #[test]
    fn a_dam_level_with_its_banks_spills_over_both() {
        // A dam across the channel at x = 32 as high as the banks: the river rises to the banks'
        // height and spills over the dam and the banks around its ends (real water's way).
        let mut g = valley(64, 16, 0.02, 1.0);
        for z in 0..16 {
            let i = g.at(32, z);
            let bank = 10.0 - 0.02 * 32.0 + 0.5 + 0.25;
            g.ground[i] = g.ground[i].max(bank);
        }
        for _ in 0..40_000 {
            g.step(0.05);
        }
        let up = g.at(31, 8);
        let dam = g.at(32, 8);
        assert!(
            g.surface(up) > g.ground[dam],
            "risen over the dam: {} vs {}",
            g.surface(up),
            g.ground[dam]
        );
        assert!(g.depth[g.at(33, 8)] > 0.0, "flowing on below the dam");
    }

    /// Native cost: ns per column per step on a large grid, and the steps a second of water time
    /// takes. Run: `cargo test -p dwell-sim --release watergrid -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cost() {
        for side in [256usize, 1024, 2048] {
            let mut g = valley(side, side, 0.005, 5.0);
            // Water everywhere a little, so every column works.
            g.depth.iter_mut().for_each(|d| *d = 0.2);
            let t = std::time::Instant::now();
            let steps = (8_000_000 / (side * side)).max(2);
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
        for dt in [0.02f32, 0.05, 0.1, 0.2, 0.3, 0.5, 1.0] {
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
