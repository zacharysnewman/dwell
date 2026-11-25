//! Villagers and doors (owner, 2026-11-14: "And doors also." / "2m doors."; docs/BLOCK_REGISTRY.md,
//! Doors): an NPC whose data says `opensDoors` walks through a closed door in a wall, the server
//! opening it (all four cells, an edit) as it comes and closing it once it has gone; one that
//! doesn't open doors (a doe) is stopped by it. Both faces.

mod common;

use common::*;
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, both_faces, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;
/// The wall's column (face-local x) and the door's cell in it (z).
const WALL_X: i32 = 4;
const DOOR_Z: i32 = 8;

fn config(f: FaceFrame) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        // Ground written by hand into an empty world: all of it "placed".
        placed_blocks_fall: false,
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        ..flat()
    }
}

fn door(part: &str, open: bool) -> u16 {
    dwell_blocks::require(&format!(
        "dwell:door[facing=east,hinge=left,open={open},part={part}]"
    ))
}

const PARTS: [&str; 4] = ["bottom", "lower", "upper", "top"];

/// A stone floor under `BASE`, a 1 m stone brick wall 2.5 m tall at x = `WALL_X`, and a closed
/// door in it at z = `DOOR_Z`: its four cells, bottom to top (world cells).
fn walled(fx: &mut Fixture, f: FaceFrame) -> Vec<[i32; 3]> {
    let stone = f.state(dwell_blocks::require("dwell:stone"));
    let brick = f.state(dwell_blocks::require("dwell:stone_brick"));
    let air = dwell_blocks::require("dwell:air");
    for x in -8..24 {
        for z in -8..24 {
            for r in -2..12 {
                let m = if r < 0 {
                    stone
                } else if x == WALL_X && r < 5 {
                    brick
                } else {
                    air
                };
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, m);
            }
        }
    }
    (0..4)
        .map(|k| {
            let c = f.cell([WALL_X, BASE + k, DOOR_Z]);
            fx.server
                .world()
                .set_voxel(c[0], c[1], c[2], f.state(door(PARTS[k as usize], false)));
            c
        })
        .collect()
}

fn stand(fx: &mut Fixture, f: FaceFrame, id: u16, x: f64, z: f64) {
    let h = fx.server.player_handle_of(id).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = players.half_height(h) as f64;
    let c = f.point([x, m_of_row(BASE) + half, z]);
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::from(c),
        dwell_physics::Vec3::ZERO,
    );
}

/// A creature of `name` walking to face-local (tx, tz) from (x, z) on the floor.
fn walker(fx: &mut Fixture, f: FaceFrame, name: &str, x: f64, z: f64, to: [f64; 2]) -> u32 {
    let cs = fx.server.creatures_mut();
    let k = cs.kind_of(name).expect("a creature of the data");
    let id = cs
        .add_driven(k, f.point([x, m_of_row(BASE), z]), 90.0, 3)
        .expect("room");
    cs.get_mut(id).unwrap().target = Some(to);
    id
}

fn local_x(fx: &mut Fixture, f: FaceFrame, id: u32) -> f64 {
    let c = fx.server.creatures().iter().find(|c| c.id == id).unwrap();
    f.point(c.position)[0]
}

fn open_cells(fx: &mut Fixture, cells: &[[i32; 3]]) -> usize {
    cells
        .iter()
        .filter(|c| {
            let m = fx.server.world().get_voxel(c[0], c[1], c[2]);
            dwell_blocks::state_property(m, "open") == Some("true")
        })
        .count()
}

both_faces!(
    fn a_villager_opens_a_door_walks_through_and_closes_it_behind_it(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        let cells = walled(&mut fx, f);
        fx.join(1, &Client::new(1));
        // The player stands back from the wall, on the far side, out of the doorway's way.
        stand(&mut fx, f, 1, 12.5, 20.5);
        let to = [10.5, DOOR_Z as f64 + 0.5];
        let v = walker(&mut fx, f, "dwell:villager", 0.5, DOOR_Z as f64 + 0.5, to);
        let mut opened = false;
        for _ in 0..60 * 12 {
            fx.server.step();
            opened |= open_cells(&mut fx, &cells) == 4;
            // Every cell of the door together: open or closed, never half.
            assert!(matches!(open_cells(&mut fx, &cells), 0 | 4));
        }
        assert!(opened, "the door opened as it came");
        let x = local_x(&mut fx, f, v);
        assert!(x > WALL_X as f64 + 2.0, "through the doorway: x {x}");
        assert_eq!(open_cells(&mut fx, &cells), 0, "closed behind it");
        for (k, c) in cells.iter().enumerate() {
            assert_eq!(
                fx.server.world().get_voxel(c[0], c[1], c[2]),
                f.state(door(PARTS[k], false)),
                "cell {k} whole and closed"
            );
        }
    }
);

both_faces!(
    fn a_doe_is_stopped_by_a_closed_door(f: FaceFrame) {
        let mut fx = Fixture::new(config(f));
        let cells = walled(&mut fx, f);
        fx.join(1, &Client::new(1));
        stand(&mut fx, f, 1, 12.5, 20.5);
        let to = [10.5, DOOR_Z as f64 + 0.5];
        let d = walker(&mut fx, f, "dwell:doe", 0.5, DOOR_Z as f64 + 0.5, to);
        for _ in 0..60 * 8 {
            fx.server.step();
        }
        assert_eq!(open_cells(&mut fx, &cells), 0, "nothing opened it");
        let x = local_x(&mut fx, f, d);
        assert!(x < WALL_X as f64, "stopped at the door: x {x}");
    }
);
