//! Time budgets. `std::time::Instant` panics on wasm32-unknown-unknown (no clock), where the core
//! runs single-threaded in the browser's local mode; there a budget is a count of work items
//! instead, chosen so a tick stays well inside its 16 ms (the host may pass a better clock later).

use std::time::Duration;

pub struct Deadline {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    at: std::time::Instant,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    left: std::cell::Cell<u32>,
}

impl Deadline {
    pub fn after(budget: Duration) -> Self {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            Self {
                at: std::time::Instant::now() + budget,
            }
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            // About one work item (a chunk or a section) per millisecond of budget.
            Self {
                left: std::cell::Cell::new((budget.as_millis() as u32).max(1)),
            }
        }
    }
    /// Whether the budget is spent (counts one item on wasm32).
    pub fn passed(&self) -> bool {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            std::time::Instant::now() >= self.at
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let n = self.left.get().saturating_sub(1);
            self.left.set(n);
            n == 0
        }
    }
}

/// A stopwatch for the tick's phase times (`Server::tick_phases`): wall time natively; on wasm32
/// the embedder's clock (`set_clock`, the local worker's `performance.now`), nothing without one.
pub struct Stopwatch {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    at: std::time::Instant,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    at: f64,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
thread_local! {
    static WASM_NOW: std::cell::Cell<Option<fn() -> f64>> = const { std::cell::Cell::new(None) };
}

/// The clock (ms) the tick's phases are timed by on wasm32 (dwell-sim has no JS): the local
/// worker's `performance.now`, so `__dwell.serverStats()` shows the browser's phases.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn set_clock(f: fn() -> f64) {
    WASM_NOW.with(|c| c.set(Some(f)));
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn wasm_now() -> f64 {
    WASM_NOW.with(|f| f.get().map_or(0.0, |f| f()))
}

impl Stopwatch {
    pub fn start() -> Self {
        Self {
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            at: std::time::Instant::now(),
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            at: wasm_now(),
        }
    }
    /// Milliseconds since the last lap (or the start), and starts the next lap.
    pub fn lap_ms(&mut self) -> f64 {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            let now = std::time::Instant::now();
            let ms = (now - self.at).as_secs_f64() * 1000.0;
            self.at = now;
            ms
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let now = wasm_now();
            let ms = now - self.at;
            self.at = now;
            ms
        }
    }
}
