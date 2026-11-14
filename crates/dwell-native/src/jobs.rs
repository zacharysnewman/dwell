//! The client's background jobs on native threads: the browser's Web Workers (`web/jobs.rs`
//! `WebJobs`) one for one. Two sets of threads, generation (each with the world's generator) and
//! meshing; a column's generation jobs go to one thread (`dwell_world::affinity`), as there.
//! Jobs and results cross as the same encoded bytes the workers' messages carry, so the
//! comparison with the browser keeps that cost.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use dwell_client::jobs::codec::{decode_job, decode_output, encode_job, encode_output};
use dwell_client::jobs::{Job, Output, Runner};
use dwell_client::pool::Jobs;
use dwell_world::affinity;

struct Slot {
    tx: Option<Sender<Vec<u8>>>,
    busy: usize,
    thread: Option<JoinHandle<()>>,
}

pub struct ThreadJobs {
    slots: [Vec<Slot>; 2],
    running: Vec<(u32, usize, usize)>,
    done: Receiver<Vec<u8>>,
}

impl ThreadJobs {
    pub fn new(
        generators: usize,
        meshers: usize,
        world: (u32, u64, dwell_protocol::world_size::WorldSize),
    ) -> Self {
        let (done_tx, done) = channel::<Vec<u8>>();
        let mut slots: [Vec<Slot>; 2] = Default::default();
        for (kind, count) in [(0usize, generators.max(1)), (1, meshers.max(1))] {
            for i in 0..count {
                let (tx, rx) = channel::<Vec<u8>>();
                let out = done_tx.clone();
                let thread = std::thread::Builder::new()
                    .name(format!("{}-{i}", if kind == 0 { "gen" } else { "mesh" }))
                    .spawn(move || {
                        let runner = if kind == 0 {
                            Runner::world_sized(world.0, world.1, world.2, None)
                        } else {
                            Runner::mesher()
                        };
                        while let Ok(bytes) = rx.recv() {
                            let result = match decode_job(&bytes) {
                                Ok((id, job)) => encode_output(id, &runner.run(job)),
                                Err(_) => encode_output(0, &Output::Failed("malformed job".into())),
                            };
                            if out.send(result).is_err() {
                                return;
                            }
                        }
                    })
                    .expect("a job thread");
                slots[kind].push(Slot {
                    tx: Some(tx),
                    busy: 0,
                    thread: Some(thread),
                });
            }
        }
        Self {
            slots,
            running: Vec::new(),
            done,
        }
    }
}

impl Jobs for ThreadJobs {
    fn capacity(&self, mesh: bool) -> usize {
        // Two per thread, as per worker: one running, one waiting.
        2 * self.slots[usize::from(mesh)].len()
    }
    fn submit(&mut self, id: u32, job: Job) {
        let kind = usize::from(job.is_mesh());
        let slots = &mut self.slots[kind];
        let busy: Vec<usize> = slots.iter().map(|s| s.busy).collect();
        let own = job
            .column()
            .map(|(level, x, z)| affinity::worker_for(level, x, z, slots.len()));
        let at = affinity::pick_worker(&busy, own, 2);
        let slot = &mut slots[at];
        slot.busy += 1;
        self.running.push((id, kind, at));
        if let Some(tx) = &slot.tx {
            let _ = tx.send(encode_job(id, &job));
        }
    }
    fn broadcast(&mut self, job: Job) {
        let bytes = encode_job(0, &job);
        for slot in self.slots.iter().flatten() {
            if let Some(tx) = &slot.tx {
                let _ = tx.send(bytes.clone());
            }
        }
    }
    fn poll(&mut self) -> Vec<(u32, Output)> {
        let mut out = Vec::new();
        while let Ok(bytes) = self.done.try_recv() {
            let Ok((id, output)) = decode_output(&bytes) else {
                continue;
            };
            if let Some(at) = self.running.iter().position(|r| r.0 == id) {
                let (_, kind, slot) = self.running.swap_remove(at);
                self.slots[kind][slot].busy -= 1;
            }
            out.push((id, output));
        }
        out
    }
}

impl Drop for ThreadJobs {
    fn drop(&mut self) {
        for s in self.slots.iter_mut().flatten() {
            s.tx = None;
        }
        for s in self.slots.iter_mut().flatten() {
            if let Some(t) = s.thread.take() {
                let _ = t.join();
            }
        }
    }
}
