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

dwell_world::both_faces!(
    fn notching_fells_a_trunk_in_about_ten_swings_toward_the_notch(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        let n = swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("felled");
        assert!((8..=12).contains(&n), "{n} swings");
        // Phase 55 T2 (owner, 2026-11-09): the stump keeps the wood ramp rising away from the
        // notch (the notch on −x, the player's side at x = 0.5; the uncut +x side the hinge), the
        // trunk's new bottom is the upside-down ramp (the log's `fellWedge`, data), thin at the
        // notch: the gash (`fellWedgeTrunk: open`; owner 2026-11-12, WORK_QUEUE item 24).
        let wood = dwell_protocol::chopping::wood("dwell:log").expect("log");
        assert!(
            !wood.fell_wedge_matching,
            "the log's trunk wedge is the open one (the gash)"
        );
        assert!(wood.fell_tip > 0.0, "the log tips when felled (`fellTip`)");
        let (stump, top) = dwell_sim::felling::fell_wedges(
            wood.fell_wedge.expect("the log's wedge"),
            [-1.0, 0.0],
            dwell_sim::felling::WedgeTrunk::Open,
        )
        .expect("the log's wedges");
        assert_eq!(
            cell_is(&mut fx, f, [3, AT, 0]),
            f.state(stump),
            "the stump's wedge"
        );
        let prop = dwell_blocks::state_property;
        assert_eq!(
            (prop(stump, "facing"), prop(stump, "half")),
            (Some("west"), Some("bottom")),
            "facing (falling toward) −x, the notch: rising away from it"
        );
        assert_eq!(
            (prop(top, "facing"), prop(top, "half")),
            (Some("west"), Some("top")),
            "upside down, thin at the notch: the gash opens toward it"
        );
        let sent = fx.take(1);
        let spawn = sent
            .iter()
            .find_map(|(m, _)| match m {
                Message::EntitySpawn(s) => Some(s.clone()),
                _ => None,
            })
            .expect("a piece spawned");
        // Its bottom cell (face-local lowest) is the upside-down wedge.
        let up = f.up()[1].signum() as i16;
        let bottom = spawn
            .cells
            .iter()
            .min_by_key(|c| c.offset[1] * up)
            .expect("cells");
        assert_eq!(
            bottom.state,
            f.state(top),
            "the trunk's bottom is the inverted wedge"
        );
        // Released tipping toward the notch (−x) with the old scripted tip (the log's `fellTip`;
        // owner 2026-11-09: "make the tree tip if needed, like we used to, maybe 3/4 of the
        // force"): within four seconds it is down (leaning past 60°) on that side, not sideways.
        // Without the tip a matching wedge stood on the stump's slope; at 0.75 and 0.85 of it this
        // 3.5 m trunk leant ~5° and rocked back upright, so the log's share is 1 (kept with the
        // open gash, 2026-11-12).
        let id = spawn.id;
        let mut lean = [0.0; 3];
        let mut ticks = 0;
        for _ in 0..4 * 60 {
            fx.server.step();
            ticks += 1;
            let Some((_, q)) = fx.server.body_pose(id) else {
                break;
            };
            lean = f.vector((q * dwell_physics::DVec3::from(f.up())).to_array());
            if lean[1] < 0.5 {
                break;
            }
        }
        println!("{f:?}: lean {lean:?} after {ticks} ticks");
        assert!(lean[1] < 0.5, "down within 4 s: {lean:?}");
        assert!(lean[0] < -0.8, "toward −x, the notch: {lean:?}");
        assert!(lean[2].abs() < 0.2, "not sideways: {lean:?}");
        // The stump stays; the client heard the cut and then its end.
        assert_ne!(cell_is(&mut fx, f, [3, AT - 1, 0]), M.air);
        assert!(
            sent.iter()
                .any(|(m, _)| matches!(m, Message::Cuts(c) if !c.cuts.is_empty()))
        );
        assert!(fx.server.cut_of(f.cell([3, AT, 0])).is_none());
    }
);

