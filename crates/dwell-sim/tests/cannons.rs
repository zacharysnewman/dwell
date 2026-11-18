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
        for z in -6..8 {
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
