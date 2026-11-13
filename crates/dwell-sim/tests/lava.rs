//! Lava in the server (docs/proposals/VOLCANOES.md §6, V1 slice 2), on both faces: a vent placed in
//! a fixture world fills its crater and the chunks (and the clients' modifications) show lava;
//! cooled lava is basalt in the chunks; a player in lava loses health and burns on after; the lava
//! is saved with the world and loads again; the default world has none; lava's damage is `Burn` to a
//! client that knows it and `Fall` to an older one; an edit inside a field changes its bed (a dug
//! channel lets the lava in).

mod common;

use common::*;
use dwell_blocks::require;
use dwell_lava::VentKind;
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::*;
use dwell_sim::lava::VentSpot;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

/// The scene's ground level (face-local row): its stone's top row is `BASE − 1`.
const BASE: i32 = FLAT_TOP_Y + 40;
/// The scene's half-width (columns).
const HALF: i32 = 12;
/// The crater: columns within this radius of (0, 0), dug `CRATER_ROWS` rows into the stone.
const CRATER_R2: i32 = 9;
const CRATER_ROWS: i32 = 4;
const SECOND: u32 = SIM_HZ;

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        // Beside the crater, not in it.
        spawn: Some(f.point([8.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        ..flat()
    }
}

/// Stone `HALF` columns each way, rows `BASE − 8 .. BASE`, with the crater dug in it when `crater`.
fn build(fx: &mut Fixture, f: FaceFrame, crater: bool) {
    let stone = f.state(require("dwell:stone"));
    for x in -HALF..=HALF {
        for z in -HALF..=HALF {
            let dug = crater && x * x + z * z <= CRATER_R2;
            for r in BASE - 8..BASE {
                let open = dug && r >= BASE - CRATER_ROWS;
                let [x, y, z] = f.cell([x, r, z]);
                fx.server
                    .world()
                    .set_voxel(x, y, z, if open { 0 } else { stone });
            }
        }
    }
}

fn scene(f: FaceFrame, mode: &str, crater: bool, store: Option<WorldStore>) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        store,
        ..config(f, mode)
    });
    build(&mut fx, f, crater);
    fx.join(1, &Client::new(1));
    // The scene streamed to the client first (full chunks: `WorldgenCheck(0)`), so its changes
    // come as modifications.
    fx.send(1, Message::WorldgenCheck(0));
    let streamed = fx.chunks(1, 2 * SECOND as i32, None);
    assert!(!streamed.is_empty());
    fx.take_all();
    fx
}

fn step(fx: &mut Fixture, n: u32) {
    for _ in 0..n {
        fx.server.step();
    }
}

fn lake() -> VentSpot {
    VentSpot {
        fluid: "dwell:lava".into(),
        kind: VentKind::Lake,
        rate_m3s: 2.0,
        depth_m: 1.5,
        radius: 6,
    }
}

/// The face-local block at a face-local cell.
fn at(fx: &mut Fixture, f: FaceFrame, c: [i32; 3]) -> u16 {
    let [x, y, z] = f.cell(c);
    f.state(fx.server.world().get_voxel(x, y, z))
}

/// The materials the client was sent in voxel modifications.
fn sent(all: &std::collections::BTreeMap<SessionId, Vec<Message>>, id: SessionId) -> Vec<u16> {
    all.get(&id)
        .into_iter()
        .flatten()
        .filter_map(|m| match m {
            Message::VoxelModification(v) => Some(v),
            _ => None,
        })
        .flat_map(|v| {
            v.chunks
                .iter()
                .flat_map(|c| c.changes.iter().map(|ch| ch.material))
        })
        .collect()
}

/// A crater with a lake vent, filled for `secs` with the player beside it.
fn filled(f: FaceFrame, mode: &str, secs: u32, store: Option<WorldStore>) -> Fixture {
    let mut fx = scene(f, mode, true, store);
    fx.server
        .place_lava_vent(f, [0, 0], BASE, &lake())
        .expect("a vent on the crater's floor");
    step(&mut fx, secs * SECOND);
    fx
}

