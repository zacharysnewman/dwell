// Messages between the main thread and the local-mode worker (LoopbackTransport ↔ worker.ts).
import type { Channel, HostState, TransportKind } from '../protocol/constants.gen';
import type { HostPolicy, WaterStats } from './wasmCore';

export type ToWorker =
  | {
      t: 'start';
      worldSeed: number;
      generatorVersion: number;
      file?: string;
      spawnFaceB?: boolean;
      spawnPortal?: boolean;
      /** A spawn link: a saved world's player starts at the spawn too (local/world.ts). */
      spawnOnOpen?: boolean;
      /** Dev link `?at=x,y,z`: the spawn is that point, and the player starts there. */
      spawnAt?: [number, number, number];
      /** The player confirmed opening an older version line's world (local/world.ts). */
      upgrade?: boolean;
      /** A new world's game mode by name (ADR 0065); a saved world keeps its own. */
      mode?: string;
      /** The world's maps (faces A and B, ADR 0049), installed before the world starts. */
      maps?: Uint8Array[];
      /**
       * Dev link `?water=inline`: the water runs on the server's thread, not in its own worker
       * (ADR 0062; the bench's before/after).
       */
      waterInline?: boolean;
      /**
       * Dev link `?serverjobs=inline`: the server's chunks to generate and LOD sections on its
       * tick, not their own worker.
       */
      jobsInline?: boolean;
      /** `?chop=v2`: the chopping prototype (docs/WORK_QUEUE.md item 7). */
      chop2?: boolean;
      /** Dev link `?growth=N`: trees grow N times as fast (ADR 0079; a time-lapse). */
      growth?: number;
      /**
       * The maps follow in a `maps` message (the page still shaping them): the worker opens the
       * world file and loads its WASM meanwhile, then waits for them (§6.3).
       */
      mapsLater?: boolean;
    }
  /** The world's maps after a `start` with `mapsLater` (empty: none, the server computes its own). */
  | { t: 'maps'; maps: Uint8Array[] }
  | { t: 'connect'; session: number; kind: TransportKind; binding: Uint8Array }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'disconnect'; session: number }
  /** Save the world now (the page is being hidden or closed); answered with `saved`. */
  | { t: 'save' }
  /** The host switches the world's game mode (ADR 0065, by name); answered with `mode`. */
  | { t: 'setMode'; mode: string }
  /** Friend-world hosting (§10.2): start hosting with these limits and policies. */
  | {
      t: 'host';
      maxPlayers: number;
      edits: HostPolicy;
      flight: HostPolicy;
      hostKey: Uint8Array;
    }
  /** The host's page was hidden or shown: pause or resume the world and tell the guests. */
  | { t: 'hostStatus'; state: HostState }
  /** Stop hosting: end the guests' sessions (Reject(ServerClosing)). */
  | { t: 'closeGuests' }
  /** Kill the local player (the bench's `die` step). */
  | { t: 'kill' }
  /** Give the local player items (the bench's `give` step). */
  | { t: 'give'; item: string; count: number }
  /** Carry the local player on a moving raft (the bench's `carry` step). */
  | { t: 'carry'; speed: number; yaw: number; seconds: number; lift: number }
  /** The respawn preload (ARCHITECTURE.md §6.3) on or off (`?respawnPreload=off`). */
  | { t: 'respawnPreload'; on: boolean }
  /** The water near a cell (a diagnostic for the block dump); answered with `waterReport`. */
  | { t: 'waterReport'; at: [number, number, number]; radius: number }
  /** Moves the local player's feet there (the dev tools' `goto`, docs/DEV_TOOLS.md). */
  | { t: 'teleport'; at: [number, number, number] }
  /**
   * The world map's nearest water of `kind` (`water`, `sea`, `river`, `lake`) from `at` within
   * `radius` m on `face` (1 A, −1 B, 0 the point's), `minDepth` m deep at least; answered with
   * `findOnMap`.
   */
  | {
      t: 'findOnMap';
      kind: string;
      at: [number, number, number];
      radius: number;
      face: number;
      minDepth: number;
    }
  /** The lava's dev tools (local.rs `lava_dev`); answered with `lavaDev`. */
  | { t: 'lavaDev'; op: string; args: number[] };

export type FromWorker =
  /** `persisted`: the world is saved in the browser (OPFS, §6.4). */
  /** `mode`: the world's game mode (ADR 0065), as its file has it. */
  | { t: 'ready'; persisted: boolean; mode?: string }
  /** The world's game mode after a `setMode` (unchanged if the name was refused). */
  | { t: 'mode'; mode: string }
  | { t: 'error'; message: string }
  /**
   * A loading stage of the world's start (shared/look/loading.json `stages`, by id) began or ended;
   * `atMs` on the shared wall clock (`performance.timeOrigin + performance.now()`).
   */
  | { t: 'stage'; id: string; phase: 'begin' | 'end'; atMs: number }
  /** The world was saved (in reply to `save`; `ok` is false if it could not be). */
  | { t: 'saved'; ok: boolean }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'close'; session: number }
  /** The server core's WebAssembly memory (bytes), every second. */
  | { t: 'memory'; bytes: number }
  /** The server's last second: ticks run and the time they took (ms), for the status line. */
  | { t: 'perf'; ticks: number; busyMs: number }
  /** The water near a cell as JSON (in reply to `waterReport`). */
  | { t: 'waterReport'; report: string }
  /** The map's nearest water as JSON, or `null` (in reply to `findOnMap`). */
  | { t: 'findOnMap'; result: string }
  /** The lava's dev tools' answer as JSON (in reply to `lavaDev`). */
  | { t: 'lavaDev'; result: string }
  /** The server's figures, every second (docs/DEV_TOOLS.md `__dwell.serverStats`). */
  | { t: 'stats'; stats: ServerStats };

/**
 * The local server's figures over the last second: its ticks, their mean wall time, the worst
 * `advance` call (one or more ticks), where the water runs, and the water's own figures.
 */
export interface ServerStats {
  ticks: number;
  tickMeanMs: number;
  advanceMaxMs: number;
  water: 'worker' | 'inline';
  waterStats: WaterStats;
  /** Each tick phase's time over the second (ms, summed; `Server::tick_phases`' names). */
  phasesMs: Record<string, number>;
  /**
   * What the ticks did over the second (`Server::tick_counts`): `collisionBuilds`, the terrain
   * collision's chunk builds; `generatedOnTick`, chunks the tick generated itself, not the jobs;
   * `collisionCells`, full-cube cells put into or taken out of the terrain collider.
   */
  counts: Record<string, number>;
}
