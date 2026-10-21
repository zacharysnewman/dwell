//! Ambient occlusion baked at mesh time (docs/DAY_NIGHT.md §9; data: shared/look/ambient.json):
//! the classic voxel corner occlusion (three cells around a face's corner, levels 0–3) and the
//! sky visibility, the share of short rays from a vertex that escape through the voxel grid.
//! Both read an [`Occupancy`]: which cells block light, in the chunk's face-local frame (a face-B
//! chunk is meshed mirrored, so +y is always toward the face's sky), over the chunk and its apron
//! ([`dwell_world::SKY_APRON`] for the client's jobs; the plain one-voxel apron leaves rays past it
//! open).
//!
//! Each vertex's rays are a cosine-weighted set turned about its normal by a hash of its
//! face-local world position, walked on cell-relative coordinates, so a vertex on a chunk border
//! gets the same value from either chunk (the reach stays inside both chunks' aprons). The faces of
//! cells have their vertices on the cell lattice: for them the turn is one of [`TURNS`] and each
//! ray's cells are walked once per mesh into a list of offsets ([`LatticeRays`]), so a vertex costs
//! a few table lookups per ray. Shaped faces and plants, off the lattice, walk their rays.

use std::sync::OnceLock;

use dwell_look::ambient::{AMBIENT, AmbientTunables};
use dwell_world::CELL_H;

/// The corner-occlusion level of the open corner (no occluder around it).
pub(crate) const AO_OPEN: u8 = 3;

/// How many turns of the ray set a lattice vertex picks from (by its hash).
pub(crate) const TURNS: usize = 16;

/// Which cells block light (solid, non-see-through full cubes), chunk-local cells, face-local.
pub(crate) struct Occupancy {
    /// The volume's edges and its apron (cells), x, y, z.
    ext: [i32; 3],
    apron: [i32; 3],
    bits: Vec<u64>,
    /// Per column of the volume (x fastest, then z): the highest light-blocking row (the volume's
    /// own y) in any column within [`reach`] of it, or −1. A ray above it has nothing left to
    /// hit, which keeps rays over open ground short.
    reach_top: Vec<i32>,
}

/// How far (cells, across) a ray's columns can be from the column it starts in: its length and
/// the start's nudge.
fn reach() -> i32 {
    AMBIENT.length_m.ceil() as i32 + 2
}

impl Occupancy {
    /// From a block of `ext` cells with `apron` cells around the chunk, x fastest, then z, then y;
    /// `blocks(id)` says whether a state blocks light. `mirror` turns it upside down (face B's
    /// chunks are meshed in their face-local frame).
    pub(crate) fn new(
        data: &[u16],
        ext: [i32; 3],
        apron: [i32; 3],
        mirror: bool,
        blocks: impl Fn(u16) -> bool,
    ) -> Self {
        let len = (ext[0] * ext[1] * ext[2]) as usize;
        let layer = (ext[0] * ext[2]) as usize;
        let mut bits = vec![0u64; len.div_ceil(64)];
        // Blocking is per state: look each one up once.
        let mut memo: Vec<u8> = Vec::new();
        for (i, &id) in data.iter().enumerate().take(len) {
            if id == 0 {
                continue;
            }
            let k = usize::from(id);
            if memo.len() <= k {
                memo.resize(k + 1, 2);
            }
            if memo[k] == 2 {
                memo[k] = u8::from(blocks(id));
            }
            if memo[k] == 0 {
                continue;
            }
            let i = if mirror {
                let (y, rest) = (i / layer, i % layer);
                (ext[1] as usize - 1 - y) * layer + rest
            } else {
                i
            };
            bits[i / 64] |= 1 << (i % 64);
        }
        let mut occ = Self {
            ext,
            apron,
            bits,
            reach_top: Vec::new(),
        };
        occ.reach_top = occ.reach_tops(reach());
        occ
    }

    fn bit(&self, i: usize) -> bool {
        self.bits[i / 64] >> (i % 64) & 1 != 0
    }

    /// `reach_top` for reach `r`: each column's top, then the maximum over the square of
    /// columns within `r` (along x, then along z).
    fn reach_tops(&self, r: i32) -> Vec<i32> {
        let [ex, ey, ez] = self.ext;
        let mut top = vec![-1; (ex * ez) as usize];
        for z in 0..ez {
            for x in 0..ex {
                if let Some(y) = (0..ey)
                    .rev()
                    .find(|&y| self.bit((x + ex * (z + ez * y)) as usize))
                {
                    top[(x + ex * z) as usize] = y;
                }
            }
        }
        let max_along = |src: &[i32], along_x: bool| {
            let (len, other) = if along_x { (ex, ez) } else { (ez, ex) };
            let at = |i: i32, o: i32| {
                if along_x {
                    (i + ex * o) as usize
                } else {
                    (o + ex * i) as usize
                }
            };
            let mut out = vec![-1; src.len()];
            for o in 0..other {
                for i in 0..len {
                    let (lo, hi) = ((i - r).max(0), (i + r).min(len - 1));
                    out[at(i, o)] = (lo..=hi).map(|k| src[at(k, o)]).max().unwrap_or(-1);
                }
            }
            out
        };
        max_along(&max_along(&top, true), false)
    }

