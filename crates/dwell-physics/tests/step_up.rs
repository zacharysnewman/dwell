//! The step-up (PLAYER_CONTROLLER.md §4, §6.2) over a matrix of approaches: ½ m steps walked,
//! run and crouch-walked into from 0–80° off the face's normal, from a range of starting distances
//! (every sub-tick phase of the approach), onto Tier 1 boxes and voxel steps, two-step stairs, a
//! step's outside corner, a step beside a wall, a step at the top of a ramp, and jumps landing on a
//! step's edge. Owner (2026-10-20): "player step up doesn't always work, sometimes you get hung up
//! on the step and run into it instead." A case fails when the player never climbs (a hang-up), loses
//! most of its speed at the step, leaves the ground, or drops back off the step. Runs in the local
//! frame like every suite (far from the origin and on face B too).

mod common;
use common::*;
use dwell_blocks::{StateId, require};
use dwell_physics::Vec3;
use dwell_physics::config::PlayerControllerConfig;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Gait {
    Walk,
    Sprint,
    Crouch,
}

const GAITS: [Gait; 3] = [Gait::Walk, Gait::Sprint, Gait::Crouch];
/// Degrees off the face's normal.
const ANGLES: [f32; 9] = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0];
/// Ticks standing still before the approach.
const SETTLE: i32 = 3;
/// Starting distances (m, capsule surface to the face): 7 phases across a sprint tick's travel.
const STARTS: [f32; 7] = [0.6, 0.62, 0.64, 0.66, 0.68, 0.70, 0.72];

fn speed_of(g: Gait) -> f32 {
    let m = PlayerControllerConfig::default();
    match g {
        Gait::Walk => m.movement.walk_speed,
        Gait::Sprint => m.movement.run_speed,
        Gait::Crouch => m.crouch.speed,
    }
}

/// What one approach did.
#[derive(Debug, Default, Clone)]
struct Outcome {
    climbed: bool,
    /// Ticks from the first contact with the face to standing on the step.
    climb_ticks: i32,
    /// Lowest horizontal speed between reaching full speed and climbing, as a fraction of it.
    min_speed: f32,
    airborne: i32,
    /// Ticks on the step's top whose feet fell back below it.
    drops: i32,
    max_up: f32,
}

struct Approach {
    /// Builds the world; the step's face is at z = `face`, its top `top` above the floor (y = 0).
    build: fn(&mut PlayerTestWorld),
    face: f32,
    top: f32,
    /// Feet height the approach starts at.
    from: f32,
    /// x where the walker crosses the face line.
    cross_x: f32,
    /// Further distance (m) to the last face, beyond the first.
    slack: f32,
    /// The ground's height under the spawn (z), when it is not `from`.
    ground: Option<fn(f32) -> f32>,
}

fn run(a: &Approach, angle: f32, gait: Gait, start: f32) -> Outcome {
    let mut w = PlayerTestWorld::new();
    (a.build)(&mut w);
    let r = w.config.body.radius;
    let rad = angle.to_radians();
    let (s, c) = (rad.sin(), rad.cos());
    // Back along the path from where the capsule's front meets the face.
    let along = (start + r) / c;
    let (sx, sz) = (a.cross_x - s * along, a.face - c * along);
    let spawn = v(sx, a.ground.map_or(a.from, |g| g(sz) + 0.01), sz);
    let e = w.spawn(spawn);
    let yaw = angle; // yaw a° moves along (sin a, 0, cos a)
    let (run_held, crouch) = (gait == Gait::Sprint, gait == Gait::Crouch);
    // Settle on the ground first, then go.
    w.set_input(move |t, _| {
        let go = t >= SETTLE;
        input(0.0, f32::from(u8::from(go)), run_held && go, false, crouch && go, yaw, 0.0)
    });
    let speed = speed_of(gait);
    let arrive = ticks(start / (speed * c)) + ticks(0.3) + SETTLE;
    let total = arrive + ticks(0.6) + ticks(a.slack / (speed * c));
    let mut o = Outcome {
        min_speed: f32::MAX,
        ..Outcome::default()
    };
    let mut contact = None;
    let mut on_top = false;
    let mut last = w.pos(e);
    for t in 0..total {
        w.step(1);
        let p = w.pos(e);
        let feet = w.feet(e);
        let vel = w.vel(e);
        let h = ((p.x - last.x).powi(2) + (p.z - last.z).powi(2)).sqrt() * 60.0;
        last = p;
        if t > SETTLE && !w.c(e).ground.grounded {
            o.airborne += 1;
        }
        o.max_up = o.max_up.max(vel.y);
        let front = p.z + r * c;
        if contact.is_none() && front >= a.face - 0.02 {
            contact = Some(t);
        }
        if !o.climbed && feet >= a.from + a.top - 0.04 {
            o.climbed = true;
            o.climb_ticks = contact.map_or(0, |c0| t - c0);
            on_top = true;
        }
        if t > ticks(0.25) && !o.climbed {
            o.min_speed = o.min_speed.min(h / speed);
        }
        if std::env::var("DWELL_TRACE").is_ok() {
            let g = &w.c(e).ground;
            eprintln!(
                "{t:3} pos {:.3} {:.3} feet {:.3} vel {:.2} {:.2} {:.2} grounded {} gap {:.3} {:?}",
                p.x, p.z, feet, vel.x, vel.y, vel.z, g.grounded, g.gap, w.c(e).state
            );
        }
        if on_top && feet < a.from + a.top - 0.08 {
            o.drops += 1;
        }
    }
    if o.min_speed == f32::MAX {
        o.min_speed = 1.0;
    }
    o
}

