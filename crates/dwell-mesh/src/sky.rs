//! Ambient occlusion baked at mesh time (docs/DAY_NIGHT.md §10; data: shared/look/ambient.json):
//! the classic voxel corner occlusion (three cells around a face's corner, levels 0–3) and the
//! sky visibility, the share of short rays from a vertex that escape through the voxel grid.
//! Both read an [`Occupancy`]: which cells block light, in the chunk's face-local frame (a face-B
//! chunk is meshed mirrored, so +y is always toward the face's sky), over the chunk and its apron
//! ([`dwell_world::SKY_APRON`] for the client's jobs; the plain one-voxel apron leaves rays past it
//! open).
//!
//! Every vertex's rays are a fixed cosine-weighted set turned about the normal by a hash of the
//! vertex's face-local world position, and the walk runs on cell-relative coordinates, so a
//! vertex on a chunk border gets the same value from either chunk (the reach stays inside both
//! chunks' aprons).

use std::sync::OnceLock;

use dwell_look::ambient::{AMBIENT, AmbientTunables};
use dwell_world::CELL_H;

/// The corner-occlusion level of the open corner (no occluder around it).
pub(crate) const AO_OPEN: u8 = 3;

/// Which cells block light (solid, non-see-through full cubes), chunk-local cells, face-local.
pub(crate) struct Occupancy {
    /// The volume's edges and its apron (cells), x, y, z.
    ext: [i32; 3],
    apron: [i32; 3],
    bits: Vec<u64>,
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
                let layer = (ext[0] * ext[2]) as usize;
                let (y, rest) = (i / layer, i % layer);
                (ext[1] as usize - 1 - y) * layer + rest
            } else {
                i
            };
            bits[i / 64] |= 1 << (i % 64);
        }
        Self { ext, apron, bits }
    }

    /// Whether chunk-local cell `c` blocks light (outside the volume: open).
    pub(crate) fn solid(&self, c: [i32; 3]) -> bool {
        let p = [
            c[0] + self.apron[0],
            c[1] + self.apron[1],
            c[2] + self.apron[2],
        ];
        if (0..3).any(|a| p[a] < 0 || p[a] >= self.ext[a]) {
            return false;
        }
        let i = (p[0] + self.ext[0] * (p[2] + self.ext[2] * p[1])) as usize;
        self.bits[i / 64] >> (i % 64) & 1 != 0
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

    /// How many of the sky rays (`AMBIENT.rays`) from the point `cell + frac` (chunk-local
    /// cells; `frac` in [0, 1] per axis) on a surface with unit normal `n` (metres) escape.
    /// `world` is the point's face-local world position in eighths of a cell (with the normal's
    /// own salt), which turns its ray set.
    pub(crate) fn visible_rays(
        &self,
        cell: [i32; 3],
        frac: [f64; 3],
        n: [f64; 3],
        world: [i64; 4],
    ) -> u8 {
        visible_rays_with(self, &AMBIENT, rays(), cell, frac, n, world)
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

/// How far the start is nudged off the surface and along the ray (m), so the walk starts in the
/// cell the ray leaves into.
const NUDGE: f64 = 1e-3;

pub(crate) fn visible_rays_with(
    occ: &Occupancy,
    t: &AmbientTunables,
    set: &[[f64; 3]],
    cell: [i32; 3],
    frac: [f64; 3],
    n: [f64; 3],
    world: [i64; 4],
) -> u8 {
    if set.is_empty() {
        return 0;
    }
    // A tangent frame about the normal, turned by the point's hash.
    let helper = if n[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t1 = unit(cross(helper, n));
    let t2 = cross(n, t1);
    let turn = point_noise(world) * std::f64::consts::TAU;
    let (c, s) = (turn.cos(), turn.sin());
    let mut escaped = 0u8;
    for d in set {
        let (a, b) = (d[0] * c - d[1] * s, d[0] * s + d[1] * c);
        let mut dir = [
            t1[0] * a + t2[0] * b + n[0] * d[2],
            t1[1] * a + t2[1] * b + n[1] * d[2],
            t1[2] * a + t2[2] * b + n[2] * d[2],
        ];
        // Toward the face's sky: a ray below the face-local horizon is mirrored above it.
        dir[1] = dir[1].abs();
        if ray_escapes(occ, t, cell, frac, n, dir) {
            escaped += 1;
        }
    }
    escaped
}

/// Whether a ray from `cell + frac` (cells) along `dir` (unit, metres) runs `length_m` without
/// entering a cell that blocks light, and nothing blocks the probe above its end.
fn ray_escapes(
    occ: &Occupancy,
    t: &AmbientTunables,
    cell: [i32; 3],
    frac: [f64; 3],
    n: [f64; 3],
    dir: [f64; 3],
) -> bool {
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
    let at = |r: [i32; 3]| [cell[0] + r[0], cell[1] + r[1], cell[2] + r[2]];
    loop {
        if occ.solid(at(cur)) {
            return false;
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
    let end = [
        (s[0] + dc[0] * t.length_m).floor() as i32,
        end_y.floor() as i32,
        (s[2] + dc[2] * t.length_m).floor() as i32,
    ];
    for y in end[1] + 1..=top {
        if occ.solid(at([end[0], y, end[2]])) {
            return false;
        }
    }
    true
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

/// A face corner's merge key: its level and ray count in one byte.
pub(crate) fn corner_key(level: u8, escaped: u8) -> u32 {
    u32::from(level) | (u32::from(escaped) << 2)
}

/// The light of a corner key.
pub(crate) fn key_light(key: u32) -> f32 {
    vertex_light((key & 3) as u8, (key >> 2) as u8)
}
