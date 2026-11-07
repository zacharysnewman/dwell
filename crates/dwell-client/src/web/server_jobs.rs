//! The browser's server-jobs worker (docs/WATER_SURFACE_GRID.md §2d, the follow-up; ARCHITECTURE.md
//! §5.1): a worker the local-mode worker starts, its own instance of this module, running the local
//! server's jobs off its tick — the chunks it wants generated and its LOD sections to compute
//! (`dwell_sim::jobs::JobRunner`). Jobs come from `LocalWorld::takeJob`, answers go back by
//! `LocalWorld::jobIn`, transferred buffers each way.

use dwell_sim::jobs::JobRunner;
use wasm_bindgen::prelude::*;

/// The local server's jobs, run in their own worker.
#[wasm_bindgen]
pub struct ServerJobs {
    runner: JobRunner,
}

#[wasm_bindgen]
impl ServerJobs {
    /// For the world of generator `generator_version` and `world_seed` (its maps installed first).
    #[wasm_bindgen(constructor)]
    pub fn new(generator_version: u32, world_seed: u64) -> ServerJobs {
        ServerJobs {
            runner: JobRunner::for_world(generator_version, world_seed),
        }
    }
    /// A job's bytes run; its answer's (empty if the job can't be read).
    pub fn run(&mut self, job: &[u8]) -> Vec<u8> {
        self.runner.run(job).unwrap_or_default()
    }
}
