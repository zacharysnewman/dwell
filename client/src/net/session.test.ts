import { describe, expect, it } from 'vitest';
import { loadOrCreateDeviceKey, MemoryKeyStore } from '../identity/deviceKey';
import { Channel, MessageType, RejectReason, TransportKind } from '../protocol/constants.gen';
import { authTranscript, decode, encode, type Message } from '../protocol/messages';
import { ClientSession, type SessionState } from './session';
import type { Transport, TransportHandlers } from './Transport';

/** In-memory transport whose "server" side is driven by the test. */
class FakeTransport implements Transport {
  readonly kind = TransportKind.Loopback;
  readonly binding = new Uint8Array(32).fill(7);
  handlers: TransportHandlers | null = null;
  sent: Message[] = [];
  setHandlers(h: TransportHandlers): void {
    this.handlers = h;
  }
  sendReliable(_channel: Channel, bytes: Uint8Array): void {
    this.sent.push(decode(bytes));
  }
  sendDatagram(bytes: Uint8Array): void {
    this.sent.push(decode(bytes));
  }
  close(): void {
    this.handlers?.onClose({ message: 'closed' });
  }
  deliver(m: Message): void {
    this.handlers?.onReliable(Channel.control, encode(m));
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

async function setup() {
  const key = await loadOrCreateDeviceKey(new MemoryKeyStore());
  const transport = new FakeTransport();
  const session = new ClientSession(transport, key, {
    displayName: 'Tester',
    clientVersion: 'test',
    pingIntervalMs: 60_000,
    now: () => 1000,
  });
  const states: SessionState[] = [];
  session.subscribe((s) => states.push(s));
  session.start();
  return { key, transport, session, states };
}

describe('ClientSession', () => {
  it('signs the challenge bound to the transport and joins', async () => {
    const { key, transport, session, states } = await setup();
    const hello = transport.sent.find((m) => m.type === MessageType.ClientHello);
    expect(hello).toBeDefined();

    const nonce = new Uint8Array(32).fill(9);
    transport.deliver({ type: MessageType.Challenge, nonce });
    await flush();
    const auth = transport.sent.find((m) => m.type === MessageType.ClientAuth);
    if (auth?.type !== MessageType.ClientAuth) throw new Error('no ClientAuth');
    const pub = await crypto.subtle.importKey('raw', key.publicKey, 'Ed25519', false, ['verify']);
    const transcript = authTranscript(nonce, transport.binding, key.publicKey);
    expect(await crypto.subtle.verify('Ed25519', pub, auth.signature.slice(), transcript)).toBe(
      true,
    );

    transport.deliver({
      type: MessageType.Welcome,
      playerId: 3,
      worldSeed: 5n,
      generatorVersion: 0,
      serverTick: 10,
    });
    await flush();
    expect(states.at(-1)).toEqual({ phase: 'joined', playerId: 3, worldSeed: 5n });
    session.close();
  });

  it('reports rejections', async () => {
    const { transport, session, states } = await setup();
    transport.deliver({ type: MessageType.Reject, reason: RejectReason.Full, message: 'Full.' });
    await flush();
    expect(states.at(-1)).toEqual({
      phase: 'rejected',
      reason: RejectReason.Full,
      message: 'Full.',
    });
    session.close();
  });

  it('measures RTT from pongs', async () => {
    const { transport, session } = await setup();
    let rtt: number | null = null;
    session.subscribe((_s, stats) => (rtt = stats.rttMs));
    transport.deliver({
      type: MessageType.Pong,
      seq: 1,
      clientTimeMs: 960,
      serverTick: 5,
      serverTimeMs: 0,
    });
    await flush();
    expect(rtt).toBe(40);
    session.close();
  });
});
