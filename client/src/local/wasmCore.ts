// Typed wrapper around the local-mode WASM build of the server core (server/wasm/wasm_api.cpp).
import type { Channel, HostState, TransportKind } from '../protocol/constants.gen';
import { withHeapBytes, type DwellCoreFactory, type DwellCoreModule } from '../sim/module';
import type { DwellFiles } from './worldFiles';

export type { DwellCoreFactory } from '../sim/module';

export const OutgoingKind = { Reliable: 0, Datagram: 1, Close: 2 } as const;

/** Who may edit or fly in a hosted world (C++ core::EditPolicy). */
export const HostPolicy = { Everyone: 0, Host: 1, Nobody: 2 } as const;
export type HostPolicy = (typeof HostPolicy)[keyof typeof HostPolicy];
export type OutgoingKind = (typeof OutgoingKind)[keyof typeof OutgoingKind];

export interface Outgoing {
  session: number;
  kind: OutgoingKind;
  channel: Channel;
  bytes: Uint8Array;
}

/** The authoritative server core running in this JS context (a worker in the browser). */
/** `dwell_local_create`'s result for a world file this build may not open (RELEASES.md §6). */
const VERSION_LOCKED = 3;

export class LocalCore {
  private constructor(
    private readonly m: DwellCoreModule,
    /** The world is saved to `files` (§6.4). */
    readonly persisted: boolean,
    /** Why it is not, when files were given. */
    readonly storageError: string,
  ) {}

  /**
   * Starts the local server. With `files` (the world file's handles) the world is loaded from and
   * saved to it, and a saved world's seed and generator win over the arguments.
   */
  /** Bytes of the core's WebAssembly memory (it grows, never shrinks). */
  heapBytes(): number {
    return this.m.HEAPU8.buffer.byteLength;
  }

  static async load(
    factory: DwellCoreFactory,
    worldSeed = 0,
    generatorVersion = 3,
    files: DwellFiles | null = null,
  ): Promise<LocalCore> {
    const m = await factory(files ? { dwellFiles: files } : {});
    const created = m._dwell_local_create(worldSeed, generatorVersion, files ? 1 : 0);
    let error = '';
    const ptr = m._dwell_local_storage_error();
    for (let i = ptr; m.HEAPU8[i]; i++) error += String.fromCharCode(m.HEAPU8[i] ?? 0);
    // 3: the world file is locked to another version (RELEASES.md §6): no world was started.
    if (created === VERSION_LOCKED) throw new Error(error);
    return new LocalCore(m, created === 2, error);
  }

  /** Saves the world now (the page is closing). True if the save committed. */
  save(): boolean {
    return this.m._dwell_local_save() === 1;
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

  /**
   * Starts hosting (§10.2): up to `maxPlayers` players (the host included), edit and flight
   * policies (HostPolicy), and the host's device key as an op.
   */
  host(maxPlayers: number, edits: HostPolicy, flight: HostPolicy, hostKey: Uint8Array): void {
    this.withBytes(hostKey, (ptr) => {
      this.m._dwell_local_host(maxPlayers, edits, flight, ptr);
    });
  }

  /** Tells the guests the host paused or resumed. */
  hostStatus(state: HostState): void {
    this.m._dwell_local_host_status(state);
  }

  /** Stops hosting: guests are sent Reject(ServerClosing); the host keeps playing. */
  closeGuests(): void {
    this.m._dwell_local_close_guests();
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
