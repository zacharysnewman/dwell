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
//!
//! Two things keep a flood's LOD work small (the water rewrites its chunks every water tick,
//! docs/WATER_SURFACE_GRID.md §2d): a section's **generated base** (the generator's cells, a pure
//! function of its coordinate and most of a job's cost) is kept once computed (`MAX_BASES`), so
//! computing a section again is its downsample only; and a chunk changed by **settling** writes
//! (`mark_chunk_settling`: the water's) holds its section that many steps from its first such
//! change before it is computed, so a section the water keeps rewriting is computed at most once a
//! hold, not every water tick. An edit computes it at once.
//!
//! **Remote** (`new_remote`: the browser's local mode): the jobs run in a worker of their own, not
//! on the tick. The embedder takes each job's bytes (`take_job`), runs them there
//! ([`LodRunner`], which keeps its own generated bases) and hands the answer back (`deliver`).

use dwell_world::hash::{HashMap, HashSet};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use dwell_blocks::require;
use dwell_protocol::{decode_lod_cells, encode_lod_cells};
use dwell_world::lod::{
    LodCells, LodCoord, LodGenerator, MAX_LEVEL, PAD, SECTION_CELLS, VOLUME, cell_index, child,
    chunk_of_lod, downsample_octant, downsample_section_octant, in_world_of, lod_of_chunk, parent,
    section_origin, section_size,
};
use dwell_world::{Chunk, ChunkCoord, local_index};

use crate::clock::Deadline;

const MAX_GENERATED: usize = 256; // full-chunk-mode sections kept for answering
/// Modified sections' generated bases kept (34³ cells each, 78 KB): computing one again is its
/// downsample only.
pub const MAX_BASES: usize = 96;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LodSection {
    pub revision: u32,
    /// Palette + RLE of the 34³ cells (`encode_lod_cells`).
    pub encoded: Vec<u8>,
}

struct Job {
    coord: LodCoord,
    generate_only: bool,
    /// The section's generated cells, if kept (`LodPropagation::bases`).
    base: Option<Arc<LodCells>>,
    /// Modified children (octant order): chunk voxels (level 1) or section cells (level ≥ 2).
    children: [Vec<u16>; 8],
}

