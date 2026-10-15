//! The marcher's GPU counters (docs/RAY_MARCHED_RENDERER.md §4.3): a steps-per-ray histogram the
//! compute pass fills with atomics on the frames it is asked to, read back occasionally and
//! summarised as median, p95, p99 and the share at the cap; and, for the GPU-vs-reference test,
//! one debug record per pixel after it. The layout of the `stats` buffer (u32 words):
//!
//! - `0..HIST_BINS`: rays that ended (hit or missed) after `i` steps (`i` ≤ the cap ≤ 1,024);
//! - `HIST_CAPPED`: rays that reached the step cap; `HIST_HITS`, `HIST_MISSES`: outcomes;
//! - from `HIST_WORDS`, when the debug flag is set: `DEBUG_WORDS` per pixel (row-major): kind
//!   (0 miss, 1 hit, 2 cap), steps, level, cell x/y/z (i32 bits), face (6 none), t (f32 bits),
//!   state, the ray's direction x/y/z (f32 bits).

use dwell_march::StepSummary;

/// The largest step cap the histogram resolves.
pub const MAX_CAP: u32 = 1024;
pub const HIST_BINS: usize = MAX_CAP as usize + 1;
pub const HIST_CAPPED: usize = HIST_BINS;
pub const HIST_HITS: usize = HIST_BINS + 1;
pub const HIST_MISSES: usize = HIST_BINS + 2;
pub const HIST_WORDS: usize = HIST_BINS + 3;
pub const DEBUG_WORDS: usize = 12;

/// One pixel's debug record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugRay {
    pub kind: u32,
    pub steps: u32,
    pub level: u32,
    pub cell: [i32; 3],
    pub face: u32,
    pub t: f32,
    pub state: u32,
    pub dir: [f32; 3],
}

impl DebugRay {
    pub fn from_words(w: &[u32]) -> Self {
        Self {
            kind: w[0],
            steps: w[1],
            level: w[2],
            cell: [w[3] as i32, w[4] as i32, w[5] as i32],
            face: w[6],
            t: f32::from_bits(w[7]),
            state: w[8],
            dir: [
                f32::from_bits(w[9]),
                f32::from_bits(w[10]),
                f32::from_bits(w[11]),
            ],
        }
    }
}

/// The histogram's summary (nearest-rank percentiles, as `dwell_march::StepHistogram`); capped
/// rays count as `cap + 1` steps, as the reference marcher reports them.
pub fn summarize(words: &[u32], cap: u32) -> StepSummary {
    let bins = &words[..HIST_BINS];
    let capped = u64::from(words[HIST_CAPPED]);
    let hits = u64::from(words[HIST_HITS]);
    let n: u64 = bins.iter().map(|&b| u64::from(b)).sum::<u64>() + capped;
    let rank = |q: f64| -> u32 {
        if n == 0 {
            return 0;
        }
        let r = ((q * n as f64).ceil() as u64).clamp(1, n);
        let mut seen = 0u64;
        for (i, &b) in bins.iter().enumerate() {
            seen += u64::from(b);
            if seen >= r {
                return i as u32;
            }
        }
        cap + 1
    };
    let max = if capped > 0 {
        cap + 1
    } else {
        bins.iter().rposition(|&b| b > 0).unwrap_or(0) as u32
    };
    let total: f64 = bins
        .iter()
        .enumerate()
        .map(|(i, &b)| i as f64 * f64::from(b))
        .sum::<f64>()
        + capped as f64 * f64::from(cap + 1);
    let share = |x: u64| if n == 0 { 0.0 } else { x as f64 / n as f64 };
    StepSummary {
        rays: n as usize,
        median: rank(0.5),
        p95: rank(0.95),
        p99: rank(0.99),
        max,
        mean: if n == 0 { 0.0 } else { total / n as f64 },
        capped_share: share(capped),
        hit_share: share(hits),
    }
}

/// The telemetry samples of a summary (DEV_TOOLS.md: `march.steps.*`).
pub fn telemetry_samples(s: &StepSummary) -> [(&'static str, f64); 6] {
    [
        ("march.steps.p50", f64::from(s.median)),
        ("march.steps.p95", f64::from(s.p95)),
        ("march.steps.p99", f64::from(s.p99)),
        ("march.steps.max", f64::from(s.max)),
        ("march.steps.mean", s.mean),
        ("march.steps.capped", s.capped_share),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_march::{March, Outcome, StepHistogram};

    #[test]
    fn the_gpu_histogram_summarises_as_the_reference_histogram_does() {
        // The same rays counted both ways give the same percentiles.
        let cap = 64;
        let mut words = vec![0u32; HIST_WORDS];
        let mut reference = StepHistogram::default();
        let mut s: u32 = 12345;
        for _ in 0..5000 {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let steps = (s >> 16) % 80;
            let outcome = if steps > cap {
                words[HIST_CAPPED] += 1;
                Outcome::Cap
            } else {
                words[steps as usize] += 1;
                if steps.is_multiple_of(3) {
                    words[HIST_HITS] += 1;
                    Outcome::Hit(dwell_march::Hit {
                        level: 0,
                        cell: [0; 3],
                        face: None,
                        t: 0.0,
                        state: 1,
                    })
                } else {
                    words[HIST_MISSES] += 1;
                    Outcome::Miss
                }
            };
            let steps = if steps > cap { cap + 1 } else { steps };
            reference.add(&March { outcome, steps });
        }
        let a = summarize(&words, cap);
        let b = reference.summary();
        assert_eq!(
            (a.rays, a.median, a.p95, a.p99, a.max),
            (b.rays, b.median, b.p95, b.p99, b.max)
        );
        assert!((a.mean - b.mean).abs() < 1e-9);
        assert!((a.capped_share - b.capped_share).abs() < 1e-12);
        assert!((a.hit_share - b.hit_share).abs() < 1e-12);
    }

    #[test]
    fn an_empty_histogram_is_zeros() {
        let s = summarize(&vec![0; HIST_WORDS], 128);
        assert_eq!((s.rays, s.median, s.max), (0, 0, 0));
    }
}
