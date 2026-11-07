//! Off-tick chunk generation (ARCHITECTURE.md §6.3, ADR 0007). The server says which chunks it
//! wants next (nearest first); worker threads generate them and the tick collects the results, each
//! taking the chunks of its own blocks of columns first (`dwell_world::affinity`).
//! With zero threads (the browser's single-threaded local mode) `collect` generates on the calling
//! thread instead, within a time budget, so a tick never stalls on a burst of new terrain.
//! **Remote** (`new_remote`: the browser's local mode with its server-jobs worker): the chunks are
//! generated elsewhere; the embedder takes each one to generate (`take_job`, a few at a time so the
//! queue's order stays current) and hands it back (`deliver`). Startup's `drain` still generates
//! here.

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
    remote: bool,
}

/// The most chunks a remote pool has out at its worker at once.
pub const REMOTE_IN_FLIGHT: usize = 4;

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
            remote: false,
        }
    }

    /// A pool whose chunks are generated elsewhere (`take_job`, `deliver`).
    pub fn new_remote(generator: ChunkGenerator) -> Self {
        let mut p = Self::new(generator, 0);
        p.remote = true;
        p
    }

    /// The next chunk to generate elsewhere (a remote pool's), at most `REMOTE_IN_FLIGHT` out.
    pub fn take_job(&self) -> Option<ChunkCoord> {
        if !self.remote {
            return None;
        }
        let mut s = self.shared.state.lock().expect("worldgen lock");
        if s.in_flight.len() >= REMOTE_IN_FLIGHT {
            return None;
        }
        let c = s.queue.pop_front()?;
        s.in_flight.insert(c);
        Some(c)
    }

    /// A chunk generated elsewhere, collected at the next `collect`.
    pub fn deliver(&self, c: ChunkCoord, mut chunk: Box<Chunk>) {
        chunk.reset_revision();
        let mut s = self.shared.state.lock().expect("worldgen lock");
        if s.in_flight.remove(&c) {
            s.done.push((c, chunk));
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
        if self.workers.is_empty() && !self.remote {
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
            // here, remote or not (startup: nothing is out at a worker yet)
            let mut s = self.shared.state.lock().expect("worldgen lock");
            out.append(&mut s.done);
            while let Some(c) = s.queue.pop_front() {
                out.push((c, generate(&self.generator, c)));
            }
            return;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_pool_hands_out_a_few_chunks_and_takes_them_back() {
        // face-a-only: chunks are world volumes; no up or down in a queue.
        let pool = WorldgenPool::new_remote(dwell_world::flat_generator());
        let wanted: Vec<ChunkCoord> = (0..6).map(|x| ChunkCoord::new(x, 1, 0)).collect();
        pool.set_wanted(&wanted);
        let mut out = Vec::new();
        pool.collect(&mut out, Duration::from_secs(1));
        assert!(out.is_empty(), "nothing generated on the calling thread");
        let jobs: Vec<ChunkCoord> = std::iter::from_fn(|| pool.take_job()).collect();
        assert_eq!(
            jobs,
            wanted[..REMOTE_IN_FLIGHT],
            "the queue's first, a few at a time"
        );
        for &c in &jobs {
            pool.deliver(c, generate(&dwell_world::flat_generator(), c));
        }
        pool.collect(&mut out, Duration::from_secs(1));
        assert_eq!(out.len(), REMOTE_IN_FLIGHT);
        pool.set_wanted(&wanted);
        assert_eq!(pool.take_job(), Some(wanted[0]));
        // startup's drain generates here what isn't out at the worker
        let mut rest = Vec::new();
        pool.drain(&mut rest);
        assert_eq!(rest.len(), wanted.len() - 1);
    }
}
