//! Touch controls for phones and tablets: a floating joystick on the left half of the screen,
//! drag-to-look on the right half, and Jump / Crouch / Run / Fly buttons. A pure state machine fed
//! pointer events (one captured pointer per control, so every control works at once with several
//! fingers); the shell builds the DOM, captures the pointers and draws what [`TouchControls`] says.

use crate::input::Look;
use crate::wrap_degrees;

/// Pixels from the stick's centre to full deflection.
pub const STICK_RADIUS: f64 = 56.0;
const STICK_DEADZONE: f64 = 0.12;
/// Dragging past this multiple of the radius runs (the ring lights up).
const STICK_RUN: f64 = 1.3;
/// Degrees of view rotation per pixel of drag.
pub const TOUCH_LOOK_SENSITIVITY: f64 = 0.3;
/// A touch on the view that moves less than this (px) and lifts within TAP_MS is a tap (§6.5).
pub const TAP_SLOP: f64 = 12.0;
pub const TAP_MS: f64 = 500.0;

/// Is a touch that moved (dx, dy) px in total and lasted `ms` a tap rather than a look drag?
pub fn is_tap(dx: f64, dy: f64, ms: f64) -> bool {
    dx.hypot(dy) <= TAP_SLOP && ms <= TAP_MS
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StickOutput {
    /// Right.
    pub move_x: f64,
    /// Forward (dragging up the screen).
    pub move_y: f64,
    pub run: bool,
}

/// Stick deflection from a drag of (dx, dy) screen pixels: clamped to the unit circle.
pub fn stick_output(dx: f64, dy: f64, radius: f64) -> StickOutput {
    let distance = dx.hypot(dy);
    let run = distance >= radius * STICK_RUN;
    let magnitude = (distance / radius).min(1.0);
    if magnitude < STICK_DEADZONE {
        return StickOutput::default();
    }
    // Rescale past the dead zone so small deflections still start from zero.
    let scaled = (magnitude - STICK_DEADZONE) / (1.0 - STICK_DEADZONE);
    StickOutput {
        move_x: dx / distance * scaled,
        move_y: -dy / distance * scaled,
        run,
    }
}

/// How a touch button reports its state: pressed while held, or latched on/off per tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchButtonMode {
    Hold,
    Toggle,
}

/// The on-screen buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TouchButton {
    /// What a tap on the view does: Break, or (latched) Place (§6.5).
    Edit,
    Run,
    /// Creative flight on/off (the flight toggle holds the state; the button shows it).
    Fly,
    /// The F3 debug overlay (phones have no function keys).
    Debug,
    Crouch,
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchButtonSpec {
    pub label: &'static str,
    pub id: &'static str,
    pub mode: TouchButtonMode,
}

impl TouchButton {
    /// Left to right (the debug button sits apart, top right).
    pub const ALL: [TouchButton; 6] = [
        TouchButton::Edit,
        TouchButton::Run,
        TouchButton::Fly,
        TouchButton::Debug,
        TouchButton::Crouch,
        TouchButton::Jump,
    ];

    pub fn spec(self) -> TouchButtonSpec {
        use TouchButtonMode::{Hold, Toggle};
        let (label, id, mode) = match self {
            TouchButton::Edit => ("Break", "touch-edit", Toggle),
            TouchButton::Run => ("Run", "touch-run", Toggle),
            TouchButton::Fly => ("Fly", "touch-fly", Hold),
            TouchButton::Debug => ("i", "touch-debug", Hold),
            TouchButton::Crouch => ("Crouch", "touch-crouch", Hold),
            TouchButton::Jump => ("Jump", "touch-jump", Hold),
        };
        TouchButtonSpec { label, id, mode }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A button's on/off state as pointers press and release it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchButtonState {
    pub active: bool,
    pub mode: TouchButtonMode,
}

impl TouchButtonState {
    pub fn new(mode: TouchButtonMode) -> Self {
        Self {
            active: false,
            mode,
        }
    }

    pub fn press(&mut self) -> bool {
        self.active = self.mode == TouchButtonMode::Hold || !self.active;
        self.active
    }

