//! The land cannon fires (docs/CANNONS.md C1; owner, 2026-11-17: "using the same input as doors, you
//! can fire the cannon, clicking on it"): a Use of any of its cells fires it once, then not again
//! until its data's `reloadS` has passed. Both faces.

mod common;

use common::*;
use dwell_protocol::{BlockEditAction, BlockEditRequest, Message, SIM_HZ};
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, both_faces, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        // Ground written by hand into an empty world: all of it "placed".
        placed_blocks_fall: false,
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        ..flat()
    }
}

/// A stone floor and a north-facing cannon at x = 2 (its rear at z 1, its barrel to z 0): its cells.
fn cannon_scene(fx: &mut Fixture, f: FaceFrame) -> [[i32; 3]; 4] {
    let stone = f.state(dwell_blocks::require("dwell:stone"));
    for x in -6..8 {
        for z in -160..8 {
            for r in -2..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    let parts = [
        ("rear_bottom", 1, 0),
        ("rear_top", 1, 1),
        ("front_bottom", 0, 0),
        ("front_top", 0, 1),
    ];
    parts.map(|(part, z, up)| {
        let c = f.cell([2, BASE + up, z]);
        let m = f.state(dwell_blocks::require(&format!(
            "dwell:cannon[facing=north,part={part}]"
        )));
        fx.server.world().set_voxel(c[0], c[1], c[2], m);
        c
    })
}

fn use_it(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3]) {
    use_face(fx, f, cell, 1);
}

/// A Use of `cell` through its face `face` (face-local: 1 −x, 5 −z), as seen from the player.
fn use_face(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3], face: usize) {
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell,
            face: f.cell_face(face) as u8,
            material: dwell_blocks::require("dwell:dirt"),
            rows: 1,
            down: false,
            point: None,
        }),
    );
    for _ in 0..4 {
        fx.server.step();
    }
}

both_faces!(
    fn a_cannon_fires_on_every_use(f: FaceFrame) {
        // No fire-rate cap (owner, 2026-11-18: "set that to off or none, so you can fire quickly").
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        let cells = cannon_scene(&mut fx, f);
        // Any of its cells is the cannon, each Use a shot, ticks apart.
        for (k, c) in [cells[2], cells[1], cells[3]].into_iter().enumerate() {
            use_it(&mut fx, f, c);
            assert_eq!(fx.server.shots_fired(), k as u64 + 1);
        }
        // A Use writes nothing.
        for c in cells {
            assert_ne!(fx.server.world().get_voxel(c[0], c[1], c[2]), 0);
        }
    }
);

both_faces!(
    fn a_shot_launches_a_heavy_ball_along_the_barrel(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        let cells = cannon_scene(&mut fx, f);
        use_it(&mut fx, f, cells[0]);
        let id = fx.server.last_ball().expect("a ball fired");
        let (p0, _) = fx.server.body_pose(id).unwrap();
        let l0 = f.point(p0.to_array());
        // At the muzzle: in line with the barrel (x 2.5), beyond its front (z 0), ~0.6 m up.
        assert!((l0[0] - 2.5).abs() < 0.05, "{l0:?}");
        assert!(l0[2] < 0.0, "{l0:?}");
        // Its body weighs the ball's mass.
        let h = fx.server.body_handle(id).unwrap();
        let (_, physics, _) = fx.server.sim();
        let mass = physics.mass(h);
        let want = dwell_world::cannons::ball("dwell:iron_ball")
            .unwrap()
            .mass_kg();
        assert!((mass - want).abs() < 1.0, "mass {mass} want {want}");
        // It leaves rising at the barrel's elevation (owner, 2026-11-18: "angled up a bit").
        let v = fx.server.body_velocity(id).unwrap();
        let vl = f.vector(v.to_array());
        let elevation = vl[1]
            .atan2((vl[0] * vl[0] + vl[2] * vl[2]).sqrt())
            .to_degrees();
        let want = dwell_world::cannons::cannon("dwell:cannon")
            .unwrap()
            .elevation_deg;
        assert!(
            (elevation - want).abs() < 1.5,
            "fired at {elevation} degrees, want {want}"
        );
        // It flies north (−z) fast.
        for _ in 0..6 {
            fx.server.step();
        }
        let l1 = f.point(fx.server.body_pose(id).unwrap().0.to_array());
        assert!(l0[2] - l1[2] > 3.0, "{l0:?} -> {l1:?}");
        // And lands on the floor north of the cannon, rolling on along it, not through it (its arc
        // up at the barrel's elevation lasts about a second).
        for _ in 0..80 {
            fx.server.step();
        }
        let l2 = f.point(fx.server.body_pose(id).unwrap().0.to_array());
        assert!(l2[2] < -10.0, "{l2:?}");
        assert!((l2[1] - m_of_row(BASE)).abs() < 0.3, "on the floor: {l2:?}");
    }
);

