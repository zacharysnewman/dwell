//! The authoritative server core (ARCHITECTURE.md §4). It has no sockets, threads it doesn't own,
//! or OS calls: the host feeds it transport events, calls `step` at SIM_HZ, and drains the outbox
//! (§4.3). The same type runs in the native server, in the browser's local mode, and in
//! friend-world hosts. Sessions are kept in id order, so every pass over them is deterministic.

use dwell_world::hash::{HashMap, HashSet};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dwell_blocks::REGISTRY_HASH;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerHandle, Players, events};
use dwell_physics::net::{dequantize_input, player_flags_of, quantize_pitch, quantize_yaw, to_net};
use dwell_physics::query::GroundRef;
use dwell_physics::{DVec3, Physics, Vec3};
use dwell_protocol::*;
use dwell_protocol::{
    Channel, ChunkForm, DamageCause, HostState, LodForm, MAX_DATAGRAM_BYTES, Message,
    PROTOCOL_VERSION, PlayerEventKind, PlayerState, RejectReason, SIM_HZ, SNAPSHOT_HZ,
    TransportKind, VoxelModificationReason, auth_transcript, limits, lod, player_flags,
    players as pc, welcome_flags, world as wc,
};
use dwell_storage::{PlayerRecord, SaveBatch, SavedLodSection, WorldMeta, WorldStore};
use dwell_world::edit::{DEFAULT_REACH, EditCapsule, EditCheck, check_block_edit};
use dwell_world::lod::{INDEX_LEVEL, LodCoord, in_world, lod_of_chunk, parent};
use dwell_world::region::{DEFAULT_REGION_REACH, RegionCheck, check_region_edit};
use dwell_world::{
    AirChunkTest, BUILD_VOLUME, CHUNK_SIZE, Chunk, ChunkCoord, ChunkGenerator,
    SavedChunks, VoxelWorld, chunk_distance, chunk_hash, chunk_of, local_index,
};

use dwell_protocol::auth::verify_ed25519;

use crate::app_version::APP_VERSION;
use crate::entropy::Entropy;
use crate::lod_propagation::{LodPropagation, LodSection};
use crate::worldgen_pool::WorldgenPool;

pub type SessionId = u32;
pub type TransportBinding = [u8; 32];

const SNAPSHOT_EVERY: u32 = SIM_HZ / SNAPSHOT_HZ; // ticks
const MAX_QUEUED_INPUTS: usize = 32; // hard cap per session (> a slow client's frame of inputs)
const INPUT_BUFFER: usize = 2; // jitter buffer depth (ticks)
/// Inputs queued beyond this for a whole window are skipped, to bound input latency. A window's
/// low-water mark decides, not the queue at one tick: a client drawing a few frames a second sends
/// a frame's ticks at once (up to the client's MAX_TICKS_PER_FRAME), and skipping that burst would
/// drop inputs the client predicted (a correction every frame).
const MAX_BUFFERED_INPUTS: usize = 6;
const LATENCY_WINDOW_TICKS: u32 = 30;
const MAX_SEQ_AHEAD: u32 = 256; // inputs further ahead are rejected
const INPUT_DATAGRAMS_PER_SECOND: u32 = 2 * SIM_HZ; // §11: SIM_HZ + redundancy
const PREFETCH_CHUNKS: i32 = 2; // generated ahead around each player (collision reach + 1)
const KEEP_CHUNKS: i32 = 3; // unmodified chunks kept around players; farther: evicted
const EVICT_EVERY_TICKS: u32 = 64;
const MAX_EXPLICIT_WANTED: usize = 64; // full-mode chunks queued for generation per tick
const MAX_QUEUED_EDITS: usize = 8; // per session per tick; more are dropped
/// Block edit rate (§6.5, §11): a token bucket refilled once per BLOCK_EDIT_INTERVAL_MS, holding a
/// small burst so edits bunched by the network are not rejected.
const EDITS_PER_TICK: f64 = 1000.0 / (pc::BLOCK_EDIT_INTERVAL_MS as f64 * SIM_HZ as f64);
const EDIT_BURST: f64 = 3.0;
/// Region edits (§6.5): a token bucket of cells, each request costing its box's volume.
const REGION_CELLS_PER_TICK: f64 = region_edit::CELLS_PER_SECOND as f64 / SIM_HZ as f64;
const REGION_BURST_CELLS: f64 = region_edit::BURST_CELLS as f64;
const MAX_SAVED_LOADS_PER_TICK: i32 = 16; // saved chunks read from the world file per tick
const INDEX_UPDATE_EVERY_TICKS: u32 = (lod::INDEX_UPDATE_MS as u32) * SIM_HZ / 1000;
const MAX_QUEUED_LOD_REQUESTS: usize = 256; // per session; more are dropped
const LOD_REQUESTS_PER_TICK: f64 = lod::REQUESTS_PER_SECOND as f64 / SIM_HZ as f64;
const MAX_QUEUED_CHUNK_REQUESTS: usize = 1024;
const CHUNK_REQUESTS_PER_TICK: f64 = wc::CHUNK_REQUESTS_PER_SECOND as f64 / SIM_HZ as f64;

/// One tick's voxel changes per chunk (the next VoxelModification), in the order chunks and cells
/// were first touched; a cell changed twice keeps its last state.
#[derive(Default)]
struct TickChanges {
    chunks: Vec<ChunkChanges>,
    /// Chunk → (its index in `chunks`, local index → its index in that chunk's changes).
    index: HashMap<ChunkCoord, (usize, HashMap<u16, usize>)>,
}

impl TickChanges {
    /// Sets the cell in the world and records the change.
    fn set(&mut self, world: &mut VoxelWorld, cell: [i32; 3], material: u16) {
        let coord = chunk_of(cell[0], cell[1], cell[2]);
        let local = local_index(
            cell[0] - coord.x * CHUNK_SIZE,
            cell[1] - coord.y * CHUNK_SIZE,
            cell[2] - coord.z * CHUNK_SIZE,
        );
        world.get_or_create(coord).set_at(local, material);
        let chunks = &mut self.chunks;
        let (at, cells) = self.index.entry(coord).or_insert_with(|| {
            chunks.push(ChunkChanges {
                coord: coord.to_array(),
                revision: 0,
                changes: Vec::new(),
            });
            (chunks.len() - 1, HashMap::default())
        });
        let list = &mut chunks[*at].changes;
        match cells.get(&(local as u16)) {
            Some(&i) => list[i].material = material,
            None => {
                cells.insert(local as u16, list.len());
                list.push(VoxelChange {
                    index: local as u16,
                    material,
                });
            }
        }
    }
}

/// Who may break and place blocks (§6.5, §11), and who may fly (creative flight, §8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditPolicy {
    Everyone,
    Ops,
    Nobody,
}

pub struct ServerConfig {
    pub name: String,
    pub motd: String,
    pub max_players: u16,
    pub world_seed: u64,
    /// §6.3; `GENERATOR_FLAT` is the movement playground's flat world.
    pub generator_version: u32,
    /// Feet position; players spread out around it. Unset: the generator's spawn point.
    pub spawn: Option<[f64; 3]>,
    /// Terrain generation and streaming (§6.3): 0 threads generate on the tick within the budget.
    pub worldgen_threads: usize,
    pub worldgen_budget: Duration,
    /// Around the spawn, generated at startup.
    pub pregen_radius_chunks: i32,
    /// Sphere streamed to each client.
    pub view_radius_chunks: i32,
    /// Beyond the view, chunks a client asks for (ChunkRequest, §6.6) are streamed within this.
    pub render_radius_chunks: i32,
    pub chunk_bytes_per_second: u32,
    /// Level of detail (§6.6): propagation threads (`None`: one when worldgen has threads, else
    /// none — then it runs on the tick within `lod_budget`), and the per-client stream budget.
    pub lod_threads: Option<usize>,
    pub lod_budget: Duration,
    pub lod_bytes_per_second: u32,
    /// Block edits (§6.5) and flight: who may; the players (device public keys) who are ops.
    pub edits: EditPolicy,
    pub flight: EditPolicy,
    pub ops: Vec<PublicKey>,
    /// Access (§10.1): banned keys are refused; with an allow-list, only its keys may join.
    pub banned: Vec<PublicKey>,
    pub allow_list: Option<Vec<PublicKey>>,
    /// World persistence (§6.4). With a store, a saved world's seed, generator and spawn replace
    /// the ones above, modified chunks and players load from it, and dirty data saves every
    /// `autosave_seconds`. Without one the world lives in memory only.
    pub store: Option<WorldStore>,
    pub autosave_seconds: u32,
    /// Tests and tools: replaces the generator for `generator_version`, e.g. to move a test world
    /// far from the origin (clients must generate the same chunks). Without an air test to match,
    /// every chunk is generated and streamed.
    pub generator_override: Option<ChunkGenerator>,
    pub air_test_override: Option<AirChunkTest>,
    pub player_config: PlayerControllerConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "Dwell Server".into(),
            motd: String::new(),
            max_players: 16,
            world_seed: 0,
            generator_version: dwell_world::GENERATOR_TERRAIN,
            spawn: None,
            worldgen_threads: 0,
            worldgen_budget: Duration::from_micros(4000),
            pregen_radius_chunks: 2,
            view_radius_chunks: wc::VIEW_RADIUS_CHUNKS as i32,
            render_radius_chunks: wc::RENDER_RADIUS_CHUNKS as i32,
            chunk_bytes_per_second: wc::CHUNK_BYTES_PER_SECOND as u32,
            lod_threads: None,
            lod_budget: Duration::from_micros(2000),
            lod_bytes_per_second: lod::BYTES_PER_SECOND as u32,
            edits: EditPolicy::Everyone,
            flight: EditPolicy::Everyone,
            ops: Vec::new(),
            banned: Vec::new(),
            allow_list: None,
            store: None,
            autosave_seconds: wc::AUTOSAVE_SECONDS as u32,
            generator_override: None,
            air_test_override: None,
            player_config: PlayerControllerConfig::default(),
        }
    }
}

