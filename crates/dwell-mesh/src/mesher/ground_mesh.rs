//! Ground items baked into a chunk's opaque mesh (docs/adr/0071-ground-items.md): the scatter
//! (`dwell_world::ground`) read from the job's sky volume, each item a few small polygons in the
//! face-local frame, so drawing hundreds costs no draw call and no instance buffer of its own.

use super::{Kinds, MeshArrays, N, surface};
use crate::flood::Flood;
use dwell_blocks::find_block;
use dwell_world::ground::{GroundShape, Placed, Rules};
use dwell_world::{CELL_H, SKY_APRON, SKY_EXTENT, SKY_VOLUME};

/// A mesh job's ground items: the world seed and the chunk's gone cells (chunk-local, world-oriented).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroundJob {
    pub seed: u64,
    pub gone: Vec<[i32; 3]>,
}

/// The ground items of the chunk at `origin` (world cells), read from its world-oriented sky
/// volume `padded` (a smaller apron: none, as the rules look up to the sky apron's reach).
pub fn scatter(padded: &[u16], origin: [i32; 3], job: &GroundJob) -> Vec<Placed> {
    if padded.len() != SKY_VOLUME {
        return Vec::new();
    }
    let [ex, ey, ez] = SKY_EXTENT;
    let [ax, ay, az] = SKY_APRON;
    let mut get = |c: [i32; 3]| {
        let (x, y, z) = (c[0] - origin[0] + ax, c[1] - origin[1] + ay, c[2] - origin[2] + az);
        ((0..ex).contains(&x) && (0..ey).contains(&y) && (0..ez).contains(&z))
            .then(|| padded[(x + ex * (z + ez * y)) as usize])
    };
    let c = dwell_world::ChunkCoord::from_array(origin.map(|v| v.div_euclid(N)));
    let mut out = Rules::shipped().scatter_chunk(&mut get, job.seed, c);
    if !job.gone.is_empty() {
        out.retain(|p| {
            let l = [p.cell[0] - origin[0], p.cell[1] - origin[1], p.cell[2] - origin[2]];
            !job.gone.contains(&l)
        });
    }
    out
}

/// Appends `items` to `m` (chunk-local cells, face-local: y up, rows), `mirror` for face B's chunk.
pub(super) fn append(
    m: &mut MeshArrays,
    kinds: &Kinds,
    flood: &Flood,
    items: &[Placed],
    origin: [i32; 3],
    mirror: bool,
) {
    let rules = Rules::shipped();
    for it in items {
        let Some(k) = rules.kind(it.kind) else {
            continue;
        };
        let Some(b) = find_block(k.texture) else {
            continue;
        };
        let style = &kinds.of(b.first_state).style;
        let (_, tile) = surface(style, 0, 1);
        let mut l = [
            it.cell[0] - origin[0],
            it.cell[1] - origin[1],
            it.cell[2] - origin[2],
        ];
        if mirror {
            l[1] = N - 1 - l[1];
        }
        let lit = flood.cell(l);
        let base = [
            f64::from(l[0]) + 0.5 + it.offset[0],
            f64::from(l[1]) * CELL_H,
            f64::from(l[2]) + 0.5 + it.offset[1],
        ];
        let size = k.size.map(|v| v * it.scale);
        let (s, c) = it.yaw.sin_cos();
        let place = |p: [f64; 3]| -> [f64; 3] {
            let (x, z) = (p[0] * c - p[2] * s, p[0] * s + p[2] * c);
            [base[0] + x, base[1] + p[1], base[2] + z]
        };
        let color = k.color.map(|v| f64::from(v) / 255.0);
        for face in polygons(k.shape, size) {
            let pts: Vec<[f64; 3]> = face.iter().map(|&p| place(p)).collect();
            push_polygon(m, &pts, color, &tile, lit);
        }
    }
}

