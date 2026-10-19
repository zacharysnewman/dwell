//! Procedural meshes the renderer draws itself: player capsules (and their visor), the block
//! outline's box edges.

/// A mesh in the flat layout (positions, normals, colours, u32 indices).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatGeometry {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub colors: Vec<f32>,
    pub indices: Vec<u32>,
}

/// A vertical capsule of `radius` whose total height is `height`, its bottom at y = 0 (white:
/// the draw's tint colours it). `rings` per hemisphere, `segments` around.
pub fn capsule(radius: f32, height: f32, rings: u32, segments: u32) -> FlatGeometry {
    let half_cylinder = ((height - 2.0 * radius) / 2.0).max(0.005);
    let mut g = FlatGeometry::default();
    // Profile from the bottom pole to the top pole: (r, y, normal r, normal y).
    let mut profile = Vec::new();
    for i in 0..=rings {
        let a =
            -std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_2 * i as f32 / rings as f32;
        profile.push((
            radius * a.cos(),
            -half_cylinder + radius * a.sin(),
            a.cos(),
            a.sin(),
        ));
    }
    for i in 0..=rings {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / rings as f32;
        profile.push((
            radius * a.cos(),
            half_cylinder + radius * a.sin(),
            a.cos(),
            a.sin(),
        ));
    }
    let centre = radius + half_cylinder;
    for &(r, y, nr, ny) in &profile {
        for s in 0..=segments {
            let t = std::f32::consts::TAU * s as f32 / segments as f32;
            let (sin, cos) = t.sin_cos();
            g.positions.extend([r * sin, y + centre, r * cos]);
            g.normals.extend([nr * sin, ny, nr * cos]);
            g.colors.extend([1.0, 1.0, 1.0]);
        }
    }
    let row = segments + 1;
    for p in 0..profile.len() as u32 - 1 {
        for s in 0..segments {
            let (a, b) = (p * row + s, (p + 1) * row + s);
            // Counter-clockwise seen from outside (front faces; back faces are culled).
            g.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    g
}

/// The visor's colour (sRGB 0x1b1f2a, linear): a small dark capsule so facing is visible.
const VISOR: [f32; 3] = [0.0103, 0.0137, 0.0232];

/// A player as drawn (the Three.js renderer's group): a capsule body of `radius` and total
/// `height`, and a visor sideways at mid-body facing +Z, in feet-local coordinates (feet at the
/// origin, facing +Z at yaw 0). Dead: the body lies on the ground along Z (a cosmetic pose). Face
/// B (`face < 0`): mirrored to hang from the feet toward −y, winding kept front-facing. Colours are
/// baked in (`body` linear), so the draw's tint is white.
pub fn player_mesh(
    radius: f32,
    height: f32,
    dead: bool,
    body: [f32; 3],
    face: f32,
) -> FlatGeometry {
    let mut g = FlatGeometry::default();
    // Body-local: the body capsule centred on the origin, and the visor (rotated a quarter turn
    // about Z, at z = 0.8 r).
    let mut parts = vec![(
        centred(capsule(radius, height.max(2.0 * radius + 0.01), 6, 12)),
        body,
        false,
    )];
    let visor_r = radius * 0.35;
    parts.push((
        centred(capsule(visor_r, radius * 0.6 + 2.0 * visor_r, 4, 8)),
        VISOR,
        true,
    ));
    for (part, color, is_visor) in parts {
        let base = (g.positions.len() / 3) as u32;
        for (p, n) in part.positions.chunks(3).zip(part.normals.chunks(3)) {
            let (mut p, mut n) = ([p[0], p[1], p[2]], [n[0], n[1], n[2]]);
            if is_visor {
                // rotation.z = π/2: (x, y) → (−y, x); then placed at z = 0.8 r.
                p = [-p[1], p[0], p[2] + radius * 0.8];
                n = [-n[1], n[0], n[2]];
            }
            if dead {
                // rotation.x = π/2: (y, z) → (−z, y); the body's centre one radius up.
                p = [p[0], -p[2] + radius, p[1]];
                n = [n[0], -n[2], n[1]];
            } else {
                p[1] += height / 2.0;
            }
            if face < 0.0 {
                p[1] = -p[1];
                n[1] = -n[1];
            }
            g.positions.extend(p);
            g.normals.extend(n);
            g.colors.extend(color);
        }
        for t in part.indices.chunks(3) {
            // A mirror image reverses winding: swap two corners to keep faces front-facing.
            let t = if face < 0.0 {
                [t[0], t[2], t[1]]
            } else {
                [t[0], t[1], t[2]]
            };
            g.indices.extend(t.map(|i| i + base));
        }
    }
    g
}

/// A capsule moved so its centre is at the origin.
fn centred(mut g: FlatGeometry) -> FlatGeometry {
    let half = g.positions.chunks(3).map(|p| p[1]).fold(0.0f32, f32::max) / 2.0;
    for p in g.positions.chunks_mut(3) {
        p[1] -= half;
    }
    g
}

/// The twelve edges of the unit box [0, 1]³ (line-list positions), slightly enlarged as the
/// outline was (1.004 about the cell).
pub fn box_edges() -> Vec<[f32; 3]> {
    box_edges_about([0.0; 3], [1.004; 3])
}

/// The twelve edges of the box [lo, hi] (line-list positions).
pub fn box_edges_about(lo: [f32; 3], hi: [f32; 3]) -> Vec<[f32; 3]> {
    let c = |i: u32| {
        [
            if i & 1 != 0 { hi[0] } else { lo[0] },
            if i & 2 != 0 { hi[1] } else { lo[1] },
            if i & 4 != 0 { hi[2] } else { lo[2] },
        ]
    };
    let mut out = Vec::new();
    for a in 0..8u32 {
        for bit in [1, 2, 4] {
            if a & bit == 0 {
                out.push(c(a));
                out.push(c(a | bit));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(g: &FlatGeometry) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in g.positions.chunks(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    /// Every triangle's winding agrees with its vertices' normals (front faces outward).
    fn wound_outward(g: &FlatGeometry) -> bool {
        let v = |i: u32| {
            let i = i as usize * 3;
            [g.positions[i], g.positions[i + 1], g.positions[i + 2]]
        };
        g.indices.chunks(3).all(|t| {
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            let (e1, e2) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let i = t[0] as usize * 3;
            let vn = &g.normals[i..i + 3];
            let area = n.iter().map(|x| x * x).sum::<f32>();
            area < 1e-12 || n[0] * vn[0] + n[1] * vn[1] + n[2] * vn[2] > 0.0
        })
    }

    #[test]
    fn a_player_stands_on_its_feet_with_a_visor_facing_forward() {
        let g = player_mesh(0.4, 1.8, false, [1.0, 0.0, 0.0], 1.0);
        let (lo, hi) = bounds(&g);
        assert!(
            lo[1].abs() < 1e-4 && (hi[1] - 1.8).abs() < 1e-4,
            "{lo:?} {hi:?}"
        );
        // The visor sticks out in front (+Z) of the body.
        assert!(hi[2] > 0.4 + 0.05 && lo[2] > -0.41, "{lo:?} {hi:?}");
        assert!(wound_outward(&g));
    }

    #[test]
    fn a_dead_player_lies_on_the_ground() {
        let g = player_mesh(0.4, 1.8, true, [0.4; 3], 1.0);
        let (lo, hi) = bounds(&g);
        // Lying along Z: two radii tall, the body's length along Z.
        assert!(lo[1] > -0.35 && hi[1] < 0.81, "{lo:?} {hi:?}");
        assert!((hi[2] - lo[2] - 1.8).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!(wound_outward(&g));
    }

    #[test]
    fn on_face_b_a_player_hangs_from_its_feet_still_facing_outward() {
        let g = player_mesh(0.4, 1.8, false, [1.0; 3], -1.0);
        let (lo, hi) = bounds(&g);
        assert!(
            (lo[1] + 1.8).abs() < 1e-4 && hi[1].abs() < 1e-4,
            "{lo:?} {hi:?}"
        );
        assert!(wound_outward(&g));
    }

    #[test]
    fn a_capsule_spans_its_height_with_unit_normals_and_valid_indices() {
        let g = capsule(0.3, 1.8, 6, 12);
        let ys: Vec<f32> = g.positions.chunks(3).map(|p| p[1]).collect();
        let (lo, hi) = ys
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        assert!(lo.abs() < 1e-5 && (hi - 1.8).abs() < 1e-5, "{lo} {hi}");
        for n in g.normals.chunks(3) {
            assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-5);
        }
        let vertices = (g.positions.len() / 3) as u32;
        assert!(g.indices.iter().all(|&i| i < vertices) && g.indices.len().is_multiple_of(3));
    }

    #[test]
    fn the_box_has_twelve_edges() {
        assert_eq!(box_edges().len(), 24);
    }
}
