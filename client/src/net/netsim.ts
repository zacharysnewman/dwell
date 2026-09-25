// Network condition simulator (Phase 2 tooling): wraps a Transport and adds latency, jitter, and
// datagram loss in both directions. Enabled with `?netsim=<rtt ms>,<jitter ms>,<loss %>`.
import type { Channel } from '../protocol/constants.gen';
import type { Transport, TransportHandlers } from './Transport';

export interface NetConditions {
  /** Round trip added; each direction gets half. */
  rttMs: number;
  /** Uniform ± jitter per packet, ms. */
  jitterMs: number;
  /** Datagram loss probability, 0..1 (reliable messages are only delayed, in order). */
  loss: number;
}

/** Parses `rtt,jitter,loss%` (e.g. `150,20,5`); null if absent or malformed. */
export function parseNetConditions(value: string | null): NetConditions | null {
  if (!value) return null;
  const [rtt, jitter = '0', loss = '0'] = value.split(',');
  const n = [Number(rtt), Number(jitter), Number(loss)];
  if (n.some((v) => !Number.isFinite(v) || v < 0)) return null;
  return { rttMs: n[0] ?? 0, jitterMs: n[1] ?? 0, loss: Math.min(1, (n[2] ?? 0) / 100) };
}

type Schedule = (fn: () => void, delayMs: number) => void;

class Direction {
  private lastReliableAt = 0;
  constructor(
    private readonly c: NetConditions,
    private readonly random: () => number,
    private readonly now: () => number,
    private readonly schedule: Schedule,
  ) {}

  send(reliable: boolean, deliver: () => void): void {
    if (!reliable && this.random() < this.c.loss) return;
    const delay = Math.max(0, this.c.rttMs / 2 + (this.random() * 2 - 1) * this.c.jitterMs);
    let at = this.now() + delay;
    if (reliable) {
      at = Math.max(at, this.lastReliableAt); // stay in order
      this.lastReliableAt = at;
    }
    this.schedule(deliver, at - this.now());
  }
}

export class SimulatedTransport implements Transport {
  private handlers: TransportHandlers | null = null;
  private readonly up: Direction;
  private readonly down: Direction;

  constructor(
    private readonly inner: Transport,
    readonly conditions: NetConditions,
    random: () => number = Math.random,
    now: () => number = () => performance.now(),
    schedule: Schedule = (fn, ms) => setTimeout(fn, ms),
  ) {
    this.up = new Direction(conditions, random, now, schedule);
    this.down = new Direction(conditions, random, now, schedule);
    inner.setHandlers({
      onReliable: (channel, bytes) => {
        this.down.send(true, () => this.handlers?.onReliable(channel, bytes));
      },
      onDatagram: (bytes) => {
        this.down.send(false, () => this.handlers?.onDatagram(bytes));
      },
      onClose: (info) => {
        this.handlers?.onClose(info);
      },
    });
  }

  get kind() {
    return this.inner.kind;
  }

  get binding() {
    return this.inner.binding;
  }

  setHandlers(handlers: TransportHandlers): void {
    this.handlers = handlers;
  }

  sendReliable(channel: Channel, bytes: Uint8Array): void {
    this.up.send(true, () => {
      this.inner.sendReliable(channel, bytes);
    });
  }

  sendDatagram(bytes: Uint8Array): void {
    this.up.send(false, () => {
      this.inner.sendDatagram(bytes);
    });
  }

  close(): void {
    this.inner.close();
  }
}
