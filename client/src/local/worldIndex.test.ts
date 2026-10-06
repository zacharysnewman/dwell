import { describe, expect, it } from 'vitest';
import { recordedVersion } from '../buildInfo';
import { GENERATORS } from './world';
import {
  cleanName,
  hashSeed,
  INDEX_KEY,
  newWorldId,
  parseSeed,
  typeOfGenerator,
  WorldIndex,
  type KeyValueStore,
} from './worldIndex';

function memoryStore(initial: Record<string, string> = {}): KeyValueStore & {
  data: Record<string, string>;
} {
  const data = { ...initial };
  return {
    data,
    getItem: (k) => data[k] ?? null,
    setItem: (k, v) => {
      data[k] = v;
    },
  };
}

/** A repeatable stand-in for Math.random. */
function sequence(...values: number[]): () => number {
  let i = 0;
  return () => values[i++ % values.length] ?? 0;
}

describe('world seeds', () => {
  it('uses whole numbers as they are', () => {
    expect(parseSeed('42')).toBe(42);
    expect(parseSeed(' 0 ')).toBe(0);
    expect(parseSeed(String(Number.MAX_SAFE_INTEGER))).toBe(Number.MAX_SAFE_INTEGER);
  });

  it('hashes text, and numbers too large to carry exactly', () => {
    expect(parseSeed('mountains')).toBe(hashSeed('mountains'));
    expect(parseSeed('mountains')).not.toBe(parseSeed('Mountains'));
    expect(parseSeed('-3')).toBe(hashSeed('-3'));
    expect(parseSeed('99999999999999999999')).toBe(hashSeed('99999999999999999999'));
  });

  it('picks a random 32-bit seed when blank', () => {
    expect(parseSeed('', () => 0.5)).toBe(2 ** 31);
    expect(parseSeed('  ', () => 0)).toBe(0);
  });

  it('hashes with FNV-1a', () => {
    expect(hashSeed('')).toBe(0x811c9dc5);
    expect(hashSeed('a')).toBe(0xe40c292c);
  });
});

