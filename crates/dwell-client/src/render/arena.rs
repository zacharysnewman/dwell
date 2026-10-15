//! Meshes packed into a few large GPU buffers ("pages"), one set of pages per vertex format, so a
//! run of draws from one page binds its vertex and index buffers once and each draw is a single
//! `drawIndexed` (first index, base vertex). One buffer per mesh needed two bind calls per draw,
//! each a call from WASM into JS — most of the frame's encoding time (docs/DEV_TOOLS.md, 2026-10-14).
//!
//! A page holds a mesh's vertices (at a multiple of the format's stride, so `base_vertex` reaches
//! them) and its indices (u32), each allocated from the page's [`FreeList`]. Meshes are written with
//! `Queue::write_buffer`, ordered after the frames already submitted, so a region freed and reused
//! never changes what an earlier frame draws.

use std::cell::RefCell;
use std::rc::Rc;

/// A page's size: large enough for hundreds of meshes, small enough to allocate anywhere.
pub const PAGE_BYTES: u64 = 32 << 20;

/// First-fit allocation over `0..size` with coalescing frees. Pure (tested without a GPU).
#[derive(Debug)]
pub struct FreeList {
    size: u64,
    /// Free spans (offset, length), sorted by offset, never touching (merged on free).
    free: Vec<(u64, u64)>,
}

impl FreeList {
    pub fn new(size: u64) -> Self {
        Self {
            size,
            free: vec![(0, size)],
        }
    }

    /// An offset that is a multiple of `align` with `len` free bytes from it, taken; `None` when no
    /// free span is large enough.
    pub fn alloc(&mut self, len: u64, align: u64) -> Option<u64> {
        let len = len.max(1);
        for i in 0..self.free.len() {
            let (start, span) = self.free[i];
            let at = start.div_ceil(align) * align;
            let pad = at - start;
            if pad + len > span {
                continue;
            }
            let end = start + span;
            // What is left before (alignment padding) and after the allocation stays free.
            let mut rest = Vec::with_capacity(2);
            if pad > 0 {
                rest.push((start, pad));
            }
            if at + len < end {
                rest.push((at + len, end - at - len));
            }
            self.free.splice(i..=i, rest);
            return Some(at);
        }
        None
    }

