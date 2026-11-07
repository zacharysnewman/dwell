// Local-mode host (ARCHITECTURE.md §2.1): runs the Rust server core (LocalWorld) in a worker and exchanges
// protocol bytes with the main thread's LoopbackTransport.
import { HostState } from '../protocol/constants.gen';
import type { FromWorker, ToWorker } from './messages';
import { installWorldMap } from '../engine/wasm';
import { LocalCore, OutgoingKind } from './wasmCore';
import { localWorldName, openWorldFile } from './worldFiles';

interface WorkerScope {
  postMessage(message: FromWorker, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ToWorker>) => void) | null;
}
const scope = self as unknown as WorkerScope;

let core: LocalCore | null = null;
/** The water worker (ADR 0062): the server's water grid on its own thread; null: inline. */
let water: Worker | null = null;
/**
 * The server-jobs worker: the chunks to generate and the LOD's sections off the server's tick
 * (WATER_SURFACE_GRID.md §2d); null: on it.
 */
let jobs: Worker | null = null;
/** Water reports asked for and not yet come back from the water worker. */
let reportsDue = 0;
/** This second's server ticks: their count and wall time, and the worst `advance` call (ms). */
const tickStats = { ticks: 0, ms: 0, maxMs: 0 };
/** The host paused the world (their page is hidden, §10.2): no simulation steps. */
let paused = false;
const queued: ToWorker[] = [];

/** A loading stage (shared/look/loading.json `stages`) began or ended, for the page's screen (§6.3). */
function stage(id: string, phase: 'begin' | 'end'): void {
  scope.postMessage({ t: 'stage', id, phase, atMs: performance.timeOrigin + performance.now() });
}