dwell_world::both_faces!(
    fn a_lake_vent_fills_its_crater_and_the_chunks_and_clients_show_lava(f: FaceFrame) {
        let lava = require("dwell:lava");
        let mut fx = scene(f, "creative", true, None);
        fx.server
            .place_lava_vent(f, [0, 0], BASE, &lake())
            .expect("a vent on the crater's floor");
        step(&mut fx, 40 * SECOND);
        let all = fx.take_all();
        // The crater's floor is BASE − 4; the lake held 1.5 m (3 rows) over it, below the rim.
        for (x, z) in [(0, 0), (2, 0), (0, -2), (-2, 2)] {
            for r in BASE - CRATER_ROWS..BASE - 1 {
                assert_eq!(at(&mut fx, f, [x, r, z]), lava, "lava at ({x}, {r}, {z})");
            }
            assert_eq!(
                at(&mut fx, f, [x, BASE - 1, z]),
                0,
                "the rim's row stays open"
            );
        }
        // Held in the crater: none over the rim, and the crater's own lake held molten.
        assert_eq!(at(&mut fx, f, [6, BASE, 0]), 0);
        assert!(
            sent(&all, 1).contains(&f.state(lava)),
            "the client was sent the lava cells"
        );
        let field = &fx.server.lava().fields[0].field;
        assert!(field.stored() > 30.0, "{} m³", field.stored());
    }
);

/// The trench: columns x 0..=11, z −1..=1, dug `TRENCH_ROWS` rows (1 m) into the stone.
const TRENCH_ROWS: i32 = 2;

/// A trench scene with a river vent at its x = 0 end, poured for `secs`, then turned off (rate 0:
/// the vent stays a source, `sourceKeepM`).
fn trench(f: FaceFrame, secs: u32) -> Fixture {
    let mut fx = scene(f, "creative", false, None);
    for x in 0..=11 {
        for z in -1..=1 {
            for r in BASE - TRENCH_ROWS..BASE {
                let [x, y, z] = f.cell([x, r, z]);
                fx.server.world().set_voxel(x, y, z, 0);
            }
        }
    }
    let river = VentSpot {
        fluid: "dwell:lava".into(),
        kind: VentKind::River,
        rate_m3s: 0.5,
        depth_m: 0.0,
        radius: 8,
    };
    fx.server
        .place_lava_vent(f, [0, 0], BASE - TRENCH_ROWS, &river)
        .expect("a vent in the trench");
    step(&mut fx, secs * SECOND);
    fx.server.scale_lava_rate(0.0);
    fx
}

dwell_world::both_faces!(
    fn lava_that_stops_is_basalt_in_the_chunks_and_by_its_source_stays_lava(f: FaceFrame) {
        // Owner, 2026-11-13: lava hardens when it stops moving, not by expiring; lava near a
        // source block stays molten. The trench's still lava past `sourceKeepM` is basalt where
        // the lava was; by the vent it is still lava.
        let basalt = require("dwell:basalt");
        let lava = require("dwell:lava");
        let fl = dwell_lava::DATA.fluid("dwell:lava").unwrap();
        let mut fx = trench(f, 20);
        fx.take_all();
        let row = BASE - TRENCH_ROWS;
        let is = |fx: &mut Fixture, x: i32, b| {
            let m = at(fx, f, [x, row, 0]);
            m != 0 && dwell_blocks::info(f.state(m)).block == dwell_blocks::info(b).block
        };
        let keep = fl.source_keep_m as i32;
        // Viscous lava levels along the trench for a few minutes (flowing: molten), then rests
        // and hardens `restSeconds` later.
        let mut all = std::collections::BTreeMap::new();
        let mut rock = 0;
        for _ in 0..300 {
            step(&mut fx, SECOND);
            for (k, v) in fx.take_all() {
                all.entry(k).or_insert_with(Vec::new).extend(v);
            }
            rock = (keep + 1..=11).filter(|&x| is(&mut fx, x, basalt)).count();
            if rock >= 2 {
                break;
            }
        }
        assert!(rock >= 2, "basalt where the lava stopped: {rock} columns");
        for x in 0..=keep {
            assert!(is(&mut fx, x, lava), "lava by the source at x {x}");
        }
        assert!(
            sent(&all, 1).contains(&f.state(basalt)),
            "and sent to the client"
        );
    }
);

