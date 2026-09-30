// Protocol messages (ARCHITECTURE.md §8.3). Mirrors server/core/include/dwell/protocol/messages.h;
// layouts are pinned by shared golden vectors.
import { ByteReader, ByteWriter, DecodeError } from './bytes';
import { CHUNK_VOLUME, readLodCells, readVoxels, writeLodCells, writeVoxels } from './chunkVoxels';
import {
  AUTH_DOMAIN_TAG,
  BlockEditAction,
  ChunkForm,
  ControllerFlags,
  DamageCause,
  GroundKind,
  HostState,
  InputButtons,
  Limits,
  Lod,
  LodForm,
  MAX_INPUTS_PER_DATAGRAM,
  MessageType,
  PlayerEventKind,
  PlayerFlags,
  PlayerState,
  RejectReason,
  VoxelModificationReason,
  World,
} from './constants.gen';

export const STATUS_FLAG_ONLINE_MODE = 1 << 0;

export type Vec3 = [number, number, number];
/** Chunk coordinate (i32 × 3). */
export type ChunkCoord = [number, number, number];

/** A modified section at LOD_INDEX_LEVEL (§6.6) and its lodRevision. */
export interface LodIndexEntry {
  i: number;
  k: number;
  revision: number;
}

/** One section of a LodRequest: [level, i, j, k] and the revision the client holds (0 = none). */
export interface LodSectionRequest {
  level: number;
  section: Vec3;
  knownRevision: number;
}

/** One tick of quantized input (see quantizeInput in predict/input.ts). */
export interface InputFrame {
  seq: number;
  moveX: number;
  moveY: number;
  buttons: number;
  yaw: number;
  pitch: number;
}

/** Local controller state needed to resume simulation (PLAYER_CONTROLLER.md §8.4). */
export interface ControllerState {
  flags: number;
  currentX: number;
  currentZ: number;
  externalX: number;
  externalZ: number;
  contributionX: number;
  contributionZ: number;
  accumulatedY: number;
  platformY: number;
  targetY: number;
  groundVelocityY: number;
  groundKind: GroundKind;
  groundId: number;
  bufferTicks: number;
  coyoteTicks: number;
  stepGrace: number;
  ladder: Vec3; // on the wire only while climbing
  released: [number, number]; // x, z; only when hasReleased
}

// World positions (protocol v4, ADR 0011): `pos64` (f64 × 3) where prediction must match the
// server exactly, `posfix` (i32 × 3 in 1/positionFixedScale m) elsewhere.
export interface LocalPlayerState {
  position: Vec3; // capsule centre (pos64)
  velocity: Vec3;
  flags: number;
  health: number;
  state: PlayerState;
  /** Inputs queued on the server (the client steers its tick rate to keep this near target). */
  inputBuffer: number;
  /** Input seq of the latest knockback applied to this player (0 = none). */
  lastKnockbackSeq: number;
  controller: ControllerState;
}

export interface RemotePlayerState {
  playerId: number;
  position: Vec3; // feet (posfix)
  velocity: Vec3; // f16 on the wire
  yaw: number;
  pitch: number;
  state: PlayerState;
  flags: number;
}

/** Changes to one chunk in a VoxelModification (§8.3). */
export interface ChunkChanges {
  coord: ChunkCoord;
  /** The chunk's revision after the changes. */
  revision: number;
  /** Interleaved (localIndex, material) pairs; localIndex = x | y << 5 | z << 10. */
  changes: Uint16Array;
}

