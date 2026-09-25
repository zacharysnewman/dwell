// Typed wrapper around the local-mode WASM build of the server core (server/wasm/wasm_api.cpp).
import type { Channel, TransportKind } from '../protocol/constants.gen';
import { withHeapBytes, type DwellCoreFactory, type DwellCoreModule } from '../sim/module';

export type { DwellCoreFactory } from '../sim/module';

export const OutgoingKind = { Reliable: 0, Datagram: 1, Close: 2 } as const;
export type OutgoingKind = (typeof OutgoingKind)[keyof typeof OutgoingKind];

export interface Outgoing {
  session: number;
  kind: OutgoingKind;
  channel: Channel;
  bytes: Uint8Array;
}

/** The authoritative server core running in this JS context (a worker in the browser). */
export class LocalCore {
  private constructor(private readonly m: DwellCoreModule) {}

  static async load(factory: DwellCoreFactory, worldSeed = 0): Promise<LocalCore> {
    const m = await factory();
    m._dwell_local_create(worldSeed);
    return new LocalCore(m);
  }

  private withBytes(bytes: Uint8Array, fn: (ptr: number) => void): void {
    withHeapBytes(this.m, bytes, fn);
  }

  connected(session: number, kind: TransportKind, binding: Uint8Array): void {
    this.withBytes(binding, (ptr) => {
      this.m._dwell_local_connected(session, kind, ptr);
    });
  }

  disconnected(session: number): void {
    this.m._dwell_local_disconnected(session);
  }

  reliable(session: number, channel: Channel, bytes: Uint8Array): void {
    this.withBytes(bytes, (ptr) => {
      this.m._dwell_local_reliable(session, channel, ptr, bytes.length);
    });
  }

  datagram(session: number, bytes: Uint8Array): void {
    this.withBytes(bytes, (ptr) => {
      this.m._dwell_local_datagram(session, ptr, bytes.length);
    });
  }

  /** Runs the simulation steps due for `elapsedSeconds` of real time; returns the tick. */
  advance(elapsedSeconds: number): number {
    return this.m._dwell_local_advance(elapsedSeconds);
  }

  takeOutbox(): Outgoing[] {
    const lenPtr = this.m._malloc(4);
    try {
      const ptr = this.m._dwell_local_take_outbox(lenPtr);
      const len = this.m.HEAPU32[lenPtr >> 2] ?? 0;
      // Copy out before anything else can grow (and detach) the heap.
      const buf = this.m.HEAPU8.slice(ptr, ptr + len);
      const view = new DataView(buf.buffer);
      const count = view.getUint32(0, true);
      const out: Outgoing[] = [];
      let pos = 4;
      for (let i = 0; i < count; i++) {
        const session = view.getUint32(pos, true);
        const kind = view.getUint8(pos + 4) as OutgoingKind;
        const channel = view.getUint8(pos + 5) as Channel;
        const n = view.getUint32(pos + 6, true);
        out.push({ session, kind, channel, bytes: buf.slice(pos + 10, pos + 10 + n) });
        pos += 10 + n;
      }
      return out;
    } finally {
      this.m._free(lenPtr);
    }
  }
}
