//! Input sampling (PLAYER_CONTROLLER.md §8.4) as a platform-free state machine: the shell's DOM
//! listeners feed it key, mouse and wheel events and it keeps the held keys, the view angles and the
//! flight toggle, and says what each event asks for (debug toggles, hotbar, break/place). The
//! client quantizes each tick's input exactly once with [`quantize_input`] (`dwell-physics`'s
//! quantizer, the same integers the server and the prediction dequantize).

use std::collections::HashSet;

use dwell_physics::controller::Input;
use dwell_protocol::InputFrame;

pub use dwell_physics::net::{dequantize_pitch, dequantize_yaw, quantize_pitch, quantize_yaw};

use crate::flight::FlightToggle;
use crate::flight_speed::clamp_fly_speed_level;
use crate::touch::TouchState;
use crate::wrap_degrees;

/// One tick of player input before quantization.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerInputState {
    /// Right.
    pub move_x: f64,
    /// Forward.
    pub move_y: f64,
    /// Degrees; 0 = +Z, 90 = +X.
    pub yaw: f64,
    /// Degrees, positive up.
    pub pitch: f64,
    pub jump: bool,
    pub run: bool,
    pub crouch: bool,
    /// Creative flight is on (PLAYER_CONTROLLER.md §6.7): held as a mode bit every tick.
    pub fly: bool,
    /// Flight speed level, 0 (normal) to `MAX_FLY_SPEED_LEVEL` (clamped when quantized).
    pub fly_speed: f64,
}

impl PlayerInputState {
    pub const IDLE: Self = Self {
        move_x: 0.0,
        move_y: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        jump: false,
        run: false,
        crouch: false,
        fly: false,
        fly_speed: 0.0,
    };

    /// The controller's input (before quantization).
    pub fn to_input(&self) -> Input {
        Input {
            move_x: self.move_x as f32,
            move_y: self.move_y as f32,
            look_yaw: self.yaw as f32,
            look_pitch: self.pitch as f32,
            jump: self.jump,
            run: self.run,
            crouch: self.crouch,
            fly: self.fly,
            fly_speed: clamp_fly_speed_level(self.fly_speed),
        }
    }
}

impl Default for PlayerInputState {
    fn default() -> Self {
        Self::IDLE
    }
}

/// Quantizes one tick's input (`dwell_physics::net::quantize_input`): move clamped to the unit
/// circle (the server rejects more), the speed level clamped into range.
pub fn quantize_input(input: &PlayerInputState, seq: u32) -> InputFrame {
    dwell_physics::net::quantize_input(&input.to_input(), seq)
}

/// Block interaction actions (§6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditAction {
    Break,
    Place,
}

/// What a key press asks of the shell, besides the held key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Debug toggles: F3 overlay, F4 terrain map, F6; Minus and Equal zoom the map.
    Toggle(String),
    /// Hotbar: a number key (its code, e.g. `Digit3`).
    Digit(String),
    /// Flight turned on or off (double-tapped Space).
    FlightChanged(bool),
}

/// A key press's results: whether the shell should `preventDefault()` it, and what it asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyDown {
    pub prevent_default: bool,
    pub events: Vec<InputEvent>,
}

/// The view angles (degrees), turned by the mouse and the touch look area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub yaw: f64,
    pub pitch: f64,
    /// +1 on face A; −1 on face B, where the player is upside down and turning right is turning the
    /// other way round in the world (BIFACIAL_WORLD.md §6): the heading's sign is the face's.
    pub turn_sign: f64,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            turn_sign: 1.0,
        }
    }
}

/// Keyboard + pointer-lock mouse look. WASD move, Space jump, Shift run, C / Ctrl crouch, double-tap
/// Space to fly (up with Space, down with crouch); left and right click break and place, number keys
/// and the wheel pick a block (§6.5).
#[derive(Clone, Debug)]
pub struct KeyboardMouseInput {
    keys: HashSet<String>,
    pub look: Look,
    /// Degrees per pixel of mouse movement.
    pub sensitivity: f64,
    /// Creative flight, toggled by double-tapping Space (or the touch Fly button).
    pub flight: FlightToggle,
    /// The pointer is locked to the game view (the shell tracks `pointerLockElement`).
    pub pointer_locked: bool,
}

impl Default for KeyboardMouseInput {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            look: Look::default(),
            sensitivity: 0.12,
            flight: FlightToggle::new(),
            pointer_locked: false,
        }
    }
}

