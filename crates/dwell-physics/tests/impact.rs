//! Impact damage (PLAYER_CONTROLLER.md §6.10, ARCHITECTURE.md §9.6): a hit hurts by the downward
//! speed × the surface's cos θ, face-local `max(0, −v.y) · n.y` (owner, 2026-11-06;
//! `impact_measure`), with `v` the
//! body's velocity going into the physics step that met the surface — on the ground only
//! (`impact_surfaces`): a walkable normal, floors and ramps; walls, steep faces and ceilings never
//! hurt. Once per
//! contact, the largest of a tick's. Flight, swimming and climbing are
//! exempt, as falls were. Runs in the local frame like every suite (far from the origin and on face
//! B too).

mod common;

use common::*;
use dwell_blocks::StateId;
use dwell_blocks::require;
use dwell_physics::Vec3;
use dwell_physics::config::{ImpactMeasure, ImpactSurfaces, PlayerControllerConfig};
use dwell_physics::controller::{Input, PlayerHandle, events};
use dwell_world::ramps::{gentle_cell, steep_cells};
use dwell_world::rows_for_m;
use dwell_worldgen::ramps::{Around, Top, top_of};

/// The damage this tick's controller events deal (what the server applies).
fn hurt(w: &PlayerTestWorld, h: PlayerHandle) -> i32 {
    let c = w.c(h);
    if c.events & events::IMPACTED == 0 {
        return 0;
    }
    w.config.impact_damage(c.impact_speed)
}

/// The damage dealt over `n` ticks, tick by tick (the ticks that hurt).
fn hits(w: &mut PlayerTestWorld, h: PlayerHandle, n: i32) -> Vec<i32> {
    hits_at(w, h, n).into_iter().map(|(d, _)| d).collect()
}

/// [`hits`] with each hit's surface normal (face-local).
fn hits_at(w: &mut PlayerTestWorld, h: PlayerHandle, n: i32) -> Vec<(i32, Vec3)> {
    let mut out = Vec::new();
    for _ in 0..n {
        w.step(1);
        let d = hurt(w, h);
        if d > 0 {
            out.push((d, w.c(h).impact_normal));
        }
    }
    out
}

/// The fastest impact over `n` ticks, hurting or not.
fn fastest_impact(w: &mut PlayerTestWorld, h: PlayerHandle, n: i32) -> f32 {
    let mut best = 0.0f32;
    for _ in 0..n {
        w.step(1);
        if w.c(h).events & events::IMPACTED != 0 {
            best = best.max(w.c(h).impact_speed);
        }
    }
    best
}

// --- floors -------------------------------------------------------------------------------------

#[test]
fn a_perpendicular_landing_on_flat_ground_hits_once_and_a_22_m_fall_kills() {
    // 22 m of free fall at 20 m/s²: ~29.7 m/s, one hit on the floor's normal, past the lethal 20 m
    // (0.49.21; a 20 m fall killed under the 12 m lethal height before, and the linear rule's 131
    // points before 0.49.18).
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 22.0, 0.5));
    let dealt = hits_at(&mut w, e, ticks(2.5));
    assert_eq!(dealt.len(), 1, "one hit: {dealt:?}");
    assert!(
        dealt[0].0 >= dwell_protocol::players::MAX_HEALTH as i32,
        "lethal: {}",
        dealt[0].0
    );
    assert_eq!(w.count(e, events::LANDED), 1);
    let n = dealt[0].1;
    assert!(
        approx(n.y, 1.0, 1e-4),
        "the floor's normal, face-local: {n}"
    );
}

