//! The axe through the server, as the pickaxe and the shovel (owner, 2026-11-14: "Let's simplify
//! the axe to be like the shovel and pick, without the multiple modes"; 2026-11-15: "it was
//! different from the other tools"; docs/BLOCK_DAMAGE.md §14): an axe hit is a block edit's break
//! that takes block damage (hits to break by the axe, a mark each, the drops spread over them), the
//! hit that reaches the count breaks the whole log, and a tree it held up falls as a body. No wood
//! is cut (no notch, kerfs or bites): an older save's cuts are ignored, and an older client's axe
//! swing lands as one block-damage hit.
mod common;

use common::*;
use dwell_blocks::{parse_state, require};
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{SaveBatch, WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, M, empty_generator, m_of_row};

/// The scene's ground level (face-local row).
const BASE: i32 = FLAT_TOP_Y + 40;
/// The trunk's row the player hits (about eye height).
const AT: i32 = BASE + 3;
const AXE: &str = "dwell:stone_axe";

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        // The axe hits a log through block damage: its real hits to break.
        one_hit_breaks: false,
        ..flat()
    }
}

fn log(f: FaceFrame, axis: &str) -> u16 {
    f.state(parse_state(&format!("dwell:log[axis={axis}]")).expect("log"))
}

/// Stone ground in face `f`'s frame; a standing trunk of logs at x = 3 when `trunk`, else a log
/// lying along z at (3, BASE, 0); one player at (0.5, 0.5) holding the axe (in adventure the
/// held slot; in creative told as the hotbar's tool).
fn scene_with(f: FaceFrame, config: ServerConfig, trunk: bool) -> Fixture {
    let creative = config.mode == "creative";
    let mut fx = Fixture::new(config);
    let stone = f.state(require("dwell:stone"));
    for x in -6..=10 {
        for z in -6..=6 {
            for r in -4..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    if trunk {
        for r in BASE..BASE + 10 {
            let [x, y, z] = f.cell([3, r, 0]);
            fx.server.world().set_voxel(x, y, z, log(f, "y"));
        }
    } else {
        let [x, y, z] = f.cell([3, BASE, 0]);
        fx.server.world().set_voxel(x, y, z, log(f, "z"));
    }
    fx.join(1, &Client::new(1));
    start_with_axe(&mut fx.server, 1);
    fx.send(1, Message::InventoryAction(InventoryAction::Hello));
    fx.send(1, Message::Chop(ChopAction::Hello));
    if creative {
        say_held(&mut fx, 1, AXE, false);
    } else {
        fx.server.give(1, AXE, 1);
        let slot = fx
            .server
            .inventory_of(1)
            .unwrap()
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.item == AXE))
            .expect("given") as u8;
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot }),
        );
    }
    move_to_local(&mut fx, f, 0.5, m_of_row(BASE), 0.5);
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

fn scene(f: FaceFrame, mode: &str, trunk: bool) -> Fixture {
    scene_with(f, config(f, mode), trunk)
}

fn move_to_local(fx: &mut Fixture, f: FaceFrame, x: f64, feet: f64, z: f64) {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = f64::from(players.half_height(h));
    let centre = f.point([x, feet + half, z]);
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::from(centre),
        dwell_physics::Vec3::ZERO,
    );
}

fn cell_is(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3]) -> u16 {
    let [x, y, z] = f.cell(cell);
    fx.server.world().get_voxel(x, y, z)
}

/// The ticks from a hit's request to its tool's contact (`contactS` in shared/tools/tools.json).
fn contact_ticks(item: &str) -> u32 {
    (chopping::break_contact_s(Some(item)) * f64::from(SIM_HZ) - 1e-9).ceil() as u32
}

/// One break (`BlockEditRequest`, as the pickaxe's) of face-local `cell` from its face-local −x
/// face, struck at face-local `at`; then past the edit rate and the tool's contact.
fn hit_at(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3], at: [f64; 3]) {
    hit_face(fx, f, cell, 1, at);
}

