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
    /// with `spawn_portal` by a portal's opening on that face (`?spawn=portal`, `portal-b`).
    /// `worldgen`: a new world's generation settings, encoded (WORLD_GENERATION.md §9; a saved
    /// world keeps its own); throws on settings this build can't read.
    #[wasm_bindgen(constructor)]
    pub fn new(
        world_seed: f64,
        generator_version: u32,
        file: Option<FileSystemSyncAccessHandle>,
        spawn_face_b: Option<bool>,
        spawn_portal: Option<bool>,
        worldgen: Option<String>,
    ) -> Result<LocalWorld, JsError> {
        let worldgen = dwell_worldgen::WorldSettings::decode(worldgen.as_deref().unwrap_or(""))
            .map_err(|e| JsError::new(&e))?;
        let backend = file.map(OpfsBackend::register);
        let face = if spawn_face_b == Some(true) {
            dwell_world::Face::B
        } else {
            dwell_world::Face::A
        };
        match LocalCore::create_spawning(
            face,
            spawn_portal == Some(true),
            world_seed as u64,
            generator_version,
            worldgen,
            backend,
            Box::new(WebEntropy),
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
    /// Kills the local player (the bench's `die` step).
    pub fn kill(&mut self) {
        self.core.kill_local_player();
    }
    /// The respawn preload on or off (`?respawnPreload=off`).
    #[wasm_bindgen(js_name = setRespawnPreload)]
    pub fn set_respawn_preload(&mut self, on: bool) {
        self.core.set_respawn_preload(on);
    }
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        self.core.advance(elapsed_seconds)
    }
    /// The outbox, encoded as `local::encode_outbox` describes.
    #[wasm_bindgen(js_name = takeOutbox)]
    pub fn take_outbox(&mut self) -> Vec<u8> {
        encode_outbox(&self.core.take_outbox())
    }
}
