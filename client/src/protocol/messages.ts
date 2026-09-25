// Protocol messages (ARCHITECTURE.md §8.3). Mirrors server/core/include/dwell/protocol/messages.h;
// layouts are pinned by shared golden vectors.
import { ByteReader, ByteWriter, DecodeError } from './bytes';
import {
  AUTH_DOMAIN_TAG,
  ControllerFlags,
  DamageCause,
  GroundKind,
  InputButtons,
  Limits,
  MAX_INPUTS_PER_DATAGRAM,
  MessageType,
  PlayerEventKind,
  PlayerFlags,
  PlayerState,
  RejectReason,
} from './constants.gen';

export const STATUS_FLAG_ONLINE_MODE = 1 << 0;

export type Vec3 = [number, number, number];

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

export interface LocalPlayerState {
  position: Vec3; // capsule centre
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
  position: Vec3;
  velocity: Vec3; // f16 on the wire
  yaw: number;
  pitch: number;
  state: PlayerState;
  flags: number;
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
      /** Knockback: velocity change; Respawn: position. */
      vector: Vec3;
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
      writeVec3(w, m.local.position);
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
        writeVec3(w, r.position);
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
        case PlayerEventKind.Respawn:
          writeVec3(w, m.vector);
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
      };
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
      const position = readVec3(r);
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
          position: readVec3(r),
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
        amount: 0,
        cause: DamageCause.Fall as DamageCause,
      };
      switch (event.kind) {
        case PlayerEventKind.Knockback:
        case PlayerEventKind.Respawn:
          event.vector = readVec3(r);
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