both_faces!(
    fn a_ball_rolls_to_rest_not_on_for_hundreds_of_metres(f: FaceFrame) {
        // Its data's `rollDamping`: a sphere's contact alone rolled it on past 200 m (the browser,
        // 2026-11-17: no ball within 200 m of the cannon).
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        let cells = cannon_scene(&mut fx, f);
        use_it(&mut fx, f, cells[0]);
        let id = fx.server.last_ball().expect("a ball fired");
        let mut last = None;
        for _ in 0..(8 * SIM_HZ) {
            fx.server.step();
            if let Some((p, _)) = fx.server.body_pose(id) {
                last = Some(f.point(p.to_array()));
            }
        }
        let at = last.expect("it flew");
        // At rest (settled into the grid, its body gone) or nearly, well within the floor.
        let moving = fx.server.body_pose(id).is_some_and(|(p, _)| {
            let now = f.point(p.to_array());
            fx.server.step();
            let next = f.point(
                fx.server
                    .body_pose(id)
                    .map_or(p.to_array(), |q| q.0.to_array()),
            );
            (next[2] - now[2]).abs() * f64::from(SIM_HZ) > 0.5
        });
        assert!(!moving, "still rolling at {at:?}");
        // Landing fast on the flat at the barrel's elevation it plows a few cells where it strikes
        // (a block hit at 20 m/s or more is destroyed, the owner's rule); rolling breaks none.
        let (smashed, loosened) = fx.server.ball_hits();
        assert!(
            smashed + loosened <= 6,
            "only where it landed: {smashed}, {loosened}"
        );
        assert!(at[2] > -150.0 && at[2] < -10.0, "rested at {at:?}");
    }
);

/// A stone brick wall across the barrel's path at face-local z = `WALL_Z` and the `deep` - 1 rows of
/// z behind it, 5 rows tall.
const WALL_Z: i32 = -8;
fn wall(fx: &mut Fixture, f: FaceFrame, deep: i32) -> Vec<[i32; 3]> {
    let brick = f.state(dwell_blocks::require("dwell:stone_brick"));
    let mut cells = Vec::new();
    for x in -2..8 {
        for r in 0..5 {
            for d in 0..deep {
                let c = f.cell([x, BASE + r, WALL_Z - d]);
                fx.server.world().set_voxel(c[0], c[1], c[2], brick);
                cells.push(c);
            }
        }
    }
    cells
}

fn fire_at_wall(f: FaceFrame, deep: i32) -> (Fixture, Vec<[i32; 3]>, u32) {
    let mut fx = Fixture::new(config(f));
    fx.join(1, &Client::new(1));
    for _ in 0..10 {
        fx.server.step();
    }
    let cells = cannon_scene(&mut fx, f);
    let wall = wall(&mut fx, f, deep);
    use_it(&mut fx, f, cells[0]);
    let id = fx.server.last_ball().expect("a ball fired");
    for _ in 0..60 {
        fx.server.step();
    }
    (fx, wall, id)
}

