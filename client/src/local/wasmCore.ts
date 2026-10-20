// The local-mode server core (ARCHITECTURE.md §2.1): the Rust `LocalWorld` (dwell-client's
// local.rs, the dwell-sim server with its world file) in the local-mode worker.
import type { Channel, HostState, TransportKind } from '../protocol/constants.gen';
import { loadWasm, LocalWorld } from '../engine/wasm';

export const OutgoingKind = { Reliable: 0, Datagram: 1, Close: 2 } as const;

/** Who may edit or fly in a hosted world (dwell-sim EditPolicy). */
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
export class LocalCore {
  private constructor(private readonly world: LocalWorld) {}

  /** The world is saved to its file (§6.4). */
  get persisted(): boolean {
    return this.world.persisted;
  }
  /** Why it is not, when a file was given. */
  get storageError(): string {
    return this.world.storageError;
  }

  /** Bytes of the module's WebAssembly memory (it grows, never shrinks). */
  heapBytes(): number {
    return this.memory?.buffer.byteLength ?? 0;
  }
  private memory: WebAssembly.Memory | null = null;

  /**
   * Starts the local server. With `file` (the world file's OPFS sync access handle) the world is
   * loaded from and saved to it, and a saved world's seed and generator win over the arguments.
   * Throws if the file is locked to another version (RELEASES.md §6).
   */
  static async load(
    worldSeed = 0,
    generatorVersion = 3,
    file: FileSystemSyncAccessHandle | null = null,
    spawnFaceB = false,
  ): Promise<LocalCore> {
    const exports = (await loadWasm()) as { memory?: WebAssembly.Memory };
    const core = new LocalCore(
      new LocalWorld(worldSeed, generatorVersion, file ?? undefined, spawnFaceB),
    );
    core.memory = exports.memory ?? null;
    return core;
  }

  /** Saves the world now (the page is closing). True if the save committed. */
  save(): boolean {
    return this.world.save();
  }
  connected(session: number, kind: TransportKind, binding: Uint8Array): void {
    this.world.connected(session, kind, binding);
  }
  disconnected(session: number): void {
    this.world.disconnected(session);
  }
  reliable(session: number, channel: Channel, bytes: Uint8Array): void {
    this.world.reliable(session, channel, bytes);
  }
  datagram(session: number, bytes: Uint8Array): void {
    this.world.datagram(session, bytes);
  }
  host(maxPlayers: number, edits: HostPolicy, flight: HostPolicy, hostKey: Uint8Array): void {
    this.world.host(maxPlayers, edits, flight, hostKey);
  }
  hostStatus(state: HostState): void {
    this.world.hostStatus(state);
  }
  closeGuests(): void {
    this.world.closeGuests();
  }
  /** Advances real time; steps the fixed 60 Hz simulation as far as it reaches. */
  advance(elapsedSeconds: number): number {
    return this.world.advance(elapsedSeconds);
  }

  /** What the server sends, oldest first (local.rs `encode_outbox`). */
  takeOutbox(): Outgoing[] {
    const buf = this.world.takeOutbox();
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
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
  }
}
