//! The camera (port of render/fov.ts, render/three/aimCamera.ts, world/face.ts's viewForward), in
//! f64: positions stay precise ~8,000 km out because the renderer draws camera-relative.

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

/// The view direction for yaw/pitch (degrees; yaw 0 = +Z) on `face` (±1: pitch is toward the
/// face's up).
pub fn view_forward(yaw_deg: f64, pitch_deg: f64, face: f64) -> V3 {
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    [
        yaw.sin() * pitch.cos(),
        face * pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
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

/// Aims the camera along yaw/pitch for the player's `face` (up is ±y); `flip` (radians, 0 at rest)
/// then pitches the view over about its own right axis, nose first: the camera turning over across
/// the midplane. The heading has already turned by 180°, so a flip of π is the view before.
pub fn aim(yaw_deg: f64, pitch_deg: f64, face: f64, flip: f64) -> Basis {
    let up = [0.0, face, 0.0];
    // three's lookAt for cameras: back = eye − target, right = up × back, up = back × right.
    let back = scale(normalize(view_forward(yaw_deg, pitch_deg, face)), -1.0);
    let mut right = cross(up, back);
    if dot(right, right) < 1e-20 {
        // Looking straight along up: three nudges the back axis.
        right = cross(up, normalize(add(back, [1e-4, 0.0, 0.0])));
    }
    let right = normalize(right);
    let up = cross(back, right);
    if flip == 0.0 {
        return Basis { right, up, back };
    }
    // rotateX(flip): a rotation about the local right axis.
    let (c, s) = (flip.cos(), flip.sin());
    Basis {
        right,
        up: add(scale(up, c), scale(back, s)),
        back: add(scale(back, c), scale(up, -s)),
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

    #[test]
    fn the_half_forward_somersault_across_the_midplane() {
        let yaw = 37.0;
        for pitch in [-80.0, -30.0, 0.0, 45.0] {
            let b = aim(yaw, pitch, 1.0, 0.0);
            // The crossing frame: heading turned by 180°, face −1, a flip of π: the same view.
            let a = aim(yaw + 180.0, pitch, -1.0, std::f64::consts::PI);
            close(a.forward(), b.forward());
            close(a.up, b.up);
            // The right vector is the same all the way: the player's own left and right.
            for flip in [std::f64::consts::PI, std::f64::consts::FRAC_PI_2, 0.0] {
                close(aim(yaw + 180.0, pitch, -1.0, flip).right, b.right);
            }
            // Halfway the view looks along the old camera's down.
            close(
                aim(yaw + 180.0, pitch, -1.0, std::f64::consts::FRAC_PI_2).forward(),
                scale(b.up, -1.0),
            );
        }
    }

    #[test]
    fn looks_along_view_forward_with_the_faces_up() {
        let b = aim(90.0, 20.0, 1.0, 0.0);
        close(b.forward(), normalize(view_forward(90.0, 20.0, 1.0)));
        assert!(b.up[1] > 0.0);
        assert!(
            aim(90.0, 20.0, -1.0, 0.0).up[1] < 0.0,
            "face B's view is upside down"
        );
    }

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
