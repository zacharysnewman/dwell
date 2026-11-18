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
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell,
            face: f.cell_face(1) as u8,
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
    fn a_cannon_fires_on_a_use_and_again_once_reloaded(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        fx.join(1, &Client::new(1));
        for _ in 0..10 {
            fx.server.step();
        }
        let cells = cannon_scene(&mut fx, f);
        use_it(&mut fx, f, cells[2]);
        assert_eq!(fx.server.shots_fired(), 1, "fired");
        // Any cell is the cannon; too soon after a shot it doesn't fire.
        use_it(&mut fx, f, cells[1]);
        assert_eq!(fx.server.shots_fired(), 1, "reloading");
        // Its cells are all still there: a Use writes nothing.
        for c in cells {
            assert_ne!(fx.server.world().get_voxel(c[0], c[1], c[2]), 0);
        }
        let reload = dwell_world::cannons::cannon("dwell:cannon")
            .unwrap()
            .reload_s;
        for _ in 0..(reload * f64::from(SIM_HZ)) as u32 {
            fx.server.step();
        }
        use_it(&mut fx, f, cells[3]);
        assert_eq!(fx.server.shots_fired(), 2, "reloaded");
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
        // It flies north (−z) fast.
        for _ in 0..6 {
            fx.server.step();
        }
        let l1 = f.point(fx.server.body_pose(id).unwrap().0.to_array());
        assert!(l0[2] - l1[2] > 3.0, "{l0:?} -> {l1:?}");
        // And lands on the floor north of the cannon, rolling on along it, not through it.
        for _ in 0..30 {
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
        // Landing and rolling on the flat struck nothing loose (a grazing landing is no strike).
        assert_eq!(fx.server.ball_strikes().1, 0, "knocked cells on the flat");
        assert!(at[2] > -150.0 && at[2] < -10.0, "rested at {at:?}");
    }
);

/// A stone brick wall across the barrel's path at face-local z = `WALL_Z`, 5 rows tall.
const WALL_Z: i32 = -8;
fn wall(fx: &mut Fixture, f: FaceFrame) -> Vec<[i32; 3]> {
    let brick = f.state(dwell_blocks::require("dwell:stone_brick"));
    let mut cells = Vec::new();
    for x in -2..8 {
        for r in 0..5 {
            let c = f.cell([x, BASE + r, WALL_Z]);
            fx.server.world().set_voxel(c[0], c[1], c[2], brick);
            cells.push(c);
        }
    }
    cells
}

fn fire_at_wall(f: FaceFrame, explosive: bool) -> (Fixture, Vec<[i32; 3]>, u32) {
    let mut fx = Fixture::new(config(f));
    fx.join(1, &Client::new(1));
    for _ in 0..10 {
        fx.server.step();
    }
    let cells = cannon_scene(&mut fx, f);
    let wall = wall(&mut fx, f);
    if explosive {
        fx.server.set_cannon_ball("dwell:explosive_ball");
    }
    use_it(&mut fx, f, cells[0]);
    let id = fx.server.last_ball().expect("a ball fired");
    for _ in 0..40 {
        fx.server.step();
    }
    (fx, wall, id)
}

both_faces!(
    fn an_iron_ball_knocks_loose_the_cells_round_where_it_strikes_a_wall(f: FaceFrame) {
        // docs/CANNONS.md C3; owner, 2026-11-17: "one simple impact knocking loose cells it strikes
        // (and maybe knocks loose in an area around where it collides)".
        let (mut fx, wall, id) = fire_at_wall(f, false);
        let (strikes, knocked) = fx.server.ball_strikes();
        assert!(strikes >= 1, "it struck the wall");
        let max = match dwell_world::cannons::ball("dwell:iron_ball")
            .unwrap()
            .impact
        {
            dwell_world::cannons::Impact::Knock { max_cells, .. } => u64::from(max_cells),
            _ => unreachable!(),
        };
        assert!(
            knocked >= 1 && knocked <= max * strikes,
            "knocked {knocked} in {strikes}"
        );
        // Gone from the wall: in line with the barrel (x 2), at its height.
        let gone: Vec<[i32; 3]> = wall
            .iter()
            .copied()
            .filter(|c| fx.server.world().get_voxel(c[0], c[1], c[2]) == 0)
            .collect();
        assert_eq!(gone.len() as u64, knocked, "{gone:?}");
        assert!(
            gone.contains(&f.cell([2, BASE + 1, WALL_Z])),
            "the cell it hit: {gone:?}"
        );
        // The ball itself is still about (flown on, or resting).
        let _ = fx.server.body_pose(id);
    }
);

both_faces!(
    fn an_explosive_ball_blows_up_where_it_strikes(f: FaceFrame) {
        let (mut fx, wall, id) = fire_at_wall(f, true);
        assert!(fx.server.body_pose(id).is_none(), "the ball is gone");
        let gone = wall
            .iter()
            .filter(|c| fx.server.world().get_voxel(c[0], c[1], c[2]) == 0)
            .count();
        // Dynamite's blast takes far more than the iron ball's knock.
        assert!(gone > 6, "the blast cleared {gone} cells of the wall");
        assert!(gone < wall.len(), "not the whole wall");
    }
);