    /// The volume coordinates of chunk-local cell `c`, if inside it.
    fn inside(&self, c: [i32; 3]) -> Option<[i32; 3]> {
        let p = [
            c[0] + self.apron[0],
            c[1] + self.apron[1],
            c[2] + self.apron[2],
        ];
        (0..3).all(|a| p[a] >= 0 && p[a] < self.ext[a]).then_some(p)
    }

    fn index(&self, p: [i32; 3]) -> i32 {
        p[0] + self.ext[0] * (p[2] + self.ext[2] * p[1])
    }

    /// Whether chunk-local cell `c` blocks light (outside the volume: open).
    pub(crate) fn solid(&self, c: [i32; 3]) -> bool {
        self.inside(c)
            .is_some_and(|p| self.bit(self.index(p) as usize))
    }

    /// The highest blocking row (volume y) within a ray's reach of chunk-local column (x, z); −1
    /// past the volume.
    fn top_near(&self, x: i32, z: i32) -> i32 {
        let (px, pz) = (x + self.apron[0], z + self.apron[2]);
        if px < 0 || pz < 0 || px >= self.ext[0] || pz >= self.ext[2] {
            return -1;
        }
        self.reach_top[(px + self.ext[0] * pz) as usize]
    }

    /// The classic corner-occlusion level (0 a full crease … 3 open) of the corner of an
    /// axis-aligned face whose front cell (the cell the face looks into) is `front`, toward
    /// `du` along `u` and `dv` along `v` (axes; ±1 each).
    pub(crate) fn corner_ao(&self, front: [i32; 3], u: usize, du: i32, v: usize, dv: i32) -> u8 {
        let at = |a: i32, b: i32| {
            let mut c = front;
            c[u] += a;
            c[v] += b;
            self.solid(c)
        };
        let (s1, s2, corner) = (at(du, 0), at(0, dv), at(du, dv));
        if s1 && s2 {
            0
        } else {
            AO_OPEN - u8::from(s1) - u8::from(s2) - u8::from(corner)
        }
    }

    /// How many of the sky rays from the point `cell + frac` (chunk-local cells; `frac` in
    /// [0, 1] per axis) on a surface with unit normal `n` (metres) escape. `world` is the
    /// point's face-local world position in eighths of a cell, with the surface's salt, which
    /// turns its ray set.
    pub(crate) fn visible_rays(
        &self,
        cell: [i32; 3],
        frac: [f64; 3],
        n: [f64; 3],
        world: [i64; 4],
    ) -> u8 {
        let t = &AMBIENT;
        let ceiling = self.top_near(
            cell[0] + frac[0].floor() as i32,
            cell[2] + frac[2].floor() as i32,
        );
        let turn = point_noise(world) * std::f64::consts::TAU;
        let mut escaped = 0u8;
        for d in rays() {
            let dir = turned(*d, n, turn);
            let mut blocked = false;
            walk(t, frac, n, dir, |r| {
                let c = [cell[0] + r[0], cell[1] + r[1], cell[2] + r[2]];
                match self.inside(c) {
                    // Out of the volume, or above every blocking row within reach: nothing on
                    // (rays never descend, and an axis never turns back).
                    None => false,
                    Some(p) if p[1] > ceiling => false,
                    Some(p) => {
                        blocked = self.bit(self.index(p) as usize);
                        !blocked
                    }
                }
            });
            escaped += u8::from(!blocked);
        }
        escaped
    }

    /// [`Self::visible_rays`] for a vertex on the cell lattice (chunk-local `at`) of an
    /// axis-aligned face `face` (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z), its ray set's turn picked by
    /// `world` (as `visible_rays`'s): the cells of its rays from `paths`.
    pub(crate) fn lattice_rays(
        &self,
        paths: &LatticeRays,
        at: [i32; 3],
        face: usize,
        world: [i64; 4],
    ) -> u8 {
        let turn = ((point_noise(world) * TURNS as f64) as usize).min(TURNS - 1);
        let rays = &paths.rays[face * TURNS + turn];
        let Some(p) = self.inside(at) else {
            return rays.len() as u8;
        };
        let ceiling = self.top_near(at[0], at[2]);
        // Inside the volume whatever the ray (the sky apron): the walk needs no bounds checks.
        let fits = (0..3).all(|a| p[a] - paths.lo[a] >= 0 && p[a] + paths.hi[a] < self.ext[a]);
        let base = self.index(p);
        let mut escaped = 0u8;
        for r in rays {
            let mut open = true;
            for s in &paths.steps[r.clone()] {
                if p[1] + s.dy > ceiling {
                    break;
                }
                let hit = if fits {
                    self.bit((base + s.delta) as usize)
                } else {
                    self.solid([at[0] + s.off[0], at[1] + s.dy, at[2] + s.off[1]])
                };
                if hit {
                    open = false;
                    break;
                }
            }
            escaped += u8::from(open);
        }
        escaped
    }

