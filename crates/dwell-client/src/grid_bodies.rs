//! Other grids drawn up close (docs/proposals/MOON_WORLD.md §5, M-5): a grid with a `nearMesh`
//! (`grids.json`, the moon) is drawn nearer than its `farM` as a coarse mesh of its real surface,
//! both faces, from its generator through the same `FarSource` its far bake reads, in place of the
//! sky's far disc. The mesh is drawn as a piece at the grid's pose in the frame the view is drawn
//! in: at home the moon's orbit pose; on the moon itself the identity, where each vertex sits at the
//! lowest ground round it (over the cells about it) less `sinkM`, so the full-detail chunks cover it where they are and it
//! shows where they are not yet (nothing to swap at the capture: no pop).

use dwell_mesh::ChunkMeshes;
use dwell_mesh::tile_rect;
use dwell_world::{Face, FaceFrame, GridFrame};
use dwell_worldgen::grids::{FarSource, NearMesh};

type V3 = [f64; 3];
type Quat = [f64; 4];

/// The piece ids of the grids' meshes: `ID_BASE | grid` (in the loose items' bit range, so the
/// shadows' moving casters leave them out; the shadow maps skip them too).
pub const ID_BASE: u32 = 0xE000_0000;

/// The piece id of grid `grid`'s mesh.
pub fn id(grid: u8) -> u32 {
    ID_BASE | u32::from(grid)
}

/// Whether piece `id` is a grid's mesh.
pub fn is_grid_body(id: u32) -> bool {
    id & 0xFFFF_FF00 == ID_BASE
}

/// Grid `src`'s coarse mesh (grid coordinates, both faces): `near.cells` quads across its
/// diameter, each vertex the lowest of the ground over the cells round it (sampled at a quarter
/// cell, the detail at least an eighth of a cell wide) less `near.sink_m`; coloured by the ground's
/// albedo, normals by central differences; quads with a corner off the disc left out. Returns the
/// mesh and its radius round the grid's origin.
pub fn near_mesh(src: &(impl FarSource + ?Sized), near: NearMesh) -> (ChunkMeshes, f64) {
    let r = src.far_radius_m();
    let n = near.cells.max(2);
    let step = 2.0 * r / n as f64;
    let at = |i: usize| -r + i as f64 * step;
    let tile = tile_rect("plain");
    let mut m = ChunkMeshes::default();
    let o = &mut m.opaque;
    let mut radius: f64 = 0.0;
    for face in [Face::A, Face::B] {
        let f = FaceFrame::of(face);
        // The ground at a quarter cell's spacing (detail at least an eighth of a cell wide: the
        // craters left out are at most a fifth of that deep, under the sink), then each vertex the
        // lowest of it over the two cells each way round it, so every quad lies under the ground
        // it covers.
        let sub = 4 * n + 1;
        let fine = step / 4.0;
        let ground: Vec<f64> = (0..sub * sub)
            .map(|k| {
                let (x, z) = (-r + (k % sub) as f64 * fine, -r + (k / sub) as f64 * fine);
                src.far_height(face, x, z, step / 8.0)
            })
            .collect();
        let window = |v: &[f64], k: usize, stride: usize| {
            let lo = k.saturating_sub(4);
            let hi = (k + 4).min(sub - 1);
            (lo..=hi)
                .map(|q| v[q * stride])
                .fold(f64::INFINITY, f64::min)
        };
        // Separable: the rows' minima, then the columns'.
        let mut rows = vec![0.0; sub * sub];
        for zk in 0..sub {
            for xk in 0..sub {
                rows[zk * sub + xk] = window(&ground[zk * sub..], xk, 1);
            }
        }
        let side = n + 1;
        let heights: Vec<f64> = (0..side * side)
            .map(|k| window(&rows[4 * (k % side)..], 4 * (k / side), sub) - near.sink_m)
            .collect();
        let h = |i: usize, j: usize| heights[j.min(n) * side + i.min(n)];
        let base = (o.positions.len() / 3) as u32;
        for j in 0..side {
            for i in 0..side {
                let (x, z) = (at(i), at(j));
                let p = f.point([x, h(i, j), z]);
                radius = radius.max(p.iter().map(|c| c * c).sum::<f64>().sqrt());
                let dx = (h(i + 1, j) - h(i.saturating_sub(1), j)) / (2.0 * step);
                let dz = (h(i, j + 1) - h(i, j.saturating_sub(1))) / (2.0 * step);
                let l = (dx * dx + 1.0 + dz * dz).sqrt();
                let nrm = f.vector([-dx / l, 1.0 / l, -dz / l]);
                // Its water (home's seas and lakes) in the water block's colour (blocks.json).
                let c = water_mixed(src.far_albedo(face, x, z), src.far_water(face, x, z, step));
                o.positions.extend(p.map(|v| v as f32));
                o.normals.extend(nrm.map(|v| v as f32));
                o.colors.extend(c.map(|v| v as f32));
                // The tile repeats per metre, as the chunks' faces do.
                o.uvs.extend([x as f32, z as f32]);
                o.tiles.extend([
                    tile.u0 as f32,
                    tile.v0 as f32,
                    (tile.u1 - tile.u0) as f32,
                    (tile.v1 - tile.v0) as f32,
                ]);
                o.light.push(1.0);
                o.flood.extend([1.0, 0.0]);
            }
        }
        let inside = |i: usize, j: usize| at(i).hypot(at(j)) <= r;
        let v = |i: usize, j: usize| base + (j * side + i) as u32;
        for j in 0..n {
            for i in 0..n {
                if !(inside(i, j) && inside(i + 1, j) && inside(i, j + 1) && inside(i + 1, j + 1)) {
                    continue;
                }
                let (a, b, c, d) = (v(i, j), v(i, j + 1), v(i + 1, j + 1), v(i + 1, j));
                // Counter-clockwise seen from over the face; a face B point is mirrored, so its
                // winding turns.
                if f.is_b() {
                    o.indices.extend([a, c, b, a, d, c]);
                } else {
                    o.indices.extend([a, b, c, a, c, d]);
                }
            }
        }
    }
    (m, radius)
}

