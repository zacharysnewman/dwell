//! The GPU mirror of `dwell_march::Scene` (docs/RAY_MARCHED_RENDERER.md §4.1): one storage buffer
//! per word array of the layout, grown by doubling (a grown buffer is re-filled from the CPU copy,
//! which holds every word anyway), and otherwise written incrementally — only the slots, nodes,
//! column grids and pool ranges `Scene::take_dirty` names, each a `queue.writeBuffer`, coalesced
//! where they touch. The allocator lives on the CPU (`dwell_march::alloc`): the GPU pool is the same
//! words at the same offsets, so a brick's offset in a node is valid on both.
//!
//! **Limits.** Each buffer is one binding, so it must fit `max_storage_buffer_binding_size`
//! (WebGPU's default is 128 MiB; the device's own limit is read, never assumed) and
//! `max_buffer_size`. The pool is the only buffer that gets near it (35 MiB measured for today's
//! view distance, §4.3); a build that would push it past the limit is not placed (its node stays
//! NONE, so a coarser level draws there) and counted (`march.pool_full`).

use dwell_march::layout::NODE_WORDS;
use dwell_march::{NodeBuild, Scene, StateTable};

/// The storage buffers, in binding order 1..=6 (binding 0 is the uniforms, 7 the stats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Buf {
    Slots,
    Nodes,
    NodeColumns,
    Columns,
    Pool,
    Looks,
}

const BUFS: [Buf; 6] = [
    Buf::Slots,
    Buf::Nodes,
    Buf::NodeColumns,
    Buf::Columns,
    Buf::Pool,
    Buf::Looks,
];

/// Word ranges `(start, len)` sorted and joined where they touch or overlap.
pub fn coalesce(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (at, n) in ranges {
        if n == 0 {
            continue;
        }
        match out.last_mut() {
            Some((a, m)) if at <= *a + *m => *m = (*m).max(at + n - *a),
            _ => out.push((at, n)),
        }
    }
    out
}

/// A buffer's capacity in words for `words` (doubling from 1,024, at least 4 words).
pub fn capacity_for(words: usize, current: usize) -> usize {
    if words <= current {
        return current;
    }
    let mut c = current.max(1024);
    while c < words {
        c *= 2;
    }
    c
}

struct GpuBuf {
    buffer: wgpu::Buffer,
    words: usize,
}

pub struct GpuScene {
    pub scene: Scene,
    looks: Vec<u32>,
    bufs: Vec<GpuBuf>,
    /// The bind group must be made again (a buffer was replaced).
    pub rebind: bool,
    /// Bytes of the binding limit (the smaller of the device's binding and buffer sizes).
    limit: u64,
    pub uploaded_bytes: u64,
    pub refused: u64,
    /// The first sync writes every buffer (the looks among them).
    synced: bool,
}

impl GpuScene {
    pub fn new(device: &wgpu::Device, table: StateTable, looks: Vec<u32>) -> Self {
        let limits = device.limits();
        let limit = u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        let bufs = BUFS
            .iter()
            .map(|b| GpuBuf {
                buffer: make(device, *b, 1024),
                words: 1024,
            })
            .collect();
        Self {
            scene: Scene::new(table),
            looks,
            bufs,
            rebind: true,
            limit,
            uploaded_bytes: 0,
            refused: 0,
            synced: false,
        }
    }

    pub fn buffer(&self, i: usize) -> &wgpu::Buffer {
        &self.bufs[i].buffer
    }

    /// Whether a build fits the pool's binding (counting it all as new words: freed slots of its
    /// size class may take some of it, so this errs on the side of refusing).
    pub fn fits(&self, build: &NodeBuild) -> bool {
        let words = self.scene.pool_words().len() as u64 + build.gpu_bytes() as u64 / 4;
        4 * words <= self.limit
    }

    /// Places a build at node `n` of `level`, unless the pool would not fit its binding.
    pub fn put(&mut self, level: u32, n: [i32; 3], build: &NodeBuild) -> bool {
        if !self.fits(build) {
            self.refused += 1;
            crate::telemetry::count("march.pool_full", 1.0);
            return false;
        }
        self.scene.put(level, n, build);
        true
    }

    fn words_of(&self, b: Buf) -> usize {
        match b {
            Buf::Slots => self.scene.slot_words().len(),
            Buf::Nodes => self.scene.node_count() * NODE_WORDS,
            Buf::NodeColumns => self.scene.node_columns().len(),
            Buf::Columns => self.scene.column_words().len(),
            Buf::Pool => self.scene.pool_words().len(),
            Buf::Looks => self.looks.len(),
        }
    }

