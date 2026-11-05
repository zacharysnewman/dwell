// Still water as pools (the lab's §5.8 pattern, for lakes): a flat, slow body becomes one pool with
// one level from its volume (sorted grounds, prefix sums). A chunk wholly inside pools is `pooled`:
// never stepped, never woken. A chunk holding or touching a pool's column (`shore`) stays awake and
// computes the faces between the pool and the grid as usual, against the pool's level (its members'
// depths there kept in step); the faces inside a pool carry nothing. Each substep the pools' outflow
// is limited by their volume, and their volume takes the net of their faces. Included into sim.rs.

pub const POOL_EVERY: f64 = 10.0;
pub const POOL_MIN: usize = 64;
pub const POOL_DEPTH: f32 = 0.05;
pub const POOL_RATE: f32 = 0.01;
pub static mut POOL_V: f32 = 0.05;
pub static mut POOL_BAND: f32 = 0.05;

pub struct Pool {
    pub members: Vec<u32>,
    g: Vec<f32>,
    pre: Vec<f64>,
    pub v: f64,
    pub l: f32,
    /// members whose depth others read: those in shore chunks, or on a pooled chunk's edge
    sync: Vec<u32>,
    sync_min_g: f32,
    /// members beside a column not in this pool (where it grows from)
    bnd: Vec<u32>,
    alive: bool,
}

impl Pool {
    fn level(&self) -> f32 {
        let (g, pre, v) = (&self.g, &self.pre, self.v);
        let n = g.len();
        if v <= 0.0 {
            return g[0];
        }
        let (mut lo, mut hi) = (0usize, n - 1);
        while lo < hi {
            let m = (lo + hi + 1) / 2;
            if m as f64 * g[m] as f64 - pre[m] <= v { lo = m } else { hi = m - 1 }
        }
        ((v + pre[lo + 1]) / (lo + 1) as f64) as f32
    }
}

pub struct Pools {
    pub on: bool,
    pub of: Vec<i32>,
    pub list: Vec<Pool>,
    pub pooled: Vec<u8>,
    pub shore: Vec<u8>,
    /// faces between a pool and anything else (2·column + axis)
    pub faces: Vec<u32>,
    /// faces inside a pool whose owner chunk is stepped (a shore chunk, or a pooled one beside one): kept at 0
    pub inside: Vec<u32>,
    out: Vec<f64>,
    pub scale: Vec<f32>,
    net: Vec<f64>,
    pub clock: f64,
    pub deep: f32,
    pub formed: u64,
    pub dissolved: u64,
    pub any: bool,
    pub tm: [f64; 5],
    /// springs' and outlets' columns (never pooled), and a visit mark per column
    fixed: Vec<u8>,
    mark: Vec<u8>,
    marked: Vec<u32>,
}

impl Pools {
    pub fn new(n: usize, nch: usize) -> Self {
        Pools { on: false, of: vec![-1; n], list: Vec::new(), pooled: vec![0; nch], shore: vec![0; nch], faces: Vec::new(), inside: Vec::new(), out: Vec::new(), scale: Vec::new(), net: Vec::new(), clock: 0.0, deep: 0.0, formed: 0, dissolved: 0, any: false, tm: [0.0; 5], fixed: Vec::new(), mark: vec![0; n], marked: Vec::new() }
    }
}

impl Sim {
    /// The water stored, pools by their volume.
    pub fn stored_all(&mut self) -> f64 {
        self.sync_pools_all();
        self.stored()
    }

    pub fn sync_pools_all(&mut self) {
        for p in &self.pools.list {
            for &i in &p.members {
                self.depth[i as usize] = (p.l - self.ground[i as usize]).max(0.0);
            }
        }
    }

    fn sync_pools_edges(&mut self) {
        let mut deep = 0.0f32;
        let depth = Ptr(self.depth.as_mut_ptr());
        let ground = &self.ground;
        for p in &self.pools.list {
            let l = p.l;
            p.sync.par_iter().with_min_len(4096).for_each(|&i| unsafe { *depth.at(i as usize) = (l - ground[i as usize]).max(0.0); });
            deep = deep.max(p.l - p.sync_min_g);
        }
        self.pools.deep = deep;
    }

