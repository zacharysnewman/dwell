//! Client-side prediction and reconciliation for the local player (PLAYER_CONTROLLER.md §8). Runs
//! the same controller as the server against a small prediction world: the client's terrain, the
//! local player as the only dynamic body, and remote players as kinematic capsules.

use dwell_world::hash::HashMap;

use dwell_protocol::{
    GroundKind, InputFrame, PhysicsSnapshot, controller_flags, player_flags,
    players::RECONCILE_SNAP_DISTANCE,
};
use dwell_world::{FaceFrame, VoxelWorld};
use rapier3d_f64::prelude::*;

use crate::config::PlayerControllerConfig;
use crate::controller::{PlayerController, PlayerHandle, Players};
use crate::net::{dequantize_input, face_of_flags, from_net, to_net};
use crate::physics::{BodyHandle, DVec3, Layer, Physics, Vec3, groups};
use crate::query::GroundRef;

const DT: f32 = PlayerControllerConfig::dt();
/// Knockback corrections are smoothed up to this multiple of the snap distance: they are expected
/// to be large (the client learns of them about one RTT late) and must not pop.
const KNOCKBACK_SNAP_FACTOR: f32 = 4.0;
const HISTORY: u32 = 256; // ticks (> 4 s)
const MAX_DEAD_RECKONING: f32 = 0.5; // s

/// A piece is stood in for in the prediction only while it could touch the local player soon
/// (docs/FELLING.md §3): within its cells' reach plus `STAND_IN_M`, plus how far it closes on the
/// player in `STAND_IN_LOOKAHEAD_S` at their relative speed; once stood in, until it is
/// `STAND_IN_KEEP_M` past that. The rest are drawn from the snapshots alone: a blast's ~300 pieces
/// stood in for cost the browser's frame 20–25 ms of prediction (2026-11-06).
pub const STAND_IN_M: f64 = 4.0;
pub const STAND_IN_KEEP_M: f64 = 2.0;
pub const STAND_IN_LOOKAHEAD_S: f64 = 0.5;

/// Whether a piece (its cells' reach `extent` from its origin, at `position` moving at `velocity`)
/// is stood in for, by the player at `me` moving at `mine`; `kept`: it is now.
pub fn stands_in(
    extent: f64,
    position: DVec3,
    velocity: DVec3,
    me: DVec3,
    mine: DVec3,
    kept: bool,
) -> bool {
    let off = me - position;
    let d = off.length();
    let closing = if d > 1e-6 {
        (velocity - mine).dot(off / d).max(0.0)
    } else {
        0.0
    };
    let keep = if kept { STAND_IN_KEEP_M } else { 0.0 };
    d < extent + STAND_IN_M + keep + closing * STAND_IN_LOOKAHEAD_S
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PredictorStats {
    pub ticks: u32,
    /// Snapshots compared against history.
    pub snapshots: u32,
    /// Snapshots that needed a replay.
    pub replays: u32,
    /// Corrections above RECONCILE_SNAP_DISTANCE.
    pub snaps: u32,
    pub knockback_replays: u32,
    /// History lost: restarted from the server state.
    pub resets: u32,
    /// |present before − present after| of the last replay (m).
    pub last_correction: f32,
    /// Position error at ack of the last compared snapshot (m).
    pub last_error: f32,
    pub last_replay_ticks: u32,
}

#[derive(Clone, Copy, Default)]
struct Entry {
    seq: u32, // 0 = empty
    input: InputFrame,
    controller: PlayerController,
    position: DVec3,
    velocity: Vec3,
    /// The player touched a remote player's proxy this tick.
    touched: bool,
}

/// A falling piece's stand-in in the prediction (docs/FELLING.md §3): at the server's pose as of
/// the input `seq` its snapshot acknowledged, carried on at its velocity to each tick predicted,
/// and moved by the local player's own pushes since (docs/BUILT_BODIES.md §6).
struct PieceProxy {
    body: BodyHandle,
    position: DVec3,
    rotation: crate::DQuat,
    velocity: Vec3,
    seq: u32,
    /// The local player's pushes after `seq`: the tick's input seq and the velocity given (world).
    pushes: Vec<(u32, DVec3)>,
    /// What slows it as the server's body is slowed, as of the last tick predicted: the water
    /// (`Predictor::set_water_drag`) and, unpushed, its friction on the ground (m/s²).
    water: WaterDrag,
    friction: f64,
}

/// How water slows a body sideways (the sim's `felling::water_drag`, docs/BUILT_BODIES.md §6),
/// for the prediction's stand-ins: a speed-squared drag of `kx`/`kz` (1/m) moving along x/z
/// (`k · speed²` m/s², blended by the direction) and a linear `linear` (/s), relative to the
/// water's `current` (m/s along world x and z, docs/WATER_SURFACE_GRID.md §2c): a body drifts with
/// it. In a waterfall's sheet (`fall`, on face `face`; docs/WATER_SURFACE_GRID.md §2e) it is also
/// pushed down and out, and can't rise.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterDrag {
    pub kx: f64,
    pub kz: f64,
    pub linear: f64,
    pub current: [f64; 2],
    pub fall: Option<dwell_world::falls::FallPush>,
    pub face: dwell_world::FaceFrame,
}

