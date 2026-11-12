//! Out of a portal (docs/BIFACIAL_WORLD.md §12.3; owner 2026-11-12: "land beside the hole"): a
//! player who drops into a portal's shaft falls to the midplane, swings across it and rises out
//! of the far face's opening, where the shaft's exit push (`midplane.json` `portals.exit`,
//! `VoxelWorld::portals`) carries it sideways, so it arcs out and lands on the ground beside the
//! hole instead of falling back in and swinging between the faces. Geometry is in world
//! coordinates (the generated world itself has both faces).

mod common;

use common::*;
use dwell_physics::DVec3;
use dwell_protocol::world_size::WorldSize;
use dwell_world::portal_shafts::{PortalShafts, Shaft};
use dwell_world::{FaceFrame, GENERATOR_TERRAIN};

const SEED: u64 = 0;

fn shafts() -> PortalShafts {
    dwell_worldgen::portal_shafts_for_sized(GENERATOR_TERRAIN, SEED, WorldSize::DEFAULT)
}

fn world(portals: PortalShafts) -> PlayerTestWorld {
    let mut w = PlayerTestWorld::world_frame_of(
        dwell_worldgen::generator_for(GENERATOR_TERRAIN, SEED),
        Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, SEED)),
    );
    w.world.portals = portals;
    w
}

/// Where a drop ended: the face it landed on, its distance from the axis (m), its feet's height
/// over that face's opening (m), when (s), and how many times it crossed the midplane.
#[derive(Debug)]
struct Landing {
    face: FaceFrame,
    from_axis: f64,
    over_opening: f64,
    seconds: f32,
    crossings: u32,
}

/// Drops a player into `shaft` from face `f`: on the axis, its feet 1 m over the opening, facing
/// `yaw` with no input (the look continued across the midplane as the client does), until it
/// stands on the ground or `seconds` pass.
fn drop_in(
    w: &mut PlayerTestWorld,
    shaft: Shaft,
    f: FaceFrame,
    yaw: f64,
    seconds: f32,
) -> Option<Landing> {
    drop_in_at(w, shaft, [0.0, 0.0], f, yaw, seconds)
}

/// [`drop_in`] from `offset` (m, x and z) off the axis.
fn drop_in_at(
    w: &mut PlayerTestWorld,
    shaft: Shaft,
    offset: [f64; 2],
    f: FaceFrame,
    yaw: f64,
    seconds: f32,
) -> Option<Landing> {
    let start = DVec3::new(
        shaft.axis[0] + offset[0],
        f.y(shaft.top(f.face) + 1.0),
        shaft.axis[1] + offset[1],
    );
    let h = w.spawn_at(start, yaw as f32);
    let (mut yaw, mut pitch, mut face, mut crossings) = (yaw, 0.0f64, f, 0);
    for t in 0..ticks(seconds) {
        let (y, p) = (yaw as f32, pitch as f32);
        w.set_input(move |_, _| input(0.0, 0.0, false, false, false, y, p));
        w.step(1);
        let c = *w.c(h);
        if c.face != face {
            face = c.face;
            crossings += 1;
            (yaw, pitch) = FaceFrame::look_across(yaw, pitch);
        }
        if t > 0 && c.ground.grounded {
            let pos = w.players.position(&w.physics, h);
            let feet = c.face.y(w.players.feet(&w.physics, h));
            return Some(Landing {
                face: c.face,
                from_axis: (pos.x - shaft.axis[0]).hypot(pos.z - shaft.axis[1]),
                over_opening: feet - shaft.top(c.face.face),
                seconds: t as f32 / 60.0,
                crossings,
            });
        }
    }
    None
}

