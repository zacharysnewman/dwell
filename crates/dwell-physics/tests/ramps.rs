//! The controller on the half-height grid's generated ground (HALF_HEIGHT_GRID.md §4, Phase 41):
//! columns of ½ m cells topped by the stored cardinal ramps the generation rule picks
//! (`dwell_worldgen::ramps::top_of`) — ½ m ramps (26.57°) are rested on and walked, 1 m ramps
//! (45°) slide (PLAYER_CONTROLLER.md §6.8; the slide itself is in `slide.rs`) — with no launches
//! or hops, and nothing reshapes when a block is placed. Replaces the Phase 18 adaptive suite.
//! Runs in the local frame like every suite (far from the origin and on face B too).

mod common;

use common::*;
use dwell_blocks::shape::{shape_of, solid_span_at};
use dwell_blocks::{StateId, require};
use dwell_physics::Vec3;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{State, events};
use dwell_world::ramps::{gentle_cell, steep_cells};
use dwell_world::{CELL_H, rows_for_m};
use dwell_worldgen::ramps::{Around, Top, top_of};

fn grass() -> StateId {
    require("dwell:grass")
}

/// The rule's top for column (x, z) of `height` (rows).
fn top_at(height: &dyn Fn(i32, i32) -> i32, x: i32, z: i32) -> Top {
    let around: Around = std::array::from_fn(|r| {
        std::array::from_fn(|c| height(x + c as i32 - 1, z + r as i32 - 1))
    });
    top_of(&around, rows_for_m(1.0))
}

/// The cells of column (x, z), from row 0 up: grass, its top replaced by the rule's ramp pieces.
fn column(height: &dyn Fn(i32, i32) -> i32, x: i32, z: i32) -> Vec<StateId> {
    let h = height(x, z);
    let mut cells = vec![grass(); h.max(0) as usize];
    let pieces = match top_at(height, x, z) {
        Top::Flat => vec![],
        Top::Gentle(f) => vec![gentle_cell("dwell:grass", f, false).expect("gentle ramp")],
        Top::Steep(f) => steep_cells("dwell:grass", f, false).expect("steep ramp"),
    };
    if pieces.len() <= cells.len() {
        let at = cells.len() - pieces.len();
        cells[at..].copy_from_slice(&pieces);
    }
    cells
}

/// Columns of `height(x, z)` cells of grass (rows) on a floor of cubes whose top is at y = 0, over
/// the box [x0, x1] × [z0, z1], shaped by the generation rule.
fn terrain(
    w: &mut PlayerTestWorld,
    (x0, z0, x1, z1): (i32, i32, i32, i32),
    height: &dyn Fn(i32, i32) -> i32,
) {
    w.fill(x0 - 2, -1, z0 - 2, x1 + 2, -1, z1 + 2, cube_block());
    for z in z0..=z1 {
        for x in x0..=x1 {
            for (row, &c) in column(height, x, z).iter().enumerate() {
                w.set_cell(x, row as i32, z, c);
            }
        }
    }
}

/// The surface height (m, local frame) over (x, z).
fn surface(height: &dyn Fn(i32, i32) -> i32, x: f32, z: f32) -> f32 {
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let mut top = 0.0;
    for (row, &c) in column(height, cx, cz).iter().enumerate() {
        let span = solid_span_at(shape_of(c), x - cx as f32, z - cz as f32);
        if span.hi > span.lo {
            top = (row as f32 + span.hi) * CELL_H as f32;
        }
    }
    top
}

/// What one walker saw.
#[derive(Debug, Default)]
struct Walk {
    /// Ticks in the air rising (a hop or a launch) without jumping.
    rising_airborne: i32,
    /// Fastest upward speed (m/s).
    max_up: f32,
    /// Highest feet height above the surface under the body's centre.
    max_gap: f32,
}

