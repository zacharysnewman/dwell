//! A player put inside solid ground (BUGS.md #82: `?at=` to a y under the ground, a teleport while
//! flying into a mountain) is lifted to the first free space above it, never falling through the
//! ground toward the midplane and face B (both faces).

mod common;

use common::*;
use dwell_blocks::require;
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

/// The scene's ground level (face-local row): 20 m of stone below it.
const BASE: i32 = FLAT_TOP_Y + 40;
const DEPTH: i32 = 40;

fn scene(f: FaceFrame, spawn_row: i32) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(spawn_row), 0.5])),
        ..flat()
    });
    let stone = f.state(require("dwell:stone"));
    for x in -6..=6 {
        for z in -6..=6 {
            for r in -DEPTH..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    fx.join(1, &Client::new(1));
    fx
}

/// Player 1's face-local feet after `ticks` steps.
fn feet_after(fx: &mut Fixture, f: FaceFrame, ticks: u32) -> [f64; 3] {
    for _ in 0..ticks {
        fx.server.step();
    }
    f.point(fx.server.feet_of(1).expect("a live player"))
}

dwell_world::both_faces!(
    fn a_spawn_inside_the_ground_stands_on_the_ground_above(f: FaceFrame) {
        // `?at=` with a y 10 m under the surface.
        let mut fx = scene(f, BASE - 20);
        let feet = feet_after(&mut fx, f, 120);
        assert!(
            (feet[1] - m_of_row(BASE)).abs() < 0.3,
            "on the ground above: feet {:.2}, ground {:.2}",
            feet[1],
            m_of_row(BASE)
        );
        assert_eq!(FaceFrame::at_height(fx.server.feet_of(1).unwrap()[1]), f);
    }
);

dwell_world::both_faces!(
    fn a_teleport_into_the_ground_lifts_the_player_out(f: FaceFrame) {
        let mut fx = scene(f, BASE);
        feet_after(&mut fx, f, 30);
        // A teleport (flying, at rest) to 6 m inside the stone.
        let h = fx.server.player_handle_of(1).unwrap();
        let (_, physics, players) = fx.server.sim();
        let half = f64::from(players.half_height(h));
        let centre = f.point([2.5, m_of_row(BASE - 12) + half, -1.5]);
        players.teleport(
            physics,
            h,
            dwell_physics::DVec3::from(centre),
            dwell_physics::Vec3::ZERO,
        );
        let feet = feet_after(&mut fx, f, 120);
        assert!(
            (feet[1] - m_of_row(BASE)).abs() < 0.3,
            "lifted to the ground: feet {:.2}, ground {:.2}",
            feet[1],
            m_of_row(BASE)
        );
        assert!(
            (feet[0] - 2.5).abs() < 0.5 && (feet[2] + 1.5).abs() < 0.5,
            "in its column: {feet:?}"
        );
    }
);
