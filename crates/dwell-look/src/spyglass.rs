//! The spyglass (docs/SPYGLASS.md, ADR 0039): a toggle key narrows the view by a magnification
//! step from `shared/look/spyglass.json`, the distant terrain refines inside the narrow view, and a
//! vignette rings it. This is the platform-free part: the tunables, the FOV rule and the zoom's
//! state machine with its eased transition. The client draws the frame with [`zoom_fov`] of its FOV
//! and gives the LOD selection the [`SpyglassView`]'s magnification and detail.

#[path = "spyglass.gen.rs"]
mod generated;
pub use generated::SPYGLASS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpyglassStep {
    /// How many times the view is enlarged (> 1).
    pub magnification: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VignetteTunables {
    /// Clear out to here, dark from `outer_radius` on (units of half the screen height).
    pub inner_radius: f64,
    pub outer_radius: f64,
    /// The ring's opacity when fully zoomed (0–1), and its colour (sRGB 0–1).
    pub opacity: f64,
    pub color: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpyglassTunables {
    /// The toggle key (`KeyboardEvent.code`).
    pub key: &'static str,
    /// The zoom steps, least magnification first.
    pub steps: &'static [SpyglassStep],
    /// Inside the zoomed view the LOD refines as if the screen were magnified by at most this.
    pub detail_boost: f64,
    /// Seconds a zoom change eases over.
    pub transition_s: f64,
    pub vignette: VignetteTunables,
}

/// The vertical FOV (degrees) of `fov_y_deg` magnified `magnification` times: the half-angle's
/// tangent divided by it, so every pixel covers 1/magnification of the angle it did.
pub fn zoom_fov(fov_y_deg: f64, magnification: f64) -> f64 {
    if !magnification.is_finite() || magnification <= 1.0 {
        return fov_y_deg;
    }
    let m = magnification;
    2.0 * ((fov_y_deg.to_radians() / 2.0).tan() / m)
        .atan()
        .to_degrees()
}

/// Whether a key press is the spyglass's toggle (not a browser shortcut with Ctrl or ⌘).
pub fn is_spyglass_key(t: &SpyglassTunables, code: &str, ctrl: bool, meta: bool) -> bool {
    code == t.key && !ctrl && !meta
}

/// What a frame draws with: the magnification (1 unzoomed), the LOD's detail magnification inside
/// the view (at most the magnification and `detail_boost`), the vignette's strength 0–1 and the
/// look speed's scale (1 / magnification: the view turns as many pixels per mouse movement as it
/// does unzoomed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpyglassView {
    pub magnification: f64,
    pub detail: f64,
    pub vignette: f64,
    pub look_scale: f64,
}

impl SpyglassView {
    pub const NONE: Self = Self {
        magnification: 1.0,
        detail: 1.0,
        vignette: 0.0,
        look_scale: 1.0,
    };
}

/// The zoom: on or off, the step it is on, and the eased magnification (in log space, so each
/// doubling takes the same share of the transition).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spyglass {
    on: bool,
    step: usize,
    /// ln(magnification): where the current transition started, where it goes, and now.
    from: f64,
    to: f64,
    now: f64,
    /// Seconds into the transition.
    elapsed: f64,
}

impl Default for Spyglass {
    fn default() -> Self {
        Self::new()
    }
}

