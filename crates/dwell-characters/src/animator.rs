//! Which clips play (CHARACTERS.md §6): the controller's state → a role, from what every remote
//! snapshot already carries (state, flags, velocity, yaw and pitch; §9.3), cross-faded by the new
//! clip's `blend`; the jump and the landing as one-shots; the hold layer while an item is held; a
//! break or place swing as the action layer; the head following the look's pitch. The same machine
//! drives the first-person arms with the arms' roles.
//!
//! Roles are the data's names (`walk`, `sprint`): a character without a role's clip falls back to
//! `idle`, so a new character needs no code.

use dwell_protocol::PlayerState;

use crate::Tables;
use crate::anim::{Pose, crossfade, layer, sample};
use crate::types::{Character, Clip, Layer};

/// A swing for an edit (CHARACTERS.md §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swing {
    Break,
    Place,
}

impl Swing {
    pub fn role(self) -> &'static str {
        match self {
            Swing::Break => "break",
            Swing::Place => "place",
        }
    }
}

/// What the animator reads each frame: a remote snapshot's controller state (or the local
/// prediction's, for the arms).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveInput {
    pub state: PlayerState,
    /// World velocity (m/s).
    pub velocity: [f32; 3],
    /// ±1: the face it stands on (its up is ±y).
    pub face: f32,
    pub crouched: bool,
    pub climbing: bool,
    pub swimming: bool,
    pub flying: bool,
    /// The look's pitch (degrees, up positive).
    pub pitch: f32,
    /// An item is in hand (the hold layer).
    pub holding: bool,
    /// The ground under each foot (left, right), metres above the feet's height (0 on flat
    /// ground). A hook for smooth step-ups (foot IK and a hip lift on a step): nothing fills it
    /// yet and `plant_feet` leaves the pose as the clips made it, until the controller reports the
    /// step inputs (claude/smooth-steps).
    pub foot_ground: [f32; 2],
}

impl Default for MoveInput {
    fn default() -> Self {
        Self {
            state: PlayerState::Idle,
            velocity: [0.0; 3],
            face: 1.0,
            crouched: false,
            climbing: false,
            swimming: false,
            flying: false,
            pitch: 0.0,
            holding: false,
            foot_ground: [0.0; 2],
        }
    }
}

/// Horizontal speed (m/s) above which a grounded idle or crouched player is drawn walking.
pub const MOVING: f32 = 0.4;
/// Horizontal speed (m/s) under which touching down plays the landing; faster goes straight into the
/// gait (a landing at a run would slide the feet).
pub const LAND_MAX_SPEED: f32 = 1.5;
/// Upward speed (m/s, face-local) above which an airborne player is still in the jump.
pub const RISING: f32 = 0.5;

fn horizontal(v: [f32; 3]) -> f32 {
    v[0].hypot(v[2])
}

fn airborne(i: &MoveInput) -> bool {
    matches!(i.state, PlayerState::Jumping | PlayerState::Falling)
}

/// The body's base role for a controller state (§6's table), given the role playing now (for the
/// jump → fall → land sequence). Pure: the clip selection the tests pin.
pub fn body_role(i: &MoveInput, now: &str) -> &'static str {
    let speed = horizontal(i.velocity);
    let up = i.velocity[1] * i.face;
    if i.flying || i.state == PlayerState::Flying {
        return "fly";
    }
    if i.swimming || i.state == PlayerState::Swimming {
        return "swim";
    }
    if i.climbing || i.state == PlayerState::Climbing {
        return "climb";
    }
    if airborne(i) {
        return if up > RISING { "jump" } else { "fall" };
    }
    if matches!(now, "jump" | "fall") && speed < LAND_MAX_SPEED {
        return "land";
    }
    match i.state {
        PlayerState::Sliding => "slide",
        PlayerState::Running => "sprint",
        PlayerState::Walking => "walk",
        PlayerState::Crouching if speed > MOVING => "crouch_walk",
        PlayerState::Crouching => "crouch",
        _ if i.crouched => {
            if speed > MOVING {
                "crouch_walk"
            } else {
                "crouch"
            }
        }
        _ if speed > MOVING => "walk",
        _ => "idle",
    }
}