both_faces!(
    fn a_fast_ball_smashes_through_and_loosens_the_blocks_round_its_hole(f: FaceFrame) {
        // Owner, 2026-11-18: "Every time the cannonball hits a block at greater than a certain
        // velocity, it should destroy it"; "detaches neighboring blocks from their structures. 1m
        // radius". A 1-block wall at 45 m/s: smashed through, the cells round the hole loose.
        let (mut fx, wall, _) = fire_at_wall(f, 1);
        let (smashed, loosened) = fx.server.ball_hits();
        assert!(smashed >= 1, "smashed {smashed}");
        assert!(loosened >= 1, "loosened {loosened}");
        let gone =
            |fx: &mut Fixture, c: [i32; 3]| fx.server.world().get_voxel(c[0], c[1], c[2]) == 0;
        assert!(
            gone(&mut fx, f.cell([2, BASE + 1, WALL_Z])),
            "the cell in its path"
        );
        // Only round the hole: the wall's far ends stand.
        assert!(
            !gone(&mut fx, f.cell([-2, BASE + 1, WALL_Z]))
                && !gone(&mut fx, f.cell([7, BASE + 1, WALL_Z]))
        );
        let left = wall.iter().filter(|&&c| !gone(&mut fx, c)).count();
        assert!(
            left + 10 < wall.len() + 10 && left > wall.len() / 2,
            "{left} of {} left",
            wall.len()
        );
    }
);

both_faces!(
    fn a_thick_wall_slows_the_ball_by_the_weight_it_smashes_until_it_knocks_loose(f: FaceFrame) {
        // "By the block's weight": every brick it destroys costs its energy; under 20 m/s it knocks
        // the next loose instead. A wall 12 bricks deep stops it inside.
        let (fx, _, id) = fire_at_wall(f, 12);
        let (smashed, _) = fx.server.ball_hits();
        let ball = dwell_world::cannons::ball("dwell:iron_ball").unwrap();
        let brick = dwell_blocks::info(dwell_blocks::require("dwell:stone_brick"));
        let kg = f64::from(brick.density_kg_m3) * dwell_world::CELL_H;
        let (v0, v1) = (45.0f64, ball.impact.destroy_speed_ms);
        // The bricks the energy between 45 and 20 m/s pays for (the hits per tick round up).
        let most = (0.5 * ball.mass_kg() * (v0 * v0 - v1 * v1) / (ball.impact.energy_per_kg_j * kg))
            .ceil() as u64
            + 4;
        assert!(
            smashed >= 1 && smashed <= most,
            "smashed {smashed}, at most {most}"
        );
        // It didn't come out the far side.
        let far = fx
            .server
            .body_pose(id)
            .map(|(p, _)| f.point(p.to_array())[2]);
        assert!(
            far.is_none_or(|z| z > f64::from(WALL_Z - 12)),
            "came through at z {far:?}"
        );
    }
);

both_faces!(
    fn the_ball_drags_loose_pieces_along_its_path(f: FaceFrame) {
        // Owner, 2026-11-18: "it should exert continuous force as it flies around it, in its velocity
        // direction". A loose piece beside its path (within 1 m) is pushed north, the ball's way.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        let cells = cannon_scene(&mut fx, f);
        let dirt = f.state(dwell_blocks::require("dwell:dirt"));
        // Hung in the air beside the line 6 m out, its box clear of the ball (x 2.5 ± 0.24) and its centre 0.9 m off it.
        // (0.44 m higher for the barrel's 5° rise over those 6 m.)
        let at = f.point([3.4, m_of_row(BASE) + 1.04, -6.0]);
        let piece = fx.server.add_body(0, vec![([0, 0, 0], dirt)], at);
        use_it(&mut fx, f, cells[0]);
        let z0 = at[2];
        for _ in 0..30 {
            fx.server.step();
        }
        let z1 = fx.server.body_pose(piece).map_or(z0, |p| p.0.z);
        assert!(z0 - z1 > 0.3, "dragged north: {z0} -> {z1}");
    }
);

/// The centre of face-local cell (x, row, z) in the world (m).
fn centre(f: FaceFrame, local: [i32; 3]) -> [f64; 3] {
    let c = f.cell(local);
    [
        f64::from(c[0]) + 0.5,
        (f64::from(c[1]) + 0.5) * dwell_world::CELL_H,
        f64::from(c[2]) + 0.5,
    ]
}

