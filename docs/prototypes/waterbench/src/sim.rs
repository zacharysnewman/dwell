//! The lab's surface grid (local-inertial face law, θ-weighting, outflow limiter) with sleeping
//! chunks that are skipped entirely, written for speed: sparse 16 × 16 chunks, SoA f32 fields in one
//! allocation per field, rayon over the awake (and fringe) chunks, lazy imbalance for sleeping ones.

use rayon::prelude::*;

pub const CS: usize = 16;
pub const CA: usize = CS * CS;
pub const NONE: u32 = u32::MAX;
pub const G: f32 = 9.81;
pub static mut MANNING: f32 = 0.03;
pub static mut CFL: f32 = 0.7;
#[inline(always)]
fn manning() -> f32 { unsafe { MANNING } }
pub const DRY: f32 = 1e-4;
pub const VOID: f32 = 1.0e4; // ground of a column outside the water's world: a wall
pub const SLEEP_DQ: f32 = 1e-4;
pub const SLEEP_DIV: f32 = 1e-5;
/// The band (m): with it set (> 0), a chunk sleeps once no level in it moved more than this over its
/// calm window (and its faces changed less than BAND_DQ a substep), and a sleeping chunk wakes once
/// the water it owes would move its level this much, or a face it shares jumps by BAND_DQ. 0: the
/// strict tests (rates of 0.01 mm/s, discharge changes of 1e-4 m²/s a substep).
pub static mut BAND: f32 = 0.0;
pub static mut BAND_DQ: f32 = 1e-3;
/// The calm window's least length (water s) when a band is set.
pub static mut WIN_SECS: f64 = 0.0;
#[inline(always)]
fn band() -> f32 { unsafe { BAND } }
/// The wake band (WAKE_BAND, m; 0 = the band): how far a sleeping chunk's account may drift (what it
/// owes, its neighbours' flow) before it wakes. Separate from the band a chunk must be still within to
/// fall asleep: that one must be tight (a chunk asleep off its level gives it back as a wave), this one
/// is the visible scale.
pub static mut WAKE_BAND: f32 = 0.0;
#[inline(always)]
fn wake_band() -> f32 { let w = unsafe { WAKE_BAND }; if w > 0.0 { w } else { band() } }
#[inline(always)]
fn sleep_dq() -> f32 { if band() > 0.0 { if unsafe { SMOOTH } { BIG_DQ } else { unsafe { BAND_DQ } } } else { SLEEP_DQ } }
/// Smooth waking (band mode): a woken chunk takes the water it owes over REL_SECS, not at once; a
/// sleeping chunk wakes when the flow its awake neighbours trade with it has drifted enough to move its
/// wet columns half the band in REACT_SECS (or what it owes would), or a shared face jumps BIG_DQ in a
/// substep (an edit, a flood front); a face's wobble alone no longer wakes it.
pub static mut SMOOTH: bool = false;
/// The neighbours'-drift wake (on by default): off, a sleeping chunk wakes on what it owes, a face's jump or an edit only.
pub static mut DRIFT: bool = true;
/// The absorbing edge (SPONGE, 0 = off): each substep, a face between an awake chunk and a sleeping one
/// is pulled this share of the way back to its recent average, so a wave's swing is damped where it
/// meets a sleeping chunk instead of bouncing off it; steady flow (its own average) passes untouched.
pub static mut SPONGE: f32 = 0.0;
pub const REL_SECS: f64 = 5.0;
pub const REACT_SECS: f64 = 10.0;
pub const BIG_DQ: f32 = 0.1;
pub const SLEEP_CALM: u16 = 20;

#[derive(Clone, Copy)]
pub struct Ptr<T>(pub *mut T);
unsafe impl<T> Send for Ptr<T> {}
unsafe impl<T> Sync for Ptr<T> {}
impl<T> Ptr<T> {
    #[inline(always)]
    pub fn at(self, i: usize) -> *mut T {
        unsafe { self.0.add(i) }
    }
}

pub struct Sim {
    pub nch: usize,
    pub key: Vec<(i32, i32)>,
    /// E (+x), W (−x), S (+z), N (−z)
    pub nbr: Vec<[u32; 4]>,
    pub ground: Vec<f32>,
    pub depth: Vec<f32>,
    pub qx: Vec<f32>,
    pub qz: Vec<f32>,
    pub nqx: Vec<f32>,
    pub nqz: Vec<f32>,
    pub scale: Vec<f32>,
    // per chunk
    pub awake: Vec<u8>,
    pub calm: Vec<u16>,
    /// the window test: each column's depth when the chunk's calm began, and when
    pub win_depth: Vec<f32>,
    pub win_t: Vec<f64>,
    pub window_test: bool,
    pub dq: Vec<f32>,
    pub div: Vec<f32>,
    pub dmax: Vec<f32>,
    pub edge_dq_e: Vec<f32>,
    pub edge_dq_s: Vec<f32>,
    pub charge_e: Vec<f32>,
    pub charge_s: Vec<f32>,
    pub wake_flag: Vec<u8>,
    pub rate: Vec<f64>,
    pub owed: Vec<f64>,
    pub t_rate: Vec<f64>,
    pub mode: Vec<u8>, // per substep: 0 idle, 1 awake, 2 fringe E, 4 fringe S (bits)
    pub chunk_in: Vec<f64>,
    pub chunk_out: Vec<f64>,
    /// per chunk: (local column, inflow m³/s)
    pub sources: Vec<Vec<(u16, f32)>>,
    pub sinks: Vec<Vec<u16>>,
    /// Outlets as a fixed sea level (HOLD_SEA): each outlet column held at this depth (the sea's
    /// level over its ground); what runs in leaves the world, and the column reads as steady, so its
    /// chunk can sleep, its outflow then kept as one steady number (sink_out) until it wakes.
    pub hold: Vec<f32>,
    pub pools: Pools,
    pub hold_sea: bool,
    pub sink_out: Vec<f64>,
    pub t_sleep: Vec<f64>,
    /// per column inflow (m³/s), from `sources`; counted in the balance, so a steady spring can sleep
    pub inflow: Vec<f32>,
    pub src_total: f64,
    pub brought_acc: f64,
    pub spring_sleep: bool,
    pub theta: f32,
    pub sleep: bool,
    pub t: f64,
    pub brought: f64,
    pub taken: f64,
    pub owed_lost: f64,
    // lists
    pub active: Vec<u32>,  // awake ∪ fringe
    pub awake_list: Vec<u32>,
    pub dirty: bool,
    pub substeps: u64,
    pub wakes: u64,
    pub sleeps: u64,
    pub col_steps: u64,
    pub wake_face: u64,
    /// wet columns when the chunk fell asleep (band mode: what owed water is spread over)
    pub wet_n: Vec<u16>,
    pub rate0: Vec<f64>,
    /// per chunk: wakes by a face jump, by owed water, by the neighbours' drift; substeps awake
    pub why: Vec<[u32; 4]>,
    /// the drift from rate0, averaged over REACT_SECS (sampled each step)
    pub drift: Vec<f64>,
    pub rel_left: Vec<f64>,
    pub rel_rate: Vec<f64>,
    pub wake_rate: u64,
    /// each face's discharge averaged over the last ~EMA_SECS (band mode: what a chunk sleeps with)
    pub ex: Vec<f32>,
    pub ez: Vec<f32>,
    /// each column's depth averaged over the last ~ema_secs (MEAN_SLEEP: a chunk sleeps at it)
    pub ed: Vec<f32>,
    pub mean_sleep: bool,
    pub ema_secs: f32,
    pub wake_owed: u64,
    pub wake_edit: u64,
}