export type Message =
  | { type: typeof MessageType.DatagramPing; seq: number; clientTimeMs: number }
  | { type: typeof MessageType.DatagramPong; seq: number; clientTimeMs: number; serverTick: number }
  | { type: typeof MessageType.StatusRequest }
  | {
      type: typeof MessageType.StatusResponse;
      protocolVersion: number;
      serverName: string;
      motd: string;
      players: number;
      maxPlayers: number;
      flags: number;
    }
  | {
      type: typeof MessageType.ClientHello;
      protocolVersion: number;
      clientVersion: string;
      publicKey: Uint8Array;
      displayName: string;
    }
  | { type: typeof MessageType.Challenge; nonce: Uint8Array }
  | { type: typeof MessageType.ClientAuth; signature: Uint8Array }
  | {
      type: typeof MessageType.Welcome;
      playerId: number;
      worldSeed: bigint;
      generatorVersion: number;
      serverTick: number;
      /** Chunk the client generates and hashes for WorldgenCheck (§6.3). */
      verificationChunk: ChunkCoord;
      /** WelcomeFlags, e.g. whether this player may use creative flight (§8.3). */
      flags: number;
    }
  | {
      type: typeof MessageType.WorldgenCheck;
      /** FNV-1a 64 of the client-generated verification chunk; 0n asks for full-chunk mode. */
      hash: bigint;
    }
  | {
      type: typeof MessageType.ChunkData;
      form: ChunkForm;
      coord: ChunkCoord;
      revision: number;
      /** Explicit: CHUNK_VOLUME materials in chunk index order (x | y << 5 | z << 10); else null. */
      voxels: Uint16Array | null;
    }
  | { type: typeof MessageType.ChunkUnload; coords: ChunkCoord[] }
  /** A friend-world host's page was hidden (the world paused) or shown again (§10.2). */
  | { type: typeof MessageType.HostStatus; state: HostState }
  | {
      type: typeof MessageType.BlockEditRequest;
      action: BlockEditAction;
      /** World voxel coordinate of the targeted cell. */
      cell: Vec3;
      /** Targeted face: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z. */
      face: number;
      /** Place only (0 for Break). */
      material: number;
    }
  | {
      type: typeof MessageType.VoxelModification;
      reason: VoxelModificationReason;
      serverTick: number;
      chunks: ChunkChanges[];
    }
  | { type: typeof MessageType.ChunkResync; coords: ChunkCoord[] }
  | { type: typeof MessageType.ChunkRequest; coords: ChunkCoord[] }
  | { type: typeof MessageType.LodIndex; last: boolean; entries: LodIndexEntry[] }
  | { type: typeof MessageType.LodIndexUpdate; entries: LodIndexEntry[] }
  | { type: typeof MessageType.LodRequest; sections: LodSectionRequest[] }
  | {
      type: typeof MessageType.LodData;
      form: LodForm;
      level: number;
      section: Vec3;
      revision: number;
      /** Explicit: the 34³ cells with the apron (lod/grid.ts `lodCell` order); else null. */
      cells: Uint16Array | null;
    }
  | { type: typeof MessageType.Reject; reason: RejectReason; message: string }
  | { type: typeof MessageType.Ping; seq: number; clientTimeMs: number }
  | {
      type: typeof MessageType.Pong;
      seq: number;
      clientTimeMs: number;
      serverTick: number;
      serverTimeMs: number;
    }
  | { type: typeof MessageType.PlayerInput; lastSnapshotTick: number; inputs: InputFrame[] }
  | {
      type: typeof MessageType.PhysicsSnapshot;
      serverTick: number;
      ackInputSeq: number;
      local: LocalPlayerState;
      remotes: RemotePlayerState[];
    }
  | {
      type: typeof MessageType.PlayerEvent;
      kind: PlayerEventKind;
      playerId: number;
      serverTick: number;
      inputSeq: number;
      /** Knockback: velocity change. */
      vector: Vec3;
      /** Respawn: feet position (pos64). */
      position: Vec3;
      /** Damage only. */
      amount: number;
      /** Damage and Death. */
      cause: DamageCause;
    };

function fixedLength(b: Uint8Array, n: number, what: string): Uint8Array {
  if (b.length !== n) throw new RangeError(`${what} must be ${String(n)} bytes`);
  return b;
}

