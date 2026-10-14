//! The server's LOD sections (ARCHITECTURE.md §6.6): which sections are modified (any modified
//! chunk below them), their content and lodRevision, and the propagation that keeps them current.
//!
//! A chunk change marks its level-1 section dirty. Each tick, up to
//! LOD_PROPAGATION_SECTIONS_PER_TICK dirty sections — lowest level first, then nearest to a player
//! — are snapshotted (their modified children's content) and computed off the tick: the generated
//! section with the octants of modified children replaced by their downsample. Writing a section
//! gives it the next lodRevision and marks its parent dirty, up to the root. With no threads (the
//! browser's local mode) the jobs run on the calling thread within a time budget. The same pool
//! generates unmodified sections for full-chunk-mode clients.

use dwell_world::hash::{HashMap, HashSet};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use dwell_blocks::require;
use dwell_protocol::{decode_lod_cells, encode_lod_cells};
use dwell_world::lod::{
    LodCells, LodCoord, LodGenerator, MAX_LEVEL, PAD, SECTION_CELLS, VOLUME, cell_index, child,
    chunk_of_lod, downsample_octant, downsample_section_octant, in_world, lod_of_chunk, parent,
    section_origin, section_size,
};
use dwell_world::{Chunk, ChunkCoord, local_index};

use crate::clock::Deadline;

const MAX_GENERATED: usize = 256; // full-chunk-mode sections kept for answering

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LodSection {
    pub revision: u32,
    /// Palette + RLE of the 34³ cells (`encode_lod_cells`).
    pub encoded: Vec<u8>,
}

struct Job {
    coord: LodCoord,
    generate_only: bool,
    /// Modified children (octant order): chunk voxels (level 1) or section cells (level ≥ 2).
    children: [Vec<u16>; 8],
}

struct Done {
    coord: LodCoord,
    generate_only: bool,
    encoded: Vec<u8>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    done: Vec<Done>,
    running: usize,
    stop: bool,
}

struct Shared {
    q: Mutex<Queue>,
    work: Condvar,
    idle: Condvar,
}

pub struct LodPropagation {
    generator: LodGenerator,
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    // Tick thread only.
    sections: HashMap<LodCoord, LodSection>,
    dirty: HashSet<LodCoord>,
    in_flight: HashSet<LodCoord>,
    written: Vec<LodCoord>,
    next_revision: u32,
    // Full-chunk mode: generated sections (bounded), and those queued.
    generated: HashMap<LodCoord, Vec<u8>>,
    generated_order: VecDeque<LodCoord>,
    generating: HashSet<LodCoord>,
}

fn decode(encoded: &[u8]) -> LodCells {
    decode_lod_cells(encoded).unwrap_or_else(|| vec![require("dwell:air"); VOLUME])
}

fn distance_sq(c: LodCoord, centers: &[[f64; 3]]) -> f64 {
    if centers.is_empty() {
        return 0.0;
    }
    let o = section_origin(c);
    let half = section_size(c.level) as f64 / 2.0;
    centers
        .iter()
        .map(|p| {
            let (dx, dy, dz) = (
                o.x as f64 + half - p[0],
                o.y as f64 + half - p[1],
                o.z as f64 + half - p[2],
            );
            dx * dx + dy * dy + dz * dz
        })
        .fold(f64::INFINITY, f64::min)
}

fn run(generator: &LodGenerator, job: &Job) -> Done {
    let mut cells: LodCells = vec![require("dwell:air"); VOLUME];
    generator(job.coord, &mut cells);
    if !job.generate_only {
        for (o, kid) in job.children.iter().enumerate() {
            if kid.is_empty() {
                continue; // unmodified: the generated octant stays
            }
            if job.coord.level == 1 {
                downsample_octant(
                    |x, y, z| kid[local_index(x, y, z)],
                    job.coord,
                    o as i32,
                    &mut cells,
                );
            } else {
                downsample_section_octant(kid, job.coord, o as i32, &mut cells);
            }
        }
    }
    Done {
        coord: job.coord,
        generate_only: job.generate_only,
        encoded: encode_lod_cells(&cells),
    }
}

impl LodPropagation {
    pub fn new(generator: LodGenerator, threads: usize) -> Self {
        let shared = Arc::new(Shared {
            q: Mutex::new(Queue::default()),
            work: Condvar::new(),
            idle: Condvar::new(),
        });
        let workers = (0..threads)
            .map(|_| {
                let (shared, generator) = (shared.clone(), generator.clone());
                std::thread::spawn(move || Self::work(&shared, &generator))
            })
            .collect();
        Self {
            generator,
            shared,
            workers,
            sections: HashMap::default(),
            dirty: HashSet::default(),
            in_flight: HashSet::default(),
            written: Vec::new(),
            next_revision: 1,
            generated: HashMap::default(),
            generated_order: VecDeque::new(),
            generating: HashSet::default(),
        }
    }

