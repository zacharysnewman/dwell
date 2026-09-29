// A meshing worker (ARCHITECTURE.md §5.1, ADR 0007): greedy-meshes chunks for the main thread,
// which receives the geometry as transferred buffers.
import { meshBuffers, meshChunk } from './mesher';
import type { FromMesher, ToMesher } from './messages';

interface WorkerScope {
  postMessage(message: FromMesher, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ToMesher>) => void) | null;
}
const scope = self as unknown as WorkerScope;

scope.onmessage = (e) => {
  const meshes = meshChunk(e.data.voxels);
  scope.postMessage({ t: 'mesh', id: e.data.id, meshes }, meshBuffers(meshes));
};
