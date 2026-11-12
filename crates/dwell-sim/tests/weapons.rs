//! Weapons through the server (WORK_QUEUE item 35): a sword's swing (`ChopAction::Sweep`, as the
//! axe's) lands at its contact and strikes the creatures its arc crosses by the sword's data —
//! damage, knockback, one wear a creature struck — never a player; a deer killed drops its data
//! drops; a creature holding a raised sword (the defender API) blocks a strike across its blade.
//! Both faces.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::chopping::{self, Clash};
use dwell_protocol::*;
use dwell_sim::creatures::Guard;
use dwell_sim::server::ServerConfig;
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;
const SWORD: &str = "dwell:sword";

fn sword() -> &'static chopping::ToolDef {
    chopping::tool(SWORD).expect("the sword is data")
}

/// A whole swing in ticks (a miss's length and a tick): past its contact and its wait.
fn swing_ticks() -> u32 {
    let t = sword();
    let s = t.swing.miss_length().max(t.swing_seconds);
    (s * f64::from(SIM_HZ)).ceil() as u32 + 2
}

/// A stone floor; player 1 at face-local (0.5, 0.5) holding the sword (in `mode`), player 2
/// standing 1.5 m ahead along face-local +x, both looking on.
fn scene(f: FaceFrame, mode: &str) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        ..flat()
    });
    let stone = f.state(require("dwell:stone"));
    for x in -8..=10 {
        for z in -6..=6 {
            for r in -4..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    fx.join(1, &Client::new(1));
    fx.join(2, &Client::new(2));
    fx.send(1, Message::Chop(ChopAction::Hello));
    fx.server.give(1, SWORD, 1);
    // Held: the slot it went to (adventure starts with an axe in the first).
    let slot = fx
        .server
        .inventory_of(1)
        .and_then(|i| {
            i.slots
                .iter()
                .position(|s| s.as_ref().is_some_and(|s| s.item == SWORD))
        })
        .expect("given");
    fx.send(
        1,
        Message::InventoryAction(InventoryAction::Select { slot: slot as u8 }),
    );
    move_to(&mut fx, 1, f, 0.5, 0.5);
    move_to(&mut fx, 2, f, 2.0, 0.5);
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

fn move_to(fx: &mut Fixture, id: u16, f: FaceFrame, x: f64, z: f64) {
    let h = fx.server.player_handle_of(id).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = f64::from(players.half_height(h));
    let centre = f.point([x, m_of_row(BASE) + half, z]);
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::from(centre),
        dwell_physics::Vec3::ZERO,
    );
}

/// A creature of `name` driven (it stays put) with its feet at face-local (x, floor, z).
fn put(fx: &mut Fixture, f: FaceFrame, name: &str, x: f64, z: f64, yaw: f64) -> u32 {
    let cs = fx.server.creatures_mut();
    let k = cs.kind_of(name).expect("a creature of the data");
    cs.add_driven(k, f.point([x, m_of_row(BASE), z]), yaw, 7)
        .expect("room")
}

fn health(fx: &Fixture, id: u32) -> Option<f64> {
    fx.server
        .creatures()
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.health)
}

/// A level (or vertical) swing along face-local +x, then the whole swing's ticks.
fn slash(fx: &mut Fixture, f: FaceFrame, vertical: bool) {
    fx.send(
        1,
        Message::Chop(ChopAction::Sweep {
            tool: SWORD.into(),
            look: f.vector([1.0, 0.0, 0.0]).map(|v| v as f32),
            vertical,
        }),
    );
    for _ in 0..swing_ticks() {
        fx.server.step();
    }
}

fn sword_wear(fx: &Fixture) -> Option<u16> {
    let inv = fx.server.inventory_of(1)?;
    inv.slots[usize::from(inv.held)]
        .as_ref()
        .filter(|s| s.item == SWORD)
        .map(|s| s.wear)
}