    /// The cell lists of the lattice vertices' rays for this volume's layout (walked once per
    /// layout: the sky volume's and the one-voxel block's).
    pub(crate) fn lattice_paths(&self) -> &'static LatticeRays {
        static SKY: OnceLock<LatticeRays> = OnceLock::new();
        static PADDED: OnceLock<LatticeRays> = OnceLock::new();
        static OTHER: OnceLock<std::sync::Mutex<Vec<&'static LatticeRays>>> = OnceLock::new();
        let make = || LatticeRays::new(&AMBIENT, rays(), self.ext);
        if self.ext == dwell_world::SKY_EXTENT {
            SKY.get_or_init(make)
        } else if self.ext == [crate::PAD as i32; 3] {
            PADDED.get_or_init(make)
        } else {
            let mut other = OTHER
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(p) = other.iter().find(|p| p.ext == self.ext) {
                return p;
            }
            let p: &'static LatticeRays = Box::leak(Box::new(make()));
            other.push(p);
            p
        }
    }
}

/// One cell of a lattice ray's walk: its offset from the vertex's cell (x, z), its rise, and its
/// offset in the volume's flat index.
#[derive(Clone, Copy, Debug)]
struct Step {
    off: [i32; 2],
    dy: i32,
    delta: i32,
}

/// Every lattice vertex's rays as cell lists, per face and turn: walked once for a volume layout.
pub(crate) struct LatticeRays {
    steps: Vec<Step>,
    /// Per face × turn, the ranges of `steps` of each ray.
    rays: Vec<Vec<std::ops::Range<usize>>>,
    /// How far the lists reach below and above the vertex's cell (cells, x y z).
    lo: [i32; 3],
    hi: [i32; 3],
    /// The volume layout the flat offsets are for.
    ext: [i32; 3],
}

impl LatticeRays {
    fn new(t: &AmbientTunables, set: &[[f64; 3]], ext: [i32; 3]) -> Self {
        let mut steps = Vec::new();
        let mut rays = Vec::with_capacity(6 * TURNS);
        let (mut lo, mut hi) = ([0; 3], [0; 3]);
        for face in 0..6 {
            let mut n = [0.0; 3];
            n[face / 2] = if face % 2 == 0 { 1.0 } else { -1.0 };
            for k in 0..TURNS {
                let turn = (k as f64 + 0.5) / TURNS as f64 * std::f64::consts::TAU;
                let mut list = Vec::with_capacity(set.len());
                for d in set {
                    let start = steps.len();
                    walk(t, [0.0; 3], n, turned(*d, n, turn), |r| {
                        for a in 0..3 {
                            lo[a] = lo[a].max(-r[a]);
                            hi[a] = hi[a].max(r[a]);
                        }
                        steps.push(Step {
                            off: [r[0], r[2]],
                            dy: r[1],
                            delta: r[0] + ext[0] * (r[2] + ext[2] * r[1]),
                        });
                        true
                    });
                    list.push(start..steps.len());
                }
                rays.push(list);
            }
        }
        Self {
            steps,
            rays,
            lo,
            hi,
            ext,
        }
    }
}

/// The cosine-weighted ray set about +z (Fibonacci points on the unit disc lifted to the
/// hemisphere, Malley's method), for `AMBIENT.rays`.
fn rays() -> &'static [[f64; 3]] {
    static RAYS: OnceLock<Vec<[f64; 3]>> = OnceLock::new();
    RAYS.get_or_init(|| ray_set(AMBIENT.rays))
}

pub(crate) fn ray_set(count: u32) -> Vec<[f64; 3]> {
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    (0..count)
        .map(|i| {
            let r = ((f64::from(i) + 0.5) / f64::from(count)).sqrt();
            let phi = f64::from(i) * golden;
            [r * phi.cos(), r * phi.sin(), (1.0 - r * r).max(0.0).sqrt()]
        })
        .collect()
}

