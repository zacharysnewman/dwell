//! The camera (port of render/fov.ts, render/three/aimCamera.ts, world/face.ts's viewForward), in
//! f64: positions stay precise ~8,000 km out because the renderer draws camera-relative.

use dwell_world::FaceFrame;

pub type V3 = [f64; 3];

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn normalize(v: V3) -> V3 {
    let n = dot(v, v).sqrt();
    if n == 0.0 {
        v
    } else {
        [v[0] / n, v[1] / n, v[2] / n]
    }
}
fn scale(v: V3, s: f64) -> V3 {
    [v[0] * s, v[1] * s, v[2] * s]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The field of view's rule is dwell-lod's (`base_fov`), so the renderer and the LOD selection
/// agree: the Field of view setting (default 75° vertical), narrowed on wide screens.
pub use dwell_lod::frustum::{MAX_HORIZONTAL_FOV, VERTICAL_FOV, vertical_fov};
/// Horizontal field of view (degrees) that a vertical one gives at an aspect ratio.
pub fn horizontal_fov(vertical_deg: f64, aspect: f64) -> f64 {
    (2.0 * ((vertical_deg.to_radians() / 2.0).tan() * aspect).atan()).to_degrees()
}

/// The camera's world axes: right, up, and back (it looks along −back), as three.js's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    pub right: V3,
    pub up: V3,
    pub back: V3,
}

impl Basis {
    pub fn forward(&self) -> V3 {
        scale(self.back, -1.0)
    }
}

/// Aims the camera along yaw/pitch for the player's `face` (its up, the look by
/// [`FaceFrame::look`]); `roll` (radians, 0 at rest) then turns the view about its own forward axis
/// (rotateZ): the camera rolling upright after a crossing of the midplane, whose look has continued
/// on the other face pointing the same way (`face_flip.rs`), so a roll of `face_flip::residual()`
/// (half a turn) is the view before. The forward does not depend on the roll.
pub fn aim(yaw_deg: f64, pitch_deg: f64, face: FaceFrame, roll: f64) -> Basis {
    // three's lookAt for cameras: back = eye − target, right = up × back, up = back × right. The
    // right is the heading's (up × back, normalized, for every pitch short of straight up or
    // down), so it is the same looking straight along up, where up × back vanishes: a heading of
    // 0 (+Z) has the player's right at −X, mirrored on face B.
    let back = scale(normalize(face.look(yaw_deg, pitch_deg)), -1.0);
    let yaw = yaw_deg.to_radians();
    let right = [face.handed(-yaw.cos()), 0.0, face.handed(yaw.sin())];
    let up = cross(back, right);
    if roll == 0.0 {
        return Basis { right, up, back };
    }
    // rotateZ(roll): a rotation about the local back (forward) axis.
    let (c, s) = (roll.cos(), roll.sin());
    Basis {
        right: add(scale(right, c), scale(up, s)),
        up: add(scale(up, c), scale(right, -s)),
        back,
    }
}

/// A column-major 4×4 matrix (f32, for the GPU).
pub type Mat4 = [[f32; 4]; 4];

