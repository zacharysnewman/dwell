//! The client's predictor against a minimal in-process server (the same controller on its own
//! physics world). The latency, loss and multi-client suites run against the real server (24d).

use std::collections::VecDeque;

use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerHandle, Players};
use dwell_physics::net::{player_flags_of, quantize_input, to_net};
use dwell_physics::predictor::Predictor;
use dwell_physics::{DVec3, Physics, Vec3};
use dwell_protocol::{InputFrame, LocalPlayerState, PhysicsSnapshot, player_flags};
use dwell_world::{FaceFrame, VoxelWorld};

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
            DVec3::new(0.5, dwell_world::FLAT_TOP_M, 0.5),
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
            pieces: Vec::new(),
            swing_kinds: false,
            grid: None,
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
    let top = dwell_world::FLAT_TOP_M;
    p.set_remote(
        7,
        DVec3::new(0.5, top, 3.0),
        Vec3::ZERO,
        false,
        0.0,
        FaceFrame::A,
    );
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

/// Sprints, slides (crouch at 60), lets go at 70 and crouches again at 76, inside the slide's
/// cooldown (PLAYER_CONTROLLER.md §6.9).
fn sprint_slide_script(seq: u32) -> Input {
    Input {
        move_y: 1.0,
        run: true,
        crouch: (60..70).contains(&seq) || seq >= 76,
        ..Input::default()
    }
}

#[test]
fn prediction_resumes_a_sprint_slide_and_its_cooldown_from_the_wire() {
    // A client whose prediction starts from a snapshot (a join, or history lost) mid-slide, and
    // again inside the cooldown: the slide's state comes only from the wire, and the prediction
    // must carry on exactly as the server does.
    for join in [62, 74] {
        let mut server = Server::new();
        let mut world = VoxelWorld::flat();
        for seq in 1..=join {
            server.step(&quantize_input(&sprint_slide_script(seq), seq), None);
        }
        let c = server.players.controller(server.h);
        if join == 62 {
            assert!(c.sprint_slide.active, "the server slides at {join}");
        } else {
            assert!(
                c.sprint_slide.cooldown_ticks > 0,
                "the cooldown runs at {join}"
            );
        }
        let mut p = Predictor::new(PlayerControllerConfig::default());
        p.on_snapshot(&mut world, &server.snapshot(join));
        assert_eq!(p.next_seq(), join + 1);
        for seq in join + 1..=join + 60 {
            let input = quantize_input(&sprint_slide_script(seq), seq);
            p.tick(&mut world, &input);
            server.step(&input, None);
            p.on_snapshot(&mut world, &server.snapshot(seq));
        }
        assert_eq!(p.stats().replays, 0, "joined at {join}: {:?}", p.stats());
    }
}

#[test]
fn the_local_player_rides_a_falling_pieces_stand_in() {
    // docs/FELLING.md §3: a log lying under the player (a 5 × 1 m slab, its top 1 m over the floor)
    // sliding +x at 2 m/s, its pose from the snapshots each tick: the predicted player standing
    // on it, without input, goes with it.
    // face-a-only: the predictor's face handling is the remote proxies' (tested above); the slab
    // is placed in world coordinates.
    let server = Server::new();
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    let mut s = server.snapshot(0);
    s.local.position[1] += 1.0;
    p.on_snapshot(&mut world, &s);
    let top = dwell_world::FLAT_TOP_M;
    let cells: Vec<dwell_physics::PieceCell> = (-2..=2)
        .map(|x| dwell_physics::PieceCell {
            center: DVec3::new(f64::from(x), 0.0, 0.0),
            half: DVec3::new(0.5, 0.5, 2.0),
            mass: 300.0,
            collide: dwell_physics::Collide::All,
            hull: None,
            joined: 0,
        })
        .collect();
    let start = DVec3::new(0.5, top + 0.5, 0.5);
    let speed = 2.0;
    let x0 = p.position().x;
    for seq in 1..=90u32 {
        let at = start + DVec3::X * speed * f64::from(seq) * DT;
        p.set_piece(
            3,
            &cells,
            at,
            dwell_physics::DQuat::IDENTITY,
            Vec3::new(speed as f32, 0.0, 0.0),
            seq,
        );
        p.tick(&mut world, &quantize_input(&Input::default(), seq));
    }
    let moved = p.position().x - x0;
    assert!(
        (moved - speed * 1.5).abs() < 0.4,
        "rode {moved} m of the slab's {}",
        speed * 1.5
    );
    assert!(p.position().y > top + 1.5, "on it: {}", p.position());
    p.remove_piece(3);
}

