// Codec tests against shared/protocol/vectors.txt. Field values mirror make_vectors.py and
// server/tests/protocol_test.cpp.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { floatToHalf, halfToFloat } from './bytes';
import {
  ControllerFlags,
  DamageCause,
  GroundKind,
  InputButtons,
  MessageType,
  PlayerEventKind,
  PlayerFlags,
  PlayerState,
  RejectReason,
} from './constants.gen';
import {
  authTranscript,
  decode,
  encode,
  STATUS_FLAG_ONLINE_MODE,
  type ControllerState,
  type LocalPlayerState,
  type Message,
} from './messages';

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

const controller = (flags: number): ControllerState => ({
  flags,
  currentX: 1.5,
  currentZ: -2.25,
  externalX: 0.5,
  externalZ: 0,
  contributionX: 2,
  contributionZ: -2.25,
  accumulatedY: 3,
  platformY: 0,
  targetY: -1,
  groundVelocityY: 0.25,
  groundKind: GroundKind.Player,
  groundId: 7,
  bufferTicks: 11,
  coyoteTicks: 5,
  stepGrace: 2,
  ladder: [0, 0, 0],
  released: [0, 0],
});

const local = (
  flags: number,
  health: number,
  state: PlayerState,
  c: ControllerState,
): LocalPlayerState => ({
  position: [10.5, Math.fround(0.9), -3.25],
  velocity: [5, -0.5, 0],
  flags,
  health,
  state,
  inputBuffer: 0,
  lastKnockbackSeq: 0,
  controller: c,
});

const event = (kind: PlayerEventKind) => ({
  type: MessageType.PlayerEvent,
  kind,
  playerId: 5,
  serverTick: 1200,
  inputSeq: 77,
  vector: [0, 0, 0] as [number, number, number],
  amount: 0,
  cause: DamageCause.Fall as DamageCause,
});

const expected: Record<string, Message> = {
  player_input: {
    type: MessageType.PlayerInput,
    lastSnapshotTick: 300,
    inputs: [
      {
        seq: 41,
        moveX: 127,
        moveY: -127,
        buttons: InputButtons.jump | InputButtons.run,
        yaw: -16384,
        pitch: 32767,
      },
      { seq: 42, moveX: 0, moveY: 90, buttons: InputButtons.crouch, yaw: 12345, pitch: -100 },
    ],
  },
  physics_snapshot: {
    type: MessageType.PhysicsSnapshot,
    serverTick: 603,
    ackInputSeq: 42,
    local: {
      ...local(PlayerFlags.grounded | PlayerFlags.climbing, 87, PlayerState.Climbing, {
        ...controller(
          ControllerFlags.grounded | ControllerFlags.climbing | ControllerFlags.hasReleased,
        ),
        ladder: [-5, 64, 1000000],
        released: [-5, 7],
      }),
      inputBuffer: 3,
      lastKnockbackSeq: 40,
    },
    remotes: [
      {
        playerId: 3,
        position: [1, 2, 3],
        velocity: [0.5, -8, 2 ** -14],
        yaw: 16384,
        pitch: -8192,
        state: PlayerState.Running,
        flags: PlayerFlags.grounded,
      },
      {
        playerId: 9,
        position: [-1, 0, 65504],
        velocity: [65504, -0, 1],
        yaw: 0,
        pitch: 0,
        state: PlayerState.Swimming,
        flags: PlayerFlags.swimming | PlayerFlags.dead,
      },
    ],
  },
  physics_snapshot_min: {
    type: MessageType.PhysicsSnapshot,
    serverTick: 3,
    ackInputSeq: 0,
    local: local(0, 100, PlayerState.Idle, controller(0)),
    remotes: [],
  },
  player_event_knockback: { ...event(PlayerEventKind.Knockback), vector: [0, 14, -0.5] },
  player_event_damage: { ...event(PlayerEventKind.Damage), amount: 17, cause: DamageCause.Fall },
  player_event_death: { ...event(PlayerEventKind.Death), cause: DamageCause.Crush },
  player_event_respawn: { ...event(PlayerEventKind.Respawn), vector: [0.5, 0, 0.5] },
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
    expect(malformed.length).toBeGreaterThanOrEqual(15);
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

describe('half floats', () => {
  it('round to nearest even and saturate', () => {
    expect(floatToHalf(1)).toBe(0x3c00);
    expect(floatToHalf(-2)).toBe(0xc000);
    expect(floatToHalf(65504)).toBe(0x7bff);
    expect(floatToHalf(1e6)).toBe(0x7c00);
    expect(floatToHalf(5.9604645e-8)).toBe(0x0001);
    expect(floatToHalf(1 + 1 / 2048)).toBe(0x3c00);
    expect(floatToHalf(1 + 3 / 2048)).toBe(0x3c02);
    expect(floatToHalf(2 ** -25)).toBe(0); // tie → even
    expect(floatToHalf(3 * 2 ** -25)).toBe(2); // tie → even (up)
  });
  it('round-trip every finite half', () => {
    let mismatches = 0;
    for (let h = 0; h < 0x7c00; h++) if (floatToHalf(halfToFloat(h)) !== h) mismatches++;
    expect(mismatches).toBe(0);
  });
});
