//! The baked ambient light (docs/DAY_NIGHT.md §10): corner occlusion, the diagonal it splits quads
//! along, and the sky visibility of open ground, overhangs and caves, across chunk borders, after
//! edits and on both faces.

use super::*;
use crate::mesher::flip_diagonal;
use crate::{MeshArrays, PADDED_VOLUME, mesh_chunk_at, padded_index};
use dwell_world::{
    CELL_H, CHUNK_SIZE, ChunkCoord, FaceFrame, MIDPLANE_Y, SKY_APRON, VoxelWorld, empty_generator,
};

const N: i32 = CHUNK_SIZE;

/// A full cube that blocks light.
fn cube() -> u16 {
    dwell_blocks::require("dwell:sandbox_floor")
}

/// An occupancy of the one-voxel padded block with `cells` (chunk-local) solid.
fn padded_occupancy(cells: &[[i32; 3]]) -> Occupancy {
    let mut v = vec![0u16; PADDED_VOLUME];
    for &[x, y, z] in cells {
        v[padded_index(x, y, z)] = 1;
    }
    Occupancy::new(&v, [crate::PAD as i32; 3], [1; 3], false, |id| id == 1)
}

#[test]
fn corner_levels_count_the_three_cells_around_the_corner() {
    // face-a-only: the occupancy is face-local; the faces' meshes are compared in the tests below.
    // A top face's front cell (5, 5, 5); its corner toward −x, −z.
    let front = [5, 5, 5];
    let level = |cells: &[[i32; 3]]| padded_occupancy(cells).corner_ao(front, 0, -1, 2, -1);
    let (side_x, side_z, corner) = ([4, 5, 5], [5, 5, 4], [4, 5, 4]);
    assert_eq!(level(&[]), 3, "open");
    assert_eq!(level(&[side_x]), 2, "one side");
    assert_eq!(level(&[side_z]), 2, "the other side");
    assert_eq!(level(&[corner]), 2, "the corner alone");
    assert_eq!(level(&[side_x, corner]), 1, "a side and the corner");
    assert_eq!(level(&[side_x, side_z]), 0, "both sides: a full crease");
    assert_eq!(level(&[side_x, side_z, corner]), 0);
    // Cells on the other side of the corner, or behind the face, do not count.
    assert_eq!(level(&[[6, 5, 5], [5, 5, 6], [4, 4, 4], [5, 4, 5]]), 3);
    // What each level leaves of the ambient: all when open, less in deeper creases.
    let f = |l| AMBIENT.ao_factor(l);
    assert!(f(3) == 1.0 && f(2) < f(3) && f(1) < f(2) && f(0) < f(1));
}

#[test]
fn quads_split_along_their_brighter_diagonal() {
    // face-a-only: a rule on four numbers; the mirror parity test checks the meshes' indices.
    assert!(flip_diagonal([0.2, 1.0, 1.0, 1.0]), "a dark corner 0: 1–3");
    assert!(flip_diagonal([1.0, 1.0, 0.2, 1.0]), "a dark corner 2: 1–3");
    assert!(!flip_diagonal([1.0, 0.2, 1.0, 1.0]), "a dark corner 1: 0–2");
    assert!(!flip_diagonal([1.0, 1.0, 1.0, 1.0]), "level: as before");
    assert!(
        !flip_diagonal([0.5, 0.5, 1.0, 1.0]),
        "a gradient: either, as before"
    );
}

/// A world of face-local cells on `f`'s face (local rows: the face-A row they mirror).
struct Fixture {
    f: FaceFrame,
    world: VoxelWorld,
}

/// The face-local chunk row the fixtures are built in, and its first row.
const CHUNK_ROW: i32 = MIDPLANE_Y.div_euclid(N) + 2;
const BASE: i32 = CHUNK_ROW * N;

impl Fixture {
    fn new(f: FaceFrame) -> Self {
        Self {
            f,
            world: VoxelWorld::new(empty_generator(), None),
        }
    }

    fn put(&mut self, [x, y, z]: [i32; 3], id: u16) {
        let [wx, wy, wz] = self.f.cell([x, y, z]);
        self.world.set_voxel(wx, wy, wz, id);
    }

    fn fill(&mut self, lo: [i32; 3], hi: [i32; 3], id: u16) {
        for y in lo[1]..hi[1] {
            for z in lo[2]..hi[2] {
                for x in lo[0]..hi[0] {
                    self.put([x, y, z], id);
                }
            }
        }
    }

    /// The opaque mesh of face-local chunk (cx, CHUNK_ROW + dy, cz), meshed as the client does,
    /// with the world position (m) of its origin.
    fn mesh(&mut self, cx: i32, dy: i32, cz: i32) -> (MeshArrays, [f64; 3]) {
        let c = self.f.chunk(ChunkCoord::new(cx, CHUNK_ROW + dy, cz));
        let block = self.world.padded_box(c, SKY_APRON);
        let m = mesh_chunk_at(&block, self.f.is_b(), [c.x * N, c.y * N, c.z * N]).expect("block");
        let origin = [
            f64::from(c.x * N),
            f64::from(c.y * N) * CELL_H,
            f64::from(c.z * N),
        ];
        (m.opaque, origin)
    }

