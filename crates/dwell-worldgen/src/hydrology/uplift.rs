//! The uplift layer (docs/proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md §5.1): a per-seed, per-face
//! map of tectonic **strength** that drives the stream-power erosion, after Cordonnier et al. 2016,
//! where the uplift map is the key input and the mountains emerge where it is high. The strength
//! is 0 on the coast's shelf and up to about 1 on a range's spine; it sets the initial ground
//! (`base_height` × strength over the kept relief) and the uplift rate (`rate` × strength).
//!
//! Its parts, all data (`Tectonics`): a floor for the **plains**; **ranges**, long curved spines
//! (random walks of heading from a centre inland) with a smooth profile across and tapered ends;
//! broad **highlands** (low-frequency noise above a threshold); **plates**, Voronoi cells of random
//! points with random motions, whose convergent boundaries rise and divergent ones sink (rifts).
//! The parts combine by maximum (rifts add), and fade to 0 toward the coast over a shelf whose
//! width varies along it, so some coasts are steep and some flat. Rivers are not placed: they
//! follow from where the uplift is high and low.
//!
//! Also here: the **rock bands**, parallel bands of hard rock (lower erodibility).
//!
//! Deterministic like the rest of the module: hashes for the random draws, the integer-lattice
//! noise (`noise::fbm2`) at the cells' integer centres, and only +, −, ×, ÷ and `sqrt` in f64.

use super::flood::Grid;
use super::{Plates, Ranges, RockBands, Tectonics};
use crate::noise::{fbm2, mix32, seed_word};

/// The streams of a face seed this module draws from.
const STREAM: u32 = 0x4859_7000;

/// Draws in [0, 1) from a seed: the `k`th value of a hashed sequence.
struct Draws {
    seed: u32,
    k: u32,
}

impl Draws {
    fn new(seed: u64, stream: u32) -> Self {
        Self {
            seed: seed_word(seed, STREAM + stream),
            k: 0,
        }
    }
    fn next(&mut self) -> f64 {
        self.k += 1;
        f64::from(mix32(self.seed ^ mix32(self.k.wrapping_mul(0x9E37_79B9)))) / 4_294_967_296.0
    }
    /// A uniformly random unit vector (rejection from the square).
    fn direction(&mut self) -> (f64, f64) {
        loop {
            let (x, z) = (2.0 * self.next() - 1.0, 2.0 * self.next() - 1.0);
            let r2 = x * x + z * z;
            if r2 > 1e-4 && r2 <= 1.0 {
                let r = r2.sqrt();
                return (x / r, z / r);
            }
        }
    }
}

fn smooth01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A smooth bump: 1 at 0, 0 from |t| ≥ 1.
fn bump(t: f64) -> f64 {
    if t >= 1.0 {
        0.0
    } else {
        let u = 1.0 - t * t;
        u * u
    }
}

/// Distance (m) from each cell to the nearest sea cell: a two-pass chamfer transform with the
/// exact cell and diagonal steps (within ~8 % of the Euclidean distance).
pub fn coast_distance(grid: Grid, cell: f64, sea: &[bool]) -> Vec<f64> {
    let n = grid.n as i64;
    let diag = cell * std::f64::consts::SQRT_2;
    let mut d: Vec<f64> = sea
        .iter()
        .map(|&s| if s { 0.0 } else { f64::INFINITY })
        .collect();
    let pass = |d: &mut Vec<f64>, offsets: &[(i64, i64, f64)], forward: bool| {
        for k in 0..n * n {
            let i = if forward { k } else { n * n - 1 - k };
            let (x, z) = (i % n, i / n);
            let mut best = d[i as usize];
            for &(dx, dz, w) in offsets {
                let (nx, nz) = (x + dx, z + dz);
                if nx >= 0 && nz >= 0 && nx < n && nz < n {
                    best = best.min(d[(nz * n + nx) as usize] + w);
                }
            }
            d[i as usize] = best;
        }
    };
    pass(
        &mut d,
        &[(-1, 0, cell), (-1, -1, diag), (0, -1, cell), (1, -1, diag)],
        true,
    );
    pass(
        &mut d,
        &[(1, 0, cell), (1, 1, diag), (0, 1, cell), (-1, 1, diag)],
        false,
    );
    d
}

/// A range's spine: a polyline (face-local m), its points' distance along it, and its own
/// strength and half-width.
#[derive(Clone, Debug, PartialEq)]
pub struct Spine {
    pub points: Vec<(f64, f64)>,
    pub along: Vec<f64>,
    pub strength: f64,
    pub width: f64,
}

