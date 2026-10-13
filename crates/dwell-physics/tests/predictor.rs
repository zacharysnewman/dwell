//! The client's predictor against a minimal in-process server (the same controller on its own
//! physics world). The latency, loss and multi-client suites run against the real server (24d).

use std::collections::VecDeque;

use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerHandle, Players};
use dwell_physics::net::{player_flags_of, quantize_input, to_net};
use dwell_physics::predictor::Predictor;
use dwell_physics::{DVec3, Physics, Vec3};
use dwell_protocol::{InputFrame, LocalPlayerState, PhysicsSnapshot, player_flags};
use dwell_world::VoxelWorld;

const DT: f64 = 1.0 / 60.0;

struct Server {
    world: VoxelWorld,
    physics: Physics,
    players: Players,
    h: PlayerHandle,
    last_knockback_seq: u32,
}

impl Server {
    fn new() -> Self {
        let mut physics = Physics::default();
        let mut players = Players::new(&mut physics, true);
        let h = players.spawn(
            &mut physics,
            PlayerControllerConfig::default(),
            DVec3::new(0.5, 0.0, 0.5),
            0.0,
        );
        Self {
            world: VoxelWorld::flat(),
            physics,
            players,
            h,
            last_knockback_seq: 0,
        }
    }
    fn step(&mut self, input: &InputFrame, knockback: Option<Vec3>) {
        self.players
            .set_input(self.h, dwell_physics::net::dequantize_input(input));
        self.players.tick(&mut self.world, &mut self.physics);
        if let Some(dv) = knockback {
            self.players.add_velocity(&mut self.physics, self.h, dv);
            self.last_knockback_seq = input.seq;
        }
        self.physics.step(DT);
    }
    fn snapshot(&self, ack: u32) -> PhysicsSnapshot {
        let c = self.players.controller(self.h);
        PhysicsSnapshot {
            server_tick: ack,
            ack_input_seq: ack,
            local: LocalPlayerState {
                position: self.players.position(&self.physics, self.h).to_array(),
                velocity: self.players.velocity(&self.physics, self.h).to_array(),
                flags: player_flags_of(c, false),
                last_knockback_seq: self.last_knockback_seq,
                controller: to_net(c, |_| 0),
                ..LocalPlayerState::default()
            },
            remotes: Vec::new(),
        }
    }
}

/// Walks, runs, turns and jumps.
fn script(seq: u32) -> Input {
    let t = seq as i32;
    Input {
        move_x: if t % 120 < 60 { 0.0 } else { 0.5 },
        move_y: 1.0,
        run: t % 200 > 100,
        jump: t % 90 == 45,
        look_yaw: (t % 360) as f32,
        ..Input::default()
    }
}

/// Runs `ticks` ticks; snapshots reach the client `delay` ticks after the server makes them.
fn run(ticks: u32, delay: usize, knockback_at: Option<u32>, event: bool) -> (Predictor, Server) {
    let mut server = Server::new();
    let mut client_world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.on_snapshot(&mut client_world, &server.snapshot(0));
    assert!(p.active());
    let mut in_flight: VecDeque<PhysicsSnapshot> = VecDeque::new();
    let kick = Vec3::new(6.0, 4.0, 0.0);
    for seq in 1..=ticks {
        let input = quantize_input(&script(seq), seq);
        assert_eq!(p.next_seq(), seq);
        p.tick(&mut client_world, &input);
        server.step(&input, (knockback_at == Some(seq)).then_some(kick));
        if event && knockback_at == Some(seq) {
            p.on_knockback(&mut client_world, seq, kick); // the event outruns the snapshot
        }
        in_flight.push_back(server.snapshot(seq));
        if in_flight.len() > delay {
            let s = in_flight.pop_front().unwrap();
            p.on_snapshot(&mut client_world, &s);
        }
    }
    (p, server)
}

