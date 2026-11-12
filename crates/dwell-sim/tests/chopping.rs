//! Chopping through the server (docs/adr/0067-stone-axe-and-chopping.md, docs/BLOCK_DAMAGE.md §11): the
//! axe's swing (its arc, contact, clears and wear) lands on a log as a block-damage hit (WORK_QUEUE item
//! 50: hits to break by the axe, a mark each, the drops spread over them), the last breaking it and
//! felling the trunk above as one body toward the side struck; older saves' cuts still load and are
//! sent only to clients that said `ChopAction::Hello`; the chopping prototype (`chop2`) keeps its cuts.
mod common;

use common::*;
use dwell_blocks::{parse_state, require};
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, M, empty_generator, m_of_row};

/// The scene's ground level (face-local row).
const BASE: i32 = FLAT_TOP_Y + 40;
/// The trunk's row the player swings at (about eye height).
const AT: i32 = BASE + 3;
const AXE: &str = "dwell:stone_axe";
/// A swing's wait (the axe's `swingSeconds`, 0.45 s at 60 Hz, and a tick).
const SWING_TICKS: u32 = 28;

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        // The axe hits a log through block damage (WORK_QUEUE item 51): its real hits to break.
        one_hit_breaks: false,
        ..flat()
    }
}

fn log(f: FaceFrame, axis: &str) -> u16 {
    f.state(parse_state(&format!("dwell:log[axis={axis}]")).expect("log"))
}

/// Stone ground in face `f`'s frame; a standing trunk of logs at x = 3 when `trunk`, else a log
/// lying along z at (3, BASE, 0); one player at (0.5, 0.5) holding the axe (said `Hello` when
/// `hello`).
fn scene_with(f: FaceFrame, config: ServerConfig, trunk: bool, hello: bool) -> Fixture {
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
    let welcome = fx.join(1, &Client::new(1));
    assert_ne!(welcome.flags & welcome_flags::CHOPPING, 0, "announced");
    if hello {
        fx.send(1, Message::Chop(ChopAction::Hello));
    }
    fx.server.give(1, AXE, 1);
    move_to_local(&mut fx, f, 0.5, m_of_row(BASE), 0.5);
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

fn scene(f: FaceFrame, mode: &str, trunk: bool) -> Fixture {
    scene_with(f, config(f, mode), trunk, true)
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

/// One swing at face-local cell `cell`'s face with face-local outward normal `normal`, at the
/// face-local point `at`; then the swing's wait.
fn swing(
    fx: &mut Fixture,
    f: FaceFrame,
    cell: [i32; 3],
    normal: [f64; 3],
    at: [f64; 3],
    vertical: bool,
) {
    swing_with(fx, f, AXE, cell, normal, at, vertical);
}

/// [`swing`] with axe `tool` (held).
fn swing_with(
    fx: &mut Fixture,
    f: FaceFrame,
    tool: &str,
    cell: [i32; 3],
    normal: [f64; 3],
    at: [f64; 3],
    vertical: bool,
) {
    let n = f.vector(normal);
    let axis = n.iter().position(|v| v.abs() > 0.5).expect("an axis");
    let p = f.point(at);
    fx.send(
        1,
        Message::Chop(ChopAction::Swing {
            tool: tool.into(),
            cell: f.cell(cell),
            face: (axis * 2) as u8 + u8::from(n[axis] > 0.0),
            point: p.map(|v| v as f32),
            vertical,
        }),
    );
    for _ in 0..SWING_TICKS {
        fx.server.step();
    }
}

/// A swing at the trunk's −x side (facing the player) in band `band` (0 lower, 1 upper).
fn at_trunk(fx: &mut Fixture, f: FaceFrame, band: usize, vertical: bool) {
    at_trunk_with(fx, f, AXE, band, vertical);
}

fn at_trunk_with(fx: &mut Fixture, f: FaceFrame, tool: &str, band: usize, vertical: bool) {
    let y = m_of_row(AT) + if band == 0 { 0.12 } else { 0.37 };
    swing_with(
        fx,
        f,
        tool,
        [3, AT, 0],
        [-1.0, 0.0, 0.0],
        [3.0, y, 0.5],
        vertical,
    );
}

fn cell_is(fx: &mut Fixture, f: FaceFrame, cell: [i32; 3]) -> u16 {
    let [x, y, z] = f.cell(cell);
    fx.server.world().get_voxel(x, y, z)
}

/// Swings `band_of(i)` until the cut cell is gone (at most `limit`); the count.
fn swings_to_fell(
    fx: &mut Fixture,
    f: FaceFrame,
    vertical: bool,
    limit: usize,
    band_of: impl Fn(usize) -> usize,
) -> Option<usize> {
    swings_to_fell_with(fx, f, AXE, vertical, limit, band_of)
}

/// [`swings_to_fell`] with axe `tool` (held).
fn swings_to_fell_with(
    fx: &mut Fixture,
    f: FaceFrame,
    tool: &str,
    vertical: bool,
    limit: usize,
    band_of: impl Fn(usize) -> usize,
) -> Option<usize> {
    for i in 0..limit {
        at_trunk_with(fx, f, tool, band_of(i), vertical);
        // Felled: the log is gone (the stump's wedge in its place, Phase 55 T2).
        if cell_is(fx, f, [3, AT, 0]) != log(f, "y") {
            return Some(i + 1);
        }
    }
    None
}

dwell_world::both_faces!(
    fn a_metal_axe_fells_in_about_half_the_stone_axes_swings_and_wears_half_as_fast(f: FaceFrame) {
        // Owner, 2026-11-12: "Metal tools break double sized bigger chunks off and reach 50%
        // broken faster, which is when the blocks break. Also have double durability."
        const METAL: &str = "dwell:metal_axe";
        let mut fx = scene(f, "adventure", true);
        let stone = swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("felled");
        let mut fx = scene(f, "adventure", true);
        fx.server.give(1, METAL, 1);
        let slot = fx
            .server
            .inventory_of(1)
            .unwrap()
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.item == METAL))
            .expect("given") as u8;
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot }),
        );
        for _ in 0..2 {
            fx.server.step();
        }
        let metal = swings_to_fell_with(&mut fx, f, METAL, false, 20, |i| i % 2).expect("felled");
        assert!(
            metal <= stone.div_ceil(2) + 1 && 2 * metal + 2 >= stone,
            "metal {metal} swings against stone {stone}"
        );
        let wear = fx.server.inventory_of(1).unwrap().slots[usize::from(slot)]
            .as_ref()
            .map(|s| s.wear);
        assert_eq!(wear, Some(metal as u16), "a wear a swing");
        assert_eq!(
            chopping::durability(METAL),
            chopping::durability(AXE).map(|h| 2 * h),
            "double durability"
        );
    }
);

