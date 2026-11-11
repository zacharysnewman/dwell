import { describe, expect, it } from 'vitest';
import {
  ARCHIPELAGO_FIXTURE,
  ARCHIPELAGO_WORLD,
  BUILD_GEOLOGY,
  BUILD_VEGETATION,
  DESERT_FIXTURE,
  PRESETS,
  STORE_KEY,
  addBiome,
  buildEdit,
  fromJson,
  knob,
  loadEdit,
  saveEdit,
  setTreeDensity,
  treeDensity,
  playInGame,
  TALL_PEAK_WORLD,
  toJson,
  validateEdit,
} from './state';
import { WorldIndex } from '../local/worldIndex';

describe("Chisel's Play in game", () => {
  it("records the edit as the new world's worldgen config, which reopens unchanged", () => {
    const store = memory();
    const e = { ...buildEdit(), world: structuredClone(TALL_PEAK_WORLD) };
    const id = playInGame(new WorldIndex(store), e, 42, Date.UTC(2026, 10, 10, 18, 0));
    // A later page reads the record back: the same seed, a terrain world, the edit's JSON.
    const world = new WorldIndex(store).get(id);
    expect(world?.type).toBe('terrain');
    expect(world?.seed).toBe(42);
    expect(world?.name).toBe('Chisel 2026-11-10 18:00');
    const config = JSON.parse(world?.worldgen ?? '{}') as { world: typeof TALL_PEAK_WORLD };
    expect(config).toEqual(JSON.parse(toJson(e)));
    expect(config.world).toEqual(TALL_PEAK_WORLD);
    // Played again (touch): the config stays.
    new WorldIndex(store).touch(id, Date.UTC(2026, 10, 11));
    expect(new WorldIndex(store).get(id)?.worldgen).toBe(world?.worldgen);
  });
});

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

  it('round-trips the rain shadow’s wind and loads an older edit without its keys', () => {
    const e = buildEdit();
    expect(e.biomeMap.rainShare).toBe(0);
    const windy = { ...e, biomeMap: { ...e.biomeMap, windDirection: 90, rainShare: 0.8 } };
    expect(fromJson(toJson(windy))).toEqual({ edit: windy });
    // An edit saved before the rain shadow: the new keys come back at terrain.json's values.
    const rainKeys = [
      'windDirection',
      'windReach',
      'baseRain',
      'upliftRain',
      'leeDry',
      'rainShare',
      'windSmoothM',
    ];
    const old = Object.fromEntries(
      Object.entries(e.biomeMap).filter(([k]) => !rainKeys.includes(k)),
    );
    expect(fromJson(JSON.stringify({ biomes: e.biomes, biomeMap: old }))).toEqual({ edit: e });
    const store = memory({
      [STORE_KEY]: JSON.stringify({ edit: { biomes: e.biomes, biomeMap: old } }),
    });
    expect(loadEdit(store)).toEqual(e);
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

  it('lists the preset biomes by label, each valid and addable by data alone', () => {
    expect(PRESETS.length).toBeGreaterThanOrEqual(12);
    const labels = PRESETS.map((p) => p.label);
    expect(labels).toEqual([...labels].sort((a, b) => a.localeCompare(b)));
    expect(labels).toContain('Sand desert');
    expect(labels).toContain('Alpine tundra');
    for (const p of PRESETS) expect(p.description.length).toBeGreaterThan(0);
    let e = buildEdit();
    for (const p of PRESETS) e = addBiome(e, p);
    expect(e.biomes.length).toBe(PRESETS.length + 1);
    expect(validateEdit(e)).toEqual([]);
    expect(fromJson(toJson(e))).toEqual({ edit: e });
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

  it('the red-rock mesa with strong rock shaping round-trips, and a bad rock is named', () => {
    const mesa = PRESETS.find((p) => p.name === 'preset:red_rock_mesa');
    expect(mesa?.rockStack).toBe('dwell:red_beds');
    if (!mesa) throw new Error('the mesa preset');
    const base = addBiome(buildEdit(), mesa);
    const geology = structuredClone(BUILD_GEOLOGY);
    geology.shaping.strength = 0.9;
    geology.provinces.wavelength = 4000;
    const e = { ...base, geology };
    expect(validateEdit(e)).toEqual([]);
    expect(fromJson(toJson(e))).toEqual({ edit: e });
    // An edit without its geology (an older Chisel's) still loads: the build's is used.
    const old = { ...e };
    delete (old as { geology?: unknown }).geology;
    expect(fromJson(JSON.stringify(old))).toEqual({ edit: old });
    const bad = structuredClone(geology);
    bad.rocks[0] = { ...(bad.rocks[0] ?? { name: '?' }), jointing: 'wobbly' };
    expect(validateEdit({ ...e, geology: bad }).join()).toContain('jointing must be one of');
    const lost = structuredClone(geology);
    lost.stacks = lost.stacks.filter((s) => s.name !== 'dwell:red_beds');
    expect(validateEdit({ ...e, geology: lost }).join()).toContain('rockStack');
  });
  it('the boreal and desert presets opt in to the climate, a tuned tree line round-trips, a bad kind is named', () => {
    for (const n of ['boreal', 'cold_rainforest', 'savanna', 'sand_desert', 'alpine_tundra']) {
      expect(PRESETS.find((p) => p.name === `preset:${n}`)?.vegetation).toBe(1);
    }
    const boreal = PRESETS.find((p) => p.name === 'preset:boreal');
    if (!boreal) throw new Error('the boreal preset');
    const vegetation = structuredClone(BUILD_VEGETATION);
    const kind = vegetation.kinds[0];
    if (!kind) throw new Error('a kind');
    kind.temperature[0] = 9;
    kind.moisture[0] = 0.1;
    const e = { ...addBiome(buildEdit(), boreal), vegetation };
    expect(validateEdit(e)).toEqual([]);
    expect(fromJson(toJson(e))).toEqual({ edit: e });
    // An edit without its vegetation (an older Chisel's) still loads: the build's is used.
    const old = { ...e };
    delete (old as { vegetation?: unknown }).vegetation;
    expect(fromJson(JSON.stringify(old))).toEqual({ edit: old });
    const bad = structuredClone(vegetation);
    if (bad.kinds[0]) bad.kinds[0].placement = 'no_such_rule';
    expect(validateEdit({ ...e, vegetation: bad }).join()).toContain('no_such_rule');
    const upside = structuredClone(vegetation);
    if (upside.kinds[0]) upside.kinds[0].temperature = [20, 10];
    expect(validateEdit({ ...e, vegetation: upside }).join()).toContain('least below most');
  });
});

describe("a biome's trees and structures (ADR 0074 B5)", () => {
  it('sets a density per placement, round-trips through JSON and validates', () => {
    const e = buildEdit();
    const b = e.biomes[0];
    if (!b) throw new Error('the default biome');
    expect(treeDensity(b, 'pine_groves')).toBe(1);
    expect(treeDensity(b, 'boulders')).toBe(0);
    setTreeDensity(b, 'boulders', 0.5);
    setTreeDensity(b, 'pine_groves', 2);
    expect(b.placements).toEqual(['pine_groves', 'boulders']);
    expect(b.trees).toEqual({ pine_groves: 2, boulders: 0.5 });
    expect(validateEdit(e)).toEqual([]);
    expect(fromJson(toJson(e))).toEqual({ edit: e });
    // Back to the default's form: listed at 1 with no entry, unlisted at 0.
    setTreeDensity(b, 'pine_groves', 1);
    setTreeDensity(b, 'boulders', 0);
    expect(b.placements).toEqual(['pine_groves']);
    expect(b.trees).toBeUndefined();
    // A density for a rule it does not list, or out of range, fails with a message.
    b.trees = { boulders: 1 };
    expect(validateEdit(e).join()).toContain('not one of its placements');
    b.trees = { pine_groves: 9 };
    expect(validateEdit(e).join()).toContain('trees.pine_groves must be in [0, 3]');
  });

  it('the presets pick their trees', () => {
    const p = (n: string) => {
      const b = PRESETS.find((x) => x.name === `preset:${n}`);
      if (!b) throw new Error(n);
      return b;
    };
    expect(treeDensity(p('sand_desert'), 'pine_groves')).toBe(0);
    expect(treeDensity(p('sand_desert'), 'boulders')).toBeGreaterThan(0);
    expect(treeDensity(p('boreal'), 'pine_groves')).toBeGreaterThan(1);
  });
});
