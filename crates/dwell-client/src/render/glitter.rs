//! The sun and moon on the water (docs/WATER_RENDERING.md §15): on a tier with `bodyPath`
//! (`shared/look/reflections.json`), the disc mirrored in the reflected sky and a glitter path, a
//! GGX lobe of the disc's light off the rippled normal (the WGSL `body_paths`, `reflected_bodies`).
//! The tested twins here are face-local and cross into the world through `FaceFrame`.

use dwell_look::reflections::{BodyPath, REFLECTIONS};
use dwell_world::FaceFrame;

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn schlick(cos: f64, f0: f64) -> f64 {
    f0 + (1.0 - f0) * (1.0 - cos.clamp(0.0, 1.0)).powi(5)
}

/// Where a calm surface shows a body toward `body` (world, unit): its direction with the face-local
/// up component negated, so a body `e` over the face's horizon is seen `e` under it.
pub fn mirror_dir(f: FaceFrame, body: V3) -> V3 {
    let b = f.vector(body);
    f.vector([b[0], -b[1], b[2]])
}

/// The WGSL `ggx_glint`'s twin (height-correlated Smith, Schlick Fresnel; not clamped).
pub fn ggx(n: V3, v: V3, l: V3, a: f64, f0: f64) -> f64 {
    let nl = dot(n, l);
    if nl <= 0.0 {
        return 0.0;
    }
    let h = normalize([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    let nv = dot(n, v).max(1e-3);
    let nh = dot(n, h).max(0.0);
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    let dist = a2 / (std::f64::consts::PI * d * d);
    let vis = 0.5
        / (nl * (nv * nv * (1.0 - a2) + a2).sqrt() + nv * (nl * nl * (1.0 - a2) + a2).sqrt());
    dist * vis * schlick(dot(v, h), f0) * nl
}

/// The path lobe's α for a body over water whose faded ripple layers lost `lost` slope variance
/// (Toksvig, as the glint's): rougher water spreads the path wider and dimmer.
pub fn path_alpha(p: &BodyPath, lost: f64) -> f64 {
    (p.path_alpha * p.path_alpha + 2.0 * lost).sqrt()
}

/// The path's light on a calm surface at face-local `(x, z)` (the surface at face-local height 0)
/// for an eye `eye_h` over it at the origin and a body toward `body` (world, unit).
pub fn path_at(f: FaceFrame, eye_h: f64, x: f64, z: f64, body: V3, a: f64) -> f64 {
    let n = f.vector([0.0, 1.0, 0.0]);
    let v = f.vector(normalize([-x, eye_h, -z]));
    ggx(n, v, body, a, 0.02)
}

/// The constants the WGSL takes from the data: per body (strength, α, whole under, gone over:
/// radians of elevation), and the clouds' fade.
pub fn wgsl() -> String {
    let b = REFLECTIONS.bodies;
    let body = |name: &str, p: BodyPath| {
        format!(
            "const {name}_PATH: vec4f = vec4f({:?}, {:?}, {:?}, {:?});\n",
            p.strength as f32,
            p.path_alpha as f32,
            p.full_below_deg.to_radians() as f32,
            p.gone_above_deg.to_radians() as f32
        )
    };
    format!(
        "{}{}const PATH_CLOUD_FADE: f32 = {:?};\n",
        body("SUN", b.sun),
        body("MOON", b.moon),
        b.cloud_fade as f32
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body at azimuth `az` (radians from +x toward +z) and elevation `e` in the face-local frame,
    /// as a world vector through `f`.
    fn toward(f: FaceFrame, az: f64, e: f64) -> V3 {
        f.vector([e.cos() * az.cos(), e.sin(), e.cos() * az.sin()])
    }

    dwell_world::both_faces!(
        fn a_body_at_elevation_e_is_mirrored_at_minus_e(f: FaceFrame) {
            for (az, e) in [(0.3, 0.2), (2.0, 0.6), (-1.0, 1.2)] {
                let m = f.vector(mirror_dir(f, toward(f, az, e)));
                assert!((m[1] + e.sin()).abs() < 1e-12, "seen at −e: {m:?}");
                let want = [e.cos() * az.cos(), e.cos() * az.sin()];
                assert!((m[0] - want[0]).abs() < 1e-12 && (m[2] - want[1]).abs() < 1e-12);
                // On a calm surface the mirrored ray is the reflection of the view toward it.
                let w = mirror_dir(f, toward(f, az, e));
                let up = f.vector([0.0, 1.0, 0.0]);
                let d = dot(w, up);
                let r = [w[0] - 2.0 * d * up[0], w[1] - 2.0 * d * up[1], w[2] - 2.0 * d * up[2]];
                let b = toward(f, az, e);
                assert!((0..3).all(|i| (r[i] - b[i]).abs() < 1e-12));
            }
        }
    );

    dwell_world::both_faces!(
        fn the_glitter_path_peaks_along_the_viewer_body_azimuth(f: FaceFrame) {
            // A low moon at azimuth 0.7 rad; the eye 2 m over calm water. Sample the surface around
            // the eye: the brightest point lies on the line toward the body's azimuth, and the
            // streak is longer along it than across.
            let az = 0.7;
            let b = toward(f, az, 0.15);
            let a = path_alpha(&REFLECTIONS.bodies.moon, 0.0);
            let mut best = (0.0, 0.0, 0.0);
            for i in -200..=200 {
                for j in -200..=200 {
                    let (x, z) = (f64::from(i) * 0.25, f64::from(j) * 0.25);
                    let p = path_at(f, 2.0, x, z, b, a);
                    if p > best.0 {
                        best = (p, x, z);
                    }
                }
            }
            let (peak, x, z) = best;
            assert!(peak > 0.0);
            let got = z.atan2(x);
            assert!((got - az).abs() < 0.03, "peak at azimuth {got}, want {az}");
            // Along the line the path stays bright further than across it.
            let r = (x * x + z * z).sqrt();
            let (ux, uz) = (az.cos(), az.sin());
            let along = path_at(f, 2.0, ux * r * 1.5, uz * r * 1.5, b, a);
            let across = path_at(f, 2.0, ux * r - uz * r * 0.5, uz * r + ux * r * 0.5, b, a);
            assert!(along > across * 4.0, "along {along}, across {across}");
        }
    );

    dwell_world::both_faces!(
        fn calm_water_lays_a_brighter_path_than_rough(f: FaceFrame) {
            let b = toward(f, 0.0, 0.2);
            let m = &REFLECTIONS.bodies.moon;
            let peak = |lost: f64| {
                let a = path_alpha(m, lost);
                (0..400)
                    .map(|i| path_at(f, 2.0, f64::from(i) * 0.1, 0.0, b, a))
                    .fold(0.0, f64::max)
            };
            assert!(peak(0.0) > peak(0.05) * 2.0);
        }
    );

    #[test]
    fn the_wgsl_takes_the_data() {
        let w = wgsl();
        assert!(w.contains("const SUN_PATH: vec4f") && w.contains("const MOON_PATH: vec4f"));
        assert!(w.contains(&format!(
            "{:?}",
            REFLECTIONS.bodies.moon.strength as f32
        )));
        assert!(w.contains("PATH_CLOUD_FADE"));
    }
}
