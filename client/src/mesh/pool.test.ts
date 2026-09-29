import { describe, expect, it } from 'vitest';
import type { WorkerLike } from '../worldgen/pool';
import { LOD_VOLUME, lodCell } from '../lod/grid';
import { meshSection } from './lodMesher';
import { meshBuffers, meshChunk, PADDED_VOLUME, paddedIndex } from './mesher';
import type { ToMesher } from './messages';
import { MeshPool } from './pool';

/** A worker that meshes on demand, when the test says so. */
class FakeWorker implements WorkerLike {
  onmessage: ((e: MessageEvent) => void) | null = null;
  inbox: ToMesher[] = [];
  transferred = 0;
  postMessage(message: unknown, transfer?: Transferable[]): void {
    this.inbox.push(message as ToMesher);
    this.transferred += transfer?.length ?? 0;
  }
  terminate(): void {}
  answer(): void {
    const job = this.inbox.shift();
    if (!job) throw new Error('no job');
    if (job.t === 'lod') {
      this.onmessage?.({
        data: { t: 'lod', id: job.id, meshes: meshSection(job.cells, job.liquids, job.surface) },
      } as MessageEvent);
      return;
    }
    const meshes = meshChunk(job.voxels);
    meshBuffers(meshes);
    this.onmessage?.({ data: { t: 'mesh', id: job.id, meshes } } as MessageEvent);
  }
}

function block(): Uint16Array<ArrayBuffer> {
  const v = new Uint16Array(PADDED_VOLUME);
  v[paddedIndex(0, 0, 0)] = 2;
  return v;
}

describe('MeshPool', () => {
  it('spreads jobs over workers, a few each, transferring the voxels', async () => {
    const workers = [new FakeWorker(), new FakeWorker()];
    const pool = new MeshPool(workers);
    const results = [1, 2, 3, 4, 5].map(() => pool.mesh(block()));
    expect(workers.map((w) => w.inbox.length)).toEqual([2, 2]);
    expect(pool.pending).toBe(5);
    expect(workers[0]?.transferred).toBe(2);
    workers[0]?.answer();
    expect(workers[0]?.inbox.length).toBe(2); // the fifth job moved in
    for (let i = 0; i < 2; i++) for (const w of workers) if (w.inbox.length) w.answer();
    const meshes = await Promise.all(results);
    expect(meshes.every((m) => m.opaque.indices.length === 36)).toBe(true);
    expect(pool.pending).toBe(0);
  });

  it('meshes LOD sections in the same workers', async () => {
    const workers = [new FakeWorker()];
    const pool = new MeshPool(workers);
    const cells = new Uint16Array(LOD_VOLUME);
    cells[lodCell(3, 3, 3)] = 2;
    const chunk = pool.mesh(block());
    const section = pool.meshSection(cells);
    expect(workers[0]?.inbox.map((j) => j.t)).toEqual(['mesh', 'lod']);
    workers[0]?.answer();
    workers[0]?.answer();
    expect((await chunk).opaque.indices.length).toBe(36);
    expect((await section).opaque.indices.length).toBe(36);
  });
});