/// Where a body is in water and how that slows it (the sim's, which `dwell-physics` can't see);
/// the waterfalls' reads kept in the memo across one tick's stand-ins (the world unchanged).
pub type WaterDragFn =
    fn(&mut VoxelWorld, &Physics, BodyHandle, &mut dwell_world::falls::FallMemo) -> WaterDrag;

impl WaterDrag {
    /// A velocity (world) after a tick of it: sideways slowed toward the current, and a fall's push.
    pub fn slow(self, v: DVec3, dt: f64) -> DVec3 {
        let [cx, cz] = self.current;
        let (rx, rz) = (v.x - cx, v.z - cz);
        let speed = rx.hypot(rz);
        let v = if speed < 1e-9 {
            v
        } else {
            let k = (rx.abs() * self.kx + rz.abs() * self.kz) / speed;
            let keep = (-self.linear * dt).exp() / (1.0 + k * speed * dt);
            DVec3::new(cx + rx * keep, v.y, cz + rz * keep)
        };
        match self.fall {
            Some(push) => {
                let f = self.face;
                let local = f.vector(v.to_array());
                DVec3::from_array(f.vector(dwell_world::falls::push_velocity(local, push, dt)))
            }
            None => v,
        }
    }
}

impl PieceProxy {
    /// Where it is after the tick of input `seq`: the server's pose carried on tick by tick, plus
    /// the pushes, slowed as the server slows it (the water, and the ground while unpushed).
    fn at(&self, seq: u32) -> DVec3 {
        let cap = (MAX_DEAD_RECKONING / DT) as u32;
        let dt = f64::from(DT);
        let mut p = self.position;
        let mut v = self.velocity.as_dvec3();
        for s in self.seq + 1..=seq.min(self.seq + cap) {
            let mut pushed = false;
            for &(at, dv) in &self.pushes {
                if at == s {
                    v += dv;
                    pushed = true;
                }
            }
            if !pushed && self.friction > 0.0 {
                let side = DVec3::new(v.x, 0.0, v.z);
                let speed = side.length();
                let keep = if speed > 1e-9 {
                    (speed - self.friction * dt).max(0.0) / speed
                } else {
                    0.0
                };
                v = DVec3::new(v.x * keep, v.y, v.z * keep);
            }
            v = self.water.slow(v, dt);
            p += v * dt;
        }
        p
    }
}

struct Remote {
    body: BodyHandle,
    target: DVec3,
    velocity: Vec3,
    dead_reckoning_ticks: i32,
    crouched: bool,
}

pub struct Predictor {
    config: PlayerControllerConfig,
    physics: Physics,
    players: Players,
    handle: Option<PlayerHandle>,
    history: Vec<Entry>,
    latest_seq: u32,
    last_ack: u32,
    /// The latest knockback seen (event or snapshot).
    last_knockback_seq: u32,
    knockbacks: HashMap<u32, Vec3>,
    remotes: HashMap<u16, Remote>,
    /// The falling pieces' stand-ins (docs/FELLING.md §3).
    pieces: HashMap<u32, PieceProxy>,
    water_drag: Option<WaterDragFn>,
    offset: Vec3,
    stats: PredictorStats,
}

