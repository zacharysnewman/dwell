import { beforeEach, describe, expect, it } from 'vitest';
import {
  forgetWorldMaps,
  GENERATOR_TERRAIN,
  type MapDeps,
  type MapStore,
  oldest,
  worldMaps,
} from './worldMaps';

class MemoryStore implements MapStore {
  readonly entries = new Map<string, Uint8Array>();
  failing = false;
  get(key: string): Promise<Uint8Array | null> {
    if (this.failing) return Promise.reject(new Error('blocked'));
    return Promise.resolve(this.entries.get(key) ?? null);
  }
  put(key: string, bytes: Uint8Array): Promise<void> {
    if (this.failing) return Promise.reject(new Error('blocked'));
    this.entries.set(key, bytes);
    return Promise.resolve();
  }
}

function deps(store: MapStore | null): MapDeps & { builds: string[]; progress: number[][] } {
  const builds: string[] = [];
  const progress: number[][] = [];
  return {
    builds,
    progress,
    keys: (seed) => [`fp-a-${seed.toString()}`, `fp-b-${seed.toString()}`],
    build: (seed, face) => {
      builds.push(`${seed.toString()}/${String(face)}`);
      return Promise.resolve(new Uint8Array([Number(seed % 256n), face]));
    },
    store,
    onProgress: (ready, needed) => progress.push([ready, needed]),
  };
}

describe('world maps (ADR 0049)', () => {
  beforeEach(() => {
    forgetWorldMaps();
  });

  it('builds both faces once, keeps them, and reads them back on a later visit', async () => {
    const store = new MemoryStore();
    const first = deps(store);
    const maps = await worldMaps(7n, GENERATOR_TERRAIN, first);
    expect(maps).toEqual([new Uint8Array([7, 0]), new Uint8Array([7, 1])]);
    expect(first.builds).toEqual(['7/0', '7/1']);
    expect([...store.entries.keys()]).toEqual(['fp-a-7', 'fp-b-7']);
    expect(first.progress).toEqual([
      [0, 2],
      [1, 2],
      [2, 2],
    ]);
    // A later page (nothing in memory): from the store, nothing built.
    forgetWorldMaps();
    const later = deps(store);
    expect(await worldMaps(7n, GENERATOR_TERRAIN, later)).toEqual(maps);
    expect(later.builds).toEqual([]);
  });

  it('shares one promise between the local world and the client on a page', async () => {
    const d = deps(null);
    const a = worldMaps(3n, GENERATOR_TERRAIN, d);
    const b = worldMaps(3n, GENERATOR_TERRAIN, d);
    expect(a).toBe(b);
    await a;
    expect(d.builds).toEqual(['3/0', '3/1']);
  });

  it('has no maps for a world without terrain', () => {
    expect(worldMaps(3n, 0, deps(null))).toBeNull();
    expect(worldMaps(3n, 100, deps(null))).toBeNull();
  });

  it('builds when the store is blocked, and still answers', async () => {
    const store = new MemoryStore();
    store.failing = true;
    const d = deps(store);
    expect(await worldMaps(9n, GENERATOR_TERRAIN, d)).toEqual([
      new Uint8Array([9, 0]),
      new Uint8Array([9, 1]),
    ]);
    expect(d.builds).toEqual(['9/0', '9/1']);
  });

  it('a failed build is not kept: the next join tries again', async () => {
    const d = deps(null);
    let fail = true;
    const build = d.build;
    d.build = (seed, face) => (fail ? Promise.reject(new Error('no worker')) : build(seed, face));
    await expect(worldMaps(5n, GENERATOR_TERRAIN, d)).rejects.toThrow('no worker');
    fail = false;
    expect(await worldMaps(5n, GENERATOR_TERRAIN, d)).toHaveLength(2);
  });

  it('drops the least recently used maps past the limit', () => {
    const entries = [
      { k: 'a', used: 30 },
      { k: 'b', used: 10 },
      { k: 'c', used: 20 },
    ];
    expect(oldest(entries, 2).map((e) => e.k)).toEqual(['b', 'c']);
    expect(oldest(entries, 0)).toEqual([]);
  });
});
