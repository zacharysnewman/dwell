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
        hold(&mut fx, &mut seq, turning, 90);
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

both_faces!(
    fn a_cannon_on_a_turned_raft_fires_along_the_raft_and_kicks_it_back(f: FaceFrame) {
        // docs/CANNONS.md C4 (owner, 2026-11-17: "It faces the direction of placement (still
        // correctly rotates with dynamic objects)"): a cannon on a raft turned a quarter turn fires
        // along its facing turned with the raft, and the raft recoils the other way.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 60);
        // A north-facing cannon (its barrel −z in the raft's grid) on the deck, then the raft turned
        // a quarter about the face's up.
        let up = up_rows(f);
        let cannon = |part: &str| {
            f.state(dwell_blocks::require(&format!(
                "dwell:cannon[facing=north,part={part}]"
            )))
        };
        let cells = [
            ([2, up, 2], cannon("rear_bottom")),
            ([2, 2 * up, 2], cannon("rear_top")),
            ([2, up, 1], cannon("front_bottom")),
            ([2, 2 * up, 1], cannon("front_top")),
        ];
        let (pose, _) = fx.server.body_pose(raft).unwrap();
        let mut all: Vec<([i32; 3], u16)> = (0..6)
            .flat_map(|x| {
                (0..4).map(move |z| ([x, 0, z], f.state(dwell_blocks::require("dwell:planks"))))
            })
            .collect();
        all.extend(cells);
        let turned = dwell_physics::DQuat::from_axis_angle(
            dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0])),
            std::f64::consts::FRAC_PI_2,
        );
        let ship = fx.server.add_body_turned(1, all, pose.to_array(), turned);
        fx.server.remove_body(raft);
        hold(&mut fx, &mut seq, Input::default(), 60);
        let (_, q) = fx.server.body_pose(ship).unwrap();
        // The barrel as the raft holds it: level (it floats level), a quarter turn from north.
        let barrel = q * dwell_physics::DVec3::new(0.0, 0.0, -1.0);
        let flat = dwell_physics::DVec3::new(barrel.x, 0.0, barrel.z).normalize();
        assert!(barrel.dot(flat) > 0.98, "the raft floats level: {barrel:?}");
        assert!(flat.z.abs() < 0.2, "turned: {flat:?}");
        let v0 = fx.server.body_velocity(ship).unwrap();
        // Stand by the cannon and use it.
        let (p, _) = fx.server.body_pose(ship).unwrap();
        let upv = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let feet = p
            + q * dwell_physics::DVec3::new(4.5, 0.0, 2.5)
            + upv * (0.5 * dwell_world::CELL_H + 0.05);
        stand(&mut fx, feet, upv);
        hold(&mut fx, &mut seq, Input::default(), 10);
        fx.send(
            1,
            Message::BodyEditRequest(BodyEditRequest {
                body: ship,
                cell: [2, up, 1].map(|v| v as i16),
                face: 4,
                action: BlockEditAction::Place,
                material: 0,
            }),
        );
        hold(&mut fx, &mut seq, Input::default(), 2);
        assert_eq!(fx.server.shots_fired(), 1, "fired from the raft");
        let ball = fx.server.last_ball().expect("a ball");
        let v = fx.server.body_velocity(ball).unwrap();
        let along = v.dot(barrel);
        assert!(
            along > 30.0,
            "along the turned barrel: {v:?} barrel {barrel:?}"
        );
        assert!(
            (v - barrel * along).length() < 0.25 * along,
            "not along the raft's grid: {v:?}"
        );
        // The raft kicked back, against the barrel.
        let kick = (fx.server.body_velocity(ship).unwrap() - v0).dot(barrel);
        assert!(kick < -0.05, "recoil {kick}");
    }
);