/// The player's centre, face-local.
fn player_at(fx: &mut Fixture, f: FaceFrame) -> [f64; 3] {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    f.point(players.position(physics, h).to_array())
}

// both-faces: run on each face by the two tests below (BUGS.md #52; they were ignored until fixed).
fn a_felled_trunk_falling_on_the_chopper_does_not_fling_them(f: FaceFrame) {
    // 0.50.36's screenshots: the trunk tips toward the notch, where the chopper stands, and
    // its fall threw them ~20 m. A falling piece may knock a player down or aside, not
    // launch them: over the fall and its landing they move a few metres at most.
    let mut fx = scene(f, "creative", true);
    // A tree's height (10 m of trunk), as the forest's.
    for r in BASE + 10..BASE + 20 {
        let [x, y, z] = f.cell([3, r, 0]);
        fx.server.world().set_voxel(x, y, z, log(f, "y"));
    }
    let start = player_at(&mut fx, f);
    swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("felled");
    let mut far: f64 = 0.0;
    // The speed it was moved at over a quarter second (by its positions: one step's solver
    // velocity is not a throw).
    let mut fastest: f64 = 0.0;
    let mut trail = vec![start];
    for _ in 0..4 * 60 {
        fx.server.step();
        let p = player_at(&mut fx, f);
        let d = ((p[0] - start[0]).powi(2) + (p[2] - start[2]).powi(2)).sqrt();
        far = far.max(d);
        trail.push(p);
        if trail.len() > 15 {
            let q = trail[trail.len() - 16];
            let moved = (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt();
            fastest = fastest.max(moved * 4.0);
        }
    }
    println!("{f:?}: pushed {far:.2} m at most, {fastest:.1} m/s");
    assert!(far < 4.0, "the falling trunk threw the chopper {far:.1} m");
    assert!(
        fastest < 12.0,
        "the falling trunk threw the chopper at {fastest:.1} m/s"
    );
}

#[test]
fn a_felled_trunk_falling_on_the_chopper_does_not_fling_them_a() {
    a_felled_trunk_falling_on_the_chopper_does_not_fling_them(FaceFrame::A);
}

#[test]
fn a_felled_trunk_falling_on_the_chopper_does_not_fling_them_b() {
    a_felled_trunk_falling_on_the_chopper_does_not_fling_them(FaceFrame::B);
}

/// Swings at the lying log's top (face-local up), from the −x side, in band `band` along z.
fn at_log(fx: &mut Fixture, f: FaceFrame, band: usize, vertical: bool) {
    let z = if band == 0 { 0.3 } else { 0.7 };
    swing(
        fx,
        f,
        [3, BASE, 0],
        [0.0, 1.0, 0.0],
        [3.5, m_of_row(BASE + 1), z],
        vertical,
    );
}

#[test]
fn creative_cuts_but_drops_nothing() {
    // face-a-only: a mode rule.
    let f = FaceFrame::A;
    let mut fx = scene(f, "creative", false);
    for i in 0..12 {
        at_log(&mut fx, f, i % 2, true);
    }
    assert_eq!(cell_is(&mut fx, f, [3, BASE, 0]), M.air);
    assert!(fx.server.loose_items().is_empty());
}

#[test]
fn without_the_axe_in_hand_adventure_swings_do_nothing() {
    // face-a-only: a mode rule.
    let f = FaceFrame::A;
    let mut fx = scene(f, "adventure", true);
    fx.send(
        1,
        Message::InventoryAction(InventoryAction::Select { slot: 3 }),
    );
    at_trunk(&mut fx, f, 0, false);
    assert!(fx.server.cut_of(f.cell([3, AT, 0])).is_none());
}

/// A hand break (`BlockEditRequest`) of face-local `cell` from its −x face.
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
    for _ in 0..3 {
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
        assert_ne!(cell_is(&mut fx, f, [3, AT, 0]), M.air, "the log stays");
        assert!(fx.server.loose_items().is_empty(), "and drops nothing");
        hand_break(&mut fx, f, [2, AT, 1]);
        assert_eq!(
            cell_is(&mut fx, f, [2, AT, 1]),
            M.air,
            "leaves break by hand"
        );
    }
);

dwell_world::both_faces!(
    fn in_creative_hands_break_a_log(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot: 3 }),
        );
        hand_break(&mut fx, f, [3, AT, 0]);
        assert_eq!(cell_is(&mut fx, f, [3, AT, 0]), M.air);
    }
);

// The swing's path (owner, 2026-11-08: "the weapon's actual path is what hits"): the client sends
// the swing's start (`ChopAction::Sweep`: the look and the plane) and the server sweeps the axe's
// arc from its own eye, hitting the first solid cell on it.

/// A swept swing along the face-local look `look` (normalised here); then the swing's wait.
fn sweep(fx: &mut Fixture, f: FaceFrame, look: [f64; 3], vertical: bool) {
    let l = (look[0] * look[0] + look[1] * look[1] + look[2] * look[2]).sqrt();
    let w = f.vector(look.map(|v| v / l));
    fx.send(
        1,
        Message::Chop(ChopAction::Sweep {
            tool: AXE.into(),
            look: w.map(|v| v as f32),
            vertical,
        }),
    );
    for _ in 0..SWING_TICKS {
        fx.server.step();
    }
}

