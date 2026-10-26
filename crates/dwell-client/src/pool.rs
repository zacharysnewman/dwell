//! The main thread's side of the background jobs: a queue per kind (generation, meshing) that keeps
//! at most the workers' capacity in flight, remembers what each job is for, and hands the
//! streamer, the LOD system and the debug tools their collaborator traits.

use dwell_world::hash::HashMap;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_game::chunk_stream::{ChunkSource, Coord, MeshJob, Mesher};
use dwell_world::lod::LodCoord;

use crate::jobs::{Job, Output, Runner};

/// Where jobs run: Web Workers in the browser, inline natively.
pub trait Jobs {
    /// Jobs of this kind (meshing or generation) that may be in flight at once.
    fn capacity(&self, mesh: bool) -> usize;
    fn submit(&mut self, id: u32, job: Job);
    /// Runs `job` on every worker, before anything submitted after it (`Job::Registry`); its
    /// results are dropped.
    fn broadcast(&mut self, job: Job);
    /// Finished jobs.
    fn poll(&mut self) -> Vec<(u32, Output)>;
    /// Each worker's memory (bytes), where the jobs run in workers.
    fn memory(&self) -> Vec<f64> {
        Vec::new()
    }
    /// Sets what to call when a job finishes between frames (Web Workers answer whenever they
    /// are done): the pool collects the result and starts the next queued job then, so a slow
    /// page's workers don't sit idle until its next frame.
    fn on_done(&mut self, _wake: Rc<dyn Fn()>) {}
}

/// Runs jobs on the calling thread when polled (tests, and a fallback without workers).
pub struct InlineJobs {
    runner: Runner,
    queue: VecDeque<(u32, Job)>,
    /// Jobs run per poll.
    pub per_poll: usize,
}

impl InlineJobs {
    pub fn new(runner: Runner) -> Self {
        Self {
            runner,
            queue: VecDeque::new(),
            per_poll: usize::MAX,
        }
    }
}

impl Jobs for InlineJobs {
    fn capacity(&self, _mesh: bool) -> usize {
        4
    }
    fn submit(&mut self, id: u32, job: Job) {
        self.queue.push_back((id, job));
    }
    fn broadcast(&mut self, job: Job) {
        self.runner.run(job);
    }
    fn poll(&mut self) -> Vec<(u32, Output)> {
        let n = self.per_poll.min(self.queue.len());
        self.queue
            .drain(..n)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(id, job)| (id, self.runner.run(job)))
            .collect()
    }
}

/// What a job is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Purpose {
    Chunk(Coord, u64),
    ChunkMesh(Coord, u64),
    Bounds(i32, i32, i32),
    Section(u64),
    SectionMesh(u64),
    /// The verification chunk's hash (§6.3).
    Verify,
    /// The F4 map.
    Map,
    /// The F3 overlay's regenerate-and-diff of a chunk.
    Diff(Coord),
    /// A strip of the LOD colour map (`crate::lod_map`).
    LodMap(crate::lod_map::TileJob),
}

impl Purpose {
    /// The telemetry names of this kind of job's wait (queued in the pool) and run (from
    /// submitting to a worker until its result is polled, the trip included).
    fn telemetry_names(&self) -> (&'static str, &'static str) {
        match self {
            Purpose::Chunk(..) => ("job.chunk.wait", "job.chunk.run"),
            Purpose::ChunkMesh(..) => ("job.chunk_mesh.wait", "job.chunk_mesh.run"),
            Purpose::Bounds(..) => ("job.bounds.wait", "job.bounds.run"),
            Purpose::Section(..) => ("job.section.wait", "job.section.run"),
            Purpose::SectionMesh(..) => ("job.section_mesh.wait", "job.section_mesh.run"),
            Purpose::Verify => ("job.verify.wait", "job.verify.run"),
            Purpose::Map => ("job.map.wait", "job.map.run"),
            Purpose::Diff(..) => ("job.diff.wait", "job.diff.run"),
            Purpose::LodMap(..) => ("job.lod_map.wait", "job.lod_map.run"),
        }
    }
}

/// A finished job with its purpose.
pub type Done = (Purpose, Output);

pub struct Pool {
    jobs: Box<dyn Jobs>,
    next: u32,
    purposes: HashMap<u32, (Purpose, bool)>,
    /// When each job was queued and started (telemetry).
    times: HashMap<u32, (f64, f64)>,
    /// Waiting for room: generation, meshing.
    queued: [VecDeque<(u32, Job)>; 2],
    in_flight: [usize; 2],
    /// Results collected between frames ([`Jobs::on_done`]), for the next [`Self::poll`].
    ready: Vec<Done>,
}

pub type PoolRef = Rc<RefCell<Pool>>;

