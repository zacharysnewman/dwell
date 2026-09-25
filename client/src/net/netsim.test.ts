import { describe, expect, it } from 'vitest';
import { Channel, TransportKind } from '../protocol/constants.gen';
import { parseNetConditions, SimulatedTransport } from './netsim';
import type { Transport, TransportHandlers } from './Transport';

class FakeTransport implements Transport {
  readonly kind = TransportKind.Loopback;
  readonly binding = new Uint8Array(32);
  handlers: TransportHandlers | null = null;
  sent: string[] = [];
  setHandlers(h: TransportHandlers): void {
    this.handlers = h;
  }
  sendReliable(_c: Channel, b: Uint8Array): void {
    this.sent.push(`r${String(b[0])}`);
  }
  sendDatagram(b: Uint8Array): void {
    this.sent.push(`d${String(b[0])}`);
  }
  close(): void {}
}

describe('network condition simulator', () => {
  it('parses rtt,jitter,loss%', () => {
    expect(parseNetConditions('150,20,5')).toEqual({ rttMs: 150, jitterMs: 20, loss: 0.05 });
    expect(parseNetConditions('80')).toEqual({ rttMs: 80, jitterMs: 0, loss: 0 });
    expect(parseNetConditions('x')).toBeNull();
    expect(parseNetConditions(null)).toBeNull();
  });

  it('delays both directions by half the RTT, drops datagrams, keeps reliable order', () => {
    let now = 0;
    const timers: { at: number; fn: () => void }[] = [];
    const randoms = [0.5, 0.01, 0.5, 0.99, 0.5]; // 0.01 < loss: that datagram is dropped
    const inner = new FakeTransport();
    const sim = new SimulatedTransport(
      inner,
      { rttMs: 100, jitterMs: 10, loss: 0.05 },
      () => randoms.shift() ?? 0.5,
      () => now,
      (fn, ms) => timers.push({ at: now + ms, fn }),
    );
    const received: string[] = [];
    sim.setHandlers({
      onReliable: (_c, b) => received.push(`r${String(b[0])}`),
      onDatagram: (b) => received.push(`d${String(b[0])}`),
      onClose: () => {},
    });
    sim.sendDatagram(Uint8Array.of(1)); // random 0.5: not lost; then 0.01 for jitter
    sim.sendDatagram(Uint8Array.of(2)); // random 0.5: not lost…
    inner.handlers?.onReliable(Channel.world, Uint8Array.of(3));
    expect(timers.length).toBe(3);
    expect(inner.sent).toEqual([]);
    now = 1000;
    for (const t of timers.sort((a, b) => a.at - b.at)) t.fn();
    expect(inner.sent).toEqual(['d1', 'd2']);
    expect(received).toEqual(['r3']);
    expect(timers.every((t) => t.at >= 40 && t.at <= 60)).toBe(true);
  });
});