pub static mut VECTOR: bool = true;
pub static mut VECTOR_LIMIT: bool = false;

/// The face law. `qp` is the discharge across the face's line (the mean of the four perpendicular
/// faces around it): with VECTOR, friction and the critical limit use the flow's whole speed
/// √(q² + qp²), not just this face's share of it (diagonal flow splits between x and z faces, and
/// alone each face would see ~70% of the speed and let the flow run ~1.4× critical).
#[inline(always)]
fn face(ga: f32, da: f32, gb: f32, db: f32, q: f32, qa: f32, qb: f32, dt: f32, theta: f32, qp: f32) -> f32 {
    let ea = ga + da;
    let eb = gb + db;
    let h = ea.max(eb) - ga.max(gb);
    if h <= DRY {
        return 0.0;
    }
    let vector = unsafe { VECTOR };
    let speed = if vector { (q * q + qp * qp).sqrt() } else { q.abs() };
    let friction = if speed == 0.0 {
        1.0
    } else {
        // h^(7/3) = h² · ∛h
        1.0 + G * dt * manning() * manning() * speed / (h * h * h.cbrt())
    };
    let qt = theta * q + (1.0 - theta) * 0.5 * (qa + qb);
    let nq = (qt - G * h * dt * (eb - ea)) / friction;
    let crit = h * (G * h).sqrt();
    // the limit on this face's share: the whole speed no faster than critical
    let limit = if vector && unsafe { VECTOR_LIMIT } { (crit * crit - qp * qp).max(0.0).sqrt() } else { crit };
    nq.clamp(-limit, limit)
}

/// The column next to global column `i` toward E (0), W (1), S (2) or N (3), or NONE.
#[inline(always)]
fn step_col(nbr: &[[u32; 4]], i: usize, dir: usize) -> usize {
    let (c, l) = (i / CA, i % CA);
    let (x, z) = (l % CS, l / CS);
    let inside = match dir { 0 => x + 1 < CS, 1 => x > 0, 2 => z + 1 < CS, _ => z > 0 };
    if inside {
        return match dir { 0 => i + 1, 1 => i - 1, 2 => i + CS, _ => i - CS };
    }
    let n = nbr[c][dir];
    if n == NONE { return usize::MAX; }
    let b = n as usize * CA;
    match dir { 0 => b + z * CS, 1 => b + z * CS + CS - 1, 2 => b + x, _ => b + (CS - 1) * CS + x }
}

