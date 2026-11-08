//! The axe's visuals on the client (docs/adr/0067-stone-axe-and-chopping.md §11): the gouges drawn
//! on a cut wood cell from its `Cut` (each side's notch as a wedge-shaded mark and each band's kerf
//! past it as a dark slit, flat on the side's face), and the wood chips a swing knocks out of the cut
//! face, flying out along the swing and falling. Both are quads of the billboard pass (`Puff`s,
//! render/blasts.rs), coloured by the wood's `chips` data; a CPU adapter draws a third of the chips.
//! Draw-only: nothing here feeds the simulation or the aim.
//!
//! Face-local: written with +y up and crossed into the world through `FaceFrame` (chips fall toward
//! the face's ground; a standing trunk's bands are face-locally lower first).
//!
//! The owner (2026-11-08): the gouges read as bark chips stuck on, grey-olive in shade. A notch is
//! now drawn as a V cut into the side, each of its two cut faces lit by its own normal
//! ([`Gouge::cut_normals`], packed into the quad as [`Gouge::bits`]): the face toward the sky
//! bright, the one under it in shade, fresh end grain, a crease at its apex darkening with depth
//! and a bark lip; and fresh wood keeps its tan hue in shade (render/blasts.rs `wood_lit`). Each
//! hit also shakes leaves down from the tree's leaf blocks ([`leaf_cells`], [`Chips::shake_leaves`]).
use dwell_blocks::StateId;

use crate::blasts::{Puff, PuffKind};
use dwell_protocol::chopping::{self as chop, Cut, WOODS};
use dwell_world::{CELL_H, FaceFrame, m_of_row};

/// How long a chip flies (s).
pub const CHIP_SECONDS: f64 = 0.9;
/// How long a leaf flutters down (s), its fall's settled speed (m/s) and its flutter (m).
pub const LEAF_SECONDS: f64 = 2.8;
const LEAF_FALL: f64 = 1.1;
const LEAF_FLUTTER: f64 = 0.3;
/// The most leaf cells a hit looks through for its leaves.
const LEAF_CELLS_MAX: usize = 512;
/// Gravity on a chip (m/s²), toward the face's ground.
const CHIP_GRAVITY: f64 = 9.0;
/// How far a gouge's quad sits out from its face (m), over the block's own face.
const GOUGE_LIFT: f64 = 0.004;
/// A kerf's slit (m, along the grain).
const KERF_WIDTH: f64 = 0.03;
/// A side's rim left uncut at its two edges across (m).
const GOUGE_RIM: f64 = 0.06;
/// The kerfs and notches shallower than this are not drawn (m).
const GOUGE_MIN: f32 = 0.004;

/// One chip in flight: from `at` (world) at `vel` (world m/s), since `start_s`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chip {
    pub at: [f64; 3],
    pub vel: [f64; 3],
    pub start_s: f64,
    /// The wood's index in `WOODS`.
    pub wood: usize,
    pub seed: u32,
    /// A leaf shaken down (it flutters and falls slowly), not a chip.
    pub leaf: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Chips {
    pub list: Vec<Chip>,
    /// A CPU adapter: a third of the chips.
    pub cpu: bool,
}

fn hash(seed: u32, k: u32) -> f64 {
    let mut h = seed ^ k.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h) / f64::from(u32::MAX)
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-9 { v } else { v.map(|c| c / l) }
}

/// A face index (`axis × 2 + positive`, world) as its outward normal (world).
pub fn face_normal(face: u8) -> [f64; 3] {
    let mut n = [0.0; 3];
    n[usize::from(face / 2) % 3] = if face % 2 == 1 { 1.0 } else { -1.0 };
    n
}

/// The way a swing travels at the cut, face-local (+y up), for a look `look` (face-local): a level
/// swing sweeps from the right across to the left, a vertical one down.
pub fn swing_travel(vertical: bool, look: [f64; 3]) -> [f64; 3] {
    if vertical {
        return [0.0, -1.0, 0.0];
    }
    // The look's right, level: look × up = (−look.z, 0, look.x); the swing goes the other way.
    let r = norm([-look[2], 0.0, look[0]]);
    [-r[0], 0.0, -r[2]]
}

