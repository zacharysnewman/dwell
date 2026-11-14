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

/// Sends player 1's inputs for `ticks` ticks (one a tick, numbered from `*seq`), stepping the server.
fn hold(fx: &mut Fixture, seq: &mut u32, input: dwell_physics::controller::Input, ticks: u32) {
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

/// Player 1's height above the scene's ground (face-local metres).
fn height(fx: &Fixture, f: FaceFrame) -> f64 {
    let feet = fx.server.feet_of(1).expect("a live player");
    f.point(feet)[1] - m_of_row(BASE)
}

dwell_world::both_faces!(
    /// The owner's switch (2026-11-08): creative → adventure → creative live. A player flying when
    /// flight goes falls and lands unhurt (the host's doing), later falls hurt; the adventure
    /// inventory is the player's through the creative spell and back.
    fn switching_modes_live_lands_the_flyer_unhurt_and_keeps_the_inventory(f: FaceFrame) {
        let full = players::MAX_HEALTH as i32;
        let mut fx = scene(f, "adventure");
        assert_eq!(fx.server.give(1, "dwell:dirt", 5), 0, "all of it fits");
        assert!(fx.server.set_mode("creative"));
        let fly = dwell_physics::controller::Input {
            fly: true,
            jump: true,
            ..Default::default()
        };
        let mut seq = 0;
        hold(&mut fx, &mut seq, fly, 120);
        let up = height(&fx, f);
        assert!(up > 12.0, "flying in creative: {up} m up");
        // Creative neither used nor dropped the inventory.
        let dirt = |fx: &Fixture| fx.server.inventory_of(1).unwrap().count_of("dwell:dirt");
        assert_eq!(dirt(&fx), 5);
        assert!(fx.server.set_mode("adventure"));
        // Still holding fly: the server clears it, the player falls and lands unhurt.
        hold(&mut fx, &mut seq, fly, 300);
        assert!(height(&fx, f) < 1.0, "landed: {} m up", height(&fx, f));
        assert_eq!(
            fx.server.health_of(1),
            full,
            "the host's fall hurts nothing"
        );
        assert_eq!(dirt(&fx), 5, "the adventure inventory kept");
        // The grace was that one landing: a later hard fall hurts.
        let h = drop_from(&mut fx, f, 15.0);
        assert!(h < full, "a later fall hurts: {h}");
        // And back to creative: flight again, the inventory as it was.
        assert!(fx.server.set_mode("creative"));
        hold(&mut fx, &mut seq, fly, 60);
        assert!(height(&fx, f) > 3.0, "flies again");
        assert_eq!(dirt(&fx), 5);
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

/// A one-cell stone placement beside player 1 in face `f`'s frame, as `op` (a plain `Paste` or a
/// saved structure's `Structure`); the server's check of it.
fn place_one(fx: &mut Fixture, f: FaceFrame, op: RegionEditOp) -> RegionCheck {
    let [x, y, z] = f.cell([2, BASE, 0]);
    fx.send(
        1,
        Message::RegionEdit(RegionEdit {
            op,
            min: [x, y, z],
            size: [1, 1, 1],
            material: 0,
            from: 0,
            cells: vec![f.state(require("dwell:stone"))],
        }),
    );
    for _ in 0..3 {
        fx.server.step();
    }
    fx.server.stats_of(1).expect("joined").last_region_check
}

dwell_world::both_faces!(
    /// STRUCTURES.md §9 (owner, 2026-11-12): the server refuses a structure placement as such
    /// where the world's mode lacks the `structures` ability, and takes it in creative. A plain
    /// paste of the same cell in adventure still goes, paid in items (ADR 0066): the refusal is
    /// the op's, not the cell's.
    fn a_structure_placement_is_refused_outside_creative_and_placed_in_it(f: FaceFrame) {
        let stone = f.state(require("dwell:stone"));
        let [x, y, z] = f.cell([2, BASE, 0]);
        let mut creative = scene(f, "creative");
        creative.send(
            1,
            Message::Capabilities(dwell_protocol::capabilities::Capabilities::this_build()),
        );
        assert_eq!(
            place_one(&mut creative, f, RegionEditOp::Structure),
            RegionCheck::Ok
        );
        assert_eq!(creative.server.world().get_voxel(x, y, z), stone);

        let mut adventure = scene(f, "adventure");
        assert_eq!(adventure.server.give(1, "dwell:stone", 4), 0);
        assert_eq!(
            place_one(&mut adventure, f, RegionEditOp::Structure),
            RegionCheck::NotInMode
        );
        assert_ne!(adventure.server.world().get_voxel(x, y, z), stone);
        let stats = adventure.server.stats_of(1).unwrap();
        assert_eq!(stats.region_edits_rejected, 1);
        assert_eq!(
            adventure
                .server
                .inventory_of(1)
                .unwrap()
                .count_of("dwell:stone"),
            4,
            "nothing paid for the refused placement"
        );
        // The same cell as a plain paste: placed, one stone paid.
        assert_eq!(
            place_one(&mut adventure, f, RegionEditOp::Paste),
            RegionCheck::Ok
        );
        assert_eq!(adventure.server.world().get_voxel(x, y, z), stone);
        assert_eq!(
            adventure
                .server
                .inventory_of(1)
                .unwrap()
                .count_of("dwell:stone"),
            3
        );
        // The host switching to creative allows it at once.
        assert!(adventure.server.set_mode("creative"));
        let [x2, y2, z2] = f.cell([2, BASE + 1, 0]);
        adventure.send(
            1,
            Message::RegionEdit(RegionEdit {
                op: RegionEditOp::Structure,
                min: [x2, y2, z2],
                size: [1, 1, 1],
                material: 0,
                from: 0,
                cells: vec![stone],
            }),
        );
        for _ in 0..3 {
            adventure.server.step();
        }
        assert_eq!(adventure.server.world().get_voxel(x2, y2, z2), stone);
    }
);

#[test]
fn the_server_announces_its_capabilities_and_keeps_its_clients() {
    // both-faces: no face is involved (the join's messages only).
    use dwell_protocol::capabilities::Capabilities;
    use dwell_protocol::capability_names::STRUCTURES_PLACE;
    // `Fixture::join` checks the join's settings announce this build's capabilities.
    let mut fx = scene(FaceFrame::A, "creative");
    // A client that never says it (an older build) has none, and plays on.
    assert_eq!(
        fx.server.client_capabilities(1),
        Some(&Capabilities::default())
    );
    assert_eq!(stroke(&mut fx), RegionCheck::Ok, "an older client's paste");
    // This build's client: its answer is kept.
    fx.send(1, Message::Capabilities(Capabilities::this_build()));
    let got = fx.server.client_capabilities(1).expect("joined");
    assert!(got.has(STRUCTURES_PLACE));
    // A newer client's names the server doesn't know are kept and match nothing here.
    fx.send(
        1,
        Message::Capabilities(Capabilities::of(["mymod:wet", STRUCTURES_PLACE])),
    );
    let got = fx.server.client_capabilities(1).expect("still joined");
    assert!(got.has("mymod:wet") && got.has(STRUCTURES_PLACE));
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