    fn work(shared: &Shared, generator: &LodGenerator) {
        let mut q = shared.q.lock().expect("lod lock");
        loop {
            q = shared
                .work
                .wait_while(q, |q| !q.stop && q.jobs.is_empty())
                .expect("lod lock");
            if q.stop {
                return;
            }
            let job = q.jobs.pop_front().expect("queued");
            q.running += 1;
            drop(q);
            let done = run(generator, &job);
            q = shared.q.lock().expect("lod lock");
            q.running -= 1;
            q.done.push(done);
            shared.idle.notify_all();
        }
    }

    pub fn threads(&self) -> usize {
        self.workers.len()
    }

    /// A chunk's voxels changed (any VoxelModification source).
    pub fn mark_chunk(&mut self, c: ChunkCoord) {
        self.mark_dirty(parent(lod_of_chunk(c)));
    }
    pub fn mark_dirty(&mut self, c: LodCoord) {
        if in_world(c) {
            self.dirty.insert(c);
        }
    }
    /// Loading a saved world: a section's cached content.
    pub fn restore(&mut self, c: LodCoord, section: LodSection) {
        self.next_revision = self.next_revision.max(section.revision + 1);
        self.sections.insert(c, section);
    }

    fn snapshot(
        &self,
        c: LodCoord,
        modified: &mut dyn FnMut(ChunkCoord) -> Option<Vec<u16>>,
    ) -> Job {
        let children = std::array::from_fn(|o| {
            let kid = child(c, o as i32);
            if kid.level == 0 {
                modified(chunk_of_lod(kid)).unwrap_or_default()
            } else {
                self.find(kid)
                    .map(|s| decode(&s.encoded))
                    .unwrap_or_default()
            }
        });
        Job {
            coord: c,
            generate_only: false,
            children,
        }
    }

    fn apply(&mut self, d: Done) {
        if d.generate_only {
            self.generating.remove(&d.coord);
            if self.generated.len() >= MAX_GENERATED
                && let Some(old) = self.generated_order.pop_front()
            {
                self.generated.remove(&old);
            }
            self.generated_order.push_back(d.coord);
            self.generated.insert(d.coord, d.encoded);
            return;
        }
        self.in_flight.remove(&d.coord);
        self.sections.insert(
            d.coord,
            LodSection {
                revision: self.next_revision,
                encoded: d.encoded,
            },
        );
        self.next_revision += 1;
        self.written.push(d.coord);
        if d.coord.level < MAX_LEVEL {
            self.mark_dirty(parent(d.coord));
        }
    }

    fn collect(&mut self) {
        let done = std::mem::take(&mut self.shared.q.lock().expect("lod lock").done);
        for d in done {
            self.apply(d);
        }
    }

