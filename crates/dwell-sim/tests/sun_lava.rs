//! The sun's lava (docs/proposals/SUN_WORLD.md §6, the second slice; owner: "Constant flowing lava, oceans
//! of lava", "Lava has the same function on the sun as the earth world"): a player on the sun burns in
//! its generated lava by the same rules as home's; its rivers are kept flowing by lava fields fed at
//! each river head (grids.json `flow`), placed and run only near a player there, within the data's
//! caps, both faces.

mod common;

use common::{Client, Fixture, flat};
use dwell_protocol::capabilities::Capabilities;
use dwell_protocol::players;
use dwell_protocol::{
    DamageCause, InputFrame, Message, PlayerEventKind, PlayerInput, input_buttons,
};
use dwell_sim::server::ServerConfig;
use dwell_world::FaceFrame;
use dwell_worldgen::grids::{grid, grids};
use dwell_worldgen::volcanic::VolcanicTerrain;

type V3 = [f64; 3];
const SECOND: u32 = 60;

fn sun_index() -> u8 {
    grids().iter().position(|g| g.name == "dwell:sun").unwrap() as u8
}

fn input(fx: &mut Fixture, seq: u32, buttons: u16) {
    let m = Message::PlayerInput(PlayerInput {
        last_snapshot_tick: 0,
        inputs: vec![InputFrame {
            seq,
            move_x: 0,
            move_y: 0,
            buttons,
            yaw: 0,
            pitch: 0,
        }],
    });
    fx.server.on_datagram(1, &m.encode());
}

/// A server with one client of this build (it says `grid.sun`), its player captured into the sun's
/// space over face `f` (flying, in creative), then the world's mode set to `mode`.
fn on_the_sun(f: FaceFrame, mode: &str) -> (Fixture, VolcanicTerrain, u32) {
    let mut fx = Fixture::new(ServerConfig {
        mode: "creative".into(),
        ..flat()
    });
    fx.join(1, &Client::new(1));
    fx.send(1, Message::Capabilities(Capabilities::this_build()));
    fx.send(1, Message::WorldgenCheck(0));
    let sun = grid("dwell:sun").unwrap();
    let slot = fx
        .server
        .grids()
        .slots
        .iter()
        .position(|s| s.index == sun_index())
        .unwrap();
    let mut seq = 1;
    while fx
        .server
        .grid_state_of(1)
        .is_none_or(|g| g.0 != sun_index())
    {
        let t = f64::from(fx.server.tick()) / 60.0;
        let local = f.point([0.0, sun.capture_m.unwrap() - 1_000.0, 0.0]);
        let (wp, wv) = fx.server.grids().frame(slot, t).release(local, [0.0; 3]);
        assert!(fx.server.place_on_grid(1, wp, wv));
        input(&mut fx, seq, input_buttons::FLY);
        fx.server.step();
        fx.server.take_outbox();
        seq += 1;
        assert!(seq < 600, "never captured by the sun");
    }
    assert!(fx.server.set_mode(mode));
    let seed = fx.server.config().world_seed;
    (
        fx,
        VolcanicTerrain::for_grid("dwell:sun", seed).unwrap(),
        seq,
    )
}

/// Steps `n` ticks with an input each (walking off, flight released).
fn step(fx: &mut Fixture, seq: &mut u32, n: u32) -> Vec<Message> {
    let mut got = Vec::new();
    for _ in 0..n {
        input(fx, *seq, 0);
        *seq += 1;
        fx.server.step();
        for o in fx.server.take_outbox() {
            if let Some(m) = Message::decode(&o.bytes) {
                got.push(m);
            }
        }
    }
    got
}

/// The sun's first river head from its spawn on face `f`, and its column.
fn a_river_head(t: &VolcanicTerrain, f: FaceFrame) -> (V3, dwell_worldgen::volcanic::Column) {
    let s = t.spawn_point(f.face);
    let h = *t
        .river_heads(f.face, s[0], s[2], 4_000.0)
        .first()
        .expect("a river head within 4 km of the spawn");
    ([h.x, h.lava, h.z], t.column(f.face, h.x, h.z))
}