fn watch(w: &PlayerTestWorld, e: u32, height: &dyn Fn(i32, i32) -> i32, walk: &mut Walk) {
    let v = w.vel(e);
    let p = w.pos(e);
    walk.max_up = walk.max_up.max(v.y);
    if !w.c(e).ground.grounded && v.y > 0.5 {
        walk.rising_airborne += 1;
    }
    walk.max_gap = walk.max_gap.max(w.feet(e) - surface(height, p.x, p.z));
}

/// The Shape Playground's column heights (x, z in 0..32), read as ½ m cells.
fn playground() -> impl Fn(i32, i32) -> i32 + Clone {
    let text = include_str!("../../../shared/blocks/playground-export.json");
    let start = text.find("\"columns\"").expect("columns");
    let rows: Vec<Vec<i32>> = text[start..]
        .lines()
        .skip(1)
        .take(32)
        .map(|l| {
            l.trim()
                .trim_matches(|c| c == '"' || c == ',')
                .chars()
                .map(|c| c.to_digit(36).expect("base 36") as i32)
                .collect()
        })
        .collect();
    assert_eq!(rows.len(), 32);
    move |x: i32, z: i32| {
        if (0..32).contains(&x) && (0..32).contains(&z) {
            rows[z as usize][x as usize]
        } else {
            0
        }
    }
}

/// The playground's heights doubled: 1 m steps, so 45° ramps.
fn steep_playground() -> impl Fn(i32, i32) -> i32 + Clone {
    let h = playground();
    move |x, z| 2 * h(x, z)
}

#[test]
fn the_rule_makes_gentle_and_steep_ramps_on_the_playgrounds() {
    let count = |height: &dyn Fn(i32, i32) -> i32| {
        let (mut gentle, mut steep) = (0, 0);
        for z in 0..32 {
            for x in 0..32 {
                match top_at(height, x, z) {
                    Top::Gentle(_) => gentle += 1,
                    Top::Steep(_) => steep += 1,
                    Top::Flat => {}
                }
            }
        }
        (gentle, steep)
    };
    let (g, s) = count(&playground());
    assert!(g > 40, "{g} gentle");
    let (_, s2) = count(&steep_playground());
    assert!(s2 > 40 && s2 > s, "{s2} steep");
}

/// Players on the centre of every column's top of `height`: they rest on flat and ½ m ramps, and
/// slide off 1 m ramps.
fn standing_on_every_column(height: &dyn Fn(i32, i32) -> i32) {
    let min_angle = PlayerControllerConfig::default().slide.min_angle;
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (0, 0, 31, 31), height);
    let mut players = Vec::new();
    for z in 0..32 {
        for x in 0..32 {
            if height(x, z) == 0 {
                continue;
            }
            let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
            let steep = matches!(top_at(height, x, z), Top::Steep(_));
            let feet = surface(height, fx, fz) + 0.02;
            players.push((x, z, steep, w.spawn(Vec3::new(fx, feet, fz))));
        }
    }
    assert!(players.len() > 240, "{}", players.len());
    let spawned: Vec<Vec3> = players.iter().map(|&(.., e)| w.pos(e)).collect();
    w.step(ticks(1.0));
    let start: Vec<Vec3> = players.iter().map(|&(.., e)| w.pos(e)).collect();
    w.step(ticks(2.0));
    let (mut moved, mut stayed, mut resting_on_steep) = (vec![], vec![], vec![]);
    for ((&(x, z, steep, e), s), s0) in players.iter().zip(&start).zip(&spawned) {
        let c = w.c(e);
        let p = w.pos(e);
        // (A slider can come to rest against the player on the next column, or a player beside a
        // steep ramp be pushed by one.)
        let touching = players.iter().any(|&(.., o)| {
            let d = w.pos(o) - p;
            o != e && d.x.hypot(d.z) < 2.0 * 0.3 + 0.05 && d.y.abs() < 1.8
        });
        if c.ground.grounded
            && c.ground.under_angle > min_angle
            && w.vel(e).length() < 0.2
            && !touching
        {
            resting_on_steep.push(format!("({x},{z}) at {p}"));
        }
        if steep {
            if (p - *s0).length() < 0.05 {
                stayed.push(format!("({x},{z}) under {} at {p}", c.ground.under_angle));
            }
            continue;
        }
        let d = p - *s;
        if (!c.ground.grounded || d.length() > 0.01) && !touching {
            moved.push(format!(
                "({x},{z}) moved {d} grounded {}",
                c.ground.grounded
            ));
        }
    }
    assert!(moved.is_empty(), "{} moved: {moved:?}", moved.len());
    assert!(
        stayed.is_empty(),
        "{} stayed on 45°: {stayed:?}",
        stayed.len()
    );
    assert!(
        resting_on_steep.is_empty(),
        "{} at rest on steep ground: {resting_on_steep:?}",
        resting_on_steep.len()
    );
}

