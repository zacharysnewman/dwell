// The water worker (ADR 0062, docs/WATER_SURFACE_GRID.md §2d): the local server's water grid on a
// thread of its own, started by the local-mode worker (worker.ts). It takes the server's batches
// (transferred ArrayBuffers, `LocalWorld::takeWaterOut`), runs what the water is owed in runs of a
// bounded budget, and answers each with a batch of its own (`LocalWorld::waterIn`).
import { WaterWorker, installWorldMap, loadWasm } from '../engine/wasm';

type Start = { t: 'start'; maps?: Uint8Array[] };

interface WorkerScope {
  postMessage(message: ArrayBuffer, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ArrayBuffer | Start>) => void) | null;
}
const scope = self as unknown as WorkerScope;

let water: WaterWorker | null = null;
const queued: ArrayBuffer[] = [];
let scheduled = false;

/** Runs once the messages waiting have been taken: the water's work, then again while it owes. */
function schedule(): void {
  if (scheduled) return;
  scheduled = true;
  setTimeout(() => {
    scheduled = false;
    if (!water) return;
    const out = water.work();
    scope.postMessage(out.buffer as ArrayBuffer, [out.buffer as ArrayBuffer]);
    if (water.busy()) schedule();
  }, 0);
}

function take(w: WaterWorker, b: ArrayBuffer): void {
  if (!w.receive(new Uint8Array(b)))
    console.warn('Dwell: the water worker got a batch it cannot read');
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg instanceof ArrayBuffer) {
    if (water) {
      take(water, msg);
      schedule();
    } else queued.push(msg);
    return;
  }
  void loadWasm().then(() => {
    // The world's maps first (ADR 0049): the water's rivers read them.
    for (const map of msg.maps ?? []) {
      try {
        installWorldMap(map);
      } catch (err) {
        console.warn('Dwell: a world map was refused by the water worker', err);
      }
    }
    const w = new WaterWorker();
    water = w;
    for (const b of queued.splice(0)) take(w, b);
    schedule();
  });
};
