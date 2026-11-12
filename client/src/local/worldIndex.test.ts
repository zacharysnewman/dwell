import { describe, expect, it } from 'vitest';
import { recordedVersion } from '../buildInfo';
import { GENERATORS } from './world';
import {
  cleanName,
  creatableType,
  hashSeed,
  INDEX_KEY,
  newWorldId,
  parseSeed,
  testWorldId,
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

describe('test worlds (docs/DEV_TOOLS.md "Play a test world")', () => {
  it("a ?worldgen= link's world has its own id and record, which keeps the config", () => {
    const store = memoryStore();
    expect(testWorldId('test:tall_peak', 7)).toBe('test-test-tall-peak-s7');
    const w = new WorldIndex(store).adoptTest('test:tall_peak', 7, 1000);
    expect(w.id).toBe(testWorldId('test:tall_peak', 7));
    expect(w).toMatchObject({ type: 'terrain', seed: 7, worldgen: 'test:tall_peak' });
    expect(w.generatorVersion).toBe(GENERATORS.terrain);
    // Reopened (a later page, then played, copied): the same config.
    const again = new WorldIndex(store);
    expect(again.adoptTest('test:tall_peak', 7, 2000)).toEqual(w);
    again.touch(w.id, 3000);
    expect(again.get(w.id)?.worldgen).toBe('test:tall_peak');
    expect(again.copy(w, 4000, sequence(0.5)).worldgen).toBe('test:tall_peak');
  });

  it("the New world form's worlds have no worldgen config (the build's terrain)", () => {
    const index = new WorldIndex(memoryStore());
    const w = index.create({ name: 'Plain', type: 'terrain', seed: 7 }, 1000, sequence(0.1));
    expect('worldgen' in w).toBe(false);
    expect('worldgen' in (index.adopt('local-g8-s7', 1000) ?? {})).toBe(false);
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

  it('records the host’s mode switch, keeping fields it doesn’t know', () => {
    const store = memoryStore();
    const index = new WorldIndex(store);
    const w = index.create({ name: 'M', type: 'terrain', seed: 1, mode: 'adventure' }, 1);
    const raw = JSON.parse(store.data[INDEX_KEY] ?? '[]') as Record<string, unknown>[];
    store.setItem(INDEX_KEY, JSON.stringify([{ ...raw[0], future: 42 }]));
    new WorldIndex(store).setMode(w.id, 'creative');
    expect(new WorldIndex(store).get(w.id)?.mode).toBe('creative');
    const [stored] = JSON.parse(store.data[INDEX_KEY] ?? '[]') as Record<string, unknown>[];
    expect(stored?.future).toBe(42);
  });

  it('keeps a bigger terrain world’s size, and none for today’s (BIG_MAP.md M2b)', () => {
    const store = memoryStore();
    const index = new WorldIndex(store);
    const big = index.create({ name: 'B', type: 'terrain', seed: 1, radiusScale: 2 }, 1);
    const plain = index.create({ name: 'A', type: 'terrain', seed: 1, radiusScale: 1 }, 2);
    const flat = index.create({ name: 'F', type: 'flat', seed: 1, radiusScale: 2 }, 3);
    const odd = index.create({ name: 'O', type: 'terrain', seed: 1, radiusScale: 3 }, 4);
    const again = new WorldIndex(store);
    expect(again.get(big.id)?.radiusScale).toBe(2);
    // Today's size, a flat world and a size the data doesn't name record nothing (as before).
    for (const w of [plain, flat, odd])
      expect('radiusScale' in (again.get(w.id) ?? {})).toBe(false);
    // A copy is the same size.
    expect(again.copy(big, 5).radiusScale).toBe(2);
  });

  it('keeps a new world’s chosen spawn, and none for the usual one', () => {
    const store = memoryStore();
    const index = new WorldIndex(store);
    const portal = index.create({ name: 'P', type: 'terrain', seed: 1, spawn: 'portal' }, 1);
    const plain = index.create({ name: 'A', type: 'terrain', seed: 1 }, 2);
    expect(new WorldIndex(store).get(portal.id)?.spawn).toBe('portal');
    expect(new WorldIndex(store).get(plain.id)).not.toHaveProperty('spawn');
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
    const b = index.create({ name: 'B', type: 'flat', seed: 2 }, 2);
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

  it('copies a world for another version: a new id and name, the same seed and spawn', () => {
    // Owner (2026-11-06): an older line's world can be played as a copy in a newer build; the
    // copy's file is the original's, so it records the original's version until it is played.
    const index = new WorldIndex(memoryStore());
    const original = index.create({ name: 'Hills', type: 'terrain', seed: 7, spawn: 'b' }, 100);
    index.put({ ...original, appVersion: '0.47.3', generatorVersion: 21 });
    const copy = index.copy(index.get(original.id) ?? original, 200, sequence(0.5, 0.25));
    expect(copy.id).not.toBe(original.id);
    expect(copy.name).toBe('Hills (from 0.47.3)');
    expect(copy).toMatchObject({ seed: 7, spawn: 'b', type: 'terrain', appVersion: '0.47.3' });
    expect(copy.createdAt).toBe(200);
    expect(index.get(original.id)?.appVersion).toBe('0.47.3');
    expect(index.list().map((w) => w.id)).toContain(copy.id);
  });

  it("records this build's generator of the world's type when it plays one", () => {
    // A world from an older line opens on this build's terrain (dwell_world::current_generator).
    const index = new WorldIndex(memoryStore());
    const w = index.create({ name: 'Old', type: 'terrain', seed: 1 }, 1);
    index.put({ ...w, generatorVersion: 21, appVersion: '0.47.3' });
    index.touch(w.id, 2);
    expect(index.get(w.id)).toMatchObject({
      generatorVersion: GENERATORS.terrain,
      appVersion: recordedVersion(),
    });
  });

  it('sets worlds saved before versioned releases apart, to be deleted but never opened', () => {
    const old = {
      id: 'wold',
      name: 'Old',
      type: 'terrain',
      seed: 1,
      generatorVersion: 5,
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

    it('creates sandbox worlds and keeps them through a rewrite', () => {
      const store = memoryStore({ [INDEX_KEY]: JSON.stringify([record]) });
      const index = new WorldIndex(store);
      const sandbox = index.create({ name: 'Trees', type: 'sandbox', seed: 3 }, 7);
      expect(sandbox).toMatchObject({ type: 'sandbox', generatorVersion: GENERATORS.sandbox });
      index.touch('wabc', 9);
      expect(index.get(sandbox.id)).toMatchObject({ type: 'sandbox', name: 'Trees' });
      // A link-opened sandbox file (`local-g<generator>-s<seed>`) is adopted as a sandbox.
      expect(index.adopt(`local-g${String(GENERATORS.sandbox)}-s5`, 1)?.type).toBe('sandbox');
    });

    it('leaves a sandbox record as stored when it rewrites another', () => {
      // Builds from before the sandbox cannot read it; they keep it (the test below), as this one does.
      const sandbox = { ...record, id: 'wsand', type: 'sandbox', generatorVersion: 100, extra: 1 };
      const store = memoryStore({ [INDEX_KEY]: JSON.stringify([sandbox, record]) });
      const index = new WorldIndex(store);
      index.touch('wabc', 99);
      expect(JSON.parse(store.data[INDEX_KEY] ?? '[]')).toContainEqual(sandbox);
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
    expect(typeOfGenerator(1)).toBe('playground'); // retired: old worlds only
    expect(typeOfGenerator(GENERATORS.terrain)).toBe('terrain');
    expect(typeOfGenerator(2)).toBe('terrain');
    expect(typeOfGenerator(GENERATORS.sandbox)).toBe('sandbox');
  });

  it('makes the type a new world names, terrain for one it does not know', () => {
    expect(creatableType('sandbox')).toBe('sandbox');
    expect(creatableType('flat')).toBe('flat');
    expect(creatableType('terrain')).toBe('terrain');
    expect(creatableType('playground')).toBe('terrain'); // retired
    expect(creatableType('')).toBe('terrain');
  });

  it('makes ids from the random source', () => {
    expect(newWorldId(sequence(0))).toBe('w0000000000');
  });
});
