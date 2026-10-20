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
}

pub type PoolRef = Rc<RefCell<Pool>>;

impl Pool {
    pub fn new(jobs: Box<dyn Jobs>) -> PoolRef {
        Rc::new(RefCell::new(Self {
            jobs,
            next: 1,
            purposes: HashMap::default(),
            times: HashMap::default(),
            queued: Default::default(),
            in_flight: [0; 2],
        }))
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

    /// Finished jobs, oldest first; starts queued ones in their place.
    pub fn poll(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
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
            out.push((purpose, output));
        }
        self.pump();
        out
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

    struct Held {
        cap: usize,
        started: Vec<u32>,
        done: Vec<(u32, Output)>,
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
    }

    #[test]
    fn keeps_capacity_in_flight_and_cancels_only_queued_chunks() {
        let held = Rc::new(RefCell::new(Held {
            cap: 1,
            started: vec![],
            done: vec![],
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
