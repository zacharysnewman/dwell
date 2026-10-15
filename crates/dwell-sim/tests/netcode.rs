//! Networked players (PLAYER_CONTROLLER.md §8, §10): server input handling, snapshots, prediction
//! and reconciliation under simulated latency, jitter and loss, knockback replay, health, death and
//! respawn, block edits seen by prediction, the flight policy. The real `Server` and one
//! `Predictor` per client, joined by simulated links; clients and server tick together.
//! `DWELL_ORIGIN_X=far` moves the world ~8,000 km east (ADR 0011).

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use dwell_blocks::require;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerController, State};
use dwell_physics::net::{player_flags_of, quantize_input};
use dwell_physics::predictor::Predictor;
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::*;
use dwell_sim::entropy::SeededEntropy;
use dwell_sim::server::{EditPolicy, OutKind, Server, ServerConfig, SessionId};
use dwell_world::edit::apply_chunk_changes;
use dwell_world::structure::{Structure, with_structure};
use dwell_world::{
    CHUNK_SIZE, Chunk, ChunkCoord, ChunkGenerator, GENERATOR_FLAT, VoxelWorld, chunk_hash,
    flat_generator,
};
use ed25519_dalek::{Signer, SigningKey};

// --- the test origin -----------------------------------------------------------------------------

const FAR_ORIGIN_X: i32 = 3906 * 2048;

fn origin_x() -> i32 {
    match std::env::var("DWELL_ORIGIN_X").as_deref() {
        Ok("far") => FAR_ORIGIN_X,
        Ok(v) => v.parse().unwrap_or(0),
        Err(_) => 0,
    }
}
fn to_world(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x + f64::from(origin_x()), y, z)
}
fn to_local(p: DVec3) -> Vec3 {
    Vec3::new((p.x - f64::from(origin_x())) as f32, p.y as f32, p.z as f32)
}
fn world_cell_x(x: i32) -> i32 {
    x + origin_x()
}
/// A generator's output moved to the test origin (whole chunks).
fn shifted(g: ChunkGenerator) -> ChunkGenerator {
    let cx = origin_x() / CHUNK_SIZE;
    if cx == 0 {
        return g;
    }
    Arc::new(move |c, chunk| g(ChunkCoord::new(c.x - cx, c.y, c.z), chunk))
}

static PLAYGROUND: LazyLock<Arc<Structure>> = LazyLock::new(|| {
    // The movement playground (PLAYER_CONTROLLER.md §9), shared with the C++ suites until the cutover.
    let text = include_str!("../../../shared/structures/playground.structure");
    Arc::new(Structure::parse(text).expect("the playground structure"))
});

/// The flat world with the playground stamped over it.
fn playground_generator() -> ChunkGenerator {
    with_structure(flat_generator(), PLAYGROUND.clone())
}

fn ticks(seconds: f32) -> i32 {
    (seconds * 60.0).round() as i32
}

// --- links ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct LinkConditions {
    /// Round trip (ms); each direction gets half.
    rtt_ms: f64,
    /// Uniform ± jitter per packet (ms).
    jitter_ms: f64,
    /// Datagram loss probability (reliable messages are retransmitted once).
    loss: f64,
}

struct Packet {
    at: f64,
    order: u64,
    reliable: bool,
    channel: Channel,
    bytes: Vec<u8>,
}

/// One direction of a connection.
struct Link {
    c: LinkConditions,
    rng: u64,
    queue: Vec<Packet>,
    last_reliable: f64,
    next_order: u64,
}

impl Link {
    fn new(c: LinkConditions, seed: u64) -> Self {
        Self {
            c,
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            queue: Vec::new(),
            last_reliable: 0.0,
            next_order: 0,
        }
    }
    /// Uniform in [0, 1) (xorshift64*).
    fn uniform(&mut self) -> f64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn send(&mut self, now: f64, reliable: bool, channel: Channel, bytes: Vec<u8>) {
        let lost = self.uniform() < self.c.loss;
        if lost && !reliable {
            return;
        }
        let mut at = now + self.c.rtt_ms / 2.0 + (self.uniform() * 2.0 - 1.0) * self.c.jitter_ms;
        if reliable {
            if lost {
                at += self.c.rtt_ms; // one retransmission
            }
            at = at.max(self.last_reliable); // ordered
            self.last_reliable = at;
        }
        self.queue.push(Packet {
            at,
            order: self.next_order,
            reliable,
            channel,
            bytes,
        });
        self.next_order += 1;
    }
    fn deliver(&mut self, now: f64) -> Vec<Packet> {
        let (mut due, rest): (Vec<Packet>, Vec<Packet>) = std::mem::take(&mut self.queue)
            .into_iter()
            .partition(|p| p.at <= now);
        self.queue = rest;
        due.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.order.cmp(&b.order)));
        due
    }
}

// --- clients -------------------------------------------------------------------------------------

type Script = Box<dyn Fn(i32, &SimClient) -> Input>;

