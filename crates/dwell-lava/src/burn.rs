//! A player's lava damage (owner, VOLCANOES.md Q1 default): heavy damage a second while in it and a
//! moment of burning after leaving, both the fluid's data. Whole points of health come out as they
//! accrue, for the server's `damage` (the vitals' drowning works the same way).

use crate::data::HotFluid;

/// A player's burn state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Burn {
    /// Burning left after leaving the lava (s).
    pub left_s: f32,
    /// Damage accrued but not yet a whole point.
    owed: f32,
}

impl Burn {
    /// One tick of `dt` seconds, in the lava or not; returns the whole points of damage due.
    pub fn step(&mut self, fluid: &HotFluid, in_lava: bool, dt: f32) -> i32 {
        if in_lava {
            self.owed += fluid.damage_per_second * dt;
            self.left_s = fluid.burn_seconds;
        } else if self.left_s > 0.0 {
            let t = dt.min(self.left_s);
            self.owed += fluid.burn_damage_per_second * t;
            self.left_s -= t;
            if self.left_s < 1e-4 {
                self.left_s = 0.0;
            }
        }
        let whole = self.owed.floor();
        self.owed -= whole;
        whole as i32
    }

    /// Whether the player is still burning (the clients' flames, later).
    pub fn burning(&self) -> bool {
        self.left_s > 0.0
    }
}