    pub fn release(&mut self) -> bool {
        if self.mode == TouchButtonMode::Hold {
            self.active = false;
        }
        self.active
    }
}

/// What touch contributes to the player's input each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchState {
    pub move_x: f64,
    pub move_y: f64,
    pub run: bool,
    pub jump: bool,
    pub crouch: bool,
}

/// What a touch event asks of the shell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchEvent {
    /// A tap on the view: break or place at the crosshair (§6.5).
    Tap,
    /// The Break/Place toggle changed: true = place.
    PlaceMode(bool),
    /// The Fly button was pressed (toggles flight).
    Fly,
    /// The debug button was pressed (toggles the overlay, as F3 does).
    Debug,
}

/// Where to draw the floating stick (screen px): its base and the knob's offset from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickView {
    pub origin: (f64, f64),
    pub knob: (f64, f64),
    /// Dragged past the ring: running.
    pub running: bool,
}

#[derive(Clone, Copy, Debug)]
struct LookPointer {
    id: i32,
    last: (f64, f64),
    start: (f64, f64, f64),
}

/// The touch controls' state: stick, look area and buttons.
#[derive(Clone, Debug)]
pub struct TouchControls {
    pub state: TouchState,
    stick_pointer: Option<i32>,
    stick_origin: (f64, f64),
    stick_offset: (f64, f64),
    look_pointer: Option<LookPointer>,
    buttons: [TouchButtonState; 6],
    run_latched: bool,
    stick_run: bool,
}

impl Default for TouchControls {
    fn default() -> Self {
        Self {
            state: TouchState::default(),
            stick_pointer: None,
            stick_origin: (0.0, 0.0),
            stick_offset: (0.0, 0.0),
            look_pointer: None,
            buttons: TouchButton::ALL.map(|b| TouchButtonState::new(b.spec().mode)),
            run_latched: false,
            stick_run: false,
        }
    }
}

impl TouchControls {
    pub fn new() -> Self {
        Self::default()
    }

    /// A pointer went down in the move zone; ignored while another finger holds the stick.
    pub fn stick_down(&mut self, pointer_id: i32, x: f64, y: f64) {
        if self.stick_pointer.is_some() {
            return;
        }
        self.stick_pointer = Some(pointer_id);
        self.stick_origin = (x, y);
        self.update_stick(0.0, 0.0);
    }

    pub fn stick_move(&mut self, pointer_id: i32, x: f64, y: f64) {
        if self.stick_pointer != Some(pointer_id) {
            return;
        }
        self.update_stick(x - self.stick_origin.0, y - self.stick_origin.1);
    }

    /// `pointerup` or `pointercancel` in the move zone.
    pub fn stick_up(&mut self, pointer_id: i32) {
        if self.stick_pointer != Some(pointer_id) {
            return;
        }
        self.stick_pointer = None;
        self.update_stick(0.0, 0.0);
    }

    /// The stick to draw, while a finger holds it.
    pub fn stick_view(&self) -> Option<StickView> {
        self.stick_pointer?;
        let (dx, dy) = self.stick_offset;
        let distance = dx.hypot(dy);
        let k = if distance > STICK_RADIUS {
            STICK_RADIUS / distance
        } else {
            1.0
        };
        Some(StickView {
            origin: self.stick_origin,
            knob: (dx * k, dy * k),
            running: self.stick_run,
        })
    }

    fn update_stick(&mut self, dx: f64, dy: f64) {
        let out = stick_output(dx, dy, STICK_RADIUS);
        self.state.move_x = out.move_x;
        self.state.move_y = out.move_y;
        self.stick_run = out.run;
        self.state.run = self.run_latched || self.stick_run;
        self.stick_offset = (dx, dy);
    }

    /// A pointer went down in the look zone at `time_ms` (`timeStamp`).
    pub fn look_down(&mut self, pointer_id: i32, x: f64, y: f64, time_ms: f64) {
        if self.look_pointer.is_some() {
            return;
        }
        self.look_pointer = Some(LookPointer {
            id: pointer_id,
            last: (x, y),
            start: (x, y, time_ms),
        });
    }

