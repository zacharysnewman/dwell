//! Block edits in a big world (BUGS.md #109, docs/proposals/BIG_MAP.md M2): the server checks an
//! edit against the world's own disc and build volume, not the 1× build's 50 km, so in a 4× or 10×
//! world a player 60 km and ~200 km out places and breaks a block; a 1× world still refuses a
//! place past its rim.

mod common;

use common::*;
use dwell_protocol::world_size::WorldSize;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_world::edit::EditCheck;
use dwell_world::{FaceFrame, M};

fn edit(action: BlockEditAction, cell: [i32; 3], face: u8, material: u16) -> Message {
    Message::BlockEditRequest(BlockEditRequest {
        action,
        cell,
        face,
        material,
        rows: 1,
        down: false,
        point: None,
    })
}

/// A player `x` m out on face `face` places a stone against a floating one and breaks the placed
/// one: what the server made of each (the check, and the cell's state after).
fn place_and_break(
    f: &mut Fixture,
    face: FaceFrame,
    x: i32,
) -> ((EditCheck, u16), (EditCheck, u16)) {
    let w = f.join(1, &Client::new(1));
    f.send(1, Message::WorldgenCheck(0));
    // Open sky 500 m up (the flat ground ends at the 1× disc), inside the spheroid build volume
    // even 0.5 km from the 4× rim (where it is ~1.7 km tall).
    let row = 1000;
    let target = face.cell([x, row, 2]);
    let into = face.cell([x, row, 1]);
    let stone = face.state(M.stone);
    f.server
        .world()
        .set_voxel(target[0], target[1], target[2], stone);
    // The eye level with the block's middle, 1.5 m in front of it, looking +Z at its −Z face (5).
    let centre = face.point([f64::from(x) + 0.5, dwell_world::m_of_row(row), -1.0]);
    let half = {
        let h = f.server.player_handle_of(w.player_id).unwrap();
        let (_, _, players) = f.server.sim();
        f64::from(players.half_height(h))
    };
    f.move_to(w.player_id, centre[0], centre[1] - half, centre[2]);
    let check = |f: &mut Fixture, m: Message, at: [i32; 3]| {
        f.send(1, m);
        f.server.step();
        let c = f.server.stats_of(w.player_id).unwrap().last_edit_check;
        (c, f.server.world().get_voxel(at[0], at[1], at[2]))
    };
    let placed = check(f, edit(BlockEditAction::Place, target, 5, stone), into);
    for _ in 0..30 {
        f.server.step(); // past the edit interval
    }
    let broken = check(f, edit(BlockEditAction::Break, into, 5, 0), into);
    (placed, broken)
}

fn world(size: &str) -> Fixture {
    Fixture::new(ServerConfig {
        world_size: WorldSize::named(size).expect("a named size"),
        ..flat()
    })
}

dwell_world::both_faces!(
    fn a_4x_and_a_10x_world_take_edits_60_and_200_km_out(f: FaceFrame) {
        // 4×'s rim is at 200 km: its far player just inside it.
        for (size, x) in [
            ("4x", 60_000),
            ("4x", 199_500),
            ("10x", 60_000),
            ("10x", 200_000),
        ] {
            let mut fx = world(size);
            let (placed, broken) = place_and_break(&mut fx, f, x);
            let stone = f.state(M.stone);
            assert_eq!(placed, (EditCheck::Ok, stone), "{size}: the place at {x} m");
            assert_eq!(broken, (EditCheck::Ok, M.air), "{size}: the break at {x} m");
        }
    }
);

dwell_world::both_faces!(
    fn a_1x_world_still_refuses_a_place_past_its_rim(f: FaceFrame) {
        let mut fx = world("1x");
        let (placed, _) = place_and_break(&mut fx, f, 60_000);
        assert_eq!(placed, (EditCheck::OutOfWorld, M.air));
    }
);