impl Spyglass {
    pub const fn new() -> Self {
        Self {
            on: false,
            step: 0,
            from: 0.0,
            to: 0.0,
            now: 0.0,
            elapsed: 0.0,
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }
    /// The step it zooms to (kept while off: the next toggle returns to it).
    pub fn step(&self) -> usize {
        self.step
    }

    /// The toggle key: zooms in to the last step used, or back out. Returns whether it is on.
    pub fn toggle(&mut self, t: &SpyglassTunables) -> bool {
        let on = !self.on;
        self.set(on, self.step, t);
        on
    }

    /// The next (+1) or previous (−1) step while zoomed, stopping at the ends. True if it moved.
    pub fn cycle(&mut self, dir: i32, t: &SpyglassTunables) -> bool {
        if !self.on || t.steps.is_empty() {
            return false;
        }
        let last = t.steps.len() as i64 - 1;
        let next = (self.step as i64 + i64::from(dir.signum())).clamp(0, last) as usize;
        if next == self.step {
            return false;
        }
        self.set(true, next, t);
        true
    }

    /// Zooms to `step` (clamped to the steps) or out, easing from where it is now.
    pub fn set(&mut self, on: bool, step: usize, t: &SpyglassTunables) {
        self.on = on && !t.steps.is_empty();
        self.step = step.min(t.steps.len().saturating_sub(1));
        let target = if self.on {
            t.steps[self.step].magnification.max(1.0).ln()
        } else {
            0.0
        };
        if target != self.to {
            self.from = self.now;
            self.to = target;
            self.elapsed = 0.0;
        }
    }

    /// Advances the transition by `dt` seconds (all at once when `instant`: reduced motion, or a
    /// zero transition time) and returns the frame's view.
    pub fn frame(&mut self, dt: f64, instant: bool, t: &SpyglassTunables) -> SpyglassView {
        if instant || t.transition_s <= 0.0 {
            self.elapsed = t.transition_s.max(0.0);
            self.now = self.to;
        } else {
            self.elapsed = (self.elapsed + dt.max(0.0)).min(t.transition_s);
            let x = self.elapsed / t.transition_s;
            let eased = x * x * (3.0 - 2.0 * x);
            self.now = self.from + (self.to - self.from) * eased;
        }
        self.view(t)
    }

    /// The view at the current point of the transition.
    pub fn view(&self, t: &SpyglassTunables) -> SpyglassView {
        let magnification = self.now.exp().max(1.0);
        let first = t.steps.first().map_or(1.0, |s| s.magnification.max(1.0));
        let vignette = if first > 1.0 {
            (self.now / first.ln()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        SpyglassView {
            magnification,
            detail: magnification.min(t.detail_boost.max(1.0)),
            vignette,
            look_scale: 1.0 / magnification,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_zoomed_fov_divides_the_half_angle_tangent() {
        assert!(close(zoom_fov(75.0, 1.0), 75.0));
        // 90° magnified 2×: tan 45° = 1 → tan(fov/2) = 0.5.
        assert!(close(zoom_fov(90.0, 2.0), 2.0 * 0.5f64.atan().to_degrees()));
        for fov in [60.0, 75.0, 100.0, 120.0] {
            for m in [2.0, 4.0, 8.0, 16.0] {
                let z = zoom_fov(fov, m);
                let ratio = (fov.to_radians() / 2.0).tan() / (z.to_radians() / 2.0).tan();
                assert!(close(ratio, m), "{fov}° ×{m}: {ratio}");
                assert!(z < fov);
            }
        }
        // Never wider than the frame's, whatever it is given.
        assert!(close(zoom_fov(75.0, 0.5), 75.0));
        assert!(close(zoom_fov(75.0, f64::NAN), 75.0));
    }

    #[test]
    fn the_checked_in_data_is_usable() {
        let t = &SPYGLASS;
        assert!(!t.steps.is_empty());
        assert!(
            t.steps
                .windows(2)
                .all(|w| w[1].magnification > w[0].magnification)
        );
        assert!(t.steps[0].magnification > 1.0 && t.detail_boost >= 1.0);
        assert!(is_spyglass_key(t, t.key, false, false));
        assert!(!is_spyglass_key(t, t.key, true, false));
        assert!(!is_spyglass_key(t, t.key, false, true));
        assert!(!is_spyglass_key(t, "KeyF", false, false));
    }

    #[test]
    fn the_key_toggles_and_the_wheel_steps_while_zoomed() {
        let t = &SPYGLASS;
        let mut s = Spyglass::new();
        assert!(!s.cycle(1, t), "the wheel does nothing while unzoomed");
        assert!(s.toggle(t));
        let v = s.frame(0.0, true, t);
        assert!(close(v.magnification, t.steps[0].magnification));
        assert!(close(v.vignette, 1.0) && close(v.look_scale, 1.0 / v.magnification));
        assert!(s.cycle(1, t));
        assert!(close(
            s.frame(0.0, true, t).magnification,
            t.steps[1].magnification
        ));
        // Stops at the ends.
        while s.cycle(1, t) {}
        assert_eq!(s.step(), t.steps.len() - 1);
        assert!(!s.cycle(1, t));
        assert!(!s.toggle(t));
        assert_eq!(s.frame(0.0, true, t), SpyglassView::NONE);
        // The next toggle returns to the step last used.
        s.toggle(t);
        assert_eq!(s.step(), t.steps.len() - 1);
    }

    /// A step added by data alone is reached by the wheel and zooms by its magnification.
    #[test]
    fn a_step_added_by_data_alone_works() {
        let steps: Vec<SpyglassStep> = SPYGLASS
            .steps
            .iter()
            .copied()
            .chain([SpyglassStep {
                magnification: 32.0,
            }])
            .collect();
        let t = SpyglassTunables {
            steps: Box::leak(steps.into_boxed_slice()),
            detail_boost: 16.0,
            ..SPYGLASS
        };
        let mut s = Spyglass::new();
        s.toggle(&t);
        while s.cycle(1, &t) {}
        let v = s.frame(0.0, true, &t);
        assert!(close(v.magnification, 32.0));
        assert!(close(v.detail, 16.0), "the detail stops at the boost");
        assert!(close(zoom_fov(75.0, v.magnification), zoom_fov(75.0, 32.0)));
    }

    #[test]
    fn the_zoom_eases_over_the_transition_and_is_instant_under_reduced_motion() {
        let t = SpyglassTunables {
            transition_s: 0.5,
            ..SPYGLASS
        };
        let m = t.steps[0].magnification;
        let mut s = Spyglass::new();
        s.toggle(&t);
        let mut last = 1.0;
        let mut frames = 0;
        loop {
            let v = s.frame(0.05, false, &t);
            assert!(v.magnification >= last - 1e-12, "rises steadily");
            last = v.magnification;
            frames += 1;
            if close(v.magnification, m) {
                break;
            }
            assert!(frames < 20);
        }
        assert_eq!(frames, 10, "0.5 s at 20 fps");
        // Halfway (in log space) at half the time.
        let mut s = Spyglass::new();
        s.toggle(&t);
        let half = s.frame(0.25, false, &t);
        assert!(
            close(half.magnification, m.sqrt()),
            "{}",
            half.magnification
        );
        assert!(half.vignette > 0.4 && half.vignette < 0.6);
        // Out again from mid-way: no jump.
        s.toggle(&t);
        let v = s.frame(0.0, false, &t);
        assert!(close(v.magnification, half.magnification));
        // Reduced motion: at once.
        let mut s = Spyglass::new();
        s.toggle(&t);
        assert!(close(s.frame(0.0, true, &t).magnification, m));
    }
}
