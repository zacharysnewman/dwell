//! Steering a ship (docs/CANNONS.md C5; owner, 2026-11-17: "ship should be moveable with player
//! controls, force should be exerted like a sail would, from the center of mass I think. Turning
//! like you have a rudder."): a Use of a helm on a raft afloat puts the player at it; forward pushes
//! the raft along the helm's facing, sideways turns it (a rudder: only while it moves), the player
//! stands still on the deck, and jump leaves the helm. Both faces.

mod common;

use common::*;
use dwell_physics::controller::Input;
use dwell_protocol::{BlockEditAction, BodyEditRequest, Message, PlayerInput};
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, both_faces, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE + 4), 0.5])),
        ..flat()
    }
}

/// The world cell rows go up on face `f` (+1 on A, −1 on B).
fn up_rows(f: FaceFrame) -> i32 {
    f.cell([0, 1, 0])[1] - f.cell([0, 0, 0])[1]
}

/// A pool (stone under 6 rows of water, its surface at row `BASE`) and a 6 × 4 plank raft on it
/// with a helm facing north (−z) on its deck: the raft's id and the helm's cell in its grid.
fn harbour(fx: &mut Fixture, f: FaceFrame) -> (u32, [i32; 3]) {
    let water = dwell_blocks::require("dwell:water");
    let stone = f.state(dwell_blocks::require("dwell:stone"));
    for x in -12..20 {
        for z in -60..12 {
            for r in -7..0 {
                let c = f.cell([x, BASE + r, z]);
                let m = if r == -7 { stone } else { water };
                fx.server.world().set_voxel(c[0], c[1], c[2], m);
            }
        }
    }
    let planks = f.state(dwell_blocks::require("dwell:planks"));
    let helm = f.state(dwell_blocks::require("dwell:helm[facing=north]"));
    let up = up_rows(f);
    let mut cells: Vec<([i32; 3], u16)> = (0..6)
        .flat_map(|x| (0..4).map(move |z| ([x, 0, z], planks)))
        .collect();
    let at = [3, up, 1];
    cells.push((at, helm));
    let c = f.cell([0, BASE, 0]);
    let centre = [
        f64::from(c[0]) + 0.5,
        f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H },
        f64::from(c[2]) + 0.5,
    ];
    (fx.server.add_body(1, cells, centre), at)
}

/// Puts player 1's feet at `feet`, its body above them along `up` (face-local up), at rest.
fn stand(fx: &mut Fixture, feet: dwell_physics::DVec3, up: dwell_physics::DVec3) {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = f64::from(players.half_height(h));
    players.teleport(physics, h, feet + up * half, dwell_physics::Vec3::ZERO);
}

/// Player 1's input `input` for `ticks` ticks, stepping the server.
fn hold(fx: &mut Fixture, seq: &mut u32, input: Input, ticks: u32) {
    for _ in 0..ticks {
        *seq += 1;
        let m = Message::PlayerInput(PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![dwell_physics::net::quantize_input(&input, *seq)],
        });
        fx.server.on_datagram(1, &m.encode());
        fx.server.step();
    }
}

/// The raft's heading: the helm's forward (north, −z) turned with it, flat.
fn heading(fx: &Fixture, raft: u32) -> dwell_physics::DVec3 {
    let (_, q) = fx.server.body_pose(raft).expect("the raft");
    let h = q * dwell_physics::DVec3::new(0.0, 0.0, -1.0);
    dwell_physics::DVec3::new(h.x, 0.0, h.z).normalize()
}

both_faces!(
    fn a_player_at_the_helm_sails_and_steers_the_raft(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, helm) = harbour(&mut fx, f);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 120);
        // On the deck beside the helm.
        let (p0, _) = fx.server.body_pose(raft).expect("afloat");
        let up = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let feet =
            p0 + dwell_physics::DVec3::new(1.5, 0.0, 1.5) + up * (0.5 * dwell_world::CELL_H + 0.05);
        stand(&mut fx, feet, up);
        hold(&mut fx, &mut seq, Input::default(), 30);
        // Its Use: the player is at the helm, nothing placed.
        fx.send(
            1,
            Message::BodyEditRequest(BodyEditRequest {
                body: raft,
                cell: helm.map(|v| v as i16),
                face: 4,
                action: BlockEditAction::Place,
                material: 0,
            }),
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        assert_eq!(fx.server.at_helms(), vec![(1, raft)], "at the helm");
        // Forward: the raft sails north; the player rides along, not walking.
        let (start, _) = fx.server.body_pose(raft).unwrap();
        let ahead = Input {
            move_y: 1.0,
            ..Input::default()
        };
        hold(&mut fx, &mut seq, ahead, 240);
        let (now, _) = fx.server.body_pose(raft).unwrap();
        assert!(start.z - now.z > 1.0, "sailed north: {start:?} -> {now:?}");
        let rider = dwell_physics::DVec3::from(fx.server.feet_of(1).unwrap());
        let (rx, rz) = (rider.x - now.x, rider.z - now.z);
        assert!(
            (rx - 1.5).abs() < 0.6 && (rz - 1.5).abs() < 0.6,
            "stood still on the deck: {rx}, {rz}"
        );
        // Right as well: the heading turns to its right (heading × up), as a rudder turns a ship.
        let h0 = heading(&fx, raft);
        let right = h0.cross(up);
        let turning = Input {
            move_x: 1.0,
            move_y: 1.0,
            ..Input::default()
        };
        hold(&mut fx, &mut seq, turning, 240);
        let h1 = heading(&fx, raft);
        assert!(h1.dot(right) > 0.15, "turned right: {h0:?} -> {h1:?}");
        // Jump leaves the helm.
        hold(
            &mut fx,
            &mut seq,
            Input {
                jump: true,
                ..Input::default()
            },
            2,
        );
        assert!(fx.server.at_helms().is_empty(), "left the helm");
    }
);

both_faces!(
    fn a_rudder_does_not_turn_a_still_raft(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, helm) = harbour(&mut fx, f);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 120);
        let (p0, _) = fx.server.body_pose(raft).expect("afloat");
        let up = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let feet =
            p0 + dwell_physics::DVec3::new(1.5, 0.0, 1.5) + up * (0.5 * dwell_world::CELL_H + 0.05);
        stand(&mut fx, feet, up);
        hold(&mut fx, &mut seq, Input::default(), 30);
        fx.send(
            1,
            Message::BodyEditRequest(BodyEditRequest {
                body: raft,
                cell: helm.map(|v| v as i16),
                face: 4,
                action: BlockEditAction::Place,
                material: 0,
            }),
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        let h0 = heading(&fx, raft);
        hold(
            &mut fx,
            &mut seq,
            Input {
                move_x: 1.0,
                ..Input::default()
            },
            120,
        );
        let h1 = heading(&fx, raft);
        assert!(
            h0.dot(h1) > 0.995,
            "a still raft barely turns: {h0:?} -> {h1:?}"
        );
    }
);
