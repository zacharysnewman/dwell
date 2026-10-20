//! The view the LOD octree is walked for (§6.6; `client/src/lod/frustum.ts`): a camera anywhere in
//! the world (the player's eye or the dev camera), its view frustum as planes, and the screen-space
//! size of a cell.

use crate::grid::Vec3;

/// The default vertical field of view (degrees): the Field of view setting's default.
pub const VERTICAL_FOV: f64 = 75.0;
/// The Field of view setting's range (degrees).
pub const FOV_SETTING_RANGE: (f64, f64) = (60.0, 100.0);
/// Widest horizontal field of view (degrees) at the default setting; wider screens get a narrower
/// vertical one.
pub const MAX_HORIZONTAL_FOV: f64 = 100.0;

/// The aspect ratio (width / height) beyond which the view narrows vertically: where the default
/// 75° vertical meets the 100° horizontal cap (≈ 1.55). Any setting uses the same aspect, so the
/// cap scales with the setting: a screen wider than this sees the horizontal FOV the setting has at
/// this aspect, and a narrower one the setting's vertical FOV.
pub fn wide_aspect() -> f64 {
    (MAX_HORIZONTAL_FOV.to_radians() / 2.0).tan() / (VERTICAL_FOV.to_radians() / 2.0).tan()
}

/// The setting clamped into its range (anything not a number: the default).
pub fn sanitize_fov_setting(deg: f64) -> f64 {
    if deg.is_finite() {
        deg.clamp(FOV_SETTING_RANGE.0, FOV_SETTING_RANGE.1)
    } else {
        VERTICAL_FOV
    }
}

/// The base vertical field of view (degrees) for the Field of view setting `setting_deg` at an
/// aspect ratio (width / height): the setting, narrowed on screens wider than `wide_aspect()` so the
/// horizontal FOV stays what the setting gives at that aspect (100° at the default 75°).
pub fn base_fov(setting_deg: f64, aspect: f64) -> f64 {
    let wide = wide_aspect();
    if aspect <= wide {
        return setting_deg;
    }
    let t = (setting_deg.to_radians() / 2.0).tan();
    2.0 * (t * wide / aspect).atan().to_degrees()
}

/// Vertical field of view (degrees) at the default setting for the given aspect ratio.
pub fn vertical_fov(aspect: f64) -> f64 {
    base_fov(VERTICAL_FOV, aspect)
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
    /// The spyglass (docs/SPYGLASS.md): how many times `fov_y_deg` is magnified (1: not zoomed),
    /// and how many times finer the sections inside the view refine (1 to `magnification`). Outside
    /// the view, detail is chosen for the unzoomed FOV, as without the spyglass.
    pub magnification: f64,
    pub detail_magnification: f64,
}

/// The drawing's field of view, aspect and height for LOD selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodViewport {
    pub fov_y_deg: f64,
    pub aspect: f64,
    pub height_px: f64,
}

/// The viewport from the drawing's size in CSS pixels at the Field of view setting (degrees). The
/// pixel error is in CSS pixels, whatever the screen's density: counted in device pixels, a 2×
/// screen asked for four times the sections.
pub fn lod_viewport(setting_deg: f64, css_width: f64, css_height: f64) -> LodViewport {
    let aspect = css_width / css_height.max(1.0);
    LodViewport {
        fov_y_deg: base_fov(setting_deg, aspect),
        aspect,
        height_px: css_height,
    }
}

