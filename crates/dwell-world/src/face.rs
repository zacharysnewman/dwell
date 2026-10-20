//! The face frame (BIFACIAL_WORLD.md §11): one transform between the world and a face's local
//! coordinates, so code is written once, for an upright face, and face B is its mirror image.
//!
//! **The rule.** Code that depends on which way is up (terrain, meshing, collision, the controller,
//! targeting, brushes, the sky) is written in face-local coordinates — +y up, the ground below —
//! and crosses into the world only through a [`FaceFrame`]: heights through [`FaceFrame::y`],
//! voxel rows through [`FaceFrame::row`], chunk rows through [`FaceFrame::chunk_y`], points and
//! vectors through [`FaceFrame::point`] and [`FaceFrame::vector`], block states through
//! [`FaceFrame::state`] (shaped blocks turned over), and geometry through
//! [`FaceFrame::flips_winding`]. On face A every map is the identity; on face B each is the mirror
//! in the midplane and is its own inverse, so the same call goes either way.
//!
//! Tests of face-dependent code run on both faces: [`FaceFrame::under_test`] (or the
//! [`both_faces!`](crate::both_faces) macro) gives the faces a suite covers, both by default and
//! one when `DWELL_FACE` is `a` or `b`.

use dwell_blocks::{ShapeInfo, StateId};

use crate::{ChunkCoord, Face, MIDPLANE_CHUNK_Y, MIDPLANE_Y, MIRROR_CHUNK_SUM, MIRROR_SUM};

/// A face's local frame: the identity for face A, the mirror in the midplane for face B.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceFrame {
    pub face: Face,
}

impl FaceFrame {
    pub const A: Self = Self { face: Face::A };
    pub const B: Self = Self { face: Face::B };
    /// Continuous heights of face B sum to this with their face-local height (voxel rows to one
    /// less: a row spans [y, y + 1), its mirror [MIRROR − y − 1, MIRROR − y)).
    pub const MIRROR: f64 = (MIRROR_SUM + 1) as f64;

    pub const fn of(face: Face) -> Self {
        Self { face }
    }
    /// The face a continuous world height is on (the midplane itself is face A's).
    pub fn at_height(y: f64) -> Self {
        Self::of(if y >= f64::from(MIDPLANE_Y) {
            Face::A
        } else {
            Face::B
        })
    }
    /// The face a voxel row is on.
    pub const fn at_row(y: i32) -> Self {
        Self::of(if y >= MIDPLANE_Y { Face::A } else { Face::B })
    }
    /// The face a chunk row is on.
    pub const fn at_chunk_y(cy: i32) -> Self {
        Self::of(if cy >= MIDPLANE_CHUNK_Y {
            Face::A
        } else {
            Face::B
        })
    }
    /// The frame of a `DWELL_FACE`-style name: `a` or `b` (any case).
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "a" | "A" => Some(Self::A),
            "b" | "B" => Some(Self::B),
            _ => None,
        }
    }

    pub const fn is_b(self) -> bool {
        matches!(self.face, Face::B)
    }
    /// The world's y component of up: +1 on face A, −1 on face B.
    pub const fn sign(self) -> i32 {
        if self.is_b() { -1 } else { 1 }
    }
    pub const fn sign_f64(self) -> f64 {
        if self.is_b() { -1.0 } else { 1.0 }
    }
    /// Up in the world.
    pub const fn up(self) -> [f64; 3] {
        [0.0, self.sign_f64(), 0.0]
    }
    /// Whether geometry mapped through this frame has its triangles turned inside out (a mirror
    /// reverses winding): swap two vertices of each triangle after [`FaceFrame::point`].
    pub const fn flips_winding(self) -> bool {
        self.is_b()
    }

    /// A continuous height, world ↔ face-local.
    pub fn y(self, y: f64) -> f64 {
        if self.is_b() { Self::MIRROR - y } else { y }
    }
    /// A voxel row, world ↔ face-local.
    pub const fn row(self, y: i32) -> i32 {
        if self.is_b() { MIRROR_SUM - y } else { y }
    }
    /// A chunk row, world ↔ face-local.
    pub const fn chunk_y(self, cy: i32) -> i32 {
        if self.is_b() { MIRROR_CHUNK_SUM - cy } else { cy }
    }
    /// A chunk, world ↔ face-local.
    pub const fn chunk(self, c: ChunkCoord) -> ChunkCoord {
        ChunkCoord {
            x: c.x,
            y: self.chunk_y(c.y),
            z: c.z,
        }
    }
    /// A voxel, world ↔ face-local.
    pub const fn cell(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        [x, self.row(y), z]
    }
    /// A point, world ↔ face-local.
    pub fn point(self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [x, self.y(y), z]
    }
    /// A direction (velocity, normal, ray), world ↔ face-local.
    pub fn vector(self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [x, y * self.sign_f64(), z]
    }
    /// [`FaceFrame::vector`] in `f32`.
    pub fn vector_f32(self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        [x, if self.is_b() { -y } else { y }, z]
    }
    /// A block state, world ↔ face-local: a shaped block (its `half`) turned over on face B.
    pub fn state(self, s: StateId) -> StateId {
        if self.is_b() { dwell_blocks::mirror(s) } else { s }
    }
    /// A span of heights within a cell (0 its floor, 1 its top), world ↔ face-local: turned over
    /// on face B. Returns (low, high).
    pub fn in_cell(self, lo: f32, hi: f32) -> (f32, f32) {
        if self.is_b() { (1.0 - hi, 1.0 - lo) } else { (lo, hi) }
    }
    /// A shape, world ↔ face-local: turned over on face B.
    pub fn shape(self, s: &'static ShapeInfo) -> &'static ShapeInfo {
        if self.is_b() { dwell_blocks::shape::turned_over(s) } else { s }
    }
    /// A cell-face index (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z), world ↔ face-local: ±Y swap on
    /// face B.
    pub const fn cell_face(self, face: usize) -> usize {
        if self.is_b() && (face == 2 || face == 3) {
            face ^ 1
        } else {
            face
        }
    }

    /// The faces a face-dependent test suite runs on: both, unless `DWELL_FACE` names one.
    pub fn under_test() -> Vec<Self> {
        match std::env::var("DWELL_FACE") {
            Ok(v) if !v.trim().is_empty() => match Self::parse(&v) {
                Some(f) => vec![f],
                None => panic!("DWELL_FACE must be a or b, not {v:?}"),
            },
            _ => vec![Self::A, Self::B],
        }
    }
}