struct SimClient {
    session: SessionId,
    player_id: u16,
    welcome_flags: u8,
    world: VoxelWorld,
    predictor: Predictor,
    up: Link,
    down: Link,
    script: Option<Script>,
    last_snapshot_tick: u32,
    recent: VecDeque<InputFrame>,
    // Metrics.
    corrections: Vec<f32>, // per replay
    ack_errors: Vec<f32>,  // per compared snapshot
    events: Vec<PlayerEvent>,
    remotes: BTreeMap<u16, RemotePlayerState>, // latest view of others
    last_render: Vec3,
    max_render_step: f32,
    modifications: Vec<VoxelModification>,
}

impl SimClient {
    /// Position in the test's local frame.
    fn position(&self) -> Vec3 {
        to_local(self.predictor.position())
    }
    fn render(&self) -> Vec3 {
        self.position() + self.predictor.render_offset()
    }
    fn count(&self, kind: PlayerEventKind) -> usize {
        self.events.iter().filter(|e| e.kind == kind).count()
    }
}

struct NetSim {
    conditions: LinkConditions,
    server: Server,
    clients: Vec<SimClient>,
    tick: i32,
    /// The clients' own world generator, when it is not the playground (`with_world`).
    client_base: Option<ChunkGenerator>,
}

/// The flat world's config with the playground stamped over it.
fn playground_config() -> ServerConfig {
    ServerConfig {
        generator_version: GENERATOR_FLAT,
        generator_override: Some(playground_generator()),
        ..ServerConfig::default()
    }
}

impl NetSim {
    fn new(conditions: LinkConditions) -> Self {
        Self::with(conditions, playground_config())
    }
    /// The client side builds the playground, so the server does too unless the config names
    /// another generator; both are moved to the test origin.
    fn with(conditions: LinkConditions, mut config: ServerConfig) -> Self {
        let base = config.generator_override.take().unwrap_or_else(|| {
            dwell_worldgen::generator_for(config.generator_version, config.world_seed)
        });
        config.generator_override = Some(shifted(base));
        let mut spawn = config.spawn.unwrap_or_else(|| {
            dwell_worldgen::spawn_point_for(config.generator_version, config.world_seed)
        });
        spawn[0] += f64::from(origin_x());
        config.spawn = Some(spawn);
        Self {
            conditions,
            server: Server::new(config, Box::new(SeededEntropy(1))),
            clients: Vec::new(),
            tick: 0,
            client_base: None,
        }
    }
    /// A world of its own (`base`, before moving to the test origin) on the server and the clients.
    fn with_world(conditions: LinkConditions, base: ChunkGenerator, spawn: [f64; 3]) -> Self {
        let config = ServerConfig {
            generator_version: GENERATOR_FLAT,
            generator_override: Some(base.clone()),
            spawn: Some(spawn),
            ..ServerConfig::default()
        };
        let mut sim = Self::with(conditions, config);
        sim.client_base = Some(base);
        sim
    }

    fn now(&self) -> f64 {
        f64::from(self.tick) * (1000.0 / f64::from(SIM_HZ))
    }

    fn control(&mut self, id: SessionId, m: Message) {
        self.server.on_reliable(id, Channel::Control, &m.encode());
    }