impl Chips {
    /// How many chips a swing at wood `wood` knocks out.
    pub fn count(&self, wood: usize) -> usize {
        let n = WOODS.get(wood).map_or(0, |w| w.chips.count as usize);
        if self.cpu { n.div_ceil(3) } else { n }
    }

    /// A swing's chips at wood `wood`: out of face `face` (world) at `point` (world), along the
    /// swing (`vertical`, the look `look`, world), now (`now_s`).
    pub fn burst(
        &mut self,
        wood: usize,
        face: u8,
        point: [f64; 3],
        vertical: bool,
        look: [f64; 3],
        now_s: f64,
    ) {
        let Some(w) = WOODS.get(wood) else {
            return;
        };
        let f = FaceFrame::at_height(point[1]);
        let n = f.vector(face_normal(face));
        let travel = swing_travel(vertical, f.vector(look));
        let seed = (point[0] * 977.0) as i32 as u32
            ^ ((point[1] * 613.0) as i32 as u32).wrapping_mul(0x85eb_ca6b)
            ^ ((point[2] * 331.0) as i32 as u32).wrapping_mul(0xc2b2_ae35)
            ^ (now_s * 1000.0) as u32;
        for k in 0..self.count(wood) as u32 {
            let r: [f64; 3] = std::array::from_fn(|j| hash(seed, k * 3 + j as u32) * 2.0 - 1.0);
            let s = w.chips.speed * (0.7 + 0.6 * hash(seed, 101 + k));
            // Out of the cut, carried along the swing, a little up, scattered.
            let d = norm(std::array::from_fn(|j| {
                0.8 * n[j] + 0.6 * travel[j] + 0.35 * r[j] + if j == 1 { 0.3 } else { 0.0 }
            }));
            self.list.push(Chip {
                at: point,
                vel: f.vector(d.map(|c| c * s)),
                start_s: now_s,
                wood,
                seed: seed ^ k.wrapping_mul(0x27d4_eb2d),
                leaf: false,
            });
        }
    }

    /// How many leaves a hit at wood `wood` shakes down (a CPU adapter: a third).
    pub fn leaf_count(&self, wood: usize) -> usize {
        let n = WOODS.get(wood).map_or(0, |w| w.leaves.count as usize);
        if self.cpu { n.div_ceil(3) } else { n }
    }

    /// A hit's leaves: each from one of the tree's leaf cells `cells` (world), drifting down.
    pub fn shake_leaves(&mut self, wood: usize, cells: &[[i32; 3]], now_s: f64, seed: u32) {
        if cells.is_empty() {
            return;
        }
        for k in 0..self.leaf_count(wood) as u32 {
            let cell = cells[(hash(seed, 300 + k) * cells.len() as f64) as usize % cells.len()];
            let f = FaceFrame::at_row(cell[1]);
            let l = f.cell(cell);
            // From under the leaf cell, somewhere across it.
            let at = f.point([
                f64::from(l[0]) + hash(seed, 400 + k),
                m_of_row(l[1]) - 0.02,
                f64::from(l[2]) + hash(seed, 500 + k),
            ]);
            let r: [f64; 3] = std::array::from_fn(|j| hash(seed, 600 + k * 3 + j as u32) - 0.5);
            self.list.push(Chip {
                at,
                vel: f.vector([0.6 * r[0], 0.3 * r[1], 0.6 * r[2]]),
                start_s: now_s,
                wood,
                seed: seed ^ k.wrapping_mul(0x1656_67b1),
                leaf: true,
            });
        }
    }

    pub fn prune(&mut self, now_s: f64) {
        self.list
            .retain(|c| now_s - c.start_s < if c.leaf { LEAF_SECONDS } else { CHIP_SECONDS });
    }

