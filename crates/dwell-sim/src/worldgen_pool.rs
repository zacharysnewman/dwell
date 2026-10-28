//! Off-tick chunk generation (ARCHITECTURE.md §6.3, ADR 0007). The server says which chunks it
//! wants next (nearest first); worker threads generate them and the tick collects the results, each
//! taking the chunks of its own blocks of columns first (`dwell_world::affinity`).
//! With zero threads (the browser's single-threaded local mode) `collect` generates on the calling
//! thread instead, within a time budget, so a tick never stalls on a burst of new terrain.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use dwell_world::{Chunk, ChunkCoord, ChunkGenerator, affinity};

use crate::clock::Deadline;

pub type Generated = (ChunkCoord, Box<Chunk>);

#[derive(Default)]
struct State {
    queue: VecDeque<ChunkCoord>,
    in_flight: HashSet<ChunkCoord>,
    done: Vec<Generated>,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar, // queue non-empty or stopping
    idle: Condvar, // a chunk finished
}

pub struct WorldgenPool {
    generator: ChunkGenerator,
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

fn generate(generator: &ChunkGenerator, c: ChunkCoord) -> Box<Chunk> {
    let mut chunk = Box::<Chunk>::default();
    generator(c, &mut chunk);
    chunk.reset_revision();
    chunk
}

impl WorldgenPool {
    pub fn new(generator: ChunkGenerator, threads: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            work: Condvar::new(),
            idle: Condvar::new(),
        });
        let workers = (0..threads)
            .map(|k| {
                let (shared, generator) = (shared.clone(), generator.clone());
                std::thread::spawn(move || Self::work(&shared, &generator, k, threads))
            })
            .collect();
        Self {
            generator,
            shared,
            workers,
        }
    }

    pub fn threads(&self) -> usize {
        self.workers.len()
    }

    fn pending(s: &State, c: ChunkCoord) -> bool {
        s.in_flight.contains(&c) || s.done.iter().any(|(d, _)| *d == c)
    }

    /// Replaces the queue of chunks to generate, in priority order. Chunks already being
    /// generated or finished (not yet collected) are skipped.
    pub fn set_wanted(&self, wanted: &[ChunkCoord]) {
        let mut s = self.shared.state.lock().expect("worldgen lock");
        s.queue.clear();
        let mut seen = HashSet::new();
        for &c in wanted {
            if seen.insert(c) && !Self::pending(&s, c) {
                s.queue.push_back(c);
            }
        }
        drop(s);
        self.shared.work.notify_all();
    }

    /// Moves finished chunks to `out`. Without threads, first generates queued chunks here until
    /// `budget` has elapsed (at least one when any is queued).
    pub fn collect(&self, out: &mut Vec<Generated>, budget: Duration) {
        let mut s = self.shared.state.lock().expect("worldgen lock");
        if self.workers.is_empty() {
            let deadline = Deadline::after(budget);
            while let Some(c) = s.queue.pop_front() {
                out.push((c, generate(&self.generator, c)));
                if deadline.passed() {
                    break;
                }
            }
            return;
        }
        out.append(&mut s.done);
    }

    /// Blocks until every queued chunk is generated, then collects them (startup pre-generation).
    pub fn drain(&self, out: &mut Vec<Generated>) {
        if self.workers.is_empty() {
            loop {
                let empty = self
                    .shared
                    .state
                    .lock()
                    .expect("worldgen lock")
                    .queue
                    .is_empty();
                if empty {
                    return;
                }
                self.collect(out, Duration::from_secs(3600));
            }
        }
        let s = self.shared.state.lock().expect("worldgen lock");
        let mut s = self
            .shared
            .idle
            .wait_while(s, |s| !s.queue.is_empty() || !s.in_flight.is_empty())
            .expect("worldgen lock");
        out.append(&mut s.done);
    }

    /// Thread `worker` of `workers`: the queue's first chunk of its own blocks of columns, else
    /// its first (`dwell_world::affinity`: a column's chunks on one thread share its cache).
    fn work(shared: &Shared, generator: &ChunkGenerator, worker: usize, workers: usize) {
        let mut s = shared.state.lock().expect("worldgen lock");
        loop {
            s = shared
                .work
                .wait_while(s, |s| !s.stop && s.queue.is_empty())
                .expect("worldgen lock");
            if s.stop {
                return;
            }
            let at =
                affinity::next_for(s.queue.make_contiguous(), worker, workers).expect("queued");
            let c = s.queue.remove(at).expect("queued");
            s.in_flight.insert(c);
            drop(s);
            let chunk = generate(generator, c);
            s = shared.state.lock().expect("worldgen lock");
            s.in_flight.remove(&c);
            s.done.push((c, chunk));
            shared.idle.notify_all();
        }
    }
}

impl Drop for WorldgenPool {
    fn drop(&mut self) {
        self.shared.state.lock().expect("worldgen lock").stop = true;
        self.shared.work.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}
