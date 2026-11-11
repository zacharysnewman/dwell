// Chisel's state (ADR 0074 B3): the world's biomes (the default first) and the biome map's
// geography, as the JSON of shared/worldgen/biomes/*.json and terrain.json's `biomeMap`; checked
// by shared/worldgen/biomes.mjs, the same rules gen.mjs applies; kept in the browser as an
// append-only record (unknown fields kept on rewrite).
import {
  validateBiome,
  validateBiomeMap,
  validatePresets,
  validateWorldKnobs,
  validateRocks,
  validateVegetation,
  type BlockInfo,
} from '../../../shared/worldgen/biomes.mjs';
import type { WorldIndex } from '../local/worldIndex';
import defaultBiome from '../../../shared/worldgen/biomes/default.json';
import desert from '../../../shared/worldgen/biomes/fixtures/desert.json';
import archipelago from '../../../shared/worldgen/biomes/fixtures/archipelago.json';
import archipelagoWorld from '../../../shared/worldgen/worlds/archipelago.json';
import tallPeakWorld from '../../../shared/worldgen/worlds/tall_peak.json';
import terrain from '../../../shared/worldgen/terrain.json';
import rocks from '../../../shared/worldgen/rocks.json';
import vegetation from '../../../shared/worldgen/vegetation.json';
import decorations from '../../../shared/worldgen/decorations.json';
import placements from '../../../shared/worldgen/placements.json';
import blocksDwell from '../../../shared/blocks/dwell.json';

/** A biome as its file holds it. */
export type BiomeJson = { name: string; bands: BandJson[]; [k: string]: unknown };
export interface BandJson {
  name: string;
  from?: number;
  ground: string;
  under: string;
  rock: string;
  shoreBlock: string;
}
export type BiomeMapJson = Record<string, number | string>;
/** World knobs (terrain.json's groups, `WORLD_KNOBS`' keys): the archipelago's (ADR 0074 B4). */
export type WorldJson = Record<string, Record<string, number>>;

/** The geology as rocks.json holds it (systemic terrain T2): rocks, stacks, provinces, shaping. */
export interface GeologyJson {
  rocks: { name: string; [k: string]: unknown }[];
  stacks: { name: string; [k: string]: unknown }[];
  provinces: Record<string, number>;
  shaping: Record<string, number>;
  /** The rule-based erosion (T4); absent in an edit stored before it. */
  erosion?: Record<string, number>;
  [k: string]: unknown;
}

/** A vegetation kind as vegetation.json holds it (systemic terrain T3): a placement's envelope. */
export interface KindJson {
  name: string;
  placement: string;
  temperature: [number, number];
  moisture: [number, number];
  [k: string]: unknown;
}

/** The soil and vegetation as vegetation.json holds it (systemic terrain T3). */
export interface VegetationJson {
  climate: Record<string, number>;
  soil: Record<string, number>;
  kinds: KindJson[];
  [k: string]: unknown;
}

/**
 * What Chisel edits: the biomes (the default first), the biome map, optional world knobs, the
 * geology (rocks.json) and the soil and vegetation (vegetation.json; each optional in a stored or
 * imported edit: the build's).
 */
export interface Edit {
  biomes: BiomeJson[];
  biomeMap: BiomeMapJson;
  world?: WorldJson;
  geology?: GeologyJson;
  vegetation?: VegetationJson;
}

/** The build's geology (shared/worldgen/rocks.json). */
export const BUILD_GEOLOGY = rocks as unknown as GeologyJson;
/** The build's soil and vegetation (shared/worldgen/vegetation.json). */
export const BUILD_VEGETATION = vegetation as unknown as VegetationJson;

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/** The build's blocks by id, as gen.mjs's `registryBlocks`. */
export const BLOCKS: Map<string, BlockInfo> = new Map(
  (blocksDwell.blocks as { name: string }[]).map((b) => [
    `${blocksDwell.namespace}:${b.name}`,
    b as BlockInfo,
  ]),
);
/** The full blocks a band may name (its pickers). */
export const FULL_BLOCKS = [...BLOCKS].filter(([, b]) => b.shape === 'full').map(([id]) => id);
const REFS = {
  decorations: new Set(decorations.decorations.map((r) => r.name)),
  placements: new Set(placements.placements.map((r) => r.name)),
  stacks: new Set(BUILD_GEOLOGY.stacks.map((s) => s.name)),
};

