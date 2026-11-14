//! Easing curves (THEATER_MODE.md §3.3): a small fixed vocabulary, each a map of [0, 1] onto
//! [0, 1] with `f(0) = 0` and `f(1) = 1`, named in a path's or a preset's data.

/// An easing curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    Linear,
    /// Smoothstep: `3u² − 2u³`.
    Smooth,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InSine,
    OutSine,
    InOutSine,
}

impl Easing {
    /// Every curve with its data name.
    pub const ALL: [(&'static str, Easing); 11] = [
        ("linear", Easing::Linear),
        ("smooth", Easing::Smooth),
        ("inQuad", Easing::InQuad),
        ("outQuad", Easing::OutQuad),
        ("inOutQuad", Easing::InOutQuad),
        ("inCubic", Easing::InCubic),
        ("outCubic", Easing::OutCubic),
        ("inOutCubic", Easing::InOutCubic),
        ("inSine", Easing::InSine),
        ("outSine", Easing::OutSine),
        ("inOutSine", Easing::InOutSine),
    ];

    /// The curve of a data name.
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|&(_, e)| e)
    }
    /// The curve's data name.
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, e)| *e == self)
            .map(|&(n, _)| n)
            .unwrap_or("linear")
    }
    /// The names, for messages.
    pub fn names() -> String {
        Self::ALL.map(|(n, _)| n).join(", ")
    }

    /// The eased value of `u` (clamped to [0, 1]).
    pub fn apply(self, u: f64) -> f64 {
        use std::f64::consts::PI;
        let u = u.clamp(0.0, 1.0);
        match self {
            Easing::Linear => u,
            Easing::Smooth => u * u * (3.0 - 2.0 * u),
            Easing::InQuad => u * u,
            Easing::OutQuad => 1.0 - (1.0 - u) * (1.0 - u),
            Easing::InOutQuad => {
                if u < 0.5 {
                    2.0 * u * u
                } else {
                    1.0 - 2.0 * (1.0 - u) * (1.0 - u)
                }
            }
            Easing::InCubic => u * u * u,
            Easing::OutCubic => 1.0 - (1.0 - u).powi(3),
            Easing::InOutCubic => {
                if u < 0.5 {
                    4.0 * u * u * u
                } else {
                    1.0 - 4.0 * (1.0 - u).powi(3)
                }
            }
            Easing::InSine => 1.0 - (u * PI / 2.0).cos(),
            Easing::OutSine => (u * PI / 2.0).sin(),
            Easing::InOutSine => 0.5 - 0.5 * (u * PI).cos(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_runs_from_0_to_1_without_jumps_or_turning_back() {
        for (name, e) in Easing::ALL {
            assert!(e.apply(0.0).abs() < 1e-12, "{name} starts at 0");
            assert!((e.apply(1.0) - 1.0).abs() < 1e-12, "{name} ends at 1");
            let n = 1000;
            let mut last = 0.0;
            for k in 1..=n {
                let v = e.apply(k as f64 / n as f64);
                assert!(v + 1e-12 >= last, "{name} turns back at {k}");
                // The steepest of these is 3 (in/out cubic): a step of 1/n moves at most 3/n.
                assert!(v - last <= 3.0 / n as f64 + 1e-9, "{name} jumps at {k}");
                last = v;
            }
            assert_eq!(Easing::by_name(name), Some(e));
            assert_eq!(e.name(), name);
        }
    }

    #[test]
    fn in_out_curves_start_and_end_at_rest() {
        let slope = |e: Easing, u: f64| (e.apply(u + 1e-6) - e.apply(u)) / 1e-6;
        for e in [
            Easing::Smooth,
            Easing::InOutQuad,
            Easing::InOutCubic,
            Easing::InOutSine,
        ] {
            assert!(slope(e, 0.0) < 1e-3, "{e:?} leaves at rest");
            assert!(slope(e, 1.0 - 1e-6) < 1e-3, "{e:?} arrives at rest");
        }
    }
}
