//! Impact sounds from the real controller (docs/AUDIO.md §5): a body launched into a wall, its
//! motion sampled each tick as the client samples its prediction, through the same body probe
//! (`Surroundings`) and `MoverSounds`. The hit sounds once, as loud as its speed says, on either
//! face. A wall never hurts (PLAYER_CONTROLLER.md §6.10: only the ground does), so the server deals
//! nothing for it and no hurt follows (`player_event_sound` would sound one for damage dealt).

#[path = "../../dwell-physics/tests/common/mod.rs"]
mod common;

use common::*;
use dwell_blocks::require;
use dwell_game::sound::{
    MoverSample, MoverSounds, SoundEvent, SoundKind, Surroundings, player_event_sound,
    speed_intensity,
};
use dwell_physics::controller::{Input, events};
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::{PlayerEventKind, PlayerState};
use dwell_world::{FaceFrame, SEA_LEVEL, SEA_LEVEL_M, rows_for_m};

/// What a run produced: the sounds, the server's damage, the controller's hardest impact speed.
struct Run {
    sounds: Vec<SoundEvent>,
    dealt: i32,
    impact: f32,
}

/// On face `f`: a floor at sea level and a 10 m stone wall across +z at z = 6, placed through the
/// face's frame; a body at (0.5, 0.5), launched toward it at `kick` m/s (or run at it with `input`)
/// for `seconds`.
fn into_the_wall(f: FaceFrame, kick: f32, input: Option<Input>, seconds: f32) -> Run {
    let mut w = PlayerTestWorld::world_frame();
    let stone = f.state(require("dwell:stone"));
    let rows = rows_for_m(1.0);
    for x in -8..=8 {
        for z in -8..=8 {
            w.world.set_voxel(x, f.row(SEA_LEVEL - 1), z, stone);
        }
        for row in SEA_LEVEL..SEA_LEVEL + 10 * rows {
            w.world.set_voxel(x, f.row(row), 6, stone);
        }
    }
    let [x, y, z] = f.point([0.5, SEA_LEVEL_M + if kick > 0.0 { 3.0 } else { 0.0 }, 0.5]);
    let h = w.spawn_at(DVec3::new(x, y, z), 0.0);
    if kick > 0.0 {
        let [vx, vy, vz] = f.vector_f32([0.0, 0.0, kick]);
        w.kick(0, h, Vec3::new(vx, vy, vz));
    }
    if let Some(i) = input {
        w.set_input(move |_, _| i);
    }
    let config = w.config;
    let mut sounds = MoverSounds::new();
    let mut run = Run {
        sounds: Vec::new(),
        dealt: 0,
        impact: 0.0,
    };
    for _ in 0..ticks(seconds) {
        w.step(1);
        let c = *w.c(h);
        if c.events & events::IMPACTED != 0 {
            run.dealt += config.damage.impact_damage(c.impact_speed);
            run.impact = run.impact.max(c.impact_speed);
        }
        let p = w.players.position(&w.physics, h);
        let feet = [p.x, w.players.feet(&w.physics, h), p.z];
        let v = w.players.velocity(&w.physics, h);
        let height = f64::from(if c.crouching {
            config.crouch.height
        } else {
            config.body.standing_height
        });
        let world = &mut w.world;
        let near = Surroundings::read(feet, f, f64::from(config.body.radius), height, |c| {
            world.get_voxel(c[0], c[1], c[2])
        });
        let sample = MoverSample {
            feet,
            velocity: [f64::from(v.x), f64::from(v.y), f64::from(v.z)],
            state: PlayerState::from_u8(c.state as u8).unwrap_or(PlayerState::Idle),
            grounded: c.ground.grounded,
            crouched: c.crouching,
            face: f,
            submerged: 0.0,
            ground: near.ground,
            body: near.body,
            walls: near.walls,
            ceiling: near.ceiling,
            dt: 1.0 / 60.0,
        };
        sounds.update(&sample, None, &mut run.sounds);
    }
    // The server sends a damage event for what it dealt: its hurt.
    if run.dealt > 0 {
        run.sounds.extend(player_event_sound(
            PlayerEventKind::Damage,
            run.dealt.min(255) as u8,
            None,
        ));
    }
    run
}

fn of(run: &Run, kind: SoundKind) -> Vec<SoundEvent> {
    run.sounds
        .iter()
        .copied()
        .filter(|e| e.kind == kind)
        .collect()
}

dwell_world::both_faces!(
    /// Launched into a wall at 25 m/s: one loud impact, as loud as the launch speed, on the wall's
    /// material, and no damage nor hurt (walls never hurt); at 8 m/s a soft thud.
    fn a_hard_wall_hit_is_loud_and_does_not_hurt_and_a_soft_one_a_thud(f: FaceFrame) {
        let hard = into_the_wall(f, 25.0, None, 1.5);
        let hits = of(&hard, SoundKind::Impact);
        assert_eq!(hits.len(), 1, "{:?}", hard.sounds);
        let expected = speed_intensity(SoundKind::Impact, 25.0).unwrap_or(0.0);
        assert!(
            (f64::from(hits[0].intensity) - expected).abs() < 0.05,
            "{hits:?} vs 25 m/s's {expected}"
        );
        assert!(hits[0].intensity > 0.85, "loud: {hits:?}");
        assert_eq!(hits[0].block, f.state(require("dwell:stone")));
        assert_eq!(hard.dealt, 0, "a wall: impact {}", hard.impact);
        assert!(of(&hard, SoundKind::Hurt).is_empty());

        let soft = into_the_wall(f, 8.0, None, 1.5);
        let hits = of(&soft, SoundKind::Impact);
        assert_eq!(hits.len(), 1, "{:?}", soft.sounds);
        assert!(
            hits[0].intensity > 0.1 && hits[0].intensity < 0.4,
            "soft: {hits:?} ({} m/s)",
            soft.impact
        );
        assert_eq!(soft.dealt, 0);
        assert!(of(&soft, SoundKind::Hurt).is_empty());
    }
);

dwell_world::both_faces!(
    /// Sprinting into the wall: a soft bump, once, while it keeps pressing on.
    fn sprinting_into_a_wall_bumps_once(f: FaceFrame) {
        let input = Input {
            move_y: 1.0,
            run: true,
            ..Input::default()
        };
        let run = into_the_wall(f, 0.0, Some(input), 3.0);
        let hits = of(&run, SoundKind::Impact);
        assert_eq!(hits.len(), 1, "{:?} (impact {})", run.sounds, run.impact);
        assert!(hits[0].intensity < 0.35, "{hits:?}");
        assert_eq!(run.dealt, 0);
    }
);