    /// The chips to draw now: tumbling flat chips (`PuffKind::Chip`), falling to the face's ground.
    pub fn puffs(&self, now_s: f64) -> Vec<Puff> {
        self.list
            .iter()
            .filter_map(|c| {
                let t = now_s - c.start_s;
                let w = WOODS.get(c.wood)?;
                let f = FaceFrame::at_height(c.at[1]);
                let up = f.up();
                if c.leaf {
                    if !(0.0..LEAF_SECONDS).contains(&t) {
                        return None;
                    }
                    // Settles to a slow fall, fluttering side to side as it goes.
                    let fall = LEAF_FALL * (t - (1.0 - (-t * 3.0).exp()) / 3.0);
                    let ph = hash(c.seed, 9) * std::f64::consts::TAU;
                    let sway = LEAF_FLUTTER * (t * 4.2 + ph).sin();
                    let side = f.vector([ph.cos(), 0.0, ph.sin()]);
                    let at = std::array::from_fn(|k| {
                        c.at[k] + c.vel[k] * t.min(0.6) - fall * up[k] + sway * side[k]
                    });
                    return Some(Puff {
                        at,
                        size: 0.05,
                        alpha: (1.0 - (t / LEAF_SECONDS).powi(4)).clamp(0.0, 1.0),
                        kind: PuffKind::Leaf,
                        explosive: c.wood,
                        seed: c.seed.wrapping_add((t * 12.0) as u32),
                    });
                }
                if !(0.0..CHIP_SECONDS).contains(&t) {
                    return None;
                }
                let at = std::array::from_fn(|k| {
                    c.at[k] + c.vel[k] * t - 0.5 * CHIP_GRAVITY * t * t * up[k]
                });
                Some(Puff {
                    at,
                    size: 0.5 * w.chips.size * (0.7 + 0.6 * hash(c.seed, 7)),
                    alpha: (1.0 - (t / CHIP_SECONDS).powi(3)).clamp(0.0, 1.0),
                    kind: PuffKind::Chip,
                    explosive: c.wood,
                    seed: c.seed.wrapping_add((t * 30.0) as u32),
                })
            })
            .collect()
    }
}

/// A gouge to draw on a cut cell's side: its centre (world), its half extents `u` (across the side)
/// and `v` (along the grain, face-locally up for a standing trunk), how deep (0–1 of the cell
/// across) and whether it is a kerf's slit (else the notch's wedge).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gouge {
    pub at: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub depth: f64,
    pub kerf: bool,
    /// The side's outward normal (world).
    pub normal: [f64; 3],
}

impl Gouge {
    /// A notch's two cut faces' normals (world): the half toward +`v`, then the half toward −`v`.
    /// The V opens 1.25× its depth each way along the grain, so each face slopes back toward the
    /// apex: on a standing trunk the upper face looks down and the lower up (lit by the sky).
    pub fn cut_normals(&self) -> [[f64; 3]; 2] {
        let v = norm(self.v);
        let n = self.normal;
        [
            norm(std::array::from_fn(|k| 1.25 * n[k] - v[k])),
            norm(std::array::from_fn(|k| 1.25 * n[k] + v[k])),
        ]
    }

    /// The side's normal and the grain's way as the shader reads them (`chip_colour`'s `bits`):
    /// bits 0–2 the side's face (`axis × 2 + positive`), 3–4 the axis `v` lies along, 5 set when
    /// `v` points down that axis.
    pub fn bits(&self) -> u32 {
        let axis = |v: [f64; 3]| {
            (0..3)
                .max_by(|&a, &b| v[a].abs().total_cmp(&v[b].abs()))
                .unwrap_or(0)
        };
        let a = axis(self.normal);
        let face = (a * 2) as u32 + u32::from(self.normal[a] > 0.0);
        let g = axis(self.v);
        face | (g as u32) << 3 | u32::from(self.v[g] < 0.0) << 5
    }
}

/// [`Gouge::bits`] back as the side's normal and the grain's way (unit; the shader's decoding).
pub fn decode_bits(bits: u32) -> ([f64; 3], [f64; 3]) {
    let unit = |a: u32, s: f64| {
        let mut v = [0.0; 3];
        v[a as usize % 3] = s;
        v
    };
    let f = bits & 7;
    (
        unit(f / 2, if f & 1 == 1 { 1.0 } else { -1.0 }),
        unit(
            (bits >> 3) & 3,
            if (bits >> 5) & 1 == 1 { -1.0 } else { 1.0 },
        ),
    )
}