dwell_world::both_faces!(
    fn lava_cells_show_their_columns_heat_stop_hot_by_the_source_darker_as_it_rests(f: FaceFrame) {
        // Owner, 2026-11-11: "It should darken as it solidifies." Each column's cells are the
        // `heat` state of its stop (VOLCANOES.md §6.5), rewritten as it rests toward hardening.
        let stops = dwell_sim::lava::heat_states("dwell:lava");
        assert_eq!(stops.len(), dwell_lava::DATA.heat.len());
        assert!(stops.windows(2).all(|w| w[0] != w[1]), "one state per stop");
        let mut fx = trench(f, 20);
        fx.take_all();
        // Until the still lava past the source's reach has darkened (rested), not yet hardened.
        let mut all = std::collections::BTreeMap::new();
        for _ in 0..300 {
            step(&mut fx, SECOND);
            for (k, v) in fx.take_all() {
                all.entry(k).or_insert_with(Vec::new).extend(v);
            }
            let field = &fx.server.lava().fields[0].field;
            if field
                .columns()
                .any(|i| field.depth[i] > 0.25 && field.heat[i] < 0.5)
            {
                break;
            }
        }
        let mut seen = vec![0; stops.len()];
        let field = fx.server.lava().fields[0].field.clone();
        for x in -8..=12 {
            for z in -8..=8 {
                for r in BASE - TRENCH_ROWS..BASE + 4 {
                    let m = at(&mut fx, f, [x, r, z]);
                    if let Some(k) = stops.iter().position(|&s| f.state(s) == m) {
                        seen[k] += 1;
                        // The cell's stop is its column's heat's, within the hysteresis.
                        let i = field.slot(x, z).expect("a column of the field");
                        let h = field.heat[i];
                        let d = &dwell_lava::DATA;
                        let lo = d.heat[k].above - d.heat_hysteresis;
                        let hi = if k == 0 {
                            2.0
                        } else {
                            d.heat[k - 1].above + d.heat_hysteresis
                        };
                        assert!(
                            h >= lo && h < hi || h == 0.0,
                            "({x}, {r}, {z}) {} at heat {h}",
                            d.heat[k].name
                        );
                    }
                }
            }
        }
        assert!(seen[0] > 0, "hot lava by the vent: {seen:?}");
        assert!(
            seen[1..].iter().sum::<i32>() > 0,
            "darker lava where it rests: {seen:?}"
        );
        assert!(
            stops[1..]
                .iter()
                .any(|&s| sent(&all, 1).contains(&f.state(s))),
            "the client was sent a darker stop"
        );
    }
);

dwell_world::both_faces!(
    fn a_player_in_lava_loses_health_and_burns_on_after(f: FaceFrame) {
        let full = players::MAX_HEALTH as i32;
        let mut fx = filled(f, "adventure", 30, None);
        assert_eq!(fx.server.health_of(1), full, "beside the lake: unhurt");
        // Into the lake, on its floor.
        let h = fx.server.player_handle_of(1).expect("a live player");
        let (_, physics, players) = fx.server.sim();
        let half = f64::from(players.half_height(h));
        let into = f.point([0.5, m_of_row(BASE - CRATER_ROWS) + half + 0.05, 0.5]);
        players.teleport(physics, h, DVec3::from(into), Vec3::ZERO);
        step(&mut fx, SECOND / 4);
        let hurt = fx.server.health_of(1);
        assert!(hurt < full && hurt > 0, "in lava: hurt, {hurt} of {full}");
        assert!(fx.server.burning(1));
        // Out onto the ground beside it: still burning a while, then not.
        let (_, physics, players) = fx.server.sim();
        let out = f.point([8.5, m_of_row(BASE) + half + 0.05, 0.5]);
        players.teleport(physics, h, DVec3::from(out), Vec3::ZERO);
        step(&mut fx, 1);
        let left = fx.server.health_of(1);
        step(&mut fx, 2 * SECOND);
        assert!(
            fx.server.health_of(1) < left,
            "burning on after leaving the lava"
        );
        step(&mut fx, 2 * SECOND);
        assert!(!fx.server.burning(1), "the burn is over");
        let after = fx.server.health_of(1);
        step(&mut fx, SECOND);
        assert_eq!(fx.server.health_of(1), after, "and the damage stops");
    }
);

dwell_world::both_faces!(
    fn the_lava_is_saved_with_the_world(f: FaceFrame) {
        let lava = require("dwell:lava");
        let db = WorldDb::in_memory().expect("db");
        let store = || Some(WorldStore::new(db.clone(), false));
        let stored = {
            let mut fx = filled(f, "creative", 40, store());
            fx.server.save_now();
            fx.server.flush_store();
            fx.server.lava().fields[0].field.stored()
        };
        assert!(db.lava().is_some(), "a lava record in the world's table");
        // Loaded again (the scene not rebuilt: its chunks come from the save).
        let mut fx = Fixture::new(ServerConfig {
            store: store(),
            ..config(f, "creative")
        });
        let fields = &fx.server.lava().fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].face, f);
        assert!((fields[0].field.stored() - stored).abs() < 1e-3);
        assert_eq!(
            at(&mut fx, f, [0, BASE - CRATER_ROWS, 0]),
            lava,
            "its cells as saved"
        );
        // And it runs on.
        fx.join(1, &Client::new(1));
        step(&mut fx, 5 * SECOND);
        assert_eq!(at(&mut fx, f, [0, BASE - CRATER_ROWS, 0]), lava);
    }
);