dwell_world::both_faces!(
    fn a_slash_hurts_the_deer_in_its_arc_only_never_a_player(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        let w = sword().weapon.unwrap();
        // 1.6 m ahead, a little to the side of player 2; another behind the swinger.
        let near = put(&mut fx, f, "dwell:doe", 2.1, 1.4, 0.0);
        let behind = put(&mut fx, f, "dwell:doe", -2.5, 0.5, 0.0);
        let whole = health(&fx, near).unwrap();
        let players = [fx.server.health_of(1), fx.server.health_of(2)];
        fx.send(
            1,
            Message::Chop(ChopAction::Sweep {
                tool: SWORD.into(),
                look: f.vector([1.0, 0.0, 0.0]).map(|v| v as f32),
                vertical: false,
            }),
        );
        // Nothing lands during the wind-up: hits land at contact.
        for _ in 0..(sword().swing.windup_s * f64::from(SIM_HZ)) as u32 {
            fx.server.step();
        }
        assert_eq!(health(&fx, near), Some(whole), "not before contact");
        for _ in 0..swing_ticks() {
            fx.server.step();
        }
        assert!(
            (health(&fx, near).unwrap() - (whole - w.damage)).abs() < 1e-9,
            "the sword's damage, by data"
        );
        assert_eq!(health(&fx, behind), Some(whole), "the deer outside the arc");
        assert_eq!(
            [fx.server.health_of(1), fx.server.health_of(2)],
            players,
            "no player is ever hurt, though one stood in the arc"
        );
        assert_eq!(fx.server.strikes().len(), 1);
        assert_eq!(sword_wear(&fx), Some(1), "one creature struck, one wear");
        // Knocked back, away from the swinger along the ground.
        let c = fx.server.creatures().iter().find(|c| c.id == near).unwrap();
        let l = f.point(c.position);
        assert!(l[0] > 2.1 + 0.5 * w.knockback_m, "pushed away: {l:?}");
        assert!((l[1] - m_of_row(BASE)).abs() < 1e-6, "on the floor: {l:?}");
    }
);

dwell_world::both_faces!(
    fn a_deer_killed_by_the_sword_drops_its_data_drops(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        move_to(&mut fx, 2, f, -4.0, 0.5);
        let doe = put(&mut fx, f, "dwell:doe", 2.0, 0.5, 0.0);
        let kind = fx
            .server
            .creatures()
            .iter()
            .find(|c| c.id == doe)
            .unwrap()
            .kind;
        let def = fx.server.creature_table().creatures[kind];
        let before = fx.server.loose_items().len();
        let mut swings = 0;
        while health(&fx, doe).is_some() && swings < 10 {
            // Back where it stood (the knockback pushed it).
            let at = f.point([2.0, m_of_row(BASE), 0.5]);
            if let Some(c) = fx.server.creatures_mut().get_mut(doe) {
                c.position = at;
                c.knock = [0.0; 2];
            }
            slash(&mut fx, f, false);
            swings += 1;
        }
        let w = sword().weapon.unwrap();
        assert_eq!(
            swings,
            (def.health / w.damage).ceil() as i32,
            "killed by its health"
        );
        assert!(fx.server.strikes().last().unwrap().killed);
        let dropped: Vec<_> = fx.server.loose_items()[before..]
            .iter()
            .map(|i| (i.stack.item.clone(), i.stack.count))
            .collect();
        let (item, min, max) = def.drops[0];
        assert_eq!(dropped.len(), 1, "{dropped:?}");
        assert_eq!(dropped[0].0, item);
        assert!((min..=max).contains(&u32::from(dropped[0].1)));
    }
);