    /// Joins a client (handshake without latency: the link matters for gameplay only). `stream`:
    /// the client passes the worldgen check, so the server streams it chunks and modifications
    /// (its own world is generated, so only Explicit chunks and edits are applied). Returns its index.
    fn join(&mut self, script: Option<Script>, stream: bool) -> usize {
        let id = (self.clients.len() + 1) as SessionId;
        let binding = [0u8; 32];
        self.server
            .on_connected(id, TransportKind::WebTransport, binding);
        let key = SigningKey::from_bytes(&[id as u8; 32]);
        let public_key = key.verifying_key().to_bytes();
        self.control(
            id,
            Message::ClientHello(ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_version: "sim".into(),
                public_key,
                display_name: "Sim".into(),
            }),
        );
        let nonce = self
            .server
            .take_outbox()
            .into_iter()
            .find_map(|o| match Message::decode(&o.bytes) {
                Some(Message::Challenge(n)) => Some(n),
                _ => None,
            })
            .expect("a challenge");
        self.control(
            id,
            Message::ClientAuth(
                key.sign(&auth_transcript(&nonce, &binding, &public_key))
                    .to_bytes(),
            ),
        );
        let welcome = self
            .server
            .take_outbox()
            .into_iter()
            .find_map(|o| match Message::decode(&o.bytes) {
                Some(Message::Welcome(w)) => Some(w),
                _ => None,
            })
            .expect("a welcome");
        let base = self
            .client_base
            .clone()
            .unwrap_or_else(playground_generator);
        let mut world = VoxelWorld::new(shifted(base), None);
        if stream {
            let hash = chunk_hash(world.read(self.server.verification_chunk()));
            self.control(id, Message::WorldgenCheck(hash));
        }
        let seed = u64::from(id);
        self.clients.push(SimClient {
            session: id,
            player_id: welcome.player_id,
            welcome_flags: welcome.flags,
            world,
            predictor: Predictor::new(*self.server.player_config()),
            up: Link::new(self.conditions, seed * 2 + 1),
            down: Link::new(self.conditions, seed * 2 + 2),
            script,
            last_snapshot_tick: 0,
            recent: VecDeque::new(),
            corrections: Vec::new(),
            ack_errors: Vec::new(),
            events: Vec::new(),
            remotes: BTreeMap::new(),
            last_render: Vec3::ZERO,
            max_render_step: 0.0,
            modifications: Vec::new(),
        });
        self.clients.len() - 1
    }

    /// A reliable control message from a client (e.g. a BlockEditRequest), over its link.
    fn send(&mut self, client: usize, m: Message) {
        let now = self.now();
        self.clients[client]
            .up
            .send(now, true, Channel::Control, m.encode());
    }

    fn step(&mut self, n: i32) {
        for _ in 0..n {
            self.step_once();
        }
    }

    fn client_receive(c: &mut SimClient, now: f64) {
        for p in c.down.deliver(now) {
            let Some(m) = Message::decode(&p.bytes) else {
                continue;
            };
            match m {
                Message::PhysicsSnapshot(snap) => {
                    c.last_snapshot_tick = c.last_snapshot_tick.max(snap.server_tick);
                    for r in &snap.remotes {
                        c.remotes.insert(r.player_id, *r);
                        if r.flags & player_flags::DEAD != 0 {
                            c.predictor.remove_remote(r.player_id);
                        } else {
                            // Proxies sit at the latest snapshot position, dead-reckoned between
                            // snapshots (a slightly-past proxy corrects far more gently in
                            // player-vs-player bumps than one extrapolated to the present; measured).
                            let crouched = r.flags & player_flags::CROUCHED != 0;
                            c.predictor.set_remote(
                                r.player_id,
                                DVec3::from(r.position),
                                Vec3::from(r.velocity),
                                crouched,
                                0.0,
                                false,
                            );
                        }
                    }
                    let (replays, snapshots) =
                        (c.predictor.stats().replays, c.predictor.stats().snapshots);
                    c.predictor.on_snapshot(&mut c.world, &snap);
                    if c.predictor.stats().snapshots > snapshots {
                        c.ack_errors.push(c.predictor.stats().last_error);
                    }
                    if c.predictor.stats().replays > replays {
                        c.corrections.push(c.predictor.stats().last_correction);
                    }
                }
                Message::ChunkData(chunk) if chunk.form == ChunkForm::Explicit => {
                    let put =
                        Chunk::from_voxels(&chunk.voxels, chunk.revision).expect("a whole chunk");
                    c.world.put(ChunkCoord::from_array(chunk.coord), put);
                }
                Message::VoxelModification(m) => {
                    for changes in &m.chunks {
                        apply_chunk_changes(&mut c.world, changes);
                    }
                    c.modifications.push(m);
                }
                Message::PlayerEvent(e) => {
                    if e.kind == PlayerEventKind::Knockback && e.player_id == c.player_id {
                        c.predictor
                            .on_knockback(&mut c.world, e.input_seq, Vec3::from(e.vector));
                    }
                    c.events.push(e);
                }
                _ => {}
            }
        }
    }

    fn client_tick(c: &mut SimClient, tick: i32, now: f64) {
        if !c.predictor.active() {
            return;
        }
        let input = match c.script.take() {
            Some(s) => {
                let i = s(tick, c);
                c.script = Some(s);
                i
            }
            None => Input::default(),
        };
        let frame = quantize_input(&input, c.predictor.next_seq());
        c.predictor.tick(&mut c.world, &frame);
        c.recent.push_back(frame);
        while c.recent.len() > MAX_INPUTS_PER_DATAGRAM {
            c.recent.pop_front();
        }
        let msg = PlayerInput {
            last_snapshot_tick: c.last_snapshot_tick,
            inputs: c.recent.iter().copied().collect(),
        };
        c.up.send(
            now,
            false,
            Channel::Control,
            Message::PlayerInput(msg).encode(),
        );
        let render = c.render();
        if c.predictor.stats().ticks > 1 {
            c.max_render_step = c.max_render_step.max((render - c.last_render).length());
        }
        c.last_render = render;
    }

    fn step_once(&mut self) {
        let (now, tick) = (self.now(), self.tick);
        for c in &mut self.clients {
            Self::client_receive(c, now);
        }
        for c in &mut self.clients {
            Self::client_tick(c, tick, now);
        }
        for c in &mut self.clients {
            for p in c.up.deliver(now) {
                if p.reliable {
                    self.server.on_reliable(c.session, p.channel, &p.bytes);
                } else {
                    self.server.on_datagram(c.session, &p.bytes);
                }
            }
        }
        self.server.step();
        self.tick += 1;
        let now = self.now();
        for out in self.server.take_outbox() {
            let Some(c) = self.clients.iter_mut().find(|c| c.session == out.session) else {
                continue;
            };
            match out.kind {
                OutKind::Close => {}
                OutKind::Reliable(channel) => c.down.send(now, true, channel, out.bytes),
                OutKind::Datagram => c.down.send(now, false, Channel::Control, out.bytes),
            }
        }
    }
}

