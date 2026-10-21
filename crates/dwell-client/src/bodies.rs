//! The game's side of the characters (docs/CHARACTERS.md §9.2–9.3, Phase 40 C1): one animator per
//! remote player, fed by what its snapshots carry, and one for your own first-person arms, fed by
//! the prediction. Poses are never sent: each client animates the others itself.

use dwell_blocks::info;
use dwell_blocks::shape::solid_span_at;
use dwell_characters::anim::bone_transforms;
use dwell_characters::animator::{Animator, MoveInput, Space, Swing};
use dwell_characters::{Character, TABLES};
use dwell_game::eye::{Step, StepRamp, StepSample, StepSmoother};
use dwell_game::remotes::RemoteView;
use dwell_protocol::{controller_flags, held_item, player_flags, swing_flags};
use dwell_world::hash::HashMap;
use dwell_world::{CELL_H, FaceFrame, VoxelWorld, m_of_row, row_of_m};

use crate::render::{ArmsView, Held, PlayerView};

/// What a remote snapshot says about its player's movement, for the clip selection.
pub fn remote_input(v: &RemoteView, holding: bool) -> MoveInput {
    MoveInput {
        state: v.state,
        velocity: v.face().vector_f32(v.velocity),
        crouched: v.is_crouched(),
        climbing: v.flags & player_flags::CLIMBING != 0,
        swimming: v.flags & player_flags::SWIMMING != 0,
        flying: v.flags & player_flags::FLYING != 0,
        pitch: v.pitch,
        holding,
        foot_ground: [0.0; 2],
        jumped: v.jumped,
    }
}

/// The local prediction's movement (controller flags), for the arms.
pub fn local_input(
    state: dwell_protocol::PlayerState,
    velocity: [f64; 3],
    controller: u8,
    face: FaceFrame,
) -> MoveInput {
    MoveInput {
        state,
        velocity: face.vector(velocity).map(|v| v as f32),
        crouched: controller & controller_flags::CROUCHING != 0,
        climbing: controller & controller_flags::CLIMBING != 0,
        swimming: controller & controller_flags::SWIMMING != 0,
        flying: controller & controller_flags::FLYING != 0,
        pitch: 0.0,
        holding: true,
        foot_ground: [0.0; 2],
        jumped: false,
    }
}

/// A held item as the wire carries it (`held_item`, server ids) as the client draws it: `to_client`
/// maps the block (registry sync, ADR 0038); an unknown block is an empty hand.
pub fn held_from_wire(held: u16, to_client: impl Fn(u16) -> Option<u16>) -> Held {
    let block = held & held_item::BLOCK_MASK;
    let Some(block) = (block != 0).then(|| to_client(block)).flatten() else {
        return Held::Empty;
    };
    if held & held_item::BRUSH != 0 {
        Held::Brush(block)
    } else {
        Held::Block(block)
    }
}

/// The wire form of what you hold: `to_server` maps the block.
pub fn held_to_wire(held: Held, to_server: impl Fn(u16) -> Option<u16>) -> u16 {
    let (brush, block) = match held {
        Held::Empty => return 0,
        Held::Block(b) => (0, b),
        Held::Brush(b) => (held_item::BRUSH, b),
    };
    match to_server(block) {
        Some(b) if b != 0 && b <= held_item::BLOCK_MASK => brush | b,
        _ => 0,
    }
}

/// The swing a changed swing byte starts (none for the first one seen: that edit was before).
pub fn swing_of(last: Option<u8>, now: u8) -> Option<Swing> {
    let changed = last.is_some_and(|l| l != now);
    if !changed || now & swing_flags::COUNT_MASK == last.unwrap_or(0) & swing_flags::COUNT_MASK {
        return None;
    }
    if now & swing_flags::BREAK != 0 {
        Some(Swing::Break)
    } else {
        Some(Swing::Place)
    }
}

/// How many of a body's latest base roles `Bodies::roles` keeps (tests, telemetry).
pub const ROLE_HISTORY: usize = 32;

/// A remote body's animation state for tests and telemetry (`__dwell.state().bodies`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyState {
    pub player_id: u16,
    /// The base role playing now, and the swing.
    pub role: &'static str,
    pub action: Option<&'static str>,
    /// The base roles it started, oldest first: the last `ROLE_HISTORY` of `plays`. A test reads
    /// what the body played since it looked, however few frames it sampled the role in.
    pub played: Vec<&'static str>,
    /// How many base roles it has started since it appeared.
    pub plays: u32,
}

