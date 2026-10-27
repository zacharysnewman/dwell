//! Probes for the player controller (PLAYER_CONTROLLER.md §5): static terrain is answered from the
//! voxel grid (DDA, cell overlap), moving things (Tier 1 bodies, other players) from Rapier.

use std::cell::RefCell;

use dwell_blocks::shape::{ray_enter_shape, vertical_segment_distance_sq};
use dwell_blocks::{ShapeInfo, StateId, info};
use dwell_world::{
    CELL, CELL_H, CHUNK_SIZE, VoxelWorld, cell_dir, cell_space, chunk_of, m_of_row, normal_to_m,
    pos_of_cell, row_of_m,
};
use rapier3d_f64::parry::query::Ray;
use rapier3d_f64::parry::shape::Capsule as ParryCapsule;
use rapier3d_f64::prelude::*;

use crate::physics::{BodyHandle, DVec3, Layer, Physics, Vec3, layer_of, moving_groups};

/// What a probe hit: terrain, a Tier 1 body or a player (by body handle).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroundRef {
    #[default]
    None,
    Terrain,
    Tier1Body(BodyHandle),
    Player(BodyHandle),
}

impl GroundRef {
    pub fn body(self) -> Option<BodyHandle> {
        match self {
            Self::Tier1Body(h) | Self::Player(h) => Some(h),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeHit {
    pub distance: f32,
    pub point: DVec3,
    pub normal: Vec3,
    pub ground: GroundRef,
}

impl Default for ProbeHit {
    fn default() -> Self {
        Self {
            distance: 0.0,
            point: DVec3::ZERO,
            normal: Vec3::Y,
            ground: GroundRef::None,
        }
    }
}

/// A vertical capsule: centre, radius, half the cylinder's height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    pub center: DVec3,
    pub radius: f32,
    pub half_cylinder: f32,
}

/// Probes over one tick's world and physics.
pub struct VoxelQuery<'a> {
    world: RefCell<&'a mut VoxelWorld>,
    physics: &'a Physics,
    /// Shapes derived during this query (the world does not change while it lives): a ring of
    /// probes reads the same few cells, and an adaptive block's shape reads 18 voxels.
    shapes: RefCell<dwell_world::hash::HashMap<[i32; 3], &'static ShapeInfo>>,
}

const STEP: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

impl<'a> VoxelQuery<'a> {
    pub fn new(world: &'a mut VoxelWorld, physics: &'a Physics) -> Self {
        Self {
            world: RefCell::new(world),
            physics,
            shapes: RefCell::new(dwell_world::hash::HashMap::default()),
        }
    }

    /// The state of a voxel (generating its chunk if needed).
    pub fn material(&self, x: i32, y: i32, z: i32) -> StateId {
        let c = chunk_of(x, y, z);
        let mut w = self.world.borrow_mut();
        w.read(c).get(
            x - c.x * CHUNK_SIZE,
            y - c.y * CHUNK_SIZE,
            z - c.z * CHUNK_SIZE,
        )
    }

    /// The shape of a voxel as it collides: its own, or none for a passable block (leaves:
    /// players pass through them, docs/FELLING.md).
    pub fn shape(&self, x: i32, y: i32, z: i32) -> &'static ShapeInfo {
        if let Some(&s) = self.shapes.borrow().get(&[x, y, z]) {
            return s;
        }
        let s = if info(self.material(x, y, z)).passable {
            dwell_blocks::shape::shape_of(dwell_world::M.air)
        } else {
            self.world.borrow_mut().shape_at(x, y, z)
        };
        self.shapes.borrow_mut().insert([x, y, z], s);
        s
    }

    fn pipeline<'q>(&'q self, filter: QueryFilter<'q>) -> QueryPipeline<'q> {
        let w = &self.physics.w;
        w.broad_phase.as_query_pipeline(
            w.narrow_phase.query_dispatcher(),
            &w.bodies,
            &w.colliders,
            filter,
        )
    }

