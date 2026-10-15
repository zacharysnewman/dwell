//! The physics world (ARCHITECTURE.md §7): Rapier in f64 with enhanced determinism, wrapped so the
//! rest of Dwell never handles Rapier types beyond body handles. Terrain is one static voxel
//! collider (terrain.rs); Tier 1 bodies and player characters are dynamic or kinematic.

use rapier3d_f64::prelude::*;

pub use rapier3d_f64::glamx::Vec3;
pub use rapier3d_f64::math::Vector as DVec3;

use dwell_world::MIDPLANE_Y;

/// A body in the physics world (opaque: game code never sees Rapier's types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyHandle(pub(crate) RigidBodyHandle);

impl BodyHandle {
    /// Rapier's handle, with `Physics::rapier`: for tests and tools that inspect the engine's
    /// bodies, never for game code.
    pub fn rapier(self) -> RigidBodyHandle {
        self.0
    }
}

/// What a body is, for collision filtering and probes (the former Jolt object layers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Static voxel terrain: never collides with terrain.
    Terrain,
    /// Voxel clusters (Phase 20) and test platforms.
    Tier1,
    /// Player capsules.
    Character,
}

const TERRAIN: Group = Group::GROUP_1;
const TIER1: Group = Group::GROUP_2;
const CHARACTER: Group = Group::GROUP_3;

pub(crate) fn groups(layer: Layer) -> InteractionGroups {
    let (member, filter) = match layer {
        Layer::Terrain => (TERRAIN, TIER1 | CHARACTER),
        Layer::Tier1 => (TIER1, TERRAIN | TIER1 | CHARACTER),
        Layer::Character => (CHARACTER, TERRAIN | TIER1 | CHARACTER),
    };
    InteractionGroups::new(member, filter, InteractionTestMode::And)
}

/// The groups a probe of moving bodies matches (Tier 1 and characters).
pub(crate) fn moving_groups() -> InteractionGroups {
    InteractionGroups::new(Group::ALL, TIER1 | CHARACTER, InteractionTestMode::And)
}

pub(crate) fn layer_of(collider: &Collider) -> Layer {
    let m = collider.collision_groups().memberships;
    if m.contains(CHARACTER) {
        Layer::Character
    } else if m.contains(TIER1) {
        Layer::Tier1
    } else {
        Layer::Terrain
    }
}

/// Tunables of the world (the former `PhysicsConfig`).
#[derive(Clone, Copy, Debug)]
pub struct PhysicsConfig {
    /// World gravity for Tier 1 bodies (players use their own; §9).
    pub gravity_y: f64,
    /// The flip band (BIFACIAL_WORLD.md §3): within this distance of the midplane a body moving
    /// toward it is accelerated (`band_boost_gravities`); moving away, gravity fades to zero at it.
    pub flip_band: f64,
    pub band_boost_gravities: f64,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            gravity_y: -9.81,
            flip_band: 4.0,
            band_boost_gravities: 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Static,
    /// Moved by its velocity.
    Kinematic,
    Dynamic,
}

pub use rapier3d_f64::glamx::DQuat;

/// A box body to add (`Physics::add_box`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxBody {
    pub motion: Motion,
    pub center: DVec3,
    /// Half extents.
    pub half: DVec3,
    pub rotation: DQuat,
    pub velocity: DVec3,
    pub angular_velocity: DVec3,
    /// Total mass (kg); `None`: the density of water.
    pub mass: Option<f64>,
    /// World gravity applies (dynamic bodies).
    pub gravity: bool,
    pub lock_rotation: bool,
}

impl BoxBody {
    pub fn new(motion: Motion, center: DVec3, half: DVec3) -> Self {
        Self {
            motion,
            center,
            half,
            rotation: DQuat::IDENTITY,
            velocity: DVec3::ZERO,
            angular_velocity: DVec3::ZERO,
            mass: None,
            gravity: true,
            lock_rotation: false,
        }
    }
}

pub struct Physics {
    pub(crate) w: PhysicsWorld,
    config: PhysicsConfig,
}

impl Physics {
    pub fn new(config: PhysicsConfig) -> Self {
        let mut w = PhysicsWorld::new();
        w.gravity = DVec3::new(0.0, config.gravity_y, 0.0);
        w.integration_parameters.dt = 1.0 / f64::from(dwell_protocol::SIM_HZ);
        Self { w, config }
    }

    /// Steps the world. First sets each dynamic Tier 1 body's gravity by its side of the midplane
    /// (a body on face B falls toward the midplane), faded across the flip band.
    pub fn step(&mut self, dt: f64) {
        let tier1: Vec<(RigidBodyHandle, f64, f64)> = self
            .w
            .bodies
            .iter()
            .filter(|(_, b)| b.is_dynamic() && b.user_data == TIER1_BODY)
            .map(|(h, b)| (h, b.translation().y, b.linvel().y))
            .collect();
        for (h, y, vy) in tier1 {
            let over = y - f64::from(MIDPLANE_Y);
            let side = if over >= 0.0 { 1.0 } else { -1.0 };
            let depth = over.abs();
            let toward = side * vy < 0.0;
            let fade = if depth < self.config.flip_band {
                if toward {
                    self.config.band_boost_gravities
                } else {
                    depth / self.config.flip_band
                }
            } else {
                1.0
            };
            if let Some(b) = self.w.bodies.get_mut(h) {
                b.set_gravity_scale(side * fade, false);
            }
        }
        self.w.integration_parameters.dt = dt;
        self.w.step();
    }

