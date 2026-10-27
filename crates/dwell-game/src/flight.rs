//! Creative flight toggle (PLAYER_CONTROLLER.md §6.7). Flight is a mode the client holds, turned on
//! and off by the fly key (`input::FLY_KEY`) or the touch Fly button: while it is on, every input frame carries the fly bit, and the shared controller flies the player. The
//! server clears the bit for players its flight policy excludes; Welcome says whether this one may.
//! Where the TS raised `onChange` / `onSpeedChange`, the methods here return the new value when it
//! changed.

use crate::flight_speed::clamp_fly_speed_level;

#[derive(Clone, Debug)]
pub struct FlightToggle {
    /// The server lets this player fly (Welcome's flight flag).
    may_fly: bool,
    on: bool,
    level: u8,
}

impl Default for FlightToggle {
    fn default() -> Self {
        Self {
            may_fly: true,
            on: false,
            level: 0,
        }
    }
}

impl FlightToggle {
    pub fn new() -> Self {
        Self::default()
    }

    /// The flight speed level sent with every input (0: normal).
    pub fn speed_level(&self) -> u8 {
        self.level
    }

    /// Sets the speed level (clamped); returns the new level if it changed.
    pub fn set_speed_level(&mut self, level: f64) -> Option<u8> {
        let l = clamp_fly_speed_level(level);
        if l == self.level {
            return None;
        }
        self.level = l;
        Some(l)
    }

    pub fn flying(&self) -> bool {
        self.may_fly && self.on
    }

    pub fn allowed(&self) -> bool {
        self.may_fly
    }

    /// Sets whether the server allows flight; returns the new flying state if it changed.
    pub fn set_allowed(&mut self, v: bool) -> Option<bool> {
        let was = self.flying();
        self.may_fly = v;
        self.changed(was)
    }

    /// Turns flight on or off; returns the new flying state if it changed.
    pub fn set(&mut self, on: bool) -> Option<bool> {
        let was = self.flying();
        self.on = on && self.may_fly;
        self.changed(was)
    }

    pub fn toggle(&mut self) -> Option<bool> {
        self.set(!self.on)
    }

    fn changed(&self, was: bool) -> Option<bool> {
        (self.flying() != was).then_some(self.flying())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{PlayerInputState, quantize_input};
    use dwell_protocol::{input_buttons, players};

    #[test]
    fn toggling_turns_flight_on_and_off_and_reports_each_change() {
        let mut f = FlightToggle::new();
        let mut changes = Vec::new();
        for _ in 0..3 {
            changes.extend(f.toggle());
        }
        assert!(f.flying());
        assert_eq!(changes, vec![true, false, true]);
        assert_eq!(f.set(true), None);
    }

    #[test]
    fn a_server_that_disallows_flight_turns_it_off_and_keeps_it_off() {
        let mut f = FlightToggle::new();
        f.toggle();
        assert!(f.flying());
        f.set_allowed(false);
        assert!(!f.flying());
        assert_eq!(f.toggle(), None);
        assert_eq!(f.set(true), None);
        assert!(!f.flying());
    }

    #[test]
    fn flying_sets_the_fly_bit_of_every_input_frame() {
        let fly = PlayerInputState {
            fly: true,
            ..PlayerInputState::IDLE
        };
        assert_eq!(quantize_input(&fly, 1).buttons, input_buttons::FLY);
        assert_eq!(quantize_input(&PlayerInputState::IDLE, 1).buttons, 0);
    }

    #[test]
    fn the_speed_level_rides_in_the_fly_speed_bits_capped_at_the_highest_level() {
        let input = |fly_speed: f64| PlayerInputState {
            fly: true,
            fly_speed,
            ..PlayerInputState::IDLE
        };
        let level = |fly_speed: f64| {
            i64::from(quantize_input(&input(fly_speed), 1).buttons & input_buttons::FLY_SPEED)
                >> players::FLY_SPEED_SHIFT
        };
        assert_eq!(level(0.0), 0);
        assert_eq!(level(7.0), 7);
        assert_eq!(level(99.0), players::FLY_SPEED_MAX_LEVEL);
        assert_eq!(level(-4.0), 0);
        assert_eq!(
            quantize_input(&input(3.0), 1).buttons & input_buttons::FLY,
            input_buttons::FLY
        );
    }

    #[test]
    fn keeps_the_speed_level_in_range_and_reports_changes_once() {
        let mut f = FlightToggle::new();
        let mut seen = Vec::new();
        for l in [5.0, 5.0, 1000.0, -1.0] {
            seen.extend(f.set_speed_level(l));
        }
        assert_eq!(seen, vec![5, players::FLY_SPEED_MAX_LEVEL as u8, 0]);
        assert_eq!(f.speed_level(), 0);
    }
}
