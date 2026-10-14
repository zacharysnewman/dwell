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
            g.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    g
}

/// The twelve edges of the unit box [0, 1]³ (line-list positions), slightly enlarged as the
/// outline was (1.004 about the cell).
pub fn box_edges() -> Vec<[f32; 3]> {
    let (lo, hi) = (0.0, 1.004);
    let c = |i: u32| {
        [
            if i & 1 != 0 { hi } else { lo },
            if i & 2 != 0 { hi } else { lo },
            if i & 4 != 0 { hi } else { lo },
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
