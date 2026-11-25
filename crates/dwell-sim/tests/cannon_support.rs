//! Cannons standing up, hanging down and falling whole (owner, 2026-11-25, WORK_QUEUE first pass
//! 3a): "Looking more down than out, it should place a cannon pointing up" ("Yes, down too"), and
//! "they currently act unexpectedly by breaking apart when the block beneath them breaks, they
//! shouldn't do that. All blocks should behave like that, falling when disconnected from their
//! supporting (connecting to solid ground) blocks" (BUGS.md #243).
//!
//! face-a-only: the flat world's generator makes face A's ground only, and the rule is "placed
//! blocks are held by natural ground"; the rule itself is tested on both faces in dwell-world's
//! `anchoring` tests.

mod common;

use common::*;
use dwell_protocol::{BlockEditAction, BlockEditRequest, Message};
use dwell_world::{FLAT_TOP_Y, M, m_of_row};

const TOP: i32 = FLAT_TOP_Y; // the first row of air over the flat ground

fn state(name: &str) -> u16 {
    dwell_blocks::require(name)
}

fn edit(fx: &mut Fixture, action: BlockEditAction, cell: [i32; 3], face: u8, material: u16) {
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action,
            cell,
            face,
            material,
            rows: 1,
            down: false,
            point: None,
        }),
    );
    for _ in 0..4 {
        fx.server.step();
    }
}

fn joined() -> (Fixture, u16) {
    let mut fx = Fixture::new(flat());
    let w = fx.join(1, &Client::new(1));
    for _ in 0..10 {
        fx.server.step();
    }
    (fx, w.player_id)
}

#[test]
fn a_cannon_on_two_pillars_falls_whole_once_both_are_cut() {
    // face-a-only: see the module.
    let (mut fx, _) = joined();
    let planks = state("dwell:planks");
    // Two plank pillars, 3 rows, under a north-facing cannon's two bottom cells (x 2, z 0 and 1).
    for z in 0..2 {
        for r in 0..3 {
            fx.server.world().set_voxel(2, TOP + r, z, planks);
        }
    }
    for (part, z, up) in [
        ("rear_bottom", 1, 0),
        ("rear_top", 1, 1),
        ("front_bottom", 0, 0),
        ("front_top", 0, 1),
    ] {
        let m = state(&format!("dwell:cannon[facing=north,part={part}]"));
        fx.server.world().set_voxel(2, TOP + 3 + up, z, m);
    }
    let bodies = fx.server.body_count();
    // One pillar cut at its foot: the cannon still stands on the other (it was broken apart and
    // dropped before, its cell over the cut losing the block under it).
    edit(&mut fx, BlockEditAction::Break, [2, TOP, 0], 1, 0);
    assert_eq!(fx.server.body_count(), bodies, "held by the other pillar");
    for (z, up) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        assert_ne!(
            fx.server.world().get_voxel(2, TOP + 3 + up, z),
            M.air,
            "the cannon whole"
        );
    }
    // The other cut too: the cannon and what is left of both pillars fall as one body.
    edit(&mut fx, BlockEditAction::Break, [2, TOP, 1], 1, 0);
    assert_eq!(fx.server.body_count(), bodies + 1, "one body");
    for (z, up) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        assert_eq!(
            fx.server.world().get_voxel(2, TOP + 3 + up, z),
            M.air,
            "gone from the grid"
        );
    }
    let report: serde_json::Value = serde_json::from_str(&fx.server.bodies_report()).unwrap();
    let cells = report.as_array().unwrap().last().unwrap()["cells"]
        .as_array()
        .unwrap()
        .clone();
    let cannon = cells
        .iter()
        .filter(|c| c[3].as_str().unwrap().starts_with("dwell:cannon"))
        .count();
    assert_eq!(
        (cells.len(), cannon),
        (8, 4),
        "the cannon's 4 cells and 4 planks: {cells:?}"
    );
}

#[test]
fn a_cannon_placed_looking_up_hangs_under_a_beam_pointing_down() {
    // face-a-only: see the module.
    let (mut fx, player) = joined();
    let planks = state("dwell:planks");
    // A pillar at x 5 and a beam out from its top over x 6..7, 5 rows up.
    for r in 0..6 {
        fx.server.world().set_voxel(5, TOP + r, 0, planks);
    }
    for x in 6..8 {
        fx.server.world().set_voxel(x, TOP + 5, 0, planks);
    }
    fx.move_to(player, 6.5, m_of_row(TOP), 2.5);
    for _ in 0..4 {
        fx.server.step();
    }
    // Placed against the beam's underside (face −y, 3) at x 7: the state the client picks
    // looking up (`facing=down`), its rear bottom cell under the beam, the rest stacked down.
    let down = state("dwell:cannon[facing=down,part=rear_bottom]");
    edit(&mut fx, BlockEditAction::Place, [7, TOP + 5, 0], 3, down);
    for (k, part) in ["rear_bottom", "rear_top", "front_bottom", "front_top"]
        .into_iter()
        .enumerate()
    {
        let want = state(&format!("dwell:cannon[facing=down,part={part}]"));
        assert_eq!(
            fx.server.world().get_voxel(7, TOP + 4 - k as i32, 0),
            want,
            "row {} the {part}",
            TOP + 4 - k as i32
        );
    }
    assert_eq!(fx.server.body_count(), 0, "it hangs: held through the beam");
}

#[test]
fn a_cannon_placed_looking_down_stands_up_and_fires_straight_up() {
    // face-a-only: see the module.
    let (mut fx, player) = joined();
    fx.move_to(player, 0.5, m_of_row(TOP), 2.5);
    for _ in 0..4 {
        fx.server.step();
    }
    let up = state("dwell:cannon[facing=up,part=rear_bottom]");
    // Placed on the ground's top (face +y, 2) at (2, TOP − 1, 0): it stands on end, 4 rows.
    edit(&mut fx, BlockEditAction::Place, [2, TOP - 1, 0], 2, up);
    for (k, part) in ["rear_bottom", "rear_top", "front_bottom", "front_top"]
        .into_iter()
        .enumerate()
    {
        let want = state(&format!("dwell:cannon[facing=up,part={part}]"));
        assert_eq!(
            fx.server.world().get_voxel(2, TOP + k as i32, 0),
            want,
            "{part}"
        );
    }
    // Fired (a Use of a side cell): the ball leaves from over its top, straight up.
    edit(
        &mut fx,
        BlockEditAction::Place,
        [2, TOP + 1, 0],
        4,
        state("dwell:dirt"),
    );
    let id = fx.server.last_ball().expect("fired");
    let (p, _) = fx.server.body_pose(id).unwrap();
    let v = fx.server.body_velocity(id).unwrap();
    assert!(
        (p.x - 2.5).abs() < 0.05 && (p.z - 0.5).abs() < 0.05,
        "over its middle: {p:?}"
    );
    assert!(p.y > m_of_row(TOP + 4), "over its top: {p:?}");
    assert!(
        v.y > 30.0 && v.x.abs() < 0.5 && v.z.abs() < 0.5,
        "straight up: {v:?}"
    );
}
