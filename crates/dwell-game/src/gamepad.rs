//! Controller support (docs/GAMEPAD.md) as a platform-free state machine, like the keyboard's
//! (`input.rs`) and the touch controls' (`touch.rs`): the shell hands it the W3C Standard
//! Gamepad's buttons and axes every frame; it keeps what is held (the move stick, jump, crouch,
//! run), turns the view with the right stick, and says which actions were pressed this frame.
//!
//! The actions are this module's fixed vocabulary ([`PlayAction`]); the button each one is on, the
//! sticks and the tunables are data (`shared/input/gamepad.json`, generated into
//! `gamepad.gen.rs`), and a player rebinds them in Settings ([`Gamepad::bind`]). Menus are the
//! shell's (`client/src/input/gamepadMenu.ts`): while one is open, nothing here acts, but the
//! buttons are still tracked so that one held over closing it doesn't count as a press.

use crate::input::Look;
use crate::wrap_degrees;

/// A button of the standard mapping (data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Button {
    pub name: &'static str,
    pub label: &'static str,
    /// Its index in `Gamepad.buttons`.
    pub index: u8,
}

/// Which axes the sticks are (data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sticks {
    pub move_x: u8,
    pub move_y: u8,
    pub look_x: u8,
    pub look_y: u8,
}

/// The tunables (data; `gamepad.json` `tunables`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GamepadTunables {
    /// Stick travel (0–1) read as rest.
    pub deadzone: f64,
    /// The view's turn at full right-stick tilt (°/s), before the player's look speed.
    pub look_deg_per_s: f64,
    /// The tilt's response exponent.
    pub look_curve: f64,
    /// How far a trigger (an analog button) is pressed to count.
    pub trigger_threshold: f64,
    /// The shell's menu repeat (ms); not used here.
    pub menu_repeat_ms: f64,
    pub menu_repeat_delay_ms: f64,
}

mod table {
    include!("gamepad.gen.rs");
}
pub use table::{BUTTONS, PLAY, STICKS, TUNABLES};

/// What the controller does while playing: held (jump, crouch, run) or pressed (the rest).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayAction {
    Jump,
    Crouch,
    Run,
    Break,
    Place,
    HotbarNext,
    HotbarPrev,
    Fly,
    FlyFaster,
    FlySlower,
    Spyglass,
    Map,
    Pause,
}

impl PlayAction {
    /// Every action, in the data's order (`gamepad.gen.rs` `PLAY`).
    pub const ALL: [PlayAction; 13] = [
        Self::Jump,
        Self::Crouch,
        Self::Run,
        Self::Break,
        Self::Place,
        Self::HotbarNext,
        Self::HotbarPrev,
        Self::Fly,
        Self::FlyFaster,
        Self::FlySlower,
        Self::Spyglass,
        Self::Map,
        Self::Pause,
    ];

    /// Its name in the data and in Settings.
    pub fn name(self) -> &'static str {
        PLAY[self as usize].0
    }

    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.name() == name)
    }

    /// Held for as long as its button is (else it acts on the press).
    fn held(self) -> bool {
        matches!(self, Self::Jump | Self::Crouch | Self::Run)
    }
}

/// The button named `name` (data).
pub fn button(name: &str) -> Option<Button> {
    BUTTONS.iter().copied().find(|b| b.name == name)
}

/// One controller's state.
#[derive(Clone, Debug)]
pub struct Gamepad {
    /// Each action's button index (`PlayAction` order).
    bindings: [u8; PlayAction::ALL.len()],
    pub tunables: GamepadTunables,
    /// The player's look speed (× `look_deg_per_s`) and whether up on the stick looks down.
    pub look_speed: f64,
    pub invert_y: bool,
    /// The buttons pressed at the last frame (bit per index).
    down: u32,
    held: [bool; PlayAction::ALL.len()],
    run_latched: bool,
    move_xy: [f64; 2],
    /// A controller sent a frame since the last `disconnect`.
    pub connected: bool,
}