/** The biomes a world can start from: the build's default and the fixtures. */
export const DEFAULT_BIOME: BiomeJson = defaultBiome;
export const DESERT_FIXTURE: BiomeJson = desert;
export const ARCHIPELAGO_FIXTURE: BiomeJson = archipelago;
export const TEMPLATES: readonly BiomeJson[] = [DEFAULT_BIOME, DESERT_FIXTURE, ARCHIPELAGO_FIXTURE];

/** A preset biome (shared/worldgen/biomes/presets/*.json): a starting point with words for people. */
export type PresetJson = BiomeJson & { label: string; description: string };
/**
 * The preset biomes, by label: starting points for a world's biomes (never on the default world's
 * map), checked by the same rules gen.mjs applies (biomes.mjs `validatePresets`).
 */
export const PRESETS: readonly PresetJson[] = validatePresets(
  Object.entries(
    import.meta.glob<PresetJson>('../../../shared/worldgen/biomes/presets/*.json', {
      eager: true,
      import: 'default',
    }),
  )
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([, b]) => b),
  BLOCKS,
  REFS,
).sort((a, b) => a.label.localeCompare(b.label));
/** The archipelago example's world knobs (shared/worldgen/worlds/archipelago.json). */
export const ARCHIPELAGO_WORLD: WorldJson = archipelagoWorld.overrides;
/** The tall peak example's world knobs (shared/worldgen/worlds/tall_peak.json): a 4800 m cone. */
export const TALL_PEAK_WORLD: WorldJson = tallPeakWorld.overrides;

/** A world knob's value: the edit's, else terrain.json's. */
export function knob(e: Edit, group: string, key: string): number {
  const own = e.world?.[group]?.[key];
  if (own !== undefined) return own;
  return Number((terrain as unknown as Record<string, Record<string, unknown>>)[group]?.[key]);
}

/** The build's edit: its default biome alone, the build's biome map. */
export function buildEdit(): Edit {
  return { biomes: [clone(DEFAULT_BIOME)], biomeMap: clone(terrain.biomeMap as BiomeMapJson) };
}

/** The edit's problems (biomes.mjs's messages), none when it is valid. */
export function validateEdit(e: Edit): string[] {
  const errors: string[] = [];
  if (!Array.isArray(e.biomes) || e.biomes.length === 0)
    return ['biomes: a world has at least one biome'];
  const stacks = e.geology?.stacks;
  const refs = Array.isArray(stacks)
    ? { ...REFS, stacks: new Set(stacks.map((s) => s.name)) }
    : REFS;
  for (const b of e.biomes) {
    try {
      validateBiome(b, BLOCKS, refs);
    } catch (err) {
      errors.push((err as Error).message);
    }
  }
  const names = e.biomes.map((b) => b.name);
  const dup = names.find((n, i) => names.indexOf(n) !== i);
  if (dup) errors.push(`biome ${dup} is defined twice`);
  try {
    validateBiomeMap(e.biomeMap);
    if (e.world !== undefined) validateWorldKnobs(e.world);
    if (e.geology !== undefined) validateRocks(e.geology, BLOCKS);
    if (e.vegetation !== undefined) {
      const rockNames = new Set((e.geology ?? BUILD_GEOLOGY).rocks.map((r) => r.name));
      validateVegetation(e.vegetation, rockNames, REFS.placements);
    }
  } catch (err) {
    errors.push((err as Error).message);
  }
  return errors;
}

/** The edit as one JSON file: `{ biomes, biomeMap, world? }` (each biome a biomes/<name>.json). */
export function toJson(e: Edit): string {
  const out: Edit = { biomes: e.biomes, biomeMap: e.biomeMap };
  if (e.world) out.world = e.world;
  if (e.geology) out.geology = e.geology;
  if (e.vegetation) out.vegetation = e.vegetation;
  return JSON.stringify(out, null, 2);
}

