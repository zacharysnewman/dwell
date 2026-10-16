//! The physics player controller (PLAYER_CONTROLLER.md): the PPC's Quantum systems on the voxel
//! grid. Each player is a dynamic capsule driven by velocity layers; the ordered passes below run
//! for every player before the physics step. Transcendental functions come from `libm`, so native
//! and WASM builds agree bit for bit.

use dwell_blocks::{Facing, VoxelShape, info, shape::solid_span_at};
use dwell_world::{
    MIDPLANE_Y, SEA_LEVEL, VoxelWorld, WORLD_MAX_Y, chunk_distance, chunk_of, mirror_y,
};
use rapier3d_f64::prelude::*;

use crate::config::PlayerControllerConfig;
use crate::frame::{FaceQuery, Frame};
use crate::physics::{BodyHandle, DVec3, Layer, Physics, Vec3, groups};
use crate::query::{Capsule, GroundRef, ProbeHit, VoxelQuery};
use crate::terrain::TerrainCollision;

const DT: f32 = PlayerControllerConfig::dt();
const RAD_TO_DEG: f32 = 180.0 / std::f32::consts::PI;
const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;

// Constants from the PPC Quantum systems.
const RING_RAYS: usize = 16;
const RISING_MARGIN: f32 = 0.10;
const WALL_MIN_ANGLE: f32 = 45.0;
const INPUT_DEADZONE: f32 = 0.10;
const REVERSE_DOT: f32 = -0.10;
const MIN_DIRECTION: f32 = 0.01;
/// How often far terrain collision unloads, and within how many chunks of a player it is kept.
const RETAIN_EVERY_TICKS: u32 = 64;
const RETAIN_CHUNKS: i32 = 2;
const MIN_STEP: f32 = 0.01;
const LAUNCH_THRESHOLD: f32 = 0.10;
/// Dwell: during step_grace.
const STEP_LAUNCH_THRESHOLD: f32 = 1.5;
const STEP_GRACE_TICKS: u8 = 6;
const ABSORB_THRESHOLD: f32 = 0.01;
const MOVING_SPEED: f32 = 0.10;
const CROUCH_SKIN: f32 = 0.02;
/// Slightly inside touching distance (PPCClimbSystem).
const LADDER_INSET: f32 = 0.10;
/// A voxel ladder is a thin plate on the cell's back face (§6.3).
const LADDER_PLATE_HALF_DEPTH: f32 = 0.05;
/// Climbable region above a column's top cell.
const LADDER_TOP_REACH: f32 = 0.6;
/// How far past the midplane (m) a body has to be before the face switches.
const FACE_HYSTERESIS: f64 = 0.05;
const MAX_CONTACTS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Idle,
    Walking,
    Running,
    Crouching,
    Sliding,
    Jumping,
    Falling,
    Climbing,
    Swimming,
    Flying,
}

/// One tick of input. Buttons are held state; presses are detected in-sim.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// x = right, y = forward, camera-relative, |move| ≤ 1.
    pub move_x: f32,
    pub move_y: f32,
    /// Degrees; 0 = +Z, 90 = +X (right-handed: right of +Z is −X).
    pub look_yaw: f32,
    /// Degrees; positive = up.
    pub look_pitch: f32,
    pub jump: bool,
    pub run: bool,
    pub crouch: bool,
    /// ◆ Creative flight: held while the flight mode is on.
    pub fly: bool,
    /// ◆ Flight speed level (the speed slider): a true minimum speed.
    pub fly_speed: u8,
}

