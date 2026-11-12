import { beforeEach, describe, expect, it } from 'vitest';
import {
  forgetWorldMaps,
  GENERATOR_TERRAIN,
  type MapDeps,
  type MapStore,
  oldest,
  worldMaps,
  workerBuilder,
} from './worldMaps';
import { mapProgress } from '../ui/mapProgress';

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
    keys: (seed, worldgen, scale) => {
      const x = scale === 1 ? '' : `-x${String(scale)}`;
      return [`fp-a-${seed.toString()}${worldgen}${x}`, `fp-b-${seed.toString()}${worldgen}${x}`];
    },
    build: (seed, face, worldgen, scale) => {
      builds.push(
        `${seed.toString()}/${String(face)}${worldgen}${scale === 1 ? '' : `x${String(scale)}`}`,
      );
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

  it("reports each face's build steps in order, the count reaching both faces (BUGS.md #91)", async () => {
    const seen: { done: number; detail: string | null }[] = [];
    const d: MapDeps = {
      keys: (seed) => [`a-${seed.toString()}`, `b-${seed.toString()}`],
      build: async (_seed, face, _w, _s, onStep) => {
        // Face B's build trails face A's by a step.
        for (let step = 0; step < 6; step++) {
          await Promise.resolve();
          if (face === 1) await Promise.resolve();
          onStep?.(step, 0);
          onStep?.(step, 0.5);
          onStep?.(step, 1);
        }
        return new Uint8Array([face]);
      },
      store: null,
      onProgress: (ready, needed, faces) => {
        const p = mapProgress(faces);
        expect(needed).toBe(2);
        seen.push({ done: Math.max(ready, p.done), detail: p.detail });
      },
    };
    await worldMaps(5n, GENERATOR_TERRAIN, d);
    const dones = seen.map((s) => s.done);
    // Never backward, and to 100 %: both faces.
    for (let i = 1; i < dones.length; i++)
      expect(dones[i]).toBeGreaterThanOrEqual(dones[i - 1] ?? 0);
    expect(dones[0]).toBe(0);
    expect(dones.at(-1)).toBe(2);
    // Fractions between whole faces: the bar moves inside a face's build.
    expect(dones.some((v) => v > 0 && v < 1)).toBe(true);
    expect(seen.map((s) => s.detail)).toContain('rivers (5/6), face 2 of 2');
    expect(seen.at(-1)?.detail).toBeNull();
  });

  it('says when a face cannot be built, rather than waiting at its count', async () => {
    const failed: string[] = [];
    const d: MapDeps = {
      keys: () => ['a', 'b'],
      build: () => Promise.reject(new Error('out of memory')),
      store: null,
      onFailed: (m) => failed.push(m),
    };
    await expect(worldMaps(6n, GENERATOR_TERRAIN, d)).rejects.toThrow('out of memory');
    expect(failed).toContain('out of memory');
  });

  it("passes a map worker's step messages on before its map", async () => {
    const posted: unknown[] = [];
    const fake = {
      onmessage: null as ((e: MessageEvent) => void) | null,
      onerror: null,
      terminate: () => posted.push('terminated'),
      postMessage: (m: unknown) => {
        posted.push(m);
        queueMicrotask(() => {
          fake.onmessage?.({ data: { step: 4, fraction: 0.25 } } as MessageEvent);
          fake.onmessage?.({ data: { map: new Uint8Array([9]) } } as MessageEvent);
        });
      },
    };
    const steps: [number, number][] = [];
    const build = workerBuilder(() => fake as unknown as Worker);
    const map = await build(1n, 0, '', 1, (s, f) => steps.push([s, f]));
    expect(steps).toEqual([[4, 0.25]]);
    expect(map).toEqual(new Uint8Array([9]));
    expect(posted.at(-1)).toBe('terminated');
  });

  it('shares one promise between the local world and the client on a page', async () => {
    const d = deps(null);
    const a = worldMaps(3n, GENERATOR_TERRAIN, d);
    const b = worldMaps(3n, GENERATOR_TERRAIN, d);
    expect(a).toBe(b);
    await a;
    expect(d.builds).toEqual(['3/0', '3/1']);
  });

  it("keeps a test world's maps (its worldgen config) apart from its seed's own", async () => {
    const store = new MemoryStore();
    const d = deps(store);
    await worldMaps(7n, GENERATOR_TERRAIN, d);
    await worldMaps(7n, GENERATOR_TERRAIN, d, 'test:tall_peak');
    expect(d.builds).toEqual(['7/0', '7/1', '7/0test:tall_peak', '7/1test:tall_peak']);
    expect([...store.entries.keys()]).toEqual([
      'fp-a-7',
      'fp-b-7',
      'fp-a-7test:tall_peak',
      'fp-b-7test:tall_peak',
    ]);
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
    const build = d.build.bind(d);
    d.build = (seed, face, worldgen, scale) =>
      fail ? Promise.reject(new Error('no worker')) : build(seed, face, worldgen, scale);
    await expect(worldMaps(5n, GENERATOR_TERRAIN, d)).rejects.toThrow('no worker');
    fail = false;
    expect(await worldMaps(5n, GENERATOR_TERRAIN, d)).toHaveLength(2);
  });

  it("builds and keeps a bigger world's maps apart from the seed's 1× maps (BIG_MAP.md M2b)", async () => {
    const store = new MemoryStore();
    const d = deps(store);
    await worldMaps(4n, GENERATOR_TERRAIN, d);
    // A 2× world of the same seed: its own keys and builds, never the 1× maps.
    expect(await worldMaps(4n, GENERATOR_TERRAIN, d, '', 2)).toHaveLength(2);
    expect(d.builds).toEqual(['4/0', '4/1', '4/0x2', '4/1x2']);
    expect([...store.entries.keys()].sort()).toEqual([
      'fp-a-4',
      'fp-a-4-x2',
      'fp-b-4',
      'fp-b-4-x2',
    ]);
    // Asked again (the client's join at the same size): the same maps, nothing built.
    await worldMaps(4n, GENERATOR_TERRAIN, d, '', 2);
    // Today's size by its scale or by default is one and the same; a bad scale is today's.
    await worldMaps(4n, GENERATOR_TERRAIN, d, '', 1);
    await worldMaps(4n, GENERATOR_TERRAIN, d, '', 0);
    expect(d.builds).toHaveLength(4);
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