/// The damage causes a client heard.
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

dwell_world::both_faces!(
    fn lava_hurts_as_burn_and_an_older_client_never_hears_burn(f: FaceFrame) {
        let mut fx = filled(f, "adventure", 30, None);
        // Client 1 knows every cause of this build; client 2 is an older one (no `CausesHello`).
        fx.send(1, Message::CausesHello(DamageCause::MAX));
        fx.join(2, &Client::new(2));
        fx.take_all();
        let h = fx.server.player_handle_of(1).expect("a live player");
        let (_, physics, players) = fx.server.sim();
        let half = f64::from(players.half_height(h));
        let into = f.point([0.5, m_of_row(BASE - CRATER_ROWS) + half + 0.05, 0.5]);
        players.teleport(physics, h, DVec3::from(into), Vec3::ZERO);
        step(&mut fx, SECOND / 2);
        let all = fx.take_all();
        let heard = damage_causes(&all, 1);
        assert!(!heard.is_empty(), "hurt in the lava");
        assert!(heard.iter().all(|c| *c == DamageCause::Burn), "{heard:?}");
        let older = damage_causes(&all, 2);
        assert_eq!(older.len(), heard.len());
        assert!(older.iter().all(|c| *c == DamageCause::Fall), "{older:?}");
        // A client that says it knows only the causes before `Burn` (`Drown`'s build) hears `Fall`.
        fx.send(1, Message::CausesHello(DamageCause::Drown as u8));
        step(&mut fx, SECOND / 2);
        let heard = damage_causes(&fx.take_all(), 1);
        assert!(!heard.is_empty() && heard.iter().all(|c| *c == DamageCause::Fall));
    }
);

dwell_world::both_faces!(
    fn a_channel_dug_from_the_crater_lets_the_lava_flow_into_it(f: FaceFrame) {
        let lava = f.state(require("dwell:lava"));
        let mut fx = filled(f, "creative", 30, None);
        // The crater's rim at z = 0 is x = 4; the channel: x 4..=6 down to the crater's floor.
        let field_bed = |fx: &Fixture, x: i32| {
            let p = &fx.server.lava().fields[0];
            let o = p.field.origin;
            p.field.ground[p.field.at((x - o[0]) as usize, (0 - o[1]) as usize)]
        };
        assert_eq!(field_bed(&fx, 5), m_of_row(BASE) as f32, "the stone's top");
        assert_ne!(at(&mut fx, f, [5, BASE - CRATER_ROWS, 0]), lava);
        // Dug nearest the player first, in passes (a cell is broken once the player sees it).
        for (x, r) in (0..3).flat_map(|_| {
            (4..=6)
                .rev()
                .flat_map(|x| (BASE - CRATER_ROWS..BASE).rev().map(move |r| (x, r)))
        }) {
            let [wx, wy, wz] = f.cell([x, r, 0]);
            let face = f.cell_face(2) as u8; // its top
            fx.send(
                1,
                Message::BlockEditRequest(BlockEditRequest {
                    action: BlockEditAction::Break,
                    cell: [wx, wy, wz],
                    face,
                    material: 0,
                    rows: 1,
                    down: false,
                    point: None,
                }),
            );
            step(&mut fx, 2);
        }
        assert_eq!(at(&mut fx, f, [5, BASE - CRATER_ROWS, 0]), 0, "dug out");
        step(&mut fx, 20 * SECOND);
        assert_eq!(
            field_bed(&fx, 5),
            m_of_row(BASE - CRATER_ROWS) as f32,
            "the field read the dug bed"
        );
        assert_eq!(
            at(&mut fx, f, [5, BASE - CRATER_ROWS, 0]),
            lava,
            "the lava flowed into the channel"
        );
    }
);

