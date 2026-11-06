//! The browser's water worker (ADR 0062, docs/WATER_SURFACE_GRID.md §2d): a second Web Worker the
//! local-mode worker starts, its own instance of this module, running the water grid
//! (`dwell_sim::water::host::WaterCore`) on the batches the local server's remote water host sends
//! (`LocalWorld::takeWaterOut`) and answering with its own (`LocalWorld::waterIn`), transferred
//! buffers each way. Its first batch is `In::Init`, the world it makes its water for.

use dwell_sim::water::Water;
use dwell_sim::water::host::{CoreConfig, WaterCore, encode_out};
use wasm_bindgen::prelude::*;

/// `performance.now()` in this worker.
fn now() -> f64 {
    js_sys::Reflect::get(&js_sys::global(), &"performance".into())
        .ok()
        .and_then(|p| p.dyn_into::<web_sys::Performance>().ok())
        .map_or(0.0, |p| p.now())
}

/// The water grid in its worker.
#[wasm_bindgen]
pub struct WaterWorker {
    core: WaterCore,
}

#[wasm_bindgen]
impl WaterWorker {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> WaterWorker {
        dwell_sim::water::host::set_clock(now);
        WaterWorker {
            core: WaterCore::new(Water::new(), CoreConfig::thread()),
        }
    }
    /// A batch from the server's host; false if it can't be read.
    pub fn receive(&mut self, bytes: &[u8]) -> bool {
        self.core.handle(bytes).is_some()
    }
    /// Runs what is owed (within its budget) and answers: the batch to send back.
    pub fn work(&mut self) -> Vec<u8> {
        let outs = self.core.work();
        encode_out(self.core.seq(), &outs)
    }
    /// Water time still owed: work again soon.
    pub fn busy(&self) -> bool {
        self.core.busy()
    }
}
