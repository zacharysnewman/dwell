//! Far herds drawn simply (docs/ECOLOGY.md §11): each herd or pack of the far ecology's snapshot
//! (`Ecology`, §9) is drawn as a few lit billboards in its species' colour (`far` in
//! shared/ecology/ecology.json), one per head, scattered round the group's place on the ground,
//! fading out with the distance; none on a CPU adapter. A group near players is not in the snapshot
//! (it is creatures), so the two never draw at once.

use crate::blasts::{Puff, PuffKind};
use dwell_protocol::EcologyMessage;
use dwell_world::FaceFrame;

/// How a far group of a species is drawn (generated from its `far` data).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FarLook {
    pub species: &'static str,
    /// sRGB.
    pub color: [u8; 3],
    /// A head's height (m).
    pub size_m: f64,
    /// The fade (m from the eye): whole until the start, gone at the end.
    pub fade_start_m: f64,
    pub fade_end_m: f64,
}

include!("far_herds.gen.rs");

/// The most heads drawn per group.
pub const MAX_HEADS: usize = 24;

/// One head as laid out: face-local feet, its look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    pub face: FaceFrame,
    pub feet: [f64; 3],
    pub look: FarLook,
    pub seed: u32,
}

fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

fn unit(h: u32) -> f64 {
    f64::from(h >> 8) / f64::from(1u32 << 24)
}

/// The heads of `snapshot`'s groups in `looks` (a group whose species has no look is not drawn):
/// round(size) of them (at least 1, at most `MAX_HEADS`), scattered in a disc that grows with the
/// group (stable per group id and head, so they hold still between snapshots), each stood on
/// `ground(face, x, z)` (face-local y; `None`: not known yet, that head is left out).
pub fn lay_out(
    snapshot: &EcologyMessage,
    looks: &[FarLook],
    ground: &mut dyn FnMut(FaceFrame, f64, f64) -> Option<f64>,
) -> Vec<Head> {
    let mut out = Vec::new();
    for g in &snapshot.groups {
        let Some(look) = looks.iter().find(|l| l.species == g.species) else {
            continue;
        };
        let face = if g.face == 1 {
            FaceFrame::B
        } else {
            FaceFrame::A
        };
        let n = (f64::from(g.size).round() as usize).clamp(1, MAX_HEADS);
        let spread = 2.0 + 1.6 * (n as f64).sqrt();
        for k in 0..n as u32 {
            let h = hash(g.id, k);
            let r = spread * unit(h).sqrt();
            let th = unit(hash(h, 7)) * std::f64::consts::TAU;
            let (x, z) = (f64::from(g.x) + r * th.cos(), f64::from(g.z) + r * th.sin());
            if let Some(y) = ground(face, x, z) {
                out.push(Head {
                    face,
                    feet: [x, y, z],
                    look: *look,
                    seed: h,
                });
            }
        }
    }
    out
}

/// A head's opacity seen from `eye` (world): whole until its look's fade start, gone at its end;
/// none on the other face.
pub fn fade(head: &Head, eye: [f64; 3]) -> f64 {
    if FaceFrame::at_height(eye[1]) != head.face {
        return 0.0;
    }
    let local = head.face.point(eye);
    let d = (local[0] - head.feet[0]).hypot(local[2] - head.feet[2]);
    let l = &head.look;
    (1.0 - (d - l.fade_start_m) / (l.fade_end_m - l.fade_start_m)).clamp(0.0, 1.0)
}