/// Steers towards `target` (x, z) at walking pace; stops within 0.3 m.
fn steer_to(c: &SimClient, x: f32, z: f32, run: bool) -> Input {
    let p = c.position();
    let (dx, dz) = (x - p.x, z - p.z);
    if dx * dx + dz * dz < 0.09 {
        return Input::default();
    }
    Input {
        look_yaw: dx.atan2(dz).to_degrees(),
        move_y: 1.0,
        run,
        ..Input::default()
    }
}

/// A varied movement script on open ground (walk, run, strafe, turn, jump, crouch).
fn wander(tick: i32, offset: i32) -> Input {
    let mut i = Input {
        look_yaw: ((tick * 2 + offset * 60) % 360) as f32,
        ..Input::default()
    };
    match (tick / 90 + offset) % 6 {
        0 => i.move_y = 1.0,
        1 => {
            i.move_y = 1.0;
            i.run = true;
        }
        2 => {
            i.move_x = 1.0;
            i.move_y = 0.5;
            i.jump = tick % 45 < 2;
        }
        3 => {
            i.move_y = -1.0;
            i.crouch = true;
        }
        4 => i.move_x = -1.0,
        _ => {}
    }
    i
}

/// Keeps a wandering player inside a box on open ground (the flat world away from the playground).
fn roam(cx: f32, cz: f32, offset: i32) -> Script {
    Box::new(move |tick, c| {
        let p = c.position();
        if (p.x - cx).abs() > 6.0 || (p.z - cz).abs() > 6.0 {
            steer_to(c, cx, cz, false)
        } else {
            wander(tick, offset)
        }
    })
}

fn percentile(mut v: Vec<f32>, p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[(p * (v.len() - 1) as f32) as usize]
}

fn lossy() -> LinkConditions {
    LinkConditions {
        rtt_ms: 150.0,
        jitter_ms: 20.0,
        loss: 0.05,
    }
}

// --- server --------------------------------------------------------------------------------------

#[test]
fn joining_spawns_a_player_and_snapshots_start() {
    let mut sim = NetSim::new(LinkConditions::default());
    let c = sim.join(None, false);
    assert!(
        sim.server
            .player_handle_of(sim.clients[c].player_id)
            .is_some()
    );
    sim.step(10);
    let c = &sim.clients[c];
    assert!(c.predictor.active());
    assert_eq!(c.predictor.stats().resets, 1);
    assert_eq!(
        sim.server.health_of(c.player_id),
        players::MAX_HEALTH as i32
    );
}

#[test]
fn out_of_range_inputs_are_rejected_and_never_simulated() {
    let mut sim = NetSim::new(LinkConditions::default());
    let c = sim.join(None, false);
    sim.step(5);
    let (session, player) = (sim.clients[c].session, sim.clients[c].player_id);
    let before = sim.server.stats_of(player).unwrap();
    let seq = sim.clients[c].predictor.next_seq() + 10;
    let frame = |seq, move_x, move_y, pitch| InputFrame {
        seq,
        move_x,
        move_y,
        buttons: 0,
        yaw: 0,
        pitch,
    };
    let datagram = |f: InputFrame| {
        Message::PlayerInput(PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![f],
        })
        .encode()
    };
    sim.server
        .on_datagram(session, &datagram(frame(seq, 127, 127, 0))); // |move| = √2
    sim.server
        .on_datagram(session, &datagram(frame(seq + 1, 0, 0, -32768)));
    sim.server
        .on_datagram(session, &datagram(frame(seq + 100_000, 0, 127, 0)));
    let after = sim.server.stats_of(player).unwrap();
    assert_eq!(after.inputs_rejected - before.inputs_rejected, 3);
    assert_eq!(after.inputs_received, before.inputs_received);
    // A malformed datagram (unknown button bit) doesn't even decode, and is dropped.
    let mut bytes = datagram(frame(seq + 2, 0, 0, 0));
    bytes[10] = 0x80;
    sim.server.on_datagram(session, &bytes);
    assert_eq!(
        sim.server.stats_of(player).unwrap().inputs_received,
        before.inputs_received
    );
    // A diagonal of full axes is clamped to the unit circle by the client's quantizer.
    let q = quantize_input(
        &Input {
            move_x: 1.0,
            move_y: 1.0,
            ..Input::default()
        },
        1,
    );
    let (x, y) = (i32::from(q.move_x), i32::from(q.move_y));
    assert!(x * x + y * y <= 127 * 127 + 254);
}

#[test]
fn input_datagrams_are_rate_limited() {
    let mut sim = NetSim::new(LinkConditions::default());
    let c = sim.join(None, false);
    let (session, player) = (sim.clients[c].session, sim.clients[c].player_id);
    for i in 1..=300u32 {
        let m = Message::PlayerInput(PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![InputFrame {
                seq: i,
                ..InputFrame::default()
            }],
        });
        sim.server.on_datagram(session, &m.encode());
    }
    assert_eq!(
        sim.server.stats_of(player).unwrap().datagrams_dropped,
        300 - 2 * SIM_HZ,
        "120 per second pass"
    );
}

