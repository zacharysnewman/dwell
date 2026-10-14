//! The view the LOD octree is walked for (§6.6; `client/src/lod/frustum.ts`): a camera anywhere in
//! the world (the player's eye or the dev camera), its view frustum as planes, and the screen-space
//! size of a cell.

use crate::grid::Vec3;

/// Vertical field of view (degrees) on screens narrow enough not to hit the horizontal cap
/// (`client/src/render/fov.ts`, ported here for the viewport).
pub const VERTICAL_FOV: f64 = 75.0;
/// Widest horizontal field of view (degrees); wider screens get a narrower vertical one.
pub const MAX_HORIZONTAL_FOV: f64 = 100.0;

/// Vertical field of view (degrees) for the given aspect ratio (width / height).
pub fn vertical_fov(aspect: f64) -> f64 {
    let h = MAX_HORIZONTAL_FOV.to_radians();
    let capped = 2.0 * ((h / 2.0).tan() / aspect).atan() * 180.0 / std::f64::consts::PI;
    VERTICAL_FOV.min(capped)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodCamera {
    pub position: Vec3,
    /// Degrees; yaw 0 = +Z, pitch up > 0 (as the renderer's camera).
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    /// Vertical field of view (degrees), aspect (width / height), and drawing height in pixels.
    pub fov_y_deg: f64,
    pub aspect: f64,
    pub height_px: f64,
    /// Camera velocity (m/s): detail loads ahead along it (see `LodSystem`).
    pub velocity: Option<Vec3>,
}

/// The drawing's field of view, aspect and height for LOD selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodViewport {
    pub fov_y_deg: f64,
    pub aspect: f64,
    pub height_px: f64,
}

/// The viewport from the drawing's size in CSS pixels. The pixel error is in CSS pixels, whatever
/// the screen's density: counted in device pixels, a 2× screen asked for four times the sections.
pub fn lod_viewport(css_width: f64, css_height: f64) -> LodViewport {
    let aspect = css_width / css_height.max(1.0);
    LodViewport {
        fov_y_deg: vertical_fov(aspect),
        aspect,
        height_px: css_height,
    }
}

/// Inward plane normals through the camera (sides) and the near plane, for AABB tests.
#[derive(Clone, Debug)]
pub struct Frustum {
    pub camera: LodCamera,
    planes: [Vec3; 5],
    /// Pixels per metre at 1 m distance: heightPx / (2 tan(fovY / 2)).
    pub pixels_per_radian: f64,
}

impl Frustum {
    pub fn new(camera: LodCamera) -> Self {
        let yaw = camera.yaw_deg.to_radians();
        let pitch = camera.pitch_deg.to_radians();
        let f: Vec3 = [
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        ];
        // Horizontal right vector from the yaw alone (f × up, but defined looking straight down too).
        let r: Vec3 = [-yaw.cos(), 0.0, yaw.sin()];
        let u = cross(r, f);
        let half = (camera.fov_y_deg * std::f64::consts::PI / 360.0).tan();
        let ty = half * 1.05;
        let tx = ty * camera.aspect;
        let combine = |a: Vec3, sa: f64, b: Vec3, sb: f64| -> Vec3 {
            [
                a[0] * sa + b[0] * sb,
                a[1] * sa + b[1] * sb,
                a[2] * sa + b[2] * sb,
            ]
        };
        Self {
            camera,
            planes: [
                combine(f, tx, r, 1.0),
                combine(f, tx, r, -1.0),
                combine(f, ty, u, 1.0),
                combine(f, ty, u, -1.0),
                f,
            ],
            pixels_per_radian: camera.height_px / (2.0 * half),
        }
    }

    /// True when the box [lo, hi] (world metres) may be in view.
    pub fn intersects(&self, lo: Vec3, hi: Vec3) -> bool {
        let p = self.camera.position;
        self.planes.iter().all(|n| {
            // The box corner furthest along the normal must be on the inner side.
            let c = |a: usize| (if n[a] >= 0.0 { hi[a] } else { lo[a] }) - p[a];
            n[0] * c(0) + n[1] * c(1) + n[2] * c(2) >= 0.0
        })
    }

    /// Distance from the camera to the box (0 inside it).
    pub fn distance(&self, lo: Vec3, hi: Vec3) -> f64 {
        box_distance(self.camera.position, lo, hi)
    }
}

/// Distance from p to the box [lo, hi] (0 inside it).
pub fn box_distance(p: Vec3, lo: Vec3, hi: Vec3) -> f64 {
    let mut d = 0.0;
    for a in 0..3 {
        let v = p[a];
        let e = if v < lo[a] {
            lo[a] - v
        } else if v > hi[a] {
            v - hi[a]
        } else {
            0.0
        };
        d += e * e;
    }
    d.sqrt()
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_the_pixel_error_in_css_pixels_whatever_the_screen_density() {
        // Regression (playtest: frame rate and memory on 2× screens): the height was in device
        // pixels, so a 2× screen refined as if it were twice as tall — four times the sections.
        let v = lod_viewport(1440.0, 900.0);
        assert_eq!(
            v,
            LodViewport {
                fov_y_deg: vertical_fov(1.6),
                aspect: 1.6,
                height_px: 900.0
            }
        );
        let cell = Frustum::new(LodCamera {
            position: [0.0; 3],
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            fov_y_deg: v.fov_y_deg,
            aspect: v.aspect,
            height_px: v.height_px,
            velocity: None,
        });
        // A 1 m cell 1 km away covers under a CSS pixel there.
        assert!(cell.pixels_per_radian / 1000.0 < 1.0);
    }

    #[test]
    fn caps_the_horizontal_field_of_view() {
        assert_eq!(vertical_fov(1.0), VERTICAL_FOV);
        let wide = vertical_fov(3.0);
        assert!(wide < VERTICAL_FOV);
        let horizontal = 2.0 * ((wide.to_radians() / 2.0).tan() * 3.0).atan();
        assert!((horizontal.to_degrees() - MAX_HORIZONTAL_FOV).abs() < 1e-9);
    }
}