#[test]
fn only_the_pieces_that_could_touch_the_player_soon_are_stood_in_for() {
    // 2026-11-06: every piece had a stand-in, its pose set and the prediction's physics stepping
    // it each tick and each replayed tick: a dynamite blast's ~300 pieces cost the browser's
    // frame 20–25 ms. Now only a piece that could reach the player within the lookahead is.
    // Red before: one stood in for while the player was unplaced (and all 302 after).
    // face-a-only: the rule is distances and speeds alone, no up; the pieces are placed in world
    // coordinates round a face-A player.
    let server = Server::new();
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    let one = [dwell_physics::PieceCell {
        center: DVec3::ZERO,
        half: DVec3::new(0.5, 0.25, 0.5),
        mass: 500.0,
        collide: dwell_physics::Collide::All,
        hull: None,
        joined: 0,
    }];
    let q = dwell_physics::DQuat::IDENTITY;
    // Dead (no snapshot yet): nothing is stood in for.
    p.set_piece(1, &one, DVec3::ZERO, q, Vec3::ZERO, 0);
    assert_eq!(p.stand_ins(), 0, "none while the player isn't placed");
    p.on_snapshot(&mut world, &server.snapshot(0));
    let me = p.position();
    // 300 still pieces 8–38 m round the player: none could touch it soon.
    for k in 0..300u32 {
        let a = f64::from(k) * 0.37;
        let r = 8.0 + f64::from(k % 30);
        let at = me + DVec3::new(a.cos() * r, 0.0, a.sin() * r);
        p.set_piece(100 + k, &one, at, q, Vec3::ZERO, 0);
    }
    // One 14 m off flying at the player at 20 m/s, one 3 m off flying away.
    let toward = Vec3::new(-20.0, 0.0, 0.0);
    p.set_piece(2, &one, me + DVec3::X * 14.0, q, toward, 0);
    p.set_piece(3, &one, me - DVec3::X * 3.0, q, toward, 0);
    assert_eq!(
        p.stand_ins(),
        2,
        "the one closing in and the one beside the player"
    );
    assert!(p.piece_pose(2).is_some() && p.piece_pose(3).is_some());
    assert!(
        p.piece_pose(100).is_none(),
        "a far one is drawn from the snapshots"
    );
    for seq in 1..=10u32 {
        p.tick(&mut world, &quantize_input(&Input::default(), seq));
    }
    // The one going away is kept a little past the reach it was stood in at, then dropped.
    let still = Vec3::ZERO;
    p.set_piece(3, &one, me - DVec3::X * 6.0, q, still, 10);
    assert!(p.piece_pose(3).is_some(), "kept within the margin");
    p.set_piece(3, &one, me - DVec3::X * 7.0, q, still, 10);
    assert!(p.piece_pose(3).is_none(), "dropped past it");
    // A far one coming near is stood in for again.
    p.set_piece(100, &one, me + DVec3::Z * 4.0, q, still, 10);
    assert!(p.piece_pose(100).is_some());
    assert_eq!(p.stand_ins(), 2);
}