    fn moving_filter(&self, me: Option<BodyHandle>) -> QueryFilter<'static> {
        let mut f = QueryFilter::default().groups(moving_groups());
        f.exclude_rigid_body = me.map(|h| h.0);
        f
    }

    /// Is any moving body other than `me` within the box? (Lets a batch of probes skip Rapier.)
    pub fn bodies_near(&self, lo: DVec3, hi: DVec3, me: Option<BodyHandle>) -> bool {
        let q = self.pipeline(self.moving_filter(me));
        q.intersect_aabb_conservative(Aabb::new(lo, hi))
            .next()
            .is_some()
    }

    /// Nearest solid along the ray (unit `dir`), ignoring `me`. A ray starting inside solid
    /// terrain only hits once it has left it. `bodies` false skips moving bodies.
    pub fn cast_ray(
        &self,
        origin: DVec3,
        dir: Vec3,
        max: f32,
        me: Option<BodyHandle>,
        bodies: bool,
    ) -> Option<ProbeHit> {
        let a = self.cast_voxels(origin, dir, max);
        let b = if bodies {
            self.cast_bodies(origin, dir, a.map_or(max, |h| h.distance), me)
        } else {
            None
        };
        match (a, b) {
            (_, Some(b)) if a.is_none_or(|a| b.distance < a.distance) => Some(b),
            _ => a,
        }
    }

    /// Amanatides–Woo walk over cells, entering each cell's shape; in f32 relative to the
    /// origin's cell, so equally precise anywhere in the world (ADR 0011). The walk is in cell
    /// space (y in rows, HALF_HEIGHT_GRID.md §2), the direction scaled but not renormalised, so
    /// distances stay metres.
    pub fn cast_voxels(&self, origin: DVec3, dir: Vec3, max: f32) -> Option<ProbeHit> {
        let oc = cell_space(origin.to_array());
        let base = oc.map(|v| v.floor() as i32);
        let o = [
            (oc[0] - f64::from(base[0])) as f32,
            (oc[1] - f64::from(base[1])) as f32,
            (oc[2] - f64::from(base[2])) as f32,
        ];
        let d = cell_dir([f64::from(dir.x), f64::from(dir.y), f64::from(dir.z)]).map(|v| v as f32);
        let mut cell = [0i32; 3];
        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];
        for i in 0..3 {
            cell[i] = o[i].floor() as i32;
            if d[i] > 0.0 {
                step[i] = 1;
                t_delta[i] = 1.0 / d[i];
                t_max[i] = (cell[i] as f32 + 1.0 - o[i]) / d[i];
            } else if d[i] < 0.0 {
                step[i] = -1;
                t_delta[i] = -1.0 / d[i];
                t_max[i] = (cell[i] as f32 - o[i]) / d[i];
            }
        }
        let mut t_cell = 0.0f32;
        let mut first = true; // a ray starting on a solid's surface does not enter it
        while t_cell <= max {
            let t_next = t_max[0].min(t_max[1]).min(t_max[2]);
            let (cx, cy, cz) = (base[0] + cell[0], base[1] + cell[1], base[2] + cell[2]);
            let shape = self.shape(cx, cy, cz);
            if shape.face_count > 0 {
                // Faces against a solid neighbour are internal: a ray crossing them is inside.
                let mut nb =
                    |f: usize| self.shape(cx + STEP[f][0], cy + STEP[f][1], cz + STEP[f][2]);
                let corner = cell.map(|c| c as f32);
                if let Some(e) = ray_enter_shape(
                    shape,
                    corner,
                    o,
                    d,
                    t_next,
                    Some(&mut nb),
                    if first { 1e-6 } else { 0.0 },
                ) {
                    if e.t > max {
                        return None;
                    }
                    let n = normal_to_m(e.normal.map(f64::from)).map(|v| v as f32);
                    return Some(ProbeHit {
                        distance: e.t,
                        point: origin + dir.as_dvec3() * f64::from(e.t),
                        normal: Vec3::from(n),
                        ground: GroundRef::Terrain,
                    });
                }
            }
            first = false;
            let mut axis = 0;
            if t_max[1] < t_max[axis] {
                axis = 1;
            }
            if t_max[2] < t_max[axis] {
                axis = 2;
            }
            if t_max[axis] == f32::INFINITY {
                break;
            }
            t_cell = t_max[axis];
            t_max[axis] += t_delta[axis];
            cell[axis] += step[axis];
        }
        None
    }

    pub fn cast_bodies(
        &self,
        origin: DVec3,
        dir: Vec3,
        max: f32,
        me: Option<BodyHandle>,
    ) -> Option<ProbeHit> {
        let q = self.pipeline(self.moving_filter(me));
        let ray = Ray::new(origin, dir.as_dvec3());
        let (h, hit) = q.cast_ray_and_get_normal(&ray, f64::from(max), true)?;
        let collider = self.physics.w.colliders.get(h)?;
        let body = collider.parent()?;
        let ground = match layer_of(collider) {
            Layer::Character => GroundRef::Player(BodyHandle(body)),
            _ => GroundRef::Tier1Body(BodyHandle(body)),
        };
        Some(ProbeHit {
            distance: hit.time_of_impact as f32,
            point: ray.point_at(hit.time_of_impact),
            normal: hit.normal.as_vec3(),
            ground,
        })
    }

    /// Distance between a vertical capsule's axis segment and an axis-aligned box.
    pub fn segment_box_distance(c: &Capsule, lo: DVec3, hi: DVec3) -> f32 {
        let lo = (lo - c.center).as_vec3();
        let hi = (hi - c.center).as_vec3();
        let dx = lo.x.max(0.0).max(-hi.x);
        let dz = lo.z.max(0.0).max(-hi.z);
        let dy = (lo.y - c.half_cylinder)
            .max(0.0)
            .max(-c.half_cylinder - hi.y);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Calls `f(x, y, z, state)` for every non-air cell the capsule overlaps (cell as a box).
    pub fn for_each_overlapping_cell(
        &self,
        c: &Capsule,
        mut f: impl FnMut(i32, i32, i32, StateId),
    ) {
        let r = f64::from(c.radius);
        let hh = f64::from(c.half_cylinder + c.radius);
        let (x0, x1) = (
            (c.center.x - r).floor() as i32,
            (c.center.x + r).floor() as i32,
        );
        let (y0, y1) = (row_of_m(c.center.y - hh), row_of_m(c.center.y + hh));
        let (z0, z1) = (
            (c.center.z - r).floor() as i32,
            (c.center.z + r).floor() as i32,
        );
        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let m = self.material(x, y, z);
                    if m == 0 {
                        continue; // air is state 0
                    }
                    let lo = DVec3::from_array(pos_of_cell([x, y, z]));
                    if Self::segment_box_distance(c, lo, lo + DVec3::from_array(CELL)) < c.radius {
                        f(x, y, z, m);
                    }
                }
            }
        }
    }

    pub fn overlaps_voxels(&self, c: &Capsule) -> bool {
        let mut overlaps = false;
        self.for_each_overlapping_cell(c, |x, y, z, _| {
            if overlaps {
                return;
            }
            let shape = self.shape(x, y, z);
            if shape.face_count == 0 {
                return;
            }
            let rel = (c.center - DVec3::from_array(pos_of_cell([x, y, z]))).as_vec3();
            if vertical_segment_distance_sq(
                shape,
                CELL_H as f32,
                rel.x,
                rel.z,
                rel.y - c.half_cylinder,
                rel.y + c.half_cylinder,
            ) < c.radius * c.radius
            {
                overlaps = true;
            }
        });
        overlaps
    }

    /// Does the capsule overlap solid terrain or a moving body other than `me`?
    pub fn overlaps_solid(&self, c: &Capsule, me: Option<BodyHandle>) -> bool {
        if self.overlaps_voxels(c) {
            return true;
        }
        let shape = ParryCapsule::new_y(f64::from(c.half_cylinder), f64::from(c.radius));
        let q = self.pipeline(self.moving_filter(me));
        let mut hits = q.intersect_shape(Pose::from_translation(c.center), &shape);
        hits.next().is_some()
    }

    /// Fraction [0, 1] of the capsule's height inside liquid cells, sampled at its centre column.
    pub fn submerged_fraction(&self, center: DVec3, half_height: f32) -> f32 {
        let feet = center.y as f32 - half_height;
        let head = center.y as f32 + half_height;
        let (x, z) = (center.x.floor() as i32, center.z.floor() as i32);
        let mut wet = 0.0f32;
        for y in row_of_m(f64::from(feet))..=row_of_m(f64::from(head)) {
            let cell = info(self.material(x, y, z));
            if !cell.liquid && !self.world.borrow_mut().flooded_at(x, y, z) {
                continue;
            }
            let lo = feet.max(m_of_row(y) as f32);
            let hi = head.min(m_of_row(y + 1) as f32);
            wet += (hi - lo).max(0.0);
        }
        (wet / (head - feet)).clamp(0.0, 1.0)
    }
}