dwell_world::both_faces!(
    fn a_creature_with_a_raised_sword_blocks_a_slash_across_it(f: FaceFrame) {
        let mut fx = scene(f, "adventure");
        move_to(&mut fx, 2, f, -4.0, 0.5);
        // Facing the swinger (face-local -x).
        let wolf = put(&mut fx, f, "dwell:wolf", 2.0, 0.5, -90.0);
        let whole = health(&fx, wolf).unwrap();
        let mut g = Guard::holding(SWORD, true);
        g.raised = true;
        assert!(fx.server.set_creature_guard(wolf, Some(g)));
        slash(&mut fx, f, false);
        let s = fx.server.strikes().last().unwrap().clone();
        assert_eq!(
            s.clash,
            Some(Clash::Blocked),
            "level across upright: blocked"
        );
        assert_eq!(health(&fx, wolf), Some(whole));
        // The recoil: the next swing waits it out, then a vertical slash along it connects.
        for _ in 0..(sword().weapon.unwrap().guard.recoil_s * f64::from(SIM_HZ)) as u32 {
            fx.server.step();
        }
        slash(&mut fx, f, true);
        let s = fx.server.strikes().last().unwrap().clone();
        assert_eq!(s.clash, Some(Clash::Connected), "vertical along upright");
        assert!(health(&fx, wolf).unwrap() < whole);
    }
);

// --- Round 2: swordplay on the wire, the player's block, NPCs that fight back. ---

/// Client 2 says the `weapons.clash` capability (this build's client); client 1 says none (an
/// older build). Player 1 says it holds the sword, level.
fn swordplay_scene(f: FaceFrame) -> Fixture {
    let mut fx = scene(f, "adventure");
    fx.send(
        2,
        Message::Capabilities(dwell_protocol::capabilities::Capabilities::this_client()),
    );
    fx.send(
        1,
        Message::Chop(ChopAction::Held {
            tool: SWORD.into(),
            vertical: false,
        }),
    );
    fx.server.step();
    fx.take_all();
    fx
}

