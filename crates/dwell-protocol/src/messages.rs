//! Protocol messages (ARCHITECTURE.md §8.3). Every message starts with a u8 `MessageType`. Layouts
//! are pinned by the golden vectors in `shared/protocol/vectors.txt`.

use crate::bytes::{ByteReader, ByteWriter, clamp_str};
use crate::{
    AUTH_DOMAIN_TAG, BlockEditAction, ChunkForm, DamageCause, GroundKind, HostState,
    MAX_INPUTS_PER_DATAGRAM, MessageType, PlayerEventKind, PlayerState, RegionEditOp, RejectReason,
    controller_flags, input_buttons, limits, lod, player_flags, region_edit, world,
};

pub type PublicKey = [u8; 32];
pub type Nonce = [u8; 32];
pub type Signature = [u8; 64];
pub type ChunkCoordNet = [i32; 3];

pub const CHUNK_VOLUME: usize = 32 * 32 * 32;
/// Cells of a LOD section with its one-cell apron (§6.6): 34³.
pub const LOD_CELL_COUNT: usize = 34 * 34 * 34;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DatagramPing {
    pub seq: u32,
    pub client_time_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DatagramPong {
    pub seq: u32,
    pub client_time_ms: f64,
    pub server_tick: u32,
}

/// Steps a water current is sent in (m/s) in [`WaterCurrents`]: an i8 each way, ±6.35 m/s.
pub const CURRENT_STEP_MS: f32 = 0.05;

/// A water chunk's currents (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3a), server → client as a
/// datagram, sent again every so often while it has any (a lost one is made good by the next; a
/// client forgets a chunk not sent again within `CURRENTS_KEEP_MS`): the chunk of 16 × 16 columns
/// (`face_b`, `cx`, `cz`), and each column's current along x and z in [`CURRENT_STEP_MS`] steps,
/// column `x + 16 z`; all zero: the chunk's water is still. An optional message: a client that
/// doesn't know it drops it, as any unknown datagram.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterCurrents {
    pub face_b: bool,
    pub cx: i32,
    pub cz: i32,
    pub cells: Box<[[i8; 2]; 256]>,
}

impl Default for WaterCurrents {
    fn default() -> Self {
        Self {
            face_b: false,
            cx: 0,
            cz: 0,
            cells: Box::new([[0; 2]; 256]),
        }
    }
}

/// How long a client keeps a chunk's currents not sent again (ms).
pub const CURRENTS_KEEP_MS: f64 = 8000.0;