impl Predictor {
    /// Tolerances for "prediction matched the server" (PLAYER_CONTROLLER.md §8.2).
    pub const POSITION_TOLERANCE: f32 = 0.01; // m
    pub const VELOCITY_TOLERANCE: f32 = 0.05; // m/s
    /// Time constant of the visual correction smoothing.
    pub const SMOOTHING_SECONDS: f32 = 0.1;

    pub fn new(config: PlayerControllerConfig) -> Self {
        let mut physics = Physics::default();
        let players = Players::new(&mut physics, true);
        Self {
            config,
            physics,
            players,
            handle: None,
            history: vec![Entry::default(); HISTORY as usize],
            latest_seq: 0,
            last_ack: 0,
            last_knockback_seq: 0,
            knockbacks: HashMap::default(),
            remotes: HashMap::default(),
            pieces: HashMap::default(),
            water_drag: None,
            offset: Vec3::ZERO,
            stats: PredictorStats::default(),
        }
    }

    /// True once a snapshot has placed the local player (and while alive).
    pub fn active(&self) -> bool {
        self.handle.is_some()
    }
    /// Sequence number the next `tick` must carry.
    pub fn next_seq(&self) -> u32 {
        self.latest_seq + 1
    }
    /// Inputs predicted that no snapshot has acknowledged yet.
    pub fn unacked(&self) -> u32 {
        self.latest_seq.saturating_sub(self.last_ack)
    }
    /// Predicted capsule centre.
    pub fn position(&self) -> DVec3 {
        self.handle
            .map_or(DVec3::ZERO, |h| self.players.position(&self.physics, h))
    }
    pub fn velocity(&self) -> Vec3 {
        self.handle
            .map_or(Vec3::ZERO, |h| self.players.velocity(&self.physics, h))
    }
    /// Visual correction still being smoothed out: render at `position() + render_offset()`.
    pub fn render_offset(&self) -> Vec3 {
        self.offset
    }
    pub fn controller(&self) -> PlayerController {
        self.handle
            .map_or_else(PlayerController::default, |h| *self.players.controller(h))
    }
    pub fn half_height(&self) -> f32 {
        self.handle.map_or(self.config.half_height(false), |h| {
            self.players.half_height(h)
        })
    }
    pub fn stats(&self) -> &PredictorStats {
        &self.stats
    }
    pub fn config(&self) -> &PlayerControllerConfig {
        &self.config
    }
    pub fn physics(&mut self) -> &mut Physics {
        &mut self.physics
    }

    fn ground_from_net(remotes: &HashMap<u16, Remote>, kind: GroundKind, id: u16) -> GroundRef {
        match kind {
            GroundKind::Terrain => GroundRef::Terrain,
            GroundKind::Player => remotes
                .get(&id)
                .map_or(GroundRef::None, |r| GroundRef::Player(r.body)),
            _ => GroundRef::None, // Tier 1 bodies arrive in Phase 20
        }
    }

    fn simulate(&mut self, world: &mut VoxelWorld, input: &InputFrame, forward: bool) {
        let h = self.handle.expect("active");
        if forward {
            let max_ticks = (MAX_DEAD_RECKONING / DT) as i32;
            for r in self.remotes.values_mut() {
                if r.dead_reckoning_ticks < max_ticks {
                    r.target += (r.velocity * DT).as_dvec3();
                }
                r.dead_reckoning_ticks += 1;
                self.physics.move_kinematic(r.body, r.target, f64::from(DT));
            }
        }
        self.players.set_input(h, dequantize_input(input));
        self.players.tick(world, &mut self.physics);
        // The stand-ins, replays too: where each is after this tick, this tick's push included.
        let seq = input.seq;
        let mut falls = dwell_world::falls::FallMemo::default();
        for p in self.pieces.values_mut() {
            p.pushes.retain(|&(s, _)| s != seq);
            p.pushes.extend(
                self.players
                    .pushes()
                    .iter()
                    .filter(|&&(b, _)| b == p.body)
                    .map(|&(_, dv)| (seq, dv)),
            );
            if p.pushes.len() > HISTORY as usize {
                p.pushes.remove(0);
            }
            if let Some(water) = self.water_drag {
                p.water = water(world, &self.physics, p.body, &mut falls);
            }
            let frame = crate::frame::Frame {
                face: FaceFrame::at_height(self.physics.position(p.body).y),
            };
            p.friction = crate::controller::ground_friction(world, &self.physics, frame, p.body)
                * self.physics.gravity();
            self.physics
                .set_kinematic_pose(p.body, p.at(seq), p.rotation);
        }
        if let Some(&dv) = self.knockbacks.get(&input.seq) {
            self.players.add_velocity(&mut self.physics, h, dv);
        }
        self.physics.step(f64::from(DT));
        if forward {
            for r in self.remotes.values() {
                self.physics.set_velocity(r.body, DVec3::ZERO); // replays see them standing still
            }
            self.offset *= libm::expf(-DT / Self::SMOOTHING_SECONDS);
            if self.offset.length_squared() < 1e-8 {
                self.offset = Vec3::ZERO;
            }
        }
    }