both_faces!(
    fn a_cannon_placed_on_a_raft_goes_in_whole(f: FaceFrame) {
        // BUGS.md #208: a multi-cell block placed on a moving body put only its first cell there.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 120);
        let (p0, _) = fx.server.body_pose(raft).expect("afloat");
        let up = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let feet =
            p0 + dwell_physics::DVec3::new(4.5, 0.0, 2.5) + up * (0.5 * dwell_world::CELL_H + 0.05);
        stand(&mut fx, feet, up);
        hold(&mut fx, &mut seq, Input::default(), 30);
        let before = fx.server.body_cells(raft).len();
        // On the deck's top at (1, 0, 2), facing north: its barrel runs −z to (1, ·, 1).
        let top = (0..6)
            .find(|&k| dwell_world::edit::FACE_DIRS[k][1] == up_rows(f))
            .unwrap() as u8;
        fx.send(
            1,
            Message::BodyEditRequest(BodyEditRequest {
                body: raft,
                cell: [1, 0, 2],
                face: top,
                action: BlockEditAction::Place,
                material: f.state(dwell_blocks::require(
                    "dwell:cannon[facing=north,part=rear_bottom]",
                )),
            }),
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        let cells = fx.server.body_cells(raft);
        let cannon: Vec<[i32; 3]> = cells
            .iter()
            .filter(|(_, m)| dwell_blocks::block_of(*m).name == "dwell:cannon")
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(cells.len(), before + 4, "four cells added: {cannon:?}");
        let u = up_rows(f);
        for want in [[1, u, 2], [1, 2 * u, 2], [1, u, 1], [1, 2 * u, 1]] {
            assert!(cannon.contains(&want), "{want:?} in {cannon:?}");
        }
    }
);

both_faces!(
    fn a_ball_blows_cells_off_the_raft_it_hits(f: FaceFrame) {
        // The goal (owner, 2026-11-17): "ship battles where cannons can blow off pieces of ships".
        // A raft with an east-facing cannon fires at a second raft 8 m east: cells of that raft are
        // smashed or knocked off, and it is still afloat with the rest.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        fx.server.remove_body(raft);
        let up = up_rows(f);
        let planks = f.state(dwell_blocks::require("dwell:planks"));
        let deck = |w: i32, d: i32| -> Vec<([i32; 3], u16)> {
            (0..w)
                .flat_map(|x| (0..d).map(move |z| ([x, 0, z], planks)))
                .collect()
        };
        let cannon = |part: &str| {
            f.state(dwell_blocks::require(&format!(
                "dwell:cannon[facing=east,part={part}]"
            )))
        };
        let mut a = deck(4, 4);
        a.extend([
            ([1, 2 * up, 1], cannon("rear_bottom")),
            ([1, 3 * up, 1], cannon("rear_top")),
            ([2, 2 * up, 1], cannon("front_bottom")),
            ([2, 3 * up, 1], cannon("front_top")),
        ]);
        // Raft B: a deck with a 2-high wall of planks along its west side, in the line of fire.
        let mut b = deck(4, 4);
        for z in 0..4 {
            for r in 2..4 {
                b.push(([0, r * up, z], planks));
            }
        }
        let c = f.cell([0, BASE, 0]);
        let y =
            f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
        let ra = fx.server.add_body(1, a, [0.5, y, 0.5]);
        let rb = fx.server.add_body(1, b, [12.5, y, 0.5]);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 90);
        let before = fx.server.body_cells(rb).len();
        assert!(fx.server.use_body_cell(1, ra, [2, 2 * up, 1]), "fired");
        hold(&mut fx, &mut seq, Input::default(), 60);
        let (smashed, loosened) = fx.server.ball_hits();
        let after = fx.server.body_cells(rb).len();
        assert!(smashed + loosened >= 1, "hit {smashed} / {loosened}");
        assert!(after < before, "raft B lost cells: {before} -> {after}");
        assert!(fx.server.body_pose(rb).is_some(), "raft B still afloat");
    }
);

both_faces!(
    fn blocks_placed_on_a_log_raft_join_it(f: FaceFrame) {
        // BUGS.md #208 (owner, 2026-11-18: "It was a log raft, I tried to place any blocks on it"):
        // a built raft of logs lying along x, afloat; a block placed on a log's top joins it.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        fx.server.remove_body(raft);
        let log = f.state(dwell_blocks::require(
            "dwell:log[axis=x,joined=auto,width=4]",
        ));
        let cells: Vec<([i32; 3], u16)> = (0..5)
            .flat_map(|x| (0..4).map(move |z| ([x, 0, z], log)))
            .collect();
        let c = f.cell([0, BASE, 0]);
        let up = up_rows(f);
        let y =
            f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
        let r = fx.server.add_body(1, cells, [0.5, y, 0.5]);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 120);
        let (p0, _) = fx.server.body_pose(r).expect("afloat");
        let upv = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let feet = p0
            + dwell_physics::DVec3::new(3.5, 0.0, 1.5)
            + upv * (0.5 * dwell_world::CELL_H + 0.05);
        stand(&mut fx, feet, upv);
        hold(&mut fx, &mut seq, Input::default(), 30);
        let before = fx.server.body_cells(r).len();
        let top = (0..6)
            .find(|&k| dwell_world::edit::FACE_DIRS[k][1] == up)
            .unwrap() as u8;
        fx.send(
            1,
            Message::BodyEditRequest(BodyEditRequest {
                body: r,
                cell: [1, 0, 2],
                face: top,
                action: BlockEditAction::Place,
                material: f.state(dwell_blocks::require("dwell:planks")),
            }),
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        assert_eq!(
            fx.server.body_cells(r).len(),
            before + 1,
            "the plank joined the log raft"
        );
    }
);

