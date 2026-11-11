//! Local mode in the browser (ARCHITECTURE.md §2.1, §6.4): [`LocalWorld`] is the integrated server
//! the local-mode worker script drives (the same messages as before: connect, reliable, datagram,
//! host, save…), and its world file is a redb database on an OPFS sync access handle.
//!
//! redb wants a `Send + Sync` backend, but a JS handle is neither (and `unsafe` is forbidden), so the
//! backend holds an id and the handle stays in this worker's thread-local table: a worker is one
//! thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;

use dwell_protocol::{Channel, HostState, TransportKind};
use dwell_sim::entropy::Entropy;
use dwell_sim::server::EditPolicy;
use dwell_storage::StorageBackend;
use wasm_bindgen::prelude::*;
use web_sys::{FileSystemReadWriteOptions, FileSystemSyncAccessHandle};

use crate::local::{LocalCore, LocalError, encode_outbox};

/// `performance.now()` in this worker.
fn worker_now() -> f64 {
    js_sys::Reflect::get(&js_sys::global(), &"performance".into())
        .ok()
        .and_then(|p| p.dyn_into::<web_sys::Performance>().ok())
        .map_or(0.0, |p| p.now())
}

thread_local! {
    static HANDLES: RefCell<HashMap<u32, FileSystemSyncAccessHandle>> = RefCell::new(HashMap::new());
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
}

fn js_io(e: JsValue) -> io::Error {
    io::Error::other(e.as_string().unwrap_or_else(|| "OPFS error".into()))
}

/// A world file on OPFS, through the handle registered under `id`.
#[derive(Debug)]
struct OpfsBackend {
    id: u32,
}

impl OpfsBackend {
    fn register(handle: FileSystemSyncAccessHandle) -> Self {
        let id = NEXT.with(|n| {
            let id = *n.borrow();
            *n.borrow_mut() += 1;
            id
        });
        HANDLES.with(|h| h.borrow_mut().insert(id, handle));
        Self { id }
    }
    fn with<T>(
        &self,
        f: impl FnOnce(&FileSystemSyncAccessHandle) -> Result<T, JsValue>,
    ) -> io::Result<T> {
        HANDLES.with(|h| match h.borrow().get(&self.id) {
            Some(handle) => f(handle).map_err(js_io),
            None => Err(io::Error::other("the world file was closed")),
        })
    }
}

fn at(offset: u64) -> FileSystemReadWriteOptions {
    let o = FileSystemReadWriteOptions::new();
    o.set_at(offset as f64);
    o
}

impl StorageBackend for OpfsBackend {
    fn len(&self) -> io::Result<u64> {
        self.with(|h| h.get_size()).map(|s| s as u64)
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        let n = self.with(|h| h.read_with_u8_array_and_options(out, &at(offset)))? as usize;
        if n == out.len() {
            Ok(())
        } else {
            Err(io::ErrorKind::UnexpectedEof.into())
        }
    }
    fn set_len(&self, len: u64) -> io::Result<()> {
        self.with(|h| h.truncate_with_f64(len as f64))
    }
    fn sync_data(&self) -> io::Result<()> {
        self.with(|h| h.flush())
    }
    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let n = self.with(|h| h.write_with_u8_array_and_options(data, &at(offset)))? as usize;
        if n == data.len() {
            Ok(())
        } else {
            Err(io::ErrorKind::WriteZero.into())
        }
    }
    fn close(&self) -> io::Result<()> {
        HANDLES.with(|h| {
            if let Some(handle) = h.borrow_mut().remove(&self.id) {
                handle.close();
            }
        });
        Ok(())
    }
}

/// `crypto.getRandomValues` (the worker's global).
struct WebEntropy;
impl Entropy for WebEntropy {
    fn fill(&mut self, out: &mut [u8]) {
        let crypto = js_sys::Reflect::get(&js_sys::global(), &"crypto".into()).expect("crypto");
        let array = js_sys::Uint8Array::new_with_length(out.len() as u32);
        let get =
            js_sys::Reflect::get(&crypto, &"getRandomValues".into()).expect("getRandomValues");
        let _ = get
            .unchecked_ref::<js_sys::Function>()
            .call1(&crypto, &array);
        array.copy_to(out);
    }
}

fn policy(v: u8) -> EditPolicy {
    match v {
        1 => EditPolicy::Ops,
        2 => EditPolicy::Nobody,
        _ => EditPolicy::Everyone,
    }
}