/// Fails: never climbed, stalled (below 40 % of its speed before climbing), left the ground, or fell
/// back off the step.
fn failed(o: &Outcome) -> bool {
    !o.climbed || o.min_speed < 0.4 || o.airborne > 0 || o.drops > 0
}

fn matrix(name: &str, a: &Approach, angles: &[f32]) -> (usize, usize, Vec<String>) {
    let (mut fails, mut total) = (0, 0);
    let mut lines = Vec::new();
    for &gait in &GAITS {
        for &angle in angles {
            let mut row = String::new();
            for &start in &STARTS {
                let o = run(a, angle, gait, start);
                total += 1;
                if failed(&o) {
                    fails += 1;
                    row.push_str(&format!(
                        " [{start}: climbed {} min {:.2} air {} drops {} up {:.2}]",
                        o.climbed, o.min_speed, o.airborne, o.drops, o.max_up
                    ));
                }
            }
            if !row.is_empty() {
                lines.push(format!("{name} {gait:?} {angle}°:{row}"));
            }
        }
    }
    eprintln!("{name}: {fails} of {total} approaches failed");
    for l in &lines {
        eprintln!("  {l}");
    }
    (fails, total, lines)
}

fn assert_matrix(name: &str, a: &Approach, angles: &[f32]) {
    let (fails, total, lines) = matrix(name, a, angles);
    assert_eq!(fails, 0, "{name}: {fails} of {total} failed:\n{}", lines.join("\n"));
}

// --- the geometry -------------------------------------------------------------------------------

/// A ½ m Tier 1 box step, 20 m wide, face at z = 2.
fn box_step(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.half_step(-10, 0, 2, 9, 12);
}

fn grass() -> StateId {
    require("dwell:grass")
}

/// A ½ m voxel step: a level grass field (adaptive: it rests at half height, ADAPTIVE_BLOCKS.md) up
/// to a row of non-adaptive cubes, whose tops are a ½ m above it, face at z = 2.
fn voxel_step(w: &mut PlayerTestWorld) {
    w.fill(-14, -1, -14, 13, -1, 13, cube_block());
    w.fill(-12, 0, -12, 11, 0, 1, grass());
    w.fill(-14, 0, -14, 13, 0, -13, cube_block());
    w.fill(-14, 0, -14, -13, 0, 13, cube_block());
    w.fill(12, 0, -14, 13, 0, 13, cube_block());
    w.fill(-14, 0, 2, 13, 0, 13, cube_block());
}

/// Two ½ m box steps (stairs), faces at z = 2 and z = 2.6.
fn two_steps(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.half_step(-10, 0, 2, 9, 12);
    w.static_box(v(0.0, 0.75, 7.3), v(10.0, 0.25, 4.7));
}

/// A ½ m box step whose outside corner is at (0, 2): the step covers x ≥ 0, z ≥ 2.
fn corner_step(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.half_step(0, 0, 2, 9, 12);
}

