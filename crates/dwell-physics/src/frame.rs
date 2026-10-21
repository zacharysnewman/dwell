//! The controller's face-local frame (BIFACIAL_WORLD.md §6, PLAYER_CONTROLLER.md §11). Down is
//! always toward the midplane, so a face-B player stands upside down. The controller is written for
//! an upright player; for face B it runs in the *mirror image* of the world — heights mirrored in
//! the midplane, vertical velocities and normals negated, shaped blocks turned over. On face A the
//! maps are the identity, so face A plays exactly as before. Both maps are involutions.
//!
//! It is the physics view of `dwell_world::FaceFrame` (BIFACIAL_WORLD.md §11): every map here goes
//! through it.

use dwell_blocks::StateId;
use dwell_world::FaceFrame;

use crate::physics::{BodyHandle, DVec3, Vec3};
use crate::query::{Capsule, ProbeHit, VoxelQuery};

/// A player's face frame in the physics' vector types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub face: FaceFrame,
}

impl Frame {
    /// The world's face frame for this face.
    pub fn face_frame(self) -> FaceFrame {
        self.face
    }
    pub fn b(self) -> bool {
        self.face.is_b()
    }
    pub fn y(self, y: f64) -> f64 {
        self.face_frame().y(y)
    }
    pub fn p(self, p: DVec3) -> DVec3 {
        DVec3::new(p.x, self.y(p.y), p.z)
    }
    pub fn v(self, v: Vec3) -> Vec3 {
        let [x, y, z] = self.face_frame().vector_f32([v.x, v.y, v.z]);
        Vec3::new(x, y, z)
    }
    pub fn row(self, y: i32) -> i32 {
        self.face_frame().row(y)
    }
    pub fn m(self, m: StateId) -> StateId {
        self.face_frame().state(m)
    }
    /// The unit vector the player's head points along, in the world.
    pub fn world_up(self) -> Vec3 {
        Vec3::new(0.0, self.face.sign_f32(), 0.0)
    }
}

/// The probes of `VoxelQuery` in a player's face-local frame: arguments and results are local; the
/// grid and the moving bodies are read in the world.
pub struct FaceQuery<'q, 'a> {
    pub q: &'q VoxelQuery<'a>,
    pub f: Frame,
}

impl FaceQuery<'_, '_> {
    pub fn cast_ray(
        &self,
        origin: DVec3,
        dir: Vec3,
        max: f32,
        me: Option<BodyHandle>,
        bodies: bool,
    ) -> Option<ProbeHit> {
        let f = self.f;
        let mut hit = self.q.cast_ray(f.p(origin), f.v(dir), max, me, bodies)?;
        hit.point = f.p(hit.point);
        hit.normal = f.v(hit.normal);
        Some(hit)
    }
    pub fn bodies_near(&self, lo: DVec3, hi: DVec3, me: Option<BodyHandle>) -> bool {
        let (a, c) = (self.f.p(lo), self.f.p(hi));
        self.q.bodies_near(a.min(c), a.max(c), me)
    }
    fn world(&self, c: &Capsule) -> Capsule {
        Capsule {
            center: self.f.p(c.center),
            ..*c
        }
    }
    pub fn overlaps_solid(&self, c: &Capsule, me: Option<BodyHandle>) -> bool {
        self.q.overlaps_solid(&self.world(c), me)
    }
    /// `f(x, y, z, state)` with the cell's row and state in the local frame.
    pub fn for_each_overlapping_cell(
        &self,
        c: &Capsule,
        mut f: impl FnMut(i32, i32, i32, StateId),
    ) {
        let fr = self.f;
        self.q
            .for_each_overlapping_cell(&self.world(c), |x, y, z, m| f(x, fr.row(y), z, fr.m(m)));
    }
    pub fn submerged_fraction(&self, center: DVec3, half_height: f32) -> f32 {
        self.q.submerged_fraction(self.f.p(center), half_height)
    }
    /// The cell's shape (derived for an adaptive top block) in the local frame: turned over on
    /// face B.
    pub fn shape(&self, x: i32, y: i32, z: i32) -> &'static dwell_blocks::ShapeInfo {
        self.f.face_frame().shape(self.q.shape(x, self.f.row(y), z))
    }
    pub fn material(&self, x: i32, y: i32, z: i32) -> StateId {
        self.f.m(self.q.material(x, self.f.row(y), z))
    }
}