    fn record(&mut self, input: InputFrame) {
        let h = self.handle.expect("active");
        self.history[(input.seq % HISTORY) as usize] = Entry {
            seq: input.seq,
            input,
            controller: *self.players.controller(h),
            position: self.players.position(&self.physics, h),
            velocity: self.players.velocity(&self.physics, h),
            touched: self.touching_remote(),
        };
    }

    /// Whether the local capsule touches (or nearly touches) a remote player's proxy.
    fn touching_remote(&self) -> bool {
        let Some(h) = self.handle else { return false };
        let p = self.players.position(&self.physics, h);
        let reach = f64::from(2.0 * self.config.body.radius) + 0.05;
        let tall = f64::from(2.0 * self.config.half_height(false));
        self.remotes.values().any(|r| {
            let q = self.physics.position(r.body);
            let (dx, dz) = (p.x - q.x, p.z - q.z);
            dx * dx + dz * dz < reach * reach && (p.y - q.y).abs() < tall
        })
    }

    /// Whether the prediction from `from` to the present touched a remote player.
    fn touched_since(&self, from: u32) -> bool {
        (from..=self.latest_seq).any(|seq| {
            let e = &self.history[(seq % HISTORY) as usize];
            e.seq == seq && e.touched
        })
    }

    /// Predicts one tick with this (quantized) input; `input.seq` must equal `next_seq()`.
    pub fn tick(&mut self, world: &mut VoxelWorld, input: &InputFrame) {
        if self.handle.is_none() || input.seq != self.next_seq() {
            return;
        }
        self.simulate(world, input, true);
        self.latest_seq = input.seq;
        self.record(*input);
        self.stats.ticks += 1;
    }

    /// Re-runs inputs `from_seq..=latest_seq` from the current state.
    fn replay(&mut self, world: &mut VoxelWorld, from_seq: u32) {
        let mut replayed = 0;
        for seq in from_seq..=self.latest_seq {
            let e = self.history[(seq % HISTORY) as usize];
            if e.seq != seq {
                break; // history ran out
            }
            self.simulate(world, &e.input, false);
            self.record(e.input);
            replayed += 1;
        }
        self.stats.last_replay_ticks = replayed;
    }

    fn reset(&mut self, snapshot: &PhysicsSnapshot) {
        let l = &snapshot.local;
        let center = DVec3::from(l.position);
        let h = *self.handle.get_or_insert_with(|| {
            // The capsule's head points away from the midplane: its feet are above the centre on face B.
            let face = face_of_flags(l.controller.flags, controller_flags::FACE_B);
            let up = DVec3::from(face.up());
            let feet = center - up * f64::from(self.config.half_height(false));
            self.players
                .spawn(&mut self.physics, self.config, feet, 0.0)
        });
        let mut c = PlayerController::default();
        from_net(
            &l.controller,
            |k, id| Self::ground_from_net(&self.remotes, k, id),
            &mut c,
        );
        self.players
            .restore(&mut self.physics, h, &c, center, Vec3::from(l.velocity));
        self.latest_seq = self.latest_seq.max(snapshot.ack_input_seq);
        self.last_ack = snapshot.ack_input_seq;
        self.history.iter_mut().for_each(|e| e.seq = 0);
        if snapshot.ack_input_seq == self.latest_seq {
            self.record(InputFrame {
                seq: snapshot.ack_input_seq,
                ..InputFrame::default()
            });
        }
        self.knockbacks.clear();
        self.offset = Vec3::ZERO;
        self.stats.resets += 1;
    }

