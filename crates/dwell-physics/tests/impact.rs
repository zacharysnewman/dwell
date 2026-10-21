//! Impact damage (PLAYER_CONTROLLER.md §6.10, ARCHITECTURE.md §9.6): a hit hurts by the speed into
//! the surface, `max(0, −v · n)`, with `v` the body's velocity going into the physics step that
//! met it and `n` the surface's normal (face-local) — floors, ramps, walls and ceilings alike, over
//! the same thresholds; once per contact, the largest of a tick's. Flight, swimming and climbing are
//! exempt, as falls were. Runs in the local frame like every suite (far from the origin and on face
//! B too).

mod common;

use common::*;
use dwell_blocks::StateId;
use dwell_blocks::require;
use dwell_physics::Vec3;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerHandle, events};
use dwell_world::ramps::gentle_cell;
use dwell_world::rows_for_m;
use dwell_worldgen::ramps::{Around, Top, top_of};

/// The damage this tick's controller events deal (what the server applies).
fn hurt(w: &PlayerTestWorld, h: PlayerHandle) -> i32 {
    let c = w.c(h);
    if c.events & events::IMPACTED == 0 {
        return 0;
    }
    w.config.damage.impact_damage(c.impact_speed)
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
fn a_perpendicular_landing_on_flat_ground_damages_as_before() {
    // 20 m of free fall at 20 m/s²: ~28.3 m/s, (28.3 − 12) × 8 ≈ 131 points, exactly what the
    // vertical landing speed gave before impacts (pinned from that code).
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 20.0, 0.5));
    let dealt = hits_at(&mut w, e, ticks(2.5));
    assert_eq!(dealt.len(), 1, "one hit: {dealt:?}");
    assert_eq!(dealt[0].0, 131, "as before");
    assert_eq!(w.count(e, events::LANDED), 1);
    let n = dealt[0].1;
    assert!(
        approx(n.y, 1.0, 1e-4),
        "the floor's normal, face-local: {n}"
    );
}

