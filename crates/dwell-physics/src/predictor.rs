//! Client-side prediction and reconciliation for the local player (PLAYER_CONTROLLER.md §8). Runs
//! the same controller as the server against a small prediction world: the client's terrain, the
//! local player as the only dynamic body, and remote players as kinematic capsules.

use std::collections::HashMap;

use dwell_protocol::{
    GroundKind, InputFrame, PhysicsSnapshot, controller_flags, player_flags,
    players::RECONCILE_SNAP_DISTANCE,
};
use dwell_world::VoxelWorld;
use rapier3d_f64::prelude::*;

use crate::config::PlayerControllerConfig;
use crate::controller::{PlayerController, PlayerHandle, Players};
use crate::net::{dequantize_input, from_net, to_net};
use crate::physics::{BodyHandle, DVec3, Layer, Physics, Vec3, groups};
use crate::query::GroundRef;

const DT: f32 = PlayerControllerConfig::dt();
/// Knockback corrections are smoothed up to this multiple of the snap distance: they are expected
/// to be large (the client learns of them about one RTT late) and must not pop.
const KNOCKBACK_SNAP_FACTOR: f32 = 4.0;
const HISTORY: u32 = 256; // ticks (> 4 s)
const MAX_DEAD_RECKONING: f32 = 0.5; // s

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
            knockbacks: HashMap::new(),
            remotes: HashMap::new(),
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
        };
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
            let face = if l.controller.flags & controller_flags::FACE_B != 0 {
                -1.0
            } else {
                1.0
            };
            let feet =
                center - DVec3::new(0.0, face * f64::from(self.config.half_height(false)), 0.0);
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
        self.record(at_ack);
        self.latest_seq = self.latest_seq.max(ack);
        self.replay(world, ack + 1);
        let correction = (before - self.position()).as_vec3();
        self.stats.last_correction = correction.length();
        let snap = RECONCILE_SNAP_DISTANCE
            * if knockback {
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

    fn remote_shape(&self, crouched: bool) -> SharedShape {
        let r = f64::from(self.config.body.radius);
        let half = f64::from(self.config.half_height(crouched));
        SharedShape::capsule_y((half - r).max(0.0), r)
    }

    /// A remote player as a kinematic capsule. `feet` and `velocity` are from a snapshot
    /// `lead_seconds` older than the local player's predicted present; the proxy is extrapolated to
    /// the present and dead-reckoned until the next update (at most `MAX_DEAD_RECKONING`).
    /// `face_b`: the player is upside down (its feet are above its capsule's centre).
    pub fn set_remote(
        &mut self,
        player_id: u16,
        feet: DVec3,
        velocity: Vec3,
        crouched: bool,
        lead_seconds: f32,
        face_b: bool,
    ) {
        let half = self.config.half_height(crouched);
        let lead = lead_seconds.clamp(0.0, MAX_DEAD_RECKONING);
        let center = feet
            + DVec3::new(0.0, f64::from(if face_b { -half } else { half }), 0.0)
            + (velocity * lead).as_dvec3();
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
