//! Game modes through the server (ADR 0065): a world's mode, from `shared/modes/modes.json`, says
//! what its players may do. Creative hurts no one; adventure hurts (falls on both faces), refuses
//! brush strokes and flight; the host changing the mode applies at once to the players in the world,
//! and the world keeps it.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::world_settings::tag;
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_world::region::{KEEP, RegionCheck};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

/// The scene's ground level (face-local row): stone below it.
const BASE: i32 = FLAT_TOP_Y + 40;

/// A server in `mode` on an empty world with a stone ground in face `f`'s frame, one player joined.
fn scene(f: FaceFrame, mode: &str) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        ..flat()
    });
    let stone = f.state(require("dwell:stone"));
    for x in -6..=6 {
        for z in -6..=6 {
            for r in -4..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    fx.join(1, &Client::new(1));
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

/// Drops player 1 from `metres` above the ground in face `f`'s frame and lets it land; its health.
fn drop_from(fx: &mut Fixture, f: FaceFrame, metres: f64) -> i32 {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = f64::from(players.half_height(h));
    let centre = f.point([0.5, m_of_row(BASE) + metres + half, 0.5]);
    players.teleport(physics, h, DVec3::from(centre), Vec3::ZERO);
    for _ in 0..150 {
        fx.server.step();
    }
    fx.server.health_of(1)
}

fn damage_events(fx: &mut Fixture) -> usize {
    fx.take(1)
        .iter()
        .filter(|(m, _)| matches!(m, Message::PlayerEvent(p) if p.kind == PlayerEventKind::Damage))
        .count()
}

dwell_world::both_faces!(
    fn a_hard_fall_hurts_in_adventure_and_not_at_all_in_creative(f: FaceFrame) {
        let full = players::MAX_HEALTH as i32;
        let mut creative = scene(f, "creative");
        assert_eq!(
            drop_from(&mut creative, f, 15.0),
            full,
            "creative: no fall damage"
        );
        assert_eq!(damage_events(&mut creative), 0);
        let mut adventure = scene(f, "adventure");
        let h = drop_from(&mut adventure, f, 15.0);
        assert!(h > 0 && h < full, "adventure: a 15 m fall hurts: {h}");
        assert_eq!(damage_events(&mut adventure), 1);
        // A lethal one kills in adventure; in creative it is only a landing.
        assert_eq!(drop_from(&mut creative, f, 30.0), full);
        assert_eq!(drop_from(&mut adventure, f, 30.0), 0);
    }
);

dwell_world::both_faces!(
    fn the_host_changing_the_mode_applies_to_the_player_in_the_world(f: FaceFrame) {
        let full = players::MAX_HEALTH as i32;
        let mut fx = scene(f, "creative");
        assert_eq!(drop_from(&mut fx, f, 15.0), full);
        assert!(fx.server.set_mode("adventure"));
        assert!(
            !fx.server.set_mode("no_such_mode"),
            "a name not in the data is refused"
        );
        assert_eq!(fx.server.world_settings().mode, "adventure");
        // The joined player is told the world's settings again, with the new mode.
        let sent: Vec<_> = fx
            .take(1)
            .into_iter()
            .filter_map(|(m, _)| match m {
                Message::WorldSettings(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].0.contains(&(tag::MODE, b"adventure".to_vec())));
        let h = drop_from(&mut fx, f, 15.0);
        assert!(h < full, "hurt from the change on: {h}");
        // And back.
        assert!(fx.server.set_mode("creative"));
        let before = fx.server.health_of(1);
        assert_eq!(drop_from(&mut fx, f, 15.0), before);
    }
);

/// A brush stroke's paste (a `KEEP` cell beside a stone one) beside player 1.
fn stroke(fx: &mut Fixture) -> RegionCheck {
    let [x, y, z] = FaceFrame::A.cell([2, BASE, 0]);
    fx.send(
        1,
        Message::RegionEdit(RegionEdit {
            op: RegionEditOp::Paste,
            min: [x, y, z],
            size: [2, 1, 1],
            material: 0,
            from: 0,
            cells: vec![KEEP, require("dwell:stone")],
        }),
    );
    for _ in 0..3 {
        fx.server.step();
    }
    fx.server.stats_of(1).expect("joined").last_region_check
}

#[test]
fn adventure_refuses_brush_strokes_creative_takes_them() {
    // face-a-only: the check is on the edit's cells, before any face-dependent reading.
    let mut creative = scene(FaceFrame::A, "creative");
    assert_eq!(stroke(&mut creative), RegionCheck::Ok);
    let mut adventure = scene(FaceFrame::A, "adventure");
    assert_eq!(stroke(&mut adventure), RegionCheck::NotPlaceable);
    assert_eq!(
        adventure.server.stats_of(1).unwrap().region_edits_rejected,
        1
    );
}

/// Connects player `id` and returns its Welcome's flags.
fn welcome_flags_of(fx: &mut Fixture, id: SessionId) -> u8 {
    fx.join(id, &Client::new(id as u8)).flags
}

#[test]
fn welcome_says_flight_and_brushes_by_the_mode_and_the_policy_apart() {
    // face-a-only: flags, no geometry.
    let mut fx = Fixture::new(ServerConfig {
        mode: "adventure".into(),
        ..flat()
    });
    let flags = welcome_flags_of(&mut fx, 1);
    assert_eq!(flags & welcome_flags::FLIGHT, 0, "adventure: no flight");
    assert_eq!(flags & welcome_flags::BRUSHES, 0, "adventure: no brushes");
    assert_ne!(
        flags & welcome_flags::FLIGHT_POLICY,
        0,
        "the policy would allow it"
    );
    let mut fx = Fixture::new(flat());
    let flags = welcome_flags_of(&mut fx, 1);
    assert_ne!(
        flags & welcome_flags::FLIGHT,
        0,
        "creative (the default): flight"
    );
    assert_ne!(flags & welcome_flags::BRUSHES, 0);
}