#[test]
fn drops_up_to_10_m_never_hurt_and_damage_climbs_steeply_to_lethal_at_20_m() {
    // Owner (2026-11-06): "I don't want to get nickeled and dimed all the time for small drops",
    // then "Scale it so approximately safe is 10m and lethal is 20m". The thresholds are landing
    // speeds written as the drop that reaches them (`v² ÷ 2g`): safe 10 m is 20 m/s, lethal 20 m is
    // 28.3 m/s; horizontal speed never counts. Until 0.49.21 safe was 5 m and lethal 12 m (8 m took
    // 18 points, 12 m killed); before 0.49.18 it hurt from 3.6 m.
    for (height, least, most) in [
        (5.0, 0, 0),
        (8.0, 0, 0),
        (9.9, 0, 0),
        (12.0, 2, 8),
        (15.0, 22, 28),
        (18.0, 60, 68),
        (20.2, 100, 110),
    ] {
        let mut w = PlayerTestWorld::new();
        w.floor();
        let e = w.spawn(Vec3::new(0.5, height, 0.5));
        let dealt: i32 = hits(&mut w, e, ticks(3.0)).iter().sum();
        assert!(
            (least..=most).contains(&dealt),
            "{height} m: {dealt} points (expected {least}–{most})"
        );
    }
}

#[test]
fn the_damage_curve_is_data() {
    // The curve's three numbers are the config's: a straight line (exponent 1) from 2 m to 4 m.
    let mut config = PlayerControllerConfig::default();
    config.damage.impact_safe_height = 2.0;
    config.damage.impact_lethal_height = 4.0;
    config.damage.impact_curve_exponent = 1.0;
    let at = |h: f32| config.impact_damage((2.0 * config.body.gravity * h).sqrt());
    assert_eq!((at(1.9), at(3.0), at(4.0)), (0, 50, 100));
    // Just past the safe height a hit always takes a point.
    assert_eq!(at(2.001), 1);
}

// --- ramps --------------------------------------------------------------------------------------

/// A hillside of ½ m ramps rising toward +z (one row per metre): 26.57°, its normal leaning to −z.
fn hillside(w: &mut PlayerTestWorld) -> fn(i32, i32) -> i32 {
    let height: fn(i32, i32) -> i32 = |_, z| z.clamp(0, 60) + 2;
    let grass: StateId = require("dwell:grass");
    w.fill(-6, -1, -8, 5, -1, 64, cube_block());
    for z in -6..=62 {
        for x in -4..=3 {
            let around: Around = std::array::from_fn(|r| {
                std::array::from_fn(|c| height(x + c as i32 - 1, z + r as i32 - 1))
            });
            let h = height(x, z);
            for row in 0..h {
                w.set_cell(x, row, z, grass);
            }
            if let Top::Gentle(f) = top_of(&around, rows_for_m(1.0)) {
                w.set_cell(
                    x,
                    h - 1,
                    z,
                    gentle_cell("dwell:grass", f, false).expect("ramp"),
                );
            }
        }
    }
    height
}

/// What a launch onto the hillside dealt.
struct Landing {
    dealt: Vec<i32>,
    /// The fastest impact raised (m/s).
    fastest: f32,
    /// The body's velocity the tick before it landed.
    before: Vec3,
}

/// Spawns 0.6 m over the hillside at z = 40 and launches the player at `speed` m/s, `angle`° below
/// the horizontal, heading downhill (−z).
fn launch_onto_hillside(speed: f32, angle: f32) -> Landing {
    let mut w = PlayerTestWorld::new();
    let height = hillside(&mut w);
    let ground = height(0, 40) as f32 * 0.5;
    let e = w.spawn(Vec3::new(0.5, ground + 0.6, 40.5));
    let a = angle.to_radians();
    w.kick(0, e, Vec3::new(0.0, -speed * a.sin(), -speed * a.cos()));
    let mut out = Landing {
        dealt: Vec::new(),
        fastest: 0.0,
        before: Vec3::ZERO,
    };
    for _ in 0..ticks(1.0) {
        let before = w.vel(e);
        w.step(1);
        let d = hurt(&w, e);
        if d > 0 {
            out.dealt.push(d);
        }
        if w.c(e).events & events::IMPACTED != 0 {
            out.fastest = out.fastest.max(w.c(e).impact_speed);
        }
        if w.count(e, events::LANDED) > 0 {
            out.before = before;
            // And nothing more while it settles on the slope.
            out.dealt.extend(hits(&mut w, e, 10));
            break;
        }
    }
    assert_eq!(w.count(e, events::LANDED), 1, "lands on the hillside");
    out
}