export function encode(m: Message): Uint8Array<ArrayBuffer> {
  const w = new ByteWriter();
  w.u8(m.type);
  switch (m.type) {
    case MessageType.DatagramPing:
    case MessageType.Ping:
      w.u32(m.seq);
      w.f64(m.clientTimeMs);
      break;
    case MessageType.DatagramPong:
      w.u32(m.seq);
      w.f64(m.clientTimeMs);
      w.u32(m.serverTick);
      break;
    case MessageType.StatusRequest:
      break;
    case MessageType.StatusResponse:
      w.u16(m.protocolVersion);
      w.str(m.serverName, Limits.serverNameMaxBytes);
      w.str(m.motd, Limits.motdMaxBytes);
      w.u16(m.players);
      w.u16(m.maxPlayers);
      w.u8(m.flags);
      break;
    case MessageType.ClientHello:
      w.u16(m.protocolVersion);
      w.str(m.clientVersion, Limits.clientVersionMaxBytes);
      w.bytes(fixedLength(m.publicKey, 32, 'publicKey'));
      w.str(m.displayName, Limits.displayNameMaxBytes);
      break;
    case MessageType.Challenge:
      w.bytes(fixedLength(m.nonce, 32, 'nonce'));
      break;
    case MessageType.ClientAuth:
      w.bytes(fixedLength(m.signature, 64, 'signature'));
      break;
    case MessageType.Welcome:
      w.u16(m.playerId);
      w.u64(m.worldSeed);
      w.u32(m.generatorVersion);
      w.u32(m.serverTick);
      for (const v of m.verificationChunk) w.i32(v);
      w.u8(m.flags);
      break;
    case MessageType.WorldgenCheck:
      w.u64(m.hash);
      break;
    case MessageType.ChunkData:
      w.u8(m.form);
      for (const v of m.coord) w.i32(v);
      w.u32(m.revision);
      if (m.form === ChunkForm.Explicit) {
        if (!m.voxels) throw new RangeError('Explicit ChunkData needs voxels');
        writeVoxels(w, m.voxels);
      }
      break;
    case MessageType.HostStatus:
      w.u8(m.state);
      break;
    case MessageType.ChunkUnload:
      if (m.coords.length < 1 || m.coords.length > 0xffff) throw new RangeError('unload count');
      w.u16(m.coords.length);
      for (const c of m.coords) for (const v of c) w.i32(v);
      break;
    case MessageType.BlockEditRequest:
      w.u8(m.action);
      for (const v of m.cell) w.i32(v);
      w.u8(m.face);
      if (m.action === BlockEditAction.Place) w.u16(m.material);
      break;
    case MessageType.VoxelModification:
      if (m.chunks.length < 1 || m.chunks.length > 0xffff) throw new RangeError('chunk count');
      w.u8(m.reason);
      w.u32(m.serverTick);
      w.u16(m.chunks.length);
      for (const c of m.chunks) {
        const n = c.changes.length / 2;
        if (n < 1 || n > 0xffff || !Number.isInteger(n)) throw new RangeError('change count');
        for (const v of c.coord) w.i32(v);
        w.u32(c.revision);
        w.u16(n);
        for (const v of c.changes) w.u16(v);
      }
      break;
    case MessageType.ChunkResync:
    case MessageType.ChunkRequest:
      if (m.coords.length < 1 || m.coords.length > Limits.maxResyncChunks) {
        throw new RangeError('resync count');
      }
      w.u16(m.coords.length);
      for (const c of m.coords) for (const v of c) w.i32(v);
      break;
    case MessageType.LodIndex:
      if (m.entries.length > Limits.maxLodIndexEntries) throw new RangeError('index entries');
      w.u8(m.last ? 1 : 0);
      w.u32(m.entries.length);
      writeIndexEntries(w, m.entries);
      break;
    case MessageType.LodIndexUpdate:
      if (m.entries.length < 1 || m.entries.length > Limits.maxLodIndexEntries) {
        throw new RangeError('index update entries');
      }
      w.u16(m.entries.length);
      writeIndexEntries(w, m.entries);
      break;
    case MessageType.LodRequest:
      if (m.sections.length < 1 || m.sections.length > Lod.maxRequestSections) {
        throw new RangeError('LOD request count');
      }
      w.u8(m.sections.length);
      for (const s of m.sections) {
        w.u8(s.level);
        for (const v of s.section) w.i32(v);
        w.u32(s.knownRevision);
      }
      break;
    case MessageType.LodData:
      w.u8(m.form);
      w.u8(m.level);
      for (const v of m.section) w.i32(v);
      w.u32(m.revision);
      if (m.form === LodForm.Explicit) {
        if (!m.cells) throw new RangeError('Explicit LodData needs cells');
        writeLodCells(w, m.cells);
      }
      break;
    case MessageType.Reject:
      w.u8(m.reason);
      w.str(m.message, Limits.rejectMessageMaxBytes);
      break;
    case MessageType.Pong:
      w.u32(m.seq);
      w.f64(m.clientTimeMs);
      w.u32(m.serverTick);
      w.f64(m.serverTimeMs);
      break;
    case MessageType.PlayerInput: {
      w.u32(m.lastSnapshotTick);
      const inputs = m.inputs.slice(-MAX_INPUTS_PER_DATAGRAM);
      w.u8(inputs.length);
      for (const f of inputs) {
        w.u32(f.seq);
        w.i8(f.moveX);
        w.i8(f.moveY);
        w.u16(f.buttons);
        w.i16(f.yaw);
        w.i16(f.pitch);
      }
      break;
    }
    case MessageType.PhysicsSnapshot:
      w.u32(m.serverTick);
      w.u32(m.ackInputSeq);
      writePos64(w, m.local.position);
      writeVec3(w, m.local.velocity);
      w.u8(m.local.flags);
      w.u8(m.local.health);
      w.u8(m.local.state);
      w.u8(m.local.inputBuffer);
      w.u32(m.local.lastKnockbackSeq);
      writeController(w, m.local.controller);
      w.u8(Math.min(m.remotes.length, 255));
      for (const r of m.remotes.slice(0, 255)) {
        w.u16(r.playerId);
        writePosFix(w, r.position);
        for (const v of r.velocity) w.f16(v);
        w.i16(r.yaw);
        w.i16(r.pitch);
        w.u8(r.state);
        w.u8(r.flags);
      }
      break;
    case MessageType.PlayerEvent:
      w.u8(m.kind);
      w.u16(m.playerId);
      w.u32(m.serverTick);
      w.u32(m.inputSeq);
      switch (m.kind) {
        case PlayerEventKind.Knockback:
          writeVec3(w, m.vector);
          break;
        case PlayerEventKind.Respawn:
          writePos64(w, m.position);
          break;
        case PlayerEventKind.Damage:
          w.u8(m.amount);
          w.u8(m.cause);
          break;
        case PlayerEventKind.Death:
          w.u8(m.cause);
          break;
      }
      break;
  }
  return w.finish();
}

