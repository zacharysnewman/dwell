//! Background jobs in Web Workers (ARCHITECTURE.md §5.1, ADR 0007): each worker loads this same
//! WASM module and runs a [`JobWorker`]; the main thread's [`WebJobs`] posts encoded jobs to the
//! least busy worker of their kind and collects the encoded results. No `SharedArrayBuffer`:
//! every job and result is one transferred buffer.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, Worker};

use crate::jobs::codec::{decode_job, decode_output, encode_job, encode_output};
use crate::jobs::{Job, Output, Runner};
use crate::pool::Jobs;

/// A worker's job runner (called by the worker script).
#[wasm_bindgen]
pub struct JobWorker {
    runner: Runner,
}

#[wasm_bindgen]
impl JobWorker {
    /// A worker for a world (generation and meshing).
    pub fn world(generator_version: u32, world_seed: u64) -> JobWorker {
        Self {
            runner: Runner::world(generator_version, world_seed),
        }
    }
    /// A worker that only meshes.
    pub fn mesher() -> JobWorker {
        Self {
            runner: Runner::mesher(),
        }
    }
    /// Runs one encoded job; returns the encoded result.
    pub fn run(&self, bytes: &[u8]) -> Vec<u8> {
        match decode_job(bytes) {
            Ok((id, job)) => encode_output(id, &self.runner.run(job)),
            Err(_) => encode_output(0, &Output::Failed("malformed job".into())),
        }
    }
}

struct WorkerSlot {
    worker: Worker,
    busy: usize,
    /// The worker's WebAssembly memory (bytes), as it last reported it.
    memory: Rc<std::cell::Cell<f64>>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
}

/// Jobs run by two sets of workers: generation (with the world) and meshing.
pub struct WebJobs {
    slots: [Vec<WorkerSlot>; 2],
    /// Which slot runs each job: (id, kind, slot).
    running: Vec<(u32, usize, usize)>,
    done: Rc<RefCell<VecDeque<Vec<u8>>>>,
}

impl WebJobs {
    /// `make_worker` creates a worker running the job worker script; `world` is the generator
    /// version and seed the generation workers load.
    pub fn new(
        make_worker: &Function,
        generators: usize,
        meshers: usize,
        world: (u32, u64),
    ) -> Result<Self, JsValue> {
        let done: Rc<RefCell<VecDeque<Vec<u8>>>> = Rc::default();
        let mut slots: [Vec<WorkerSlot>; 2] = Default::default();
        for (kind, count) in [(0usize, generators.max(1)), (1, meshers.max(1))] {
            for _ in 0..count {
                let worker: Worker = make_worker.call0(&JsValue::NULL)?.dyn_into()?;
                let sink = done.clone();
                let memory: Rc<std::cell::Cell<f64>> = Rc::default();
                let reported = memory.clone();
                let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                    let data = e.data();
                    if data.is_instance_of::<js_sys::ArrayBuffer>() {
                        sink.borrow_mut().push_back(Uint8Array::new(&data).to_vec());
                    } else if let Some(bytes) = Reflect::get(&data, &"memory".into())
                        .ok()
                        .and_then(|m| m.as_f64())
                    {
                        reported.set(bytes);
                    }
                });
                worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
                let init = Object::new();
                Reflect::set(&init, &"t".into(), &"init".into())?;
                if kind == 0 {
                    Reflect::set(&init, &"generatorVersion".into(), &world.0.into())?;
                    Reflect::set(
                        &init,
                        &"worldSeed".into(),
                        &js_sys::BigInt::from(world.1).into(),
                    )?;
                }
                worker.post_message(&init)?;
                slots[kind].push(WorkerSlot {
                    worker,
                    busy: 0,
                    memory,
                    _onmessage: onmessage,
                });
            }
        }
        Ok(Self {
            slots,
            running: Vec::new(),
            done,
        })
    }
}

impl Jobs for WebJobs {
    /// Each worker's WebAssembly memory (bytes): generation workers, then meshing ones.
    fn memory(&self) -> Vec<f64> {
        self.slots
            .iter()
            .flatten()
            .map(|s| s.memory.get())
            .collect()
    }
    fn capacity(&self, mesh: bool) -> usize {
        // Two per worker: one running, one already there when it finishes.
        2 * self.slots[usize::from(mesh)].len()
    }
    fn submit(&mut self, id: u32, job: Job) {
        let kind = usize::from(job.is_mesh());
        let Some((at, slot)) = self.slots[kind]
            .iter_mut()
            .enumerate()
            .min_by_key(|(_, s)| s.busy)
        else {
            return;
        };
        slot.busy += 1;
        self.running.push((id, kind, at));
        let bytes = Uint8Array::from(&encode_job(id, &job)[..]);
        let buffer = bytes.buffer();
        let _ = slot
            .worker
            .post_message_with_transfer(&buffer, &Array::of1(&buffer));
    }
    fn poll(&mut self) -> Vec<(u32, Output)> {
        let mut out = Vec::new();
        while let Some(bytes) = self.done.borrow_mut().pop_front() {
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

impl Drop for WebJobs {
    fn drop(&mut self) {
        for s in self.slots.iter().flatten() {
            s.worker.terminate();
        }
    }
}
