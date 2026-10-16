//! Compare mode's difference view on the GPU (compare.rs has its rules): the right side's scene
//! targets and the counts buffer the `difference_fs` shader adds to (group 3), and the read back of
//! those counts — a ring of buffers, each copied into only when free (unmapped, no map pending),
//! mapped after the submit and read on a later frame, never waited on (as the pass timer's ring,
//! timing.rs). A frame finding every buffer still in use skips its read back.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::compare::DiffCounts;
use super::frame::Targets;

const BYTES: u64 = 16;
/// Readback buffers: the browser's map callback can land frames after the submit.
pub const RING: usize = 3;
const FREE: u8 = 0;
const COPIED: u8 = 1;
const PENDING: u8 = 2;
const MAPPED: u8 = 3;
const FAILED: u8 = 4;

/// Group 3 of the difference pipeline: the right side's colour and view depth (as the composite
/// reads a side, `frame::scene_layout`), and the counts (pixels, coverage, depth, colour).
pub fn layout(device: &wgpu::Device, samples: u32) -> wgpu::BindGroupLayout {
    let texture = |binding, multisampled| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("difference"),
        entries: &[
            texture(0, false),
            texture(1, samples > 1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(BYTES),
                },
                count: None,
            },
        ],
    })
}

/// The readback buffers' states: free → copied (this frame's copy) → pending (mapping, after the
/// submit) → mapped or failed (set by the map callback) → free (read and unmapped). Only a free
/// buffer is copied into and only a copied one mapped, so none is used in a submit while mapped
/// or mapped twice.
#[derive(Debug, Default)]
pub struct Slots {
    state: [Arc<AtomicU8>; RING],
    next: usize,
    /// The buffer this frame copied into, between `copy` and `submitted`.
    current: Option<usize>,
}

impl Slots {
    /// A free buffer to copy this frame's counts into, taken in turn; none when every one is in use.
    pub fn copy(&mut self) -> Option<usize> {
        let i = (0..RING).map(|k| (self.next + k) % RING).find(|&i| {
            self.state[i]
                .compare_exchange(FREE, COPIED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        })?;
        self.next = (i + 1) % RING;
        self.current = Some(i);
        Some(i)
    }

    /// After the submit: the buffer copied into, now pending, and the flag its map callback sets
    /// (with `mapped`).
    pub fn submitted(&mut self) -> Option<(usize, Arc<AtomicU8>)> {
        let i = self.current.take()?;
        self.state[i].store(PENDING, Ordering::Release);
        Some((i, self.state[i].clone()))
    }

    /// The map callback's result.
    pub fn mapped(flag: &AtomicU8, ok: bool) {
        flag.store(if ok { MAPPED } else { FAILED }, Ordering::Release);
    }

    /// Buffers mapped and ready to read (each then `release`d); failed maps are freed.
    pub fn ready(&mut self) -> Vec<usize> {
        let mut out = Vec::new();
        for i in 0..RING {
            match self.state[i].load(Ordering::Acquire) {
                MAPPED => out.push(i),
                FAILED => self.state[i].store(FREE, Ordering::Release),
                _ => {}
            }
        }
        out
    }

    /// A read buffer, unmapped: free again.
    pub fn release(&mut self, i: usize) {
        self.state[i].store(FREE, Ordering::Release);
    }
}

pub struct Difference {
    counts: wgpu::Buffer,
    readback: [wgpu::Buffer; RING],
    slots: Slots,
    /// The latest counts read back.
    pub last: Option<DiffCounts>,
}

impl Difference {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            counts: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("difference counts"),
                size: BYTES,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: std::array::from_fn(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("difference readback"),
                    size: BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            }),
            slots: Slots::default(),
            last: None,
        }
    }

    /// Group 3 for the right side's `targets`.
    pub fn group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        targets: &Targets,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("difference"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&targets.colour.1),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&targets.view_depth.1),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.counts.as_entire_binding(),
                },
            ],
        })
    }

    /// Reads counts the GPU has finished with (call once a frame, before drawing): the newest
    /// mapped buffer's, freeing every mapped one.
    pub fn collect(&mut self) {
        for i in self.slots.ready() {
            if let Ok(view) = self.readback[i].get_mapped_range(..) {
                let raw: Vec<u32> = view
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
                    .collect();
                drop(view);
                self.last = Some(DiffCounts::from_raw([raw[0], raw[1], raw[2], raw[3]]));
            }
            self.readback[i].unmap();
            self.slots.release(i);
        }
    }

    /// Zeroes the counts for this frame's difference pass.
    pub fn reset(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.counts, 0, &[0; BYTES as usize]);
    }

    /// After the difference pass: copies the counts out when a readback buffer is free.
    pub fn copy(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(i) = self.slots.copy() {
            encoder.copy_buffer_to_buffer(&self.counts, 0, &self.readback[i], 0, BYTES);
        }
    }

    /// After the submit: maps this frame's copy, read by `collect` once mapped.
    pub fn submitted(&mut self) {
        if let Some((i, flag)) = self.slots.submitted() {
            self.readback[i].map_async(wgpu::MapMode::Read, .., move |r| {
                Slots::mapped(&flag, r.is_ok());
            });
        }
    }
}

impl Drop for Difference {
    fn drop(&mut self) {
        self.counts.destroy();
        for b in &self.readback {
            b.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readback_buffer_is_mapped_once_and_copied_into_only_when_free() {
        // The browser's map callback can take more than a frame: a buffer copied and mapped must
        // not be mapped again, or copied into, until it has been read and unmapped
        // ("buffer used in submit while mapped").
        let mut s = Slots::default();
        let first = s.copy().expect("a free buffer");
        let (i, flag) = s.submitted().expect("mapped after the submit");
        assert_eq!(i, first);
        // Next frame, the map still pending: nothing to read, and no buffer copied is this one.
        assert!(s.ready().is_empty());
        let second = s.copy();
        assert_ne!(second, Some(first), "copied into a buffer still mapping");
        let again = s.submitted();
        assert!(
            again.as_ref().is_none_or(|(j, _)| *j != first),
            "mapped a buffer twice"
        );
        // Its map lands: read, then free again.
        Slots::mapped(&flag, true);
        assert_eq!(s.ready(), vec![first]);
        s.release(first);
    }

    #[test]
    fn frames_skip_the_read_back_while_every_buffer_is_in_use() {
        let mut s = Slots::default();
        let mut flags = Vec::new();
        for want in 0..RING {
            assert_eq!(s.copy(), Some(want));
            flags.push(s.submitted().expect("mapped").1);
        }
        // Every map pending: the frame copies nothing and maps nothing.
        assert_eq!(s.copy(), None);
        assert!(s.submitted().is_none());
        // A failed map frees its buffer without a read; a landed one is read, then freed.
        Slots::mapped(&flags[1], false);
        Slots::mapped(&flags[2], true);
        assert_eq!(s.ready(), vec![2]);
        s.release(2);
        assert_eq!(s.copy(), Some(1));
        assert_eq!(s.copy(), Some(2));
        assert_eq!(s.copy(), None, "buffer 0 still pending");
    }
}