/// The trunk's cells (face-local rows BASE..BASE + 10 at x = 3) that were hit: damaged (the log
/// takes block damage, WORK_QUEUE item 51) or cut (the chopping prototype's).
fn cut_rows(fx: &Fixture, f: FaceFrame) -> Vec<i32> {
    (BASE..BASE + 10)
        .filter(|&r| {
            let c = f.cell([3, r, 0]);
            fx.server.cut_of(c).is_some() || fx.server.damage_of(c).is_some()
        })
        .collect()
}

/// The player's right, face-locally, while looking along +x: +z on face A, −z on face B (a
/// mirror reverses handedness).
fn right_z(f: FaceFrame) -> i32 {
    f.handed(1.0) as i32
}

/// The scene with the player 1.5 m from the trunk's side (within the axe's reach).
fn near(f: FaceFrame, mode: &str) -> Fixture {
    let mut fx = scene(f, mode, true);
    move_to_local(&mut fx, f, 1.5, m_of_row(BASE), 0.5);
    fx.server.step();
    fx
}

dwell_world::both_faces!(
    fn a_swing_whose_arc_passes_the_trunk_cuts_it_though_the_crosshair_is_off_it(f: FaceFrame) {
        // Looking 40° to the left of the trunk: the crosshair passes it, the level swing starts
        // on the right and its head meets the bark.
        let a = 40f64.to_radians();
        let left = -f64::from(right_z(f));
        let mut fx = near(f, "adventure");
        sweep(&mut fx, f, [a.cos(), 0.0, left * a.sin()], false);
        assert!(
            !cut_rows(&fx, f).is_empty(),
            "the swing's path cut the trunk"
        );
        // Looking well to its right, the level swing starts past it and sweeps away: a miss.
        let b = 75f64.to_radians();
        let mut fx = near(f, "adventure");
        sweep(&mut fx, f, [b.cos(), 0.0, -left * b.sin()], false);
        assert!(
            cut_rows(&fx, f).is_empty(),
            "swept away from it: {:?}",
            cut_rows(&fx, f)
        );
    }
);

dwell_world::both_faces!(
    fn a_nearer_block_on_the_swings_path_takes_the_hit_and_the_trunk_is_spared(f: FaceFrame) {
        // Aimed straight at the trunk, it is cut, on its side facing the swing (−x: side 0).
        // Red before: the sweep's face was `edit::FACE_DIRS`'s order, so the notch went on the far
        // side (out of sight, and the tree fell away from it).
        let mut fx = near(f, "adventure");
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        let rows = cut_rows(&fx, f);
        assert!(!rows.is_empty(), "the trunk aimed at");
        // The mark's face: −x, an edit's face 1 (x is the same on both faces).
        let d = fx.server.damage_of(f.cell([3, rows[0], 0])).unwrap();
        assert_eq!(d.marks[0].face, 1, "the near side: {d:?}");
        // A stone post to the right, between the swing's start and the trunk: it takes the hit
        // (a bounce: nothing changes) and the trunk is not cut.
        let mut fx = near(f, "adventure");
        let stone = f.state(require("dwell:stone"));
        for r in BASE..BASE + 6 {
            let [x, y, z] = f.cell([2, r, right_z(f)]);
            fx.server.world().set_voxel(x, y, z, stone);
        }
        fx.server.step();
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        assert!(
            cut_rows(&fx, f).is_empty(),
            "the post took it: {:?}",
            cut_rows(&fx, f)
        );
        for r in BASE..BASE + 6 {
            assert_eq!(
                cell_is(&mut fx, f, [2, r, right_z(f)]),
                stone,
                "a bounce: no effect"
            );
        }
    }
);

dwell_world::both_faces!(
    fn a_free_swing_at_nothing_or_at_stone_does_nothing_and_spends_the_swing(f: FaceFrame) {
        let mut fx = near(f, "adventure");
        // Looking away from the trunk at open air: nothing.
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], true);
        assert!(cut_rows(&fx, f).is_empty());
        // At the stone ground, straight down in a vertical swing: a bounce, the ground as it was.
        let stone = f.state(require("dwell:stone"));
        sweep(&mut fx, f, [-0.05, -1.0, 0.0], true);
        assert_eq!(cell_is(&mut fx, f, [1, BASE - 1, 0]), stone);
        assert!(cut_rows(&fx, f).is_empty());
        // A missed swing still spends the axe's wait: one sent at once after it is refused.
        let w = f.vector([-1.0, 0.0, 0.0]).map(|v| v as f32);
        fx.send(
            1,
            Message::Chop(ChopAction::Sweep {
                tool: AXE.into(),
                look: w,
                vertical: false,
            }),
        );
        fx.server.step();
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        assert!(cut_rows(&fx, f).is_empty(), "too soon after the miss");
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        assert!(!cut_rows(&fx, f).is_empty(), "then it cuts");
    }
);

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
    say_held(&mut fx, 1, AXE, true);
    let got = held_tools_to(&mut fx, 2);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].tool, AXE);
    assert!(got[0].vertical);
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

// The axe clears leaves (owner, 2026-11-09: "the axe should be able to clear leaves too"): its
// `clears` (shared/tools) let the swing's head through, breaking every such cell it touches, on to
// the wood behind (chopped) or another solid (a bounce).

/// A wall of leaves at face-local x = `x`, z −3..=3, rows BASE..BASE + 8; its cells.
fn leaf_wall(fx: &mut Fixture, f: FaceFrame, x: i32) -> Vec<[i32; 3]> {
    let leaves = f.state(require("dwell:leaves"));
    let mut cells = Vec::new();
    for z in -3..=3 {
        for r in BASE..BASE + 8 {
            let [wx, wy, wz] = f.cell([x, r, z]);
            fx.server.world().set_voxel(wx, wy, wz, leaves);
            cells.push([x, r, z]);
        }
    }
    fx.server.step();
    cells
}

