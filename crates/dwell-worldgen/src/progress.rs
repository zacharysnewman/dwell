//! A world map build's progress (BUGS.md #91): the builder reports each of its steps as it begins
//! and how far it is through it, so the loading screen can say what it is doing ("Shaping the
//! world map — rivers (5/6)") rather than sitting on a whole face for minutes, and the native
//! `world_size` example can time each step.
//!
//! The steps are a small fixed vocabulary (the builder's own stages, code); what the player reads
//! for each is data (`shared/look/loading.json` `mapSteps`, by id, in this order: a test checks the
//! two agree). The sink is per thread: a browser worker builds one face, and a native build runs
//! each face on its own thread, so each installs its own with [`with_sink`]; without one,
//! [`report`] does nothing.

use std::cell::RefCell;

/// One step of a face's map build, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStep {
    /// Sampling the base field (the island and its relief) on the map's grid.
    Base,
    /// The uplift layer (ranges, plates, highlands) and the rain.
    Uplift,
    /// The stream-power erosion: its fraction is its iterations done.
    Erosion,
    /// Filling pits, the drainage and the lakes.
    Lakes,
    /// The discharge and the rivers' courses.
    Rivers,
    /// The kept map's derived parts: the channels and the shores.
    Shores,
}

impl MapStep {
    /// Every step, in order.
    pub const ALL: [MapStep; 6] = [
        MapStep::Base,
        MapStep::Uplift,
        MapStep::Erosion,
        MapStep::Lakes,
        MapStep::Rivers,
        MapStep::Shores,
    ];

    /// Its id (`loading.json` `mapSteps[].id`).
    pub const fn id(self) -> &'static str {
        match self {
            MapStep::Base => "base",
            MapStep::Uplift => "uplift",
            MapStep::Erosion => "erosion",
            MapStep::Lakes => "lakes",
            MapStep::Rivers => "rivers",
            MapStep::Shores => "shores",
        }
    }

    /// Its place in [`MapStep::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }
}

type Sink = Box<dyn FnMut(MapStep, f64)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// Runs `f` with `sink` receiving this thread's map-build progress (the step, and the fraction of
/// it done, 0 at its start and 1 at its end), then restores the sink before it.
pub fn with_sink<R>(sink: impl FnMut(MapStep, f64) + 'static, f: impl FnOnce() -> R) -> R {
    let before = SINK.with(|s| s.borrow_mut().replace(Box::new(sink)));
    struct Restore(Option<Option<Sink>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let before = self.0.take().flatten();
            SINK.with(|s| *s.borrow_mut() = before);
        }
    }
    let _restore = Restore(Some(before));
    f()
}

/// Reports `fraction` (clamped to 0..=1) of `step` done to this thread's sink, if any.
pub fn report(step: MapStep, fraction: f64) {
    SINK.with(|s| {
        // A sink that reports from inside itself is not re-entered.
        if let Ok(mut s) = s.try_borrow_mut()
            && let Some(sink) = s.as_mut()
        {
            sink(step, fraction.clamp(0.0, 1.0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn the_steps_agree_with_the_loading_data() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/look/loading.json")).unwrap();
        let ids: Vec<&str> = data["mapSteps"]["list"]
            .as_array()
            .expect("loading.json mapSteps.list")
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        let ours: Vec<&str> = MapStep::ALL.iter().map(|s| s.id()).collect();
        assert_eq!(
            ids, ours,
            "loading.json mapSteps must list the builder's steps in order"
        );
    }

    #[test]
    fn a_sink_hears_reports_only_while_installed() {
        let heard = Rc::new(RefCell::new(Vec::new()));
        let h = heard.clone();
        report(MapStep::Base, 0.0);
        with_sink(
            move |s, f| h.borrow_mut().push((s, f)),
            || {
                report(MapStep::Erosion, 0.5);
                report(MapStep::Rivers, 2.0);
            },
        );
        report(MapStep::Shores, 1.0);
        assert_eq!(
            *heard.borrow(),
            vec![(MapStep::Erosion, 0.5), (MapStep::Rivers, 1.0)]
        );
    }
}