impl Default for Gamepad {
    fn default() -> Self {
        let mut g = Self {
            bindings: [0; PlayAction::ALL.len()],
            tunables: TUNABLES,
            look_speed: 1.0,
            invert_y: false,
            down: 0,
            held: [false; PlayAction::ALL.len()],
            run_latched: false,
            move_xy: [0.0; 2],
            connected: false,
        };
        g.reset_bindings();
        g
    }
}

impl Gamepad {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every action back on its default button (the data's).
    pub fn reset_bindings(&mut self) {
        for (i, (_, b)) in PLAY.iter().enumerate() {
            self.bindings[i] = button(b).map_or(0, |b| b.index);
        }
    }

    /// Puts `action` on button `button_name` (a player's rebinding); the action that had that
    /// button takes `action`'s old one, so no two share a button. False for an unknown name.
    pub fn bind(&mut self, action: &str, button_name: &str) -> bool {
        let (Some(a), Some(b)) = (PlayAction::by_name(action), button(button_name)) else {
            return false;
        };
        let old = self.bindings[a as usize];
        if let Some(other) = self.bindings.iter().position(|&i| i == b.index) {
            self.bindings[other] = old;
        }
        self.bindings[a as usize] = b.index;
        true
    }

    /// The button `action` is on.
    pub fn binding(&self, action: PlayAction) -> Button {
        let i = self.bindings[action as usize];
        BUTTONS
            .iter()
            .copied()
            .find(|b| b.index == i)
            .unwrap_or(BUTTONS[0])
    }

    /// The controller went away: nothing is held.
    pub fn disconnect(&mut self) {
        self.down = 0;
        self.held = [false; PlayAction::ALL.len()];
        self.run_latched = false;
        self.move_xy = [0.0; 2];
        self.connected = false;
    }

    /// One frame: `buttons` (values 0–1) and `axes` (−1–1) of the standard mapping, `dt` seconds
    /// since the last. While `playing` (no menu open) the right stick turns `look` (by `scale`, the
    /// spyglass's) and the actions pressed this frame are returned; otherwise nothing acts and
    /// nothing is held, but the buttons are tracked.
    pub fn frame(
        &mut self,
        buttons: &[f32],
        axes: &[f32],
        dt: f64,
        playing: bool,
        look: &mut Look,
        scale: f64,
    ) -> Vec<PlayAction> {
        self.connected = true;
        let t = self.tunables;
        let pressed = |i: u8| {
            buttons
                .get(usize::from(i))
                .is_some_and(|&v| f64::from(v) >= t.trigger_threshold)
        };
        let mut down = 0u32;
        for b in BUTTONS {
            if pressed(b.index) {
                down |= 1 << b.index;
            }
        }
        let was = self.down;
        self.down = down;
        if !playing {
            self.held = [false; PlayAction::ALL.len()];
            self.run_latched = false;
            self.move_xy = [0.0; 2];
            return Vec::new();
        }
        let axis = |i: u8| axes.get(usize::from(i)).map_or(0.0, |&v| f64::from(v));
        let mv = radial([axis(STICKS.move_x), axis(STICKS.move_y)], t.deadzone);
        // The stick's up is −Y; forward is +move_y.
        self.move_xy = [mv[0], -mv[1]];
        let lk = radial([axis(STICKS.look_x), axis(STICKS.look_y)], t.deadzone);
        let tilt = lk[0].hypot(lk[1]);
        if tilt > 0.0 {
            let rate = t.look_deg_per_s * self.look_speed * tilt.powf(t.look_curve) / tilt;
            let turn = lk[0] * rate * dt * scale;
            let up = lk[1] * rate * dt * scale * if self.invert_y { -1.0 } else { 1.0 };
            // As the mouse: right turns right (decreasing yaw on face A), up on the stick (−Y)
            // looks up.
            look.yaw = wrap_degrees(look.yaw - look.face.handed(turn));
            look.pitch = (look.pitch - up).clamp(-89.0, 89.0);
        }
        let mut events = Vec::new();
        for a in PlayAction::ALL {
            let bit = 1u32 << self.bindings[a as usize];
            let now = down & bit != 0;
            if a.held() {
                self.held[a as usize] = now;
            } else if now && was & bit == 0 {
                events.push(a);
            }
        }
        // Run latches on its press and lets go when the stick does.
        let run_bit = 1u32 << self.bindings[PlayAction::Run as usize];
        if down & run_bit != 0 && was & run_bit == 0 {
            self.run_latched = true;
        }
        if self.move_xy == [0.0; 2] {
            self.run_latched = false;
        }
        events
    }

    /// The move stick (right, forward), each −1–1, past the dead zone.
    pub fn move_xy(&self) -> [f64; 2] {
        self.move_xy
    }

    pub fn jump(&self) -> bool {
        self.held[PlayAction::Jump as usize]
    }

    pub fn crouch(&self) -> bool {
        self.held[PlayAction::Crouch as usize]
    }

    pub fn run(&self) -> bool {
        self.run_latched || self.held[PlayAction::Run as usize]
    }
}

/// A stick's reading with a radial dead zone, rescaled so its edge reads 0 and full tilt 1.
fn radial([x, y]: [f64; 2], deadzone: f64) -> [f64; 2] {
    let m = x.hypot(y);
    if m <= deadzone || m == 0.0 {
        return [0.0, 0.0];
    }
    let k = ((m - deadzone) / (1.0 - deadzone)).min(1.0) / m;
    [x * k, y * k]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(name: &str) -> usize {
        usize::from(button(name).expect("a button").index)
    }

    fn pad_frame(held: &[&str], axes: [f32; 4]) -> (Vec<f32>, [f32; 4]) {
        let mut b = vec![0.0; 17];
        for n in held {
            b[index(n)] = 1.0;
        }
        (b, axes)
    }

    #[test]
    fn every_action_has_its_default_button_from_the_data() {
        let g = Gamepad::new();
        for (i, a) in PlayAction::ALL.iter().enumerate() {
            assert_eq!(a.name(), PLAY[i].0);
            assert_eq!(g.binding(*a).name, PLAY[i].1, "{a:?}");
            assert_eq!(PlayAction::by_name(a.name()), Some(*a));
        }
    }

    #[test]
    fn presses_act_once_and_held_buttons_hold() {
        let mut g = Gamepad::new();
        let mut look = Look::default();
        let jump = g.binding(PlayAction::Jump).name;
        let brk = g.binding(PlayAction::Break).name;
        let (b, a) = pad_frame(&[jump, brk], [0.0; 4]);
        assert_eq!(
            g.frame(&b, &a, 0.016, true, &mut look, 1.0),
            vec![PlayAction::Break]
        );
        assert!(g.jump());
        // Still down: no second break.
        assert!(g.frame(&b, &a, 0.016, true, &mut look, 1.0).is_empty());
        let (b, a) = pad_frame(&[], [0.0; 4]);
        g.frame(&b, &a, 0.016, true, &mut look, 1.0);
        assert!(!g.jump());
    }

    #[test]
    fn a_trigger_counts_past_its_threshold() {
        let mut g = Gamepad::new();
        let mut look = Look::default();
        let rt = index(g.binding(PlayAction::Break).name);
        let mut b = vec![0.0; 17];
        b[rt] = (g.tunables.trigger_threshold * 0.5) as f32;
        assert!(
            g.frame(&b, &[0.0; 4], 0.016, true, &mut look, 1.0)
                .is_empty()
        );
        b[rt] = 0.9;
        assert_eq!(
            g.frame(&b, &[0.0; 4], 0.016, true, &mut look, 1.0),
            vec![PlayAction::Break]
        );
    }

    #[test]
    fn nothing_acts_while_a_menu_is_open_nor_on_closing_it_with_a_button_held() {
        let mut g = Gamepad::new();
        let mut look = Look::default();
        let place = g.binding(PlayAction::Place).name;
        let (b, a) = pad_frame(&[place, "a"], [1.0, 0.0, 1.0, 0.0]);
        assert!(g.frame(&b, &a, 0.1, false, &mut look, 1.0).is_empty());
        assert_eq!((look.yaw, g.move_xy(), g.jump()), (0.0, [0.0, 0.0], false));
        // The menu closes with it still held: not a press.
        assert!(g.frame(&b, &a, 0.1, true, &mut look, 1.0).is_empty());
    }

    #[test]
    fn the_sticks_have_a_dead_zone_and_reach_full_tilt() {
        let mut g = Gamepad::new();
        let mut look = Look::default();
        let dz = g.tunables.deadzone as f32;
        g.frame(
            &[0.0; 17],
            &[dz * 0.9, 0.0, 0.0, 0.0],
            0.016,
            true,
            &mut look,
            1.0,
        );
        assert_eq!(g.move_xy(), [0.0, 0.0]);
        g.frame(
            &[0.0; 17],
            &[0.0, -1.0, 0.0, 0.0],
            0.016,
            true,
            &mut look,
            1.0,
        );
        let [x, y] = g.move_xy();
        assert!(
            x.abs() < 1e-9 && (y - 1.0).abs() < 1e-9,
            "stick up is forward: {y}"
        );
    }

    dwell_world::both_faces!(
        /// The right stick turns the view as the mouse does: right turns right (the face's
        /// handedness), up looks up, at `look_deg_per_s` × the look speed at full tilt; inverted Y
        /// looks down.
        fn the_right_stick_turns_the_view_like_the_mouse(f: FaceFrame) {
            let mut g = Gamepad::new();
            let mut look = Look {
                face: f,
                ..Look::default()
            };
            let mut mouse = crate::input::KeyboardMouseInput::new();
            mouse.look.face = f;
            mouse.pointer_locked = true;
            mouse.mouse_move(10.0, -10.0);
            g.frame(
                &[0.0; 17],
                &[0.0, 0.0, 1.0, -1.0],
                0.1,
                true,
                &mut look,
                1.0,
            );
            assert_eq!(
                look.yaw.signum(),
                mouse.look.yaw.signum(),
                "right turns right"
            );
            assert!(look.pitch > 0.0, "up looks up: {}", look.pitch);
            // Full tilt right for 1 s: look_deg_per_s.
            let mut look = Look {
                face: f,
                ..Look::default()
            };
            g.frame(&[0.0; 17], &[0.0, 0.0, 1.0, 0.0], 1.0, true, &mut look, 1.0);
            let want = wrap_degrees(-f.handed(g.tunables.look_deg_per_s));
            assert!((look.yaw - want).abs() < 1e-6, "{} {want}", look.yaw);
            g.invert_y = true;
            let mut look = Look {
                face: f,
                ..Look::default()
            };
            g.frame(
                &[0.0; 17],
                &[0.0, 0.0, 0.0, -1.0],
                0.1,
                true,
                &mut look,
                1.0,
            );
            assert!(look.pitch < 0.0);
        }
    );

    #[test]
    fn run_latches_until_the_stick_lets_go() {
        let mut g = Gamepad::new();
        let mut look = Look::default();
        let run = g.binding(PlayAction::Run).name;
        let (b, _) = pad_frame(&[run], [0.0; 4]);
        g.frame(&b, &[0.0, -1.0, 0.0, 0.0], 0.016, true, &mut look, 1.0);
        g.frame(
            &[0.0; 17],
            &[0.0, -1.0, 0.0, 0.0],
            0.016,
            true,
            &mut look,
            1.0,
        );
        assert!(g.run(), "still running after the press");
        g.frame(&[0.0; 17], &[0.0; 4], 0.016, true, &mut look, 1.0);
        assert!(!g.run(), "the stick let go");
    }

    #[test]
    fn rebinding_swaps_with_the_action_that_had_the_button() {
        let mut g = Gamepad::new();
        let jump = g.binding(PlayAction::Jump);
        let fly = g.binding(PlayAction::Fly);
        assert!(g.bind("jump", fly.name));
        assert_eq!(g.binding(PlayAction::Jump), fly);
        assert_eq!(g.binding(PlayAction::Fly), jump);
        assert!(!g.bind("dance", "a") && !g.bind("jump", "nope"));
        g.reset_bindings();
        assert_eq!(g.binding(PlayAction::Jump), jump);
    }
}
