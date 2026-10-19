//! `PlayerTestWorld` (PLAYER_CONTROLLER.md §10): a headless world for controller tests, the voxel
//! counterpart of the PPC's test world — voxel primitives, Tier 1 boxes as platforms, scheduled
//! kicks and explosions, per-player event counters. Everything is in the test's local frame, which
//! sits at the test origin: local y 0 is sea level ([`ORIGIN_Y`], the flat worlds' surface, 32 m
//! above the midplane), `DWELL_ORIGIN_X=far` puts it ~49 km east, near the rim (ADR 0025), and
//! `DWELL_FACE=b` on face B's mirror image (ADR 0023); the suites must pass unchanged in each.

#![allow(dead_code)]

use std::sync::Arc;

use dwell_blocks::{StateId, mirror, require};
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::*;
use dwell_physics::*;
use dwell_world::*;

#[allow(unused_imports)]
pub use dwell_physics::controller::State;

/// A far origin inside the disc and the build sphere: 49,152 m east, 848 m from the rim (Phase 16:
/// the 50 km world).
pub const FAR_ORIGIN_X: i32 = 24 * 2048;
/// The local frame's y = 0 in world y on face A: sea level, where the flight speeds' heights count
/// from and the flat worlds' surface lies; the midplane is this far below.
pub const ORIGIN_Y: i32 = dwell_world::SEA_LEVEL;
const _: () = assert!(ORIGIN_Y == dwell_world::FLAT_TOP_Y && ORIGIN_Y % CHUNK_SIZE == 0);

pub fn origin_x() -> i32 {
    match std::env::var("DWELL_ORIGIN_X").as_deref() {
        Ok("far") => FAR_ORIGIN_X,
        Ok(v) => v.parse().unwrap_or(0),
        Err(_) => 0,
    }
}
pub fn face_b() -> bool {
    std::env::var("DWELL_FACE").as_deref() == Ok("b")
}
pub const MIRROR: f64 = MIRROR_SUM as f64 + 1.0;

pub fn to_world(l: Vec3) -> DVec3 {
    let y = f64::from(l.y) + f64::from(ORIGIN_Y);
    DVec3::new(
        f64::from(l.x) + f64::from(origin_x()),
        if face_b() { MIRROR - y } else { y },
        f64::from(l.z),
    )
}
pub fn to_local(w: DVec3) -> Vec3 {
    Vec3::new(
        (w.x - f64::from(origin_x())) as f32,
        to_local_y(w.y) as f32,
        w.z as f32,
    )
}
pub fn to_local_y(y: f64) -> f64 {
    (if face_b() { MIRROR - y } else { y }) - f64::from(ORIGIN_Y)
}
/// Velocities and directions (the same map both ways).
pub fn mirror_vec(v: Vec3) -> Vec3 {
    if face_b() {
        Vec3::new(v.x, -v.y, v.z)
    } else {
        v
    }
}
pub fn world_cell_x(x: i32) -> i32 {
    x + origin_x()
}
pub fn world_cell_y(y: i32) -> i32 {
    let y = y + ORIGIN_Y;
    if face_b() { mirror_y(y) } else { y }
}
pub fn world_material(m: StateId) -> StateId {
    if face_b() { mirror(m) } else { m }
}
/// The mirror image of a rotation about the midplane: yaw kept, pitch and roll reversed.
pub fn mirror_rotation(q: DQuat) -> DQuat {
    if face_b() {
        DQuat::from_xyzw(-q.x, q.y, -q.z, q.w)
    } else {
        q
    }
}

