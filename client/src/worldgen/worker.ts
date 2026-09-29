// A worldgen worker (ARCHITECTURE.md §5.1, ADR 0007): its own instance of the terrain generator
// (dwell_worldgen.wasm) generating chunks for the main thread, which receives each chunk's voxels
// as a transferred buffer.
import { ChunkGenerator, dwellWorldgenUrl, type DwellWorldgenFactory } from './generator';
import type { FromWorldgen, ToWorldgen } from './messages';

interface WorkerScope {
  postMessage(message: FromWorldgen, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ToWorldgen>) => void) | null;
}
const scope = self as unknown as WorkerScope;

let generator: ChunkGenerator | null = null;
const queued: ToWorldgen[] = [];

function generate(msg: ToWorldgen, g: ChunkGenerator): void {
  if (msg.t === 'generate') {
    const voxels = g.generate(msg.coord);
    scope.postMessage({ t: 'chunk', id: msg.id, voxels, hash: g.lastHash() }, [voxels.buffer]);
  } else if (msg.t === 'map') {
    const bytes = g.map(msg.x0, msg.z0, msg.step, msg.n);
    scope.postMessage({ t: 'map', id: msg.id, bytes }, bytes ? [bytes.buffer] : []);
  } else if (msg.t === 'lod') {
    const s = g.lod(msg.coord);
    scope.postMessage({ t: 'lod', id: msg.id, kind: s.kind, cells: s.cells }, [s.cells.buffer]);
  } else if (msg.t === 'bounds') {
    scope.postMessage({ t: 'bounds', id: msg.id, ...g.lodBounds(msg.level, msg.i, msg.k) });
  }
}

async function init(generatorVersion: number, worldSeed: bigint): Promise<void> {
  try {
    const mod = (await import(/* @vite-ignore */ dwellWorldgenUrl())) as {
      default: DwellWorldgenFactory;
    };
    generator = await ChunkGenerator.load(mod.default, generatorVersion, worldSeed);
  } catch (err) {
    scope.postMessage({
      t: 'error',
      message: `Worldgen unavailable (${err instanceof Error ? err.message : String(err)}).`,
    });
    return;
  }
  scope.postMessage({ t: 'ready' });
  for (const msg of queued.splice(0)) generate(msg, generator);
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg.t === 'init') void init(msg.generatorVersion, msg.worldSeed);
  else if (generator) generate(msg, generator);
  else queued.push(msg);
};