both_faces!(
    fn felled_logs_afloat_take_blocks_and_become_a_raft_on_land_they_dont(f: FaceFrame) {
        // BUGS.md #208; owner, 2026-11-18: natural bodies take placed blocks "Only floating ones".
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        fx.server.remove_body(raft);
        let up = up_rows(f);
        let log = f.state(dwell_blocks::require(
            "dwell:log[axis=x,joined=auto,width=4]",
        ));
        let logs: Vec<([i32; 3], u16)> = (0..4)
            .flat_map(|x| (0..3).map(move |z| ([x, 0, z], log)))
            .collect();
        let c = f.cell([0, BASE, 0]);
        let y =
            f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
        // Natural (kind 0): one afloat in the pool, one on the stone shore past it.
        let afloat = fx.server.add_body(0, logs.clone(), [0.5, y, 0.5]);
        let shore = f.cell([24, BASE - 7, 0]);
        for x in 22..30 {
            for z in -2..6 {
                for r in -7..-6 {
                    let s = f.cell([x, BASE + r, z]);
                    fx.server.world().set_voxel(
                        s[0],
                        s[1],
                        s[2],
                        f.state(dwell_blocks::require("dwell:stone")),
                    );
                }
            }
        }
        let on_land = fx.server.add_body(
            0,
            logs,
            [
                f64::from(shore[0]) + 0.5,
                (f64::from(shore[1]) + if up > 0 { 1.5 } else { -0.5 }) * dwell_world::CELL_H,
                f64::from(shore[2]) + 0.5,
            ],
        );
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 20);
        let top = (0..6)
            .find(|&k| dwell_world::edit::FACE_DIRS[k][1] == up)
            .unwrap() as u8;
        let plank = f.state(dwell_blocks::require("dwell:planks"));
        let try_place = |fx: &mut Fixture, seq: &mut u32, body: u32| {
            let (p, _) = fx.server.body_pose(body).expect("a body");
            let upv = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
            stand(
                fx,
                p + dwell_physics::DVec3::new(2.5, 0.0, 1.0)
                    + upv * (0.5 * dwell_world::CELL_H + 0.05),
                upv,
            );
            hold(fx, seq, Input::default(), 10);
            let before = fx.server.body_cells(body).len();
            fx.send(
                1,
                Message::BodyEditRequest(BodyEditRequest {
                    body,
                    cell: [1, 0, 1],
                    face: top,
                    action: BlockEditAction::Place,
                    material: plank,
                }),
            );
            hold(fx, seq, Input::default(), 3);
            fx.server.body_cells(body).len() - before
        };
        assert_eq!(
            try_place(&mut fx, &mut seq, afloat),
            1,
            "afloat: the plank joined"
        );
        assert!(
            fx.server.body_pose(on_land).is_some(),
            "the logs on land are still a body"
        );
        {
            assert_eq!(
                try_place(&mut fx, &mut seq, on_land),
                0,
                "on land: break-only"
            );
        }
    }
);