#[test]
fn prediction_runs_stamina_out_as_the_server_does_and_takes_its_corrections() {
    // ADR 0069: the vitals step in the controller on both sides, so a sprint run out to
    // exhaustion (its sprint then refused) predicts without a replay; a cost the client didn't
    // predict (an axe swing, server-side) comes back in `Vitals` and is replayed in.
    use dwell_physics::controller::{Vitals, VitalsRules};
    let rules = VitalsRules {
        stamina: true,
        oxygen: true,
    };
    let mut server = Server::new();
    server.players.set_vitals(rules);
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.set_vitals_rules(rules);
    p.on_snapshot(&mut world, &server.snapshot(0));
    let sprint = |seq: u32| Input {
        move_y: 1.0,
        run: true,
        look_yaw: ((seq / 120) * 90 % 360) as f32,
        ..Input::default()
    };
    let delay = 9;
    let mut in_flight: VecDeque<(PhysicsSnapshot, u32, Vitals)> = VecDeque::new();
    let mut exhausted_at = None;
    let mut swing_seen = false;
    for seq in 1..=900 {
        let input = quantize_input(&sprint(seq), seq);
        p.tick(&mut world, &input);
        server.step(&input, None);
        if seq == 700 {
            // A swing's cost on the server only.
            server.players.controller_mut(server.h).vitals.spend(8.0);
        }
        let v = server.players.controller(server.h).vitals;
        if v.exhausted && exhausted_at.is_none() {
            exhausted_at = Some(seq);
        }
        in_flight.push_back((server.snapshot(seq), seq, v));
        if in_flight.len() > delay {
            let (s, at, v) = in_flight.pop_front().unwrap();
            p.on_snapshot(&mut world, &s);
            p.on_vitals(&mut world, at, v);
            if at == 700 {
                swing_seen = true;
            }
        }
        if !swing_seen && seq >= 700 {
            continue;
        }
        // Once heard of, the prediction agrees with the server at every tick.
        assert!(
            !p.controller().vitals.differs(&v),
            "tick {seq}: {:?} vs {v:?}",
            p.controller().vitals
        );
    }
    let at = exhausted_at.expect("the sprint ran stamina out");
    assert!(at < 700, "exhausted at {at}");
    assert_eq!(p.stats().replays, 0, "{:?}", p.stats());
    let error = (p.position() - server.players.position(&server.physics, server.h)).length();
    assert!(error <= f64::from(Predictor::POSITION_TOLERANCE), "{error}");
}

/// A server and a client predicting it on a generated world with portals (BIFACIAL_WORLD.md
/// §12.3), the client's world given `client_portals`: the player starts 1 m over a portal's
/// opening on face A and drops in, facing 45° with no input. The replays, and the face it ends on.
fn portal_drop(
    client_portals: dwell_world::portal_shafts::PortalShafts,
) -> (dwell_physics::predictor::PredictorStats, FaceFrame) {
    use dwell_world::GENERATOR_TERRAIN;
    let seed = 0;
    let world = || {
        VoxelWorld::new(
            dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed),
            Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, seed)),
        )
    };
    let portals =
        dwell_worldgen::portal_shafts_for_sized(GENERATOR_TERRAIN, seed, Default::default());
    let shaft = portals.shafts[0];
    let mut server = Server::new();
    server.world = world();
    server.world.portals = portals;
    // Its feet 1 m over the opening (the body's centre is half its height over its feet).
    let centre =
        shaft.top(dwell_world::Face::A) + 1.0 + f64::from(server.players.half_height(server.h));
    let start = DVec3::new(shaft.axis[0], centre, shaft.axis[1]);
    server
        .players
        .teleport(&mut server.physics, server.h, start, Vec3::ZERO);
    let mut client_world = world();
    client_world.portals = client_portals;
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.on_snapshot(&mut client_world, &server.snapshot(0));
    let mut in_flight: VecDeque<PhysicsSnapshot> = VecDeque::new();
    for seq in 1..=60 * 8 {
        // Facing 45° (between the monument's pillars), level: the look continued across the
        // midplane is the same (`FaceFrame::look_across` keeps the heading, mirrors the pitch).
        let input = quantize_input(
            &Input {
                look_yaw: 45.0,
                ..Input::default()
            },
            seq,
        );
        p.tick(&mut client_world, &input);
        server.step(&input, None);
        in_flight.push_back(server.snapshot(seq));
        if in_flight.len() > 6 {
            let s = in_flight.pop_front().unwrap();
            p.on_snapshot(&mut client_world, &s);
        }
    }
    let c = server.players.controller(server.h);
    assert!(c.ground.grounded, "still in the air after 8 s");
    (*p.stats(), c.face)
}

