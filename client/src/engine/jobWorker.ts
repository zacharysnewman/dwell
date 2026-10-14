// A job worker (ARCHITECTURE.md §5.1, ADR 0007): its own instance of the Rust client's WASM runs
// the jobs the main thread's pool posts (generation, LOD sections, the map, meshing), one
// transferred buffer each way (web/jobs.rs).
import { JobWorker, loadWasm } from './wasm';

interface WorkerScope {
  postMessage(message: ArrayBuffer, transfer: Transferable[]): void;
  onmessage:
    | ((
        e: MessageEvent<ArrayBuffer | { t: 'init'; generatorVersion?: number; worldSeed?: bigint }>,
      ) => void)
    | null;
}
const scope = self as unknown as WorkerScope;

let worker: JobWorker | null = null;
const queued: ArrayBuffer[] = [];

function run(w: JobWorker, job: ArrayBuffer): void {
  const out = w.run(new Uint8Array(job));
  scope.postMessage(out.buffer as ArrayBuffer, [out.buffer as ArrayBuffer]);
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg instanceof ArrayBuffer) {
    if (worker) run(worker, msg);
    else queued.push(msg);
    return;
  }
  void loadWasm().then(() => {
    const w =
      msg.generatorVersion === undefined || msg.worldSeed === undefined
        ? JobWorker.mesher()
        : JobWorker.world(msg.generatorVersion, msg.worldSeed);
    worker = w;
    for (const job of queued.splice(0)) run(w, job);
  });
};