/// A ½ m box step with a 2 m wall of cubes along x = −1 (cells x = −2), the walker against it.
fn step_by_wall(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.half_step(-1, 0, 2, 9, 12);
    w.fill(-2, 0, -10, -2, 1, 12, cube_block());
}

/// A 20° Tier 1 ramp rising toward +z, with a ½ m step at its top: the step's face at z = 7 rises
/// a ½ m above the ramp's surface there.
fn ramp_then_step(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.boxed(
        v(0.5, 0.0, RAMP_Z),
        v(12.0, 0.5, 2.5),
        euler(-20.0, 0.0, 0.0),
        Vec3::ZERO,
        0.0,
    );
    let y = ramp_y(7.0);
    w.static_box(v(0.5, (y + 0.5) / 2.0, 13.5), v(12.0, (y + 0.5) / 2.0, 6.5));
}

/// The ramp's centre: its top ends at the step's face.
const RAMP_Z: f32 = 4.8;

/// The 20° ramp's surface height at z.
fn ramp_y(z: f32) -> f32 {
    let a = 20f32.to_radians();
    // Top face through the centre + 0.5 along the normal (0, cos a, −sin a).
    0.5 * a.cos() + (z - (RAMP_Z - 0.5 * a.sin())) * a.tan()
}

// --- the tests ----------------------------------------------------------------------------------

#[test]
fn every_approach_climbs_a_half_box_step() {
    assert_matrix(
        "box step",
        &Approach {
            build: box_step,
            face: 2.0,
            top: 0.5,
            from: 0.0,
            cross_x: 0.5,
            slack: 0.0,
            ground: None,
        },
        &ANGLES,
    );
}

#[test]
fn every_approach_climbs_a_half_voxel_step() {
    assert_matrix(
        "voxel step",
        &Approach {
            build: voxel_step,
            face: 2.0,
            top: 0.5,
            from: 0.5,
            cross_x: 0.5,
            slack: 0.0,
            ground: None,
        },
        &ANGLES,
    );
}

#[test]
fn every_approach_climbs_two_steps() {
    assert_matrix(
        "two steps",
        &Approach {
            build: two_steps,
            face: 2.0,
            top: 1.0,
            from: 0.0,
            cross_x: 0.5,
            slack: 0.7,
            ground: None,
        },
        &ANGLES,
    );
}

#[test]
fn every_approach_climbs_a_steps_outside_corner() {
    // Crossing the face line just inside the corner: the capsule meets the corner's edge.
    for cross in [0.05, 0.2] {
        assert_matrix(
            &format!("corner at {cross}"),
            &Approach {
                build: corner_step,
                face: 2.0,
                top: 0.5,
                from: 0.0,
                cross_x: cross,
                slack: 0.0,
            ground: None,
            },
            &[0.0, 20.0, 40.0, 60.0],
        );
    }
}

#[test]
fn every_approach_climbs_a_step_beside_a_wall() {
    // Walking along the wall (pressed into it, up to 30° toward it) into the step.
    assert_matrix(
        "step by a wall",
        &Approach {
            build: step_by_wall,
            face: 2.0,
            top: 0.5,
            from: 0.0,
            cross_x: -0.68,
            slack: 0.0,
            ground: None,
        },
        &[0.0, -10.0, -20.0, -30.0],
    );
}

#[test]
fn every_approach_climbs_a_step_at_a_ramps_top() {
    assert_matrix("ramp then step", &ramp(), &[0.0, 20.0, 40.0, 60.0]);
}

fn ramp() -> Approach {
    Approach {
        build: ramp_then_step,
        face: 7.0,
        top: 0.5,
        from: ramp_y(7.0),
        cross_x: 0.5,
        slack: 0.3,
        ground: Some(ramp_y),
    }
}

/// `DWELL_TRACE=<gait>,<angle>,<start>` prints the ramp approach tick by tick.
#[test]
fn trace_one() {
    let Ok(spec) = std::env::var("DWELL_TRACE") else {
        return;
    };
    let f: Vec<&str> = spec.split(',').collect();
    let gait = match f[0] {
        "sprint" => Gait::Sprint,
        "crouch" => Gait::Crouch,
        _ => Gait::Walk,
    };
    eprintln!("{:?}", run(&ramp(), f[1].parse().unwrap(), gait, f[2].parse().unwrap()));
}