impl Spine {
    fn length(&self) -> f64 {
        self.along[self.along.len() - 1]
    }
}

/// The uplift layer of one face.
#[derive(Clone, Debug)]
pub struct UpliftMap {
    /// The tectonic strength per cell (0 at sea).
    pub strength: Vec<f64>,
    pub spines: Vec<Spine>,
    /// The plates' points (face-local m), for drawing.
    pub plates: Vec<(f64, f64)>,
}

/// The grid's cell centres as integer metres (for the integer-lattice noise).
fn centre(grid: Grid, cell: f64, x0: f64, i: usize) -> (f64, f64) {
    (
        x0 + (i % grid.n) as f64 * cell,
        x0 + (i / grid.n) as f64 * cell,
    )
}

/// A noise in about [−1, 1] at a face-local point (rounded to whole metres).
fn noise(seed: u32, x: f64, z: f64, wavelength: i32, octaves: u32) -> f64 {
    2.0 * f64::from(fbm2(
        seed,
        x.floor() as i64,
        z.floor() as i64,
        wavelength,
        octaves,
        None,
    ))
}

/// The uplift layer for a face (its generator's seed), over the land of a grid.
pub fn generate(
    grid: Grid,
    cell: f64,
    x0: f64,
    sea: &[bool],
    coast: &[f64],
    seed: u64,
    t: &Tectonics,
) -> UpliftMap {
    let len = grid.len();
    let land: Vec<usize> = (0..len).filter(|&i| !sea[i]).collect();
    // The land's extent: plates' points and range centres are drawn within it.
    let reach = land
        .iter()
        .map(|&i| {
            let (x, z) = centre(grid, cell, x0, i);
            (x * x + z * z).sqrt()
        })
        .fold(cell, f64::max);
    let coast_at = |x: f64, z: f64| -> f64 {
        let col = ((x - x0) / cell + 0.5).floor();
        let row = ((z - x0) / cell + 0.5).floor();
        if col < 0.0 || row < 0.0 || col >= grid.n as f64 || row >= grid.n as f64 {
            return 0.0;
        }
        coast[row as usize * grid.n + col as usize]
    };
    let spines = spines(&t.ranges, seed, reach, &coast_at);
    let plates = plate_points(&t.plates, seed, reach);
    let warp_seeds = (
        seed_word(seed, STREAM + 10),
        seed_word(seed, STREAM + 11),
        seed_word(seed, STREAM + 12),
        seed_word(seed, STREAM + 13),
    );
    let highland_seed = seed_word(seed, STREAM + 14);
    let shelf_seed = seed_word(seed, STREAM + 15);
    let apron = Apron {
        strength: t.apron_strength,
        width: t.apron_width,
    };
    let mut strength = vec![0.0f64; len];
    for &i in &land {
        let (x, z) = centre(grid, cell, x0, i);
        let mut s = t.plains;
        if !spines.is_empty() {
            let r = &t.ranges;
            let (wx, wz) = if r.warp > 0.0 {
                (
                    r.warp * noise(warp_seeds.0, x, z, r.warp_wavelength, 2),
                    r.warp * noise(warp_seeds.1, x, z, r.warp_wavelength, 2),
                )
            } else {
                (0.0, 0.0)
            };
            s = s.max(range_strength(&spines, &apron, x + wx, z + wz));
        }
        let h = &t.highlands;
        if h.strength > 0.0 {
            let v = noise(highland_seed, x, z, h.wavelength, 3);
            let edge = 1.0 - 2.0 * h.cover;
            s = s.max(h.strength * smooth01((v - edge + 0.15) / 0.3));
        }
        if !plates.is_empty() {
            let pl = &t.plates;
            let (wx, wz) = if pl.warp > 0.0 {
                (
                    pl.warp * noise(warp_seeds.2, x, z, pl.warp_wavelength, 2),
                    pl.warp * noise(warp_seeds.3, x, z, pl.warp_wavelength, 2),
                )
            } else {
                (0.0, 0.0)
            };
            let (up, down) = plate_strength(pl, &apron, &plates, x + wx, z + wz);
            s = s.max(up) + down;
        }
        let v = smooth01(0.5 + 0.5 * noise(shelf_seed, x, z, t.shelf_wavelength, 2));
        let shelf = t.shelf_min + (t.shelf_max - t.shelf_min) * v;
        strength[i] = s * smooth01(coast[i] / shelf);
    }
    UpliftMap {
        strength,
        spines,
        plates: plates.iter().map(|p| (p.x, p.z)).collect(),
    }
}

