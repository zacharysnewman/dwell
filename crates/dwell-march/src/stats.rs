//! Steps-per-ray histograms (RAY_MARCHED_RENDERER.md §4.3): median, p95, p99 and the share at the
//! cap — what the marcher reports to telemetry and the bench.

use crate::march::{March, Outcome};

#[derive(Clone, Debug, Default)]
pub struct StepHistogram {
    steps: Vec<u32>,
    capped: usize,
    hits: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepSummary {
    pub rays: usize,
    pub median: u32,
    pub p95: u32,
    pub p99: u32,
    pub max: u32,
    pub mean: f64,
    pub capped_share: f64,
    pub hit_share: f64,
}

impl StepHistogram {
    pub fn add(&mut self, m: &March) {
        self.steps.push(m.steps);
        match m.outcome {
            Outcome::Cap => self.capped += 1,
            Outcome::Hit(_) => self.hits += 1,
            Outcome::Miss => {}
        }
    }
    /// The share of rays that took more than `cap` steps (what a lower cap would end).
    pub fn share_over(&self, cap: u32) -> f64 {
        if self.steps.is_empty() {
            return 0.0;
        }
        self.steps.iter().filter(|&&s| s > cap).count() as f64 / self.steps.len() as f64
    }
    /// Nearest-rank percentiles.
    pub fn summary(&self) -> StepSummary {
        let mut s = self.steps.clone();
        s.sort_unstable();
        let n = s.len();
        let rank = |q: f64| {
            if n == 0 {
                0
            } else {
                s[((q * n as f64).ceil() as usize).clamp(1, n) - 1]
            }
        };
        StepSummary {
            rays: n,
            median: rank(0.5),
            p95: rank(0.95),
            p99: rank(0.99),
            max: s.last().copied().unwrap_or(0),
            mean: if n == 0 {
                0.0
            } else {
                s.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64
            },
            capped_share: if n == 0 {
                0.0
            } else {
                self.capped as f64 / n as f64
            },
            hit_share: if n == 0 {
                0.0
            } else {
                self.hits as f64 / n as f64
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_by_nearest_rank() {
        let mut h = StepHistogram::default();
        for s in 1..=100 {
            h.add(&March {
                outcome: if s == 100 {
                    Outcome::Cap
                } else {
                    Outcome::Miss
                },
                steps: s,
            });
        }
        let s = h.summary();
        assert_eq!((s.median, s.p95, s.p99, s.max), (50, 95, 99, 100));
        assert_eq!(s.capped_share, 0.01);
        assert_eq!(h.share_over(90), 0.1);
    }
}