    /// Dragging in the look zone turns `look`.
    pub fn look_move(&mut self, pointer_id: i32, x: f64, y: f64, look: &mut Look) {
        let Some(p) = self.look_pointer.as_mut().filter(|p| p.id == pointer_id) else {
            return;
        };
        let (dx, dy) = (x - p.last.0, y - p.last.1);
        p.last = (x, y);
        // Right-handed, Y up: dragging right turns right, which decreases yaw (increases it upside down).
        look.yaw = wrap_degrees(look.yaw - look.turn_sign * dx * TOUCH_LOOK_SENSITIVITY);
        look.pitch = (look.pitch - dy * TOUCH_LOOK_SENSITIVITY).clamp(-89.0, 89.0);
    }

    /// The look finger lifted (`cancelled`: `pointercancel`) at `time_ms`; [`TouchEvent::Tap`] if
    /// it was a tap.
    pub fn look_up(
        &mut self,
        pointer_id: i32,
        time_ms: f64,
        cancelled: bool,
    ) -> Option<TouchEvent> {
        let p = self.look_pointer.filter(|p| p.id == pointer_id)?;
        self.look_pointer = None;
        let (sx, sy, st) = p.start;
        // Travel up to the last move (a lifted touch may not report where it ended).
        let (lx, ly) = p.last;
        (!cancelled && is_tap(lx - sx, ly - sy, time_ms - st)).then_some(TouchEvent::Tap)
    }

    /// Whether a button is drawn pressed.
    pub fn button_active(&self, button: TouchButton) -> bool {
        self.buttons[button.index()].active
    }