#[test]
fn standing_on_every_column_of_the_half_metre_playground_rests() {
    standing_on_every_column(&playground());
}

#[test]
fn standing_on_every_column_of_the_metre_playground_rests_unless_on_a_45_degree_ramp() {
    standing_on_every_column(&steep_playground());
}

/// Walkers along every row (`along_x`, +x) or every column (+z) of `height`, from just outside
/// the grid, 1 m apart; returns the ones that hopped, launched or jumped.
fn walk(height: &dyn Fn(i32, i32) -> i32, along_x: bool) -> Vec<String> {
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (0, 0, 31, 31), height);
    let yaw = if along_x { 90.0 } else { 0.0 };
    let walkers: Vec<u32> = (1..31)
        .map(|k| {
            let c = k as f32 + 0.5;
            let start = if along_x {
                Vec3::new(-1.5, 0.02, c)
            } else {
                Vec3::new(c, 0.02, -1.5)
            };
            w.spawn_yaw(start, yaw)
        })
        .collect();
    w.set_input(move |_, _| input(0.0, 1.0, false, false, false, yaw, 0.0));
    let mut seen: Vec<Walk> = walkers.iter().map(|_| Walk::default()).collect();
    for _ in 0..ticks(8.0) {
        w.step(1);
        for (k, &e) in walkers.iter().enumerate() {
            watch(&w, e, height, &mut seen[k]);
        }
    }
    let speed = w.config.movement.walk_speed;
    let mut bad = Vec::new();
    for (k, &e) in walkers.iter().enumerate() {
        let s = &seen[k];
        // Up a 45° ramp at walking speed the body rises at most at that speed; a ½ m step up is
        // a step, not a hop.
        if s.rising_airborne > 3 || s.max_up > speed + 0.5 || w.count(e, events::JUMPED) > 0 {
            bad.push(format!(
                "{} {}: {s:?}",
                if along_x { "row" } else { "column" },
                k + 1
            ));
        }
    }
    bad
}

#[test]
fn walking_along_every_row_and_column_of_the_playgrounds_never_launches() {
    for height in [
        &playground() as &dyn Fn(i32, i32) -> i32,
        &steep_playground(),
    ] {
        for along_x in [true, false] {
            let bad = walk(height, along_x);
            assert!(bad.is_empty(), "{} launched: {bad:?}", bad.len());
        }
    }
}

#[test]
fn a_half_metre_rise_is_a_ramp_walked_up_and_down_without_jumping() {
    // One cell deep for z < 4, two from z = 4: the edge column takes a ½ m ramp.
    let height = |_: i32, z: i32| if z < 4 { 1 } else { 2 };
    assert!(matches!(top_at(&height, 0, 4), Top::Gentle(_)));
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-8, -8, 8, 16), &height);
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    assert!(approx(w.feet(e), 0.5, 0.01), "{}", w.feet(e));
    w.set_input(|_, _| mv(0.0, 1.0));
    let mut walk = Walk::default();
    for _ in 0..ticks(2.0) {
        w.step(1);
        watch(&w, e, &height, &mut walk);
    }
    assert!(w.pos(e).z > 7.0, "{}", w.pos(e));
    assert!(
        approx(w.feet(e), 1.0, 0.01),
        "on the upper level: {}",
        w.feet(e)
    );
    assert_eq!(w.count(e, events::JUMPED), 0);
    assert_eq!(walk.rising_airborne, 0, "{walk:?}");
    // (Over the crest the capsule's round bottom (radius 0.3 m) bridges from the ramp to the
    // level above, so its feet sit up to ~0.13 m over the surface under its centre: more is a hop.)
    assert!(walk.max_gap < 0.15, "{walk:?}");
    w.set_input(|_, _| mv(0.0, -1.0));
    let mut airborne = 0;
    for _ in 0..ticks(2.0) {
        w.step(1);
        if !w.c(e).ground.grounded {
            airborne += 1;
        }
    }
    assert!(w.pos(e).z < 2.0, "{}", w.pos(e));
    assert!(approx(w.feet(e), 0.5, 0.01), "{}", w.feet(e));
    assert!(
        airborne <= 2,
        "walks down the ramp, not off a ledge: {airborne}"
    );
}