    /// Handles this client's snapshot: starts prediction, or compares and reconciles.
    pub fn on_snapshot(&mut self, world: &mut VoxelWorld, snapshot: &PhysicsSnapshot) {
        let l = &snapshot.local;
        if l.flags & player_flags::DEAD != 0 {
            if let Some(h) = self.handle.take() {
                self.players.despawn(&mut self.physics, h);
            }
            return;
        }
        let Some(h) = self.handle else {
            self.reset(snapshot);
            return;
        };
        let ack = snapshot.ack_input_seq;
        if ack < self.last_ack {
            return; // an older snapshot, reordered
        }
        self.last_ack = ack;
        // A knockback we hadn't heard of yet (its event still in flight) explains a large correction.
        let knockback = l.last_knockback_seq > self.last_knockback_seq;
        self.last_knockback_seq = self.last_knockback_seq.max(l.last_knockback_seq);
        self.knockbacks.retain(|&seq, _| seq > ack);
        self.stats.snapshots += 1;

        let e = self.history[(ack % HISTORY) as usize];
        let known = e.seq == ack && ack <= self.latest_seq;
        let server_position = DVec3::from(l.position);
        let server_velocity = Vec3::from(l.velocity);
        if known {
            self.stats.last_error = (e.position - server_position).length() as f32;
            let predicted = to_net(&e.controller, |_| 0);
            let matched = self.stats.last_error <= Self::POSITION_TOLERANCE
                && (e.velocity - server_velocity).length() <= Self::VELOCITY_TOLERANCE
                && predicted.flags == l.controller.flags
                && predicted.slide_flags == l.controller.slide_flags
                && predicted.slide_cooldown == l.controller.slide_cooldown
                && (predicted.external_x - l.controller.external_x).abs()
                    <= Self::VELOCITY_TOLERANCE
                && (predicted.external_z - l.controller.external_z).abs()
                    <= Self::VELOCITY_TOLERANCE;
            if matched {
                return; // the common case: no replay
            }
        }

        // Mispredicted (or history lost): restart from the server's state at `ack` and replay.
        self.stats.replays += 1;
        let before = self.position();
        let mut c = if known {
            e.controller
        } else {
            *self.players.controller(h)
        };
        from_net(
            &l.controller,
            |k, id| Self::ground_from_net(&self.remotes, k, id),
            &mut c,
        );
        self.players
            .restore(&mut self.physics, h, &c, server_position, server_velocity);
        let at_ack = InputFrame {
            seq: ack,
            ..if known {
                e.input
            } else {
                InputFrame::default()
            }
        };
        // A bump into another player, like a knockback, is learnt of a round trip late: its
        // correction is expected to be large and is smoothed further before snapping (Rapier's
        // player-vs-player contacts make these up to ~1.3 m where Jolt's stayed under ~0.7 m).
        let bumped = self.touched_since(ack);
        self.record(at_ack);
        self.latest_seq = self.latest_seq.max(ack);
        self.replay(world, ack + 1);
        let bumped = bumped || self.touched_since(ack);
        let correction = (before - self.position()).as_vec3();
        self.stats.last_correction = correction.length();
        let snap = RECONCILE_SNAP_DISTANCE
            * if knockback || bumped {
                KNOCKBACK_SNAP_FACTOR
            } else {
                1.0
            };
        if self.stats.last_correction > snap {
            self.offset = Vec3::ZERO;
            self.stats.snaps += 1;
        } else {
            self.offset += correction;
        }
    }

    /// A server knockback (PlayerEvent) applied after the pipeline of input `input_seq`.
    pub fn on_knockback(&mut self, world: &mut VoxelWorld, input_seq: u32, delta_v: Vec3) {
        let Some(h) = self.handle else { return };
        if input_seq <= self.last_ack {
            return; // already in the authoritative state
        }
        self.last_knockback_seq = self.last_knockback_seq.max(input_seq);
        self.knockbacks.insert(input_seq, delta_v);
        if input_seq > self.latest_seq {
            return; // applied when that input is predicted
        }
        let previous = self.history[((input_seq - 1) % HISTORY) as usize];
        if previous.seq != input_seq - 1 {
            return; // too old to replay; the next snapshot corrects
        }
        let before = self.position();
        self.players.restore(
            &mut self.physics,
            h,
            &previous.controller,
            previous.position,
            previous.velocity,
        );
        self.replay(world, input_seq);
        self.stats.knockback_replays += 1;
        let correction = (before - self.position()).as_vec3();
        if correction.length() > RECONCILE_SNAP_DISTANCE * KNOCKBACK_SNAP_FACTOR {
            self.offset = Vec3::ZERO;
            self.stats.snaps += 1;
        } else {
            self.offset += correction;
        }
    }