dwell_world::both_faces!(
    // V2 (VOLCANOES.md §6.4): a field grows with its flow. A river vent at the top of a stone ramp,
    // its field covering 3 columns round it to start, runs down the ramp: the field reads the
    // tiles its lava reaches from the world, and the lava and its basalt are in the chunks well
    // past where the field started.
    fn a_river_vents_field_grows_down_a_ramp_and_writes_its_lava_there(f: FaceFrame) {
        let stone = f.state(require("dwell:stone"));
        let (lava, basalt) = (
            f.state(require("dwell:lava")),
            f.state(require("dwell:basalt")),
        );
        let top = |x: i32| BASE + ((70 - x) as f64 * 0.6) as i32;
        let mut fx = Fixture::new(ServerConfig {
            spawn: Some(f.point([20.5, m_of_row(top(20)) + 1.0, 4.5])),
            ..config(f, "creative")
        });
        for x in -4..=72 {
            for z in -3..=3 {
                for r in BASE - 4..top(x) {
                    let [x, y, z] = f.cell([x, r, z]);
                    fx.server.world().set_voxel(x, y, z, stone);
                }
            }
        }
        fx.join(1, &Client::new(1));
        let river = VentSpot {
            fluid: "dwell:lava".into(),
            kind: VentKind::River,
            rate_m3s: 1.0,
            depth_m: 0.0,
            radius: 3,
        };
        fx.server
            .place_lava_vent(f, [0, 0], top(0), &river)
            .expect("a vent at the ramp's top");
        let start = fx.server.lava().fields[0].field.tiles().len();
        step(&mut fx, 120 * SECOND);
        let field = &fx.server.lava().fields[0].field;
        assert!(
            field.tiles().len() > start,
            "grew from {start} tiles to {}",
            field.tiles().len()
        );
        let far = (16..=72)
            .filter(|&x| {
                (-3..=3).any(|z| {
                    (top(x) - 2..top(x) + 6).any(|r| {
                        let m = at(&mut fx, f, [x, r, z]);
                        dwell_blocks::info(m).block == dwell_blocks::info(lava).block || m == basalt
                    })
                })
            })
            .max();
        assert!(
            far.is_some_and(|x| x >= 12),
            "lava in the chunks down to x {far:?}, past the field's start (x 7)"
        );
    }
);

#[test]
fn the_default_world_has_no_lava() {
    // face-a-only: the default world's generator places no vent on either face.
    let db = WorldDb::in_memory().expect("db");
    let mut fx = Fixture::new(ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..flat()
    });
    fx.join(1, &Client::new(1));
    step(&mut fx, 2 * SECOND);
    assert!(fx.server.lava().is_empty());
    fx.server.save_now();
    fx.server.flush_store();
    assert_eq!(db.lava(), None, "no lava table written");
}

#[test]
fn the_lava_blocks_light_is_the_fluids() {
    // face-a-only: block data.
    let fluid = dwell_lava::DATA.fluid("dwell:lava").expect("dwell:lava");
    let block = dwell_blocks::info(require("dwell:lava"));
    assert_eq!(
        block.emits, fluid.light,
        "shared/blocks emits = shared/fluids light"
    );
    assert!(!block.solid, "a player wades into it");
}

dwell_world::both_faces!(
    fn steam_goes_only_to_a_client_that_said_steam_hello(f: FaceFrame) {
        let mut fx = scene(f, "creative", true, None);
        // Water standing in the crater's east side, where the lake will reach it.
        let water = f.state(require("dwell:water"));
        for x in 1..=2 {
            for z in -1..=1 {
                for r in BASE - CRATER_ROWS..BASE - 2 {
                    let [wx, wy, wz] = f.cell([x, r, z]);
                    fx.server.world().set_voxel(wx, wy, wz, water);
                }
            }
        }
        // Client 1 draws steam; client 2 is an older one (no `SteamHello`).
        fx.send(1, Message::SteamHello);
        fx.join(2, &Client::new(2));
        fx.take_all();
        fx.server
            .place_lava_vent(f, [0, 0], BASE, &lake())
            .expect("a vent on the crater's floor");
        step(&mut fx, 20 * SECOND);
        let all = fx.take_all();
        let steam = |id: SessionId| -> Vec<[i32; 3]> {
            all.get(&id)
                .into_iter()
                .flatten()
                .filter_map(|m| match m {
                    Message::Steam(c) => Some(c.clone()),
                    _ => None,
                })
                .flatten()
                .collect()
        };
        let heard = steam(1);
        assert!(!heard.is_empty(), "lava boiled the water: steam sent");
        assert!(
            heard.iter().all(|c| {
                let [x, _, z] = *c;
                (1..=2).contains(&x) && (-1..=1).contains(&z)
            }),
            "steam where the water stood: {heard:?}"
        );
        assert!(steam(2).is_empty(), "an older client never gets Steam");
    }
);
