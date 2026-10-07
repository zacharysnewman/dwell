// A meshing worker (ARCHITECTURE.md §5.1, ADR 0007): greedy-meshes chunks for the main thread,
// which receives the geometry as transferred buffers.
import { meshSection, sectionBuffers } from './lodMesher';
import { meshBuffers, meshChunk } from './mesher';
import type { FromMesher, ToMesher } from './messages';

interface WorkerScope {
  postMessage(message: FromMesher, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ToMesher>) => void) | null;
}
const scope = self as unknown as WorkerScope;

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg.t === 'lod') {
    const meshes = meshSection(msg.cells, msg.options);
    scope.postMessage({ t: 'lod', id: msg.id, meshes }, sectionBuffers(meshes));
    return;
  }
  const meshes = meshChunk(msg.voxels, msg.tint, msg.mirror);
  scope.postMessage({ t: 'mesh', id: msg.id, meshes }, meshBuffers(meshes));
};