    /// After phase 1: faces inside a pool carry nothing.
    pub(crate) fn pools_zero_inside(&mut self) {
        if !self.pools.any { return; }
        let (nqx, nqz) = (Ptr(self.nqx.as_mut_ptr()), Ptr(self.nqz.as_mut_ptr()));
        self.pools.inside.par_iter().with_min_len(4096).for_each(|&f| unsafe {
            if f & 1 == 1 { *nqz.at((f >> 1) as usize) = 0.0; } else { *nqx.at((f >> 1) as usize) = 0.0; }
        });
    }

    /// Before phase 2's scales are used: each pool's outflow limited by its volume.
    pub(crate) fn pools_limit(&mut self, kdt: f32) {
        if !self.pools.any { return; }
        let np = self.pools.list.len();
        self.pools.out.clear(); self.pools.out.resize(np, 0.0);
        for &f in &self.pools.faces {
            let i = (f >> 1) as usize;
            let j = if f & 1 == 1 { step_col(&self.nbr, i, 2) } else { step_col(&self.nbr, i, 0) };
            let nq = if f & 1 == 1 { self.nqz[i] } else { self.nqx[i] };
            let (pi, pj) = (self.pools.of[i], if j != usize::MAX { self.pools.of[j] } else { -1 });
            if nq > 0.0 && pi >= 0 { self.pools.out[pi as usize] += nq as f64; } else if nq < 0.0 && pj >= 0 { self.pools.out[pj as usize] -= nq as f64; }
        }
        self.pools.scale.clear(); self.pools.scale.resize(np, 1.0);
        for p in 0..np {
            let o = self.pools.out[p] * kdt as f64;
            if o > self.pools.list[p].v { self.pools.scale[p] = (self.pools.list[p].v / o) as f32; }
            let s = self.pools.scale[p];
            for &i in &self.pools.list[p].sync { self.scale[i as usize] = s; }
        }
    }

    /// After phase 4: each pool's volume takes the net of its faces; its level follows.
    pub(crate) fn pools_update(&mut self, dt: f32) {
        if !self.pools.any { return; }
        let np = self.pools.list.len();
        self.pools.net.clear(); self.pools.net.resize(np, 0.0);
        for &f in &self.pools.faces {
            let i = (f >> 1) as usize;
            let j = if f & 1 == 1 { step_col(&self.nbr, i, 2) } else { step_col(&self.nbr, i, 0) };
            let q = if f & 1 == 1 { self.qz[i] } else { self.qx[i] } as f64;
            let pi = self.pools.of[i];
            if pi >= 0 { self.pools.net[pi as usize] -= q; }
            if j != usize::MAX { let pj = self.pools.of[j]; if pj >= 0 { self.pools.net[pj as usize] += q; } }
        }
        let mut gone = false;
        for p in 0..np {
            let pool = &mut self.pools.list[p];
            pool.v = (pool.v + self.pools.net[p] * dt as f64).max(0.0);
            let l = pool.level();
            if ((l - pool.l) / dt).abs() > POOL_RATE { pool.alive = false; gone = true; }
            pool.l = l;
        }
        self.sync_pools_edges();
        if gone {
            self.sync_pools_all();
            let dead: Vec<usize> = (0..np).filter(|&p| !self.pools.list[p].alive).collect();
            self.pools.dissolved += dead.len() as u64;
            self.rebuild_pools();
        }
    }

    fn pool_candidate(&self, i: usize) -> bool {
        if self.pools.of[i] >= 0 || self.ground[i] >= VOID || self.depth[i] < POOL_DEPTH || self.pools.fixed[i] == 1 { return false; }
        let mut q = self.qx[i].abs().max(self.qz[i].abs());
        let w = step_col(&self.nbr, i, 1); if w != usize::MAX { q = q.max(self.qx[w].abs()); }
        let n = step_col(&self.nbr, i, 3); if n != usize::MAX { q = q.max(self.qz[n].abs()); }
        q / self.depth[i] < unsafe { POOL_V }
    }