fn cleared(fx: &mut Fixture, f: FaceFrame, cells: &[[i32; 3]]) -> usize {
    cells
        .iter()
        .filter(|&&c| cell_is(fx, f, c) == M.air)
        .count()
}

fn sticks(fx: &Fixture) -> u32 {
    fx.server
        .loose_items()
        .iter()
        .filter(|it| it.stack.item == "dwell:stick")
        .map(|it| u32::from(it.stack.count))
        .sum()
}

dwell_world::both_faces!(
    fn a_swing_through_leaves_clears_them_and_still_chops_the_trunk_behind(f: FaceFrame) {
        // Red before: the leaves in front took the hit (a bounce) and the trunk was not cut.
        let mut fx = near(f, "adventure");
        let wall = leaf_wall(&mut fx, f, 2);
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        let n = cleared(&mut fx, f, &wall);
        assert!(n >= 2, "the leaves on the arc are cleared: {n}");
        assert!(n < wall.len(), "only those the arc touched: {n}");
        assert!(!cut_rows(&fx, f).is_empty(), "and the trunk behind is cut");
    }
);

dwell_world::both_faces!(
    fn a_swing_through_only_leaves_clears_them_and_a_swing_at_stone_still_bounces(f: FaceFrame) {
        let mut fx = near(f, "creative");
        // Looking away from the trunk, through a wall of leaves at x = 0, nothing behind.
        let wall = leaf_wall(&mut fx, f, 0);
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        assert!(cleared(&mut fx, f, &wall) >= 2, "the leaves are cleared");
        assert!(cut_rows(&fx, f).is_empty());
        // Stone stops the head and stays (a bounce): the leaves behind it are untouched.
        let mut fx = near(f, "creative");
        let wall = leaf_wall(&mut fx, f, -1);
        let stone = f.state(require("dwell:stone"));
        for z in -3..=3 {
            for r in BASE..BASE + 8 {
                let [x, y, wz] = f.cell([0, r, z]);
                fx.server.world().set_voxel(x, y, wz, stone);
            }
        }
        fx.server.step();
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        assert_eq!(cleared(&mut fx, f, &wall), 0, "behind the stone");
        assert_eq!(
            cell_is(&mut fx, f, [0, AT, 0]),
            stone,
            "a bounce: no effect"
        );
    }
);

dwell_world::both_faces!(
    fn cleared_leaves_drop_their_items_in_adventure_not_in_creative(f: FaceFrame) {
        for (mode, drops) in [("adventure", true), ("creative", false)] {
            let mut fx = near(f, mode);
            let wall = leaf_wall(&mut fx, f, 0);
            // A level and a vertical swing and a level one lower: many leaves (sticks half the time).
            sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
            sweep(&mut fx, f, [-1.0, 0.0, 0.0], true);
            sweep(&mut fx, f, [-1.0, -0.6, 0.0], false);
            let n = cleared(&mut fx, f, &wall) as u32;
            assert!(n >= 6, "{mode}: {n} cleared");
            let s = sticks(&fx);
            if drops {
                assert!((1..=n).contains(&s), "{mode}: {s} sticks from {n} leaves");
            } else {
                assert!(fx.server.loose_items().is_empty(), "{mode}: no drops");
            }
        }
    }
);

// The chopping prototype (docs/WORK_QUEUE.md item 7, `chop2` in shared/tools/tools.json; owner
// 2026-11-11): behind the `chop2` switch, each chop removes a share of the section, a tree falls
// once more than half is cut, toward its notch, and the split keeps the notch.

/// The scene with the chopping prototype on (adventure).
fn scene2(f: FaceFrame) -> Fixture {
    scene2_in(f, "adventure")
}

fn scene2_in(f: FaceFrame, mode: &str) -> Fixture {
    scene_with(
        f,
        ServerConfig {
            chop2: true,
            ..config(f, mode)
        },
        true,
        true,
    )
}

/// A chop at the trunk's +x side (away from the player, reached round the trunk).
fn at_trunk_far(fx: &mut Fixture, f: FaceFrame) {
    let y = m_of_row(AT) + 0.25;
    swing(fx, f, [3, AT, 0], [1.0, 0.0, 0.0], [4.0, y, 0.5], false);
}

