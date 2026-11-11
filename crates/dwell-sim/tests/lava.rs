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

dwell_world::both_faces!(
    fn cooled_lava_is_basalt_in_the_chunks(f: FaceFrame) {
        let basalt = require("dwell:basalt");
        let mut fx = scene(f, "creative", false, None);
        let river = VentSpot {
            fluid: "dwell:lava".into(),
            kind: VentKind::River,
            rate_m3s: 0.4,
            depth_m: 0.0,
            radius: 8,
        };
        fx.server
            .place_lava_vent(f, [0, 0], BASE, &river)
            .expect("a vent on the stone");
        step(&mut fx, 90 * SECOND);
        let all = fx.take_all();
        let mut rock = 0;
        for x in -8..=8 {
            for z in -8..=8 {
                if at(&mut fx, f, [x, BASE, z]) == f.state(basalt) {
                    rock += 1;
                }
            }
        }
        assert!(rock > 0, "basalt on the stone where the flow cooled");
        assert!(
            sent(&all, 1).contains(&f.state(basalt)),
            "and sent to the client"
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
