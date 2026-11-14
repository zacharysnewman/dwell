//! How much of an effect's particles are drawn (Graphics → Effects, WORK_QUEUE item 30: "Ensure
//! all of our graphics have settings in our graphics menu"): Hit shards for a pickaxe hit's flying
//! fragments (crate::hit_shards), Particles for the chips, sparks, falling leaves and steam puffs
//! (crate::chips, crate::steam). A share is 0–1 of the data's count, after the adapter's own tier
//! (a CPU adapter's third, a phone's half).

/// A share of an effect's count, 0–1 (1: all of it, the default; 0: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Share(f64);

impl Default for Share {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Share {
    /// The setting's value, clamped to 0–1; anything not a number is all of it.
    pub fn new(v: f64) -> Self {
        Self(if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            1.0
        })
    }

    pub fn get(self) -> f64 {
        self.0
    }

    /// `n` at this share: rounded, at least one while the share is above 0 and `n` is, none at 0.
    pub fn of(self, n: usize) -> usize {
        if n == 0 || self.0 <= 0.0 {
            0
        } else {
            ((n as f64 * self.0).round() as usize).clamp(1, n)
        }
    }

    /// Whether the `i`th of a run of items is kept at this share (spread evenly along the run).
    pub fn keeps(self, i: usize) -> bool {
        let s = self.0;
        s >= 1.0 || (s > 0.0 && ((i + 1) as f64 * s).floor() > (i as f64 * s).floor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_scales_a_count_and_keeps_one_while_above_zero() {
        assert_eq!(Share::default().of(12), 12);
        assert_eq!(Share::new(0.5).of(12), 6);
        assert_eq!(Share::new(0.01).of(12), 1);
        assert_eq!(Share::new(0.0).of(12), 0);
        assert_eq!(Share::new(2.0).of(12), 12);
        assert_eq!(Share::new(f64::NAN).of(12), 12);
        assert_eq!(Share::new(0.5).of(0), 0);
    }

    #[test]
    fn a_share_keeps_its_part_of_a_run() {
        let kept = |s: f64| (0..100).filter(|&i| Share::new(s).keeps(i)).count();
        assert_eq!(kept(1.0), 100);
        assert_eq!(kept(0.0), 0);
        assert_eq!(kept(0.25), 25);
        assert_eq!(kept(0.5), 50);
    }
}