impl WaterCurrents {
    /// A current (m/s) in steps, clamped to the range.
    pub fn quantize(v: f32) -> i8 {
        (v / CURRENT_STEP_MS).round().clamp(-127.0, 127.0) as i8
    }
    /// A column's current (m/s along x and z).
    pub fn current(&self, i: usize) -> [f32; 2] {
        self.cells[i].map(|q| f32::from(q) * CURRENT_STEP_MS)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusResponse {
    pub protocol_version: u16,
    pub server_name: String,
    pub motd: String,
    pub players: u16,
    pub max_players: u16,
    pub flags: u8,
}

impl StatusResponse {
    pub const FLAG_ONLINE_MODE: u8 = 1;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientHello {
    pub protocol_version: u16,
    pub client_version: String,
    pub public_key: PublicKey,
    pub display_name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Welcome {
    pub player_id: u16,
    pub world_seed: u64,
    pub generator_version: u32,
    pub server_tick: u32,
    /// The chunk the client generates and hashes for `WorldgenCheck` (§6.3).
    pub verification_chunk: ChunkCoordNet,
    /// `welcome_flags`.
    pub flags: u8,
    /// The definitions hash: FNV-1a 64 of the registry payload the server sent in its
    /// `BlockRegistry` parts (ADR 0038), for logs and the join prompt's "don't ask again".
    pub registry_hash: u64,
}

/// S→C reliable, control channel, before `Welcome` (ADR 0038): part `part` of `parts` of the
/// server's registry payload (`dwell_blocks::sync`), each at most `MAX_REGISTRY_PART_BYTES`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockRegistry {
    pub part: u16,
    pub parts: u16,
    pub data: Vec<u8>,
}

/// The largest `BlockRegistry` part.
pub const MAX_REGISTRY_PART_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Reject {
    pub reason: RejectReason,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ping {
    pub seq: u32,
    pub client_time_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pong {
    pub seq: u32,
    pub client_time_ms: f64,
    pub server_tick: u32,
    pub server_time_ms: f64,
}

/// S→C reliable. Generated: the client generates the chunk. Explicit: the voxels travel as palette
/// + RLE (`voxels` holds `CHUNK_VOLUME` ids in chunk index order). Air: an unmodified all-air chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkData {
    pub form: ChunkForm,
    pub coord: ChunkCoordNet,
    pub revision: u32,
    pub voxels: Vec<u16>,
    /// The grid the chunk is of (its index in `shared/worldgen/grids.json`; 0 the home grid), in
    /// that grid's own space (MOON_WORLD.md §7.2). Optional on the wire, a byte at the end, written
    /// only when not 0: sent only to a client with the `grid.frames` capability.
    pub grid: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChunkUnload {
    pub coords: Vec<ChunkCoordNet>,
    /// The grid the chunks are of (as `ChunkData::grid`; a byte at the end when not 0).
    pub grid: u8,
}

/// The local player's grid in a snapshot (MOON_WORLD.md §7.2, the `grid.frames` capability): the
/// snapshot's `local` position and velocity are in this grid's space, and off home the grid's frame
/// at the snapshot's tick (its origin in the world, its rotation `[x, y, z, w]`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridView {
    pub grid: u8,
    pub origin: [f64; 3],
    pub rotation: [f64; 4],
}

/// C→S: break the targeted cell, or place `material` against its `face` (0 +X, 1 −X, 2 +Y, 3 −Y,
/// 4 +Z, 5 −Z), as a piece `rows` cells tall (the held item's height: 2 a 1 m block, 1 a ½ m one;
/// docs/ITEMS.md). A placed piece goes on from the placed cell away from the face; a broken one
/// into the block from the face. Against a side, up from the cell, or down with `down` (the piece
/// lined up with what is aimed at).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEditRequest {
    pub action: BlockEditAction,
    pub cell: [i32; 3],
    pub face: u8,
    pub material: u16,
    pub rows: u8,
    pub down: bool,
    /// Where a break struck the face (docs/BLOCK_DAMAGE.md): each axis in 1/255ths of the cell,
    /// world-oriented. Optional on the wire (three bytes after the piece), sent only to a server
    /// whose `Inventory` has `inventory_flags::DAMAGE`: an older server reads none.
    pub point: Option<[u8; 3]>,
}

/// The tallest piece a place request may ask for (cells).
pub const MAX_PIECE_ROWS: u8 = 2;

/// C→S: one edit of a box of cells (§6.5) — `size` cells from the `min` corner, each side
/// 1..=`region_edit::MAX_SIDE`. Fill and Hollow set `material` (Hollow: the box's shell, air
/// inside); Replace turns cells holding `from` into `material`; Paste sets the box to `cells`.
/// Structure is a Paste that places a saved structure (STRUCTURES.md §9): the same body, refused by
/// the server where the world's mode lacks the `structures` ability; sent only to a server that
/// announced the `structures.place` capability (others get a plain Paste).
/// Sent only to a server that announced `welcome_flags::REGION_EDITS`.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionEdit {
    pub op: RegionEditOp,
    pub min: [i32; 3],
    pub size: [u8; 3],
    /// Fill, Hollow, Replace.
    pub material: u16,
    /// Replace.
    pub from: u16,
    /// Paste, Structure: `volume()` states in layer order, x fastest, then z, then y
    /// (`x + size.x · (z + size.z · y)`).
    pub cells: Vec<u16>,
}

impl RegionEdit {
    pub fn volume(&self) -> usize {
        self.size.iter().map(|&s| usize::from(s)).product()
    }

    /// The index into `cells` of box-local (x, y, z).
    pub fn cell_index(size: [u8; 3], x: usize, y: usize, z: usize) -> usize {
        x + usize::from(size[0]) * (z + usize::from(size[2]) * y)
    }
}

/// S→C on the world channel, right after the tick's `VoxelModification`: the server has processed
/// this session's edit requests (`BlockEditRequest`s and `RegionEdit`s, numbered together from 1
/// in the order they arrived) up to `through`; `rejected` (ascending, each ≤ `through`, at most
/// `limits::MAX_EDIT_ACK_REJECTED`) are those it refused. The rest were applied, and their changes
/// arrived before this message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditAck {
    pub through: u32,
    pub rejected: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoxelChange {
    /// Chunk-local `x | y << 5 | z << 10`.
    pub index: u16,
    pub material: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkChanges {
    pub coord: ChunkCoordNet,
    /// The chunk's revision after the changes.
    pub revision: u32,
    pub changes: Vec<VoxelChange>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoxelModification {
    pub reason: crate::VoxelModificationReason,
    pub server_tick: u32,
    pub chunks: Vec<ChunkChanges>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodIndexEntry {
    pub i: i32,
    pub j: i32,
    pub k: i32,
    pub revision: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LodIndex {
    pub last: bool,
    pub entries: Vec<LodIndexEntry>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodSectionRequest {
    pub level: u8,
    pub section: [i32; 3],
    /// The revision the client holds (0 = none).
    pub known_revision: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LodData {
    pub form: crate::LodForm,
    pub level: u8,
    pub section: [i32; 3],
    pub revision: u32,
    pub cells: Vec<u16>,
}

/// One tick of quantized input: move ∈ [−127, 127]², buttons (`input_buttons`), yaw as a wrapped
/// fraction of a turn (65536 per 360°), pitch in [−32767, 32767] for ±90°.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputFrame {
    pub seq: u32,
    pub move_x: i8,
    pub move_y: i8,
    pub buttons: u16,
    pub yaw: i16,
    pub pitch: i16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerInput {
    pub last_snapshot_tick: u32,
    pub inputs: Vec<InputFrame>,
}

/// Controller state needed to resume simulation exactly (PLAYER_CONTROLLER.md §8.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerState {
    /// `controller_flags`.
    pub flags: u8,
    pub current_x: f32,
    pub current_z: f32,
    pub external_x: f32,
    pub external_z: f32,
    pub contribution_x: f32,
    pub contribution_z: f32,
    pub accumulated_y: f32,
    pub platform_y: f32,
    pub target_y: f32,
    pub ground_velocity_y: f32,
    pub ground_kind: GroundKind,
    pub ground_id: u16,
    pub buffer_ticks: u8,
    pub coyote_ticks: u8,
    pub step_grace: u8,
    /// `slide_flags` (the sprint slide, protocol v15).
    pub slide_flags: u8,
    /// Ticks until another sprint slide may start (capped at 255).
    pub slide_cooldown: u8,
    pub ladder: [i32; 3],
    pub released: [i32; 2],
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            flags: 0,
            current_x: 0.0,
            current_z: 0.0,
            external_x: 0.0,
            external_z: 0.0,
            contribution_x: 0.0,
            contribution_z: 0.0,
            accumulated_y: 0.0,
            platform_y: 0.0,
            target_y: 0.0,
            ground_velocity_y: 0.0,
            ground_kind: GroundKind::None,
            ground_id: 0,
            buffer_ticks: 0,
            coyote_ticks: 0,
            step_grace: 0,
            slide_flags: 0,
            slide_cooldown: 0,
            ladder: [0; 3],
            released: [0; 2],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalPlayerState {
    /// Capsule centre (pos64).
    pub position: [f64; 3],
    pub velocity: [f32; 3],
    pub flags: u8,
    pub health: u8,
    pub state: PlayerState,
    /// Inputs queued on the server (client clock steering).
    pub input_buffer: u8,
    /// Input seq of the latest knockback applied (0 = none).
    pub last_knockback_seq: u32,
    pub controller: ControllerState,
}

impl Default for LocalPlayerState {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            velocity: [0.0; 3],
            flags: 0,
            health: 0,
            state: PlayerState::Idle,
            input_buffer: 0,
            last_knockback_seq: 0,
            controller: ControllerState::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemotePlayerState {
    pub player_id: u16,
    /// Feet (posfix).
    pub position: [f64; 3],
    /// f16 on the wire.
    pub velocity: [f32; 3],
    pub yaw: i16,
    pub pitch: i16,
    pub state: PlayerState,
    pub flags: u8,
    /// What it holds (`held_item`: a block state, the server's id, and the brush bit; protocol v17).
    pub held: u16,
    /// Its last swing (`swing_flags`: a count and break or place; v17).
    pub swing: u8,
    /// Which swing that is (`SwingKind`'s value: `Plain` is the break or place `swing` says, the
    /// axe's chops by their direction), sent only in a snapshot with `swing_kinds` (to a client
    /// that said `SwingHello`); a value this build doesn't know reads as `Plain`. 0 otherwise.
    pub swing_kind: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhysicsSnapshot {
    pub server_tick: u32,
    /// The last input of this client processed.
    pub ack_input_seq: u32,
    pub local: LocalPlayerState,
    pub remotes: Vec<RemotePlayerState>,
    /// The falling pieces (docs/FELLING.md §3; protocol v20).
    pub pieces: Vec<PieceState>,
    /// It ends with the remotes' swing kinds (`RemotePlayerState::swing_kind`, in their order):
    /// only to a client that said `SwingHello`, so an older one (which refuses trailing bytes)
    /// never gets it; an older server never writes it.
    pub swing_kinds: bool,
    /// The local player's grid (MOON_WORLD.md §7.2), after the swing kinds (which are written,
    /// empty or not, whenever this is): only to a client with the `grid.frames` capability, which
    /// says `SwingHello` too.
    pub grid: Option<GridView>,
}

/// A falling piece's pose and motion in a snapshot: its origin (posfix), its rotation (each
/// component × 32767 as i16), its velocity and spin (f16).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PieceState {
    pub id: u32,
    pub position: [f64; 3],
    /// A unit quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    pub angular: [f32; 3],
}

/// One cell of a falling piece: its offset in cells from the piece's reference cell, and its
/// block state (the server's id).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PieceCellNet {
    pub offset: [i16; 3],
    pub state: u16,
}

/// A piece that began to fall (S→C, world channel, after the `VoxelModification` that cleared its
/// cells; docs/FELLING.md §3): its id, its reference cell's centre (pos64) and rotation, and its
/// cells, carriers first (`carriers` of them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntitySpawn {
    pub id: u32,
    pub origin: [f64; 3],
    pub rotation: [f32; 4],
    /// Its kind: an index into the body kinds (`shared/physics/bodies.json`; 0 natural; protocol
    /// v21, docs/BUILT_BODIES.md §2). Not checked here: an unknown kind reads as natural.
    pub kind: u8,
    pub carriers: u16,
    pub cells: Vec<PieceCellNet>,
}

/// C→S, world channel: break a cell of a moving body, or place a block against one of its cells'
/// faces (protocol v21, docs/BUILT_BODIES.md §4). `cell` is in the body's own grid (offsets from its
/// reference cell, as `EntitySpawn` sends them); `face` as `BlockEditRequest`'s, in the body's frame.
/// Not counted in `EditAck`'s numbering: a body edit isn't predicted.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyEditRequest {
    pub body: u32,
    pub cell: [i16; 3],
    pub face: u8,
    pub action: BlockEditAction,
    /// The placed block's state (Place only).
    pub material: u16,
}

/// S→C, world channel: cells of a body that changed (state 0: removed), in its own grid (protocol
/// v21, docs/BUILT_BODIES.md §4). A split sends this for the cells that left and `EntitySpawn` for
/// each new body.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodyEdit {
    pub body: u32,
    pub cells: Vec<PieceCellNet>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerEvent {
    pub kind: PlayerEventKind,
    pub player_id: u16,
    pub server_tick: u32,
    /// That player's input processed on `server_tick` (for replay).
    pub input_seq: u32,
    /// Knockback: the velocity change.
    pub vector: [f32; 3],
    /// Respawn: feet position (pos64).
    pub position: [f64; 3],
    /// Damage.
    pub amount: u8,
    /// Damage, Death.
    pub cause: DamageCause,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    DatagramPing(DatagramPing),
    /// C→S datagram, after the join (WORK_QUEUE item 17): this client draws others' swings by kind;
    /// end my snapshots with the remotes' swing kinds (`PhysicsSnapshot::swing_kinds`). A datagram
    /// because an older server drops one it can't read (a control message it can't read ends the
    /// session), so the client sends it without asking and repeats it until a snapshot carries
    /// the kinds (or gives up: an older server).
    SwingHello,
    /// C→S datagram, with `ChopAction::Hello` (WORK_QUEUE item 26): this client draws the axe's
    /// bites; end my `Cuts` with them (`CutsMessage::bites`; the server sends them all again on the
    /// first). A datagram for `SwingHello`'s reason: an older server drops it; the client repeats
    /// it until a `Cuts` carries bites (or gives up: an older server).
    BitesHello,
    DatagramPong(DatagramPong),
    WaterCurrents(WaterCurrents),
    StatusRequest,
    StatusResponse(StatusResponse),
    ClientHello(ClientHello),
    Challenge(Nonce),
    ClientAuth(Signature),
    Welcome(Welcome),
    Reject(Reject),
    Ping(Ping),
    Pong(Pong),
    PlayerInput(PlayerInput),
    PhysicsSnapshot(PhysicsSnapshot),
    PlayerEvent(PlayerEvent),
    /// FNV-1a 64 of the client-generated verification chunk; 0 asks for full-chunk mode.
    WorldgenCheck(u64),
    ChunkData(ChunkData),
    ChunkUnload(ChunkUnload),
    BlockEditRequest(BlockEditRequest),
    VoxelModification(VoxelModification),
    /// Chunks the client wants sent again (a revision gap), 1..`MAX_RESYNC_CHUNKS`.
    ChunkResync(Vec<ChunkCoordNet>),
    LodIndex(LodIndex),
    /// Index entries written since the last update.
    LodIndexUpdate(Vec<LodIndexEntry>),
    LodRequest(Vec<LodSectionRequest>),
    LodData(LodData),
    /// Chunks beyond the view to draw at full detail, 1..`MAX_RESYNC_CHUNKS`.
    ChunkRequest(Vec<ChunkCoordNet>),
    HostStatus(HostState),
    /// C→S, only to a server whose Welcome has `welcome_flags::REGION_EDITS`.
    RegionEdit(RegionEdit),
    BlockRegistry(BlockRegistry),
    /// S→C, world channel: which of this client's edits the server has processed (edit
    /// prediction, ARCHITECTURE.md §6.5).
    EditAck(EditAck),
    /// C→S, control channel, sent when it changes: what the player holds (`held_item`; protocol
    /// v17, CHARACTERS.md §9.3). Others see it in snapshots.
    HeldItem(u16),
    /// S→C, world channel: a piece began to fall (protocol v20, docs/FELLING.md).
    EntitySpawn(EntitySpawn),
    /// S→C, world channel: a piece is gone (settled into blocks, after their `VoxelModification`).
    EntityDespawn(u32),
    /// C→S, world channel: an edit of a moving body (protocol v21).
    BodyEditRequest(BodyEditRequest),
    /// S→C, world channel: a body's changed cells (protocol v21).
    BodyEdit(BodyEdit),
    /// S→C reliable, control channel, before `Welcome` (protocol v22): the world's settings
    /// (`crate::world_settings`: its clock, its water speed) as tagged fields.
    WorldSettings(crate::world_settings::SettingsFields),
    /// C→S reliable, control channel, only to a server whose Welcome has `welcome_flags::INVENTORY`
    /// (ADR 0066): `Hello` first, then moves and the held slot.
    InventoryAction(InventoryAction),
    /// S→C reliable, control channel, only to a client that sent `InventoryAction::Hello`: the
    /// player's whole inventory, when it changes (ADR 0066).
    Inventory(InventoryMessage),
    /// S→C reliable, world channel, only to a client that sent `InventoryAction::Hello`: loose
    /// items that appeared or moved, and those gone (ADR 0066).
    ItemDrops(ItemDrops),
    /// S→C reliable, world channel, only to a client that sent `InventoryAction::GroundHello`
    /// (ADR 0071): ground items gone (picked up or knocked loose), by world cell; at the hello,
    /// all of them.
    GroundGone(Vec<[i32; 3]>),
    /// C→S reliable, world channel, only to a server whose Welcome has `welcome_flags::CHOPPING`
    /// (ADR 0067): `Hello` first, then swings.
    Chop(ChopAction),
    /// S→C reliable, world channel, only to a client that sent `ChopAction::Hello`: wood cells'
    /// cuts that changed, and cells whose cut is gone (ADR 0067).
    Cuts(CutsMessage),
    /// S→C reliable, world channel, only to a client that sent `ChopAction::Held` (so it
    /// understands): the tools other players hold, by item name (ADR 0067; the held-item wire
    /// carries blocks only). Each changed player's entry; an empty tool: none.
    HeldTools(Vec<HeldTool>),
    /// C→S reliable, control channel, only to a server whose Welcome has `welcome_flags::VITALS`:
    /// this client understands `Vitals` and the `Drown` damage cause (ADR 0069).
    VitalsHello,
    /// S→C datagram, with each snapshot, only to a client that sent `VitalsHello` and while the
    /// world's mode has stamina or oxygen (ADR 0069).
    Vitals(VitalsMessage),
    /// C→S reliable, control channel, only to a server whose `Inventory` has
    /// `inventory_flags::CREATURES` (ADR 0073): this client draws creatures; send it `Creatures`.
    CreaturesHello,
    /// C→S reliable, control channel, only to a server whose `Inventory` has
    /// `inventory_flags::DAMAGE` (docs/BLOCK_DAMAGE.md): this client draws block damage; send it
    /// `Damage` (every damaged cell now, then each change).
    DamageHello,
    /// S→C reliable, world channel, only to a client that sent `DamageHello`: damaged cells and
    /// cells no longer damaged.
    Damage(DamageMessage),
    /// S→C reliable, world channel, only to a client that sent `CreaturesHello`: creatures that
    /// appeared or moved, and those gone (ADR 0073).
    Creatures(CreaturesMessage),
    /// C→S reliable, control channel, only to a server whose `WorldSettings` has the required
    /// `world_settings::tag::WORLDGEN` (ADR 0074 B7): send this world's worldgen config.
    WorldgenRequest,
    /// S→C reliable, control channel, only to a client that sent `WorldgenRequest`: the world's
    /// worldgen config text (`dwell_worldgen::config::world_config`), whose hash the `WORLDGEN`
    /// tag names. u32 length (at most `limits::WORLDGEN_CONFIG_MAX_BYTES`), UTF-8.
    WorldgenConfig(String),
    /// C→S reliable, control channel, only to a server whose `Inventory` has
    /// `inventory_flags::CAUSES` (docs/proposals/VOLCANOES.md §6.2): this client understands every
    /// damage cause up to this one (`DamageCause::MAX` as it was built); a cause past it (or `Drown`
    /// without `VitalsHello`) goes to it as `Fall`.
    CausesHello(u8),
    /// C→S reliable, control channel, only to a server whose `Inventory` has
    /// `inventory_flags::ECOLOGY` (docs/ECOLOGY.md §9): this client reads the far ecology; send it
    /// `Ecology` (now, then at a low rate).
    EcologyHello,
    /// S→C reliable, world channel, only to a client that sent `EcologyHello`: the far herds and
    /// packs on both faces (not those near players: those are creatures), for far rendering.
    Ecology(EcologyMessage),
    /// C→S reliable, control channel, only to a server whose `Inventory` has
    /// `inventory_flags::STEAM` (docs/proposals/VOLCANOES.md §6.3): send this client the steam
    /// where lava boils water away.
    SteamHello,
    /// S→C reliable, world channel, only to a client that sent `SteamHello`: the world cells
    /// whose water lava just boiled away (the clients' puffs and hiss). u16 count (at most
    /// `MAX_STEAM_PER_MESSAGE`), each cell three i32.
    Steam(Vec<[i32; 3]>),
    /// C→S reliable, control channel, only to a server whose `WorldSettings` had
    /// `world_settings::tag::CAPABILITIES` (`crate::capabilities`, ARCHITECTURE.md §8.3), once
    /// after `Welcome`: the capabilities this client has. A feature is used only when both sides
    /// listed it.
    Capabilities(crate::capabilities::Capabilities),
}

/// The most groups one `Ecology` carries (more are left out).
pub const MAX_ECOLOGY_GROUPS: usize = 1024;
/// The longest species name on the wire.
pub const MAX_SPECIES_NAME: usize = 64;

/// A far herd or pack as sent (docs/ECOLOGY.md §9): its record's id, its face (0 face A, 1 face
/// B), its species by name, its place (face-local metres on its face's island) and its head.
#[derive(Clone, Debug, PartialEq)]
pub struct EcologyGroupNet {
    pub id: u32,
    pub face: u8,
    pub species: String,
    pub x: f32,
    pub z: f32,
    pub size: f32,
}

/// `Ecology`: the ecology's day and its far groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EcologyMessage {
    pub day: f32,
    pub groups: Vec<EcologyGroupNet>,
}

/// The most cells one `Steam` carries (more are dropped: a puff each is plenty).
pub const MAX_STEAM_PER_MESSAGE: usize = 256;

/// A creature as sent (ADR 0073): its skinned model (shared/models/models.json), its feet (world
/// metres), its heading (degrees, 0 faces +z) and its speed (m/s; the client picks its clip by it).
#[derive(Clone, Debug, PartialEq)]
pub struct CreatureNet {
    pub id: u32,
    pub model: String,
    pub position: [f32; 3],
    pub yaw: f32,
    pub speed: f32,
}

/// The most creatures one `Creatures` carries (more go in more messages).
pub const MAX_CREATURES_PER_MESSAGE: usize = 512;
/// The longest model name on the wire.
pub const MAX_MODEL_NAME: usize = 64;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreaturesMessage {
    pub creatures: Vec<CreatureNet>,
    pub gone: Vec<u32>,
}

/// The local player's stamina and oxygen as of its input `seq` (`Message::Vitals`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VitalsMessage {
    pub seq: u32,
    pub stamina: f32,
    /// Seconds of air left.
    pub oxygen: f32,
    /// `vitals_flags`: exhausted, and the mode's stamina and oxygen switches.
    pub flags: u8,
}

/// A player's held tool (`Message::HeldTools`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldTool {
    pub player_id: u16,
    /// The tool's item name, empty for none.
    pub tool: String,
    /// Its swing's plane (the hold): vertical, else level.
    pub vertical: bool,
}

/// What a client asks of chopping (ADR 0067).
#[derive(Clone, Debug, PartialEq)]
pub enum ChopAction {
    /// This client understands cuts: send it `Cuts` from now on (all of them now).
    Hello,
    /// A swing of `tool` (an item name) at the wood in `cell`, hitting its face `face` (0..6,
    /// `axis × 2 + positive`, world) at `point` (world metres), its plane `vertical` or level.
    Swing {
        tool: String,
        cell: [i32; 3],
        face: u8,
        point: [f32; 3],
        vertical: bool,
    },
    /// A swing of `tool` started along the look `look` (world, unit) in its plane `vertical` or
    /// level, only to a server whose Welcome has `welcome_flags::CHOP_SWEEP`: the server sweeps
    /// the tool's arc from its own eye and hits the first solid cell on the path.
    Sweep {
        tool: String,
        look: [f32; 3],
        vertical: bool,
    },
    /// What tool this player holds (an empty name: none) and its hold, only to a server with
    /// `CHOP_SWEEP`: it tells the others (`HeldTools`), and this client gets theirs from now on.
    Held { tool: String, vertical: bool },
}

/// The most players one `HeldTools` carries.
pub const MAX_HELD_TOOLS: usize = 1024;

/// The most cells one `Cuts` carries (more go in more messages).
pub const MAX_CUTS_PER_MESSAGE: usize = 512;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutsMessage {
    pub cuts: Vec<([i32; 3], crate::chopping::Cut)>,
    pub gone: Vec<[i32; 3]>,
    /// It ends with the cuts' bites (`Cut::write_bites`, WORK_QUEUE item 26): only to a client
    /// that said `BitesHello`, so an older one (which refuses trailing bytes) never gets them; an
    /// older server never writes them (its cuts carry none, drawn as before).
    pub bites: bool,
}

/// What a client asks of its inventory (ADR 0066).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryAction {
    /// This client understands inventories: send it `Inventory` and `ItemDrops` from now on.
    Hello,
    /// Move `count` (0: all) from slot `from` to slot `to` (`Inventory::move_items`).
    Move { from: u8, to: u8, count: u16 },
    /// The held hotbar slot.
    Select { slot: u8 },
    /// This client understands ground items (ADR 0071): send it `GroundGone` from now on. Sent only
    /// to a server whose `Inventory` has `inventory_flags::GROUND`.
    GroundHello,
    /// The break action aimed at the ground item in world cell `cell` (ADR 0071, amended
    /// 2026-11-09; the name is from when it picked the item up): the server checks it lies there
    /// and is within reach, and knocks it loose (a loose item in a mode that uses items; in
    /// creative it vanishes). Only to a server whose `Inventory` has `inventory_flags::GROUND`.
    Pick { cell: [i32; 3] },
    /// Make the recipe of this id once (`inventory::recipe_id`; owner, 2026-11-11: combining): the
    /// server takes the inputs and adds the output, or changes nothing. Only to a server whose
    /// `Inventory` has `inventory_flags::COMBINE` (an older one would read an unknown action as
    /// malformed).
    Combine { recipe: u32 },
}

/// `Inventory`'s flags.
pub mod inventory_flags {
    /// The world's mode uses items (no `unlimitedBlocks`): show the inventory, not the creative items.
    pub const USES_ITEMS: u8 = 1;
    /// The server keeps ground items (ADR 0071): it takes `GroundHello` and `Pick`.
    pub const GROUND: u8 = 2;
    /// The server keeps creatures (ADR 0073): it takes `CreaturesHello`.
    pub const CREATURES: u8 = 4;
    /// The server keeps block damage (docs/BLOCK_DAMAGE.md): it takes `DamageHello` and a break
    /// request's struck point.
    pub const DAMAGE: u8 = 8;
    /// The server makes recipes (shared/inventory `recipes`): it takes `InventoryAction::Combine`.
    pub const COMBINE: u8 = 16;
    /// The server takes `CausesHello` (docs/proposals/VOLCANOES.md §6.2): it sends a damage cause
    /// past the client's last known one (`Burn`, …) only to a client that said it knows it.
    pub const CAUSES: u8 = 32;
    /// The server runs a far ecology (docs/ECOLOGY.md §9): it takes `EcologyHello`.
    pub const ECOLOGY: u8 = 64;
    /// The server has lava (docs/proposals/VOLCANOES.md §6.3): it takes `SteamHello` and then
    /// sends `Steam`.
    pub const STEAM: u8 = 128;
}

/// The most cells one `Damage` carries (more go in more messages).
pub const MAX_DAMAGE_PER_MESSAGE: usize = 512;

/// `Damage`'s cells (docs/BLOCK_DAMAGE.md): each damaged cell's damage, and the cells whose damage
/// is gone (broken, replaced).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DamageMessage {
    pub cells: Vec<([i32; 3], crate::damage::DamageView)>,
    pub gone: Vec<[i32; 3]>,
}

/// The most cells one `GroundGone` carries (more go in more messages).
pub const MAX_GROUND_GONE_PER_MESSAGE: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct InventoryMessage {
    /// `inventory_flags`.
    pub flags: u8,
    pub inventory: crate::inventory::Inventory,
}

/// A loose item as sent: where it is now (world metres).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemDropNet {
    pub id: u32,
    pub item: String,
    pub count: u16,
    pub position: [f32; 3],
}

