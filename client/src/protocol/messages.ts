// Protocol v0 messages used in Phase 1 (ARCHITECTURE.md §8.3). Mirrors
// server/core/include/dwell/protocol/messages.h; layouts are pinned by shared golden vectors.
import { ByteReader, ByteWriter, DecodeError } from './bytes';
import { AUTH_DOMAIN_TAG, Limits, MessageType, RejectReason } from './constants.gen';

export const STATUS_FLAG_ONLINE_MODE = 1 << 0;

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
  }
  return w.finish();
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
