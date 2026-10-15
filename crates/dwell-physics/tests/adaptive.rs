//! The controller on adaptive natural blocks (docs/ADAPTIVE_BLOCKS.md, Phase 18): grass shapes
//! itself from its neighbours, and the player walks the result — the Shape Playground's 28
//! situations (every shape the rules make: slopes, ridges, saddles, half-and-half corners, 45°
//! ramps) built as terrain, a terrace's 1-block step, a ridge, a field across chunk seams — with no
//! launches, hops or sliding at rest. Runs in the local frame like every suite (far from the origin
//! and on face B too).

mod common;

use common::*;
use dwell_blocks::adaptive;
use dwell_blocks::shape::solid_span_at;
use dwell_blocks::{StateId, require};
use dwell_physics::Vec3;
use dwell_physics::controller::events;

fn grass() -> StateId {
    require("dwell:grass")
}

/// Columns of grass on a flat floor of non-adaptive cubes whose top is at y = 0: `height(x, z)`
/// blocks of grass at (x, z), over the box [x0, x1] × [z0, z1].
fn terrain(
    w: &mut PlayerTestWorld,
    (x0, z0, x1, z1): (i32, i32, i32, i32),
    height: &dyn Fn(i32, i32) -> i32,
) {
    w.fill(x0 - 2, -1, z0 - 2, x1 + 2, -1, z1 + 2, cube_block());
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in 0..height(x, z) {
                w.set(x, y, z, grass());
            }
        }
    }
}

/// The surface height (local frame) over (x, z): the top block's derived shape, as the rules give
/// it for these columns (the floor of cubes: its top, y = 0).
fn surface(height: &dyn Fn(i32, i32) -> i32, x: f32, z: f32) -> f32 {
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let h = height(cx, cz);
    if h == 0 {
        return 0.0;
    }
    let (g, cube) = (grass(), cube_block());
    let at = |dx: i32, dy: i32, dz: i32| {
        let y = h - 1 + dy;
        if y < 0 {
            cube
        } else if y < height(cx + dx, cz + dz) {
            g
        } else {
            0
        }
    };
    let s = adaptive::shape_at(at, 1);
    (h - 1) as f32 + solid_span_at(s, x - cx as f32, z - cz as f32).hi
}

/// What one walker saw.
#[derive(Debug, Default)]
struct Walk {
    /// Ticks in the air rising (a hop or a launch) without jumping.
    rising_airborne: i32,
    /// Fastest upward speed (m/s).
    max_up: f32,
    jumps: i32,
    /// Highest feet height above the surface under the body's centre.
    max_gap: f32,
}

fn watch(w: &PlayerTestWorld, e: u32, height: &dyn Fn(i32, i32) -> i32, walk: &mut Walk) {
    let v = w.vel(e);
    let p = w.pos(e);
    walk.max_up = walk.max_up.max(v.y);
    if !w.c(e).ground.grounded && v.y > 0.5 {
        walk.rising_airborne += 1;
        if std::env::var("DWELL_DEBUG").is_ok() {
            eprintln!("player {e} tick {} rising at {p} v {v} surface {}", w.tick, surface(height, p.x, p.z));
        }
    }
    walk.max_gap = walk.max_gap.max(w.feet(e) - surface(height, p.x, p.z));
}

/// The playground's column heights (x, z in 0..32).
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

#[test]
fn the_game_world_shapes_the_playground_like_the_owner_did() {
    // The Rust side of the export check (adaptive.test.mjs): the world's derived shapes, read
    // where the meshers and the collider read them, match the owner's shapes for every block.
    let height = playground();
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (0, 0, 31, 31), &height);
    let text = include_str!("../../../shared/blocks/playground-export.json");
    let start = text.find("\"blocks\"").expect("blocks");
    let mut n = 0;
    for line in text[start..].lines().skip(1) {
        let line = line.trim().trim_end_matches(',');
        if !line.starts_with('[') {
            break;
        }
        let f: Vec<&str> = line
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .collect();
        let (x, z, h): (i32, i32, i32) = (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].parse().unwrap());
        assert_eq!(height(x, z), h);
        let s = w
            .world
            .shape_at(world_cell_x(x), world_cell_y(h - 1), z);
        // The points the rules gave, against the local derivation `surface` uses.
        for (fx, fz) in [(0.5, 0.5), (0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9), (0.5, 0.05), (0.05, 0.5)] {
            let world = solid_span_at(s, fx, fz);
            let height_world = if face_b() { 1.0 - world.lo } else { world.hi };
            let local = surface(&height, x as f32 + fx, z as f32 + fz) - (h - 1) as f32;
            assert!(
                (height_world - local).abs() < 1e-6,
                "({x},{z}) {}: world {height_world}, local {local}",
                f[3]
            );
        }
        n += 1;
    }
    assert_eq!(n, 151);
}