function writeVec3(w: ByteWriter, v: Vec3): void {
  for (const x of v) w.f32(x);
}

function writePos64(w: ByteWriter, v: Vec3): void {
  for (const x of v) w.f64(x);
}

/** posfix: the nearest multiple of 1/positionFixedScale m (halves round up), clamped to i32. */
export function toFixedPosition(v: number): number {
  const scaled = Math.floor(v * World.positionFixedScale + 0.5);
  if (!(scaled > -2147483648)) return -2147483648; // also NaN
  return Math.min(scaled, 2147483647);
}

function writePosFix(w: ByteWriter, v: Vec3): void {
  for (const x of v) w.i32(toFixedPosition(x));
}

function writeController(w: ByteWriter, c: ControllerState): void {
  w.u8(c.flags);
  for (const v of [
    c.currentX,
    c.currentZ,
    c.externalX,
    c.externalZ,
    c.contributionX,
    c.contributionZ,
    c.accumulatedY,
    c.platformY,
    c.targetY,
    c.groundVelocityY,
  ]) {
    w.f32(v);
  }
  w.u8(c.groundKind);
  w.u16(c.groundId);
  w.u8(c.bufferTicks);
  w.u8(c.coyoteTicks);
  w.u8(c.stepGrace);
  if (c.flags & ControllerFlags.climbing) for (const v of c.ladder) w.i32(v);
  if (c.flags & ControllerFlags.hasReleased) for (const v of c.released) w.i32(v);
}

const allBits = (flags: Record<string, number>) =>
  Object.values(flags).reduce((mask, bit) => mask | bit, 0);
const INPUT_BUTTONS = allBits(InputButtons);
const PLAYER_FLAGS = allBits(PlayerFlags);
const CONTROLLER_FLAGS = allBits(ControllerFlags);
const maxOf = (values: Record<string, number>) => Math.max(...Object.values(values));
const MAX_STATE = maxOf(PlayerState);
const MAX_GROUND_KIND = maxOf(GroundKind);
const MAX_EVENT_KIND = maxOf(PlayerEventKind);
const MAX_CAUSE = maxOf(DamageCause);

function readVec3(r: ByteReader): Vec3 {
  return [r.f32(), r.f32(), r.f32()];
}

// The world and the sky above it, up to the creative-flight ceiling (posfix positions of other
// players clamp at ±8,388 km).
const POS64_LIMIT = World.pos64Limit;