#[test]
fn a_short_drop_does_not_hurt() {
    // 3 m: ~11 m/s, under the 12 m/s threshold.
    let mut w = PlayerTestWorld::new();
    w.floor();
    let e = w.spawn(Vec3::new(0.5, 3.0, 0.5));
    assert_eq!(hits(&mut w, e, ticks(1.5)), Vec::<i32>::new());
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

/// The ½ m ramps' normal (face-local): up, leaning downhill (−z).
fn ramp_normal() -> Vec3 {
    Vec3::new(0.0, 2.0, -1.0).normalize()
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
fn a_vertical_landing_on_a_ramp_counts_the_ramps_normal() {
    // 25 m/s straight down onto 26.57°: × cos 26.57° ≈ 0.894 of it into the ramp, so less than the
    // same landing on the flat.
    let l = launch_onto_hillside(25.0, 90.0);
    assert_eq!(l.dealt.len(), 1, "{:?}", l.dealt);
    let into = -l.before.dot(ramp_normal());
    let flat = PlayerControllerConfig::default()
        .damage
        .impact_damage(l.before.length());
    assert!(
        l.dealt[0] > 0 && l.dealt[0] < flat,
        "{:?} vs {flat}",
        l.dealt
    );
    assert!((l.fastest - into).abs() < 0.05, "{} vs {into}", l.fastest);
}

#[test]
fn a_glancing_landing_on_a_ramp_hurts_less_and_a_skim_not_at_all() {
    let vertical = launch_onto_hillside(25.0, 90.0);
    // 60° below the horizontal, downhill: ~33° to the ramp's surface.
    let glancing = launch_onto_hillside(25.0, 60.0);
    assert!(
        !glancing.dealt.is_empty()
            && glancing.dealt.iter().sum::<i32>() < vertical.dealt.iter().sum::<i32>(),
        "{:?} vs {:?}",
        glancing.dealt,
        vertical.dealt
    );
    // Along the ramp's own slope: a skim, only gravity's few m/s into it.
    let skim = launch_onto_hillside(25.0, 26.565);
    assert_eq!(
        skim.dealt,
        Vec::<i32>::new(),
        "{} m/s into the ramp",
        skim.fastest
    );
    assert!(skim.fastest < 6.0, "{}", skim.fastest);
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
fn launched_into_a_wall_at_25_m_s_hurts_once() {
    let mut w = wall_ahead();
    let e = w.spawn(Vec3::new(0.5, 3.0, 0.5));
    w.kick(0, e, Vec3::new(0.0, 0.0, 25.0));
    let dealt = hits_at(&mut w, e, ticks(1.5));
    // (25 − 12) × 8 = 104, less the little the fall turned the velocity: one hit, not one a tick
    // while the launch keeps pressing the body into the wall.
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
fn a_hard_ceiling_hit_hurts() {
    // A ceiling 4 m up; launched up at 25 m/s from the floor: ~23 m/s at the ceiling.
    let mut w = PlayerTestWorld::new();
    w.floor();
    w.fill(-8, 4, -8, 8, 4, 8, cube_block());
    let e = w.spawn(Vec3::new(0.5, 0.0, 0.5));
    w.step(5);
    w.kick(w.tick, e, Vec3::new(0.0, 25.0, 0.0));
    let mut dealt = Vec::new();
    let mut normal = Vec3::ZERO;
    for _ in 0..ticks(0.5) {
        w.step(1);
        let d = hurt(&w, e);
        if d > 0 {
            dealt.push(d);
            normal = w.c(e).impact_normal;
        }
    }
    assert_eq!(dealt.len(), 1, "{dealt:?}");
    assert!(dealt[0] > 60, "{dealt:?}");
    assert!(
        approx(normal.y, -1.0, 1e-3),
        "the ceiling's normal: {normal}"
    );
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

/// The ramp a ½ m step counts as: ½ m over one cell's run, facing the mover (−z).
fn step_ramp_normal() -> Vec3 {
    Vec3::new(0.0, 1.0, -0.5).normalize()
}

/// Launched off the floor (feet 0.2 m up) into a ½ m step's side: its top 0.3 m over the feet, in
/// step-up reach. (Running along the ground the step-up takes it, at any speed: no hit at all.)
const HOP: f32 = 0.2;

#[test]
fn a_14_m_s_slide_into_a_half_metre_step_does_not_hurt() {
    // The sprint slide's top speed into the side of one ½ m step, off the ground: a ramp's share,
    // ~6.6 m/s, not a wall's 14.7 (which hurt: 21 points).
    let r = launch_into(1, 14.0, HOP);
    assert_eq!(r.dealt, Vec::new(), "fastest {}", r.fastest);
    assert!(r.fastest < 8.0, "{}", r.fastest);
}

#[test]
fn a_25_m_s_launch_into_a_half_metre_step_hurts_as_a_ramp() {
    // Into the ramp's normal: 25 × sin 26.57° ≈ 11.2 m/s (and the little it fell), under the
    // 12 m/s threshold, where the 1 m wall below takes 104.
    let r = launch_into(1, 25.0, HOP);
    let ramp = -Vec3::new(0.0, 0.0, 25.0).dot(step_ramp_normal());
    assert!(
        r.fastest > ramp - 0.5 && r.fastest < ramp + 1.0,
        "{} vs {ramp}",
        r.fastest
    );
    let worst = r.dealt.iter().map(|d| d.0).max().unwrap_or(0);
    assert!(worst < 10, "{:?}", r.dealt);
    // Faster still, it hurts, by the ramp's share of the speed.
    let r = launch_into(1, 40.0, HOP);
    let ramp = -Vec3::new(0.0, 0.0, 40.0).dot(step_ramp_normal());
    let expected = PlayerControllerConfig::default().damage.impact_damage(ramp);
    assert_eq!(r.dealt.len(), 1, "{:?}", r.dealt);
    let (dealt, n) = r.dealt[0];
    assert!(
        dealt >= expected && dealt <= expected + 8,
        "{dealt} vs {expected}"
    );
    assert!(n.dot(step_ramp_normal()) > 0.999, "the step's ramp: {n}");
}

#[test]
fn a_25_m_s_launch_into_a_metre_tall_obstacle_still_hurts_as_a_wall() {
    // Two ½ m cells (a full 1 m block) are too tall to step: a wall, on the ground or off it.
    for height in [0.0, HOP] {
        let r = launch_into(2, 25.0, height);
        assert_eq!(r.dealt.len(), 1, "{height}: {:?}", r.dealt);
        assert!(r.dealt[0].0 >= 100, "{height}: {:?}", r.dealt);
        assert!(approx(r.dealt[0].1.z, -1.0, 1e-3), "{:?}", r.dealt);
    }
}

#[test]
fn hitting_a_step_edge_from_lower_in_the_air_hurts_as_a_ramp() {
    // Lower over the floor the capsule's base meets the step's top edge (a normal partly up), not
    // its side: the ramp too.
    let r = launch_into(1, 25.0, 0.3);
    let worst = r.dealt.iter().map(|d| d.0).max().unwrap_or(0);
    assert!(worst < 10, "{:?} (fastest {})", r.dealt, r.fastest);
}

#[test]
fn running_into_a_half_metre_step_at_any_speed_steps_up() {
    // On the ground the step-up takes it (no hit), as before.
    for speed in [5.0, 8.0, 14.0, 25.0] {
        let r = launch_into(1, speed, 0.0);
        assert_eq!(r.fastest, 0.0, "{speed}");
    }
}

#[test]
fn a_step_is_a_wall_when_the_config_says_so() {
    // The rule is data: off, the ½ m step's side is a wall again.
    let mut cfg = PlayerControllerConfig::default();
    cfg.damage.impact_step_as_ramp = false;
    let mut w = PlayerTestWorld::with_config(cfg);
    w.floor_at(0, 32);
    for z in 3..=13 {
        for x in -8..=8 {
            w.set_cell(x, 0, z, cube_block());
        }
    }
    let e = w.spawn(Vec3::new(0.5, HOP, 2.0));
    w.kick(0, e, Vec3::new(0.0, 0.0, 25.0));
    let dealt = hits_at(&mut w, e, ticks(1.0));
    assert!(dealt.iter().any(|d| d.0 >= 100), "{dealt:?}");
}
