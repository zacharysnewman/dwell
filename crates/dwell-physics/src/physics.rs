//! The physics world (ARCHITECTURE.md §7): Rapier in f64 with enhanced determinism, wrapped so the
//! rest of Dwell never handles Rapier types beyond body handles. Terrain is one static voxel
//! collider (terrain.rs); Tier 1 bodies and player characters are dynamic or kinematic.

use rapier3d_f64::prelude::*;

pub use rapier3d_f64::glamx::Vec3;
pub use rapier3d_f64::math::Vector as DVec3;

use dwell_world::{FaceFrame, MIDPLANE_M};

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

pub use rapier3d_f64::glamx::{DMat3, DQuat};

/// One cell of a falling piece (`Physics::add_piece`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceCell {
    /// The box's centre relative to the piece's origin (m).
    pub center: DVec3,
    pub half: DVec3,
    /// Its mass (kg).
    pub mass: f64,
    /// What it collides with.
    pub collide: Collide,
}

/// What a falling piece's cell collides with (docs/FELLING.md §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collide {
    /// Nothing: it only adds mass (leaves riding a falling trunk).
    None,
    /// The terrain and other pieces, not players (leaves falling on their own: they land, and
    /// players pass through leaves).
    World,
    /// Everything (a trunk's logs, sand).
    All,
}

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
    /// The pieces' bodies (`add_piece`, `add_kinematic_piece`): what players push.
    pieces: dwell_world::hash::HashSet<RigidBodyHandle>,
}

impl Physics {
    pub fn new(config: PhysicsConfig) -> Self {
        let mut w = PhysicsWorld::new();
        w.gravity = DVec3::new(0.0, config.gravity_y, 0.0);
        w.integration_parameters.dt = 1.0 / f64::from(dwell_protocol::SIM_HZ);
        Self {
            w,
            config,
            pieces: Default::default(),
        }
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
            // Face-local: the body's height over the midplane, and whether it moves toward it.
            let face = FaceFrame::at_height(y);
            let depth = face.y(y) - MIDPLANE_M;
            let toward = face.vector([0.0, vy, 0.0])[1] < 0.0;
            let side = face.sign_f64();
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

    /// A falling piece (docs/FELLING.md §3): one dynamic body at `origin` (world) of boxes, one
    /// per cell (`cells`: each box's centre relative to `origin`, its half extents, its mass), on
    /// the Tier 1 layer. A cell that doesn't `collide` (a leaf) adds its mass but touches nothing.
    /// Continuous collision keeps a fast trunk from tunnelling through the ground.
    pub fn add_piece(&mut self, origin: DVec3, cells: &[PieceCell], dominant: bool) -> BodyHandle {
        let body = RigidBodyBuilder::dynamic()
            .translation(origin)
            .linear_damping(0.05)
            .angular_damping(0.2)
            .can_sleep(false)
            .ccd_enabled(true)
            // Too heavy for players to push (`dominant`): players' contacts move it no more than
            // the terrain's do, while it still pushes them.
            .dominance_group(i8::from(dominant))
            .user_data(TIER1_BODY)
            .build();
        let h = self.w.bodies.insert(body);
        self.pieces.insert(h);
        for c in cells {
            self.w
                .colliders
                .insert_with_parent(piece_collider(c), h, &mut self.w.bodies);
        }
        BodyHandle(h)
    }

    /// A piece's cells changed (docs/BUILT_BODIES.md §4: a cell broken off it, or one placed on it):
    /// its colliders are made again from `cells`, in its own frame; its pose and velocities stay,
    /// its mass, centre of mass and inertia follow the colliders.
    pub fn set_piece_cells(&mut self, h: BodyHandle, cells: &[PieceCell]) {
        let Some(body) = self.w.bodies.get(h.0) else {
            return;
        };
        let old: Vec<_> = body.colliders().to_vec();
        for c in old {
            self.w.remove_collider(c);
        }
        for c in cells {
            self.w
                .colliders
                .insert_with_parent(piece_collider(c), h.0, &mut self.w.bodies);
        }
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.wake_up(true);
        }
    }