function readPos64(r: ByteReader): Vec3 {
  const v: Vec3 = [r.f64(), r.f64(), r.f64()];
  for (const x of v) r.check(x >= -POS64_LIMIT && x <= POS64_LIMIT, 'position out of range');
  return v;
}

function readPosFix(r: ByteReader): Vec3 {
  return [r.i32(), r.i32(), r.i32()].map((x) => x / World.positionFixedScale) as Vec3;
}

function readState(r: ByteReader): PlayerState {
  const v = r.u8();
  r.check(v <= MAX_STATE, 'unknown player state');
  return v as PlayerState;
}

function readCause(r: ByteReader): DamageCause {
  const v = r.u8();
  r.check(v >= 1 && v <= MAX_CAUSE, 'unknown damage cause');
  return v as DamageCause;
}

function readController(r: ByteReader): ControllerState {
  const flags = r.u8();
  r.check((flags & ~CONTROLLER_FLAGS) === 0, 'unknown controller flags');
  const f = Array.from({ length: 10 }, () => r.f32());
  const groundKind = r.u8();
  r.check(groundKind <= MAX_GROUND_KIND, 'unknown ground kind');
  const c: ControllerState = {
    flags,
    currentX: f[0] ?? 0,
    currentZ: f[1] ?? 0,
    externalX: f[2] ?? 0,
    externalZ: f[3] ?? 0,
    contributionX: f[4] ?? 0,
    contributionZ: f[5] ?? 0,
    accumulatedY: f[6] ?? 0,
    platformY: f[7] ?? 0,
    targetY: f[8] ?? 0,
    groundVelocityY: f[9] ?? 0,
    groundKind: groundKind as GroundKind,
    groundId: r.u16(),
    bufferTicks: r.u8(),
    coyoteTicks: r.u8(),
    stepGrace: r.u8(),
    ladder: [0, 0, 0],
    released: [0, 0],
  };
  if (flags & ControllerFlags.climbing) c.ladder = [r.i32(), r.i32(), r.i32()];
  if (flags & ControllerFlags.hasReleased) c.released = [r.i32(), r.i32()];
  return c;
}

const rejectReasons = new Set<number>(Object.values(RejectReason));
const editActions = new Set<number>(Object.values(BlockEditAction));
const hostStates = new Set<number>(Object.values(HostState));
const modificationReasons = new Set<number>(Object.values(VoxelModificationReason));
const chunkForms = new Set<number>(Object.values(ChunkForm));
const lodForms = new Set<number>(Object.values(LodForm));

function writeIndexEntries(w: ByteWriter, entries: LodIndexEntry[]): void {
  for (const e of entries) {
    w.i32(e.i);
    w.i32(e.k);
    w.u32(e.revision);
  }
}

function readIndexEntries(r: ByteReader, count: number): LodIndexEntry[] {
  const out: LodIndexEntry[] = [];
  for (let n = 0; n < count; n++) out.push({ i: r.i32(), k: r.i32(), revision: r.u32() });
  return out;
}

function coord(r: ByteReader): ChunkCoord {
  return [r.i32(), r.i32(), r.i32()];
}