/// The arms' base role (§6: the arms show the gait, the slide and the jump; climbing, swimming and
/// flying hold the item still).
pub fn arms_role(i: &MoveInput, now: &str) -> &'static str {
    match body_role(i, now) {
        "crouch" | "climb" | "swim" | "fly" => "idle",
        "crouch_walk" => "walk",
        r => r,
    }
}

/// Which roles a machine plays: the full body's, or the first-person arms'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Body,
    Arms,
}

#[derive(Clone, Copy, Debug)]
struct Playing {
    role: &'static str,
    clip: usize,
    t: f32,
}

/// One character's animation state.
#[derive(Clone, Debug)]
pub struct Animator {
    tables: &'static Tables,
    character: &'static Character,
    space: Space,
    base: Option<Playing>,
    /// The clip faded out of, and how far the fade is (s) of how long.
    from: Option<(Playing, f32, f32)>,
    action: Option<Playing>,
}

impl Animator {
    pub fn new(tables: &'static Tables, character: &'static Character, space: Space) -> Self {
        Self {
            tables,
            character,
            space,
            base: None,
            from: None,
            action: None,
        }
    }

    fn clip_of(&self, role: &str) -> Option<usize> {
        match self.space {
            Space::Body => self.character.clip(role),
            Space::Arms => self.character.arms_clip(role),
        }
    }

