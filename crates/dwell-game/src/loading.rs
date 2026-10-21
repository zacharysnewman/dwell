//! The loading screen (ARCHITECTURE.md §6.3): on joining a world, and on respawning, the game waits
//! behind a progress readout until the full-detail chunks of the near ring around the player are
//! generated, lit, meshed and drawn, or until a timeout; the player does not move until it opens
//! (the prediction gate, `gate.rs`). Its radius and timeout are data (`shared/look/loading.json`,
//! generated into `loading.gen.rs`). Opted out for testing (`?loading=off`, the `dwell.loading`
//! store), it never shows.
//!
//! Owner (2026-10-21): "Perhaps opt-out-able for testing, I'm thinking of doing a loading screen
//! until the chunks around the player load on initial join."

/// The loading screen's tunables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadingTunables {
    /// The near ring: the server's view sphere of this radius in chunks (|offset|² ≤ r² + r).
    pub radius_chunks: i32,
    /// The screen opens after this long whatever has loaded (s).
    pub timeout_s: f64,
}

include!("loading.gen.rs");

/// The chunk offsets of the near ring of `radius` chunks, nearest first: the server's view sphere
/// (`dwell-sim` `within_radius`: |offset|² ≤ r² + r), so every one is streamed without a request.
pub fn near_ring_offsets(radius: i32) -> Vec<[i32; 3]> {
    let r = radius.max(0);
    let mut out = Vec::new();
    for y in -r..=r {
        for z in -r..=r {
            for x in -r..=r {
                if x * x + y * y + z * z <= r * r + r {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out.sort_by_key(|c| c[0] * c[0] + c[1] * c[1] + c[2] * c[2]);
    out
}

/// How far the near ring has loaded: chunks shown out of those needed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NearProgress {
    pub ready: usize,
    pub needed: usize,
}

impl NearProgress {
    pub fn complete(&self) -> bool {
        self.ready >= self.needed
    }
}

/// What the loading screen shows: progress while it waits; nothing once open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadingScreen {
    pub progress: NearProgress,
    /// Seconds since it started.
    pub elapsed_s: f64,
}

/// Why the screen last opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// The near ring was ready.
    Ready,
    /// The timeout passed first.
    TimedOut,
}

/// The loading screen's state: closed (waiting, shown) from `start` until the near ring is ready
/// or the timeout passes. Disabled, it is always open.
#[derive(Clone, Debug)]
pub struct LoadingGate {
    tunables: LoadingTunables,
    enabled: bool,
    /// When the current wait started (ms); None while open.
    started_ms: Option<f64>,
    progress: NearProgress,
    elapsed_s: f64,
    /// How the last wait ended, and after how long (s).
    last: Option<(Opened, f64)>,
}

impl LoadingGate {
    pub fn new(tunables: LoadingTunables, enabled: bool) -> Self {
        Self {
            tunables,
            enabled,
            started_ms: None,
            progress: NearProgress::default(),
            elapsed_s: 0.0,
            last: None,
        }
    }

    pub fn tunables(&self) -> LoadingTunables {
        self.tunables
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The player was placed (joined or respawned): wait for the near ring again. Without the
    /// screen (opted out) nothing waits.
    pub fn start(&mut self, now_ms: f64) {
        if self.enabled {
            self.started_ms = Some(now_ms);
            self.elapsed_s = 0.0;
            self.progress = NearProgress::default();
        }
    }

    /// This frame's progress: opens once it is complete or the timeout has passed. True while open.
    pub fn update(&mut self, now_ms: f64, progress: NearProgress) -> bool {
        let Some(start) = self.started_ms else {
            return true;
        };
        self.progress = progress;
        self.elapsed_s = ((now_ms - start) / 1000.0).max(0.0);
        let opened = if progress.complete() {
            Some(Opened::Ready)
        } else if self.elapsed_s >= self.tunables.timeout_s {
            Some(Opened::TimedOut)
        } else {
            None
        };
        if let Some(how) = opened {
            self.started_ms = None;
            self.last = Some((how, self.elapsed_s));
        }
        self.is_open()
    }

    pub fn is_open(&self) -> bool {
        self.started_ms.is_none()
    }

    /// The screen to show, while waiting.
    pub fn screen(&self) -> Option<LoadingScreen> {
        self.started_ms.map(|_| LoadingScreen {
            progress: self.progress,
            elapsed_s: self.elapsed_s,
        })
    }

    /// How the last wait ended and its length (s): the bench's join-to-gate time.
    pub fn last_opened(&self) -> Option<(Opened, f64)> {
        self.last
    }
}

/// The screen's readout: "Loading terrain… 37 / 123".
pub fn loading_text(screen: &LoadingScreen) -> String {
    format!(
        "Loading terrain… {} / {}",
        screen.progress.ready.min(screen.progress.needed),
        screen.progress.needed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LoadingTunables = LoadingTunables {
        radius_chunks: 2,
        timeout_s: 10.0,
    };

    fn p(ready: usize, needed: usize) -> NearProgress {
        NearProgress { ready, needed }
    }

    #[test]
    fn the_checked_in_tunables_keep_the_ring_within_the_streamed_view() {
        let view = dwell_protocol::world::VIEW_RADIUS_CHUNKS as i32;
        assert!((0..=view).contains(&LOADING.radius_chunks));
        assert!(LOADING.timeout_s >= 1.0);
    }

    #[test]
    fn the_near_ring_is_the_servers_view_sphere_nearest_first() {
        let ring = near_ring_offsets(3);
        // |c|² ≤ r² + r = 12: the server's within_radius.
        assert_eq!(ring.len(), 179);
        assert_eq!(ring[0], [0, 0, 0]);
        assert!(ring.contains(&[-3, 0, 0]) && ring.contains(&[3, 0, 0]));
        assert!(
            ring.contains(&[2, 2, 2]) && ring.contains(&[3, 1, 1]) && !ring.contains(&[3, 1, 2])
        );
        let d: Vec<i32> = ring.iter().map(|c| c.iter().map(|v| v * v).sum()).collect();
        assert!(d.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(near_ring_offsets(0), vec![[0, 0, 0]]);
    }

    #[test]
    fn waits_until_the_near_ring_is_ready() {
        let mut g = LoadingGate::new(T, true);
        assert!(
            g.is_open(),
            "nothing to wait for before the player is placed"
        );
        g.start(1000.0);
        assert!(!g.update(1500.0, p(10, 50)));
        let s = g.screen().expect("shown");
        assert_eq!(s.progress, p(10, 50));
        assert_eq!(loading_text(&s), "Loading terrain… 10 / 50");
        assert!(g.update(3000.0, p(50, 50)));
        assert_eq!(g.screen(), None);
        assert_eq!(g.last_opened(), Some((Opened::Ready, 2.0)));
        // Open stays open.
        assert!(g.update(4000.0, p(0, 50)));
    }

    #[test]
    fn opens_on_the_timeout_whatever_has_loaded() {
        let mut g = LoadingGate::new(T, true);
        g.start(0.0);
        assert!(!g.update(9_999.0, p(3, 50)));
        assert!(g.update(10_000.0, p(3, 50)));
        assert_eq!(g.last_opened(), Some((Opened::TimedOut, 10.0)));
    }

    #[test]
    fn opted_out_it_never_shows() {
        let mut g = LoadingGate::new(T, false);
        g.start(0.0);
        assert!(g.is_open());
        assert!(g.update(1.0, p(0, 50)));
        assert_eq!(g.screen(), None);
        assert_eq!(g.last_opened(), None);
    }

    #[test]
    fn a_respawn_waits_again() {
        let mut g = LoadingGate::new(T, true);
        g.start(0.0);
        assert!(g.update(100.0, p(5, 5)));
        g.start(5_000.0);
        assert!(!g.update(5_100.0, p(1, 5)));
        assert!(g.update(5_200.0, p(5, 5)));
        assert_eq!(g.last_opened(), Some((Opened::Ready, 0.2)));
    }
}