#[test]
fn landing_on_a_ramp_hurts_by_its_downward_speed_into_it() {
    // Owner (2026-11-06): "fall damage should only be applied based on Y (vertical downward
    // velocity)", and "the angle of the downward hit reduces fall damage too, so landing straight
    // down on a steep ramp would mitigate some damage": the downward speed × the surface's cos θ
    // (face-local n.y). On a ½ m ramp (26.57°) × 0.894, whatever the angle the body comes in at;
    // its horizontal speed never counts (until 0.49.1 the speed into the normal counted it: 60°
    // below the horizontal dealt 21, along the slope 0; in 0.49.2 ramps dealt nothing).
    let cos = Vec3::new(0.0, 2.0, -1.0).normalize().y;
    // (35 m/s since 0.49.21's 10 m safe height: 25 m/s straight down was under it on the ramp.)
    for angle in [90.0, 60.0, 26.565] {
        let l = launch_onto_hillside(35.0, angle);
        let down = -l.before.y * cos;
        assert!(
            (l.fastest - down).abs() < 0.05,
            "{angle}°: {} vs {down} m/s down into the ramp",
            l.fastest
        );
        let damage = PlayerControllerConfig::default().impact_damage(down);
        // Along the slope the downward speed is under the safe height: nothing.
        let expected = if damage > 0 { vec![damage] } else { vec![] };
        assert!(
            (angle < 30.0 || damage > 0) && l.dealt == expected,
            "{angle}°: {:?} vs {damage}",
            l.dealt
        );
    }
}

/// A hillside of 1 m ramps (45°) rising toward +z, two rows per metre.
fn steep_hillside(w: &mut PlayerTestWorld) -> f32 {
    let height = |_: i32, z: i32| 2 * z.clamp(0, 40) + 2;
    let grass: StateId = require("dwell:grass");
    w.fill(-6, -1, -8, 5, -1, 44, cube_block());
    for z in -6..=42 {
        for x in -4..=3 {
            let around: Around = std::array::from_fn(|r| {
                std::array::from_fn(|c| height(x + c as i32 - 1, z + r as i32 - 1))
            });
            let h = height(x, z);
            for row in 0..h {
                w.set_cell(x, row, z, grass);
            }
            if let Top::Steep(f) = top_of(&around, rows_for_m(1.0)) {
                let cells = steep_cells("dwell:grass", f, false).expect("steep ramp");
                for (k, &c) in cells.iter().enumerate() {
                    w.set_cell(x, h - cells.len() as i32 + k as i32, z, c);
                }
            }
        }
    }
    height(0, 20) as f32 * 0.5
}

#[test]
fn landing_on_a_steep_ramp_hurts_less_than_on_the_flat() {
    // 35 m/s straight down onto a 45° ramp: the downward speed × cos 45°, so less than the same fall
    // onto the flat (owner, 2026-11-06: "landing straight down on a steep ramp would mitigate some
    // damage"; 0.49.2: nothing).
    let mut w = PlayerTestWorld::new();
    let ground = steep_hillside(&mut w);
    let e = w.spawn(Vec3::new(0.5, ground + 0.8, 20.5));
    w.kick(0, e, Vec3::new(0.0, -35.0, 0.0));
    let mut before = Vec3::ZERO;
    let mut fastest = 0.0f32;
    for _ in 0..ticks(1.0) {
        let v = w.vel(e);
        w.step(1);
        if w.c(e).events & events::IMPACTED != 0 && w.c(e).impact_speed > fastest {
            fastest = w.c(e).impact_speed;
            before = v;
        }
    }
    assert!(w.count(e, events::LANDED) >= 1, "landed on the slope");
    let down = -before.y;
    let into = down * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (fastest - into).abs() < 0.05,
        "{fastest} vs {into} ({down} down)"
    );
    let damage = &w.config;
    assert!(
        damage.impact_damage(fastest) > 0
            && damage.impact_damage(fastest) < damage.impact_damage(down),
        "{} on the ramp vs {} on the flat",
        damage.impact_damage(fastest),
        damage.impact_damage(down)
    );
}