    /// Once per tick: collects finished jobs (writing sections, marking parents), then dispatches
    /// dirty sections (at most `budget`) nearest to `centers` (world metres); without threads runs
    /// queued jobs here for up to `inline_budget`. `modified` gives a modified chunk's voxels
    /// (revision > 0 or saved), `None` for an unmodified one.
    pub fn step(
        &mut self,
        centers: &[[f64; 3]],
        modified: &mut dyn FnMut(ChunkCoord) -> Option<Vec<u16>>,
        budget: usize,
        inline_budget: Duration,
    ) {
        self.collect();
        // Lowest level first (children before parents), then nearest to a player.
        if !self.dirty.is_empty() && budget > 0 {
            let mut ready: Vec<((i32, f64), LodCoord)> = self
                .dirty
                .iter()
                .filter(|c| !self.in_flight.contains(c))
                .map(|&c| ((c.level, distance_sq(c, centers)), c))
                .collect();
            ready.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .expect("finite")
                    .then_with(|| (a.1.i, a.1.j, a.1.k).cmp(&(b.1.i, b.1.j, b.1.k)))
            });
            ready.truncate(budget);
            let mut jobs = Vec::with_capacity(ready.len());
            for (_, c) in ready {
                self.dirty.remove(&c);
                self.in_flight.insert(c);
                jobs.push(self.snapshot(c, modified));
            }
            self.shared.q.lock().expect("lod lock").jobs.extend(jobs);
            self.shared.work.notify_all();
        }
        if self.workers.is_empty() {
            let deadline = Deadline::after(inline_budget);
            loop {
                let Some(job) = self.shared.q.lock().expect("lod lock").jobs.pop_front() else {
                    break;
                };
                let done = run(&self.generator, &job);
                self.shared.q.lock().expect("lod lock").done.push(done);
                if deadline.passed() {
                    break;
                }
            }
            self.collect();
        }
    }

    /// Blocks until nothing is dirty or in flight (tests, tools, shutdown saves).
    pub fn drain(
        &mut self,
        centers: &[[f64; 3]],
        modified: &mut dyn FnMut(ChunkCoord) -> Option<Vec<u16>>,
    ) {
        while !self.dirty.is_empty() || !self.in_flight.is_empty() || !self.generating.is_empty() {
            self.step(centers, modified, usize::MAX, Duration::from_secs(3600));
            if !self.workers.is_empty() {
                let q = self.shared.q.lock().expect("lod lock");
                let _q = self
                    .shared
                    .idle
                    .wait_while(q, |q| {
                        q.done.is_empty() && !(q.jobs.is_empty() && q.running == 0)
                    })
                    .expect("lod lock");
            }
        }
    }

    /// Sections written since the last call, in order.
    pub fn take_written(&mut self) -> Vec<LodCoord> {
        std::mem::take(&mut self.written)
    }

    /// Generated content of an unmodified section (full-chunk-mode clients): encoded cells once
    /// ready, else `None` (and queued). Kept in a small cache.
    pub fn generated(&mut self, c: LodCoord) -> Option<&Vec<u8>> {
        if self.generated.contains_key(&c) {
            return self.generated.get(&c);
        }
        if self.generating.insert(c) {
            self.shared.q.lock().expect("lod lock").jobs.push_back(Job {
                coord: c,
                generate_only: true,
                children: Default::default(),
            });
            self.shared.work.notify_all();
        }
        None
    }

    /// Modified: written, or dirty / being computed.
    pub fn modified(&self, c: LodCoord) -> bool {
        self.sections.contains_key(&c) || self.dirty.contains(&c) || self.in_flight.contains(&c)
    }
    pub fn find(&self, c: LodCoord) -> Option<&LodSection> {
        self.sections.get(&c)
    }
    pub fn sections(&self) -> &HashMap<LodCoord, LodSection> {
        &self.sections
    }
    pub fn dirty(&self) -> &HashSet<LodCoord> {
        &self.dirty
    }
    pub fn in_flight(&self) -> &HashSet<LodCoord> {
        &self.in_flight
    }
    pub fn pending(&self) -> usize {
        self.dirty.len() + self.in_flight.len()
    }
    pub fn last_revision(&self) -> u32 {
        self.next_revision - 1
    }

    /// A section's 34³ cells with its apron taken from the same-level neighbours that are modified
    /// (the stored content's apron is generated): what LodData Explicit carries.
    pub fn cells_for_client(&self, c: LodCoord) -> Vec<u16> {
        let Some(s) = self.find(c) else {
            return Vec::new();
        };
        let mut cells = decode(&s.encoded);
        let n = SECTION_CELLS;
        for axis in 0..3 {
            for dir in [-1, 1] {
                let mut nb = c;
                match axis {
                    0 => nb.i += dir,
                    1 => nb.j += dir,
                    _ => nb.k += dir,
                }
                let Some(ns) = self.find(nb) else { continue };
                let other = decode(&ns.encoded);
                let (ours, theirs) = if dir > 0 { (n, 0) } else { (-1, n - 1) };
                let at = |layer: i32, a: i32, b: i32| match axis {
                    0 => cell_index(layer, a, b),
                    1 => cell_index(a, layer, b),
                    _ => cell_index(a, b, layer),
                };
                for a in 0..n {
                    for b in 0..n {
                        cells[at(ours, a, b)] = other[at(theirs, a, b)];
                    }
                }
            }
        }
        debug_assert_eq!(cells.len(), (PAD * PAD * PAD) as usize);
        cells
    }
}

impl Drop for LodPropagation {
    fn drop(&mut self) {
        self.shared.q.lock().expect("lod lock").stop = true;
        self.shared.work.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

/// A modified chunk's voxels for `step`: the content of a chunk with revision > 0.
pub fn modified_voxels(chunk: Option<&Chunk>) -> Option<Vec<u16>> {
    chunk
        .filter(|c| c.revision() > 0)
        .map(|c| c.voxels().to_vec())
}
