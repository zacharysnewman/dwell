//! Compare mode's difference view on the GPU (compare.rs has its rules): the right side's scene
//! targets and the counts buffer the `difference_fs` shader adds to (group 3), and the read back of
//! those counts — one buffer, copied and mapped when it is free, read on a later frame, never
//! waited on (as the pass timer does, timing.rs).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::compare::DiffCounts;
use super::frame::Targets;

const BYTES: u64 = 16;
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPED: u8 = 2;
const FAILED: u8 = 3;

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

pub struct Difference {
    counts: wgpu::Buffer,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
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
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("difference readback"),
                size: BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(IDLE)),
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

    /// Reads counts the GPU has finished with (call once a frame, before drawing).
    pub fn collect(&mut self) {
        match self.state.load(Ordering::Acquire) {
            MAPPED => {
                if let Ok(view) = self.readback.get_mapped_range(..) {
                    let raw: Vec<u32> = view
                        .chunks_exact(4)
                        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
                        .collect();
                    drop(view);
                    self.last = Some(DiffCounts::from_raw([raw[0], raw[1], raw[2], raw[3]]));
                }
                self.readback.unmap();
                self.state.store(IDLE, Ordering::Release);
            }
            FAILED => self.state.store(IDLE, Ordering::Release),
            _ => {}
        }
    }

    /// Zeroes the counts for this frame's difference pass.
    pub fn reset(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.counts, 0, &[0; BYTES as usize]);
    }

    /// After the difference pass: copies the counts out when the readback buffer is free.
    pub fn copy(&self, encoder: &mut wgpu::CommandEncoder) {
        if self
            .state
            .compare_exchange(IDLE, COPIED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            encoder.copy_buffer_to_buffer(&self.counts, 0, &self.readback, 0, BYTES);
        }
    }

    /// After the submit: maps the copy, read by `collect` once mapped.
    pub fn submitted(&self) {
        if self.state.load(Ordering::Acquire) != COPIED {
            return;
        }
        let flag = self.state.clone();
        self.readback.map_async(wgpu::MapMode::Read, .., move |r| {
            flag.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
        });
    }
}

impl Drop for Difference {
    fn drop(&mut self) {
        self.counts.destroy();
        self.readback.destroy();
    }
}