function decodeBody(r: ByteReader, type: number): Message {
  switch (type) {
    case MessageType.DatagramPing:
      return { type, seq: r.u32(), clientTimeMs: r.f64() };
    case MessageType.DatagramPong:
      return { type, seq: r.u32(), clientTimeMs: r.f64(), serverTick: r.u32() };
    case MessageType.StatusRequest:
      return { type };
    case MessageType.StatusResponse:
      return {
        type,
        protocolVersion: r.u16(),
        serverName: r.str(Limits.serverNameMaxBytes),
        motd: r.str(Limits.motdMaxBytes),
        players: r.u16(),
        maxPlayers: r.u16(),
        flags: r.u8(),
      };
    case MessageType.ClientHello:
      return {
        type,
        protocolVersion: r.u16(),
        clientVersion: r.str(Limits.clientVersionMaxBytes),
        publicKey: r.fixed(32),
        displayName: r.str(Limits.displayNameMaxBytes),
      };
    case MessageType.Challenge:
      return { type, nonce: r.fixed(32) };
    case MessageType.ClientAuth:
      return { type, signature: r.fixed(64) };
    case MessageType.Welcome:
      return {
        type,
        playerId: r.u16(),
        worldSeed: r.u64(),
        generatorVersion: r.u32(),
        serverTick: r.u32(),
        verificationChunk: coord(r),
        flags: r.u8(),
      };
    case MessageType.WorldgenCheck:
      return { type, hash: r.u64() };
    case MessageType.ChunkData: {
      const form = r.u8();
      r.check(chunkForms.has(form), 'chunk form');
      const at = coord(r);
      const revision = r.u32();
      const voxels = form === ChunkForm.Explicit ? readVoxels(r) : null;
      return { type, form: form as ChunkForm, coord: at, revision, voxels };
    }
    case MessageType.HostStatus: {
      const state = r.u8();
      r.check(hostStates.has(state), 'host state');
      return { type, state: state as HostState };
    }
    case MessageType.ChunkUnload: {
      const count = r.u16();
      r.check(count >= 1, 'unload count');
      const coords: ChunkCoord[] = [];
      for (let i = 0; i < count; i++) coords.push(coord(r));
      return { type, coords };
    }
    case MessageType.BlockEditRequest: {
      const action = r.u8();
      r.check(editActions.has(action), 'unknown block edit action');
      const cell: Vec3 = [r.i32(), r.i32(), r.i32()];
      const face = r.u8();
      r.check(face < 6, 'bad face');
      const material = action === BlockEditAction.Place ? r.u16() : 0;
      return { type, action: action as BlockEditAction, cell, face, material };
    }
    case MessageType.VoxelModification: {
      const reason = r.u8();
      r.check(modificationReasons.has(reason), 'unknown modification reason');
      const serverTick = r.u32();
      const count = r.u16();
      r.check(count >= 1, 'modification chunk count');
      const chunks: ChunkChanges[] = [];
      for (let i = 0; i < count; i++) {
        const at = coord(r);
        const revision = r.u32();
        const n = r.u16();
        r.check(n >= 1, 'modification change count');
        const changes = new Uint16Array(n * 2);
        for (let k = 0; k < n; k++) {
          const index = r.u16();
          r.check(index < CHUNK_VOLUME, 'voxel index');
          changes[k * 2] = index;
          changes[k * 2 + 1] = r.u16();
        }
        chunks.push({ coord: at, revision, changes });
      }
      return { type, reason: reason as VoxelModificationReason, serverTick, chunks };
    }
    case MessageType.ChunkResync:
    case MessageType.ChunkRequest: {
      const count = r.u16();
      r.check(count >= 1 && count <= Limits.maxResyncChunks, 'resync count');
      const coords: ChunkCoord[] = [];
      for (let i = 0; i < count; i++) coords.push(coord(r));
      return { type, coords };
    }
    case MessageType.LodIndex: {
      const flags = r.u8();
      r.check(flags <= 1, 'index flags');
      const count = r.u32();
      r.check(count <= Limits.maxLodIndexEntries, 'index entries');
      return { type, last: flags === 1, entries: readIndexEntries(r, count) };
    }
    case MessageType.LodIndexUpdate: {
      const count = r.u16();
      r.check(count >= 1 && count <= Limits.maxLodIndexEntries, 'index update entries');
      return { type, entries: readIndexEntries(r, count) };
    }
    case MessageType.LodRequest: {
      const count = r.u8();
      r.check(count >= 1 && count <= Lod.maxRequestSections, 'LOD request count');
      const sections: LodSectionRequest[] = [];
      for (let n = 0; n < count; n++) {
        const level = r.u8();
        r.check(level >= 1 && level <= Lod.maxLevel, 'LOD level');
        sections.push({ level, section: [r.i32(), r.i32(), r.i32()], knownRevision: r.u32() });
      }
      return { type, sections };
    }
    case MessageType.LodData: {
      const form = r.u8();
      r.check(lodForms.has(form), 'LOD form');
      const level = r.u8();
      r.check(level <= Lod.maxLevel, 'LOD level');
      const section: Vec3 = [r.i32(), r.i32(), r.i32()];
      const revision = r.u32();
      const cells = form === LodForm.Explicit ? readLodCells(r) : null;
      return { type, form: form as LodForm, level, section, revision, cells };
    }
    case MessageType.Reject: {
      const reason = r.u8();
      if (!rejectReasons.has(reason)) throw new DecodeError('unknown reject reason');
      return { type, reason: reason as RejectReason, message: r.str(Limits.rejectMessageMaxBytes) };
    }
    case MessageType.Ping:
      return { type, seq: r.u32(), clientTimeMs: r.f64() };
    case MessageType.Pong:
      return {
        type,
        seq: r.u32(),
        clientTimeMs: r.f64(),
        serverTick: r.u32(),
        serverTimeMs: r.f64(),
      };
    case MessageType.PlayerInput: {
      const lastSnapshotTick = r.u32();
      const count = r.u8();
      r.check(count >= 1 && count <= MAX_INPUTS_PER_DATAGRAM, 'bad input count');
      const inputs: InputFrame[] = [];
      for (let i = 0; i < count; i++) {
        const f = {
          seq: r.u32(),
          moveX: r.i8(),
          moveY: r.i8(),
          buttons: r.u16(),
          yaw: r.i16(),
          pitch: r.i16(),
        };
        r.check((f.buttons & ~INPUT_BUTTONS) === 0, 'unknown input buttons');
        inputs.push(f);
      }
      return { type, lastSnapshotTick, inputs };
    }
    case MessageType.PhysicsSnapshot: {
      const serverTick = r.u32();
      const ackInputSeq = r.u32();
      const position = readPos64(r);
      const velocity = readVec3(r);
      const flags = r.u8();
      r.check((flags & ~PLAYER_FLAGS) === 0, 'unknown player flags');
      const health = r.u8();
      const state = readState(r);
      const inputBuffer = r.u8();
      const lastKnockbackSeq = r.u32();
      const controller = readController(r);
      const count = r.u8();
      const remotes: RemotePlayerState[] = [];
      for (let i = 0; i < count; i++) {
        const remote: RemotePlayerState = {
          playerId: r.u16(),
          position: readPosFix(r),
          velocity: [r.f16(), r.f16(), r.f16()],
          yaw: r.i16(),
          pitch: r.i16(),
          state: readState(r),
          flags: r.u8(),
        };
        r.check((remote.flags & ~PLAYER_FLAGS) === 0, 'unknown player flags');
        remotes.push(remote);
      }
      return {
        type,
        serverTick,
        ackInputSeq,
        local: {
          position,
          velocity,
          flags,
          health,
          state,
          inputBuffer,
          lastKnockbackSeq,
          controller,
        },
        remotes,
      };
    }
    case MessageType.PlayerEvent: {
      const kind = r.u8();
      r.check(kind >= 1 && kind <= MAX_EVENT_KIND, 'unknown player event');
      const event = {
        type,
        kind: kind as PlayerEventKind,
        playerId: r.u16(),
        serverTick: r.u32(),
        inputSeq: r.u32(),
        vector: [0, 0, 0] as Vec3,
        position: [0, 0, 0] as Vec3,
        amount: 0,
        cause: DamageCause.Fall as DamageCause,
      };
      switch (event.kind) {
        case PlayerEventKind.Knockback:
          event.vector = readVec3(r);
          break;
        case PlayerEventKind.Respawn:
          event.position = readPos64(r);
          break;
        case PlayerEventKind.Damage:
          event.amount = r.u8();
          event.cause = readCause(r);
          break;
        case PlayerEventKind.Death:
          event.cause = readCause(r);
          break;
      }
      return event;
    }
    default:
      throw new DecodeError(`unknown message type ${String(type)}`);
  }
}

/** Decodes exactly one message; throws DecodeError on malformed input. */
export function decode(bytes: Uint8Array): Message {
  const r = new ByteReader(bytes);
  const m = decodeBody(r, r.u8());
  r.end();
  return m;
}

/** Bytes the client signs in ClientAuth (ADR 0004): tag ‖ nonce ‖ transportBinding ‖ publicKey. */
export function authTranscript(
  nonce: Uint8Array,
  transportBinding: Uint8Array,
  publicKey: Uint8Array,
): Uint8Array<ArrayBuffer> {
  const w = new ByteWriter();
  w.bytes(new TextEncoder().encode(AUTH_DOMAIN_TAG));
  w.bytes(fixedLength(nonce, 32, 'nonce'));
  w.bytes(fixedLength(transportBinding, 32, 'transportBinding'));
  w.bytes(fixedLength(publicKey, 32, 'publicKey'));
  return w.finish();
}
