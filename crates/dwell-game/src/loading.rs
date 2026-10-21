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

/// Whether the respawn preload (§6.3) runs: on unless the page's query has `respawnPreload=off`
/// (the bench's comparison).
pub fn respawn_preload_from_query(search: &str) -> bool {
    !search
        .trim_start_matches('?')
        .split('&')
        .any(|p| p == "respawnPreload=off")
}

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
    /// False until the player is placed (joining): nothing to count yet.
    pub placed: bool,
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

/// The loading screen's state: closed (shown) from creation until the player is placed (joining:
/// so the screen is the first thing shown, before the view or the HUD), then from `start` until
/// the near ring is ready or the timeout passes. Disabled, it is always open.
#[derive(Clone, Debug)]
pub struct LoadingGate {
    tunables: LoadingTunables,
    enabled: bool,
    /// Shown, waiting for the player to be placed (from creation until the join places them).
    joining: bool,
    /// When the current wait started (ms); None while open.
    started_ms: Option<f64>,
    progress: NearProgress,
    elapsed_s: f64,
    /// How the last wait ended, and after how long (s).
    last: Option<(Opened, f64)>,
    /// How many times the screen has opened: the shell fades the view in on each (§6.3).
    opens: u32,
}

impl LoadingGate {
    pub fn new(tunables: LoadingTunables, enabled: bool) -> Self {
        Self {
            tunables,
            enabled,
            joining: enabled,
            started_ms: None,
            progress: NearProgress::default(),
            elapsed_s: 0.0,
            last: None,
            opens: 0,
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
            self.joining = false;
            self.started_ms = Some(now_ms);
            self.elapsed_s = 0.0;
            self.progress = NearProgress::default();
        }
    }

    /// The player was placed where nothing is waited for (in flight, which outruns the
    /// streaming): a screen still shown for the join opens at once.
    pub fn skip(&mut self) {
        if self.joining {
            self.joining = false;
            self.last = Some((Opened::Ready, 0.0));
            self.opens += 1;
        }
    }

    /// This frame's progress: opens once it is complete or the timeout has passed. True while open.
    pub fn update(&mut self, now_ms: f64, progress: NearProgress) -> bool {
        let Some(start) = self.started_ms else {
            return self.is_open();
        };
        // Shown without going back: a chunk the LOD stops showing as chunks for a moment (its
        // root just became ready, and the level-1 section above it is not yet) counts again soon.
        let ready = if progress.needed == self.progress.needed {
            progress.ready.max(self.progress.ready)
        } else {
            progress.ready
        };
        self.progress = NearProgress {
            ready,
            needed: progress.needed,
        };
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
            self.opens += 1;
        }
        self.is_open()
    }

    pub fn is_open(&self) -> bool {
        !self.joining && self.started_ms.is_none()
    }

    /// The screen to show, while waiting (from creation while joining).
    pub fn screen(&self) -> Option<LoadingScreen> {
        if self.joining {
            return Some(LoadingScreen {
                placed: false,
                progress: NearProgress::default(),
                elapsed_s: 0.0,
            });
        }
        self.started_ms.map(|_| LoadingScreen {
            placed: true,
            progress: self.progress,
            elapsed_s: self.elapsed_s,
        })
    }

    /// How many times the screen has opened (the shell's cue to fade the view in).
    pub fn opens(&self) -> u32 {
        self.opens
    }

    /// How the last wait ended and its length (s): the bench's join-to-gate time.
    pub fn last_opened(&self) -> Option<(Opened, f64)> {
        self.last
    }
}

/// The screen's readout: "Joining…" until placed, then "Loading terrain… 37 / 123".
pub fn loading_text(screen: &LoadingScreen) -> String {
    if !screen.placed {
        return "Joining…".to_string();
    }
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
        const { assert!(LOADING.timeout_s >= 1.0) };
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
    fn the_respawn_preload_is_off_only_when_asked() {
        assert!(respawn_preload_from_query(""));
        assert!(respawn_preload_from_query("?world=flat&loading=off"));
        assert!(!respawn_preload_from_query("?respawnPreload=off"));
        assert!(!respawn_preload_from_query("?seed=3&respawnPreload=off"));
        assert!(respawn_preload_from_query("?respawnPreload=on"));
    }

    #[test]
    fn waits_until_the_near_ring_is_ready() {
        let mut g = LoadingGate::new(T, true);
        g.start(1000.0);
        assert!(!g.update(1500.0, p(10, 50)));
        let s = g.screen().expect("shown");
        assert_eq!(s.progress, p(10, 50));
        assert_eq!(loading_text(&s), "Loading terrain… 10 / 50");
        assert!(g.update(3000.0, p(50, 50)));
        assert_eq!(g.screen(), None);
        assert_eq!(g.last_opened(), Some((Opened::Ready, 2.0)));
        assert_eq!(g.opens(), 1);
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
        g.skip();
        assert_eq!(g.opens(), 0);
    }

    #[test]
    fn shows_from_creation_until_the_join_places_the_player() {
        // Owner (2026-10-21): "you see ingame UI briefly before loading screen appears": the
        // screen only showed once the player was placed, so the view and the HUD came first.
        let mut g = LoadingGate::new(T, true);
        assert!(!g.is_open(), "closed before the player is placed");
        let s = g.screen().expect("shown from the start");
        assert!(!s.placed);
        assert_eq!(loading_text(&s), "Joining…");
        assert!(!g.update(500.0, p(0, 0)), "nothing placed: still joining");
        g.start(1000.0);
        assert!(g.screen().unwrap().placed);
        assert!(g.update(1100.0, p(5, 5)));
        assert_eq!(g.opens(), 1);
    }

    #[test]
    fn a_join_in_flight_opens_at_once() {
        let mut g = LoadingGate::new(T, true);
        g.skip();
        assert!(g.is_open());
        assert_eq!(g.screen(), None);
        assert_eq!(g.opens(), 1);
        assert_eq!(g.last_opened(), Some((Opened::Ready, 0.0)));
        g.skip();
        assert_eq!(g.opens(), 1, "only the join's screen");
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
        assert_eq!(g.opens(), 2, "each opening fades the view in");
    }

    #[test]
    fn the_readout_does_not_go_back_but_the_gate_waits_for_the_real_count() {
        // The bench's first screenshots read 0 / 179 after 21: the LOD's root became ready and hid
        // the chunks until their level-1 sections were.
        let mut g = LoadingGate::new(T, true);
        g.start(0.0);
        assert!(!g.update(100.0, p(21, 179)));
        assert!(!g.update(200.0, p(0, 179)));
        assert_eq!(g.screen().unwrap().progress, p(21, 179));
        assert!(!g.update(300.0, p(178, 179)));
        assert!(g.update(400.0, p(179, 179)));
        // A new ring (the player moved to another chunk) counts afresh.
        g.start(1000.0);
        assert!(!g.update(1100.0, p(30, 170)));
        assert!(!g.update(1200.0, p(5, 179)));
        assert_eq!(g.screen().unwrap().progress, p(5, 179));
    }
}