struct Done {
    coord: LodCoord,
    generate_only: bool,
    encoded: Vec<u8>,
    /// The generated cells it computed (none if it was given them), to keep.
    base: Option<Arc<LodCells>>,
    /// It ran the generator (diagnostics).
    generated: bool,
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
    /// Modified sections' generated cells, oldest first out (`MAX_BASES`).
    bases: HashMap<LodCoord, Arc<LodCells>>,
    bases_order: VecDeque<LodCoord>,
    /// Dirty sections held until this step (`mark_chunk_settling`).
    hold: HashMap<LodCoord, u64>,
    steps: u64,
    /// Generator runs and kept bases used (diagnostics, tests).
    pub generator_runs: u64,
    pub base_hits: u64,
    /// Jobs run elsewhere (`take_job`, `deliver`), never on the calling thread but to drain.
    remote: bool,
    /// The world's disc and volume: sections outside are never marked.
    world: dwell_world::WorldBounds,
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
    let (mut cells, base, generated) = match &job.base {
        Some(b) => ((**b).clone(), None, false),
        None => {
            let mut cells: LodCells = vec![require("dwell:air"); VOLUME];
            generator(job.coord, &mut cells);
            let base = (!job.generate_only).then(|| Arc::new(cells.clone()));
            (cells, base, true)
        }
    };
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
        base,
        generated,
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
            bases: HashMap::default(),
            bases_order: VecDeque::new(),
            hold: HashMap::default(),
            steps: 0,
            generator_runs: 0,
            base_hits: 0,
            remote: false,
            world: dwell_world::WorldBounds::DEFAULT,
        }
    }

    /// Replaces the generator of the sections' generated bases (a world retuned in the game, Phase
    /// 56 B7): the kept bases and generated sections dropped. Worker threads (a dedicated server's)
    /// keep the generator they were made with; local worlds run none.
    pub fn set_generator(&mut self, generator: LodGenerator) {
        self.generator = generator;
        self.bases.clear();
        self.bases_order.clear();
        self.generated.clear();
        self.generated_order.clear();
    }

    /// Propagation over a world of these bounds (its size's disc and volume, BIG_MAP.md M2).
    pub fn with_world(mut self, world: dwell_world::WorldBounds) -> Self {
        self.world = world;
        self
    }

    /// Propagation whose jobs run elsewhere (the browser's LOD worker): `take_job`, `deliver`.
    pub fn new_remote(generator: LodGenerator) -> Self {
        let mut p = Self::new(generator, 0);
        p.remote = true;
        p
    }

    pub fn is_remote(&self) -> bool {
        self.remote
    }

    /// The next job to run elsewhere (a remote propagation's), as bytes for [`LodRunner::run`].
    pub fn take_job(&mut self) -> Option<Vec<u8>> {
        if !self.remote {
            return None;
        }
        let job = self.shared.q.lock().expect("lod lock").jobs.pop_front()?;
        Some(encode_job(&job))
    }

    /// A job's answer from elsewhere ([`LodRunner::run`]'s bytes), applied at the next step. False
    /// if it can't be read.
    pub fn deliver(&mut self, bytes: &[u8]) -> bool {
        match decode_done(bytes) {
            Some(d) => {
                self.shared.q.lock().expect("lod lock").done.push(d);
                true
            }
            None => false,
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
        if in_world_of(c, &self.world) {
            self.dirty.insert(c);
            self.hold.remove(&c);
        }
    }
    /// A chunk's voxels changed by writes that go on changing it (the water's): its section is
    /// computed `hold` steps after the first such change (not again for each), unless something
    /// marks it dirty at once meanwhile.
    pub fn mark_chunk_settling(&mut self, c: ChunkCoord, hold: u64) {
        let s = parent(lod_of_chunk(c));
        if in_world_of(s, &self.world) && self.dirty.insert(s) {
            self.hold.insert(s, self.steps + hold);
        }
    }
    /// Dirty sections held (`mark_chunk_settling`) and not yet due.
    pub fn held(&self) -> usize {
        self.hold.values().filter(|&&d| d > self.steps).count()
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
            base: self.bases.get(&c).cloned(),
            children,
        }
    }

    fn keep_base(&mut self, c: LodCoord, base: Arc<LodCells>) {
        if self.bases.insert(c, base).is_none() {
            self.bases_order.push_back(c);
            if self.bases_order.len() > MAX_BASES
                && let Some(old) = self.bases_order.pop_front()
            {
                self.bases.remove(&old);
            }
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
        if d.generated {
            self.generator_runs += 1;
        } else {
            self.base_hits += 1;
        }
        if let Some(base) = d.base {
            self.keep_base(d.coord, base);
        }
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
        self.steps += 1;
        // Lowest level first (children before parents), then nearest to a player.
        if !self.dirty.is_empty() && budget > 0 {
            let steps = self.steps;
            let hold = &self.hold;
            let mut ready: Vec<((i32, f64), LodCoord)> = self
                .dirty
                .iter()
                .filter(|c| !self.in_flight.contains(c))
                .filter(|c| hold.get(c).is_none_or(|&d| d <= steps))
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
                self.hold.remove(&c);
                self.in_flight.insert(c);
                jobs.push(self.snapshot(c, modified));
            }
            self.shared.q.lock().expect("lod lock").jobs.extend(jobs);
            self.shared.work.notify_all();
        }
        self.run_inline(inline_budget);
    }

    fn run_inline(&mut self, inline_budget: Duration) {
        if self.workers.is_empty() && !self.remote {
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
        self.hold.clear();
        // A remote propagation's jobs, here.
        let remote = std::mem::replace(&mut self.remote, false);
        if remote {
            // jobs out at the worker are run again here (a late answer writes the same content)
            let out: Vec<LodCoord> = self.in_flight.drain().collect();
            self.dirty.extend(out);
            self.generating.clear();
            self.shared.q.lock().expect("lod lock").jobs.clear();
        }
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
        self.remote = remote;
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
                base: None,
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
    /// To be computed again: dirty (held ones too) or being computed (at a remote worker too).
    pub fn recomputing(&self, c: LodCoord) -> bool {
        self.dirty.contains(&c) || self.in_flight.contains(&c)
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

/// Runs a remote propagation's jobs (the browser's LOD worker): a job's bytes in, its answer's out,
/// keeping the generated bases of the sections it computed (`MAX_BASES`).
pub struct LodRunner {
    generator: LodGenerator,
    bases: HashMap<LodCoord, Arc<LodCells>>,
    order: VecDeque<LodCoord>,
}

impl LodRunner {
    pub fn new(generator: LodGenerator) -> Self {
        Self {
            generator,
            bases: HashMap::default(),
            order: VecDeque::new(),
        }
    }

    /// A job's bytes (`LodPropagation::take_job`) run; its answer for `deliver` (None if the job
    /// can't be read).
    pub fn run(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        let mut job = decode_job(bytes)?;
        if !job.generate_only {
            job.base = self.bases.get(&job.coord).cloned();
        }
        let mut done = run(&self.generator, &job);
        if let Some(base) = done.base.take()
            && self.bases.insert(job.coord, base).is_none()
        {
            self.order.push_back(job.coord);
            if self.order.len() > MAX_BASES
                && let Some(old) = self.order.pop_front()
            {
                self.bases.remove(&old);
            }
        }
        Some(encode_done(&done))
    }
}

// A job's bytes: "LJ1", the coordinate (level, i, j, k: i32), generate-only (u8), then each of the
// 8 children as a u32 count and its u16 cells, little-endian. An answer's: "LD1", the coordinate,
// generate-only and generated (u8 each), then the encoded cells (u32 length and bytes).

fn put_coord(out: &mut Vec<u8>, c: LodCoord) {
    for v in [c.level, c.i, c.j, c.k] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn take<'a>(b: &'a [u8], at: &mut usize, n: usize) -> Option<&'a [u8]> {
    let s = b.get(*at..at.checked_add(n)?)?;
    *at += n;
    Some(s)
}

fn take_u32(b: &[u8], at: &mut usize) -> Option<u32> {
    Some(u32::from_le_bytes(take(b, at, 4)?.try_into().ok()?))
}

fn take_coord(b: &[u8], at: &mut usize) -> Option<LodCoord> {
    let mut v = [0i32; 4];
    for x in &mut v {
        *x = take_u32(b, at)? as i32;
    }
    Some(LodCoord::new(v[0], v[1], v[2], v[3]))
}

fn encode_job(job: &Job) -> Vec<u8> {
    let mut out = b"LJ1".to_vec();
    put_coord(&mut out, job.coord);
    out.push(u8::from(job.generate_only));
    for kid in &job.children {
        out.extend_from_slice(&(kid.len() as u32).to_le_bytes());
        for v in kid {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn decode_job(b: &[u8]) -> Option<Job> {
    let mut at = 0;
    if take(b, &mut at, 3)? != b"LJ1" {
        return None;
    }
    let coord = take_coord(b, &mut at)?;
    let generate_only = take(b, &mut at, 1)?[0] != 0;
    let mut children: [Vec<u16>; 8] = Default::default();
    for kid in &mut children {
        let n = take_u32(b, &mut at)? as usize;
        let s = take(b, &mut at, n.checked_mul(2)?)?;
        *kid = s
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
    }
    (at == b.len()).then_some(Job {
        coord,
        generate_only,
        base: None,
        children,
    })
}

fn encode_done(d: &Done) -> Vec<u8> {
    let mut out = b"LD1".to_vec();
    put_coord(&mut out, d.coord);
    out.push(u8::from(d.generate_only));
    out.push(u8::from(d.generated));
    out.extend_from_slice(&(d.encoded.len() as u32).to_le_bytes());
    out.extend_from_slice(&d.encoded);
    out
}

fn decode_done(b: &[u8]) -> Option<Done> {
    let mut at = 0;
    if take(b, &mut at, 3)? != b"LD1" {
        return None;
    }
    let coord = take_coord(b, &mut at)?;
    let flags = take(b, &mut at, 2)?;
    let n = take_u32(b, &mut at)? as usize;
    let encoded = take(b, &mut at, n)?.to_vec();
    (at == b.len()).then_some(Done {
        coord,
        generate_only: flags[0] != 0,
        encoded,
        base: None,
        generated: flags[1] != 0,
    })
}

/// A modified chunk's voxels for `step`: the content of a chunk with revision > 0.
pub fn modified_voxels(chunk: Option<&Chunk>) -> Option<Vec<u16>> {
    chunk
        .filter(|c| c.revision() > 0)
        .map(|c| c.voxels().to_vec())
}