/// Terrain streaming counters, for tests and diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStats {
    pub generated_sent: u32,
    pub explicit_sent: u32,
    pub air_sent: u32,
    pub unloaded: u32,
    /// Of the above, sent for a ChunkRequest (§6.6).
    pub requested_sent: u32,
    /// Over the rate or queue limit, or out of range.
    pub requests_dropped: u32,
    /// Chunks the client currently has.
    pub streamed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    Reliable(Channel),
    Datagram,
    /// Close the session's transport (after its queued messages).
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub session: SessionId,
    pub kind: OutKind,
    /// Empty for `Close`.
    pub bytes: Vec<u8>,
}

/// Level-of-detail traffic per client (§6.6), for tests and diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodStats {
    /// Sections requested and accepted.
    pub requests: u32,
    /// Over LOD_REQUESTS_PER_SECOND (or the queue), or too early.
    pub requests_dropped: u32,
    pub generated_sent: u32,
    pub explicit_sent: u32,
    pub unchanged_sent: u32,
    /// In LodIndex and LodIndexUpdate messages.
    pub index_entries: u32,
    pub index_updates: u32,
    /// On the lod stream.
    pub bytes: u64,
}

/// Per-session counters, for tests and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionStats {
    pub inputs_received: u32,
    /// Out of range (§11).
    pub inputs_rejected: u32,
    /// Over the rate limit.
    pub datagrams_dropped: u32,
    /// Dropped to bound input latency.
    pub inputs_skipped: u32,
    /// No input queued: the last one repeated.
    pub ticks_starved: u32,
    pub edits_applied: u32,
    /// Failed validation, cooldown, or permission.
    pub edits_rejected: u32,
    pub last_edit_check: EditCheck,
    pub region_edits_applied: u32,
    /// Failed validation, over the cell rate, too many queued, or no permission.
    pub region_edits_rejected: u32,
    pub last_region_check: RegionCheck,
    /// Cells region edits changed.
    pub region_cells_changed: u64,
    /// Chunks re-sent on the client's request (§6.3).
    pub resyncs: u32,
}

impl Default for SessionStats {
    fn default() -> Self {
        Self {
            inputs_received: 0,
            inputs_rejected: 0,
            datagrams_dropped: 0,
            inputs_skipped: 0,
            ticks_starved: 0,
            edits_applied: 0,
            edits_rejected: 0,
            last_edit_check: EditCheck::Ok,
            region_edits_applied: 0,
            region_edits_rejected: 0,
            last_region_check: RegionCheck::Ok,
            region_cells_changed: 0,
            resyncs: 0,
        }
    }
}

/// World persistence counters, for tests and diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveStats {
    /// Batches committed.
    pub saves: u32,
    /// Batches that failed (their chunks stay dirty).
    pub failed: u32,
    /// Modified chunks not yet committed.
    pub dirty: usize,
    /// Chunks in the world file.
    pub saved_chunks: usize,
    /// Chunks read back from the file.
    pub loaded: u64,
    pub last_error: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    AwaitingHello,
    AwaitingAuth,
    Joined,
}

/// How a client receives chunks (§6.3): nothing until its WorldgenCheck arrives, then Generated
/// messages for unmodified chunks when its generator matched, otherwise every chunk explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChunkMode {
    AwaitingCheck,
    Generated,
    Full,
}

struct Session {
    kind: TransportKind,
    binding: TransportBinding,
    phase: Phase,
    nonce: Nonce,
    public_key: PublicKey,
    display_name: String,
    player_id: u16,
    // Player (joined only).
    handle: Option<PlayerHandle>,   // none while dead
    inputs: VecDeque<(u32, Input)>, // ordered by seq
    primed: bool,                   // jitter buffer: consume only once INPUT_BUFFER are queued
    /// The fewest inputs queued at any tick of this latency window (`MAX_BUFFERED_INPUTS`).
    low_water: usize,
    window_ticks: u32,
    last_processed_seq: u32,
    last_input: Input,
    health: i32,
    respawn_tick: u32,
    death_position: [f64; 3],
    last_knockback_seq: u32,
    rate_window_tick: u32,
    rate_window_count: u32,
    edits: Vec<BlockEditRequest>, // applied at the start of the next step
    edit_credit: f64,
    region_edits: Vec<RegionEdit>, // applied after the block edits, in order
    region_credit: f64,            // cells

    stats: SessionStats,
    // Terrain streaming.
    chunk_mode: ChunkMode,
    streamed: HashSet<ChunkCoord>, // sent and not unloaded
    stream_center: Option<ChunkCoord>,
    stream_complete: bool, // everything in view sent (until the center moves)
    chunk_credit: f64,     // bytes the client may still receive this tick
    stream_stats: StreamStats,
    // Chunks beyond the view the client asked for (§6.6): queued, then streamed (and kept until
    // they leave the render radius) after the view's own chunks.
    chunk_requests: VecDeque<ChunkCoord>,
    requested: HashSet<ChunkCoord>, // queued or streamed
    chunk_request_credit: f64,
    // Level of detail (§6.6).
    lod_index_sent: bool,
    lod_requests: VecDeque<LodSectionRequest>,
    lod_credit: f64,
    lod_request_credit: f64,
    lod_stats: LodStats,
}

impl Session {
    fn new(kind: TransportKind, binding: TransportBinding) -> Self {
        Self {
            kind,
            binding,
            phase: Phase::AwaitingHello,
            nonce: [0; 32],
            public_key: [0; 32],
            display_name: String::new(),
            player_id: 0,
            handle: None,
            inputs: VecDeque::new(),
            primed: false,
            low_water: usize::MAX,
            window_ticks: 0,
            last_processed_seq: 0,
            last_input: Input::default(),
            health: pc::MAX_HEALTH as i32,
            respawn_tick: 0,
            death_position: [0.0; 3],
            last_knockback_seq: 0,
            rate_window_tick: 0,
            rate_window_count: 0,
            edits: Vec::new(),
            edit_credit: 0.0,
            region_edits: Vec::new(),
            region_credit: 0.0,
            stats: SessionStats::default(),
            chunk_mode: ChunkMode::AwaitingCheck,
            streamed: HashSet::default(),
            stream_center: None,
            stream_complete: false,
            chunk_credit: 0.0,
            stream_stats: StreamStats::default(),
            chunk_requests: VecDeque::new(),
            requested: HashSet::default(),
            chunk_request_credit: wc::CHUNK_REQUESTS_PER_SECOND as f64,
            lod_index_sent: false,
            lod_requests: VecDeque::new(),
            lod_credit: 0.0,
            lod_request_credit: lod::REQUESTS_PER_SECOND as f64,
            lod_stats: LodStats::default(),
        }
    }
    fn joined(&self) -> bool {
        self.phase == Phase::Joined
    }
}

fn within_radius(a: ChunkCoord, b: ChunkCoord, r: i32) -> bool {
    let (dx, dy, dz) = (
        i64::from(a.x - b.x),
        i64::from(a.y - b.y),
        i64::from(a.z - b.z),
    );
    let r = i64::from(r);
    dx * dx + dy * dy + dz * dz <= r * r + r
}

fn chunk_at(x: f64, y: f64, z: f64) -> ChunkCoord {
    chunk_of(x.floor() as i32, y.floor() as i32, z.floor() as i32)
}