/// How far (m) a plank boat `w` × `d` cells and `layers` deep, its helm facing north on its deck,
/// sails in 3 s at the helm from still (`harbour`'s pool; its own raft taken away).
fn sails_in_3_s(f: FaceFrame, w: i32, d: i32, layers: i32) -> f64 {
    let mut fx = Fixture::new(config(f));
    fx.join(1, &Client::new(1));
    let (raft, _) = harbour(&mut fx, f);
    fx.server.remove_body(raft);
    let up = up_rows(f);
    let planks = f.state(dwell_blocks::require("dwell:planks"));
    let mut cells: Vec<([i32; 3], u16)> = Vec::new();
    for x in 0..w {
        for z in 0..d {
            for l in 0..layers {
                cells.push(([x, l * up, z], planks));
            }
        }
    }
    let helm = [w / 2, layers * up, d - 2];
    cells.push((
        helm,
        f.state(dwell_blocks::require("dwell:helm[facing=north]")),
    ));
    let c = f.cell([0, BASE, 0]);
    let y = f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
    let boat = fx.server.add_body(1, cells, [0.5, y, 0.5]);
    let mut seq = 0;
    hold(&mut fx, &mut seq, Input::default(), 120);
    let (p0, q) = fx.server.body_pose(boat).expect("afloat");
    let upv = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
    let off = dwell_physics::DVec3::new(
        f64::from(w / 2),
        f64::from(layers - 1) * dwell_world::CELL_H * f64::from(up),
        f64::from(d - 1),
    );
    stand(
        &mut fx,
        p0 + q * off + upv * (0.5 * dwell_world::CELL_H + 0.05),
        upv,
    );
    hold(&mut fx, &mut seq, Input::default(), 20);
    assert!(fx.server.use_body_cell(1, boat, helm), "the helm");
    hold(&mut fx, &mut seq, Input::default(), 3);
    let (a, _) = fx.server.body_pose(boat).unwrap();
    hold(
        &mut fx,
        &mut seq,
        Input {
            move_y: 1.0,
            ..Input::default()
        },
        180,
    );
    let (b, _) = fx.server.body_pose(boat).unwrap();
    a.z - b.z
}

both_faces!(
    fn a_big_boat_sails_about_as_fast_as_a_small_raft(f: FaceFrame) {
        // Owner, 2026-11-18: "The movement force should be scaled based on the mass of the boat, so
        // big boats can move at a reasonable speed too." 4 × 4 × 1 against 8 × 12 × 2 (6× the mass).
        let small = sails_in_3_s(f, 4, 4, 1);
        let big = sails_in_3_s(f, 8, 12, 2);
        assert!(small > 1.0, "the raft sails: {small}");
        assert!(
            big > 0.7 * small,
            "the big boat keeps up: {big} against {small}"
        );
    }
);

/// Where (face-local m) the floor of plank body `cells` rides once afloat in `harbour`'s pool.
fn floor_height(f: FaceFrame, cells: Vec<([i32; 3], u16)>) -> f64 {
    let mut fx = Fixture::new(config(f));
    fx.join(1, &Client::new(1));
    let (raft, _) = harbour(&mut fx, f);
    fx.server.remove_body(raft);
    let up = up_rows(f);
    let c = f.cell([0, BASE, 0]);
    let y = f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
    let body = fx.server.add_body(1, cells, [0.5, y, 0.5]);
    let mut seq = 0;
    hold(&mut fx, &mut seq, Input::default(), 300);
    let (p, _) = fx.server.body_pose(body).expect("afloat");
    f.point(p.to_array())[1]
}

both_faces!(
    fn an_open_hulls_air_floats_it_higher_than_the_same_hull_holed(f: FaceFrame) {
        // docs/CANNONS.md §12 H1; owner, 2026-11-18: "water filling the hull, which would also make
        // boats more buoyant? I think based on air inside?" The holed hull is a plank lighter, yet
        // without its air it rides lower.
        let up = up_rows(f);
        let planks = f.state(dwell_blocks::require("dwell:planks"));
        let mut hull = Vec::new();
        for x in 0..5 {
            for z in 0..4 {
                hull.push(([x, 0, z], planks));
                if x == 0 || z == 0 || x == 4 || z == 3 {
                    hull.push(([x, up, z], planks));
                    hull.push(([x, 2 * up, z], planks));
                }
            }
        }
        let holed: Vec<_> = hull
            .iter()
            .copied()
            .filter(|&(c, _)| c != [0, up, 1])
            .collect();
        let (h, w) = (floor_height(f, hull), floor_height(f, holed));
        assert!(
            h > w + 0.05,
            "the sealed hull's floor rides higher: {h} against the holed one's {w}"
        );
    }
);

