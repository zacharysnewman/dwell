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

/**
 * The water's figures (ADR 0062, docs/DEV_TOOLS.md): batches not yet answered, runs, the last run's
 * water time run, owed and dropped (water s) and its wall time (ms), the longest run (ms), the
 * water's clock (s), its awake and all chunks, its copy of the world (bytes, chunks), writes dropped
 * as overtaken by an edit.
 */
export interface WaterStats {
  queue: number;
  runs: number;
  ranS: number;
  owedS: number;
  droppedS: number;
  busyMs: number;
  maxBusyMs: number;
  waterTimeS: number;
  awakeChunks: number;
  waterChunks: number;
  cellBytes: number;
  cellChunks: number;
  overtaken: number;
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
  /** Loads the WASM module (before anything is installed in it). */
  static async ready(): Promise<void> {
    await loadWasm();
  }

  static async load(
    worldSeed = 0,
    generatorVersion = 3,
    file: FileSystemSyncAccessHandle | null = null,
    spawnFaceB = false,
    spawnPortal = false,
    spawnOnOpen = false,
    spawnAt?: [number, number, number],
    upgrade = false,
    waterWorker = false,
    jobsWorker = false,
  ): Promise<LocalCore> {
    const exports = (await loadWasm()) as { memory?: WebAssembly.Memory };
    const core = new LocalCore(
      new LocalWorld(
        worldSeed,
        generatorVersion,
        file ?? undefined,
        spawnFaceB,
        spawnPortal,
        spawnOnOpen,
        spawnAt ? Float64Array.from(spawnAt) : undefined,
        upgrade,
        waterWorker,
        jobsWorker,
      ),
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
  kill(): void {
    this.world.kill();
  }
  /**
   * The water near a cell (JSON, `Water::report`): a diagnostic for the block dump; undefined
   * while the water worker's is on its way (`takeWaterReport`).
   */
  waterReport(at: [number, number, number], radius: number): string | undefined {
    return this.world.waterReport(at[0], at[1], at[2], radius);
  }
  /** Moves the local player's feet to `at` (the dev tools' `goto`); false otherwise. */
  teleport(at: [number, number, number]): boolean {
    return this.world.teleport(at[0], at[1], at[2]);
  }
  /** The world map's nearest water (JSON or `null`; local.rs `find_on_map`). */
  findOnMap(
    kind: string,
    at: [number, number, number],
    radius: number,
    face: number,
    minDepth: number,
  ): string {
    return this.world.findOnMap(kind, at[0], at[1], at[2], radius, face, minDepth);
  }
  /** A report the water worker sent back. */
  takeWaterReport(): string | undefined {
    return this.world.takeWaterReport();
  }
  /** The world's generator and seed (a saved world's own), for the server-jobs worker. */
  get generatorVersion(): number {
    return this.world.generatorVersion;
  }
  get worldSeed(): bigint {
    return this.world.worldSeed;
  }
  /** The next job for the server-jobs worker (a chunk to generate, a LOD section). */
  takeJob(): Uint8Array | undefined {
    return this.world.takeJob();
  }
  /** The server-jobs worker's answer to a job. */
  jobIn(bytes: Uint8Array): boolean {
    return this.world.jobIn(bytes);
  }
  /** The next batch for the water worker (ADR 0062). */
  takeWaterOut(): Uint8Array | undefined {
    return this.world.takeWaterOut();
  }
  /** The water worker's answer. */
  waterIn(bytes: Uint8Array): boolean {
    return this.world.waterIn(bytes);
  }
  /** Asks the water for its record (a save is coming). */
  requestWaterRecord(): void {
    this.world.requestWaterRecord();
  }
  /** The record asked for has come (or none was asked). */
  waterRecordReady(): boolean {
    return this.world.waterRecordReady();
  }
  /** The water's figures (`LocalWorld::waterStats`, docs/DEV_TOOLS.md). */
  waterStats(): WaterStats {
    const v = Array.from(this.world.waterStats());
    const at = (i: number) => v[i] ?? 0;
    return {
      queue: at(0),
      runs: at(1),
      ranS: at(2),
      owedS: at(3),
      droppedS: at(4),
      busyMs: at(5),
      maxBusyMs: at(6),
      waterTimeS: at(7),
      awakeChunks: at(8),
      waterChunks: at(9),
      cellBytes: at(10),
      cellChunks: at(11),
      overtaken: at(12),
    };
  }
  /**
   * Each tick phase's time (ms) summed since the last call, by name (`Server::tick_phases`,
   * docs/DEV_TOOLS.md): where the server's ticks went.
   */
  tickPhases(): Record<string, number> {
    const names = LocalWorld.tickPhaseNames();
    const ms = Array.from(this.world.tickPhases());
    return Object.fromEntries(names.map((n, i) => [n, Math.round((ms[i] ?? 0) * 10) / 10]));
  }
  /** What the ticks did since the last call, by name (`Server::tick_counts`). */
  tickCounts(): Record<string, number> {
    const flat = this.world.tickCounts() as (string | number)[];
    const out: Record<string, number> = {};
    for (let i = 0; i + 1 < flat.length; i += 2) out[flat[i] as string] = flat[i + 1] as number;
    return out;
  }
  setRespawnPreload(on: boolean): void {
    this.world.setRespawnPreload(on);
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
