// A job worker (ARCHITECTURE.md §5.1, ADR 0007): its own instance of the Rust client's WASM runs
// the jobs the main thread's pool posts (generation, LOD sections, the map, meshing), one
// transferred buffer each way (web/jobs.rs).
import { JobWorker, loadWasm } from './wasm';

interface WorkerScope {
  postMessage(message: ArrayBuffer | { memory: number }, transfer?: Transferable[]): void;
  onmessage:
    | ((
        e: MessageEvent<ArrayBuffer | { t: 'init'; generatorVersion?: number; worldSeed?: bigint }>,
      ) => void)
    | null;
}
const scope = self as unknown as WorkerScope;

let worker: JobWorker | null = null;
const queued: ArrayBuffer[] = [];

let memory: WebAssembly.Memory | null = null;
let reported = 0;

function run(w: JobWorker, job: ArrayBuffer): void {
  const out = w.run(new Uint8Array(job));
  scope.postMessage(out.buffer as ArrayBuffer, [out.buffer as ArrayBuffer]);
  // The module's memory grows, never shrinks: say when it has (the F3 memory line).
  const bytes = memory?.buffer.byteLength ?? 0;
  if (bytes !== reported) {
    reported = bytes;
    scope.postMessage({ memory: bytes });
  }
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg instanceof ArrayBuffer) {
    if (worker) run(worker, msg);
    else queued.push(msg);
    return;
  }
  void loadWasm().then((exports) => {
    memory = (exports as { memory?: WebAssembly.Memory }).memory ?? null;
    const w =
      msg.generatorVersion === undefined || msg.worldSeed === undefined
        ? JobWorker.mesher()
        : JobWorker.world(msg.generatorVersion, msg.worldSeed);
    worker = w;
    for (const job of queued.splice(0)) run(w, job);
  });
};