impl Sim {
    pub fn save(&self, path: &str) {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        f.write_all(&self.t.to_le_bytes()).unwrap();
        for v in [&self.depth, &self.qx, &self.qz] {
            let b = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) };
            f.write_all(b).unwrap();
        }
        for v in [&self.chunk_in, &self.chunk_out] {
            let b = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 8) };
            f.write_all(b).unwrap();
        }
    }
    pub fn load(&mut self, path: &str) -> bool {
        use std::io::Read;
        let Ok(mut f) = std::fs::File::open(path) else { return false };
        let mut t = [0u8; 8];
        f.read_exact(&mut t).unwrap();
        self.t = f64::from_le_bytes(t);
        for v in [&mut self.depth, &mut self.qx, &mut self.qz] {
            let b = unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u8, v.len() * 4) };
            f.read_exact(b).unwrap();
        }
        for v in [&mut self.chunk_in, &mut self.chunk_out] {
            let b = unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u8, v.len() * 8) };
            f.read_exact(b).unwrap();
        }
        true
    }

    pub fn new(keys: Vec<(i32, i32)>) -> Self {
        let nch = keys.len();
        let mut index = std::collections::HashMap::with_capacity(nch);
        for (c, &k) in keys.iter().enumerate() {
            index.insert(k, c as u32);
        }
        let nbr = keys
            .iter()
            .map(|&(cx, cz)| {
                let g = |dx: i32, dz: i32| *index.get(&(cx + dx, cz + dz)).unwrap_or(&NONE);
                [g(1, 0), g(-1, 0), g(0, 1), g(0, -1)]
            })
            .collect();
        let n = nch * CA;
        Sim {
            nch,
            key: keys,
            nbr,
            ground: vec![VOID; n],
            depth: vec![0.0; n],
            qx: vec![0.0; n],
            qz: vec![0.0; n],
            nqx: vec![0.0; n],
            nqz: vec![0.0; n],
            scale: vec![1.0; n],
            awake: vec![1; nch],
            calm: vec![0; nch],
            win_depth: vec![0.0; n],
            win_t: vec![0.0; nch],
            window_test: true,
            dq: vec![0.0; nch],
            div: vec![0.0; nch],
            dmax: vec![0.0; nch],
            edge_dq_e: vec![0.0; nch],
            edge_dq_s: vec![0.0; nch],
            charge_e: vec![0.0; nch],
            charge_s: vec![0.0; nch],
            wake_flag: vec![0; nch],
            rate: vec![0.0; nch],
            owed: vec![0.0; nch],
            t_rate: vec![0.0; nch],
            mode: vec![0; nch],
            chunk_in: vec![0.0; nch],
            chunk_out: vec![0.0; nch],
            sources: vec![Vec::new(); nch],
            sinks: vec![Vec::new(); nch],
            hold: vec![0.0; n],
            pools: Pools::new(n, nch),
            hold_sea: false,
            sink_out: vec![0.0; nch],
            t_sleep: vec![0.0; nch],
            inflow: vec![0.0; n],
            src_total: 0.0,
            brought_acc: 0.0,
            spring_sleep: true,
            theta: 0.95,
            sleep: true,
            t: 0.0,
            brought: 0.0,
            taken: 0.0,
            owed_lost: 0.0,
            active: Vec::new(),
            awake_list: Vec::new(),
            dirty: true,
            substeps: 0,
            wakes: 0,
            sleeps: 0,
            col_steps: 0,
            wake_face: 0,
            wet_n: vec![0; nch],
            rate0: vec![0.0; nch],
            why: vec![[0; 4]; nch],
            drift: vec![0.0; nch],
            rel_left: vec![0.0; nch],
            rel_rate: vec![0.0; nch],
            wake_rate: 0,
            ex: vec![0.0; n],
            ez: vec![0.0; n],
            ed: vec![0.0; n],
            mean_sleep: false,
            ema_secs: 0.0,
            wake_owed: 0,
            wake_edit: 0,
        }
    }

    /// After `sources` change: the per-column inflow and its total.
    pub fn rebuild_inflow(&mut self) {
        self.inflow.iter_mut().for_each(|v| *v = 0.0);
        self.src_total = 0.0;
        for c in 0..self.nch {
            for &(l, q) in &self.sources[c] {
                self.inflow[c * CA + l as usize] += q;
                self.src_total += q as f64;
            }
        }
    }

    pub fn awake_count(&self) -> usize {
        self.awake.iter().filter(|&&a| a != 0).count()
    }

    pub fn stored(&self) -> f64 {
        self.depth.par_iter().map(|&d| d as f64).sum()
    }

    pub fn owed_total(&self) -> f64 {
        (0..self.nch)
            .filter(|&c| self.awake[c] == 0)
            .map(|c| self.owed[c] + self.rate[c] * (self.t - self.t_rate[c]))
            .sum::<f64>()
            + self.rel_left.iter().sum::<f64>()
    }

    fn rebuild_lists(&mut self) {
        self.mode.iter_mut().for_each(|m| *m = 0);
        self.awake_list.clear();
        for c in 0..self.nch {
            if self.awake[c] != 0 {
                self.awake_list.push(c as u32);
                self.mode[c] |= 1;
            }
        }
        // fringe: a sleeping chunk whose east or south neighbour is awake owns faces it must compute
        for &c in &self.awake_list {
            let [_, w, _, n] = self.nbr[c as usize];
            if w != NONE && self.awake[w as usize] == 0 {
                self.mode[w as usize] |= 2;
            }
            if n != NONE && self.awake[n as usize] == 0 {
                self.mode[n as usize] |= 4;
            }
        }
        self.active.clear();
        for c in 0..self.nch {
            if self.mode[c] != 0 {
                self.active.push(c as u32);
            }
        }
        self.dirty = false;
    }

    pub fn wake_all(&mut self) {
        for c in 0..self.nch {
            self.wake(c);
        }
        self.dirty = true;
    }

    pub fn wake(&mut self, c: usize) {
        if self.awake[c] != 0 || self.pools.pooled[c] == 1 {
            return;
        }
        self.wakes += 1;
        self.awake[c] = 1;
        self.calm[c] = 0;
        if self.sink_out[c] != 0.0 {
            self.chunk_out[c] += self.sink_out[c] * (self.t - self.t_sleep[c]);
            self.sink_out[c] = 0.0;
        }
        let v = self.owed[c] + self.rate[c] * (self.t - self.t_rate[c]);
        self.owed[c] = 0.0;
        self.rate[c] = 0.0;
        self.dirty = true;
        if v == 0.0 {
            return;
        }
        let b = c * CA;
        if band() > 0.0 && unsafe { SMOOTH } {
            self.rel_left[c] += v;
            self.rel_rate[c] = self.rel_left[c] / REL_SECS;
            return;
        }
        if band() > 0.0 {
            // evenly over its wet columns (a level change, not a share of depth), removing no more than is there
            let d = &mut self.depth[b..b + CA];
            let n = d.iter().filter(|&&x| x > 0.0).count();
            if n > 0 {
                let mut left = v;
                for _ in 0..4 {
                    let wet = d.iter().filter(|&&x| x > 0.0).count().max(1);
                    let per = (left / wet as f64) as f32;
                    left = 0.0;
                    for x in d.iter_mut().filter(|x| **x > 0.0) {
                        let nx = *x + per;
                        if nx < 0.0 { left += nx as f64; *x = 0.0; } else { *x = nx; }
                    }
                    if left == 0.0 { break; }
                }
                self.owed_lost += -left;
                return;
            }
        }
        let d = &mut self.depth[b..b + CA];
        let tot: f64 = d.iter().map(|&x| x as f64).sum();
        if v < 0.0 && -v > tot {
            self.owed_lost += -v - tot;
            d.iter_mut().for_each(|x| *x = 0.0);
            return;
        }
        if tot > 0.0 {
            let k = (v / tot) as f32;
            d.iter_mut().for_each(|x| *x += *x * k);
        } else {
            // spread over its columns that aren't void
            let g = &self.ground[b..b + CA];
            let n = g.iter().filter(|&&x| x < VOID).count().max(1);
            let per = (v / n as f64) as f32;
            for (x, &gg) in d.iter_mut().zip(g) {
                if gg < VOID {
                    *x += per;
                }
            }
        }
    }

    pub fn wake_around(&mut self, c: usize) {
        self.wake(c);
        self.calm[c] = 0;
        for k in 0..4 {
            let n = self.nbr[c][k];
            if n != NONE {
                self.wake(n as usize);
                self.calm[n as usize] = 0;
            }
        }
    }

    /// One step of `dt`: each substep as long as the deepest active water allows now (CFL 0.7),
    /// so water piling up partway through a step can't outrun its substep.
    pub fn step(&mut self, dt: f64) {
        if self.sleep && band() > 0.0 {
            // a sleeping chunk whose owed water would now move its level by the band wakes
            let t = self.t;
            let smooth = unsafe { SMOOTH };
            let mut wake: Vec<usize> = (0..self.nch).filter(|&c| self.awake[c] == 0 && (self.owed[c] + self.rate[c] * (t - self.t_rate[c])).abs() > (wake_band() as f64 * self.wet_n[c].max(1) as f64) * 0.5).collect();
            self.wake_owed += wake.len() as u64;
            for &c in &wake { self.why[c][1] += 1; }
            if smooth && unsafe { DRIFT } {
                let drift: Vec<usize> = (0..self.nch).filter(|&c| self.awake[c] == 0 && !wake.contains(&c)
                    && (self.rate[c] - self.rate0[c]).abs() > wake_band() as f64 * self.wet_n[c].max(1) as f64 * 0.5 / REACT_SECS).collect();
                self.wake_rate += drift.len() as u64;
                for &c in &drift { self.why[c][2] += 1; }
                wake.extend(drift);
            }
            for c in wake { self.wake(c); }
        }
        if self.pools.on {
            self.pools.clock += dt;
            if self.pools.clock >= POOL_EVERY { self.pools.clock = 0.0; let t = std::time::Instant::now(); self.pools_detect(); self.pools.tm[0] += t.elapsed().as_secs_f64(); }
        }
        let end = self.t + dt;
        while self.t < end - 1e-9 {
            if self.dirty {
                self.rebuild_lists();
            }
            let mut deepest = 0.01f32;
            for &c in &self.active {
                deepest = deepest.max(self.dmax[c as usize]);
            }
            if self.pools.any { deepest = deepest.max(self.pools.deep); }
            let cfl = (unsafe { CFL } / (G * deepest).sqrt()) as f64;
            // equal substeps for what's left, none over the limit
            let left = end - self.t;
            let n = (left / cfl).ceil().max(1.0);
            self.substep((left / n) as f32);
        }
    }

    pub fn substep(&mut self, dt: f32) {
        if self.dirty {
            self.rebuild_lists();
        }
        self.substeps += 1;
        let theta = self.theta;
        let kdt = dt; // CELL = 1
        let ground = Ptr(self.ground.as_mut_ptr());
        let depth = Ptr(self.depth.as_mut_ptr());
        let qx = Ptr(self.qx.as_mut_ptr());
        let qz = Ptr(self.qz.as_mut_ptr());
        let nqx = Ptr(self.nqx.as_mut_ptr());
        let nqz = Ptr(self.nqz.as_mut_ptr());
        let scale = Ptr(self.scale.as_mut_ptr());
        let nbr: &[[u32; 4]] = unsafe { std::slice::from_raw_parts(self.nbr.as_ptr(), self.nbr.len()) }; // (not changed during a substep)
        let mode: &[u8] = unsafe { std::slice::from_raw_parts(self.mode.as_ptr(), self.mode.len()) };
        let awake: &[u8] = unsafe { std::slice::from_raw_parts(self.awake.as_ptr(), self.awake.len()) };

        // 1. momentum: each active chunk computes the faces it owns (+x and +z of its columns)
        self.active.par_iter().with_min_len(4).for_each(|&c| unsafe {
            let c = c as usize;
            let m = mode[c];
            let b = c * CA;
            let [e, w, s, n] = nbr[c];
            let g = |i: usize| *ground.at(i);
            let d = |i: usize| *depth.at(i);
            let gq = |p: Ptr<f32>, i: usize| *p.at(i);
            // the mean perpendicular discharge around the x face (i, j) and the z face (i, j)
            let px = |i: usize, j: usize| -> f32 {
                let (ni, nj) = (step_col(nbr, i, 3), step_col(nbr, j, 3));
                0.25 * (gq(qz, i) + gq(qz, j) + if ni != usize::MAX { gq(qz, ni) } else { 0.0 } + if nj != usize::MAX { gq(qz, nj) } else { 0.0 })
            };
            let pz = |i: usize, j: usize| -> f32 {
                let (wi, wj) = (step_col(nbr, i, 1), step_col(nbr, j, 1));
                0.25 * (gq(qx, i) + gq(qx, j) + if wi != usize::MAX { gq(qx, wi) } else { 0.0 } + if wj != usize::MAX { gq(qx, wj) } else { 0.0 })
            };
            if m & 1 != 0 {
                for z in 0..CS {
                    let row = b + z * CS;
                    // +x faces inside the chunk
                    for x in 0..CS - 1 {
                        let i = row + x;
                        let j = i + 1;
                        let (di, dj) = (d(i), d(j));
                        *nqx.at(i) = if di > 0.0 || dj > 0.0 {
                            let qa = if x > 0 { gq(qx, i - 1) } else if w != NONE { gq(qx, w as usize * CA + z * CS + CS - 1) } else { gq(qx, i) };
                            let qb = gq(qx, j);
                            face(g(i), di, g(j), dj, gq(qx, i), qa, qb, dt, theta, px(i, j))
                        } else {
                            0.0
                        };
                    }
                    // the +x face across to the east chunk
                    let i = row + CS - 1;
                    *nqx.at(i) = if e != NONE {
                        let j = e as usize * CA + z * CS;
                        let (di, dj) = (d(i), d(j));
                        if di > 0.0 || dj > 0.0 {
                            face(g(i), di, g(j), dj, gq(qx, i), gq(qx, i - 1), gq(qx, j), dt, theta, px(i, j))
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };
                }
                for z in 0..CS {
                    let row = b + z * CS;
                    if z < CS - 1 {
                        for x in 0..CS {
                            let i = row + x;
                            let j = i + CS;
                            let (di, dj) = (d(i), d(j));
                            *nqz.at(i) = if di > 0.0 || dj > 0.0 {
                                let qa = if z > 0 { gq(qz, i - CS) } else if n != NONE { gq(qz, n as usize * CA + (CS - 1) * CS + x) } else { gq(qz, i) };
                                let qb = gq(qz, j);
                                face(g(i), di, g(j), dj, gq(qz, i), qa, qb, dt, theta, pz(i, j))
                            } else {
                                0.0
                            };
                        }
                    } else {
                        for x in 0..CS {
                            let i = row + x;
                            *nqz.at(i) = if s != NONE {
                                let j = s as usize * CA + x;
                                let (di, dj) = (d(i), d(j));
                                if di > 0.0 || dj > 0.0 {
                                    face(g(i), di, g(j), dj, gq(qz, i), gq(qz, i - CS), gq(qz, j), dt, theta, pz(i, j))
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            };
                        }
                    }
                }
            } else {
                // fringe: only the edge faces toward an awake neighbour
                if m & 2 != 0 {
                    let ec = e as usize * CA;
                    for z in 0..CS {
                        let i = b + z * CS + CS - 1;
                        let j = ec + z * CS;
                        let (di, dj) = (d(i), d(j));
                        *nqx.at(i) = if di > 0.0 || dj > 0.0 { face(g(i), di, g(j), dj, gq(qx, i), gq(qx, i - 1), gq(qx, j), dt, theta, px(i, j)) } else { 0.0 };
                    }
                }
                if m & 4 != 0 {
                    let sc = s as usize * CA;
                    for x in 0..CS {
                        let i = b + (CS - 1) * CS + x;
                        let j = sc + x;
                        let (di, dj) = (d(i), d(j));
                        *nqz.at(i) = if di > 0.0 || dj > 0.0 { face(g(i), di, g(j), dj, gq(qz, i), gq(qz, i - CS), gq(qz, j), dt, theta, pz(i, j)) } else { 0.0 };
                    }
                }
            }
        });

        let tz = std::time::Instant::now(); self.pools_zero_inside(); self.pools.tm[1] += tz.elapsed().as_secs_f64();
        // 2. outflow limiter for awake columns
        self.awake_list.par_iter().with_min_len(4).for_each(|&c| unsafe {
            let c = c as usize;
            let b = c * CA;
            let [_, w, _, n] = nbr[c];
            for z in 0..CS {
                for x in 0..CS {
                    let i = b + z * CS + x;
                    let mut out = (*nqx.at(i)).max(0.0) + (*nqz.at(i)).max(0.0);
                    let wq = if x > 0 { *nqx.at(i - 1) } else if w != NONE { *nqx.at(w as usize * CA + z * CS + CS - 1) } else { 0.0 };
                    let nq = if z > 0 { *nqz.at(i - CS) } else if n != NONE { *nqz.at(n as usize * CA + (CS - 1) * CS + x) } else { 0.0 };
                    out += (-wq).max(0.0) + (-nq).max(0.0);
                    out *= kdt;
                    let dd = *depth.at(i);
                    *scale.at(i) = if out > dd { dd / out } else { 1.0 };
                }
            }
        });

        let tz = std::time::Instant::now(); self.pools_limit(kdt); self.pools.tm[2] += tz.elapsed().as_secs_f64();
        // 3. settle the faces each active chunk computed
        let dqv = Ptr(self.dq.as_mut_ptr());
        let edge_e = Ptr(self.edge_dq_e.as_mut_ptr());
        let edge_s = Ptr(self.edge_dq_s.as_mut_ptr());
        let ch_e = Ptr(self.charge_e.as_mut_ptr());
        let ch_s = Ptr(self.charge_s.as_mut_ptr());
        let wflag = Ptr(self.wake_flag.as_mut_ptr());
        let ratev = Ptr(self.rate.as_mut_ptr());
        let owedv = Ptr(self.owed.as_mut_ptr());
        let t_ratev = Ptr(self.t_rate.as_mut_ptr());
        let tnow = self.t;
        let exv = Ptr(self.ex.as_mut_ptr());
        let ezv = Ptr(self.ez.as_mut_ptr());
        let alpha = if self.ema_secs > 0.0 { (dt / self.ema_secs).min(1.0) } else { 0.0 };
        let sponge = unsafe { SPONGE };
        self.active.par_iter().with_min_len(4).for_each(|&c| unsafe {
            let c = c as usize;
            let m = mode[c];
            let b = c * CA;
            let [e, _, s, _] = nbr[c];
            let me_awake = m & 1 != 0;
            let mut dq_in = 0.0f32;
            let (mut dq_e, mut dq_s) = (0.0f32, 0.0f32);
            let (mut chg_e, mut chg_s) = (0.0f32, 0.0f32);
            let mut own_charge = 0.0f64;
            let e_awake = e != NONE && awake[e as usize] != 0;
            let s_awake = s != NONE && awake[s as usize] != 0;
            let settle = |p: Ptr<f32>, np: Ptr<f32>, em: Ptr<f32>, i: usize, j: usize| -> f32 {
                let mut nq = *np.at(i);
                nq *= if nq > 0.0 { *scale.at(i) } else { *scale.at(j) };
                let d = nq - *p.at(i);
                *p.at(i) = nq;
                if alpha > 0.0 { *em.at(i) += alpha * (nq - *em.at(i)); }
                d
            };
            if me_awake {
                for z in 0..CS {
                    for x in 0..CS - 1 {
                        let i = b + z * CS + x;
                        let d = settle(qx, nqx, exv, i, i + 1);
                        dq_in = dq_in.max(d.abs());
                    }
                }
                for z in 0..CS - 1 {
                    for x in 0..CS {
                        let i = b + z * CS + x;
                        let d = settle(qz, nqz, ezv, i, i + CS);
                        dq_in = dq_in.max(d.abs());
                    }
                }
            }
            if e != NONE && (me_awake || m & 2 != 0) {
                for z in 0..CS {
                    let i = b + z * CS + CS - 1;
                    let j = e as usize * CA + z * CS;
                    let d = settle(qx, nqx, exv, i, j);
                    let d = if sponge > 0.0 && (!me_awake || !e_awake) { let v = *qx.at(i); let nv = v + sponge * (*exv.at(i) - v); *qx.at(i) = nv; d + (nv - v) } else { d };
                    dq_e = dq_e.max(d.abs());
                    if !me_awake { own_charge -= d as f64; } else if !e_awake { chg_e += d; }
                }
            }
            if s != NONE && (me_awake || m & 4 != 0) {
                for x in 0..CS {
                    let i = b + (CS - 1) * CS + x;
                    let j = s as usize * CA + x;
                    let d = settle(qz, nqz, ezv, i, j);
                    let d = if sponge > 0.0 && (!me_awake || !s_awake) { let v = *qz.at(i); let nv = v + sponge * (*ezv.at(i) - v); *qz.at(i) = nv; d + (nv - v) } else { d };
                    dq_s = dq_s.max(d.abs());
                    if !me_awake { own_charge -= d as f64; } else if !s_awake { chg_s += d; }
                }
            }
            *dqv.at(c) = if me_awake { dq_in.max(dq_e).max(dq_s) } else { 0.0 };
            *edge_e.at(c) = dq_e;
            *edge_s.at(c) = dq_s;
            *ch_e.at(c) = chg_e;
            *ch_s.at(c) = chg_s;
            if !me_awake {
                if own_charge != 0.0 {
                    *owedv.at(c) += *ratev.at(c) * (tnow - *t_ratev.at(c));
                    *t_ratev.at(c) = tnow;
                    *ratev.at(c) += own_charge;
                }
                if dq_e.max(dq_s) > sleep_dq() {
                    *wflag.at(c) = 1;
                }
            }
        });

        // 4. water balance for awake columns, springs and outlets
        let divv = Ptr(self.div.as_mut_ptr());
        let dmaxv = Ptr(self.dmax.as_mut_ptr());
        let inv = Ptr(self.chunk_in.as_mut_ptr());
        let outv = Ptr(self.chunk_out.as_mut_ptr());
        let sources = &self.sources;
        let sinks = &self.sinks;
        let inflow = Ptr(self.inflow.as_mut_ptr());
        let rell = Ptr(self.rel_left.as_mut_ptr());
        let edv = Ptr(self.ed.as_mut_ptr());
        let dalpha = if self.mean_sleep && self.ema_secs > 0.0 { (dt / self.ema_secs).min(1.0) } else { 0.0 };
        let pof = Ptr(self.pools.of.as_mut_ptr());
        let pany = self.pools.any;
        let holdv = Ptr(self.hold.as_mut_ptr());
        let hold_sea = self.hold_sea;
        let relr = Ptr(self.rel_rate.as_mut_ptr());
        let spring_sleep = self.spring_sleep;
        self.awake_list.par_iter().with_min_len(4).for_each(|&c| unsafe {
            let c = c as usize;
            let b = c * CA;
            let [_, w, _, n] = nbr[c];
            let mut dv = 0.0f32;
            let mut mx = 0.0f32;
            for z in 0..CS {
                for x in 0..CS {
                    let i = b + z * CS + x;
                    if pany && *pof.at(i) >= 0 { continue; }
                    let mut net = -*qx.at(i) - *qz.at(i);
                    net += if x > 0 { *qx.at(i - 1) } else if w != NONE { *qx.at(w as usize * CA + z * CS + CS - 1) } else { 0.0 };
                    net += if z > 0 { *qz.at(i - CS) } else if n != NONE { *qz.at(n as usize * CA + (CS - 1) * CS + x) } else { 0.0 };
                    if spring_sleep { net += *inflow.at(i); }
                    let nd = *depth.at(i) + net * kdt;
                    let nd = if nd > 0.0 { nd } else { 0.0 };
                    *depth.at(i) = nd;
                    if dalpha > 0.0 { *edv.at(i) += dalpha * (nd - *edv.at(i)); }
                    dv = dv.max(net.abs());
                    mx = mx.max(nd);
                }
            }
            if !spring_sleep { for &(l, q) in &sources[c] {
                *depth.at(b + l as usize) += q * kdt;
                *inv.at(c) += (q * kdt) as f64;
                dv = dv.max(1.0);
            } }
            // smooth waking: the water this chunk still owes comes in a little each substep, where the
            // water is, in proportion to each column's depth (an even share would perch a puddle on a
            // bank film, or drain a small pocket dry)
            let left = *rell.at(c);
            if left != 0.0 {
                let want = (*relr.at(c) * kdt as f64).abs().min(left.abs()) * left.signum();
                let d = std::slice::from_raw_parts_mut(depth.at(b), CA);
                let mut tot = 0.0f64;
                for l in 0..CA { if !(pany && *pof.at(b + l) >= 0) { tot += d[l] as f64; } }
                if tot > 0.0 {
                    let k = (want / tot).max(-1.0);
                    let mut applied = 0.0f64;
                    for l in 0..CA {
                        if pany && *pof.at(b + l) >= 0 { continue; }
                        let add = (d[l] as f64 * k) as f32;
                        d[l] += add;
                        applied += add as f64;
                    }
                    *rell.at(c) = left - applied;
                    if (*rell.at(c)).abs() < 1e-9 { *rell.at(c) = 0.0; }
                }
            }
            for &l in &sinks[c] {
                let i = b + l as usize;
                if hold_sea {
                    let h = *holdv.at(i);
                    *outv.at(c) += (*depth.at(i) - h) as f64;
                    *depth.at(i) = h;
                } else {
                    *outv.at(c) += *depth.at(i) as f64;
                    if *depth.at(i) > 0.0 {
                        dv = dv.max(1.0);
                    }
                    *depth.at(i) = 0.0;
                }
            }
            *divv.at(c) = dv;
            *dmaxv.at(c) = mx;
        });
        let tz = std::time::Instant::now(); self.pools_update(dt); self.pools.tm[3] += tz.elapsed().as_secs_f64();
        self.col_steps += (self.awake_list.len() * CA) as u64;
        for &c in &self.awake_list { self.why[c as usize][3] += 1; }
        self.t += dt as f64;
        self.brought_acc += self.src_total * dt as f64;

        let t5 = std::time::Instant::now();
        // 5. bookkeeping (only active chunks): charges to sleeping neighbours, sleep and wake
        if !self.sleep {
            return;
        }
        let tnow = self.t;
        let mut to_wake: Vec<usize> = Vec::new();
        for k in 0..self.active.len() {
            let c = self.active[k] as usize;
            let [e, _, s, _] = self.nbr[c];
            for (nb, ch) in [(e, self.charge_e[c]), (s, self.charge_s[c])] {
                if nb == NONE || ch == 0.0 {
                    continue;
                }
                let nb = nb as usize;
                if self.awake[nb] == 0 && self.pools.pooled[nb] == 0 {
                    self.owed[nb] += self.rate[nb] * (tnow - self.t_rate[nb]);
                    self.t_rate[nb] = tnow;
                    self.rate[nb] += ch as f64;
                    if ch.abs() > sleep_dq() {
                        self.wake_flag[nb] = 1;
                    }
                    if band() == 0.0 && self.rate[nb].abs() / CA as f64 > SLEEP_DIV as f64 {
                        self.wake_flag[nb] = 1;
                    }
                }
            }
            if band() == 0.0 && self.awake[c] == 0 && self.rate[c].abs() / CA as f64 > SLEEP_DIV as f64 {
                self.wake_flag[c] = 1;
            }
        }
        // sleep tests for awake chunks
        for k in 0..self.awake_list.len() {
            let c = self.awake_list[k] as usize;
            let [_, w, _, n] = self.nbr[c];
            let mut dq = self.dq[c];
            if w != NONE { dq = dq.max(self.edge_dq_e[w as usize]); }
            if n != NONE { dq = dq.max(self.edge_dq_s[n as usize]); }
            // the level test: per substep (the first), or over the calm window — whether each column's
            // level actually moved, so a micrometre flicker between neighbours (diagonal flow) passes
            if self.pools.shore[c] == 1 { self.calm[c] = 0; continue; }
            let level_ok = if self.window_test { true } else { self.div[c] < SLEEP_DIV };
            if dq < sleep_dq() && level_ok {
                if self.window_test && self.calm[c] == 0 {
                    let b = c * CA;
                    let (src, dst) = (&self.depth[b..b + CA], &mut self.win_depth[b..b + CA]);
                    dst.copy_from_slice(src);
                    self.win_t[c] = tnow;
                }
                self.calm[c] += 1;
                let mut ready = self.calm[c] >= SLEEP_CALM && (band() == 0.0 || tnow - self.win_t[c] >= unsafe { WIN_SECS });
                if ready && self.window_test {
                    let b = c * CA;
                    let span = (tnow - self.win_t[c]).max(1e-6) as f32;
                    let moved = self.depth[b..b + CA].iter().zip(&self.win_depth[b..b + CA]).fold(0.0f32, |m, (a, w)| m.max((a - w).abs()));
                    // a spring's or outlet's column is steady only by the balance, which div holds
                    let fixed = self.div[c] >= 1.0;
                    let too_much = if band() > 0.0 { moved > band() } else { moved / span >= SLEEP_DIV };
                    if too_much || fixed {
                        self.calm[c] = 0;
                        ready = false;
                    }
                }
                if ready {
                    if self.ema_secs > 0.0 { self.faces_to_mean(c, w, n, tnow); }
                    if self.mean_sleep { self.depths_to_mean(c); }
                    // its imbalance from now
                    let b = c * CA;
                    let mut r = 0.0f64;
                    let mut so = 0.0f64;
                    for z in 0..CS {
                        for x in 0..CS {
                            let i = b + z * CS + x;
                            let mut net = -self.qx[i] - self.qz[i];
                            net += if x > 0 { self.qx[i - 1] } else if w != NONE { self.qx[w as usize * CA + z * CS + CS - 1] } else { 0.0 };
                            net += if z > 0 { self.qz[i - CS] } else if n != NONE { self.qz[n as usize * CA + (CS - 1) * CS + x] } else { 0.0 };
                            if self.spring_sleep { net += self.inflow[i]; }
                            r += net as f64;
                            self.scale[i] = 1.0;
                        }
                    }
                    if self.hold_sea {
                        // what runs into its outlets leaves the world (a steady outflow while it sleeps)
                        for &l in &self.sinks[c] {
                            let i = b + l as usize; let (x, z) = (l as usize % CS, l as usize / CS);
                            let mut net = -self.qx[i] - self.qz[i];
                            net += if x > 0 { self.qx[i - 1] } else if w != NONE { self.qx[w as usize * CA + z * CS + CS - 1] } else { 0.0 };
                            net += if z > 0 { self.qz[i - CS] } else if n != NONE { self.qz[n as usize * CA + (CS - 1) * CS + x] } else { 0.0 };
                            so += net as f64;
                        }
                        r -= so;
                    }
                    self.sink_out[c] = so;
                    self.t_sleep[c] = tnow;
                    self.rate[c] = r;
                    self.rate0[c] = r;
                    self.drift[c] = 0.0;
                    self.wet_n[c] = self.depth[c * CA..c * CA + CA].iter().filter(|&&d| d > 0.0).count() as u16;
                    // water still to come in from its last waking is owed again
                    self.owed[c] = self.rel_left[c];
                    self.rel_left[c] = 0.0;
                    self.t_rate[c] = tnow;
                    self.awake[c] = 0;
                    self.sleeps += 1;
                    self.dirty = true;
                }
            } else {
                self.calm[c] = 0;
            }
        }
        for k in 0..self.active.len() {
            let c = self.active[k] as usize;
            if self.wake_flag[c] != 0 {
                self.wake_flag[c] = 0;
                if self.awake[c] == 0 {
                    to_wake.push(c);
                }
            }
            let [e, _, s, _] = self.nbr[c];
            for nb in [e, s] {
                if nb != NONE && self.wake_flag[nb as usize] != 0 && self.awake[nb as usize] == 0 {
                    self.wake_flag[nb as usize] = 0;
                    to_wake.push(nb as usize);
                }
            }
        }
        for c in to_wake {
            if self.awake[c] == 0 && self.pools.pooled[c] == 0 { self.wake_face += 1; self.why[c][0] += 1; }
            self.wake(c);
        }
        // a pooled chunk's edge faces belong to its pool's account, not its own
        if self.pools.any {
            for k in 0..self.active.len() {
                let c = self.active[k] as usize;
                if self.pools.pooled[c] == 1 { self.rate[c] = 0.0; self.owed[c] = 0.0; self.wake_flag[c] = 0; }
            }
        }
    }

    /// A chunk falling asleep takes each of its faces at its recent average (a steady flow's own),
    /// so it doesn't freeze a wobble's instant; a sleeping neighbour's imbalance takes the change.
    fn faces_to_mean(&mut self, c: usize, w: u32, n: u32, t: f64) {
        let [e, _, s, _] = self.nbr[c];
        let b = c * CA;
        let charge = |sim: &mut Sim, nb: u32, d: f64| {
            if nb == NONE { return; }
            let nb = nb as usize;
            if sim.awake[nb] != 0 { return; }
            sim.owed[nb] += sim.rate[nb] * (t - sim.t_rate[nb]);
            sim.t_rate[nb] = t;
            sim.rate[nb] += d;
        };
        for z in 0..CS {
            for x in 0..CS {
                let i = b + z * CS + x;
                let d = self.ex[i] - self.qx[i]; self.qx[i] = self.ex[i];
                if x == CS - 1 { charge(self, e, d as f64); }
                let d = self.ez[i] - self.qz[i]; self.qz[i] = self.ez[i];
                if z == CS - 1 { charge(self, s, d as f64); }
            }
        }
        if w != NONE { for z in 0..CS { let i = w as usize * CA + z * CS + CS - 1; let d = self.ex[i] - self.qx[i]; self.qx[i] = self.ex[i]; charge(self, w, -(d as f64)); } }
        if n != NONE { for x in 0..CS { let i = n as usize * CA + (CS - 1) * CS + x; let d = self.ez[i] - self.qz[i]; self.qz[i] = self.ez[i]; charge(self, n, -(d as f64)); } }
    }

    /// A chunk falling asleep takes each column at its recent average level, its water unchanged (the
    /// difference spread over its wet columns): it doesn't freeze a wave's crest or trough to release
    /// later as a new wave.
    fn depths_to_mean(&mut self, c: usize) {
        let b = c * CA;
        let (mut now, mut mean, mut n) = (0.0f64, 0.0f64, 0usize);
        for l in 0..CA {
            let i = b + l;
            if self.pools.of[i] >= 0 || self.ground[i] >= VOID { continue; }
            now += self.depth[i] as f64; mean += self.ed[i].max(0.0) as f64;
            if self.ed[i] > 0.0 { n += 1; }
        }
        if n == 0 || mean <= 0.0 { return; }
        let k = (now / mean) as f32; // keep the chunk's water exact
        for l in 0..CA {
            let i = b + l;
            if self.pools.of[i] >= 0 || self.ground[i] >= VOID { continue; }
            self.depth[i] = self.ed[i].max(0.0) * k;
        }
    }

    pub fn totals(&mut self) {
        self.brought = if self.spring_sleep { self.brought_acc } else { self.chunk_in.iter().sum() };
        self.taken = self.chunk_out.iter().sum::<f64>() + (0..self.nch).filter(|&c| self.awake[c] == 0).map(|c| self.sink_out[c] * (self.t - self.t_sleep[c])).sum::<f64>();
    }
}

include!("pools.rs");
