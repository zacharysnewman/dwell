import { describe, expect, it } from 'vitest';
import { PADDED_VOLUME } from '../mesh/mesher';
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import { CHUNK_SIZE, ChunkForm } from '../protocol/constants.gen';
import type { ChunkCoord, Vec3 } from '../protocol/messages';
import type { ChunkMeshes } from '../mesh/mesher';
import { InlineMesher } from '../mesh/pool';
import type { ChunkSource, GeneratedChunk } from '../worldgen/pool';
import {
  chunkKey,
  ChunkStreamer,
  MIN_CHUNK_Y,
  type ChunkStore,
  type TerrainView,
} from './chunkStream';

/** Chunk source whose jobs the test completes by hand. */
class ManualSource implements ChunkSource {
  jobs = new Map<string, { coord: ChunkCoord; resolve: (c: GeneratedChunk) => void }>();
  cancelled: string[] = [];
  get pending(): number {
    return this.jobs.size;
  }
  generate(coord: ChunkCoord): Promise<GeneratedChunk> {
    return new Promise((resolve) => this.jobs.set(chunkKey(coord), { coord, resolve }));
  }
  cancel(coord: ChunkCoord): boolean {
    this.cancelled.push(chunkKey(coord));
    return this.jobs.delete(chunkKey(coord));
  }
  /** Completes a job with voxels filled with the chunk's x coordinate + 1. */
  finish(coord: ChunkCoord): void {
    const job = this.jobs.get(chunkKey(coord));
    if (!job) throw new Error(`no job for ${chunkKey(coord)}`);
    this.jobs.delete(chunkKey(coord));
    job.resolve({ voxels: new Uint16Array(CHUNK_VOLUME).fill(coord[0] + 1), hash: 0n });
  }
}

class FakeStore implements ChunkStore {
  chunks = new Map<string, { revision: number; first: number }>();
  edits: { key: string; revision: number; changes: number[] }[] = [];
  meshedFrom: string[] = [];
  setChunk(coord: ChunkCoord, revision: number, voxels: Uint16Array): void {
    this.chunks.set(chunkKey(coord), { revision, first: voxels[0] ?? -1 });
  }
  removeChunk(coord: ChunkCoord): void {
    this.chunks.delete(chunkKey(coord));
  }
  editChunk(coord: ChunkCoord, revision: number, changes: Uint16Array): void {
    const key = chunkKey(coord);
    this.edits.push({ key, revision, changes: [...changes] });
    const c = this.chunks.get(key);
    if (c) c.revision = revision;
  }
  paddedChunk(cx: number, cy: number, cz: number): Uint16Array<ArrayBuffer> {
    this.meshedFrom.push(chunkKey([cx, cy, cz]));
    return new Uint16Array(PADDED_VOLUME);
  }
}