#[test]
fn without_latency_prediction_matches_the_server_and_never_replays() {
    let (p, server) = run(600, 0, None, false);
    assert_eq!(p.stats().replays, 0, "{:?}", p.stats());
    assert_eq!(p.stats().snapshots, 600);
    assert!(p.stats().last_error <= Predictor::POSITION_TOLERANCE);
    let error = (p.position() - server.players.position(&server.physics, server.h)).length();
    assert!(error <= f64::from(Predictor::POSITION_TOLERANCE), "{error}");
}

#[test]
fn with_latency_and_no_surprises_there_are_no_replays() {
    let (p, _) = run(600, 9, None, false);
    assert_eq!(p.stats().replays, 0, "{:?}", p.stats());
    assert_eq!(p.stats().snapshots, 600 - 9);
}

#[test]
fn an_unannounced_server_knockback_replays_once_and_smoothly() {
    let (p, server) = run(300, 9, Some(150), false);
    let s = p.stats();
    assert_eq!(s.replays, 1, "{s:?}");
    assert_eq!(s.snaps, 0, "a knockback is smoothed, not snapped: {s:?}");
    assert!(s.last_correction > 0.1, "{s:?}");
    // Back in step with the server: the last snapshot it saw matched.
    assert!(s.last_error <= Predictor::POSITION_TOLERANCE, "{s:?}");
    let lag = (p.position() - server.players.position(&server.physics, server.h)).length();
    assert!(lag <= f64::from(Predictor::POSITION_TOLERANCE), "{lag}");
}

#[test]
fn a_knockback_event_replays_at_once_and_the_snapshots_then_match() {
    let (p, _) = run(300, 9, Some(150), true);
    let s = p.stats();
    assert_eq!(s.knockback_replays, 1, "{s:?}");
    assert_eq!(s.replays, 0, "{s:?}");
}

#[test]
fn the_render_offset_decays() {
    let mut server = Server::new();
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.on_snapshot(&mut world, &server.snapshot(0));
    let idle = |seq| quantize_input(&Input::default(), seq);
    for seq in 1..=10 {
        p.tick(&mut world, &idle(seq));
        server.step(&idle(seq), None);
    }
    // The server says the player is 0.5 m east: a correction under the snap distance is smoothed.
    let mut s = server.snapshot(10);
    s.local.position[0] += 0.5;
    p.on_snapshot(&mut world, &s);
    assert_eq!(p.stats().replays, 1);
    assert!(
        (p.render_offset().x + 0.5).abs() < 0.01,
        "{}",
        p.render_offset()
    );
    for seq in 11..=70 {
        p.tick(&mut world, &idle(seq));
    }
    assert!(p.render_offset().length() < 1e-3, "{}", p.render_offset());
    // Beyond the snap distance it snaps.
    let mut s = server.snapshot(70);
    s.local.position[0] += 5.0;
    p.on_snapshot(&mut world, &s);
    assert_eq!(p.stats().snaps, 1);
    assert_eq!(p.render_offset(), Vec3::ZERO);
}

#[test]
fn death_stops_prediction() {
    let server = Server::new();
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.on_snapshot(&mut world, &server.snapshot(0));
    let mut s = server.snapshot(0);
    s.local.flags |= player_flags::DEAD;
    p.on_snapshot(&mut world, &s);
    assert!(!p.active());
}

#[test]
fn a_remote_proxy_blocks_the_local_player() {
    let server = Server::new();
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.on_snapshot(&mut world, &server.snapshot(0));
    p.set_remote(7, DVec3::new(0.5, 0.0, 3.0), Vec3::ZERO, false, 0.0, false);
    for seq in 1..=120 {
        p.tick(
            &mut world,
            &quantize_input(
                &Input {
                    move_y: 1.0,
                    ..Input::default()
                },
                seq,
            ),
        );
    }
    assert!(p.position().z < 2.5, "{}", p.position());
    assert!(p.remote_position(7).is_some());
    p.remove_remote(7);
    assert!(p.remote_position(7).is_none());
}