    /// Uploads what changed since the last sync. Returns the bytes written.
    pub fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> u64 {
        let dirty = self.scene.take_dirty();
        let mut bytes = 0u64;
        let mut write = |buffer: &wgpu::Buffer, at: usize, words: &[u32]| {
            if !words.is_empty() {
                queue.write_buffer(buffer, 4 * at as u64, bytemuck::cast_slice(words));
                bytes += 4 * words.len() as u64;
            }
        };
        let full = !self.synced;
        self.synced = true;
        let max_words = (self.limit / 4) as usize;
        for (i, &b) in BUFS.iter().enumerate() {
            let need = self.words_of(b);
            let cap = capacity_for(need, self.bufs[i].words)
                .min(max_words)
                .max(need);
            let grown = cap != self.bufs[i].words;
            if grown {
                self.bufs[i].buffer.destroy();
                self.bufs[i] = GpuBuf {
                    buffer: make(device, b, cap),
                    words: cap,
                };
                self.rebind = true;
            }
            let buffer = &self.bufs[i].buffer;
            let s = &self.scene;
            let all = grown || full;
            match b {
                Buf::Slots if all || dirty.layout => write(buffer, 0, &s.slot_words()),
                Buf::Slots => {
                    let ranges = coalesce(dirty.slots.iter().map(|&i| (i, 1)).collect());
                    for (at, n) in ranges {
                        let w: Vec<u32> = (at..at + n).map(|i| s.slot_word(i)).collect();
                        write(buffer, at as usize, &w);
                    }
                }
                Buf::Nodes | Buf::NodeColumns if all => {
                    if b == Buf::Nodes {
                        let w: Vec<u32> = (0..s.node_count() as u32)
                            .flat_map(|i| s.node_words(i))
                            .collect();
                        write(buffer, 0, &w);
                    } else {
                        write(buffer, 0, s.node_columns());
                    }
                }
                Buf::Nodes => {
                    for (at, n) in coalesce(dirty.nodes.iter().map(|&i| (i, 1)).collect()) {
                        let w: Vec<u32> = (at..at + n).flat_map(|i| s.node_words(i)).collect();
                        write(buffer, at as usize * NODE_WORDS, &w);
                    }
                }
                Buf::NodeColumns => {
                    for (at, n) in coalesce(dirty.nodes.iter().map(|&i| (i, 1)).collect()) {
                        let (a, z) = (at as usize, (at + n) as usize);
                        write(buffer, a, &s.node_columns()[a..z]);
                    }
                }
                Buf::Columns if all => write(buffer, 0, s.column_words()),
                Buf::Columns => {
                    let len = dwell_march::layout::COLUMN_WORDS as u32;
                    for (at, n) in coalesce(dirty.columns.iter().map(|&c| (c, len)).collect()) {
                        let (a, z) = (at as usize, (at + n) as usize);
                        write(buffer, a, &s.column_words()[a..z]);
                    }
                }
                Buf::Pool if all => write(buffer, 0, s.pool_words()),
                Buf::Pool => {
                    for (at, n) in coalesce(dirty.pool.clone()) {
                        let (a, z) = (at as usize, (at + n) as usize);
                        write(buffer, a, &s.pool_words()[a..z]);
                    }
                }
                Buf::Looks if all => write(buffer, 0, &self.looks),
                Buf::Looks => {}
            }
        }
        self.uploaded_bytes += bytes;
        bytes
    }

    /// GPU bytes held (the buffers' capacities).
    pub fn gpu_bytes(&self) -> u64 {
        self.bufs.iter().map(|b| 4 * b.words as u64).sum()
    }
}

impl Drop for GpuScene {
    fn drop(&mut self) {
        for b in &self.bufs {
            b.buffer.destroy();
        }
    }
}

fn make(device: &wgpu::Device, b: Buf, words: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(match b {
            Buf::Slots => "march slots",
            Buf::Nodes => "march nodes",
            Buf::NodeColumns => "march node columns",
            Buf::Columns => "march columns",
            Buf::Pool => "march pool",
            Buf::Looks => "march looks",
        }),
        size: 4 * words as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_coalesce_where_they_touch() {
        assert_eq!(
            coalesce(vec![(10, 2), (0, 3), (3, 2), (12, 1), (20, 0), (11, 5)]),
            vec![(0, 5), (10, 6)]
        );
    }

    #[test]
    fn buffers_double_and_never_shrink() {
        assert_eq!(capacity_for(10, 0), 1024);
        assert_eq!(capacity_for(1025, 1024), 2048);
        assert_eq!(capacity_for(5000, 1024), 8192);
        assert_eq!(capacity_for(10, 8192), 8192);
    }
}