/// Inward plane normals through the camera (sides) and the near plane, for AABB tests.
#[derive(Clone, Debug)]
pub struct Frustum {
    pub camera: LodCamera,
    planes: [Vec3; 5],
    /// Pixels per metre at 1 m distance inside the view: heightPx / (2 tan(fovY / 2)) for the
    /// unzoomed FOV, times the spyglass's detail magnification.
    pub pixels_per_radian: f64,
    /// The same outside the view: the unzoomed FOV's (equal to `pixels_per_radian` unzoomed).
    pub around_pixels_per_radian: f64,
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
        let magnification = sane_magnification(camera.magnification);
        let detail = sane_magnification(camera.detail_magnification).min(magnification);
        // The unzoomed view's pixels per radian: the drawn FOV widened back by the magnification.
        let around = camera.height_px / (2.0 * half * magnification);
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
            pixels_per_radian: around * detail,
            around_pixels_per_radian: around,
        }
    }

    /// Pixels per metre at 1 m for the box [lo, hi]: inside the view the zoomed detail's, outside
    /// it the unzoomed FOV's, so the spyglass refines only the narrow cone it shows.
    pub fn pixels_per_radian_for(&self, lo: Vec3, hi: Vec3) -> f64 {
        if self.pixels_per_radian > self.around_pixels_per_radian && self.intersects(lo, hi) {
            self.pixels_per_radian
        } else {
            self.around_pixels_per_radian
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

fn sane_magnification(m: f64) -> f64 {
    if m.is_finite() { m.max(1.0) } else { 1.0 }
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
        let v = lod_viewport(VERTICAL_FOV, 1440.0, 900.0);
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
            magnification: 1.0,
            detail_magnification: 1.0,
        });
        // A 1 m cell 1 km away covers under a CSS pixel there.
        assert!(cell.pixels_per_radian / 1000.0 < 1.0);
    }

    #[test]
    fn the_spyglass_measures_pixels_at_its_magnification_inside_the_view_only() {
        let cam = |fov: f64, m: f64, detail: f64| {
            Frustum::new(LodCamera {
                position: [0.0; 3],
                yaw_deg: 0.0,
                pitch_deg: 0.0,
                fov_y_deg: fov,
                aspect: 16.0 / 9.0,
                height_px: 900.0,
                magnification: m,
                detail_magnification: detail,
            })
        };
        let plain = cam(75.0, 1.0, 1.0);
        let narrow = 2.0 * ((37.5f64.to_radians()).tan() / 8.0).atan().to_degrees();
        let zoom = cam(narrow, 8.0, 8.0);
        let ahead = ([-1.0, -1.0, 999.0], [1.0, 1.0, 1001.0]);
        let behind = ([-1.0, -1.0, -1001.0], [1.0, 1.0, -999.0]);
        let ppr = |f: &Frustum, b: ([f64; 3], [f64; 3])| f.pixels_per_radian_for(b.0, b.1);
        assert!((ppr(&zoom, ahead) / ppr(&plain, ahead) - 8.0).abs() < 1e-9);
        assert!((ppr(&zoom, behind) - ppr(&plain, behind)).abs() < 1e-9);
        assert_eq!(ppr(&plain, ahead), ppr(&plain, behind));
        // The detail stops at the boost, and never exceeds the magnification.
        assert!((ppr(&cam(narrow, 8.0, 2.0), ahead) / ppr(&plain, ahead) - 2.0).abs() < 1e-9);
        assert!((ppr(&cam(narrow, 8.0, 50.0), ahead) / ppr(&plain, ahead) - 8.0).abs() < 1e-9);
        // The narrow FOV without its magnification refines everywhere (what the client must not do).
        assert!(ppr(&cam(narrow, 1.0, 1.0), behind) > 7.9 * ppr(&plain, behind));
    }

    #[test]
    fn caps_the_horizontal_field_of_view() {
        assert_eq!(vertical_fov(1.0), VERTICAL_FOV);
        let wide = vertical_fov(3.0);
        assert!(wide < VERTICAL_FOV);
        let horizontal = 2.0 * ((wide.to_radians() / 2.0).tan() * 3.0).atan();
        assert!((horizontal.to_degrees() - MAX_HORIZONTAL_FOV).abs() < 1e-9);
    }

    #[test]
    fn the_setting_is_the_base_and_the_wide_screen_cap_scales_with_it() {
        let h =
            |v: f64, aspect: f64| 2.0 * ((v.to_radians() / 2.0).tan() * aspect).atan().to_degrees();
        for setting in [60.0, 75.0, 100.0] {
            // Up to the wide aspect the base is the setting itself.
            for aspect in [0.5, 1.0, 4.0 / 3.0, wide_aspect()] {
                assert!((base_fov(setting, aspect) - setting).abs() < 1e-9);
            }
            // Wider screens keep the horizontal FOV the setting has at the wide aspect.
            let cap = h(setting, wide_aspect());
            for aspect in [16.0 / 9.0, 21.0 / 9.0] {
                let v = base_fov(setting, aspect);
                assert!(v < setting && (h(v, aspect) - cap).abs() < 1e-9);
            }
        }
        // The default is today's rule exactly: 75° vertical, at most 100° horizontal.
        assert!((h(VERTICAL_FOV, wide_aspect()) - MAX_HORIZONTAL_FOV).abs() < 1e-9);
        assert_eq!(sanitize_fov_setting(120.0), 100.0);
        assert_eq!(sanitize_fov_setting(f64::NAN), VERTICAL_FOV);
        assert_eq!(lod_viewport(90.0, 1000.0, 1000.0).fov_y_deg, 90.0);
    }
}
