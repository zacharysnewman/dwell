//! The CPU reference marcher (RAY_MARCHED_RENDERER.md §4.2–4.3, §5 "Correctness under both"): the
//! same traversal the GPU marcher runs, over the same words, in plain Rust — the oracle the GPU is
//! checked against per pixel, and the place skipping is measured.
//!
//! # Ray state: integer + fraction
//!
//! A ray is a grid cell (`i32`), a fraction in it (`f32`, 0..1) and a unit direction. Every plane
//! the ray crosses is an integer grid coordinate `b`, and its distance is
//! `t = (f32(b − cell) − frac) / dir` — the integer difference is exact in `f32` (below 2²⁴), so
//! precision depends on the distance travelled, not on where the camera is in an 8,000 km world
//! (the raster renderer's two depth passes are not needed). A hit's position is rebuilt from its
//! integer cell, its face and `t` ([`Hit::point`]), never from `floor(origin + t·dir)`.
//!
//! # Traversal
//!
//! Each iteration finds the ray's level (cone LOD: the coarsest level whose cell is under
//! `lod_pixels` pixels at distance `t`, or the finest coarser level with data there), its node
//! slot, then:
//! - NONE at every level, or ALL_AIR: skip the node's box;
//! - a uniform brick (or ALL_SOLID): hit where the ray entered;
//! - an empty brick: skip the cube of bricks its distance says is empty (§4.3 item 4), or the
//!   largest column region (whole node down to one column) whose occupied rows the ray stays
//!   above or below across the region (§4.3 item 3), whichever reaches further;
//! - a mixed brick: skip such a column region if there is one, else Amanatides–Woo DDA over its
//!   voxels until a hit or it leaves the brick.
//!
//! Where the wanted level grows mid-node the ray carries on at the coarser level from where it
//! is; if that coarser cell is occupied it is hit there, on the plane the ray last crossed (a LOD
//! transition, like the raster LOD's).
//!
//! After a skip the ray re-enters at the box's exit plane: that axis is the exact plane, the
//! others the cell the ray is in at `t` (checked against the same plane distances the DDA uses,
//! then clamped to the box it left), so a skip lands in the cell a voxel-by-voxel DDA would reach
//! — no cracks at brick, node, or chunk borders. The DDA's axis is the nearest plane, ties to the
//! lowest axis; the GPU's branchless step mask must keep that rule (x if `tx <= ty && tx <= tz`,
//! else y if `ty <= tz`, else z), and the mask is also the hit normal.
//!
//! **Both faces**: data is in world orientation (face B's chunks are already mirrored into place),
//! so rays need nothing special; faces are world faces, and [`face_up`] turns one into the face
//! the player's up sees (what the mesher's mirror does for textures and tints).

use dwell_blocks::StateId;
use dwell_world::Face;

use crate::layout::*;
use crate::scene::{COLUMNS_NONE, Scene, shr};
use crate::state::MarchClass;

/// A ray in the grid frame (crate::layout).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub cell: [i32; 3],
    /// In [0, 1).
    pub frac: [f32; 3],
    /// Unit length: `t` is metres.
    pub dir: [f32; 3],
}