#[test]
fn a_hard_fall_damages_a_lethal_one_kills_and_every_client_sees_the_respawn() {
    let mut sim = NetSim::new(LinkConditions::default());
    let (a, b) = (sim.join(None, false), sim.join(None, false));
    sim.step(10);
    let pa = sim.clients[a].player_id;
    let teleport = |sim: &mut NetSim, y: f64| {
        let h = sim.server.player_handle_of(pa).unwrap();
        let (_, physics, players) = sim.server.sim();
        players.teleport(physics, h, to_world(8.5, y, -8.5), Vec3::ZERO);
    };
    // Drop player A from 5 m (lands at ~14 m/s: damaged) and later from 30 m (lethal).
    teleport(&mut sim, 5.9);
    sim.step(90);
    let health = sim.server.health_of(pa);
    assert!(
        health < players::MAX_HEALTH as i32 && health > 0,
        "{health}"
    );
    teleport(&mut sim, 30.9);
    sim.step(150);
    assert_eq!(sim.server.health_of(pa), 0);
    assert!(sim.server.player_handle_of(pa).is_none());
    assert!(
        !sim.clients[a].predictor.active(),
        "the dead player's own client stops predicting"
    );
    for c in [a, b] {
        assert_eq!(sim.clients[c].count(PlayerEventKind::Damage), 2);
        assert_eq!(sim.clients[c].count(PlayerEventKind::Death), 1);
    }
    assert_ne!(sim.clients[b].remotes[&pa].flags & player_flags::DEAD, 0);
    sim.step(players::RESPAWN_SECONDS as i32 * SIM_HZ as i32 + 30);
    assert_eq!(sim.server.health_of(pa), players::MAX_HEALTH as i32);
    assert!(sim.clients[a].predictor.active());
    for c in [a, b] {
        assert_eq!(sim.clients[c].count(PlayerEventKind::Respawn), 1);
    }
    assert_eq!(sim.clients[b].remotes[&pa].flags & player_flags::DEAD, 0);
}

#[test]
fn players_block_each_other_on_the_server() {
    let mut sim = NetSim::new(LinkConditions::default());
    let a = sim.join(Some(Box::new(|_, c| steer_to(c, 8.5, -8.5, false))), false);
    let b = sim.join(Some(Box::new(|_, c| steer_to(c, 8.5, -8.5, false))), false);
    sim.step(ticks(6.0));
    let pos = |sim: &NetSim, c: usize| {
        to_local(DVec3::from(
            sim.server.feet_of(sim.clients[c].player_id).unwrap(),
        ))
    };
    let d = (pos(&sim, a) - pos(&sim, b)).length();
    assert!(d > 0.55, "two 0.3 m capsules can't overlap: {d}");
}

// --- prediction ----------------------------------------------------------------------------------

#[test]
fn without_latency_prediction_matches_the_server_and_never_replays() {
    let mut sim = NetSim::new(LinkConditions::default());
    let c = sim.join(Some(roam(8.5, -8.5, 0)), false);
    sim.step(ticks(20.0));
    let c = &sim.clients[c];
    assert!(c.predictor.stats().snapshots > 300);
    assert!(
        c.predictor.stats().replays <= 2,
        "{:?}",
        c.predictor.stats()
    );
    assert!(percentile(c.ack_errors.clone(), 1.0) < 0.001);
}

#[test]
fn two_clients_at_150_ms_rtt_20_ms_jitter_5_percent_loss() {
    let mut sim = NetSim::new(lossy());
    let a = sim.join(Some(roam(9.0, -9.0, 0)), false);
    let b = sim.join(Some(roam(-9.0, -12.0, 3)), false);
    sim.step(ticks(3.0)); // settle the input buffers
    for c in [a, b] {
        let c = &mut sim.clients[c];
        c.corrections.clear();
        c.ack_errors.clear();
        assert_eq!(c.predictor.stats().snaps, 0);
    }
    let (replays_before, snapshots_before) = (
        sim.clients[a].predictor.stats().replays,
        sim.clients[a].predictor.stats().snapshots,
    );
    sim.step(ticks(60.0));
    for c in [a, b] {
        let c = &sim.clients[c];
        let s = c.predictor.stats();
        let (p95, worst) = (
            percentile(c.ack_errors.clone(), 0.95),
            percentile(c.corrections.clone(), 1.0),
        );
        eprintln!(
            "client {}: {} snapshots, {} replays, error at ack p95 {p95} m; corrections max {worst} m, snaps {}",
            c.player_id, s.snapshots, s.replays, s.snaps
        );
        // Steady-state correction error under ~5 cm (the position error at each acknowledged
        // input, i.e. what a replay would have to correct), and no large corrections at all.
        assert!(p95 < 0.05, "{p95}");
        assert!(worst < 0.2, "{worst}");
        assert_eq!(s.snaps, 0);
    }
    let s = sim.clients[a].predictor.stats();
    let ratio = (s.replays - replays_before) as f32 / (s.snapshots - snapshots_before) as f32;
    assert!(ratio < 0.5, "most snapshots need no replay: {ratio}");
    // Each client sees the other move (remote state in snapshots).
    assert!(
        sim.clients[a]
            .remotes
            .contains_key(&sim.clients[b].player_id)
    );
    assert!(
        sim.clients[b]
            .remotes
            .contains_key(&sim.clients[a].player_id)
    );
}