dwell_world::both_faces!(
    fn a_player_dropped_into_a_portal_lands_on_the_far_face_beside_the_hole(f: FaceFrame) {
        // both-faces: dropped in from face `f`, it comes out on the other.
        let portals = shafts();
        let shaft = portals.shafts[0];
        let rules = dwell_worldgen::BifacialTerrain::new(SEED).portals().rules;
        let mut w = world(portals);
        // Facing between the monument's pillars (north, east, south, west of the axis).
        let landing = drop_in(&mut w, shaft, f, 45.0, 8.0)
            .expect("still in the air after 8 s: swinging between the faces");
        assert_eq!(landing.face, f.other(), "{landing:?}");
        assert_eq!(landing.crossings, 1, "{landing:?}");
        assert!(landing.seconds < 7.0, "{landing:?}");
        // On the ground beside the opening: past the rim, 2–4 m out (within the clear ground),
        // at the ground's height (the rim's top is ½ m over it), not down the shaft.
        let rim = f64::from(rules.radius + rules.rim_width);
        assert!(
            (rim + 1.5..=rim + 4.0).contains(&landing.from_axis),
            "landed {:.2} m from the axis (the rim ends at {rim} m): {landing:?}",
            landing.from_axis
        );
        assert!((-1.6..=0.1).contains(&landing.over_opening), "{landing:?}");
    }
);

dwell_world::both_faces!(
    fn a_player_who_falls_against_the_shafts_wall_still_lands_beside_the_far_hole(f: FaceFrame) {
        // both-faces: dropped in from face `f`, it comes out on the other.
        // BUGS.md #107: a player who walks into the hole on the diagonal (yaw 45) falls with the
        // walk's momentum into the wall's corner there (1.29 m out on each axis, in the browser)
        // and rises against it. A push inside the shaft along its facing went into that wall and
        // it fell straight back; over the opening it is clear.
        let portals = shafts();
        let shaft = portals.shafts[0];
        let rules = dwell_worldgen::BifacialTerrain::new(SEED).portals().rules;
        let mut w = world(portals);
        let landing = drop_in_at(&mut w, shaft, [1.29, 1.29], f, 45.0, 8.0)
            .expect("still in the air after 8 s: swinging between the faces");
        assert_eq!(landing.face, f.other(), "{landing:?}");
        assert_eq!(landing.crossings, 1, "{landing:?}");
        let rim = f64::from(rules.radius + rules.rim_width);
        assert!(landing.from_axis > rim, "{landing:?}");
        assert!((-1.6..=0.1).contains(&landing.over_opening), "{landing:?}");
    }
);

#[test]
fn without_the_exit_push_a_dropped_player_swings_between_the_faces() {
    // both-faces: the swing crosses the faces. What the push fixes (owner 2026-11-12): with no
    // push, nothing carries the player out of the far opening and it falls back, again and again.
    let portals = shafts();
    let shaft = portals.shafts[0];
    let mut w = world(PortalShafts::new(
        portals.shafts.to_vec(),
        Default::default(),
    ));
    assert!(
        drop_in(&mut w, shaft, FaceFrame::A, 45.0, 8.0).is_none(),
        "landed without the push"
    );
}

#[test]
fn every_portal_lets_a_dropped_player_out_beside_an_opening() {
    // both-faces: drops from each face's opening of every portal of a world.
    // Most come out of the far opening on the first swing and land 2–4 m past its rim. A far
    // opening higher than the swing reaches, or a rise so tall it meets the monument's arches
    // (~23 m over the opening), falls back; the band speeds every pass (BIFACIAL_WORLD.md §3), so
    // a later swing comes out. None swings forever.
    let portals = shafts();
    let rules = dwell_worldgen::BifacialTerrain::new(SEED).portals().rules;
    let mut first_swing = 0;
    let mut drops = 0;
    for &shaft in portals.shafts.iter() {
        for f in [FaceFrame::A, FaceFrame::B] {
            let mut w = world(portals.clone());
            let landing = drop_in(&mut w, shaft, f, 45.0, 25.0)
                .unwrap_or_else(|| panic!("{shaft:?} from {f:?}: still swinging after 25 s"));
            drops += 1;
            assert!(landing.crossings <= 3, "{shaft:?} from {f:?}: {landing:?}");
            // Out of the hole: on the rim or the ground round it, not down the shaft.
            assert!(
                landing.from_axis > f64::from(rules.radius) + 0.5,
                "{shaft:?} from {f:?}: {landing:?}"
            );
            assert!(
                landing.over_opening > -1.6,
                "{shaft:?} from {f:?}: {landing:?}"
            );
            if landing.crossings == 1 {
                first_swing += 1;
            }
        }
    }
    assert!(
        first_swing * 4 >= drops * 3,
        "{first_swing} of {drops} out on the first swing"
    );
}