/// Declares a test that runs once per face: `both_faces!(fn name(f: FaceFrame) { … })` (any path to `FaceFrame`) expands to
/// a module `name` with tests `a` and `b`, each calling the body with its frame (`DWELL_FACE`
/// skips the other face's). A face-dependent test written this way cannot be face A's alone.
#[macro_export]
macro_rules! both_faces {
    ($(#[$m:meta])* fn $name:ident($f:ident : $t:ty) $body:block) => {
        $(#[$m])*
        mod $name {
            #[allow(unused_imports)]
            use super::*;
            fn body($f: $crate::FaceFrame) $body
            fn run(face: $crate::FaceFrame) {
                if $crate::FaceFrame::under_test().contains(&face) {
                    body(face);
                }
            }
            #[test]
            fn a() {
                run($crate::FaceFrame::A);
            }
            #[test]
            fn b() {
                run($crate::FaceFrame::B);
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_map_is_an_involution_and_the_identity_on_face_a() {
        for f in [FaceFrame::A, FaceFrame::B] {
            for y in [-5000, -2049, -2048, -2047, 0, 37] {
                assert_eq!(f.row(f.row(y)), y);
                assert_eq!(f.chunk_y(f.chunk_y(y / 32)), y / 32);
                let h = f64::from(y) + 0.25;
                assert_eq!(f.y(f.y(h)), h);
                assert_eq!(f.vector(f.vector([1.0, h, 2.0])), [1.0, h, 2.0]);
            }
        }
        assert_eq!(FaceFrame::A.row(7), 7);
        assert_eq!(FaceFrame::A.y(7.5), 7.5);
    }

    #[test]
    fn face_b_mirrors_rows_heights_and_chunks_consistently() {
        let b = FaceFrame::B;
        // The row just under the midplane is face B's first, the mirror of face A's first.
        assert_eq!(b.row(MIDPLANE_Y - 1), MIDPLANE_Y);
        // A row [y, y + 1) maps to the row whose span is the mirrored span.
        for y in [-4200, -2049, -100] {
            let lo = b.y(f64::from(y) + 1.0);
            assert_eq!(lo, f64::from(b.row(y)));
        }
        // A chunk's rows map into the mirrored chunk.
        let cy = -70;
        let rows = (cy * 32)..(cy * 32 + 32);
        for y in rows {
            assert_eq!(b.row(y).div_euclid(32), b.chunk_y(cy));
        }
        assert_eq!(FaceFrame::at_row(MIDPLANE_Y - 1), b);
        assert_eq!(FaceFrame::at_height(f64::from(MIDPLANE_Y) - 0.01), b);
        assert_eq!(FaceFrame::at_chunk_y(MIDPLANE_CHUNK_Y - 1), b);
        assert_eq!(b.cell_face(2), 3);
        assert_eq!(b.cell_face(0), 0);
        assert_eq!(b.up(), [0.0, -1.0, 0.0]);
    }

    crate::both_faces!(
        fn the_macro_runs_the_body_with_each_face(f: FaceFrame) {
            assert_eq!(f.row(f.row(-3000)), -3000);
        }
    );
}