#[test]
fn input_takes_effect_on_the_very_next_predicted_tick() {
    let mut sim = NetSim::new(lossy());
    let go = Rc::new(Cell::new(false));
    let g = go.clone();
    let c = sim.join(
        Some(Box::new(move |_, _| Input {
            move_y: if g.get() { 1.0 } else { 0.0 },
            ..Input::default()
        })),
        false,
    );
    sim.step(ticks(1.0));
    go.set(true);
    sim.step(1);
    assert!(
        sim.clients[c].predictor.velocity().z > 0.5,
        "{}",
        sim.clients[c].predictor.velocity()
    );
}

#[test]
fn a_server_knockback_replays_smoothly_at_150_ms_rtt() {
    let mut sim = NetSim::new(lossy());
    let c = sim.join(
        Some(Box::new(|tick, c| {
            if tick < ticks(4.0) {
                steer_to(c, 0.5, -5.5, false)
            } else {
                Input::default()
            }
        })),
        false,
    );
    sim.step(ticks(2.0));
    // A server-originated upward kick (an explosion, a hit) while walking: 14 m/s up.
    sim.server
        .knockback(sim.clients[c].player_id, Vec3::new(0.0, 14.0, 0.0));
    sim.step(ticks(4.0));
    let c = &sim.clients[c];
    assert!(c.count(PlayerEventKind::Knockback) >= 1);
    assert!(
        c.predictor.stats().knockback_replays >= 1,
        "{:?}",
        c.predictor.stats()
    );
    assert_eq!(c.predictor.stats().snaps, 0);
    eprintln!("largest rendered step per tick: {} m", c.max_render_step);
    assert!(
        c.max_render_step < 0.8,
        "continuous: the ~2 m correction is smoothed, not popped: {}",
        c.max_render_step
    );
}

#[test]
fn bumping_into_another_player_corrects_smoothly() {
    let mut sim = NetSim::new(lossy());
    let a = sim.join(
        Some(Box::new(|tick, c| {
            if tick < ticks(3.0) {
                steer_to(c, 9.0, -9.0, false)
            } else {
                steer_to(c, 9.0, -4.0, false)
            }
        })),
        false,
    );
    let b = sim.join(
        Some(Box::new(|tick, c| {
            if tick < ticks(3.0) {
                steer_to(c, 9.0, -4.0, false)
            } else {
                steer_to(c, 9.0, -9.0, false)
            }
        })),
        false,
    );
    sim.step(ticks(8.0));
    for c in [a, b] {
        let c = &sim.clients[c];
        eprintln!(
            "client {}: max rendered step {} m, snaps {}",
            c.player_id,
            c.max_render_step,
            c.predictor.stats().snaps
        );
        assert_eq!(c.predictor.stats().snaps, 0);
        assert!(c.max_render_step < 0.3, "{}", c.max_render_step);
    }
}

#[test]
fn predictor_remote_proxy_right_after_the_first_snapshot() {
    let mut world = VoxelWorld::new(playground_generator(), None);
    let mut p = Predictor::new(PlayerControllerConfig::default());
    let mut snap = PhysicsSnapshot {
        server_tick: 3,
        ..PhysicsSnapshot::default()
    };
    snap.local.position = [0.0, 0.9, -0.25];
    snap.local.controller.flags = controller_flags::GROUNDED;
    snap.local.controller.ground_kind = GroundKind::Terrain;
    p.on_snapshot(&mut world, &snap);
    p.set_remote(
        1,
        DVec3::new(-1.0, 0.0, -0.25),
        Vec3::ZERO,
        false,
        0.0,
        false,
    );
    for _ in 0..10 {
        let f = quantize_input(&Input::default(), p.next_seq());
        p.tick(&mut world, &f);
    }
    assert!(p.active());
}

// --- sliding (PLAYER_CONTROLLER.md §6.8) ---------------------------------------------------------

/// Single-block stone stairs 1–4 high along +z at x = 30 (z 30…33) on the flat world: 45° ramps,
/// which slide.
fn stairs_generator() -> ChunkGenerator {
    let text = "fill 30 0 30  30 0 33  dwell:stone\n\
                fill 30 1 31  30 1 33  dwell:stone\n\
                fill 30 2 32  30 2 33  dwell:stone\n\
                fill 30 3 33  30 3 33  dwell:stone\n";
    let stairs = Arc::new(Structure::parse(text).expect("the stairs"));
    with_structure(flat_generator(), stairs)
}