#[test]
fn standing_still_on_every_playground_block_never_slides() {
    let height = playground();
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (0, 0, 31, 31), &height);
    // One player on the centre of every block's top (they stand 1 m apart: no touching).
    let mut players = Vec::new();
    for z in 0..32 {
        for x in 0..32 {
            if height(x, z) == 0 {
                continue;
            }
            let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
            let feet = surface(&height, fx, fz) + 0.02;
            players.push((x, z, w.spawn(Vec3::new(fx, feet, fz))));
        }
    }
    assert!(players.len() > 240, "{}", players.len());
    w.step(ticks(1.0));
    let start: Vec<Vec3> = players.iter().map(|&(_, _, e)| w.pos(e)).collect();
    w.step(ticks(2.0));
    let mut moved = Vec::new();
    for (&(x, z, e), s) in players.iter().zip(&start) {
        let d = w.pos(e) - *s;
        if !w.c(e).ground.grounded || d.length() > 0.01 {
            moved.push(format!("({x},{z}) moved {d} grounded {}", w.c(e).ground.grounded));
        }
    }
    assert!(moved.is_empty(), "{} slid: {moved:?}", moved.len());
}

/// Walkers along every row (`along_x`, +x) or every column (+z) of the playground, from just
/// outside the grid, 1 m apart; returns what each saw.
fn walk_the_playground(along_x: bool) -> Vec<String> {
    let height = playground();
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (0, 0, 31, 31), &height);
    let yaw = if along_x { 90.0 } else { 0.0 };
    let walkers: Vec<u32> = (1..31)
        .map(|k| {
            let c = k as f32 + 0.5;
            let start = if along_x { Vec3::new(-1.5, 0.02, c) } else { Vec3::new(c, 0.02, -1.5) };
            w.spawn_yaw(start, yaw)
        })
        .collect();
    w.set_input(move |_, _| input(0.0, 1.0, false, false, false, yaw, 0.0));
    let mut seen: Vec<Walk> = walkers.iter().map(|_| Walk::default()).collect();
    for _ in 0..ticks(8.0) {
        w.step(1);
        for (k, &e) in walkers.iter().enumerate() {
            watch(&w, e, &height, &mut seen[k]);
        }
    }
    let walk = w.config.movement.walk_speed;
    let mut bad = Vec::new();
    for (k, &e) in walkers.iter().enumerate() {
        let s = &seen[k];
        // Up a 45° ramp at walking speed the body rises at most at that speed.
        if s.rising_airborne > 3 || s.max_up > walk + 0.5 || w.count(e, events::JUMPED) > 0 {
            bad.push(format!("{} {}: {s:?}", if along_x { "row" } else { "column" }, k + 1));
        }
    }
    bad
}

#[test]
fn walking_along_every_playground_row_never_launches() {
    let bad = walk_the_playground(true);
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn walking_along_every_playground_column_never_launches() {
    let bad = walk_the_playground(false);
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_one_block_terrace_step_is_walked_up_and_down_without_jumping() {
    // Two levels of grass: one block deep for z < 4, two from z = 4 — the step is one even slope
    // across both blocks (ADAPTIVE_BLOCKS.md §1), so no jump is needed.
    let height = |_: i32, z: i32| if z < 4 { 1 } else { 2 };
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-8, -8, 8, 16), &height);
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    assert!(approx(w.feet(e), 0.5, 0.01), "rests at half height: {}", w.feet(e));
    w.set_input(|_, _| mv(0.0, 1.0));
    let mut walk = Walk::default();
    for _ in 0..ticks(2.0) {
        w.step(1);
        watch(&w, e, &height, &mut walk);
    }
    assert!(w.pos(e).z > 7.0, "{}", w.pos(e));
    assert!(approx(w.feet(e), 1.5, 0.01), "on the upper level: {}", w.feet(e));
    assert_eq!(w.count(e, events::JUMPED), 0);
    assert_eq!(walk.rising_airborne, 0, "{walk:?}");
    // And back down.
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
    assert!(airborne <= 2, "walks down the slope, not off a ledge: {airborne}");
}

#[test]
fn a_one_wide_ridge_is_walked_over_without_a_launch() {
    // A row of blocks one higher than the field: a half-height ridge between two 27° slopes.
    let height = |_: i32, z: i32| if z == 4 { 2 } else { 1 };
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-8, -8, 8, 16), &height);
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    w.set_input(|_, _| input(0.0, 1.0, true, false, false, 0.0, 0.0));
    let mut walk = Walk::default();
    for _ in 0..ticks(2.0) {
        w.step(1);
        watch(&w, e, &height, &mut walk);
    }
    assert!(w.pos(e).z > 8.0, "{}", w.pos(e));
    // (The capsule's round bottom on a 45° flank or the crest sits up to its radius, 0.3 m, above
    // the surface under its centre: more is a hop.)
    assert!(walk.max_gap < 0.4, "never far above the ground: {walk:?}");
    assert!(walk.rising_airborne <= 3, "{walk:?}");
    assert!(approx(w.feet(e), 0.5, 0.01), "{}", w.feet(e));
}