impl Pool {
    pub fn new(jobs: Box<dyn Jobs>) -> PoolRef {
        let pool = Rc::new(RefCell::new(Self {
            jobs,
            next: 1,
            purposes: HashMap::default(),
            times: HashMap::default(),
            queued: Default::default(),
            in_flight: [0; 2],
            ready: Vec::new(),
        }));
        let weak = Rc::downgrade(&pool);
        // A result between frames: collected, and the worker given the next queued job. Never
        // while the pool is in use (a wake from inside it waits for the next poll).
        let wake: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(pool) = weak.upgrade()
                && let Ok(mut pool) = pool.try_borrow_mut()
            {
                pool.collect();
            }
        });
        pool.borrow_mut().jobs.on_done(wake);
        pool
    }

    pub fn capacity(&self, mesh: bool) -> usize {
        self.jobs.capacity(mesh)
    }

    /// Queues a job; it starts when its kind has room.
    pub fn submit(&mut self, purpose: Purpose, job: Job) {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        let mesh = job.is_mesh();
        self.purposes.insert(id, (purpose, mesh));
        let now = crate::telemetry::now_ms();
        self.times.insert(id, (now, now));
        self.queued[usize::from(mesh)].push_back((id, job));
        self.pump();
    }

    /// Runs a job on every worker ahead of every job not yet started (`Job::Registry`).
    pub fn broadcast(&mut self, job: Job) {
        self.jobs.broadcast(job);
    }

    /// Drops a queued (not yet started) chunk generation; true if there was one.
    pub fn cancel_chunk(&mut self, coord: Coord) -> bool {
        let q = &mut self.queued[0];
        let Some(at) = q.iter().position(|(id, _)| matches!(self.purposes.get(id), Some((Purpose::Chunk(c, _), _)) if *c == coord)) else {
            return false;
        };
        if let Some((id, _)) = q.remove(at) {
            self.purposes.remove(&id);
            self.times.remove(&id);
        }
        true
    }

    fn pump(&mut self) {
        for kind in 0..2 {
            while self.in_flight[kind] < self.jobs.capacity(kind == 1) {
                let Some((id, job)) = self.queued[kind].pop_front() else {
                    break;
                };
                self.in_flight[kind] += 1;
                if let Some(t) = self.times.get_mut(&id) {
                    t.1 = crate::telemetry::now_ms();
                }
                self.jobs.submit(id, job);
            }
        }
    }

    /// Finished jobs, oldest first (those collected between frames too); starts queued ones in
    /// their place.
    pub fn poll(&mut self) -> Vec<Done> {
        self.collect();
        std::mem::take(&mut self.ready)
    }

    /// Takes the finished jobs' results and starts queued jobs in their place.
    fn collect(&mut self) {
        for (id, output) in self.jobs.poll() {
            let Some((purpose, mesh)) = self.purposes.remove(&id) else {
                continue;
            };
            self.in_flight[usize::from(mesh)] -= 1;
            if let Some((queued, started)) = self.times.remove(&id) {
                let (wait, run) = purpose.telemetry_names();
                crate::telemetry::sample(wait, started - queued);
                crate::telemetry::sample(run, crate::telemetry::now_ms() - started);
            }
            self.ready.push((purpose, output));
        }
        self.pump();
    }

    /// The workers' memory (bytes).
    pub fn memory(&self) -> Vec<f64> {
        self.jobs.memory()
    }

    /// Jobs queued or running: generation, meshing.
    pub fn pending(&self) -> [usize; 2] {
        [
            self.queued[0].len() + self.in_flight[0],
            self.queued[1].len() + self.in_flight[1],
        ]
    }
}

/// The streamer's worldgen.
#[derive(Clone)]
pub struct ChunkJobs(pub PoolRef);

impl ChunkSource for ChunkJobs {
    fn generate(&mut self, coord: Coord, token: u64) {
        self.0.borrow_mut().submit(
            Purpose::Chunk(coord, token),
            Job::Generate {
                coord: dwell_world::ChunkCoord::from_array(coord),
            },
        );
    }
    fn cancel(&mut self, coord: Coord) -> bool {
        self.0.borrow_mut().cancel_chunk(coord)
    }
}

/// The streamer's mesher.
#[derive(Clone)]
pub struct MeshJobs(pub PoolRef);

impl Mesher for MeshJobs {
    type Meshes = dwell_mesh::ChunkMeshes;
    fn capacity(&self) -> usize {
        // Enough in flight to keep every worker busy while results travel back.
        2 * self.0.borrow().capacity(true)
    }
    fn mesh(&mut self, job: MeshJob) {
        self.0.borrow_mut().submit(
            Purpose::ChunkMesh(job.coord, job.token),
            Job::MeshChunk {
                padded: job.padded,
                light: job.light,
                mirror: job.mirror,
                origin: job.coord.map(|c| c * dwell_world::CHUNK_SIZE),
            },
        );
    }
}

impl dwell_lod::SectionSource for ChunkJobs {
    fn lod_bounds(&mut self, level: i32, i: i32, k: i32) {
        self.0
            .borrow_mut()
            .submit(Purpose::Bounds(level, i, k), Job::LodBounds { level, i, k });
    }
    fn lod(&mut self, c: LodCoord, token: u64) {
        self.0
            .borrow_mut()
            .submit(Purpose::Section(token), Job::Lod { coord: c });
    }
}