/// A spawns on the third stair's ramp and slides off it, steers, then tries to walk up the stairs
/// and goes off to roam; B later runs into them from their foot. Returns the run (12 s) after
/// checking that both slid, B saw A sliding, and neither climbed the stairs.
fn sliding_on_steep_stairs(conditions: LinkConditions) -> (NetSim, [usize; 2]) {
    let mut sim = NetSim::with_world(conditions, stairs_generator(), [32.0, 2.6, 33.25]);
    let a = sim.join(
        Some(Box::new(|t, c| match t {
            0..90 => Input::default(),
            90..150 => Input {
                move_x: 1.0,
                ..Input::default()
            },
            150..270 => steer_to(c, 30.5, 36.0, false),
            // Away from B: player bumps mispredict on their own (§8.2).
            _ => roam(42.0, 22.0, 1)(t, c),
        })),
        false,
    );
    let b = sim.join(
        Some(Box::new(|t, c| match t {
            0..180 => Input::default(),
            // Round to the foot of the stairs, then run up them.
            180..330 => steer_to(c, 30.5, 27.0, false),
            330..480 => steer_to(c, 30.5, 36.0, true),
            _ => roam(20.0, 26.0, 4)(t, c),
        })),
        false,
    );
    let (mut slid_a, mut slid_b, mut b_saw_a_slide) = (0, 0, false);
    let id_a = sim.clients[a].player_id;
    for _ in 0..ticks(12.0) {
        sim.step(1);
        slid_a += i32::from(sim.clients[a].predictor.controller().state == State::Sliding);
        slid_b += i32::from(sim.clients[b].predictor.controller().state == State::Sliding);
        b_saw_a_slide |= sim.clients[b]
            .remotes
            .get(&id_a)
            .is_some_and(|r| r.state == PlayerState::Sliding);
    }
    assert!(slid_a > 10, "A slid for {slid_a} ticks");
    assert!(slid_b > 3, "B slid for {slid_b} ticks");
    assert!(b_saw_a_slide, "B sees A sliding (for its sounds)");
    for c in [a, b] {
        let c = &sim.clients[c];
        let s = c.predictor.stats();
        eprintln!(
            "client {}: {} snapshots, {} replays, error at ack p95 {} m; corrections max {} m, snaps {}",
            c.player_id,
            s.snapshots,
            s.replays,
            percentile(c.ack_errors.clone(), 0.95),
            percentile(c.corrections.clone(), 1.0),
            s.snaps
        );
        // Neither climbed the stairs (the top is 4 m up; this is the capsule's centre).
        assert!(c.position().y < 2.5, "{}", c.position());
    }
    (sim, [a, b])
}

#[test]
fn sliding_is_predicted_exactly_without_latency() {
    // The slide is derived from the probe and the resumable state each tick, so the prediction
    // matches the server: only the one replay every join makes (the spawn, before any input).
    let (sim, clients) = sliding_on_steep_stairs(LinkConditions::default());
    for c in clients {
        let c = &sim.clients[c];
        assert!(
            c.predictor.stats().replays <= 2,
            "{:?}",
            c.predictor.stats()
        );
        // (The spawn replay's error is the fall onto the ramp before the first snapshot.)
        assert!(percentile(c.ack_errors.clone(), 0.99) < 0.001);
        assert!(percentile(c.corrections.clone(), 1.0) < 0.05);
    }
}

#[test]
fn sliding_predicts_without_snaps_at_150_ms_rtt_20_ms_jitter_5_percent_loss() {
    let (sim, clients) = sliding_on_steep_stairs(lossy());
    for c in clients {
        let c = &sim.clients[c];
        assert_eq!(c.predictor.stats().snaps, 0);
        assert!(percentile(c.ack_errors.clone(), 0.95) < 0.05);
        // The residual corrections are a tick or two of motion when jitter starves the server's
        // input buffer (§8.2): up to ~0.17 m a tick at slide speed, against 0.13 m when running.
        let worst = percentile(c.corrections.clone(), 1.0);
        assert!(worst < 0.5, "{worst}");
    }
}

// --- block edits ---------------------------------------------------------------------------------

#[test]
fn a_block_placed_by_one_player_blocks_another_on_the_server_and_in_its_prediction() {
    // Open ground; slot 0 at x 19, slot 1 at x 20.
    let config = ServerConfig {
        spawn: Some([20.5, 0.0, 0.5]),
        ..playground_config()
    };
    let mut sim = NetSim::with(
        LinkConditions {
            rtt_ms: 100.0,
            jitter_ms: 10.0,
            loss: 0.0,
        },
        config,
    );
    let walk = Rc::new(Cell::new(false));
    let w = walk.clone();
    let a = sim.join(None, true);
    let b = sim.join(
        Some(Box::new(move |_, c| {
            if w.get() {
                steer_to(c, 20.5, 8.0, false)
            } else {
                Input::default()
            }
        })),
        true,
    );
    sim.step(ticks(1.0));

    // A builds a wall two blocks high across B's path at z = 3 (edits spaced for the rate limit).
    let stone = require("dwell:stone");
    for y in [-1, 0] {
        // The second row from the far end: a natural block beside a higher one is a ramp rising to
        // it (ADAPTIVE_BLOCKS.md), whose top A (at x 19) must face to place on it.
        let xs = if y < 0 { [19, 20, 21] } else { [21, 20, 19] };
        for x in xs {
            let edit = BlockEditRequest {
                action: BlockEditAction::Place,
                cell: [world_cell_x(x), y, 3],
                face: 2,
                material: stone,
            };
            sim.send(a, Message::BlockEditRequest(edit));
            sim.step(8);
        }
    }
    sim.step(ticks(0.5));
    for y in [0, 1] {
        for x in [19, 20, 21] {
            let at = (world_cell_x(x), y, 3);
            assert_eq!(
                sim.server.world().get_voxel(at.0, at.1, at.2),
                stone,
                "server {x} {y}"
            );
            assert_eq!(
                sim.clients[a].world.get_voxel(at.0, at.1, at.2),
                stone,
                "client a {x} {y}"
            );
            assert_eq!(
                sim.clients[b].world.get_voxel(at.0, at.1, at.2),
                stone,
                "client b {x} {y}"
            );
        }
    }
    assert_eq!(
        sim.server
            .stats_of(sim.clients[a].player_id)
            .unwrap()
            .edits_applied,
        6
    );
    assert!(sim.clients[b].modifications.len() >= 2);

    // B walks into it: stopped by the wall on the server and in its own prediction.
    walk.set(true);
    let (mut server_max, mut client_max) = (-10.0f32, -10.0f32);
    let pb = sim.clients[b].player_id;
    for _ in 0..ticks(3.0) {
        sim.step(1);
        let feet = sim.server.feet_of(pb).unwrap();
        server_max = server_max.max(to_local(DVec3::from(feet)).z);
        client_max = client_max.max(sim.clients[b].position().z);
    }
    eprintln!("closest approach: server {server_max}, client {client_max}");
    assert!(server_max > 2.5, "it did walk up to the wall: {server_max}");
    assert!(server_max < 3.0 - 0.29, "{server_max}");
    assert!(client_max < 3.0 - 0.29, "{client_max}");
}