#[test]
fn a_fast_horizontal_hit_into_a_slope_does_not_hurt() {
    // The owner's report (2026-11-05): hitting "a half slope or steep slope really fast" dealt "up
    // to lethal fall damage": the speed into the slope's normal counted the horizontal speed. Only
    // the downward speed counts: launched along the ground at 25 m/s into a 45° slope, ~0.
    let mut w = PlayerTestWorld::new();
    steep_hillside(&mut w); // 1 m of ground for z < 0, the slope rising from z = 0
    let e = w.spawn(Vec3::new(0.5, 1.05, -4.0));
    w.kick(0, e, Vec3::new(0.0, 0.0, 25.0));
    let fastest = fastest_impact(&mut w, e, ticks(1.0));
    assert!(w.pos(e).z > -0.5, "reached the slope: {}", w.pos(e));
    assert_eq!(w.config.impact_damage(fastest), 0, "{fastest} m/s counted");
}

// --- walls --------------------------------------------------------------------------------------

/// A floor and a wall across the path at z = 6 (its face at z = 6), 10 m tall.
fn wall_ahead() -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 32);
    w.fill(-8, 0, 6, 8, 9, 6, cube_block());
    w
}

#[test]
fn launched_into_a_wall_at_25_m_s_does_not_hurt() {
    // Only the ground hurts (owner, 2026-10-21): the wall took 104 when every surface did. The
    // landing after it, from 3 m, is under the threshold.
    let mut w = wall_ahead();
    let e = w.spawn(Vec3::new(0.5, 3.0, 0.5));
    w.kick(0, e, Vec3::new(0.0, 0.0, 25.0));
    let dealt = hits_at(&mut w, e, ticks(1.5));
    assert_eq!(dealt, Vec::new());
    assert!(
        w.pos(e).z > 5.0 && w.pos(e).z < 6.0,
        "met the wall: {}",
        w.pos(e)
    );
}

#[test]
fn walls_and_ceilings_hurt_again_when_the_config_says_so() {
    // The rule is data: `impact_surfaces: All` with `impact_measure: IntoSurface` counts every
    // contact by the speed into it, as before.
    let mut cfg = PlayerControllerConfig::default();
    cfg.damage.impact_surfaces = ImpactSurfaces::All;
    cfg.damage.impact_measure = ImpactMeasure::IntoSurface;
    let mut w = PlayerTestWorld::with_config(cfg);
    w.floor_at(0, 32);
    w.fill(-8, 0, 6, 8, 9, 6, cube_block());
    let e = w.spawn(Vec3::new(0.5, 3.0, 0.5));
    w.kick(0, e, Vec3::new(0.0, 0.0, 30.0));
    let dealt = hits_at(&mut w, e, ticks(1.5));
    assert_eq!(dealt.len(), 1, "{dealt:?}");
    assert!(dealt[0].0 >= 100, "{dealt:?}");
    let n = dealt[0].1;
    assert!(approx(n.z, -1.0, 1e-3), "the wall's normal: {n}");
}

#[test]
fn brushing_along_a_wall_at_25_m_s_does_not_hurt() {
    // A wall along the path at x = 2 (its face at x = 2): launched along it, a little into it.
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 32);
    w.fill(2, 0, -8, 2, 9, 30, cube_block());
    let e = w.spawn(Vec3::new(1.65, 3.0, -6.0));
    w.kick(0, e, Vec3::new(1.0, 0.0, 25.0));
    let fastest = fastest_impact(&mut w, e, ticks(0.5));
    assert!(fastest < 2.0, "{fastest}");
    assert!(w.pos(e).z > 2.0, "slid along it: {}", w.pos(e));
}