#[test]
fn single_block_stairs_are_a_run_of_45_degree_ramps_walked_without_jumping() {
    // Situation P: one-wide stairs 1, 2, 3, 4, 5 high along +x; each is a wedge up to the next.
    let height = |x: i32, z: i32| if z == 0 && (0..5).contains(&x) { x + 1 } else { 0 };
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-4, -4, 8, 4), &height);
    let e = w.spawn_yaw(Vec3::new(-2.5, 0.02, 0.5), 90.0);
    w.set_input(|_, _| input(0.0, 1.0, false, false, false, 90.0, 0.0));
    let mut walk = Walk::default();
    for _ in 0..ticks(1.6) {
        w.step(1);
        watch(&w, e, &height, &mut walk);
    }
    assert_eq!(w.count(e, events::JUMPED), 0);
    assert!(w.feet(e) > 4.0, "climbed the ramps: {} at {}", w.feet(e), w.pos(e));
    assert!(walk.rising_airborne <= 3, "{walk:?}");
}

#[test]
fn walking_a_grass_field_across_chunk_seams_never_loses_the_ground() {
    // Shaped cells collide as one triangle mesh per chunk: crossing between meshes must not catch.
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
        if std::env::var("DWELL_DEBUG").is_ok() && (w.feet(e) - 0.5).abs() > 0.002 {
            eprintln!("tick {} at {} feet {}", w.tick, w.pos(e), w.feet(e));
        }
    }
    assert_eq!(airborne, 0);
    assert!(max_lift < 0.003, "sub-millimetre ripples at most: {max_lift}");
}

#[test]
fn a_block_placed_beside_a_standing_player_reshapes_the_ground_without_a_launch() {
    // The block under the player rises toward a new higher neighbour: corners lift by up to half a
    // block under the feet. The player is carried up, not thrown.
    let height = |_: i32, _: i32| 1;
    let mut w = PlayerTestWorld::new();
    terrain(&mut w, (-8, -8, 8, 8), &height);
    let e = w.spawn(Vec3::new(0.5, 0.52, 0.5));
    w.step(ticks(0.5));
    w.set(1, 1, 0, grass());
    let mut walk = Walk::default();
    for _ in 0..ticks(1.0) {
        w.step(1);
        let v = w.vel(e);
        walk.max_up = walk.max_up.max(v.y);
        if !w.c(e).ground.grounded && v.y > 0.5 {
            walk.rising_airborne += 1;
        }
    }
    assert!(walk.max_up < 3.0 && walk.rising_airborne <= 3, "{walk:?}");
    assert!(w.c(e).ground.grounded);
    let p = w.pos(e);
    let ground = surface(&|x, z| if (x, z) == (1, 0) { 2 } else { 1 }, p.x, p.z);
    // (On the 27° slope the capsule's round bottom sits ~4 cm above the surface under its centre.)
    assert!((w.feet(e) - ground).abs() < 0.05, "on the new surface: {} vs {ground}", w.feet(e));
}

#[test]
fn every_slope_the_adaptive_table_makes_is_walkable() {
    // The steepest faces are the saddle's and the dimple's: from their crest (a raised diagonal)
    // down to a floor corner, arctan(√2) ≈ 54.74° — the "tetrahedral corner" Phase 9 rejected for
    // placed slopes, which the owner's rules make. They must be ground the controller stands on.
    let mut steepest = 0.0f32;
    for &i in dwell_blocks::ADAPTIVE_UP.iter() {
        let s = &dwell_blocks::SHAPES[usize::from(i)];
        for f in dwell_blocks::shape::faces_of(s) {
            if f.tag != dwell_blocks::shape::SURFACE_TAG {
                continue;
            }
            let (a, b, c) = (Vec3::from(f.v[0]), Vec3::from(f.v[1]), Vec3::from(f.v[2]));
            let n = (b - a).cross(c - a).normalize();
            steepest = steepest.max(n.y.clamp(-1.0, 1.0).acos().to_degrees());
        }
    }
    assert!((steepest - 54.7356).abs() < 0.01, "{steepest}");
    let limit = dwell_physics::config::PlayerControllerConfig::default()
        .probes
        .max_slope_angle;
    assert!(steepest < limit - 1.0, "{steepest}° against a limit of {limit}°");
}