/// The leaf cells (world) a hit at world cell `hit` of wood `wood` shakes leaves from: the wood's
/// `leaves.block` within its `radiusM` across and `heightM` above, face-locally. `voxel` reads the
/// world.
pub fn leaf_cells(
    hit: [i32; 3],
    wood: usize,
    mut voxel: impl FnMut([i32; 3]) -> StateId,
) -> Vec<[i32; 3]> {
    let Some(w) = WOODS.get(wood) else {
        return Vec::new();
    };
    let Some(leaf) = w.leaves.block else {
        return Vec::new();
    };
    let f = FaceFrame::at_row(hit[1]);
    let l = f.cell(hit);
    let r = w.leaves.radius_m.ceil() as i32;
    let rows = (w.leaves.height_m / CELL_H).ceil() as i32;
    let mut out = Vec::new();
    for dy in 0..=rows {
        for dx in -r..=r {
            for dz in -r..=r {
                if f64::from(dx * dx + dz * dz) > w.leaves.radius_m * w.leaves.radius_m {
                    continue;
                }
                let c = f.cell([l[0] + dx, l[1] + dy, l[2] + dz]);
                let m = voxel(c);
                if dwell_blocks::try_info(m).is_some() && dwell_blocks::block_of(m).name == leaf {
                    out.push(c);
                    if out.len() >= LEAF_CELLS_MAX {
                        return out;
                    }
                }
            }
        }
    }
    out
}

/// The gouges of world cell `cell` (grain along world axis `grain`) cut `cut`: on each cut side, the
/// notch's wedge over the middle of the cell along the grain (as tall as its depth opens it), and a
/// slit at each band's middle whose kerf goes past the notch.
pub fn gouges(cell: [i32; 3], grain: usize, cut: &Cut) -> Vec<Gouge> {
    let f = FaceFrame::at_row(cell[1]);
    let local = f.cell(cell);
    let lo = [f64::from(local[0]), m_of_row(local[1]), f64::from(local[2])];
    let size = [1.0, CELL_H, 1.0];
    let mut out = Vec::new();
    for side in 0..chop::SIDES {
        let notch = cut.notch[side];
        let kerfs = cut.kerf[side];
        if notch < GOUGE_MIN && kerfs.iter().all(|&k| k < GOUGE_MIN) {
            continue;
        }
        let n = f.vector(chop::side_normal(grain, side).map(f64::from));
        let a = chop::side_axis(grain, side);
        let b = 3 - a - grain;
        let across = chop::extent(a);
        let mut centre = [0.0; 3];
        centre[a] = lo[a] + if n[a] > 0.0 { size[a] } else { 0.0 } + n[a] * GOUGE_LIFT;
        centre[b] = lo[b] + 0.5 * size[b];
        let mut u = [0.0; 3];
        u[b] = 0.5 * size[b] - GOUGE_RIM;
        let along = size[grain];
        let mut push = |g: f64, half: f64, depth: f64, kerf: bool| {
            let mut at = centre;
            at[grain] = g;
            let mut v = [0.0; 3];
            v[grain] = half;
            out.push(Gouge {
                at: f.point(at),
                u: f.vector(u),
                v: f.vector(v),
                depth: (depth / across).clamp(0.0, 1.0),
                kerf,
                normal: f.vector(n),
            });
        };
        if notch >= GOUGE_MIN {
            // A notch's two angled cuts open it about 2.5× its depth along the grain.
            let half = (1.25 * f64::from(notch)).clamp(0.03, 0.5 * along);
            push(lo[grain] + 0.5 * along, half, f64::from(notch), false);
        }
        for (band, &k) in kerfs.iter().enumerate() {
            if k >= GOUGE_MIN {
                let g = lo[grain] + (band as f64 + 0.5) * 0.5 * along;
                push(g, 0.5 * KERF_WIDTH, f64::from(notch + k), true);
            }
        }
    }
    out
}

