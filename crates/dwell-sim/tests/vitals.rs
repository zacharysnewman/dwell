//! Stamina and oxygen through the server (docs/adr/0069-stamina-and-oxygen.md): drowning in
//! adventure and not in creative, on both faces; an exhausted player's axe swing refused and a
//! rested one's cost drawn; `Vitals` only to a client that said hello, and `Drown` sent to the
//! others as `Fall`; the vitals saved with the world.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_physics::controller::{VITALS, Vitals};
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

/// The scene's ground level (face-local row).
const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        ..flat()
    }
}

/// Stone with a sealed water room in it (rows BASE−8..BASE−3, x and z −2..2), players 1 and 2
/// joined, 1 having said `VitalsHello`.
fn scene_with(f: FaceFrame, cfg: ServerConfig) -> Fixture {
    let mut fx = Fixture::new(cfg);
    let stone = f.state(require("dwell:stone"));
    let water = f.state(require("dwell:water"));
    for x in -6..=6 {
        for z in -6..=6 {
            for r in -10..0 {
                let inside =
                    (-2..=2).contains(&x) && (-2..=2).contains(&z) && (-8..=-3).contains(&r);
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server
                    .world()
                    .set_voxel(x, y, z, if inside { water } else { stone });
            }
        }
    }
    fx.join(1, &Client::new(1));
    fx.join(2, &Client::new(2));
    fx.send(1, Message::VitalsHello);
    step(&mut fx, 5);
    fx.take_all();
    fx
}

fn scene(f: FaceFrame, mode: &str) -> Fixture {
    scene_with(f, config(f, mode))
}

fn step(fx: &mut Fixture, n: u32) {
    for _ in 0..n {
        fx.server.step();
    }
}

/// Puts player `id` in the sealed water room.
fn dunk(fx: &mut Fixture, f: FaceFrame, id: u16) {
    let h = fx.server.player_handle_of(id).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = f64::from(players.half_height(h));
    let centre = f.point([0.5, m_of_row(BASE - 8) + half + 0.05, 0.5]);
    players.teleport(physics, h, DVec3::from(centre), Vec3::ZERO);
}

fn vitals(fx: &mut Fixture, id: u16) -> Vitals {
    let h = fx.server.player_handle_of(id).expect("a live player");
    fx.server.sim().2.controller(h).vitals
}

fn damage_causes(
    all: &std::collections::BTreeMap<SessionId, Vec<Message>>,
    id: SessionId,
) -> Vec<DamageCause> {
    all.get(&id)
        .into_iter()
        .flatten()
        .filter_map(|m| match m {
            Message::PlayerEvent(e) if e.kind == PlayerEventKind::Damage => Some(e.cause),
            _ => None,
        })
        .collect()
}

const SECOND: u32 = SIM_HZ;

dwell_world::both_faces!(
    fn a_player_out_of_air_drowns_in_adventure_and_never_in_creative(f: FaceFrame) {
        let full = players::MAX_HEALTH as i32;
        let air = VITALS.oxygen_seconds as u32;
        let mut creative = scene(f, "creative");
        dunk(&mut creative, f, 1);
        step(&mut creative, (air + 3) * SECOND);
        assert_eq!(creative.server.health_of(1), full);
        assert_eq!(vitals(&mut creative, 1).oxygen, VITALS.oxygen_seconds);

        let mut fx = scene(f, "adventure");
        dunk(&mut fx, f, 1);
        step(&mut fx, (air - 1) * SECOND);
        let v = vitals(&mut fx, 1);
        assert!(v.oxygen > 0.5 && v.oxygen < 1.5, "{v:?}");
        assert_eq!(fx.server.health_of(1), full, "air left: not hurt yet");
        fx.take_all();
        step(&mut fx, 4 * SECOND);
        let lost = full - fx.server.health_of(1);
        let each = i32::from(VITALS.drown_damage);
        assert!(lost >= 2 * each && lost <= 3 * each, "lost {lost}");
        // The hello'd client hears `Drown`; the other, which never said hello, `Fall`.
        let all = fx.take_all();
        let heard = damage_causes(&all, 1);
        assert!(!heard.is_empty() && heard.iter().all(|c| *c == DamageCause::Drown));
        let other = damage_causes(&all, 2);
        assert_eq!(other.len(), heard.len());
        assert!(other.iter().all(|c| *c == DamageCause::Fall));
        // Out of the water (teleported onto the ground): the air comes back and the damage stops.
        let h = fx.server.player_handle_of(1).unwrap();
        let (_, physics, players) = fx.server.sim();
        let half = f64::from(players.half_height(h));
        let above = f.point([0.5, m_of_row(BASE) + half + 0.05, 4.5]);
        players.teleport(physics, h, DVec3::from(above), Vec3::ZERO);
        let health = fx.server.health_of(1);
        step(&mut fx, 3 * SECOND);
        assert_eq!(fx.server.health_of(1), health);
        assert_eq!(vitals(&mut fx, 1).oxygen, VITALS.oxygen_seconds);
    }
);