/// The view matrix of a camera at the origin with this basis (rendering is camera-relative).
pub fn view_matrix(b: &Basis) -> Mat4 {
    let r = |v: V3| v.map(|c| c as f32);
    let (x, y, z) = (r(b.right), r(b.up), r(b.back));
    [
        [x[0], y[0], z[0], 0.0],
        [x[1], y[1], z[1], 0.0],
        [x[2], y[2], z[2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// A right-handed perspective projection with WebGPU's [0, 1] depth.
pub fn perspective(fov_y_deg: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let f = 1.0 / (fov_y_deg.to_radians() / 2.0).tan();
    let range = far / (near - far);
    [
        [(f / aspect) as f32, 0.0, 0.0, 0.0],
        [0.0, f as f32, 0.0, 0.0],
        [0.0, 0.0, range as f32, -1.0],
        [0.0, 0.0, (near * range) as f32, 0.0],
    ]
}

/// The scene's projection (ARCHITECTURE.md §6.6 Depth): [`perspective`] with **reversed** depth,
/// the near plane at 1 and the far plane at 0 (cleared to `frame::DEPTH_CLEAR`, tested with
/// `frame::DEPTH_COMPARE`). A `depth32float` value is densest near 0, so reversed depth resolves
/// about 2⁻²³ of the distance at every distance, where standard depth's precision falls with the
/// distance squared over the near plane (5 cm): two surfaces a metre apart a kilometre off fought.
pub fn perspective_reversed(fov_y_deg: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let f = 1.0 / (fov_y_deg.to_radians() / 2.0).tan();
    let range = near / (far - near);
    [
        [(f / aspect) as f32, 0.0, 0.0, 0.0],
        [0.0, f as f32, 0.0, 0.0],
        [0.0, 0.0, range as f32, -1.0],
        [0.0, 0.0, (far * range) as f32, 0.0],
    ]
}

pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for (c, col) in out.iter_mut().enumerate() {
        for (r, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V3, b: V3) {
        assert!(dot(sub(a, b), sub(a, b)).sqrt() < 1e-6, "{a:?} vs {b:?}");
    }

    use dwell_game::face_flip::{FaceFlip, ROLL_SECONDS};

    fn angle(a: V3, b: V3) -> f64 {
        dot(a, b).clamp(-1.0, 1.0).acos()
    }

    /// Crosses from face `f` looking at (`yaw`, `pitch`), as the game does: the views drawn from
    /// the last frame before the crossing to the first at rest, and the look at rest.
    fn crossing(f: FaceFrame, yaw: f64, pitch: f64) -> (Vec<Basis>, (f64, f64)) {
        let mut flip = FaceFlip::new();
        flip.tick(f, yaw, pitch);
        let mut views = vec![aim(yaw, pitch, f, flip.draw(0.016))];
        let to = f.other();
        let look = flip.tick(to, yaw, pitch);
        let mut dt = 0.0;
        for _ in 0..=(ROLL_SECONDS / 0.01) as usize + 1 {
            views.push(aim(look.0, look.1, to, flip.draw(dt)));
            dt = 0.01;
        }
        (views, look)
    }

    dwell_world::both_faces!(
        /// The owner (BIFACIAL_WORLD.md §6, 2026-10-21): "whichever method lets you remain looking
        /// in the same direction". Across the midplane from face `f`, at every heading and pitch,
        /// the world forward is the same before, through the roll and after; the first frame is
        /// the old view; the roll alone changes (right and up turn about forward by the same
        /// angle, monotonically, half a turn in all), ending upright on the other face.
        fn across_the_midplane_the_view_points_the_same_way_and_only_rolls(f: FaceFrame) {
            for yaw in (0..360).step_by(30) {
                for pitch in (-90..=90).step_by(15) {
                    let (yaw, pitch) = (f64::from(yaw), f64::from(pitch));
                    let (views, look) = crossing(f, yaw, pitch);
                    let first = views[0];
                    close(views[1].right, first.right);
                    close(views[1].up, first.up);
                    let mut rolled = 0.0;
                    for v in &views {
                        close(v.forward(), first.forward());
                        let (r, u) = (angle(first.right, v.right), angle(first.up, v.up));
                        assert!((r - u).abs() < 1e-6, "{yaw} {pitch}: {r} {u}");
                        assert!(r >= rolled - 1e-9, "monotone");
                        rolled = r;
                    }
                    assert!((rolled - std::f64::consts::PI).abs() < 1e-6, "{rolled}");
                    let rest = aim(look.0, look.1, f.other(), 0.0);
                    let last = views.last().expect("views");
                    close(last.up, rest.up);
                    close(last.right, rest.right);
                    // Upright on the other face: its up has the other face's up's sign.
                    assert!(dot(rest.up, f.other().up()) >= -1e-9);
                }
            }
        }
    );

    dwell_world::both_faces!(
        /// The snap experiment (`?crossingroll=snap`, owner 2026-10-21): with a 0 s roll the view
        /// drawn on the crossing frame from face `f` is already the upright view on the other face,
        /// pointing exactly where the view before pointed, at every heading and pitch.
        fn a_snapped_crossing_draws_upright_pointing_the_same_way(f: FaceFrame) {
            for yaw in (0..360).step_by(30) {
                for pitch in (-90..=90).step_by(15) {
                    let (yaw, pitch) = (f64::from(yaw), f64::from(pitch));
                    let mut flip = FaceFlip::new();
                    assert!(flip.set_seconds(0.0));
                    flip.tick(f, yaw, pitch);
                    let before = aim(yaw, pitch, f, flip.draw(0.016));
                    let to = f.other();
                    let look = flip.tick(to, yaw, pitch);
                    let roll = flip.draw(0.0);
                    assert_eq!(roll, 0.0, "{yaw} {pitch}");
                    let drawn = aim(look.0, look.1, to, roll);
                    close(drawn.forward(), before.forward());
                    let rest = aim(look.0, look.1, to, 0.0);
                    close(drawn.up, rest.up);
                    close(drawn.right, rest.right);
                    assert!(dot(drawn.up, to.up()) >= -1e-9, "upright on the other face");
                }
            }
        }
    );

    dwell_world::both_faces!(
        /// On face `f` the camera looks along the face's look with the face's up: face-local, its
        /// forward and up are face A's, and its right is face A's mirrored (a mirror reverses
        /// handedness, `FaceFrame::handed`): face B's view is face A's upside down.
        fn looks_along_the_faces_look_with_the_faces_up(f: FaceFrame) {
            for (yaw, pitch) in [(90.0, 20.0), (-30.0, -60.0), (200.0, 0.0)] {
                let a = aim(yaw, pitch, FaceFrame::A, 0.0);
                let b = aim(yaw, pitch, f, 0.0);
                close(b.forward(), normalize(f.look(yaw, pitch)));
                close(f.vector(b.forward()), a.forward());
                close(f.vector(b.up), a.up);
                close(f.vector(b.right), a.right.map(|c| f.handed(c)));
                assert!(f.vector(b.up)[1] > 0.0);
            }
        }
    );

    #[test]
    fn keeps_the_horizontal_fov_under_the_cap_on_wide_screens() {
        for aspect in [16.0 / 9.0, 21.0 / 9.0, 844.0 / 390.0] {
            let h = horizontal_fov(vertical_fov(aspect), aspect);
            assert!(h <= MAX_HORIZONTAL_FOV + 1e-9 && h > MAX_HORIZONTAL_FOV - 0.5);
        }
        for aspect in [4.0 / 3.0, 1.0, 390.0 / 844.0] {
            assert_eq!(vertical_fov(aspect), VERTICAL_FOV);
        }
    }

    /// The depth32float value a point at view distance `d` stores under projection `p`, computed as
    /// the GPU does: the clip coordinates in f32, then the divide.
    fn stored_depth(p: &Mat4, d: f32) -> f32 {
        let z = -d;
        (p[2][2] * z + p[3][2]) / (p[2][3] * z + p[3][3])
    }

    #[test]
    fn the_reversed_projection_maps_near_to_one_and_far_to_zero() {
        let p = perspective_reversed(75.0, 1.5, 0.05, 1024.0);
        assert!((stored_depth(&p, 0.05) - 1.0).abs() < 1e-5);
        assert!(stored_depth(&p, 1024.0).abs() < 1e-5);
        assert!(
            stored_depth(&p, 10.0) > stored_depth(&p, 10.5),
            "nearer is greater"
        );
    }

    /// The LOD z-fighting class (ARCHITECTURE.md §6.6, "Known bug class: coplanar and near-coplanar
    /// LOD surfaces"): the near pass spans 5 cm to the split (1,024 m), and from 150–300 m up the
    /// ground lies 200 m–1 km away. Surfaces a few centimetres apart there (a skirt behind a
    /// neighbour's top, a level fading out under the one fading in, a riser under a top) must
    /// store different depths in the right order, or they swap as the camera moves. Standard depth
    /// gave the same value to surfaces up to ~1.2 m apart at 1 km.
    #[test]
    fn the_scene_depth_resolves_centimetres_across_the_near_pass() {
        let near = super::super::NEAR_PLANE;
        let far = super::super::NEAR_SPLIT;
        let p = perspective_reversed(75.0, 1.5, near, far);
        for d in [10.0f32, 100.0, 300.0, 600.0, 1000.0] {
            // 2 cm, at any distance in the pass: front strictly greater (the test is ≥).
            let (front, back) = (stored_depth(&p, d), stored_depth(&p, d + 0.02));
            assert!(front > back, "at {d} m: {front} vs {back} (2 cm apart)");
        }
        // The far pass, its near plane at 0.95 × the split, out to the far plane: 0.1 % of the
        // distance (a metre at 1 km beyond the split, 1 km at 1,000 km).
        let p = perspective_reversed(75.0, 1.5, far * 0.95, super::super::FAR_PLANE);
        for d in [1100.0f32, 5000.0, 50_000.0, 1.0e6] {
            let (front, back) = (stored_depth(&p, d), stored_depth(&p, d * 1.001));
            assert!(front > back, "far pass at {d} m: {front} vs {back}");
        }
    }

    #[test]
    fn the_projection_maps_near_and_far_to_webgpu_depth() {
        let p = perspective(75.0, 1.5, 0.05, 1024.0);
        let depth = |z: f32| {
            let (zc, wc) = (p[2][2] * z + p[3][2], p[2][3] * z + p[3][3]);
            zc / wc
        };
        assert!(depth(-0.05).abs() < 1e-5);
        assert!((depth(-1024.0) - 1.0).abs() < 1e-5);
    }
}