// --- the flight policy (§8.3): who may fly is the server's call ------------------------------------

fn fly_script() -> Script {
    Box::new(|_, _| Input {
        fly: true,
        jump: true,
        ..Input::default()
    })
}

#[test]
fn everyone_may_fly_by_default() {
    let mut sim = NetSim::new(LinkConditions::default());
    let c = sim.join(Some(fly_script()), false);
    assert_ne!(sim.clients[c].welcome_flags & welcome_flags::FLIGHT, 0);
    sim.step(10);
    let start = sim.clients[c].position().y;
    sim.step(ticks(1.0));
    assert!(sim.clients[c].position().y > start + 5.0);
}

#[test]
fn with_flight_off_the_server_ignores_the_fly_bit() {
    let config = ServerConfig {
        flight: EditPolicy::Nobody,
        ..playground_config()
    };
    let mut sim = NetSim::with(LinkConditions::default(), config);
    let c = sim.join(Some(fly_script()), false);
    assert_eq!(sim.clients[c].welcome_flags & welcome_flags::FLIGHT, 0);
    sim.step(10);
    let start = sim.clients[c].position().y;
    sim.step(ticks(1.0));
    // Jumping in place: never much above the start once the prediction is corrected.
    assert!(
        sim.clients[c].position().y < start + 2.0,
        "{}",
        sim.clients[c].position()
    );
}

#[test]
fn fast_creative_flight_keeps_snapshots_coming_and_prediction_in_step() {
    let mut sim = NetSim::new(LinkConditions::default());
    let c = sim.join(
        Some(Box::new(|tick, _| {
            let mut i = Input {
                fly: true,
                run: true,
                ..Input::default()
            };
            if tick < 300 {
                i.jump = true; // climb
            } else {
                i.move_y = 1.0; // then race forward
                i.look_yaw = 60.0;
            }
            i
        })),
        false,
    );
    sim.step(10);
    let before = sim.clients[c].predictor.stats().snapshots;
    sim.step(ticks(20.0));
    let st = *sim.clients[c].predictor.stats();
    eprintln!(
        "snapshots {} replays {} pos {}",
        st.snapshots - before,
        st.replays,
        sim.clients[c].position()
    );
    assert!(
        st.snapshots - before > 300,
        "SNAPSHOT_HZ × 20 s, give or take"
    );
    let feet = sim.server.feet_of(sim.clients[c].player_id).expect("alive");
    let half = f64::from(sim.server.player_config().half_height(false));
    let authoritative = to_local(DVec3::new(feet[0], feet[1] + half, feet[2]));
    assert!((authoritative - sim.clients[c].position()).length() < 50.0);
}

#[test]
fn player_flags_use_the_player_flags_bits_not_the_controllers() {
    // Regression: player_flags_of picked up ControllerFlags constants of the same names, so a
    // swimming (and flying) player's snapshot carried an unknown bit and was dropped by every
    // decoder, and a climbing player showed as swimming to others.
    let flags = |f: &dyn Fn(&mut PlayerController)| {
        let mut c = PlayerController::default();
        f(&mut c);
        player_flags_of(&c, false)
    };
    assert_eq!(flags(&|_| {}), 0);
    assert_eq!(flags(&|c| c.ground.grounded = true), player_flags::GROUNDED);
    assert_eq!(flags(&|c| c.crouching = true), player_flags::CROUCHED);
    assert_eq!(flags(&|c| c.climb.climbing = true), player_flags::CLIMBING);
    assert_eq!(flags(&|c| c.swim.swimming = true), player_flags::SWIMMING);
    assert_eq!(flags(&|c| c.fly.flying = true), player_flags::FLYING);
    assert_eq!(
        player_flags_of(&PlayerController::default(), true),
        player_flags::DEAD
    );
}