    /// The world's stamina and oxygen rules (ADR 0069), from its mode.
    pub fn set_vitals_rules(&mut self, rules: crate::controller::VitalsRules) {
        self.players.set_vitals(rules);
    }

    /// The server's vitals as of input `seq` (`Message::Vitals`, ADR 0069 §7): where the
    /// prediction differs at that tick (a swing's cost, which isn't predicted, or a misprediction),
    /// the tick is patched and the inputs since replayed, as a knockback is.
    pub fn on_vitals(&mut self, world: &mut VoxelWorld, seq: u32, v: crate::controller::Vitals) {
        let Some(h) = self.handle else { return };
        if seq > self.latest_seq {
            return;
        }
        let i = (seq % HISTORY) as usize;
        let e = self.history[i];
        if e.seq != seq {
            // Too old to replay: take it as now's.
            self.players.controller_mut(h).vitals = v;
            return;
        }
        if !e.controller.vitals.differs(&v) {
            return;
        }
        let before = self.position();
        let mut c = e.controller;
        c.vitals = v;
        self.history[i].controller = c;
        self.players
            .restore(&mut self.physics, h, &c, e.position, e.velocity);
        self.replay(world, seq + 1);
        let correction = (before - self.position()).as_vec3();
        if correction.length() > RECONCILE_SNAP_DISTANCE {
            self.offset = Vec3::ZERO;
            self.stats.snaps += 1;
        } else {
            self.offset += correction;
        }
    }

    fn remote_shape(&self, crouched: bool) -> SharedShape {
        let r = f64::from(self.config.body.radius);
        let half = f64::from(self.config.half_height(crouched));
        SharedShape::capsule_y((half - r).max(0.0), r)
    }

    /// A remote player as a kinematic capsule. `feet` and `velocity` are from a snapshot
    /// `lead_seconds` older than the local player's predicted present; the proxy is extrapolated to
    /// the present and dead-reckoned until the next update (at most `MAX_DEAD_RECKONING`).
    /// On `face` B the player is upside down (its feet are above its capsule's centre).
    pub fn set_remote(
        &mut self,
        player_id: u16,
        feet: DVec3,
        velocity: Vec3,
        crouched: bool,
        lead_seconds: f32,
        face: FaceFrame,
    ) {
        let half = f64::from(self.config.half_height(crouched));
        let lead = lead_seconds.clamp(0.0, MAX_DEAD_RECKONING);
        let center = feet + DVec3::from(face.up()) * half + (velocity * lead).as_dvec3();
        let shape = self.remote_shape(crouched);
        let r = match self.remotes.get_mut(&player_id) {
            Some(r) => {
                if r.crouched != crouched {
                    let collider = self.physics.w.bodies[r.body.0].colliders()[0];
                    self.physics.w.colliders[collider].set_shape(shape);
                    r.crouched = crouched;
                }
                r
            }
            None => {
                let body = self.physics.w.bodies.insert(
                    RigidBodyBuilder::kinematic_velocity_based()
                        .translation(center)
                        .can_sleep(false)
                        .build(),
                );
                let collider = ColliderBuilder::new(shape)
                    .collision_groups(groups(Layer::Character))
                    .friction(0.0)
                    .build();
                self.physics.w.colliders.insert_with_parent(
                    collider,
                    body,
                    &mut self.physics.w.bodies,
                );
                self.remotes.entry(player_id).or_insert(Remote {
                    body: BodyHandle(body),
                    target: center,
                    velocity,
                    dead_reckoning_ticks: 0,
                    crouched,
                })
            }
        };
        r.target = center;
        r.velocity = velocity;
        r.dead_reckoning_ticks = (lead / DT) as i32;
    }