dwell_world::both_faces!(
    /// "Lava has the same function on the sun as the earth world": stood in the sun's generated
    /// river lava a player takes lava's damage as `Burn`, burns on after, and dies there in time;
    /// its death takes it home, where it stands up at the world's spawn.
    fn a_player_in_the_sun_s_lava_burns_and_dies_by_home_s_rules(f: FaceFrame) {
        let (mut fx, t, mut seq) = on_the_sun(f, "adventure");
        let full = players::MAX_HEALTH as i32;
        assert_eq!(fx.server.health_of(1), full);
        let (head, col) = a_river_head(&t, f);
        assert!(col.lava > col.ground + 1.0, "a river: {col:?}");
        // In the river, its feet a metre under the lava's surface.
        let at = f.point([head[0], col.lava - 1.0 + 0.9, head[2]]);
        assert!(fx.server.place_on_grid(1, at, [0.0; 3]));
        fx.send(1, Message::CausesHello(DamageCause::MAX));
        let got = step(&mut fx, &mut seq, SECOND / 4);
        let hurt = fx.server.health_of(1);
        assert!(
            hurt < full && hurt > 0,
            "in the sun's lava: {hurt} of {full}"
        );
        assert!(fx.server.burning(1));
        assert!(
            got.iter().any(|m| matches!(m, Message::PlayerEvent(e)
                if e.kind == PlayerEventKind::Damage && e.cause == DamageCause::Burn)),
            "the damage is lava's Burn"
        );
        // Out onto dry crust: burning on a while.
        let s = t.spawn_point(f.face);
        let dry = f.point([s[0], s[1] + 1.0, s[2]]);
        assert!(fx.server.place_on_grid(1, dry, [0.0; 3]));
        let left = fx.server.health_of(1);
        step(&mut fx, &mut seq, 2 * SECOND);
        assert!(fx.server.health_of(1) < left, "burning on after the lava");
        // Back in, held there: it dies, and is taken out of the sun's space.
        let mut died = false;
        let mut unloaded = false;
        let unload = |got: &[Message]| {
            got.iter()
                .any(|m| matches!(m, Message::ChunkUnload(u) if u.grid == sun_index()))
        };
        for _ in 0..10 * SECOND {
            assert!(fx.server.place_on_grid(1, at, [0.0; 3]) || died);
            let got = step(&mut fx, &mut seq, 1);
            unloaded |= unload(&got);
            if got.iter().any(|m| {
                matches!(m, Message::PlayerEvent(e)
                if e.kind == PlayerEventKind::Death && e.cause == DamageCause::Burn)
            }) {
                died = true;
                break;
            }
        }
        assert!(died, "died in the sun's lava");
        assert!(fx.server.grid_state_of(1).is_none(), "dead: on no grid");
        let got = step(&mut fx, &mut seq, 6 * SECOND);
        assert!(
            unloaded || unload(&got),
            "the sun's chunks unloaded on its client"
        );
        let (g, _, _) = fx.server.grid_state_of(1).expect("respawned");
        assert_eq!(g, 0, "up again at home");
        assert_eq!(fx.server.health_of(1), full);
    }
);

