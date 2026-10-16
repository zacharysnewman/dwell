//! Input sampling (PLAYER_CONTROLLER.md §8.4) as a platform-free state machine: the shell's DOM
//! listeners feed it key, mouse and wheel events and it keeps the held keys, the view angles and the
//! flight toggle, and says what each event asks for (debug toggles, hotbar, break/place). The
//! client quantizes each tick's input exactly once with [`quantize_input`] (`dwell-physics`'s
//! quantizer, the same integers the server and the prediction dequantize).

use std::collections::HashSet;

use dwell_physics::controller::Input;
use dwell_protocol::InputFrame;

pub use dwell_physics::net::{dequantize_pitch, dequantize_yaw, quantize_pitch, quantize_yaw};

use crate::brush_tool::{BrushAction, brush_key};
use crate::flight::FlightToggle;
use crate::flight_speed::clamp_fly_speed_level;
use crate::region_tools::{ToolAction, tool_for_key};
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

/// The key that turns creative flight on or off (`KeyboardEvent.code`), where the world allows it.
pub const FLY_KEY: &str = "KeyF";

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
    /// Flight turned on or off (the fly key).
    FlightChanged(bool),
    /// A region tool's key (`region_tools::tool_for_key`).
    Tool(ToolAction),
    /// A brush tool's key (`brush_tool::brush_key`).
    Brush(BrushAction),
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

/// Keyboard + pointer-lock mouse look. WASD move, Space jump, Shift run, C / Ctrl crouch, F
/// (`FLY_KEY`) to fly (up with Space, down with crouch); left and right click break and place, number keys
/// and the wheel pick a block (§6.5).
#[derive(Clone, Debug)]
pub struct KeyboardMouseInput {
    keys: HashSet<String>,
    pub look: Look,
    /// Degrees per pixel of mouse movement.
    pub sensitivity: f64,
    /// Creative flight, toggled by the fly key (or the touch Fly button).
    pub flight: FlightToggle,
    /// The pointer is locked to the game view (the shell tracks `pointerLockElement`).
    pub pointer_locked: bool,
    /// The world allows brushes (V turns the tool on), and the tool is on (its keys are live).
    pub brushes: bool,
    pub brush_active: bool,
}

