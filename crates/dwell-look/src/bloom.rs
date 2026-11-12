//! Bloom, the glow round self-lit surfaces (WORK_QUEUE item 19; docs/proposals/VOLCANOES.md §6.5):
//! the pixels the scene flags self-lit (lava, flames, lamps) above a threshold, blurred over a chain
//! of halving levels and added to the scene's linear light before the tone curve. Drawn by the
//! Bloom setting's tier (`shared/look/bloom.json`, generated into `bloom.gen.rs`).

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod generated {
    include!("bloom.gen.rs");
}
pub use generated::{BLOOM, HIGH, LOW, MAX_LEVELS};

/// The look: the bright pass's threshold (the scene's linear light, its brightest channel), the
/// glow's strength added back, and the up taps' spacing (texels of their level).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomLook {
    pub threshold: f64,
    pub intensity: f64,
    pub radius: f64,
}

/// A tier of the Bloom setting: the chain's first level against the drawing buffer (each axis) and
/// how many levels (each next one half the last; the first holds the bright pass, the rest the blur).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomTier {
    pub scale: f64,
    pub levels: u32,
}

/// The Bloom setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BloomQuality {
    Off,
    Low,
    High,
}

impl BloomQuality {
    /// From the settings store's name (`off`, `low`, `high`); anything else is None.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// The tier it draws (None: no bloom passes).
    pub fn tier(self) -> Option<BloomTier> {
        match self {
            Self::Off => None,
            Self::Low => Some(LOW),
            Self::High => Some(HIGH),
        }
    }
}

/// The chain for a drawing buffer of `size` at `tier`: the first level's size (each axis scaled,
/// rounded up) and the levels that fit (each next one half, none under one texel).
pub fn chain((w, h): (u32, u32), tier: BloomTier) -> (u32, u32, u32) {
    let s = |v: u32| ((f64::from(v) * tier.scale).ceil() as u32).max(1);
    let (w, h) = (s(w), s(h));
    let fits = 32 - w.min(h).max(1).leading_zeros();
    (w, h, tier.levels.min(fits).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tiers_pin_their_cost() {
        // Low (the Medium preset) at half of High's resolution, a level fewer; Off draws none.
        assert_eq!(HIGH.scale, 0.5);
        assert_eq!(LOW.scale, HIGH.scale / 2.0);
        assert_eq!(LOW.levels + 1, HIGH.levels);
        assert!(LOW.levels >= 4, "three blur levels or more");
        assert_eq!(BloomQuality::Off.tier(), None);
        assert_eq!(BloomQuality::Low.tier(), Some(LOW));
        assert_eq!(BloomQuality::High.tier(), Some(HIGH));
        assert_eq!(BloomQuality::from_name("low"), Some(BloomQuality::Low));
        assert_eq!(BloomQuality::from_name("ultra"), None);
    }

    #[test]
    fn a_chain_halves_from_its_first_level_and_stops_at_one_texel() {
        assert_eq!(chain((1280, 720), HIGH), (640, 360, HIGH.levels));
        assert_eq!(chain((1280, 720), LOW), (320, 180, LOW.levels));
        // A tiny buffer keeps the levels that fit (one: nothing drawn, the passes need two).
        assert_eq!(
            chain(
                (8, 8),
                BloomTier {
                    scale: 1.0,
                    levels: 8
                }
            ),
            (8, 8, 4)
        );
        assert_eq!(chain((1, 1), HIGH), (1, 1, 1));
    }
}