#[test]
fn flying_into_a_wall_does_not_hurt() {
    // Creative flight is exempt, as it always was from falls (it raises no landing).
    let mut w = wall_ahead();
    let e = w.spawn(Vec3::new(0.5, 3.0, -20.0));
    w.set_input(|_, _| Input {
        fly: true,
        run: true,
        fly_speed: 2,
        move_y: 1.0,
        ..Input::default()
    });
    let mut fastest_flight = 0.0f32;
    let mut dealt = Vec::new();
    for _ in 0..ticks(2.0) {
        w.step(1);
        fastest_flight = fastest_flight.max(w.vel(e).z);
        let d = hurt(&w, e);
        if d > 0 {
            dealt.push(d);
        }
    }
    assert!(fastest_flight > 20.0, "{fastest_flight}");
    assert!(
        w.pos(e).z < 6.0 && w.pos(e).z > 5.0,
        "at the wall: {}",
        w.pos(e)
    );
    assert_eq!(dealt, Vec::<i32>::new());
}

// --- ceilings -----------------------------------------------------------------------------------

#[test]
fn a_hard_ceiling_hit_does_not_hurt() {
    // A ceiling 4 m up; launched up at 25 m/s from the floor: ~23 m/s at the ceiling, which took
    // ~88 when every surface hurt. Only the ground does now; the fall back is under 4 m.
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-8, 4, -8, 8, 4, 8, cube_block());
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.step(5);
    w.kick(w.tick, e, Vec3::new(0.0, 25.0, 0.0));
    let mut top = 0.0f32;
    let mut dealt = Vec::new();
    for _ in 0..ticks(1.5) {
        w.step(1);
        top = top.max(w.feet(e));
        let d = hurt(&w, e);
        if d > 0 {
            dealt.push(d);
        }
    }
    assert!(top > 1.5, "reached the ceiling: {top}");
    assert_eq!(dealt, Vec::<i32>::new());
}

#[test]
fn a_jump_into_a_low_ceiling_does_not_hurt() {
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-8, 2, -8, 8, 2, 8, cube_block());
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.set_input(|t, _| Input {
        jump: t == 5,
        ..Input::default()
    });
    assert_eq!(hits(&mut w, e, ticks(1.0)), Vec::<i32>::new());
    assert_eq!(w.count(e, events::JUMPED), 1);
}

// --- gentle contacts ----------------------------------------------------------------------------

#[test]
fn walking_or_sprinting_into_a_wall_never_hurts() {
    for run in [false, true] {
        let mut w = wall_ahead();
        let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
        w.set_input(move |_, _| input(0.0, 1.0, run, false, false, 0.0, 0.0));
        let fastest = fastest_impact(&mut w, e, ticks(2.0));
        assert!(w.pos(e).z > 5.0, "at the wall: {}", w.pos(e));
        assert!(fastest < 9.0, "run {run}: {fastest}");
    }
}

#[test]
fn stepping_up_half_a_metre_and_auto_climbing_never_hurt() {
    // A ½ m step, walked and sprinted onto; then auto-jump onto a 1 m block.
    for run in [false, true] {
        let mut w = PlayerTestWorld::new();
        w.floor();
        for z in 3..=13 {
            for x in -3..=3 {
                w.set_cell(x, 0, z, cube_block());
            }
        }
        let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
        w.set_input(move |_, _| input(0.0, 1.0, run, false, false, 0.0, 0.0));
        let fastest = fastest_impact(&mut w, e, ticks(1.0));
        assert!(
            approx(w.feet(e), 0.5, 0.01),
            "on the step: {} at {}",
            w.feet(e),
            w.pos(e)
        );
        assert!(fastest < 9.0, "run {run}: {fastest}");
    }
    let mut w = PlayerTestWorld::with_config(PlayerControllerConfig::touch());
    w.floor();
    w.fill(-3, 0, 3, 3, 0, 10, cube_block());
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.set_input(|_, _| mv(0.0, 1.0));
    let fastest = fastest_impact(&mut w, e, ticks(2.0));
    assert!(approx(w.feet(e), 1.0, 0.01), "climbed: {}", w.feet(e));
    assert!(fastest < 9.0, "auto-climb: {fastest}");
}