#[test]
fn a_placed_half_metre_block_is_stepped_onto_without_jumping() {
    // A player-placed block is flat (no ramp): a ½ m ledge, within the step height.
    let height = |_: i32, _: i32| 1;
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-8, -8, 8, 8), &height);
    for x in -8..=8 {
        w.set_cell(x, 1, 4, grass());
    }
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    let mut on_top = false;
    for _ in 0..ticks(1.5) {
        w.step(1);
        let p = w.pos(e);
        on_top |= (4.3..4.7).contains(&p.z) && approx(w.feet(e), 1.0, 0.01);
    }
    assert!(on_top, "stepped onto the ledge");
    assert!(w.pos(e).z > 5.0, "{}", w.pos(e));
    assert_eq!(w.count(e, events::JUMPED), 0);
}

#[test]
fn walking_a_half_metre_grass_field_across_chunk_seams_never_loses_the_ground() {
    let height = |_: i32, _: i32| 1;
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-60, -60, 59, 59), &height);
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    w.set_input(|t, _| input(0.0, 1.0, true, false, false, ((t / 120) * 45) as f32, 0.0));
    let (mut airborne, mut max_lift) = (0, 0.0f32);
    for _ in 0..120 * 8 {
        w.step(1);
        if !w.c(e).ground.grounded {
            airborne += 1;
        }
        max_lift = max_lift.max((w.feet(e) - 0.5).abs());
    }
    assert_eq!(airborne, 0);
    assert!(
        max_lift < 0.003,
        "sub-millimetre ripples at most: {max_lift}"
    );
}

#[test]
fn a_block_placed_beside_a_standing_player_leaves_the_ground_under_them_alone() {
    // No reshaping after generation (HALF_HEIGHT_GRID.md §2): the ground under the player keeps
    // its height to the bit.
    let height = |_: i32, _: i32| 1;
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-8, -8, 8, 8), &height);
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    let before = w.feet(e);
    w.set_cell(1, 1, 0, grass());
    w.set_cell(1, 2, 0, grass());
    w.step(ticks(1.0));
    assert!(w.c(e).ground.grounded);
    assert_eq!(w.feet(e), before);
}