/// The most loose items one `ItemDrops` carries (more go in more messages).
pub const MAX_ITEM_DROPS_PER_MESSAGE: usize = 512;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemDrops {
    pub items: Vec<ItemDropNet>,
    pub gone: Vec<u32>,
}

impl Message {
    pub fn message_type(&self) -> MessageType {
        use Message as M;
        match self {
            M::DatagramPing(_) => MessageType::DatagramPing,
            M::DatagramPong(_) => MessageType::DatagramPong,
            M::WaterCurrents(_) => MessageType::WaterCurrents,
            M::StatusRequest => MessageType::StatusRequest,
            M::StatusResponse(_) => MessageType::StatusResponse,
            M::ClientHello(_) => MessageType::ClientHello,
            M::Challenge(_) => MessageType::Challenge,
            M::ClientAuth(_) => MessageType::ClientAuth,
            M::Welcome(_) => MessageType::Welcome,
            M::Reject(_) => MessageType::Reject,
            M::Ping(_) => MessageType::Ping,
            M::Pong(_) => MessageType::Pong,
            M::PlayerInput(_) => MessageType::PlayerInput,
            M::PhysicsSnapshot(_) => MessageType::PhysicsSnapshot,
            M::PlayerEvent(_) => MessageType::PlayerEvent,
            M::WorldgenCheck(_) => MessageType::WorldgenCheck,
            M::ChunkData(_) => MessageType::ChunkData,
            M::ChunkUnload(_) => MessageType::ChunkUnload,
            M::BlockEditRequest(_) => MessageType::BlockEditRequest,
            M::VoxelModification(_) => MessageType::VoxelModification,
            M::ChunkResync(_) => MessageType::ChunkResync,
            M::LodIndex(_) => MessageType::LodIndex,
            M::LodIndexUpdate(_) => MessageType::LodIndexUpdate,
            M::LodRequest(_) => MessageType::LodRequest,
            M::LodData(_) => MessageType::LodData,
            M::ChunkRequest(_) => MessageType::ChunkRequest,
            M::HostStatus(_) => MessageType::HostStatus,
            M::RegionEdit(_) => MessageType::RegionEdit,
            M::BlockRegistry(_) => MessageType::BlockRegistry,
            M::EditAck(_) => MessageType::EditAck,
            M::HeldItem(_) => MessageType::HeldItem,
            M::EntitySpawn(_) => MessageType::EntitySpawn,
            M::EntityDespawn(_) => MessageType::EntityDespawn,
            M::BodyEditRequest(_) => MessageType::BodyEditRequest,
            M::WorldSettings(_) => MessageType::WorldSettings,
            M::InventoryAction(_) => MessageType::InventoryAction,
            M::Inventory(_) => MessageType::Inventory,
            M::ItemDrops(_) => MessageType::ItemDrops,
            M::GroundGone(_) => MessageType::GroundGone,
            M::Chop(_) => MessageType::Chop,
            M::Cuts(_) => MessageType::Cuts,
            M::HeldTools(_) => MessageType::HeldTools,
            M::VitalsHello => MessageType::VitalsHello,
            M::Vitals(_) => MessageType::Vitals,
            M::CreaturesHello => MessageType::CreaturesHello,
            M::DamageHello => MessageType::DamageHello,
            M::WorldgenRequest => MessageType::WorldgenRequest,
            M::WorldgenConfig(_) => MessageType::WorldgenConfig,
            M::CausesHello(_) => MessageType::CausesHello,
            M::SteamHello => MessageType::SteamHello,
            M::Capabilities(_) => MessageType::Capabilities,
            M::SwingHello => MessageType::SwingHello,
            M::BitesHello => MessageType::BitesHello,
            M::Steam(_) => MessageType::Steam,
            M::Damage(_) => MessageType::Damage,
            M::Creatures(_) => MessageType::Creatures,
            M::EcologyHello => MessageType::EcologyHello,
            M::Ecology(_) => MessageType::Ecology,
            M::BodyEdit(_) => MessageType::BodyEdit,
        }
    }

