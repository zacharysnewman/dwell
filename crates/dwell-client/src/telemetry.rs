//! Dev telemetry (docs/DEV_TOOLS.md): what the client spends each frame on, how long its jobs take,
//! and a trace of the player controller, for the bench driver (`client/scripts/bench.ts`) and
//! the console (`__dwell.telemetry()`). Metrics are named at the call site, so adding one is one
//! line: `telemetry::add("lod", ms)` for a per-frame value, `telemetry::sample("job.mesh", ms)` for
//! an event's, `telemetry::count("upload.bytes", n)` for a running total.
//!
//! Always on: recording is a few hash lookups per frame. One per thread (the main thread's is the
//! one exported; workers keep none).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

/// Frames kept for the summary (10 s at 60 Hz).
pub const FRAMES_KEPT: usize = 600;
/// Event samples kept per name.
pub const SAMPLES_KEPT: usize = 2048;
/// Controller ticks kept (10 s at 60 Hz).
pub const TICKS_KEPT: usize = 600;

/// One frame's values: the time it started and each per-frame series' total.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub at_ms: f64,
    pub values: Vec<f64>,
}

/// One controller tick, as the predictor left it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TickSample {
    pub seq: u32,
    pub at_ms: f64,
    /// The player's feet.
    pub feet: [f64; 3],
    pub velocity: [f64; 3],
    /// The PlayerState's name.
    pub state: &'static str,
    pub grounded: bool,
    pub crouched: bool,
    pub touching_wall: bool,
    pub ceiling_blocked: bool,
    /// The input frame: movement (x right, z forward), jump, crouch.
    pub move_input: [f64; 2],
    pub jump: bool,
    pub crouch: bool,
}

/// A distribution's summary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub sum: f64,
}

impl Stats {
    pub fn of(values: &[f64]) -> Stats {
        if values.is_empty() {
            return Stats::default();
        }
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        let sum: f64 = v.iter().sum();
        Stats {
            n: v.len(),
            mean: sum / v.len() as f64,
            p50: at(0.5),
            p95: at(0.95),
            p99: at(0.99),
            max: v[v.len() - 1],
            sum,
        }
    }
}

#[derive(Default)]
pub struct Telemetry {
    /// Per-frame series, in the order first seen.
    names: Vec<&'static str>,
    index: HashMap<&'static str, usize>,
    current: Vec<f64>,
    frame_start: Option<f64>,
    frames: VecDeque<Frame>,
    samples: HashMap<&'static str, VecDeque<f64>>,
    counters: HashMap<&'static str, f64>,
    ticks: VecDeque<TickSample>,
}

impl Telemetry {
    /// Starts a frame at `at_ms`, closing the one before.
    pub fn begin_frame(&mut self, at_ms: f64) {
        if let Some(start) = self.frame_start.replace(at_ms) {
            let mut values = std::mem::take(&mut self.current);
            values.resize(self.names.len(), 0.0);
            self.frames.push_back(Frame {
                at_ms: start,
                values,
            });
            while self.frames.len() > FRAMES_KEPT {
                self.frames.pop_front();
            }
        }
        self.current = vec![0.0; self.names.len()];
    }

    /// Adds `value` to this frame's `name` (milliseconds for a span, or a count).
    pub fn add(&mut self, name: &'static str, value: f64) {
        let i = *self.index.entry(name).or_insert_with(|| {
            self.names.push(name);
            self.names.len() - 1
        });
        if self.current.len() <= i {
            self.current.resize(i + 1, 0.0);
        }
        self.current[i] += value;
    }

    /// Records one event's value (a job's time, a chunk's size).
    pub fn sample(&mut self, name: &'static str, value: f64) {
        let q = self.samples.entry(name).or_default();
        q.push_back(value);
        while q.len() > SAMPLES_KEPT {
            q.pop_front();
        }
    }

    /// Adds to a running total.
    pub fn count(&mut self, name: &'static str, n: f64) {
        *self.counters.entry(name).or_default() += n;
    }

    pub fn tick(&mut self, t: TickSample) {
        self.ticks.push_back(t);
        while self.ticks.len() > TICKS_KEPT {
            self.ticks.pop_front();
        }
    }

