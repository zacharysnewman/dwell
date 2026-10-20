//! The game's side of the characters (docs/CHARACTERS.md §9.2–9.3, Phase 40 C1): one animator per
//! remote player, fed by what its snapshots carry, and one for your own first-person arms, fed by
//! the prediction. Poses are never sent: each client animates the others itself.

use dwell_characters::animator::{Animator, MoveInput, Space, Swing};
use dwell_characters::{Character, TABLES};
use dwell_game::remotes::RemoteView;
use dwell_protocol::{controller_flags, held_item, player_flags, swing_flags};
use dwell_world::hash::HashMap;

use crate::render::{ArmsView, Held, PlayerView};

/// What a remote snapshot says about its player's movement, for the clip selection.
pub fn remote_input(v: &RemoteView, holding: bool) -> MoveInput {
    MoveInput {
        state: v.state,
        velocity: v.velocity,
        face: if v.is_face_b() { -1.0 } else { 1.0 },
        crouched: v.is_crouched(),
        climbing: v.flags & player_flags::CLIMBING != 0,
        swimming: v.flags & player_flags::SWIMMING != 0,
        flying: v.flags & player_flags::FLYING != 0,
        pitch: v.pitch,
        holding,
    }
}

/// The local prediction's movement (controller flags), for the arms.
pub fn local_input(
    state: dwell_protocol::PlayerState,
    velocity: [f64; 3],
    controller: u8,
    face: i32,
) -> MoveInput {
    MoveInput {
        state,
        velocity: velocity.map(|v| v as f32),
        face: if face < 0 { -1.0 } else { 1.0 },
        crouched: controller & controller_flags::CROUCHING != 0,
        climbing: controller & controller_flags::CLIMBING != 0,
        swimming: controller & controller_flags::SWIMMING != 0,
        flying: controller & controller_flags::FLYING != 0,
        pitch: 0.0,
        holding: true,
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
}

/// Every character the game animates.
pub struct Bodies {
    character: &'static Character,
    remotes: HashMap<u16, RemoteBody>,
    arms: Animator,
    /// Your own body (drawn only while dead, for the third-person view over it).
    own: Animator,
}

impl Default for Bodies {
    fn default() -> Self {
        let character = dwell_characters::player();
        Self {
            character,
            remotes: HashMap::default(),
            arms: Animator::new(&TABLES, character, Space::Arms),
            own: Animator::new(&TABLES, character, Space::Body),
        }
    }
}

impl Bodies {
    /// A remote player's body this frame (`dt` seconds since the last), holding `held`.
    pub fn remote(&mut self, v: &RemoteView, held: Held, dt: f64) -> PlayerView {
        let character = self.character;
        let body = self
            .remotes
            .entry(v.player_id)
            .or_insert_with(|| RemoteBody {
                animator: Animator::new(&TABLES, character, Space::Body),
                swing: None,
            });
        if let Some(s) = swing_of(body.swing, v.swing) {
            body.animator.swing(s);
        }
        body.swing = Some(v.swing);
        let input = remote_input(v, held != Held::Empty);
        let pose = body.animator.update(&input, dt as f32);
        PlayerView {
            feet: v.feet,
            face: if v.is_face_b() { -1.0 } else { 1.0 },
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
    pub fn own_dead(&mut self, feet: [f64; 3], yaw: f64, face: f64, dt: f64) -> PlayerView {
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