    /// A falling piece's stand-in (docs/FELLING.md §3), at the server's latest pose and moving on
    /// by its velocity (dead reckoning, as remote players): created on first sight with `cells`.
    /// A piece's pose and velocity as of the input `seq` its snapshot acknowledged (call before
    /// [`Predictor::on_snapshot`], so a replay meets it where the server had it). Only a piece that
    /// could touch the local player soon is stood in for ([`stands_in`]); a far one's stand-in is
    /// dropped, and none is made while the player is dead.
    pub fn set_piece(
        &mut self,
        id: u32,
        cells: &[crate::PieceCell],
        position: DVec3,
        rotation: crate::DQuat,
        velocity: Vec3,
        seq: u32,
    ) {
        let kept = self.pieces.contains_key(&id);
        let near = self.handle.is_some_and(|h| {
            let extent = cells
                .iter()
                .map(|c| c.center.length() + c.half.length())
                .fold(0.0, f64::max);
            stands_in(
                extent,
                position,
                velocity.as_dvec3(),
                self.players.position(&self.physics, h),
                self.players.velocity(&self.physics, h).as_dvec3(),
                kept,
            )
        });
        if !near {
            if kept && self.handle.is_some() {
                self.remove_piece(id);
            }
            return;
        }
        let p = self.pieces.entry(id).or_insert_with(|| PieceProxy {
            body: self.physics.add_kinematic_piece(position, rotation, cells),
            position,
            rotation,
            velocity,
            seq,
            pushes: Vec::new(),
            water: WaterDrag::default(),
            friction: 0.0,
        });
        if seq < p.seq {
            return; // an older pose
        }
        p.position = position;
        p.rotation = rotation;
        p.velocity = velocity;
        p.seq = seq;
        p.pushes.retain(|&(s, _)| s > seq);
    }

    /// How the stand-ins are slowed in water (the sim's `felling::water_drag`); none: not at all.
    pub fn set_water_drag(&mut self, f: WaterDragFn) {
        self.water_drag = Some(f);
    }

    /// How many pieces are stood in for now (telemetry `predict.standIns`).
    pub fn stand_ins(&self) -> usize {
        self.pieces.len()
    }

    /// A piece's stand-in's pose now (the predicted present), if it has one.
    pub fn piece_pose(&self, id: u32) -> Option<(DVec3, crate::DQuat)> {
        let p = self.pieces.get(&id)?;
        Some((self.physics.position(p.body), self.physics.rotation(p.body)))
    }

    pub fn remove_piece(&mut self, id: u32) {
        if let Some(p) = self.pieces.remove(&id) {
            self.physics.remove_body(p.body);
        }
    }

    pub fn remove_remote(&mut self, player_id: u16) {
        if let Some(r) = self.remotes.remove(&player_id) {
            self.physics.remove_body(r.body);
        }
    }
    /// A remote proxy's capsule centre (tests and debug views).
    pub fn remote_position(&self, player_id: u16) -> Option<DVec3> {
        self.remotes
            .get(&player_id)
            .map(|r| self.physics.position(r.body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::falls::FallPush;

    dwell_world::both_faces!(
        fn a_floating_body_under_a_fall_is_pushed_down_and_out(f: FaceFrame) {
            // docs/WATER_SURFACE_GRID.md §2e: the server's bodies and the clients' stand-ins alike.
            let drag = WaterDrag {
                linear: 0.5,
                fall: Some(FallPush {
                    dir: 2,
                    strength: 1.0,
                }),
                face: f,
                ..WaterDrag::default()
            };
            // rising (face-locally) and still: after a second it falls and moves along +z
            let mut v = DVec3::from_array(f.vector([0.0, 1.0, 0.0]));
            for _ in 0..60 {
                v = drag.slow(v, 1.0 / 60.0);
            }
            let local = f.vector(v.to_array());
            assert!(local[1] < -1.0 && local[2] > 0.5, "{local:?}");
            // out of a fall, nothing pushes it
            let calm = WaterDrag { fall: None, ..drag };
            let up = DVec3::from_array(f.vector([0.0, 1.0, 0.0]));
            assert_eq!(calm.slow(up, 1.0 / 60.0), up);
        }
    );
}