/// Walks or runs into a 45° ramp's foot from below and holds forward: (state switches between
/// sliding and walking/running while pressed against it, the feet's highest point, the last state).
fn press_into_ramp_foot(angle: f32, run: bool, start: f32) -> (i32, f32, State) {
    // One cell deep for z < 4, three from z = 4 (a 1 m rise): the edge column takes a 45° ramp.
    let height = |_: i32, z: i32| if z < 4 { 1 } else { 3 };
    assert!(matches!(top_at(&height, 0, 4), Top::Steep(_)));
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-12, -8, 12, 16), &height);
    let rad = angle.to_radians();
    // `start` m (along the path) before the capsule's front meets the ramp's foot at z = 4.
    let along = start + 0.3 / rad.cos();
    let e = w.spawn(Vec3::new(
        0.5 - rad.sin() * along,
        0.52,
        4.0 - rad.cos() * along,
    ));
    w.step(ticks(0.5));
    w.set_input(move |_, _| input(0.0, 1.0, run, false, false, angle, 0.0));
    let (mut switches, mut highest) = (0, f32::MIN);
    let mut last = None;
    for t in 0..ticks(2.5) {
        w.step(1);
        highest = highest.max(w.feet(e));
        let sliding = w.c(e).state == State::Sliding;
        if std::env::var("DWELL_TRACE_FOOT").is_ok() {
            let c = w.c(e);
            eprintln!(
                "{t} pos {} feet {:.3} vel {} {:?} gr {} gap {:.3} angle {:.1} under {:.1} grace {}",
                w.pos(e),
                w.feet(e),
                w.vel(e),
                c.state,
                c.ground.grounded,
                c.ground.gap,
                c.ground.slope_angle,
                c.ground.under_angle,
                c.vertical.step_grace
            );
        }
        if t > ticks(0.3) {
            if let Some(was) = last
                && was != sliding
            {
                switches += 1;
            }
            last = Some(sliding);
        }
    }
    (switches, highest, w.c(e).state)
}

#[test]
fn pressing_into_a_45_degree_ramps_foot_does_not_flicker_between_sliding_and_walking() {
    // Playtest (2026-10-20): walking into a 45° face from below, the player rested on its toe
    // ~0.24 m up and its state switched between sliding and walking every tick.
    let mut fails = Vec::new();
    let mut total = 0;
    for run in [false, true] {
        for angle in [0.0, 15.0, 30.0, 45.0, 60.0] {
            for start in [0.4, 0.43, 0.46, 0.49] {
                total += 1;
                let (switches, highest, state) = press_into_ramp_foot(angle, run, start);
                // The capsule's round bottom rests on the toe at most: 0.3 m over the lower level
                // (0.5 m), never far up the face.
                if switches > 2 || highest > 0.85 || state == State::Sliding {
                    fails.push(format!(
                        "run {run} {angle}° from {start}: {switches} switches, feet up to {highest:.2}, ends {state:?}"
                    ));
                }
            }
        }
    }
    eprintln!("ramp foot: {} of {total} flickered", fails.len());
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// What a walker across a hillside did: ticks it was held up (grounded and moving slower than
/// half its speed) and the longest run of them, ticks it rose in the air without jumping, how often it left the ground, switches
/// between walking states and anything else, and how far it got (m).
#[derive(Debug, Default)]
struct Crossing {
    held_up: i32,
    longest_hold: i32,
    rising_airborne: i32,
    take_offs: i32,
    switches: i32,
    distance: f32,
}

/// Walks (or runs) one player across `height` from `start` at `yaw` for `seconds`.
fn cross(
    height: &dyn Fn(i32, i32) -> i32,
    size: i32,
    start: Vec3,
    yaw: f32,
    run: bool,
    seconds: f32,
) -> Crossing {
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (0, 0, size - 1, size - 1), height);
    let feet = surface(height, start.x, start.z) + 0.02;
    let e = w.spawn_yaw(Vec3::new(start.x, feet, start.z), yaw);
    w.step(ticks(0.3));
    w.set_input(move |_, _| input(0.0, 1.0, run, false, false, yaw, 0.0));
    let speed = if run {
        w.config.movement.run_speed
    } else {
        w.config.movement.walk_speed
    };
    let from = w.pos(e);
    let mut c = Crossing::default();
    let (mut was_grounded, mut was_moving, mut last, mut hold) = (true, None, from, 0);
    for t in 0..ticks(seconds) {
        w.step(1);
        let ctl = w.c(e);
        let grounded = ctl.ground.grounded;
        let moving = matches!(ctl.state, State::Walking | State::Running);
        let p = w.pos(e);
        // How far it moved this tick (the body's velocity after a step-up is what the step's
        // shift left of it, not the motion).
        let moved = (p.x - last.x).hypot(p.z - last.z) * 60.0;
        last = p;
        if t > ticks(0.4) {
            if grounded && moved < 0.5 * speed {
                c.held_up += 1;
                hold += 1;
                c.longest_hold = c.longest_hold.max(hold);
            } else {
                hold = 0;
            }
            c.rising_airborne += i32::from(!grounded && w.vel(e).y > 0.5);
            c.take_offs += i32::from(was_grounded && !grounded);
            c.switches += i32::from(was_moving.is_some_and(|m| m != moving));
        }
        (was_grounded, was_moving) = (grounded, Some(moving));
    }
    let d = w.pos(e) - from;
    c.distance = d.x.hypot(d.z);
    assert_eq!(w.count(e, events::JUMPED), 0);
    c
}