    /// Appends the encoded message. Strings longer than their limit are truncated at a UTF-8
    /// boundary, so encoding never produces a message the peer would reject.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let mut w = ByteWriter::new(out);
        w.u8(self.message_type() as u8);
        write_body(&mut w, self);
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    /// Decodes exactly one message; `None` on an unknown type, truncation, trailing bytes,
    /// over-limit strings, invalid UTF-8 or out-of-range values.
    pub fn decode(bytes: &[u8]) -> Option<Message> {
        let mut r = ByteReader::new(bytes);
        let t = MessageType::from_u8(r.u8())?;
        let m = read_body(&mut r, t)?;
        r.at_end().then_some(m)
    }
}

// --- Chunk voxels and LOD cells: palette + RLE (§6.1) --------------------------------------------
//
// Chunk voxels go on the wire in layer order (x fastest, then z, then y) so horizontal strata make
// long runs. Palette: u16 count, then the ids in order of first appearance. Runs until full: LEB128
// length (minimal), then the palette index as u8 (palette ≤ 256) or u16. Canonical: maximal runs,
// first-appearance palette, so encode(decode(bytes)) == bytes.

fn wire_to_index(i: usize) -> usize {
    (i & 31) | (((i >> 10) & 31) << 5) | (((i >> 5) & 31) << 10)
}

fn write_cells(w: &mut ByteWriter, count: usize, at: impl Fn(usize) -> u16) {
    let mut runs: Vec<(u16, u32)> = Vec::new();
    for i in 0..count {
        let m = at(i);
        match runs.last_mut() {
            Some((last, n)) if *last == m => *n += 1,
            _ => runs.push((m, 1)),
        }
    }
    let mut palette: Vec<u16> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for &(m, _) in &runs {
        index.entry(m).or_insert_with(|| {
            palette.push(m);
            (palette.len() - 1) as u16
        });
    }
    w.u16(palette.len() as u16);
    for &m in &palette {
        w.u16(m);
    }
    let wide = palette.len() > 256;
    for &(m, length) in &runs {
        let mut v = length;
        loop {
            if v < 0x80 {
                w.u8(v as u8);
                break;
            }
            w.u8(((v & 0x7F) | 0x80) as u8);
            v >>= 7;
        }
        if wide {
            w.u16(index[&m]);
        } else {
            w.u8(index[&m] as u8);
        }
    }
}

fn read_varint(r: &mut ByteReader) -> u32 {
    let mut v = 0u32;
    let mut shift = 0;
    while shift < 21 {
        let b = r.u8();
        r.check(shift == 0 || b != 0); // minimal encoding only
        v |= u32::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return v;
        }
        shift += 7;
    }
    r.check(false);
    0
}

fn read_cells(r: &mut ByteReader, volume: usize, mut put: impl FnMut(usize, u16)) {
    let count = usize::from(r.u16());
    r.check(count >= 1 && count <= volume.min(0xFFFF));
    if !r.ok() {
        return;
    }
    let palette: Vec<u16> = (0..count).map(|_| r.u16()).collect();
    let wide = count > 256;
    let mut filled = 0usize;
    while filled < volume && r.ok() {
        let run = read_varint(r) as usize;
        let i = if wide {
            usize::from(r.u16())
        } else {
            usize::from(r.u8())
        };
        r.check(run >= 1 && run <= volume - filled && i < count);
        if !r.ok() {
            break;
        }
        for _ in 0..run {
            put(filled, palette[i]);
            filled += 1;
        }
    }
}

fn write_voxels(w: &mut ByteWriter, voxels: &[u16]) {
    write_cells(w, CHUNK_VOLUME, |wire| {
        voxels.get(wire_to_index(wire)).copied().unwrap_or(0)
    });
}

fn read_voxels(r: &mut ByteReader) -> Vec<u16> {
    let mut voxels = vec![0u16; CHUNK_VOLUME];
    read_cells(r, CHUNK_VOLUME, |wire, m| voxels[wire_to_index(wire)] = m);
    if r.ok() { voxels } else { Vec::new() }
}

fn write_lod_cells(w: &mut ByteWriter, cells: &[u16]) {
    write_cells(w, LOD_CELL_COUNT, |i| cells.get(i).copied().unwrap_or(0));
}

fn read_lod_cells(r: &mut ByteReader) -> Vec<u16> {
    let mut cells = vec![0u16; LOD_CELL_COUNT];
    read_cells(r, LOD_CELL_COUNT, |i, m| cells[i] = m);
    if r.ok() { cells } else { Vec::new() }
}

/// Chunk voxels as palette + RLE (the `ChunkData` Explicit payload; the world file's chunk
/// encoding before compression): `CHUNK_VOLUME` ids in chunk index order.
pub fn encode_chunk_voxels(voxels: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    write_voxels(&mut ByteWriter::new(&mut out), voxels);
    out
}

/// `None` unless `bytes` is exactly one canonical-length payload.
pub fn decode_chunk_voxels(bytes: &[u8]) -> Option<Vec<u16>> {
    let mut r = ByteReader::new(bytes);
    let v = read_voxels(&mut r);
    r.at_end().then_some(v)
}

/// LOD section content as palette + RLE: `LOD_CELL_COUNT` ids, already in layer order.
pub fn encode_lod_cells(cells: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    write_lod_cells(&mut ByteWriter::new(&mut out), cells);
    out
}

pub fn decode_lod_cells(bytes: &[u8]) -> Option<Vec<u16>> {
    let mut r = ByteReader::new(bytes);
    let v = read_lod_cells(&mut r);
    r.at_end().then_some(v)
}

/// posfix: the nearest multiple of 1/`POSITION_FIXED_SCALE` m (halves round up), clamped to i32.
pub fn to_fixed_position(v: f64) -> i32 {
    let scaled = (v * world::POSITION_FIXED_SCALE as f64 + 0.5).floor();
    if scaled.is_nan() || scaled <= -2_147_483_648.0 {
        return i32::MIN; // also NaN
    }
    if scaled > 2_147_483_647.0 {
        return i32::MAX;
    }
    scaled as i32
}

pub fn from_fixed_position(v: i32) -> f64 {
    f64::from(v) / world::POSITION_FIXED_SCALE as f64
}

/// Bytes the client signs in ClientAuth (ADR 0004):
/// `authDomainTag ‖ nonce ‖ transportBinding (32) ‖ publicKey`.
pub fn auth_transcript(
    nonce: &Nonce,
    transport_binding: &[u8; 32],
    public_key: &PublicKey,
) -> Vec<u8> {
    let mut out = AUTH_DOMAIN_TAG.as_bytes().to_vec();
    out.extend_from_slice(nonce);
    out.extend_from_slice(transport_binding);
    out.extend_from_slice(public_key);
    out
}

// --- Bodies ---------------------------------------------------------------------------------------

fn write_coord(w: &mut ByteWriter, c: &[i32; 3]) {
    for &v in c {
        w.i32(v);
    }
}

fn read_coord(r: &mut ByteReader) -> [i32; 3] {
    [r.i32(), r.i32(), r.i32()]
}

fn write_coords(w: &mut ByteWriter, coords: &[ChunkCoordNet], max: usize) {
    let count = coords.len().min(max);
    w.u16(count as u16);
    for c in &coords[..count] {
        write_coord(w, c);
    }
}

fn write_index_entries(w: &mut ByteWriter, entries: &[LodIndexEntry]) {
    for e in entries {
        w.i32(e.i);
        w.i32(e.j);
        w.i32(e.k);
        w.u32(e.revision);
    }
}

fn read_index_entries(r: &mut ByteReader, count: usize) -> Vec<LodIndexEntry> {
    let mut out = Vec::new();
    for _ in 0..count {
        if !r.ok() {
            break;
        }
        out.push(LodIndexEntry {
            i: r.i32(),
            j: r.i32(),
            k: r.i32(),
            revision: r.u32(),
        });
    }
    out
}