/// A hit on face-local `face` (`FACES` order: +x 0, −x 1, +z 4, −z 5) of `cell` at `at`.
fn hit_face(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3], face: usize, at: [f64; 3]) {
    let w = f.cell(cell);
    let p = f.point(at);
    let lo = [f64::from(w[0]), m_of_row(w[1]), f64::from(w[2])];
    let size = [1.0, dwell_world::CELL_H, 1.0];
    let point = std::array::from_fn(|i| ((p[i] - lo[i]) / size[i] * 255.0).round() as u8);
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: w,
            face: f.cell_face(face) as u8,
            material: 0,
            rows: 1,
            down: false,
            point: Some(point),
        }),
    );
    for _ in 0..7 + contact_ticks(AXE) {
        fx.server.step();
    }
}

/// A hit at the trunk's −x side, the lower band, mid-width.
fn at_trunk(fx: &mut Fixture, f: FaceFrame, row: i32) {
    hit_at(fx, f, [3, row, 0], [3.0, m_of_row(row) + 0.12, 0.5]);
}

/// The wood lying loose and in player 1's inventory.
fn wood_count(fx: &Fixture) -> u32 {
    let loose: u32 = fx
        .server
        .loose_items()
        .iter()
        .filter(|it| it.stack.item == "dwell:wood")
        .map(|it| u32::from(it.stack.count))
        .sum();
    let held: u32 = fx.server.inventory_of(1).map_or(0, |inv| {
        inv.slots
            .iter()
            .flatten()
            .filter(|s| s.item == "dwell:wood")
            .map(|s| u32::from(s.count))
            .sum()
    });
    loose + held
}

/// The log's hits to break with `tool` held (its `damage` data).
fn log_hits(tool: &str) -> u32 {
    dwell_protocol::damage::hits_to_break("dwell:log", Some(tool)).expect("an axe breaks a log")
}

/// The held slot's wear, if it holds the axe.
fn axe_wear(fx: &Fixture) -> Option<u16> {
    let inv = fx.server.inventory_of(1)?;
    inv.slots[usize::from(inv.held)]
        .as_ref()
        .filter(|s| s.item == AXE)
        .map(|s| s.wear)
}

/// Nothing cut reached player 1 (an empty `Cuts` is the server's sign it takes swings).
fn no_cuts_sent(sent: &[(Message, usize)]) -> bool {
    sent.iter()
        .all(|(m, _)| !matches!(m, Message::Cuts(c) if !c.cuts.is_empty()))
}

#[test]
fn the_axe_is_a_break_tool_as_the_pickaxe_and_swings_no_more() {
    // face-a-only: the tools' data.
    use dwell_protocol::chopping::{BREAK_TOOLS, tool};
    let pick = BREAK_TOOLS
        .iter()
        .find(|b| b.item == "dwell:stone_pickaxe")
        .unwrap();
    for axe in [AXE, "dwell:metal_axe"] {
        assert!(tool(axe).is_none(), "{axe}: no chopping swing");
        let b = BREAK_TOOLS.iter().find(|b| b.item == axe).expect(axe);
        assert_eq!(b.class, "axe");
        assert_eq!(
            b.contact_s, pick.contact_s,
            "{axe}: strikes at the pick's contact"
        );
        assert_eq!(b.repeat_s, pick.repeat_s, "{axe}: repeats held as the pick");
        assert!(b.clip.starts_with("break_"), "{axe}: a break clip");
    }
}

dwell_world::both_faces!(
    fn an_axe_hit_on_a_log_counts_its_hits_as_a_pickaxe_on_stone(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        at_trunk(&mut fx, f, AT);
        let d = fx
            .server
            .damage_of(f.cell([3, AT, 0]))
            .expect("damaged")
            .clone();
        assert_eq!((d.hits, d.need), (1, log_hits(AXE) as u16), "1 of its 10");
        assert_eq!(d.marks.len(), 1);
        let m = d.marks[0];
        assert_eq!(m.face, f.cell_face(1) as u8, "the edit's face");
        assert_eq!(
            m.chunk,
            dwell_protocol::damage::hit_chunk("dwell:log", Some(AXE)),
            "the bite rule's chunk, as a pickaxe's mark"
        );
        assert_eq!(axe_wear(&fx), Some(1), "one hit, one wear");
        assert_eq!(cell_is(&mut fx, f, [3, AT, 0]), log(f, "y"), "it stands");
        assert!(no_cuts_sent(&fx.take(1)), "no cut, no bites");
        // The same count as the pickaxe's on stone: the data's hits for the tool's class and tier.
        assert_eq!(
            (log_hits(AXE), log_hits("dwell:metal_axe")),
            (10, 5),
            "stone 10, metal 5"
        );
    }
);

