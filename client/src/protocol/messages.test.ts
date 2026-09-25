// Codec tests against shared/protocol/vectors.txt. Field values mirror make_vectors.py and
// server/tests/protocol_test.cpp.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { MessageType, RejectReason } from './constants.gen';
import { authTranscript, decode, encode, STATUS_FLAG_ONLINE_MODE, type Message } from './messages';

const vectors = new Map<string, Uint8Array>(
  readFileSync(new URL('../../../shared/protocol/vectors.txt', import.meta.url), 'utf8')
    .split('\n')
    .filter((l) => l && !l.startsWith('#'))
    .map((l) => {
      const [name = '', hex = ''] = l.split(' ');
      return [name, Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16))];
    }),
);

function vector(name: string): Uint8Array {
  const v = vectors.get(name);
  if (!v) throw new Error(`missing vector ${name}`);
  return v;
}

const seq = (n: number, start: number, step = 1) =>
  Uint8Array.from({ length: n }, (_, i) => (start + i * step) & 0xff);

const expected: Record<string, Message> = {
  datagram_ping: { type: MessageType.DatagramPing, seq: 0x01020304, clientTimeMs: 1234.5 },
  datagram_pong: { type: MessageType.DatagramPong, seq: 7, clientTimeMs: 0.25, serverTick: 600 },
  status_request: { type: MessageType.StatusRequest },
  status_response: {
    type: MessageType.StatusResponse,
    protocolVersion: 1,
    serverName: 'Dwell Test',
    motd: 'héllo ✓',
    players: 3,
    maxPlayers: 8,
    flags: STATUS_FLAG_ONLINE_MODE,
  },
  client_hello: {
    type: MessageType.ClientHello,
    protocolVersion: 1,
    clientVersion: '0.1.0',
    publicKey: seq(32, 0),
    displayName: 'Zack',
  },
  challenge: { type: MessageType.Challenge, nonce: seq(32, 0xa0) },
  client_auth: { type: MessageType.ClientAuth, signature: seq(64, 0, 3) },
  welcome: {
    type: MessageType.Welcome,
    playerId: 42,
    worldSeed: 0x0123456789abcdefn,
    generatorVersion: 7,
    serverTick: 123456,
  },
  reject: {
    type: MessageType.Reject,
    reason: RejectReason.ProtocolVersion,
    message: 'Server runs protocol 2',
  },
  ping: { type: MessageType.Ping, seq: 9, clientTimeMs: 1000 },
  pong: {
    type: MessageType.Pong,
    seq: 9,
    clientTimeMs: 1000,
    serverTick: 60,
    serverTimeMs: 5000.125,
  },
};

describe('protocol golden vectors', () => {
  for (const [name, message] of Object.entries(expected)) {
    it(`encodes ${name}`, () => {
      expect(encode(message)).toEqual(vector(name));
    });
    it(`decodes ${name}`, () => {
      expect(decode(vector(name))).toEqual(message);
    });
  }

  const malformed = [...vectors.keys()].filter((k) => k.startsWith('!'));
  it('has malformed vectors', () => {
    expect(malformed.length).toBeGreaterThanOrEqual(7);
  });
  for (const name of malformed) {
    it(`rejects ${name}`, () => {
      expect(() => decode(vector(name))).toThrow();
    });
  }

  it('builds the auth transcript', () => {
    expect(authTranscript(seq(32, 0xa0), seq(32, 0x10), seq(32, 0))).toEqual(
      vector('auth_transcript'),
    );
  });
});