/// The shape's faces (convex, counter-clockwise from outside), item-local metres: its base's
/// middle at the origin, +y up. The underside is left out: it lies on the ground.
fn polygons(shape: GroundShape, [sx, sy, sz]: [f64; 3]) -> Vec<Vec<[f64; 3]>> {
    match shape {
        GroundShape::Rod => {
            let (x, z) = (sx / 2.0, sz / 2.0);
            let c = |i: usize| -> [f64; 3] {
                [
                    if i & 1 != 0 { x } else { -x },
                    if i & 2 != 0 { sy } else { 0.0 },
                    if i & 4 != 0 { z } else { -z },
                ]
            };
            vec![
                vec![c(2), c(6), c(7), c(3)], // top
                vec![c(1), c(3), c(7), c(5)], // +x
                vec![c(0), c(4), c(6), c(2)], // −x
                vec![c(4), c(5), c(7), c(6)], // +z
                vec![c(0), c(2), c(3), c(1)], // −z
            ]
        }
        GroundShape::Lump => {
            // Three rings of a squat hexagon: a lump wider at its middle, rounded at the top.
            let ring = |y: f64, r: f64| -> Vec<[f64; 3]> {
                (0..6)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / 6.0 + 0.3;
                        [a.cos() * sx / 2.0 * r, y, a.sin() * sz / 2.0 * r]
                    })
                    .collect()
            };
            let (lo, mid, hi) = (ring(0.0, 0.8), ring(sy * 0.5, 1.0), ring(sy, 0.55));
            let mut out = Vec::new();
            for i in 0..6 {
                let j = (i + 1) % 6;
                out.push(vec![lo[i], mid[i], mid[j], lo[j]]);
                out.push(vec![mid[i], hi[i], hi[j], mid[j]]);
            }
            out.push(hi.iter().rev().copied().collect());
            out
        }
    }
}

fn push_polygon(
    m: &mut MeshArrays,
    pts: &[[f64; 3]],
    color: [f64; 3],
    tile: &crate::textures::TileRect,
    lit: [f32; 2],
) {
    // Its normal in metres (the points are), from Newell's sum.
    let mut n = [0.0f64; 3];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
    let n = n.map(|v| v / len);
    let tint = dwell_look::normal_tint(n[0], n[1], n[2]);
    let base = (m.positions.len() / 3) as u32;
    for p in pts {
        // Cell space: rows up (the mesh is turned into metres after).
        m.positions
            .extend([p[0] as f32, (p[1] / CELL_H) as f32, p[2] as f32]);
        m.normals.extend(n.map(|v| v as f32));
        m.colors.extend([
            (color[0] * tint[0]) as f32,
            (color[1] * tint[1]) as f32,
            (color[2] * tint[2]) as f32,
        ]);
        // In blocks, projected along the dominant axis; a few times the tile's density, so the
        // grain shows at their size.
        let (u, v) = if n[1].abs() > 0.6 {
            (p[0], p[2])
        } else if n[0].abs() > n[2].abs() {
            (p[2], p[1])
        } else {
            (p[0], p[1])
        };
        m.uvs.extend([(u * 3.0) as f32, (v * 3.0) as f32]);
        m.tiles.extend([
            tile.u0 as f32,
            tile.v0 as f32,
            (tile.u1 - tile.u0) as f32,
            (tile.v1 - tile.v0) as f32,
        ]);
        m.light.push(1.0);
        m.flood.extend(lit);
    }
    for i in 1..pts.len().saturating_sub(1) as u32 {
        m.indices.extend([base, base + i, base + i + 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_of_both_shapes_faces_outward() {
        // both-faces: item-local geometry, mirrored with the chunk's mesh.
        for shape in [GroundShape::Rod, GroundShape::Lump] {
            let size = [0.4, 0.1, 0.2];
            let faces = polygons(shape, size);
            let mut m = MeshArrays::default();
            for f in &faces {
                push_polygon(&mut m, f, [1.0; 3], &crate::textures::tile_rect("plain"), [1.0, 0.0]);
            }
            // Each face's normal points away from the shape's middle.
            let mut at = 0;
            for f in &faces {
                let c: [f64; 3] =
                    std::array::from_fn(|k| f.iter().map(|p| p[k]).sum::<f64>() / f.len() as f64);
                let n = &m.normals[at * 3..at * 3 + 3];
                let out = [c[0], c[1] - size[1] / 2.0, c[2]];
                let dot: f64 = (0..3).map(|k| f64::from(n[k]) * out[k]).sum();
                assert!(dot > 0.0, "{shape:?} face {f:?} normal {n:?}");
                at += f.len();
            }
        }
    }
}
