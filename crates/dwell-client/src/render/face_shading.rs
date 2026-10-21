//! The shaders' face-dependent shading pieces (BIFACIAL_WORLD.md §11): each is written in the
//! fragment's face frame (the WGSL face frame, `dwell_world::FaceFrame::wgsl`), so face B shades as
//! face A's mirror image. [`WGSL`] is the shader code; the functions here are its Rust twins, which
//! the tests run on both faces.

use dwell_world::FaceFrame;

/// The shader functions (after the face frame's):
///
/// - `sway_drop(frag_y, lean, height)`: how far a swaying plant's vertex drops toward its ground
///   ([`sway_drop`]);
/// - `ripple_normal(face, seen, w)`: a water surface's normal rippled by the slope `w`, toward the
///   side seen ([`ripple_normal`]);
/// - `sparkle_normal(face, n, d)`: a ripple normal tilted by a sparkle facet's slope `d`
///   ([`sparkle_normal`]).
pub const WGSL: &str = r"
fn sway_drop(frag_y: f32, lean: f32, height: f32) -> vec3f {
    return vec3f(0.0, -0.4 * lean * lean / max(height, 1e-3), 0.0);
}
fn ripple_normal(face: f32, seen: vec3f, w: vec2f) -> vec3f {
    return sign(seen.y) * normalize(vec3f(-w.x, 1.0, -w.y));
}
fn sparkle_normal(face: f32, n: vec3f, d: vec2f) -> vec3f {
    return normalize(n + vec3f(-d.x, 0.0, -d.y) * 0.35 * sign(n.y));
}
";

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / l)
}

/// Swaying shortens a plant a little, as a bent blade does: its vertex at world height `frag_y`,
/// leaning `lean` m on a plant `height` m tall, drops this far (a world offset).
pub fn sway_drop(_frag_y: f64, lean: f64, height: f64) -> [f64; 3] {
    [0.0, -0.4 * lean * lean / height.max(1e-3), 0.0]
}

/// A water top's or bottom's normal on `face`, rippled by the surface's slope `w` (x, z), toward
/// the side `seen` (the face's normal toward the eye).
pub fn ripple_normal(_face: FaceFrame, seen: [f64; 3], w: [f64; 2]) -> [f64; 3] {
    normalize([-w[0], 1.0, -w[1]]).map(|c| c * seen[1].signum())
}

/// A ripple normal `n` on `face` tilted by a sparkle facet's slope `d` (x, z), away from the side
/// it faces.
pub fn sparkle_normal(_face: FaceFrame, n: [f64; 3], d: [f64; 2]) -> [f64; 3] {
    let s = 0.35 * n[1].signum();
    normalize([n[0] - d[0] * s, n[1], n[2] - d[1] * s])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) {
        let d = (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0, f64::max);
        assert!(d < 1e-9, "{a:?} vs {b:?}");
    }

    dwell_world::both_faces!(
        /// A plant on face `f` sways as its face-A twin does: face-local, its vertex drops toward
        /// its own ground.
        fn a_plant_sways_toward_its_own_ground(f: FaceFrame) {
            for (h, lean, height) in [(70.0, 0.3, 0.8), (3.2, 0.05, 0.4)] {
                let local = sway_drop(FaceFrame::A.y(h), lean, height);
                assert!(local[1] < 0.0);
                close(f.vector(sway_drop(f.y(h), lean, height)), local);
            }
        }
    );

    dwell_world::both_faces!(
        /// Water on face `f` ripples as its face-A twin does, seen from above or below: face-local,
        /// the rippled normal and its sparkle facets are face A's.
        fn water_ripples_and_sparkles_as_its_mirror_image(f: FaceFrame) {
            let a = FaceFrame::A;
            for seen in [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0]] {
                for w in [[0.2, -0.1], [-0.05, 0.3]] {
                    let local = ripple_normal(a, seen, w);
                    let n = ripple_normal(f, f.vector(seen), w);
                    close(f.vector(n), local);
                    let d = [0.4, -0.7];
                    close(
                        f.vector(sparkle_normal(f, n, d)),
                        sparkle_normal(a, local, d),
                    );
                }
            }
        }
    );

    #[test]
    fn the_shaders_draw_with_these_pieces() {
        let src = super::super::shaders::source(1);
        assert!(src.contains(WGSL));
        for call in [
            "sway_drop(frame.eye.x + rel.y, lean, plant.x)",
            "ripple_normal(s.up, seen, s.w.xy)",
            "sparkle_normal(s.up, s.n, d.yz)",
        ] {
            assert!(src.contains(call), "the shaders call {call}");
        }
    }
}