    /// A falling piece's stand-in in a client's prediction (docs/FELLING.md §3): a kinematic body
    /// moved to the server's pose (`set_kinematic_pose`), its colliding cells' boxes, so the local
    /// player is pushed by it and can stand on it (its velocity follows its moves).
    pub fn add_kinematic_piece(
        &mut self,
        origin: DVec3,
        rotation: DQuat,
        cells: &[PieceCell],
    ) -> BodyHandle {
        let body = RigidBodyBuilder::kinematic_position_based()
            .translation(origin)
            .rotation(rotation.to_scaled_axis())
            .can_sleep(false)
            .user_data(TIER1_BODY)
            .build();
        let h = self.w.bodies.insert(body);
        self.pieces.insert(h);
        // Every cell, as the server's piece has them: the colliding ones touch the player, and
        // all of them give it the piece's mass (a player's push is predicted by it).
        for c in cells {
            self.w
                .colliders
                .insert_with_parent(piece_collider(c), h, &mut self.w.bodies);
        }
        BodyHandle(h)
    }

    /// How far off a Tier 1 body a contact with it counts as touching it ([`Physics::tier1_contacts`]).
    pub const TIER1_CONTACT_REACH: f64 = 0.1;

    /// A body's world bounds (its colliders' AABB), if it has any.
    pub fn aabb(&self, h: BodyHandle) -> Option<(DVec3, DVec3)> {
        let b = self.w.bodies.get(h.0)?;
        let mut out: Option<(DVec3, DVec3)> = None;
        for c in b
            .colliders()
            .iter()
            .filter_map(|&c| self.w.colliders.get(c))
        {
            let a = c.compute_aabb();
            out = Some(out.map_or((a.mins, a.maxs), |(lo, hi)| {
                (lo.min(a.mins), hi.max(a.maxs))
            }));
        }
        out
    }

    /// The world centres of a body's colliders (a piece's cells), into `out`.
    pub fn collider_centres(&self, h: BodyHandle, out: &mut Vec<DVec3>) {
        out.clear();
        if let Some(b) = self.w.bodies.get(h.0) {
            out.extend(
                b.colliders()
                    .iter()
                    .filter_map(|&c| self.w.colliders.get(c))
                    .map(|c| c.position().translation),
            );
        }
    }

    /// A body's mass (kg; a kinematic stand-in's too, from its cells).
    pub fn mass(&self, h: BodyHandle) -> f64 {
        self.w.bodies.get(h.0).map_or(0.0, |b| b.mass())
    }