    /// The light of the vertex at face-local point `p` (x and z in metres, y in rows) with
    /// face-local normal `n`, in mesh `m` at `origin`.
    fn light(&self, (m, origin): &(MeshArrays, [f64; 3]), p: [f64; 3], n: [f64; 3]) -> f32 {
        let w = self.f.point([p[0], p[1] * CELL_H, p[2]]);
        let wn = self.f.vector(n);
        let pos = |i: u32| -> [f64; 3] {
            std::array::from_fn(|a| f64::from(m.positions[i as usize * 3 + a]) + origin[a])
        };
        let facing = |i: u32| {
            (0..3).all(|a| (f64::from(m.normals[i as usize * 3 + a]) - wn[a]).abs() < 1e-4)
        };
        // The triangle the point lies on (merged quads span many cells): its light interpolated
        // as the GPU does, barycentrically.
        for t in m.indices.chunks_exact(3) {
            if !facing(t[0]) {
                continue;
            }
            let [a, b, c] = [pos(t[0]), pos(t[1]), pos(t[2])];
            let sub =
                |p: [f64; 3], q: [f64; 3]| -> [f64; 3] { std::array::from_fn(|k| p[k] - q[k]) };
            let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
            let (e1, e2, d) = (sub(b, a), sub(c, a), sub(w, a));
            if dot(d, wn).abs() > 1e-4 {
                continue; // not on the triangle's plane
            }
            let (d11, d12, d22) = (dot(e1, e1), dot(e1, e2), dot(e2, e2));
            let (d1, d2) = (dot(d, e1), dot(d, e2));
            let den = d11 * d22 - d12 * d12;
            let v = (d22 * d1 - d12 * d2) / den;
            let u = (d11 * d2 - d12 * d1) / den;
            if v < -1e-6 || u < -1e-6 || u + v > 1.0 + 1e-6 {
                continue;
            }
            let l = |i: u32| f64::from(m.light[i as usize]);
            return ((1.0 - u - v) * l(t[0]) + v * l(t[1]) + u * l(t[2])) as f32;
        }
        panic!("no surface at {p:?} facing {n:?}")
    }
}

const UP: [f64; 3] = [0.0, 1.0, 0.0];
/// The fixtures' ground: its top is row GROUND.
const GROUND: i32 = BASE + 8;

fn ground(fx: &mut Fixture, lo: [i32; 2], hi: [i32; 2]) {
    fx.fill([lo[0], BASE, lo[1]], [hi[0], GROUND, hi[1]], cube());
}

dwell_world::both_faces!(
    /// Open ground sees the whole sky; under an overhang it sees less, and a cave floor none.
    fn open_ground_sees_the_sky_an_overhang_less_a_cave_floor_none(f: FaceFrame) {
        let g = f64::from(GROUND);
        // Open ground.
        let mut fx = Fixture::new(f);
        ground(&mut fx, [-8, -8], [N + 8, N + 8]);
        let m = fx.mesh(0, 0, 0);
        let open = fx.light(&m, [16.0, g, 16.0], UP);
        assert_eq!(open, 1.0, "open ground");
        // An overhang: a slab 1.5 m over the ground, from x = 16 on.
        fx.fill([16, GROUND + 3, 0], [N + 8, GROUND + 4, N], cube());
        let m = fx.mesh(0, 0, 0);
        let edge = fx.light(&m, [16.0, g, 16.0], UP);
        let under = fx.light(&m, [20.0, g, 16.0], UP);
        let beside = fx.light(&m, [14.0, g, 16.0], UP);
        assert!(under < 0.15, "deep under the overhang: {under}");
        assert!(edge > under && edge < 0.8, "at its edge: {edge}");
        assert!(beside > edge && beside < 1.0, "beside it: {beside}");
        // A cave: a room 3 m tall closed on every side, 2 m of rock over it.
        let mut fx = Fixture::new(f);
        ground(&mut fx, [-8, -8], [N + 8, N + 8]);
        fx.fill([4, GROUND, 4], [28, GROUND + 10, 28], cube());
        fx.fill([8, GROUND, 8], [24, GROUND + 6, 24], 0);
        let m = fx.mesh(0, 0, 0);
        let floor = fx.light(&m, [16.0, g, 16.0], UP);
        assert_eq!(floor, 0.0, "a cave floor");
        // Its walls and ceiling are dark too.
        assert!(fx.light(&m, [8.0, g + 2.0, 16.0], [1.0, 0.0, 0.0]) < 0.1);
        assert_eq!(fx.light(&m, [16.0, g + 6.0, 16.0], [0.0, -1.0, 0.0]), 0.0);
    }
);

