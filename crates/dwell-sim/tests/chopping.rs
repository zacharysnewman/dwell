//! Chopping through the server (docs/adr/0067-stone-axe-and-chopping.md): aimed axe swings cut a
//! standing trunk, which falls toward the notch (the uncut side the hinge) once cut past its fell fraction (notching in
//! about 9 swings, hacking one spot about four times as many, vertical swings barely at all); a
//! lying log is cut through by vertical swings across it and drops wood in adventure; cuts are
//! saved with the world and sent only to clients that said `ChopAction::Hello`.

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
    let n = f.vector(normal);
    let axis = n.iter().position(|v| v.abs() > 0.5).expect("an axis");
    let p = f.point(at);
    fx.send(
        1,
        Message::Chop(ChopAction::Swing {
            tool: AXE.into(),
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
    let y = m_of_row(AT) + if band == 0 { 0.12 } else { 0.37 };
    swing(fx, f, [3, AT, 0], [-1.0, 0.0, 0.0], [3.0, y, 0.5], vertical);
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
    for i in 0..limit {
        at_trunk(fx, f, band_of(i), vertical);
        if cell_is(fx, f, [3, AT, 0]) == M.air {
            return Some(i + 1);
        }
    }
    None
}

dwell_world::both_faces!(
    fn notching_fells_a_trunk_in_about_ten_swings_toward_the_notch(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        let n = swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("felled");
        assert!((8..=12).contains(&n), "{n} swings");
        // The trunk above the cut is a piece, tipping toward the notch (−x, the player's side
        // at x = 0.5): the uncut +x side is the hinge, as in real felling (owner, 2026-11-08).
        let sent = fx.take(1);
        let id = sent
            .iter()
            .find_map(|(m, _)| match m {
                Message::EntitySpawn(s) => Some(s.id),
                _ => None,
            })
            .expect("a piece spawned");
        for _ in 0..40 {
            fx.server.step();
        }
        let (_, q) = fx.server.body_pose(id).expect("still falling");
        let up = q * dwell_physics::DVec3::from(f.up());
        let lean = f.vector(up.to_array());
        assert!(lean[0] < -0.2, "leans toward −x, the notch: {lean:?}");
        // The stump stays; the client heard the cut and then its end.
        assert_ne!(cell_is(&mut fx, f, [3, AT - 1, 0]), M.air);
        assert!(
            sent.iter()
                .any(|(m, _)| matches!(m, Message::Cuts(c) if !c.cuts.is_empty()))
        );
        assert!(fx.server.cut_of(f.cell([3, AT, 0])).is_none());
    }
);

/// The player's centre, face-local.
fn player_at(fx: &mut Fixture, f: FaceFrame) -> [f64; 3] {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    f.point(players.position(physics, h).to_array())
}

dwell_world::both_faces!(
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
);

dwell_world::both_faces!(
    fn hacking_one_spot_takes_three_to_four_times_the_swings_of_notching(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        let notch = swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("notched");
        let mut fx = scene(f, "creative", true);
        let hack = swings_to_fell(&mut fx, f, false, 60, |_| 0).expect("hacked");
        let ratio = hack as f64 / notch as f64;
        assert!(
            (3.0..=4.0).contains(&ratio),
            "{hack} vs {notch}: {ratio:.2}"
        );
    }
);

dwell_world::both_faces!(
    fn vertical_swings_barely_cut_a_standing_trunk(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        assert_eq!(swings_to_fell(&mut fx, f, true, 12, |i| i % 2), None);
        let cut = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut a little");
        let mut h = scene(f, "creative", true);
        at_trunk(&mut h, f, 0, false);
        let one = h.server.cut_of(f.cell([3, AT, 0])).unwrap().depth(0);
        assert!(
            cut.depth(0) < 2.5 * one,
            "12 vertical swings {} vs 1 level {one}",
            cut.depth(0)
        );
    }
);

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

dwell_world::both_faces!(
    fn vertical_swings_cut_a_lying_log_through_and_it_drops_wood(f: FaceFrame) {
        let mut fx = scene(f, "adventure", false);
        // Level swings run along its grain: barely.
        for i in 0..4 {
            at_log(&mut fx, f, i % 2, false);
        }
        let level = fx
            .server
            .cut_of(f.cell([3, BASE, 0]))
            .expect("cut")
            .depth(1);
        let mut fx = scene(f, "adventure", false);
        let mut n = 0;
        while cell_is(&mut fx, f, [3, BASE, 0]) != M.air {
            at_log(&mut fx, f, n % 2, true);
            n += 1;
            assert!(n < 12, "not cut through");
        }
        assert!(level < 0.1, "4 level swings cut {level} m");
        let wood: u16 = fx
            .server
            .loose_items()
            .iter()
            .filter(|it| it.stack.item == "dwell:wood")
            .map(|it| it.stack.count)
            .sum();
        assert!((2..=3).contains(&wood), "{wood} wood");
    }
);

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

#[test]
fn cuts_are_saved_with_the_world() {
    // face-a-only: a byte format.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "creative")
    };
    let before = {
        let mut fx = scene_with(f, cfg(), true, true);
        at_trunk(&mut fx, f, 0, false);
        at_trunk(&mut fx, f, 1, false);
        at_trunk(&mut fx, f, 0, false);
        let c = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut");
        fx.server.save_now();
        fx.server.flush_store();
        c
    };
    let mut fx = scene_with(f, cfg(), true, false);
    assert_eq!(fx.server.cut_of(f.cell([3, AT, 0])), Some(before));
    // A client that says Hello is sent the saved cuts.
    fx.send(1, Message::Chop(ChopAction::Hello));
    fx.server.step();
    assert!(
        fx.take(1).iter().any(
            |(m, _)| matches!(m, Message::Cuts(c) if c.cuts.iter().any(|(_, k)| *k == before))
        )
    );
}

#[test]
fn a_client_that_never_says_hello_is_sent_no_cuts() {
    // face-a-only: a wire rule.
    let f = FaceFrame::A;
    let mut fx = scene_with(f, config(f, "creative"), true, false);
    at_trunk(&mut fx, f, 0, false);
    assert!(
        fx.server.cut_of(f.cell([3, AT, 0])).is_some(),
        "cut all the same"
    );
    assert!(
        fx.take(1)
            .iter()
            .all(|(m, _)| !matches!(m, Message::Cuts(_)))
    );
}

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

/// The trunk's cells (face-local rows BASE..BASE + 10 at x = 3) that have a cut.
fn cut_rows(fx: &Fixture, f: FaceFrame) -> Vec<i32> {
    (BASE..BASE + 10)
        .filter(|&r| fx.server.cut_of(f.cell([3, r, 0])).is_some())
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
        assert!(!cut_rows(&fx, f).is_empty(), "the swing's path cut the trunk");
        // Looking well to its right, the level swing starts past it and sweeps away: a miss.
        let b = 75f64.to_radians();
        let mut fx = near(f, "adventure");
        sweep(&mut fx, f, [b.cos(), 0.0, -left * b.sin()], false);
        assert!(cut_rows(&fx, f).is_empty(), "swept away from it: {:?}", cut_rows(&fx, f));
    }
);

dwell_world::both_faces!(
    fn a_nearer_block_on_the_swings_path_takes_the_hit_and_the_trunk_is_spared(f: FaceFrame) {
        // Aimed straight at the trunk, it is cut.
        let mut fx = near(f, "adventure");
        sweep(&mut fx, f, [1.0, 0.0, 0.0], false);
        assert!(!cut_rows(&fx, f).is_empty(), "the trunk aimed at");
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
        assert!(cut_rows(&fx, f).is_empty(), "the post took it: {:?}", cut_rows(&fx, f));
        for r in BASE..BASE + 6 {
            assert_eq!(cell_is(&mut fx, f, [2, r, right_z(f)]), stone, "a bounce: no effect");
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
    assert!(held_tools_to(&mut fx, 3).is_empty(), "an older client is sent nothing new");
    // A client that says what it holds later gets the others' at once.
    say_held(&mut fx, 3, "", false);
    assert!(held_tools_to(&mut fx, 3).iter().any(|h| h.tool == AXE));
    // Not a tool: held as nothing.
    say_held(&mut fx, 1, "dwell:stone", false);
    assert!(held_tools_to(&mut fx, 2).iter().any(|h| h.tool.is_empty()));
}