    /// An impulse (N·s) on the body at its centre of mass.
    pub fn apply_impulse(&mut self, h: BodyHandle, impulse: DVec3) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.apply_impulse(impulse, true);
        }
    }

    /// Whether players move this piece by a push of their own (`Players`' push pass), not by
    /// their contacts: a dominant piece (the server holds a slow piece so) or a kinematic
    /// stand-in (the client's prediction of one). Other bodies (a moving platform) are not.
    pub fn pushed_by_players(&self, h: BodyHandle) -> bool {
        self.pieces.contains(&h.0)
            && self
                .w
                .bodies
                .get(h.0)
                .is_some_and(|b| b.is_kinematic() || (b.is_dynamic() && b.dominance_group() > 0))
    }

    /// The Tier 1 bodies `body` touched in the last step, each with its contacts' push-out normal
    /// toward `body` (world), at most `max` (as [`Physics::contact_normals`]), within
    /// [`Physics::TIER1_CONTACT_REACH`]: rapier's speculative contact holds a player that far off
    /// a dominant body (a held piece) without touching it.
    pub fn tier1_contacts(&self, body: BodyHandle, max: usize, out: &mut Vec<(BodyHandle, DVec3)>) {
        out.clear();
        let Some(b) = self.w.bodies.get(body.0) else {
            return;
        };
        let reach = Self::TIER1_CONTACT_REACH;
        for &collider in b.colliders() {
            for pair in self.w.narrow_phase.contact_pairs_with(collider) {
                let other = if pair.collider1 == collider {
                    pair.collider2
                } else {
                    pair.collider1
                };
                let Some(parent) = self.w.colliders.get(other).and_then(|c| c.parent()) else {
                    continue;
                };
                if self
                    .w
                    .bodies
                    .get(parent)
                    .is_none_or(|o| o.user_data != TIER1_BODY)
                {
                    continue;
                }
                for m in pair.manifolds() {
                    let touching = m.points.iter().any(|p| p.dist < reach);
                    if !touching || out.len() >= max {
                        continue;
                    }
                    out.push((
                        BodyHandle(parent),
                        if pair.collider2 == collider {
                            m.data.normal
                        } else {
                            -m.data.normal
                        },
                    ));
                }
            }
        }
    }

    /// Moves a position-based kinematic body (`add_kinematic_piece`) to this pose over the next
    /// step.
    pub fn set_kinematic_pose(&mut self, h: BodyHandle, position: DVec3, rotation: DQuat) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.set_next_kinematic_position(Pose::from_parts(position, rotation));
        }
    }

    /// Whether a body is held from lighter bodies' pushes (players'; Rapier's dominance groups):
    /// a piece too heavy to push (docs/FELLING.md §4).
    pub fn set_dominant(&mut self, h: BodyHandle, dominant: bool) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.set_dominance_group(i8::from(dominant));
        }
    }

    /// Holds a body still where it is (`held`: fixed, colliding as a wall does) or lets it move again
    /// (dynamic): a built body resting on its build platform (docs/BUILT_BODIES.md §3).
    pub fn set_held(&mut self, h: BodyHandle, held: bool) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            let want = if held {
                RigidBodyType::Fixed
            } else {
                RigidBodyType::Dynamic
            };
            if b.body_type() != want {
                if held {
                    b.set_linvel(DVec3::ZERO, false);
                    b.set_angvel(DVec3::ZERO, false);
                }
                b.set_body_type(want, true);
            }
        }
    }

    /// A body's rotation.
    pub fn rotation(&self, h: BodyHandle) -> DQuat {
        self.w
            .bodies
            .get(h.0)
            .map_or(DQuat::IDENTITY, |b| *b.rotation())
    }

    /// The Rapier world, read-only (tests and tools inspect bodies and colliders).
    pub fn rapier(&self) -> &PhysicsWorld {
        &self.w
    }

    pub fn remove_body(&mut self, h: BodyHandle) {
        self.pieces.remove(&h.0);
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
    /// Whether the body moves under forces (not fixed, not kinematic).
    pub fn is_dynamic(&self, h: BodyHandle) -> bool {
        self.w.bodies.get(h.0).is_some_and(|b| b.is_dynamic())
    }
    /// Gravity's strength for Tier 1 bodies (m/s²; toward the midplane on either face).
    pub fn gravity(&self) -> f64 {
        self.config.gravity_y.abs()
    }
    /// An impulse (N·s) on the body at a world point (a cell's buoyancy, docs/BUILT_BODIES.md §6).
    pub fn apply_impulse_at(&mut self, h: BodyHandle, impulse: DVec3, point: DVec3) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.apply_impulse_at_point(impulse, point, true);
        }
    }
    pub fn set_rotation(&mut self, h: BodyHandle, q: DQuat) {
        if let Some(b) = self.w.bodies.get_mut(h.0) {
            b.set_rotation(q, true);
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
        let prediction = self.w.integration_parameters.prediction_distance();
        for &collider in b.colliders() {
            for pair in self.w.narrow_phase.contact_pairs_with(collider) {
                let other = if pair.collider1 == collider {
                    pair.collider2
                } else {
                    pair.collider1
                };
                // A held piece or a stand-in holds the player off it a little (as
                // `tier1_contacts`): that contact still stops the player.
                let reach = if self
                    .w
                    .colliders
                    .get(other)
                    .and_then(|c| c.parent())
                    .is_some_and(|h| self.pushed_by_players(BodyHandle(h)))
                {
                    Self::TIER1_CONTACT_REACH
                } else {
                    prediction
                };
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

/// A piece cell's friction coefficient (rapier averages the two colliders' coefficients).
pub const PIECE_FRICTION: f64 = 0.8;
/// Terrain's friction coefficient (`terrain.rs`).
pub const TERRAIN_FRICTION: f64 = 0.0;

/// The cell `c` of a piece as its collider: a box of its mass, colliding as its `collide` says.
fn piece_collider(c: &PieceCell) -> Collider {
    let groups = match c.collide {
        Collide::All => groups(Layer::Tier1),
        Collide::World => InteractionGroups::new(TIER1, TERRAIN | TIER1, InteractionTestMode::And),
        Collide::None => InteractionGroups::new(TIER1, Group::NONE, InteractionTestMode::And),
    };
    ColliderBuilder::cuboid(c.half.x, c.half.y, c.half.z)
        .translation(c.center)
        .mass(c.mass)
        .friction(PIECE_FRICTION)
        .restitution(0.05)
        .collision_groups(groups)
        .build()
}
