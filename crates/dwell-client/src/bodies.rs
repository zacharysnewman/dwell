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

struct RemoteBody {
    animator: Animator,
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
        }
    }

    /// Your arms this frame.
    pub fn arms(&mut self, input: &MoveInput, held: Held, dt: f64) -> ArmsView {
        ArmsView {
            pose: self.arms.update(input, dt as f32),
            held,
        }
    }

    /// Your arms swing (an edit sent).
    pub fn swing_arms(&mut self, s: Swing) {
        self.arms.swing(s);
    }

    /// Every remote body's base role and swing, by player id (tests: `__dwell.state().bodies`).
    pub fn roles(&self) -> Vec<(u16, &'static str, Option<&'static str>)> {
        let mut out: Vec<_> = self
            .remotes
            .iter()
            .map(|(id, b)| (*id, b.animator.role(), b.animator.action()))
            .collect();
        out.sort_by_key(|r| r.0);
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