/// The foothills: beyond a range's flank (or a convergent boundary's), the strength falls from
/// `strength` × the spine's at the spine to 0 at `width` m from it, concave ((1 − d/width)²), so the
/// land slopes away from the mountains for tens of kilometres and its rivers gather into long
/// valleys between them.
struct Apron {
    strength: f64,
    width: f64,
}

impl Apron {
    fn at(&self, d: f64) -> f64 {
        if d >= self.width {
            0.0
        } else {
            let u = 1.0 - d / self.width;
            self.strength * u * u
        }
    }
}

/// The ranges' spines: each from a centre between `min_coast` and `max_coast` m from the sea, its
/// heading turned from a random one toward the coast's direction there by `align` (1: along the
/// coast, as a range at an active margin), grown both ways grown both ways in 500 m steps whose heading wanders.
fn spines(r: &Ranges, seed: u64, reach: f64, coast_at: &dyn Fn(f64, f64) -> f64) -> Vec<Spine> {
    const STEP: f64 = 500.0;
    let mut d = Draws::new(seed, 1);
    let mut out = Vec::new();
    for _ in 0..r.count {
        let mut found = None;
        for _ in 0..256 {
            let (x, z) = (
                reach * (2.0 * d.next() - 1.0),
                reach * (2.0 * d.next() - 1.0),
            );
            let c = coast_at(x, z);
            if c >= r.min_coast && c <= r.max_coast {
                found = Some((x, z));
                break;
            }
        }
        let Some((cx, cz)) = found else { continue };
        let random = d.direction();
        // The coast's tangent: across the coast distance's gradient, the sign that keeps
        // `random`'s side.
        let span = 1000.0;
        let gx = coast_at(cx + span, cz) - coast_at(cx - span, cz);
        let gz = coast_at(cx, cz + span) - coast_at(cx, cz - span);
        let g = (gx * gx + gz * gz).sqrt();
        let heading = if g > 0.0 && r.align > 0.0 {
            let (mut tx, mut tz) = (-gz / g, gx / g);
            if tx * random.0 + tz * random.1 < 0.0 {
                (tx, tz) = (-tx, -tz);
            }
            let (hx, hz) = (
                random.0 + (tx - random.0) * r.align,
                random.1 + (tz - random.1) * r.align,
            );
            let l = (hx * hx + hz * hz).sqrt();
            if l > 1e-9 { (hx / l, hz / l) } else { (tx, tz) }
        } else {
            random
        };
        let length = r.length_min + (r.length_max - r.length_min) * d.next();
        let strength = r.strength * (0.75 + 0.25 * d.next());
        let width = r.width * (0.8 + 0.4 * d.next());
        let half = (0.5 * length / STEP).ceil() as usize;
        // The wander per step, from radians per km.
        let wander = r.wander * STEP / 1000.0;
        let grow = |dir: (f64, f64), d: &mut Draws| {
            let (mut x, mut z) = (cx, cz);
            let (mut hx, mut hz) = dir;
            let mut turn = 0.0f64;
            let mut pts = Vec::with_capacity(half);
            for _ in 0..half {
                turn = 0.7 * turn + wander * (2.0 * d.next() - 1.0);
                // A rotation by `turn` from its half-angle tangent (exact rational form).
                let q = 0.5 * turn;
                let (c, s) = ((1.0 - q * q) / (1.0 + q * q), 2.0 * q / (1.0 + q * q));
                (hx, hz) = (c * hx - s * hz, s * hx + c * hz);
                x += hx * STEP;
                z += hz * STEP;
                pts.push((x, z));
            }
            pts
        };
        let ahead = grow(heading, &mut d);
        let behind = grow((-heading.0, -heading.1), &mut d);
        let mut points: Vec<(f64, f64)> = behind.into_iter().rev().collect();
        points.push((cx, cz));
        points.extend(ahead);
        let mut along = Vec::with_capacity(points.len());
        let mut s = 0.0;
        along.push(0.0);
        for w in points.windows(2) {
            let (dx, dz) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            s += (dx * dx + dz * dz).sqrt();
            along.push(s);
        }
        out.push(Spine {
            points,
            along,
            strength,
            width,
        });
    }
    out
}