/// A plane rising `slope` m per m toward `uphill` (degrees from +x toward +z), in rows over
/// columns, as the generator rounds a height: ceil(h ÷ cell height).
fn hillside(slope: f32, uphill: f32) -> impl Fn(i32, i32) -> i32 + Clone {
    let (s, c) = uphill.to_radians().sin_cos();
    move |x: i32, z: i32| {
        let h = 2.0 + slope * ((x as f32 + 0.5) * c + (z as f32 + 0.5) * s + 60.0);
        (h / CELL_H as f32).ceil() as i32
    }
}

#[test]
fn walking_and_running_over_diagonal_hillsides_has_no_hang_ups_or_flicker() {
    // Owner, 2026-10-20 (option A, HALF_HEIGHT_GRID.md §4): a hillside whose fall line is off the
    // axes is ½ m ramps facing the dominant axis, with ½ m wedge-shaped side faces where the
    // contour shifts sideways. Walkers cross it up, down, along and across its fall line on ½ m
    // ramp ground (no 45° pieces): step-up (0.55 m) takes the side faces in stride, so nobody is
    // held up, hops or flickers between walking and anything else.
    const SIZE: i32 = 48;
    let mut fails = Vec::new();
    let (mut walks, mut side_faces) = (0, 0);
    for slope in [0.3, 0.45] {
        for uphill in [30.0f32, 45.0, 60.0] {
            let height = hillside(slope, uphill);
            for z in 1..SIZE - 1 {
                for x in 1..SIZE - 1 {
                    let top = top_at(&height, x, z);
                    assert!(
                        !matches!(top, Top::Steep(_)),
                        "{slope} {uphill}: ({x}, {z})"
                    );
                    // A ramp with a lower column beside it: a ½ m side face.
                    if let Top::Gentle(f) = top {
                        let (dx, dz) = f.step();
                        let lower = |sx: i32, sz: i32| height(x + sx, z + sz) < height(x, z);
                        side_faces += i32::from(lower(dz, dx) || lower(-dz, -dx));
                    }
                }
            }
            // Headings (yaw 0 is +z, 90 is +x): up and down the fall line, along each axis, and
            // across the slope.
            let fall = 90.0 - uphill;
            for heading in [fall, fall + 180.0, 0.0, 90.0, 180.0, 270.0, fall + 90.0] {
                for run in [false, true] {
                    walks += 1;
                    let h = f32::to_radians(heading);
                    let mid = SIZE as f32 * 0.5;
                    // From 14 m back along the heading from the middle.
                    let start = Vec3::new(mid - h.sin() * 14.0, 0.0, mid - h.cos() * 14.0);
                    let c = cross(&height, SIZE, start, heading, run, 3.0);
                    let speed = if run { 8.0 } else { 5.0 };
                    // A hitch of a tick or two at a side face (measured: one tick, once in 84
                    // walks) is not a hang-up; being stopped against one is.
                    if c.longest_hold > 2
                        || c.held_up > 3
                        || c.rising_airborne > 3
                        || c.switches > 0
                        || c.distance < 0.75 * speed * 2.6
                    {
                        fails.push(format!(
                            "slope {slope} uphill {uphill}° heading {heading}° run {run}: {c:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(side_faces > 100, "{side_faces} ramps beside a lower side");
    eprintln!(
        "diagonal hillsides: {} of {walks} walks failed",
        fails.len()
    );
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