/// The face-local heights over a cell's floor of `m`'s hull's top and bottom at its edge toward
/// the level direction `d` (x, z) and the opposite edge: `[top toward, top away, bottom toward,
/// bottom away]`.
fn wedge_edges(m: u16, d: [f64; 2]) -> [f64; 4] {
    let half = dwell_physics::DVec3::new(0.5, 0.25, 0.5);
    let pts = dwell_physics::shape_hull_points(m, half);
    let along = |p: &dwell_physics::DVec3| p.x * d[0] + p.z * d[1];
    let at = |s: f64| {
        pts.iter()
            .filter(move |p| (along(p) - 0.5 * s).abs() < 1e-6)
    };
    let top = |s: f64| at(s).map(|p| p.y + 0.25).fold(f64::NEG_INFINITY, f64::max);
    let bottom = |s: f64| at(s).map(|p| p.y + 0.25).fold(f64::INFINITY, f64::min);
    [top(1.0), top(-1.0), bottom(1.0), bottom(-1.0)]
}

dwell_world::both_faces!(
    /// WORK_QUEUE item 24 (owner, 2026-11-12: "2 half blocks one upside down one right side up,
    /// both with the 'short side' facing the cut"; confirmed "Yes that was the shape I meant"):
    /// felled from any of the four sides, the stump is the ramp low at the notch rising away from
    /// it and the trunk's lowest cell the upside-down ramp thin at the notch, the two slopes
    /// opening a V toward the side chopped, and the trunk falls that way. Red before: the log's
    /// `fellWedgeTrunk` was `matching`, the trunk's ramp parallel to the stump's ("like you cut it
    /// through with a sword in one swipe").
    fn felled_from_each_side_the_cut_opens_a_gash_toward_the_notch(f: FaceFrame) {
        let prop = dwell_blocks::state_property;
        for (d, facing) in [
            ([-1i32, 0i32], "west"),
            ([1, 0], "east"),
            ([0, -1], "north"),
            ([0, 1], "south"),
        ] {
            let mut fx = scene(f, "creative", true);
            let (dx, dz) = (f64::from(d[0]), f64::from(d[1]));
            // The chopper stands 3 m out on that side, facing the trunk (centre 3.5, 0.5).
            move_to_local(&mut fx, f, 3.5 + 3.0 * dx, m_of_row(BASE), 0.5 + 3.0 * dz);
            for _ in 0..5 {
                fx.server.step();
            }
            fx.take_all();
            let mut felled = false;
            for i in 0..30 {
                let y = m_of_row(AT) + if i % 2 == 0 { 0.12 } else { 0.37 };
                let at = [3.5 + 0.5 * dx, y, 0.5 + 0.5 * dz];
                swing(&mut fx, f, [3, AT, 0], [dx, 0.0, dz], at, false);
                if cell_is(&mut fx, f, [3, AT, 0]) != log(f, "y") {
                    felled = true;
                    break;
                }
            }
            assert!(felled, "{facing}: felled");
            let stump = f.state(cell_is(&mut fx, f, [3, AT, 0]));
            assert_eq!(
                (
                    prop(stump, "shape"),
                    prop(stump, "half"),
                    prop(stump, "facing")
                ),
                (Some("ramp"), Some("bottom"), Some(facing)),
                "{f:?} {facing}: the stump's ramp faces the notch"
            );
            let sent = fx.take(1);
            let spawn = felled_piece(&sent);
            let up = f.up()[1].signum() as i16;
            let low = spawn
                .cells
                .iter()
                .min_by_key(|c| c.offset[1] * up)
                .expect("cells");
            let trunk = f.state(low.state);
            assert_eq!(
                (prop(trunk, "shape"), prop(trunk, "half")),
                (Some("ramp"), Some("top")),
                "{f:?} {facing}: the trunk's bottom is upside down"
            );
            let to = [dx, dz];
            let [s_top_notch, s_top_hinge, ..] = wedge_edges(stump, to);
            let [.., t_bottom_notch, t_bottom_hinge] = wedge_edges(trunk, to);
            assert!(
                s_top_notch < 1e-6 && (s_top_hinge - 0.5).abs() < 1e-6,
                "{f:?} {facing}: the stump low at the notch, rising away"
            );
            assert!(
                (t_bottom_notch - 0.5).abs() < 1e-6 && t_bottom_hinge < 1e-6,
                "{f:?} {facing}: the trunk's ramp thin at the notch (its bottom at the ceiling there)"
            );
            // Over the two cells (the trunk's a row up): the stump's top rises 0 → ½ m from the
            // notch to the hinge, the trunk's bottom falls 1 → ½ m: not parallel, a V open
            // toward the notch, closed at the hinge.
            let gap_notch = (0.5 + t_bottom_notch) - s_top_notch;
            let gap_hinge = (0.5 + t_bottom_hinge) - s_top_hinge;
            assert!(
                gap_notch > 0.9 && gap_hinge.abs() < 1e-6,
                "{f:?} {facing}: the gash opens toward the notch ({gap_notch}, {gap_hinge})"
            );
            let lean = lean_of(&mut fx, f, spawn.id);
            assert!(
                lean[1] < 0.5 && lean[0] * dx + lean[2] * dz > 0.8,
                "{f:?} {facing}: down toward the notch: {lean:?}"
            );
        }
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

dwell_world::both_faces!(
    fn aiming_at_one_spot_notches_as_alternating_the_bands_does(f: FaceFrame) {
        // BUGS.md #75: a swing at a band cut deeper than the other lands beside it, in the open
        // wood, so the same aim notches (before: it hacked one kerf, 28 swings to the notching's 9,
        // and in the browser no fall in 24).
        let mut fx = scene(f, "creative", true);
        let notch = swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("notched");
        let mut fx = scene(f, "creative", true);
        let one = swings_to_fell(&mut fx, f, false, 20, |_| 0).expect("one aim");
        assert_eq!(one, notch);
        assert!((10..=13).contains(&notch), "about twelve: {notch}");
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
    // A client that says Hello is sent the saved cuts (their depths: the bites go only to one that
    // said `BitesHello`).
    fx.send(1, Message::Chop(ChopAction::Hello));
    fx.server.step();
    let same = |k: &dwell_protocol::chopping::Cut| k.notch == before.notch && k.kerf == before.kerf;
    assert!(
        fx.take(1)
            .iter()
            .any(|(m, _)| matches!(m, Message::Cuts(c) if c.cuts.iter().any(|(_, k)| same(k))))
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
        let cut = fx.server.cut_of(f.cell([3, rows[0], 0])).unwrap();
        assert_eq!(cut.deepest(1), Some(0), "the near side: {cut:?}");
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
    fn chop2_off_the_old_chopping_is_unchanged(f: FaceFrame) {
        // Without the switch a chop opens a kerf (the old model), not the prototype's notch.
        const { assert!(!dwell_protocol::chopping::CHOP2_DEFAULT, "off by default") };
        let mut fx = scene(f, "adventure", true);
        at_trunk(&mut fx, f, 0, false);
        let cut = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut");
        assert!(cut.kerf[0][0] > 0.0 && cut.notch[0] == 0.0, "{cut:?}");
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

// The axe's bites (WORK_QUEUE item 26): each hit records where it struck across the side, the
// client carving a head-sized bite there; sent only to a client that said `BitesHello`.

/// A swing at the trunk's −x side in the lower band, struck `z` across it (face-local).
fn at_trunk_across(fx: &mut Fixture, f: FaceFrame, z: f64) {
    let y = m_of_row(AT) + 0.12;
    swing(fx, f, [3, AT, 0], [-1.0, 0.0, 0.0], [3.0, y, z], false);
}

dwell_world::both_faces!(
    fn a_hit_records_its_bite_where_it_struck_and_the_fall_is_unchanged(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        at_trunk_across(&mut fx, f, 0.3);
        let cut = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut");
        // Side 0 (−x), the face-locally lower band, 0.3 across (z, a level axis on both faces).
        let b = cut.bites[0].all();
        assert_eq!(b.len(), 1, "{cut:?}");
        assert!(
            (f64::from(b[0].u) - 0.3).abs() < 1e-6 && b[0].band == 0,
            "{b:?}"
        );
        // The next swing at the same spot lands in the open band, the same place across.
        at_trunk_across(&mut fx, f, 0.3);
        let cut = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut");
        assert_eq!(cut.bites[0].all()[1].band, 1);
        // Hacking that spot still fells it in the rule's swings (ADR 0067 §2, BUGS.md #75).
        let mut n = 2;
        while cell_is(&mut fx, f, [3, AT, 0]) == log(f, "y") && n < 20 {
            at_trunk_across(&mut fx, f, 0.3);
            n += 1;
        }
        assert_eq!(n, 11, "felled at the eleventh swing");
    }
);

#[test]
fn bites_go_only_to_a_client_that_said_bites_hello_and_are_saved() {
    // face-a-only: a wire and byte format.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "creative")
    };
    let before = {
        let mut fx = scene_with(f, cfg(), true, true);
        at_trunk_across(&mut fx, f, 0.4);
        // Without `BitesHello` (an older client): its cuts carry no bites (it can't read them).
        let cuts: Vec<CutsMessage> = fx
            .take(1)
            .into_iter()
            .filter_map(|(m, _)| match m {
                Message::Cuts(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(!cuts.is_empty() && cuts.iter().all(|c| !c.bites));
        // Saying it brings all the cuts again with their bites, and every later one has them.
        fx.server.on_datagram(1, &Message::BitesHello.encode());
        fx.server.step();
        let again: Vec<CutsMessage> = fx
            .take(1)
            .into_iter()
            .filter_map(|(m, _)| match m {
                Message::Cuts(c) => Some(c),
                _ => None,
            })
            .collect();
        assert!(
            again
                .iter()
                .any(|c| c.bites && c.cuts.iter().any(|(_, k)| k.bites[0].n == 1)),
            "{again:?}"
        );
        at_trunk_across(&mut fx, f, 0.4);
        assert!(
            fx.take(1)
                .iter()
                .any(|(m, _)| matches!(m, Message::Cuts(c) if c.bites))
        );
        let c = fx.server.cut_of(f.cell([3, AT, 0])).expect("cut");
        fx.server.save_now();
        fx.server.flush_store();
        c
    };
    assert!(before.has_bites());
    let fx = scene_with(f, cfg(), true, false);
    assert_eq!(
        fx.server.cut_of(f.cell([3, AT, 0])),
        Some(before),
        "saved with its bites"
    );
}

dwell_world::both_faces!(
    /// WORK_QUEUE item 26 (owner, 2026-11-12: "I think let's do your recommendation" — the stump
    /// keeps its gash): a 12th swing at the felled spot hits the stump's fell wedge as wood, a bite
    /// and a wear, and never clears it (red before: the axe's `clears` broke the `dwell:log_ramp`,
    /// leaving a flat stump); the stump comes down only after it is chopped like a log, and in
    /// adventure drops the wood's `buckDrops`.
    fn a_swing_after_the_fall_bites_the_stump_which_comes_down_only_when_chopped(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        let n = swings_to_fell(&mut fx, f, false, 20, |i| i % 2).expect("felled");
        let stump = cell_is(&mut fx, f, [3, AT, 0]);
        let name = dwell_blocks::block_of(f.state(stump)).name;
        assert_eq!(name, "dwell:log_ramp", "the stump's wedge");
        // On the hinge side, out of the trunk's fall (toward −x), facing the stump.
        move_to_local(&mut fx, f, 5.0, m_of_row(BASE), 0.5);
        for _ in 0..60 {
            fx.server.step();
        }
        let worn = axe_wear(&fx).expect("the axe in hand");
        // The 12th swing: its arc meets the stump's wedge.
        sweep(&mut fx, f, [-1.0, 0.0, 0.0], false);
        assert_eq!(
            cell_is(&mut fx, f, [3, AT, 0]),
            stump,
            "{f:?}: the gash stays"
        );
        let cut = fx
            .server
            .cut_of(f.cell([3, AT, 0]))
            .expect("the stump is cut");
        assert!(cut.has_bites(), "{f:?}: a bite in the stump: {cut:?}");
        assert_eq!(axe_wear(&fx), Some(worn + 1), "{f:?}: the hit wore the axe");
        // Chopped like a log: down after about as many swings as the trunk took, not one.
        let wood_before = wood_count(&fx);
        let mut k = 1;
        while cell_is(&mut fx, f, [3, AT, 0]) == stump && k < 40 {
            at_trunk(&mut fx, f, k % 2, false);
            k += 1;
        }
        assert_eq!(
            cell_is(&mut fx, f, [3, AT, 0]),
            M.air,
            "{f:?}: chopped down"
        );
        assert!(
            k + 2 >= n && k >= 6,
            "{f:?}: {k} swings to chop the stump ({n} to fell)"
        );
        for _ in 0..120 {
            fx.server.step();
        }
        assert!(
            wood_count(&fx) > wood_before,
            "{f:?}: the stump's wood dropped"
        );
    }
);

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