/// A generator whose output is moved to the test origin (and, on face B, flipped). Its rows are
/// face A's world rows: the flat world's surface (`FLAT_TOP_Y`) is local y 0.
pub fn shifted(generator: ChunkGenerator) -> ChunkGenerator {
    if origin_x() == 0 && !face_b() {
        return generator;
    }
    let cx = origin_x() / CHUNK_SIZE;
    Arc::new(move |c, chunk: &mut Chunk| {
        if !face_b() {
            generator(ChunkCoord::new(c.x - cx, c.y, c.z), chunk);
            return;
        }
        let mut local = Chunk::default();
        generator(
            ChunkCoord::new(c.x - cx, mirror_chunk_y(c.y), c.z),
            &mut local,
        );
        let dst = chunk.voxels_mut();
        for z in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    dst[local_index(x, CHUNK_SIZE - 1 - y, z)] = mirror(local.get(x, y, z));
                }
            }
        }
    })
}

pub fn ticks(seconds: f32) -> i32 {
    (seconds * 60.0).round() as i32
}

pub fn mv(x: f32, y: f32) -> Input {
    Input {
        move_x: x,
        move_y: y,
        ..Input::default()
    }
}

/// Input with every field: move, run, jump, crouch, yaw, pitch.
#[allow(clippy::too_many_arguments)]
pub fn input(x: f32, y: f32, run: bool, jump: bool, crouch: bool, yaw: f32, pitch: f32) -> Input {
    Input {
        move_x: x,
        move_y: y,
        run,
        jump,
        crouch,
        look_yaw: yaw,
        look_pitch: pitch,
        ..Input::default()
    }
}

/// Euler angles (degrees) as the C++ tests composed them: yaw about Y, then pitch X, then roll Z.
pub fn euler(pitch: f64, yaw: f64, roll: f64) -> DQuat {
    let k = std::f64::consts::PI / 180.0;
    DQuat::from_axis_angle(DVec3::Y, yaw * k)
        * DQuat::from_axis_angle(DVec3::X, pitch * k)
        * DQuat::from_axis_angle(DVec3::Z, roll * k)
}

/// The block the controller suites build geometry from: a full cube that is not adaptive, so it
/// keeps its shape whatever its neighbours (the natural blocks shape themselves, ADAPTIVE_BLOCKS.md;
/// the adaptive scenarios use them).
pub fn cube_block() -> StateId {
    require("dwell:sandbox_floor")
}
pub fn air() -> StateId {
    require("dwell:air")
}

fn all_air() -> AirChunkTest {
    Arc::new(|_| true)
}

pub type InputFn = Box<dyn FnMut(i32, PlayerHandle) -> Input>;

pub struct PlayerTestWorld {
    pub config: PlayerControllerConfig,
    pub world: VoxelWorld,
    pub physics: Physics,
    pub players: Players,
    pub input: Option<InputFn>,
    pub tick: i32,
    kicks: Vec<(i32, PlayerHandle, Vec3)>,
    explosions: Vec<(i32, Vec3, f32, f32, f32)>,
    counts: Vec<[i32; 32]>,
    /// Geometry and input in world coordinates, ignoring `DWELL_ORIGIN_X` and `DWELL_FACE` (for
    /// suites about the faces themselves).
    world_frame: bool,
}

impl PlayerTestWorld {
    pub fn new() -> Self {
        Self::with_config(PlayerControllerConfig::default())
    }
    /// An empty world: untouched chunks read as air without being generated and stored, so fast
    /// flight doesn't fill memory with empty chunks.
    pub fn with_config(config: PlayerControllerConfig) -> Self {
        Self::with_air(config, empty_generator(), Some(all_air()))
    }
    pub fn with(config: PlayerControllerConfig, generator: ChunkGenerator) -> Self {
        Self::with_air(config, generator, None)
    }
    fn with_air(
        config: PlayerControllerConfig,
        generator: ChunkGenerator,
        air: Option<AirChunkTest>,
    ) -> Self {
        let mut physics = Physics::default();
        let players = Players::new(&mut physics, true);
        Self {
            config,
            world: VoxelWorld::new(shifted(generator), air),
            physics,
            players,
            input: None,
            tick: 0,
            kicks: Vec::new(),
            explosions: Vec::new(),
            counts: Vec::new(),
            world_frame: false,
        }
    }
    /// A world in world coordinates: use `world.set_voxel` and `spawn_at` directly.
    pub fn world_frame() -> Self {
        let mut physics = Physics::default();
        let players = Players::new(&mut physics, true);
        Self {
            config: PlayerControllerConfig::default(),
            world: VoxelWorld::new(empty_generator(), Some(all_air())),
            physics,
            players,
            input: None,
            tick: 0,
            kicks: Vec::new(),
            explosions: Vec::new(),
            counts: Vec::new(),
            world_frame: true,
        }
    }