    fn clip(&self, i: usize) -> &'static Clip {
        self.tables.clip(i)
    }

    /// The base role playing now.
    pub fn role(&self) -> &'static str {
        self.base.map_or("idle", |b| b.role)
    }

    /// The base clip's name playing now (`biped_walk`).
    pub fn clip_name(&self) -> Option<&'static str> {
        self.base.map(|b| self.clip(b.clip).name)
    }

    /// The swing playing, if any.
    pub fn action(&self) -> Option<&'static str> {
        self.action.map(|a| a.role)
    }

    /// Starts a swing (an edit's break or place) on the action layer.
    pub fn swing(&mut self, s: Swing) {
        if let Some(clip) = self.clip_of(s.role()) {
            self.action = Some(Playing {
                role: s.role(),
                clip,
                t: 0.0,
            });
        }
    }

    /// Advances `dt` seconds with this frame's input and returns the pose to draw.
    pub fn update(&mut self, i: &MoveInput, dt: f32) -> Pose {
        let now = self.role();
        let mut role = match self.space {
            Space::Body => body_role(i, now),
            Space::Arms => arms_role(i, now),
        };
        // A landing plays out unless the player moves off (then the gait takes over).
        if now == "land"
            && let Some(b) = self.base
            && b.t < self.clip(b.clip).duration
            && matches!(role, "idle" | "land")
        {
            role = "land";
        } else if role == "land" && now == "land" {
            role = match self.space {
                Space::Body => body_role(i, "idle"),
                Space::Arms => arms_role(i, "idle"),
            };
        }
        let clip = self
            .clip_of(role)
            .or_else(|| self.clip_of("idle"))
            .unwrap_or(0);
        match self.base {
            Some(b) if b.clip == clip => {}
            Some(b) => {
                let blend = self.clip(clip).blend;
                self.from = (blend > 0.0).then_some((b, 0.0, blend));
                self.base = Some(Playing { role, clip, t: 0.0 });
            }
            None => self.base = Some(Playing { role, clip, t: 0.0 }),
        }
        // Gaits play at speed / cycle loops a second, so the feet don't slide.
        let speed = horizontal(i.velocity).max(if role == "climb" {
            i.velocity[1].abs()
        } else {
            0.0
        });
        let rate = |c: &Clip| {
            if c.cycle > 0.0 {
                (speed / c.cycle * c.duration).clamp(0.0, 4.0)
            } else {
                1.0
            }
        };
        let base = self.base.as_mut().expect("set above");
        base.t += dt * rate(self.tables.clip(base.clip));
        let base = *base;
        let mut pose = sample(self.clip(base.clip), base.t);
        if let Some((mut from, done, length)) = self.from {
            from.t += dt * rate(self.clip(from.clip));
            let done = done + dt;
            if done >= length {
                self.from = None;
            } else {
                let old = sample(self.clip(from.clip), from.t);
                pose = crossfade(&old, &pose, done / length);
                self.from = Some((from, done, length));
            }
        }
        if self.space == Space::Body
            && i.holding
            && let Some(hold) = self.clip_of("hold")
        {
            layer(&mut pose, &sample(self.clip(hold), base.t), 1.0);
        }
        if let Some(a) = &mut self.action {
            a.t += dt;
            let c = self.tables.clip(a.clip);
            if a.t > c.duration {
                self.action = None;
            } else {
                debug_assert_eq!(c.layer, Layer::Action);
                let a = *a;
                layer(&mut pose, &sample(c, a.t), 1.0);
            }
        }
        if self.space == Space::Body {
            self.plant_feet(&mut pose, i);
            self.look(&mut pose, i);
        }
        pose
    }

    /// Each foot onto the ground under it (`MoveInput::foot_ground`): the place foot IK and a step's
    /// hip lift go. Not built yet: the step inputs aren't reported, so it changes nothing.
    fn plant_feet(&self, _pose: &mut Pose, _i: &MoveInput) {}

    /// The head (and a little of the chest, and the held arm) follow the look's pitch, added on the
    /// clips (+x pitches a bone's top forward: looking down).
    fn look(&self, pose: &mut Pose, i: &MoveInput) {
        let pitch = i.pitch.clamp(-89.0, 89.0);
        let mut add = |bone: &str, share: f32, lo: f32, hi: f32| {
            if let Some(b) = self.character.bone(bone) {
                pose.rot[b][0] = (pose.rot[b][0] - pitch * share).clamp(lo, hi);
            }
        };
        add("head", 0.6, -60.0, 60.0);
        add("chest", 0.15, -40.0, 60.0);
        if i.holding {
            add("arm_r", 0.5, -180.0, 70.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TABLES, player};

    fn input(state: PlayerState, v: [f32; 3]) -> MoveInput {
        MoveInput {
            state,
            velocity: v,
            ..MoveInput::default()
        }
    }

    #[test]
    fn the_controller_state_picks_the_body_clip() {
        use PlayerState::*;
        let cases: &[(MoveInput, &str, &str)] = &[
            (input(Idle, [0.0; 3]), "idle", "idle"),
            (input(Walking, [3.0, 0.0, 0.0]), "idle", "walk"),
            (input(Running, [0.0, 0.0, 6.0]), "walk", "sprint"),
            (input(Sliding, [0.0, 0.0, 8.0]), "sprint", "slide"),
            (input(Crouching, [0.0; 3]), "idle", "crouch"),
            (input(Crouching, [1.2, 0.0, 0.0]), "crouch", "crouch_walk"),
            (input(Jumping, [0.0, 5.0, 3.0]), "walk", "jump"),
            (input(Jumping, [0.0, -1.0, 3.0]), "jump", "fall"),
            (input(Falling, [0.0, -8.0, 0.0]), "jump", "fall"),
            (input(Idle, [0.0; 3]), "fall", "land"),
            // Touching down at a run goes straight into the gait.
            (input(Running, [0.0, 0.0, 6.0]), "fall", "sprint"),
            (input(Climbing, [0.0, 2.0, 0.0]), "idle", "climb"),
            (input(Swimming, [1.0, 0.0, 0.0]), "idle", "swim"),
            (input(Flying, [10.0, 0.0, 0.0]), "idle", "fly"),
        ];
        for (i, now, want) in cases {
            assert_eq!(body_role(i, now), *want, "{i:?} from {now}");
        }
        // Flags win over a stale state; on face B up is −y, so falling there is +y velocity... down.
        let flying = MoveInput {
            flying: true,
            ..input(Walking, [1.0, 0.0, 0.0])
        };
        assert_eq!(body_role(&flying, "walk"), "fly");
        let rising_on_b = MoveInput {
            face: -1.0,
            ..input(Jumping, [0.0, -5.0, 0.0])
        };
        assert_eq!(body_role(&rising_on_b, "walk"), "jump");
        assert_eq!(arms_role(&input(Climbing, [0.0, 2.0, 0.0]), "idle"), "idle");
        assert_eq!(arms_role(&input(Sliding, [0.0, 0.0, 8.0]), "walk"), "slide");
    }

    #[test]
    fn switching_clips_cross_fades_over_the_new_clips_blend() {
        let mut a = Animator::new(&TABLES, player(), Space::Body);
        let walk = input(PlayerState::Walking, [0.0, 0.0, 3.0]);
        for _ in 0..60 {
            a.update(&walk, 1.0 / 60.0);
        }
        assert_eq!(a.clip_name(), Some("biped_walk"));
        let walking = a.update(&walk, 1.0 / 60.0);
        let sprint = input(PlayerState::Running, [0.0, 0.0, 6.0]);
        let first = a.update(&sprint, 1.0 / 60.0);
        assert_eq!(a.clip_name(), Some("biped_sprint"));
        // One frame into the 0.25 s fade: still close to the walk, not snapped to the sprint.
        let thigh = player().bone("thigh_l").unwrap();
        let sprinting = sample(
            TABLES.clip(player().clip("sprint").unwrap()),
            1.0 / 60.0 * 6.0 / 3.0 * 0.66,
        );
        let d_walk = (first.rot[thigh][0] - walking.rot[thigh][0]).abs();
        let d_sprint = (first.rot[thigh][0] - sprinting.rot[thigh][0]).abs();
        assert!(d_walk < d_sprint + 1.0, "{d_walk} {d_sprint}");
        for _ in 0..30 {
            a.update(&sprint, 1.0 / 60.0);
        }
        assert!(a.from.is_none(), "the fade is over");
    }

    #[test]
    fn a_jump_falls_then_lands_then_stands() {
        let mut a = Animator::new(&TABLES, player(), Space::Body);
        let dt = 1.0 / 60.0;
        a.update(&input(PlayerState::Idle, [0.0; 3]), dt);
        a.update(&input(PlayerState::Jumping, [0.0, 6.0, 0.0]), dt);
        assert_eq!(a.role(), "jump");
        a.update(&input(PlayerState::Falling, [0.0, -6.0, 0.0]), dt);
        assert_eq!(a.role(), "fall");
        a.update(&input(PlayerState::Idle, [0.0; 3]), dt);
        assert_eq!(a.role(), "land");
        for _ in 0..40 {
            a.update(&input(PlayerState::Idle, [0.0; 3]), dt);
        }
        assert_eq!(a.role(), "idle");
    }

    #[test]
    fn a_swing_plays_on_the_action_layer_over_the_gait_and_ends() {
        let mut a = Animator::new(&TABLES, player(), Space::Body);
        let walk = MoveInput {
            holding: true,
            ..input(PlayerState::Walking, [0.0, 0.0, 3.0])
        };
        a.update(&walk, 0.1);
        a.swing(Swing::Break);
        let p = a.update(&walk, 0.2);
        assert_eq!(a.action(), Some("break"));
        assert_eq!(a.role(), "walk");
        let fore = player().bone("arm_r").unwrap();
        let swing = sample(TABLES.clip(player().clip("break").unwrap()), 0.2);
        assert!(swing.keys(fore));
        for _ in 0..10 {
            a.update(&walk, 0.1);
        }
        assert_eq!(a.action(), None);
        let _ = p;
    }

    #[test]
    fn the_head_follows_the_pitch() {
        let mut a = Animator::new(&TABLES, player(), Space::Body);
        let head = player().bone("head").unwrap();
        let level = a.update(&input(PlayerState::Idle, [0.0; 3]), 0.0).rot[head][0];
        let up = MoveInput {
            pitch: 40.0,
            ..input(PlayerState::Idle, [0.0; 3])
        };
        let looking_up = a.update(&up, 0.0).rot[head][0];
        // Looking up tips the head's top back (−x).
        assert!(looking_up < level - 15.0, "{level} {looking_up}");
    }

    #[test]
    fn the_arms_play_their_own_clips() {
        let mut a = Animator::new(&TABLES, player(), Space::Arms);
        a.update(&input(PlayerState::Running, [0.0, 0.0, 6.0]), 0.1);
        assert_eq!(a.clip_name(), Some("fp_sprint"));
        a.swing(Swing::Place);
        a.update(&input(PlayerState::Running, [0.0, 0.0, 6.0]), 0.1);
        assert_eq!(a.action(), Some("place"));
    }
}