/** Starts the water worker; null where workers can't start workers (the water runs inline). */
function startWater(maps: Uint8Array[]): Worker | null {
  if (typeof Worker === 'undefined') return null;
  try {
    const w = new Worker(new URL('./waterWorker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<ArrayBuffer>) => {
      if (!core) return;
      core.waterIn(new Uint8Array(e.data));
      postReports();
    };
    w.onerror = (e) => {
      console.warn('Dwell: the water worker failed; the water stands still', e.message);
    };
    w.postMessage({ t: 'start', maps: maps.map((m) => m.slice()) });
    return w;
  } catch (err) {
    console.warn('Dwell: no water worker; the water runs on the server thread', err);
    return null;
  }
}

/**
 * Starts the server-jobs worker (the chunks to generate, the LOD's sections: WATER_SURFACE_GRID.md
 * §2d); null where workers can't start workers (the jobs run on the tick).
 */
function startJobs(maps: Uint8Array[]): Worker | null {
  if (typeof Worker === 'undefined') return null;
  try {
    const w = new Worker(new URL('./serverJobsWorker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<ArrayBuffer>) => {
      core?.jobIn(new Uint8Array(e.data));
    };
    w.onerror = (e) => {
      console.warn('Dwell: the server-jobs worker failed; new terrain stops coming', e.message);
    };
    w.postMessage({ t: 'start', maps: maps.map((m) => m.slice()) });
    return w;
  } catch (err) {
    console.warn('Dwell: no server-jobs worker; its jobs run on the server thread', err);
    return null;
  }
}

/** The server's jobs for the server-jobs worker (transferred). */
function pumpJobs(): void {
  if (!core || !jobs) return;
  for (let b = core.takeJob(); b; b = core.takeJob()) {
    jobs.postMessage(b.buffer, [b.buffer]);
  }
}

/** The server's batches for the water worker (transferred). */
function pumpWater(): void {
  if (!core || !water) return;
  for (let b = core.takeWaterOut(); b; b = core.takeWaterOut()) {
    water.postMessage(b.buffer, [b.buffer]);
  }
}

function postReports(): void {
  if (!core) return;
  while (reportsDue > 0) {
    const report = core.takeWaterReport();
    if (report === undefined) break;
    reportsDue--;
    scope.postMessage({ t: 'waterReport', report });
  }
}

/** Saves, with a fresh water record from the water worker (a second at most, ADR 0062). */
async function save(): Promise<boolean> {
  if (!core) return false;
  if (water) {
    core.requestWaterRecord();
    pumpWater();
    const end = performance.now() + 1000;
    while (!core.waterRecordReady() && performance.now() < end) {
      await new Promise((r) => setTimeout(r, 5));
    }
  }
  return core.save();
}

function flush(): void {
  if (!core) return;
  pumpWater();
  pumpJobs();
  for (const out of core.takeOutbox()) {
    switch (out.kind) {
      case OutgoingKind.Reliable:
        scope.postMessage(
          { t: 'reliable', session: out.session, channel: out.channel, bytes: out.bytes },
          [out.bytes.buffer],
        );
        break;
      case OutgoingKind.Datagram:
        scope.postMessage({ t: 'datagram', session: out.session, bytes: out.bytes }, [
          out.bytes.buffer,
        ]);
        break;
      case OutgoingKind.Close:
        scope.postMessage({ t: 'close', session: out.session });
        break;
    }
  }
}

function handle(msg: ToWorker): void {
  if (!core) {
    queued.push(msg);
    return;
  }
  switch (msg.t) {
    case 'start':
    case 'maps':
      break;
    case 'connect':
      core.connected(msg.session, msg.kind, msg.binding);
      break;
    case 'host':
      core.host(msg.maxPlayers, msg.edits, msg.flight, msg.hostKey);
      break;
    case 'hostStatus':
      // A hidden host page pauses the world (ADR 0009); guests hear why.
      paused = msg.state === HostState.Paused;
      core.hostStatus(msg.state);
      break;
    case 'closeGuests':
      core.closeGuests();
      break;
    case 'kill':
      core.kill();
      break;
    case 'waterReport': {
      const report = core.waterReport(msg.at, msg.radius);
      if (report !== undefined) scope.postMessage({ t: 'waterReport', report });
      else reportsDue++;
      break;
    }
    case 'teleport':
      core.teleport(msg.at);
      break;
    case 'findOnMap':
      scope.postMessage({
        t: 'findOnMap',
        result: core.findOnMap(msg.kind, msg.at, msg.radius, msg.face, msg.minDepth),
      });
      break;
    case 'respawnPreload':
      core.setRespawnPreload(msg.on);
      break;
    case 'reliable':
      core.reliable(msg.session, msg.channel, msg.bytes);
      break;
    case 'datagram':
      core.datagram(msg.session, msg.bytes);
      break;
    case 'disconnect':
      core.disconnected(msg.session);
      break;
    case 'save':
      void save().then((ok) => {
        scope.postMessage({ t: 'saved', ok });
      });
      break;
  }
  flush();
}

async function start(
  worldSeed: number,
  generatorVersion: number,
  file?: string,
  spawnFaceB = false,
  spawnPortal = false,
  spawnOnOpen = false,
  given: Uint8Array[] | Promise<Uint8Array[]> = [],
  spawnAt?: [number, number, number],
  upgrade = false,
  waterInline = false,
  jobsInline = false,
): Promise<void> {
  // The world file (§6.4): the chosen world's (by default one per generator and seed), in OPFS.
  // Without it the world lives in memory only (no OPFS, or the world is open in another tab).
  stage('worldFile', 'begin');
  const files = await openWorldFile(file ?? localWorldName(generatorVersion, worldSeed));
  stage('worldFile', 'end');
  if (!files) console.warn('Dwell: this local world will not be saved (no OPFS access).');
  try {
    // The world's maps first (ADR 0049), so the server's generator need not compute them.
    stage('server', 'begin');
    await LocalCore.ready();
    stage('server', 'end');
    // The maps, if the page was still shaping them (`mapsLater`): its `worldMap` stage meanwhile.
    const maps = await given;
    // The world made: its maps installed, the spawn found (and its ground made), the file read.
    stage('spawn', 'begin');
    for (const map of maps) {
      try {
        installWorldMap(map);
      } catch (err) {
        console.warn('Dwell: a world map was refused', err);
      }
    }
    // The water on its own worker (ADR 0062), unless asked for inline or workers can't start one.
    water = waterInline ? null : startWater(maps);
    // And the server's jobs on theirs (`?serverjobs=inline`: on the tick, for comparison).
    jobs = jobsInline ? null : startJobs(maps);
    core = await LocalCore.load(
      worldSeed,
      generatorVersion,
      files?.handle ?? null,
      spawnFaceB,
      spawnPortal,
      spawnOnOpen,
      spawnAt,
      upgrade,
      water !== null,
      jobs !== null,
    );
    stage('spawn', 'end');
    jobs?.postMessage({
      t: 'init',
      generatorVersion: core.generatorVersion,
      worldSeed: core.worldSeed,
    });
  } catch (err) {
    scope.postMessage({
      t: 'error',
      message: `Local world unavailable (${err instanceof Error ? err.message : String(err)}).`,
    });
    return;
  }
  if (files && !core.persisted) {
    console.warn(`Dwell: the local world file could not be opened (${core.storageError}).`);
  }
  scope.postMessage({ t: 'ready', persisted: core.persisted });
  for (const msg of queued.splice(0)) handle(msg);

  // Fixed-step simulation clock (the core accumulates real time into 60 Hz steps).
  let last = performance.now();
  // The server's ticks and their time over each second, for the status line's server figures.
  let ticks = 0;
  let busyMs = 0;
  let tick: number | null = null;
  setInterval(() => {
    if (!core) return;
    scope.postMessage({ t: 'memory', bytes: core.heapBytes() });
    scope.postMessage({ t: 'perf', ticks, busyMs });
    scope.postMessage({
      t: 'stats',
      stats: {
        ticks: tickStats.ticks,
        tickMeanMs: tickStats.ticks ? tickStats.ms / tickStats.ticks : 0,
        advanceMaxMs: tickStats.maxMs,
        water: water ? 'worker' : 'inline',
        waterStats: core.waterStats(),
      },
    });
    ticks = 0;
    busyMs = 0;
    Object.assign(tickStats, { ticks: 0, ms: 0, maxMs: 0 });
  }, 1000);
  setInterval(() => {
    const now = performance.now();
    if (!paused && core) {
      // `advance` returns the server's tick: the ticks run are its difference.
      const t = core.advance((now - last) / 1000);
      const ms = performance.now() - now;
      busyMs += ms;
      if (tick !== null && t !== tick) {
        ticks += t - tick;
        tickStats.ticks += t - tick;
        tickStats.ms += ms;
        tickStats.maxMs = Math.max(tickStats.maxMs, ms);
      }
      tick = t;
    }
    last = now;
    flush();
  }, 4);
}

/** The maps of a `start` with `mapsLater`, resolved by the `maps` message. */
let mapsArrived: (maps: Uint8Array[]) => void = () => undefined;
const laterMaps = new Promise<Uint8Array[]>((resolve) => {
  mapsArrived = resolve;
});

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg.t === 'maps') mapsArrived(msg.maps);
  else if (msg.t === 'start')
    void start(
      msg.worldSeed,
      msg.generatorVersion,
      msg.file,
      msg.spawnFaceB,
      msg.spawnPortal,
      msg.spawnOnOpen,
      msg.mapsLater ? laterMaps : msg.maps,
      msg.spawnAt,
      msg.upgrade,
      msg.waterInline,
      msg.jobsInline,
    );
  else handle(msg);
};
