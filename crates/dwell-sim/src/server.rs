//! The authoritative server core (ARCHITECTURE.md §4). It has no sockets, threads it doesn't own,
//! or OS calls: the host feeds it transport events, calls `step` at SIM_HZ, and drains the outbox
//! (§4.3). The same type runs in the native server, in the browser's local mode, and in
//! friend-world hosts. Sessions are kept in id order, so every pass over them is deterministic.

use dwell_world::hash::{HashMap, HashSet};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dwell_blocks::info;
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::controller::{Input, PlayerHandle, Players, events};
use dwell_physics::net::{dequantize_input, player_flags_of, quantize_pitch, quantize_yaw, to_net};
use dwell_physics::query::GroundRef;
use dwell_physics::{DVec3, Physics, Vec3};
use dwell_protocol::inventory as inv;
use dwell_protocol::modes::Ability;
use dwell_protocol::world_settings::{SettingsFields, WorldClock, WorldSettings};
use dwell_protocol::*;
use dwell_protocol::{
    Channel, ChunkForm, DamageCause, HostState, LodForm, MAX_DATAGRAM_BYTES, Message,
    PROTOCOL_VERSION, PlayerEventKind, PlayerState, RejectReason, SIM_HZ, SNAPSHOT_HZ,
    TransportKind, VoxelModificationReason, auth_transcript, limits, lod, player_flags,
    players as pc, welcome_flags, world as wc,
};
use dwell_storage::{PlayerRecord, SaveBatch, SavedLodSection, WorldMeta, WorldStore};
use dwell_world::edit::{
    DEFAULT_REACH, EditCapsule, EditCheck, check_block_edit, edit_writes, held_in_place,
    medium_kept, overlaps_player,
};
use dwell_world::lod::{INDEX_LEVEL, LodCoord, lod_of_chunk, parent};
use dwell_world::region::{RegionCheck, check_region_edit, region_reach};
use dwell_world::{
    AirChunkTest, CHUNK_SIZE, Chunk, ChunkCoord, ChunkGenerator, FaceFrame, M, SavedChunks,
    VoxelWorld, chunk_distance, chunk_hash, chunk_of, local_index,
};

use dwell_protocol::auth::verify_ed25519;

mod chopping;
mod creatures;
mod ecology;
pub use ecology::{
    CATCH_UP_DAYS, ECOLOGY_SEND_EVERY, EcologySnapshot, GroupView, IDLE_DAYS, MAX_STEPS_PER_TICK,
    next_step, night_on,
};
pub use grass::{Grass, GrassEnvelope};
mod grass;
mod ground;
mod growth;
mod items;
mod lava;
mod vitals;

use crate::app_version::APP_VERSION;
use crate::entropy::Entropy;
use crate::lod_propagation::{LodPropagation, LodSection};
use crate::worldgen_pool::WorldgenPool;
use dwell_world::explosives::explosive_of;

pub type SessionId = u32;
pub type TransportBinding = [u8; 32];

const SNAPSHOT_EVERY: u32 = SIM_HZ / SNAPSHOT_HZ; // ticks
/// Hard cap per session: above a slow client's frame of inputs (the client's MAX_TICKS_PER_FRAME,
/// 60), which arrives at once.
const MAX_QUEUED_INPUTS: usize = 96;
const INPUT_BUFFER: usize = 2; // jitter buffer depth (ticks)
/// Inputs queued beyond this for a whole window are skipped, to bound input latency. A window's
/// low-water mark decides, not the queue at one tick: a client drawing a few frames a second sends
/// a frame's ticks at once (up to the client's MAX_TICKS_PER_FRAME), and skipping that burst would
/// drop inputs the client predicted (a correction every frame). The window is a second, as long as
/// the slowest frame the client catches up, so it always holds the trough before a burst. It is a
/// backstop only (docs/BUGS.md 1): a skipped input is movement the client predicted and the server
/// never simulates, so every later snapshot snaps the player back. The client itself holds its
/// prediction once we report half a second queued (its `BUFFER_HOLD`, 30) and nudges its tick
/// rate for smaller drift, so only a client that ignores both reaches this.
const MAX_BUFFERED_INPUTS: usize = 60;
const LATENCY_WINDOW_TICKS: u32 = 60;
const MAX_SEQ_AHEAD: u32 = 256; // inputs further ahead are rejected
const INPUT_DATAGRAMS_PER_SECOND: u32 = 2 * SIM_HZ; // §11: SIM_HZ + redundancy
const PREFETCH_CHUNKS: i32 = 2; // generated ahead around each player (collision reach + 1)
const KEEP_CHUNKS: i32 = 3; // unmodified chunks kept around players; farther: evicted
const EVICT_EVERY_TICKS: u32 = 64;
const MAX_EXPLICIT_WANTED: usize = 64; // full-mode chunks queued for generation per tick
const MAX_QUEUED_EDITS: usize = 8; // per session per tick; more are dropped
/// Water currents to the players (docs/WATER_SURFACE_GRID.md §2c): looked at every
/// `CURRENTS_EVERY` ticks, a water chunk within `currents_radius()` chunks of a player sent when
/// its currents changed or `CURRENTS_REFRESH` ticks after it was last sent (a lost datagram made
/// good), the nearest `CURRENTS_PER_SEND` at most; a chunk gone still is sent once as all zero.
const CURRENTS_EVERY: u32 = 15;
/// A water chunk's currents as sent (`WaterCurrents::cells`).
type SentCurrents = Box<[[i8; 2]; 256]>;
const CURRENTS_REFRESH: u32 = 180;
const CURRENTS_PER_SEND: usize = 8;

/// The water chunks round a player's whose currents it is sent: as far as its flow is drawn
/// (`waterflow.json` `fadeToM`) from anywhere in its own chunk, so the river it sees runs all the
/// way out (BUGS.md #42: at 2 chunks the flow stopped 33–48 m out, inside the 40 m it is drawn
/// fully).
pub fn currents_radius() -> i32 {
    let cs = f64::from(dwell_world::currents::CS);
    (dwell_look::waterflow::WATER_FLOW.fade_to_m / cs).ceil() as i32
}
/// Ticks after a session joins before its currents are sent: its terrain first.
const CURRENTS_AFTER_JOIN: u32 = 300;
/// Ticks a place into a player's body waits before it is refused (docs/BUGS.md 30): the server's
/// view of a body lags the client's by the inputs still queued, a whole frame of them on a slow
/// device (a 2 fps frame's 30 ticks), so a place the client made once its body was clear goes in
/// when the server's body has moved clear too.
const EDIT_HOLD_TICKS: u8 = 30;
/// Block edit rate (§6.5, §11): a token bucket refilled once per BLOCK_EDIT_INTERVAL_MS, holding a
/// small burst so edits bunched by the network are not rejected.
const EDITS_PER_TICK: f64 = 1000.0 / (pc::BLOCK_EDIT_INTERVAL_MS as f64 * SIM_HZ as f64);
const EDIT_BURST: f64 = 3.0;
/// Region edits (§6.5): a token bucket of cells, each request costing its box's volume.
const REGION_CELLS_PER_TICK: f64 = region_edit::CELLS_PER_SECOND as f64 / SIM_HZ as f64;
const REGION_BURST_CELLS: f64 = region_edit::BURST_CELLS as f64;
const MAX_SAVED_LOADS_PER_TICK: i32 = 16; // saved chunks read from the world file per tick
/// Inline, the sky floors (`sky_columns.rs`) computed per tick: at least one, then until this is
/// spent (one is ~1 ms natively, a few in the browser; on wasm32 the budget is one).
const SKY_FLOOR_BUDGET: Duration = Duration::from_micros(1000);
/// The chunk requests looked at past the first waiting for its sky floor, to want theirs too.
const REQUEST_LOOKAHEAD: usize = 32;
const INDEX_UPDATE_EVERY_TICKS: u32 = (lod::INDEX_UPDATE_MS as u32) * SIM_HZ / 1000;
const MAX_QUEUED_LOD_REQUESTS: usize = 256; // per session; more are dropped
const LOD_REQUESTS_PER_TICK: f64 = lod::REQUESTS_PER_SECOND as f64 / SIM_HZ as f64;
const MAX_QUEUED_CHUNK_REQUESTS: usize = 1024;
const CHUNK_REQUESTS_PER_TICK: f64 = wc::CHUNK_REQUESTS_PER_SECOND as f64 / SIM_HZ as f64;

/// A falling piece's bytes in a snapshot: id, posfix × 3, rotation i16 × 4, velocity and spin f16 × 3.
const PIECE_STATE_BYTES: usize = 4 + 12 + 8 + 6 + 6;

/// One tick's voxel changes per chunk (the next VoxelModification), in the order chunks and cells
/// were first touched; a cell changed twice keeps its last state.
#[derive(Default)]
struct TickChanges {
    chunks: Vec<ChunkChanges>,
    /// Chunk → (its index in `chunks`, local index → its index in that chunk's changes).
    index: HashMap<ChunkCoord, (usize, HashMap<u16, usize>)>,
    /// Each changed cell's state before its first change (for water flow, docs/WATER_FLOW.md).
    before: Vec<([i32; 3], u16)>,
    /// Water's own writes: not told to water again.
    water: bool,
    /// Lava's own writes: not told to the lava fields again (`Lava::touch`).
    lava: bool,
}

impl TickChanges {
    /// Sets the cell and records it, with the other cells of a multi-cell block (a torch's upper
    /// part, placed and cleared with it) and what that leaves unsupported (block data
    /// `needsSupport`: tall grass on a broken block goes with it), in the same modification
    /// (`dwell_world::edit::edit_writes`).
    fn set_and_unsupport(&mut self, world: &mut VoxelWorld, cell: [i32; 3], material: u16) {
        // An air plant never stays flooded (block data `medium`).
        let material = medium_kept(material);
        for (at, m) in edit_writes(world, cell, material) {
            self.set(world, at, m);
        }
    }

    /// Every changed cell (world cells), in the order they were first changed.
    fn cells(&self) -> Vec<[i32; 3]> {
        let mut out = Vec::new();
        for c in &self.chunks {
            for ch in &c.changes {
                let i = i32::from(ch.index);
                let n = CHUNK_SIZE;
                out.push([
                    c.coord[0] * n + (i & 31),
                    c.coord[1] * n + ((i >> 5) & 31),
                    c.coord[2] * n + (i >> 10),
                ]);
            }
        }
        out
    }

    /// Sets the cell in the world and records the change.
    fn set(&mut self, world: &mut VoxelWorld, cell: [i32; 3], material: u16) {
        let coord = chunk_of(cell[0], cell[1], cell[2]);
        let local = local_index(
            cell[0] - coord.x * CHUNK_SIZE,
            cell[1] - coord.y * CHUNK_SIZE,
            cell[2] - coord.z * CHUNK_SIZE,
        );
        let old = world.get_voxel_if_loaded(cell);
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
                self.before.push((cell, old));
            }
        }
    }
}

use crate::water::host::{In as WaterIn, Out as WaterOut};

/// The server's side of the water host (ADR 0062, docs/WATER_SURFACE_GRID.md §2d).
#[derive(Default)]
struct WaterLink {
    /// What the water's copy of the world was told of each chunk.
    mirror: crate::water::cells::Mirror,
    /// The next batch, sent at the tick's end.
    pending: Vec<WaterIn>,
    /// The chunks the water last said it wants loaded.
    wanted: Vec<ChunkCoord>,
    /// The newest record it sent, not yet saved.
    record: Option<Vec<u8>>,
    /// A record asked for and not yet come.
    record_asked: bool,
    /// Reports come back (`Server::take_water_report`).
    reports: std::collections::VecDeque<String>,
    /// Writes dropped because the world had overtaken them (a diagnostic).
    overtaken: u64,
}

/// How long before an autosave the server asks the water for its record (ticks).
const WATER_RECORD_LEAD_TICKS: u32 = 30;
/// How long the LOD section of a chunk the water rewrites waits from its first change before it is
/// computed (ticks): the water keeps rewriting it, and the LOD is seen from afar.
const WATER_LOD_HOLD_TICKS: u32 = 2 * SIM_HZ;
/// How long a tick spends at most encoding a save on an inline store (no I/O thread: the
/// browser's, §6.4), ms; its commit is a tick's of its own.
const SAVE_STEP_MS: f64 = 2.0;
/// How long closing a world waits for the water's record.
const WATER_CLOSE_WAIT: Duration = Duration::from_secs(1);

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
    /// The launch's spawn wins over a saved world's, and returning players start at it, not where
    /// they left the world: a local world opened by a spawn link (`?spawn=portal`, `b`; owner,
    /// 2026-10-27). Off, a saved world keeps its own spawn and players continue where they were.
    pub join_at_spawn: bool,
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
    /// The respawn preload (§6.3): from a death until the respawn, the view sphere of this radius
    /// around the respawn point is streamed as a second centre, after the view around the body and
    /// within the same budgets (None: off). Default from `shared/look/loading.json`.
    pub respawn_preload_radius_chunks: Option<i32>,
    /// Level of detail (§6.6): propagation threads (`None`: one when worldgen has threads, else
    /// none — then it runs on the tick within `lod_budget`), and the per-client stream budget.
    pub lod_threads: Option<usize>,
    /// The LOD's jobs and the chunks to generate are done elsewhere (the browser's server-jobs
    /// worker: `take_job`, `deliver_job`), not on the tick; `lod_threads` and `worldgen_threads`
    /// are then ignored.
    pub jobs_remote: bool,
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
    /// A new world's water speed (docs/WATER_FLOW.md §4.1): × game time; `shared/fluids/flow.json`'s
    /// `speed` by default. A saved world keeps its own (its settings, DAY_NIGHT.md §2).
    pub water_speed: f64,
    /// Trees' growth speed (ADR 0079 S3): × the world's days, 1 by default; more for a time-lapse
    /// (dev: the page's `?growth=`), tests.
    pub growth_speed: f64,
    /// Where the water grid runs (ADR 0062, docs/WATER_SURFACE_GRID.md §2d): inline on the tick's
    /// thread (the default: tests, a fallback), on its own native thread (the dedicated server), or
    /// remote (the browser's water worker, its messages carried by the embedder).
    pub water: crate::water::host::Placement,
    /// A new world's game mode (ADR 0065, `shared/modes/modes.json`; a name not there is the
    /// default, creative). A saved world keeps its own; one saved before modes is creative.
    pub mode: String,
    /// A new world's size (`dwell_protocol::world_size`, BIG_MAP.md M0): saved with it and sent at
    /// join (`WorldSettings` tag `WORLD_SIZE`). A saved world keeps its own; one saved before sizes
    /// is the default, today's. The generators don't read it yet (BIG_MAP.md M1–M2): only the
    /// default size is playable in this build.
    pub world_size: dwell_protocol::world_size::WorldSize,
    /// Every block breaks in one hit, as before block damage (docs/BLOCK_DAMAGE.md): a test
    /// world's switch, for tests and e2e specs that break blocks with one click. Off: the data's.
    pub one_hit_breaks: bool,
    /// The chopping prototype (docs/WORK_QUEUE.md item 7, `chop2` in shared/tools/tools.json):
    /// chops land where the look meets the trunk, each removes a share of the section, a tree
    /// falls past half toward its notch. Off: today's chopping (ADR 0067). The data's `enabled`
    /// by default; dev `?chop=v2` for a local world.
    pub chop2: bool,
    /// A new world's far ecology (docs/ECOLOGY.md §6): on by default; off, the world has none
    /// (saved so: a world keeps its own). `ecology_speed` × the world's days (tests, dev; a
    /// per-world tuning hook, saved with it).
    pub ecology: bool,
    pub ecology_speed: f64,
    /// Tests and tools: the faces' islands (A, B) in place of the world maps', for worlds without
    /// one (the flat world, test generators).
    pub ecology_islands: Option<Arc<[dwell_ecology::Island; 2]>>,
    /// A new world's tall grass growth (ADR 0075 T3c, docs/ECOLOGY.md §10): on by default; saved
    /// with the world, which keeps its own. A world saved before it (no `grass_growth` table) has
    /// none: its meadows stay as they are.
    pub grass: bool,
}

/// A new world's settings: this build's clock (`shared/look/daynight.json`), `water_speed` and the
/// default game mode (creative).
pub fn default_settings(water_speed: f64) -> WorldSettings {
    let c = dwell_look::daynight::DAY_NIGHT.clock;
    WorldSettings {
        clock: WorldClock {
            day_s: c.day_s as f32,
            night_s: c.night_s as f32,
            start_hour: c.start_hour as f32,
        },
        water_speed: water_speed as f32,
        mode: dwell_protocol::modes::DEFAULT_MODE,
        size: dwell_protocol::world_size::WorldSize::DEFAULT,
        moon_phase: None,
    }
}

/// The size of a saved world from its settings' bytes (BIG_MAP.md M2): today's for a world saved
/// without settings or from before sizes, or whose settings this build can't read.
fn saved_world_size(
    settings: Option<&[u8]>,
    water_speed: f64,
) -> dwell_protocol::world_size::WorldSize {
    use dwell_protocol::world_size::WorldSize;
    let before = WorldSettings {
        size: WorldSize::DEFAULT,
        ..default_settings(water_speed)
    };
    settings
        .and_then(SettingsFields::from_bytes)
        .and_then(|f| f.settings(before).ok())
        // A size this build's LOD root can't hold (a newer build's) opens as today's.
        .map_or(WorldSize::DEFAULT, |s| {
            WorldSize::playable(Some(s.size.radius_scale))
        })
}

/// A new world's settings under `config`: [`default_settings`] with its water speed and mode.
fn new_world_settings(config: &ServerConfig) -> WorldSettings {
    WorldSettings {
        mode: dwell_protocol::modes::mode_or_default(&config.mode).name,
        size: config.world_size,
        ..default_settings(config.water_speed)
    }
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
            join_at_spawn: false,
            worldgen_threads: 0,
            worldgen_budget: Duration::from_micros(4000),
            pregen_radius_chunks: 2,
            view_radius_chunks: wc::VIEW_RADIUS_CHUNKS as i32,
            render_radius_chunks: wc::RENDER_RADIUS_CHUNKS as i32,
            chunk_bytes_per_second: wc::CHUNK_BYTES_PER_SECOND as u32,
            respawn_preload_radius_chunks: RESPAWN_PRELOAD_RADIUS_CHUNKS,
            lod_threads: None,
            jobs_remote: false,
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
            water_speed: crate::water::FLOW.speed,
            growth_speed: 1.0,
            water: crate::water::host::Placement::Inline,
            mode: dwell_protocol::modes::DEFAULT_MODE.into(),
            world_size: dwell_protocol::world_size::WorldSize::DEFAULT,
            one_hit_breaks: false,
            chop2: dwell_protocol::chopping::CHOP2_DEFAULT,
            ecology: true,
            ecology_speed: 1.0,
            ecology_islands: None,
            grass: true,
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
    /// Of the above, sent for the respawn preload (§6.3).
    pub preload_sent: u32,
    /// Chunks the client currently has.
    pub streamed: usize,
}