    /// A box body on the Tier 1 layer: platforms, ramps and walls for tests, clusters later.
    pub fn add_box(&mut self, b: BoxBody) -> BodyHandle {
        let builder = match b.motion {
            Motion::Static => RigidBodyBuilder::fixed(),
            Motion::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
            Motion::Dynamic => RigidBodyBuilder::dynamic(),
        };
        let mut builder = builder
            .translation(b.center)
            .rotation(b.rotation.to_scaled_axis())
            .linvel(b.velocity)
            .angvel(b.angular_velocity)
            .linear_damping(0.0)
            .can_sleep(false)
            .gravity_scale(if b.gravity { 1.0 } else { 0.0 })
            .user_data(TIER1_BODY);
        if b.lock_rotation {
            builder = builder.lock_rotations();
        }
        let h = self.w.bodies.insert(builder.build());
        let mut collider = ColliderBuilder::cuboid(b.half.x, b.half.y, b.half.z)
            .collision_groups(groups(Layer::Tier1));
        collider = match b.mass {
            Some(m) => collider.mass(m),
            None => collider.density(1000.0),
        };
        self.w
            .colliders
            .insert_with_parent(collider.build(), h, &mut self.w.bodies);
        BodyHandle(h)
    }

    /// The Rapier world, read-only (tests and tools inspect bodies and colliders).
    pub fn rapier(&self) -> &PhysicsWorld {
        &self.w
    }

    pub fn remove_body(&mut self, h: BodyHandle) {
        self.w.remove_body_with_colliders(h.0, true);
    }

    pub fn position(&self, h: BodyHandle) -> DVec3 {
        self.w
            .bodies
            .get(h.0)
            .map_or(DVec3::ZERO, |b| b.translation())
    }
    pub fn velocity(&self, h: BodyHandle) -> DVec3 {
        self.w.bodies.get(h.0).map_or(DVec3::ZERO, |b| b.linvel())
    }
    pub fn angular_velocity(&self, h: BodyHandle) -> DVec3 {
        self.w.bodies.get(h.0).map_or(DVec3::ZERO, |b| b.angvel())
    }
    /// The velocity of the body's material at a world point (v + ω × r).
    pub fn point_velocity(&self, h: BodyHandle, point: DVec3) -> DVec3 {
        self.w
            .bodies
            .get(h.0)
            .map_or(DVec3::ZERO, |b| b.velocity_at_point(point))
    }
    pub fn set_position(&mut self, h: BodyHandle, p: DVec3) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.set_translation(p, true);
        }
    }
    /// Raises the engine's speed limit (Rapier's default is 400 m/s) to at least `v` m/s.
    pub fn raise_max_linear_velocity(&mut self, v: f64) {
        let p = &mut self.w.integration_parameters;
        p.normalized_max_linear_velocity = p.normalized_max_linear_velocity.max(v / p.length_unit);
    }
    pub fn set_velocity(&mut self, h: BodyHandle, v: DVec3) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.set_linvel(v, true);
        }
    }
    pub fn set_angular_velocity(&mut self, h: BodyHandle, w: DVec3) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.set_angvel(w, true);
        }
    }
    /// Moves a kinematic body to `target` over the next step (by its velocity).
    pub fn move_kinematic(&mut self, h: BodyHandle, target: DVec3, dt: f64) {
        let p = self.position(h);
        self.set_velocity(h, (target - p) / dt);
    }

    /// Push-out normals (toward `body`) of its contacts in the last step, at most `max`: the
    /// manifolds the solver acts on with a point within the prediction distance. By the points, not
    /// `solver_contacts`: when a pair has several manifolds (a player on a terrain collider and
    /// against its wall) Rapier solves merged clusters and leaves the manifolds' solver contacts
    /// empty, which hid every contact of the pair.
    pub fn contact_normals(&self, body: BodyHandle, max: usize, out: &mut Vec<DVec3>) {
        out.clear();
        let Some(b) = self.w.bodies.get(body.0) else {
            return;
        };
        let reach = self.w.integration_parameters.prediction_distance();
        for &collider in b.colliders() {
            for pair in self.w.narrow_phase.contact_pairs_with(collider) {
                for m in pair.manifolds() {
                    let touching = m
                        .data
                        .solver_flags
                        .contains(SolverFlags::COMPUTE_RIGID_IMPULSES)
                        && m.points.iter().any(|p| p.dist < reach);
                    if !touching || out.len() >= max {
                        continue;
                    }
                    // The normal points from collider 1 out toward collider 2.
                    out.push(if pair.collider2 == collider {
                        m.data.normal
                    } else {
                        -m.data.normal
                    });
                }
            }
        }
    }
}

impl Default for Physics {
    fn default() -> Self {
        Self::new(PhysicsConfig::default())
    }
}

/// `user_data` of Tier 1 bodies; a player body's is its handle + 1 (below this).
pub(crate) const TIER1_BODY: u128 = u128::MAX;
