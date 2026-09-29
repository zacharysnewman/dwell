import { describe, expect, it } from 'vitest';
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import { ChunkForm } from '../protocol/constants.gen';
import type { ChunkCoord, Vec3 } from '../protocol/messages';
import type { ChunkSource, GeneratedChunk } from '../worldgen/pool';
import { chunkKey, ChunkStreamer, type ChunkStore, type TerrainView } from './chunkStream';

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
  meshedFrom: string[] = [];
  setChunk(coord: ChunkCoord, revision: number, voxels: Uint16Array): void {
    this.chunks.set(chunkKey(coord), { revision, first: voxels[0] ?? -1 });
  }
  removeChunk(coord: ChunkCoord): void {
    this.chunks.delete(chunkKey(coord));
  }
  chunkFaces(cx: number, cy: number, cz: number): Uint8Array {
    this.meshedFrom.push(chunkKey([cx, cy, cz]));
    return new Uint8Array(8);
  }
}

class FakeView implements TerrainView {
  chunks = new Map<string, Vec3>();
  setTerrainChunk(key: string, origin: Vec3, faces: Uint8Array | null): void {
    if (faces) this.chunks.set(key, origin);
    else this.chunks.delete(key);
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

function setup() {
  const source = new ManualSource();
  const store = new FakeStore();
  const view = new FakeView();
  return { source, store, view, streamer: new ChunkStreamer(store, source, view) };
}

const generated = (coord: ChunkCoord) => ({
  form: ChunkForm.Generated,
  coord,
  revision: 0,
  voxels: null,
});

describe('ChunkStreamer', () => {
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
    expect(view.chunks.has('0,0,0')).toBe(true);
    streamer.onChunkData(generated([3, 0, 0]));

    streamer.onChunkUnload([
      [0, 0, 0],
      [3, 0, 0],
    ]);
    expect(store.chunks.size).toBe(0);
    expect(view.chunks.size).toBe(0);
    expect(source.cancelled).toContain('3,0,0');
    expect(streamer.stats()).toEqual({ loaded: 0, generating: 0, meshed: 0, dirty: 0 });
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
    const at: Vec3 = [0, -120, 0]; // chunk row −4, the lowest
    for (let y = -4; y <= -3; y++)
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