/// Steps the felled piece `id` up to four seconds until it is down; its lean (face-local up).
fn lean_of(fx: &mut Fixture, f: FaceFrame, id: u32) -> [f64; 3] {
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

/// The felled piece's spawn among what was sent.
fn felled_piece(sent: &[(Message, usize)]) -> EntitySpawn {
    sent.iter()
        .find_map(|(m, _)| match m {
            Message::EntitySpawn(s) => Some(s.clone()),
            _ => None,
        })
        .expect("a piece spawned")
}

dwell_world::both_faces!(
    fn chop2_twelve_stone_chops_fell_toward_the_cut_and_the_split_keeps_the_notch(f: FaceFrame) {
        let mut fx = scene2(f);
        let n = swings_to_fell(&mut fx, f, false, 30, |_| 0).expect("felled");
        assert!((11..=13).contains(&n), "{n} chops");
        // The split keeps the notch (`| / \ |`): the stump the notch's lower face (the ramp rising
        // away from the notch, −x), the trunk's bottom its upper face (`open`: thick at the hinge).
        let wood = dwell_protocol::chopping::wood("dwell:log").expect("log");
        assert!(
            !wood.chop2.wedge_matching,
            "the prototype's trunk keeps the upper face"
        );
        let (stump, top) = dwell_sim::felling::fell_wedges(
            wood.fell_wedge.expect("the log's wedge"),
            [-1.0, 0.0],
            dwell_sim::felling::WedgeTrunk::Open,
        )
        .expect("the log's wedges");
        let prop = dwell_blocks::state_property;
        assert_eq!(
            cell_is(&mut fx, f, [3, AT, 0]),
            f.state(stump),
            "the stump's face"
        );
        assert_eq!(
            (prop(stump, "facing"), prop(stump, "half")),
            (Some("west"), Some("bottom")),
            "low at the notch (−x), rising to the hinge"
        );
        assert_eq!(
            (prop(top, "facing"), prop(top, "half")),
            (Some("west"), Some("top")),
            "upside down, thick at the hinge: the notch's upper face"
        );
        let sent = fx.take(1);
        let spawn = felled_piece(&sent);
        let up = f.up()[1].signum() as i16;
        let bottom = spawn
            .cells
            .iter()
            .min_by_key(|c| c.offset[1] * up)
            .expect("cells");
        assert_eq!(
            bottom.state,
            f.state(top),
            "the trunk's bottom keeps the upper face"
        );
        // One-sided: it falls toward the side cut (−x).
        let lean = lean_of(&mut fx, f, spawn.id);
        assert!(lean[1] < 0.5 && lean[0] < -0.8, "down toward −x: {lean:?}");
    }
);

dwell_world::both_faces!(
    fn chop2_a_notch_then_a_deeper_back_cut_falls_toward_the_notch(f: FaceFrame) {
        let mut fx = scene2(f);
        // The notch on +x (4 chops), the back cut from −x until it goes.
        for _ in 0..4 {
            at_trunk_far(&mut fx, f);
        }
        let cut = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut");
        assert_eq!(cut.notch_side(), Some(1), "the notch on +x: {cut:?}");
        let n = swings_to_fell(&mut fx, f, false, 30, |_| 0).expect("felled");
        assert!(n >= 6, "the back cut made the rest: {n}");
        let spawn = felled_piece(&fx.take(1));
        let lean = lean_of(&mut fx, f, spawn.id);
        assert!(
            lean[1] < 0.5 && lean[0] > 0.8,
            "down toward +x, the notch: {lean:?}"
        );
        assert!(lean[2].abs() < 0.2, "not sideways: {lean:?}");
    }
);

dwell_world::both_faces!(
    fn chop2_along_the_grain_takes_about_twice_as_many(f: FaceFrame) {
        // Creative: more chops than a stone axe's wear and a full stamina bar allow.
        let mut fx = scene2_in(f, "creative");
        let n = swings_to_fell(&mut fx, f, true, 40, |_| 0).expect("felled");
        assert!((22..=26).contains(&n), "{n} vertical chops");
    }
);

dwell_world::both_faces!(
    fn chop2_a_swing_lands_at_the_crosshair_its_row_the_stump_height(f: FaceFrame) {
        // From 1.5 m, looking a little down at the trunk, then nearly level: each look cuts the
        // row it meets, the higher look a higher row.
        let mut fx = scene2(f);
        move_to_local(&mut fx, f, 1.5, m_of_row(BASE), 0.5);
        fx.server.step();
        sweep(&mut fx, f, [1.5, -0.45, 0.0], false);
        let rows = cut_rows(&fx, f);
        assert_eq!(rows.len(), 1, "{rows:?}");
        sweep(&mut fx, f, [1.5, -0.05, 0.0], false);
        let rows2 = cut_rows(&fx, f);
        assert_eq!(rows2.len(), 2, "a higher look cuts a higher row: {rows2:?}");
        assert!(
            rows2.iter().any(|&r| r > rows[0]),
            "{rows:?} then {rows2:?}"
        );
    }
);

dwell_world::both_faces!(
    fn chop2_reaches_as_far_as_the_target_outline(f: FaceFrame) {
        // Round 2: a trunk you can outline you can chop. From x = 0.5 the trunk's face at x = 3
        // is 2.5 m off, past the swing's 2.2 m reach but within the outline's.
        // Red before: the aim reached only `swing.reachM`, and nothing was cut.
        let mut fx = scene2(f);
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        assert_eq!(cut_rows(&fx, f).len(), 1, "{:?}", cut_rows(&fx, f));
    }
);

dwell_world::both_faces!(
    fn chop2_a_swing_after_the_fall_keeps_the_stumps_wedge(f: FaceFrame) {
        // Round 2, seen in game: the swings after the fall broke the stump's wedge (a `log_ramp`,
        // in the axe's `clears`) as the arc swept through it, leaving a flat stump. Red before.
        let mut fx = scene2(f);
        swings_to_fell(&mut fx, f, false, 30, |_| 0).expect("felled");
        let stump = cell_is(&mut fx, f, [3, AT, 0]);
        assert_eq!(
            dwell_blocks::block_of(f.state(stump)).name,
            "dwell:log_ramp"
        );
        // From the hinge's side (+x), where the wedge rises to meet the look.
        move_to_local(&mut fx, f, 5.5, m_of_row(BASE), 0.5);
        for _ in 0..30 {
            fx.server.step();
        }
        for _ in 0..3 {
            sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        }
        assert_eq!(
            cell_is(&mut fx, f, [3, AT, 0]),
            stump,
            "the stump keeps its wedge"
        );
    }
);

/// The held slot's wear, if it holds the axe.
fn axe_wear(fx: &Fixture) -> Option<u16> {
    let inv = fx.server.inventory_of(1)?;
    inv.slots[usize::from(inv.held)]
        .as_ref()
        .filter(|s| s.item == AXE)
        .map(|s| s.wear)
}

fn send_sweep(fx: &mut Fixture, f: FaceFrame, look: [f64; 3], vertical: bool) {
    fx.send(
        1,
        Message::Chop(ChopAction::Sweep {
            tool: AXE.into(),
            look: f.vector(look).map(|v| v as f32),
            vertical,
        }),
    );
}

/// The axe's wind-up in ticks (`swing.windupS`): no swing's head reaches anything sooner.
fn windup_ticks() -> u32 {
    let t = chopping::tool(AXE).expect("the axe");
    (t.swing.windup_s * f64::from(SIM_HZ)) as u32
}

dwell_world::both_faces!(
    fn a_chop_lands_at_its_contact_not_on_the_click_and_wears_the_axe_once(f: FaceFrame) {
        // Owner, 2026-11-12: "the gashes in blocks from tools show up on click instead of when the
        // animation lines up with the block"; "Tools should only consume durability on hit."
        let mut fx = near(f, "adventure");
        assert_eq!(axe_wear(&fx), Some(0));
        fx.take_all();
        send_sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        for _ in 0..windup_ticks() {
            fx.server.step();
        }
        assert!(cut_rows(&fx, f).is_empty(), "no cut during the wind-up");
        assert_eq!(axe_wear(&fx), Some(0), "no wear before contact");
        assert!(
            !fx.take(1)
                .iter()
                .any(|(m, _)| matches!(m, Message::Cuts(c) if !c.cuts.is_empty())),
            "no cut sent before contact"
        );
        for _ in 0..SWING_TICKS {
            fx.server.step();
        }
        assert_eq!(cut_rows(&fx, f).len(), 1, "cut at contact");
        assert_eq!(axe_wear(&fx), Some(1), "one hit, one wear");
    }
);

dwell_world::both_faces!(
    fn a_bounce_wears_the_axe_once_and_a_miss_or_only_leaves_wear_nothing(f: FaceFrame) {
        // A miss at the air.
        let mut fx = near(f, "adventure");
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        assert_eq!(axe_wear(&fx), Some(0), "a miss");
        // Only leaves on the path: cleared, not hit (the client follows through as on a miss).
        let wall = leaf_wall(&mut fx, f, 0);
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        assert!(cleared(&mut fx, f, &wall) >= 2, "the leaves are cleared");
        assert_eq!(axe_wear(&fx), Some(0), "leaves alone");
        // A stone post on the path: a bounce is a hit.
        let mut fx = near(f, "adventure");
        let stone = f.state(require("dwell:stone"));
        for r in BASE..BASE + 6 {
            let [x, y, z] = f.cell([2, r, right_z(f)]);
            fx.server.world().set_voxel(x, y, z, stone);
        }
        fx.server.step();
        send_sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        for _ in 0..windup_ticks() {
            fx.server.step();
        }
        assert_eq!(axe_wear(&fx), Some(0), "not before the contact");
        for _ in 0..SWING_TICKS {
            fx.server.step();
        }
        assert!(cut_rows(&fx, f).is_empty());
        assert_eq!(axe_wear(&fx), Some(1), "a bounce wears once");
    }
);

dwell_world::both_faces!(
    fn a_swing_whose_axe_is_put_away_before_contact_lands_nothing(f: FaceFrame) {
        // Adventure: another hotbar slot selected mid-swing.
        let mut fx = near(f, "adventure");
        let other = fx.server.inventory_of(1).unwrap().slots[..6]
            .iter()
            .position(|s| s.as_ref().is_none_or(|s| s.item != AXE))
            .expect("a hotbar slot without an axe") as u8;
        send_sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        fx.server.step();
        fx.send(
            1,
            Message::InventoryAction(InventoryAction::Select { slot: other }),
        );
        for _ in 0..SWING_TICKS {
            fx.server.step();
        }
        assert!(cut_rows(&fx, f).is_empty(), "adventure: cancelled");
        let inv = fx.server.inventory_of(1).unwrap();
        assert!(inv.slots.iter().flatten().all(|s| s.wear == 0));
        // Creative: the hotbar's tool told as another.
        let mut fx = near(f, "creative");
        say_held(&mut fx, 1, AXE, false);
        send_sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        fx.server.step();
        say_held(&mut fx, 1, "dwell:stone_pickaxe", false);
        for _ in 0..SWING_TICKS {
            fx.server.step();
        }
        assert!(cut_rows(&fx, f).is_empty(), "creative: cancelled");
        // Held again, the next swing cuts.
        say_held(&mut fx, 1, AXE, false);
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        assert!(!cut_rows(&fx, f).is_empty(), "then it cuts");
    }
);

/// The last snapshot each session was sent since the outbox was drained.
fn last_snapshots(fx: &mut Fixture) -> std::collections::BTreeMap<SessionId, PhysicsSnapshot> {
    let mut out = std::collections::BTreeMap::new();
    for o in fx.server.take_outbox() {
        if matches!(o.kind, dwell_sim::server::OutKind::Datagram)
            && let Some(Message::PhysicsSnapshot(s)) = Message::decode(&o.bytes)
        {
            out.insert(o.session, s);
        }
    }
    out
}

dwell_world::both_faces!(
    /// WORK_QUEUE item 17: others saw a chop as a `break` (the swing byte says break or place). A
    /// chop steps the swinger's swing count as a break, so an older client still plays that, and a
    /// client that said `SwingHello` gets its kind too: a level chop, then a vertical one. One
    /// that didn't is never sent the kinds (an older client refuses the trailing bytes). Red
    /// before: a chop didn't step the count at all, and no snapshot carried a kind.
    fn a_chop_reaches_a_swing_hello_client_by_its_direction_and_others_as_a_break(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        let (kinds, plain) = (fx.join(2, &Client::new(2)), fx.join(3, &Client::new(3)));
        fx.server.on_datagram(2, &Message::SwingHello.encode());
        for _ in 0..3 {
            fx.server.step();
        }
        let before = last_snapshots(&mut fx);
        // Player 1: the one that is neither of the watchers.
        let swinger = |s: &PhysicsSnapshot, me: u16, other: u16| {
            *s.remotes
                .iter()
                .find(|r| r.player_id != me && r.player_id != other)
                .expect("the swinger in view")
        };
        let count = |r: RemotePlayerState| r.swing & swing_flags::COUNT_MASK;
        let start = count(swinger(&before[&2], kinds.player_id, plain.player_id));
        assert!(before[&2].swing_kinds && !before[&3].swing_kinds);
        for (i, (vertical, want)) in [
            (false, SwingKind::ChopLevel),
            (true, SwingKind::ChopVertical),
        ]
        .into_iter()
        .enumerate()
        {
            at_trunk(&mut fx, f, 0, vertical);
            let snaps = last_snapshots(&mut fx);
            let (k, p) = (
                swinger(&snaps[&2], kinds.player_id, plain.player_id),
                swinger(&snaps[&3], plain.player_id, kinds.player_id),
            );
            let stepped = (start + i as u8 + 1) & swing_flags::COUNT_MASK;
            assert_eq!(
                (count(k), count(p)),
                (stepped, stepped),
                "the swing counted"
            );
            assert!(k.swing & swing_flags::BREAK != 0 && p.swing & swing_flags::BREAK != 0);
            assert_eq!(
                k.swing_kind, want as u8,
                "the hello's client: the chop's kind"
            );
            assert!(!snaps[&3].swing_kinds, "the other: no kinds, a plain break");
            assert_eq!(p.swing_kind, SwingKind::Plain as u8);
        }
        // A break edit after the chops is plain again.
        let [x, y, z] = f.cell([5, BASE - 1, 0]);
        fx.send(
            1,
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Break,
                cell: [x, y, z],
                face: 2,
                material: 0,
                rows: 1,
                down: false,
                point: None,
            }),
        );
        for _ in 0..4 {
            fx.server.step();
        }
        let snaps = last_snapshots(&mut fx);
        let k = swinger(&snaps[&2], kinds.player_id, plain.player_id);
        assert_eq!(count(k), (start + 3) & swing_flags::COUNT_MASK);
        assert_eq!(k.swing_kind, SwingKind::Plain as u8);
    }
);