dwell_world::both_faces!(
    /// A vertex on a chunk border gets the same light from the chunks on either side (across x
    /// and across a row of chunks): its rays, their turn and the cells they read are the same.
    fn a_border_vertex_has_the_same_light_from_either_chunk(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        // Rough ground with pillars and slabs over it, across the border at x = 32.
        let h = |x: i32, z: i32| {
            let k = (x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663)) as u32;
            (k >> 7) % 5
        };
        for z in -6..N + 6 {
            for x in 8..56 {
                let top = GROUND + h(x, z) as i32;
                fx.fill([x, BASE, z], [x + 1, top, z + 1], cube());
                if h(x + 3, z + 1) == 0 {
                    fx.fill([x, top + 4, z], [x + 1, top + 6, z + 1], cube());
                }
            }
        }
        // And a wall standing across the chunk row's top border (row BASE + 32).
        fx.fill([20, GROUND, 10], [21, BASE + N + 6, 20], cube());
        let (a, b) = (fx.mesh(0, 0, 0), fx.mesh(1, 0, 0));
        let above = fx.mesh(0, 1, 0);
        // Every vertex of the shared x plane (32 in a, 0 in b) found in both meshes.
        let face_local = |m: &(MeshArrays, [f64; 3]), i: usize| {
            let p: [f64; 3] = std::array::from_fn(|k| f64::from(m.0.positions[i * 3 + k]) + m.1[k]);
            let q = f.point(p);
            let n = f.vector(std::array::from_fn(|k| f64::from(m.0.normals[i * 3 + k])));
            ([q[0], q[1] / CELL_H, q[2]], n)
        };
        let mut compared = 0;
        let mut partial = 0;
        for (one, other, on_border) in [
            (&a, &b, (|p: [f64; 3]| p[0] == 32.0) as fn([f64; 3]) -> bool),
            (&a, &above, |p| p[1] == f64::from(BASE + N)),
        ] {
            for i in 0..one.0.positions.len() / 3 {
                let (p, n) = face_local(one, i);
                if !on_border(p) {
                    continue;
                }
                let theirs = (0..other.0.positions.len() / 3)
                    .map(|j| face_local(other, j))
                    .position(|(q, m)| q == p && m == n);
                if let Some(j) = theirs {
                    assert_eq!(one.0.light[i], other.0.light[j], "{p:?} facing {n:?}");
                    compared += 1;
                    partial += usize::from(one.0.light[i] > 0.0 && one.0.light[i] < 1.0);
                }
            }
        }
        assert!(
            compared > 40 && partial > 10,
            "{compared} shared vertices, {partial} partly lit"
        );
    }
);

dwell_world::both_faces!(
    /// A roof built in the next chunk darkens the ground under it in this one once re-meshed (the
    /// streamer re-meshes the chunks whose apron an edit is in: chunk_stream.rs).
    fn an_edit_in_a_neighbour_changes_the_light_here(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        ground(&mut fx, [-8, -8], [N + 8, N + 8]);
        let at = [30.0, f64::from(GROUND), 16.0];
        let m = fx.mesh(0, 0, 0);
        let before = fx.light(&m, at, UP);
        assert_eq!(before, 1.0);
        // In chunk x = 1, 1.5 m over the ground and leaning over the border.
        fx.fill([N, GROUND + 3, 8], [N + 4, GROUND + 4, 24], cube());
        let m = fx.mesh(0, 0, 0);
        let after = fx.light(&m, at, UP);
        assert!(after < before, "{after}");
    }
);

#[test]
fn a_mesh_from_the_plain_apron_leaves_rays_past_it_open() {
    // face-a-only: the apron's cutoff is the same on either face (the parity test meshes both).
    // Ground under a slab in the apron's reach but past the one-voxel block: lit with it, darker
    // with the sky apron.
    let mut fx = Fixture::new(FaceFrame::A);
    ground(&mut fx, [-8, -8], [N + 8, N + 8]);
    fx.fill([-8, GROUND + 3, -8], [-2, GROUND + 4, N + 8], cube());
    let c = ChunkCoord::new(0, CHUNK_ROW, 0);
    let plain = fx.world.padded_block(c, 1);
    let m = mesh_chunk_at(&plain, false, [0, c.y * N, 0]).expect("padded");
    let origin = [0.0, f64::from(c.y * N) * CELL_H, 0.0];
    let at = [0.0, f64::from(GROUND), 16.0];
    let lit = fx.light(&(m.opaque, origin), at, UP);
    let m = fx.mesh(0, 0, 0);
    let deep = fx.light(&m, at, UP);
    assert_eq!(lit, AMBIENT.ao_factor(AO_OPEN) as f32);
    assert!(deep < lit, "{deep}");
}