struct RemoteBody {
    animator: Animator,
    /// The latest base roles started (oldest first, at most `ROLE_HISTORY`), and their count.
    played: std::collections::VecDeque<&'static str>,
    plays: u32,
    swing: Option<u8>,
    /// The drawn feet over steps: a virtual ramp (PLAYER_CONTROLLER.md §9, the step view).
    steps: StepSmoother,
    /// The step that started this frame, if any (for a step pose).
    stepped: Option<Step>,
    /// Each foot's (left, right) position in the character's frame (x left, z ahead) in the last
    /// pose drawn: where its ground is read.
    feet_xz: [[f32; 2]; 2],
}

/// The ground under a point, face-local (BIFACIAL_WORLD.md §11): `(frame, point)` → the height of
/// the solid top under it within reach, or `None`.
pub type GroundAt<'a> = dyn FnMut(FaceFrame, [f64; 3]) -> Option<f64> + 'a;

/// Every character the game animates.
pub struct Bodies {
    character: &'static Character,
    remotes: HashMap<u16, RemoteBody>,
    arms: Animator,
    /// Your own body (drawn only while dead, for the third-person view over it).
    own: Animator,
    /// The step view's ramp and the most one step lifts (`max_step_lift()`): a foot's ground is
    /// read within this of the drawn feet.
    ramp: StepRamp,
    max_lift: f64,
}

impl Default for Bodies {
    fn default() -> Self {
        let character = dwell_characters::player();
        let config = dwell_physics::config::PlayerControllerConfig::default();
        Self {
            character,
            remotes: HashMap::default(),
            arms: Animator::new(&TABLES, character, Space::Arms),
            own: Animator::new(&TABLES, character, Space::Body),
            ramp: config.step_view.into(),
            max_lift: f64::from(config.max_step_lift()),
        }
    }
}

/// The solid top under face-local point `p` (x, z; scanning down from `p.y + reach` to
/// `p.y − reach`) in `world`, read through `f`: the ground a drawn foot stands on (render-side
/// only; the sim never reads it).
pub fn ground_at(
    world: &mut VoxelWorld,
    f: FaceFrame,
    [x, y, z]: [f64; 3],
    reach: f64,
) -> Option<f64> {
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = ((x - f64::from(cx)) as f32, (z - f64::from(cz)) as f32);
    // Rows are `CELL_H` tall on the half-height branch (HALF_HEIGHT_GRID.md §2): heights are metres.
    for row in (row_of_m(y - reach)..=row_of_m(y + reach)).rev() {
        let [wx, wy, wz] = f.cell([cx, row, cz]);
        if !info(world.get_voxel(wx, wy, wz)).solid {
            continue;
        }
        let span = solid_span_at(world.shape_at(wx, wy, wz), fx, fz);
        if span.hi <= span.lo {
            continue;
        }
        let (_, hi) = f.in_cell(span.lo, span.hi);
        let top = m_of_row(row) + f64::from(hi) * CELL_H;
        if top <= y + reach {
            return Some(top);
        }
    }
    None
}

impl Bodies {
    /// The step that started on remote player `id`'s drawn body this frame (its height, + up):
    /// the hook for a step pose.
    pub fn step_started(&self, id: u16) -> Option<Step> {
        self.remotes.get(&id).and_then(|b| b.stepped)
    }