fn swordplay(msgs: &[Message]) -> Vec<SwordplayMessage> {
    msgs.iter()
        .filter_map(|m| match m {
            Message::Swordplay(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn clashes(msgs: &[Message]) -> Vec<ClashNet> {
    swordplay(msgs)
        .into_iter()
        .filter_map(|s| match s {
            SwordplayMessage::Clash(c) => Some(c),
            _ => None,
        })
        .collect()
}

/// Player 1 looks along face-local +x (yaw 90: toward the creatures put ahead of it) for `ticks`
/// ticks, an input frame a tick (as a client sends them), stepping the server.
fn facing_ahead(fx: &mut Fixture, ticks: u32) {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let input = dwell_physics::controller::Input {
        look_yaw: 90.0,
        ..Default::default()
    };
    for _ in 0..ticks {
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let m = Message::PlayerInput(PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![dwell_physics::net::quantize_input(&input, seq)],
        });
        fx.server.on_datagram(1, &m.encode());
        fx.server.step();
    }
}

dwell_world::both_faces!(
    fn a_strikes_outcome_reaches_a_capable_client_near_and_never_an_older_one(f: FaceFrame) {
        let mut fx = swordplay_scene(f);
        move_to(&mut fx, 2, f, 0.5, 3.0);
        let wolf = put(&mut fx, f, "dwell:wolf", 2.0, 0.5, -90.0);
        let mut g = Guard::holding(SWORD, true);
        g.raised = true;
        assert!(fx.server.set_creature_guard(wolf, Some(g)));
        fx.take_all();
        slash(&mut fx, f, false);
        let all = fx.take_all();
        let got = clashes(&all[&2]);
        assert_eq!(got.len(), 1, "one strike, one outcome: {got:?}");
        let c = &got[0];
        assert_eq!(c.outcome, Clash::Blocked);
        assert_eq!(c.striker, Fighter::Player(1));
        assert_eq!(c.defender, Fighter::Creature(wolf));
        assert_eq!(c.weapon, SWORD);
        assert!(!c.striker_vertical && c.defender_vertical);
        // Where the blades met: between the two, on this face.
        assert_eq!(FaceFrame::at_height(f64::from(c.point[1])), f);
        let local = f.point(c.point.map(f64::from));
        assert!((0.5..2.0).contains(&local[0]), "{local:?}");
        assert!(
            swordplay(all.get(&1).map_or(&[][..], |v| v)).is_empty(),
            "an older client is sent no Swordplay (it would refuse the type)"
        );
    }
);

dwell_world::both_faces!(
    fn a_clash_goes_only_to_capable_clients_within_hear_m(f: FaceFrame) {
        let mut fx = swordplay_scene(f);
        let hear = sword().weapon.unwrap().guard.clash.hear_m;
        move_to(&mut fx, 2, f, 0.5 + hear + 4.0, 0.5);
        let wolf = put(&mut fx, f, "dwell:wolf", 2.0, 0.5, -90.0);
        let _ = wolf;
        for _ in 0..5 {
            fx.server.step();
        }
        fx.take_all();
        slash(&mut fx, f, false);
        let all = fx.take_all();
        assert!(clashes(all.get(&2).map_or(&[][..], |v| v)).is_empty());
        assert_eq!(fx.server.strikes().len(), 1, "it was struck all the same");
    }
);

dwell_world::both_faces!(
    fn the_players_block_rises_and_drops_by_input_and_its_own_swing(f: FaceFrame) {
        let mut fx = swordplay_scene(f);
        assert_eq!(fx.server.player_guard(1), Some((false, false)));
        fx.send(1, Message::Chop(ChopAction::Guard { raised: true }));
        fx.server.step();
        assert_eq!(
            fx.server.player_guard(1),
            Some((false, true)),
            "raised, level"
        );
        // The others with the capability see it raised; the older client hears nothing.
        let all = fx.take_all();
        assert_eq!(
            swordplay(&all[&2]),
            vec![SwordplayMessage::Guards(vec![GuardNet {
                who: Fighter::Player(1),
                vertical: false,
                raised: true,
            }])]
        );
        // Its own swing drops the block while it runs; held, it is back after.
        fx.send(
            1,
            Message::Chop(ChopAction::Sweep {
                tool: SWORD.into(),
                look: f.vector([1.0, 0.0, 0.0]).map(|v| v as f32),
                vertical: false,
            }),
        );
        fx.server.step();
        assert_eq!(fx.server.player_guard(1), Some((false, false)), "mid-swing");
        for _ in 0..swing_ticks() {
            fx.server.step();
        }
        assert_eq!(fx.server.player_guard(1), Some((false, true)));
        // The stance turns with the hold; let go, it drops; put away, no guard at all.
        fx.send(
            1,
            Message::Chop(ChopAction::Held {
                tool: SWORD.into(),
                vertical: true,
            }),
        );
        fx.server.step();
        assert_eq!(fx.server.player_guard(1), Some((true, true)));
        fx.send(1, Message::Chop(ChopAction::Guard { raised: false }));
        fx.server.step();
        assert_eq!(fx.server.player_guard(1), Some((true, false)));
        fx.send(1, Message::Chop(ChopAction::Guard { raised: true }));
        fx.send(
            1,
            Message::Chop(ChopAction::Held {
                tool: String::new(),
                vertical: true,
            }),
        );
        fx.server.step();
        assert_eq!(fx.server.player_guard(1), None);
        fx.send(1, Message::Chop(ChopAction::Guard { raised: true }));
        fx.server.step();
        assert_eq!(fx.server.player_guard(1), None, "no weapon, no block");
    }
);

/// A villager (holding its data's sword) at face-local (1.6, 0.5) facing player 1, turned on it.
fn foe_villager(fx: &mut Fixture, f: FaceFrame) -> u32 {
    move_to(fx, 2, f, 0.5, 4.0);
    let v = put(fx, f, "dwell:villager", 1.6, 0.5, -90.0);
    // Its own mind (not driven by a herd): its fight's give-up counts down.
    fx.server.creatures_mut().get_mut(v).unwrap().driven = false;
    let eye = f.point([0.5, m_of_row(BASE) + 1.6, 0.5]);
    let cs = fx.server.creatures_mut();
    assert!(cs.attacked(v, eye), "the villager fights back (data)");
    cs.set_foe_player(v, 1);
    v
}

fn villager_guard(fx: &Fixture, v: u32) -> Guard {
    fx.server
        .creatures()
        .iter()
        .find(|c| c.id == v)
        .and_then(|c| c.guard)
        .expect("its sword")
}

dwell_world::both_faces!(
    fn an_npc_blocks_the_wind_up_by_counter_then_strikes_back_unhurting(f: FaceFrame) {
        let mut fx = swordplay_scene(f);
        facing_ahead(&mut fx, 2);
        let v = foe_villager(&mut fx, f);
        assert!(
            !villager_guard(&fx, v).raised,
            "lowered until a blade comes"
        );
        let healths = [fx.server.health_of(1), fx.server.health_of(2)];
        fx.take_all();
        // A level slash: on its wind-up the villager raises the stance that blocks it (vertical).
        fx.send(
            1,
            Message::Chop(ChopAction::Sweep {
                tool: SWORD.into(),
                look: f.vector([1.0, 0.0, 0.0]).map(|v| v as f32),
                vertical: false,
            }),
        );
        facing_ahead(&mut fx, 3);
        let g = villager_guard(&fx, v);
        assert!(
            g.raised && g.vertical,
            "counter: upright across a level slash: {g:?}"
        );
        // The clients with the capability see it raised.
        let all = fx.take_all();
        assert!(
            swordplay(&all[&2]).contains(&SwordplayMessage::Guards(vec![GuardNet {
                who: Fighter::Creature(v),
                vertical: true,
                raised: true,
            }]))
        );
        facing_ahead(&mut fx, swing_ticks());
        assert_eq!(
            fx.server.strikes().last().unwrap().clash,
            Some(Clash::Blocked)
        );
        // In the recovery it swung back: player 1's block was down, so it connected, and hurt
        // nobody (an NPC can't hurt a player; the owner's open question).
        let n = fx.server.npc_strikes().to_vec();
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!((n[0].npc, n[0].player_id), (v, 1));
        assert_eq!(n[0].outcome, Some(Clash::Connected));
        assert!(n[0].vertical, "in its stance");
        assert_eq!([fx.server.health_of(1), fx.server.health_of(2)], healths);
        let all = fx.take_all();
        assert!(
            clashes(&all[&2])
                .iter()
                .any(|c| c.striker == Fighter::Creature(v)
                    && c.defender == Fighter::Player(1)
                    && c.outcome == Clash::Connected)
        );
        // Its wait over, again: now player 1 holds its level block, facing it, and the villager's
        // upright blow across it is blocked.
        let fight = fx
            .server
            .creature_table()
            .npc("dwell:villager")
            .unwrap()
            .fight;
        facing_ahead(&mut fx, (fight.cooldown_s * f64::from(SIM_HZ)) as u32);
        fx.send(1, Message::Chop(ChopAction::Guard { raised: true }));
        fx.send(
            1,
            Message::Chop(ChopAction::Sweep {
                tool: SWORD.into(),
                look: f.vector([1.0, 0.0, 0.0]).map(|v| v as f32),
                vertical: false,
            }),
        );
        facing_ahead(&mut fx, swing_ticks());
        facing_ahead(&mut fx, swing_ticks());
        let n = fx.server.npc_strikes().to_vec();
        assert_eq!(n.len(), 2, "{n:?}");
        assert_eq!(
            n[1].outcome,
            Some(Clash::Blocked),
            "upright on a level block"
        );
        assert_eq!([fx.server.health_of(1), fx.server.health_of(2)], healths);
    }
);

dwell_world::both_faces!(
    fn an_npc_gives_up_its_fight_after_give_up_s_without_a_hit(f: FaceFrame) {
        let mut fx = swordplay_scene(f);
        let v = foe_villager(&mut fx, f);
        let fight = fx
            .server
            .creature_table()
            .npc("dwell:villager")
            .unwrap()
            .fight;
        let ticks = (fight.give_up_s * f64::from(SIM_HZ)) as u32;
        for _ in 0..ticks - 5 {
            fx.server.step();
        }
        let foe = |fx: &Fixture| {
            fx.server
                .creatures()
                .iter()
                .find(|c| c.id == v)
                .unwrap()
                .foe_player
        };
        assert_eq!(foe(&fx), Some(1), "still fighting");
        for _ in 0..10 {
            fx.server.step();
        }
        assert_eq!(foe(&fx), None, "given up");
        assert!(!villager_guard(&fx, v).raised);
    }
);