describe('world index', () => {
  it('creates worlds with the type’s current generator, newest played first', () => {
    const index = new WorldIndex(memoryStore());
    const a = index.create({ name: 'Alpha', type: 'terrain', seed: 1 }, 1000, sequence(0.1));
    const b = index.create({ name: 'Beta', type: 'flat', seed: 2 }, 2000, sequence(0.2));
    expect(a.generatorVersion).toBe(GENERATORS.terrain);
    expect(b.generatorVersion).toBe(GENERATORS.flat);
    expect(a.id).toMatch(/^w[0-9a-z]{10}$/);
    expect(index.list().map((w) => w.name)).toEqual(['Beta', 'Alpha']);
    index.touch(a.id, 3000);
    expect(index.list().map((w) => w.name)).toEqual(['Alpha', 'Beta']);
  });

  it('never reuses an id', () => {
    const index = new WorldIndex(memoryStore());
    const first = index.create({ name: 'A', type: 'terrain', seed: 1 }, 1, sequence(0.5));
    const second = index.create(
      { name: 'B', type: 'terrain', seed: 1 },
      2,
      sequence(0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.9),
    );
    expect(second.id).not.toBe(first.id);
  });

  it('removes worlds and survives a new instance on the same storage', () => {
    const store = memoryStore();
    const index = new WorldIndex(store);
    const a = index.create({ name: 'A', type: 'terrain', seed: 1 }, 1);
    const b = index.create({ name: 'B', type: 'playground', seed: 2 }, 2);
    index.remove(a.id);
    expect(new WorldIndex(store).list()).toEqual([b]);
  });

  it('adopts worlds saved per generator and seed, once', () => {
    const index = new WorldIndex(memoryStore());
    const w = index.adopt('local-g1-s42', 5, '0.1.0');
    expect(w).toMatchObject({
      id: 'local-g1-s42',
      type: 'playground',
      seed: 42,
      generatorVersion: 1,
    });
    expect(w?.name).toBe('Playground world 42');
    expect(index.adopt('local-g1-s42', 9)).toEqual(w);
    expect(index.list()).toHaveLength(1);
    // A retired terrain version is still a terrain world, keeping its own version.
    expect(index.adopt('local-g3-s0', 5, '0.1.0')).toMatchObject({
      type: 'terrain',
      generatorVersion: 3,
    });
    expect(index.adopt('w0123456789', 5)).toBeNull();
    expect(index.adopt('local-g4-s1e9', 5)).toBeNull();
  });

  it('drops damaged entries and tolerates damaged or missing storage', () => {
    const good = {
      id: 'wabc',
      name: 'Good',
      type: 'flat',
      seed: 3,
      generatorVersion: 0,
      createdAt: 1,
      lastPlayedAt: 1,
      appVersion: '0.1.0',
    };
    const bad = [{ ...good, id: '../x' }, { ...good, type: 'moon' }, { ...good, seed: -1 }, 7];
    const index = new WorldIndex(memoryStore({ [INDEX_KEY]: JSON.stringify([good, ...bad]) }));
    expect(index.list()).toEqual([good]);
    expect(new WorldIndex(memoryStore({ [INDEX_KEY]: '{not json' })).list()).toEqual([]);
    const none = new WorldIndex(null);
    expect(none.list()).toEqual([]);
    expect(none.create({ name: 'X', type: 'flat', seed: 1 }, 1).name).toBe('X');
  });

  it('records the app version of the build that created and last played a world', () => {
    const index = new WorldIndex(memoryStore());
    const w = index.create({ name: 'A', type: 'terrain', seed: 1 }, 1);
    expect(w.appVersion).toBe(recordedVersion());
    index.put({ ...w, appVersion: '0.0.9' });
    index.touch(w.id, 5);
    expect(index.get(w.id)?.appVersion).toBe(recordedVersion());
  });

  it('sets worlds saved before versioned releases apart, to be deleted but never opened', () => {
    const old = {
      id: 'wold',
      name: 'Old',
      type: 'terrain',
      seed: 1,
      generatorVersion: 4,
      createdAt: 1,
      lastPlayedAt: 1,
    };
    const store = memoryStore({ [INDEX_KEY]: JSON.stringify([old]) });
    const index = new WorldIndex(store);
    expect(index.list()).toEqual([]);
    expect(index.legacy()).toEqual([old]);
    // A file found without an entry (adopted by the menu) is the same.
    expect(index.adopt('local-g4-s7', 2)?.appVersion).toBeUndefined();
    expect(
      index
        .legacy()
        .map((w) => w.id)
        .sort(),
    ).toEqual(['local-g4-s7', 'wold']);
    index.remove('wold');
    expect(index.legacy().map((w) => w.id)).toEqual(['local-g4-s7']);
  });

  // The index is shared by every app version (RELEASES.md §6): a build rewriting it must not drop
  // what a newer build stored.
  describe('cross-version storage contract', () => {
    const record = {
      id: 'wabc',
      name: 'Future',
      type: 'flat',
      seed: 3,
      generatorVersion: 0,
      createdAt: 1,
      lastPlayedAt: 1,
      appVersion: '0.1.4',
    };

    it('keeps fields of a record that this build does not know', () => {
      const store = memoryStore({
        [INDEX_KEY]: JSON.stringify([{ ...record, sky: { islands: 3 }, favourite: true }]),
      });
      const index = new WorldIndex(store);
      index.touch('wabc', 99);
      const world = index.get('wabc');
      if (!world) throw new Error('missing world');
      index.put({ ...world, name: 'Renamed' });
      const [stored] = JSON.parse(store.data[INDEX_KEY] ?? '[]') as Record<string, unknown>[];
      expect(stored).toMatchObject({ sky: { islands: 3 }, favourite: true, name: 'Renamed' });
    });

    it('keeps records it cannot read when it rewrites the index', () => {
      const unreadable = { ...record, id: 'wnew', type: 'sky-island' };
      const notAnObject = 'future format';
      const store = memoryStore({
        [INDEX_KEY]: JSON.stringify([record, unreadable, notAnObject]),
      });
      const index = new WorldIndex(store);
      expect(index.list().map((w) => w.id)).toEqual(['wabc']);
      index.create({ name: 'New', type: 'flat', seed: 1 }, 5);
      index.remove('wabc');
      const stored = JSON.parse(store.data[INDEX_KEY] ?? '[]') as unknown[];
      expect(stored).toContainEqual(unreadable);
      expect(stored).toContain(notAnObject);
    });
  });

  it('cleans names', () => {
    expect(cleanName('  My   world ', 'New world')).toBe('My world');
    expect(cleanName('   ', 'New world')).toBe('New world');
    expect(cleanName('x'.repeat(100), 'New world')).toHaveLength(40);
  });

  it('maps generator versions to types', () => {
    expect(typeOfGenerator(GENERATORS.flat)).toBe('flat');
    expect(typeOfGenerator(GENERATORS.playground)).toBe('playground');
    expect(typeOfGenerator(GENERATORS.terrain)).toBe('terrain');
    expect(typeOfGenerator(2)).toBe('terrain');
  });

  it('makes ids from the random source', () => {
    expect(newWorldId(sequence(0))).toBe('w0000000000');
  });
});