    /// Every POOL_EVERY: grow pools by matching neighbours, form new ones in awake chunks.
    pub(crate) fn pools_detect(&mut self) {
        let band = unsafe { POOL_BAND };
        if self.pools.fixed.is_empty() {
            let mut fixed = vec![0u8; self.nch * CA];
            for c in 0..self.nch { for &l in &self.sinks[c] { fixed[c * CA + l as usize] = 1; } }
            for (i, &q) in self.inflow.iter().enumerate() { if q != 0.0 { fixed[i] = 1; } }
            self.pools.fixed = fixed;
        }
        // clear last check's marks
        let marked = std::mem::take(&mut self.pools.marked);
        for &i in &marked { self.pools.mark[i as usize] = 0; }
        let mut marked = marked; marked.clear();
        let mut changed = false;
        fn grow(sim: &Sim, mark: &mut [u8], marked: &mut Vec<u32>, seeds: &[usize], level: f32, band: f32, out: &mut Vec<usize>) {
            let mut stack: Vec<usize> = seeds.to_vec();
            while let Some(i) = stack.pop() {
                for d in 0..4 {
                    let j = step_col(&sim.nbr, i, d);
                    if j == usize::MAX || mark[j] == 1 || !sim.pool_candidate(j) { continue; }
                    if (sim.ground[j] + sim.depth[j] - level).abs() > band { continue; }
                    mark[j] = 1; marked.push(j as u32); out.push(j); stack.push(j);
                }
            }
        }
        let mut mark = std::mem::take(&mut self.pools.mark);
        // grow, from each pool's edge
        for p in 0..self.pools.list.len() {
            let seeds: Vec<usize> = self.pools.list[p].bnd.iter().map(|&i| i as usize).collect();
            let mut add = Vec::new();
            let l = self.pools.list[p].l;
            grow(self, &mut mark, &mut marked, &seeds, l, band, &mut add);
            if !add.is_empty() {
                self.wake_cols(&add);
                let pool = &mut self.pools.list[p];
                for &i in &add { pool.v += self.depth[i] as f64; pool.members.push(i as u32); }
                changed = true;
            }
        }
        // form, in awake chunks
        let awake: Vec<usize> = (0..self.nch).filter(|&c| self.awake[c] != 0 && self.pools.pooled[c] == 0).collect();
        for c in awake {
            for l in 0..CA {
                let i = c * CA + l;
                if mark[i] == 1 || !self.pool_candidate(i) { continue; }
                mark[i] = 1; marked.push(i as u32);
                let mut region = vec![i];
                let lev = self.ground[i] + self.depth[i];
                grow(self, &mut mark, &mut marked, &[i], lev, band, &mut region);
                if region.len() < POOL_MIN { continue; }
                self.wake_cols(&region);
                let v: f64 = region.iter().map(|&j| self.depth[j] as f64).sum();
                self.pools.list.push(Pool { members: region.iter().map(|&j| j as u32).collect(), g: Vec::new(), pre: Vec::new(), v, l: 0.0, sync: Vec::new(), sync_min_g: 0.0, bnd: Vec::new(), alive: true });
                self.pools.formed += 1;
                changed = true;
            }
        }
        self.pools.mark = mark;
        self.pools.marked = marked;
        if changed { self.rebuild_pools(); }
    }

    fn wake_cols(&mut self, cols: &[usize]) {
        let mut cs: Vec<usize> = cols.iter().map(|&i| i / CA).collect();
        cs.sort_unstable(); cs.dedup();
        for c in cs { if self.pools.pooled[c] == 0 { self.wake(c); } }
    }