    /// The Break/Place button's caption.
    pub fn edit_label(&self) -> &'static str {
        if self.button_active(TouchButton::Edit) {
            "Place"
        } else {
            "Break"
        }
    }

    /// A pointer went down on a button.
    pub fn button_down(&mut self, button: TouchButton) -> Option<TouchEvent> {
        let on = self.buttons[button.index()].press();
        self.apply(button, on)
    }

    /// `pointerup` or `pointercancel` on a button.
    pub fn button_up(&mut self, button: TouchButton) -> Option<TouchEvent> {
        let on = self.buttons[button.index()].release();
        self.apply(button, on)
    }

    fn apply(&mut self, button: TouchButton, on: bool) -> Option<TouchEvent> {
        match button {
            TouchButton::Run => {
                self.run_latched = on;
                self.state.run = self.run_latched || self.stick_run;
                None
            }
            TouchButton::Crouch => {
                self.state.crouch = on;
                None
            }
            TouchButton::Jump => {
                self.state.jump = on;
                None
            }
            TouchButton::Fly => on.then_some(TouchEvent::Fly),
            TouchButton::Debug => on.then_some(TouchEvent::Debug),
            TouchButton::Edit => Some(TouchEvent::PlaceMode(on)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_close;

    fn stick(dx: f64, dy: f64) -> StickOutput {
        stick_output(dx, dy, STICK_RADIUS)
    }

    #[test]
    fn stick_maps_drags_to_a_move_vector_up_is_forward_right_is_right() {
        let up = stick(0.0, -STICK_RADIUS);
        assert_close(up.move_x, 0.0, 2);
        assert_close(up.move_y, 1.0, 2);
        let right = stick(STICK_RADIUS, 0.0);
        assert_close(right.move_x, 1.0, 2);
        assert_close(right.move_y, 0.0, 2);
    }

    #[test]
    fn stick_has_a_dead_zone_clamps_to_the_unit_circle_and_runs_past_the_ring() {
        assert_eq!(stick(3.0, 2.0), StickOutput::default());
        let far = stick(STICK_RADIUS * 2.0, -STICK_RADIUS * 2.0);
        assert_close(far.move_x.hypot(far.move_y), 1.0, 2);
        assert!(far.run);
        assert!(!stick(0.0, -STICK_RADIUS).run);
        let half = stick(0.0, -STICK_RADIUS / 2.0);
        assert!(half.move_y > 0.3);
        assert!(half.move_y < 0.5);
    }

    fn press_and_release(b: &mut TouchButtonState) -> [bool; 2] {
        [b.press(), b.release()]
    }

    #[test]
    fn crouch_is_held_not_toggled() {
        let mut crouch = TouchButtonState::new(TouchButton::Crouch.spec().mode);
        assert_eq!(press_and_release(&mut crouch), [true, false]);
        assert_eq!(press_and_release(&mut crouch), [true, false]);
    }

    #[test]
    fn jump_is_held_and_run_latches_on_and_off_per_tap() {
        let mut jump = TouchButtonState::new(TouchButton::Jump.spec().mode);
        assert_eq!(press_and_release(&mut jump), [true, false]);
        let mut run = TouchButtonState::new(TouchButton::Run.spec().mode);
        assert_eq!(press_and_release(&mut run), [true, true]);
        assert_eq!(press_and_release(&mut run), [false, false]);
    }

    #[test]
    fn the_break_place_button_latches() {
        let mut edit = TouchButtonState::new(TouchButton::Edit.spec().mode);
        assert_eq!(press_and_release(&mut edit), [true, true]);
        assert_eq!(press_and_release(&mut edit), [false, false]);
    }

    #[test]
    fn a_short_touch_that_barely_moves_is_a_tap_a_drag_or_a_long_press_is_not() {
        assert!(is_tap(0.0, 0.0, 80.0));
        assert!(is_tap(TAP_SLOP, 0.0, TAP_MS));
        assert!(!is_tap(TAP_SLOP + 1.0, 0.0, 80.0));
        assert!(!is_tap(3.0, 4.0, TAP_MS + 1.0));
    }

    // Beyond the TS suite (whose controls needed a DOM): the state machine the listeners feed.
    #[test]
    fn the_stick_follows_one_finger_and_latched_run_survives_its_release() {
        let mut t = TouchControls::new();
        t.stick_down(1, 100.0, 300.0);
        t.stick_down(2, 0.0, 0.0); // a second finger is ignored
        t.stick_move(2, 0.0, 0.0);
        t.stick_move(1, 100.0, 300.0 - STICK_RADIUS * 2.0);
        assert_close(t.state.move_y, 1.0, 6);
        assert!(t.state.run);
        let v = t.stick_view().unwrap();
        assert_eq!(v.origin, (100.0, 300.0));
        assert_close(v.knob.1, -STICK_RADIUS, 9);
        assert!(v.running);
        t.button_down(TouchButton::Run);
        t.button_up(TouchButton::Run);
        t.stick_up(1);
        assert_eq!(t.stick_view(), None);
        assert_eq!((t.state.move_y, t.state.run), (0.0, true));
    }

    #[test]
    fn look_drags_turn_the_view_and_taps_are_reported() {
        let mut t = TouchControls::new();
        let mut look = Look::default();
        t.look_down(5, 0.0, 0.0, 0.0);
        t.look_move(5, 10.0, -10.0, &mut look);
        assert_close(look.yaw, 357.0, 9);
        assert_close(look.pitch, 3.0, 9);
        assert_eq!(t.look_up(5, 100.0, false), None); // moved too far for a tap
        t.look_down(6, 0.0, 0.0, 0.0);
        assert_eq!(t.look_up(6, 100.0, false), Some(TouchEvent::Tap));
        t.look_down(7, 0.0, 0.0, 0.0);
        assert_eq!(t.look_up(7, 100.0, true), None);
    }

    #[test]
    fn buttons_report_fly_debug_and_place_mode() {
        let mut t = TouchControls::new();
        // Jump is held, and only held: two quick taps are two jumps, not flight (the Fly button
        // toggles it).
        for _ in 0..2 {
            assert_eq!(t.button_down(TouchButton::Jump), None);
            assert!(t.state.jump);
            assert_eq!(t.button_up(TouchButton::Jump), None);
        }
        assert_eq!(t.button_down(TouchButton::Jump), None);
        assert!(t.state.jump);
        assert_eq!(t.button_up(TouchButton::Jump), None);
        assert!(!t.state.jump);
        assert_eq!(t.button_down(TouchButton::Fly), Some(TouchEvent::Fly));
        assert_eq!(t.button_up(TouchButton::Fly), None);
        assert_eq!(t.button_down(TouchButton::Debug), Some(TouchEvent::Debug));
        assert_eq!(
            t.button_down(TouchButton::Edit),
            Some(TouchEvent::PlaceMode(true))
        );
        assert_eq!(t.edit_label(), "Place");
        t.button_down(TouchButton::Crouch);
        assert!(t.state.crouch && t.button_active(TouchButton::Crouch));
    }
}
