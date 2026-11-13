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
use dwell_protocol::{SwingKind, controller_flags, held_item, player_flags, swing_flags};
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
        guard: None,
        defeat: None,
        armed: false,
    }
}

/// A remote player as it moves over the ground carrying it: its velocity less `carry`, the
/// velocity (world, m/s) of the moving body under its feet (`Pieces::ground_velocity`; zero on the
/// world's ground). The animator, the step view and the footsteps read this (owner, 2026-11-09:
/// standing still on a moving body played the walk).
pub fn over_its_ground(v: &RemoteView, carry: [f64; 3]) -> RemoteView {
    RemoteView {
        velocity: std::array::from_fn(|k| v.velocity[k] - carry[k] as f32),
        ..*v
    }
}

/// The local prediction's movement (controller flags), for the arms: its `velocity` (world) less
/// `carry`, the velocity of the ground carrying it (the controller's `base_velocity`, world), so
/// standing still on a moving body is standing still.
pub fn local_input(
    state: dwell_protocol::PlayerState,
    velocity: [f64; 3],
    carry: [f64; 3],
    controller: u8,
    face: FaceFrame,
) -> MoveInput {
    let over = std::array::from_fn(|k| velocity[k] - carry[k]);
    MoveInput {
        state,
        velocity: face.vector(over).map(|v| v as f32),
        crouched: controller & controller_flags::CROUCHING != 0,
        climbing: controller & controller_flags::CLIMBING != 0,
        swimming: controller & controller_flags::SWIMMING != 0,
        flying: controller & controller_flags::FLYING != 0,
        pitch: 0.0,
        holding: true,
        foot_ground: [0.0; 2],
        jumped: false,
        guard: None,
        defeat: None,
        armed: false,
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

/// What a remote player holds: its held tool by name (`HeldTools`, ADR 0067) over what the
/// held-item wire says (blocks only), drawn as the tool's held item for its hold.
pub fn with_tool(held: Held, tool: Option<(&str, bool)>) -> Held {
    match tool
        .and_then(|(name, vertical)| dwell_protocol::chopping::tool(name).map(|t| (t, vertical)))
    {
        Some((t, vertical)) => crate::render::characters::held_item(
            t.name,
            Some(t.held_for(vertical).unwrap_or("block")),
        ),
        None => held,
    }
}

/// The wire form of what you hold: `to_server` maps the block.
pub fn held_to_wire(held: Held, to_server: impl Fn(u16) -> Option<u16>) -> u16 {
    let (brush, block) = match held {
        // A tool has no wire form yet (protocol v17 carries blocks): the others see an empty hand.
        // A held look neither (it is an item, not a block).
        Held::Empty | Held::Tool(_) | Held::Look(_) | Held::Model { .. } => return 0,
        Held::Block(b) => (0, b),
        Held::Brush(b) => (held_item::BRUSH, b),
    };
    match to_server(block) {
        Some(b) if b != 0 && b <= held_item::BLOCK_MASK => brush | b,
        _ => 0,
    }
}

/// The swing a changed swing byte starts (none for the first one seen: that edit was before):
/// the chop `kind` names (`SwingKind`, from a server that sends the kinds), else the break or
/// place the byte says (a kind this build doesn't know, or none, is that too).
pub fn swing_of(last: Option<u8>, now: u8, kind: u8) -> Option<Swing> {
    let changed = last.is_some_and(|l| l != now);
    if !changed || now & swing_flags::COUNT_MASK == last.unwrap_or(0) & swing_flags::COUNT_MASK {
        return None;
    }
    match SwingKind::from_u8(kind) {
        Some(SwingKind::ChopLevel) => return Some(Swing::ChopLevel),
        Some(SwingKind::ChopVertical) => return Some(Swing::ChopVertical),
        Some(SwingKind::Plain) | None => {}
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
    /// Defeated in its last snapshot (it kneels: CHARACTERS.md §11.13), and the seconds left of its
    /// stand once it stood up at a shrine.
    was_defeated: bool,
    standing_s: f64,
}

/// Defeat's input for a body (§11.13): kneeling while `defeated`, else standing up for the
/// `standing_s` left after it stood up at a shrine (`STAND_S` from the moment it was placed
/// there), else none. Counts `standing_s` down by `dt`.
pub fn defeat_input(
    defeated: bool,
    was_defeated: &mut bool,
    standing_s: &mut f64,
    dt: f64,
) -> Option<dwell_characters::animator::Defeat> {
    use dwell_characters::animator::Defeat;
    if *was_defeated && !defeated {
        *standing_s = dwell_protocol::defeat::STAND_S;
    }
    *was_defeated = defeated;
    if defeated {
        *standing_s = 0.0;
        return Some(Defeat::Kneel);
    }
    let standing = *standing_s > 0.0;
    *standing_s = (*standing_s - dt).max(0.0);
    standing.then_some(Defeat::Stand)
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
    /// A swing held at a clip time on your arms and every remote body (the bench's `clip-strip`:
    /// `__dwell.holdSwing`), or none.
    held_swing: Option<(Swing, f32)>,
    /// Each remote player's raised-block role this frame (the server's guards; WORK_QUEUE item 35).
    pub guards: HashMap<u16, &'static str>,
}

/// Plays swing `s` on `a` held at clip time `t` (starting it if another action or none plays).
pub fn hold_swing_on(a: &mut Animator, held: Option<(Swing, f32)>) {
    if let Some((s, t)) = held {
        if a.action() != Some(s.role()) {
            a.swing(s);
        }
        a.drive_action(Some(t));
    }
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
            held_swing: None,
            guards: HashMap::default(),
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
                was_defeated: false,
                standing_s: 0.0,
            });
        if let Some(s) = swing_of(body.swing, v.swing, v.swing_kind) {
            body.animator.swing(s);
        }
        hold_swing_on(&mut body.animator, self.held_swing);
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
        input.defeat = defeat_input(
            v.is_dead(),
            &mut body.was_defeated,
            &mut body.standing_s,
            dt,
        );
        input.armed = held.is_weapon();
        input.guard = self.guards.get(&v.player_id).copied();
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

    /// Your body kneeling where you were defeated, or standing up at the shrine (`defeat`), holding
    /// `held` (a sword is planted: CHARACTERS.md §11.13), seen from the defeat camera.
    pub fn own_defeated(
        &mut self,
        feet: [f64; 3],
        yaw: f64,
        face: FaceFrame,
        defeat: dwell_characters::animator::Defeat,
        held: Held,
        dt: f64,
    ) -> PlayerView {
        let input = MoveInput {
            holding: held != Held::Empty,
            defeat: Some(defeat),
            armed: held.is_weapon(),
            ..MoveInput::default()
        };
        let pose = self.own.update(&input, dt as f32);
        PlayerView {
            feet,
            face,
            yaw,
            look: dwell_characters::DEFAULT_LOOK,
            pose,
            role: self.own.role(),
            action: None,
            held,
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
        hold_swing_on(&mut self.arms, self.held_swing);
        ArmsView {
            pose: self.arms.update(input, dt as f32),
            held,
            light: [1.0, 0.0],
        }
    }

    /// Holds swing `s` at clip time `t` (s) on your arms and every remote body until released
    /// (none): the bench's `clip-strip` frames (docs/DEV_TOOLS.md).
    pub fn hold_swing(&mut self, held: Option<(Swing, f32)>) {
        if held.is_none() && self.held_swing.is_some() {
            self.arms.drive_action(None);
            for b in self.remotes.values_mut() {
                b.animator.drive_action(None);
            }
        }
        self.held_swing = held;
    }

    /// Whether a swing is held ([`Bodies::hold_swing`]).
    pub fn swing_held(&self) -> bool {
        self.held_swing.is_some()
    }

    /// Your arms swing (an edit sent).
    pub fn swing_arms(&mut self, s: Swing) {
        self.arms.swing(s);
    }

    /// Your arms' swing driven by a melee swing under way (its hit-stop and pull-back,
    /// crate::chop::SwingRun::arms), or let go.
    pub fn drive_arms(&mut self, d: crate::chop::ArmsDrive) {
        d.apply(&mut self.arms);
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

    #[test]
    fn a_held_swing_poses_the_arms_at_its_clip_time_until_released() {
        // face-a-only: the arms' pose, no world.
        let mut b = Bodies::default();
        let input = MoveInput::default();
        b.hold_swing(Some((Swing::ChopLevel, 0.18)));
        let ch = dwell_characters::player();
        let clip = TABLES.clip(ch.arms_clip("chop_level").unwrap());
        let want = dwell_characters::anim::sample(clip, 0.18);
        let arm = ch.bone("arm_r").unwrap();
        for _ in 0..3 {
            let v = b.arms(&input, Held::Empty, 0.1);
            assert_eq!(
                v.pose.rot[arm], want.rot[arm],
                "held at 0.18 s frame after frame"
            );
        }
        b.hold_swing(Some((Swing::ChopVertical, 0.05)));
        let v = b.arms(&input, Held::Empty, 0.1);
        let vertical = TABLES.clip(ch.arms_clip("chop_vertical").unwrap());
        assert_eq!(
            v.pose.rot[arm],
            dwell_characters::anim::sample(vertical, 0.05).rot[arm]
        );
        b.hold_swing(None);
        for _ in 0..10 {
            b.arms(&input, Held::Empty, 0.1);
        }
        assert_eq!(
            b.arms.action(),
            None,
            "released, the swing plays out and ends"
        );
    }
    use super::*;
    use dwell_characters::animator::body_role;
    use dwell_protocol::PlayerState;

    /// A remote on face `f`, grounded, its world velocity `velocity` (face-local) in `state`.
    fn grounded_remote(f: FaceFrame, state: PlayerState, velocity: [f32; 3]) -> RemoteView {
        RemoteView {
            player_id: 1,
            feet: f.point([0.5, 40.5, 0.5]),
            velocity: f.vector_f32(velocity),
            yaw: 0.0,
            pitch: 0.0,
            state,
            flags: player_flags::GROUNDED | dwell_physics::net::face_bit(f, player_flags::FACE_B),
            held: 0,
            swing: 0,
            swing_kind: 0,
            jumped: false,
        }
    }

    dwell_world::both_faces!(
        /// Owner, 2026-11-09: "walking animations play while standing still on moving physics
        /// objects". The clips read the motion over the ground carrying the player, not the
        /// world's: still on a raft moving at 3 m/s is idle; walking on it walks, with it or
        /// against it, even where the two cancel and the player is still in the world. Red
        /// before: the raft's 3 m/s played the walk.
        fn a_remote_still_on_a_moving_body_is_idle_and_one_walking_on_it_walks(f: FaceFrame) {
            let carry = f.vector([3.0, 0.0, 0.0]);
            let role = |state, v: [f32; 3]| {
                let r = over_its_ground(&grounded_remote(f, state, v), carry);
                body_role(&remote_input(&r, false), "idle")
            };
            assert_eq!(role(PlayerState::Idle, [3.0, 0.0, 0.0]), "idle");
            assert_eq!(role(PlayerState::Crouching, [3.0, 0.0, 0.0]), "crouch");
            assert_eq!(role(PlayerState::Walking, [8.0, 0.0, 0.0]), "walk");
            assert_eq!(role(PlayerState::Walking, [0.0, 0.0, 0.0]), "walk");
            // The walk's clip plays at the speed over the raft (5 m/s), not the world's 0.
            let r = over_its_ground(
                &grounded_remote(f, PlayerState::Walking, [-2.0, 0.0, 0.0]),
                carry,
            );
            let i = remote_input(&r, false);
            assert!(
                (i.velocity[0].hypot(i.velocity[2]) - 5.0).abs() < 1e-4,
                "{:?}",
                i.velocity
            );
        }
    );

    dwell_world::both_faces!(
        /// As above for your own arms and body: the controller's carry is taken off.
        fn your_arms_still_on_a_moving_body_are_idle(f: FaceFrame) {
            let carry = f.vector([0.0, 0.0, 3.0]);
            let grounded = controller_flags::GROUNDED;
            let still = local_input(PlayerState::Idle, carry, carry, grounded, f);
            assert_eq!(body_role(&still, "idle"), "idle");
            assert!(
                still.velocity.iter().all(|v| v.abs() < 1e-6),
                "{:?}",
                still.velocity
            );
            let crouched = local_input(
                PlayerState::Crouching,
                carry,
                carry,
                grounded | controller_flags::CROUCHING,
                f,
            );
            assert_eq!(body_role(&crouched, "idle"), "crouch");
            let against = local_input(PlayerState::Walking, [0.0; 3], carry, grounded, f);
            assert_eq!(body_role(&against, "idle"), "walk");
            assert!(
                (against.velocity[2] + 3.0).abs() < 1e-6,
                "{:?}",
                against.velocity
            );
        }
    );

    #[test]
    fn the_held_item_round_trips_through_its_wire_form() {
        let to = |b: u16| Some(b + 1);
        let from = |b: u16| Some(b - 1);
        for h in [Held::Empty, Held::Block(5), Held::Brush(9)] {
            assert_eq!(held_from_wire(held_to_wire(h, to), from), h);
        }
        // A block the other side doesn't know is an empty hand.
        assert_eq!(held_from_wire(held_item::BRUSH | 7, |_| None), Held::Empty);
        // Others' held axe (by name, `HeldTools`) is drawn in its hold; no tool: the wire's.
        assert_eq!(
            with_tool(Held::Empty, Some(("dwell:stone_axe", false))),
            Held::Model {
                hold: "stone_axe",
                model: "stone_axes"
            }
        );
        assert_eq!(
            with_tool(Held::Empty, Some(("dwell:stone_axe", true))),
            Held::Model {
                hold: "stone_axe_vertical",
                model: "stone_axes"
            }
        );
        assert_eq!(with_tool(Held::Block(4), None), Held::Block(4));
        assert_eq!(
            with_tool(Held::Block(4), Some(("dwell:nope", false))),
            Held::Block(4)
        );
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
                    swing_kind: 0,
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
                swing_kind: 0,
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
                    swing_kind: 0,
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

    dwell_world::both_faces!(
        /// WORK_QUEUE item 17: another player's chop plays the body's chop clip by its direction
        /// (`biped_chop_level`, `biped_chop_vertical`) on the frame its swing count steps, from a
        /// snapshot carrying the kind (`SwingHello`); without the kind (an older server) the same
        /// step is a `break`. Red before: every stepped break swing played `break`.
        fn a_remote_chop_plays_the_body_chop_clip_of_its_direction(f: FaceFrame) {
            let ch = dwell_characters::player();
            let mut ground = |_: FaceFrame, _: [f64; 3]| Some(40.0);
            let mut b = Bodies::default();
            let mut v = grounded_remote(f, PlayerState::Idle, [0.0; 3]);
            v.swing = swing_flags::BREAK | 7;
            b.remote(&v, Held::Empty, 0.05, &mut ground);
            assert_eq!(
                b.roles()[0].action,
                None,
                "the first swing byte seen started before"
            );
            for (kind, role, clip) in [
                (SwingKind::ChopLevel, "chop_level", "biped_chop_level"),
                (
                    SwingKind::ChopVertical,
                    "chop_vertical",
                    "biped_chop_vertical",
                ),
                (SwingKind::Plain, "break", "biped_break"),
            ] {
                v.swing = swing_flags::BREAK | ((v.swing + 1) & swing_flags::COUNT_MASK);
                v.swing_kind = kind as u8;
                b.remote(&v, Held::Empty, 0.0, &mut ground);
                assert_eq!(b.roles()[0].action, Some(role), "{kind:?}");
                let i = ch.clip(role).expect("the body has the clip");
                assert_eq!(TABLES.clip(i).name, clip);
            }
            // A kind this build doesn't know is the break the byte says.
            let next = swing_flags::BREAK | ((v.swing + 1) & swing_flags::COUNT_MASK);
            assert_eq!(swing_of(Some(v.swing), next, 200), Some(Swing::Break));
        }
    );

    #[test]
    fn a_changed_swing_count_starts_a_swing_and_the_first_seen_does_not() {
        let place = swing_flags::PLACE | 3;
        assert_eq!(swing_of(None, place, 0), None);
        assert_eq!(swing_of(Some(place), place, 0), None);
        assert_eq!(
            swing_of(Some(place), swing_flags::BREAK | 4, 0),
            Some(Swing::Break)
        );
        assert_eq!(
            swing_of(Some(place), swing_flags::PLACE | 4, 0),
            Some(Swing::Place)
        );
    }
}