    /// Ids, sorted grounds, the faces between pools and the rest, pooled and shore chunks.
    pub fn rebuild_pools(&mut self) {
        // water a chunk still holds in its owed or incoming account goes to a pool it becomes part of
        self.pools.list.retain(|p| p.alive);
        self.pools.of.iter_mut().for_each(|v| *v = -1);
        for (k, p) in self.pools.list.iter().enumerate() { for &i in &p.members { self.pools.of[i as usize] = k as i32; } }
        // pooled: every column not a wall is in a pool; shore: holds or touches a pool's column
        let mut has = vec![0u8; self.nch];
        for p in &self.pools.list { for &i in &p.members { has[i as usize / CA] = 1; } }
        for c in 0..self.nch {
            let b = c * CA;
            let full = has[c] == 1 && (0..CA).all(|l| self.ground[b + l] >= VOID || self.pools.of[b + l] >= 0);
            let was = self.pools.pooled[c];
            self.pools.pooled[c] = full as u8;
            if full && was == 0 {
                // it joins its pool for good: what it owed or was taking in goes to the pool
                let k = (0..CA).map(|l| self.pools.of[b + l]).find(|&k| k >= 0).unwrap() as usize;
                let extra = self.owed[c] + self.rate[c] * (self.t - self.t_rate[c]) * (self.awake[c] == 0) as i32 as f64 + self.rel_left[c];
                self.pools.list[k].v += extra;
                self.owed[c] = 0.0; self.rate[c] = 0.0; self.rel_left[c] = 0.0;
                self.awake[c] = 0;
                self.dirty = true;
            }
            if !full && was == 1 { self.awake[c] = 1; self.calm[c] = 0; self.dirty = true; }
        }
        for c in 0..self.nch {
            let touch = has[c] == 1 || self.nbr[c].iter().any(|&nb| nb != NONE && has[nb as usize] == 1);
            self.pools.shore[c] = (touch && self.pools.pooled[c] == 0) as u8;
            if self.pools.shore[c] == 1 && self.awake[c] == 0 { self.wake(c); }
        }
        // faces, sync lists, sorted grounds; inside faces still
        let mut faces = Vec::new();
        let mut inside = Vec::new();
        // a pooled chunk beside a shore chunk may be stepped as its fringe
        let stepped = |sim: &Sim, c: usize| sim.pools.shore[c] == 1 || (sim.pools.pooled[c] == 1 && sim.nbr[c].iter().any(|&nb| nb != NONE && sim.pools.shore[nb as usize] == 1));
        for k in 0..self.pools.list.len() {
            let mut g: Vec<f32> = self.pools.list[k].members.iter().map(|&i| self.ground[i as usize]).collect();
            g.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut pre = vec![0.0f64; g.len() + 1];
            for m in 0..g.len() { pre[m + 1] = pre[m] + g[m] as f64; }
            let mut sync = Vec::new();
            let mut bnd = Vec::new();
            let mut ming = f32::INFINITY;
            for &i in &self.pools.list[k].members {
                let i = i as usize;
                let c = i / CA;
                let own_stepped = stepped(self, c);
                let mut boundary = false;
                for d in 0..4 {
                    let j = step_col(&self.nbr, i, d);
                    if j == usize::MAX { continue; }
                    if self.pools.of[j] != k as i32 {
                        boundary = true;
                        if d == 0 { faces.push((2 * i) as u32); }
                        if d == 2 { faces.push((2 * i + 1) as u32); }
                        if d == 1 { faces.push((2 * j) as u32); }
                        if d == 3 { faces.push((2 * j + 1) as u32); }
                    } else {
                        if d == 0 { self.qx[i] = 0.0; self.nqx[i] = 0.0; if own_stepped { inside.push((2 * i) as u32); } }
                        if d == 2 { self.qz[i] = 0.0; self.nqz[i] = 0.0; if own_stepped { inside.push((2 * i + 1) as u32); } }
                    }
                }
                if boundary { bnd.push(i as u32); }
                let _ = c;
                if boundary {
                    sync.push(i as u32);
                    ming = ming.min(self.ground[i]);
                }
            }
            let p = &mut self.pools.list[k];
            p.g = g; p.pre = pre; p.sync = sync; p.sync_min_g = ming; p.bnd = bnd;
            p.l = p.level();
        }
        faces.sort_unstable(); faces.dedup();
        self.pools.faces = faces;
        self.pools.inside = inside;
        self.pools.any = !self.pools.list.is_empty();
        self.sync_pools_all();
        self.dirty = true;
    }
}