fn add(a: ChunkCoord, b: ChunkCoord) -> ChunkCoord {
    ChunkCoord::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn len_sq(c: &ChunkCoord) -> i32 {
    c.x * c.x + c.y * c.y + c.z * c.z
}

/// Offsets within Chebyshev distance `r`, nearest first.
fn cube_offsets(r: i32) -> Vec<ChunkCoord> {
    let mut out = Vec::new();
    for y in -r..=r {
        for z in -r..=r {
            for x in -r..=r {
                out.push(ChunkCoord::new(x, y, z));
            }
        }
    }
    out.sort_by_key(len_sq); // stable
    out
}

/// §11: move within the unit circle (plus quantization slack), pitch within ±90°.
fn input_in_range(f: &InputFrame) -> bool {
    let (mx, my) = (i32::from(f.move_x), i32::from(f.move_y));
    mx >= -127 && my >= -127 && mx * mx + my * my <= 127 * 127 + 254 && f.pitch >= -32767
}

fn encode(m: &Message) -> Vec<u8> {
    m.encode()
}

/// The world file behind the voxel grid: chunks the world has saved (the server's index) load from it.
struct StoreChunks {
    db: Arc<dwell_storage::WorldDb>,
    saved: Arc<Mutex<HashMap<ChunkCoord, u32>>>,
}

impl SavedChunks for StoreChunks {
    fn has(&self, c: ChunkCoord) -> bool {
        self.saved.lock().expect("saved index").contains_key(&c)
    }
    fn load(&mut self, c: ChunkCoord) -> Option<Chunk> {
        let saved = self.db.load_chunk(c)?;
        Chunk::from_voxels(&saved.voxels, saved.revision)
    }
}

pub struct Server {
    config: ServerConfig,
    entropy: Box<dyn Entropy>,
    physics: Physics,
    air_test: Option<AirChunkTest>,
    world: VoxelWorld,
    players: Players,
    sessions: BTreeMap<SessionId, Session>,
    outbox: Vec<Outgoing>,
    knockbacks: Vec<(u16, Vec3)>, // applied in the next step
    tick: u32,
    next_player_id: u16,
    worldgen: WorldgenPool,
    generator: ChunkGenerator,
    verification_chunk: ChunkCoord,
    verification_hash: u64,
    view_offsets: Vec<ChunkCoord>, // chunks in view relative to the center, nearest first
    prefetch_offsets: Vec<ChunkCoord>, // generated ahead around each player
    explicit_wanted: Vec<ChunkCoord>, // full-mode chunks to generate for streaming
    // Persistence: chunks in the world file (revision saved), modified chunks not saved yet, saves
    // in flight (their chunk revisions), and players who left since the last save.
    saved: Arc<Mutex<HashMap<ChunkCoord, u32>>>,
    dirty: HashSet<ChunkCoord>,
    in_flight: HashMap<u64, Vec<(ChunkCoord, u32)>>,
    departed: Vec<PlayerRecord>,
    next_save_tick: u32,
    save_stats: SaveStats,
    // Level of detail: sections, index entries changed since the last LodIndexUpdate, sections to
    // save (and those in saves in flight), and whether the saved cache must be dropped (rebuild).
    lod: LodPropagation,
    index_updates: BTreeMap<(i32, i32, i32), LodIndexEntry>,
    next_index_update_tick: u32,
    lod_unsaved: HashSet<LodCoord>,
    lod_in_flight: HashMap<u64, Vec<LodCoord>>,
    lod_rebuild: bool,
}

impl Server {
    pub fn new(mut config: ServerConfig, entropy: Box<dyn Entropy>) -> Self {
        // A saved world keeps its own seed, generator and spawn (launch options apply to new worlds).
        if let Some(meta) = config.store.as_ref().and_then(|s| s.db().load_meta()) {
            config.world_seed = meta.world_seed;
            config.generator_version = meta.generator_version;
            if meta.spawn.is_some() {
                config.spawn = meta.spawn;
            }
        }
        let (generator, air_test) = match &config.generator_override {
            Some(g) => (g.clone(), config.air_test_override.clone()),
            None => (
                dwell_worldgen::generator_for(config.generator_version, config.world_seed),
                Some(dwell_worldgen::air_test_for(
                    config.generator_version,
                    config.world_seed,
                )),
            ),
        };
        let mut physics = Physics::default();
        let players = Players::new(&mut physics, true);
        let lod_threads = config
            .lod_threads
            .unwrap_or(usize::from(config.worldgen_threads > 0));
        let spawn = *config.spawn.get_or_insert_with(|| {
            dwell_worldgen::spawn_point_for(config.generator_version, config.world_seed)
        });
        let mut server = Self {
            entropy,
            physics,
            world: VoxelWorld::new(generator.clone(), air_test.clone()),
            air_test,
            players,
            sessions: BTreeMap::new(),
            outbox: Vec::new(),
            knockbacks: Vec::new(),
            tick: 0,
            next_player_id: 1,
            worldgen: WorldgenPool::new(generator.clone(), config.worldgen_threads),
            generator,
            verification_chunk: chunk_at(spawn[0], spawn[1] - 1.0, spawn[2]),
            verification_hash: 0,
            view_offsets: Vec::new(),
            prefetch_offsets: cube_offsets(PREFETCH_CHUNKS),
            explicit_wanted: Vec::new(),
            saved: Arc::new(Mutex::new(HashMap::default())),
            dirty: HashSet::default(),
            in_flight: HashMap::default(),
            departed: Vec::new(),
            next_save_tick: 0,
            save_stats: SaveStats::default(),
            lod: LodPropagation::new(
                dwell_worldgen::lod_generator_for(config.generator_version, config.world_seed),
                lod_threads,
            ),
            index_updates: BTreeMap::new(),
            next_index_update_tick: 0,
            lod_unsaved: HashSet::default(),
            lod_in_flight: HashMap::default(),
            lod_rebuild: false,
            config,
        };
        server.init_storage();

        // Pre-generate the spawn region, so the first players never wait for terrain. The
        // verification chunk is always generated: clients check their generator against it,
        // edited or not.
        let spawn_chunk = chunk_at(spawn[0], spawn[1], spawn[2]);
        let mut region: Vec<ChunkCoord> = cube_offsets(server.config.pregen_radius_chunks)
            .into_iter()
            .map(|o| add(spawn_chunk, o))
            .filter(|&c| !server.is_air(c) && !server.is_saved(c))
            .collect();
        region.push(server.verification_chunk);
        server.worldgen.set_wanted(&region);
        let mut ready = Vec::new();
        server.worldgen.drain(&mut ready);
        let mut hash = None;
        for (c, chunk) in ready {
            if c == server.verification_chunk {
                hash = Some(chunk_hash(&chunk));
            }
            if !server.is_saved(c) {
                server.world.put(c, *chunk);
            }
        }
        server.verification_hash = hash.unwrap_or_else(|| {
            let mut generated = Chunk::default();
            (server.generator)(server.verification_chunk, &mut generated);
            chunk_hash(&generated)
        });

        // The view: a sphere of view_radius_chunks around the center (§6.3).
        let r = server.config.view_radius_chunks;
        server.view_offsets = cube_offsets(r)
            .into_iter()
            .filter(|c| len_sq(c) <= r * r + r)
            .collect();
        server
    }

    // --- transport events (from the host) ---

    pub fn on_connected(&mut self, id: SessionId, kind: TransportKind, binding: TransportBinding) {
        self.sessions.insert(id, Session::new(kind, binding));
    }

    pub fn on_disconnected(&mut self, id: SessionId) {
        self.remove_session(id);
    }

    fn remove_session(&mut self, id: SessionId) {
        let Some(s) = self.sessions.remove(&id) else {
            return;
        };
        if self.config.store.is_some() && s.joined() {
            self.departed.push(self.record_of(&s));
        }
        if let Some(h) = s.handle {
            self.players.despawn(&mut self.physics, h);
        }
    }

    pub fn on_reliable(&mut self, id: SessionId, channel: Channel, bytes: &[u8]) {
        if !self.sessions.contains_key(&id) {
            return;
        }
        // Clients may only send on the control channel; the world channel is server → client.
        match (channel == Channel::Control)
            .then(|| Message::decode(bytes))
            .flatten()
        {
            Some(m) => self.handle_control(id, m),
            None => self.reject(id, RejectReason::Malformed, "Malformed message."),
        }
    }

    pub fn on_datagram(&mut self, id: SessionId, bytes: &[u8]) {
        if !self.sessions.get(&id).is_some_and(Session::joined) {
            return;
        }
        // Datagrams are unreliable: malformed ones are dropped, not fatal.
        match Message::decode(bytes) {
            Some(Message::DatagramPing(p)) => {
                let pong = Message::DatagramPong(DatagramPong {
                    seq: p.seq,
                    client_time_ms: p.client_time_ms,
                    server_tick: self.tick,
                });
                self.send_datagram(id, &pong);
            }
            Some(Message::PlayerInput(m)) => self.handle_input(id, &m),
            _ => {}
        }
    }

    fn handle_input(&mut self, id: SessionId, m: &PlayerInput) {
        let may_fly = self.may_fly(&self.sessions[&id]);
        let tick = self.tick;
        let s = self.sessions.get_mut(&id).expect("session");
        // Rate limit (§11): at most INPUT_DATAGRAMS_PER_SECOND per SIM_HZ-tick window.
        if tick - s.rate_window_tick >= SIM_HZ {
            s.rate_window_tick = tick;
            s.rate_window_count = 0;
        }
        s.rate_window_count += 1;
        if s.rate_window_count > INPUT_DATAGRAMS_PER_SECOND {
            s.stats.datagrams_dropped += 1;
            return;
        }
        for f in &m.inputs {
            if f.seq <= s.last_processed_seq || s.inputs.iter().any(|q| q.0 == f.seq) {
                continue; // a redundant copy of a processed or queued input
            }
            if !input_in_range(f) || f.seq > s.last_processed_seq + MAX_SEQ_AHEAD {
                s.stats.inputs_rejected += 1;
                continue;
            }
            s.stats.inputs_received += 1;
            let at = s
                .inputs
                .iter()
                .position(|q| q.0 > f.seq)
                .unwrap_or(s.inputs.len());
            let mut input = dequantize_input(f);
            input.fly = input.fly && may_fly; // creative flight is the server's to allow (§8.3)
            s.inputs.insert(at, (f.seq, input));
            while s.inputs.len() > MAX_QUEUED_INPUTS {
                s.inputs.pop_front();
            }
        }
    }

    fn handle_control(&mut self, id: SessionId, m: Message) {
        match &m {
            Message::StatusRequest => {
                let status = StatusResponse {
                    protocol_version: PROTOCOL_VERSION,
                    server_name: self.config.name.clone(),
                    motd: self.config.motd.clone(),
                    players: self.joined_players() as u16,
                    max_players: self.config.max_players,
                    flags: 0,
                };
                self.send_reliable(id, &Message::StatusResponse(status), Channel::Control);
                return;
            }
            Message::Ping(p) => {
                let pong = Pong {
                    seq: p.seq,
                    client_time_ms: p.client_time_ms,
                    server_tick: self.tick,
                    server_time_ms: self.time_ms(),
                };
                self.send_reliable(id, &Message::Pong(pong), Channel::Control);
                return;
            }
            _ => {}
        }
        let phase = self.sessions[&id].phase;
        match (phase, m) {
            (Phase::AwaitingHello, Message::ClientHello(hello)) => {
                if hello.protocol_version != PROTOCOL_VERSION {
                    let text = format!(
                        "Server runs Dwell {APP_VERSION} (protocol {PROTOCOL_VERSION}), client runs protocol {}.",
                        hello.protocol_version
                    );
                    self.reject(id, RejectReason::ProtocolVersion, &text);
                    return;
                }
                if self.joined_players() >= usize::from(self.config.max_players) {
                    self.reject(id, RejectReason::Full, "Server is full.");
                    return;
                }
                let mut nonce = [0u8; 32];
                self.entropy.fill(&mut nonce);
                let s = self.sessions.get_mut(&id).expect("session");
                s.public_key = hello.public_key;
                s.display_name = if hello.display_name.is_empty() {
                    "Player".into()
                } else {
                    hello.display_name
                };
                s.nonce = nonce;
                s.phase = Phase::AwaitingAuth;
                self.send_reliable(id, &Message::Challenge(nonce), Channel::Control);
            }
            (Phase::AwaitingAuth, Message::ClientAuth(signature)) => {
                self.authenticate(id, &signature)
            }
            (Phase::Joined, m) => self.handle_joined(id, m),
            _ => self.reject(id, RejectReason::Malformed, "Unexpected message."),
        }
    }

    fn authenticate(&mut self, id: SessionId, signature: &Signature) {
        let s = &self.sessions[&id];
        let key = s.public_key;
        if !verify_ed25519(
            signature,
            &key,
            &auth_transcript(&s.nonce, &s.binding, &key),
        ) {
            self.reject(id, RejectReason::AuthFailed, "Identity check failed.");
            return;
        }
        if self.config.banned.contains(&key) {
            self.reject(id, RejectReason::Banned, "You are banned from this server.");
            return;
        }
        if self
            .config
            .allow_list
            .as_ref()
            .is_some_and(|l| !l.contains(&key))
        {
            self.reject(
                id,
                RejectReason::NotAllowListed,
                "This server only admits listed players.",
            );
            return;
        }
        // The signature proves key ownership, so a new login replaces any older session for the
        // same player (e.g. one whose connection dropped but hasn't timed out yet).
        let replaced: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(o, s)| **o != id && s.joined() && s.public_key == key)
            .map(|(o, _)| *o)
            .collect();
        for other in replaced {
            self.reject(
                other,
                RejectReason::Replaced,
                "Signed in from another connection.",
            );
        }
        let player_id = self.allocate_player_id();
        let s = self.sessions.get_mut(&id).expect("session");
        s.player_id = player_id;
        s.phase = Phase::Joined;
        s.edit_credit = EDIT_BURST;
        s.region_credit = REGION_BURST_CELLS;
        let flight = self.may_fly(&self.sessions[&id]);
        let welcome = Welcome {
            player_id,
            world_seed: self.config.world_seed,
            generator_version: self.config.generator_version,
            server_tick: self.tick,
            verification_chunk: self.verification_chunk.to_array(),
            // REGION_EDITS: this server understands RegionEdit (older builds of the line reject an
            // unknown message, so clients send one only when this is set).
            flags: welcome_flags::REGION_EDITS | if flight { welcome_flags::FLIGHT } else { 0 },
            registry_hash: REGISTRY_HASH,
        };
        self.send_reliable(id, &Message::Welcome(welcome), Channel::Control);
        // A returning player continues where it left the world (§6.4), unless it was dead.
        let saved = self
            .config
            .store
            .as_ref()
            .and_then(|st| st.db().load_player(&key));
        match saved {
            Some(p) if p.health > 0 => {
                self.spawn_player(id, Some(p.feet));
                self.sessions.get_mut(&id).expect("session").health = p.health;
            }
            _ => self.spawn_player(id, None),
        }
    }

    /// Gameplay input comes as datagrams. On control: the one WorldgenCheck, which picks how the
    /// client receives chunks (§6.3), block edits (§6.5), resync and chunk/LOD requests; anything
    /// else is ignored.
    fn handle_joined(&mut self, id: SessionId, m: Message) {
        let verification_hash = self.verification_hash;
        let s = self.sessions.get_mut(&id).expect("session");
        match m {
            Message::WorldgenCheck(hash) if s.chunk_mode == ChunkMode::AwaitingCheck => {
                s.chunk_mode = if hash != 0 && hash == verification_hash {
                    ChunkMode::Generated
                } else {
                    ChunkMode::Full
                };
                self.send_lod_index(id); // §6.6: the modified sections, right after the check
            }
            Message::BlockEditRequest(edit) => {
                if s.edits.len() < MAX_QUEUED_EDITS {
                    s.edits.push(edit);
                } else {
                    s.stats.edits_rejected += 1;
                }
            }
            Message::RegionEdit(edit) => {
                if s.region_edits.len() < region_edit::MAX_QUEUED {
                    s.region_edits.push(edit);
                } else {
                    s.stats.region_edits_rejected += 1;
                }
            }
            Message::ChunkResync(coords) => self.resync(id, &coords),
            Message::ChunkRequest(coords) => self.request_chunks(id, &coords),
            Message::LodRequest(sections) => {
                // Rate-limited (LOD_REQUESTS_PER_SECOND, a token bucket); answered within the budget.
                for r in sections {
                    if s.chunk_mode == ChunkMode::AwaitingCheck
                        || s.lod_request_credit < 1.0
                        || s.lod_requests.len() >= MAX_QUEUED_LOD_REQUESTS
                    {
                        s.lod_stats.requests_dropped += 1;
                        continue;
                    }
                    s.lod_request_credit -= 1.0;
                    s.lod_stats.requests += 1;
                    s.lod_requests.push_back(r);
                }
            }
            _ => {}
        }
    }

    /// At the spawn point, or at `feet` (a saved position).
    fn spawn_player(&mut self, id: SessionId, feet: Option<[f64; 3]>) {
        let s = self.sessions.get_mut(&id).expect("session");
        // Spread players around the spawn point so they don't start inside each other.
        let slot = (i32::from(s.player_id) - 1).rem_euclid(8);
        let at = self.config.spawn.expect("spawn");
        let spawn = match feet {
            Some(f) => DVec3::from(f),
            None => DVec3::new(
                at[0] + f64::from(slot % 4) - 1.5,
                at[1],
                at[2] + f64::from(slot / 4) * 1.5 - 0.75,
            ),
        };
        s.handle =
            Some(
                self.players
                    .spawn(&mut self.physics, self.config.player_config, spawn, 0.0),
            );
        s.health = pc::MAX_HEALTH as i32;
        s.inputs.clear();
        s.primed = false;
        s.last_input = Input::default();
    }

    /// Server-originated velocity change for a player (knockback), applied before this tick's
    /// physics step and announced to its client as PlayerEvent(Knockback) for predicted replay
    /// (§9.3).
    pub fn knockback(&mut self, player_id: u16, delta_v: Vec3) {
        self.knockbacks.push((player_id, delta_v));
    }

    fn event(&self, kind: PlayerEventKind, s: &Session) -> PlayerEvent {
        PlayerEvent {
            kind,
            player_id: s.player_id,
            server_tick: self.tick,
            input_seq: s.last_processed_seq,
            vector: [0.0; 3],
            position: [0.0; 3],
            amount: 0,
            cause: DamageCause::Fall, // only read for Damage and Death
        }
    }

    fn damage(&mut self, id: SessionId, amount: i32, cause: DamageCause) {
        let s = self.sessions.get_mut(&id).expect("session");
        if s.handle.is_none() || amount <= 0 {
            return;
        }
        s.health = (s.health - amount).max(0);
        let mut e = self.event(PlayerEventKind::Damage, &self.sessions[&id]);
        e.amount = amount.min(255) as u8;
        e.cause = cause;
        self.broadcast_world(&Message::PlayerEvent(e));
        if self.sessions[&id].health == 0 {
            self.kill(id, cause);
        }
    }

    fn feet_y(&self, h: PlayerHandle) -> f64 {
        // Below the capsule's centre on face A, above it on face B.
        self.players.position(&self.physics, h).y
            - f64::from(self.players.controller(h).face) * f64::from(self.players.half_height(h))
    }

    fn kill(&mut self, id: SessionId, cause: DamageCause) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let p = self.players.position(&self.physics, h);
        let death = [p.x, self.feet_y(h), p.z];
        self.players.despawn(&mut self.physics, h);
        let respawn_tick = self.tick + (pc::RESPAWN_SECONDS as u32) * SIM_HZ;
        let s = self.sessions.get_mut(&id).expect("session");
        s.death_position = death;
        s.handle = None;
        s.health = 0;
        s.inputs.clear();
        s.respawn_tick = respawn_tick;
        let mut e = self.event(PlayerEventKind::Death, &self.sessions[&id]);
        e.cause = cause;
        self.broadcast_world(&Message::PlayerEvent(e));
    }

    /// Server-side effects of the controller tick: fall damage.
    fn after_controller_tick(&mut self, id: SessionId) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let c = self.players.controller(h);
        if c.events & events::LANDED != 0 {
            let d = self.config.player_config.damage;
            if c.landed_speed > d.fall_damage_min_speed {
                let amount = ((c.landed_speed - d.fall_damage_min_speed) * d.fall_damage_per_speed)
                    .round() as i32;
                self.damage(id, amount, DamageCause::Fall);
            }
        }
    }

    /// Advances the simulation by one 1/SIM_HZ step.
    pub fn step(&mut self) {
        // 0. Chunks generated since the last tick (off-thread, or here within the budget), and
        // saves the I/O thread finished.
        let mut ready = Vec::new();
        self.worldgen
            .collect(&mut ready, self.config.worldgen_budget);
        for (c, chunk) in ready {
            if self.world.find(c).is_none() && !self.is_saved(c) {
                self.world.put(c, *chunk);
            }
        }
        self.collect_saves();

        // 1. Block edits queued since the last tick (§6.5); collision syncs in the controller pass.
        self.apply_edits();

        // 2. One input per player per tick, in sequence order (§9.3). Starved: repeat the last one.
        for s in self.sessions.values_mut() {
            let Some(h) = s.handle.filter(|_| s.joined()) else {
                continue;
            };
            // Input latency: when the queue stayed above MAX_BUFFERED_INPUTS a whole window, the
            // excess the window never drained is skipped.
            s.low_water = s.low_water.min(s.inputs.len());
            s.window_ticks += 1;
            if s.window_ticks >= LATENCY_WINDOW_TICKS {
                let excess = s.low_water.saturating_sub(MAX_BUFFERED_INPUTS);
                for _ in 0..excess.min(s.inputs.len()) {
                    s.last_processed_seq = s.inputs.pop_front().expect("queued").0;
                    s.stats.inputs_skipped += 1;
                }
                s.low_water = usize::MAX;
                s.window_ticks = 0;
            }
            // Jitter buffer: after starving, wait until INPUT_BUFFER inputs are queued again.
            if !s.primed && s.inputs.len() >= INPUT_BUFFER {
                s.primed = true;
            }
            match s.inputs.front().copied().filter(|_| s.primed) {
                Some((seq, input)) => {
                    s.last_input = input;
                    s.last_processed_seq = seq;
                    s.inputs.pop_front();
                }
                None => {
                    s.primed = false;
                    s.stats.ticks_starved += 1;
                }
            }
            self.players.set_input(h, s.last_input);
        }

        // 3. Controller pipeline (PLAYER_CONTROLLER.md §4), then its server-side consequences.
        self.players.tick(&mut self.world, &mut self.physics);
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        for &id in &ids {
            if self.sessions.get(&id).is_some_and(Session::joined) {
                self.after_controller_tick(id);
            }
        }
        for (player_id, dv) in std::mem::take(&mut self.knockbacks) {
            let Some(id) = self.session_of_player(player_id) else {
                continue;
            };
            let Some(h) = self.sessions[&id].handle else {
                continue;
            };
            self.players.add_velocity(&mut self.physics, h, dv);
            let s = self.sessions.get_mut(&id).expect("session");
            s.last_knockback_seq = s.last_processed_seq;
            let mut e = self.event(PlayerEventKind::Knockback, &self.sessions[&id]);
            e.vector = dv.to_array();
            self.broadcast_world(&Message::PlayerEvent(e));
        }

        // 4. Physics.
        self.physics.step(1.0 / f64::from(SIM_HZ));
        self.tick += 1;

        // 5. Respawns, then snapshots at SNAPSHOT_HZ.
        for &id in &ids {
            let Some(s) = self.sessions.get(&id) else {
                continue;
            };
            if !s.joined() || s.handle.is_some() || self.tick < s.respawn_tick {
                continue;
            }
            self.spawn_player(id, None);
            let h = self.sessions[&id].handle.expect("spawned");
            let p = self.players.position(&self.physics, h);
            let mut e = self.event(PlayerEventKind::Respawn, &self.sessions[&id]);
            e.position = [p.x, p.y - f64::from(self.players.half_height(h)), p.z];
            self.broadcast_world(&Message::PlayerEvent(e));
        }
        if self.tick.is_multiple_of(SNAPSHOT_EVERY) {
            self.send_snapshots();
        }

        // 6. Terrain streaming, then what to generate next and what to forget.
        self.explicit_wanted.clear();
        for &id in &ids {
            if self.sessions.contains_key(&id) {
                self.stream_chunks(id);
            }
        }
        self.update_worldgen();

        // 6b. Level of detail: propagation off the tick, index updates, answers (§6.6).
        self.update_lod();

        // 7. Autosave (§6.4): prepared here, committed off the tick.
        if self.config.store.is_some() && self.tick >= self.next_save_tick {
            self.save_now();
        }
    }

    fn view_center(&self, s: &Session) -> Option<ChunkCoord> {
        if !s.joined() {
            return None;
        }
        Some(match s.handle {
            Some(h) => {
                let p = self.players.position(&self.physics, h);
                chunk_at(p.x, p.y, p.z)
            }
            None => chunk_at(
                s.death_position[0],
                s.death_position[1],
                s.death_position[2],
            ),
        })
    }

    fn update_worldgen(&mut self) {
        let centers: Vec<ChunkCoord> = self
            .sessions
            .values()
            .filter_map(|s| self.view_center(s))
            .collect();
        // Around players first (collision needs them), then chunks full-mode clients wait for.
        // Saved chunks are read from the world file instead (a few per tick).
        let mut wanted = Vec::new();
        let mut loads = 0;
        for o in &self.prefetch_offsets {
            for &center in &centers {
                let c = add(center, *o);
                if !BUILD_VOLUME.chunk_may_hold(c) || self.world.find(c).is_some() {
                    continue;
                }
                if self.is_saved(c) {
                    if loads < MAX_SAVED_LOADS_PER_TICK {
                        self.world.get_or_create(c);
                    }
                    loads += 1;
                } else if !self.is_air(c) {
                    wanted.push(c);
                }
            }
        }
        wanted.extend_from_slice(&self.explicit_wanted);
        self.worldgen.set_wanted(&wanted);

        if self.tick.is_multiple_of(EVICT_EVERY_TICKS) {
            // Unmodified chunks can be generated again, and saved ones read again; modified chunks
            // not yet saved stay.
            let saved = self.saved.lock().expect("saved index").clone();
            let (verification, dirty) = (self.verification_chunk, &self.dirty);
            self.world.evict(|c, chunk| {
                if c == verification || dirty.contains(&c) {
                    return false;
                }
                let clean = chunk.revision() == 0 || saved.get(&c) == Some(&chunk.revision());
                clean
                    && centers
                        .iter()
                        .all(|&center| chunk_distance(c, center) > KEEP_CHUNKS)
            });
        }
    }

    fn request_chunks(&mut self, id: SessionId, coords: &[ChunkCoordNet]) {
        let center = self.view_center(&self.sessions[&id]);
        let render = self.config.render_radius_chunks;
        let s = self.sessions.get_mut(&id).expect("session");
        for &at in coords {
            let c = ChunkCoord::from_array(at);
            if s.requested.contains(&c) {
                continue; // already on its way or there
            }
            if s.streamed.contains(&c) {
                s.requested.insert(c); // streamed by the view: kept, now, while within the render radius
                continue;
            }
            let ok = s.chunk_mode != ChunkMode::AwaitingCheck
                && center.is_some_and(|center| within_radius(c, center, render))
                && BUILD_VOLUME.chunk_may_hold(c)
                && s.chunk_request_credit >= 1.0
                && s.chunk_requests.len() < MAX_QUEUED_CHUNK_REQUESTS;
            if !ok {
                s.stream_stats.requests_dropped += 1;
                continue;
            }
            s.chunk_request_credit -= 1.0;
            s.chunk_requests.push_back(c);
            s.requested.insert(c);
        }
    }

    fn stream_chunks(&mut self, id: SessionId) {
        let center = self.view_center(&self.sessions[&id]);
        let (view, render) = (
            self.config.view_radius_chunks,
            self.config.render_radius_chunks,
        );
        let per_tick = f64::from(self.config.chunk_bytes_per_second) / f64::from(SIM_HZ);
        let s = self.sessions.get_mut(&id).expect("session");
        s.chunk_request_credit = (s.chunk_request_credit + CHUNK_REQUESTS_PER_TICK)
            .min(wc::CHUNK_REQUESTS_PER_SECOND as f64);
        if s.chunk_mode == ChunkMode::AwaitingCheck {
            return;
        }
        let Some(center) = center else { return };
        s.chunk_credit = (s.chunk_credit + per_tick).min(per_tick);

        if s.stream_center != Some(center) {
            s.stream_center = Some(center);
            s.stream_complete = false;
            // Chunks beyond the view plus a margin (hysteresis) leave the client; requested ones
            // once beyond the render radius plus the margin.
            let margin = wc::UNLOAD_MARGIN_CHUNKS as i32;
            let (r, rr) = (view + margin, render + margin);
            let mut unload: Vec<ChunkCoord> = s
                .streamed
                .iter()
                .copied()
                .filter(|c| {
                    !within_radius(
                        *c,
                        center,
                        if s.requested.contains(c) {
                            r.max(rr)
                        } else {
                            r
                        },
                    )
                })
                .collect();
            unload.sort_by_key(|c| (c.x, c.y, c.z));
            for c in &unload {
                s.streamed.remove(c);
                s.requested.remove(c);
            }
            s.stream_stats.unloaded += unload.len() as u32;
            for part in unload.chunks(0xFFFF) {
                let m = Message::ChunkUnload(ChunkUnload {
                    coords: part.iter().map(|c| c.to_array()).collect(),
                });
                self.send_reliable(id, &m, Channel::World);
            }
        }
        let mut sent = 0;
        if !self.sessions[&id].stream_complete {
            self.stream_view(id, center, &mut sent);
        }
        if self.sessions[&id].stream_complete {
            // The view first (collision needs it).
            self.stream_requested(id, center, &mut sent);
        }
    }

    fn count_sent(stats: &mut StreamStats, form: ChunkForm) {
        match form {
            ChunkForm::Generated => stats.generated_sent += 1,
            ChunkForm::Air => stats.air_sent += 1,
            _ => stats.explicit_sent += 1,
        }
    }

    fn stream_view(&mut self, id: SessionId, center: ChunkCoord, sent: &mut i64) {
        // Nearest first, within the bandwidth budget (§6.3).
        let mut complete = true;
        for i in 0..self.view_offsets.len() {
            let c = add(center, self.view_offsets[i]);
            let s = &self.sessions[&id];
            // Chunks wholly outside the build volume are air everywhere: never sent (ADR 0025).
            if !BUILD_VOLUME.chunk_may_hold(c) || s.streamed.contains(&c) {
                continue;
            }
            complete = false;
            if s.chunk_credit <= 0.0 || *sent >= wc::MAX_CHUNKS_PER_TICK {
                break;
            }
            let Some(m) = self.chunk_message(id, c, false) else {
                // Full mode: generate it first (a later tick sends it).
                if self.explicit_wanted.len() < MAX_EXPLICIT_WANTED {
                    self.explicit_wanted.push(c);
                }
                continue;
            };
            let form = m.form;
            let bytes = encode(&Message::ChunkData(m));
            let s = self.sessions.get_mut(&id).expect("session");
            s.chunk_credit -= bytes.len() as f64;
            s.streamed.insert(c);
            Self::count_sent(&mut s.stream_stats, form);
            self.outbox.push(Outgoing {
                session: id,
                kind: OutKind::Reliable(Channel::World),
                bytes,
            });
            *sent += 1;
        }
        self.sessions.get_mut(&id).expect("session").stream_complete = complete;
    }

    fn stream_requested(&mut self, id: SessionId, center: ChunkCoord, sent: &mut i64) {
        // In the order asked (the client asks nearest first, §6.6), within what the budget leaves.
        for _ in 0..self.sessions[&id].chunk_requests.len() {
            let s = self.sessions.get_mut(&id).expect("session");
            if s.chunk_credit <= 0.0 || *sent >= wc::MAX_CHUNKS_PER_TICK {
                break;
            }
            let c = s.chunk_requests.pop_front().expect("queued");
            if s.streamed.contains(&c) {
                continue; // the view reached it first
            }
            // Full mode would generate on the server for rendering alone: only what is stored.
            let message = within_radius(c, center, self.config.render_radius_chunks)
                .then(|| self.chunk_message(id, c, false))
                .flatten();
            let s = self.sessions.get_mut(&id).expect("session");
            let Some(m) = message else {
                s.requested.remove(&c);
                s.stream_stats.requests_dropped += 1;
                continue;
            };
            let form = m.form;
            let bytes = encode(&Message::ChunkData(m));
            s.chunk_credit -= bytes.len() as f64;
            s.streamed.insert(c);
            s.stream_stats.requested_sent += 1;
            Self::count_sent(&mut s.stream_stats, form);
            self.outbox.push(Outgoing {
                session: id,
                kind: OutKind::Reliable(Channel::World),
                bytes,
            });
            *sent += 1;
        }
    }

    /// The message streaming chunk `c` to a client: Air, Generated or Explicit. `None` while a
    /// full-mode client's chunk is not generated yet (unless `generate`).
    fn chunk_message(&mut self, id: SessionId, c: ChunkCoord, generate: bool) -> Option<ChunkData> {
        if self.world.find(c).is_none() && self.is_saved(c) {
            self.world.get_or_create(c); // read from the world file
        }
        let modified = self.world.find(c).is_some_and(|ch| ch.revision() > 0);
        let mode = self.sessions[&id].chunk_mode;
        let mut m = ChunkData {
            form: ChunkForm::Explicit,
            coord: c.to_array(),
            revision: 0,
            voxels: Vec::new(),
        };
        if !modified && self.is_air(c) {
            m.form = ChunkForm::Air; // nothing to generate or store, in either mode
        } else if mode == ChunkMode::Generated && !modified {
            m.form = ChunkForm::Generated;
        } else {
            if self.world.find(c).is_none() && !generate {
                return None;
            }
            let chunk = self.world.get_or_create(c);
            m.revision = chunk.revision();
            m.voxels = chunk.voxels().to_vec();
        }
        Some(m)
    }

    fn resync(&mut self, id: SessionId, coords: &[ChunkCoordNet]) {
        // Only chunks the client has: the rest arrive by streaming anyway.
        for &at in coords {
            let c = ChunkCoord::from_array(at);
            if !self.sessions[&id].streamed.contains(&c) {
                continue;
            }
            if let Some(m) = self.chunk_message(id, c, true) {
                self.send_reliable(id, &Message::ChunkData(m), Channel::World);
                self.sessions.get_mut(&id).expect("session").stats.resyncs += 1;
            }
        }
    }

    /// Friend-world hosting (§10.2): the integrated server's player limit and edit and flight
    /// policies change when its player starts hosting, and `host` (their device key) becomes an
    /// op. Not saved: they apply to this run, to players who join afterwards.
    pub fn set_hosting(
        &mut self,
        max_players: u16,
        edits: EditPolicy,
        flight: EditPolicy,
        host: PublicKey,
    ) {
        self.config.max_players = max_players.max(1);
        self.config.edits = edits;
        self.config.flight = flight;
        if !self.config.ops.contains(&host) {
            self.config.ops.push(host);
        }
    }

    /// Tells every joined client that a friend-world host's page was hidden (paused) or shown.
    pub fn broadcast_host_status(&mut self, state: HostState) {
        let bytes = encode(&Message::HostStatus(state));
        for (&id, s) in &self.sessions {
            if s.joined() {
                self.outbox.push(Outgoing {
                    session: id,
                    kind: OutKind::Reliable(Channel::Control),
                    bytes: bytes.clone(),
                });
            }
        }
    }

    /// Ends sessions with Reject(ServerClosing, message): every session, or all but the loopback
    /// one (a host that stops hosting keeps playing).
    pub fn close_sessions(&mut self, message: &str, keep_loopback: bool) {
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| !(keep_loopback && s.kind == TransportKind::Loopback))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.reject(id, RejectReason::ServerClosing, message);
        }
    }

    fn may_edit(&self, s: &Session) -> bool {
        self.allowed(self.config.edits, s)
    }
    fn may_fly(&self, s: &Session) -> bool {
        self.allowed(self.config.flight, s)
    }
    fn allowed(&self, policy: EditPolicy, s: &Session) -> bool {
        match policy {
            EditPolicy::Everyone => true,
            EditPolicy::Ops => self.config.ops.contains(&s.public_key),
            EditPolicy::Nobody => false,
        }
    }

    fn eye_of(&self, h: PlayerHandle) -> [f64; 3] {
        let p = self.players.position(&self.physics, h);
        let body = self.config.player_config.body;
        let c = self.players.controller(h);
        let eye = if c.crouching {
            body.crouch_eye_height
        } else {
            body.eye_height
        };
        let face = f64::from(c.face);
        [
            p.x,
            p.y - face * f64::from(self.players.half_height(h)) + face * f64::from(eye),
            p.z,
        ]
    }

    fn apply_edits(&mut self) {
        let capsules: Vec<EditCapsule> = self
            .sessions
            .values()
            .filter_map(|s| s.handle)
            .map(|h| {
                let c = self.players.position(&self.physics, h);
                let (half, radius) = (
                    self.players.half_height(h),
                    self.config.player_config.body.radius,
                );
                EditCapsule {
                    center: c.to_array(),
                    radius,
                    half_cylinder: half - radius,
                }
            })
            .collect();
        // Changes of this tick per chunk, in the order chunks were first touched.
        let mut changes = TickChanges::default();
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        for id in ids {
            let s = &self.sessions[&id];
            if !s.joined() {
                continue;
            }
            let (may_edit, handle) = (self.may_edit(s), s.handle);
            let eye = handle.map(|h| self.eye_of(h));
            let s = self.sessions.get_mut(&id).expect("session");
            s.edit_credit = (s.edit_credit + EDITS_PER_TICK).min(EDIT_BURST);
            for edit in std::mem::take(&mut s.edits) {
                let Some(eye) = eye.filter(|_| may_edit && s.edit_credit >= 1.0) else {
                    s.stats.edits_rejected += 1;
                    continue;
                };
                let outcome =
                    check_block_edit(&mut self.world, &edit, eye, &capsules, DEFAULT_REACH);
                s.stats.last_edit_check = outcome.check;
                if outcome.check != EditCheck::Ok {
                    s.stats.edits_rejected += 1;
                    continue;
                }
                s.edit_credit -= 1.0;
                s.stats.edits_applied += 1;
                changes.set(&mut self.world, outcome.cell, outcome.material);
            }
            // Region edits (§6.5), after the block edits, in the order they arrived; each costs
            // its box's volume in cells, valid or not (validation reads the whole box).
            s.region_credit = (s.region_credit + REGION_CELLS_PER_TICK).min(REGION_BURST_CELLS);
            for edit in std::mem::take(&mut s.region_edits) {
                let cost = edit.volume() as f64;
                let Some(eye) = eye.filter(|_| may_edit && s.region_credit >= cost) else {
                    s.stats.region_edits_rejected += 1;
                    continue;
                };
                s.region_credit -= cost;
                let outcome =
                    check_region_edit(&mut self.world, &edit, eye, &capsules, DEFAULT_REGION_REACH);
                s.stats.last_region_check = outcome.check;
                if outcome.check != RegionCheck::Ok {
                    s.stats.region_edits_rejected += 1;
                    continue;
                }
                s.stats.region_edits_applied += 1;
                s.stats.region_cells_changed += outcome.changes.len() as u64;
                for (cell, m) in outcome.changes {
                    changes.set(&mut self.world, cell, m);
                }
            }
        }
        let TickChanges {
            chunks: mut changed,
            ..
        } = changes;
        if changed.is_empty() {
            return;
        }
        for c in &changed {
            let coord = ChunkCoord::from_array(c.coord);
            self.dirty.insert(coord);
            self.lod.mark_chunk(coord);
        }
        // One revision per chunk per modification, then each client gets the chunks it streams.
        for c in &mut changed {
            let chunk = self.world.get_or_create(ChunkCoord::from_array(c.coord));
            chunk.bump_revision();
            c.revision = chunk.revision();
        }
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let s = &self.sessions[&id];
            let chunks: Vec<ChunkChanges> = changed
                .iter()
                .filter(|c| s.streamed.contains(&ChunkCoord::from_array(c.coord)))
                .cloned()
                .collect();
            if !chunks.is_empty() {
                let m = VoxelModification {
                    reason: VoxelModificationReason::Edit,
                    server_tick: self.tick,
                    chunks,
                };
                self.send_reliable(id, &Message::VoxelModification(m), Channel::World);
            }
        }
    }

    fn player_id_of_body(&self, g: GroundRef) -> u16 {
        let GroundRef::Player(body) = g else { return 0 };
        self.sessions
            .values()
            .find(|s| s.handle.is_some_and(|h| self.players.body(h) == body))
            .map_or(0, |s| s.player_id)
    }

    fn send_snapshots(&mut self) {
        // Remote view of every joined player (dead ones where they died).
        let mut views: Vec<RemotePlayerState> = Vec::new();
        for s in self.sessions.values().filter(|s| s.joined()) {
            let mut r = RemotePlayerState {
                player_id: s.player_id,
                position: s.death_position,
                velocity: [0.0; 3],
                yaw: 0,
                pitch: 0,
                state: PlayerState::Idle,
                flags: player_flags::DEAD,
            };
            if let Some(h) = s.handle {
                let c = self.players.controller(h);
                let p = self.players.position(&self.physics, h);
                r.position = [p.x, self.feet_y(h), p.z];
                r.velocity = self.players.velocity(&self.physics, h).to_array();
                r.yaw = quantize_yaw(c.input.look_yaw);
                r.pitch = quantize_pitch(c.input.look_pitch);
                r.state = PlayerState::from_u8(c.state as u8).unwrap_or(PlayerState::Idle);
                r.flags = player_flags_of(c, false);
            }
            views.push(r);
        }
        let mut out = Vec::new();
        for (&id, s) in self.sessions.iter().filter(|(_, s)| s.joined()) {
            let mut l = LocalPlayerState {
                health: s.health.clamp(0, 255) as u8,
                input_buffer: s.inputs.len().min(255) as u8,
                last_knockback_seq: s.last_knockback_seq,
                position: s.death_position,
                flags: player_flags::DEAD,
                ..LocalPlayerState::default()
            };
            if let Some(h) = s.handle {
                let c = self.players.controller(h);
                // Capsule centre (resumes the body exactly).
                l.position = self.players.position(&self.physics, h).to_array();
                l.velocity = self.players.velocity(&self.physics, h).to_array();
                l.flags = player_flags_of(c, false);
                l.state = PlayerState::from_u8(c.state as u8).unwrap_or(PlayerState::Idle);
                l.controller = to_net(c, |g| self.player_id_of_body(g));
            }
            let mut snap = PhysicsSnapshot {
                server_tick: self.tick,
                ack_input_seq: s.last_processed_seq,
                local: l,
                remotes: Vec::new(),
            };
            // Remote players, nearest first, as many as fit in one datagram.
            let distance = |r: &RemotePlayerState| {
                (0..3)
                    .map(|i| (r.position[i] - l.position[i]).powi(2))
                    .sum::<f64>()
            };
            let mut remotes: Vec<RemotePlayerState> = views
                .iter()
                .filter(|r| r.player_id != s.player_id)
                .copied()
                .collect();
            remotes.sort_by(|a, b| {
                distance(a)
                    .total_cmp(&distance(b))
                    .then(a.player_id.cmp(&b.player_id))
            });
            let base = encode(&Message::PhysicsSnapshot(snap.clone())).len();
            if let Some(&first) = remotes.first() {
                snap.remotes.push(first);
                let each = encode(&Message::PhysicsSnapshot(snap.clone())).len() - base;
                remotes.truncate((MAX_DATAGRAM_BYTES - base) / each);
            }
            snap.remotes = remotes;
            out.push((id, Message::PhysicsSnapshot(snap)));
        }
        for (id, m) in out {
            self.send_datagram(id, &m);
        }
    }

    fn player_positions(&self) -> Vec<[f64; 3]> {
        self.sessions
            .values()
            .filter(|s| s.joined())
            .map(|s| match s.handle {
                Some(h) => self.players.position(&self.physics, h).to_array(),
                None => s.death_position,
            })
            .collect()
    }

    /// Runs LOD propagation until nothing is dirty (tests, tools).
    pub fn drain_lod(&mut self) {
        let centers = self.player_positions();
        let (world, saved) = (&mut self.world, &self.saved);
        self.lod
            .drain(&centers, &mut |c| Self::modified_chunk(world, saved, c));
        self.update_lod();
    }

    fn modified_chunk(
        world: &mut VoxelWorld,
        saved: &Mutex<HashMap<ChunkCoord, u32>>,
        c: ChunkCoord,
    ) -> Option<Vec<u16>> {
        if world.find(c).is_none() && saved.lock().expect("saved index").contains_key(&c) {
            world.get_or_create(c); // read from the world file
        }
        world
            .find(c)
            .filter(|ch| ch.revision() > 0)
            .map(|ch| ch.voxels().to_vec())
    }

    fn update_lod(&mut self) {
        let centers = self.player_positions();
        {
            let (world, saved) = (&mut self.world, &self.saved);
            self.lod.step(
                &centers,
                &mut |c| Self::modified_chunk(world, saved, c),
                lod::PROPAGATION_SECTIONS_PER_TICK as usize,
                self.config.lod_budget,
            );
        }
        for c in self.lod.take_written() {
            self.lod_unsaved.insert(c);
            if c.level == INDEX_LEVEL {
                let revision = self.lod.find(c).expect("written").revision;
                self.index_updates.insert(
                    (c.i, c.j, c.k),
                    LodIndexEntry {
                        i: c.i,
                        j: c.j,
                        k: c.k,
                        revision,
                    },
                );
            }
        }
        // Index changes, coalesced to at most one LodIndexUpdate per LOD_INDEX_UPDATE_MS.
        if !self.index_updates.is_empty() && self.tick >= self.next_index_update_tick {
            self.next_index_update_tick = self.tick + INDEX_UPDATE_EVERY_TICKS;
            let entries: Vec<LodIndexEntry> = std::mem::take(&mut self.index_updates)
                .into_values()
                .collect();
            for part in entries.chunks(limits::MAX_LOD_INDEX_ENTRIES) {
                let bytes = encode(&Message::LodIndexUpdate(part.to_vec()));
                for (&id, s) in self.sessions.iter_mut().filter(|(_, s)| s.lod_index_sent) {
                    s.lod_stats.index_updates += 1;
                    s.lod_stats.index_entries += part.len() as u32;
                    s.lod_stats.bytes += bytes.len() as u64;
                    self.outbox.push(Outgoing {
                        session: id,
                        kind: OutKind::Reliable(Channel::Lod),
                        bytes: bytes.clone(),
                    });
                }
            }
        }
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let s = self.sessions.get_mut(&id).expect("session");
            s.lod_request_credit =
                (s.lod_request_credit + LOD_REQUESTS_PER_TICK).min(lod::REQUESTS_PER_SECOND as f64);
            self.serve_lod(id);
        }
    }

    fn send_lod_index(&mut self, id: SessionId) {
        let mut entries: Vec<LodIndexEntry> = self
            .lod
            .sections()
            .iter()
            .filter(|(c, _)| c.level == INDEX_LEVEL)
            .map(|(c, s)| LodIndexEntry {
                i: c.i,
                j: c.j,
                k: c.k,
                revision: s.revision,
            })
            .collect();
        entries.sort_by_key(|e| (e.i, e.j, e.k));
        let parts: Vec<&[LodIndexEntry]> = if entries.is_empty() {
            vec![&[]]
        } else {
            entries.chunks(limits::MAX_LOD_INDEX_ENTRIES).collect()
        };
        let n = parts.len();
        for (i, part) in parts.into_iter().enumerate() {
            let bytes = encode(&Message::LodIndex(LodIndex {
                last: i + 1 == n,
                entries: part.to_vec(),
            }));
            let s = self.sessions.get_mut(&id).expect("session");
            s.lod_stats.index_entries += part.len() as u32;
            s.lod_stats.bytes += bytes.len() as u64;
            self.outbox.push(Outgoing {
                session: id,
                kind: OutKind::Reliable(Channel::Lod),
                bytes,
            });
        }
        self.sessions.get_mut(&id).expect("session").lod_index_sent = true;
    }

    fn serve_lod(&mut self, id: SessionId) {
        let per_tick = f64::from(self.config.lod_bytes_per_second) / f64::from(SIM_HZ);
        let s = self.sessions.get_mut(&id).expect("session");
        s.lod_credit = (s.lod_credit + per_tick).min(per_tick);
        loop {
            let s = &self.sessions[&id];
            let Some(&r) = s.lod_requests.front().filter(|_| s.lod_credit > 0.0) else {
                break;
            };
            let full = s.chunk_mode == ChunkMode::Full;
            let c = LodCoord::new(i32::from(r.level), r.section[0], r.section[1], r.section[2]);
            let mut m = LodData {
                form: LodForm::Generated,
                level: r.level,
                section: r.section,
                revision: 0,
                cells: Vec::new(),
            };
            if !in_world(c) {
                // Nothing there: the client's generator says so too.
            } else if let Some(section) = self.lod.find(c) {
                m.revision = section.revision;
                if r.known_revision == section.revision {
                    m.form = LodForm::Unchanged;
                } else {
                    m.form = LodForm::Explicit;
                    m.cells = self.lod.cells_for_client(c);
                }
            } else if full {
                // Unmodified (or not written yet), and the client cannot generate: send it generated.
                let Some(encoded) = self.lod.generated(c) else {
                    break;
                }; // answered later, in order
                m.form = LodForm::Explicit;
                m.cells = dwell_protocol::decode_lod_cells(encoded).unwrap_or_default();
            }
            let form = m.form;
            let bytes = encode(&Message::LodData(m));
            let s = self.sessions.get_mut(&id).expect("session");
            s.lod_credit -= bytes.len() as f64;
            s.lod_stats.bytes += bytes.len() as u64;
            match form {
                LodForm::Generated => s.lod_stats.generated_sent += 1,
                LodForm::Explicit => s.lod_stats.explicit_sent += 1,
                _ => s.lod_stats.unchanged_sent += 1,
            }
            s.lod_requests.pop_front();
            self.outbox.push(Outgoing {
                session: id,
                kind: OutKind::Reliable(Channel::Lod),
                bytes,
            });
        }
    }

    fn is_saved(&self, c: ChunkCoord) -> bool {
        self.saved.lock().expect("saved index").contains_key(&c)
    }
    fn is_air(&self, c: ChunkCoord) -> bool {
        self.air_test
            .as_ref()
            .is_some_and(|air| !self.is_saved(c) && air(c))
    }

    fn init_storage(&mut self) {
        let Some(store) = &self.config.store else {
            return;
        };
        let db = store.db().clone();
        let meta = db.load_meta();
        if let Some(m) = &meta {
            self.tick = m.world_tick;
        }
        let saved: HashMap<ChunkCoord, u32> = db.chunk_index().into_iter().collect();
        // The LOD cache (§6.6): restored as saved, rebuilt from the chunks when it belongs to
        // another generator version, and derived for saved chunks it does not cover.
        let (rows, stale) = db.lod_sections(self.config.generator_version);
        for row in rows {
            if !row.encoded.is_empty() {
                self.lod.restore(
                    row.coord,
                    LodSection {
                        revision: row.revision,
                        encoded: row.encoded,
                    },
                );
            }
            if row.dirty || self.lod.find(row.coord).is_none() {
                self.lod.mark_dirty(row.coord);
            }
        }
        self.lod_rebuild = stale;
        for &c in saved.keys() {
            if !self.lod.modified(parent(lod_of_chunk(c))) {
                self.lod.mark_chunk(c);
            }
        }
        *self.saved.lock().expect("saved index") = saved;
        self.world.set_saved(Box::new(StoreChunks {
            db,
            saved: self.saved.clone(),
        }));
        self.next_save_tick = self.tick + self.config.autosave_seconds * SIM_HZ;
        if meta.is_none() {
            self.save_now(); // record the new world's seed, generator and spawn right away
        }
    }

    fn record_of(&self, s: &Session) -> PlayerRecord {
        let (feet, health) = match s.handle {
            Some(h) => {
                let p = self.players.position(&self.physics, h);
                ([p.x, self.feet_y(h), p.z], s.health)
            }
            None => (s.death_position, 0),
        };
        PlayerRecord {
            key: s.public_key,
            display_name: s.display_name.clone(),
            feet,
            health,
            first_seen: 0,
            last_seen: 0,
        }
    }

    /// Saves dirty chunks, players and meta now (committed off the tick natively). No-op without
    /// a store. Hosts call it before shutting down, then `flush_store`.
    pub fn save_now(&mut self) {
        if self.config.store.is_none() {
            return;
        }
        self.next_save_tick = self.tick + self.config.autosave_seconds * SIM_HZ;
        let mut batch = SaveBatch {
            // This build's version is the world's last (and, for a new file, its first): RELEASES.md §6.
            meta: Some(WorldMeta {
                world_seed: self.config.world_seed,
                generator_version: self.config.generator_version,
                spawn: self.config.spawn,
                world_tick: self.tick,
                created_at: 0,
                saved_at: 0,
                app_version_created: APP_VERSION.into(),
                app_version_last: APP_VERSION.into(),
            }),
            ..SaveBatch::default()
        };
        let mut revisions = Vec::new();
        let mut dirty: Vec<ChunkCoord> = self.dirty.drain().collect();
        dirty.sort_by_key(|c| (c.x, c.y, c.z));
        for c in dirty {
            let Some(chunk) = self.world.find(c) else {
                continue;
            };
            batch.chunks.push(dwell_storage::SavedChunk {
                coord: c,
                revision: chunk.revision(),
                voxels: chunk.voxels().to_vec(),
            });
            revisions.push((c, chunk.revision()));
        }
        batch.players = std::mem::take(&mut self.departed);
        // LOD sections written since the last save; sections still to compute (dirty, or computing
        // now) are saved dirty, to recompute on load.
        let mut lod_saved = Vec::new();
        for &c in &self.lod_unsaved {
            let Some(section) = self.lod.find(c) else {
                continue;
            };
            let dirty = self.lod.dirty().contains(&c);
            batch.lod_sections.push(SavedLodSection {
                coord: c,
                revision: section.revision,
                dirty,
                encoded: section.encoded.clone(),
            });
            lod_saved.push(c);
        }
        for &c in self.lod.dirty().iter().chain(self.lod.in_flight()) {
            let section = self.lod.find(c);
            if self.lod_unsaved.contains(&c) && section.is_some() {
                continue; // saved above, flagged dirty
            }
            batch.lod_sections.push(SavedLodSection {
                coord: c,
                revision: section.map_or(0, |s| s.revision),
                dirty: true,
                encoded: section.map(|s| s.encoded.clone()).unwrap_or_default(),
            });
        }
        self.lod_unsaved.clear();
        batch.clear_lod = std::mem::take(&mut self.lod_rebuild);
        for s in self.sessions.values().filter(|s| s.joined()) {
            batch.players.push(self.record_of(s));
        }
        let save_id = self.config.store.as_mut().expect("store").save(batch);
        self.in_flight.insert(save_id, revisions);
        self.lod_in_flight.insert(save_id, lod_saved);
        self.collect_saves(); // saves commit inline without an I/O thread
    }

    fn collect_saves(&mut self) {
        let Some(store) = &self.config.store else {
            return;
        };
        for result in store.take_completed() {
            let Some(revisions) = self.in_flight.remove(&result.id) else {
                continue;
            };
            if let Some(lod) = self.lod_in_flight.remove(&result.id)
                && !result.ok
            {
                self.lod_unsaved.extend(lod);
            }
            if result.ok {
                self.save_stats.saves += 1;
                let mut saved = self.saved.lock().expect("saved index");
                for (c, revision) in revisions {
                    let r = saved.entry(c).or_insert(0);
                    *r = (*r).max(revision);
                }
            } else {
                self.save_stats.failed += 1;
                self.save_stats.last_error = result.error;
                self.dirty.extend(revisions.into_iter().map(|(c, _)| c)); // try again
            }
        }
    }

    /// Waits for saves in flight to commit (hosts, before shutting down).
    pub fn flush_store(&mut self) {
        if let Some(store) = &self.config.store {
            store.flush();
        }
        self.collect_saves();
    }

    pub fn save_stats(&self) -> SaveStats {
        SaveStats {
            dirty: self.dirty.len(),
            saved_chunks: self.saved.lock().expect("saved index").len(),
            loaded: self.world.loaded_saved(),
            ..self.save_stats.clone()
        }
    }

    /// Messages produced since the last call, in order.
    pub fn take_outbox(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.outbox)
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }
    pub fn time_ms(&self) -> f64 {
        f64::from(self.tick) * (1000.0 / f64::from(SIM_HZ))
    }
    pub fn joined_players(&self) -> usize {
        self.sessions.values().filter(|s| s.joined()).count()
    }
    pub fn world(&mut self) -> &mut VoxelWorld {
        &mut self.world
    }
    pub fn physics(&mut self) -> &mut Physics {
        &mut self.physics
    }
    pub fn players(&self) -> &Players {
        &self.players
    }
    /// The world, physics and players together (tests that move bodies or edit terrain directly).
    pub fn sim(&mut self) -> (&mut VoxelWorld, &mut Physics, &mut Players) {
        (&mut self.world, &mut self.physics, &mut self.players)
    }
    pub fn player_config(&self) -> &PlayerControllerConfig {
        &self.config.player_config
    }
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }
    pub fn lod(&self) -> &LodPropagation {
        &self.lod
    }
    pub fn verification_chunk(&self) -> ChunkCoord {
        self.verification_chunk
    }

    fn session_of_player(&self, player_id: u16) -> Option<SessionId> {
        self.sessions
            .iter()
            .find(|(_, s)| s.joined() && s.player_id == player_id)
            .map(|(id, _)| *id)
    }
    fn by_player(&self, player_id: u16) -> Option<&Session> {
        self.session_of_player(player_id)
            .map(|id| &self.sessions[&id])
    }
    // Test and diagnostics access by player id.
    pub fn player_handle_of(&self, player_id: u16) -> Option<PlayerHandle> {
        self.by_player(player_id).and_then(|s| s.handle)
    }
    pub fn stats_of(&self, player_id: u16) -> Option<SessionStats> {
        self.by_player(player_id).map(|s| s.stats)
    }
    pub fn stream_stats_of(&self, player_id: u16) -> Option<StreamStats> {
        self.by_player(player_id).map(|s| StreamStats {
            streamed: s.streamed.len(),
            ..s.stream_stats
        })
    }
    pub fn lod_stats_of(&self, player_id: u16) -> Option<LodStats> {
        self.by_player(player_id).map(|s| s.lod_stats)
    }
    /// −1 when unknown.
    pub fn health_of(&self, player_id: u16) -> i32 {
        self.by_player(player_id).map_or(-1, |s| s.health)
    }
    /// A joined player's feet (world metres), for tests and tools.
    pub fn feet_of(&self, player_id: u16) -> Option<[f64; 3]> {
        let h = self.player_handle_of(player_id)?;
        let p = self.players.position(&self.physics, h);
        Some([p.x, self.feet_y(h), p.z])
    }

    fn send_reliable(&mut self, id: SessionId, m: &Message, channel: Channel) {
        self.outbox.push(Outgoing {
            session: id,
            kind: OutKind::Reliable(channel),
            bytes: encode(m),
        });
    }
    fn send_datagram(&mut self, id: SessionId, m: &Message) {
        self.outbox.push(Outgoing {
            session: id,
            kind: OutKind::Datagram,
            bytes: encode(m),
        });
    }
    fn broadcast_world(&mut self, m: &Message) {
        let bytes = encode(m);
        for (&id, s) in &self.sessions {
            if s.joined() {
                self.outbox.push(Outgoing {
                    session: id,
                    kind: OutKind::Reliable(Channel::World),
                    bytes: bytes.clone(),
                });
            }
        }
    }
    fn reject(&mut self, id: SessionId, reason: RejectReason, message: &str) {
        self.send_reliable(
            id,
            &Message::Reject(Reject {
                reason,
                message: message.into(),
            }),
            Channel::Control,
        );
        self.outbox.push(Outgoing {
            session: id,
            kind: OutKind::Close,
            bytes: Vec::new(),
        });
        self.remove_session(id);
    }

    fn allocate_player_id(&mut self) -> u16 {
        // Player ids are unique among joined players; skip 0 and ids in use.
        loop {
            let candidate = self.next_player_id;
            self.next_player_id = self.next_player_id.wrapping_add(1);
            if candidate != 0
                && !self
                    .sessions
                    .values()
                    .any(|s| s.joined() && s.player_id == candidate)
            {
                return candidate;
            }
        }
    }
}