/// Gouges as the billboard pass's quads (`PuffKind::Gouge`), the wood's colour by `wood`.
pub fn gouge_puffs(gs: &[Gouge], wood: usize, seed: u32) -> impl Iterator<Item = Puff> + '_ {
    gs.iter().map(move |g| Puff {
        at: g.at,
        size: g.depth,
        alpha: 1.0,
        kind: PuffKind::Gouge {
            u: g.u,
            v: g.v,
            kerf: g.kerf,
            bits: g.bits(),
        },
        explosive: wood,
        seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::both_faces;

    fn log() -> usize {
        WOODS
            .iter()
            .position(|w| w.block == "dwell:log")
            .expect("log")
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    both_faces!(
        fn a_swing_sprays_its_woods_chips_out_of_the_face_along_the_swing(f: FaceFrame) {
            let mut c = Chips::default();
            // Face-local: a level swing at a trunk's −x face, looking +x; it travels from the
            // look's right to its left (`swing_travel`).
            let point = f.point([3.0, 80.3, 0.5]);
            let face = 0; // −x (world and face-local: x is not mirrored)
            let look = f.vector([1.0, 0.0, 0.0]);
            c.burst(log(), face, point, false, look, 10.0);
            assert_eq!(c.list.len(), WOODS[log()].chips.count as usize);
            let travel = swing_travel(false, [1.0, 0.0, 0.0]);
            let n = [-1.0, 0.0, 0.0];
            let mean: [f64; 3] = std::array::from_fn(|k| {
                c.list.iter().map(|ch| f.vector(ch.vel)[k]).sum::<f64>() / c.list.len() as f64
            });
            for ch in &c.list {
                assert!(
                    dot(f.vector(ch.vel), n) > 0.0,
                    "out of the face: {:?}",
                    ch.vel
                );
            }
            assert!(
                dot(mean, travel) > 0.5,
                "along the swing: {mean:?} vs {travel:?}"
            );
            // They fall toward the face's ground and are gone after their flight.
            let early = c.puffs(10.05);
            let late = c.puffs(10.6);
            let height = |p: &[Puff]| p.iter().map(|q| f.point(q.at)[1]).sum::<f64>();
            assert!(height(&late) < height(&early), "they fall");
            assert!(early.iter().all(|p| p.kind == PuffKind::Chip));
            c.prune(10.0 + CHIP_SECONDS + 0.01);
            assert!(c.list.is_empty());
            // A vertical swing sprays them down along it.
            let mut v = Chips::default();
            v.burst(log(), face, point, true, look, 0.0);
            let down = v.list.iter().map(|ch| f.vector(ch.vel)[1]).sum::<f64>();
            assert!(down < 0.0, "downward: {down}");
        }
    );

    both_faces!(
        fn a_notchs_lower_cut_face_looks_up_and_its_upper_one_down(f: FaceFrame) {
            // Red before 0.50.39: the notch was one flat quad lit as if it faced the sky.
            let cell = f.cell([3, 80, 0]);
            let mut cut = Cut::default();
            cut.notch[0] = 0.12;
            let g = gouges(cell, 1, &cut)[0];
            let [upper, lower] = g.cut_normals().map(|n| f.vector(n));
            assert!(lower[1] > 0.5 && upper[1] < -0.5, "{upper:?} {lower:?}");
            // Both still face out of the −x side.
            assert!(lower[0] < -0.5 && upper[0] < -0.5);
            // The shader's decoding gives the same side and grain.
            let (n, v) = decode_bits(g.bits());
            assert_eq!(n, g.normal);
            assert_eq!(v, norm(g.v));
        }
    );

    both_faces!(
        fn each_hit_shakes_the_woods_leaves_down_from_its_trees_leaf_cells(f: FaceFrame) {
            let leaves = dwell_blocks::parse_state("dwell:leaves").expect("leaves");
            // A canopy 3 m above a hit at face-local (0, 80, 0); nothing else.
            let hit = f.cell([0, 80, 0]);
            let canopy: Vec<[i32; 3]> = (-1..=1)
                .flat_map(|x| (-1..=1).map(move |z| f.cell([x, 86, z])))
                .collect();
            let at = |c: [i32; 3]| if canopy.contains(&c) { leaves } else { 0 };
            let cells = leaf_cells(hit, log(), at);
            assert_eq!(cells.len(), canopy.len());
            let mut c = Chips::default();
            c.shake_leaves(log(), &cells, 1.0, 7);
            let n = WOODS[log()].leaves.count as usize;
            assert!(n > 0);
            assert_eq!(c.list.len(), n, "the wood's leaves per hit");
            // They fall slowly from under the canopy, fluttering, for longer than chips fly.
            let early = c.puffs(1.1);
            let late = c.puffs(2.5);
            assert_eq!(early.len(), n);
            assert!(early.iter().all(|p| p.kind == PuffKind::Leaf));
            let h = |p: &[Puff]| p.iter().map(|q| f.point(q.at)[1]).sum::<f64>() / p.len() as f64;
            assert!(h(&early) > m_of_row(85), "from the canopy: {}", h(&early));
            assert!(
                h(&late) < h(&early) - 0.8,
                "falling: {} then {}",
                h(&early),
                h(&late)
            );
            c.prune(1.0 + CHIP_SECONDS + 0.1);
            assert_eq!(c.list.len(), n, "still falling after a chip would be gone");
            // No tree, no leaves.
            assert!(leaf_cells(hit, log(), |_| 0).is_empty());
        }
    );

    #[test]
    fn a_cpu_adapter_draws_a_third_of_the_chips() {
        // face-a-only: a count, before anything face-dependent.
        let c = Chips {
            cpu: true,
            ..Chips::default()
        };
        let n = WOODS[log()].chips.count as usize;
        assert_eq!(c.count(log()), n.div_ceil(3));
        assert!(c.count(log()) >= 1 && c.count(log()) < n);
    }

    both_faces!(
        fn a_notched_trunk_cell_draws_its_notch_wedge_and_kerf_slits_on_the_cut_face(f: FaceFrame) {
            // A standing trunk's cell (grain y) at face-local (3, 80, 0), notched 0.1 m on its −x
            // side (side 0) with a further 0.04 m kerf in its upper band.
            let cell = f.cell([3, 80, 0]);
            let mut cut = Cut::default();
            cut.notch[0] = 0.1;
            cut.kerf[0][1] = 0.04;
            let gs = gouges(cell, 1, &cut);
            assert_eq!(gs.len(), 2, "{gs:?}");
            let (wedge, slit) = (gs[0], gs[1]);
            assert!(!wedge.kerf && slit.kerf);
            // Both on the −x face (x = 3, lifted out), centred across it (z = 0.5).
            for g in [wedge, slit] {
                let p = f.point(g.at);
                assert!((p[0] - (3.0 - GOUGE_LIFT)).abs() < 1e-9, "{p:?}");
                assert!((p[2] - 0.5).abs() < 1e-9);
                assert_eq!(f.vector(g.normal), [-1.0, 0.0, 0.0]);
            }
            // The wedge at the cell's middle height, opened 2.5× its depth; the slit in the upper
            // band's middle (face-locally): 3/4 of the ½ m row up.
            let base = m_of_row(80);
            assert!((f.point(wedge.at)[1] - (base + 0.25)).abs() < 1e-9);
            assert!((f.vector(wedge.v)[1] - 0.125).abs() < 1e-6);
            assert!((f.point(slit.at)[1] - (base + 0.375)).abs() < 1e-9);
            assert!((wedge.depth - 0.1).abs() < 1e-6 && (slit.depth - 0.14).abs() < 1e-6);
            // An uncut cell draws nothing.
            assert!(gouges(cell, 1, &Cut::default()).is_empty());
        }
    );

    both_faces!(
        fn a_lying_logs_cut_on_its_top_is_drawn_on_its_top_face(f: FaceFrame) {
            // A log along x (grain 0); a cut on its face-local top. Its sides across x are y and z;
            // the side whose world normal is the face's up.
            let cell = f.cell([0, 80, 0]);
            let up = f.up().map(|v| v as i32);
            let side = (0..chop::SIDES)
                .find(|&s| chop::side_normal(0, s) == up)
                .expect("a top side");
            let mut cut = Cut::default();
            cut.notch[side] = 0.08;
            let gs = gouges(cell, 0, &cut);
            assert_eq!(gs.len(), 1);
            let p = f.point(gs[0].at);
            assert!(
                (p[1] - (m_of_row(80) + CELL_H + GOUGE_LIFT)).abs() < 1e-9,
                "{p:?}"
            );
            assert!((p[0] - 0.5).abs() < 1e-9, "the middle of its 1 m length");
            assert_eq!(f.vector(gs[0].normal), [0.0, 1.0, 0.0]);
        }
    );
}