/// Level swings swept as a player swings them (`ChopAction::Sweep`), standing 1.5 m from the
/// trunk and looking at it `pitch_deg` below level (from the eye's own height), until a trunk
/// cell is felled (at most `limit`); the count.
fn sweeps_to_fell(f: FaceFrame, pitch_deg: f64, limit: usize) -> Option<usize> {
    let mut fx = near(f, "adventure");
    let p = pitch_deg.to_radians();
    for i in 0..limit {
        sweep(&mut fx, f, [p.cos(), -p.sin(), 0.0], false);
        if (BASE..BASE + 10).any(|r| cell_is(&mut fx, f, [3, r, 0]) != log(f, "y")) {
            return Some(i + 1);
        }
    }
    None
}

dwell_world::both_faces!(
    /// A player chopping a trunk as one does, the same aim swing after swing (BUGS.md #75), fells
    /// it in about the stone axe's dozen chops (the owner, WORK_QUEUE item 7): the swing comes in
    /// where its side is open, so one aim notches. Red before: the aim's band was hacked alone.
    fn the_same_level_aim_swing_after_swing_fells_a_trunk_in_about_twelve(f: FaceFrame) {
        for pitch in [3.0, 18.0] {
            let n = sweeps_to_fell(f, pitch, 40);
            assert!(
                n.is_some_and(|n| (9..=14).contains(&n)),
                "aimed {pitch} degrees down: {n:?} swings"
            );
        }
    }
);