/** An edit from `toJson`'s text (or one biome file alone); errors when it is not valid. */
export function fromJson(
  text: string,
  base: Edit = buildEdit(),
): { edit: Edit } | { errors: string[] } {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch (err) {
    return { errors: [`not JSON: ${(err as Error).message}`] };
  }
  const o = (v ?? {}) as Partial<Edit> & { name?: unknown };
  const edit: Edit =
    typeof o.name === 'string'
      ? {
          biomes: [...base.biomes.filter((b) => b.name !== o.name), v as BiomeJson],
          biomeMap: base.biomeMap,
          ...(base.geology ? { geology: base.geology } : {}),
          ...(base.vegetation ? { vegetation: base.vegetation } : {}),
        }
      : {
          biomes: o.biomes as BiomeJson[],
          biomeMap: withNewKeys(o.biomeMap ?? base.biomeMap),
          ...(o.world ? { world: o.world } : {}),
          ...(o.geology ? { geology: o.geology } : {}),
          ...(o.vegetation ? { vegetation: o.vegetation } : {}),
        };
  const errors = validateEdit(edit);
  return errors.length ? { errors } : { edit };
}

/**
 * A biome map saved by an older Chisel (before the rain shadow's keys) with the keys it lacks at
 * terrain.json's values, so it still loads.
 */
export function withNewKeys(bm: BiomeMapJson | undefined): BiomeMapJson {
  // Not an object: left for the validation to name.
  if (typeof bm !== 'object') return bm as unknown as BiomeMapJson;
  return { ...(terrain.biomeMap as BiomeMapJson), ...bm };
}

/** The browser record's key (append-only: new fields optional, unknown ones kept). */
export const STORE_KEY = 'dwell.chisel';

/** The stored edit, or the build's when none is stored or it no longer validates. */
export function loadEdit(store: Pick<Storage, 'getItem'> | undefined): Edit {
  try {
    const raw = store?.getItem(STORE_KEY);
    if (raw) {
      const rec = JSON.parse(raw) as { edit?: Edit };
      const edit = rec.edit && { ...rec.edit, biomeMap: withNewKeys(rec.edit.biomeMap) };
      if (edit && validateEdit(edit).length === 0) return edit;
    }
  } catch {
    // Private mode or an unreadable record: the build's.
  }
  return buildEdit();
}

/** Stores `e`, keeping the record's other fields. */
export function saveEdit(
  store: Pick<Storage, 'getItem' | 'setItem'> | undefined,
  e: Edit | null,
): void {
  try {
    let rec: Record<string, unknown> = {};
    try {
      const raw = store?.getItem(STORE_KEY);
      const parsed: unknown = raw ? JSON.parse(raw) : {};
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed))
        rec = parsed as Record<string, unknown>;
    } catch {
      // Unreadable: rewritten.
    }
    if (e) rec.edit = e;
    else delete rec.edit;
    store?.setItem(STORE_KEY, JSON.stringify(rec));
  } catch {
    // No storage: the edit lives as long as the page.
  }
}

/** A new biome from `template`, named so it doesn't clash with the edit's. */
export function addBiome(e: Edit, template: BiomeJson): Edit {
  const taken = new Set(e.biomes.map((b) => b.name));
  const b = clone(template);
  let name = b.name;
  for (let i = 2; taken.has(name); i++) name = `${b.name}_${String(i)}`;
  b.name = name;
  delete b.$comment;
  return { ...e, biomes: [...e.biomes, b] };
}

/**
 * Chisel's Play in game (docs/DEV_TOOLS.md "Play a test world"): a new local terrain world of
 * `seed` whose record keeps the edit as its worldgen config, so its maps are built over the edit's
 * world knobs now and whenever it reopens. Its biomes, geology and vegetation are not the game's
 * yet (only the height field reaches the world maps). Returns the world's id (`?play=<id>`).
 */
export function playInGame(index: WorldIndex, e: Edit, seed: number, now: number): string {
  const stamp = new Date(now).toISOString().slice(0, 16).replace('T', ' ');
  return index.create(
    { name: `Chisel ${stamp}`, type: 'terrain', seed, worldgen: JSON.stringify(JSON.parse(toJson(e))) },
    now,
  ).id;
}