include!("respawn_preload.gen.rs");

/// The chunk the body of a player standing at `feet` is centred in (the view's centre), on either
/// face: `half_height` up from the feet, face-locally.
fn body_centre_chunk(feet: [f64; 3], half_height: f64) -> ChunkCoord {
    let up = FaceFrame::at_height(feet[1]).vector([0.0, half_height, 0.0]);
    chunk_at(feet[0] + up[0], feet[1] + up[1], feet[2] + up[2])
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
    /// The queue's length when this latency window began: a queue that shrank over the window is
    /// draining a backlog (a stalled tick loop catching up), not holding steady latency.
    window_start_len: usize,
    last_processed_seq: u32,
    last_input: Input,
    health: i32,
    respawn_tick: u32,
    death_position: [f64; 3],
    last_knockback_seq: u32,
    rate_window_tick: u32,
    rate_window_count: u32,
    /// The water chunks' currents last sent (`send_currents`) and when.
    currents_sent: HashMap<dwell_world::currents::CurrentKey, (SentCurrents, u32)>,
    /// Ticks still to wait before this session's currents are sent (`CURRENTS_AFTER_JOIN`).
    currents_wait: u32,
    /// Applied at the start of the next step, by edit number: (edit number, edit, ticks held while
    /// it would go into a player, EDIT_HOLD_TICKS), and a tool's hit's contact (the tick it lands
    /// and the tool it was swung with; WORK_QUEUE item 25: a break with a tool waits for its swing's
    /// `contactS`, and is dropped if the tool is no longer held then).
    edits: Vec<(u32, BlockEditRequest, u8, Option<Contact>)>,
    /// Body edits (docs/BUILT_BODIES.md §4), applied after the block edits.
    body_edits: Vec<BodyEditRequest>,
    edit_credit: f64,
    /// Applied after the block edits, in order: (edit number, edit, ticks held while all of it
    /// would go into a player, EDIT_HOLD_TICKS).
    region_edits: Vec<(u32, RegionEdit, u8)>,
    region_credit: f64, // cells
    /// Edit requests (block and region) that have arrived, numbered from 1 (`EditAck`).
    edit_seq: u32,
    /// The last edit number acknowledged, and those refused since.
    edit_acked: u32,
    edit_refused: Vec<u32>,
    /// What the player holds (`held_item`, the server's ids; protocol v17) and its last swing
    /// (`swing_flags`), both carried to the others in snapshots (CHARACTERS.md §9.3).
    held: u16,
    swing: u8,
    /// Which swing `swing` is (`SwingKind`: a break or place, or a chop and its direction), sent
    /// only to a client that said `SwingHello` (`swing_hello`; WORK_QUEUE item 17).
    swing_kind: SwingKind,
    swing_hello: bool,
    /// Which falling pieces this player's snapshots carry (`felling::PieceBook`).
    piece_book: crate::felling::PieceBook,
    /// The player's items (ADR 0066), loaded at join and saved with the world; whether its client
    /// understands inventories (`InventoryAction::Hello`) and is owed an `Inventory`; and whether
    /// the saved record couldn't be read (a newer build's: it is kept, not overwritten).
    inventory: dwell_protocol::inventory::Inventory,
    inventory_hello: bool,
    /// It said `GroundHello` (ADR 0071): it is sent `GroundGone`.
    ground_hello: bool,
    inventory_dirty: bool,
    inventory_unread: bool,
    /// The host took flight away (a mode change, ADR 0065) while this player was in the air: its
    /// next landing hurts nothing (the fall is the host's doing), then it is cleared.
    flight_grace: bool,
    /// Whether its client understands cuts (`ChopAction::Hello`, ADR 0067), and the tick its next
    /// swing may land.
    chop_hello: bool,
    chop_ready: u32,
    /// Swings on their way to their contact (WORK_QUEUE item 25): the tick each lands, and it.
    pending_chops: Vec<(u32, ChopAction)>,
    /// Whether its client understands `Vitals` and the `Drown` cause (`VitalsHello`, ADR 0069),
    /// and the ticks it has been out of air since the last drowning damage.
    vitals_hello: bool,
    drown_ticks: u32,
    /// The last damage cause its client understands (`CausesHello`; 0: it never said, so only the
    /// causes every build knows, and `Drown` with `vitals_hello`).
    known_cause: u8,
    /// Its lava burn (docs/proposals/VOLCANOES.md §6): burning left after leaving lava.
    burn: dwell_lava::Burn,
    /// Whether its client draws creatures (`CreaturesHello`, ADR 0073).
    creatures_hello: bool,
    /// Whether its client draws block damage (`DamageHello`, docs/BLOCK_DAMAGE.md).
    damage_hello: bool,
    /// Whether its client reads the far ecology (`EcologyHello`, docs/ECOLOGY.md §9).
    ecology_hello: bool,
    /// Whether its client draws steam (`SteamHello`, VOLCANOES.md §6.3).
    steam_hello: bool,
    /// The tool it holds (item name, the hold's plane), from `ChopAction::Held`; having sent one,
    /// it understands `HeldTools` (ADR 0067).
    held_tool: Option<(String, bool)>,

    stats: SessionStats,
    // Terrain streaming.
    chunk_mode: ChunkMode,
    streamed: HashSet<ChunkCoord>, // sent and not unloaded
    stream_center: Option<ChunkCoord>,
    stream_complete: bool, // everything in view sent (until the center moves)
    /// The respawn preload (§6.3): while dead, the respawn point's chunk, whose ring is streamed
    /// after the view; and whether all of it has been sent.
    preload_center: Option<ChunkCoord>,
    preload_complete: bool,
    chunk_credit: f64, // bytes the client may still receive this tick
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
    /// One more swing the others see (`swing_flags`): a break or place (`broke`), and which it is
    /// (`kind`: a chop steps a break, so an older client plays that).
    fn swung(&mut self, broke: bool, kind: SwingKind) {
        self.swing = next_swing(self.swing, broke);
        self.swing_kind = kind;
    }

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
            currents_sent: HashMap::default(),
            currents_wait: CURRENTS_AFTER_JOIN,
            inventory: Default::default(),
            inventory_hello: false,
            ground_hello: false,
            inventory_dirty: false,
            inventory_unread: false,
            flight_grace: false,
            chop_hello: false,
            chop_ready: 0,
            pending_chops: Vec::new(),
            vitals_hello: false,
            known_cause: 0,
            drown_ticks: 0,
            burn: dwell_lava::Burn::default(),
            creatures_hello: false,
            damage_hello: false,
            ecology_hello: false,
            steam_hello: false,
            held_tool: None,
            primed: false,
            low_water: usize::MAX,
            window_ticks: 0,
            window_start_len: 0,
            last_processed_seq: 0,
            last_input: Input::default(),
            health: pc::MAX_HEALTH as i32,
            respawn_tick: 0,
            death_position: [0.0; 3],
            last_knockback_seq: 0,
            rate_window_tick: 0,
            rate_window_count: 0,
            edits: Vec::new(),
            body_edits: Vec::new(),
            edit_credit: 0.0,
            region_edits: Vec::new(),
            region_credit: 0.0,
            edit_seq: 0,
            edit_acked: 0,
            edit_refused: Vec::new(),
            held: 0,
            swing: 0,
            swing_kind: SwingKind::Plain,
            swing_hello: false,
            piece_book: crate::felling::PieceBook::default(),
            stats: SessionStats::default(),
            chunk_mode: ChunkMode::AwaitingCheck,
            streamed: HashSet::default(),
            stream_center: None,
            stream_complete: false,
            preload_center: None,
            preload_complete: false,
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
    dwell_world::chunk_of_pos([x, y, z])
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

/// A player's swing after one more edit (`swing_flags`): the count steps (wrapping) and the kind
/// is the edit's, so the others start a break or place swing when they see it change. A chop
/// steps it as a break (what an older client plays) and names itself in the session's
/// `swing_kind` ([`Session::swung`]).
fn next_swing(swing: u8, broke: bool) -> u8 {
    let count = (swing.wrapping_add(1)) & swing_flags::COUNT_MASK;
    count
        | if broke {
            swing_flags::BREAK
        } else {
            swing_flags::PLACE
        }
}

fn encode(m: &Message) -> Vec<u8> {
    m.encode()
}

/// The world file behind the voxel grid: chunks the world has saved (the server's index) load from it.
struct StoreChunks {
    db: Arc<dwell_storage::WorldDb>,
    saved: Arc<Mutex<HashMap<ChunkCoord, u32>>>,
    /// Lit explosives found in the chunks loaded (docs/EXPLOSIVES.md §3): their fuses start again.
    lit: Arc<Mutex<Vec<[i32; 3]>>>,
}

impl SavedChunks for StoreChunks {
    fn has(&self, c: ChunkCoord) -> bool {
        self.saved.lock().expect("saved index").contains_key(&c)
    }
    fn load(&mut self, c: ChunkCoord) -> Option<Chunk> {
        let saved = self.db.load_chunk(c)?;
        let lit: Vec<dwell_blocks::StateId> = dwell_world::explosives::EXPLOSIVES
            .iter()
            .filter_map(dwell_world::explosives::lit_state)
            .collect();
        if saved.voxels.iter().any(|m| lit.contains(m)) {
            let n = CHUNK_SIZE;
            let mut found = self.lit.lock().expect("lit explosives");
            for (i, m) in saved.voxels.iter().enumerate() {
                if lit.contains(m) {
                    let i = i as i32;
                    found.push([
                        c.x * n + (i & 31),
                        c.y * n + ((i >> 5) & 31),
                        c.z * n + (i >> 10),
                    ]);
                }
            }
        }
        Chunk::from_voxels(&saved.voxels, saved.revision)
    }
}

/// A block gone into a body in a tick: (the body, its new cell; none: a new body), the block.
type BodyBuilt = ((u32, Option<[i32; 3]>), dwell_blocks::StateId);

/// Block changes kept for the block dump's `edits`.
const EDIT_LOG_LEN: usize = 256;

/// One block change in the edit log: when (the server's tick), where, what it was and became, and
/// why.
#[derive(Clone, Copy, Debug)]
struct EditLogEntry {
    tick: u32,
    cell: [i32; 3],
    from: dwell_blocks::StateId,
    to: dwell_blocks::StateId,
    reason: VoxelModificationReason,
}

/// The tick's phases, in order, as `Server::tick_phases` names them.
pub const TICK_PHASES: [&str; 14] = [
    "collect",
    "edits",
    "terrain",
    "players",
    "pushes+damage",
    "pieces",
    "physics",
    "settle",
    "water+respawn",
    "snapshots",
    "stream",
    "worldgen",
    "lod",
    "save",
];

pub struct Server {
    config: ServerConfig,
    entropy: Box<dyn Entropy>,
    physics: Physics,
    air_test: Option<AirChunkTest>,
    /// The air test's sky floors, computed off the tick (`sky_columns.rs`; none for a generator
    /// override, whose air test computes on the tick).
    sky: Option<crate::sky_columns::SkyColumns>,
    world: VoxelWorld,
    players: Players,
    sessions: BTreeMap<SessionId, Session>,
    outbox: Vec<Outgoing>,
    knockbacks: Vec<(u16, Vec3)>, // applied in the next step
    /// The falling pieces (docs/FELLING.md).
    felling: crate::felling::Felling,
    /// The explosives' fuses burning and their table (docs/EXPLOSIVES.md §3).
    fuses: crate::blasts::Fuses,
    /// Lit explosives the saved chunks loaded held (their fuses start again, in full).
    relit: Arc<Mutex<Vec<[i32; 3]>>>,
    /// Each tick phase's wall time (ms) summed since the last `tick_phases` (native only).
    phase_ms: [f64; TICK_PHASES.len()],
    /// The counts `tick_counts` last read (collision builds, chunks generated on the tick).
    counts_at: [u64; 3],
    /// The water surface grid (docs/WATER_SURFACE_GRID.md): the world's water, held as found until edits free it.
    water: crate::water::host::WaterHost,
    /// The water's side of the world (ADR 0062): what its copy was told, what to send it next, the
    /// chunks it wants loaded, its newest record and reports.
    water_link: WaterLink,
    /// The last block changes not the water's own (edits, collapses, settles): a diagnostic for
    /// the block dump (docs/DEV_TOOLS.md), oldest first, at most `EDIT_LOG_LEN`.
    edit_log: std::collections::VecDeque<EditLogEntry>,
    /// The world's settings (DAY_NIGHT.md §2): sent at join, saved in the world's meta, fields a
    /// newer build added kept in `settings_fields`.
    settings: WorldSettings,
    settings_fields: SettingsFields,
    /// The world's worldgen config (ADR 0074 B7), from its installed maps: `""` for the build's
    /// ground. Non-empty, the join's settings name its hash (`tag::WORLDGEN`) and a guest asks for it.
    worldgen_text: String,
    tick: u32,
    next_player_id: u16,
    worldgen: WorldgenPool,
    generator: ChunkGenerator,
    verification_chunk: ChunkCoord,
    verification_hash: u64,
    /// The registry payload sent at join (ADR 0038) and its definitions hash, for the table size
    /// it was made at (states registered later — placeholders, mods — remake it).
    registry: (usize, std::sync::Arc<Vec<u8>>, u64),
    /// First state ids of the blocks this server registered at run time (mods, placeholders for
    /// states its world file names): with the build's, its registry. Other servers in the process
    /// (local mode, tests) do not serve them.
    own_blocks: Vec<dwell_blocks::StateId>,
    view_offsets: Vec<ChunkCoord>, // chunks in view relative to the center, nearest first
    prefetch_offsets: Vec<ChunkCoord>, // generated ahead around each player
    preload_offsets: Vec<ChunkCoord>, // the respawn preload's ring (§6.3), nearest first
    explicit_wanted: Vec<ChunkCoord>, // full-mode chunks to generate for streaming
    // Persistence: chunks in the world file (revision saved), modified chunks not saved yet, saves
    // in flight (their chunk revisions), and players who left since the last save.
    saved: Arc<Mutex<HashMap<ChunkCoord, u32>>>,
    dirty: HashSet<ChunkCoord>,
    in_flight: HashMap<u64, Vec<(ChunkCoord, u32)>>,
    departed: Vec<PlayerRecord>,
    /// Inventories of players who left since the last save (ADR 0066).
    departed_inventories: Vec<([u8; 32], Vec<u8>)>,
    /// Vitals records of players who left since the last save (ADR 0069).
    departed_vitals: Vec<([u8; 32], Vec<u8>)>,
    /// The loose items lying in the world (ADR 0066), and the moved ones not yet sent.
    loose: crate::loose_items::LooseItems,
    loose_moved: HashSet<u32>,
    /// The creatures wandering near players (ADR 0073).
    creatures: crate::creatures::Creatures,
    /// The wood cells cut by axes (ADR 0067).
    cuts: dwell_protocol::chopping::Cuts,
    cuts_unread: bool,
    /// The blocks hit short of breaking (docs/BLOCK_DAMAGE.md, ADR 0078), saved with the world.
    damage: dwell_protocol::damage::Damage,
    /// The saved damage is a newer build's: kept, not written over.
    damage_unread: bool,
    /// The lava fields (docs/proposals/VOLCANOES.md §6), saved with the world (`lava` table).
    lava: crate::lava::Lava,
    /// The saved lava is a newer build's: kept, not written over.
    lava_unread: bool,
    /// Steam from lava meeting water since the last `take_lava_steam` (the clients' puffs, later).
    lava_steam: Vec<crate::lava::Steam>,
    /// Steam cells sent since the server started (the dev report's `steamCells`).
    lava_steam_cells: u64,
    /// The trees' growth records (ADR 0079 S3), saved with the world.
    growth: dwell_worldgen::growth::Records,
    /// The saved records are a newer build's: kept, not written over.
    growth_unread: bool,
    /// The far ecology (docs/ECOLOGY.md §6): both faces' herds and packs, saved with the world.
    ecology: ecology::Ecology,
    /// Tall grass growth (ADR 0075 T3c): the switch and each chunk's last day, saved with the world.
    grass: grass::Grass,
    /// The saved record is a newer build's: kept, not written over.
    grass_unread: bool,
    grass_rule: dwell_worldgen::grass::Rule,
    /// Tests: an envelope in place of the terrain's.
    grass_envelope: Option<grass::GrassEnvelope>,
    /// The face terrains grass grows over (shared with the ecology's forage share).
    grass_terrain: grass::GrassTerrain,
    /// The world's growers (none: no species trees), built on first use.
    growers: Option<Option<Arc<dwell_worldgen::growth::Growers>>>,
    /// The loaded chunks already looked at for trees (not saved: looked at again after a load).
    growth_scanned: HashSet<ChunkCoord>,
    /// The ground items gone (picked up or knocked loose; ADR 0071), saved with the world.
    ground: dwell_world::ground::GroundGone,
    /// The saved gone set is a newer build's: kept, not written over.
    ground_unread: bool,
    /// Edited chunks as generated, for the ground items' rules (ADR 0071; not saved).
    ground_generated: dwell_world::hash::HashMap<dwell_world::ChunkCoord, Box<dwell_world::Chunk>>,
    /// The ground densities from the biomes (ADR 0074 B5), built at the first ground item asked
    /// for (none: they do not vary, the default world), and each chunk's once computed.
    ground_density: Option<Option<dwell_worldgen::GroundDensityFn>>,
    ground_density_chunks:
        dwell_world::hash::HashMap<dwell_world::ChunkCoord, dwell_world::ground::GroundDensity>,
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
        // A saved world keeps its own seed, generator and spawn (launch options apply to new worlds),
        // unless the launch puts its players at its own spawn.
        if let Some(meta) = config.store.as_ref().and_then(|s| s.db().load_meta()) {
            config.world_seed = meta.world_seed;
            config.generator_version = meta.generator_version;
            if meta.spawn.is_some() && !(config.join_at_spawn && config.spawn.is_some()) {
                config.spawn = meta.spawn;
            }
            // And its own size (BIG_MAP.md M2): its settings' (`init_storage` reads the rest); a
            // world from before sizes, or saved without settings, is today's size.
            config.world_size = saved_world_size(meta.settings.as_deref(), config.water_speed);
        }
        // A world from an older line (or a launch naming a retired version) opens on this build's
        // generator of its kind.
        config.generator_version = dwell_world::current_generator(config.generator_version);
        let (generator, air_test, sky) = match &config.generator_override {
            Some(g) => (g.clone(), config.air_test_override.clone(), None),
            None => {
                let air = dwell_worldgen::AirColumns::for_generator_sized(
                    config.generator_version,
                    config.world_seed,
                    config.world_size,
                );
                (
                    dwell_worldgen::generator_for_sized(
                        config.generator_version,
                        config.world_seed,
                        config.world_size,
                    ),
                    Some(air.test()),
                    Some(crate::sky_columns::SkyColumns::new(
                        air,
                        config.worldgen_threads > 0,
                        config.jobs_remote,
                    )),
                )
            }
        };
        let mut physics = Physics::default();
        let players = Players::new(&mut physics, true);
        let lod_threads = config
            .lod_threads
            .unwrap_or(usize::from(config.worldgen_threads > 0));
        let spawn = *config.spawn.get_or_insert_with(|| {
            dwell_worldgen::spawn_point_for_sized(
                config.generator_version,
                config.world_seed,
                config.world_size,
            )
        });
        let mut server = Self {
            edit_log: std::collections::VecDeque::new(),
            entropy,
            physics,
            world: VoxelWorld::new(generator.clone(), air_test.clone()),
            air_test,
            sky,
            players,
            sessions: BTreeMap::new(),
            outbox: Vec::new(),
            knockbacks: Vec::new(),
            felling: crate::felling::Felling::default(),
            fuses: crate::blasts::Fuses::default(),
            relit: Arc::default(),
            phase_ms: [0.0; TICK_PHASES.len()],
            counts_at: [0; 3],
            // The water surface grid (Phase 53, docs/WATER_SURFACE_GRID.md): the world's water read
            // from its blocks, held at rest until edits free it; the world map's flow kept for
            // the sources to come.
            water: crate::water::host::WaterHost::new(
                config.water,
                if config.water == crate::water::host::Placement::Remote {
                    // its worker makes its own (`In::Init`)
                    crate::water::Water::new()
                } else {
                    crate::water::Water::for_world(
                        config.generator_version,
                        config.world_seed,
                        config.generator_override.is_none(),
                    )
                },
            ),
            water_link: WaterLink {
                pending: if config.water == crate::water::host::Placement::Remote {
                    vec![WaterIn::Init {
                        generator_version: config.generator_version,
                        world_seed: config.world_seed,
                        map: config.generator_override.is_none(),
                    }]
                } else {
                    Vec::new()
                },
                ..WaterLink::default()
            },
            settings: new_world_settings(&config),
            settings_fields: SettingsFields::default().with(new_world_settings(&config)),
            worldgen_text: if dwell_world::has_terrain(config.generator_version)
                && config.generator_override.is_none()
            {
                dwell_worldgen::config::installed_worldgen(config.world_seed)
            } else {
                String::new()
            },
            tick: 0,
            next_player_id: 1,
            worldgen: if config.jobs_remote {
                WorldgenPool::new_remote(generator.clone())
            } else {
                WorldgenPool::new(generator.clone(), config.worldgen_threads)
            },
            generator,
            // The chunk of the cell under the feet.
            verification_chunk: chunk_at(spawn[0], spawn[1] - dwell_world::CELL_H, spawn[2]),
            verification_hash: 0,
            registry: (0, Default::default(), 0),
            own_blocks: Vec::new(),
            view_offsets: Vec::new(),
            prefetch_offsets: cube_offsets(PREFETCH_CHUNKS),
            preload_offsets: Vec::new(),
            explicit_wanted: Vec::new(),
            saved: Arc::new(Mutex::new(HashMap::default())),
            dirty: HashSet::default(),
            in_flight: HashMap::default(),
            departed: Vec::new(),
            departed_inventories: Vec::new(),
            departed_vitals: Vec::new(),
            loose: Default::default(),
            loose_moved: HashSet::default(),
            creatures: Default::default(),
            cuts: Default::default(),
            cuts_unread: false,
            damage: Default::default(),
            damage_unread: false,
            lava: Default::default(),
            lava_unread: false,
            lava_steam: Vec::new(),
            lava_steam_cells: 0,
            growth: Default::default(),
            growth_unread: false,
            ecology: ecology::Ecology::new(
                config.ecology,
                config.ecology_speed,
                config.world_seed,
                config.generator_version == dwell_world::GENERATOR_TERRAIN
                    && config.generator_override.is_none(),
                config.ecology_islands.clone(),
            ),
            grass: grass::Grass {
                enabled: config.grass,
                days: Default::default(),
            },
            grass_unread: false,
            grass_rule: dwell_worldgen::grass::Rule::build(config.world_seed),
            grass_envelope: None,
            grass_terrain: Default::default(),
            growers: None,
            growth_scanned: Default::default(),
            ground: Default::default(),
            ground_unread: false,
            ground_generated: Default::default(),
            ground_density: None,
            ground_density_chunks: Default::default(),
            next_save_tick: 0,
            save_stats: SaveStats::default(),
            lod: if config.jobs_remote {
                LodPropagation::new_remote(dwell_worldgen::lod_generator_for_sized(
                    config.generator_version,
                    config.world_seed,
                    config.world_size,
                ))
            } else {
                LodPropagation::new(
                    dwell_worldgen::lod_generator_for_sized(
                        config.generator_version,
                        config.world_seed,
                        config.world_size,
                    ),
                    lod_threads,
                )
            }
            .with_world(dwell_world::WorldBounds::of(config.world_size)),
            index_updates: BTreeMap::new(),
            next_index_update_tick: 0,
            lod_unsaved: HashSet::default(),
            lod_in_flight: HashMap::default(),
            lod_rebuild: false,
            config,
        };
        server.init_storage();
        server.place_world_vents();

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
        // The respawn preload's ring: the same sphere, of its own radius (at most the view's).
        server.set_respawn_preload(server.config.respawn_preload_radius_chunks);
        server.share_grass_with_ecology();
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
            if !s.inventory_unread {
                self.departed_inventories
                    .push((s.public_key, s.inventory.to_bytes()));
            }
            let record = self.vitals_record(&s);
            self.departed_vitals.push((s.public_key, record));
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
            Some(Message::SwingHello) => {
                if let Some(s) = self.sessions.get_mut(&id) {
                    s.swing_hello = true;
                }
            }
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
        let flight_policy = self.may_fly_by_policy(&self.sessions[&id]);
        let welcome = Welcome {
            player_id,
            world_seed: self.config.world_seed,
            generator_version: self.config.generator_version,
            server_tick: self.tick,
            verification_chunk: self.verification_chunk.to_array(),
            // REGION_EDITS: this server understands RegionEdit (older builds of the line reject an
            // unknown message, so clients send one only when this is set).
            // BRUSHES (BRUSHES.md): pastes may hold KEEP cells and region edits reach the brush
            // reach; set only in game modes that allow brushes (ADR 0065).
            // FLIGHT: this player may fly now (the mode and the flight policy); FLIGHT_POLICY:
            // the policy alone, for a client to recompute FLIGHT when the host changes the mode
            // (older clients ignore the bit and keep FLIGHT).
            // INVENTORY (ADR 0066): this server keeps inventories; a client that understands them
            // says `InventoryAction::Hello` (older servers reject the message, older clients
            // ignore the bit).
            flags: welcome_flags::REGION_EDITS
                | welcome_flags::INVENTORY
                | welcome_flags::CHOPPING
                | welcome_flags::CHOP_SWEEP
                | welcome_flags::VITALS
                | if flight { welcome_flags::FLIGHT } else { 0 }
                | if flight_policy {
                    welcome_flags::FLIGHT_POLICY
                } else {
                    0
                }
                | if self.brushes_allowed() {
                    welcome_flags::BRUSHES
                } else {
                    0
                },
            registry_hash: self.registry().1,
        };
        // The registry first (ADR 0038): the client maps ids with it when Welcome arrives.
        let (payload, _) = self.registry();
        let parts = dwell_blocks::sync::parts(&payload);
        for (i, part) in parts.iter().enumerate() {
            let m = Message::BlockRegistry(BlockRegistry {
                part: i as u16,
                parts: parts.len() as u16,
                data: part.to_vec(),
            });
            self.send_reliable(id, &m, Channel::Control);
        }
        // The world's settings (its clock, its water speed), which the client uses over its own.
        let settings = Message::WorldSettings(self.join_settings());
        self.send_reliable(id, &settings, Channel::Control);
        self.send_reliable(id, &Message::Welcome(welcome), Channel::Control);
        // The moving bodies already here (falling pieces, built bodies; docs/BUILT_BODIES.md §4):
        // a player joining sees them, not only the ones that start after.
        for i in 0..self.felling.pieces.len() {
            let m = self.spawn_message(i);
            self.send_reliable(id, &m, Channel::World);
        }
        // A returning player continues where it left the world (§6.4), unless it was dead, or the
        // launch puts players at its spawn (keeping their health).
        let saved = self
            .config
            .store
            .as_ref()
            .and_then(|st| st.db().load_player(&key));
        self.load_inventory(id, &key);
        match &saved {
            Some(p) if p.health > 0 => {
                let feet = (!self.config.join_at_spawn).then_some(p.feet);
                self.spawn_player(id, feet);
                self.sessions.get_mut(&id).expect("session").health = p.health;
            }
            _ => self.spawn_player(id, None),
        }
        if saved.as_ref().is_some_and(|p| p.health > 0) {
            self.load_vitals(id, &key);
        }
    }

    /// Adds a block to this server's registry from data (a mod's block; ADR 0038): it is sent to
    /// every client that joins after. Its first state id.
    pub fn register_block(
        &mut self,
        block: dwell_blocks::NewBlock,
    ) -> Result<dwell_blocks::StateId, String> {
        let first = dwell_blocks::register(dwell_blocks::Origin::Server, block)?;
        if !self.own_blocks.contains(&first) {
            self.own_blocks.push(first);
        }
        Ok(first)
    }

    /// The registry payload this server sends at join and its definitions hash (ADR 0038).
    pub fn registry(&mut self) -> (std::sync::Arc<Vec<u8>>, u64) {
        // Placeholders for states the world file names and this server lacks are its own too.
        let placeholders = self
            .config
            .store
            .as_ref()
            .map(|st| st.db().placeholders())
            .unwrap_or_default();
        for p in placeholders {
            if !self.own_blocks.contains(&p) {
                self.own_blocks.push(p);
            }
        }
        // Aliases (ADR 0063: states named with a property or value this build doesn't know,
        // from the world file or a structure) are served under their full strings.
        let aliases = dwell_blocks::blocks()
            .filter(|b| b.origin == dwell_blocks::Origin::Alias)
            .count();
        let count = self.own_blocks.len() + aliases;
        if self.registry.0 != count || self.registry.1.is_empty() {
            let own = &self.own_blocks;
            let bytes = dwell_blocks::sync::Payload::served_by(|b| {
                b.origin == dwell_blocks::Origin::Build
                    || b.origin == dwell_blocks::Origin::Alias
                    || own.iter().any(|&id| b.states().contains(&usize::from(id)))
            })
            .encode();
            let hash = dwell_blocks::sync::definitions_hash(&bytes);
            self.registry = (count, std::sync::Arc::new(bytes), hash);
        }
        (self.registry.1.clone(), self.registry.2)
    }

    /// Gameplay input comes as datagrams. On control: the one WorldgenCheck, which picks how the
    /// client receives chunks (§6.3), block edits (§6.5), resync and chunk/LOD requests; anything
    /// else is ignored.
    fn handle_joined(&mut self, id: SessionId, m: Message) {
        let verification_hash = self.verification_hash;
        let (uses_items, tick) = (self.uses_items(), self.tick);
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
            // Every edit gets the next number, applied or not: the client's prediction waits for
            // its `EditAck` (§6.5).
            // What the player holds, for the others' view of it: a block this server doesn't
            // know is an empty hand.
            Message::HeldItem(held) => {
                let block = usize::from(held & held_item::BLOCK_MASK);
                s.held = if block < dwell_blocks::state_count() {
                    held
                } else {
                    0
                };
            }
            Message::BlockEditRequest(edit) => {
                s.swung(edit.action == BlockEditAction::Break, SwingKind::Plain);
                s.edit_seq += 1;
                // A tool's hit lands at its swing's contact, never before the client's (the
                // request left at the click): that long after it arrives (WORK_QUEUE item 25).
                let contact = (edit.action == BlockEditAction::Break)
                    .then(|| held_break_item(s, uses_items))
                    .flatten()
                    .and_then(|t| {
                        let c = dwell_protocol::chopping::break_contact_s(Some(t));
                        (c > 0.0).then(|| (tick + contact_ticks(c), t.to_owned()))
                    });
                if s.edits.len() < MAX_QUEUED_EDITS {
                    s.edits.push((s.edit_seq, edit, 0, contact));
                } else {
                    s.stats.edits_rejected += 1;
                    s.edit_refused.push(s.edit_seq);
                }
            }
            Message::BodyEditRequest(edit) => {
                s.swung(edit.action == BlockEditAction::Break, SwingKind::Plain);
                if s.body_edits.len() < MAX_QUEUED_EDITS {
                    s.body_edits.push(edit);
                } else {
                    s.stats.edits_rejected += 1;
                }
            }
            Message::RegionEdit(edit) => {
                s.swung(false, SwingKind::Plain);
                s.edit_seq += 1;
                if s.region_edits.len() < region_edit::MAX_QUEUED {
                    s.region_edits.push((s.edit_seq, edit, 0));
                } else {
                    s.stats.region_edits_rejected += 1;
                    s.edit_refused.push(s.edit_seq);
                }
            }
            Message::InventoryAction(a) => self.inventory_action(id, a),
            Message::Chop(a) => self.chop_action(id, a),
            Message::VitalsHello => s.vitals_hello = true,
            Message::CausesHello(n) => s.known_cause = n,
            Message::CreaturesHello => self.creatures_hello(id),
            Message::DamageHello => self.damage_hello(id),
            Message::EcologyHello => self.ecology_hello(id),
            Message::SteamHello => s.steam_hello = true,
            Message::WorldgenRequest => {
                let m = Message::WorldgenConfig(self.worldgen_text.clone());
                self.send_reliable(id, &m, Channel::Control);
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
    /// Where a player (re)spawns without a saved position: around the spawn point, spread by
    /// player id so they don't start inside each other.
    fn respawn_feet(&self, player_id: u16) -> [f64; 3] {
        let at = self.config.spawn.expect("spawn");
        let [dx, dz] = spawn_spread(player_id);
        [at[0] + dx, at[1], at[2] + dz]
    }

    fn spawn_player(&mut self, id: SessionId, feet: Option<[f64; 3]>) {
        let spawn =
            DVec3::from(feet.unwrap_or_else(|| self.respawn_feet(self.sessions[&id].player_id)));
        let s = self.sessions.get_mut(&id).expect("session");
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
        // Every cause (falls, blasts, falling pieces) goes through here: a mode without the
        // `damage` ability (creative) hurts no one (ADR 0065).
        if !self.settings.allows(Ability::Damage) {
            return;
        }
        let s = self.sessions.get_mut(&id).expect("session");
        if s.handle.is_none() || amount <= 0 {
            return;
        }
        s.health = (s.health - amount).max(0);
        let mut e = self.event(PlayerEventKind::Damage, &self.sessions[&id]);
        e.amount = amount.min(255) as u8;
        e.cause = cause;
        self.broadcast_event(e);
        if self.sessions[&id].health == 0 {
            self.kill(id, cause);
        }
    }

    fn feet_y(&self, h: PlayerHandle) -> f64 {
        // Below the capsule's centre on face A, above it on face B.
        self.players.feet(&self.physics, h)
    }

    fn kill(&mut self, id: SessionId, cause: DamageCause) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let p = self.players.position(&self.physics, h);
        let death = [p.x, self.feet_y(h), p.z];
        self.players.despawn(&mut self.physics, h);
        let respawn_tick = self.tick + (pc::RESPAWN_SECONDS as u32) * SIM_HZ;
        let respawn = self.respawn_feet(self.sessions[&id].player_id);
        let half = f64::from(self.config.player_config.half_height(false));
        let preload = self
            .config
            .respawn_preload_radius_chunks
            .map(|_| body_centre_chunk(respawn, half));
        self.drop_inventory(id, death);
        let s = self.sessions.get_mut(&id).expect("session");
        s.death_position = death;
        s.handle = None;
        s.health = 0;
        s.inputs.clear();
        s.respawn_tick = respawn_tick;
        s.burn = dwell_lava::Burn::default(); // the dead don't burn on into their respawn
        s.preload_center = preload;
        s.preload_complete = false;
        let mut e = self.event(PlayerEventKind::Death, &self.sessions[&id]);
        e.cause = cause;
        self.broadcast_event(e);
    }

    /// Server-side effects of the controller tick: impact damage (a hit on the ground, a floor or a
    /// ramp, by the speed into it; walls and ceilings never hurt, PLAYER_CONTROLLER.md §6.10), sent as cause `Fall`.
    fn after_controller_tick(&mut self, id: SessionId) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let c = self.players.controller(h);
        let (impacted, grounded) = (c.events & events::IMPACTED != 0, c.ground.grounded);
        let (speed, flying) = (c.impact_speed, c.fly.flying);
        let s = self.sessions.get_mut(&id).expect("session");
        let graced = s.flight_grace;
        if s.flight_grace && !flying && (impacted || grounded) {
            s.flight_grace = false; // landed: the host's fall is over
        }
        if impacted && !graced {
            let amount = self.config.player_config.impact_damage(speed);
            self.damage(id, amount, DamageCause::Fall);
        }
        self.drown(id);
        self.burn(id);
    }

    /// Advances the simulation by one 1/SIM_HZ step.
    pub fn step(&mut self) {
        let mut watch = crate::clock::Stopwatch::start();
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
        // What the water sent since (its writes, currents, wanted chunks, record).
        self.apply_water();
        self.phase_ms[0] += watch.lap_ms();

        // 1. Block edits queued since the last tick (§6.5); collision syncs in the controller pass.
        // Swings whose head reaches what they hit this tick (WORK_QUEUE item 25).
        self.land_swings();
        self.apply_edits();
        self.apply_body_edits();
        // Fuses burn; the ones out blow (docs/EXPLOSIVES.md §3): their knockbacks go with this
        // tick's, their pieces into this tick's physics step.
        // A world saved with a fuse burning: the lit explosives its chunks held burn again.
        let relit = std::mem::take(&mut *self.relit.lock().expect("lit explosives"));
        for c in relit {
            if let Some((i, true)) =
                explosive_of(&self.fuses.table, self.world.get_voxel_if_loaded(c))
            {
                let seconds = self.fuses.table[i].fuse_seconds;
                self.fuses.light(c, i, seconds);
            }
        }
        for fuse in self.fuses.tick() {
            self.blast(fuse);
        }
        self.spawn_queued();
        self.phase_ms[1] += watch.lap_ms();

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
                // Only steady excess is trimmed: a draining backlog is movement the client already
                // predicted, and skipping it would snap the player back (docs/BUGS.md 1).
                let draining = s.inputs.len() < s.window_start_len;
                let excess = if draining {
                    0
                } else {
                    s.low_water.saturating_sub(MAX_BUFFERED_INPUTS)
                };
                for _ in 0..excess.min(s.inputs.len()) {
                    s.last_processed_seq = s.inputs.pop_front().expect("queued").0;
                    s.stats.inputs_skipped += 1;
                }
                s.low_water = usize::MAX;
                s.window_ticks = 0;
                s.window_start_len = s.inputs.len();
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

        // 3. Controller pipeline (PLAYER_CONTROLLER.md §4), then its server-side consequences. The
        // falling pieces' terrain collision loads with the players' (docs/FELLING.md §3).
        let boxes = self.felling.terrain_boxes(&self.physics);
        self.players
            .set_terrain_boxes(boxes, self.felling.rules.terrain_chunks_per_tick);
        self.players
            .tick_terrain(&mut self.world, &mut self.physics);
        self.phase_ms[2] += watch.lap_ms();
        // The mode's stamina and oxygen (ADR 0069), as it is this tick.
        let rules = self.vitals_rules();
        self.players.set_vitals(rules);
        self.players
            .tick_controllers(&mut self.world, &mut self.physics);
        self.phase_ms[3] += watch.lap_ms();
        // Pieces pushed are held from contacts while pushed (docs/BUILT_BODIES.md §6).
        let pushes = self.players.pushes().to_vec();
        self.felling.pushed(&mut self.physics, &pushes);
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

        // 4. The falling pieces' hard hits hurt (before the step resolves them), physics, then the
        // pieces at rest settle (docs/FELLING.md §4).
        self.phase_ms[4] += watch.lap_ms();
        self.piece_hits();
        // Held bodies whose build platform is gone move (docs/BUILT_BODIES.md §3); bodies in water
        // float (§6).
        self.felling
            .release_unheld(&mut self.world, &mut self.physics);
        self.felling.float(&mut self.world, &mut self.physics);
        self.phase_ms[5] += watch.lap_ms();
        self.physics.step(1.0 / f64::from(SIM_HZ));
        self.phase_ms[6] += watch.lap_ms();
        self.rejoin_apart();
        self.tick += 1;
        self.settle_pieces();
        self.phase_ms[7] += watch.lap_ms();
        self.flow_water();
        // Lava flows, cools and quenches (docs/proposals/VOLCANOES.md §6).
        self.flow_lava();
        self.send_lava_steam();
        // Trees grow by the world's days (ADR 0079 S3).
        self.grow_trees();
        // Tall grass grows and spreads by them (ADR 0075 T3c).
        self.grow_grass();
        // The far ecology steps by them too (docs/ECOLOGY.md §6).
        self.step_ecology();
        self.send_ecology();

        // Loose items fall, are picked up and despawn (ADR 0066); inventories changed are sent.
        self.step_items();
        // Creatures spawn near players, wander and despawn far away (ADR 0073).
        self.step_creatures();

        // 5. Respawns, then snapshots at SNAPSHOT_HZ.
        for &id in &ids {
            let Some(s) = self.sessions.get(&id) else {
                continue;
            };
            if !s.joined() || s.handle.is_some() || self.tick < s.respawn_tick {
                continue;
            }
            self.spawn_player(id, None);
            self.sessions.get_mut(&id).expect("session").preload_center = None;
            let h = self.sessions[&id].handle.expect("spawned");
            let p = self.players.position(&self.physics, h);
            let mut e = self.event(PlayerEventKind::Respawn, &self.sessions[&id]);
            e.position = [p.x, p.y - f64::from(self.players.half_height(h)), p.z];
            self.broadcast_world(&Message::PlayerEvent(e));
        }
        self.phase_ms[8] += watch.lap_ms();
        if self.tick.is_multiple_of(SNAPSHOT_EVERY) {
            self.send_snapshots();
            self.send_vitals();
        }
        if self.tick.is_multiple_of(CURRENTS_EVERY) {
            self.send_currents();
        }
        self.phase_ms[9] += watch.lap_ms();

        // 6. Terrain streaming, then what to generate next and what to forget.
        self.explicit_wanted.clear();
        if let Some(sky) = &mut self.sky {
            sky.begin();
        }
        for &id in &ids {
            if self.sessions.contains_key(&id) {
                self.stream_chunks(id);
            }
        }
        self.phase_ms[10] += watch.lap_ms();
        self.update_worldgen();
        self.phase_ms[11] += watch.lap_ms();

        // 6b. Level of detail: propagation off the tick, index updates, answers (§6.6).
        self.update_lod();
        self.phase_ms[12] += watch.lap_ms();

        // 7. Autosave (§6.4): the batch built here; committed by the I/O thread natively, and
        // without one (the browser) encoded a little a tick and committed on a tick of its own.
        if self.config.store.is_some() {
            // The water's record asked for a moment before (it comes back by the save).
            if !self.water_link.record_asked
                && self.tick + WATER_RECORD_LEAD_TICKS >= self.next_save_tick
            {
                self.water_link.pending.push(WaterIn::Record);
                self.water_link.record_asked = true;
            }
            // One save at a time: a save still encoding holds the next back (its chunks stay
            // dirty, and the next takes them all).
            let budget = crate::clock::Stopwatch::start();
            let encoding = self.config.store.as_ref().is_some_and(WorldStore::pending);
            if self.tick >= self.next_save_tick && !encoding {
                self.autosave();
            }
            // An inline store's save, a little a tick (natively the I/O thread's), the batch's
            // building counted in.
            if let Some(store) = &self.config.store
                && store.pending()
            {
                store.step(|| budget.elapsed_ms() < SAVE_STEP_MS);
                self.collect_saves();
            }
        }
        self.phase_ms[13] += watch.lap_ms();
        // 8. What the water is told this tick goes as one batch.
        self.flush_water();
        self.phase_ms[8] += watch.lap_ms();
    }

    /// What the ticks did since the last call, by name: the terrain collision's chunk builds
    /// (`collisionBuilds`) and the chunks the tick generated itself, not the jobs
    /// (`generatedOnTick`), and the full-cube cells put into or taken out of the terrain collider
    /// (`collisionCells`, a little a tick but those near players and pieces): where a slow
    /// `terrain` phase's time goes (docs/DEV_TOOLS.md).
    pub fn tick_counts(&mut self) -> [(&'static str, u64); 3] {
        let now = [
            self.players.terrain().map_or(0, |t| t.loads()),
            self.world.generated_on_access(),
            self.players.terrain().map_or(0, |t| t.cells()),
        ];
        let before = std::mem::replace(&mut self.counts_at, now);
        [
            ("collisionBuilds", now[0].saturating_sub(before[0])),
            ("generatedOnTick", now[1].saturating_sub(before[1])),
            ("collisionCells", now[2].saturating_sub(before[2])),
        ]
    }

    /// Each tick phase's wall time (ms, natively; zeros on wasm32) summed since the last call,
    /// named by [`TICK_PHASES`]: where a slow tick's time goes (tests, probes, tools).
    pub fn tick_phases(&mut self) -> [(&'static str, f64); TICK_PHASES.len()] {
        let ms = std::mem::take(&mut self.phase_ms);
        std::array::from_fn(|k| (TICK_PHASES[k], ms[k]))
    }

    /// How many bodies (falling pieces and built bodies) there are now.
    pub fn body_count(&self) -> usize {
        self.felling.pieces.len()
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
        for k in 0..self.prefetch_offsets.len() {
            for &center in &centers {
                let c = add(center, self.prefetch_offsets[k]);
                if !dwell_world::WorldBounds::of(self.config.world_size)
                    .volume()
                    .chunk_may_hold(c)
                    || self.world.find(c).is_some()
                {
                    continue;
                }
                if self.is_saved(c) {
                    if loads < MAX_SAVED_LOADS_PER_TICK {
                        self.world.get_or_create(c);
                    }
                    loads += 1;
                } else if self.air_known(c) == Some(false) {
                    wanted.push(c);
                }
            }
        }
        // Then the respawn preloads' rings (§6.3), for the respawned players' collision.
        let preloads: Vec<ChunkCoord> = self
            .sessions
            .values()
            .filter_map(|s| s.preload_center)
            .collect();
        if !preloads.is_empty() {
            for k in 0..self.preload_offsets.len() {
                for &center in &preloads {
                    let c = add(center, self.preload_offsets[k]);
                    if dwell_world::WorldBounds::of(self.config.world_size)
                        .volume()
                        .chunk_may_hold(c)
                        && self.world.find(c).is_none()
                        && !self.is_saved(c)
                        && self.air_known(c) == Some(false)
                    {
                        wanted.push(c);
                    }
                }
            }
        }
        wanted.extend_from_slice(&self.explicit_wanted);
        // Then those of moving water and beside it, with no player near (WATER_SURFACE_GRID.md §2b):
        // it flows on until it sleeps.
        let water = self.water_link.wanted.clone();
        for &c in &water {
            if !dwell_world::WorldBounds::of(self.config.world_size)
                .volume()
                .chunk_may_hold(c)
                || self.world.find(c).is_some()
            {
                continue;
            }
            if self.is_saved(c) {
                if loads < MAX_SAVED_LOADS_PER_TICK {
                    self.world.get_or_create(c);
                }
                loads += 1;
            } else {
                match self.air_known(c) {
                    Some(false) => wanted.push(c),
                    Some(true) => {
                        self.world.get_or_create(c);
                    }
                    None => {} // its sky floor first
                }
            }
        }
        self.worldgen.set_wanted(&wanted);
        // Inline (no thread or worker): the sky floors wanted this tick, within a budget.
        if let Some(sky) = &mut self.sky {
            sky.run_inline(SKY_FLOOR_BUDGET);
        }

        if self.tick.is_multiple_of(EVICT_EVERY_TICKS) {
            // Unmodified chunks can be generated again, and saved ones read again; modified chunks
            // not yet saved stay.
            let saved = self.saved.lock().expect("saved index").clone();
            let (verification, dirty) = (self.verification_chunk, &self.dirty);
            let water: HashSet<ChunkCoord> = water.into_iter().collect();
            self.world.evict(|c, chunk| {
                if c == verification || dirty.contains(&c) || water.contains(&c) {
                    return false;
                }
                let clean = chunk.revision() == 0 || saved.get(&c) == Some(&chunk.revision());
                clean
                    && centers
                        .iter()
                        .chain(&preloads)
                        .all(|&center| chunk_distance(c, center) > KEEP_CHUNKS)
            });
        }
    }

    /// Whether a chunk request is answered: within the render radius of the view's centre, or
    /// (while dead) within two chunks past the respawn preload's ring (§6.3), which the client's
    /// LOD asks for to show the level-1 sections the ring straddles as chunks.
    fn requestable(&self, s: &Session, c: ChunkCoord, center: ChunkCoord) -> bool {
        within_radius(c, center, self.config.render_radius_chunks)
            || s.preload_center
                .zip(self.config.respawn_preload_radius_chunks)
                .is_some_and(|(p, r)| within_radius(c, p, r + 2))
    }

    fn request_chunks(&mut self, id: SessionId, coords: &[ChunkCoordNet]) {
        let center = self.view_center(&self.sessions[&id]);
        let ok_at: Vec<bool> = coords
            .iter()
            .map(|&at| {
                center.is_some_and(|center| {
                    self.requestable(&self.sessions[&id], ChunkCoord::from_array(at), center)
                })
            })
            .collect();
        let s = self.sessions.get_mut(&id).expect("session");
        for (&at, &in_range) in coords.iter().zip(&ok_at) {
            let c = ChunkCoord::from_array(at);
            if s.requested.contains(&c) {
                continue; // already on its way or there
            }
            if s.streamed.contains(&c) {
                s.requested.insert(c); // streamed by the view: kept, now, while within the render radius
                continue;
            }
            let ok = s.chunk_mode != ChunkMode::AwaitingCheck
                && in_range
                && dwell_world::WorldBounds::of(self.config.world_size)
                    .volume()
                    .chunk_may_hold(c)
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
        let preload = s
            .preload_center
            .zip(self.config.respawn_preload_radius_chunks);

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
                    ) && !preload.is_some_and(|(p, pr)| within_radius(*c, p, pr))
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
        if self.sessions[&id].stream_complete
            && let Some((p, _)) = preload
            && !self.sessions[&id].preload_complete
        {
            // Then the respawn point's ring (§6.3): the view first (collision needs it).
            self.stream_preload(id, p, &mut sent);
        }
        let s = &self.sessions[&id];
        if s.stream_complete && (s.preload_complete || s.preload_center.is_none()) {
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
        // Nearest first, within the bandwidth budget (§6.3). A chunk whose sky floor is not known
        // yet stops the sending (the order kept); those after it only want theirs.
        let mut complete = true;
        let mut waiting = false;
        for i in 0..self.view_offsets.len() {
            let c = add(center, self.view_offsets[i]);
            let s = &self.sessions[&id];
            // Chunks wholly outside the build volume are air everywhere: never sent (ADR 0025).
            if !dwell_world::WorldBounds::of(self.config.world_size)
                .volume()
                .chunk_may_hold(c)
                || s.streamed.contains(&c)
            {
                continue;
            }
            complete = false;
            if s.chunk_credit <= 0.0 || *sent >= wc::MAX_CHUNKS_PER_TICK {
                break;
            }
            if !self.air_ready(c) || waiting {
                waiting = true;
                continue;
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

    /// The respawn preload's ring around `center` (§6.3), nearest first, within what the view's
    /// streaming left of the budgets: the server's view sphere of `respawn_preload_radius_chunks`.
    fn stream_preload(&mut self, id: SessionId, center: ChunkCoord, sent: &mut i64) {
        let mut complete = true;
        let mut waiting = false;
        for i in 0..self.preload_offsets.len() {
            let c = add(center, self.preload_offsets[i]);
            let s = &self.sessions[&id];
            if !dwell_world::WorldBounds::of(self.config.world_size)
                .volume()
                .chunk_may_hold(c)
                || s.streamed.contains(&c)
            {
                continue;
            }
            complete = false;
            if s.chunk_credit <= 0.0 || *sent >= wc::MAX_CHUNKS_PER_TICK {
                break;
            }
            if !self.air_ready(c) || waiting {
                waiting = true;
                continue;
            }
            let Some(m) = self.chunk_message(id, c, false) else {
                // Full mode: generated first (update_worldgen wants the ring).
                continue;
            };
            let form = m.form;
            let bytes = encode(&Message::ChunkData(m));
            let s = self.sessions.get_mut(&id).expect("session");
            s.chunk_credit -= bytes.len() as f64;
            s.streamed.insert(c);
            s.stream_stats.preload_sent += 1;
            Self::count_sent(&mut s.stream_stats, form);
            self.outbox.push(Outgoing {
                session: id,
                kind: OutKind::Reliable(Channel::World),
                bytes,
            });
            *sent += 1;
        }
        self.sessions
            .get_mut(&id)
            .expect("session")
            .preload_complete = complete;
    }

    fn stream_requested(&mut self, id: SessionId, center: ChunkCoord, sent: &mut i64) {
        // In the order asked (the client asks nearest first, §6.6), within what the budget leaves;
        // one waiting for its sky floor stops the sending, and those just after want theirs.
        loop {
            let s = self.sessions.get_mut(&id).expect("session");
            if s.chunk_credit <= 0.0 || *sent >= wc::MAX_CHUNKS_PER_TICK {
                break;
            }
            let Some(&c) = s.chunk_requests.front() else {
                break;
            };
            if s.streamed.contains(&c) {
                s.chunk_requests.pop_front();
                continue; // the view reached it first
            }
            let requestable = self.requestable(&self.sessions[&id], c, center);
            if requestable && !self.air_ready(c) {
                let ahead: Vec<ChunkCoord> = self.sessions[&id]
                    .chunk_requests
                    .iter()
                    .skip(1)
                    .take(REQUEST_LOOKAHEAD)
                    .copied()
                    .collect();
                for a in ahead {
                    self.air_ready(a);
                }
                break;
            }
            self.sessions
                .get_mut(&id)
                .expect("session")
                .chunk_requests
                .pop_front();
            // Full mode would generate on the server for rendering alone: only what is stored.
            let message = requestable
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
    /// Flight: the world's mode allows it (ADR 0065) and the flight policy lets this player.
    fn may_fly(&self, s: &Session) -> bool {
        self.settings.allows(Ability::Flight) && self.may_fly_by_policy(s)
    }
    fn may_fly_by_policy(&self, s: &Session) -> bool {
        self.allowed(self.config.flight, s)
    }
    fn brushes_allowed(&self) -> bool {
        self.settings.allows(Ability::Brushes)
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
        let face = c.face.sign_f64();
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
        // Bodies placed blocks start (cell, block, kind).
        // Blocks gone into bodies this tick: (body, its new cell; none: a new body), the block.
        let mut bodies_built: Vec<BodyBuilt> = Vec::new();
        let brushes = self.brushes_allowed();
        let reach = region_reach(brushes);
        // Without unlimited blocks (ADR 0066): placing takes an item, breaking drops the block's.
        let uses_items = self.uses_items();
        let mut broken: Vec<([i32; 3], dwell_blocks::StateId)> = Vec::new();
        // Hits short of breaking (cell, state, hits to break, this hit's number), for their drops;
        // and the cells a last hit broke, with their hits (the destroy drops what is left).
        let mut hits: Vec<([i32; 3], dwell_blocks::StateId, u32, u32)> = Vec::new();
        let mut destroyed_after: HashMap<[i32; 3], u32> = HashMap::default();
        // Cells a hit damaged this tick, for the clients that draw damage.
        let mut damaged: Vec<[i32; 3]> = Vec::new();
        // Every broken cell and what it was, in any mode: ground items on them are knocked loose.
        let mut under_ground: Vec<([i32; 3], dwell_blocks::StateId)> = Vec::new();
        // Saplings planted this tick (ADR 0079 S4): each becomes its species' stage-0 tree.
        let mut planted: Vec<dwell_worldgen::growth::Record> = Vec::new();
        let day = self.world_days();
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
            let mut held = Vec::new();
            for (seq, edit, ticks, contact) in std::mem::take(&mut s.edits) {
                // A tool's hit waits for its contact; switched away from by then, it is dropped.
                if let Some((at, tool)) = &contact {
                    if self.tick < *at {
                        held.push((seq, edit, ticks, contact));
                        continue;
                    }
                    if held_break_item(s, uses_items) != Some(tool.as_str()) {
                        s.edit_refused.push(seq);
                        continue;
                    }
                }
                let Some(eye) = eye.filter(|_| may_edit && s.edit_credit >= 1.0) else {
                    s.stats.edits_rejected += 1;
                    s.edit_refused.push(seq);
                    continue;
                };
                let outcome =
                    check_block_edit(&mut self.world, &edit, eye, &capsules, DEFAULT_REACH);
                s.stats.last_edit_check = outcome.check;
                // Into a body: it waits for the body's queued inputs to move it (EDIT_HOLD_TICKS).
                if outcome.check == EditCheck::IntoPlayer && ticks < EDIT_HOLD_TICKS {
                    held.push((seq, edit, ticks + 1, None));
                    continue;
                }
                if outcome.check != EditCheck::Ok {
                    s.stats.edits_rejected += 1;
                    s.edit_refused.push(seq);
                    continue;
                }
                // A species' sapling takes only on its `plantOn` ground (shared/trees; ADR 0079 S4).
                let plant = match edit.action {
                    BlockEditAction::Place => sapling_planted(&self.world, &outcome, day),
                    _ => Ok(None),
                };
                let Ok(plant) = plant else {
                    s.stats.edits_rejected += 1;
                    s.edit_refused.push(seq);
                    continue;
                };
                // The item asked for must be held; it is used up when it is what goes in (a torch
                // held against dynamite lights it and stays in the hand).
                if uses_items && edit.action == BlockEditAction::Place {
                    let item = items::cost_of(edit.material);
                    let placed = items::cost_of(outcome.material) == item;
                    if s.inventory.count_of(&item) == 0 {
                        s.stats.edits_rejected += 1;
                        s.edit_refused.push(seq);
                        continue;
                    }
                    if placed {
                        s.inventory.take_one(&item);
                        s.inventory_dirty = true;
                    }
                }
                // A block that needs a tool kind to break (its `breakWith`: a log, an axe; ADR
                // 0067) is refused to a hand without one where blocks are not unlimited.
                if uses_items && edit.action == BlockEditAction::Break {
                    let [x, y, z] = outcome.cell;
                    let block = dwell_blocks::block_of(self.world.get_voxel(x, y, z)).name;
                    let held = s
                        .inventory
                        .slots
                        .get(usize::from(s.inventory.held))
                        .and_then(|h| h.as_ref())
                        .map(|st| st.item.as_str());
                    if !dwell_protocol::chopping::may_break_holding(block, held) {
                        s.stats.edits_rejected += 1;
                        s.edit_refused.push(seq);
                        continue;
                    }
                }
                // Blocks take hits to break by the tool held (docs/BLOCK_DAMAGE.md, ADR 0078; the
                // same in every mode): a hit short of the count is recorded and the block stays
                // (the client's predicted break gives way, as to a refusal); none: it can't be
                // broken so (stone by hand). A mode with `instantBreaks` (creative; owner
                // 2026-11-11) breaks every block in one hit with anything but a tool held.
                if edit.action == BlockEditAction::Break {
                    let state = self.world.get_voxel_if_loaded(outcome.cell);
                    let block = dwell_blocks::block_of(state).name;
                    let held = held_break_item(s, uses_items);
                    let need = if self.config.one_hit_breaks {
                        Some(1)
                    } else {
                        dwell_protocol::damage::hits_to_break_for(
                            block,
                            held,
                            self.settings.allows(Ability::InstantBreaks),
                        )
                    };
                    // A hit that counts wears the held tool (owner, 2026-11-11), where items are
                    // used (or creative too, `creativeWear`).
                    if need.is_some() {
                        match wear_tool(&mut s.inventory, uses_items) {
                            inv::Wear::None => {}
                            inv::Wear::Worn(_) | inv::Wear::Broke => s.inventory_dirty = true,
                        }
                    }
                    match need {
                        None => {
                            s.stats.edits_rejected += 1;
                            s.edit_refused.push(seq);
                            continue;
                        }
                        Some(n) if n > 1 => {
                            let mark = dwell_protocol::damage::HitMark {
                                face: edit.face,
                                point: edit.point.unwrap_or([128; 3]),
                            };
                            let k = u32::from(self.damage.hit(
                                outcome.cell,
                                state,
                                n.min(u32::from(u16::MAX)) as u16,
                                mark,
                            ));
                            s.edit_credit -= 1.0;
                            s.stats.edits_applied += 1;
                            if k < n {
                                if uses_items {
                                    hits.push((outcome.cell, state, n, k));
                                }
                                damaged.push(outcome.cell);
                                s.edit_refused.push(seq);
                                continue;
                            }
                            destroyed_after.insert(outcome.cell, n);
                        }
                        Some(_) => {
                            s.edit_credit -= 1.0;
                            s.stats.edits_applied += 1;
                        }
                    }
                } else {
                    s.edit_credit -= 1.0;
                    s.stats.edits_applied += 1;
                }
                // On a build platform's top, or touching a held body, a block goes into that body,
                // not the grid (docs/BUILT_BODIES.md §3); the client's predicted block gives way to
                // it when the edit is acknowledged.
                if edit.action == BlockEditAction::Place
                    && let Some(d) = self.felling.divert(
                        &mut self.world,
                        &self.physics,
                        outcome.cell,
                        outcome.material,
                    )
                {
                    if let Some(done) = self.felling.place_diverted(
                        &mut self.physics,
                        d,
                        outcome.cell,
                        outcome.material,
                    ) {
                        bodies_built.push((done, outcome.material));
                    }
                    continue;
                }
                planted.extend(plant);
                // Every cell of the piece (a 1 m block's two; docs/ITEMS.md) in one set of writes.
                for (at, m) in outcome.writes(&mut self.world, medium_kept) {
                    if edit.action == BlockEditAction::Break {
                        let was = self.world.get_voxel(at[0], at[1], at[2]);
                        under_ground.push((at, was));
                        if uses_items {
                            broken.push((at, was));
                        }
                    }
                    changes.set(&mut self.world, at, m);
                }
            }
            s.edits = held;
            // Region edits (§6.5), after the block edits, in the order they arrived; each costs
            // its box's volume in cells, valid or not (validation reads the whole box).
            s.region_credit = (s.region_credit + REGION_CELLS_PER_TICK).min(REGION_BURST_CELLS);
            let mut held = Vec::new();
            for (seq, edit, ticks) in std::mem::take(&mut s.region_edits) {
                let cost = edit.volume() as f64;
                let Some(eye) = eye.filter(|_| may_edit && s.region_credit >= cost) else {
                    s.stats.region_edits_rejected += 1;
                    s.edit_refused.push(seq);
                    continue;
                };
                s.region_credit -= cost;
                // A brush stroke's KEEP cells only in a mode with brushes (ADR 0065).
                let outcome = if !brushes
                    && edit.op == dwell_protocol::RegionEditOp::Paste
                    && edit.cells.contains(&dwell_world::region::KEEP)
                {
                    dwell_world::region::RegionOutcome::fail(RegionCheck::NotPlaceable)
                } else {
                    check_region_edit(&mut self.world, &edit, eye, &capsules, reach)
                };
                s.stats.last_region_check = outcome.check;
                // Without unlimited blocks, a region places only what the player holds (ADR 0066).
                let held_item = s
                    .inventory
                    .slots
                    .get(usize::from(s.inventory.held))
                    .and_then(|h| h.as_ref())
                    .map(|st| st.item.clone());
                // Nor does a region clear a block that needs a tool the hand lacks (`breakWith`).
                let outcome = if uses_items
                    && outcome.check == RegionCheck::Ok
                    && outcome.changes.iter().any(|&(c, m)| {
                        (m == M.air || info(m).liquid)
                            && !dwell_protocol::chopping::may_break_holding(
                                dwell_blocks::block_of(self.world.get_voxel(c[0], c[1], c[2])).name,
                                held_item.as_deref(),
                            )
                    }) {
                    dwell_world::region::RegionOutcome::fail(RegionCheck::NotPlaceable)
                } else {
                    outcome
                };
                let outcome = if uses_items && outcome.check == RegionCheck::Ok {
                    let need = items::region_costs(&outcome.changes);
                    if need
                        .iter()
                        .all(|(item, n)| s.inventory.count_of(item) >= *n)
                    {
                        for (item, n) in &need {
                            for _ in 0..*n {
                                s.inventory.take_one(item);
                            }
                        }
                        s.inventory_dirty |= !need.is_empty();
                        outcome
                    } else {
                        dwell_world::region::RegionOutcome::fail(RegionCheck::NotPlaceable)
                    }
                } else {
                    outcome
                };
                if outcome.check != RegionCheck::Ok {
                    s.stats.region_edits_rejected += 1;
                    s.edit_refused.push(seq);
                    continue;
                }
                // All of it into a body (a drag's piece under a rising player): it waits, as a
                // block edit does (EDIT_HOLD_TICKS), and costs nothing until it goes.
                if outcome.changes.is_empty() && outcome.into_player > 0 && ticks < EDIT_HOLD_TICKS
                {
                    s.region_credit += cost;
                    held.push((seq, edit, ticks + 1));
                    continue;
                }
                s.stats.region_edits_applied += 1;
                s.stats.region_cells_changed += outcome.changes.len() as u64;
                let cells: Vec<[i32; 3]> = outcome.changes.iter().map(|(c, _)| *c).collect();
                for (cell, m) in outcome.changes {
                    if uses_items && (m == M.air || info(m).liquid) {
                        broken.push((cell, self.world.get_voxel(cell[0], cell[1], cell[2])));
                    }
                    // As a block edit's: into a held body, or a new one on a build platform.
                    let d = (m != M.air && !info(m).liquid)
                        .then(|| self.felling.divert(&mut self.world, &self.physics, cell, m))
                        .flatten();
                    if let Some(d) = d {
                        if let Some(done) =
                            self.felling.place_diverted(&mut self.physics, d, cell, m)
                        {
                            bodies_built.push((done, m));
                        }
                        continue;
                    }
                    changes.set_and_unsupport(&mut self.world, cell, m);
                }
                // A region may put blocks that need support where nothing holds them (tall grass
                // filled into the air, or onto itself; a ladder with no wall behind it): those are
                // not kept.
                for cell in cells {
                    let m = self.world.get_voxel(cell[0], cell[1], cell[2]);
                    if m != M.air && !held_in_place(&mut self.world, cell, m) {
                        let left = if info(m).flooded { M.water } else { M.air };
                        changes.set_and_unsupport(&mut self.world, cell, left);
                    }
                }
            }
            s.region_edits = held;
        }
        // A planted sapling stands as its species' stage-0 tree, in what is still air (or the
        // sapling itself), and its record grows it from today (the growers step).
        for r in planted {
            for (c, m) in dwell_worldgen::growth::cells_at(&r, 0) {
                if c == r.anchor || self.world.get_voxel(c[0], c[1], c[2]) == M.air {
                    changes.set(&mut self.world, c, m);
                }
            }
            self.growth.trees.insert(r.anchor, r);
        }
        let edited = changes.cells();
        self.broadcast_changes(changes, VoxelModificationReason::Edit);
        self.send_damage(&damaged, Vec::new());
        for (cell, was, n, k) in hits {
            self.drop_hit(was, dwell_world::centre_of_cell(cell), n, k);
        }
        for (cell, was) in broken {
            match destroyed_after.get(&cell) {
                Some(&n) => self.drop_destroyed(was, dwell_world::centre_of_cell(cell), n),
                None => self.drop_block(was, dwell_world::centre_of_cell(cell)),
            }
            self.drop_sapling(cell, dwell_world::centre_of_cell(cell));
        }
        self.knock_ground(&under_ground);
        // An explosive lit by an edit (a torch held against it) starts its fuse (docs/EXPLOSIVES.md).
        for &c in &edited {
            let m = self.world.get_voxel_if_loaded(c);
            if let Some((i, true)) = explosive_of(&self.fuses.table, m) {
                let seconds = self.fuses.table[i].fuse_seconds;
                self.fuses.light(c, i, seconds);
            }
        }
        for ((id, joined), m) in bodies_built {
            match joined {
                Some(cell) => self.broadcast_world(&Message::BodyEdit(BodyEdit {
                    body: id,
                    cells: vec![PieceCellNet {
                        offset: cell.map(|v| v as i16),
                        state: m,
                    }],
                })),
                None => {
                    if let Some(i) = self.felling.pieces.iter().position(|p| p.id == id) {
                        let spawn = self.spawn_message(i);
                        self.broadcast_world(&spawn);
                    }
                }
            }
        }
        // What the edits left without support falls (docs/FELLING.md §2), tipped away from the
        // nearest player (who cut it, as a rule).
        let near = edited.first().and_then(|&c| {
            let at = crate::felling::cell_centre(c);
            capsules
                .iter()
                .map(|p| DVec3::from(p.center))
                .min_by(|a, b| (*a - at).length().total_cmp(&(*b - at).length()))
        });
        self.fell(&edited, 0, near);
        self.send_edit_acks();
    }

    /// Sends a tick's voxel changes (`reason`) to every client streaming their chunks, one revision
    /// per chunk, and marks the chunks dirty for saving and the LOD.
    fn broadcast_changes(&mut self, changes: TickChanges, reason: VoxelModificationReason) {
        let TickChanges {
            chunks: mut changed,
            before,
            water,
            lava,
            ..
        } = changes;
        // The lava fields read an edited column's bed and water again (a dug channel, poured water).
        if !lava && !self.lava.is_empty() {
            for &(cell, _) in &before {
                self.lava.touch(cell);
            }
        }
        if !water {
            for &(cell, from) in &before {
                if self.edit_log.len() == EDIT_LOG_LEN {
                    self.edit_log.pop_front();
                }
                self.edit_log.push_back(EditLogEntry {
                    tick: self.tick,
                    cell,
                    from,
                    to: self.world.get_voxel_if_loaded(cell),
                    reason,
                });
            }
            // The water's copy of the edited cells (their columns touched).
            for &(cell, _) in &before {
                let byte = crate::water::cells::byte_of(self.world.get_voxel_if_loaded(cell));
                self.water_link.pending.push(WaterIn::Edit { cell, byte });
            }
        }
        // A changed cell's cut goes with its wood (ADR 0067), and its damage with its block.
        self.cuts_dropped(&before);
        if !self.damage.cells.is_empty() {
            let gone: Vec<[i32; 3]> = before
                .iter()
                .filter(|(c, _)| self.damage.cells.remove(c).is_some())
                .map(|(c, _)| *c)
                .collect();
            self.send_damage(&[], gone);
        }
        if changed.is_empty() {
            return;
        }
        for c in &changed {
            let coord = ChunkCoord::from_array(c.coord);
            self.dirty.insert(coord);
            // The water's writes go on changing a chunk while it moves: its LOD section is computed
            // a while after the first (once a hold, not every water tick); an edit's at once.
            if water {
                self.lod
                    .mark_chunk_settling(coord, u64::from(WATER_LOD_HOLD_TICKS));
            } else {
                self.lod.mark_chunk(coord);
            }
        }
        // One revision per chunk per modification, then each client gets the chunks it streams.
        for c in &mut changed {
            let coord = ChunkCoord::from_array(c.coord);
            let chunk = self.world.get_or_create(coord);
            let (stamp, before) = (chunk.stamp(), chunk.revision());
            chunk.bump_revision();
            c.revision = chunk.revision();
            // told to the water cell by cell (edits above, its own writes it made)
            self.water_link
                .mirror
                .told(coord, stamp, before, c.revision);
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
                    reason,
                    server_tick: self.tick,
                    chunks,
                };
                self.send_reliable(id, &Message::VoxelModification(m), Channel::World);
            }
        }
    }

    /// Detaches what the cells `changed` left without support (docs/FELLING.md §2): the pieces'
    /// cells are cleared in one `Collapse` modification, then each piece's `EntitySpawn` goes to
    /// every joined client. `settles` counts how often these cells settled unanchored before.
    fn fell(&mut self, changed: &[[i32; 3]], settles: u8, away_from: Option<DVec3>) {
        self.fell_tipped(changed, settles, away_from.map(|p| (p, 1.0)));
    }

    /// [`Self::fell`] with the share of the topple spin a carrying piece tips away with.
    pub(crate) fn fell_tipped(
        &mut self,
        changed: &[[i32; 3]],
        settles: u8,
        away_from: Option<(DVec3, f64)>,
    ) {
        if changed.is_empty() {
            return;
        }
        let mut collapse = TickChanges::default();
        let new = self.felling.detach(
            &mut self.world,
            &mut self.physics,
            changed,
            settles,
            away_from,
            &mut |world, cell, m| collapse.set(world, cell, m),
        );
        self.broadcast_changes(collapse, VoxelModificationReason::Collapse);
        for i in new {
            let m = self.spawn_message(i);
            self.broadcast_world(&m);
        }
    }

    /// Sets off the blast of `fuse` (docs/EXPLOSIVES.md §3), if its explosive is still there, lit.
    /// Every other block whose cell centre is within its radius is destroyed outright within
    /// `destroyRadiusM`, and beyond it becomes a natural piece flung out from it (nearest first, at
    /// most `limits.maxPieces`), but liquids, which within `waterRadiusM` are blown out, and
    /// `indestructible` blocks, which stay; an explosive caught in it is lit with a short random
    /// fuse (`chainDelaySeconds`). The cells go in one `Explosion` modification (its own cell first:
    /// the clients' effects start from it), then the pieces' spawns. Players within `reachM` are
    /// knocked back by the falloff share, and hurt only within `destroyRadiusM`; bodies are pushed;
    /// what the blast left without support falls.
    fn blast(&mut self, fuse: crate::blasts::Fuse) {
        use dwell_world::explosives::{
            ShellRule, blast_damage, cells_within, cracking, destroys, fling_speed, launching,
            open_dir, piece_dir, push_dir, radial_dir, share,
        };
        let Some(e) = self.fuses.table.get(fuse.explosive).copied() else {
            return;
        };
        let at = fuse.cell;
        if explosive_of(&self.fuses.table, self.world.get_voxel(at[0], at[1], at[2]))
            != Some((fuse.explosive, true))
        {
            return;
        }
        let centre = crate::felling::cell_centre(at);
        let centre_a = centre.to_array();
        // The way out (docs/EXPLOSIVES.md §3): sensed from the cells round it before it clears any.
        let solid_at = |world: &mut VoxelWorld, o: [i32; 3]| {
            let m = world.get_voxel(at[0] + o[0], at[1] + o[1], at[2] + o[2]);
            m != M.air && !info(m).liquid
        };
        let open = open_dir(e.open_sense_m, |o| solid_at(&mut self.world, o));
        let crack = e.shell == ShellRule::Crack;
        // The water round the charge as it stood before the blast (offsets), for `water_shields`.
        let water_before: std::collections::HashSet<[i32; 3]> = if e.water_shields {
            cells_within(e.loosen_radius_m.max(e.radius_m))
                .into_iter()
                .filter(|&o| {
                    let c = [at[0] + o[0], at[1] + o[1], at[2] + o[2]];
                    let m = self.world.get_voxel(c[0], c[1], c[2]);
                    m != M.air && (info(m).liquid || info(m).flooded)
                })
                .collect()
        } else {
            Default::default()
        };
        // A charge touching water spares the blocks (owner, 2026-11-07: "not destroy or dislodge
        // (physics) blocks when it's touching water"): its six neighbours, read before any clears.
        let wet = e.water_spares_blocks
            && [
                [0, 0, 0],
                [1, 0, 0],
                [-1, 0, 0],
                [0, 1, 0],
                [0, -1, 0],
                [0, 0, 1],
                [0, 0, -1],
            ]
            .iter()
            .any(|o: &[i32; 3]| {
                let c = [at[0] + o[0], at[1] + o[1], at[2] + o[2]];
                let m = self.world.get_voxel(c[0], c[1], c[2]);
                m != M.air && (info(m).liquid || info(m).flooded)
            });
        // A cracking shell is judged by what touched open space before the blast: the open cells
        // round it, read before any clears (docs/EXPLOSIVES.md §3.6).
        let open_before: std::collections::HashSet<[i32; 3]> = if crack {
            cells_within(e.radius_m + 1.0)
                .into_iter()
                .filter(|&o| !solid_at(&mut self.world, o))
                .map(|o| [at[0] + o[0], at[1] + o[1], at[2] + o[2]])
                .collect()
        } else {
            Default::default()
        };
        // The cells the core destroys (a cracking shell's roof over them drops).
        let mut destroyed = std::collections::HashSet::new();
        let mut changes = TickChanges::default();
        changes.set(&mut self.world, at, M.air);
        // The shell's breakable cells (cell, state, centre, distance, offset), judged below.
        let mut shell = Vec::new();
        // Out to the loosen radius: past `radius_m` the pieces get no impulse (`fling_speed`'s
        // share is nothing there), so they slump and fall.
        for o in cells_within(e.loosen_radius_m.max(e.radius_m))
            .into_iter()
            .skip(1)
        {
            let c = [at[0] + o[0], at[1] + o[1], at[2] + o[2]];
            let m = self.world.get_voxel(c[0], c[1], c[2]);
            if m == M.air {
                continue;
            }
            let p = crate::felling::cell_centre(c);
            let d = (p - centre).length();
            let s = info(m);
            if s.liquid {
                if d <= e.water_radius_m {
                    changes.set(&mut self.world, c, M.air);
                }
                continue;
            }
            if let Some((i, lit)) = explosive_of(&self.fuses.table, m) {
                // Caught in the blast: lit, or its fuse cut short.
                let delay =
                    dwell_world::explosives::chain_delay(&self.fuses.table[i], c, self.tick);
                if !lit && let Some(l) = dwell_world::explosives::lit_state(&self.fuses.table[i]) {
                    changes.set(&mut self.world, c, l);
                }
                self.fuses.light(c, i, delay);
                continue;
            }
            if s.indestructible || wet {
                continue;
            }
            // Water between the charge and this cell shields it (`behind_water`).
            if e.water_shields
                && dwell_world::explosives::behind_water(o, |w| water_before.contains(&w))
            {
                continue;
            }
            if destroys(&e, d) {
                let left = if s.flooded && d > e.water_radius_m {
                    M.water
                } else {
                    M.air
                };
                changes.set(&mut self.world, c, left);
                destroyed.insert(c);
            } else {
                shell.push((c, m, p, d, o));
            }
        }
        // A cracking shell's exposed cells (they touched open space before the blast, or sit on
        // the cleared core) fly whatever lies in their way, with the blast's push.
        let mut exposed = vec![false; shell.len()];
        if crack {
            let f = FaceFrame::at_row(at[1]);
            let (here, below) = (f.cell([0, 0, 0]), f.cell([0, -1, 0]));
            let down: [i32; 3] = std::array::from_fn(|k| below[k] - here[k]);
            let cells: Vec<[i32; 3]> = shell.iter().map(|s| s.0).collect();
            exposed = cracking(
                &cells,
                down,
                e.crack_over_cleared,
                e.crack_vertical_only,
                |c| open_before.contains(&c),
                |c| destroyed.contains(&c),
            );
        }
        // Each shell cell's push: its line from the blast turned toward the way out. Those whose
        // push meets rock (the core cleared, the cells that fly not rock) stay as terrain.
        let dirs: Vec<[f64; 3]> = shell
            .iter()
            .map(|&(_, _, p, _, _)| {
                piece_dir(
                    radial_dir(centre_a, p.to_array()),
                    open,
                    e.open_blend,
                    e.open_spread_min,
                )
            })
            .collect();
        let judged: Vec<([i32; 3], [f64; 3], bool)> = shell
            .iter()
            .zip(&dirs)
            .map(|(&(_, _, _, d, o), &dir)| (o, dir, d > e.radius_m))
            .collect();
        // A cracking shell frees only its exposed cells, judged on the world before the blast
        // (owner, 2026-11-07: cells beside the destroyed core "aren't supposed to go live"; "the air
        // check is supposed to happen before"): nothing flies out through the cleared core or behind
        // other flying cells. Any other shell is wall-checked (`launching`).
        let flies: Vec<bool> = if crack {
            exposed.clone()
        } else {
            launching(&judged, e.open_path_m, |o| solid_at(&mut self.world, o))
        };
        let mut flung = Vec::new();
        for ((&(c, m, p, d, _), &dir), fly) in shell.iter().zip(&dirs).zip(flies) {
            if !fly || flung.len() >= self.fuses.max_pieces {
                continue;
            }
            let left = if info(m).flooded && d > e.water_radius_m {
                M.water
            } else {
                M.air
            };
            changes.set(&mut self.world, c, left);
            flung.push((c, m, p, d, dir));
        }
        let cleared = changes.cells();
        self.broadcast_changes(changes, VoxelModificationReason::Explosion);
        // The water round it kicked outward (docs/EXPLOSIVES.md §3.5), after the cells it cleared,
        // in the same batch: the grid's own law carries the wave to the banks and back.
        if e.wave_impulse > 0.0 && e.wave_reach_m > 0.0 {
            self.water_link
                .pending
                .push(WaterIn::Wave(crate::water::Wave {
                    at: centre_a,
                    reach: e.wave_reach_m,
                    speed: e.wave_impulse,
                    free: e.wave_free_m,
                }));
        }
        // Pieces already moving within the destroy radius are destroyed, as blocks there are
        // (owner, 2026-11-09: chained dynamite left "a ton of physics blocks that are affected by
        // multiple explosion forces"); a piece is judged by its body's centre, a queued one by its
        // cell's.
        let doomed: Vec<u32> = self
            .felling
            .pieces
            .iter()
            .filter(|p| (self.physics.position(p.body) - centre).length() <= e.destroy_radius_m)
            .map(|p| p.id)
            .collect();
        for id in doomed {
            if self.felling.destroy_piece(&mut self.physics, id) {
                self.broadcast_world(&Message::EntityDespawn(id));
            }
        }
        self.fuses.queued.retain(|q| {
            (crate::felling::cell_centre(q.cell) - centre).length() > e.destroy_radius_m
        });
        // Bodies already moving are pushed by the same falloff (their mass resists), and the
        // earlier blasts' pieces still waiting to spawn take the push in their velocity.
        for k in 0..self.felling.pieces.len() {
            let body = self.felling.pieces[k].body;
            let p = self.physics.position(body);
            let sh = share((p - centre).length(), e.reach_m, e.falloff);
            if sh <= 0.0 {
                continue;
            }
            let dir = DVec3::from(push_dir(centre_a, p.to_array(), e.lift));
            let dv = (e.impulse * sh / self.physics.mass(body).max(1.0)).min(e.max_speed_ms);
            let v = self.physics.velocity(body);
            self.physics.set_velocity(body, v + dir * dv);
        }
        for q in &mut self.fuses.queued {
            let p = crate::felling::cell_centre(q.cell);
            let sh = share((p - centre).length(), e.reach_m, e.falloff);
            if sh <= 0.0 {
                continue;
            }
            let dir = DVec3::from(push_dir(centre_a, p.to_array(), e.lift));
            let mass = (f64::from(info(q.state).density_kg_m3) * dwell_world::CELL_H).max(1.0);
            q.velocity += dir * (e.impulse * sh / mass).min(e.max_speed_ms);
        }
        // This blast's pieces wait their turn (`limits.piecesPerTick`, docs/EXPLOSIVES.md §3.12):
        // their cells are already cleared; the nearest spawn first, this tick's share in this
        // tick's physics step (`spawn_queued`, after the blasts).
        let apart_ticks = if e.no_piece_contact_s > 0.0 {
            crate::blasts::ticks_of(e.no_piece_contact_s)
        } else {
            0
        };
        for &(c, m, _, d, dir) in &flung {
            self.fuses.queued.push_back(crate::blasts::Queued {
                cell: c,
                state: m,
                velocity: DVec3::from(dir) * fling_speed(&e, m, d),
                apart_ticks,
            });
        }
        // Players: knocked back and hurt by their distance from it (to the nearest point of the
        // body's capsule).
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        let radius = f64::from(self.config.player_config.body.radius);
        for id in ids {
            let s = &self.sessions[&id];
            let Some(h) = s.handle.filter(|_| s.joined()) else {
                continue;
            };
            let player_id = s.player_id;
            let c = self.players.position(&self.physics, h);
            let half = (f64::from(self.players.half_height(h)) - radius).max(0.0);
            let y = centre.y.clamp(c.y - half, c.y + half);
            let nearest = DVec3::new(c.x, y, c.z);
            let d = ((nearest - centre).length() - radius).max(0.0);
            let sh = share(d, e.reach_m, e.falloff);
            if sh <= 0.0 {
                continue;
            }
            let dir = push_dir(centre_a, c.to_array(), e.lift);
            let dv = Vec3::new(dir[0] as f32, dir[1] as f32, dir[2] as f32)
                * (e.knockback_ms * sh) as f32;
            self.knockback(player_id, dv);
            // Hurt only within the destroy radius (none past it).
            self.damage(
                id,
                blast_damage(&e, d).round() as i32,
                DamageCause::Explosion,
            );
        }
        // What the blast left without support falls (a trunk cut through, sand undermined).
        self.fell(&cleared, 0, Some(centre));
    }

    /// Spawns the blasts' waiting pieces, at most `limits.piecesPerTick` a tick (docs/EXPLOSIVES.md
    /// §3.12), each at its cleared cell with its velocity.
    fn spawn_queued(&mut self) {
        let n = self.fuses.pieces_per_tick.min(self.fuses.queued.len());
        let ready: Vec<crate::blasts::Queued> = self.fuses.queued.drain(..n).collect();
        for q in ready {
            let i = self.felling.spawn_body(
                &mut self.physics,
                None,
                0,
                0,
                vec![([0, 0, 0], q.state)],
                crate::felling::cell_centre(q.cell),
                dwell_physics::DQuat::IDENTITY,
                q.velocity,
                DVec3::ZERO,
            );
            if q.apart_ticks > 0 {
                // Its pieces fly out packed: kept apart from one another for the window, they
                // touch only the terrain, players and other bodies (docs/EXPLOSIVES.md §3).
                let body = self.felling.pieces[i].body;
                self.physics.set_apart(body, true);
                self.fuses.apart.push((body, q.apart_ticks));
            }
            let spawn = self.spawn_message(i);
            self.broadcast_world(&spawn);
        }
    }

    /// Sets the most blast pieces spawned a tick (tests and measurements; `limits.piecesPerTick`).
    pub fn set_pieces_per_tick(&mut self, n: usize) {
        self.fuses.pieces_per_tick = n.max(1);
    }

    /// The blasts' pieces waiting to spawn (tests).
    pub fn queued_pieces(&self) -> usize {
        self.fuses.queued.len()
    }

    /// Counts down the blasts' pieces kept apart (`noPieceContactS`): past its window each lets
    /// the other pieces touch it again once it overlaps none of them, so pieces that landed in
    /// one another don't pop apart; a piece gone (settled, split) is dropped.
    fn rejoin_apart(&mut self) {
        if self.fuses.apart.is_empty() {
            return;
        }
        let live: std::collections::HashSet<dwell_physics::BodyHandle> =
            self.felling.pieces.iter().map(|p| p.body).collect();
        let physics = &mut self.physics;
        self.fuses.apart.retain_mut(|(body, ticks)| {
            if !live.contains(body) {
                return false;
            }
            if *ticks > 0 {
                *ticks -= 1;
                return true;
            }
            if physics.overlaps_pieces(*body) {
                return true;
            }
            physics.set_apart(*body, false);
            false
        });
    }

    /// Lights the explosive `explosive` of the table at `cell` (already its lit state) with a fuse
    /// of `seconds` (tests and tools).
    pub fn light_fuse(&mut self, cell: [i32; 3], explosive: usize, seconds: f64) {
        self.fuses.light(cell, explosive, seconds);
    }

    /// The blasts' pieces still kept apart from one another (tests).
    pub fn apart_pieces(&self) -> usize {
        self.fuses.apart.len()
    }

    /// The explosives' table the server blasts by (tests: a fixture table by data alone).
    pub fn set_explosives(&mut self, table: Vec<dwell_world::explosives::ExplosiveData>) {
        self.fuses.table = table;
    }

    /// The fuses burning: each one's cell and ticks left.
    pub fn fuses(&self) -> Vec<([i32; 3], u32)> {
        self.fuses
            .burning
            .iter()
            .map(|f| (f.cell, f.ticks))
            .collect()
    }

    /// Adds a moving body of `kind` from `cells` (offsets in its grid), its reference cell's centre
    /// at `position`, unturned and still, as a placed build platform starts one (tests and tools;
    /// docs/BUILT_BODIES.md). Every client hears its spawn. Its id.
    pub fn add_body(
        &mut self,
        kind: u8,
        cells: Vec<([i32; 3], dwell_blocks::StateId)>,
        position: [f64; 3],
    ) -> u32 {
        let i = self.felling.spawn_body(
            &mut self.physics,
            None,
            kind,
            0,
            cells,
            DVec3::from(position),
            dwell_physics::DQuat::IDENTITY,
            DVec3::ZERO,
            DVec3::ZERO,
        );
        let m = self.spawn_message(i);
        self.broadcast_world(&m);
        self.felling.pieces[i].id
    }

    /// A body's pose now (its reference cell's centre and rotation), if it is still one.
    /// Body `id`'s physics handle (tests).
    pub fn body_handle(&self, id: u32) -> Option<dwell_physics::BodyHandle> {
        self.felling
            .pieces
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.body)
    }

    pub fn body_pose(&self, id: u32) -> Option<(DVec3, dwell_physics::DQuat)> {
        let p = self.felling.pieces.iter().find(|p| p.id == id)?;
        Some((self.physics.position(p.body), self.physics.rotation(p.body)))
    }

    /// Piece `i`'s `EntitySpawn`: its id, pose now, kind and cells.
    fn spawn_message(&self, i: usize) -> Message {
        let p = &self.felling.pieces[i];
        let origin = self.physics.position(p.body);
        let q = self.physics.rotation(p.body);
        Message::EntitySpawn(EntitySpawn {
            id: p.id,
            origin: origin.to_array(),
            rotation: [q.x as f32, q.y as f32, q.z as f32, q.w as f32],
            kind: p.kind,
            carriers: p.carriers as u16,
            cells: p
                .cells
                .iter()
                .map(|&(o, state)| PieceCellNet {
                    offset: o.map(|v| v as i16),
                    state,
                })
                .collect(),
        })
    }

    /// The body edits queued this tick (docs/BUILT_BODIES.md §4), each paid from its player's edit
    /// rate as a block edit is, within reach of their eye at the body's pose now: a break takes the
    /// cell (`Felling::break_cell`), and every client hears `BodyEdit` (the cells gone, a split's
    /// too), `EntitySpawn` for each part split off, or `EntityDespawn` for a body left empty.
    /// A place joins the body (a built one), clear of every player.
    fn apply_body_edits(&mut self) {
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        let reach = f64::from(players::REACH_DISTANCE) + 1.0;
        for id in ids {
            let Some(s) = self.sessions.get(&id) else {
                continue;
            };
            if !s.joined() || s.body_edits.is_empty() {
                continue;
            }
            let (may_edit, handle) = (self.may_edit(s), s.handle);
            let eye = handle.map(|h| DVec3::from(self.eye_of(h)));
            let edits =
                std::mem::take(&mut self.sessions.get_mut(&id).expect("session").body_edits);
            for e in edits {
                let s = self.sessions.get_mut(&id).expect("session");
                let Some(eye) = eye.filter(|_| may_edit && s.edit_credit >= 1.0) else {
                    continue;
                };
                let cell = e.cell.map(i32::from);
                if e.action == BlockEditAction::Place {
                    // A block placed against a body's cell joins it (a built body; its kind's
                    // `place`), clear of every player.
                    if !dwell_world::edit::placeable(e.material) {
                        continue;
                    }
                    let Some(p) = self.felling.pieces.iter().find(|p| p.id == e.body) else {
                        continue;
                    };
                    let at = self.physics.position(p.body)
                        + self.physics.rotation(p.body) * crate::felling::cell_offset_m(cell);
                    if (at - eye).length() > reach {
                        continue;
                    }
                    let radius = f64::from(self.config.player_config.body.radius);
                    let bodies: Vec<(DVec3, f64)> = self
                        .sessions
                        .values()
                        .filter_map(|s| s.handle)
                        .map(|h| {
                            let half = f64::from(self.players.half_height(h)) - radius;
                            (self.players.position(&self.physics, h), half)
                        })
                        .collect();
                    let taken = |w: DVec3| {
                        bodies.iter().any(|&(c, half)| {
                            let y = (w.y - c.y).clamp(-half, half);
                            (w - (c + DVec3::new(0.0, y, 0.0))).length() < radius + 0.45
                        })
                    };
                    let cost = self.uses_items().then(|| items::cost_of(e.material));
                    if let Some(cost) = &cost
                        && self.sessions[&id].inventory.count_of(cost) == 0
                    {
                        continue;
                    }
                    let Some(new) = self.felling.place_cell(
                        &mut self.physics,
                        e.body,
                        cell,
                        e.face,
                        e.material,
                        &taken,
                    ) else {
                        continue;
                    };
                    if let Some(s) = self.sessions.get_mut(&id) {
                        s.edit_credit -= 1.0;
                        if let Some(cost) = &cost {
                            s.inventory.take_one(cost);
                            s.inventory_dirty = true;
                        }
                    }
                    self.broadcast_world(&Message::BodyEdit(BodyEdit {
                        body: e.body,
                        cells: vec![PieceCellNet {
                            offset: new.map(|v| v as i16),
                            state: e.material,
                        }],
                    }));
                    continue;
                }
                if e.action != BlockEditAction::Break {
                    continue;
                }
                let Some(p) = self.felling.pieces.iter().find(|p| p.id == e.body) else {
                    continue;
                };
                let at = self.physics.position(p.body)
                    + self.physics.rotation(p.body) * crate::felling::cell_offset_m(cell);
                if (at - eye).length() > reach {
                    continue;
                }
                let was = p
                    .cells
                    .iter()
                    .find(|&&(o, _)| o == cell)
                    .map_or(0, |&(_, m)| m);
                let Some(out) = self.felling.break_cell(&mut self.physics, e.body, cell) else {
                    continue;
                };
                if self.uses_items() {
                    self.drop_block(was, at.to_array());
                }
                if let Some(s) = self.sessions.get_mut(&id) {
                    s.edit_credit -= 1.0;
                }
                let edit = Message::BodyEdit(BodyEdit {
                    body: e.body,
                    cells: out
                        .removed
                        .iter()
                        .map(|&o| PieceCellNet {
                            offset: o.map(|v| v as i16),
                            state: 0,
                        })
                        .collect(),
                });
                self.broadcast_world(&edit);
                for &i in &out.spawned {
                    let m = self.spawn_message(i);
                    self.broadcast_world(&m);
                }
                if out.gone {
                    self.broadcast_world(&Message::EntityDespawn(e.body));
                }
            }
        }
    }

    /// The pieces at rest (or out of time) become blocks again (docs/FELLING.md §4): their cells
    /// in one `Rebake` modification, each piece's `EntityDespawn` after it, then what they placed
    /// passes through the support rule again.
    /// The players the falling pieces hit hard this tick are hurt (`DamageCause::Crush`).
    fn piece_hits(&mut self) {
        if self.felling.pieces.is_empty() {
            return;
        }
        let radius = self.config.player_config.body.radius;
        let bodies: Vec<crate::felling::PlayerBody> = self
            .sessions
            .iter()
            .filter_map(|(&id, s)| Some((id, s.handle?)))
            .map(|(id, h)| crate::felling::PlayerBody {
                key: u64::from(id),
                center: self.players.position(&self.physics, h),
                radius: f64::from(radius),
                half_cylinder: f64::from(self.players.half_height(h) - radius),
                velocity: self.players.velocity(&self.physics, h).as_dvec3(),
            })
            .collect();
        for (key, points) in self.felling.hits(&self.physics, &bodies) {
            self.damage(key as SessionId, points, DamageCause::Crush);
        }
    }

    fn settle_pieces(&mut self) {
        let due = self.felling.step(&mut self.physics);
        if due.is_empty() {
            return;
        }
        let capsules = self.player_capsules();
        let mut rebake = TickChanges::default();
        let mut gone = Vec::new();
        let mut placed: Vec<([i32; 3], u8)> = Vec::new();
        for i in due {
            // A cell is taken by a block there, or by a player for a block that blocks a body (a
            // leaf may land where a player stands: players pass through leaves).
            let taken = |world: &mut VoxelWorld, c: [i32; 3], placing: dwell_blocks::StateId| {
                let m = world.get_voxel(c[0], c[1], c[2]);
                let s = info(m);
                !(m == M.air || s.liquid || s.replaceable) || overlaps_player(placing, c, &capsules)
            };
            let settled = self
                .felling
                .settle(&mut self.world, &mut self.physics, i, &taken);
            for &(c, m) in &settled.cells {
                rebake.set(&mut self.world, c, m);
                placed.push((c, settled.settles));
            }
            gone.push(settled.id);
        }
        self.broadcast_changes(rebake, VoxelModificationReason::Rebake);
        for id in gone {
            self.broadcast_world(&Message::EntityDespawn(id));
        }
        // What settled may itself stand on nothing (a trunk resting on leaves): judged again,
        // counting the settles so a piece that never finds an anchor stays put.
        let settles = placed.iter().map(|&(_, n)| n).max().unwrap_or(0);
        let cells: Vec<[i32; 3]> = placed.into_iter().map(|(c, _)| c).collect();
        self.fell(&cells, settles, None);
    }

    /// Every `tickEvery` ticks, water's tick (docs/WATER_SURFACE_GRID.md): its time is game time (the
    /// day/night clock's day against a real one) × the world's water speed; its writes go out as
    /// one modification.
    fn flow_water(&mut self) {
        let every = crate::water::FLOW.tick_every;
        if !self.tick.is_multiple_of(every) {
            return;
        }
        let game = 86_400.0 / self.settings.clock.day_length_s();
        let dt = f64::from(every) / f64::from(SIM_HZ) * game * f64::from(self.settings.water_speed);
        let players: Vec<[f64; 3]> = self
            .sessions
            .values()
            .filter_map(|s| s.handle)
            .map(|h| self.players.position(&self.physics, h).to_array())
            .collect();
        // The world's chunks as the water's copy should have them, then the run.
        let changes = self.water_link.mirror.sync(&self.world);
        let link = &mut self.water_link;
        for ch in changes {
            link.pending.push(match ch {
                crate::water::cells::Change::Load { at, cells, fresh } => {
                    WaterIn::Load { at, cells, fresh }
                }
                crate::water::cells::Change::Unload(at) => WaterIn::Unload(at),
            });
        }
        link.pending.push(WaterIn::Players(players));
        link.pending.push(WaterIn::Run(dt));
    }

    /// Sends the water what it was told this tick, as one batch.
    fn flush_water(&mut self) {
        let items = std::mem::take(&mut self.water_link.pending);
        self.water.send(items);
    }

    /// What the water sent back: its writes (a write the world has since overtaken, its cell made
    /// anything but open or water, is dropped: the edit that did it is on its way to the water)
    /// as one modification, its currents, the chunks it wants loaded, its record and reports.
    fn apply_water(&mut self) {
        let mut changes = TickChanges {
            water: true,
            ..TickChanges::default()
        };
        for out in self.water.poll() {
            match out {
                WaterOut::Ran {
                    writes,
                    currents,
                    wanted,
                    ..
                } => {
                    for w in writes {
                        let Some(m) = self.water_write_stands(&w) else {
                            self.water_link.overtaken += 1;
                            continue;
                        };
                        changes.set(&mut self.world, w.cell, m);
                    }
                    for (key, c) in currents {
                        self.world.currents.set(key, c);
                    }
                    self.water_link.wanted = wanted;
                }
                WaterOut::Record(b) => {
                    self.water_link.record = Some(b);
                    self.water_link.record_asked = false;
                }
                WaterOut::Report(r) => self.water_link.reports.push_back(r),
                WaterOut::Restored(None) => eprintln!(
                    "dwell: the world's saved water can't be read; it is found again by its edits"
                ),
                WaterOut::Restored(Some(_)) => {}
            }
        }
        if !changes.chunks.is_empty() {
            self.broadcast_changes(changes, VoxelModificationReason::Rebake);
        }
    }

    /// The state a water write leaves, if it still stands: its chunk loaded and the cell open or
    /// water (or, for a ramp's flooding, still a block holding water: `Write::apply_to`).
    fn water_write_stands(&self, w: &crate::water::Write) -> Option<dwell_blocks::StateId> {
        let c = w.cell;
        self.world.find(chunk_of(c[0], c[1], c[2]))?;
        w.apply_to(self.world.get_voxel_if_loaded(c))
    }

    /// The world's settings (its clock, its water speed, its game mode).
    /// The world's settings as sent to players: the saved fields, with the worldgen config's hash
    /// for a world made from one (ADR 0074 B7; a default world's are the saved fields unchanged).
    fn join_settings(&self) -> SettingsFields {
        let hash = (!self.worldgen_text.is_empty())
            .then(|| dwell_worldgen::config::worldgen_hash(&self.worldgen_text));
        self.settings_fields.with_worldgen(hash)
    }

    /// The world's worldgen config text (ADR 0074 B7): `""` for the build's ground.
    pub fn worldgen_text(&self) -> &str {
        &self.worldgen_text
    }

    /// The world's disc and build volume (its size's, BIG_MAP.md M2).
    pub fn bounds(&self) -> dwell_world::WorldBounds {
        dwell_world::WorldBounds::of(self.config.world_size)
    }

    pub fn world_settings(&self) -> WorldSettings {
        self.settings
    }

    /// The host changes the world's game mode (ADR 0065; a name not in `shared/modes/modes.json`
    /// is refused: false). It applies at once to every player in the world: the server's checks
    /// (damage, flight, brush strokes) read it, the joined clients get the world's settings again
    /// (older ones ignore them after join), and the world saves it with its next save.
    pub fn set_mode(&mut self, name: &str) -> bool {
        let Some(m) = dwell_protocol::modes::mode(name) else {
            return false;
        };
        if m.name == self.settings.mode {
            return true;
        }
        self.settings.mode = m.name;
        self.settings_fields = self.settings_fields.with(self.settings);
        // Flight taken away: nobody flies on (their queued inputs too), and whoever was in the air
        // lands unhurt (`flight_grace`). The inventory is the player's either way (ADR 0066): a
        // creative spell neither uses nor drops it, so switching back finds it as it was.
        if !m.allows(Ability::Flight) {
            for s in self.sessions.values_mut() {
                s.last_input.fly = false;
                for (_, input) in &mut s.inputs {
                    input.fly = false;
                }
                if let Some(h) = s.handle {
                    let c = self.players.controller(h);
                    s.flight_grace |= c.fly.flying || !c.ground.grounded;
                }
            }
        }
        // Whether the mode uses items is in each `Inventory` (ADR 0066).
        for s in self.sessions.values_mut() {
            s.inventory_dirty = true;
        }
        let bytes = encode(&Message::WorldSettings(self.join_settings()));
        for (&id, s) in &self.sessions {
            if s.joined() {
                self.outbox.push(Outgoing {
                    session: id,
                    kind: OutKind::Reliable(Channel::Control),
                    bytes: bytes.clone(),
                });
            }
        }
        true
    }

    /// The last block changes not the water's own, oldest first, as JSON (a diagnostic for the
    /// block dump, docs/DEV_TOOLS.md): each one's tick (the server's, 60 a second), cell, the
    /// state it was and became (registry names) and why.
    pub fn edit_log_json(&self) -> String {
        let name = |m: dwell_blocks::StateId| {
            dwell_blocks::STATES
                .get(usize::from(m))
                .map_or_else(|| format!("#{m}"), |s| s.name.to_string())
        };
        let rows: Vec<String> = self
            .edit_log
            .iter()
            .map(|e| {
                format!(
                    "{{\"tick\":{},\"cell\":[{},{},{}],\"from\":{:?},\"to\":{:?},\"reason\":\"{:?}\"}}",
                    e.tick, e.cell[0], e.cell[1], e.cell[2], name(e.from), name(e.to), e.reason
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// The water, for tests and tools (docs/WATER_SURFACE_GRID.md), where it runs inline.
    pub fn water(&self) -> &crate::water::Water {
        self.water
            .water()
            .expect("the water runs inline (Server::water): its own thread's isn't here")
    }

    /// The water's writes come back and not yet applied (applied at the next tick's start; tests).
    pub fn pending_water_writes(&mut self) -> Vec<crate::water::Write> {
        self.water.pending_writes()
    }

    /// Puts the water on `host` (tests: a host slower than any tick). What the old one held is
    /// dropped; the new one's copy of the world is sent whole at the next water tick.
    pub fn set_water_host(&mut self, host: crate::water::host::WaterHost) {
        self.water = host;
        self.water_link.mirror = Default::default();
    }

    /// Where the water runs (ADR 0062).
    pub fn water_placement(&self) -> crate::water::host::Placement {
        self.water.placement()
    }

    /// The water host's figures (its thread's run time, the time it owes, the batches not yet
    /// answered: docs/DEV_TOOLS.md) and the writes dropped as overtaken.
    pub fn water_stats(&self) -> (crate::water::host::HostStats, u64) {
        (self.water.stats(), self.water_link.overtaken)
    }

    /// Waits (at most `timeout`) until the water has answered all it was sent, then applies its
    /// answers (tests; closing a world). False if it hadn't by then (a remote host never waits).
    pub fn wait_water(&mut self, timeout: Duration) -> bool {
        self.flush_water();
        let done = self.water.wait(timeout);
        self.apply_water();
        done
    }

    /// The water near `at` (`Water::report`): at once where the water runs inline or on a native
    /// thread (waiting a second at most); a remote host's comes later (`take_water_report`).
    pub fn water_report(&mut self, at: [i32; 3], radius: i32) -> Option<String> {
        if let Some(w) = self.water.water() {
            return Some(w.report(at, radius));
        }
        self.water_link.pending.push(WaterIn::Report { at, radius });
        if self.water.placement() == crate::water::host::Placement::Remote {
            self.flush_water();
            return None;
        }
        self.wait_water(WATER_CLOSE_WAIT);
        self.take_water_report()
    }

    /// A report the water sent back, oldest first.
    pub fn take_water_report(&mut self) -> Option<String> {
        self.apply_water();
        self.water_link.reports.pop_front()
    }

    /// Asks the water for its record now (a remote host's embedder then carries the answer back
    /// before the world is saved: closing a world in the browser).
    pub fn request_water_record(&mut self) {
        if !self.water_link.record_asked {
            self.water_link.pending.push(WaterIn::Record);
            self.water_link.record_asked = true;
        }
        self.flush_water();
    }

    /// Whether the record asked for (`request_water_record`) has come, applying what came.
    pub fn water_record_ready(&mut self) -> bool {
        self.apply_water();
        !self.water_link.record_asked
    }

    /// The water host's next batch, for a remote host's embedder to carry to the water worker.
    pub fn take_water_outgoing(&mut self) -> Option<Vec<u8>> {
        self.water.take_outgoing()
    }

    /// An answer from the water worker (a remote host's embedder); applied at the next tick's
    /// start. False if it can't be read.
    pub fn deliver_water(&mut self, bytes: &[u8]) -> bool {
        self.water.deliver(bytes)
    }

    /// The players' capsules (cells overlapping one are taken).
    fn player_capsules(&self) -> Vec<EditCapsule> {
        self.sessions
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
            .collect()
    }

    /// Each session's `EditAck` for the edits that arrived since its last (all of them are
    /// processed by now), after the tick's `VoxelModification` on the same channel, so a client
    /// has the changes of the edits it is told are done (edit prediction, §6.5). A long list of
    /// refusals goes in several acks.
    fn send_edit_acks(&mut self) {
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.edit_seq > s.edit_acked)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let s = self.sessions.get_mut(&id).expect("session");
            // Through the edit before the first still held (EDIT_HOLD_TICKS); refusals after it
            // wait for that ack.
            let through = s
                .edits
                .iter()
                .map(|e| e.0)
                .chain(s.region_edits.iter().map(|e| e.0))
                .min()
                .map_or(s.edit_seq, |first| first - 1);
            if through <= s.edit_acked {
                continue;
            }
            let (mut refused, later): (Vec<u32>, Vec<u32>) = std::mem::take(&mut s.edit_refused)
                .into_iter()
                .partition(|&r| r <= through);
            s.edit_refused = later;
            refused.sort_unstable();
            s.edit_acked = through;
            let mut acks: Vec<EditAck> = refused
                .chunks(limits::MAX_EDIT_ACK_REJECTED)
                .map(|part| EditAck {
                    through: *part.last().expect("a chunk"),
                    rejected: part.to_vec(),
                })
                .collect();
            match acks.last_mut() {
                Some(last) => last.through = through,
                None => acks.push(EditAck {
                    through,
                    rejected: Vec::new(),
                }),
            }
            for a in acks {
                self.send_reliable(id, &Message::EditAck(a), Channel::World);
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
        // The falling pieces' poses (the same for every client).
        let pieces: Vec<PieceState> = self
            .felling
            .pieces
            .iter()
            .map(|p| {
                let q = self.physics.rotation(p.body);
                PieceState {
                    id: p.id,
                    position: self.physics.position(p.body).to_array(),
                    rotation: [q.x as f32, q.y as f32, q.z as f32, q.w as f32],
                    velocity: self.physics.velocity(p.body).to_array().map(|v| v as f32),
                    angular: self
                        .physics
                        .angular_velocity(p.body)
                        .to_array()
                        .map(|v| v as f32),
                }
            })
            .collect();
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
                held: s.held,
                swing: s.swing,
                swing_kind: s.swing_kind as u8,
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
                pieces: Vec::new(),
                swing_kinds: s.swing_hello,
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
            // The falling pieces in what room is left: chosen by the player's ledger below
            // (docs/FELLING.md §3.1).
            let used = encode(&Message::PhysicsSnapshot(snap.clone())).len();
            let room = MAX_DATAGRAM_BYTES.saturating_sub(used) / PIECE_STATE_BYTES;
            out.push((id, snap, room, l.position));
        }
        for (id, mut snap, room, at) in out {
            if let Some(s) = self.sessions.get_mut(&id) {
                snap.pieces = s.piece_book.choose(&pieces, at, room);
            }
            self.send_datagram(id, &Message::PhysicsSnapshot(snap));
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
            if !dwell_world::lod::in_world_of(c, &self.bounds()) {
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
    /// `is_air` without computing a sky floor on the tick: None while chunk `c`'s column's is not
    /// known (it is wanted, `sky_columns.rs`).
    fn air_known(&mut self, c: ChunkCoord) -> Option<bool> {
        if self.is_saved(c) {
            return Some(false);
        }
        let Some(sky) = &mut self.sky else {
            return Some(self.is_air(c));
        };
        match sky.air().known(c) {
            Ok(air) => Some(air),
            Err(col) => {
                sky.want(col);
                None
            }
        }
    }
    /// Whether `chunk_message(c)` is answered without computing a sky floor on the tick (else the
    /// column is wanted).
    fn air_ready(&mut self, c: ChunkCoord) -> bool {
        self.world.find(c).is_some_and(|ch| ch.revision() > 0) || self.air_known(c).is_some()
    }

    fn init_storage(&mut self) {
        let Some(store) = &self.config.store else {
            return;
        };
        let db = store.db().clone();
        let meta = db.load_meta();
        if let Some(m) = &meta {
            self.tick = m.world_tick;
            // A world saved with settings keeps them (a world from before them takes this build's).
            if let Some(f) = m.settings.as_deref().and_then(SettingsFields::from_bytes) {
                // A world from before modes is creative, and one from before sizes today's size.
                let before = WorldSettings {
                    mode: dwell_protocol::modes::DEFAULT_MODE,
                    size: dwell_protocol::world_size::WorldSize::DEFAULT,
                    moon_phase: None,
                    ..self.settings
                };
                match f.settings(before) {
                    Ok(s) => {
                        self.settings = s;
                        self.settings_fields = f;
                    }
                    Err(e) => eprintln!(
                        "dwell: the world's settings can't be used ({e:?}); this build's instead"
                    ),
                }
            }
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
        // The free water as saved (WATER_SURFACE_GRID.md §2b); held water is found again from the
        // blocks, and an older world's basin record is ignored.
        // (Through the host, before any chunk: `Out::Restored` says if it couldn't be read.)
        if let Some(bytes) = db.water() {
            self.water_link.pending.push(WaterIn::Restore(bytes));
            self.flush_water();
        }
        *self.saved.lock().expect("saved index") = saved;
        if let Some(saved) = db
            .loose_items()
            .and_then(|b| crate::loose_items::LooseItems::from_bytes(&b))
        {
            self.loose.restore(saved);
        }
        // A record this build can't read (a newer one's) is kept: not written over.
        if let Some(b) = db.cuts() {
            match dwell_protocol::chopping::Cuts::from_bytes(&b) {
                Some(c) => self.cuts = c,
                None => self.cuts_unread = true,
            }
        }
        if let Some(b) = db.damage() {
            match dwell_protocol::damage::Damage::from_bytes(&b) {
                Some(d) => self.damage = d,
                None => self.damage_unread = true,
            }
        }
        if let Some(b) = db.lava() {
            match crate::lava::Lava::from_bytes(&b, &dwell_lava::DATA) {
                Some(l) => self.lava = l,
                None => self.lava_unread = true,
            }
        }
        if let Some(b) = db.tree_growth() {
            match dwell_worldgen::growth::Records::from_bytes(&b) {
                Some(g) => self.growth = g,
                None => self.growth_unread = true,
            }
        }
        if let Some(b) = db.ecology() {
            self.ecology.restore(b);
        }
        // Grass growth: the world's own; a world saved before it has none, a new one the config's.
        match db.grass() {
            Some(b) => match grass::Grass::from_bytes(&b) {
                Some(g) => self.grass = g,
                None => {
                    self.grass_unread = true;
                    self.grass.enabled = false;
                }
            },
            None => self.grass.enabled = meta.is_none() && self.config.grass,
        }
        self.share_grass_with_ecology();
        if let Some(b) = db.ground_gone() {
            match dwell_world::ground::GroundGone::from_bytes(&b) {
                Some(g) => self.ground = g,
                None => self.ground_unread = true,
            }
        }
        let bodies = db.bodies();
        self.world.set_saved(Box::new(StoreChunks {
            db,
            saved: self.saved.clone(),
            lit: self.relit.clone(),
        }));
        // The moving bodies saved with it (docs/BUILT_BODIES.md §7), as they were: after the saved
        // chunks are the world's source, so one resting on its build platform is held again.
        self.felling
            .restore(&mut self.world, &mut self.physics, bodies);
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
    /// a store. Hosts call it before shutting down, then `flush_store`. The water's record is a
    /// fresh one: asked for and waited on (a second at most; a remote host's is the newest its
    /// embedder brought back, `request_water_record`).
    pub fn save_now(&mut self) {
        if self.config.store.is_none() {
            return;
        }
        if self.water.placement() != crate::water::host::Placement::Remote {
            self.water_link.record_asked = false;
            self.request_water_record();
            self.wait_water(WATER_CLOSE_WAIT);
        } else {
            self.apply_water();
        }
        self.autosave();
        // Now, as said: an inline store's save is not left to the ticks.
        if let Some(store) = &self.config.store
            && !store.threaded()
        {
            store.flush();
            self.collect_saves();
        }
    }

    /// Saves as `save_now`, with the water's newest record (asked for a moment before).
    fn autosave(&mut self) {
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
                settings: Some(self.settings_fields.to_bytes()),
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
        batch.inventories = std::mem::take(&mut self.departed_inventories);
        for s in self
            .sessions
            .values()
            .filter(|s| s.joined() && !s.inventory_unread)
        {
            batch
                .inventories
                .push((s.public_key, s.inventory.to_bytes()));
        }
        batch.vitals = std::mem::take(&mut self.departed_vitals);
        for s in self.sessions.values().filter(|s| s.joined()) {
            batch.vitals.push((s.public_key, self.vitals_record(s)));
        }
        batch.loose_items = Some(self.loose.to_bytes());
        if !self.cuts_unread {
            batch.cuts = Some(self.cuts.to_bytes());
        }
        if !self.lava_unread && !self.lava.is_empty() {
            batch.lava = Some(self.lava.to_bytes());
        }
        if !self.growth_unread {
            batch.tree_growth = Some(self.growth.to_bytes());
        }
        if !self.ground_unread {
            batch.ground_gone = Some(self.ground.to_bytes());
        }
        batch.ecology = self.ecology.record_to_save();
        if !self.grass_unread {
            batch.grass = Some(self.grass.to_bytes());
        }
        if !self.damage_unread {
            batch.damage = Some(self.damage.to_bytes());
        }
        // The world's moving bodies, all of them each save (docs/BUILT_BODIES.md §7).
        batch.bodies = Some(self.felling.saved(&self.physics));
        // The free water (WATER_SURFACE_GRID.md §2b): the newest record the water sent (none
        // yet: the one saved before stays).
        batch.water = self.water_link.record.take();
        // LOD sections written since the last save; sections still to compute (dirty, held, or
        // computing now, at the server-jobs worker too) are saved dirty, to recompute on load: a
        // page closed before the next save would otherwise keep the old content (BUGS.md #44).
        let mut lod_saved = Vec::new();
        for &c in &self.lod_unsaved {
            let Some(section) = self.lod.find(c) else {
                continue;
            };
            let dirty = self.lod.recomputing(c);
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
                self.ecology.save_failed();
                self.dirty.extend(revisions.into_iter().map(|(c, _)| c)); // try again
            }
        }
    }

    /// Marks a chunk changed outside the edit path (tools, tests that write `world()` directly):
    /// the next save writes it.
    pub fn mark_dirty(&mut self, c: ChunkCoord) {
        self.dirty.insert(c);
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
    /// The next job for the server-jobs worker (`jobs_remote`), as bytes for
    /// `dwell_sim::jobs::JobRunner::run`: a chunk to generate first, else a LOD section.
    pub fn take_job(&mut self) -> Option<Vec<u8>> {
        // Sky floors first: streaming waits for them.
        if let Some(col) = self.sky.as_mut().and_then(|s| s.take_job()) {
            return Some(crate::sky_columns::encode_job(col));
        }
        if let Some(c) = self.worldgen.take_job() {
            return Some(crate::jobs::encode_chunk_job(c));
        }
        self.lod.take_job()
    }
    /// The server-jobs worker's answer to a job; applied at the next tick. False if it can't be
    /// read.
    pub fn deliver_job(&mut self, bytes: &[u8]) -> bool {
        if let Some((c, chunk)) = crate::jobs::decode_chunk_done(bytes) {
            self.worldgen.deliver(c, chunk);
            return true;
        }
        if let Some((col, floor)) = crate::sky_columns::decode_done(bytes) {
            if let Some(sky) = &mut self.sky {
                sky.deliver(col, floor);
            }
            return true;
        }
        self.lod.deliver(bytes)
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
    /// The player id of a joined session.
    pub fn player_of_session(&self, id: SessionId) -> Option<u16> {
        self.sessions
            .get(&id)
            .filter(|s| s.joined())
            .map(|s| s.player_id)
    }

    /// Kills a joined, living player (as a lethal fall would; tools and tests): they respawn after
    /// `RESPAWN_SECONDS`.
    pub fn kill_player(&mut self, player_id: u16) {
        if let Some(id) = self.session_of_player(player_id) {
            self.kill(id, DamageCause::Fall);
        }
    }

    /// Turns the respawn preload (§6.3) on with a radius (at most the view's), or off: for deaths
    /// from now on.
    pub fn set_respawn_preload(&mut self, radius_chunks: Option<i32>) {
        self.config.respawn_preload_radius_chunks = radius_chunks;
        let p = radius_chunks.map_or(-1, |p| p.min(self.config.view_radius_chunks));
        self.preload_offsets = cube_offsets(p.max(0))
            .into_iter()
            .filter(|c| p >= 0 && len_sq(c) <= p * p + p)
            .collect();
    }

    /// The chunk whose ring is preloaded for a dead player's respawn (§6.3), until the respawn.
    pub fn respawn_preload_of(&self, player_id: u16) -> Option<ChunkCoord> {
        self.by_player(player_id).and_then(|s| s.preload_center)
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
    /// The water's currents near each player (`CURRENTS_EVERY`), as `WaterCurrents` datagrams.
    fn send_currents(&mut self) {
        use dwell_world::currents::current_key;
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        for id in ids {
            let s = &self.sessions[&id];
            let Some(h) = s.handle.filter(|_| s.joined()) else {
                continue;
            };
            if s.currents_wait > 0 {
                let s = self.sessions.get_mut(&id).expect("session");
                s.currents_wait = s.currents_wait.saturating_sub(CURRENTS_EVERY);
                continue;
            }
            let p = self.players.position(&self.physics, h);
            let ((fb, cx, cz), _) = current_key(
                FaceFrame::at_height(p.y),
                p.x.floor() as i32,
                p.z.floor() as i32,
            );
            let r = currents_radius();
            let mut near: Vec<(i32, i32)> = (-r..=r)
                .flat_map(|dx| (-r..=r).map(move |dz| (dx, dz)))
                .collect();
            near.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
            let mut out = Vec::new();
            let s = self.sessions.get_mut(&id).expect("session");
            for (dx, dz) in near {
                if out.len() >= CURRENTS_PER_SEND {
                    break;
                }
                let key = (fb, cx + dx, cz + dz);
                let now = self.world.currents.get(key).map(|c| {
                    Box::new(
                        c.map(|[u, v]| [WaterCurrents::quantize(u), WaterCurrents::quantize(v)]),
                    )
                });
                let sent = s.currents_sent.get(&key);
                let cells = match (now, sent) {
                    (Some(q), Some((was, at)))
                        if *was == q && self.tick.wrapping_sub(*at) < CURRENTS_REFRESH =>
                    {
                        continue;
                    }
                    (Some(q), _) => {
                        s.currents_sent.insert(key, (q.clone(), self.tick));
                        q
                    }
                    (None, Some(_)) => {
                        s.currents_sent.remove(&key);
                        Box::new([[0; 2]; 256])
                    }
                    (None, None) => continue,
                };
                out.push(WaterCurrents {
                    face_b: key.0,
                    cx: key.1,
                    cz: key.2,
                    cells,
                });
            }
            // Chunks left behind: the client forgets them (CURRENTS_KEEP_MS).
            s.currents_sent
                .retain(|k, _| k.0 == fb && (k.1 - cx).abs() <= r && (k.2 - cz).abs() <= r);
            for m in out {
                self.send_datagram(id, &Message::WaterCurrents(m));
            }
        }
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

/// Where player `player_id` spawns relative to the spawn point (x, z, m): spread over a 4 × 2 grid
/// by id, so players don't start inside each other.
pub fn spawn_spread(player_id: u16) -> [f64; 2] {
    let slot = (i32::from(player_id) - 1).rem_euclid(8);
    [f64::from(slot % 4) - 1.5, f64::from(slot / 4) * 1.5 - 0.75]
}

impl Server {
    /// A client draws block damage (`DamageHello`): every damaged cell now, then each change.
    fn damage_hello(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        s.damage_hello = true;
        let all: Vec<_> = self
            .damage
            .sorted()
            .into_iter()
            .map(|(c, d)| (c, d.view()))
            .collect();
        for part in all.chunks(dwell_protocol::MAX_DAMAGE_PER_MESSAGE) {
            let m = Message::Damage(dwell_protocol::DamageMessage {
                cells: part.to_vec(),
                gone: Vec::new(),
            });
            self.send_reliable(id, &m, Channel::World);
        }
    }

    /// Tells the clients that draw damage of `cells`' damage now and of the cells `gone`.
    fn send_damage(&mut self, cells: &[[i32; 3]], gone: Vec<[i32; 3]>) {
        let cells: Vec<_> = cells
            .iter()
            .filter_map(|c| self.damage.cells.get(c).map(|d| (*c, d.view())))
            .collect();
        if cells.is_empty() && gone.is_empty() {
            return;
        }
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.damage_hello)
            .map(|(&id, _)| id)
            .collect();
        for part in cells.chunks(dwell_protocol::MAX_DAMAGE_PER_MESSAGE.max(1)) {
            let m = Message::Damage(dwell_protocol::DamageMessage {
                cells: part.to_vec(),
                gone: Vec::new(),
            });
            for &id in &to {
                self.send_reliable(id, &m, Channel::World);
            }
        }
        for part in gone.chunks(dwell_protocol::MAX_DAMAGE_PER_MESSAGE) {
            let m = Message::Damage(dwell_protocol::DamageMessage {
                cells: Vec::new(),
                gone: part.to_vec(),
            });
            for &id in &to {
                self.send_reliable(id, &m, Channel::World);
            }
        }
    }

    /// A cell's damage (tests, the bench).
    pub fn damage_of(&self, cell: [i32; 3]) -> Option<&dwell_protocol::damage::CellDamage> {
        self.damage.cells.get(&cell)
    }
}

/// A tool hit's contact: the tick it lands and the tool it was swung with (WORK_QUEUE item 25).
type Contact = (u32, String);

/// Ticks from now to a contact `s` seconds away, rounded up: never before it.
pub(crate) fn contact_ticks(s: f64) -> u32 {
    (s * f64::from(dwell_protocol::SIM_HZ) - 1e-9)
        .ceil()
        .max(0.0) as u32
}

/// The item a session's break is made with (docs/BLOCK_DAMAGE.md): the held inventory slot's
/// where blocks are not unlimited, else the tool its hotbar holds (`ChopAction::Held`); none: an
/// empty hand.
/// One hit that counts with the held tool (owner, 2026-11-11: tools wear by `durability`): worn
/// where items are used, and where blocks are unlimited only if the data says (`creativeWear`).
pub(crate) fn wear_tool(inventory: &mut inv::Inventory, uses_items: bool) -> inv::Wear {
    if !uses_items && !dwell_protocol::chopping::CREATIVE_WEAR {
        return inv::Wear::None;
    }
    inventory.wear_held(dwell_protocol::chopping::durability)
}

fn held_break_item(s: &Session, uses_items: bool) -> Option<&str> {
    if uses_items {
        s.inventory
            .slots
            .get(usize::from(s.inventory.held))
            .and_then(|h| h.as_ref())
            .map(|st| st.item.as_str())
    } else {
        s.held_tool
            .as_ref()
            .map(|(t, _)| t.as_str())
            .filter(|t| !t.is_empty())
    }
}

/// A place edit's sapling (ADR 0079 S4): `Ok(None)` when it places no species' sapling, `Ok(record)`
/// for one on its species' `plantOn` ground (face-local below the cell), `Err` on other ground.
fn sapling_planted(
    world: &VoxelWorld,
    outcome: &dwell_world::edit::EditOutcome,
    day: f64,
) -> Result<Option<dwell_worldgen::growth::Record>, dwell_worldgen::growth::Refused> {
    let item = dwell_blocks::block_of(outcome.material).name;
    if dwell_worldgen::species::of_sapling(item).is_none() {
        return Ok(None);
    }
    let f = FaceFrame::at_row(outcome.cell[1]);
    let [x, y, z] = f.cell(outcome.cell);
    let ground = dwell_blocks::block_of(world.get_voxel_if_loaded(f.cell([x, y - 1, z]))).name;
    dwell_worldgen::growth::plant(item, outcome.cell, ground, day).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    dwell_world::both_faces!(
        fn the_respawn_centre_is_the_body_centres_chunk_on_either_face(f: FaceFrame) {
            // Feet on a chunk's top row boundary (face-locally): the body's centre, 0.9 m up, is in
            // the chunk above, as the view centres it once the player is there.
            let top = f64::from(CHUNK_SIZE) * dwell_world::CELL_H * 2.0;
            let feet = f.point([5.5, top, 7.5]);
            let c = body_centre_chunk(feet, 0.9);
            let up = f.point([5.5, top + 0.9, 7.5]);
            assert_eq!(c, chunk_at(up[0], up[1], up[2]));
            assert_eq!(c, f.chunk(chunk_at(5.5, top + 0.9, 7.5)));
        }
    );
}
