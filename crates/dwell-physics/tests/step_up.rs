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
    /// Largest one-tick feet change while grounded (a step-up's lift).
    max_lift: f32,
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
        input(
            0.0,
            f32::from(u8::from(go)),
            run_held && go,
            false,
            crouch && go,
            yaw,
            0.0,
        )
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
    let mut last_feet = w.feet(e);
    for t in 0..total {
        w.step(1);
        let p = w.pos(e);
        let feet = w.feet(e);
        if t > SETTLE && w.c(e).ground.grounded {
            o.max_lift = o.max_lift.max((feet - last_feet).abs());
        }
        last_feet = feet;
        let vel = w.vel(e);
        let h = ((p.x - last.x).powi(2) + (p.z - last.z).powi(2)).sqrt() * 60.0;
        last = p;
        if t > SETTLE && !w.c(e).ground.grounded {
            o.airborne += 1;
        }
        o.max_up = o.max_up.max(vel.y);
        let front = p.z + r;
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
                p.x,
                p.z,
                feet,
                vel.x,
                vel.y,
                vel.z,
                g.grounded,
                g.gap,
                w.c(e).state
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

/// Fails: never climbed, stalled (below 40 % of its speed before climbing), left the ground, fell
/// back off the step, or lifted further in a tick than the camera smooths (a pop of the view).
fn failed(o: &Outcome) -> bool {
    let lift_limit = PlayerControllerConfig::default().max_step_lift() + 0.05;
    !o.climbed || o.min_speed < 0.4 || o.airborne > 0 || o.drops > 0 || o.max_lift > lift_limit
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
                        " [{start}: climbed {} min {:.2} air {} drops {} up {:.2} lift {:.2}]",
                        o.climbed, o.min_speed, o.airborne, o.drops, o.max_up, o.max_lift
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
    assert_eq!(
        fails,
        0,
        "{name}: {fails} of {total} failed:\n{}",
        lines.join("\n")
    );
}

// --- the geometry -------------------------------------------------------------------------------

/// A ½ m Tier 1 box step, 20 m wide, face at z = 2.
fn box_step(w: &mut PlayerTestWorld) {
    w.floor_at(0, 24);
    w.half_step(-10, 0, 2, 9, 12);
}

/// A ½ m voxel step (half-height cells, ADR 0042: one cell): a floor of cubes whose top is y = 0 and
/// one row of cells on it from z = 2.
fn voxel_step(w: &mut PlayerTestWorld) {
    w.floor_at(0, 16);
    for z in 2..=13 {
        for x in -14..=13 {
            w.set_cell(x, 0, z, cube_block());
        }
    }
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

/// The ramp's centre: its top runs on under the step (a box's end face would be a wall).
const RAMP_Z: f32 = 5.2;

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
    assert_matrix("voxel step", &voxel(), &ANGLES);
}

fn voxel() -> Approach {
    Approach {
        build: voxel_step,
        face: 2.0,
        top: 0.5,
        from: 0.0,
        cross_x: 0.5,
        slack: 0.0,
        ground: None,
    }
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
    let (mut fails, mut lines) = (0, Vec::new());
    for cross in [0.05, 0.2] {
        let (f, _, l) = matrix(
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
        fails += f;
        lines.extend(l);
    }
    assert_eq!(fails, 0, "{}", lines.join("\n"));
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
    let a = if f.get(3) == Some(&"voxel") {
        voxel()
    } else {
        ramp()
    };
    eprintln!(
        "{:?}",
        run(&a, f[1].parse().unwrap(), gait, f[2].parse().unwrap())
    );
}

/// Jumps toward a step, taking off `distance` before its face: (landed on top, ticks stalled
/// against the face below its top).
fn jump_onto(top: f32, angle: f32, run_held: bool, distance: f32) -> (bool, i32) {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    if top < 0.75 {
        w.half_step(-30, 0, 2, 29, 30);
    } else {
        w.fill(-30, 0, 2, 29, 0, 30, cube_block());
    }
    let r = w.config.body.radius;
    let rad = angle.to_radians();
    // Start 1.5 m back so the run is up to speed; jump once the front is `distance` from the face.
    let along = (1.5 + distance + r) / rad.cos();
    let e = w.spawn(v(0.5 - rad.sin() * along, 0.0, 2.0 - rad.cos() * along));
    let jump_at = std::rc::Rc::new(std::cell::Cell::new(i32::MAX));
    let at = jump_at.clone();
    w.set_input(move |t, _| {
        let jump = t >= at.get() && t < at.get() + 3;
        input(0.0, 1.0, run_held, jump, false, angle, 0.0)
    });
    let mut stalled = 0;
    let mut last = w.pos(e);
    for _ in 0..ticks(2.0) {
        w.step(1);
        let p = w.pos(e);
        if jump_at.get() == i32::MAX && 2.0 - (p.z + r) <= distance {
            jump_at.set(w.tick);
        }
        let moved = ((p.x - last.x).powi(2) + (p.z - last.z).powi(2)).sqrt() * 60.0;
        if std::env::var("DWELL_TRACE_JUMP").is_ok() {
            let c = w.c(e);
            eprintln!(
                "{} pos {p} feet {:.3} vel {} grounded {} {:?}",
                w.tick,
                w.feet(e),
                w.vel(e),
                c.ground.grounded,
                c.state
            );
        }
        if w.c(e).ground.grounded && w.feet(e) < top - 0.05 && p.z > 1.0 && moved < 0.5 {
            stalled += 1;
        }
        last = p;
    }
    (w.feet(e) > top - 0.05, stalled)
}

#[test]
fn a_jump_toward_a_step_lands_on_it_without_hanging_on_its_edge() {
    // Take-offs from 0.1 m to 1.5 m before the face, as far as the jump reaches (it is 1.25 m high,
    // and above 1 m for ~0.5 s), all land on a ½ m step and a 1 m block, never hanging on the edge.
    if let Ok(spec) = std::env::var("DWELL_TRACE_JUMP") {
        // `top,run,angle,distance`: one jump, tick by tick.
        let f: Vec<f32> = spec.split(',').map(|s| s.parse().unwrap()).collect();
        eprintln!("{:?}", jump_onto(f[0], f[2], f[1] > 0.0, f[3]));
        return;
    }
    let mut fails = Vec::new();
    let mut total = 0;
    for top in [0.5, 1.0] {
        for run_held in [false, true] {
            for angle in [0.0, 20.0, 40.0, 60.0] {
                for k in 0..8 {
                    let distance = 0.1 + 0.2 * k as f32;
                    // Only take-offs the jump can reach: it is above the block's top for ~0.5 s.
                    let speed = if run_held { 8.0 } else { 5.0 };
                    if distance > 0.5 * speed * f32::to_radians(angle).cos() - 0.3 {
                        continue;
                    }
                    total += 1;
                    let (on, stalled) = jump_onto(top, angle, run_held, distance);
                    if !on || stalled > 0 {
                        fails.push(format!(
                            "top {top} run {run_held} {angle}° from {distance:.1}: on {on} stalled {stalled}"
                        ));
                    }
                }
            }
        }
    }
    eprintln!("jumps: {} of {total} failed", fails.len());
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn a_step_under_a_low_ceiling_is_not_stepped_onto() {
    // The standing capsule (1.8 m) does not fit between the step's top and a ceiling 2.1 m above
    // the floor; crouched (0.9 m) it does.
    for crouch in [false, true] {
        let mut w = PlayerTestWorld::new();
        w.floor_at(0, 24);
        w.half_step(-10, 0, 2, 9, 12);
        // The ceiling overhangs the step's face by 0.1 m.
        w.static_box(v(0.0, 2.6, 7.0), v(10.0, 0.5, 5.1));
        let e = w.spawn(v(0.5, 0.0, 0.5));
        w.set_input(move |t, _| {
            let go = t >= SETTLE;
            input(0.0, f32::from(u8::from(go)), false, false, crouch, 0.0, 0.0)
        });
        let mut highest_head = f32::MIN;
        for _ in 0..ticks(1.5) {
            w.step(1);
            highest_head = highest_head.max(w.head(e));
        }
        if crouch {
            assert!(w.feet(e) > 0.45, "crouched: feet {}", w.feet(e));
            assert!(w.pos(e).z > 2.5, "crouched: {}", w.pos(e));
        } else {
            assert!(w.feet(e) < 0.05, "standing: feet {}", w.feet(e));
            assert!(w.pos(e).z < 2.0, "standing: {}", w.pos(e));
        }
        assert!(
            highest_head <= 2.1 + 0.01,
            "crouch {crouch}: head {highest_head}"
        );
    }
}

#[test]
fn walking_along_a_steps_face_does_not_step_onto_it() {
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 24);
    w.half_step(-10, 0, 2, 9, 12);
    // Pressed against the face, walking along it (+x).
    let e = w.spawn(v(-6.0, 0.0, 2.0 - 0.3));
    w.set_input(|_, _| input(0.0, 1.0, false, false, false, 90.0, 0.0));
    for _ in 0..ticks(2.0) {
        w.step(1);
        assert!(w.feet(e) < 0.05, "stepped up at {}", w.pos(e));
    }
    assert!(w.pos(e).x > 2.0, "{}", w.pos(e));
}

#[test]
fn a_sprint_slide_steps_up_a_half_step_and_slides_on() {
    for angle in [0.0f32, 30.0, 60.0] {
        let mut w = PlayerTestWorld::new();
        w.floor_at(0, 40);
        w.half_step(-30, 0, 10, 29, 30);
        let rad = angle.to_radians();
        // Sprint for a second (8 m along the path), crouch into a slide ~1.5 m before the face.
        let e = w.spawn(v(0.5 - rad.sin() * 9.5, 0.0, 10.0 - rad.cos() * 9.5));
        w.set_input(move |t, _| input(0.0, 1.0, true, false, t >= 60, angle, 0.0));
        w.step(62);
        assert!(w.c(e).sprint_slide.active, "{angle}°: not sliding");
        let mut last = w.pos(e);
        let mut slowest = f32::MAX;
        let mut stepped = false;
        for _ in 0..ticks(0.6) {
            w.step(1);
            let p = w.pos(e);
            let moved = ((p.x - last.x).powi(2) + (p.z - last.z).powi(2)).sqrt() * 60.0;
            last = p;
            if !stepped {
                slowest = slowest.min(moved);
            }
            stepped |= w.feet(e) > 0.45;
        }
        assert!(stepped, "{angle}°: never stepped up, at {}", w.pos(e));
        assert!(
            slowest > 4.0,
            "{angle}°: slowed to {slowest} m/s at the step"
        );
        assert!(
            w.c(e).sprint_slide.active,
            "{angle}°: the slide ended at the step"
        );
    }
}

/// A stair of `n` ½ m Tier 1 box steps with treads of `tread` m, rising toward +z from z = 2, on a
/// floor at y = 0; its top runs on 6 m past the last step.
fn stair(w: &mut PlayerTestWorld, tread: f32, n: i32) {
    w.floor_at(0, 40);
    for k in 0..n {
        let z0 = 2.0 + k as f32 * tread;
        let z1 = 2.0 + n as f32 * tread + 6.0;
        let top = 0.5 * (k + 1) as f32;
        w.static_box(
            v(0.0, top / 2.0, (z0 + z1) / 2.0),
            v(10.0, top / 2.0, (z1 - z0) / 2.0),
        );
    }
}

#[test]
fn half_metre_stairs_are_walked_and_run_up_and_down_at_speed_and_on_the_ground() {
    // Owner, 2026-10-20 ("treat steps like half ramps"): the hitch on V2's ½ m steps was measured
    // to be the camera's (PLAYER_CONTROLLER.md §9, the step view), not the body's. The body keeps
    // stepping in one tick, and this pins what the camera relies on: through every step-up and
    // step-down of a stair the horizontal speed stays above 90 % of the gait's, the player stays on
    // the ground, and no tick moves the feet further than the camera smooths (`max_step_lift`).
    // Measured on 0.20.5: 93–100 % (mean 100 %), no airborne tick, 0.5 m a step.
    let n = 6;
    for tread in [0.6f32, 1.0, 2.0] {
        for run in [false, true] {
            for down in [false, true] {
                let mut w = PlayerTestWorld::new();
                stair(&mut w, tread, n);
                let top_z = 2.0 + n as f32 * tread + 3.0;
                let (start, yaw) = if down {
                    (v(0.0, 0.5 * n as f32 + 0.02, top_z), 180.0)
                } else {
                    (v(0.0, 0.02, -6.0), 0.0)
                };
                let e = w.spawn_yaw(start, yaw);
                w.step(ticks(0.3));
                w.set_input(move |_, _| input(0.0, 1.0, run, false, false, yaw, 0.0));
                let speed = if run {
                    w.config.movement.run_speed
                } else {
                    w.config.movement.walk_speed
                };
                let lift = w.config.max_step_lift();
                let at = format!("tread {tread} run {run} down {down}");
                let (mut last, mut feet) = (w.pos(e), w.feet(e));
                let (mut slowest, mut airborne, mut largest, mut steps) = (f32::MAX, 0, 0.0f32, 0);
                for _ in 0..ticks(4.0) {
                    w.step(1);
                    let p = w.pos(e);
                    let moved = (p.x - last.x).hypot(p.z - last.z) * 60.0;
                    let f = w.feet(e);
                    // On the stair: from a metre before its first face to a metre past its last.
                    if p.z > 1.0 && p.z < 2.0 + (n - 1) as f32 * tread + 1.0 {
                        slowest = slowest.min(moved);
                        airborne += i32::from(!w.c(e).ground.grounded);
                        largest = largest.max((f - feet).abs());
                    }
                    steps += i32::from((f - feet).abs() > 0.3);
                    (last, feet) = (p, f);
                    // Off the stair at either end (its top runs on 6 m; the floor 40 m).
                    if (!down && p.z > top_z) || (down && p.z < -2.0) {
                        break;
                    }
                }
                assert_eq!(steps, n, "{at}: steps taken");
                assert!(slowest > 0.9 * speed, "{at}: slowed to {slowest} m/s");
                assert_eq!(airborne, 0, "{at}: airborne ticks");
                assert!(largest <= lift, "{at}: feet moved {largest} m in a tick");
                let end = if down { 0.0 } else { 0.5 * n as f32 };
                assert!(
                    (w.feet(e) - end).abs() < 0.01,
                    "{at}: ended at {}",
                    w.feet(e)
                );
            }
        }
    }
}

#[test]
fn walking_off_a_drop_higher_than_a_step_falls() {
    // The ground snap down a step (which the camera smooths as a virtual ramp) never takes a real
    // drop: off a 1 m ledge the player leaves the ground and falls.
    let mut w = PlayerTestWorld::new();
    w.floor_at(0, 40);
    w.fill(-10, 0, -10, 9, 0, 2, cube_block());
    let e = w.spawn_yaw(v(0.0, 1.02, 0.0), 0.0);
    w.step(ticks(0.3));
    w.set_input(|_, _| input(0.0, 1.0, false, false, false, 0.0, 0.0));
    let mut airborne = 0;
    let mut largest = 0.0f32;
    let mut feet = w.feet(e);
    for _ in 0..ticks(1.5) {
        w.step(1);
        airborne += i32::from(!w.c(e).ground.grounded);
        largest = largest.max(feet - w.feet(e));
        feet = w.feet(e);
    }
    assert!(airborne >= 5, "{airborne} airborne ticks");
    assert!(largest < 0.3, "dropped {largest} m in a tick");
    assert!(w.feet(e).abs() < 0.01, "landed at {}", w.feet(e));
}