impl dwell_lod::SectionMesher for MeshJobs {
    fn mesh_section(&mut self, job: dwell_lod::LodMeshJob, token: u64) {
        self.0.borrow_mut().submit(
            Purpose::SectionMesh(token),
            Job::MeshSection {
                cells: job.cells,
                surface: job.surface,
                water_drop: job.water_drop,
                mirror: job.mirror,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Held {
        cap: usize,
        started: Vec<u32>,
        done: Vec<(u32, Output)>,
        wake: Option<Rc<dyn Fn()>>,
    }
    impl Jobs for Rc<RefCell<Held>> {
        fn capacity(&self, _mesh: bool) -> usize {
            self.borrow().cap
        }
        fn submit(&mut self, id: u32, _job: Job) {
            self.borrow_mut().started.push(id);
        }
        fn broadcast(&mut self, _job: Job) {}
        fn poll(&mut self) -> Vec<(u32, Output)> {
            std::mem::take(&mut self.borrow_mut().done)
        }
        fn on_done(&mut self, wake: Rc<dyn Fn()>) {
            self.borrow_mut().wake = Some(wake);
        }
    }

    /// A worker finishing: its result arrives, and the pool is woken as a Web Worker's message
    /// would wake it.
    fn finish(held: &Rc<RefCell<Held>>, id: u32) {
        held.borrow_mut()
            .done
            .push((id, Output::Failed("x".into())));
        let wake = held.borrow().wake.clone();
        if let Some(w) = wake {
            w();
        }
    }

    #[test]
    fn a_job_finishing_between_frames_starts_the_next_one_at_once() {
        // On CI's software renderer frames took 2–15 s, and the workers were refilled only at a
        // frame's poll: two generation jobs per worker a frame, chunks waited 5–17 s for one
        // (job.chunk.wait) though each ran in ~0.1 s, and the native-server e2e tests' terrain
        // never finished generating (play.spec:129, edit.spec:204).
        let held = Rc::new(RefCell::new(Held {
            cap: 1,
            ..Default::default()
        }));
        let pool = Pool::new(Box::new(held.clone()));
        let mut source = ChunkJobs(pool.clone());
        for (x, token) in [(0, 1), (1, 2), (2, 3)] {
            source.generate([x, 0, 0], token);
        }
        assert_eq!(held.borrow().started, vec![1]);
        finish(&held, 1);
        assert_eq!(held.borrow().started, vec![1, 2], "started between frames");
        finish(&held, 2);
        assert_eq!(held.borrow().started, vec![1, 2, 3]);
        // The frame's poll hands over both results, oldest first.
        let done: Vec<Purpose> = pool.borrow_mut().poll().into_iter().map(|d| d.0).collect();
        assert_eq!(
            done,
            vec![Purpose::Chunk([0, 0, 0], 1), Purpose::Chunk([1, 0, 0], 2)]
        );
        assert_eq!(pool.borrow().pending(), [1, 0]);
        assert!(pool.borrow_mut().poll().is_empty());
    }

    #[test]
    fn a_wake_while_the_pool_is_in_use_waits_for_the_next_poll() {
        let held = Rc::new(RefCell::new(Held {
            cap: 1,
            ..Default::default()
        }));
        let pool = Pool::new(Box::new(held.clone()));
        let mut source = ChunkJobs(pool.clone());
        source.generate([0, 0, 0], 1);
        source.generate([1, 0, 0], 2);
        {
            let _busy = pool.borrow_mut();
            finish(&held, 1); // no panic: the pool is borrowed
        }
        assert_eq!(held.borrow().started, vec![1]);
        assert_eq!(pool.borrow_mut().poll().len(), 1);
        assert_eq!(held.borrow().started, vec![1, 2]);
    }

    #[test]
    fn keeps_capacity_in_flight_and_cancels_only_queued_chunks() {
        let held = Rc::new(RefCell::new(Held {
            cap: 1,
            ..Default::default()
        }));
        let pool = Pool::new(Box::new(held.clone()));
        let mut source = ChunkJobs(pool.clone());
        source.generate([0, 0, 0], 1);
        source.generate([1, 0, 0], 2);
        source.generate([2, 0, 0], 3);
        assert_eq!(held.borrow().started, vec![1]);
        assert!(!source.cancel([0, 0, 0]), "a started job runs on");
        assert!(source.cancel([2, 0, 0]));
        assert_eq!(pool.borrow().pending(), [2, 0]);
        held.borrow_mut().done.push((1, Output::Failed("x".into())));
        let done = pool.borrow_mut().poll();
        assert_eq!(done[0].0, Purpose::Chunk([0, 0, 0], 1));
        assert_eq!(
            held.borrow().started,
            vec![1, 2],
            "the next queued job starts"
        );
    }
}