both_faces!(
    fn a_holed_hull_floods_sinks_and_settles_into_the_world(f: FaceFrame) {
        // docs/CANNONS.md §12 H2 (owner, 2026-11-18: water comes in "Through holes, by hole size";
        // a sunk ship "Settles into the world"). A sealed plank hull afloat; a hole in its floor.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        fx.server.remove_body(raft);
        let up = up_rows(f);
        // Stone so that its air alone keeps it up: a stone hull sinks once flooded.
        let stone = f.state(dwell_blocks::require("dwell:stone"));
        let planks = f.state(dwell_blocks::require("dwell:planks"));
        // 8 × 8, walls 3 high, a third of its cells stone (~600 kg a cell): sealed, its air keeps it
        // under water's density (148 cells and 108 of air); flooded, it is over it.
        let mut hull = Vec::new();
        let mut k = 0;
        let next = |k: &mut i32| {
            *k += 1;
            if *k % 3 == 0 { stone } else { planks }
        };
        for x in 0..8 {
            for z in 0..8 {
                hull.push(([x, 0, z], next(&mut k)));
                if x == 0 || z == 0 || x == 7 || z == 7 {
                    for r in 1..4 {
                        hull.push(([x, r * up, z], next(&mut k)));
                    }
                }
            }
        }
        let c = f.cell([0, BASE, 0]);
        let y =
            f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
        let body = fx.server.add_body(1, hull, [0.5, y, 0.5]);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 240);
        let floor = |fx: &Fixture| {
            fx.server
                .body_pose(body)
                .map(|(p, _)| f.point(p.to_array())[1])
        };
        let afloat = floor(&fx).expect("afloat");
        // Its rim (3 rows over the floor's top) clears the water.
        let rim = afloat + 0.5 * dwell_world::CELL_H + 3.0 * dwell_world::CELL_H;
        assert!(rim > m_of_row(BASE), "sealed, it floats: its rim at {rim}");
        // A hole in its floor.
        assert!(fx.server.break_body_cell(body, [3, 0, 3]), "holed");
        hold(&mut fx, &mut seq, Input::default(), 600);
        let flooding = floor(&fx).expect("still a body");
        assert!(
            flooding < afloat - 0.1,
            "it rides lower as it floods: {afloat} -> {flooding}"
        );
        // In time it goes down and settles on the bed as blocks of the world.
        for _ in 0..40 {
            hold(&mut fx, &mut seq, Input::default(), 60);
            if floor(&fx).is_none() {
                break;
            }
        }
        assert!(floor(&fx).is_none(), "it settled: {:?}", floor(&fx));
        let mut planks_on_bed = 0;
        for x in -4..10 {
            for z in -4..10 {
                for r in -7..0 {
                    let w = f.cell([x, BASE + r, z]);
                    if fx.server.world().get_voxel(w[0], w[1], w[2]) == planks {
                        planks_on_bed += 1;
                    }
                }
            }
        }
        assert!(
            planks_on_bed > 20,
            "its planks lie in the pool: {planks_on_bed}"
        );
    }
);

/// A plank raft afloat with player 1 standing on it beside (1, ·, 2): the fixture, the raft, its
/// top face's index and the input sequence.
fn raft_to_build_on(f: FaceFrame) -> (Fixture, u32, u8, u32) {
    let mut fx = Fixture::new(config(f));
    fx.join(1, &Client::new(1));
    let (raft, _) = harbour(&mut fx, f);
    let mut seq = 0;
    hold(&mut fx, &mut seq, Input::default(), 120);
    let (p0, _) = fx.server.body_pose(raft).expect("afloat");
    let up = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
    stand(
        &mut fx,
        p0 + dwell_physics::DVec3::new(4.5, 0.0, 2.5) + up * (0.5 * dwell_world::CELL_H + 0.05),
        up,
    );
    hold(&mut fx, &mut seq, Input::default(), 30);
    let top = (0..6)
        .find(|&k| dwell_world::edit::FACE_DIRS[k][1] == up_rows(f))
        .unwrap() as u8;
    (fx, raft, top, seq)
}

fn send_body_edit(
    fx: &mut Fixture,
    body: u32,
    cell: [i16; 3],
    face: u8,
    action: BlockEditAction,
    material: u16,
) {
    fx.send(
        1,
        Message::BodyEditRequest(BodyEditRequest {
            body,
            cell,
            face,
            action,
            material,
        }),
    );
}

