//! The world database: redb tables mirroring the old SQLite schema (§6.4).

use std::path::Path;
use std::sync::{Arc, Mutex};

use dwell_blocks::{REGISTRY_HASH, StateId, info, parse_state};
use dwell_world::ChunkCoord;
use dwell_world::lod::LodCoord;
use redb::{Database, ReadableDatabase, ReadableTable, StorageBackend, TableDefinition};

use crate::codec::*;

/// Format written by this build (`meta.format_version`). Format 1 is the first Rust format (ADR
/// 0028: a new compatibility line, so the SQLite formats of the C++ builds are not read). A newer
/// file is refused: worlds are version-locked, not converted (RELEASES.md §6).
pub const FORMAT_VERSION: u64 = 1;
/// File extension of world files (and of the export format: the database itself).
pub const WORLD_EXTENSION: &str = ".dwellworld";

type Key3 = (i32, i32, i32);
type Key4 = (i32, i32, i32, i32);

/// Scalars by name: format_version, registry_hash, world_seed, generator_version, world_tick,
/// world_settings (the world's settings, tagged fields),
/// created_at, saved_at (little-endian u64/i64), spawn (3 × f64), app versions (UTF-8).
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const SETTINGS: TableDefinition<&str, &str> = TableDefinition::new("settings");
/// (cx, cy, cz) → (revision, generator version, LZ4 palette + RLE of world state ids).
const CHUNKS: TableDefinition<Key3, (u32, u32, &[u8])> = TableDefinition::new("chunks");
/// (level, i, j, k) → (revision, dirty, generator version, LZ4 palette + RLE of runtime ids).
const LOD_SECTIONS: TableDefinition<Key4, (u32, bool, u32, &[u8])> =
    TableDefinition::new("lod_sections");
/// public key → (display name, state blob, first seen, last seen).
const PLAYERS: TableDefinition<&[u8; 32], (&str, &[u8], i64, i64)> =
    TableDefinition::new("players");
/// (public key, kind) → (reason, granted by, at).
type PermissionRow = (&'static str, Option<&'static [u8; 32]>, i64);
const PERMISSIONS: TableDefinition<(&[u8; 32], u8), PermissionRow> =
    TableDefinition::new("permissions");
