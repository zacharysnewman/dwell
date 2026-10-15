//! GPU pass timing (docs/DEV_TOOLS.md): a timestamp at the start and end of each of the frame's
//! passes, resolved and copied into one of a ring of readback buffers, mapped after the submit and
//! read on a later frame once the GPU has finished with it. A frame finding every buffer still in
//! flight goes untimed: reading never waits on the GPU. Only when the device has
//! `Features::TIMESTAMP_QUERY` (browsers coarsen timestamps to 100 µs unless Chrome's WebGPU
//! developer features are on: the bench turns them on).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// The frame's passes in order, as their telemetry samples are named.
pub const PASSES: [&str; 3] = ["gpu.far", "gpu.near", "gpu.composite"];
/// The whole frame on the GPU: the first pass's start to the last one's end.
pub const FRAME: &str = "gpu.frame";
/// Readback buffers: the GPU runs a frame or two behind, so three keep one free.
pub const RING: usize = 3;

/// Which readback buffers are in use (from the frame that writes one until its values are read).
#[derive(Debug, Default)]
pub struct Ring {
    busy: [bool; RING],
    next: usize,
}

impl Ring {
    /// A free buffer for this frame, taken in turn; none when every one is still in use.
    pub fn acquire(&mut self) -> Option<usize> {
        let i = (0..RING)
            .map(|k| (self.next + k) % RING)
            .find(|&i| !self.busy[i])?;
        self.busy[i] = true;
        self.next = (i + 1) % RING;
        Some(i)
    }

    pub fn release(&mut self, i: usize) {
        self.busy[i] = false;
    }
}

/// A frame's samples (ms) from its raw timestamps (a begin and an end per pass, in ticks of
/// `period_ns`): each pass that ran forward, and the whole frame. A pass whose end is not after its
/// begin (unwritten, or a counter reset between them) is left out.
pub fn samples(raw: &[u64], period_ns: f64) -> Vec<(&'static str, f64)> {
    samples_of(raw, period_ns, &PASSES, FRAME)
}

/// [`samples`] for any list of pass names and the whole frame's name.
pub fn samples_of(
    raw: &[u64],
    period_ns: f64,
    passes: &[&'static str],
    frame: &'static str,
) -> Vec<(&'static str, f64)> {
    let ms = |a: u64, b: u64| (b > a).then(|| (b - a) as f64 * period_ns / 1e6);
    let mut out: Vec<(&'static str, f64)> = passes
        .iter()
        .zip(raw.chunks_exact(2))
        .filter_map(|(name, t)| ms(t[0], t[1]).map(|v| (*name, v)))
        .collect();
    if out.len() == passes.len()
        && let Some(v) = ms(raw[0], raw[raw.len() - 1])
    {
        out.push((frame, v));
    }
    out
}

const IN_FLIGHT: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// The timestamp queries and their readback ring.
pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: [wgpu::Buffer; RING],
    /// Set by each buffer's map callback.
    mapped: [Arc<AtomicU8>; RING],
    ring: Ring,
    /// The buffer this frame writes, between `begin` and `submitted`.
    current: Option<usize>,
    period_ns: f64,
    passes: &'static [&'static str],
    frame: &'static str,
}

