//! The controller's face-local frame (BIFACIAL_WORLD.md §6, PLAYER_CONTROLLER.md §11). Down is
//! always toward the midplane, so a face-B player stands upside down. The controller is written for
//! an upright player; for face B it runs in the *mirror image* of the world — heights y → −4,096 − y
//! (voxel rows y → −4,097 − y), vertical velocities and normals negated, shaped blocks turned over.
//! On face A the maps are the identity, so face A plays exactly as before. Both maps are
//! involutions.

use dwell_blocks::{StateId, mirror};
use dwell_world::{MIRROR_M, mirror_y};

use crate::physics::{BodyHandle, DVec3, Vec3};
use crate::query::{Capsule, ProbeHit, VoxelQuery};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// +1 face A, −1 face B.
    pub face: i8,
}

impl Frame {
    /// The continuous mirror: heights (m) sum to this (voxel rows to `MIRROR_SUM`, one less than
    /// the mirror in rows).
    pub const MIRROR: f64 = MIRROR_M;

    pub fn b(self) -> bool {
        self.face < 0
    }
    pub fn y(self, y: f64) -> f64 {
        if self.b() { Self::MIRROR - y } else { y }
    }
    pub fn p(self, p: DVec3) -> DVec3 {
        if self.b() {
            DVec3::new(p.x, Self::MIRROR - p.y, p.z)
        } else {
            p
        }
    }
    pub fn v(self, v: Vec3) -> Vec3 {
        if self.b() {
            Vec3::new(v.x, -v.y, v.z)
        } else {
            v
        }
    }
    pub fn row(self, y: i32) -> i32 {
        if self.b() { mirror_y(y) } else { y }
    }
    pub fn m(self, m: StateId) -> StateId {
        if self.b() { mirror(m) } else { m }
    }
    /// The unit vector the player's head points along, in the world.
    pub fn world_up(self) -> Vec3 {
        Vec3::new(0.0, f32::from(self.face), 0.0)
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
        let s = self.q.shape(x, self.f.row(y), z);
        if self.f.b() {
            dwell_blocks::shape::turned_over(s)
        } else {
            s
        }
    }
    pub fn material(&self, x: i32, y: i32, z: i32) -> StateId {
        self.f.m(self.q.material(x, self.f.row(y), z))
    }
}