fn write_controller(w: &mut ByteWriter, c: &ControllerState) {
    w.u8(c.flags);
    for v in [
        c.current_x,
        c.current_z,
        c.external_x,
        c.external_z,
        c.contribution_x,
        c.contribution_z,
        c.accumulated_y,
        c.platform_y,
        c.target_y,
        c.ground_velocity_y,
    ] {
        w.f32(v);
    }
    w.u8(c.ground_kind as u8);
    w.u16(c.ground_id);
    w.u8(c.buffer_ticks);
    w.u8(c.coyote_ticks);
    w.u8(c.step_grace);
    w.u8(c.slide_flags);
    w.u8(c.slide_cooldown);
    if c.flags & controller_flags::CLIMBING != 0 {
        write_coord(w, &c.ladder);
    }
    if c.flags & controller_flags::HAS_RELEASED != 0 {
        w.i32(c.released[0]);
        w.i32(c.released[1]);
    }
}

fn read_controller(r: &mut ByteReader) -> ControllerState {
    let mut c = ControllerState {
        flags: r.u8(),
        ..Default::default()
    };
    for v in [
        &mut c.current_x,
        &mut c.current_z,
        &mut c.external_x,
        &mut c.external_z,
        &mut c.contribution_x,
        &mut c.contribution_z,
        &mut c.accumulated_y,
        &mut c.platform_y,
        &mut c.target_y,
        &mut c.ground_velocity_y,
    ] {
        *v = r.f32();
    }
    let kind = GroundKind::from_u8(r.u8());
    r.check(kind.is_some());
    c.ground_kind = kind.unwrap_or(GroundKind::None);
    c.ground_id = r.u16();
    c.buffer_ticks = r.u8();
    c.coyote_ticks = r.u8();
    c.step_grace = r.u8();
    c.slide_flags = r.u8();
    c.slide_cooldown = r.u8();
    if c.flags & controller_flags::CLIMBING != 0 {
        c.ladder = read_coord(r);
    }
    if c.flags & controller_flags::HAS_RELEASED != 0 {
        c.released = [r.i32(), r.i32()];
    }
    c
}

/// Positions must be finite and within ±`POS64_LIMIT`.
fn read_pos64(r: &mut ByteReader) -> [f64; 3] {
    let limit = world::POS64_LIMIT as f64;
    let mut v = [0.0; 3];
    for x in &mut v {
        *x = r.f64();
        r.check(*x >= -limit && *x <= limit); // also rejects NaN
    }
    v
}

fn read_state(r: &mut ByteReader) -> PlayerState {
    let s = PlayerState::from_u8(r.u8());
    r.check(s.is_some());
    s.unwrap_or(PlayerState::Idle)
}

fn read_cause(r: &mut ByteReader) -> DamageCause {
    let c = DamageCause::from_u8(r.u8());
    r.check(c.is_some());
    c.unwrap_or(DamageCause::Fall)
}