    /// A remote player's body this frame (`dt` seconds since the last), holding `held`: drawn at
    /// its feet plus the step view's offset, each foot planted on the ground `ground` reads.
    pub fn remote(
        &mut self,
        v: &RemoteView,
        held: Held,
        dt: f64,
        ground: &mut GroundAt,
    ) -> PlayerView {
        let character = self.character;
        let (ramp, max_lift) = (self.ramp, self.max_lift);
        let body = self
            .remotes
            .entry(v.player_id)
            .or_insert_with(|| RemoteBody {
                animator: Animator::new(&TABLES, character, Space::Body),
                played: std::collections::VecDeque::new(),
                plays: 0,
                swing: None,
                steps: StepSmoother::new(),
                stepped: None,
                feet_xz: [[0.0; 2]; 2],
            });
        if let Some(s) = swing_of(body.swing, v.swing) {
            body.animator.swing(s);
        }
        body.swing = Some(v.swing);
        let f = v.face();
        let local = f.point(v.feet);
        let grounded = v.flags & player_flags::GROUNDED != 0 && !v.is_dead();
        let [vx, _, vz] = v.velocity;
        body.stepped = body.steps.follow(
            StepSample {
                feet: local[1],
                grounded,
                speed: f64::from(vx.hypot(vz)),
                max_step: max_lift + 0.05,
                ramp,
            },
            dt,
        );
        let drawn = local[1] + body.steps.offset();
        let mut input = remote_input(v, held != Held::Empty);
        if grounded {
            // Each foot's place in the world: the body's frame (render/characters.rs
            // `body_frame`: on face B its left turns over).
            let (s, c) = f64::from(v.yaw).to_radians().sin_cos();
            let side = f.sign_f64();
            input.foot_ground = body.feet_xz.map(|[lx, lz]| {
                let (lx, lz) = (f64::from(lx), f64::from(lz));
                let at = [
                    local[0] + side * c * lx + s * lz,
                    drawn,
                    local[2] - side * s * lx + c * lz,
                ];
                ground(f, at).map_or(0.0, |g| (g - drawn).clamp(-max_lift, max_lift) as f32)
            });
        }
        let pose = body.animator.update(&input, dt as f32);
        let role = body.animator.role();
        if body.plays == 0 || body.played.back() != Some(&role) {
            if body.played.len() == ROLE_HISTORY {
                body.played.pop_front();
            }
            body.played.push_back(role);
            body.plays += 1;
        }
        let bones = bone_transforms(character, &pose);
        for (k, name) in ["foot_l", "foot_r"].into_iter().enumerate() {
            if let Some(b) = character.bone(name) {
                body.feet_xz[k] = [bones[b].t[0], bones[b].t[2]];
            }
        }
        PlayerView {
            feet: f.point([local[0], drawn, local[2]]),
            face: f,
            yaw: f64::from(v.yaw),
            dead: v.is_dead(),
            look: dwell_characters::DEFAULT_LOOK,
            pose,
            role: body.animator.role(),
            action: body.animator.action(),
            held,
            light: [1.0, 0.0],
        }
    }

    /// Forgets the players not in `ids`.
    pub fn retain(&mut self, keep: impl Fn(u16) -> bool) {
        self.remotes.retain(|id, _| keep(*id));
    }

    /// Your body lying where you died.
    pub fn own_dead(&mut self, feet: [f64; 3], yaw: f64, face: FaceFrame, dt: f64) -> PlayerView {
        let pose = self.own.update(&MoveInput::default(), dt as f32);
        PlayerView {
            feet,
            face,
            yaw,
            dead: true,
            look: dwell_characters::DEFAULT_LOOK,
            pose,
            role: self.own.role(),
            action: None,
            held: Held::Empty,
            light: [1.0, 0.0],
        }
    }

    /// Your own body standing at `feet` this frame, moving as `input` says: cast into the shadow
    /// maps only, so you have a shadow in first person (docs/DAY_NIGHT.md §10).
    pub fn own(
        &mut self,
        feet: [f64; 3],
        yaw: f64,
        face: FaceFrame,
        input: &MoveInput,
        held: Held,
        dt: f64,
    ) -> PlayerView {
        let pose = self.own.update(input, dt as f32);
        PlayerView {
            feet,
            face,
            yaw,
            dead: false,
            look: dwell_characters::DEFAULT_LOOK,
            pose,
            role: self.own.role(),
            action: self.own.action(),
            held,
            light: [1.0, 0.0],
        }
    }

    /// Your arms this frame.
    pub fn arms(&mut self, input: &MoveInput, held: Held, dt: f64) -> ArmsView {
        ArmsView {
            pose: self.arms.update(input, dt as f32),
            held,
            light: [1.0, 0.0],
        }
    }

    /// Your arms swing (an edit sent).
    pub fn swing_arms(&mut self, s: Swing) {
        self.arms.swing(s);
    }

    /// Every remote body's roles, by player id (tests: `__dwell.state().bodies`).
    pub fn roles(&self) -> Vec<BodyState> {
        let mut out: Vec<_> = self
            .remotes
            .iter()
            .map(|(id, b)| BodyState {
                player_id: *id,
                role: b.animator.role(),
                action: b.animator.action(),
                played: b.played.iter().copied().collect(),
                plays: b.plays,
            })
            .collect();
        out.sort_by_key(|r| r.player_id);
        out
    }