impl KeyboardMouseInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Presses or releases a key programmatically (tests and automation).
    pub fn set_key(&mut self, code: &str, down: bool) {
        if down {
            self.keys.insert(code.to_owned());
        } else {
            self.keys.remove(code);
        }
    }

    pub fn key_held(&self, code: &str) -> bool {
        self.keys.contains(code)
    }

    /// This tick's input, with the on-screen touch controls (if any) merged in.
    pub fn sample(&self, touch: Option<&TouchState>) -> PlayerInputState {
        let k = |code: &str| self.keys.contains(code);
        let b = |v: bool| if v { 1.0 } else { 0.0 };
        PlayerInputState {
            move_x: (b(k("KeyD")) - b(k("KeyA")) + touch.map_or(0.0, |t| t.move_x))
                .clamp(-1.0, 1.0),
            move_y: (b(k("KeyW")) - b(k("KeyS")) + touch.map_or(0.0, |t| t.move_y))
                .clamp(-1.0, 1.0),
            yaw: self.look.yaw,
            pitch: self.look.pitch,
            jump: k("Space") || touch.is_some_and(|t| t.jump),
            run: k("ShiftLeft") || k("ShiftRight") || touch.is_some_and(|t| t.run),
            crouch: k("KeyC") || k("ControlLeft") || touch.is_some_and(|t| t.crouch),
            fly: self.flight.flying(),
            fly_speed: f64::from(self.flight.speed_level()),
        }
    }

    /// A `keydown`: `code` is `KeyboardEvent.code`, `time_stamp_ms` its `timeStamp`.
    pub fn key_down(
        &mut self,
        code: &str,
        repeat: bool,
        ctrl: bool,
        meta: bool,
        time_stamp_ms: f64,
    ) -> KeyDown {
        if matches!(code, "F3" | "F4" | "F6") {
            return KeyDown {
                prevent_default: true,
                events: vec![InputEvent::Toggle(code.to_owned())],
            };
        }
        // Map zoom (F4): the minus and equals keys, unless the browser's own zoom is meant.
        if matches!(code, "Minus" | "Equal") && !ctrl && !meta {
            return KeyDown {
                prevent_default: false,
                events: vec![InputEvent::Toggle(code.to_owned())],
            };
        }
        self.keys.insert(code.to_owned());
        let mut events = Vec::new();
        if code == "Space"
            && !repeat
            && let Some(on) = self.flight.jump_pressed(time_stamp_ms)
        {
            events.push(InputEvent::FlightChanged(on));
        }
        if code.starts_with("Digit") && !repeat {
            events.push(InputEvent::Digit(code.to_owned()));
        }
        KeyDown {
            prevent_default: code == "Space" || code.starts_with("Arrow"),
            events,
        }
    }

    pub fn key_up(&mut self, code: &str) {
        self.keys.remove(code);
    }

    /// The window lost focus: every key is released.
    pub fn blur(&mut self) {
        self.keys.clear();
    }

    /// A `mousedown` (`MouseEvent.button`). The first click only locks the pointer (the shell
    /// requests the lock); it doesn't break anything.
    pub fn mouse_down(&self, button: i16) -> Option<EditAction> {
        if !self.pointer_locked {
            return None;
        }
        match button {
            0 => Some(EditAction::Break),
            2 => Some(EditAction::Place),
            _ => None,
        }
    }

    /// A `wheel` event: the hotbar step (+1 next slot, −1 previous), if any.
    pub fn wheel(&self, delta_y: f64) -> Option<i32> {
        (self.pointer_locked && delta_y != 0.0).then_some(if delta_y > 0.0 { 1 } else { -1 })
    }

    /// A `mousemove` (`movementX`, `movementY`) while the pointer is locked turns the view.
    pub fn mouse_move(&mut self, movement_x: f64, movement_y: f64) {
        if !self.pointer_locked {
            return;
        }
        // Right-handed, Y up: facing +Z (yaw 0), right is −X, so turning right decreases yaw (and on
        // face B, with the head down, increases it).
        self.look.yaw =
            wrap_degrees(self.look.yaw - self.look.turn_sign * movement_x * self.sensitivity);
        self.look.pitch = (self.look.pitch - movement_y * self.sensitivity).clamp(-89.0, 89.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::input_buttons;

    #[test]
    fn quantizes_moves_buttons_and_view_angles() {
        let f = quantize_input(
            &PlayerInputState {
                move_y: 1.0,
                move_x: -0.5,
                jump: true,
                crouch: true,
                yaw: 90.0,
                pitch: -45.0,
                ..PlayerInputState::IDLE
            },
            7,
        );
        assert_eq!(f.seq, 7);
        // (−0.5, 1) is longer than 1: clamped to the unit circle first.
        let len = 0.5f64.hypot(1.0);
        assert_eq!(
            i32::from(f.move_x),
            (-0.5 / len * 127.0 + 0.5).floor() as i32
        );
        assert_eq!(
            i32::from(f.move_y),
            (1.0 / len * 127.0 + 0.5).floor() as i32
        );
        let (mx, my) = (i32::from(f.move_x), i32::from(f.move_y));
        assert!(mx * mx + my * my <= 127 * 127 + 254);
        assert_eq!(f.buttons, input_buttons::JUMP | input_buttons::CROUCH);
        assert_eq!(f.yaw, 16384);
        assert_eq!(f.pitch, -16383); // −16383.5 rounds half up, like Math.round
    }

    #[test]
    fn wraps_yaw_into_int16_and_clamps_pitch() {
        assert_eq!(quantize_yaw(180.0), -32768);
        assert_eq!(quantize_yaw(270.0), -16384);
        assert_eq!(quantize_yaw(-90.0), -16384);
        assert_eq!(quantize_yaw(360.0), 0);
        assert_eq!(quantize_pitch(120.0), 32767);
        assert_eq!(quantize_pitch(-120.0), -32767);
        assert_eq!(dequantize_yaw(16384), 90.0);
        assert_eq!(dequantize_pitch(32767), 90.0);
    }

    // Beyond the TS suite (whose listeners needed a DOM): the state machine the listeners feed.
    #[test]
    fn maps_keys_and_merges_touch() {
        let mut input = KeyboardMouseInput::new();
        input.key_down("KeyW", false, false, false, 0.0);
        input.key_down("KeyA", false, false, false, 0.0);
        input.key_down("ShiftLeft", false, false, false, 0.0);
        let s = input.sample(None);
        assert_eq!((s.move_x, s.move_y, s.run), (-1.0, 1.0, true));
        let touch = TouchState {
            move_x: 0.5,
            crouch: true,
            ..TouchState::default()
        };
        let s = input.sample(Some(&touch));
        assert_eq!((s.move_x, s.crouch), (-0.5, true));
        input.key_up("KeyW");
        assert_eq!(input.sample(None).move_y, 0.0);
        input.blur();
        assert_eq!(input.sample(None), PlayerInputState::IDLE);
    }

    #[test]
    fn reports_toggles_digits_and_double_tap_flight() {
        let mut input = KeyboardMouseInput::new();
        let f3 = input.key_down("F3", false, false, false, 0.0);
        assert!(f3.prevent_default);
        assert_eq!(f3.events, vec![InputEvent::Toggle("F3".into())]);
        assert!(!input.key_held("F3"));
        assert!(
            input
                .key_down("Minus", false, true, false, 0.0)
                .events
                .is_empty()
        );
        assert_eq!(
            input.key_down("Digit3", false, false, false, 0.0).events,
            vec![InputEvent::Digit("Digit3".into())]
        );
        assert!(
            input
                .key_down("Space", false, false, false, 100.0)
                .prevent_default
        );
        input.key_up("Space");
        // A key repeat does not count as the second press.
        assert!(
            input
                .key_down("Space", true, false, false, 150.0)
                .events
                .is_empty()
        );
        assert_eq!(
            input.key_down("Space", false, false, false, 200.0).events,
            vec![InputEvent::FlightChanged(true)]
        );
        assert!(input.sample(None).fly);
    }

    #[test]
    fn looks_and_clicks_only_while_the_pointer_is_locked() {
        let mut input = KeyboardMouseInput::new();
        input.mouse_move(100.0, 0.0);
        assert_eq!(input.mouse_down(0), None);
        assert_eq!(input.wheel(3.0), None);
        input.pointer_locked = true;
        input.mouse_move(100.0, -1000.0);
        assert!((input.look.yaw - 348.0).abs() < 1e-9); // turning right decreases yaw
        assert_eq!(input.look.pitch, 89.0);
        input.look.turn_sign = -1.0;
        input.mouse_move(100.0, 0.0);
        assert!(input.look.yaw.abs() < 1e-9);
        assert_eq!(input.mouse_down(0), Some(EditAction::Break));
        assert_eq!(input.mouse_down(2), Some(EditAction::Place));
        assert_eq!(input.wheel(-0.5), Some(-1));
    }
}