both_faces!(
    fn breaking_one_cell_of_a_cannon_on_a_raft_breaks_the_whole_cannon(f: FaceFrame) {
        // BUGS.md #210 (owner, 2026-11-18: "Breaking one cannon block, should break the whole cannon
        // not 1 of the 4 blocks"; "on a dynamic body").
        let (mut fx, raft, top, mut seq) = raft_to_build_on(f);
        let cannon = f.state(dwell_blocks::require(
            "dwell:cannon[facing=north,part=rear_bottom]",
        ));
        send_body_edit(
            &mut fx,
            raft,
            [1, 0, 2],
            top,
            BlockEditAction::Place,
            cannon,
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        let is_cannon = |fx: &Fixture| {
            fx.server
                .body_cells(raft)
                .iter()
                .filter(|(_, m)| dwell_blocks::block_of(*m).name == "dwell:cannon")
                .count()
        };
        assert_eq!(is_cannon(&fx), 4, "placed whole");
        // Its front top cell hit until it breaks.
        let u = up_rows(f);
        for _ in 0..40 {
            send_body_edit(
                &mut fx,
                raft,
                [1, 2 * u as i16, 1],
                top,
                BlockEditAction::Break,
                0,
            );
            hold(&mut fx, &mut seq, Input::default(), 8);
            if is_cannon(&fx) < 4 {
                break;
            }
        }
        assert_eq!(is_cannon(&fx), 0, "the whole cannon went");
    }
);

both_faces!(
    fn a_torch_placed_on_a_raft_stands_there_whole(f: FaceFrame) {
        // BUGS.md #212 (owner, 2026-11-18: "Placing torches on dynamic bodies should work too").
        let (mut fx, raft, top, mut seq) = raft_to_build_on(f);
        let torch = f.state(dwell_blocks::require("dwell:torch"));
        send_body_edit(&mut fx, raft, [2, 0, 1], top, BlockEditAction::Place, torch);
        hold(&mut fx, &mut seq, Input::default(), 3);
        let names: Vec<&str> = fx
            .server
            .body_cells(raft)
            .iter()
            .map(|(_, m)| dwell_blocks::block_of(*m).name)
            .filter(|n| n.starts_with("dwell:torch"))
            .collect();
        assert_eq!(names.len(), 2, "the torch and its upper: {names:?}");
    }
);

both_faces!(
    fn a_cannonball_in_water_sinks_and_rests_flooded_on_the_bed(f: FaceFrame) {
        // BUGS.md #213 (owner, 2026-11-18: "Cannonballs shouldn't settle on the water surface or
        // float, they should sink and settle flooded on the bottom").
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        fx.server.remove_body(raft);
        let ball = f.state(dwell_blocks::require("dwell:iron_ball[flooded=false]"));
        let c = f.cell([4, BASE, 4]);
        let at = [
            f64::from(c[0]) + 0.5,
            (f64::from(c[1]) + 0.5) * dwell_world::CELL_H,
            f64::from(c[2]) + 0.5,
        ];
        let id = fx.server.add_body(0, vec![([0, 0, 0], ball)], at);
        let mut seq = 0;
        for _ in 0..40 {
            hold(&mut fx, &mut seq, Input::default(), 30);
            if fx.server.body_pose(id).is_none() {
                break;
            }
        }
        assert!(fx.server.body_pose(id).is_none(), "it settled");
        // On the bed (the pool's lowest water row), flooded; the water above it still there.
        let bed = f.cell([4, BASE - 6, 4]);
        let m = fx.server.world().get_voxel(bed[0], bed[1], bed[2]);
        assert_eq!(
            dwell_blocks::block_of(m).name,
            "dwell:iron_ball",
            "the ball on the bed"
        );
        assert_eq!(
            dwell_blocks::state_property(m, "flooded"),
            Some("true"),
            "flooded"
        );
        let above = f.cell([4, BASE - 5, 4]);
        let w = fx.server.world().get_voxel(above[0], above[1], above[2]);
        assert!(dwell_blocks::info(w).liquid, "the water above it");
    }
);

/// The boat's speed (m/s) after `secs` s at the helm from still and its heading's turn (degrees)
/// in 2 s more with the rudder hard over: a `w` × `d` × 2 plank boat in `harbour`'s pool.
fn helm_figures(f: FaceFrame, secs: u32, w: i32, d: i32) -> (f64, f64) {
    let mut fx = Fixture::new(config(f));
    fx.join(1, &Client::new(1));
    let (raft, _) = harbour(&mut fx, f);
    fx.server.remove_body(raft);
    let up = up_rows(f);
    let planks = f.state(dwell_blocks::require("dwell:planks"));
    let mut cells: Vec<([i32; 3], u16)> = Vec::new();
    for x in 0..w {
        for z in 0..d {
            cells.push(([x, 0, z], planks));
            cells.push(([x, up, z], planks));
        }
    }
    let helm = [w / 2, 2 * up, d - 2];
    cells.push((
        helm,
        f.state(dwell_blocks::require("dwell:helm[facing=north]")),
    ));
    let c = f.cell([0, BASE, 0]);
    let y = f64::from(c[1]) * dwell_world::CELL_H + if up > 0 { 0.0 } else { dwell_world::CELL_H };
    let boat = fx.server.add_body(1, cells, [0.5, y, 0.5]);
    let mut seq = 0;
    hold(&mut fx, &mut seq, Input::default(), 120);
    let (p0, q) = fx.server.body_pose(boat).expect("afloat");
    let upv = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
    stand(
        &mut fx,
        p0 + q * dwell_physics::DVec3::new(2.0, 0.5 * f64::from(up), 9.0) + upv * 0.3,
        upv,
    );
    hold(&mut fx, &mut seq, Input::default(), 20);
    assert!(fx.server.use_body_cell(1, boat, helm), "the helm");
    hold(
        &mut fx,
        &mut seq,
        Input {
            move_y: 1.0,
            ..Input::default()
        },
        secs * 60,
    );
    let speed = fx.server.body_velocity(boat).unwrap().length();
    let h0 = heading(&fx, boat);
    hold(
        &mut fx,
        &mut seq,
        Input {
            move_x: 1.0,
            move_y: 1.0,
            ..Input::default()
        },
        120,
    );
    let h1 = heading(&fx, boat);
    (speed, h0.angle_between(h1).to_degrees())
}

#[test]
#[ignore = "a measurement: `cargo test -p dwell-sim --test ships helm_figures -- --ignored --nocapture`"]
fn measure_helm_figures() {
    // face-a-only: a measurement print.
    let (v, turn) = helm_figures(FaceFrame::A, 3, 6, 10);
    eprintln!("speed after 3 s {v:.2} m/s; turn in 2 s {turn:.1} degrees");
}

both_faces!(
    fn a_small_and_a_big_raft_turn_alike_at_the_helm(f: FaceFrame) {
        // Owner, 2026-11-18: "a 3x3 rafter turns very quickly ... and a 7x7 raft turns super slowly.
        // Perhaps it just needs to be scaled too?" Both are pushed by their mass (the same speed) and
        // turned the same rate at it.
        let (vs, small) = helm_figures(f, 2, 3, 3);
        let (vb, big) = helm_figures(f, 2, 7, 7);
        assert!(
            small > 3.0 && big > 3.0,
            "both turn: {small} and {big} degrees"
        );
        assert!(
            small < 2.0 * big && big < 2.0 * small,
            "alike: {small} and {big} degrees ({vs} and {vb} m/s)"
        );
    }
);

both_faces!(
    fn at_the_helm_nothing_is_built_or_broken_but_a_cannon_still_fires(f: FaceFrame) {
        // Owner, 2026-11-18: "block breaking/building should be disabled while steering the ship.
        // But "use" should still be allowed like currently."
        let (mut fx, raft, top, mut seq) = raft_to_build_on(f);
        let cannon = f.state(dwell_blocks::require(
            "dwell:cannon[facing=north,part=rear_bottom]",
        ));
        send_body_edit(
            &mut fx,
            raft,
            [1, 0, 2],
            top,
            BlockEditAction::Place,
            cannon,
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        // The harbour raft's helm is at (3, up, 1).
        let u = up_rows(f) as i16;
        send_body_edit(&mut fx, raft, [3, u, 1], top, BlockEditAction::Place, 0);
        hold(&mut fx, &mut seq, Input::default(), 3);
        assert_eq!(fx.server.at_helms(), vec![(1, raft)], "at the helm");
        let before = fx.server.body_cells(raft).len();
        let planks = f.state(dwell_blocks::require("dwell:planks"));
        send_body_edit(
            &mut fx,
            raft,
            [5, 0, 3],
            top,
            BlockEditAction::Place,
            planks,
        );
        for _ in 0..10 {
            send_body_edit(&mut fx, raft, [0, 0, 0], top, BlockEditAction::Break, 0);
            hold(&mut fx, &mut seq, Input::default(), 6);
        }
        assert_eq!(
            fx.server.body_cells(raft).len(),
            before,
            "nothing built or broken"
        );
        send_body_edit(&mut fx, raft, [1, 2 * u, 1], top, BlockEditAction::Place, 0);
        hold(&mut fx, &mut seq, Input::default(), 3);
        assert_eq!(fx.server.shots_fired(), 1, "the cannon fired");
    }
);

both_faces!(
    fn a_ball_taking_one_cannon_cell_takes_the_whole_cannon(f: FaceFrame) {
        // BUGS.md #216 (owner, 2026-11-18: "Also ensure you check every one of the 4 cannon blocks to
        // see if it is destroyed. I noticed half of cannons fall off the boat").
        let (mut fx, raft, top, mut seq) = raft_to_build_on(f);
        let cannon = f.state(dwell_blocks::require(
            "dwell:cannon[facing=north,part=rear_bottom]",
        ));
        send_body_edit(
            &mut fx,
            raft,
            [1, 0, 2],
            top,
            BlockEditAction::Place,
            cannon,
        );
        hold(&mut fx, &mut seq, Input::default(), 3);
        let count = |fx: &Fixture| {
            fx.server
                .body_cells(raft)
                .iter()
                .filter(|(_, m)| dwell_blocks::block_of(*m).name == "dwell:cannon")
                .count()
        };
        assert_eq!(count(&fx), 4);
        // One cell knocked out as a ball's hit does (its front top).
        let u = up_rows(f);
        assert!(fx.server.break_body_cell(raft, [1, 2 * u, 1]));
        assert_eq!(count(&fx), 0, "no part of the cannon left on the raft");
    }
);

both_faces!(
    fn a_block_placed_on_open_water_floats_a_new_raft_one_by_the_shore_stays_land(f: FaceFrame) {
        // Owner, 2026-11-18: a boat is "Built on water"; the first block "Aim at the surface", in
        // "Both modes"; "Water should be last in priority" (a block under water still builds).
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        let (raft, _) = harbour(&mut fx, f);
        fx.server.remove_body(raft);
        let up = up_rows(f);
        let top = (0..6)
            .find(|&k| dwell_world::edit::FACE_DIRS[k][1] == up)
            .unwrap() as u8;
        let stone = f.state(dwell_blocks::require("dwell:stone"));
        let plank = f.state(dwell_blocks::require("dwell:planks"));
        // A stone shore west of the pool, its top level with the water's.
        for x in -18..-12 {
            for z in -3..4 {
                for r in -7..0 {
                    let c = f.cell([x, BASE + r, z]);
                    fx.server.world().set_voxel(c[0], c[1], c[2], stone);
                }
            }
        }
        let upv = dwell_physics::DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let feet = dwell_physics::DVec3::from(f.point([-13.5, m_of_row(BASE), 0.5]));
        stand(&mut fx, feet, upv);
        let mut seq = 0;
        hold(&mut fx, &mut seq, Input::default(), 20);
        let place = |fx: &mut Fixture, cell: [i32; 3], face: u8| {
            fx.send(
                1,
                Message::BlockEditRequest(dwell_protocol::BlockEditRequest {
                    action: BlockEditAction::Place,
                    cell,
                    face,
                    material: plank,
                    rows: 1,
                    down: false,
                    point: None,
                }),
            );
            for _ in 0..3 {
                fx.server.step();
            }
        };
        let bodies = |fx: &Fixture| {
            serde_json::from_str::<Vec<serde_json::Value>>(&fx.server.bodies_report())
                .unwrap()
                .len()
        };
        // Onto the water's surface 3 m out: a new body, nothing in the world there.
        place(&mut fx, f.cell([-10, BASE - 1, 0]), top);
        assert_eq!(bodies(&fx), 1, "a raft started");
        let above = f.cell([-10, BASE, 0]);
        assert_eq!(
            fx.server.world().get_voxel(above[0], above[1], above[2]),
            0,
            "not in the world"
        );
        // Let it settle, riding low (its grid offset no longer a whole row off the surface).
        hold(&mut fx, &mut seq, Input::default(), 180);
        // Onto the surface beside it: into that raft.
        place(&mut fx, f.cell([-9, BASE - 1, 0]), top);
        assert_eq!(bodies(&fx), 1, "joined, no second raft");
        let cells: Vec<serde_json::Value> =
            serde_json::from_str(&fx.server.bodies_report()).unwrap();
        assert_eq!(cells[0]["cells"].as_array().unwrap().len(), 2, "two cells");
        // Onto the surface by the shore: a pier, in the world.
        place(&mut fx, f.cell([-12, BASE - 1, 1]), top);
        let pier = f.cell([-12, BASE, 1]);
        assert_eq!(bodies(&fx), 1, "no new body by the shore");
        assert_eq!(
            fx.server.world().get_voxel(pier[0], pier[1], pier[2]),
            plank,
            "the pier is land"
        );
        // On a stone under water: an ordinary block in the world, in the water (water last).
        let rock = f.cell([-11, BASE - 3, -1]);
        fx.server
            .world()
            .set_voxel(rock[0], rock[1], rock[2], stone);
        place(&mut fx, rock, top);
        let under = f.cell([-11, BASE - 2, -1]);
        assert_eq!(
            fx.server.world().get_voxel(under[0], under[1], under[2]),
            plank,
            "built under water"
        );
        assert_eq!(bodies(&fx), 1);
    }
);