class FakeView implements TerrainView {
  chunks = new Map<string, Vec3>();
  setTerrainChunk(key: string, origin: Vec3, meshes: ChunkMeshes | null): void {
    if (meshes) this.chunks.set(key, origin);
    else this.chunks.delete(key);
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

function setup() {
  const source = new ManualSource();
  const store = new FakeStore();
  const view = new FakeView();
  const resyncs: string[] = [];
  const streamer = new ChunkStreamer(store, source, new InlineMesher(), view, (coords) => {
    resyncs.push(...coords.map(chunkKey));
  });
  return { source, store, view, resyncs, streamer };
}

const modification = (coord: ChunkCoord, revision: number, ...pairs: number[]) => ({
  coord,
  revision,
  changes: Uint16Array.from(pairs),
});

const generated = (coord: ChunkCoord) => ({
  form: ChunkForm.Generated,
  coord,
  revision: 0,
  voxels: null,
});

describe('ChunkStreamer', () => {
  it('treats Air chunks as loaded without generating, storing, or meshing them', async () => {
    const { source, store, view, streamer } = setup();
    for (let y = -1; y <= 1; y++)
      for (let z = -1; z <= 1; z++)
        for (let x = -1; x <= 1; x++)
          streamer.onChunkData({
            form: ChunkForm.Air,
            coord: [x, y, z],
            revision: 0,
            voxels: null,
          });
    expect(source.pending).toBe(0);
    expect(store.chunks.size).toBe(0);
    expect(streamer.readyAround([16, 16, 16])).toBe(true);
    expect(streamer.meshDirty([0, 0, 0], 100)).toBe(0);
    expect(view.chunks.size).toBe(0);
    // Unloading an Air chunk removes nothing from the store; a stored chunk replaced by Air is
    // dropped from the store and the view.
    streamer.onChunkData({
      form: ChunkForm.Explicit,
      coord: [5, 0, 0],
      revision: 1,
      voxels: new Uint16Array(CHUNK_VOLUME).fill(2),
    });
    streamer.meshDirty([0, 0, 0], 100);
    await flush();
    expect(view.chunks.has('5,0,0')).toBe(true);
    streamer.onChunkData({ form: ChunkForm.Air, coord: [5, 0, 0], revision: 0, voxels: null });
    expect(store.chunks.has('5,0,0')).toBe(false);
    expect(view.chunks.has('5,0,0')).toBe(false);
    streamer.onChunkUnload([[0, 0, 0]]);
    expect(streamer.isLoaded([0, 0, 0])).toBe(false);
    expect(streamer.readyAround([16, 16, 16])).toBe(false);
  });

  it('stores Explicit chunks directly and Generated ones once generated', async () => {
    const { source, store, streamer } = setup();
    streamer.onChunkData({
      form: ChunkForm.Explicit,
      coord: [5, 0, 0],
      revision: 3,
      voxels: new Uint16Array(CHUNK_VOLUME).fill(9),
    });
    expect(store.chunks.get('5,0,0')).toEqual({ revision: 3, first: 9 });

    streamer.onChunkData(generated([1, 0, 0]));
    expect(store.chunks.has('1,0,0')).toBe(false);
    expect(streamer.stats().generating).toBe(1);
    source.finish([1, 0, 0]);
    await flush();
    expect(store.chunks.get('1,0,0')).toEqual({ revision: 0, first: 2 });
    expect(streamer.isLoaded([1, 0, 0])).toBe(true);
  });

  it('unloads chunks: from the sim, from the renderer, and cancels pending generation', async () => {
    const { source, store, view, streamer } = setup();
    streamer.onChunkData(generated([0, 0, 0]));
    source.finish([0, 0, 0]);
    await flush();
    streamer.meshDirty([0, 0, 0], 10);
    await flush();
    expect(view.chunks.has('0,0,0')).toBe(true);
    streamer.onChunkData(generated([3, 0, 0]));

    streamer.onChunkUnload([
      [0, 0, 0],
      [3, 0, 0],
    ]);
    expect(store.chunks.size).toBe(0);
    expect(view.chunks.size).toBe(0);
    expect(source.cancelled).toContain('3,0,0');
    expect(streamer.stats()).toEqual({
      loaded: 0,
      generating: 0,
      meshed: 0,
      dirty: 0,
      meshing: 0,
      resyncs: 0,
    });
  });

  it('ignores a generation that finishes after its chunk was unloaded', async () => {
    const { source, store, streamer } = setup();
    streamer.onChunkData(generated([2, 0, 0]));
    const job = source.jobs.get('2,0,0');
    streamer.onChunkUnload([[2, 0, 0]]);
    job?.resolve({ voxels: new Uint16Array(CHUNK_VOLUME), hash: 0n });
    await flush();
    expect(store.chunks.size).toBe(0);
  });

  it('meshes nearest first, within the budget, after neighbours being generated arrive', async () => {
    const { source, store, streamer } = setup();
    for (const c of [
      [0, 0, 0],
      [4, 0, 0],
      [8, 0, 0],
    ] as ChunkCoord[]) {
      streamer.onChunkData(generated(c));
      source.finish(c);
    }
    await flush();
    streamer.onChunkData(generated([5, 0, 0])); // a neighbour of [4, 0, 0], still generating
    // A clock that runs out after every chunk: one chunk per call.
    let t = 0;
    const clock = () => (t += 10);
    const center: Vec3 = [8 * 32 + 16, 16, 16];
    expect(streamer.meshDirty(center, 5, clock)).toBe(1);
    expect(store.meshedFrom).toEqual(['8,0,0']);
    expect(streamer.meshDirty(center, 5, clock)).toBe(1);
    expect(store.meshedFrom).toEqual(['8,0,0', '0,0,0']); // [4,0,0] waits for [5,0,0]
    expect(streamer.meshDirty(center, 5, clock)).toBe(0);
    source.finish([5, 0, 0]);
    await flush();
    streamer.meshDirty(center, 100);
    expect(store.meshedFrom.slice(2).sort()).toEqual(['4,0,0', '5,0,0']);
  });

  it('re-meshes loaded neighbours when a chunk arrives', async () => {
    const { source, store, streamer } = setup();
    streamer.onChunkData(generated([0, 0, 0]));
    source.finish([0, 0, 0]);
    await flush();
    streamer.meshDirty([0, 0, 0], 100);
    streamer.onChunkData(generated([0, 1, 0]));
    source.finish([0, 1, 0]);
    await flush();
    streamer.meshDirty([0, 0, 0], 100);
    expect(store.meshedFrom.sort()).toEqual(['0,0,0', '0,0,0', '0,1,0']);
  });

  it("keeps at most the mesher's capacity of jobs in flight", async () => {
    const source = new ManualSource();
    const store = new FakeStore();
    const view = new FakeView();
    const mesher = Object.assign(new InlineMesher(), { capacity: 2 });
    const streamer = new ChunkStreamer(store, source, mesher, view);
    for (let x = 0; x < 5; x += 2)
      streamer.onChunkData({
        form: ChunkForm.Explicit,
        coord: [x, 0, 0],
        revision: 0,
        voxels: new Uint16Array(CHUNK_VOLUME),
      });
    expect(streamer.meshDirty([0, 0, 0], 100)).toBe(2);
    expect(streamer.meshDirty([0, 0, 0], 100)).toBe(0);
    await flush();
    expect(view.chunks.size).toBe(2);
    expect(streamer.meshDirty([0, 0, 0], 100)).toBe(1);
  });

  it('applies voxel modifications in revision order and re-meshes the chunk and its neighbours', async () => {
    const { store, streamer, resyncs } = setup();
    const explicit = (coord: ChunkCoord, revision: number) => ({
      form: ChunkForm.Explicit,
      coord,
      revision,
      voxels: new Uint16Array(CHUNK_VOLUME),
    });
    streamer.onChunkData(explicit([0, 0, 0], 3));
    streamer.onChunkData(explicit([1, 0, 0], 0));
    streamer.onChunkData(explicit([0, 1, 0], 0));
    streamer.meshDirty([0, 0, 0], 100);
    await flush();
    store.meshedFrom = [];
    // A voxel on the +X border (x = 31): the chunk and its +X neighbour re-mesh.
    streamer.onVoxelModification([modification([0, 0, 0], 4, 31 | (5 << 5), 2)]);
    expect(store.edits).toEqual([{ key: '0,0,0', revision: 4, changes: [31 | (5 << 5), 2] }]);
    expect(streamer.revision([0, 0, 0])).toBe(4);
    streamer.meshDirty([0, 0, 0], 100);
    expect(store.meshedFrom.sort()).toEqual(['0,0,0', '1,0,0']);
    // Stale revisions are ignored; unknown chunks too.
    streamer.onVoxelModification([modification([0, 0, 0], 4, 0, 3)]);
    streamer.onVoxelModification([modification([9, 9, 9], 1, 0, 3)]);
    expect(store.edits.length).toBe(1);
    // A gap asks for a resync (once), and nothing is applied until the chunk comes again.
    streamer.onVoxelModification([modification([0, 0, 0], 6, 0, 3)]);
    streamer.onVoxelModification([modification([0, 0, 0], 7, 0, 3)]);
    expect(resyncs).toEqual(['0,0,0']);
    expect(store.edits.length).toBe(1);
    expect(streamer.stats().resyncs).toBe(1);
    streamer.onChunkData(explicit([0, 0, 0], 7));
    streamer.onVoxelModification([modification([0, 0, 0], 8, 0, 3)]);
    expect(store.edits.length).toBe(2);
    expect(streamer.revision([0, 0, 0])).toBe(8);
  });

  it('holds modifications of a chunk still generating, and edits Air chunks into stored ones', async () => {
    const { source, store, streamer, resyncs } = setup();
    streamer.onChunkData(generated([2, 0, 0]));
    streamer.onVoxelModification([modification([2, 0, 0], 1, 7, 2)]);
    streamer.onVoxelModification([modification([2, 0, 0], 2, 8, 2)]);
    expect(store.edits).toEqual([]);
    source.finish([2, 0, 0]);
    await flush();
    expect(store.edits.map((e) => e.revision)).toEqual([1, 2]);
    expect(resyncs).toEqual([]);

    streamer.onChunkData({ form: ChunkForm.Air, coord: [0, 5, 0], revision: 0, voxels: null });
    streamer.onVoxelModification([modification([0, 5, 0], 1, 0, 2)]);
    expect(store.chunks.get('0,5,0')).toEqual({ revision: 1, first: 0 });
    streamer.meshDirty([0, 160, 0], 100);
    expect(store.meshedFrom).toContain('0,5,0');
  });

  it('is ready once the chunks around the player are loaded', () => {
    const { streamer } = setup();
    const at: Vec3 = [10, 70, -5]; // chunk (0, 2, -1)
    expect(streamer.readyAround(at)).toBe(false);
    for (let y = 1; y <= 3; y++)
      for (let z = -2; z <= 0; z++)
        for (let x = -1; x <= 1; x++)
          streamer.onChunkData({
            form: ChunkForm.Explicit,
            coord: [x, y, z],
            revision: 0,
            voxels: new Uint16Array(CHUNK_VOLUME),
          });
    expect(streamer.readyAround(at)).toBe(true);
  });

  it('only needs rows inside the world at its bottom', () => {
    const { streamer } = setup();
    const at: Vec3 = [0, MIN_CHUNK_Y * CHUNK_SIZE + 8, 0]; // the lowest chunk row
    for (let y = MIN_CHUNK_Y; y <= MIN_CHUNK_Y + 1; y++)
      for (let z = -1; z <= 1; z++)
        for (let x = -1; x <= 1; x++)
          streamer.onChunkData({
            form: ChunkForm.Explicit,
            coord: [x, y, z],
            revision: 0,
            voxels: new Uint16Array(CHUNK_VOLUME),
          });
    expect(streamer.readyAround(at)).toBe(true);
  });
});