#[test]
fn walking_up_and_down_a_ramp_hillside_never_hurts() {
    let mut w = PlayerTestWorld::new();
    let height = hillside(&mut w);
    let e = w.spawn(Vec3::new(0.5, height(0, 10) as f32 * 0.5 + 0.02, 10.5));
    w.step(10);
    w.set_input(|t, _| {
        input(
            0.0,
            if t < 150 { 1.0 } else { -1.0 },
            true,
            false,
            false,
            0.0,
            0.0,
        )
    });
    let fastest = fastest_impact(&mut w, e, ticks(5.0));
    assert!(fastest < 9.0, "{fastest}");
}

// --- steps --------------------------------------------------------------------------------------

/// A floor and an obstacle `rows` ½ m cells tall across the path, its face at z = 3.
fn obstacle_ahead(rows: i32) -> PlayerTestWorld {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 32);
    for z in 3..=13 {
        for x in -8..=8 {
            for row in 0..rows {
                w.set_cell(x, row, z, cube_block());
            }
        }
    }
    w
}

/// What a launch at `speed` m/s (+z, toward the obstacle) from feet `height` m over the floor dealt.
struct Run {
    dealt: Vec<(i32, Vec3)>,
    /// The fastest impact raised (m/s), hurting or not.
    fastest: f32,
}

fn launch_into(rows: i32, speed: f32, height: f32) -> Run {
    let mut w = obstacle_ahead(rows);
    let e = w.spawn(Vec3::new(0.5, height, 2.0));
    w.kick(0, e, Vec3::new(0.0, 0.0, speed));
    let mut out = Run {
        dealt: Vec::new(),
        fastest: 0.0,
    };
    for _ in 0..ticks(1.0) {
        w.step(1);
        let d = hurt(&w, e);
        if d > 0 {
            out.dealt.push((d, w.c(e).impact_normal));
        }
        if w.c(e).events & events::IMPACTED != 0 {
            out.fastest = out.fastest.max(w.c(e).impact_speed);
        }
    }
    out
}

/// Launched off the floor (feet 0.2 m up) into a ½ m step's side: its top 0.3 m over the feet, in
/// step-up reach. (Running along the ground the step-up takes it, at any speed: no hit at all.)
const HOP: f32 = 0.2;

#[test]
fn launches_into_a_half_metre_step_or_a_metre_tall_obstacle_do_not_hurt() {
    // A step's side is a wall: no damage at any speed (a ½ m step hurt as a ramp, and a 1 m block
    // as a wall, ≥ 100 at 25 m/s, before only the ground hurt).
    for (rows, speed, height) in [
        (1, 14.0, HOP),
        (1, 25.0, HOP),
        (1, 40.0, HOP),
        (2, 25.0, 0.0),
        (2, 25.0, HOP),
    ] {
        let r = launch_into(rows, speed, height);
        assert_eq!(
            r.dealt,
            Vec::new(),
            "{rows} rows at {speed} m/s, {height} m up"
        );
    }
}

#[test]
fn hitting_a_step_edge_from_lower_in_the_air_does_not_hurt() {
    // Lower over the floor the capsule's base meets the step's top edge (a normal partly up), not
    // its side: ground only by its own normal where walkable, a glancing share of the speed.
    let r = launch_into(1, 25.0, 0.3);
    assert_eq!(r.dealt, Vec::new(), "fastest {}", r.fastest);
    assert!(r.fastest < 6.0, "{}", r.fastest);
}

#[test]
fn running_into_a_half_metre_step_at_any_speed_steps_up() {
    // On the ground the step-up takes it (no hit), as before.
    for speed in [5.0, 8.0, 14.0, 25.0] {
        let r = launch_into(1, speed, 0.0);
        assert_eq!(r.fastest, 0.0, "{speed}");
    }
}
