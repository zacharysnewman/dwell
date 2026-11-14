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
/// Pieces kept apart (a blast's, for its `noPieceContactS`; docs/EXPLOSIVES.md §3): Tier 1 bodies
/// that touch everything but one another. Every other layer's filter takes them.
const APART: Group = Group::GROUP_4;

pub(crate) fn groups(layer: Layer) -> InteractionGroups {
    let (member, filter) = match layer {
        Layer::Terrain => (TERRAIN, TIER1 | CHARACTER | APART),
        Layer::Tier1 => (TIER1, TERRAIN | TIER1 | CHARACTER | APART),
        Layer::Character => (CHARACTER, TERRAIN | TIER1 | CHARACTER | APART),
    };
    InteractionGroups::new(member, filter, InteractionTestMode::And)
}

/// The groups a probe of moving bodies matches (Tier 1 and characters).
pub(crate) fn moving_groups() -> InteractionGroups {
    InteractionGroups::new(
        Group::ALL,
        TIER1 | CHARACTER | APART,
        InteractionTestMode::And,
    )
}

pub(crate) fn layer_of(collider: &Collider) -> Layer {
    let m = collider.collision_groups().memberships;
    if m.contains(CHARACTER) {
        Layer::Character
    } else if m.intersects(TIER1 | APART) {
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
    /// A shaped cell (Phase 55 T2: a felled trunk's upside-down wood ramp): its collider is the
    /// convex hull of this state's shape (`dwell_blocks::shape`) over the box, not the box. None:
    /// the box. A shaped cell is never merged with its neighbours, except an octagonal log's
    /// (ADR 0072, Phase 55 T4): its hull is the octagon prism, merged along a run of the same log.
    pub hull: Option<dwell_blocks::StateId>,
    /// An octagonal hull's sides joined to the same axis's logs beside it in the piece
    /// (`dwell_blocks::octagon::joined_mask`, squared off as the terrain's are); set by
    /// [`merged_boxes`] from the piece's cells (0 on the way in).
    pub joined: u8,
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
    /// The pieces' bodies (`add_piece`, `add_kinematic_piece`): what players push; each with its
    /// cells' centres in its frame (its colliders are the cells merged into boxes).
    pieces: dwell_world::hash::HashMap<RigidBodyHandle, Vec<DVec3>>,
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
        self.w.step_with_events(&PinchGrip, &());
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
        self.insert_piece_cells(h, cells);
        BodyHandle(h)
    }

    /// Piece `h`'s colliders from `cells` (merged into boxes) and its cells' centres.
    fn insert_piece_cells(&mut self, h: RigidBodyHandle, cells: &[PieceCell]) {
        self.pieces
            .insert(h, cells.iter().map(|c| c.center).collect());
        for c in merged_boxes(cells) {
            self.w
                .colliders
                .insert_with_parent(piece_collider(&c), h, &mut self.w.bodies);
        }
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
        self.insert_piece_cells(h.0, cells);
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
        // Every cell, as the server's piece has them: the colliding ones touch the player, and
        // all of them give it the piece's mass (a player's push is predicted by it).
        self.insert_piece_cells(h, cells);
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

    /// The world centres of a piece's cells (or another body's colliders), into `out`.
    pub fn collider_centres(&self, h: BodyHandle, out: &mut Vec<DVec3>) {
        out.clear();
        if let (Some(cells), Some(b)) = (self.pieces.get(&h.0), self.w.bodies.get(h.0)) {
            let pose = b.position();
            out.extend(cells.iter().map(|&c| pose.translation + pose.rotation * c));
            return;
        }
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

    /// Keeps piece `body` apart from the other apart pieces (`apart`: it still touches the terrain,
    /// players and every other body), or lets it touch them again (docs/EXPLOSIVES.md §3,
    /// `noPieceContactS`). Its colliders that touch nothing stay so.
    pub fn set_apart(&mut self, body: BodyHandle, apart: bool) {
        let Some(b) = self.w.bodies.get(body.0) else {
            return;
        };
        for &c in b.colliders() {
            let Some(col) = self.w.colliders.get_mut(c) else {
                continue;
            };
            let g = col.collision_groups();
            if g.filter.is_empty() {
                continue;
            }
            let (memberships, filter) = if apart {
                ((g.memberships - TIER1) | APART, g.filter - APART)
            } else {
                ((g.memberships - APART) | TIER1, g.filter | APART)
            };
            col.set_collision_groups(InteractionGroups::new(memberships, filter, g.test_mode));
        }
    }

    /// Whether piece `body` is kept apart (`set_apart`).
    pub fn is_apart(&self, body: BodyHandle) -> bool {
        self.w.bodies.get(body.0).is_some_and(|b| {
            b.colliders().iter().any(|&c| {
                self.w
                    .colliders
                    .get(c)
                    .is_some_and(|col| col.collision_groups().memberships.contains(APART))
            })
        })
    }

    /// Whether any of `body`'s colliders overlaps another body's (a piece, apart or not; not a
    /// player or the terrain): an apart piece lets the others touch it again only once this is
    /// false, so pieces that landed in one another don't pop apart.
    pub fn overlaps_pieces(&self, body: BodyHandle) -> bool {
        let Some(b) = self.w.bodies.get(body.0) else {
            return false;
        };
        let mut filter = QueryFilter::default().groups(InteractionGroups::new(
            Group::ALL,
            TIER1 | APART,
            InteractionTestMode::And,
        ));
        filter.exclude_rigid_body = Some(body.0);
        let q = self.w.broad_phase.as_query_pipeline(
            self.w.narrow_phase.query_dispatcher(),
            &self.w.bodies,
            &self.w.colliders,
            filter,
        );
        b.colliders().iter().any(|&c| {
            self.w.colliders.get(c).is_some_and(|col| {
                !col.collision_groups().filter.is_empty()
                    && q.intersect_shape(*col.position(), col.shape())
                        .next()
                        .is_some()
            })
        })
    }

    /// The contact pairs with touching contacts between two pieces (Tier 1 bodies, apart or
    /// not), the terrain and players left out (tests: an apart blast's pieces make none).
    pub fn piece_contact_pairs(&self) -> usize {
        let piece = |c: ColliderHandle| {
            self.w
                .colliders
                .get(c)
                .is_some_and(|c| c.collision_groups().memberships.intersects(TIER1 | APART))
        };
        self.w
            .narrow_phase
            .contact_pairs()
            .filter(|p| p.has_any_active_contact() && piece(p.collider1) && piece(p.collider2))
            .count()
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
        self.pieces.contains_key(&h.0)
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

/// A piece's contact pressing a player down grips them (BUGS.md #52, PLAYER_CONTROLLER.md §6.6):
/// a contact whose push-out normal toward the player points down more than this (face-local y,
/// a surface within 60° of facing straight down) takes the piece's friction, `PIECE_FRICTION`.
pub const PINCH_MIN_DOWN: f64 = 0.5;

/// The contact hook of every step ([`Physics::step`]; the player's collider asks for it,
/// `ActiveHooks::MODIFY_SOLVER_CONTACTS`). The player's capsule is frictionless (the controller
/// owns its walking), which a body coming down on them turned into a launch: a falling trunk's
/// slanted underside squeezing the capsule against the ground can only push it out sideways, at
/// the trunk's descent over the slant's slope (a 14° underside at 5 m/s: 20 m/s, and faster as it
/// flattens — 57 m/s, 155 m: the watermelon seed). Real bodies grip what they press down on, so a
/// piece's contact from above takes the piece's own friction (wood's 0.8 holds a wedge flatter
/// than 39°): the player is pinned and carried along at the trunk's own speed there, not shot
/// out. Contacts from the side (a body swung into a player, a piece pushed) and the ground
/// stay frictionless: a hit throws the player as hard as the solver says.
struct PinchGrip;

impl PhysicsHooks for PinchGrip {
    fn modify_solver_contacts(&self, ctx: &mut ContactModificationContext) {
        let user = |h: Option<RigidBodyHandle>| {
            h.and_then(|h| ctx.bodies.get(h))
                .map_or((0, 0.0), |b| (b.user_data, b.translation().y))
        };
        let ((u1, y1), (u2, y2)) = (user(ctx.rigid_body1), user(ctx.rigid_body2));
        let player = |u: u128| u != 0 && u != TIER1_BODY;
        // The push-out normal toward the player (the manifold's points from collider 1 to 2).
        let (sign, y) = if player(u2) && u1 == TIER1_BODY {
            (1.0, y2)
        } else if player(u1) && u2 == TIER1_BODY {
            (-1.0, y1)
        } else {
            return;
        };
        let Some(m) = ctx.rigid_mut() else {
            return;
        };
        let n = *m.normal * sign;
        let down = -FaceFrame::at_height(y).vector(n.to_array())[1];
        if down > PINCH_MIN_DOWN {
            *m.friction = PIECE_FRICTION;
        }
    }
}

/// A piece cell's friction coefficient (rapier averages the two colliders' coefficients).
pub const PIECE_FRICTION: f64 = 0.8;
/// Terrain's friction coefficient (`terrain.rs`).
pub const TERRAIN_FRICTION: f64 = 0.0;

/// A piece's cells as fewer boxes (ARCHITECTURE.md §7.1): runs of cells alike (the same size, mass
/// and collision) merged greedily along x, then z, then y into one box each, its mass their sum.
/// The same mass, centre of mass and inertia as the cells' boxes (each box uniform), and the same
/// surface outside; a 1.5 m sand column is one box, not three, and its contacts a third.
pub fn merged_boxes(cells: &[PieceCell]) -> Vec<PieceCell> {
    use dwell_world::hash::HashMap;
    // A cell's place in its piece's grid (its size apart), with its kind.
    type Kind = ([u64; 3], u64, u8);
    let kind = |c: &PieceCell| -> Kind {
        (
            [c.half.x.to_bits(), c.half.y.to_bits(), c.half.z.to_bits()],
            c.mass.to_bits(),
            c.collide as u8,
        )
    };
    let index = |c: &PieceCell| -> [i64; 3] {
        let size = c.half * 2.0;
        [
            (c.center.x / size.x).round() as i64,
            (c.center.y / size.y).round() as i64,
            (c.center.z / size.z).round() as i64,
        ]
    };
    let mut grid: HashMap<(Kind, [i64; 3]), usize> = HashMap::default();
    for (k, c) in cells.iter().enumerate() {
        if c.hull.is_none() {
            grid.insert((kind(c), index(c)), k);
        }
    }
    let mut used = vec![false; cells.len()];
    let mut out = Vec::new();
    octagon_prisms(cells, &index, &mut used, &mut out);
    for (k, c) in cells.iter().enumerate() {
        if used[k] {
            continue;
        }
        if c.hull.is_some() {
            // Shaped: its own collider, never merged.
            used[k] = true;
            out.push(*c);
            continue;
        }
        let (kd, [x0, y0, z0]) = (kind(c), index(c));
        let free = |used: &[bool], x: i64, y: i64, z: i64| {
            grid.get(&(kd, [x, y, z])).is_some_and(|&j| !used[j])
        };
        let mut nx = 1;
        while free(&used, x0 + nx, y0, z0) {
            nx += 1;
        }
        let mut nz = 1;
        while (0..nx).all(|i| free(&used, x0 + i, y0, z0 + nz)) {
            nz += 1;
        }
        let mut ny = 1;
        while (0..nx).all(|i| (0..nz).all(|j| free(&used, x0 + i, y0 + ny, z0 + j))) {
            ny += 1;
        }
        for y in 0..ny {
            for z in 0..nz {
                for x in 0..nx {
                    used[grid[&(kd, [x0 + x, y0 + y, z0 + z])]] = true;
                }
            }
        }
        let n = DVec3::new(nx as f64, ny as f64, nz as f64);
        out.push(PieceCell {
            center: c.center + c.half * (n - DVec3::ONE),
            half: c.half * n,
            mass: c.mass * n.x * n.y * n.z,
            collide: c.collide,
            hull: None,
            joined: 0,
        });
    }
    out
}

/// The octagonal cells of a piece (ADR 0072, Phase 55 T4: a felled trunk's logs and wood ramps) as
/// prisms: each cell's sides joined where a log of its axis lies beside it in the piece (as the
/// terrain's are), and a log's run along its axis of the same state, join and collision one prism.
/// Marks them `used` and pushes them to `out`.
fn octagon_prisms(
    cells: &[PieceCell],
    index: &dyn Fn(&PieceCell) -> [i64; 3],
    used: &mut [bool],
    out: &mut Vec<PieceCell>,
) {
    use dwell_blocks::octagon;
    use dwell_world::hash::HashMap;
    let oct = |c: &PieceCell| c.hull.filter(|&m| octagon::is_octagon(m));
    let grid: HashMap<[i64; 3], usize> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| oct(c).is_some())
        .map(|(k, c)| (index(c), k))
        .collect();
    if grid.is_empty() {
        return;
    }
    let state_at = |p: [i64; 3]| grid.get(&p).and_then(|&k| oct(&cells[k]));
    let joined: Vec<u8> = cells
        .iter()
        .map(|c| {
            let Some(m) = oct(c) else { return 0 };
            let p = index(c);
            octagon::joined_mask(m, &|o| {
                state_at([
                    p[0] + i64::from(o[0]),
                    p[1] + i64::from(o[1]),
                    p[2] + i64::from(o[2]),
                ])
                .unwrap_or(dwell_blocks::AIR)
            })
        })
        .collect();
    for (k, c) in cells.iter().enumerate() {
        let Some(m) = oct(c) else { continue };
        if used[k] {
            continue;
        }
        used[k] = true;
        let Some((_, axis)) = octagon::section_of(m) else {
            continue;
        };
        if octagon::full(m) {
            // The run along the axis from here (cells visited in any order: extend both ways).
            let p = index(c);
            let same = |j: usize| {
                cells[j].hull == c.hull
                    && joined[j] == joined[k]
                    && cells[j].collide == c.collide
                    && cells[j].half == c.half
                    && cells[j].mass == c.mass
            };
            let at = |d: i64| {
                let mut q = p;
                q[axis] += d;
                grid.get(&q).copied()
            };
            let (mut lo, mut hi) = (0i64, 0i64);
            while let Some(j) = at(lo - 1).filter(|&j| !used[j] && same(j)) {
                used[j] = true;
                lo -= 1;
            }
            while let Some(j) = at(hi + 1).filter(|&j| !used[j] && same(j)) {
                used[j] = true;
                hi += 1;
            }
            let n = hi - lo + 1;
            let mut off = DVec3::ZERO;
            off[axis] = (lo + hi) as f64 * c.half[axis];
            let mut half = c.half;
            half[axis] *= n as f64;
            out.push(PieceCell {
                center: c.center + off,
                half,
                mass: c.mass * n as f64,
                collide: c.collide,
                hull: c.hull,
                joined: joined[k],
            });
            continue;
        }
        out.push(PieceCell {
            joined: joined[k],
            ..*c
        });
    }
}

/// How far (m) each face of a piece cell's box is drawn in from the cell's. Cells detached together
/// lie face to face with their neighbours and the walls round them: boxes the cells' size touch
/// exactly, and the solver held a packed raft of falling sand by its walls' friction, creeping
/// down at ~0.2 m/s for the 30 s until it timed out (seed 0's dam site, 2026-11-06). A centimetre
/// apart they fall freely; nothing draws a piece by its colliders.
pub const PIECE_SKIN: f64 = 0.01;

/// The cell `c` of a piece as its collider: a box of its mass, colliding as its `collide` says.
fn piece_collider(c: &PieceCell) -> Collider {
    let groups = match c.collide {
        Collide::All => groups(Layer::Tier1),
        Collide::World => {
            InteractionGroups::new(TIER1, TERRAIN | TIER1 | APART, InteractionTestMode::And)
        }
        Collide::None => InteractionGroups::new(TIER1, Group::NONE, InteractionTestMode::And),
    };
    // Each face drawn in by PIECE_SKIN, so pieces side by side (and a piece beside a wall) don't
    // touch until they move together.
    let half = (c.half - DVec3::splat(PIECE_SKIN)).max(c.half * 0.5);
    let hull = c.hull.and_then(|m| {
        // A log's run is its octagon prism; a wood ramp riding a felled trunk keeps its square
        // ramp's hull (Phase 55 T4 deviation: on the octagon's narrower hinge the felling test's
        // trunk slid and rolled off the stump instead of tipping into the notch).
        let pts = if dwell_blocks::octagon::is_octagon(m) && dwell_blocks::octagon::full(m) {
            octagon_hull_points(m, c.joined, c.half, half)
        } else {
            shape_hull_points(m, half)
        };
        ColliderBuilder::convex_hull(&pts)
    });
    hull.unwrap_or_else(|| ColliderBuilder::cuboid(half.x, half.y, half.z))
        .translation(c.center)
        .mass(c.mass)
        .friction(PIECE_FRICTION)
        .restitution(0.05)
        .collision_groups(groups)
        .build()
}

/// The hull a piece cell of state `m` collides as (`PieceCell::hull`): its shape when that is a
/// convex shape other than the full cube (a ramp), else none (the box). A log stays its box for now
/// (Phase 55 T4): with its octagon prism (`octagon_prisms`, built and tested, given by setting
/// `hull: Some(log)`) a player's push no longer slid a log on land
/// (`pushing_a_log_on_land_is_predicted_without_snaps_at_150_ms_rtt`: 0 m pushed square on, from
/// 1.5–4 m), so the switch waits on that.
pub fn piece_hull(m: dwell_blocks::StateId) -> Option<dwell_blocks::StateId> {
    let s = dwell_blocks::shape::shape_of(m);
    let cube = s.corners == [2, 2, 2, 2] && s.min_y == 0.0 && s.max_y == 1.0;
    (s.convex && s.face_count > 0 && !cube && dwell_blocks::try_info(m).is_some_and(|i| i.solid))
        .then_some(m)
}

/// The corners of octagonal state `m`'s prism (`dwell_blocks::octagon::state_faces`, sides
/// `joined`), its cell `size` half-extents (`PieceCell::half`, the run's along the axis) drawn in to
/// `half` about the origin: the chamfer square in metres, as the terrain's.
pub fn octagon_hull_points(
    m: dwell_blocks::StateId,
    joined: u8,
    size: DVec3,
    half: DVec3,
) -> Vec<DVec3> {
    let size_m = [2.0 * size.x, 2.0 * size.y, 2.0 * size.z];
    let mut out: Vec<DVec3> = Vec::new();
    for f in dwell_blocks::octagon::state_faces(m, joined, size_m) {
        for p in &f.pts {
            let q = DVec3::new(
                (p[0] * 2.0 - 1.0) * half.x,
                (p[1] * 2.0 - 1.0) * half.y,
                (p[2] * 2.0 - 1.0) * half.z,
            );
            if !out.iter().any(|o| (*o - q).length() < 1e-9) {
                out.push(q);
            }
        }
    }
    out
}

/// The corners of state `m`'s shape (its faces' points, fractions of the cell) over a box of
/// half-extents `half` about the origin: a shaped piece cell's convex hull (`PieceCell::hull`).
pub fn shape_hull_points(m: dwell_blocks::StateId, half: DVec3) -> Vec<DVec3> {
    let shape = dwell_blocks::shape::shape_of(m);
    let mut out: Vec<DVec3> = Vec::new();
    for f in dwell_blocks::shape::faces_of(shape) {
        for p in &f.v[..usize::from(f.count)] {
            let q = DVec3::new(
                (f64::from(p[0]) * 2.0 - 1.0) * half.x,
                (f64::from(p[1]) * 2.0 - 1.0) * half.y,
                (f64::from(p[2]) * 2.0 - 1.0) * half.z,
            );
            if !out.iter().any(|o| (*o - q).length() < 1e-9) {
                out.push(q);
            }
        }
    }
    out
}

#[cfg(test)]
mod piece_box_tests {
    use super::*;

    fn cell(x: f64, row: f64, z: f64, mass: f64, collide: Collide) -> PieceCell {
        PieceCell {
            center: DVec3::new(x, row * 0.5, z),
            half: DVec3::new(0.5, 0.25, 0.5),
            mass,
            collide,
            hull: None,
            joined: 0,
        }
    }

    // face-a-only: the boxes are in the piece's own frame, which has no up.
    #[test]
    fn a_shaped_piece_cell_is_its_own_convex_hull_never_merged() {
        let ramp = dwell_blocks::parse_state("dwell:log_ramp[facing=west,half=top,shape=ramp]")
            .expect("the wood ramp");
        let mut cells = vec![
            cell(0.0, 0.0, 0.0, 100.0, Collide::All),
            cell(0.0, 1.0, 0.0, 100.0, Collide::All),
        ];
        cells[0].hull = piece_hull(ramp);
        let merged = merged_boxes(&cells);
        assert_eq!(
            merged.len(),
            2,
            "the shaped cell is not merged with the box above"
        );
        let c = piece_collider(&merged[0]);
        assert!(
            c.shape().as_convex_polyhedron().is_some(),
            "a hull, not a box"
        );
        assert!(piece_collider(&merged[1]).shape().as_cuboid().is_some());
    }

    /// Whether a ray from `o` along `d` (the collider's own frame, about its centre) hits `c`.
    fn ray_hits(c: &Collider, o: DVec3, d: DVec3) -> bool {
        use rapier3d_f64::parry::query::Ray;
        let ray = Ray::new(o - c.translation(), d);
        c.shape().cast_local_ray(&ray, 10.0, true).is_some()
    }

    // face-a-only: the prism is in the piece's own frame, which has no up (the octagon is
    // symmetric in y; tests/pieces.rs drops one on both faces).
    #[test]
    fn a_felled_trunks_logs_collide_as_one_octagonal_prism() {
        // Phase 55 T4 (owner, 2026-11-09: "everything log/tree should be octagonal"): a felled
        // trunk's logs were boxes merged into one, so a lying trunk rested on square corners. Red
        // before: the run merged into a cuboid and the ray past the cut corner hit it. (Not yet
        // what `piece_hull` gives a log: see there.)
        let log = dwell_blocks::require("dwell:log[axis=x]");
        let cells: Vec<PieceCell> = (0..4)
            .map(|x| PieceCell {
                hull: Some(log),
                ..cell(f64::from(x), 0.0, 0.0, 350.0, Collide::All)
            })
            .collect();
        let merged = merged_boxes(&cells);
        assert_eq!(
            merged.len(),
            1,
            "a run of one log along its axis is one prism"
        );
        assert_eq!(merged[0].mass, 1400.0);
        assert_eq!(merged[0].half, DVec3::new(2.0, 0.25, 0.5));
        let c = piece_collider(&merged[0]);
        assert!(
            c.shape().as_convex_polyhedron().is_some(),
            "a hull, not a box"
        );
        let down = DVec3::new(0.0, -1.0, 0.0);
        let along = DVec3::new(1.0, 0.0, 0.0);
        // Along the trunk 2 cm in from its top and +z edges: past the chamfer, inside the box.
        let corner = DVec3::new(-5.0, 0.22, 0.47);
        assert!(!ray_hits(&c, corner, along), "the corner is cut");
        assert!(
            ray_hits(&c, DVec3::new(-5.0, 0.0, 0.0), along),
            "its middle is solid"
        );
        assert!(ray_hits(&c, DVec3::new(1.5, 1.0, 0.0), down));
        // Two runs side by side join (as the terrain's do): the corner between them is squared.
        let mut two = cells.clone();
        two.extend((0..4).map(|x| PieceCell {
            hull: Some(log),
            ..cell(f64::from(x), 0.0, 1.0, 350.0, Collide::All)
        }));
        let merged = merged_boxes(&two);
        assert_eq!(merged.len(), 2);
        let near = merged
            .iter()
            .find(|m| m.center.z < 0.5)
            .expect("the z = 0 run");
        assert_ne!(near.joined, 0, "joined to its neighbour");
        assert!(
            ray_hits(&piece_collider(near), corner, along),
            "the corner at the join is squared"
        );
    }

    #[test]
    fn a_pieces_cells_merge_into_fewer_boxes_of_the_same_mass_and_extent() {
        // 2026-11-06: a box per cell made a 1.5 m sand column three colliders, each touching the
        // terrain and its neighbours: a pit's 177 columns cost the server's physics 2.5 ms a
        // tick natively. Merged, a column is one box.
        let column: Vec<PieceCell> = (0..3)
            .map(|r| cell(0.0, f64::from(r), 0.0, 400.0, Collide::World))
            .collect();
        let boxes = merged_boxes(&column);
        assert_eq!(boxes.len(), 1, "a column is one box");
        assert_eq!(boxes[0].center, DVec3::new(0.0, 0.5, 0.0));
        assert_eq!(boxes[0].half, DVec3::new(0.5, 0.75, 0.5));
        assert_eq!(boxes[0].mass, 1200.0);
        // An L of logs with a leaf riding it: logs merge, the leaf (another mass and collision)
        // stays its own; the mass and its centre are the cells'.
        let mut l: Vec<PieceCell> = (0..4)
            .map(|x| cell(f64::from(x), 0.0, 0.0, 300.0, Collide::All))
            .collect();
        l.push(cell(0.0, 1.0, 0.0, 300.0, Collide::All));
        l.push(cell(1.0, 1.0, 0.0, 20.0, Collide::None));
        let boxes = merged_boxes(&l);
        assert_eq!(boxes.len(), 3, "the run of logs, the log over it, the leaf");
        let mass = |b: &[PieceCell]| b.iter().map(|c| c.mass).sum::<f64>();
        let centre = |b: &[PieceCell]| b.iter().map(|c| c.center * c.mass).sum::<DVec3>() / mass(b);
        let volume = |b: &[PieceCell]| {
            b.iter()
                .map(|c| 8.0 * c.half.x * c.half.y * c.half.z)
                .sum::<f64>()
        };
        assert_eq!(mass(&boxes), mass(&l));
        assert!((centre(&boxes) - centre(&l)).length() < 1e-12);
        assert!((volume(&boxes) - volume(&l)).abs() < 1e-12);
    }

    // face-a-only: as above.
    #[test]
    fn a_merged_piece_still_reports_each_cells_centre() {
        // The water (`felling::water_drag`, `float`) reads a piece's cells, not its colliders.
        let mut physics = Physics::default();
        let column: Vec<PieceCell> = (0..3)
            .map(|r| cell(0.0, f64::from(r), 0.0, 400.0, Collide::World))
            .collect();
        let h = physics.add_piece(DVec3::new(10.0, 20.0, 30.0), &column, false);
        assert_eq!(physics.w.bodies[h.0].colliders().len(), 1);
        let mut centres = Vec::new();
        physics.collider_centres(h, &mut centres);
        assert_eq!(
            centres,
            vec![
                DVec3::new(10.0, 20.0, 30.0),
                DVec3::new(10.0, 20.5, 30.0),
                DVec3::new(10.0, 21.0, 30.0)
            ]
        );
        assert!((physics.mass(h) - 1200.0).abs() < 1e-9);
    }
}