// The axe like the other tools (WORK_QUEUE item 51; owner, 2026-11-12: "Like the others, felling
// on top"): a log takes block damage, hits to break by the axe (its `damage` data), a mark where
// each hit struck, its drops spread over the hits; the hit that breaks it fells the tree above.

/// A swing at the trunk's −x side in the lower band, struck `z` across it (face-local).
fn at_trunk_across(fx: &mut Fixture, f: FaceFrame, z: f64) {
    let y = m_of_row(AT) + 0.12;
    swing(fx, f, [3, AT, 0], [-1.0, 0.0, 0.0], [3.0, y, z], false);
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

/// Swings at the trunk's −x side until the cell at AT is no longer a log (at most `limit`).
fn hits_to_break_trunk(fx: &mut Fixture, f: FaceFrame, limit: usize) -> Option<usize> {
    swings_to_fell(fx, f, false, limit, |i| i % 2)
}

dwell_world::both_faces!(
    fn an_axe_breaks_a_log_in_its_damage_hits_stone_ten_metal_five(f: FaceFrame) {
        // The data: the stone axe ≈ the old fell's 11 swings scaled to the 50 % rule (11 × 0.5 /
        // 0.55), the metal axe half (owner: metal reaches 50 % broken faster).
        assert_eq!((log_hits(AXE), log_hits("dwell:metal_axe")), (10, 5));
        let mut fx = scene(f, "adventure", true);
        let n = hits_to_break_trunk(&mut fx, f, 20).expect("broken");
        assert_eq!(n as u32, log_hits(AXE), "stone axe");
        assert_eq!(cell_is(&mut fx, f, [3, AT, 0]), M.air, "broken, no wedge");
        // Each hit before the last was recorded with its mark (no cut).
        assert!(fx.server.cut_of(f.cell([3, AT, 0])).is_none());
        assert!(
            fx.server.damage_of(f.cell([3, AT, 0])).is_none(),
            "gone with it"
        );
    }
);

dwell_world::both_faces!(
    fn each_axe_hit_marks_where_it_struck_with_the_bite_rules_chunk_and_wears_once(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        at_trunk_across(&mut fx, f, 0.3);
        let d = fx
            .server
            .damage_of(f.cell([3, AT, 0]))
            .expect("damaged")
            .clone();
        assert_eq!((d.hits, d.need), (1, log_hits(AXE) as u16));
        assert_eq!(d.marks.len(), 1);
        let m = d.marks[0];
        assert_eq!(m.face, 1, "−x (an edit's face 1)");
        assert!((i32::from(m.point[2]) - 77).abs() <= 2, "0.3 across: {m:?}");
        assert_eq!(m.point[0], 0, "on the −x face");
        // Face-locally the lower band; world-oriented, so turned over on face B.
        let up = if f == FaceFrame::A {
            m.point[1]
        } else {
            255 - m.point[1]
        };
        assert!(up < 128, "the lower band: {m:?}");
        assert_eq!(
            m.chunk,
            dwell_protocol::damage::hit_chunk("dwell:log", Some(AXE)),
            "the bite rule's chunk"
        );
        let bite = dwell_protocol::damage::bite_m3_of("dwell:log", Some(AXE)).unwrap();
        assert!(
            (bite - 0.5 * 0.5 / 10.0).abs() < 1e-9,
            "½ the cell over 10 hits"
        );
        assert_eq!(axe_wear(&fx), Some(1), "one hit, one wear");
        assert!(fx.server.cut_of(f.cell([3, AT, 0])).is_none(), "no cut");
        // No cut reaches the client either: its marks are its damage.
        assert!(
            fx.take(1)
                .iter()
                .all(|(m, _)| !matches!(m, Message::Cuts(c) if !c.cuts.is_empty()))
        );
    }
);

dwell_world::both_faces!(
    fn breaking_a_trunk_log_fells_the_tree_above_as_one_body_toward_the_side_struck(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        hits_to_break_trunk(&mut fx, f, 20).expect("broken");
        let sent = fx.take(1);
        let spawns: Vec<EntitySpawn> = sent
            .iter()
            .filter_map(|(m, _)| match m {
                Message::EntitySpawn(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(spawns.len(), 1, "one body");
        let spawn = &spawns[0];
        assert_eq!(
            spawn.cells.len(),
            (BASE + 10 - AT - 1) as usize,
            "the trunk above the broken log"
        );
        for r in AT + 1..BASE + 10 {
            assert_eq!(
                cell_is(&mut fx, f, [3, r, 0]),
                M.air,
                "row {r} went with it"
            );
        }
        assert_eq!(
            cell_is(&mut fx, f, [3, AT - 1, 0]),
            log(f, "y"),
            "the stump stays"
        );
        // The block's drops: its 8 wood, half over the hits before the last, the rest at the break
        // (the trunk itself falls as a body, not as drops).
        assert_eq!(wood_count(&fx), 8);
        // It tips toward the side struck (−x, the player's): the uncut side is the hinge.
        let id = spawn.id;
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
        assert!(lean[0] < -0.8, "toward −x, the side struck: {lean:?}");
        assert!(lean[2].abs() < 0.2, "not sideways: {lean:?}");
    }
);

dwell_world::both_faces!(
    fn a_lone_log_breaks_in_its_hits_drops_its_wood_and_nothing_falls(f: FaceFrame) {
        // Bucking: a lying log (a felled trunk's, settled) takes the same hits.
        let mut fx = scene(f, "adventure", false);
        move_to_local(&mut fx, f, 2.5, m_of_row(BASE), 0.5);
        fx.server.step();
        let n = log_hits(AXE);
        for i in 0..n {
            assert_eq!(
                cell_is(&mut fx, f, [3, BASE, 0]),
                log(f, "z"),
                "hit {i}: still a log"
            );
            at_log(&mut fx, f, (i % 2) as usize, true);
        }
        assert_eq!(cell_is(&mut fx, f, [3, BASE, 0]), M.air, "broken after {n}");
        assert_eq!(wood_count(&fx), 8, "its drops");
        assert!(
            fx.take(1)
                .iter()
                .all(|(m, _)| !matches!(m, Message::EntitySpawn(_))),
            "nothing to fell"
        );
    }
);

#[test]
fn creative_breaks_a_log_in_its_hits_and_drops_nothing() {
    // face-a-only: a mode rule (the hits are the axe's in creative too: a tool keeps its hits).
    let f = FaceFrame::A;
    let mut fx = scene(f, "creative", false);
    move_to_local(&mut fx, f, 2.5, m_of_row(BASE), 0.5);
    fx.server.step();
    for i in 0..log_hits(AXE) {
        at_log(&mut fx, f, (i % 2) as usize, false);
    }
    assert_eq!(cell_is(&mut fx, f, [3, BASE, 0]), M.air);
    assert!(fx.server.loose_items().is_empty(), "creative drops nothing");
}

#[test]
fn an_older_saves_cut_still_loads_and_draws_and_its_progress_carries_into_damage() {
    // face-a-only: a byte format. An older build's chopping saved cuts (notches, kerfs, bites):
    // they still load and reach clients (drawn as before) until the cell is hit again, when its
    // share of the swings to fell carries over as hits.
    use dwell_protocol::chopping::{BITE_LOOK, Cut};
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
        let mut fx = scene_with(f, cfg(), true, true);
        fx.server.put_cut(cell, old);
        fx.server.save_now();
        fx.server.flush_store();
    }
    let mut fx = scene_with(f, cfg(), true, false);
    assert_eq!(fx.server.cut_of(cell), Some(old), "loaded");
    fx.send(1, Message::Chop(ChopAction::Hello));
    fx.server.step();
    assert!(
        fx.take(1).iter().any(
            |(m, _)| matches!(m, Message::Cuts(c) if c.cuts.iter().any(|(at, k)| *at == cell && k.notch == old.notch && k.kerf == old.kerf))
        ),
        "sent to a client, drawn as before"
    );
    let wood = dwell_protocol::chopping::wood("dwell:log").unwrap();
    let carried = (old.progress(wood, 1) * f64::from(log_hits(AXE))).floor() as u16;
    assert!(carried >= 1, "{carried}");
    at_trunk(&mut fx, f, 0, false);
    assert!(fx.server.cut_of(cell).is_none(), "the cut gives way");
    assert_eq!(
        fx.server.damage_of(cell).map(|d| d.hits),
        Some(carried + 1),
        "its progress and the hit"
    );
    assert!(
        fx.take(1)
            .iter()
            .any(|(m, _)| matches!(m, Message::Cuts(c) if c.gone.contains(&cell))),
        "the clients drop the cut"
    );
}