fn sweep(fx: &mut Fixture, id: SessionId) {
    fx.send(
        id,
        Message::Chop(ChopAction::Sweep {
            tool: "dwell:stone_axe".into(),
            look: [1.0, 0.0, 0.0],
            vertical: false,
        }),
    );
}

#[test]
fn an_exhausted_swing_is_refused_and_a_rested_one_draws_its_cost() {
    // face-a-only: the swing's cost is no face's; the drowning test covers both.
    let f = FaceFrame::A;
    let cost = dwell_protocol::chopping::tool("dwell:stone_axe")
        .unwrap()
        .swing
        .stamina_cost as f32;
    let mut fx = scene(f, "adventure");
    sweep(&mut fx, 1);
    let v = vitals(&mut fx, 1);
    assert!(
        (VITALS.max_stamina - v.stamina - cost).abs() < 0.01,
        "{v:?}"
    );
    step(&mut fx, SECOND);
    let h = fx.server.player_handle_of(1).unwrap();
    fx.server.sim().2.controller_mut(h).vitals = Vitals {
        stamina: 3.0,
        exhausted: true,
        rest_ticks: 0,
        ..Vitals::default()
    };
    sweep(&mut fx, 1);
    let v = vitals(&mut fx, 1);
    assert_eq!(v.stamina, 3.0, "refused: nothing drawn");
    // In creative a swing costs nothing.
    let mut fx = scene(f, "creative");
    sweep(&mut fx, 1);
    assert_eq!(vitals(&mut fx, 1).stamina, VITALS.max_stamina);
}

fn vitals_messages(fx: &mut Fixture, id: SessionId) -> Vec<VitalsMessage> {
    fx.server
        .take_outbox()
        .into_iter()
        .filter(|o| o.session == id)
        .filter_map(|o| match Message::decode(&o.bytes) {
            Some(Message::Vitals(v)) => Some(v),
            _ => None,
        })
        .collect()
}

#[test]
fn vitals_go_only_to_a_client_that_said_hello_and_only_in_a_mode_with_them() {
    // face-a-only: a wire rule.
    let f = FaceFrame::A;
    let mut fx = scene(f, "adventure");
    step(&mut fx, 10);
    let mut out = fx.server.take_outbox();
    let to = |out: &mut Vec<dwell_sim::server::Outgoing>, id: SessionId| {
        out.iter()
            .filter(|o| o.session == id)
            .filter_map(|o| match Message::decode(&o.bytes) {
                Some(Message::Vitals(v)) => Some(v),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let one = to(&mut out, 1);
    assert!(!one.is_empty());
    let last = one.last().unwrap();
    assert_eq!(
        last.flags,
        vitals_flags::STAMINA | vitals_flags::OXYGEN,
        "rested, both rules on"
    );
    assert_eq!(last.stamina, VITALS.max_stamina);
    assert!(to(&mut out, 2).is_empty(), "no hello, no vitals");
    let mut fx = scene(f, "creative");
    step(&mut fx, 10);
    assert!(vitals_messages(&mut fx, 1).is_empty());
}

#[test]
fn vitals_are_saved_with_the_world() {
    // face-a-only: a byte format.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "adventure")
    };
    {
        let mut fx = scene_with(f, cfg());
        let h = fx.server.player_handle_of(1).unwrap();
        fx.server.sim().2.controller_mut(h).vitals = Vitals {
            stamina: 10.0,
            oxygen: 4.0,
            exhausted: true,
            rest_ticks: 0,
        };
        fx.server.save_now();
        fx.server.flush_store();
    }
    let mut fx = scene_with(f, cfg());
    let v = vitals(&mut fx, 1);
    assert!(v.exhausted, "{v:?}");
    assert!(v.stamina < 12.0 && v.oxygen > 3.0, "{v:?}");
}