    /// Forgets everything recorded (the names stay), for measuring from a mark.
    pub fn reset(&mut self) {
        self.frames.clear();
        self.samples.clear();
        self.counters.clear();
        self.ticks.clear();
        self.current = vec![0.0; self.names.len()];
        self.frame_start = None;
    }

    pub fn names(&self) -> &[&'static str] {
        &self.names
    }

    pub fn frames(&self) -> impl Iterator<Item = &Frame> {
        self.frames.iter()
    }

    pub fn ticks(&self) -> impl Iterator<Item = &TickSample> {
        self.ticks.iter()
    }

    pub fn counters(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        self.counters.iter().map(|(k, v)| (*k, *v))
    }

    /// Each per-frame series' summary over the kept frames, `interval` (the time from one frame
    /// to the next) first.
    pub fn frame_stats(&self) -> Vec<(&'static str, Stats)> {
        let intervals: Vec<f64> = self
            .frames
            .iter()
            .zip(self.frames.iter().skip(1))
            .map(|(a, b)| b.at_ms - a.at_ms)
            .collect();
        let mut out = vec![("interval", Stats::of(&intervals))];
        for (i, name) in self.names.iter().enumerate() {
            let v: Vec<f64> = self
                .frames
                .iter()
                .map(|f| f.values.get(i).copied().unwrap_or(0.0))
                .collect();
            out.push((name, Stats::of(&v)));
        }
        out
    }

    /// Each event series' summary.
    pub fn sample_stats(&self) -> Vec<(&'static str, Stats)> {
        let mut out: Vec<_> = self
            .samples
            .iter()
            .map(|(k, q)| (*k, Stats::of(&q.iter().copied().collect::<Vec<_>>())))
            .collect();
        out.sort_by_key(|(k, _)| *k);
        out
    }
}

thread_local! {
    static TELEMETRY: RefCell<Telemetry> = RefCell::default();
}

/// The thread's telemetry.
pub fn with<R>(f: impl FnOnce(&mut Telemetry) -> R) -> R {
    TELEMETRY.with(|t| f(&mut t.borrow_mut()))
}

pub fn add(name: &'static str, value: f64) {
    with(|t| t.add(name, value));
}

pub fn sample(name: &'static str, value: f64) {
    with(|t| t.sample(name, value));
}

pub fn count(name: &'static str, n: f64) {
    with(|t| t.count(name, n));
}

/// A monotonic clock in milliseconds: `performance.now()` in the browser.
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.performance())
            .map_or(0.0, |p| p.now())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

/// Runs `f`, adding its time to this frame's `name`.
pub fn span<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    let t0 = now_ms();
    let r = f();
    add(name, now_ms() - t0);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_sum_their_series_and_summarise_intervals() {
        let mut t = Telemetry::default();
        for i in 0..5 {
            t.begin_frame(f64::from(i) * 16.0);
            t.add("lod", 1.0);
            t.add("lod", 2.0);
            if i == 3 {
                // A series first seen late reads 0 in earlier frames.
                t.add("upload", 10.0);
            }
        }
        t.begin_frame(80.0);
        let stats: HashMap<_, _> = t.frame_stats().into_iter().collect();
        assert_eq!(stats["interval"].n, 4);
        assert_eq!(stats["interval"].p50, 16.0);
        assert_eq!(stats["lod"].n, 5);
        assert_eq!(stats["lod"].mean, 3.0);
        assert_eq!(stats["upload"].max, 10.0);
        assert_eq!(stats["upload"].sum, 10.0);
    }

    #[test]
    fn keeps_a_window_and_resets_from_a_mark() {
        let mut t = Telemetry::default();
        for i in 0..(FRAMES_KEPT + 10) {
            t.begin_frame(i as f64);
            t.sample("job", i as f64);
        }
        assert_eq!(t.frames().count(), FRAMES_KEPT);
        t.count("bytes", 5.0);
        t.reset();
        assert_eq!(t.frames().count(), 0);
        assert!(t.sample_stats().is_empty());
        assert_eq!(t.counters().count(), 0);
    }

    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = Stats::of(&v);
        assert_eq!((s.p50, s.p95, s.p99, s.max), (51.0, 95.0, 99.0, 100.0));
        assert_eq!(Stats::of(&[]).n, 0);
    }
}