/// The heads' billboards seen from `eye` (world), the faded-out ones left out; none on a CPU
/// adapter (`cpu`), which draws the cheapest tier of everything.
pub fn puffs(heads: &[Head], eye: [f64; 3], cpu: bool) -> Vec<Puff> {
    if cpu {
        return Vec::new();
    }
    heads
        .iter()
        .filter_map(|h| {
            let alpha = fade(h, eye);
            (alpha > 0.01).then(|| {
                let half = 0.5 * h.look.size_m;
                let feet = h.feet;
                let at = h.face.point([feet[0], feet[1] + half, feet[2]]);
                let [r, g, b] = h.look.color.map(u32::from);
                Puff {
                    at,
                    size: half,
                    alpha,
                    kind: PuffKind::Herd,
                    explosive: 0,
                    // The colour (sRGB, 24 bits), a byte of the head's own seed.
                    seed: r | g << 8 | b << 16 | (h.seed & 0xff) << 24,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::EcologyGroupNet;

    const LOOK: FarLook = FarLook {
        species: "test:elk",
        color: [16, 32, 48],
        size_m: 2.0,
        fade_start_m: 100.0,
        fade_end_m: 200.0,
    };

    fn snapshot(face: u8) -> EcologyMessage {
        EcologyMessage {
            day: 1.0,
            groups: vec![
                EcologyGroupNet {
                    id: 7,
                    face,
                    species: "test:elk".into(),
                    x: 300.0,
                    z: -40.0,
                    size: 5.6,
                },
                EcologyGroupNet {
                    id: 8,
                    face,
                    species: "test:none".into(),
                    x: 0.0,
                    z: 0.0,
                    size: 3.0,
                },
                EcologyGroupNet {
                    id: 9,
                    face,
                    species: "test:elk".into(),
                    x: 50.0,
                    z: 0.0,
                    size: 400.0,
                },
            ],
        }
    }

    dwell_world::both_faces!(
        /// A group draws round(size) heads (capped) of its species' look, near its place and on the
        /// ground; a species without a look draws none; the far ones fade; none on the CPU tier.
        fn far_herds_are_heads_on_the_ground_by_their_snapshot(f: FaceFrame) {
            let face = u8::from(f.is_b());
            let mut asked = Vec::new();
            let heads = lay_out(&snapshot(face), &[LOOK], &mut |ff, x, z| {
                asked.push(ff);
                Some(10.0 + 0.01 * x + 0.02 * z)
            });
            assert!(asked.iter().all(|&a| a == f), "asked on its own face");
            let of = |x: f64| {
                heads
                    .iter()
                    .filter(|h| (h.feet[0] - x).abs() < 40.0)
                    .count()
            };
            assert_eq!(of(300.0), 6, "round(5.6) heads");
            assert_eq!(of(50.0), MAX_HEADS, "a big herd is capped");
            assert_eq!(
                heads.len(),
                6 + MAX_HEADS,
                "a species without a look draws none"
            );
            for h in heads.iter().filter(|h| (h.feet[0] - 300.0).abs() < 40.0) {
                let d = (h.feet[0] - 300.0).hypot(h.feet[2] + 40.0);
                assert!(
                    d < 2.0 + 1.6 * 6f64.sqrt() + 1e-9,
                    "scattered round its place: {d}"
                );
                let ground = 10.0 + 0.01 * h.feet[0] + 0.02 * h.feet[2];
                assert!((h.feet[1] - ground).abs() < 1e-9, "on the ground");
            }
            // Stable between snapshots.
            assert_eq!(
                heads,
                lay_out(&snapshot(face), &[LOOK], &mut |_, x, z| Some(
                    10.0 + 0.01 * x + 0.02 * z
                ))
            );
            // Seen from the eye at x 0 on this face, 2 m up: the herd at 50 m whole, the one at
            // ~300 m gone (past the fade's end); on the other face, none.
            let eye = f.point([0.0, 12.0, 0.0]);
            let p = puffs(&heads, eye, false);
            assert_eq!(p.len(), MAX_HEADS, "the far herd faded out");
            for q in &p {
                assert_eq!(q.kind, PuffKind::Herd);
                assert!((q.alpha - 1.0).abs() < 1e-9);
                assert_eq!(
                    q.seed & 0xff_ffff,
                    16 | 32 << 8 | 48 << 16,
                    "the look's colour"
                );
                assert_eq!(FaceFrame::at_height(q.at[1]), f, "drawn on its face");
                let up = f.y(q.at[1]);
                assert!(up > 10.0, "stood on the ground, not under it");
            }
            let mid = f.point([150.0, 12.0, -40.0]);
            let half: Vec<_> = puffs(&heads, mid, false)
                .into_iter()
                .filter(|q| q.alpha < 1.0)
                .collect();
            assert!(!half.is_empty(), "fading between the fade's start and end");
            assert!(puffs(&heads, eye, true).is_empty(), "none on the CPU tier");
            assert!(
                puffs(&heads, f.other().point([0.0, 12.0, 0.0]), false).is_empty(),
                "none seen from the other face"
            );
        }
    );

    #[test]
    fn the_shipped_species_have_far_looks() {
        // both-faces: data only.
        let deer = FAR_LOOKS
            .iter()
            .find(|l| l.species == "dwell:deer")
            .expect("deer's far look is data");
        assert!(deer.size_m > 0.0 && deer.fade_end_m > deer.fade_start_m);
        assert!(FAR_LOOKS.iter().any(|l| l.species == "dwell:wolf"));
    }
}