fn write_body(w: &mut ByteWriter, m: &Message) {
    use Message as M;
    match m {
        M::DatagramPing(m) => {
            w.u32(m.seq);
            w.f64(m.client_time_ms);
        }
        M::DatagramPong(m) => {
            w.u32(m.seq);
            w.f64(m.client_time_ms);
            w.u32(m.server_tick);
        }
        M::WaterCurrents(m) => {
            w.u8(u8::from(m.face_b));
            w.i32(m.cx);
            w.i32(m.cz);
            for c in m.cells.iter() {
                w.i8(c[0]);
                w.i8(c[1]);
            }
        }
        M::StatusRequest => {}
        M::StatusResponse(m) => {
            w.u16(m.protocol_version);
            w.str(clamp_str(&m.server_name, limits::SERVER_NAME_MAX_BYTES));
            w.str(clamp_str(&m.motd, limits::MOTD_MAX_BYTES));
            w.u16(m.players);
            w.u16(m.max_players);
            w.u8(m.flags);
        }
        M::ClientHello(m) => {
            w.u16(m.protocol_version);
            w.str(clamp_str(
                &m.client_version,
                limits::CLIENT_VERSION_MAX_BYTES,
            ));
            w.bytes(&m.public_key);
            w.str(clamp_str(&m.display_name, limits::DISPLAY_NAME_MAX_BYTES));
        }
        M::Challenge(n) => w.bytes(n),
        M::ClientAuth(s) => w.bytes(s),
        M::Welcome(m) => {
            w.u16(m.player_id);
            w.u64(m.world_seed);
            w.u32(m.generator_version);
            w.u32(m.server_tick);
            write_coord(w, &m.verification_chunk);
            w.u8(m.flags);
            w.u64(m.registry_hash);
        }
        M::Reject(m) => {
            w.u8(m.reason as u8);
            w.str(clamp_str(&m.message, limits::REJECT_MESSAGE_MAX_BYTES));
        }
        M::Ping(m) => {
            w.u32(m.seq);
            w.f64(m.client_time_ms);
        }
        M::Pong(m) => {
            w.u32(m.seq);
            w.f64(m.client_time_ms);
            w.u32(m.server_tick);
            w.f64(m.server_time_ms);
        }
        M::PlayerInput(m) => {
            w.u32(m.last_snapshot_tick);
            let count = m.inputs.len().min(MAX_INPUTS_PER_DATAGRAM);
            w.u8(count as u8);
            for f in &m.inputs[m.inputs.len() - count..] {
                w.u32(f.seq);
                w.i8(f.move_x);
                w.i8(f.move_y);
                w.u16(f.buttons);
                w.i16(f.yaw);
                w.i16(f.pitch);
            }
        }
        M::PhysicsSnapshot(m) => {
            w.u32(m.server_tick);
            w.u32(m.ack_input_seq);
            for v in m.local.position {
                w.f64(v);
            }
            for v in m.local.velocity {
                w.f32(v);
            }
            w.u8(m.local.flags);
            w.u8(m.local.health);
            w.u8(m.local.state as u8);
            w.u8(m.local.input_buffer);
            w.u32(m.local.last_knockback_seq);
            write_controller(w, &m.local.controller);
            let count = m.remotes.len().min(255);
            w.u8(count as u8);
            for p in &m.remotes[..count] {
                w.u16(p.player_id);
                for v in p.position {
                    w.i32(to_fixed_position(v));
                }
                for v in p.velocity {
                    w.f16(v);
                }
                w.i16(p.yaw);
                w.i16(p.pitch);
                w.u8(p.state as u8);
                w.u8(p.flags);
                w.u16(p.held);
                w.u8(p.swing);
            }
            let count = m.pieces.len().min(255);
            w.u8(count as u8);
            for p in &m.pieces[..count] {
                w.u32(p.id);
                for v in p.position {
                    w.i32(to_fixed_position(v));
                }
                for v in p.rotation {
                    w.i16((v.clamp(-1.0, 1.0) * 32767.0).round() as i16);
                }
                for v in p.velocity {
                    w.f16(v);
                }
                for v in p.angular {
                    w.f16(v);
                }
            }
            if m.swing_kinds || m.grid.is_some() {
                let count = m.remotes.len().min(255);
                w.u8(count as u8);
                for p in &m.remotes[..count] {
                    w.u8(p.swing_kind);
                }
            }
            if let Some(g) = &m.grid {
                w.u8(g.grid);
                if g.grid != 0 {
                    g.origin.iter().for_each(|&v| w.f64(v));
                    g.rotation.iter().for_each(|&v| w.f64(v));
                }
            }
        }
        M::PlayerEvent(m) => {
            w.u8(m.kind as u8);
            w.u16(m.player_id);
            w.u32(m.server_tick);
            w.u32(m.input_seq);
            match m.kind {
                PlayerEventKind::Knockback => m.vector.iter().for_each(|&v| w.f32(v)),
                PlayerEventKind::Respawn => m.position.iter().for_each(|&v| w.f64(v)),
                PlayerEventKind::Damage => {
                    w.u8(m.amount);
                    w.u8(m.cause as u8);
                }
                PlayerEventKind::Death => w.u8(m.cause as u8),
            }
        }
        M::WorldgenCheck(h) => w.u64(*h),
        M::ChunkData(m) => {
            w.u8(m.form as u8);
            write_coord(w, &m.coord);
            w.u32(m.revision);
            if m.form == ChunkForm::Explicit {
                write_voxels(w, &m.voxels);
            }
            if m.grid != 0 {
                w.u8(m.grid);
            }
        }
        M::ChunkUnload(m) => {
            write_coords(w, &m.coords, 0xFFFF);
            if m.grid != 0 {
                w.u8(m.grid);
            }
        }
        M::BlockEditRequest(m) => {
            w.u8(m.action as u8);
            write_coord(w, &m.cell);
            w.u8(m.face);
            if m.action == BlockEditAction::Place {
                w.u16(m.material);
            }
            // The piece: rows in the low bits, `down` the top bit.
            w.u8(m.rows | if m.down { 0x80 } else { 0 });
            if let Some(p) = m.point {
                for v in p {
                    w.u8(v);
                }
            }
        }
        M::VoxelModification(m) => {
            w.u8(m.reason as u8);
            w.u32(m.server_tick);
            let count = m.chunks.len().min(0xFFFF);
            w.u16(count as u16);
            for c in &m.chunks[..count] {
                write_coord(w, &c.coord);
                w.u32(c.revision);
                let n = c.changes.len().min(0xFFFF);
                w.u16(n as u16);
                for v in &c.changes[..n] {
                    w.u16(v.index);
                    w.u16(v.material);
                }
            }
        }
        M::ChunkResync(c) | M::ChunkRequest(c) => write_coords(w, c, limits::MAX_RESYNC_CHUNKS),
        M::LodIndex(m) => {
            let count = m.entries.len().min(limits::MAX_LOD_INDEX_ENTRIES);
            w.u8(u8::from(m.last));
            w.u32(count as u32);
            write_index_entries(w, &m.entries[..count]);
        }
        M::LodIndexUpdate(e) => {
            let count = e.len().min(limits::MAX_LOD_INDEX_ENTRIES);
            w.u16(count as u16);
            write_index_entries(w, &e[..count]);
        }
        M::LodRequest(s) => {
            let count = s.len().min(lod::MAX_REQUEST_SECTIONS as usize);
            w.u8(count as u8);
            for s in &s[..count] {
                w.u8(s.level);
                write_coord(w, &s.section);
                w.u32(s.known_revision);
            }
        }
        M::LodData(m) => {
            w.u8(m.form as u8);
            w.u8(m.level);
            write_coord(w, &m.section);
            w.u32(m.revision);
            if m.form == crate::LodForm::Explicit {
                write_lod_cells(w, &m.cells);
            }
        }
        M::HostStatus(s) => w.u8(*s as u8),
        M::HeldItem(h) => w.u16(*h),
        M::EntitySpawn(m) => {
            w.u32(m.id);
            for v in m.origin {
                w.f64(v);
            }
            for v in m.rotation {
                w.f32(v);
            }
            w.u8(m.kind);
            w.u16(m.carriers);
            let count = m.cells.len().min(limits::MAX_PIECE_CELLS);
            w.u16(count as u16);
            for c in &m.cells[..count] {
                for v in c.offset {
                    w.i16(v);
                }
                w.u16(c.state);
            }
        }
        M::EntityDespawn(id) => w.u32(*id),
        M::WorldSettings(f) => f.write(w),
        M::InventoryAction(a) => match *a {
            InventoryAction::Hello => w.u8(0),
            InventoryAction::Move { from, to, count } => {
                w.u8(1);
                w.u8(from);
                w.u8(to);
                w.u16(count);
            }
            InventoryAction::Select { slot } => {
                w.u8(2);
                w.u8(slot);
            }
            InventoryAction::GroundHello => w.u8(3),
            InventoryAction::Pick { cell } => {
                w.u8(4);
                cell.iter().for_each(|v| w.i32(*v));
            }
            InventoryAction::Combine { recipe } => {
                w.u8(5);
                w.u32(recipe);
            }
        },
        M::GroundGone(cells) => {
            let n = cells.len().min(MAX_GROUND_GONE_PER_MESSAGE);
            w.u16(n as u16);
            for c in &cells[..n] {
                c.iter().for_each(|v| w.i32(*v));
            }
        }
        M::Inventory(m) => {
            w.u8(m.flags);
            m.inventory.write(w);
        }
        M::ItemDrops(m) => {
            let n = m.items.len().min(MAX_ITEM_DROPS_PER_MESSAGE);
            w.u16(n as u16);
            for d in &m.items[..n] {
                w.u32(d.id);
                w.str(crate::bytes::clamp_str(
                    &d.item,
                    crate::inventory::MAX_ITEM_NAME,
                ));
                w.u16(d.count);
                for v in d.position {
                    w.f32(v);
                }
            }
            let n = m.gone.len().min(MAX_ITEM_DROPS_PER_MESSAGE);
            w.u16(n as u16);
            for &id in &m.gone[..n] {
                w.u32(id);
            }
        }
        M::Chop(a) => match a {
            ChopAction::Hello => w.u8(0),
            ChopAction::Swing {
                tool,
                cell,
                face,
                point,
                vertical,
            } => {
                w.u8(1);
                w.str(crate::bytes::clamp_str(
                    tool,
                    crate::inventory::MAX_ITEM_NAME,
                ));
                for &v in cell {
                    w.i32(v);
                }
                w.u8(*face);
                for &v in point {
                    w.f32(v);
                }
                w.u8(u8::from(*vertical));
            }
            ChopAction::Sweep {
                tool,
                look,
                vertical,
            } => {
                w.u8(2);
                w.str(crate::bytes::clamp_str(
                    tool,
                    crate::inventory::MAX_ITEM_NAME,
                ));
                for &v in look {
                    w.f32(v);
                }
                w.u8(u8::from(*vertical));
            }
            ChopAction::Held { tool, vertical } => {
                w.u8(3);
                w.str(crate::bytes::clamp_str(
                    tool,
                    crate::inventory::MAX_ITEM_NAME,
                ));
                w.u8(u8::from(*vertical));
            }
        },
        M::VitalsHello => {}
        M::CausesHello(n) => w.u8(*n),
        M::Capabilities(c) => c.write(w),
        M::CreaturesHello
        | M::DamageHello
        | M::WorldgenRequest
        | M::SteamHello
        | M::SwingHello
        | M::BitesHello
        | M::EcologyHello => {}
        M::Steam(cells) => {
            let n = cells.len().min(MAX_STEAM_PER_MESSAGE);
            w.u16(n as u16);
            for c in &cells[..n] {
                for &v in c {
                    w.i32(v);
                }
            }
        }
        M::Ecology(m) => {
            w.f32(m.day);
            let n = m.groups.len().min(MAX_ECOLOGY_GROUPS);
            w.u16(n as u16);
            for g in &m.groups[..n] {
                w.u32(g.id);
                w.u8(g.face);
                w.str(crate::bytes::clamp_str(&g.species, MAX_SPECIES_NAME));
                w.f32(g.x);
                w.f32(g.z);
                w.f32(g.size);
            }
        }
        M::WorldgenConfig(t) => {
            // Too long to send: none (the guest's hash check then refuses, with a message).
            let b: &[u8] = if t.len() <= limits::WORLDGEN_CONFIG_MAX_BYTES {
                t.as_bytes()
            } else {
                &[]
            };
            w.u32(b.len() as u32);
            w.bytes(b);
        }
        M::Damage(m) => {
            let n = m.cells.len().min(MAX_DAMAGE_PER_MESSAGE);
            w.u16(n as u16);
            for (c, d) in &m.cells[..n] {
                for &v in c {
                    w.i32(v);
                }
                d.write(w);
            }
            let n = m.gone.len().min(MAX_DAMAGE_PER_MESSAGE);
            w.u16(n as u16);
            for c in &m.gone[..n] {
                for &v in c {
                    w.i32(v);
                }
            }
        }
        M::Creatures(m) => {
            let n = m.creatures.len().min(MAX_CREATURES_PER_MESSAGE);
            w.u16(n as u16);
            for c in &m.creatures[..n] {
                w.u32(c.id);
                w.str(crate::bytes::clamp_str(&c.model, MAX_MODEL_NAME));
                for v in c.position {
                    w.f32(v);
                }
                w.f32(c.yaw);
                w.f32(c.speed);
            }
            let n = m.gone.len().min(MAX_CREATURES_PER_MESSAGE);
            w.u16(n as u16);
            for &id in &m.gone[..n] {
                w.u32(id);
            }
        }
        M::Vitals(m) => {
            w.u32(m.seq);
            w.f32(m.stamina);
            w.f32(m.oxygen);
            w.u8(m.flags);
        }
        M::HeldTools(list) => {
            let n = list.len().min(MAX_HELD_TOOLS);
            w.u16(n as u16);
            for h in &list[..n] {
                w.u16(h.player_id);
                w.str(crate::bytes::clamp_str(
                    &h.tool,
                    crate::inventory::MAX_ITEM_NAME,
                ));
                w.u8(u8::from(h.vertical));
            }
        }
        M::Cuts(m) => {
            let n = m.cuts.len().min(MAX_CUTS_PER_MESSAGE);
            w.u16(n as u16);
            for (c, k) in &m.cuts[..n] {
                for &v in c {
                    w.i32(v);
                }
                k.write(w);
            }
            let n = m.gone.len().min(MAX_CUTS_PER_MESSAGE);
            w.u16(n as u16);
            for c in &m.gone[..n] {
                for &v in c {
                    w.i32(v);
                }
            }
            if m.bites {
                w.u8(crate::chopping::BITES_MARK);
                for (_, k) in &m.cuts[..m.cuts.len().min(MAX_CUTS_PER_MESSAGE)] {
                    k.write_bites(w);
                }
            }
        }
        M::BodyEditRequest(m) => {
            w.u32(m.body);
            for v in m.cell {
                w.i16(v);
            }
            w.u8(m.face);
            w.u8(m.action as u8);
            if m.action == BlockEditAction::Place {
                w.u16(m.material);
            }
        }
        M::BodyEdit(m) => {
            w.u32(m.body);
            let count = m.cells.len().min(limits::MAX_PIECE_CELLS);
            w.u16(count as u16);
            for c in &m.cells[..count] {
                for v in c.offset {
                    w.i16(v);
                }
                w.u16(c.state);
            }
        }
        M::EditAck(m) => {
            w.u32(m.through);
            w.u16(m.rejected.len() as u16);
            for &seq in &m.rejected {
                w.u32(seq);
            }
        }
        M::BlockRegistry(m) => {
            w.u16(m.part);
            w.u16(m.parts);
            w.u32(m.data.len() as u32);
            w.bytes(&m.data);
        }
        M::RegionEdit(m) => {
            w.u8(m.op as u8);
            write_coord(w, &m.min);
            for &s in &m.size {
                w.u8(s);
            }
            match m.op {
                RegionEditOp::Fill | RegionEditOp::Hollow => w.u16(m.material),
                RegionEditOp::Replace => {
                    w.u16(m.from);
                    w.u16(m.material);
                }
                RegionEditOp::Paste | RegionEditOp::Structure => {
                    write_cells(w, m.volume(), |i| m.cells.get(i).copied().unwrap_or(0));
                }
            }
        }
    }
}

fn read_coords(r: &mut ByteReader, max: usize) -> Vec<ChunkCoordNet> {
    let count = usize::from(r.u16());
    r.check(count >= 1 && count <= max);
    let mut out = Vec::new();
    for _ in 0..count {
        if !r.ok() {
            break;
        }
        out.push(read_coord(r));
    }
    out
}

