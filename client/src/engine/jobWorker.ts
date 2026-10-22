// A job worker (ARCHITECTURE.md §5.1, ADR 0007): its own instance of the Rust client's WASM runs
// the jobs the main thread's pool posts (generation, LOD sections, the map, meshing), one
// transferred buffer each way (web/jobs.rs).
// The same script builds a world map on its own (`buildMap`, ADR 0049): a worker the page starts
// for one face's map and ends once it has the bytes.
import { JobWorker, buildWorldMap, installWorldMap, loadWasm } from './wasm';

type Init = {
  t: 'init';
  generatorVersion?: number;
  worldSeed?: bigint;
  /** The world's generation settings, encoded (WORLD_GENERATION.md §9). */
  worldgen?: string;
  /** The world's maps (faces A and B), installed before the world's generator is made. */
  maps?: Uint8Array[];
};
type BuildMap = { t: 'buildMap'; seed: bigint; face: number; worldgen?: string };

interface WorkerScope {
  postMessage(
    message: ArrayBuffer | { memory: number } | { map?: Uint8Array; error?: string },
    transfer?: Transferable[],
  ): void;
  onmessage: ((e: MessageEvent<ArrayBuffer | Init | BuildMap>) => void) | null;
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
  if (msg.t === 'buildMap') {
    void loadWasm().then(() => {
      try {
        const map = buildWorldMap(msg.seed, msg.face, msg.worldgen ?? '');
        scope.postMessage({ map }, [map.buffer as ArrayBuffer]);
      } catch (err) {
        scope.postMessage({ error: err instanceof Error ? err.message : String(err) });
      }
    });
    return;
  }
  void loadWasm().then((exports) => {
    memory = (exports as { memory?: WebAssembly.Memory }).memory ?? null;
    for (const map of msg.maps ?? []) installWorldMap(map);
    const w =
      msg.generatorVersion === undefined || msg.worldSeed === undefined
        ? JobWorker.mesher()
        : JobWorker.world(msg.generatorVersion, msg.worldSeed, msg.worldgen ?? '');
    worker = w;
    for (const job of queued.splice(0)) run(w, job);
  });
};