dwell_world::both_faces!(
    fn the_log_breaks_whole_at_its_count_and_drops_its_wood(f: FaceFrame) {
        let mut fx = scene(f, "adventure", false);
        move_to_local(&mut fx, f, 1.5, m_of_row(BASE), 0.5);
        fx.server.step();
        let n = log_hits(AXE);
        for i in 0..n {
            assert_eq!(
                cell_is(&mut fx, f, [3, BASE, 0]),
                log(f, "z"),
                "hit {i}: still a log"
            );
            hit_at(&mut fx, f, [3, BASE, 0], [3.0, m_of_row(BASE) + 0.25, 0.5]);
        }
        assert_eq!(
            cell_is(&mut fx, f, [3, BASE, 0]),
            M.air,
            "broken whole after {n}"
        );
        assert!(
            fx.server.damage_of(f.cell([3, BASE, 0])).is_none(),
            "its damage gone"
        );
        // All four on the break (owner, 2026-11-15: "4 chunks drop at the end not as you chop").
        assert_eq!(wood_count(&fx), 4, "its drops");
        let sent = fx.take(1);
        assert!(
            sent.iter()
                .all(|(m, _)| !matches!(m, Message::EntitySpawn(_))),
            "nothing to fell"
        );
        assert!(no_cuts_sent(&sent));
    }
);

dwell_world::both_faces!(
    fn breaking_the_bottom_log_of_a_tree_fells_it(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        // Beside the fall line (it falls toward the −x side cut), not under it.
        move_to_local(&mut fx, f, 1.8, m_of_row(BASE), 2.6);
        for _ in 0..log_hits(AXE) {
            at_trunk(&mut fx, f, BASE);
        }
        assert_eq!(
            cell_is(&mut fx, f, [3, BASE, 0]),
            M.air,
            "the bottom log broken"
        );
        let sent = fx.take(1);
        let spawns: Vec<EntitySpawn> = sent
            .iter()
            .filter_map(|(m, _)| match m {
                Message::EntitySpawn(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(spawns.len(), 1, "one body");
        assert_eq!(spawns[0].cells.len(), 9, "the trunk above it");
        for r in BASE + 1..BASE + 10 {
            assert_eq!(
                cell_is(&mut fx, f, [3, r, 0]),
                M.air,
                "row {r} went with it"
            );
        }
        assert_eq!(wood_count(&fx), 4, "the broken log's drops");
        // It topples: down within 4 s, away from the player who broke it (−x of it).
        let id = spawns[0].id;
        let mut lean = [0.0; 3];
        for _ in 0..4 * 60 {
            fx.server.step();
            let Some((_, q)) = fx.server.body_pose(id) else {
                break;
            };
            lean = f.vector((q * dwell_physics::DVec3::from(f.up())).to_array());
            if lean[1] < 0.5 {
                break;
            }
        }
        assert!(lean[1] < 0.5, "down within 4 s: {lean:?}");
        // Toward the side its log was cut from (the −x, where the player stands; owner,
        // 2026-11-15: "the realistic falling toward the broken side of the tree").
        assert!(lean[0] < -0.5, "toward the cut side: {lean:?}");
    }
);

/// The lean (face-local up of the body) of piece `id` once it is down, within 4 s.
fn fallen_lean(fx: &mut Fixture, f: FaceFrame, id: u32) -> [f64; 3] {
    let mut lean = [0.0, 1.0, 0.0];
    for _ in 0..4 * 60 {
        fx.server.step();
        let Some((_, q)) = fx.server.body_pose(id) else {
            break;
        };
        lean = f.vector((q * dwell_physics::DVec3::from(f.up())).to_array());
        if lean[1] < 0.5 {
            break;
        }
    }
    lean
}

dwell_world::both_faces!(
    /// A tree falls toward the side its bottom log was cut from (owner, 2026-11-15: "my
    /// description only had to do with the contents of the block you're breaking on break, than
    /// the surrounding terrain, essentially following the realistic falling toward the broken
    /// side of the tree"; BLOCK_DAMAGE.md §16): cut from +x it falls to +x, from +z to +z, the
    /// player standing on that side (the nearest player, whom the old rule tipped it away from).
    fn a_tree_falls_toward_the_side_its_bottom_log_was_cut_from(f: FaceFrame) {
        let row = m_of_row(BASE) + 0.12;
        for (stand, face, at, axis) in [
            // Beside the fall line, not under the trunk.
            ([4.8, 2.6], 0usize, [4.0, row, 0.5], 0usize),
            ([5.6, 1.8], 4, [3.5, row, 1.0], 2),
        ] {
            let mut fx = scene(f, "adventure", true);
            move_to_local(&mut fx, f, stand[0], m_of_row(BASE), stand[1]);
            for _ in 0..5 {
                fx.server.step();
            }
            fx.take_all();
            for _ in 0..log_hits(AXE) {
                hit_face(&mut fx, f, [3, BASE, 0], face, at);
            }
            assert_eq!(cell_is(&mut fx, f, [3, BASE, 0]), M.air, "broken");
            let id = fx
                .take(1)
                .iter()
                .find_map(|(m, _)| match m {
                    Message::EntitySpawn(s) => Some(s.id),
                    _ => None,
                })
                .expect("the trunk falls as a body");
            let lean = fallen_lean(&mut fx, f, id);
            assert!(lean[1] < 0.5, "face {face}: down within 4 s: {lean:?}");
            assert!(
                lean[axis] > 0.5,
                "face {face}: toward the cut side: {lean:?}"
            );
        }
    }
);

#[test]
fn creative_breaks_a_log_in_its_hits_and_drops_nothing() {
    // face-a-only: a mode rule (a tool keeps its hits in creative).
    let f = FaceFrame::A;
    let mut fx = scene(f, "creative", false);
    move_to_local(&mut fx, f, 1.5, m_of_row(BASE), 0.5);
    fx.server.step();
    for _ in 0..log_hits(AXE) - 1 {
        hit_at(&mut fx, f, [3, BASE, 0], [3.0, m_of_row(BASE) + 0.25, 0.5]);
    }
    assert_eq!(cell_is(&mut fx, f, [3, BASE, 0]), log(f, "z"), "one short");
    hit_at(&mut fx, f, [3, BASE, 0], [3.0, m_of_row(BASE) + 0.25, 0.5]);
    assert_eq!(cell_is(&mut fx, f, [3, BASE, 0]), M.air);
    assert!(fx.server.loose_items().is_empty(), "creative drops nothing");
}

/// A hand break of face-local `cell` from its −x face.
fn hand_break(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3]) {
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: f.cell(cell),
            face: f.cell_face(1) as u8,
            material: 0,
            rows: 1,
            down: false,
            point: None,
        }),
    );
    for _ in 0..30 {
        fx.server.step();
    }
}