/// World state id → canonical state string.
const BLOCK_STATES: TableDefinition<u32, &str> = TableDefinition::new("block_states");
/// Moving body id → its record (`encode_body`; docs/BUILT_BODIES.md §7): its kind, pose,
/// velocities and cells, their states by name. Rewritten whole on each save that carries bodies.
const BODIES: TableDefinition<u32, &[u8]> = TableDefinition::new("bodies");
/// The water (docs/WATER_SURFACE_GRID.md §2b): one record (key 0), `dwell_sim::water::Water::save`'s
/// bytes compressed (the free water; held water is found again from the blocks). Optional: a build
/// that doesn't know it leaves it, and a build that doesn't read its format finds the water again.
const WATER: TableDefinition<u32, &[u8]> = TableDefinition::new("water");

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMeta {
    pub world_seed: u64,
    pub generator_version: u32,
    /// Feet; unset: the generator's spawn point.
    pub spawn: Option<[f64; 3]>,
    pub world_tick: u32,
    /// Unix seconds.
    pub created_at: i64,
    pub saved_at: i64,
    /// The app versions that created the world and last saved it (RELEASES.md §6). On save,
    /// `created` is kept if the file already has one.
    pub app_version_created: String,
    pub app_version_last: String,
    /// The world's settings (`dwell_protocol::world_settings`: its clock, its water speed) in
    /// their tagged form, fields a newer build added kept; none: a world from before them (the
    /// build's defaults).
    pub settings: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedChunk {
    pub coord: ChunkCoord,
    pub revision: u32,
    /// `CHUNK_VOLUME` runtime state ids, chunk index order.
    pub voxels: Vec<u16>,
}

/// A modified LOD section: a cache derivable from the chunks. `encoded` is the palette + RLE of its
/// 34³ cells; empty for a section still to compute (`dirty`).
#[derive(Clone, Debug, PartialEq)]
pub struct SavedLodSection {
    pub coord: LodCoord,
    pub revision: u32,
    pub dirty: bool,
    pub encoded: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKind {
    Op = 0,
    Ban = 1,
    Allow = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PermissionEntry {
    pub key: [u8; 32],
    pub kind: PermissionKind,
    pub reason: String,
    /// Who granted it (unset: the server's operator).
    pub by: Option<[u8; 32]>,
    /// Unix seconds; 0 on save: now.
    pub at: i64,
}

/// A moving body saved with the world (docs/BUILT_BODIES.md §7): its kind (an index into the body
/// kinds), settles so far, pose and velocities, and cells (offsets in its grid, runtime states).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedBody {
    pub id: u32,
    pub kind: u8,
    pub settles: u8,
    pub position: [f64; 3],
    /// A unit quaternion (x, y, z, w).
    pub rotation: [f64; 4],
    pub velocity: [f64; 3],
    pub angular: [f64; 3],
    pub cells: Vec<([i16; 3], StateId)>,
}

/// A body's record: kind, settles, pose (f64s), velocities, then each cell's offset and its state's
/// canonical name (so a registry change keeps it, as chunks' state names do).
fn encode_body(b: &SavedBody) -> Vec<u8> {
    let mut out = vec![b.kind, b.settles];
    for v in b
        .position
        .iter()
        .chain(&b.rotation)
        .chain(&b.velocity)
        .chain(&b.angular)
    {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(b.cells.len() as u32).to_le_bytes());
    for &(o, m) in &b.cells {
        for v in o {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let name = dwell_blocks::try_info(m)
            .map_or("dwell:air", |s| s.name)
            .as_bytes();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
    }
    out
}

/// [`encode_body`] read back; a cell whose state this build doesn't know is left out, a record cut
/// short is none.
fn decode_body(id: u32, bytes: &[u8]) -> Option<SavedBody> {
    let mut at = 0usize;
    let mut take = |n: usize| {
        let s = bytes.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let (kind, settles) = (take(1)?[0], take(1)?[0]);
    let mut f = || take(8).map(|s| f64::from_le_bytes(s.try_into().expect("8 bytes")));
    let position = [f()?, f()?, f()?];
    let rotation = [f()?, f()?, f()?, f()?];
    let velocity = [f()?, f()?, f()?];
    let angular = [f()?, f()?, f()?];
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
    let mut cells = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let mut i = || take(2).map(|s| i16::from_le_bytes(s.try_into().expect("2 bytes")));
        let o = [i()?, i()?, i()?];
        let len = usize::from(u16::from_le_bytes(take(2)?.try_into().ok()?));
        let name = std::str::from_utf8(take(len)?).ok()?;
        if let Some(m) = parse_state(name).ok().filter(|&m| m != 0) {
            cells.push((o, m));
        }
    }
    Some(SavedBody {
        id,
        kind,
        settles,
        position,
        rotation,
        velocity,
        angular,
        cells,
    })
}

/// Everything one autosave writes, committed in one transaction.
#[derive(Clone, Debug, Default)]
pub struct SaveBatch {
    pub meta: Option<WorldMeta>,
    pub chunks: Vec<SavedChunk>,
    pub players: Vec<PlayerRecord>,
    /// LOD rows written or re-marked since the last save; `clear_lod` drops the table first.
    pub lod_sections: Vec<SavedLodSection>,
    pub clear_lod: bool,
    /// The world's moving bodies now, replacing those saved before (None: leave them).
    pub bodies: Option<Vec<SavedBody>>,
    /// The water now (`Water::save`), replacing what was saved before (None: leave it).
    pub water: Option<Vec<u8>>,
}

/// A [`SaveBatch`] being encoded for its commit (`WorldDb::prepare`): its chunks mapped to the
/// file's state ids and compressed, its LOD sections compressed, its bodies' records and its water
/// compressed, one piece each `encode_step`. Nothing is written until `WorldDb::commit`.
pub struct PreparedSave {
    /// The batch; a chunk's voxels are dropped once encoded.
    batch: SaveBatch,
    /// The file's state ids with those this batch adds (`new_states`), the file's after the commit.
    states: States,
    new_states: Vec<(u32, String)>,
    /// The blobs encoded so far, in the batch's order.
    chunks: Vec<Vec<u8>>,
    lod: Vec<Vec<u8>>,
    bodies: Vec<Vec<u8>>,
    water: Option<Vec<u8>>,
}

impl PreparedSave {
    /// Encodes the next piece (a chunk, a LOD section, a body, the water); false once all are.
    pub fn encode_step(&mut self) -> bool {
        let b = &mut self.batch;
        if let Some(c) = b.chunks.get_mut(self.chunks.len()) {
            let voxels = std::mem::take(&mut c.voxels);
            let mut world_voxels = Vec::with_capacity(voxels.len());
            for v in voxels {
                // Unknown ids read as air.
                let v = if dwell_blocks::try_info(v).is_some() {
                    v
                } else {
                    0
                };
                let id = match self.states.to_world.get(usize::from(v)).copied().flatten() {
                    Some(id) => id,
                    None => {
                        let id = self.states.strings.len() as u32;
                        let name = info(v).name;
                        self.new_states.push((id, name.to_owned()));
                        self.states.strings.push(name.to_owned());
                        self.states.to_runtime.push(Some(v));
                        self.states.set_world(v, id);
                        id
                    }
                };
                world_voxels.push(id as u16);
            }
            self.chunks.push(compress_chunk(&world_voxels));
        } else if let Some(l) = b.lod_sections.get(self.lod.len()) {
            self.lod.push(if l.encoded.is_empty() {
                Vec::new()
            } else {
                compress_bytes(&l.encoded)
            });
        } else if let Some(body) = b.bodies.as_ref().and_then(|v| v.get(self.bodies.len())) {
            self.bodies.push(encode_body(body));
        } else if let (Some(water), None) = (&b.water, &self.water) {
            self.water = Some(compress_bytes(water));
        } else {
            return false;
        }
        true
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Unix time (s). `SystemTime::now` panics on wasm32-unknown-unknown (no clock): the browser's
/// local mode asks JS.
fn now() -> i64 {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    }
}

/// The id maps between the file's world state ids and this build's runtime ids.
#[derive(Clone, Debug, Default)]
struct States {
    /// World state id → canonical string.
    strings: Vec<String>,
    /// World state id → runtime id (`None`: a state this build does not know).
    to_runtime: Vec<Option<StateId>>,
    /// Runtime id → world state id (`None`: not in the file yet).
    to_world: Vec<Option<u32>>,
    /// States the file names that are not the build's (ADR 0038): blocks registered at run time
    /// (a mod's), and placeholders for strings this process has no state for.
    placeholders: Vec<StateId>,
}

impl States {
    fn from_strings(strings: Vec<String>) -> Self {
        let mut s = Self {
            to_runtime: vec![None; strings.len()],
            to_world: vec![None; dwell_blocks::state_count()],
            placeholders: Vec::new(),
            strings,
        };
        for i in 0..s.strings.len() {
            let text = &s.strings[i];
            let runtime = match parse_state(text) {
                // The build's state (a string without a property the build has reads at its
                // default), or an alias (ADR 0063: a property or value this build doesn't know,
                // kept under the exact string and saved back unchanged).
                Ok(runtime) => Some(runtime),
                // A block this process lacks (a mod's, ADR 0038): a placeholder under the same
                // string, so the chunk loads and saves it back.
                _ => dwell_blocks::sync::placeholder(text).ok().inspect(|&p| {
                    if !s.placeholders.contains(&p) {
                        s.placeholders.push(p);
                    }
                }),
            };
            if let Some(runtime) = runtime {
                if !dwell_blocks::is_build_state(runtime) && !s.placeholders.contains(&runtime) {
                    s.placeholders.push(runtime);
                }
                s.to_runtime[i] = Some(runtime);
                s.set_world(runtime, i as u32);
            }
        }
        s
    }

    fn set_world(&mut self, runtime: StateId, world: u32) {
        let at = usize::from(runtime);
        if at >= self.to_world.len() {
            self.to_world.resize(at + 1, None);
        }
        self.to_world[at] = Some(world);
    }
}

/// A world file. Shareable across threads: redb readers never wait for a writer (MVCC), so the
/// tick reads while the save thread commits.
pub struct WorldDb {
    db: Database,
    states: Mutex<States>,
    /// Tests: called inside `save`'s transaction, just before the commit; an error aborts it.
    pub before_commit: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>,
}

impl std::fmt::Debug for WorldDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldDb").finish_non_exhaustive()
    }
}

/// The world's meta from its `meta` table rows; `None` without a seed (a new world).
fn meta_from(get: impl Fn(&str) -> Option<Vec<u8>>) -> Option<WorldMeta> {
    let seed = get("world_seed")?;
    let text = |k: &str| {
        get(k)
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default()
    };
    Some(WorldMeta {
        world_seed: u64_of(&seed),
        generator_version: get("generator_version").map_or(0, |b| u64_of(&b) as u32),
        world_tick: get("world_tick").map_or(0, |b| u64_of(&b) as u32),
        created_at: get("created_at").map_or(0, |b| u64_of(&b) as i64),
        saved_at: get("saved_at").map_or(0, |b| u64_of(&b) as i64),
        spawn: get("spawn").filter(|b| b.len() == 24).map(|b| {
            std::array::from_fn(|i| {
                f64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().expect("8 bytes"))
            })
        }),
        app_version_created: text("app_version_created"),
        app_version_last: text("app_version_last"),
        settings: get("world_settings"),
    })
}

impl WorldDb {
    /// Opens or creates the world file at `path`.
    pub fn open(path: &Path) -> Result<Arc<Self>, String> {
        Self::setup(Database::create(path).map_err(err)?)
    }

    /// Opens or creates a world on another storage (the browser's OPFS; memory in tests).
    pub fn open_backend(backend: impl StorageBackend) -> Result<Arc<Self>, String> {
        Self::setup(
            Database::builder()
                .create_with_backend(backend)
                .map_err(err)?,
        )
    }

    /// A new world in memory (tests, tools).
    pub fn in_memory() -> Result<Arc<Self>, String> {
        Self::open_backend(redb::backends::InMemoryBackend::new())
    }

    /// Whether an existing file is at this build's format and registry with every table: then
    /// opening it commits no transaction.
    fn current(db: &Database) -> bool {
        let Ok(txn) = db.begin_read() else {
            return false;
        };
        let Ok(meta) = txn.open_table(META) else {
            return false;
        };
        let value = |k: &str| meta.get(k).ok().flatten().map(|v| u64_of(v.value()));
        value("format_version") == Some(FORMAT_VERSION)
            && value("registry_hash") == Some(REGISTRY_HASH)
            && txn.open_table(SETTINGS).is_ok()
            && txn.open_table(CHUNKS).is_ok()
            && txn.open_table(LOD_SECTIONS).is_ok()
            && txn.open_table(PLAYERS).is_ok()
            && txn.open_table(PERMISSIONS).is_ok()
            && txn.open_table(BLOCK_STATES).is_ok()
    }

    fn setup(db: Database) -> Result<Arc<Self>, String> {
        if Self::current(&db) {
            return Self::ready(db);
        }
        let txn = db.begin_write().map_err(err)?;
        {
            let mut meta = txn.open_table(META).map_err(err)?;
            let stored = meta
                .get("format_version")
                .map_err(err)?
                .map(|v| u64_of(v.value()));
            let version = match stored {
                Some(v) => v,
                None => {
                    meta.insert("format_version", FORMAT_VERSION.to_le_bytes().as_slice())
                        .map_err(err)?;
                    FORMAT_VERSION
                }
            };
            if version > FORMAT_VERSION {
                return Err(format!(
                    "world file format {version} is newer than this build ({FORMAT_VERSION})"
                ));
            }
            // The LOD cache holds runtime ids: dropped when the registry changed (it is rebuilt).
            let same = meta
                .get("registry_hash")
                .map_err(err)?
                .is_some_and(|v| u64_of(v.value()) == REGISTRY_HASH);
            if !same {
                meta.insert("registry_hash", REGISTRY_HASH.to_le_bytes().as_slice())
                    .map_err(err)?;
            }
            txn.open_table(SETTINGS).map_err(err)?;
            txn.open_table(CHUNKS).map_err(err)?;
            let lod = txn.open_table(LOD_SECTIONS).map_err(err)?;
            if !same {
                drop(lod);
                txn.delete_table(LOD_SECTIONS).map_err(err)?;
                txn.open_table(LOD_SECTIONS).map_err(err)?;
            }
            txn.open_table(PLAYERS).map_err(err)?;
            txn.open_table(PERMISSIONS).map_err(err)?;
            txn.open_table(BLOCK_STATES).map_err(err)?;
        }
        txn.commit().map_err(err)?;
        Self::ready(db)
    }

    fn ready(db: Database) -> Result<Arc<Self>, String> {
        let world = Arc::new(Self {
            db,
            states: Mutex::new(States::default()),
            before_commit: None,
        });
        world.refresh_states()?;
        Ok(world)
    }

    fn refresh_states(&self) -> Result<(), String> {
        let txn = self.db.begin_read().map_err(err)?;
        let table = txn.open_table(BLOCK_STATES).map_err(err)?;
        let mut strings = Vec::new();
        for (i, row) in table.iter().map_err(err)?.enumerate() {
            let (id, s) = row.map_err(err)?;
            if id.value() as usize != i {
                return Err("block_states: ids are not dense".into()); // we assign them densely
            }
            strings.push(s.value().to_owned());
        }
        *self.states.lock().expect("states") = States::from_strings(strings);
        Ok(())
    }

    /// The states this file names that are not the build's — registered ones and placeholders
    /// (ADR 0038): a server serves them with its registry.
    pub fn placeholders(&self) -> Vec<StateId> {
        self.states.lock().expect("states").placeholders.clone()
    }

    pub fn format_version(&self) -> u64 {
        self.meta_bytes("format_version").map_or(0, |b| u64_of(&b))
    }

    fn meta_bytes(&self, key: &str) -> Option<Vec<u8>> {
        let txn = self.db.begin_read().ok()?;
        let t = txn.open_table(META).ok()?;
        t.get(key).ok()?.map(|v| v.value().to_vec())
    }

    /// `None` for a new world (no meta written yet).
    pub fn load_meta(&self) -> Option<WorldMeta> {
        meta_from(|k| self.meta_bytes(k))
    }

    /// Records `version` as the app version that last saved the world (debug tools and tests:
    /// a world "saved by another version", RELEASES.md §6).
    pub fn set_app_version_last(&self, version: &str) -> Result<(), String> {
        let txn = self.db.begin_write().map_err(err)?;
        txn.open_table(META)
            .map_err(err)?
            .insert("app_version_last", version.as_bytes())
            .map_err(err)?;
        txn.commit().map_err(err)
    }

    /// A world file's meta read without opening it for writing (the file is opened read-only, so
    /// nothing is written to it): the version lock is checked before a host touches a world it
    /// may not open (RELEASES.md §6). `Ok(None)` for a missing file or one without meta.
    pub fn peek_meta(path: &Path) -> Result<Option<WorldMeta>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let db = redb::ReadOnlyDatabase::open(path).map_err(err)?;
        let txn = db.begin_read().map_err(err)?;
        let Ok(t) = txn.open_table(META) else {
            return Ok(None);
        };
        Ok(meta_from(|k| {
            t.get(k).ok().flatten().map(|v| v.value().to_vec())
        }))
    }

    /// Modified chunks in the file and their revisions.
    pub fn chunk_index(&self) -> Vec<(ChunkCoord, u32)> {
        let Ok(txn) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(t) = txn.open_table(CHUNKS) else {
            return Vec::new();
        };
        let Ok(rows) = t.iter() else {
            return Vec::new();
        };
        rows.flatten()
            .map(|(k, v)| {
                let (x, y, z) = k.value();
                (ChunkCoord::new(x, y, z), v.value().0)
            })
            .collect()
    }

    /// A saved chunk in runtime ids; `None` if missing, unreadable, or holding a state this build
    /// does not know (the chunk is generated instead).
    pub fn load_chunk(&self, c: ChunkCoord) -> Option<SavedChunk> {
        let (revision, blob) = {
            let txn = self.db.begin_read().ok()?;
            let t = txn.open_table(CHUNKS).ok()?;
            let row = t.get((c.x, c.y, c.z)).ok()??;
            let (revision, _, blob) = row.value();
            (revision, blob.to_vec())
        };
        let mut voxels = decompress_chunk(&blob)?;
        let translate = |voxels: &mut Vec<u16>, s: &States| {
            let mapped: Option<Vec<u16>> = voxels
                .iter()
                .map(|&w| s.to_runtime.get(usize::from(w)).copied().flatten())
                .collect();
            mapped.map(|m| *voxels = m).is_some()
        };
        if !translate(&mut voxels, &self.states.lock().expect("states")) {
            // Another writer may have added states since: look again.
            self.refresh_states().ok()?;
            if !translate(&mut voxels, &self.states.lock().expect("states")) {
                return None;
            }
        }
        Some(SavedChunk {
            coord: c,
            revision,
            voxels,
        })
    }

    /// The world's saved moving bodies (docs/BUILT_BODIES.md §7), in id order; an unreadable record
    /// is left out.
    pub fn bodies(&self) -> Vec<SavedBody> {
        let Ok(txn) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(t) = txn.open_table(BODIES) else {
            return Vec::new();
        };
        let Ok(rows) = t.iter() else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok())
            .filter_map(|(k, v)| decode_body(k.value(), v.value()))
            .collect()
    }

    /// The water as last saved (`Water::save`'s bytes), if any.
    pub fn water(&self) -> Option<Vec<u8>> {
        let txn = self.db.begin_read().ok()?;
        let t = txn.open_table(WATER).ok()?;
        let v = t.get(0).ok()??;
        decompress_bytes(v.value(), 1 << 30)
    }

    /// The world's block states: world state id → canonical string.
    pub fn block_states(&self) -> Vec<String> {
        let _ = self.refresh_states();
        self.states.lock().expect("states").strings.clone()
    }

    /// The LOD cache. Rows of another generator version are skipped and `stale` returned true.
    pub fn lod_sections(&self, generator_version: u32) -> (Vec<SavedLodSection>, bool) {
        let mut stale = false;
        let mut out = Vec::new();
        let Ok(txn) = self.db.begin_read() else {
            return (out, stale);
        };
        let Ok(t) = txn.open_table(LOD_SECTIONS) else {
            return (out, stale);
        };
        let Ok(rows) = t.iter() else {
            return (out, stale);
        };
        for (k, v) in rows.flatten() {
            let (level, i, j, kk) = k.value();
            let (revision, dirty, version, blob) = v.value();
            if version != generator_version {
                stale = true;
                continue;
            }
            let mut s = SavedLodSection {
                coord: LodCoord::new(level, i, j, kk),
                revision,
                dirty,
                encoded: Vec::new(),
            };
            if !blob.is_empty() {
                match decompress_bytes(blob, MAX_LOD_RAW) {
                    Some(raw) => s.encoded = raw,
                    None => s.dirty = true, // unreadable: compute it again
                }
            }
            out.push(s);
        }
        (out, stale)
    }

    pub fn load_player(&self, key: &[u8; 32]) -> Option<PlayerRecord> {
        let txn = self.db.begin_read().ok()?;
        let t = txn.open_table(PLAYERS).ok()?;
        let row = t.get(key).ok()??;
        let (name, state, first_seen, last_seen) = row.value();
        let (feet, health) = decode_player_state(state)?;
        Some(PlayerRecord {
            key: *key,
            display_name: name.to_owned(),
            feet,
            health,
            first_seen,
            last_seen,
        })
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        let txn = self.db.begin_read().ok()?;
        let t = txn.open_table(SETTINGS).ok()?;
        t.get(key).ok()?.map(|v| v.value().to_owned())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.write(|txn| {
            txn.open_table(SETTINGS)?.insert(key, value)?;
            Ok(())
        })
    }

    pub fn permissions(&self) -> Vec<PermissionEntry> {
        let Ok(txn) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(t) = txn.open_table(PERMISSIONS) else {
            return Vec::new();
        };
        let Ok(rows) = t.iter() else {
            return Vec::new();
        };
        rows.flatten()
            .filter_map(|(k, v)| {
                let (key, kind) = k.value();
                let (reason, by, at) = v.value();
                let kind = match kind {
                    0 => PermissionKind::Op,
                    1 => PermissionKind::Ban,
                    2 => PermissionKind::Allow,
                    _ => return None,
                };
                Some(PermissionEntry {
                    key: *key,
                    kind,
                    reason: reason.to_owned(),
                    by: by.copied(),
                    at,
                })
            })
            .collect()
    }

    pub fn set_permission(&self, e: &PermissionEntry) -> Result<(), String> {
        let at = if e.at != 0 { e.at } else { now() };
        self.write(|txn| {
            txn.open_table(PERMISSIONS)?.insert(
                (&e.key, e.kind as u8),
                (e.reason.as_str(), e.by.as_ref(), at),
            )?;
            Ok(())
        })
    }

    pub fn remove_permission(&self, key: &[u8; 32], kind: PermissionKind) -> Result<(), String> {
        self.write(|txn| {
            txn.open_table(PERMISSIONS)?.remove((key, kind as u8))?;
            Ok(())
        })
    }

    fn write(
        &self,
        f: impl FnOnce(&redb::WriteTransaction) -> Result<(), redb::Error>,
    ) -> Result<(), String> {
        let txn = self.db.begin_write().map_err(err)?;
        f(&txn).map_err(err)?;
        txn.commit().map_err(err)
    }

    /// Writes the batch in one transaction: all of it or (on any failure) none of it.
    pub fn save(&self, batch: &SaveBatch) -> Result<(), String> {
        let mut prepared = self.prepare(batch.clone());
        while prepared.encode_step() {}
        self.commit(prepared)
    }

    /// The batch, to encode a piece at a time (`PreparedSave::encode_step`) and then `commit`:
    /// what a save costs a caller that has no thread to give it (the browser's server, §6.4).
    /// One prepared save at a time: its new block states are numbered from the file's now.
    pub fn prepare(&self, batch: SaveBatch) -> PreparedSave {
        PreparedSave {
            states: self.states.lock().expect("states").clone(),
            new_states: Vec::new(),
            chunks: Vec::with_capacity(batch.chunks.len()),
            lod: Vec::with_capacity(batch.lod_sections.len()),
            bodies: Vec::new(),
            water: None,
            batch,
        }
    }

    /// Writes a prepared batch (encoded to the end: the rest is encoded here) in one transaction:
    /// all of it or (on any failure) none of it.
    pub fn commit(&self, mut prepared: PreparedSave) -> Result<(), String> {
        while prepared.encode_step() {}
        let PreparedSave {
            batch,
            states,
            new_states,
            chunks: chunk_blobs,
            lod: lod_blobs,
            bodies: body_records,
            water,
        } = prepared;
        let txn = self.db.begin_write().map_err(err)?;
        let now = now();
        let generator_version;
        {
            let mut meta = txn.open_table(META).map_err(err)?;
            if let Some(m) = &batch.meta {
                generator_version = m.generator_version;
                let mut put = |k: &str, v: &[u8]| meta.insert(k, v).map(|_| ()).map_err(err);
                put("world_seed", &m.world_seed.to_le_bytes())?;
                put(
                    "generator_version",
                    &u64::from(m.generator_version).to_le_bytes(),
                )?;
                put("world_tick", &u64::from(m.world_tick).to_le_bytes())?;
                if let Some(s) = &m.settings {
                    put("world_settings", s)?;
                }
                put("saved_at", &now.to_le_bytes())?;
                match m.spawn {
                    Some(s) => put(
                        "spawn",
                        &s.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>(),
                    )?,
                    None => {
                        meta.remove("spawn").map_err(err)?;
                    }
                }
                if !m.app_version_last.is_empty() {
                    meta.insert("app_version_last", m.app_version_last.as_bytes())
                        .map_err(err)?;
                }
                // Kept if present: the world's creation is recorded once.
                let has_created = meta.get("created_at").map_err(err)?.is_some();
                if !has_created {
                    let at = if m.created_at != 0 { m.created_at } else { now };
                    meta.insert("created_at", at.to_le_bytes().as_slice())
                        .map_err(err)?;
                }
                if !m.app_version_last.is_empty()
                    && meta.get("app_version_created").map_err(err)?.is_none()
                {
                    let created = if m.app_version_created.is_empty() {
                        &m.app_version_last
                    } else {
                        &m.app_version_created
                    };
                    meta.insert("app_version_created", created.as_bytes())
                        .map_err(err)?;
                }
            } else {
                generator_version = meta
                    .get("generator_version")
                    .map_err(err)?
                    .map_or(0, |v| u64_of(v.value()) as u32);
            }
        }
        if !new_states.is_empty() {
            let mut table_states = txn.open_table(BLOCK_STATES).map_err(err)?;
            for (id, name) in &new_states {
                table_states.insert(*id, name.as_str()).map_err(err)?;
            }
        }
        if !batch.chunks.is_empty() {
            let mut chunks = txn.open_table(CHUNKS).map_err(err)?;
            for (c, blob) in batch.chunks.iter().zip(&chunk_blobs) {
                chunks
                    .insert(
                        (c.coord.x, c.coord.y, c.coord.z),
                        (c.revision, generator_version, blob.as_slice()),
                    )
                    .map_err(err)?;
            }
        }
        if batch.clear_lod {
            txn.delete_table(LOD_SECTIONS).map_err(err)?;
        }
        if !batch.lod_sections.is_empty() || batch.clear_lod {
            let mut lod = txn.open_table(LOD_SECTIONS).map_err(err)?;
            for (l, blob) in batch.lod_sections.iter().zip(&lod_blobs) {
                let c = l.coord;
                lod.insert(
                    (c.level, c.i, c.j, c.k),
                    (l.revision, l.dirty, generator_version, blob.as_slice()),
                )
                .map_err(err)?;
            }
        }
        if !batch.players.is_empty() {
            let mut players = txn.open_table(PLAYERS).map_err(err)?;
            for p in &batch.players {
                let first = match players.get(&p.key).map_err(err)? {
                    Some(old) => old.value().2,
                    None if p.first_seen != 0 => p.first_seen,
                    None => now,
                };
                let last = if p.last_seen != 0 { p.last_seen } else { now };
                let state = encode_player_state(p);
                players
                    .insert(
                        &p.key,
                        (p.display_name.as_str(), state.as_slice(), first, last),
                    )
                    .map_err(err)?;
            }
        }
        if let Some(bodies) = &batch.bodies {
            txn.delete_table(BODIES).map_err(err)?;
            let mut t = txn.open_table(BODIES).map_err(err)?;
            for (b, record) in bodies.iter().zip(&body_records) {
                t.insert(b.id, record.as_slice()).map_err(err)?;
            }
        }
        if let Some(water) = &water {
            let mut t = txn.open_table(WATER).map_err(err)?;
            t.insert(0, water.as_slice()).map_err(err)?;
        }
        if let Some(hook) = &self.before_commit {
            hook()?; // dropping the transaction aborts it
        }
        txn.commit().map_err(err)?;
        *self.states.lock().expect("states") = states;
        Ok(())
    }

    /// Whether the file is intact (redb's own check; repairs what it can).
    pub fn integrity_check(db: &mut Arc<Self>) -> Result<bool, String> {
        let me = Arc::get_mut(db).ok_or("integrity_check: the world is shared")?;
        me.db.check_integrity().map_err(err)
    }
}

fn u64_of(b: &[u8]) -> u64 {
    b.get(..8)
        .and_then(|b| b.try_into().ok())
        .map_or(0, u64::from_le_bytes)
}