impl Ray {
    /// From a world position (metres) and a direction (normalised here).
    pub fn from_world(pos: [f64; 3], dir: [f32; 3]) -> Self {
        let g = pos.map(|p| p + GRID_OFFSET as f64);
        let cell = g.map(|v| v.floor() as i32);
        let frac = std::array::from_fn(|k| {
            let f = (g[k] - g[k].floor()) as f32;
            if f >= 1.0 { 1.0 - f32::EPSILON } else { f }
        });
        Self::new(cell, frac, dir)
    }
    pub fn new(cell: [i32; 3], frac: [f32; 3], dir: [f32; 3]) -> Self {
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        Self {
            cell,
            frac,
            dir: dir.map(|d| d / len),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarchParams {
    /// The fog distance: no hit beyond it.
    pub max_t: f32,
    /// Iterations before giving up (RAY_MARCHED_RENDERER.md: 128 on phones).
    pub step_cap: u32,
    /// Radians per pixel (0: always the finest level with data).
    pub pixel_angle: f32,
    /// The LOD's pixel error: a cell may be this many pixels wide (4 desktop, 8 phones).
    pub lod_pixels: f32,
    /// §4.3 item 3: skip column regions the ray passes above or below.
    pub columns: bool,
    /// §4.3 item 4: skip empty bricks by their distance.
    pub distances: bool,
}

impl Default for MarchParams {
    fn default() -> Self {
        Self {
            max_t: 1.0e7,
            step_cap: 1024,
            pixel_angle: 0.0,
            lod_pixels: 4.0,
            columns: true,
            distances: true,
        }
    }
}

impl MarchParams {
    /// The same without empty-space skipping beyond the hierarchy's masks.
    pub fn without_skipping(self) -> Self {
        Self {
            columns: false,
            distances: false,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub level: u32,
    /// The hit cell at its level.
    pub cell: [i32; 3],
    /// The face entered (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z, the mesher's numbering): the
    /// normal. `None` when the ray starts in an occupied cell.
    pub face: Option<usize>,
    pub t: f32,
    pub state: StateId,
}

impl Hit {
    /// The hit point relative to the ray's origin (metres): the face's plane exactly on its axis.
    pub fn point(&self, ray: &Ray) -> [f32; 3] {
        let mut p: [f32; 3] = std::array::from_fn(|k| self.t * ray.dir[k]);
        if let Some(face) = self.face {
            let a = face / 2;
            let c = if face % 2 == 1 {
                self.cell[a]
            } else {
                self.cell[a] + 1
            };
            p[a] = ((c << self.level) - ray.cell[a]) as f32 - ray.frac[a];
        }
        p
    }
    /// The unit normal of the face.
    pub fn normal(&self) -> [f32; 3] {
        let mut n = [0.0; 3];
        if let Some(f) = self.face {
            n[f / 2] = if f % 2 == 0 { 1.0 } else { -1.0 };
        }
        n
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Hit(Hit),
    /// Past `max_t` or out of the data: the sky (or fog).
    Miss,
    /// The step cap: drawn as fog, counted in the histogram.
    Cap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct March {
    pub outcome: Outcome,
    pub steps: u32,
}

impl March {
    pub fn hit(&self) -> Option<Hit> {
        match self.outcome {
            Outcome::Hit(h) => Some(h),
            _ => None,
        }
    }
}

/// The face the player's up sees: on face B (terrain hanging below the midplane) world −Y is the
/// top.
pub fn face_up(world: Face, face: usize) -> usize {
    match (world, face) {
        (Face::B, 2) => 3,
        (Face::B, 3) => 2,
        _ => face,
    }
}

/// Cone LOD: the coarsest level whose cell (2^L m) is at most `lod_pixels` pixels at `t`.
pub fn level_for(t: f32, p: &MarchParams) -> u32 {
    let footprint = t * p.pixel_angle * p.lod_pixels;
    if footprint < 2.0 {
        return 0;
    }
    (footprint.log2().floor() as u32).min(MAX_LEVEL)
}

/// The shape hook: whether a ray entering an occupied cell hits it, given its class and the
/// cell's shape plane entry. Cubes only today: every occupied cell is hit where it is entered.
/// Analytic shape planes (step A5) go here, testing the ray against the shape from the plane
/// (or the state's registry shape) and returning a `t` inside the cell.
pub fn shape_hit(class: MarchClass, _shape_plane: u8) -> bool {
    class.occupied()
}

/// The ray's arithmetic, shared by the marcher and the brute-force reference.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RayMath {
    pub c: [i32; 3],
    pub f: [f32; 3],
    pub d: [f32; 3],
    pub inv: [f32; 3],
    pub step: [i32; 3],
}

impl RayMath {
    pub fn new(r: &Ray) -> Self {
        Self {
            c: r.cell,
            f: r.frac,
            d: r.dir,
            inv: r.dir.map(|d| if d == 0.0 { 0.0 } else { 1.0 / d }),
            step: r.dir.map(|d| if d < 0.0 { -1 } else { 1 }),
        }
    }
    /// Distance to the plane `grid = b` on axis k.
    pub fn tp(&self, k: usize, b: i32) -> f32 {
        if self.d[k] == 0.0 {
            f32::INFINITY
        } else {
            ((b - self.c[k]) as f32 - self.f[k]) * self.inv[k]
        }
    }
    /// Where the ray leaves box [lo, hi) (grid cells) it is in: distance and axis (ties: lowest).
    pub fn exit(&self, lo: [i32; 3], hi: [i32; 3]) -> (f32, usize) {
        let mut best = (f32::INFINITY, 0);
        for k in 0..3 {
            let t = self.tp(k, if self.step[k] > 0 { hi[k] } else { lo[k] });
            if t < best.0 {
                best = (t, k);
            }
        }
        best
    }
    /// The entry plane of the box after leaving [lo, hi) through axis a, and the box the ray is
    /// then in: one cell thick on that axis, the old box's extent on the others.
    pub fn beyond(&self, lo: [i32; 3], hi: [i32; 3], a: usize) -> (i32, [i32; 3], [i32; 3]) {
        let (mut nlo, mut nhi) = (lo, hi);
        let plane = if self.step[a] > 0 { hi[a] } else { lo[a] };
        if self.step[a] > 0 {
            nlo[a] = plane;
            nhi[a] = plane + 1;
        } else {
            nlo[a] = plane - 1;
            nhi[a] = plane;
        }
        (plane, nlo, nhi)
    }
    /// The grid cell the ray is in at `t`, inside box [lo, hi): exact on the entry axis, and on
    /// the others the cell whose planes bracket `t` by the same distances the DDA uses, with the
    /// DDA's tie rule (at equal distances the lower axis steps first: axes below the entry axis
    /// have crossed a plane at exactly `t`, axes above it have not).
    pub fn cell_at(
        &self,
        t: f32,
        entry: Option<(usize, i32)>,
        lo: [i32; 3],
        hi: [i32; 3],
    ) -> [i32; 3] {
        std::array::from_fn(|k| {
            let g = match entry {
                Some((a, plane)) if a == k => {
                    if self.step[k] > 0 {
                        plane
                    } else {
                        plane - 1
                    }
                }
                _ if self.d[k] == 0.0 => self.c[k],
                _ => {
                    // Crossed at exactly t: below the entry axis yes, above it (or at the start)
                    // no.
                    let strict = entry.is_none_or(|(a, _)| k > a);
                    let crossed = |tp: f32| tp < t || (tp == t && !strict);
                    let mut g = self.c[k] + (self.f[k] + t * self.d[k]).floor() as i32;
                    // The planes the ray leaves cell g by and enters it by.
                    let (next, enter) = if self.step[k] > 0 { (1, 0) } else { (0, 1) };
                    if crossed(self.tp(k, g + next)) {
                        g += self.step[k];
                    } else if !crossed(self.tp(k, g + enter)) && self.tp(k, g + enter) > 0.0 {
                        // (A plane at or behind the origin bounds the ray's first cell.)
                        g -= self.step[k];
                    }
                    g
                }
            };
            g.clamp(lo[k], hi[k] - 1)
        })
    }
    /// The ray's coordinate on axis k at `t`, relative to grid coordinate `base`.
    pub fn rel(&self, k: usize, base: i32, t: f32) -> f32 {
        (self.c[k] - base) as f32 + self.f[k] + t * self.d[k]
    }
    pub fn face(&self, a: usize) -> usize {
        2 * a + usize::from(self.step[a] > 0)
    }
}

/// Margin (cells) by which a ray must clear a column region's occupied rows to skip it.
const COLUMN_EPS: f32 = 1e-4;

fn box_of(cell: [i32; 3], shift: u32) -> ([i32; 3], [i32; 3]) {
    (cell.map(|c| c << shift), cell.map(|c| (c + 1) << shift))
}

impl Scene {
    /// Marches one ray (see the module docs).
    pub fn march(&self, ray: &Ray, p: &MarchParams) -> March {
        let rm = RayMath::new(ray);
        let mut steps = 0u32;
        let done = |outcome, steps| March { outcome, steps };
        let (Some((wlo, whi)), Some((_, top))) = (self.bounds(), self.level_range()) else {
            return done(Outcome::Miss, 0);
        };
        // Into the data's box.
        let mut t = 0.0f32;
        let mut entry: Option<(usize, i32)> = None;
        let (mut lo, mut hi) = (rm.c, rm.c.map(|c| c + 1));
        if (0..3).any(|k| rm.c[k] < wlo[k] || rm.c[k] >= whi[k]) {
            let mut t_in = (f32::NEG_INFINITY, 0usize);
            let mut t_out = f32::INFINITY;
            for k in 0..3 {
                if rm.d[k] == 0.0 {
                    if rm.c[k] < wlo[k] || rm.c[k] >= whi[k] {
                        return done(Outcome::Miss, 0);
                    }
                    continue;
                }
                let (near, far) = if rm.step[k] > 0 {
                    (wlo[k], whi[k])
                } else {
                    (whi[k], wlo[k])
                };
                let tn = rm.tp(k, near);
                if tn > t_in.0 {
                    t_in = (tn, k);
                }
                t_out = t_out.min(rm.tp(k, far));
            }
            if t_in.0 >= t_out || t_out <= 0.0 {
                return done(Outcome::Miss, 0);
            }
            let a = t_in.1;
            t = t_in.0.max(0.0);
            let plane = if rm.step[a] > 0 { wlo[a] } else { whi[a] };
            entry = Some((a, plane));
            (lo, hi) = (wlo, whi);
        }
        let t_end = p.max_t.min(rm.exit(wlo, whi).0);
        let hit = |level: u32, cell: [i32; 3], entry: Option<(usize, i32)>, t: f32, state| {
            Outcome::Hit(Hit {
                level,
                cell,
                face: entry.map(|(a, _)| rm.face(a)),
                t,
                state,
            })
        };
        loop {
            if t > t_end {
                return done(Outcome::Miss, steps);
            }
            steps += 1;
            if steps > p.step_cap {
                return done(Outcome::Cap, steps);
            }
            let g = rm.cell_at(t, entry, lo, hi);
            let want = level_for(t, p).min(top);
            let mut found = None;
            for l in want..=top {
                let Some(lv) = &self.levels[l as usize] else {
                    continue;
                };
                let cell = shr(g, l);
                let n = shr(cell, 5);
                if let Some(i) = lv.info.slot_of(n) {
                    let s = Slot::from_word(lv.slots[i]);
                    if s != Slot::None {
                        found = Some((l, cell, n, s));
                        break;
                    }
                }
            }
            // Leaves box [blo, bhi) and continues from its exit.
            macro_rules! skip {
                ($blo:expr, $bhi:expr) => {{
                    let (blo, bhi) = ($blo, $bhi);
                    let (te, a) = rm.exit(blo, bhi);
                    let (plane, nlo, nhi) = rm.beyond(blo, bhi, a);
                    t = te;
                    entry = Some((a, plane));
                    (lo, hi) = (nlo, nhi);
                    continue;
                }};
            }
            let Some((l, cell, n, slot)) = found else {
                let (blo, bhi) = box_of(shr(g, top + 5), top + 5);
                skip!(blo, bhi)
            };
            let node_shift = l + 5;
            let (nlo, nhi) = box_of(n, node_shift);
            let ni = match slot {
                Slot::None | Slot::AllAir => skip!(nlo, nhi),
                Slot::AllSolid(s) => return done(hit(l, cell, entry, t, s), steps),
                Slot::Node(i) => i as usize,
            };
            let node = &self.nodes[ni];
            let local: [i32; 3] = std::array::from_fn(|k| cell[k] - (n[k] << 5));
            let cols = self.node_columns[ni];
            // The largest column region the ray clears, and where it leaves it.
            let mut column = None;
            if p.columns && cols != COLUMNS_NONE {
                let words = &self.columns[cols as usize..cols as usize + COLUMN_WORDS];
                let scale = 1.0 / (1u32 << l) as f32;
                for m in (0..COLUMN_MIPS).rev() {
                    let (rx, rz) = ((local[0] >> m) as usize, (local[2] >> m) as usize);
                    let (clo, chi) = column_span(words, m, rx, rz);
                    let w = (1i32 << m) << l;
                    let blo = [nlo[0] + rx as i32 * w, nlo[1], nlo[2] + rz as i32 * w];
                    let bhi = [blo[0] + w, nhi[1], blo[2] + w];
                    let (te, _) = rm.exit(blo, bhi);
                    if clo < chi {
                        let y0 = rm.rel(1, nlo[1], t) * scale;
                        let y1 = rm.rel(1, nlo[1], te) * scale;
                        let (ymin, ymax) = (y0.min(y1), y0.max(y1));
                        let clear = ymin >= f32::from(chi) + COLUMN_EPS
                            || ymax <= f32::from(clo) - COLUMN_EPS;
                        if !clear {
                            continue;
                        }
                    }
                    column = Some((blo, bhi, te));
                    break;
                }
            }
            let bc = local.map(|v| v >> 3);
            let b = brick_index(bc[0], bc[1], bc[2]);
            let brick_shift = l + 3;
            match node.get(b) {
                BrickRef::Empty(d) => {
                    let r = if p.distances { d.max(1) as i32 - 1 } else { 0 };
                    let blo: [i32; 3] = std::array::from_fn(|k| (n[k] << 2) + (bc[k] - r).max(0));
                    let bhi: [i32; 3] =
                        std::array::from_fn(|k| (n[k] << 2) + (bc[k] + r + 1).min(4));
                    let (blo, bhi) = (blo.map(|v| v << brick_shift), bhi.map(|v| v << brick_shift));
                    // Whichever empty box reaches further.
                    match column {
                        Some((clo, chi, te)) if te > rm.exit(blo, bhi).0 => skip!(clo, chi),
                        _ => skip!(blo, bhi),
                    }
                }
                BrickRef::Uniform(s) => {
                    if shape_hit(self.table.class(s), 0) {
                        return done(hit(l, cell, entry, t, s), steps);
                    }
                    let (blo, bhi) = box_of(shr(cell, 3), brick_shift);
                    skip!(blo, bhi)
                }
                BrickRef::Mixed(off) => {
                    if let Some((clo, chi, _)) = column {
                        skip!(clo, chi)
                    }
                    let brick = self.pool.from(off);
                    let b0: [i32; 3] = std::array::from_fn(|k| (n[k] << 5) + (bc[k] << 3));
                    let mut cell = cell;
                    loop {
                        let v = voxel_index(cell[0] - b0[0], cell[1] - b0[1], cell[2] - b0[2]);
                        if brick_occupied(brick, v) {
                            let s = brick_state(brick, v);
                            if shape_hit(self.table.class(s), brick_shape(brick, v)) {
                                return done(hit(l, cell, entry, t, s), steps);
                            }
                        }
                        let (clo, chi) = box_of(cell, l);
                        let (te, a) = rm.exit(clo, chi);
                        let (plane, _, _) = rm.beyond(clo, chi, a);
                        t = te;
                        entry = Some((a, plane));
                        cell[a] += rm.step[a];
                        if t > t_end {
                            return done(Outcome::Miss, steps);
                        }
                        if cell[a] < b0[a] || cell[a] >= b0[a] + BRICK_CELLS {
                            (lo, hi) = box_of(cell, l);
                            break;
                        }
                        steps += 1;
                        if steps > p.step_cap {
                            return done(Outcome::Cap, steps);
                        }
                    }
                }
            }
        }
    }
}