dwell_world::both_faces!(
    fn in_adventure_hands_cannot_break_a_log_but_leaves_still_break(f: FaceFrame) {
        // Owner, 2026-11-08: logs need an axe in adventure (the log's `breakWith: "axe"`).
        let mut fx = scene(f, "adventure", true);
        let leaves = f.state(require("dwell:leaves"));
        let [x, y, z] = f.cell([2, AT, 1]);
        fx.server.world().set_voxel(x, y, z, leaves);
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot: 3 }),
        );
        hand_break(&mut fx, f, [3, AT, 0]);
        assert_eq!(
            cell_is(&mut fx, f, [3, AT, 0]),
            log(f, "y"),
            "the log stays"
        );
        assert!(fx.server.damage_of(f.cell([3, AT, 0])).is_none(), "not hit");
        hand_break(&mut fx, f, [2, AT, 1]);
        assert_eq!(
            cell_is(&mut fx, f, [2, AT, 1]),
            M.air,
            "leaves break by hand"
        );
    }
);

dwell_world::both_faces!(
    fn an_older_clients_axe_swing_lands_as_one_block_damage_hit(f: FaceFrame) {
        // A client before this build still swings the axe (`ChopAction::Swing` or `Sweep`): each
        // is one hit of the axe's block damage on what its aim meets, never a cut.
        let mut fx = scene(f, "adventure", true);
        let n = f.vector([-1.0, 0.0, 0.0]);
        let axis = n.iter().position(|v| v.abs() > 0.5).unwrap();
        let p = f.point([3.0, m_of_row(AT) + 0.12, 0.5]);
        fx.send(
            1,
            Message::Chop(ChopAction::Swing {
                tool: AXE.into(),
                cell: f.cell([3, AT, 0]),
                face: (axis * 2) as u8 + u8::from(n[axis] > 0.0),
                point: p.map(|v| v as f32),
                vertical: false,
            }),
        );
        for _ in 0..40 {
            fx.server.step();
        }
        let d = fx
            .server
            .damage_of(f.cell([3, AT, 0]))
            .expect("hit")
            .clone();
        assert_eq!(d.hits, 1);
        assert_eq!(d.marks[0].face, f.cell_face(1) as u8, "on its −x face");
        // A sweep at the trunk from the eye, level along +x: one more hit, on the row it meets.
        let look = f.vector([1.0, 0.0, 0.0]).map(|v| v as f32);
        fx.send(
            1,
            Message::Chop(ChopAction::Sweep {
                tool: AXE.into(),
                look,
                vertical: false,
            }),
        );
        for _ in 0..40 {
            fx.server.step();
        }
        let hits: u16 = (BASE..BASE + 10)
            .filter_map(|r| fx.server.damage_of(f.cell([3, r, 0])).map(|d| d.hits))
            .sum();
        assert_eq!(hits, 2, "two hits in all");
        assert!(no_cuts_sent(&fx.take(1)), "no cut");
    }
);