impl GpuTimer {
    /// A timer of the renderer's passes ([`PASSES`]), when the device can take timestamps.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        Self::with_passes(device, queue, &PASSES, FRAME)
    }

    /// A timer of other passes (the marcher's harness: `gpu.march`, `gpu.composite`), and the
    /// name of their whole frame.
    pub fn with_passes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        passes: &'static [&'static str],
        frame: &'static str,
    ) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let queries = 2 * passes.len() as u32;
        let bytes = u64::from(queries) * u64::from(wgpu::QUERY_SIZE);
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("pass timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: queries,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamps"),
            size: bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("timestamps readback"),
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Some(Self {
            queries,
            resolve,
            readback,
            mapped: std::array::from_fn(|_| Arc::new(AtomicU8::new(IN_FLIGHT))),
            ring: Ring::default(),
            current: None,
            period_ns: f64::from(queue.get_timestamp_period()),
            passes,
            frame,
        })
    }

    /// The samples of every earlier frame the GPU has finished, freeing their buffers.
    pub fn collect(&mut self) -> Vec<(&'static str, f64)> {
        let mut out = Vec::new();
        for i in 0..RING {
            if !self.ring.busy[i] || Some(i) == self.current {
                continue;
            }
            match self.mapped[i].load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(view) = self.readback[i].get_mapped_range(..) {
                        let raw: Vec<u64> = view
                            .chunks_exact(8)
                            .map(|b| u64::from_le_bytes(b.try_into().expect("8 bytes")))
                            .collect();
                        drop(view);
                        out.extend(samples_of(&raw, self.period_ns, self.passes, self.frame));
                    }
                    self.readback[i].unmap();
                }
                FAILED => {}
                _ => continue,
            }
            self.mapped[i].store(IN_FLIGHT, Ordering::Release);
            self.ring.release(i);
        }
        out
    }

    /// Readback buffers waiting to be read.
    pub fn in_use(&self) -> usize {
        self.ring.busy.iter().filter(|b| **b).count()
    }

    /// Starts a frame: whether it is timed (a readback buffer was free).
    pub fn begin(&mut self) -> bool {
        self.current = self.ring.acquire();
        self.current.is_some()
    }

    /// The timestamp writes of pass `pass` (an index into `PASSES`) of a timed frame.
    pub fn writes(&self, pass: usize) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.current?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(2 * pass as u32),
            end_of_pass_write_index: Some(2 * pass as u32 + 1),
        })
    }

    /// The timestamp writes of compute pass `pass` of a timed frame.
    pub fn compute_writes(&self, pass: usize) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        self.current?;
        Some(wgpu::ComputePassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(2 * pass as u32),
            end_of_pass_write_index: Some(2 * pass as u32 + 1),
        })
    }

    /// Records the copy of a timed frame's timestamps into its readback buffer (after the passes).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(i) = self.current else {
            return;
        };
        let queries = 2 * self.passes.len() as u32;
        encoder.resolve_query_set(&self.queries, 0..queries, &self.resolve, 0);
        let bytes = u64::from(queries) * u64::from(wgpu::QUERY_SIZE);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback[i], 0, bytes);
    }

    /// After the frame's submit: maps its readback buffer, to be read by `collect` once mapped.
    pub fn submitted(&mut self) {
        let Some(i) = self.current.take() else {
            return;
        };
        let flag = self.mapped[i].clone();
        self.readback[i].map_async(wgpu::MapMode::Read, .., move |r| {
            flag.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
        });
    }
}

impl Drop for GpuTimer {
    fn drop(&mut self) {
        self.queries.destroy();
        self.resolve.destroy();
        for b in &self.readback {
            b.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_hands_out_free_buffers_in_turn_and_never_waits() {
        let mut r = Ring::default();
        assert_eq!(
            (r.acquire(), r.acquire(), r.acquire()),
            (Some(0), Some(1), Some(2))
        );
        // All three in flight: the frame goes untimed instead of waiting.
        assert_eq!(r.acquire(), None);
        r.release(1);
        assert_eq!(r.acquire(), Some(1));
        assert_eq!(r.acquire(), None);
        r.release(0);
        r.release(2);
        // In turn from the last one handed out.
        assert_eq!(r.acquire(), Some(2));
        assert_eq!(r.acquire(), Some(0));
    }

    #[test]
    fn timestamps_become_pass_and_frame_milliseconds() {
        // far 1.0 ms, near 0.5 ms, composite 0.25 ms, in 1 ns ticks.
        let raw = [
            1_000_000, 2_000_000, 2_100_000, 2_600_000, 2_700_000, 2_950_000,
        ];
        assert_eq!(
            samples(&raw, 1.0),
            vec![
                ("gpu.far", 1.0),
                ("gpu.near", 0.5),
                ("gpu.composite", 0.25),
                ("gpu.frame", 1.95)
            ]
        );
        // Ticks of another period scale.
        assert_eq!(samples(&raw, 2.0)[0], ("gpu.far", 2.0));
        // Other passes: the marcher's harness.
        assert_eq!(
            samples_of(&raw[..4], 1.0, &["gpu.march", "gpu.composite"], "gpu.frame"),
            vec![
                ("gpu.march", 1.0),
                ("gpu.composite", 0.5),
                ("gpu.frame", 1.6)
            ]
        );
    }

    #[test]
    fn a_pass_without_forward_time_is_left_out() {
        // Unwritten (zeros) or reset between begin and end: no sample, and no frame total.
        let raw = [0, 0, 2_100_000, 2_600_000, 3_000_000, 10];
        assert_eq!(samples(&raw, 1.0), vec![("gpu.near", 0.5)]);
    }
}