/// `albedo` with a `share` of the ground under water in the water block's colour.
fn water_mixed(albedo: [f64; 3], share: f64) -> [f64; 3] {
    if share <= 0.0 {
        return albedo;
    }
    let c = dwell_blocks::info(dwell_blocks::require("dwell:water")).color;
    let w = [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| f64::from(v) / 255.0);
    let k = share.clamp(0.0, 1.0);
    std::array::from_fn(|i| albedo[i] + (w[i] - albedo[i]) * k)
}

fn qmul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Grid `g`'s pose in the frame the view is drawn in (`drawn`'s local frame): the point its
/// origin lands on and its rotation (x, y, z, w), so a grid point p is drawn at
/// `origin + rotation · p`.
pub fn pose_in(drawn: &GridFrame, g: &GridFrame) -> (V3, [f32; 4]) {
    let origin = drawn.to_grid(g.origin);
    let [x, y, z, w] = drawn.rotation;
    let q = qmul([-x, -y, -z, w], g.rotation);
    let l = q.iter().map(|c| c * c).sum::<f64>().sqrt().max(1e-12);
    (origin, q.map(|c| (c / l) as f32))
}

/// Which grids' meshes to draw: each grid `g` with a near mesh (`with_mesh`: its index, its
/// frame and its `farM`, `None` for a grid the sky draws no disc of, home) is drawn when its
/// centre `centre` (its grid-local point) is nearer the eye than its `farM`, or at every distance
/// without one; never the grid the view is on (its own LOD draws it: home's tree, another grid's
/// rings, `grid_lod.rs`). The eye is in the drawn grid's frame.
pub fn shown(
    drawn: u8,
    drawn_frame: &GridFrame,
    eye: V3,
    with_mesh: &[(u8, GridFrame, Option<f64>)],
    centre: V3,
) -> Vec<(u8, V3, [f32; 4])> {
    with_mesh
        .iter()
        .filter_map(|&(g, frame, far_m)| {
            if g == drawn {
                return None;
            }
            let (origin, quat) = pose_in(drawn_frame, &frame);
            let c = drawn_frame.to_grid(frame.to_world(centre));
            let d = (0..3).map(|k| (c[k] - eye[k]).powi(2)).sum::<f64>().sqrt();
            far_m.is_none_or(|far| d < far).then_some((g, origin, quat))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_worldgen::grids::{CrateredTerrain, grids};

    fn moon() -> (CrateredTerrain, NearMesh) {
        let def = grids().iter().find(|g| g.name == "dwell:moon").unwrap();
        (
            CrateredTerrain::new(def, 7).unwrap(),
            def.near_mesh.unwrap(),
        )
    }

    fn rotate(q: [f32; 4], p: V3) -> V3 {
        let q = q.map(f64::from);
        let v = qmul(
            qmul(q, [p[0], p[1], p[2], 0.0]),
            [-q[0], -q[1], -q[2], q[3]],
        );
        [v[0], v[1], v[2]]
    }

    dwell_world::both_faces!(
        /// The coarse mesh lies under the moon's real ground on face `f` everywhere the chunks
        /// would be (the vertices and the triangles' middles at least a metre under the ground there, 99.5% of them, and none over it by 4 m: a crater narrower than the samples can lie in a quad), within the sink plus a
        /// few metres of it (it is the surface, not a slab), faces outward, and covers the disc.
        fn the_moons_near_mesh_lies_just_under_its_ground(f: FaceFrame) {
            let (t, near) = moon();
            let (m, radius) = near_mesh(&t, near);
            let o = &m.opaque;
            let verts = o.positions.len() / 3;
            assert_eq!(o.normals.len(), verts * 3);
            assert_eq!(o.colors.len(), verts * 3);
            assert_eq!(o.uvs.len(), verts * 2);
            assert_eq!(o.tiles.len(), verts * 4);
            assert_eq!(o.light.len(), verts);
            assert_eq!(o.flood.len(), verts * 2);
            assert!(radius > t.radius_m() * 0.99, "{radius}");
            let mut on_face = 0;
            let (mut tris_up, mut tris, mut under, mut points) = (0, 0, 0, 0);
            let up = f.vector([0.0, 1.0, 0.0]);
            for tri in o.indices.chunks(3) {
                let g = |k: usize| {
                    let i = tri[k] as usize * 3;
                    [0, 1, 2].map(|c| f64::from(o.positions[i + c]))
                };
                let (ga, gb, gc) = (g(0), g(1), g(2));
                let (a, b, c) = (f.point(ga), f.point(gb), f.point(gc));
                // Face-local: this face's triangles are over the midplane.
                if a[1] < dwell_world::MIDPLANE_M {
                    continue;
                }
                tris += 1;
                // Counter-clockwise from outside (in the grid's coordinates): the cross product
                // points out of the face, as a chunk's tops.
                let u: V3 = std::array::from_fn(|k| gb[k] - ga[k]);
                let v: V3 = std::array::from_fn(|k| gc[k] - ga[k]);
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                tris_up += usize::from(n[0] * up[0] + n[1] * up[1] + n[2] * up[2] > 0.0);
                let mid: V3 = std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0);
                for q in [a, b, c, mid] {
                    let ground = t.height(f.face, q[0], q[2]);
                    points += 1;
                    under += usize::from(q[1] <= ground - 1.0);
                    // A crater narrower than the samples can lie in a quad: never far over it.
                    assert!(q[1] <= ground + 4.0, "{q:?} over {ground}");
                    on_face += usize::from(q[1] > ground - near.sink_m - 60.0);
                }
            }
            assert!(tris > 10_000, "the disc covered: {tris}");
            assert_eq!(tris_up, tris, "every triangle faces out of the face");
            // Nearly all of it a metre or more under the ground (the chunks cover it).
            assert!(under * 1000 >= points * 995, "{under} of {points} under");
            assert!(
                on_face * 10 > points * 9,
                "it follows the ground: {on_face}"
            );
        }
    );

    // both-faces: frames only; no face enters.
    #[test]
    fn a_grids_pose_draws_its_points_where_its_frame_puts_them() {
        let drawn = GridFrame {
            origin: [1000.0, -50.0, 20.0],
            rotation: dwell_world::quat_axis_angle([0.0, 1.0, 0.0], 0.3),
            ..GridFrame::HOME
        };
        let g = GridFrame {
            origin: [400_000.0, 300_000.0, -200_000.0],
            rotation: dwell_world::quat_axis_angle([0.6, 0.0, 0.8], 1.1),
            ..GridFrame::HOME
        };
        let (origin, q) = pose_in(&drawn, &g);
        for p in [
            [0.0, 0.0, 0.0],
            [12_000.0, 300.0, -5000.0],
            [-3.0, -9000.0, 7.0],
        ] {
            let want = drawn.to_grid(g.to_world(p));
            let r = rotate(q, p);
            let got = [origin[0] + r[0], origin[1] + r[1], origin[2] + r[2]];
            let err = (0..3).map(|k| (got[k] - want[k]).abs()).fold(0.0, f64::max);
            assert!(err < 0.05, "{p:?}: {got:?} vs {want:?}");
        }
        // Home seen from home: the identity.
        let (o, q) = pose_in(&GridFrame::HOME, &GridFrame::HOME);
        assert_eq!((o, q), ([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
    }

    // both-faces: frames only; no face enters.
    #[test]
    fn the_mesh_shows_nearer_than_far_m_never_on_its_grid_and_home_from_anywhere() {
        let moon = GridFrame {
            origin: [500_000.0, 0.0, 0.0],
            ..GridFrame::HOME
        };
        let list = [(0u8, GridFrame::HOME, None), (1u8, moon, Some(150_000.0))];
        let centre = [0.0, 0.0, 0.0];
        assert!(shown(0, &GridFrame::HOME, [0.0; 3], &list, centre).is_empty());
        let near = shown(0, &GridFrame::HOME, [400_000.0, 0.0, 0.0], &list, centre);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].1, [500_000.0, 0.0, 0.0]);
        // On the moon: not its own mesh (its rings draw it), but home, at home's place in the
        // moon's frame, however far.
        let on = shown(1, &moon, [10.0, 80.0, 0.0], &list, centre);
        assert_eq!(on, vec![(0, [-500_000.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])]);
    }
}