    /// The arms' base role and swing (tests, telemetry).
    pub fn arms_state(&self) -> (&'static str, Option<&'static str>) {
        (self.arms.role(), self.arms.action())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_item_round_trips_through_its_wire_form() {
        let to = |b: u16| Some(b + 1);
        let from = |b: u16| Some(b - 1);
        for h in [Held::Empty, Held::Block(5), Held::Brush(9)] {
            assert_eq!(held_from_wire(held_to_wire(h, to), from), h);
        }
        // A block the other side doesn't know is an empty hand.
        assert_eq!(held_from_wire(held_item::BRUSH | 7, |_| None), Held::Empty);
        assert_eq!(held_to_wire(Held::Block(3), |_| None), 0);
    }

    dwell_world::both_faces!(
        /// A foot's ground is the solid top under it, face-local, on either face.
        fn the_ground_under_a_foot_is_read_face_local(f: FaceFrame) {
            let mut w = VoxelWorld::new(dwell_world::empty_generator(), None);
            let stone = dwell_blocks::require("dwell:sandbox_floor");
            let [x, y, z] = f.cell([3, 40, 5]);
            w.set_voxel(x, y, z, f.state(stone));
            // Its top is row 41's bottom, face-local, in metres (rows are `CELL_H` tall).
            let top = m_of_row(41);
            assert_eq!(ground_at(&mut w, f, [3.5, top + 0.2, 5.5], 0.8), Some(top));
            assert_eq!(ground_at(&mut w, f, [3.5, top - 0.2, 5.5], 0.8), Some(top));
            assert_eq!(ground_at(&mut w, f, [3.5, top + 2.0, 5.5], 0.8), None);
            assert_eq!(ground_at(&mut w, f, [4.5, top, 5.5], 0.8), None);
        }
    );

    dwell_world::both_faces!(
        /// PLAYER_CONTROLLER.md §9 (the step view): a remote player's step-up, interpolated over
        /// a snapshot interval, is drawn along a virtual ramp (no faster than 45° at its speed),
        /// starts one step, and ends on the step.
        fn a_remote_step_up_is_drawn_as_a_ramp(f: FaceFrame) {
            let mut bodies = Bodies::default();
            let dt = 1.0 / 240.0;
            let mut ground =
                |_: FaceFrame, [_, _, z]: [f64; 3]| Some(if z >= 1.0 { 0.5 } else { 0.0 });
            let mut last: Option<f64> = None;
            let mut steps = 0;
            for i in 0..240 {
                let t = f64::from(i) * dt;
                let z = 5.0 * t - 0.5;
                // The interpolated feet: up 0.5 m over 50 ms as the body crosses z = 0.7.
                let feet = 0.5 * ((t - 0.24) / 0.05).clamp(0.0, 1.0);
                let v = RemoteView {
                    player_id: 1,
                    feet: f.point([0.0, 32.0 + feet, z]),
                    velocity: f.vector_f32([0.0, 0.0, 5.0]),
                    yaw: 0.0,
                    pitch: 0.0,
                    state: dwell_protocol::PlayerState::Walking,
                    flags: player_flags::GROUNDED
                        | dwell_physics::net::face_bit(f, player_flags::FACE_B),
                    held: 0,
                    swing: 0,
                    jumped: false,
                };
                let view = bodies.remote(&v, Held::Empty, dt, &mut ground);
                steps += usize::from(bodies.step_started(1).is_some());
                let drawn = f.y(view.feet[1]) - 32.0;
                if let Some(l) = last {
                    assert!((drawn - l) / dt <= 5.0 + 1e-6, "{i}: {l} → {drawn}");
                }
                last = Some(drawn);
            }
            assert_eq!(steps, 1);
            assert!((last.unwrap() - 0.5).abs() < 1e-9);
        }
    );

    /// A body keeps the base roles it played, so a test that samples it seldom (a slow page) still
    /// reads a short one (a slide, a jump) that started and ended between its looks.
    #[test]
    fn a_body_keeps_the_roles_it_played_lately() {
        use dwell_protocol::PlayerState::{Idle, Running, Sliding};
        let mut bodies = Bodies::default();
        let mut ground = |_: FaceFrame, _: [f64; 3]| Some(32.0);
        let mut frame = |b: &mut Bodies, state, speed: f32| {
            let v = RemoteView {
                player_id: 7,
                feet: [0.0, 32.0, 0.0],
                velocity: [0.0, 0.0, speed],
                yaw: 0.0,
                pitch: 0.0,
                state,
                flags: player_flags::GROUNDED,
                held: 0,
                swing: 0,
                jumped: false,
            };
            b.remote(&v, Held::Empty, 1.0 / 60.0, &mut ground);
        };
        frame(&mut bodies, Idle, 0.0);
        frame(&mut bodies, Running, 7.0);
        frame(&mut bodies, Sliding, 8.0);
        frame(&mut bodies, Running, 7.0);
        frame(&mut bodies, Running, 7.0);
        let s = &bodies.roles()[0];
        assert_eq!((s.player_id, s.role), (7, "sprint"));
        assert_eq!(s.played, ["idle", "sprint", "slide", "sprint"]);
        assert_eq!(s.plays, 4);
        // Only the latest ROLE_HISTORY are kept; the count goes on.
        for _ in 0..ROLE_HISTORY {
            frame(&mut bodies, Sliding, 8.0);
            frame(&mut bodies, Running, 7.0);
        }
        let s = &bodies.roles()[0];
        assert_eq!(s.played.len(), ROLE_HISTORY);
        assert_eq!(s.played.last(), Some(&"sprint"));
        assert_eq!(s.plays, 4 + 2 * ROLE_HISTORY as u32);
    }

    dwell_world::both_faces!(
        /// A remote's jump plays however slow the frames that draw it: a page at a few frames a
        /// second (CI's software renderer, or a long frame) samples few of the jump's snapshots,
        /// and a frame that falls past its rise, or after its landing, still starts the jump
        /// (then the landing).
        fn a_remote_jump_plays_however_slow_the_frames(f: FaceFrame) {
            use dwell_game::remotes::RemotePlayers;
            use dwell_protocol::{PlayerState, RemotePlayerState, SIM_HZ, SNAPSHOT_HZ};
            // The jump's snapshots: off the ground at 1 s, rising at 7.07 m/s against 20 m/s²
            // (the 1.25 m jump), landing 0.707 s later.
            let snapshot = |t: f64| {
                let air = t - 1.0;
                let (y, vy, state, flags) = if (0.0..0.707).contains(&air) {
                    let vy = 7.07 - 20.0 * air;
                    (7.07 * air - 10.0 * air * air, vy, PlayerState::Jumping, 0)
                } else {
                    (0.0, 0.0, PlayerState::Idle, player_flags::GROUNDED)
                };
                RemotePlayerState {
                    player_id: 3,
                    position: f.point([0.0, 32.0 + y, 0.0]),
                    velocity: f.vector_f32([0.0, vy as f32, 0.0]),
                    yaw: 0,
                    pitch: 0,
                    state,
                    flags: flags | dwell_physics::net::face_bit(f, player_flags::FACE_B),
                    held: 0,
                    swing: 0,
                }
            };
            let tick_ms = 1000.0 / f64::from(SIM_HZ);
            let mut ground = |_: FaceFrame, _: [f64; 3]| Some(32.0);
            for interval in [0.25, 0.5, 0.8, 1.0] {
                for phase in 0..20 {
                    let mut remotes = RemotePlayers::default();
                    let mut bodies = Bodies::default();
                    let mut frame = interval * f64::from(phase) / 20.0;
                    let mut tick = 0;
                    while frame < 5.0 {
                        // The snapshots that arrived (30 ms after they were sent) by this frame.
                        while f64::from(tick) * tick_ms + 30.0 <= frame * 1000.0 {
                            let t = f64::from(tick) * tick_ms;
                            remotes.push(tick, &[snapshot(t / 1000.0)], t + 30.0);
                            tick += SIM_HZ / SNAPSHOT_HZ;
                        }
                        for v in remotes.sample(frame * 1000.0) {
                            bodies.remote(&v, Held::Empty, interval, &mut ground);
                        }
                        frame += interval;
                    }
                    let s = &bodies.roles()[0];
                    assert!(
                        s.played.contains(&"jump"),
                        "frames {interval} s apart, phase {phase}/20: {:?}",
                        s.played
                    );
                    assert_eq!(s.role, "idle", "{interval} s, {phase}/20: {:?}", s.played);
                }
            }
        }
    );

    #[test]
    fn a_changed_swing_count_starts_a_swing_and_the_first_seen_does_not() {
        let place = swing_flags::PLACE | 3;
        assert_eq!(swing_of(None, place), None);
        assert_eq!(swing_of(Some(place), place), None);
        assert_eq!(
            swing_of(Some(place), swing_flags::BREAK | 4),
            Some(Swing::Break)
        );
        assert_eq!(
            swing_of(Some(place), swing_flags::PLACE | 4),
            Some(Swing::Place)
        );
    }
}