dwell_world::both_faces!(
    /// "Constant flowing lava": near a player on the sun, a source is placed at each river head
    /// within `flow.nearM` (none before a player is there), the river runs from it downhill (its
    /// field molten past where it started, the chunks holding its hot cells), still flowing a
    /// minute on, never more fields than `maxFields` nor tiles than `maxTiles`, and the time a lava
    /// tick takes stays small.
    fn the_sun_s_rivers_flow_from_their_heads_near_a_player(f: FaceFrame) {
        let (mut fx, t, mut seq) = on_the_sun(f, "creative");
        let fl = t.rules().flow.clone().expect("the sun's flow rules");
        let slot = |fx: &Fixture| {
            fx.server
                .grids()
                .slots
                .iter()
                .find(|s| s.index == sun_index())
                .unwrap()
                .lava
                .clone()
        };
        let (head, col) = a_river_head(&t, f);
        // Away from every river head: the captured spot (high over the middle) places none.
        assert!(slot(&fx).is_empty(), "no source before a player is near");
        // Stand on the bank by the head (flying, held there).
        let at = f.point([head[0], col.ground.max(col.lava) + 3.0, head[2]]);
        let mut worst_ms = 0.0_f64;
        for _ in 0..60 {
            assert!(fx.server.place_on_grid(1, at, [0.0; 3]));
            input(&mut fx, seq, input_buttons::FLY);
            seq += 1;
            let t0 = std::time::Instant::now();
            fx.server.step();
            worst_ms = worst_ms.max(t0.elapsed().as_secs_f64() * 1e3);
            fx.server.take_outbox();
        }
        let lava = slot(&fx);
        assert!(!lava.is_empty(), "a source at the river head");
        assert!(lava.fields.len() <= fl.max_fields as usize);
        let p = lava
            .fields
            .iter()
            .find(|p| {
                p.face == f && {
                    let v = p.field.origin;
                    (f64::from(v[0]) - head[0]).abs() < 1.5
                        && (f64::from(v[1]) - head[2]).abs() < 1.5
                }
            })
            .expect("the field at this river's head, on this face");
        let molten0 = p.field.depth.iter().filter(|d| **d > 0.0).count();
        // Two lava minutes later (held by the head).
        let mut basalt_sent = false;
        let basalt = dwell_blocks::require("dwell:basalt");
        for _ in 0..120 * SECOND / 60 {
            for _ in 0..60 {
                assert!(fx.server.place_on_grid(1, at, [0.0; 3]));
                input(&mut fx, seq, input_buttons::FLY);
                seq += 1;
                let t0 = std::time::Instant::now();
                fx.server.step();
                worst_ms = worst_ms.max(t0.elapsed().as_secs_f64() * 1e3);
                for o in fx.server.take_outbox() {
                    if let Some(Message::ChunkData(c)) = Message::decode(&o.bytes)
                        && c.grid == sun_index()
                    {
                        basalt_sent |= c.voxels.iter().any(|&m| f.state(m) == basalt);
                    }
                }
            }
        }
        let lava = slot(&fx);
        let p = lava
            .fields
            .iter()
            .find(|p| {
                let v = p.field.origin;
                p.face == f
                    && (f64::from(v[0]) - head[0]).abs() < 1.5
                    && (f64::from(v[1]) - head[2]).abs() < 1.5
            })
            .expect("the head's field still placed");
        let molten = p.field.depth.iter().filter(|d| **d > 0.0).count();
        assert!(
            molten > molten0 + 20,
            "the river ran on from its head: {molten0} → {molten} molten columns"
        );
        assert!(p.field.tiles().len() <= fl.max_tiles as usize);
        let at_vent = p.field.slot(0, 0).unwrap();
        assert!(p.field.depth[at_vent] > 0.0, "still flowing at its head");
        // Constant flowing lava stays molten (owner, 2026-11-13: lava hardens when it stops, not
        // while flowing): the head's river is all held hot, and no rock was written into the sun's
        // chunks (a flowing river over the rift's own hot lava changes no cell, so none is sent).
        assert!(!basalt_sent, "the sun's flowing river hardened");
        let flowing = (0..p.field.depth.len()).filter(|&i| p.field.depth[i] > 0.0);
        let hot = flowing.clone().filter(|&i| p.field.heat[i] >= 1.0).count();
        assert!(
            hot * 10 >= flowing.count() * 9,
            "the river stays molten: {hot} hot columns"
        );
        // Its hot cells are in the sun's chunks.
        let [x, z] = p.field.origin;
        let rows = p.field.lava_rows(at_vent);
        assert!(!rows.is_empty());
        let cell = f.cell([x, rows.start, z]);
        let m = fx.server.grid_voxel(sun_index(), cell).unwrap();
        assert!(
            dwell_sim::lava::hot_fluid_of(m).is_some(),
            "lava in the sun's chunk at its head"
        );
        assert_eq!(
            m,
            dwell_sim::lava::heat_states("dwell:lava")[0],
            "the hottest stop at its head"
        );
        assert!(lava.fields.len() <= fl.max_fields as usize);
        // A tick with the sun's lava stays inside a frame's share (native debug build).
        assert!(worst_ms < 250.0, "the worst tick took {worst_ms:.1} ms");
    }
);