/// A hash of a face-local world point to [0, 1): the turn of its ray set.
fn point_noise(p: [i64; 4]) -> f64 {
    let mut h = (p[0] as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (p[1] as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (p[2] as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
        ^ (p[3] as u64).wrapping_mul(0x27d4_eb2f_1656_67c5);
    h ^= h >> 31;
    h = h.wrapping_mul(0x7fb5_d329_728e_a185);
    h ^= h >> 27;
    h = h.wrapping_mul(0x81da_def4_bc2d_d44d);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Ray `d` of the set (about +z) about the normal `n`, turned by `turn` (radians) and toward the
/// face's sky: a ray below the face-local horizon is mirrored above it.
fn turned(d: [f64; 3], n: [f64; 3], turn: f64) -> [f64; 3] {
    let helper = if n[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t1 = unit(cross(helper, n));
    let t2 = cross(n, t1);
    let (c, s) = (turn.cos(), turn.sin());
    let (a, b) = (d[0] * c - d[1] * s, d[0] * s + d[1] * c);
    let mut dir = [
        t1[0] * a + t2[0] * b + n[0] * d[2],
        t1[1] * a + t2[1] * b + n[1] * d[2],
        t1[2] * a + t2[2] * b + n[2] * d[2],
    ];
    dir[1] = dir[1].abs();
    dir
}

/// How far the start is nudged off the surface and along the ray (m), so the walk starts in the
/// cell the ray leaves into.
const NUDGE: f64 = 1e-3;

/// Walks a ray from `frac` (cells, relative to its cell) along `dir` (unit, metres, never
/// descending): every cell it enters within `length_m` (a DDA over cells 1 m × CELL_H × 1 m), then
/// the probe's cells straight above its end, as offsets from the start's cell, while `visit`
/// returns true.
fn walk(
    t: &AmbientTunables,
    frac: [f64; 3],
    n: [f64; 3],
    dir: [f64; 3],
    mut visit: impl FnMut([i32; 3]) -> bool,
) {
    let scale = [1.0, 1.0 / CELL_H, 1.0];
    // Per metre along the ray, in cells.
    let dc = [dir[0] * scale[0], dir[1] * scale[1], dir[2] * scale[2]];
    let s: [f64; 3] = std::array::from_fn(|a| frac[a] + (n[a] * NUDGE + dir[a] * NUDGE) * scale[a]);
    let mut cur: [i32; 3] = std::array::from_fn(|a| s[a].floor() as i32);
    let mut step = [0i32; 3];
    let mut t_max = [f64::INFINITY; 3];
    let mut t_delta = [f64::INFINITY; 3];
    for a in 0..3 {
        if dc[a] > 0.0 {
            step[a] = 1;
            t_max[a] = (f64::from(cur[a]) + 1.0 - s[a]) / dc[a];
            t_delta[a] = 1.0 / dc[a];
        } else if dc[a] < 0.0 {
            step[a] = -1;
            t_max[a] = (f64::from(cur[a]) - s[a]) / dc[a];
            t_delta[a] = -1.0 / dc[a];
        }
    }
    loop {
        if !visit(cur) {
            return;
        }
        let a = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };
        if t_max[a] > t.length_m {
            break;
        }
        cur[a] += step[a];
        t_max[a] += t_delta[a];
    }
    // The probe: straight up from the ray's end.
    let end_y = s[1] + dc[1] * t.length_m;
    let top = (end_y + t.probe_m / CELL_H).floor() as i32;
    let end = [cur[0], end_y.floor().max(f64::from(cur[1])) as i32, cur[2]];
    for y in end[1] + 1..=top {
        if !visit([end[0], y, end[2]]) {
            return;
        }
    }
}

/// A vertex's baked light: its corner-occlusion factor × the share of its rays that escaped.
pub(crate) fn vertex_light(level: u8, escaped: u8) -> f32 {
    let vis = if AMBIENT.rays == 0 {
        1.0
    } else {
        f64::from(escaped) / f64::from(AMBIENT.rays)
    };
    (AMBIENT.ao_factor(level) * vis) as f32
}

/// A face corner's merge key: its occlusion level and ray count (bits 0–7), and its flood light,
/// sky and block, each in quarter levels (0–60, 6 bits each; `crate::flood`).
pub(crate) fn corner_key(level: u8, escaped: u8, sky: u8, block: u8) -> u32 {
    u32::from(level) | (u32::from(escaped) << 2) | (u32::from(sky) << 8) | (u32::from(block) << 14)
}

/// The flood light of a corner key: sky and block level as shares of 15.
pub(crate) fn key_flood(key: u32) -> [f32; 2] {
    let q = |shift: u32| ((key >> shift) & 63) as f32 / crate::flood::QUARTERS;
    [q(8), q(14)]
}

/// The light of a corner key.
pub(crate) fn key_light(key: u32) -> f32 {
    vertex_light((key & 3) as u8, ((key >> 2) & 63) as u8)
}

#[cfg(test)]
#[path = "sky_tests.rs"]
mod tests;