#[test]
fn an_older_saves_cuts_are_ignored() {
    // face-a-only: a byte format. An older build's chopping saved cuts (notches, kerfs, bites):
    // this build neither sends nor draws them; the cell is a whole log, hit as any other.
    use dwell_protocol::chopping::{BITE_LOOK, Cut, Cuts};
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "creative")
    };
    let cell = f.cell([3, AT, 0]);
    let mut old = Cut::default();
    for i in 0..5 {
        old.swing_by(0.1, 0.05, 1, 0, i % 2, 1.0);
        old.bite(1, 0, i % 2, 0.5, &BITE_LOOK);
    }
    {
        let mut fx = scene_with(f, cfg(), true);
        fx.server.save_now();
        fx.server.flush_store();
    }
    let mut cuts = Cuts::default();
    cuts.cells.insert(cell, old);
    db.save(&SaveBatch {
        cuts: Some(cuts.to_bytes()),
        ..Default::default()
    })
    .expect("an older build's cuts");
    let mut fx = Fixture::new(cfg());
    fx.join(1, &Client::new(1));
    fx.send(1, Message::Chop(ChopAction::Hello));
    fx.server.step();
    let sent = fx.take(1);
    assert!(
        sent.iter().any(|(m, _)| matches!(m, Message::Cuts(_))),
        "Hello answered"
    );
    assert!(no_cuts_sent(&sent), "the old cut is not sent");
}

/// The held tools `id` was sent.
fn held_tools_to(fx: &mut Fixture, id: SessionId) -> Vec<HeldTool> {
    fx.take(id)
        .into_iter()
        .filter_map(|(m, _)| match m {
            Message::HeldTools(l) => Some(l),
            _ => None,
        })
        .flatten()
        .collect()
}

fn say_held(fx: &mut Fixture, id: SessionId, tool: &str, vertical: bool) {
    fx.send(
        id,
        Message::Chop(ChopAction::Held {
            tool: tool.into(),
            vertical,
        }),
    );
    fx.server.step();
}

#[test]
fn the_held_axe_reaches_the_others_that_understand_it_by_name() {
    // face-a-only: the held tool on the wire is a name, the same on both faces.
    let f = FaceFrame::A;
    let mut fx = scene(f, "creative", true);
    let welcome = fx.join(2, &Client::new(2));
    assert_ne!(welcome.flags & welcome_flags::CHOP_SWEEP, 0, "announced");
    fx.join(3, &Client::new(3));
    // Player 2 understands held tools (it said what it holds: nothing); player 3 is an older
    // client that never does.
    say_held(&mut fx, 2, "", false);
    fx.take_all();
    say_held(&mut fx, 1, AXE, false);
    let got = held_tools_to(&mut fx, 2);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].tool, AXE);
    assert!(
        held_tools_to(&mut fx, 3).is_empty(),
        "an older client is sent nothing new"
    );
    // A client that says what it holds later gets the others' at once.
    say_held(&mut fx, 3, "", false);
    assert!(held_tools_to(&mut fx, 3).iter().any(|h| h.tool == AXE));
    // Not a tool: held as nothing.
    say_held(&mut fx, 1, "dwell:stone", false);
    assert!(held_tools_to(&mut fx, 2).iter().any(|h| h.tool.is_empty()));
}
