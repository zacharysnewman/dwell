// A worldgen worker (ARCHITECTURE.md §5.1, ADR 0007): its own instance of the terrain generator
// (dwell_worldgen.wasm) generating chunks for the main thread, which receives each chunk's voxels
// as a transferred buffer.
import { ChunkGenerator, dwellWorldgenUrl, type DwellWorldgenFactory } from './generator';
import type { FromWorldgen, ToWorldgen } from './messages';
import { versionedLocateFile } from '../sim/wasmUrl';

interface WorkerScope {
  postMessage(message: FromWorldgen, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ToWorldgen>) => void) | null;
}
const scope = self as unknown as WorkerScope;

let generator: ChunkGenerator | null = null;
const queued: ToWorldgen[] = [];
let reportedBytes = 0;

/** Tells the pool when the WebAssembly memory has grown (the debug overlay's memory line). */
function reportMemory(g: ChunkGenerator): void {
  const bytes = g.heapBytes();
  if (bytes === reportedBytes) return;
  reportedBytes = bytes;
  scope.postMessage({ t: 'memory', bytes });
}

function generate(msg: ToWorldgen, g: ChunkGenerator): void {
  if (msg.t === 'generate') {
    const voxels = g.generate(msg.coord);
    scope.postMessage({ t: 'chunk', id: msg.id, voxels, hash: g.lastHash() }, [voxels.buffer]);
  } else if (msg.t === 'map') {
    const bytes = g.map(msg.x0, msg.z0, msg.step, msg.n);
    scope.postMessage({ t: 'map', id: msg.id, bytes }, bytes ? [bytes.buffer] : []);
  } else if (msg.t === 'tint') {
    const bytes = g.tint(msg.cx, msg.cz);
    scope.postMessage({ t: 'tint', id: msg.id, bytes }, bytes ? [bytes.buffer] : []);
  } else if (msg.t === 'lod') {
    const s = g.lod(msg.coord);
    const surface = s.surface ?? null;
    scope.postMessage(
      { t: 'lod', id: msg.id, kind: s.kind, cells: s.cells, surface },
      surface ? [s.cells.buffer, surface.buffer] : [s.cells.buffer],
    );
  } else if (msg.t === 'bounds') {
    scope.postMessage({ t: 'bounds', id: msg.id, ...g.lodBounds(msg.level, msg.i, msg.k) });
  }
  reportMemory(g);
}

async function init(generatorVersion: number, worldSeed: bigint): Promise<void> {
  try {
    const mod = (await import(/* @vite-ignore */ dwellWorldgenUrl())) as {
      default: DwellWorldgenFactory;
    };
    // Its .wasm with the same build version as the loader (sim/wasmUrl.ts).
    const factory: DwellWorldgenFactory = (options = {}) =>
      mod.default({ locateFile: versionedLocateFile(), ...options });
    generator = await ChunkGenerator.load(factory, generatorVersion, worldSeed);
  } catch (err) {
    scope.postMessage({
      t: 'error',
      message: `Worldgen unavailable (${err instanceof Error ? err.message : String(err)}).`,
    });
    return;
  }
  scope.postMessage({ t: 'ready' });
  reportMemory(generator);
  for (const msg of queued.splice(0)) generate(msg, generator);
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg.t === 'init') void init(msg.generatorVersion, msg.worldSeed);
  else if (generator) generate(msg, generator);
  else queued.push(msg);
};