    // --- voxel primitives (inclusive cell ranges, local frame) ---
    #[allow(clippy::too_many_arguments)]
    pub fn fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, m: StateId) {
        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.set(x, y, z, m);
                }
            }
        }
    }
    /// A one-cell-thick floor whose top face is at `top_y`.
    pub fn floor_at(&mut self, top_y: i32, half: i32) {
        self.fill(
            -half,
            top_y - 1,
            -half,
            half - 1,
            top_y - 1,
            half - 1,
            cube_block(),
        );
    }
    pub fn floor(&mut self) {
        self.floor_at(0, 16);
    }
    pub fn set(&mut self, x: i32, y: i32, z: i32, m: StateId) {
        self.world
            .set_voxel(world_cell_x(x), world_cell_y(y), z, world_material(m));
    }
    pub fn remove(&mut self, x: i32, y: i32, z: i32) {
        self.set(x, y, z, air());
    }
    /// A 0.5 m step over cells x0..x1, z0..z1 with its floor at y: a static box.
    pub fn half_step(&mut self, x0: i32, y: i32, z0: i32, x1: i32, z1: i32) -> BodyHandle {
        self.boxed(
            Vec3::new(
                0.5 * (x0 + x1 + 1) as f32,
                y as f32 + 0.25,
                0.5 * (z0 + z1 + 1) as f32,
            ),
            Vec3::new(0.5 * (x1 - x0 + 1) as f32, 0.25, 0.5 * (z1 - z0 + 1) as f32),
            DQuat::IDENTITY,
            Vec3::ZERO,
            0.0,
        )
    }

    // --- bodies (Tier 1 layer) ---
    /// A kinematic box moving at `velocity`, turning at `yaw_rate` °/s.
    pub fn boxed(
        &mut self,
        center: Vec3,
        half: Vec3,
        rotation: DQuat,
        velocity: Vec3,
        yaw_rate: f32,
    ) -> BodyHandle {
        let mut b = BoxBody::new(Motion::Kinematic, to_world(center), half.as_dvec3());
        b.rotation = mirror_rotation(rotation);
        b.velocity = mirror_vec(velocity).as_dvec3();
        b.angular_velocity = DVec3::new(0.0, f64::from(yaw_rate).to_radians(), 0.0);
        self.physics.add_box(b)
    }
    pub fn static_box(&mut self, center: Vec3, half: Vec3) -> BodyHandle {
        self.boxed(center, half, DQuat::IDENTITY, Vec3::ZERO, 0.0)
    }
    pub fn dynamic_box(
        &mut self,
        center: Vec3,
        half: Vec3,
        mass: f32,
        velocity: Vec3,
        lock_rotation: bool,
    ) -> BodyHandle {
        let mut b = BoxBody::new(Motion::Dynamic, to_world(center), half.as_dvec3());
        b.mass = Some(f64::from(mass));
        b.gravity = false;
        b.velocity = mirror_vec(velocity).as_dvec3();
        b.lock_rotation = lock_rotation;
        self.physics.add_box(b)
    }
    pub fn body_position(&self, h: BodyHandle) -> Vec3 {
        to_local(self.physics.position(h))
    }

    pub fn spawn(&mut self, feet: Vec3) -> PlayerHandle {
        self.spawn_yaw(feet, 0.0)
    }
    pub fn spawn_yaw(&mut self, feet: Vec3, yaw: f32) -> PlayerHandle {
        let h = self
            .players
            .spawn(&mut self.physics, self.config, to_world(feet), yaw);
        self.counts
            .resize((h as usize + 1).max(self.counts.len()), [0; 32]);
        h
    }
    /// Spawns at a world position (feet), untranslated.
    pub fn spawn_at(&mut self, feet: DVec3, yaw: f32) -> PlayerHandle {
        let h = self
            .players
            .spawn(&mut self.physics, self.config, feet, yaw);
        self.counts
            .resize((h as usize + 1).max(self.counts.len()), [0; 32]);
        h
    }

    pub fn kick(&mut self, at: i32, h: PlayerHandle, dv: Vec3) {
        self.kicks.push((at, h, mirror_vec(dv)));
    }
    pub fn explosion(&mut self, at: i32, center: Vec3, radius: f32, speed: f32, bias: f32) {
        self.explosions.push((at, center, radius, speed, bias));
    }

    /// On face B a right turn is the other way round in the world: local input is sent mirrored.
    pub fn input_for(&self, mut i: Input) -> Input {
        if face_b() && !self.world_frame {
            i.move_x = -i.move_x;
        }
        i
    }

    pub fn set_input(&mut self, f: impl FnMut(i32, PlayerHandle) -> Input + 'static) {
        self.input = Some(Box::new(f));
    }

    pub fn step(&mut self, n: i32) {
        for _ in 0..n {
            let handles = self.players.handles();
            if let Some(mut f) = self.input.take() {
                for &h in &handles {
                    let i = self.input_for(f(self.tick, h));
                    self.players.set_input(h, i);
                }
                self.input = Some(f);
            }
            self.players.tick(&mut self.world, &mut self.physics);
            for &h in &handles {
                let events = self.players.controller(h).events;
                for bit in 0..32 {
                    if events & (1 << bit) != 0 {
                        self.counts[h as usize][bit] += 1;
                    }
                }
            }
            for &(t, h, dv) in &self.kicks {
                if t == self.tick {
                    self.players.add_velocity(&mut self.physics, h, dv);
                }
            }
            for &(t, c, r, s, b) in &self.explosions {
                if t == self.tick {
                    self.players
                        .add_explosion(&mut self.physics, to_world(c), r, s, b);
                }
            }
            self.physics.step(1.0 / 60.0);
            self.tick += 1;
        }
    }

    /// Times `event` (an `events::` bit) was raised for `h`.
    pub fn count(&self, h: PlayerHandle, event: u32) -> i32 {
        self.counts[h as usize][event.trailing_zeros() as usize]
    }
    pub fn c(&self, h: PlayerHandle) -> &PlayerController {
        self.players.controller(h)
    }
    pub fn pos(&self, h: PlayerHandle) -> Vec3 {
        to_local(self.players.position(&self.physics, h))
    }
    pub fn vel(&self, h: PlayerHandle) -> Vec3 {
        mirror_vec(self.players.velocity(&self.physics, h))
    }
    pub fn feet(&self, h: PlayerHandle) -> f32 {
        to_local_y(self.players.feet(&self.physics, h)) as f32
    }
    pub fn head(&self, h: PlayerHandle) -> f32 {
        to_local_y(self.players.head(&self.physics, h)) as f32
    }
    pub fn horizontal_speed(&self, h: PlayerHandle) -> f32 {
        let v = self.vel(h);
        (v.x * v.x + v.z * v.z).sqrt()
    }
    /// Highest feet height reached over `n` ticks.
    pub fn max_feet(&mut self, h: PlayerHandle, n: i32) -> f32 {
        let mut best = f32::MIN;
        for _ in 0..n {
            self.step(1);
            best = best.max(self.feet(h));
        }
        best
    }
}

impl Default for PlayerTestWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// `|a − b| ≤ eps × max(|a|, |b|)` or within `abs`, like doctest's Approx.
pub fn approx(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps * (1.0 + a.abs().max(b.abs()))
}