    /// Returns `offset..offset + len` (as allocated) to the free spans, merging neighbours.
    pub fn free(&mut self, offset: u64, len: u64) {
        let len = len.max(1);
        let i = self.free.partition_point(|&(o, _)| o < offset);
        self.free.insert(i, (offset, len));
        // Merge with the next, then with the previous.
        if i + 1 < self.free.len() && self.free[i].0 + self.free[i].1 == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == self.free[i].0 {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
    }

    pub fn free_bytes(&self) -> u64 {
        self.free.iter().map(|&(_, l)| l).sum()
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

/// Where a mesh lives: its page and the byte regions of its vertices and indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub format: usize,
    pub page: usize,
    pub vertex_offset: u64,
    pub vertex_len: u64,
    pub index_offset: u64,
    pub index_len: u64,
}

struct Page {
    buffer: wgpu::Buffer,
    free: FreeList,
}

/// The pages of each vertex format (by index; the renderer's `Kind`).
pub struct Arena {
    device: wgpu::Device,
    queue: wgpu::Queue,
    strides: Vec<u64>,
    pages: Vec<Vec<Page>>,
}

pub type ArenaRef = Rc<RefCell<Arena>>;

impl Arena {
    /// `strides`: bytes per vertex of each format.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, strides: &[u64]) -> ArenaRef {
        Rc::new(RefCell::new(Self {
            device,
            queue,
            strides: strides.to_vec(),
            pages: strides.iter().map(|_| Vec::new()).collect(),
        }))
    }

    /// Copies a mesh in: `vertices` (interleaved, the format's stride) and `indices` (relative to
    /// its first vertex).
    pub fn upload(&mut self, format: usize, vertices: &[u8], indices: &[u8]) -> Region {
        let stride = self.strides[format];
        let (vlen, ilen) = (vertices.len() as u64, indices.len() as u64);
        let fits = |p: &mut Page| -> Option<(u64, u64)> {
            let v = p.free.alloc(vlen, stride)?;
            match p.free.alloc(ilen, 4) {
                Some(i) => Some((v, i)),
                None => {
                    p.free.free(v, vlen);
                    None
                }
            }
        };
        let pages = &mut self.pages[format];
        let found = pages
            .iter_mut()
            .enumerate()
            .find_map(|(n, p)| fits(p).map(|(v, i)| (n, v, i)));
        let (page, v, i) = match found {
            Some(f) => f,
            None => {
                // A page of the usual size, or one this mesh fills on its own.
                let size = PAGE_BYTES.max((vlen + stride + ilen + 4).next_multiple_of(4));
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("mesh page"),
                    size,
                    usage: wgpu::BufferUsages::VERTEX
                        | wgpu::BufferUsages::INDEX
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut p = Page {
                    buffer,
                    free: FreeList::new(size),
                };
                let (v, i) = fits(&mut p).expect("a new page holds the mesh");
                pages.push(p);
                (pages.len() - 1, v, i)
            }
        };
        let buffer = &self.pages[format][page].buffer;
        self.queue.write_buffer(buffer, v, vertices);
        self.queue.write_buffer(buffer, i, indices);
        Region {
            format,
            page,
            vertex_offset: v,
            vertex_len: vlen,
            index_offset: i,
            index_len: ilen,
        }
    }

    pub fn free(&mut self, r: &Region) {
        if let Some(p) = self.pages.get_mut(r.format).and_then(|f| f.get_mut(r.page)) {
            p.free.free(r.vertex_offset, r.vertex_len);
            p.free.free(r.index_offset, r.index_len);
        }
    }

    pub fn buffer(&self, format: usize, page: usize) -> &wgpu::Buffer {
        &self.pages[format][page].buffer
    }

    /// The vertex index of the region's first vertex (`base_vertex`).
    pub fn base_vertex(&self, r: &Region) -> i32 {
        (r.vertex_offset / self.strides[r.format]) as i32
    }

    /// Bytes of the pages in use by meshes.
    pub fn used_bytes(&self) -> u64 {
        self.pages
            .iter()
            .flatten()
            .map(|p| p.free.size() - p.free.free_bytes())
            .sum()
    }

    /// The pages' buffers (tests: they are destroyed with the arena).
    pub fn buffers(&self) -> Vec<wgpu::Buffer> {
        self.pages
            .iter()
            .flatten()
            .map(|p| p.buffer.clone())
            .collect()
    }

    /// GPU bytes held by pages (used or not).
    pub fn page_bytes(&self) -> u64 {
        self.pages.iter().flatten().map(|p| p.free.size()).sum()
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        for p in self.pages.iter().flatten() {
            p.buffer.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_aligned_and_reuses_freed_space() {
        let mut f = FreeList::new(1000);
        let a = f.alloc(60, 60).expect("a");
        assert_eq!(a, 0);
        let b = f.alloc(10, 4).expect("b");
        assert_eq!(b, 60);
        // The next stride-aligned offset after 70 is 120; 70..120 stays free.
        let c = f.alloc(60, 60).expect("c");
        assert_eq!(c, 120);
        assert_eq!(f.alloc(40, 4), Some(72));
        f.free(a, 60);
        assert_eq!(f.alloc(60, 60), Some(0), "first fit reuses the freed span");
        assert!(f.alloc(2000, 4).is_none());
    }

    #[test]
    fn frees_merge_back_into_one_span() {
        let mut f = FreeList::new(256);
        let spans: Vec<u64> = (0..8).map(|_| f.alloc(32, 4).expect("fits")).collect();
        assert!(f.alloc(1, 1).is_none(), "full");
        // Freed out of order, they coalesce into the whole range.
        for &i in &[3usize, 1, 2, 0, 7, 5, 6, 4] {
            f.free(spans[i], 32);
        }
        assert_eq!(f.free, vec![(0, 256)]);
        assert_eq!(f.alloc(256, 4), Some(0));
    }

    #[test]
    fn random_allocations_never_overlap() {
        let mut f = FreeList::new(1 << 16);
        let mut held: Vec<(u64, u64)> = Vec::new();
        let mut seed = 12345u32;
        let mut rand = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        for _ in 0..5000 {
            if rand(3) > 0 || held.is_empty() {
                let len = u64::from(1 + rand(900));
                let align = [4u64, 36, 60][rand(3) as usize];
                if let Some(at) = f.alloc(len, align) {
                    assert_eq!(at % align, 0);
                    assert!(at + len <= 1 << 16);
                    for &(o, l) in &held {
                        assert!(at + len <= o || o + l <= at, "overlap");
                    }
                    held.push((at, len));
                }
            } else {
                let (o, l) = held.swap_remove(rand(held.len() as u32) as usize);
                f.free(o, l);
            }
        }
        for (o, l) in held.drain(..) {
            f.free(o, l);
        }
        assert_eq!(f.free_bytes(), 1 << 16);
        assert_eq!(f.free.len(), 1);
    }
}