/// 2^(level / 2), level capped at `FLY_SPEED_MAX_LEVEL`: exact in f32.
pub fn fly_speed_factor(level: u8) -> f32 {
    let l = i32::from(level).min(dwell_protocol::players::FLY_SPEED_MAX_LEVEL as i32);
    libm::ldexpf(if l & 1 != 0 { 2.0f32.sqrt() } else { 1.0 }, l >> 1)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundInfo {
    /// THE grounded flag every pass uses.
    pub grounded: bool,
    pub was_grounded: bool,
    pub normal: Vec3,
    /// Degrees.
    pub slope_angle: f32,
    /// The ground under the capsule's centre (the centre ray's hit, else `normal`) and its angle
    /// (degrees): what slides (§6.8).
    pub under_normal: Vec3,
    pub under_angle: f32,
    /// Capsule bottom → ground.
    pub gap: f32,
    pub ground: GroundRef,
    pub ceiling_blocked: bool,
    pub touching_wall: bool,
    pub wall_normal: Vec3,
}

impl Default for GroundInfo {
    fn default() -> Self {
        Self {
            grounded: false,
            was_grounded: false,
            normal: Vec3::Y,
            slope_angle: 0.0,
            under_normal: Vec3::Y,
            under_angle: 0.0,
            gap: 0.0,
            ground: GroundRef::None,
            ceiling_blocked: false,
            touching_wall: false,
            wall_normal: Vec3::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HorizontalLayer {
    /// Player-driven.
    pub current: Vec3,
    /// Absorbed external velocity, decaying.
    pub external: Vec3,
    /// This tick's output (read back next tick).
    pub contribution: Vec3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VerticalLayer {
    pub accumulated_y: f32,
    pub platform_y: f32,
    pub last_platform_y: f32,
    pub target_y: f32,
    /// Ticks after a step-up during which small upward deviations are not launches.
    pub step_grace: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JumpState {
    pub buffer_ticks: u16,
    pub coyote_ticks: u16,
    pub jumping: bool,
    pub jumped_this_tick: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClimbState {
    pub climbing: bool,
    /// A cell of the ladder being climbed.
    pub ladder: Cell,
    pub velocity: Vec3,
    pub has_released: bool,
    /// The column let go of; not re-grabbed until the capsule leaves it.
    pub released: Cell,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwimState {
    pub swimming: bool,
    pub submerged: f32,
    pub velocity: Vec3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlyState {
    pub flying: bool,
    pub velocity: Vec3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlatformState {
    /// What the ground probe sees (tracked even while airborne).
    pub ground: GroundRef,
    /// Velocity of that ground under the player.
    pub ground_velocity: Vec3,
    /// `ground_velocity` while grounded (the carry), else 0.
    pub base_velocity: Vec3,
    /// Degrees this tick, for the camera.
    pub yaw_delta: f32,
}

/// Events raised during a tick (bits of `PlayerController::events`).
pub mod events {
    pub const JUMPED: u32 = 1 << 0;
    /// Payload: `landed_speed`.
    pub const LANDED: u32 = 1 << 1;
    pub const CROUCH_CHANGED: u32 = 1 << 2;
    pub const CLIMB_STARTED: u32 = 1 << 3;
    pub const CLIMB_ENDED: u32 = 1 << 4;
    pub const SWIM_STARTED: u32 = 1 << 5;
    pub const SWIM_ENDED: u32 = 1 << 6;
    pub const FLY_STARTED: u32 = 1 << 7;
    pub const FLY_ENDED: u32 = 1 << 8;
}

/// Complete controller state: a plain value, copied into history buffers and snapshots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerController {
    pub input: Input,
    pub previous_input: Input,
    pub state: State,
    pub ground: GroundInfo,
    pub horizontal: HorizontalLayer,
    pub vertical: VerticalLayer,
    pub jump: JumpState,
    pub crouching: bool,
    pub climb: ClimbState,
    pub swim: SwimState,
    pub fly: FlyState,
    pub platform: PlatformState,
    pub target_velocity: Vec3,
    /// The face the layers' vertical quantities are in: +1 face A, −1 face B.
    pub face: i8,
    pub events: u32,
    /// Impact speed of this tick's Landed event.
    pub landed_speed: f32,
}

impl Default for PlayerController {
    fn default() -> Self {
        Self {
            input: Input::default(),
            previous_input: Input::default(),
            state: State::Idle,
            ground: GroundInfo::default(),
            horizontal: HorizontalLayer::default(),
            vertical: VerticalLayer::default(),
            jump: JumpState::default(),
            crouching: false,
            climb: ClimbState::default(),
            swim: SwimState::default(),
            fly: FlyState::default(),
            platform: PlatformState::default(),
            target_velocity: Vec3::ZERO,
            face: 1,
            events: 0,
            landed_speed: 0.0,
        }
    }
}

impl PlayerController {
    pub fn jump_pressed(&self) -> bool {
        self.input.jump && !self.previous_input.jump
    }
    pub fn crouch_pressed(&self) -> bool {
        self.input.crouch && !self.previous_input.crouch
    }
    pub fn exclusive(&self) -> bool {
        self.climb.climbing || self.swim.swimming || self.fly.flying
    }
    /// On ground steeper than `min_angle` (°), and not climbing, swimming or flying: the slide
    /// layer moves the player (§6.8). Derived from this tick's probe and the resumable flags, so it
    /// needs no state of its own on the wire. Another player's head (a capsule's round top) is not
    /// a slope.
    pub fn sliding(&self, min_angle: f32) -> bool {
        !self.exclusive()
            && self.ground.grounded
            && self.ground.under_angle > min_angle
            && !matches!(self.ground.ground, GroundRef::Player(_))
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn move_towards(current: Vec3, target: Vec3, max_delta: f32) -> Vec3 {
    let delta = target - current;
    let length = delta.length();
    if length <= max_delta || length == 0.0 {
        return target;
    }
    current + delta * (max_delta / length)
}

fn clamp_length(v: Vec3, max: f32) -> Vec3 {
    let length = v.length();
    if length > max && length > 0.0 {
        v * (max / length)
    } else {
        v
    }
}

fn safe_normalized(v: Vec3) -> Vec3 {
    let length = v.length();
    if length > 0.0 { v / length } else { Vec3::ZERO }
}

fn normalized(v: Vec3) -> Vec3 {
    v / v.length()
}

fn slope_angle(normal: Vec3) -> f32 {
    libm::acosf(normal.y.clamp(-1.0, 1.0)) * RAD_TO_DEG
}

fn ring() -> [Vec3; RING_RAYS] {
    std::array::from_fn(|i| {
        let angle = i as f32 * (2.0 * std::f32::consts::PI / RING_RAYS as f32);
        Vec3::new(libm::cosf(angle), 0.0, libm::sinf(angle))
    })
}

fn facing_vector(f: Facing) -> Vec3 {
    match f {
        Facing::North => Vec3::new(0.0, 0.0, -1.0),
        Facing::East => Vec3::new(1.0, 0.0, 0.0),
        Facing::South => Vec3::new(0.0, 0.0, 1.0),
        Facing::West => Vec3::new(-1.0, 0.0, 0.0),
        Facing::None => Vec3::new(0.0, 0.0, 1.0),
    }
}

/// Dwell's world is right-handed with Y up: facing +Z (yaw 0), right is −X.
pub fn camera_right(yaw_degrees: f32) -> Vec3 {
    let yaw = yaw_degrees * DEG_TO_RAD;
    Vec3::new(-libm::cosf(yaw), 0.0, libm::sinf(yaw))
}

/// Move input as a horizontal, camera-relative world direction (length ≤ 1).
pub fn move_direction(input: &Input) -> Vec3 {
    let yaw = input.look_yaw * DEG_TO_RAD;
    let forward = Vec3::new(libm::sinf(yaw), 0.0, libm::cosf(yaw));
    forward * input.move_y + camera_right(input.look_yaw) * input.move_x
}

pub type PlayerHandle = u32;

struct Player {
    c: PlayerController,
    pending: Input,
    cfg: PlayerControllerConfig,
    body: BodyHandle,
    collider: ColliderHandle,
    /// Push-out normals (toward the player, world) of the body's contacts in the last step.
    contacts: Vec<DVec3>,
    /// Upward velocity the contacts gave the body by deflecting last tick's horizontal drive.
    drive_lift: f32,
}

/// Ladder frame: facing axis and plate centre of the ladder in a cell.
struct LadderFrame {
    facing: Vec3,
    plate: DVec3,
}

fn frame_of(cell: Cell, material: dwell_blocks::StateId) -> LadderFrame {
    let facing = facing_vector(info(material).facing);
    let center = DVec3::new(
        f64::from(cell.x) + 0.5,
        f64::from(cell.y) + 0.5,
        f64::from(cell.z) + 0.5,
    );
    LadderFrame {
        facing,
        plate: center - (facing * (0.5 - LADDER_PLATE_HALF_DEPTH)).as_dvec3(),
    }
}

fn capsule_shape(cfg: &PlayerControllerConfig, crouching: bool) -> SharedShape {
    let r = cfg.body.radius;
    SharedShape::capsule_y(
        f64::from((cfg.half_height(crouching) - r).max(0.0)),
        f64::from(r),
    )
}

/// Runs the controller for a set of players in one physics world.
pub struct Players {
    players: Vec<Option<Player>>,
    terrain: Option<TerrainCollision>,
    ticks: u32,
    ring: [Vec3; RING_RAYS],
}

impl Players {
    /// `terrain`: build voxel collision around players (off for worlds of bodies only).
    pub fn new(physics: &mut Physics, terrain: bool) -> Self {
        Self {
            players: Vec::new(),
            terrain: terrain.then(|| TerrainCollision::new(physics)),
            ticks: 0,
            ring: ring(),
        }
    }

    pub fn terrain(&self) -> Option<&TerrainCollision> {
        self.terrain.as_ref()
    }

    /// Spawns a player standing with its feet at `feet`, on the face that position lies on.
    pub fn spawn(
        &mut self,
        physics: &mut Physics,
        cfg: PlayerControllerConfig,
        feet: DVec3,
        yaw: f32,
    ) -> PlayerHandle {
        let handle = self
            .players
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.players.len()) as PlayerHandle;
        let mut c = PlayerController::default();
        c.input.look_yaw = yaw;
        c.face = if feet.y >= f64::from(MIDPLANE_Y) {
            1
        } else {
            -1
        };
        let center = feet
            + DVec3::new(
                0.0,
                f64::from(c.face) * f64::from(cfg.half_height(false)),
                0.0,
            );
        // Creative flight near the ceiling is far faster than the engine's default limit.
        physics.raise_max_linear_velocity(f64::from(
            cfg.fly.speed
                * cfg.fly.run_factor
                * (1.0 + cfg.fly.ceiling / cfg.fly.boost_height)
                * 1.1,
        ));
        // Rotation locked, no physics gravity, frictionless, never sleeps; continuous collision so
        // fast flight cannot tunnel (§2, §6.7).
        let body = RigidBodyBuilder::dynamic()
            .translation(center)
            .lock_rotations()
            .gravity_scale(0.0)
            .linear_damping(0.0)
            .angular_damping(0.0)
            .can_sleep(false)
            .ccd_enabled(true)
            .user_data(u128::from(handle) + 1)
            .build();
        let body = physics.w.bodies.insert(body);
        let collider = ColliderBuilder::new(capsule_shape(&cfg, false))
            .mass(f64::from(cfg.body.mass))
            .friction(0.0)
            .friction_combine_rule(CoefficientCombineRule::Min)
            .restitution(0.0)
            .restitution_combine_rule(CoefficientCombineRule::Min)
            .collision_groups(groups(Layer::Character))
            .build();
        let collider =
            physics
                .w
                .colliders
                .insert_with_parent(collider, body, &mut physics.w.bodies);
        let p = Player {
            c,
            pending: Input {
                look_yaw: yaw,
                ..Input::default()
            },
            cfg,
            body: BodyHandle(body),
            collider,
            contacts: Vec::new(),
            drive_lift: 0.0,
        };
        if handle as usize == self.players.len() {
            self.players.push(Some(p));
        } else {
            self.players[handle as usize] = Some(p);
        }
        handle
    }

    pub fn despawn(&mut self, physics: &mut Physics, h: PlayerHandle) {
        if let Some(p) = self.players.get_mut(h as usize).and_then(Option::take) {
            physics.remove_body(p.body);
        }
    }

    pub fn exists(&self, h: PlayerHandle) -> bool {
        self.players.get(h as usize).is_some_and(Option::is_some)
    }

    fn get(&self, h: PlayerHandle) -> &Player {
        self.players[h as usize].as_ref().expect("a live player")
    }
    fn get_mut(&mut self, h: PlayerHandle) -> &mut Player {
        self.players[h as usize].as_mut().expect("a live player")
    }

    pub fn set_input(&mut self, h: PlayerHandle, input: Input) {
        self.get_mut(h).pending = input;
    }
    pub fn controller(&self, h: PlayerHandle) -> &PlayerController {
        &self.get(h).c
    }
    pub fn controller_mut(&mut self, h: PlayerHandle) -> &mut PlayerController {
        &mut self.get_mut(h).c
    }
    pub fn config(&self, h: PlayerHandle) -> &PlayerControllerConfig {
        &self.get(h).cfg
    }
    pub fn body(&self, h: PlayerHandle) -> BodyHandle {
        self.get(h).body
    }
    /// The player whose body this is.
    pub fn player_of_body(&self, body: BodyHandle) -> Option<PlayerHandle> {
        self.players
            .iter()
            .position(|p| p.as_ref().is_some_and(|p| p.body == body))
            .map(|i| i as PlayerHandle)
    }
    pub fn handles(&self) -> Vec<PlayerHandle> {
        (0..self.players.len() as PlayerHandle)
            .filter(|&h| self.exists(h))
            .collect()
    }

    /// Capsule centre (world).
    pub fn position(&self, physics: &Physics, h: PlayerHandle) -> DVec3 {
        physics.position(self.get(h).body)
    }
    pub fn velocity(&self, physics: &Physics, h: PlayerHandle) -> Vec3 {
        physics.velocity(self.get(h).body).as_vec3()
    }
    pub fn half_height(&self, h: PlayerHandle) -> f32 {
        let p = self.get(h);
        p.cfg.half_height(p.c.crouching)
    }
    /// World height of the feet (a face-B player's feet are above its head).
    pub fn feet(&self, physics: &Physics, h: PlayerHandle) -> f64 {
        self.position(physics, h).y - f64::from(self.get(h).c.face) * f64::from(self.half_height(h))
    }
    pub fn head(&self, physics: &Physics, h: PlayerHandle) -> f64 {
        self.position(physics, h).y + f64::from(self.get(h).c.face) * f64::from(self.half_height(h))
    }

    /// Restores a player's full state (controller + body), e.g. from a snapshot before replaying.
    pub fn restore(
        &mut self,
        physics: &mut Physics,
        h: PlayerHandle,
        state: &PlayerController,
        position: DVec3,
        velocity: Vec3,
    ) {
        let p = self.get_mut(h);
        p.c = *state;
        let crouching = state.crouching;
        Self::set_crouch_shape(physics, p, crouching);
        self.teleport(physics, h, position, velocity);
    }

    /// Moves the body.
    pub fn teleport(
        &mut self,
        physics: &mut Physics,
        h: PlayerHandle,
        position: DVec3,
        velocity: Vec3,
    ) {
        let body = self.get(h).body;
        physics.set_position(body, position);
        physics.set_velocity(body, velocity.as_dvec3());
    }

    /// Instant velocity change (knockback, explosions): absorbed next tick.
    pub fn add_velocity(&mut self, physics: &mut Physics, h: PlayerHandle, delta: Vec3) {
        let body = self.get(h).body;
        let v = physics.velocity(body).as_vec3() + delta;
        physics.set_velocity(body, v.as_dvec3());
    }

    /// Radial push with linear falloff and upward bias (PPC AddExplosion), for players in range.
    pub fn add_explosion(
        &mut self,
        physics: &mut Physics,
        center: DVec3,
        radius: f32,
        speed: f32,
        upward_bias: f32,
    ) {
        for h in self.handles() {
            let offset = (self.position(physics, h) - center).as_vec3();
            let distance = offset.length();
            if distance > radius {
                continue;
            }
            let up = Frame {
                face: self.get(h).c.face,
            }
            .world_up();
            let direction = if distance > 1e-3 {
                offset / distance
            } else {
                up
            };
            let falloff = 1.0 - distance / radius;
            self.add_velocity(
                physics,
                h,
                (direction + up * upward_bias) * (speed * falloff),
            );
        }
    }

    fn set_crouch_shape(physics: &mut Physics, p: &mut Player, crouching: bool) {
        p.c.crouching = crouching;
        if let Some(c) = physics.w.colliders.get_mut(p.collider) {
            c.set_shape(capsule_shape(&p.cfg, crouching));
        }
    }

    // --- the face-local frame -----------------------------------------------------------------

    fn frame(p: &Player) -> Frame {
        Frame { face: p.c.face }
    }
    fn pos(physics: &Physics, p: &Player) -> DVec3 {
        Self::frame(p).p(physics.position(p.body))
    }
    fn vel(physics: &Physics, p: &Player) -> Vec3 {
        Self::frame(p).v(physics.velocity(p.body).as_vec3())
    }
    fn set_pos(physics: &mut Physics, p: &Player, local: DVec3) {
        physics.set_position(p.body, Self::frame(p).p(local));
    }
    fn set_vel(physics: &mut Physics, p: &Player, local: Vec3) {
        physics.set_velocity(p.body, Self::frame(p).v(local).as_dvec3());
    }

    /// Switches the face (mirroring the layers' vertical state) once the body is across.
    fn update_face(physics: &Physics, p: &mut Player) {
        let c = &mut p.c;
        let over = physics.position(p.body).y - f64::from(MIDPLANE_Y);
        let face = if c.face > 0 {
            if over < -FACE_HYSTERESIS { -1 } else { 1 }
        } else if over > FACE_HYSTERESIS {
            1
        } else {
            -1
        };
        if face == c.face {
            return;
        }
        c.vertical.accumulated_y = -c.vertical.accumulated_y;
        c.vertical.platform_y = -c.vertical.platform_y;
        c.vertical.last_platform_y = -c.vertical.last_platform_y;
        c.vertical.target_y = -c.vertical.target_y;
        c.platform.ground_velocity.y = -c.platform.ground_velocity.y;
        c.platform.base_velocity.y = -c.platform.base_velocity.y;
        c.climb.velocity.y = -c.climb.velocity.y;
        c.swim.velocity.y = -c.swim.velocity.y;
        c.fly.velocity.y = -c.fly.velocity.y;
        c.target_velocity.y = -c.target_velocity.y;
        c.climb.ladder.y = mirror_y(c.climb.ladder.y);
        c.climb.released.y = mirror_y(c.climb.released.y);
        c.face = face;
    }

    // --- the pipeline ---------------------------------------------------------------------------

    /// Runs the pipeline (§4) for every player. Call before the physics step.
    pub fn tick(&mut self, world: &mut VoxelWorld, physics: &mut Physics) {
        if let Some(terrain) = &mut self.terrain {
            for p in self.players.iter().flatten() {
                let pos = physics.position(p.body);
                // Capped: fast flight must not ask for kilometres of collision.
                let speed = physics.velocity(p.body).length() as f32;
                let reach = f64::from(3.0 + (speed * DT * 2.0).min(64.0));
                terrain.ensure_box(
                    physics,
                    world,
                    pos - DVec3::splat(reach),
                    pos + DVec3::splat(reach),
                );
            }
            self.ticks += 1;
            if self.ticks.is_multiple_of(RETAIN_EVERY_TICKS) {
                let centers: Vec<_> = self
                    .players
                    .iter()
                    .flatten()
                    .map(|p| {
                        let q = physics.position(p.body);
                        chunk_of(q.x.floor() as i32, q.y.floor() as i32, q.z.floor() as i32)
                    })
                    .collect();
                terrain.retain(physics, |c| {
                    centers
                        .iter()
                        .any(|&k| chunk_distance(c, k) <= RETAIN_CHUNKS)
                });
            }
            terrain.sync(physics, world);
        }
        for p in self.players.iter_mut().flatten() {
            physics.contact_normals(p.body, MAX_CONTACTS, &mut p.contacts);
        }
        // Pass-major, like the PPC's Quantum system group.
        type Pass = fn(&mut Players, usize, &mut VoxelWorld, &mut Physics);
        const PASSES: [Pass; 12] = [
            Players::step_input,
            Players::step_probe,
            Players::step_platform,
            Players::step_fly,
            Players::step_crouch,
            Players::step_jump,
            Players::step_climb,
            Players::step_swim,
            Players::step_horizontal,
            Players::step_vertical,
            Players::step_aggregate,
            Players::step_state,
        ];
        for pass in PASSES {
            for i in 0..self.players.len() {
                if self.players[i].is_some() {
                    pass(self, i, world, physics);
                }
            }
        }
    }

    fn pl(&mut self, i: usize) -> &mut Player {
        self.players[i].as_mut().expect("live")
    }

    // 1 — PPCInputSystem
    fn step_input(&mut self, i: usize, _: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.pl(i);
        p.c.previous_input = p.c.input;
        p.c.input = p.pending;
        Self::update_face(physics, p);
        let c = &mut p.c;
        // A face-B player is upside down: its right is the face-local frame's left.
        if c.face < 0 {
            c.input.move_x = -c.input.move_x;
        }
        let length_sq = c.input.move_x * c.input.move_x + c.input.move_y * c.input.move_y;
        if length_sq > 1.0 {
            let length = length_sq.sqrt();
            c.input.move_x /= length;
            c.input.move_y /= length;
        }
        c.events = 0;
        c.landed_speed = 0.0;
    }

    /// Casts the ring of rays (centre plus `RING_RAYS` around it); the closest hit, or any.
    #[allow(clippy::too_many_arguments)]
    fn ring_cast(
        ring: &[Vec3; RING_RAYS],
        q: &FaceQuery,
        me: BodyHandle,
        center: DVec3,
        dir: Vec3,
        distance: f32,
        radius: f32,
        closest: bool,
    ) -> Option<ProbeHit> {
        Self::ring_cast_centre(
            ring, q, me, center, dir, distance, radius, closest, &mut None,
        )
    }

    /// `ring_cast`, also giving the centre ray's own hit in `centre`.
    #[allow(clippy::too_many_arguments)]
    fn ring_cast_centre(
        ring: &[Vec3; RING_RAYS],
        q: &FaceQuery,
        me: BodyHandle,
        center: DVec3,
        dir: Vec3,
        distance: f32,
        radius: f32,
        closest: bool,
        centre: &mut Option<ProbeHit>,
    ) -> Option<ProbeHit> {
        let end = center + (dir * distance).as_dvec3();
        let pad = DVec3::new(f64::from(radius), 0.0, f64::from(radius));
        // One broad-phase check for the whole ring; usually only the grid is walked.
        let bodies = q.bodies_near(center.min(end) - pad, center.max(end) + pad, Some(me));
        let mut best: Option<ProbeHit> = None;
        for k in 0..=RING_RAYS {
            let origin = if k == 0 {
                center
            } else {
                center + (ring[k - 1] * radius).as_dvec3()
            };
            let Some(hit) = q.cast_ray(origin, dir, distance, Some(me), bodies) else {
                continue;
            };
            if k == 0 {
                *centre = Some(hit);
            }
            if !closest {
                return Some(hit);
            }
            if best.is_none_or(|b| hit.distance < b.distance) {
                best = Some(hit);
            }
        }
        best
    }

    // 2 — PPCProbeSystem
    fn step_probe(&mut self, i: usize, world: &mut VoxelWorld, physics: &mut Physics) {
        let ring = self.ring;
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let center = Self::pos(physics, p);
        let half_height = cfg.half_height(p.c.crouching);
        let ring_radius = cfg.body.radius * cfg.advanced.probe_ring_radius;
        let vq = VoxelQuery::new(world, physics);
        let q = FaceQuery {
            q: &vq,
            f: Self::frame(p),
        };
        let c = &mut p.c;
        let g = &mut c.ground;
        g.was_grounded = g.grounded;
        // While grounded, reach down a whole step so walking down stairs keeps the ground.
        let reach = if g.grounded {
            cfg.probes
                .ground_probe_margin
                .max(cfg.movement.max_step_height)
        } else {
            cfg.probes.ground_probe_margin
        };
        let mut under = None;
        if let Some(hit) = Self::ring_cast_centre(
            &ring,
            &q,
            p.body,
            center,
            -Vec3::Y,
            half_height + reach,
            ring_radius,
            true,
            &mut under,
        ) {
            g.normal = hit.normal;
            g.gap = hit.distance - half_height;
            g.slope_angle = slope_angle(hit.normal);
            // Sliding reads the ground under the centre: the ring's highest hit can lie across a
            // crest, whose face would push the player back onto it.
            let under = under.unwrap_or(hit);
            g.under_normal = under.normal;
            g.under_angle = slope_angle(under.normal);
            g.ground = hit.ground;
            let walkable = g.slope_angle <= cfg.probes.max_slope_angle;
            // Rising faster than the ground below keeps the previous answer until the rise ends.
            let ground_vy = if c.platform.ground == hit.ground {
                c.platform.ground_velocity.y
            } else {
                0.0
            };
            let rising = c.vertical.accumulated_y > ground_vy + RISING_MARGIN;
            g.grounded = walkable && (!rising || g.was_grounded);
        } else {
            g.grounded = false;
            g.normal = Vec3::Y;
            g.slope_angle = 0.0;
            g.under_normal = Vec3::Y;
            g.under_angle = 0.0;
            g.gap = 0.0;
            g.ground = GroundRef::None;
        }
        g.ceiling_blocked = Self::ring_cast(
            &ring,
            &q,
            p.body,
            center,
            Vec3::Y,
            half_height + cfg.probes.ceiling_probe_margin,
            ring_radius,
            false,
        )
        .is_some();
        g.touching_wall = false;
        g.wall_normal = Vec3::ZERO;
        for dir in [Vec3::Z, -Vec3::Z, -Vec3::X, Vec3::X] {
            if let Some(wall) = q.cast_ray(
                center + (dir * ring_radius).as_dvec3(),
                dir,
                cfg.advanced.wall_check_distance,
                Some(p.body),
                true,
            ) && slope_angle(wall.normal) > WALL_MIN_ANGLE
            {
                g.touching_wall = true;
                g.wall_normal = wall.normal;
                break;
            }
        }
    }

    // 3 — PPCPlatformSystem: Tier 1 bodies (and players, if configured) carry by the velocity of
    // the point under the player.
    fn step_platform(&mut self, i: usize, _: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.pl(i);
        let ground = p.c.ground.ground;
        if ground == GroundRef::None {
            p.c.platform = PlatformState::default();
            return;
        }
        p.c.platform.ground = ground;
        let mut velocity = Vec3::ZERO;
        let mut yaw_delta = 0.0f32;
        let carries = matches!(ground, GroundRef::Tier1Body(_))
            || (matches!(ground, GroundRef::Player(_)) && p.cfg.movement.carried_by_characters);
        if carries && let Some(body) = ground.body() {
            let position = physics.position(p.body);
            velocity = Self::frame(p).v(physics.point_velocity(body, position).as_vec3());
            yaw_delta = physics.angular_velocity(body).y as f32 * DT * RAD_TO_DEG;
        }
        let grounded = p.c.ground.grounded;
        let max_yaw = p.cfg.advanced.max_platform_yaw_speed * DT;
        let platform = &mut p.c.platform;
        platform.ground_velocity = velocity;
        platform.base_velocity = if grounded { velocity } else { Vec3::ZERO };
        platform.yaw_delta = if grounded {
            yaw_delta.clamp(-max_yaw, max_yaw)
        } else {
            0.0
        };
    }

    fn fits_standing(q: &FaceQuery, p: &Player, center: DVec3) -> bool {
        // Slightly slimmer than the real capsule so touching the floor or a wall doesn't count.
        let r = p.cfg.body.radius;
        let capsule = Capsule {
            center,
            radius: r - CROUCH_SKIN,
            half_cylinder: (p.cfg.half_height(false) - r - CROUCH_SKIN).max(0.0),
        };
        !q.overlaps_solid(&capsule, Some(p.body))
    }

    // 4 — PPCCrouchSystem
    fn step_crouch(&mut self, i: usize, world: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let position = Self::pos(physics, p);
        let delta = f64::from(cfg.crouch_height_delta());
        // Grounded: grow upwards from the feet. Airborne: grow down to the feet first, else up.
        let stand_at = |world: &mut VoxelWorld, physics: &Physics, p: &Player| {
            let vq = VoxelQuery::new(world, physics);
            let q = FaceQuery {
                q: &vq,
                f: Self::frame(p),
            };
            let up = position + DVec3::new(0.0, delta, 0.0);
            let down = position - DVec3::new(0.0, delta, 0.0);
            if p.c.ground.grounded {
                Self::fits_standing(&q, p, up).then_some(up)
            } else if Self::fits_standing(&q, p, down) {
                Some(down)
            } else {
                Self::fits_standing(&q, p, up).then_some(up)
            }
        };
        let try_stand = |world: &mut VoxelWorld, physics: &mut Physics, p: &mut Player| {
            if let Some(at) = stand_at(world, physics, p) {
                Self::set_pos(physics, p, at);
                Self::set_crouch_shape(physics, p, false);
                p.c.events |= events::CROUCH_CHANGED;
            }
        };
        if p.c.exclusive() {
            if p.c.crouching {
                try_stand(world, physics, p);
            }
            return;
        }
        if p.c.crouch_pressed() && !p.c.crouching {
            // Grounded: keep the feet where they are. Airborne: keep the head, pull the feet up.
            let grounded = p.c.ground.grounded;
            Self::set_pos(
                physics,
                p,
                position + DVec3::new(0.0, if grounded { -delta } else { delta }, 0.0),
            );
            Self::set_crouch_shape(physics, p, true);
            if !grounded && cfg.crouch.mid_air_boost > 0.0 {
                p.c.vertical.accumulated_y += cfg.crouch.mid_air_boost;
            }
            p.c.events |= events::CROUCH_CHANGED;
        } else if !p.c.input.crouch && p.c.crouching {
            try_stand(world, physics, p);
        }
    }

    fn perform_jump(p: &mut Player) {
        let c = &mut p.c;
        // Keep any larger upward velocity already absorbed (e.g. from a knockback).
        c.vertical.accumulated_y = c
            .vertical
            .accumulated_y
            .max(p.cfg.jump_velocity() + c.vertical.platform_y);
        c.ground.grounded = false;
        c.jump.buffer_ticks = 0;
        c.jump.coyote_ticks = 0;
        c.jump.jumping = true;
        c.jump.jumped_this_tick = true;
        c.events |= events::JUMPED;
    }

    /// Auto-jump (Dwell): moving into a one-block obstacle with two free cells above it.
    fn auto_jump_obstacle(q: &FaceQuery, p: &Player, center: DVec3) -> bool {
        let c = &p.c;
        let cfg = &p.cfg;
        let f = flat(move_direction(&c.input));
        if f.length() < INPUT_DEADZONE {
            return false;
        }
        let feet = center.y as f32 - cfg.half_height(c.crouching);
        let probe = center + (safe_normalized(f) * (cfg.body.radius + 0.2)).as_dvec3();
        let (x, z) = (probe.x.floor() as i32, probe.z.floor() as i32);
        let y = (feet + cfg.movement.max_step_height + 0.05).floor() as i32;
        let shape = q.shape(x, y, z);
        let top = y as f32
            + if shape.face_count == 0 {
                0.0
            } else {
                solid_span_at(
                    shape,
                    (probe.x - f64::from(x)) as f32,
                    (probe.z - f64::from(z)) as f32,
                )
                .hi
            };
        if top <= feet + cfg.movement.max_step_height || top > feet + 1.05 {
            return false;
        }
        (1..=2).all(|above| info(q.material(x, y + above, z)).shape == VoxelShape::Empty)
    }

    // 5 — PPCJumpSystem (timers in ticks)
    fn step_jump(&mut self, i: usize, world: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let grounded = p.c.ground.grounded;
        let body_vy = Self::vel(physics, p).y;
        let j = &mut p.c.jump;
        j.jumped_this_tick = false;
        j.buffer_ticks = j.buffer_ticks.saturating_sub(1);
        j.coyote_ticks = j.coyote_ticks.saturating_sub(1);
        if !grounded && p.c.ground.was_grounded && !j.jumping {
            j.coyote_ticks = p.cfg.jump.coyote_ticks; // walked off an edge
        }
        if grounded && j.jumping && body_vy <= 0.0 {
            j.jumping = false;
        }
        if p.c.exclusive() {
            return; // jumping off a ladder / out of water is handled there
        }
        if p.c.jump_pressed() && p.c.jump.buffer_ticks == 0 {
            p.c.jump.buffer_ticks = p.cfg.jump.buffer_ticks;
        }
        if (grounded || p.c.jump.coyote_ticks > 0) && p.c.jump.buffer_ticks > 0 {
            Self::perform_jump(p);
        } else if p.cfg.movement.auto_jump && grounded && !p.c.crouching {
            let center = Self::pos(physics, p);
            let vq = VoxelQuery::new(world, physics);
            let q = FaceQuery {
                q: &vq,
                f: Self::frame(p),
            };
            if Self::auto_jump_obstacle(&q, p, center) {
                Self::perform_jump(p);
            }
        }
    }

    /// A climbable cell the capsule overlaps (and whether it is in the released column).
    fn find_ladder(q: &FaceQuery, p: &Player, center: DVec3) -> (Option<Cell>, bool) {
        let c = &p.c;
        let r = p.cfg.body.radius;
        let capsule = Capsule {
            center,
            radius: r,
            half_cylinder: (p.cfg.half_height(c.crouching) - r).max(0.0),
        };
        let mut found = None;
        let mut in_released = false;
        q.for_each_overlapping_cell(&capsule, |x, y, z, m| {
            if !info(m).climbable {
                return;
            }
            // A contiguous ladder column counts as one ladder for `released`.
            if c.climb.has_released && x == c.climb.released.x && z == c.climb.released.z {
                in_released = true;
                return;
            }
            if found.is_none() {
                found = Some(Cell { x, y, z });
            }
        });
        if found.is_some() || !c.climb.climbing {
            return (found, in_released);
        }
        // The top of a column reaches LADDER_TOP_REACH above its last cell, so the climber gets
        // their feet onto the ledge. It only keeps a climb going.
        let feet = center.y as f32 - p.cfg.half_height(c.crouching);
        let y = (feet - LADDER_TOP_REACH).floor() as i32;
        for z in (center.z - f64::from(r)).floor() as i32..=(center.z + f64::from(r)).floor() as i32
        {
            for x in
                (center.x - f64::from(r)).floor() as i32..=(center.x + f64::from(r)).floor() as i32
            {
                if !info(q.material(x, y, z)).climbable || info(q.material(x, y + 1, z)).climbable {
                    continue;
                }
                let lo = DVec3::new(f64::from(x), -1e6, f64::from(z));
                let hi = DVec3::new(f64::from(x + 1), 1e6, f64::from(z + 1));
                if VoxelQuery::segment_box_distance(&capsule, lo, hi) >= r {
                    continue;
                }
                if c.climb.has_released && x == c.climb.released.x && z == c.climb.released.z {
                    in_released = true;
                    continue;
                }
                return (Some(Cell { x, y, z }), in_released);
            }
        }
        (None, in_released)
    }

    // 6 — PPCClimbSystem: exclusive layer on climbable voxels.
    fn step_climb(&mut self, i: usize, world: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let position = Self::pos(physics, p);
        let vq = VoxelQuery::new(world, physics);
        let q = FaceQuery {
            q: &vq,
            f: Self::frame(p),
        };
        let (found, in_released) = Self::find_ladder(&q, p, position);
        let c = &mut p.c;
        if c.climb.has_released && !in_released {
            c.climb.has_released = false; // left that column
        }
        if c.fly.flying {
            return;
        }
        if !c.climb.climbing {
            let Some(ladder) = found else { return };
            if c.swim.swimming {
                return;
            }
            c.climb.climbing = true;
            c.climb.ladder = ladder;
            c.horizontal.current = Vec3::ZERO;
            c.horizontal.external = Vec3::ZERO;
            c.jump.jumping = false;
            c.events |= events::CLIMB_STARTED;
        }
        let release = |c: &mut PlayerController, jump_off: bool| {
            c.climb.climbing = false;
            c.climb.has_released = true;
            c.climb.released = c.climb.ladder;
            c.climb.velocity = Vec3::ZERO;
            c.vertical.accumulated_y = 0.0;
            if jump_off {
                let l = c.climb.ladder;
                let frame = frame_of(l, q.material(l.x, l.y, l.z));
                let depth = (position - frame.plate).as_vec3().dot(frame.facing);
                let away = if depth >= 0.0 {
                    frame.facing
                } else {
                    -frame.facing
                };
                c.horizontal.current = away * cfg.climb.jump_off_away;
                c.vertical.accumulated_y = cfg.climb.jump_off_up;
                c.ground.grounded = false;
                c.jump.jumped_this_tick = true;
            }
            c.events |= events::CLIMB_ENDED;
        };
        if c.jump_pressed() {
            release(c, true);
            return;
        }
        let Some(ladder) = found.filter(|_| !c.ground.grounded || c.ground.was_grounded) else {
            release(c, false);
            return;
        };
        c.climb.ladder = ladder;
        let material = q.material(ladder.x, ladder.y, ladder.z);
        let speed = cfg.climb.speed * info(material).climb_speed_scale;
        let mut vertical = c.input.move_y;
        if c.input.look_pitch < -cfg.climb.look_down_threshold {
            vertical = -vertical;
        }
        let mut velocity = Vec3::Y * (vertical * speed)
            + camera_right(c.input.look_yaw) * (c.input.move_x * speed);
        // Over the top (Dwell): near the column's top, climbing up also moves onto the ledge.
        let mut top = ladder.y;
        for _ in 0..256 {
            if !info(q.material(ladder.x, top + 1, ladder.z)).climbable {
                break;
            }
            top += 1;
        }
        let feet = position.y as f32 - cfg.half_height(c.crouching);
        let frame = frame_of(ladder, material);
        let over_top = vertical > 0.0 && feet > (top + 1) as f32 - 0.3;
        if over_top {
            velocity -= frame.facing * (vertical * speed);
        }
        // Pull onto the ladder face: a fixed distance in front of the plate along its facing.
        if cfg.climb.snap_strength > 0.0 && !over_top {
            let depth = (position - frame.plate).as_vec3().dot(frame.facing);
            let side = if depth >= 0.0 { 1.0 } else { -1.0 };
            let target = side * (LADDER_PLATE_HALF_DEPTH + cfg.body.radius - LADDER_INSET);
            velocity += frame.facing * ((target - depth) * cfg.climb.snap_strength);
        }
        c.climb.velocity = velocity;
        // Hold the other layers: letting go isn't mistaken for an external force.
        c.horizontal.current = Vec3::ZERO;
        c.horizontal.external = Vec3::ZERO;
        c.horizontal.contribution = flat(velocity);
        c.vertical.accumulated_y = 0.0;
        c.vertical.target_y = velocity.y;
    }

    // 7 — Swim (Dwell, §6.4): exclusive layer in water; also the flip band near the midplane.
    fn step_swim(&mut self, i: usize, world: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let position = Self::pos(physics, p);
        let body_velocity = Self::vel(physics, p);
        {
            let vq = VoxelQuery::new(world, physics);
            let q = FaceQuery {
                q: &vq,
                f: Self::frame(p),
            };
            p.c.swim.submerged = q.submerged_fraction(position, cfg.half_height(p.c.crouching));
        }
        let c = &mut p.c;
        if c.climb.climbing || c.fly.flying {
            return;
        }
        let band_height = dwell_protocol::world::FLIP_BAND as f32;
        let height = (position.y - f64::from(MIDPLANE_Y)).max(0.0) as f32;
        let band = height < band_height && !c.ground.grounded;
        if !c.swim.swimming {
            if c.swim.submerged <= cfg.swim.enter_fraction && !band {
                return;
            }
            c.swim.swimming = true;
            c.jump.jumping = false;
            c.events |= events::SWIM_STARTED;
        } else if c.swim.submerged < cfg.swim.exit_fraction && !band {
            // Hand back to the normal layers with the current velocity.
            c.swim.swimming = false;
            c.swim.velocity = Vec3::ZERO;
            c.horizontal.current = flat(body_velocity);
            c.horizontal.external = Vec3::ZERO;
            c.horizontal.contribution = flat(body_velocity);
            c.vertical.accumulated_y = body_velocity.y;
            c.vertical.target_y = body_velocity.y;
            if c.input.jump {
                // Jump out of the water, e.g. onto the bank.
                c.vertical.accumulated_y = c.vertical.accumulated_y.max(cfg.jump_velocity());
                c.ground.grounded = false;
                c.jump.jumped_this_tick = true;
                c.jump.jumping = true;
                c.events |= events::JUMPED;
            }
            c.events |= events::SWIM_ENDED;
            return;
        }
        // Move in the look direction (pitch included); jump ascends, crouch descends.
        let yaw = c.input.look_yaw * DEG_TO_RAD;
        let pitch = c.input.look_pitch * DEG_TO_RAD;
        let forward = Vec3::new(
            libm::sinf(yaw) * libm::cosf(pitch),
            libm::sinf(pitch),
            libm::cosf(yaw) * libm::cosf(pitch),
        );
        let mut wish = (forward * c.input.move_y + camera_right(c.input.look_yaw) * c.input.move_x)
            * cfg.swim.speed;
        if c.input.jump {
            wish.y = cfg.swim.speed;
        }
        if c.input.crouch {
            wish.y = -cfg.swim.speed;
        }
        // In the band nothing slows the body; the player's own input still steers.
        let dry = c.swim.submerged <= cfg.swim.enter_fraction;
        let steer = !band || !dry || c.input.jump || c.input.crouch;
        if band && dry && !steer {
            wish.y = body_velocity.y;
        }
        let mut velocity =
            body_velocity + (wish - body_velocity) * (1.0 - libm::expf(-cfg.swim.drag * DT));
        if !dry || !band {
            velocity +=
                Vec3::Y * (cfg.swim.buoyancy * (c.swim.submerged - cfg.swim.float_fraction) * DT);
        }
        if band {
            let pull = if body_velocity.y < 0.0 {
                cfg.band.boost_gravities
            } else {
                height / band_height
            };
            velocity -= Vec3::Y * (cfg.body.gravity * pull * DT);
        }
        c.swim.velocity = velocity;
        c.horizontal.current = Vec3::ZERO;
        c.horizontal.external = Vec3::ZERO;
        c.horizontal.contribution = flat(velocity);
        c.vertical.accumulated_y = 0.0;
        c.vertical.target_y = velocity.y;
    }

    // Creative flight (◆ Dwell, §6.7): exclusive while the fly input is held.
    fn step_fly(&mut self, i: usize, _: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let body_velocity = Self::vel(physics, p);
        let position = Self::pos(physics, p);
        let c = &mut p.c;
        if !c.fly.flying {
            if !c.input.fly {
                return;
            }
            c.fly.flying = true;
            c.climb.climbing = false;
            c.swim.swimming = false;
            c.jump.jumping = false;
            c.jump.buffer_ticks = 0;
            c.events |= events::FLY_STARTED;
        } else if !c.input.fly {
            // Hand back to the normal layers with the current velocity: the player falls.
            c.fly.flying = false;
            c.fly.velocity = Vec3::ZERO;
            c.horizontal.current = flat(body_velocity);
            c.horizontal.external = Vec3::ZERO;
            c.horizontal.contribution = flat(body_velocity);
            c.vertical.accumulated_y = body_velocity.y;
            c.vertical.target_y = body_velocity.y;
            c.events |= events::FLY_ENDED;
            return;
        }
        let feet = position.y - f64::from(cfg.half_height(c.crouching));
        let above_sea = (feet - f64::from(SEA_LEVEL)).max(0.0) as f32;
        let base = cfg.fly.speed * if c.input.run { cfg.fly.run_factor } else { 1.0 };
        // The height-based speed, capped in the terrain band, and the slider's true minimum.
        let mut speed = base * (1.0 + above_sea / cfg.fly.boost_height);
        if feet < f64::from(WORLD_MAX_Y) {
            speed = speed.min(cfg.fly.terrain_speed);
        }
        speed = speed.max(base * fly_speed_factor(c.input.fly_speed));
        let mut wish = move_direction(&c.input) * speed;
        wish.y = (f32::from(u8::from(c.input.jump)) - f32::from(u8::from(c.input.crouch))) * speed;
        let mut velocity =
            body_velocity + (wish - body_velocity) * (1.0 - libm::expf(-cfg.fly.drag * DT));
        // Sideways, stop just past the rim; up top, arrive at the ceiling instead of passing it.
        let limit = f64::from(cfg.fly.horizontal_limit);
        let dt = f64::from(DT);
        let hold = |at: f64, v: f32| {
            f64::from(v).clamp(((-limit - at) / dt).min(0.0), ((limit - at) / dt).max(0.0)) as f32
        };
        velocity.x = hold(position.x, velocity.x);
        velocity.z = hold(position.z, velocity.z);
        velocity.y = velocity
            .y
            .min(((f64::from(cfg.fly.ceiling) - feet).max(0.0) / dt) as f32);
        c.fly.velocity = velocity;
        c.horizontal.current = Vec3::ZERO;
        c.horizontal.external = Vec3::ZERO;
        c.horizontal.contribution = flat(velocity);
        c.vertical.accumulated_y = 0.0;
        c.vertical.target_y = velocity.y;
    }

    /// Lifts the player onto a step just ahead (PPC TryStep); returns the forward nudge.
    fn try_step(
        world: &mut VoxelWorld,
        physics: &mut Physics,
        p: &mut Player,
        move_dir: Vec3,
    ) -> Vec3 {
        let cfg = p.cfg;
        let f = flat(move_dir);
        if f.length() < MIN_DIRECTION {
            return Vec3::ZERO;
        }
        let dir = normalized(f);
        let center = Self::pos(physics, p);
        let half_height = cfg.half_height(p.c.crouching);
        let feet = center.y as f32 - half_height;
        let mut origin =
            center + (dir * (cfg.body.radius + cfg.advanced.step_probe_distance)).as_dvec3();
        // Start at centre height, or just above step height when crouched.
        origin.y = f64::from((center.y as f32).max(feet + cfg.movement.max_step_height + 0.05));
        let hit = {
            let vq = VoxelQuery::new(world, physics);
            let q = FaceQuery {
                q: &vq,
                f: Self::frame(p),
            };
            q.cast_ray(
                origin,
                -Vec3::Y,
                origin.y as f32 - feet + half_height,
                Some(p.body),
                true,
            )
        };
        let Some(hit) = hit else { return Vec3::ZERO };
        let step_height = hit.point.y as f32 - feet;
        // Never onto ground that slides (§6.8): stepping would walk the player up a steep face.
        if step_height > MIN_STEP
            && step_height <= cfg.movement.max_step_height
            && slope_angle(hit.normal) <= cfg.probes.max_slope_angle.min(cfg.slide.min_angle)
        {
            // Dwell: also nudge forward so the probe ring is over the step.
            let ring = cfg.body.radius * cfg.advanced.probe_ring_radius;
            let nudge = cfg.body.radius + cfg.advanced.step_probe_distance - ring + 0.01;
            Self::set_pos(
                physics,
                p,
                center + DVec3::new(0.0, f64::from(step_height), 0.0) + (dir * nudge).as_dvec3(),
            );
            p.c.vertical.step_grace = STEP_GRACE_TICKS;
            return dir * nudge;
        }
        Vec3::ZERO
    }

    /// Edge guard (Dwell): crouched on the ground, don't move (per axis) where the ground ring
    /// would lose ground within a step.
    fn apply_edge_guard(
        ring: &[Vec3; RING_RAYS],
        world: &mut VoxelWorld,
        physics: &Physics,
        p: &mut Player,
    ) {
        let cfg = p.cfg;
        let center = Self::pos(physics, p);
        let half_height = cfg.half_height(true);
        let ring_radius = cfg.body.radius * cfg.advanced.probe_ring_radius;
        let reach = half_height + cfg.movement.max_step_height;
        let vq = VoxelQuery::new(world, physics);
        let q = FaceQuery {
            q: &vq,
            f: Self::frame(p),
        };
        for axis in [0usize, 2] {
            let v = p.c.horizontal.contribution[axis];
            if v == 0.0 {
                continue;
            }
            let mut offset = DVec3::ZERO;
            offset[axis] = f64::from(v * DT);
            if Self::ring_cast(
                ring,
                &q,
                p.body,
                center + offset,
                -Vec3::Y,
                reach,
                ring_radius,
                false,
            )
            .is_some()
            {
                continue;
            }
            p.c.horizontal.current[axis] = 0.0;
            p.c.horizontal.contribution[axis] = 0.0;
        }
    }

    // 8 — PPCMovementLayerSystem
    fn step_horizontal(&mut self, i: usize, world: &mut VoxelWorld, physics: &mut Physics) {
        let ring = self.ring;
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let m = cfg.movement;
        let grounded = p.c.ground.grounded;
        p.drive_lift = 0.0;
        if p.c.exclusive() {
            return;
        }
        let frame = Self::frame(p);
        // External forces: whatever moved the body away from what it was driven to last tick.
        let actual = flat(Self::vel(physics, p));
        let h = p.c.horizontal;
        let mut external_delta = actual - h.contribution;
        // Dwell: velocity a contact removed because the player pushed into it is not external.
        let driven = h.contribution + Vec3::new(0.0, p.c.vertical.target_y, 0.0);
        for &n in &p.contacts {
            let normal = frame.v(n.as_vec3());
            // Driving into a contact that faces partly up deflects part of the drive upwards.
            let into_horizontal = -h.contribution.dot(normal);
            if into_horizontal > 0.0 && normal.y > 0.0 {
                p.drive_lift += into_horizontal * normal.y;
            }
            let fl = flat(normal);
            let length = fl.length();
            if length < 0.1 {
                continue; // floors and ceilings: the vertical layer's business
            }
            let fl = fl / length;
            let into = -driven.dot(normal);
            let removed = external_delta.dot(fl);
            if into > 0.0 && removed > 0.0 {
                external_delta -= fl * removed.min(into * length);
            }
        }
        let c = &mut p.c;
        if external_delta.length() > cfg.advanced.external_absorb_threshold {
            c.horizontal.external += external_delta;
        }
        if c.sliding(cfg.slide.min_angle) {
            let speed = if c.crouching {
                cfg.crouch.speed
            } else if c.input.run {
                m.run_speed
            } else {
                m.walk_speed
            };
            Self::slide(c, &cfg, move_direction(&c.input), speed);
            return;
        }
        c.horizontal.external = if grounded {
            move_towards(
                c.horizontal.external,
                Vec3::ZERO,
                m.ground_external_friction * DT,
            )
        } else {
            c.horizontal.external * libm::expf(-m.air_external_drag * DT)
        };
        let input = c.input;
        let move_dir = move_direction(&input);
        let nudge = if grounded && move_dir.length() > MIN_DIRECTION {
            Self::try_step(world, physics, p, move_dir)
        } else {
            Vec3::ZERO
        };
        let c = &mut p.c;
        let speed = if c.crouching {
            cfg.crouch.speed
        } else if input.run {
            m.run_speed
        } else {
            m.walk_speed
        };
        let player_target = move_dir * speed;
        // Accelerate in the platform's frame so standing on a moving body needs no input.
        let base = flat(c.platform.base_velocity);
        let relative = c.horizontal.current - base;
        let relative_delta = player_target - relative;
        let dot = if relative.length() > MIN_DIRECTION && player_target.length() > MIN_DIRECTION {
            normalized(relative).dot(normalized(player_target))
        } else {
            1.0
        };
        let move_length = (input.move_x * input.move_x + input.move_y * input.move_y).sqrt();
        let mut rate = if move_length > INPUT_DEADZONE {
            if dot < REVERSE_DOT {
                m.reverse_deceleration
            } else {
                m.acceleration
            }
        } else {
            m.deceleration
        };
        if !grounded {
            rate *= m.air_control;
        }
        let change = move_towards(Vec3::ZERO, relative_delta, rate * DT);
        c.horizontal.current = relative + change + base;
        c.horizontal.contribution = c.horizontal.current + c.horizontal.external;
        // Dwell: a step-up's nudge is part of this tick's movement: take it out of the velocity.
        let nudge_length = nudge.length();
        if nudge_length > 0.0 {
            let along = nudge / nudge_length;
            let pay = c
                .horizontal
                .contribution
                .dot(along)
                .clamp(0.0, nudge_length / DT);
            c.horizontal.contribution -= along * pay;
        }
        if m.edge_guard && grounded && c.crouching {
            Self::apply_edge_guard(&ring, world, physics, p);
        }
    }

    /// The slide layer (◆ Dwell, §6.8): the horizontal layer on ground steeper than
    /// `slide.min_angle`. The slope pulls the player down under gravity less friction up to the
    /// terminal speed; move input steers across the slope (at `slide.steering` of the walking
    /// acceleration) and, held up the slope, brakes to a lower terminal speed (never to a stop,
    /// never up the slope); momentum carried up the face dies at `uphill_deceleration`. The
    /// vertical layer's ground following keeps the body on the slope, as when walking. Of the
    /// result, the first `speed` (the tick's walking speed) is the player's own `current`, which
    /// carries on as walking where the slope ends; the rest is `external`: on flat ground the
    /// normal ground friction takes it away, in the air nothing does (a jump keeps the momentum).
    /// Speeds and accelerations along the slope are taken to the horizontal plane with `cos θ`.
    fn slide(c: &mut PlayerController, cfg: &PlayerControllerConfig, move_dir: Vec3, speed: f32) {
        let s = cfg.slide;
        let m = cfg.movement;
        let n = c.ground.under_normal;
        let downhill = flat(n);
        let sin = downhill.length();
        let cos = n.y;
        if sin < MIN_DIRECTION {
            return;
        }
        let down = downhill / sin;
        let base = flat(c.platform.base_velocity);
        let relative = c.horizontal.current + c.horizontal.external - base;
        let mut along = relative.dot(down);
        let mut across = relative - down * along;
        let (friction, terminal) = if c.crouching {
            (
                s.crouch_friction,
                s.terminal_speed * s.crouch_terminal_factor,
            )
        } else {
            (s.friction, s.terminal_speed)
        };
        let g = cfg.body.gravity;
        let brake = (-move_dir.dot(down)).clamp(0.0, 1.0);
        let terminal = terminal * (1.0 - s.brake * brake) * cos;
        if along < 0.0 {
            // Moving up the face: friction and the slope both pull down it.
            let decel = (s.uphill_deceleration + g * (sin + friction * cos)) * cos;
            along = (along + decel * DT).min(0.0);
        } else if along <= terminal {
            let pull = (g * (sin - friction * cos)).max(0.0) * cos;
            along = (along + pull * DT).min(terminal);
        } else {
            along = (along - s.brake_deceleration * cos * DT).max(terminal);
        }
        let side_wish = (move_dir - down * move_dir.dot(down)) * s.steer_speed;
        let rate = if move_dir.length() > INPUT_DEADZONE {
            m.acceleration
        } else {
            m.deceleration
        } * s.steering;
        across = move_towards(across, side_wish, rate * DT);
        let total = down * along + across;
        let own = clamp_length(total, speed);
        c.horizontal.current = own + base;
        c.horizontal.external = total - own;
        c.horizontal.contribution = c.horizontal.current + c.horizontal.external;
    }

    // 9 — PPCVerticalLayerSystem
    fn step_vertical(&mut self, i: usize, _: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let cfg = p.cfg;
        let gravity = -cfg.body.gravity;
        let body_y = Self::vel(physics, p).y;
        let drive_lift = p.drive_lift;
        let c = &mut p.c;
        let v = &mut c.vertical;
        v.step_grace = v.step_grace.saturating_sub(1);
        if c.climb.climbing || c.swim.swimming || c.fly.flying {
            return;
        }
        v.last_platform_y = v.platform_y;
        v.platform_y = c.platform.base_velocity.y;
        let was_grounded = c.ground.was_grounded;
        if was_grounded && !c.ground.grounded && !c.jump.jumped_this_tick {
            v.accumulated_y = v.last_platform_y; // walk-off: keep the platform's vertical motion
        }
        if c.ground.ceiling_blocked && v.accumulated_y > 0.0 {
            v.accumulated_y = 0.0;
        }
        if c.ground.grounded {
            // A significant upward deviation while grounded is a launch, unless the ground explains
            // it, or driving into a block's top edge does (Rapier: the solver rolls the capsule
            // over the edge of a step just climbed with an upward velocity, which Jolt didn't).
            let explained = v.target_y.max(c.platform.ground_velocity.y);
            let threshold = if v.step_grace > 0 {
                STEP_LAUNCH_THRESHOLD
            } else {
                LAUNCH_THRESHOLD
            };
            if body_y - drive_lift - explained > threshold {
                v.accumulated_y = body_y + gravity * DT;
                c.ground.grounded = false;
            } else {
                // Follow the ground along slopes, then close any gap within one tick.
                let n = c.ground.normal;
                let horizontal = c.horizontal.contribution - flat(c.platform.base_velocity);
                let along_slope = if n.y > 0.1 {
                    -(n.x * horizontal.x + n.z * horizontal.z) / n.y
                } else {
                    0.0
                };
                v.accumulated_y = v.platform_y + along_slope - c.ground.gap.max(0.0) / DT;
            }
        } else {
            // Airborne: absorb external vertical forces (not the lift from driving into a block's
            // top edge), then integrate gravity.
            let mut external = body_y - v.target_y;
            if external > 0.0 {
                external -= external.min(drive_lift);
            }
            if external.abs() > ABSORB_THRESHOLD && !c.jump.jumped_this_tick {
                v.accumulated_y += external;
            }
            v.accumulated_y += gravity * DT;
        }
        if !was_grounded && c.ground.grounded {
            c.events |= events::LANDED;
            c.landed_speed = (-v.target_y).max(0.0);
        }
        v.target_y = v.accumulated_y;
    }

    // 10 — PPCAggregateSystem
    fn step_aggregate(&mut self, i: usize, _: &mut VoxelWorld, physics: &mut Physics) {
        let p = self.players[i].as_mut().expect("live");
        let c = &mut p.c;
        let target = if c.climb.climbing {
            c.climb.velocity
        } else if c.swim.swimming {
            c.swim.velocity
        } else if c.fly.flying {
            c.fly.velocity
        } else {
            c.horizontal.contribution + Vec3::new(0.0, c.vertical.target_y, 0.0)
        };
        c.target_velocity = target;
        Self::set_vel(physics, p, target);
    }

    // 11 — PPCStateSystem
    fn step_state(&mut self, i: usize, _: &mut VoxelWorld, _: &mut Physics) {
        let p = self.pl(i);
        let min_angle = p.cfg.slide.min_angle;
        let c = &mut p.c;
        let v = c.target_velocity;
        c.state = if c.climb.climbing {
            State::Climbing
        } else if c.swim.swimming {
            State::Swimming
        } else if c.fly.flying {
            State::Flying
        } else if c.sliding(min_angle) {
            State::Sliding
        } else if c.crouching {
            State::Crouching
        } else if !c.ground.grounded {
            if v.y > 0.0 || c.jump.jumping {
                State::Jumping
            } else {
                State::Falling
            }
        } else if flat(v).length() > MOVING_SPEED {
            if c.input.run {
                State::Running
            } else {
                State::Walking
            }
        } else {
            State::Idle
        };
    }
}
