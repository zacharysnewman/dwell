//! The axe's visuals on the client (docs/adr/0067-stone-axe-and-chopping.md §11): the gouges drawn
//! on a cut wood cell from its `Cut` (each side's notch as a wedge-shaded mark and each band's kerf
//! past it as a dark slit, flat on the side's face), and the wood chips a swing knocks out of the cut
//! face, flying out along the swing and falling. Both are quads of the billboard pass (`Puff`s,
//! render/blasts.rs), coloured by the wood's `chips` data; a CPU adapter draws a third of the chips.
//! Draw-only: nothing here feeds the simulation or the aim.
//!
//! Face-local: written with +y up and crossed into the world through `FaceFrame` (chips fall toward
//! the face's ground; a standing trunk's bands are face-locally lower first).

use crate::blasts::{Puff, PuffKind};
use dwell_protocol::chopping::{self as chop, Cut, WOODS};
use dwell_world::{CELL_H, FaceFrame, m_of_row};

/// How long a chip flies (s).
pub const CHIP_SECONDS: f64 = 0.9;
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
            });
        }
    }

    pub fn prune(&mut self, now_s: f64) {
        self.list.retain(|c| now_s - c.start_s < CHIP_SECONDS);
    }

    /// The chips to draw now: tumbling flat chips (`PuffKind::Chip`), falling to the face's ground.
    pub fn puffs(&self, now_s: f64) -> Vec<Puff> {
        self.list
            .iter()
            .filter_map(|c| {
                let t = now_s - c.start_s;
                if !(0.0..CHIP_SECONDS).contains(&t) {
                    return None;
                }
                let w = WOODS.get(c.wood)?;
                let f = FaceFrame::at_height(c.at[1]);
                let up = f.up();
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