/// The local world, driven by the local-mode worker script.
#[wasm_bindgen]
pub struct LocalWorld {
    core: LocalCore,
}

#[wasm_bindgen]
impl LocalWorld {
    /// Starts the world: with `file` (an OPFS sync access handle) it is loaded from and saved to
    /// it, and a saved world's seed and generator win. Throws if the file is locked to another
    /// version (RELEASES.md §6). With `spawn_face_b` a new world's spawn is on face B (`?spawn=b`);
    /// with `spawn_portal` by a portal's opening on that face (`?spawn=portal`, `portal-b`). With
    /// `spawn_on_open` (a spawn link) a saved world's player starts at that spawn too, not where it
    /// left the world. With `spawn_at` (x, y, z: the dev link `?at=`) the spawn is that point, and
    /// the player starts there. With `upgrade` (the player confirmed the warning, owner 2026-11-06)
    /// a file from an older version line opens too. With `water_worker` the water runs in the
    /// water worker this worker's script starts (ADR 0062: `takeWaterOut`, `waterIn`); without,
    /// on this worker's thread. With `jobs_worker` the chunks to generate and the LOD's
    /// sections are done in the server-jobs worker this worker's script starts (`takeJob`,
    /// `jobIn`); without, on the tick within its budgets. `mode`: a new world's game mode (ADR
    /// 0065; the New world form's choice, by name); a saved world keeps its own. `growth` (dev,
    /// `?growth=N`): trees grow N times as fast (ADR 0079). `chop2` (dev, `?chop=v2`): the
    /// chopping prototype (docs/WORK_QUEUE.md item 7).
    // Positional, as JS passes them (wasm-bindgen constructor), like `LocalCore::create_spawning`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(constructor)]
    pub fn new(
        world_seed: f64,
        generator_version: u32,
        file: Option<FileSystemSyncAccessHandle>,
        spawn_face_b: Option<bool>,
        spawn_portal: Option<bool>,
        spawn_on_open: Option<bool>,
        spawn_at: Option<Vec<f64>>,
        upgrade: Option<bool>,
        water_worker: Option<bool>,
        jobs_worker: Option<bool>,
        mode: Option<String>,
        growth: Option<f64>,
        chop2: Option<bool>,
    ) -> Result<LocalWorld, JsError> {
        // The tick's phases timed by this worker's clock (`tickPhases`, docs/DEV_TOOLS.md).
        dwell_sim::clock::set_clock(worker_now);
        let backend = file.map(OpfsBackend::register);
        let face = if spawn_face_b == Some(true) {
            dwell_world::Face::B
        } else {
            dwell_world::Face::A
        };
        match LocalCore::create_tuned(
            face,
            spawn_portal == Some(true),
            spawn_at.and_then(|v| <[f64; 3]>::try_from(v).ok()),
            spawn_on_open == Some(true),
            upgrade == Some(true),
            world_seed as u64,
            generator_version,
            backend,
            Box::new(WebEntropy),
            if water_worker == Some(true) {
                dwell_sim::water::host::Placement::Remote
            } else {
                dwell_sim::water::host::Placement::Inline
            },
            jobs_worker == Some(true),
            |c| {
                if let Some(m) = mode {
                    c.mode = m;
                }
                // `?growth=N` (dev): trees grow N times as fast (ADR 0079).
                if let Some(g) = growth.filter(|g| g.is_finite() && *g > 0.0) {
                    c.growth_speed = g;
                }
                // `?chop=v2` (dev): the chopping prototype.
                if chop2 == Some(true) {
                    c.chop2 = true;
                }
            },
        ) {
            Ok(core) => Ok(Self { core }),
            Err(LocalError::VersionLocked(m)) => Err(JsError::new(&m)),
        }
    }
    #[wasm_bindgen(getter)]
    pub fn persisted(&self) -> bool {
        self.core.persisted
    }
    #[wasm_bindgen(getter, js_name = storageError)]
    pub fn storage_error(&self) -> String {
        self.core.storage_error.clone()
    }
    pub fn connected(&mut self, session: u32, kind: u8, binding: &[u8]) {
        let kind = TransportKind::from_u8(kind).unwrap_or(TransportKind::Loopback);
        self.core
            .connected(session, kind, binding.try_into().unwrap_or([0; 32]));
    }
    pub fn disconnected(&mut self, session: u32) {
        self.core.disconnected(session);
    }
    pub fn reliable(&mut self, session: u32, channel: u8, bytes: &[u8]) {
        if let Some(c) = Channel::from_u8(channel) {
            self.core.reliable(session, c, bytes);
        }
    }
    pub fn datagram(&mut self, session: u32, bytes: &[u8]) {
        self.core.datagram(session, bytes);
    }
    pub fn host(&mut self, max_players: u16, edits: u8, flight: u8, host_key: &[u8]) {
        self.core.host(
            max_players,
            policy(edits),
            policy(flight),
            host_key.try_into().unwrap_or([0; 32]),
        );
    }
    /// The host changes the world's game mode by name (ADR 0065); false for an unknown one.
    #[wasm_bindgen(js_name = setMode)]
    pub fn set_mode(&mut self, name: &str) -> bool {
        self.core.set_mode(name)
    }
    /// The world's game mode's name.
    #[wasm_bindgen(getter)]
    pub fn mode(&self) -> String {
        self.core.mode().into()
    }
    #[wasm_bindgen(js_name = hostStatus)]
    pub fn host_status(&mut self, state: u8) {
        self.core.host_status(if state == HostState::Paused as u8 {
            HostState::Paused
        } else {
            HostState::Resumed
        });
    }
    #[wasm_bindgen(js_name = closeGuests)]
    pub fn close_guests(&mut self) {
        self.core.close_guests();
    }
    pub fn save(&mut self) -> bool {
        self.core.save()
    }
    /// Carries the local player on a raft at `speed` m/s toward `yaw` (degrees, 0 = +Z) for
    /// `seconds`, `lift` m above its feet (the bench's `carry` step, docs/DEV_TOOLS.md).
    pub fn carry(&mut self, speed: f64, yaw: f64, seconds: f64, lift: f64) -> bool {
        self.core.carry_local_player(speed, yaw, seconds, lift)
    }
    /// Gives the local player `count` of `item` (the bench's `give` step); what didn't fit.
    pub fn give(&mut self, item: &str, count: u16) -> u16 {
        self.core.give_local_player(item, count)
    }
    /// Kills the local player (the bench's `die` step).
    pub fn kill(&mut self) {
        self.core.kill_local_player();
    }
    /// Moves the local player's feet to (x, y, z) (m), at rest, on its face (the dev tools'
    /// `goto`, docs/DEV_TOOLS.md); false otherwise.
    pub fn teleport(&mut self, x: f64, y: f64, z: f64) -> bool {
        self.core.teleport_local_player([x, y, z])
    }
    /// The lava's dev tools (`LocalCore::lava_dev`): `report`, `rate` [k], `vent` [x, y, z, rate,
    /// depth]; JSON back.
    #[wasm_bindgen(js_name = lavaDev)]
    pub fn lava_dev(&mut self, op: &str, args: &[f64]) -> String {
        self.core.lava_dev(op, args)
    }
    /// The nearest water of `kind` (`water`, `sea`, `river`, `lake`) on the world map from (x, y,
    /// z) within `radius` m, on face `face` (1 A, -1 B, 0 the point's), `min_depth` m deep at
    /// least: JSON or `null`
    /// (`LocalCore::find_on_map`).
    #[wasm_bindgen(js_name = findOnMap)]
    #[allow(clippy::too_many_arguments)]
    pub fn find_on_map(
        &mut self,
        kind: &str,
        x: f64,
        y: f64,
        z: f64,
        radius: f64,
        face: i32,
        min_depth: f64,
    ) -> String {
        let face = (face != 0).then(|| dwell_world::FaceFrame::of_sign(f64::from(face)));
        self.core
            .find_on_map(kind, [x, y, z], radius, face, min_depth)
    }
    /// The water near a cell as this server has it (JSON; the block dump's `water`, a diagnostic);
    /// undefined while the water worker's is on its way (`takeWaterReport`).
    #[wasm_bindgen(js_name = waterReport)]
    pub fn water_report(&mut self, x: i32, y: i32, z: i32, radius: i32) -> Option<String> {
        self.core.water_report([x, y, z], radius)
    }
    /// A report the water worker sent back.
    #[wasm_bindgen(js_name = takeWaterReport)]
    pub fn take_water_report(&mut self) -> Option<String> {
        self.core.take_water_report()
    }
    /// The next batch for the water worker (ADR 0062), if any.
    #[wasm_bindgen(js_name = takeWaterOut)]
    pub fn take_water_out(&mut self) -> Option<Vec<u8>> {
        self.core.server().take_water_outgoing()
    }
    /// The water worker's answer (applied at the next tick's start); false if unreadable.
    #[wasm_bindgen(js_name = waterIn)]
    pub fn water_in(&mut self, bytes: &[u8]) -> bool {
        self.core.server().deliver_water(bytes)
    }
    /// The world's generator (a saved world's own), for the server-jobs worker.
    #[wasm_bindgen(getter, js_name = generatorVersion)]
    pub fn generator_version(&mut self) -> u32 {
        self.core.server().config().generator_version
    }
    /// The world's seed (a saved world's own), for the server-jobs worker.
    #[wasm_bindgen(getter, js_name = worldSeed)]
    pub fn world_seed(&mut self) -> u64 {
        self.core.server().config().world_seed
    }
    /// The next job for the server-jobs worker, if any.
    #[wasm_bindgen(js_name = takeJob)]
    pub fn take_job(&mut self) -> Option<Vec<u8>> {
        self.core.server().take_job()
    }
    /// The server-jobs worker's answer to a job; false if unreadable.
    #[wasm_bindgen(js_name = jobIn)]
    pub fn job_in(&mut self, bytes: &[u8]) -> bool {
        self.core.server().deliver_job(bytes)
    }
    /// Asks the water for its record (before `save`, when the world closes).
    #[wasm_bindgen(js_name = requestWaterRecord)]
    pub fn request_water_record(&mut self) {
        self.core.server().request_water_record();
    }
    /// Whether the record asked for has come (or none was asked).
    #[wasm_bindgen(js_name = waterRecordReady)]
    pub fn water_record_ready(&mut self) -> bool {
        self.core.server().water_record_ready()
    }
    /// The water's figures (docs/DEV_TOOLS.md): batches not yet answered, runs, the last run's
    /// water time run, owed and dropped (s) and wall time (ms), the longest run (ms), the water's
    /// clock (s), awake and all water chunks, its copy of the world (bytes, chunks), writes
    /// dropped as overtaken.
    #[wasm_bindgen(js_name = waterStats)]
    pub fn water_stats(&mut self) -> Vec<f64> {
        let (h, overtaken) = self.core.server().water_stats();
        let l = h.last;
        vec![
            f64::from(h.queue),
            h.runs as f64,
            l.ran_s,
            l.owed_s,
            l.dropped_s,
            l.busy_ms,
            h.max_busy_ms,
            l.t,
            f64::from(l.awake),
            f64::from(l.chunks),
            l.cell_bytes as f64,
            f64::from(l.cell_chunks),
            overtaken as f64,
        ]
    }
    /// The respawn preload on or off (`?respawnPreload=off`).
    #[wasm_bindgen(js_name = setRespawnPreload)]
    pub fn set_respawn_preload(&mut self, on: bool) {
        self.core.set_respawn_preload(on);
    }
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        self.core.advance(elapsed_seconds)
    }
    /// Each tick phase's time (ms) summed since the last call, in `TICK_PHASES` order
    /// (`tickPhaseNames`).
    #[wasm_bindgen(js_name = tickPhases)]
    pub fn tick_phases(&mut self) -> Vec<f64> {
        self.core
            .server()
            .tick_phases()
            .iter()
            .map(|(_, ms)| *ms)
            .collect()
    }
    /// What the ticks did since the last call (`Server::tick_counts`), as `[name, count]` pairs
    /// flattened: names at even places, counts at odd ones.
    #[wasm_bindgen(js_name = tickCounts)]
    pub fn tick_counts(&mut self) -> Vec<JsValue> {
        self.core
            .server()
            .tick_counts()
            .iter()
            .flat_map(|&(n, c)| [JsValue::from_str(n), JsValue::from_f64(c as f64)])
            .collect()
    }
    /// The tick phases' names (`dwell_sim::server::TICK_PHASES`).
    #[wasm_bindgen(js_name = tickPhaseNames)]
    pub fn tick_phase_names() -> Vec<String> {
        dwell_sim::server::TICK_PHASES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
    /// The outbox, encoded as `local::encode_outbox` describes.
    #[wasm_bindgen(js_name = takeOutbox)]
    pub fn take_outbox(&mut self) -> Vec<u8> {
        encode_outbox(&self.core.take_outbox())
    }
}