both_faces!(
    fn what_comes_to_rest_on_a_built_body_joins_it_not_the_grid(f: FaceFrame) {
        // docs/CANNONS.md C4; owner, 2026-11-17: anything that comes to rest on a moving ship rides
        // along with it, not settling into the world's grid.
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        cannon_scene(&mut fx, f);
        // A 4 × 4 plank deck, a built body (kind 1) standing on the floor away from the cannon.
        let planks = f.state(dwell_blocks::require("dwell:planks"));
        let deck_cells: Vec<([i32; 3], u16)> = (0..4)
            .flat_map(|x| (0..4).map(move |z| ([x, 0, z], planks)))
            .collect();
        let deck = fx
            .server
            .add_body(1, deck_cells, centre(f, [-4, BASE, -12]));
        for _ in 0..30 {
            fx.server.step();
        }
        // A clump of dirt and a cannonball dropped on it.
        let dirt = f.state(dwell_blocks::require("dwell:dirt"));
        let ball = f.state(dwell_blocks::require("dwell:iron_ball"));
        let a = fx
            .server
            .add_body(0, vec![([0, 0, 0], dirt)], centre(f, [-3, BASE + 4, -11]));
        let b = fx
            .server
            .add_body(0, vec![([0, 0, 0], ball)], centre(f, [-2, BASE + 4, -10]));
        for _ in 0..(10 * SIM_HZ) {
            fx.server.step();
        }
        assert_eq!(fx.server.riders_joined(), 2, "both joined the deck");
        assert!(fx.server.body_pose(a).is_none() && fx.server.body_pose(b).is_none());
        let h = fx
            .server
            .body_handle(deck)
            .expect("the deck is still a body");
        let (_, physics, _) = fx.server.sim();
        let mass = physics.mass(h);
        let iron = dwell_world::cannons::ball("dwell:iron_ball")
            .unwrap()
            .mass_kg();
        assert!(mass > iron, "the deck carries the ball's mass: {mass}");
        // Nothing of them settled into the world on or round the deck.
        for x in -6..2 {
            for z in -14..-6 {
                for r in 0..6 {
                    let c = f.cell([x, BASE + r, z]);
                    let m = fx.server.world().get_voxel(c[0], c[1], c[2]);
                    assert!(m != dirt && m != ball, "settled into the grid at {c:?}");
                }
            }
        }
    }
);

both_faces!(
    fn a_ball_shoves_a_player_in_front_of_the_cannon(f: FaceFrame) {
        // Owner (2026-11-25, BUGS.md #242): "I'd like you to also be shoved by them when hit with
        // them. So if you stand in front of a cannon when it's fired, it should launch you forward
        // as expected from physics forces. Currently it seems to just pass through you." A fired
        // ball (a one-cell piece of riders only) collided with the world and bodies, not players;
        // now it collides with players too and throws them by its momentum, without hurting them
        // ("Shove only"; 2026-11-18: "a ball doesn't hurt players").
        let mut fx = Fixture::new(config(f));
        let welcome = fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        let cells = cannon_scene(&mut fx, f);
        // Standing in line with the barrel, 2 m north of its muzzle.
        let at = f.point([2.5, m_of_row(BASE), -2.5]);
        let feet = if f == FaceFrame::A {
            at[1]
        } else {
            at[1] - 1.8
        };
        fx.move_to(welcome.player_id, at[0], feet, at[2]);
        for _ in 0..10 {
            fx.server.step();
        }
        let health = fx.server.health_of(welcome.player_id);
        let h = fx.server.player_handle_of(welcome.player_id).unwrap();
        let before = {
            let (_, physics, players) = fx.server.sim();
            f.point(players.position(physics, h).to_array())
        };
        use_face(&mut fx, f, cells[3], 5);
        fx.server.last_ball().expect("fired");
        for _ in 0..30 {
            fx.server.step();
        }
        let h = fx.server.player_handle_of(welcome.player_id).unwrap();
        let after = {
            let (_, physics, players) = fx.server.sim();
            f.point(players.position(physics, h).to_array())
        };
        assert!(
            before[2] - after[2] > 2.0,
            "thrown north along the barrel: z {} → {}",
            before[2],
            after[2]
        );
        assert_eq!(fx.server.health_of(welcome.player_id), health, "not hurt");
    }
);
