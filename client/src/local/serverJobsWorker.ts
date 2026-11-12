// The server-jobs worker (docs/WATER_SURFACE_GRID.md §2d, the follow-up; ARCHITECTURE.md §5.1): the
// local server's jobs on a thread of their own — the chunks it wants generated and its LOD sections
// to compute (a section generated, its modified octants downsampled) — started by the local-mode
// worker (worker.ts), so new terrain and a flood's rewritten chunks don't hold the server's tick.
// Jobs come as transferred ArrayBuffers (`LocalWorld::takeJob`), each answered with one
// (`LocalWorld::jobIn`).
import { ServerJobs, installWorldMap, loadWasm } from '../engine/wasm';

type Start = { t: 'start'; maps?: Uint8Array[] };
/** `radiusScale`: the world's size (BIG_MAP.md M2b; absent or 1: today's). */
type Init = { t: 'init'; generatorVersion: number; worldSeed: bigint; radiusScale?: number };

interface WorkerScope {
  postMessage(message: ArrayBuffer, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ArrayBuffer | Start | Init>) => void) | null;
}
const scope = self as unknown as WorkerScope;

let runner: ServerJobs | null = null;
let ready: Promise<unknown> | null = null;
const queued: ArrayBuffer[] = [];

function run(w: ServerJobs, job: ArrayBuffer): void {
  const out = w.run(new Uint8Array(job));
  if (out.length) scope.postMessage(out.buffer as ArrayBuffer, [out.buffer as ArrayBuffer]);
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg instanceof ArrayBuffer) {
    if (runner) run(runner, msg);
    else queued.push(msg);
    return;
  }
  if (msg.t === 'start') {
    // The world's maps first (ADR 0049): the generators read them.
    ready = loadWasm().then(() => {
      for (const map of msg.maps ?? []) {
        try {
          installWorldMap(map);
        } catch (err) {
          console.warn('Dwell: a world map was refused by the server-jobs worker', err);
        }
      }
    });
    return;
  }
  void (ready ?? loadWasm()).then(() => {
    const w = new ServerJobs(msg.generatorVersion, msg.worldSeed, msg.radiusScale);
    runner = w;
    for (const job of queued.splice(0)) run(w, job);
  });
};