/// The strongest range at a point: a bump across its spine, tapered over the last quarter of its
/// length at each end.
fn range_strength(spines: &[Spine], apron: &Apron, x: f64, z: f64) -> f64 {
    let mut best = 0.0f64;
    for sp in spines {
        let mut d2 = f64::INFINITY;
        let mut at = 0.0;
        for (k, w) in sp.points.windows(2).enumerate() {
            let (ax, az) = w[0];
            let (dx, dz) = (w[1].0 - ax, w[1].1 - az);
            let l2 = dx * dx + dz * dz;
            let t = (((x - ax) * dx + (z - az) * dz) / l2).clamp(0.0, 1.0);
            let (px, pz) = (ax + t * dx - x, az + t * dz - z);
            let e = px * px + pz * pz;
            if e < d2 {
                d2 = e;
                at = sp.along[k] + t * (sp.along[k + 1] - sp.along[k]);
            }
        }
        let d = d2.sqrt();
        let l = sp.length();
        let taper = smooth01(at.min(l - at) / (0.25 * l));
        let v = bump(d / sp.width).max(apron.at(d));
        best = best.max(sp.strength * v * taper);
    }
    best
}

struct Plate {
    x: f64,
    z: f64,
    vx: f64,
    vz: f64,
}

fn plate_points(p: &Plates, seed: u64, reach: f64) -> Vec<Plate> {
    let mut d = Draws::new(seed, 2);
    let mut out = Vec::new();
    while out.len() < p.count as usize {
        let (x, z) = (2.0 * d.next() - 1.0, 2.0 * d.next() - 1.0);
        if x * x + z * z > 1.0 {
            continue;
        }
        let (vx, vz) = d.direction();
        let speed = 0.5 + 0.5 * d.next();
        out.push(Plate {
            x: x * reach,
            z: z * reach,
            vx: vx * speed,
            vz: vz * speed,
        });
    }
    out
}

/// (uplift ≥ 0, rift ≤ 0) at a point: the boundary between its nearest and second-nearest
/// plates, by how fast they close (or open) across it.
fn plate_strength(p: &Plates, apron: &Apron, plates: &[Plate], x: f64, z: f64) -> (f64, f64) {
    if plates.len() < 2 {
        return (0.0, 0.0);
    }
    let (mut a, mut b) = ((f64::INFINITY, 0usize), (f64::INFINITY, 0usize));
    for (k, q) in plates.iter().enumerate() {
        let d2 = (q.x - x) * (q.x - x) + (q.z - z) * (q.z - z);
        if d2 < a.0 {
            b = a;
            a = (d2, k);
        } else if d2 < b.0 {
            b = (d2, k);
        }
    }
    let (i, j) = (&plates[a.1], &plates[b.1]);
    let (nx, nz) = (j.x - i.x, j.z - i.z);
    let sep = (nx * nx + nz * nz).sqrt();
    // Distance to the bisector between the two points.
    let to_boundary = (b.0 - a.0) / (2.0 * sep);
    let across = bump(to_boundary / p.width);
    // How fast i moves toward j, in [−1, 1].
    let closing = ((i.vx - j.vx) * nx + (i.vz - j.vz) * nz) / sep * 0.5;
    if closing >= 0.0 {
        (
            p.strength * closing * across.max(apron.at(to_boundary)),
            0.0,
        )
    } else {
        (0.0, p.rift * closing * across)
    }
}

/// The erodibility factor per cell from the rock bands: 1 in soft rock, 1 − `contrast` in hard.
pub fn rock_factor(
    grid: Grid,
    cell: f64,
    x0: f64,
    sea: &[bool],
    seed: u64,
    r: &RockBands,
) -> Vec<f64> {
    let mut d = Draws::new(seed, 3);
    let (sx, sz) = d.direction();
    let warp = seed_word(seed, STREAM + 16);
    let mut f = vec![1.0f64; grid.len()];
    for (i, v) in f.iter_mut().enumerate() {
        if sea[i] {
            continue;
        }
        let (x, z) = centre(grid, cell, x0, i);
        let w = r.warp * noise(warp, x, z, r.warp_wavelength, 2);
        let s = (x * sx + z * sz + w) / r.wavelength;
        // A triangle wave in [0, 1]; its top `hard_share` is hard rock, with smooth edges.
        let tri = 2.0 * (s - s.floor() - 0.5).abs();
        let edge = 1.0 - r.hard_share;
        let hard = smooth01((tri - edge) / 0.08 + 0.5);
        *v = 1.0 - r.contrast * hard;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coast_distance_grows_inland_by_cells() {
        let g = Grid { n: 9 };
        let sea: Vec<bool> = (0..81).map(|i| g.on_border(i)).collect();
        let d = coast_distance(g, 100.0, &sea);
        assert_eq!(d[0], 0.0);
        assert_eq!(d[9 + 1], 100.0);
        assert_eq!(d[4 * 9 + 4], 400.0);
    }
}
