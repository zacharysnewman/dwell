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
    /// The block registry hash: runtime state ids mean the same blocks only if it matches.
    pub registry_hash: u64,
}

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChunkUnload {
    pub coords: Vec<ChunkCoordNet>,
}

/// C→S: break the targeted cell, or place `material` against its `face` (0 +X, 1 −X, 2 +Y, 3 −Y,
/// 4 +Z, 5 −Z).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEditRequest {
    pub action: BlockEditAction,
    pub cell: [i32; 3],
    pub face: u8,
    pub material: u16,
}

/// C→S: one edit of a box of cells (§6.5) — `size` cells from the `min` corner, each side
/// 1..=`region_edit::MAX_SIDE`. Fill and Hollow set `material` (Hollow: the box's shell, air
/// inside); Replace turns cells holding `from` into `material`; Paste sets the box to `cells`.
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
    /// Paste: `volume()` states in layer order, x fastest, then z, then y
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
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhysicsSnapshot {
    pub server_tick: u32,
    /// The last input of this client processed.
    pub ack_input_seq: u32,
    pub local: LocalPlayerState,
    pub remotes: Vec<RemotePlayerState>,
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
    DatagramPong(DatagramPong),
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
}

impl Message {
    pub fn message_type(&self) -> MessageType {
        use Message as M;
        match self {
            M::DatagramPing(_) => MessageType::DatagramPing,
            M::DatagramPong(_) => MessageType::DatagramPong,
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
        }
        M::ChunkUnload(m) => write_coords(w, &m.coords, 0xFFFF),
        M::BlockEditRequest(m) => {
            w.u8(m.action as u8);
            write_coord(w, &m.cell);
            w.u8(m.face);
            if m.action == BlockEditAction::Place {
                w.u16(m.material);
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
                RegionEditOp::Paste => {
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
                };
                r.check(p.flags & !player_flags::ALL == 0);
                remotes.push(p);
            }
            M::PhysicsSnapshot(PhysicsSnapshot {
                server_tick,
                ack_input_seq,
                local,
                remotes,
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
            M::ChunkData(ChunkData {
                form,
                coord,
                revision,
                voxels,
            })
        }
        T::HostStatus => M::HostStatus(HostState::from_u8(r.u8())?),
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
                RegionEditOp::Paste => {
                    let volume = m.volume();
                    let mut cells = vec![0u16; volume];
                    read_cells(r, volume, |i, v| cells[i] = v);
                    m.cells = cells;
                }
            }
            M::RegionEdit(m)
        }
        T::ChunkUnload => M::ChunkUnload(ChunkUnload {
            coords: read_coords(r, 0xFFFF),
        }),
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
            M::BlockEditRequest(BlockEditRequest {
                action,
                cell,
                face,
                material,
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