impl Default for KeyboardMouseInput {
    fn default() -> Self {
        Self {
            keys: HashSet::new(),
            look: Look::default(),
            sensitivity: 0.12,
            flight: FlightToggle::new(),
            pointer_locked: false,
            brushes: false,
            brush_active: false,
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

    /// A `keydown`: `code` is `KeyboardEvent.code`.
    pub fn key_down(&mut self, code: &str, repeat: bool, ctrl: bool, meta: bool) -> KeyDown {
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
        // Region tools (§6.5), only while playing (the pointer locked: never while typing in a
        // menu). The key is not held: with ⌘ down, macOS sends no keyup for it.
        let shift = self.keys.contains("ShiftLeft") || self.keys.contains("ShiftRight");
        // The brush tool (BRUSHES.md), in worlds that allow it: V, and its keys while it is on.
        if self.pointer_locked
            && self.brushes
            && !ctrl
            && !meta
            && let Some(action) = brush_key(code, shift, self.brush_active)
        {
            return KeyDown {
                prevent_default: true,
                events: if repeat && action == BrushAction::Toggle {
                    Vec::new()
                } else {
                    vec![InputEvent::Brush(action)]
                },
            };
        }
        if self.pointer_locked
            && let Some(tool) = tool_for_key(code, ctrl || meta, shift)
        {
            return KeyDown {
                prevent_default: true,
                events: if repeat {
                    Vec::new()
                } else {
                    vec![InputEvent::Tool(tool)]
                },
            };
        }
        self.keys.insert(code.to_owned());
        let mut events = Vec::new();
        // The fly key, unless a browser shortcut (Ctrl/⌘+F, find) is meant.
        if code == FLY_KEY
            && !repeat
            && !ctrl
            && !meta
            && let Some(on) = self.flight.toggle()
        {
            events.push(InputEvent::FlightChanged(on));
        }
        if code.starts_with("Digit") && !repeat {
            events.push(InputEvent::Digit(code.to_owned()));
        }
        KeyDown {
            prevent_default: code == "Space"
                || code.starts_with("Arrow")
                || (code == FLY_KEY && !ctrl && !meta),
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
        input.key_down("KeyW", false, false, false);
        input.key_down("KeyA", false, false, false);
        input.key_down("ShiftLeft", false, false, false);
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
    fn reports_toggles_and_digits() {
        let mut input = KeyboardMouseInput::new();
        let f3 = input.key_down("F3", false, false, false);
        assert!(f3.prevent_default);
        assert_eq!(f3.events, vec![InputEvent::Toggle("F3".into())]);
        assert!(!input.key_held("F3"));
        assert!(
            input
                .key_down("Minus", false, true, false)
                .events
                .is_empty()
        );
        assert_eq!(
            input.key_down("Digit3", false, false, false).events,
            vec![InputEvent::Digit("Digit3".into())]
        );
        assert!(input.key_down("Space", false, false, false).prevent_default);
    }

    // Owner, 2026-10-15: flight has its own key; Jump pressed twice is two jumps, not flight.
    #[test]
    fn a_double_tapped_space_jumps_and_does_not_toggle_flight() {
        let mut input = KeyboardMouseInput::new();
        for press in 1..=4 {
            assert!(
                input
                    .key_down("Space", false, false, false)
                    .events
                    .is_empty(),
                "Space press {press} asked for something besides the jump"
            );
            assert!(input.sample(None).jump);
            input.key_up("Space");
        }
        assert!(!input.sample(None).fly);
        assert!(!input.flight.flying());
    }

    #[test]
    fn the_fly_key_toggles_flight_on_and_off() {
        let mut input = KeyboardMouseInput::new();
        let on = input.key_down(FLY_KEY, false, false, false);
        assert_eq!(on.events, vec![InputEvent::FlightChanged(true)]);
        assert!(on.prevent_default);
        assert!(input.sample(None).fly);
        input.key_up(FLY_KEY);
        // Held down, the key repeats: a repeat doesn't toggle it back.
        input.key_down(FLY_KEY, false, false, false);
        assert!(
            input
                .key_down(FLY_KEY, true, false, false)
                .events
                .is_empty()
        );
        assert!(!input.sample(None).fly);
        input.key_up(FLY_KEY);
        // While flying, Space and C climb and descend (the jump and crouch bits).
        input.key_down(FLY_KEY, false, false, false);
        input.key_down("Space", false, false, false);
        input.key_down("KeyC", false, false, false);
        let s = input.sample(None);
        assert!(s.fly && s.jump && s.crouch);
        // Ctrl/⌘+F is the browser's find, not flight.
        input.key_up(FLY_KEY);
        for (ctrl, meta) in [(true, false), (false, true)] {
            let find = input.key_down(FLY_KEY, false, ctrl, meta);
            assert!(find.events.is_empty() && !find.prevent_default);
            input.key_up(FLY_KEY);
        }
        assert!(input.sample(None).fly);
    }

    #[test]
    fn the_fly_key_is_refused_where_the_world_disallows_flight() {
        let mut input = KeyboardMouseInput::new();
        input.flight.set_allowed(false);
        for _ in 0..3 {
            assert!(
                input
                    .key_down(FLY_KEY, false, false, false)
                    .events
                    .is_empty()
            );
            input.key_up(FLY_KEY);
        }
        assert!(!input.sample(None).fly);
        // Allowed again (a new Welcome), the key works.
        input.flight.set_allowed(true);
        assert_eq!(
            input.key_down(FLY_KEY, false, false, false).events,
            vec![InputEvent::FlightChanged(true)]
        );
    }

    #[test]
    fn region_tool_keys_only_while_playing_and_command_keys_are_not_held() {
        let mut input = KeyboardMouseInput::new();
        // Not playing (a menu's text box has the keys): Backspace and Ctrl+C are the page's.
        assert!(
            !input
                .key_down("Backspace", false, false, false)
                .prevent_default
        );
        assert!(input.key_down("KeyC", false, true, false).events.is_empty());
        input.blur();
        input.pointer_locked = true;
        let g = input.key_down("KeyG", false, false, false);
        assert!(g.prevent_default);
        assert_eq!(g.events, vec![InputEvent::Tool(ToolAction::Fill)]);
        assert!(input.key_down("KeyG", true, false, false).events.is_empty());
        // ⌘C copies and doesn't crouch (macOS sends no keyup for C while ⌘ is down).
        assert_eq!(
            input.key_down("KeyC", false, false, true).events,
            vec![InputEvent::Tool(ToolAction::Copy)]
        );
        assert!(!input.sample(None).crouch);
        input.key_down("ShiftLeft", false, false, false);
        assert_eq!(
            input.key_down("KeyZ", false, true, false).events,
            vec![InputEvent::Tool(ToolAction::Redo)]
        );
        // C alone still crouches.
        input.key_down("KeyC", false, false, false);
        assert!(input.sample(None).crouch);
    }

    #[test]
    fn brush_keys_only_where_brushes_are_allowed_and_while_the_tool_is_on() {
        let mut input = KeyboardMouseInput::new();
        input.pointer_locked = true;
        // A world without brushes: V and E are not taken (nothing else binds them).
        assert!(
            input
                .key_down("KeyV", false, false, false, 0.0)
                .events
                .is_empty()
        );
        input.blur();
        input.brushes = true;
        assert_eq!(
            input.key_down("KeyV", false, false, false, 0.0).events,
            vec![InputEvent::Brush(BrushAction::Toggle)]
        );
        assert!(
            input
                .key_down("KeyE", false, false, false, 0.0)
                .events
                .is_empty()
        );
        input.blur();
        input.brush_active = true;
        assert_eq!(
            input.key_down("KeyE", false, false, false, 0.0).events,
            vec![InputEvent::Brush(BrushAction::NextPreset)]
        );
        // Held ] keeps growing the brush; Ctrl+Z is still undo, not the brush cycle.
        assert_eq!(
            input
                .key_down("BracketRight", true, false, false, 0.0)
                .events,
            vec![InputEvent::Brush(BrushAction::Bigger)]
        );
        assert_eq!(
            input.key_down("KeyZ", false, true, false, 0.0).events,
            vec![InputEvent::Tool(ToolAction::Undo)]
        );
        // Walking still walks.
        input.key_down("KeyW", false, false, false, 0.0);
        assert!(input.sample(None).move_y > 0.0);
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
