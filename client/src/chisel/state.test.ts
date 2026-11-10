import { describe, expect, it } from 'vitest';
import {
  ARCHIPELAGO_FIXTURE,
  ARCHIPELAGO_WORLD,
  DESERT_FIXTURE,
  STORE_KEY,
  addBiome,
  buildEdit,
  fromJson,
  knob,
  loadEdit,
  saveEdit,
  toJson,
  validateEdit,
} from './state';

function memory(init: Record<string, string> = {}) {
  const m = new Map(Object.entries(init));
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    m,
  };
}

describe('Chisel state', () => {
  it('the archipelago (world knobs, a water hue, banks) round-trips and validates', () => {
    const e = {
      ...addBiome(buildEdit(), ARCHIPELAGO_FIXTURE),
      world: structuredClone(ARCHIPELAGO_WORLD),
    };
    expect(validateEdit(e)).toEqual([]);
    expect(knob(e, 'islands', 'amplitude')).toBe(1400);
    expect(knob(buildEdit(), 'islands', 'amplitude')).toBe(0);
    const back = fromJson(toJson(e));
    expect(back).toEqual({ edit: e });
    const bad = structuredClone(e);
    bad.world = { ...bad.world, islands: { amplitude: -1 } };
    expect(validateEdit(bad).join()).toMatch(/islands\.amplitude is out of range/);
    const hue = structuredClone(e);
    hue.biomes[1] = {
      ...ARCHIPELAGO_FIXTURE,
      name: 'test:archipelago',
      waterLook: { scatter: 'teal' },
    };
    expect(validateEdit(hue).join()).toMatch(/waterLook\.scatter must be a colour/);
    const unknown = structuredClone(e);
    (unknown.world as Record<string, unknown>).rivers = { bank: 1 };
    expect(validateEdit(unknown).join()).toMatch(/rivers is not a world knob/);
  });

  it('the build edit is valid and round-trips through its JSON', () => {
    const e = addBiome(buildEdit(), DESERT_FIXTURE);
    expect(validateEdit(e)).toEqual([]);
    const back = fromJson(toJson(e));
    expect(back).toEqual({ edit: e });
  });

  it('shows the validation rules’ messages (biomes.mjs, as gen.mjs)', () => {
    const e = addBiome(buildEdit(), DESERT_FIXTURE);
    const [, d] = e.biomes;
    if (!d) throw new Error('two biomes');
    (d.detail as Record<string, number>).octaves = 2.5;
    const errs = validateEdit(e);
    expect(errs).toEqual(['biome test:desert: detail.octaves must be a whole number in [1, 8]']);
    const r = fromJson(toJson({ ...e, biomeMap: { ...e.biomeMap, dither: 3 } }));
    expect('errors' in r && r.errors).toContain('terrain.json: biomeMap.dither must be in [0, 1]');
    expect(fromJson('{')).toMatchObject({ errors: [expect.stringMatching(/^not JSON/)] });
  });

  it('a band block that is not full is refused', () => {
    const e = buildEdit();
    const [band] = e.biomes[0]?.bands ?? [];
    if (!band) throw new Error('a band');
    band.ground = 'dwell:water';
    expect(validateEdit(e)[0]).toMatch(/must be a full block/);
  });

  it('imports one biome file into the edit', () => {
    const r = fromJson(JSON.stringify(DESERT_FIXTURE));
    expect('edit' in r && r.edit.biomes.map((b) => b.name)).toEqual([
      'dwell:default',
      'test:desert',
    ]);
  });

  it('a duplicated biome takes a fresh name', () => {
    const e = addBiome(addBiome(buildEdit(), DESERT_FIXTURE), DESERT_FIXTURE);
    expect(e.biomes.map((b) => b.name)).toEqual(['dwell:default', 'test:desert', 'test:desert_2']);
    expect(validateEdit(e)).toEqual([]);
  });

  it('the stored record keeps fields it does not know, and Reset drops only the edit', () => {
    const s = memory({ [STORE_KEY]: JSON.stringify({ later: { x: 1 } }) });
    const e = addBiome(buildEdit(), DESERT_FIXTURE);
    saveEdit(s, e);
    const stored = () => JSON.parse(s.m.get(STORE_KEY) ?? '') as unknown;
    expect(stored()).toEqual({ later: { x: 1 }, edit: e });
    expect(loadEdit(s)).toEqual(e);
    saveEdit(s, null);
    expect(stored()).toEqual({ later: { x: 1 } });
    expect(loadEdit(s)).toEqual(buildEdit());
    expect(loadEdit(memory({ [STORE_KEY]: 'nonsense' }))).toEqual(buildEdit());
  });
});