fn read_body(r: &mut ByteReader, t: MessageType) -> Option<Message> {
    use Message as M;
    use MessageType as T;
    let m = match t {
        T::DatagramPing => M::DatagramPing(DatagramPing {
            seq: r.u32(),
            client_time_ms: r.f64(),
        }),
        T::DatagramPong => M::DatagramPong(DatagramPong {
            seq: r.u32(),
            client_time_ms: r.f64(),
            server_tick: r.u32(),
        }),
        T::WaterCurrents => {
            let face = r.u8();
            if face > 1 {
                return None;
            }
            let (cx, cz) = (r.i32(), r.i32());
            let mut cells = Box::new([[0i8; 2]; 256]);
            for c in cells.iter_mut() {
                *c = [r.i8(), r.i8()];
            }
            M::WaterCurrents(WaterCurrents {
                face_b: face == 1,
                cx,
                cz,
                cells,
            })
        }
        T::StatusRequest => M::StatusRequest,
        T::StatusResponse => M::StatusResponse(StatusResponse {
            protocol_version: r.u16(),
            server_name: r.str(limits::SERVER_NAME_MAX_BYTES),
            motd: r.str(limits::MOTD_MAX_BYTES),
            players: r.u16(),
            max_players: r.u16(),
            flags: r.u8(),
        }),
        T::ClientHello => M::ClientHello(ClientHello {
            protocol_version: r.u16(),
            client_version: r.str(limits::CLIENT_VERSION_MAX_BYTES),
            public_key: r.fixed(),
            display_name: r.str(limits::DISPLAY_NAME_MAX_BYTES),
        }),
        T::Challenge => M::Challenge(r.fixed()),
        T::ClientAuth => M::ClientAuth(r.fixed()),
        T::Welcome => M::Welcome(Welcome {
            player_id: r.u16(),
            world_seed: r.u64(),
            generator_version: r.u32(),
            server_tick: r.u32(),
            verification_chunk: read_coord(r),
            flags: r.u8(),
            registry_hash: r.u64(),
        }),
        T::Reject => {
            let reason = RejectReason::from_u8(r.u8())?;
            M::Reject(Reject {
                reason,
                message: r.str(limits::REJECT_MESSAGE_MAX_BYTES),
            })
        }
        T::Ping => M::Ping(Ping {
            seq: r.u32(),
            client_time_ms: r.f64(),
        }),
        T::Pong => M::Pong(Pong {
            seq: r.u32(),
            client_time_ms: r.f64(),
            server_tick: r.u32(),
            server_time_ms: r.f64(),
        }),
        T::PlayerInput => {
            let last_snapshot_tick = r.u32();
            let count = usize::from(r.u8());
            r.check((1..=MAX_INPUTS_PER_DATAGRAM).contains(&count));
            let mut inputs = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                let f = InputFrame {
                    seq: r.u32(),
                    move_x: r.i8(),
                    move_y: r.i8(),
                    buttons: r.u16(),
                    yaw: r.i16(),
                    pitch: r.i16(),
                };
                r.check(f.buttons & !input_buttons::ALL == 0);
                inputs.push(f);
            }
            M::PlayerInput(PlayerInput {
                last_snapshot_tick,
                inputs,
            })
        }
        T::PhysicsSnapshot => {
            let server_tick = r.u32();
            let ack_input_seq = r.u32();
            let position = read_pos64(r);
            let velocity = [r.f32(), r.f32(), r.f32()];
            let flags = r.u8();
            r.check(flags & !player_flags::ALL == 0);
            let local = LocalPlayerState {
                position,
                velocity,
                flags,
                health: r.u8(),
                state: read_state(r),
                input_buffer: r.u8(),
                last_knockback_seq: r.u32(),
                controller: read_controller(r),
            };
            let count = r.u8();
            let mut remotes = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                let player_id = r.u16();
                let position = [
                    from_fixed_position(r.i32()),
                    from_fixed_position(r.i32()),
                    from_fixed_position(r.i32()),
                ];
                let velocity = [r.f16(), r.f16(), r.f16()];
                let p = RemotePlayerState {
                    player_id,
                    position,
                    velocity,
                    yaw: r.i16(),
                    pitch: r.i16(),
                    state: read_state(r),
                    flags: r.u8(),
                    held: r.u16(),
                    swing: r.u8(),
                    swing_kind: 0,
                };
                r.check(p.flags & !player_flags::ALL == 0);
                remotes.push(p);
            }
            let count = r.u8();
            let mut pieces = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                let id = r.u32();
                let position = [
                    from_fixed_position(r.i32()),
                    from_fixed_position(r.i32()),
                    from_fixed_position(r.i32()),
                ];
                let rotation = [r.i16(), r.i16(), r.i16(), r.i16()].map(|v| f32::from(v) / 32767.0);
                let velocity = [r.f16(), r.f16(), r.f16()];
                let angular = [r.f16(), r.f16(), r.f16()];
                pieces.push(PieceState {
                    id,
                    position,
                    rotation,
                    velocity,
                    angular,
                });
            }
            // Optional: the remotes' swing kinds, one each (a newer server's, to a client that
            // said `SwingHello`).
            let swing_kinds = r.ok() && !r.at_end();
            if swing_kinds {
                let n = r.u8();
                r.check(usize::from(n) == remotes.len());
                for p in &mut remotes {
                    p.swing_kind = r.u8();
                }
            }
            // Optional: the local player's grid (`grid.frames`).
            let grid = (r.ok() && !r.at_end()).then(|| {
                let grid = r.u8();
                let mut g = GridView {
                    grid,
                    ..GridView::default()
                };
                if grid != 0 {
                    g.origin = [r.f64(), r.f64(), r.f64()];
                    r.check(g.origin.iter().all(|v| v.is_finite()));
                    g.rotation = [r.f64(), r.f64(), r.f64(), r.f64()];
                }
                g
            });
            M::PhysicsSnapshot(PhysicsSnapshot {
                server_tick,
                ack_input_seq,
                local,
                remotes,
                pieces,
                swing_kinds,
                grid,
            })
        }
        T::PlayerEvent => {
            let kind = PlayerEventKind::from_u8(r.u8())?;
            let mut e = PlayerEvent {
                kind,
                player_id: r.u16(),
                server_tick: r.u32(),
                input_seq: r.u32(),
                vector: [0.0; 3],
                position: [0.0; 3],
                amount: 0,
                cause: DamageCause::Fall,
            };
            if !r.ok() {
                return None;
            }
            match kind {
                PlayerEventKind::Knockback => e.vector = [r.f32(), r.f32(), r.f32()],
                PlayerEventKind::Respawn => e.position = read_pos64(r),
                PlayerEventKind::Damage => {
                    e.amount = r.u8();
                    e.cause = read_cause(r);
                }
                PlayerEventKind::Death => e.cause = read_cause(r),
            }
            M::PlayerEvent(e)
        }
        T::WorldgenCheck => M::WorldgenCheck(r.u64()),
        T::ChunkData => {
            let form = ChunkForm::from_u8(r.u8());
            r.check(form.is_some());
            let form = form.unwrap_or(ChunkForm::Generated);
            let coord = read_coord(r);
            let revision = r.u32();
            let voxels = if r.ok() && form == ChunkForm::Explicit {
                read_voxels(r)
            } else {
                Vec::new()
            };
            // Optional: the grid (`grid.frames`), a byte at the end, never 0 (home is no byte).
            let tail = r.ok() && !r.at_end();
            let grid = if tail { r.u8() } else { 0 };
            r.check(!tail || grid != 0);
            M::ChunkData(ChunkData {
                form,
                coord,
                revision,
                voxels,
                grid,
            })
        }
        T::HostStatus => M::HostStatus(HostState::from_u8(r.u8())?),
        T::HeldItem => M::HeldItem(r.u16()),
        T::EntitySpawn => {
            let id = r.u32();
            let origin = read_pos64(r);
            let rotation = [r.f32(), r.f32(), r.f32(), r.f32()];
            r.check(rotation.iter().all(|v| v.is_finite()));
            let kind = r.u8();
            let carriers = r.u16();
            let count = usize::from(r.u16());
            r.check(count <= limits::MAX_PIECE_CELLS && usize::from(carriers) <= count);
            let mut cells = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                cells.push(PieceCellNet {
                    offset: [r.i16(), r.i16(), r.i16()],
                    state: r.u16(),
                });
            }
            M::EntitySpawn(EntitySpawn {
                id,
                origin,
                rotation,
                kind,
                carriers,
                cells,
            })
        }
        T::EntityDespawn => M::EntityDespawn(r.u32()),
        T::WorldSettings => M::WorldSettings(crate::world_settings::SettingsFields::read(r)?),
        T::InventoryAction => M::InventoryAction(match r.u8() {
            0 => InventoryAction::Hello,
            1 => InventoryAction::Move {
                from: r.u8(),
                to: r.u8(),
                count: r.u16(),
            },
            2 => InventoryAction::Select { slot: r.u8() },
            3 => InventoryAction::GroundHello,
            4 => InventoryAction::Pick {
                cell: [r.i32(), r.i32(), r.i32()],
            },
            5 => InventoryAction::Combine { recipe: r.u32() },
            _ => return None,
        }),
        T::GroundGone => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_GROUND_GONE_PER_MESSAGE);
            let mut cells = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                cells.push([r.i32(), r.i32(), r.i32()]);
            }
            M::GroundGone(cells)
        }
        T::Inventory => {
            let flags = r.u8();
            let inventory = crate::inventory::Inventory::read(r)?;
            M::Inventory(InventoryMessage { flags, inventory })
        }
        T::ItemDrops => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_ITEM_DROPS_PER_MESSAGE);
            let mut items = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                let id = r.u32();
                let item = r.str(crate::inventory::MAX_ITEM_NAME);
                let count = r.u16();
                let position = [r.f32(), r.f32(), r.f32()];
                r.check(position.iter().all(|v| v.is_finite()));
                items.push(ItemDropNet {
                    id,
                    item,
                    count,
                    position,
                });
            }
            let n = usize::from(r.u16());
            r.check(n <= MAX_ITEM_DROPS_PER_MESSAGE);
            let mut gone = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                gone.push(r.u32());
            }
            M::ItemDrops(ItemDrops { items, gone })
        }
        T::Chop => M::Chop(match r.u8() {
            0 => ChopAction::Hello,
            1 => {
                let tool = r.str(crate::inventory::MAX_ITEM_NAME);
                let cell = [r.i32(), r.i32(), r.i32()];
                let face = r.u8();
                r.check(face < 6);
                let point = [r.f32(), r.f32(), r.f32()];
                r.check(point.iter().all(|v| v.is_finite()));
                let vertical = r.u8() != 0;
                ChopAction::Swing {
                    tool,
                    cell,
                    face,
                    point,
                    vertical,
                }
            }
            2 => {
                let tool = r.str(crate::inventory::MAX_ITEM_NAME);
                let look = [r.f32(), r.f32(), r.f32()];
                r.check(look.iter().all(|v| v.is_finite()));
                ChopAction::Sweep {
                    tool,
                    look,
                    vertical: r.u8() != 0,
                }
            }
            3 => ChopAction::Held {
                tool: r.str(crate::inventory::MAX_ITEM_NAME),
                vertical: r.u8() != 0,
            },
            _ => return None,
        }),
        T::VitalsHello => M::VitalsHello,
        T::CausesHello => M::CausesHello(r.u8()),
        T::SteamHello => M::SteamHello,
        T::Capabilities => M::Capabilities(crate::capabilities::Capabilities::read(r)?),
        T::SwingHello => M::SwingHello,
        T::BitesHello => M::BitesHello,
        T::Steam => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_STEAM_PER_MESSAGE);
            let mut cells = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                cells.push([r.i32(), r.i32(), r.i32()]);
            }
            M::Steam(cells)
        }
        T::CreaturesHello => M::CreaturesHello,
        T::DamageHello => M::DamageHello,
        T::EcologyHello => M::EcologyHello,
        T::Ecology => {
            let day = r.f32();
            let n = usize::from(r.u16());
            r.check(n <= MAX_ECOLOGY_GROUPS && day.is_finite());
            let mut groups = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                let id = r.u32();
                let face = r.u8();
                let species = r.str(MAX_SPECIES_NAME);
                let (x, z, size) = (r.f32(), r.f32(), r.f32());
                r.check(face <= 1 && [x, z, size].iter().all(|v| v.is_finite()));
                groups.push(EcologyGroupNet {
                    id,
                    face,
                    species,
                    x,
                    z,
                    size,
                });
            }
            M::Ecology(EcologyMessage { day, groups })
        }
        T::WorldgenRequest => M::WorldgenRequest,
        T::WorldgenConfig => {
            let n = r.u32() as usize;
            r.check(n <= limits::WORLDGEN_CONFIG_MAX_BYTES);
            let b = r.bytes(n);
            r.check(b.len() == n);
            M::WorldgenConfig(String::from_utf8(b).ok()?)
        }
        T::Damage => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_DAMAGE_PER_MESSAGE);
            let mut cells = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                let c = [r.i32(), r.i32(), r.i32()];
                cells.push((c, crate::damage::DamageView::read(r)));
            }
            let n = usize::from(r.u16());
            r.check(n <= MAX_DAMAGE_PER_MESSAGE);
            let mut gone = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                gone.push([r.i32(), r.i32(), r.i32()]);
            }
            M::Damage(DamageMessage { cells, gone })
        }
        T::Creatures => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_CREATURES_PER_MESSAGE);
            let mut creatures = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                let id = r.u32();
                let model = r.str(MAX_MODEL_NAME);
                let position = [r.f32(), r.f32(), r.f32()];
                let (yaw, speed) = (r.f32(), r.f32());
                r.check(position.iter().chain([&yaw, &speed]).all(|v| v.is_finite()));
                creatures.push(CreatureNet {
                    id,
                    model,
                    position,
                    yaw,
                    speed,
                });
            }
            let n = usize::from(r.u16());
            r.check(n <= MAX_CREATURES_PER_MESSAGE);
            let mut gone = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                gone.push(r.u32());
            }
            M::Creatures(CreaturesMessage { creatures, gone })
        }
        T::Vitals => {
            let m = VitalsMessage {
                seq: r.u32(),
                stamina: r.f32(),
                oxygen: r.f32(),
                flags: r.u8(),
            };
            r.check(m.stamina.is_finite() && m.oxygen.is_finite());
            M::Vitals(m)
        }
        T::HeldTools => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_HELD_TOOLS);
            let mut list = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                list.push(HeldTool {
                    player_id: r.u16(),
                    tool: r.str(crate::inventory::MAX_ITEM_NAME),
                    vertical: r.u8() != 0,
                });
            }
            M::HeldTools(list)
        }
        T::Cuts => {
            let n = usize::from(r.u16());
            r.check(n <= MAX_CUTS_PER_MESSAGE);
            let mut cuts = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                let c = [r.i32(), r.i32(), r.i32()];
                cuts.push((c, crate::chopping::Cut::read(r)));
            }
            let n = usize::from(r.u16());
            r.check(n <= MAX_CUTS_PER_MESSAGE);
            let mut gone = Vec::new();
            for _ in 0..n {
                if !r.ok() {
                    break;
                }
                gone.push([r.i32(), r.i32(), r.i32()]);
            }
            let bites = r.ok() && !r.at_end();
            if bites {
                let mark = r.u8();
                r.check(mark == crate::chopping::BITES_MARK);
                for (_, k) in &mut cuts {
                    k.read_bites(r);
                }
            }
            M::Cuts(CutsMessage { cuts, gone, bites })
        }
        T::BodyEditRequest => {
            let body = r.u32();
            let cell = [r.i16(), r.i16(), r.i16()];
            let face = r.u8();
            r.check(face < 6);
            let action = BlockEditAction::from_u8(r.u8())?;
            let material = if r.ok() && action == BlockEditAction::Place {
                r.u16()
            } else {
                0
            };
            M::BodyEditRequest(BodyEditRequest {
                body,
                cell,
                face,
                action,
                material,
            })
        }
        T::BodyEdit => {
            let body = r.u32();
            let count = usize::from(r.u16());
            r.check(count <= limits::MAX_PIECE_CELLS);
            let mut cells = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                cells.push(PieceCellNet {
                    offset: [r.i16(), r.i16(), r.i16()],
                    state: r.u16(),
                });
            }
            M::BodyEdit(BodyEdit { body, cells })
        }
        T::EditAck => {
            let through = r.u32();
            let count = usize::from(r.u16());
            r.check(count <= limits::MAX_EDIT_ACK_REJECTED);
            let mut rejected = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                let seq = r.u32();
                // Ascending, from 1, none past `through`.
                r.check(seq >= 1 && seq <= through && rejected.last().is_none_or(|&l| seq > l));
                rejected.push(seq);
            }
            M::EditAck(EditAck { through, rejected })
        }
        T::BlockRegistry => {
            let (part, parts, len) = (r.u16(), r.u16(), r.u32() as usize);
            r.check(parts >= 1 && part < parts && len <= MAX_REGISTRY_PART_BYTES);
            if !r.ok() {
                return None;
            }
            M::BlockRegistry(BlockRegistry {
                part,
                parts,
                data: r.bytes(len),
            })
        }
        T::RegionEdit => {
            let op = RegionEditOp::from_u8(r.u8())?;
            let min = read_coord(r);
            let size = [r.u8(), r.u8(), r.u8()];
            r.check(
                size.iter()
                    .all(|&s| s >= 1 && usize::from(s) <= region_edit::MAX_SIDE),
            );
            let mut m = RegionEdit {
                op,
                min,
                size,
                material: 0,
                from: 0,
                cells: Vec::new(),
            };
            if !r.ok() {
                return None;
            }
            match op {
                RegionEditOp::Fill | RegionEditOp::Hollow => m.material = r.u16(),
                RegionEditOp::Replace => {
                    m.from = r.u16();
                    m.material = r.u16();
                }
                RegionEditOp::Paste | RegionEditOp::Structure => {
                    let volume = m.volume();
                    let mut cells = vec![0u16; volume];
                    read_cells(r, volume, |i, v| cells[i] = v);
                    m.cells = cells;
                }
            }
            M::RegionEdit(m)
        }
        T::ChunkUnload => {
            let coords = read_coords(r, 0xFFFF);
            let tail = r.ok() && !r.at_end();
            let grid = if tail { r.u8() } else { 0 };
            r.check(!tail || grid != 0);
            M::ChunkUnload(ChunkUnload { coords, grid })
        }
        T::BlockEditRequest => {
            let action = BlockEditAction::from_u8(r.u8())?;
            let cell = read_coord(r);
            let face = r.u8();
            r.check(face < 6);
            let material = if r.ok() && action == BlockEditAction::Place {
                r.u16()
            } else {
                0
            };
            let piece = r.u8();
            let rows = piece & 0x7f;
            r.check((1..=MAX_PIECE_ROWS).contains(&rows));
            let down = piece & 0x80 != 0;
            let point = (r.ok() && !r.at_end()).then(|| [r.u8(), r.u8(), r.u8()]);
            M::BlockEditRequest(BlockEditRequest {
                action,
                cell,
                face,
                material,
                rows,
                down,
                point,
            })
        }
        T::VoxelModification => {
            let reason = crate::VoxelModificationReason::from_u8(r.u8())?;
            let server_tick = r.u32();
            let count = r.u16();
            r.check(count >= 1);
            let mut chunks = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                let coord = read_coord(r);
                let revision = r.u32();
                let n = r.u16();
                r.check(n >= 1);
                let mut changes = Vec::new();
                for _ in 0..n {
                    if !r.ok() {
                        break;
                    }
                    let v = VoxelChange {
                        index: r.u16(),
                        material: r.u16(),
                    };
                    r.check(usize::from(v.index) < CHUNK_VOLUME);
                    changes.push(v);
                }
                chunks.push(ChunkChanges {
                    coord,
                    revision,
                    changes,
                });
            }
            M::VoxelModification(VoxelModification {
                reason,
                server_tick,
                chunks,
            })
        }
        T::ChunkResync => M::ChunkResync(read_coords(r, limits::MAX_RESYNC_CHUNKS)),
        T::ChunkRequest => M::ChunkRequest(read_coords(r, limits::MAX_RESYNC_CHUNKS)),
        T::LodIndex => {
            let flags = r.u8();
            r.check(flags <= 1);
            let count = r.u32() as usize;
            r.check(count <= limits::MAX_LOD_INDEX_ENTRIES);
            let entries = if r.ok() {
                read_index_entries(r, count)
            } else {
                Vec::new()
            };
            M::LodIndex(LodIndex {
                last: flags == 1,
                entries,
            })
        }
        T::LodIndexUpdate => {
            let count = usize::from(r.u16());
            r.check((1..=limits::MAX_LOD_INDEX_ENTRIES).contains(&count));
            M::LodIndexUpdate(if r.ok() {
                read_index_entries(r, count)
            } else {
                Vec::new()
            })
        }
        T::LodRequest => {
            let count = i64::from(r.u8());
            r.check((1..=lod::MAX_REQUEST_SECTIONS).contains(&count));
            let mut sections = Vec::new();
            for _ in 0..count {
                if !r.ok() {
                    break;
                }
                let level = r.u8();
                r.check(level >= 1 && i64::from(level) <= lod::MAX_LEVEL);
                sections.push(LodSectionRequest {
                    level,
                    section: read_coord(r),
                    known_revision: r.u32(),
                });
            }
            M::LodRequest(sections)
        }
        T::LodData => {
            let form = crate::LodForm::from_u8(r.u8());
            r.check(form.is_some());
            let form = form.unwrap_or(crate::LodForm::Generated);
            let level = r.u8();
            r.check(i64::from(level) <= lod::MAX_LEVEL);
            let section = read_coord(r);
            let revision = r.u32();
            let cells = if r.ok() && form == crate::LodForm::Explicit {
                read_lod_cells(r)
            } else {
                Vec::new()
            };
            M::LodData(LodData {
                form,
                level,
                section,
                revision,
                cells,
            })
        }
    };
    r.ok().then_some(m)
}

