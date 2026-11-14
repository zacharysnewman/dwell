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
    /// The least a head stands on screen (CSS px): past where `size_m` is smaller its billboard
    /// grows with the distance, so a herd reads from the air (WORK_QUEUE item 46; 0: never).
    pub min_px: f64,
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

/// A head's height (m) at `distance` m from the eye: its look's `size_m`, or past where that is
/// under `min_px` on screen, what stands `min_px` tall (`focal_px`: CSS px a metre at a metre, the
/// viewport's height over 2·tan(fov/2)).
pub fn head_size(look: &FarLook, distance: f64, focal_px: f64) -> f64 {
    if focal_px <= 0.0 {
        return look.size_m;
    }
    look.size_m.max(look.min_px * distance / focal_px)
}

/// The focal length in CSS px of a viewport `height_px` tall at a vertical FOV of `fov_y_deg`.
pub fn focal_px(height_px: f64, fov_y_deg: f64) -> f64 {
    height_px / (2.0 * (fov_y_deg.to_radians() / 2.0).tan())
}

/// The heads' billboards seen from `eye` (world), the faded-out ones left out, each at least its
/// look's `min_px` tall on screen ([`head_size`]); none on a CPU adapter (`cpu`), which draws the
/// cheapest tier of everything.
pub fn puffs(heads: &[Head], eye: [f64; 3], cpu: bool, focal_px: f64) -> Vec<Puff> {
    if cpu {
        return Vec::new();
    }
    heads
        .iter()
        .filter_map(|h| {
            let alpha = fade(h, eye);
            (alpha > 0.01).then(|| {
                let local = h.face.point(eye);
                let d = (local[0] - h.feet[0])
                    .hypot(local[1] - h.feet[1])
                    .hypot(local[2] - h.feet[2]);
                let half = 0.5 * head_size(&h.look, d, focal_px);
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

/// A far herd or pack as the finder lists it (`__dwell.herds()`, WORK_QUEUE item 46).
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub id: u32,
    pub species: String,
    /// Face-local metres.
    pub x: f64,
    pub z: f64,
    pub size: f64,
    /// Face-local horizontal metres from the eye.
    pub distance: f64,
    /// The face-local yaw that faces it (degrees, 0 toward +z, 90 toward +x: the look's yaw).
    pub bearing_deg: f64,
}

/// The snapshot's groups on the eye's face (`eye` world), of `species` (None: any), nearest first
/// (ties by id): the finder of the dev tools' `herds` and `goToHerd`.
pub fn find(snapshot: &EcologyMessage, eye: [f64; 3], species: Option<&str>) -> Vec<Found> {
    let face = FaceFrame::at_height(eye[1]);
    let local = face.point(eye);
    let mut out: Vec<Found> = snapshot
        .groups
        .iter()
        .filter(|g| g.face == u8::from(face.is_b()))
        .filter(|g| species.is_none_or(|s| s == g.species))
        .map(|g| {
            let (dx, dz) = (f64::from(g.x) - local[0], f64::from(g.z) - local[2]);
            Found {
                id: g.id,
                species: g.species.clone(),
                x: f64::from(g.x),
                z: f64::from(g.z),
                size: f64::from(g.size),
                distance: dx.hypot(dz),
                bearing_deg: dx.atan2(dz).to_degrees().rem_euclid(360.0),
            }
        })
        .collect();
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.cmp(&b.id)));
    out
}

/// Where the finder's `goToHerd` stands the player: `back_m` short of the herd along the line
/// from `from` (face-local x, z), or `back_m` toward +x when it is right there.
pub fn approach(from: (f64, f64), herd: (f64, f64), back_m: f64) -> (f64, f64) {
    let (dx, dz) = (herd.0 - from.0, herd.1 - from.1);
    let d = dx.hypot(dz);
    if d < 1e-6 {
        return (herd.0 - back_m, herd.1);
    }
    (herd.0 - dx / d * back_m, herd.1 - dz / d * back_m)
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
        min_px: 0.0,
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
            let p = puffs(&heads, eye, false, 0.0);
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
            let half: Vec<_> = puffs(&heads, mid, false, 0.0)
                .into_iter()
                .filter(|q| q.alpha < 1.0)
                .collect();
            assert!(!half.is_empty(), "fading between the fade's start and end");
            assert!(
                puffs(&heads, eye, true, 0.0).is_empty(),
                "none on the CPU tier"
            );
            assert!(
                puffs(&heads, f.other().point([0.0, 12.0, 0.0]), false, 0.0).is_empty(),
                "none seen from the other face"
            );
        }
    );

    dwell_world::both_faces!(
        /// WORK_QUEUE item 46: a far head stays at least its look's `min_px` on screen, its
        /// billboard growing with the distance (its look's size near), and the shipped deer read
        /// to ~2.5 km before they fade.
        fn a_far_head_grows_with_distance_to_its_least_screen_size(f: FaceFrame) {
            let look = FarLook {
                min_px: 4.0,
                fade_start_m: 2000.0,
                fade_end_m: 3000.0,
                ..LOOK
            };
            let focal = focal_px(720.0, 70.0);
            assert!(
                (head_size(&look, 10.0, focal) - 2.0).abs() < 1e-12,
                "its size near"
            );
            let px = |d: f64| head_size(&look, d, focal) * focal / d;
            for d in [600.0, 1200.0, 2400.0] {
                assert!((px(d) - 4.0).abs() < 1e-9, "{d} m: {} px", px(d));
            }
            assert!(head_size(&look, 2400.0, focal) > head_size(&look, 1200.0, focal));
            // Through the billboards, on this face: a head 1.2 km off is drawn min_px tall.
            let head = Head {
                face: f,
                feet: [1200.0, 10.0, 0.0],
                look,
                seed: 1,
            };
            let eye = f.point([0.0, 10.0, 0.0]);
            let p = puffs(&[head], eye, false, focal);
            assert_eq!(p.len(), 1);
            assert!((2.0 * p[0].size * focal / 1200.0 - 4.0).abs() < 1e-6);
            assert_eq!(FaceFrame::at_height(p[0].at[1]), f);
            assert!(f.y(p[0].at[1]) > 10.0, "grown upward from its feet");
            // The shipped deer: whole at 1.5 km, gone past 2.5 km.
            let deer = *FAR_LOOKS
                .iter()
                .find(|l| l.species == "dwell:deer")
                .unwrap();
            assert!(deer.min_px > 0.0);
            let at = |x: f64| Head {
                face: f,
                feet: [x, 10.0, 0.0],
                look: deer,
                seed: 1,
            };
            assert!((fade(&at(1500.0), eye) - 1.0).abs() < 1e-12);
            assert!(fade(&at(2200.0), eye) > 0.0);
            assert_eq!(fade(&at(2600.0), eye), 0.0);
        }
    );

    dwell_world::both_faces!(
        /// The finder (`__dwell.herds()`, `goToHerd`): the eye's face's groups nearest first, by
        /// species, with their distance and the yaw that faces them; the other face's left out.
        fn the_finder_lists_the_nearest_herds_of_the_eyes_face(f: FaceFrame) {
            let face = u8::from(f.is_b());
            let mut s = snapshot(face);
            s.groups.push(EcologyGroupNet {
                id: 20,
                face: 1 - face,
                species: "test:elk".into(),
                x: 1.0,
                z: 1.0,
                size: 3.0,
            });
            let eye = f.point([0.0, 12.0, 0.0]);
            let all = find(&s, eye, None);
            assert_eq!(
                all.iter().map(|g| g.id).collect::<Vec<_>>(),
                [8, 9, 7],
                "nearest first, this face only"
            );
            let elk = find(&s, eye, Some("test:elk"));
            assert_eq!(elk[0].id, 9);
            assert!((elk[0].distance - 50.0).abs() < 1e-9);
            assert!((elk[0].bearing_deg - 90.0).abs() < 1e-9, "toward +x");
            assert!((elk[1].bearing_deg - (300f64.atan2(-40.0)).to_degrees()).abs() < 1e-6);
            assert!(find(&s, eye, Some("test:moose")).is_empty());
            // goToHerd stands the player short of it, on the line from where it was.
            let (x, z) = approach((0.0, 0.0), (50.0, 0.0), 30.0);
            assert!((x - 20.0).abs() < 1e-9 && z.abs() < 1e-9);
            assert_eq!(approach((50.0, 0.0), (50.0, 0.0), 30.0), (20.0, 0.0));
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
