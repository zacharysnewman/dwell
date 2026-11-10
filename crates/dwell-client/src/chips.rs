//! The axe's visuals on the client (docs/adr/0067-stone-axe-and-chopping.md §11): the wood chips a
//! swing knocks out of the cut face, flying out along the swing and falling, quads of the billboard
//! pass (`Puff`s, render/blasts.rs) coloured by the wood's `chips` data; a CPU adapter draws a third
//! of them. The cuts themselves are carved into the chunk mesh (`dwell-mesh` `carve.rs`).
//! Draw-only: nothing here feeds the simulation or the aim.
//!
//! Face-local: written with +y up and crossed into the world through `FaceFrame` (chips fall toward
//! the face's ground; a standing trunk's bands are face-locally lower first).
//!
//! Each hit also shakes leaves down from the tree's leaf blocks ([`leaf_cells`],
//! [`Chips::shake_leaves`]).
use dwell_blocks::StateId;

use crate::blasts::{Puff, PuffKind};
use dwell_protocol::chopping::WOODS;
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
}