#[test]
fn the_prediction_agrees_with_the_server_through_a_portal_and_out_of_it() {
    // both-faces: the drop crosses from face A to face B. The exit push is the shared
    // controller's, read from each side's own `VoxelWorld::portals`, so the client predicts it.
    let portals = dwell_worldgen::portal_shafts_for_sized(
        dwell_world::GENERATOR_TERRAIN,
        0,
        Default::default(),
    );
    let (stats, face) = portal_drop(portals);
    assert_eq!(face, FaceFrame::B, "out on the far face");
    assert_eq!(stats.replays, 0, "{stats:?}");
    // A client without the shafts mispredicts the push: why both sides set them.
    let (stats, _) = portal_drop(Default::default());
    assert!(stats.replays > 0, "{stats:?}");
}

/// Grid 1 is home's frame moved `OFFSET` m along x (no rotation), and the client never predicts a
/// carry: the server's move into grid 1 at the same world point is learnt from a snapshot alone.
struct Shifted;
const OFFSET: f64 = 600_000.0;
impl dwell_physics::predictor::GridTravel for Shifted {
    fn carry(
        &self,
        _: u8,
        _: [f64; 3],
        _: [f64; 3],
        _: bool,
        _: f64,
    ) -> Option<dwell_physics::predictor::GridCarry> {
        None
    }
    fn far_speed(&self, _: u8, _: [f64; 3], _: f64) -> f32 {
        0.0
    }
    fn gravity_scale(&self, _: u8) -> f32 {
        1.0
    }
    fn to_world(&self, grid: u8, p: [f64; 3], _: f64) -> [f64; 3] {
        if grid == 1 {
            [p[0] + OFFSET, p[1], p[2]]
        } else {
            p
        }
    }
}

// face-a-only: the frames' coordinate change is what is checked, not which way is up.
#[test]
fn a_crossing_predicted_a_tick_apart_is_no_snap() {
    // The moon trip (BUGS.md #130): a capture or release the server makes on another tick than
    // the prediction does moves the player ~600 km in coordinates and nowhere in the world; it
    // was counted as a snap of 1 732 km, hiding the real ones around a trip.
    let mut server = Server::new();
    let mut world = VoxelWorld::flat();
    let mut p = Predictor::new(PlayerControllerConfig::default());
    p.set_travel(Box::new(Shifted));
    p.on_snapshot(&mut world, &server.snapshot(0));
    let idle = |seq| quantize_input(&Input::default(), seq);
    for seq in 1..=60 {
        p.tick(&mut world, &idle(seq));
        server.step(&idle(seq), None);
        p.on_snapshot(&mut world, &server.snapshot(seq));
    }
    let snaps = p.stats().snaps;
    // The server carries the player into grid 1 at the same world point; the client, 3 ticks
    // ahead, still predicts home.
    for seq in 61..=63 {
        p.tick(&mut world, &idle(seq));
        server.step(&idle(seq), None);
    }
    let mut s = server.snapshot(61);
    s.local.position[0] -= OFFSET;
    s.grid = Some(dwell_protocol::GridView {
        grid: 1,
        origin: [OFFSET, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
    });
    p.on_snapshot(&mut world, &s);
    assert_eq!(p.grid(), 1, "in the server's grid");
    assert_eq!(p.stats().grid_misses, 1);
    assert!(
        p.stats().last_correction < 0.05,
        "{} m corrected for a carry to the same world point",
        p.stats().last_correction
    );
    assert_eq!(p.stats().snaps, snaps, "no snap");
}