#[cfg(test)]
mod ecology_tests {
    use super::*;

    #[test]
    fn ecology_round_trips_and_rejects_bad_ones() {
        let g = |id, face| EcologyGroupNet {
            id,
            face,
            species: "dwell:deer".into(),
            x: -1200.5,
            z: 340.0,
            size: 5.5,
        };
        let m = Message::Ecology(EcologyMessage {
            day: 12.0,
            groups: vec![g(3, 0), g(7, 1)],
        });
        let b = m.encode();
        assert_eq!(b[0], MessageType::Ecology as u8);
        assert_eq!(Message::decode(&b), Some(m.clone()));
        assert_eq!(Message::decode(&b[..b.len() - 1]), None);
        let h = Message::EcologyHello.encode();
        assert_eq!(Message::decode(&h), Some(Message::EcologyHello));
        // A face past B, and a NaN head, are malformed.
        let bad = Message::Ecology(EcologyMessage {
            day: 1.0,
            groups: vec![g(1, 2)],
        });
        assert_eq!(Message::decode(&bad.encode()), None);
        let mut nan = m.encode();
        let at = nan.len() - 4;
        nan[at..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Message::decode(&nan), None);
    }
}

#[cfg(test)]
mod creature_tests {
    use super::*;

    #[test]
    fn creatures_round_trip_and_reject_bad_ones() {
        let m = Message::Creatures(CreaturesMessage {
            creatures: vec![CreatureNet {
                id: 9,
                model: "deer".into(),
                position: [1.5, 64.25, -3.0],
                yaw: 90.0,
                speed: 1.3,
            }],
            gone: vec![2, 5],
        });
        let b = m.encode();
        assert_eq!(Message::decode(&b), Some(m.clone()));
        assert_eq!(Message::decode(&b[..b.len() - 1]), None);
        let h = Message::CreaturesHello.encode();
        assert_eq!(Message::decode(&h), Some(Message::CreaturesHello));
        let mut nan = m.encode();
        // The speed (the last f32 before the gone list: 2 + 2 x 4 bytes from the end) as NaN.
        let at = nan.len() - 2 - 8 - 4;
        nan[at..at + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Message::decode(&nan), None);
    }
}
