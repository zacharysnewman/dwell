// Chisel's state (ADR 0074 B3): the world's biomes (the default first) and the biome map's
// geography, as the JSON of shared/worldgen/biomes/*.json and terrain.json's `biomeMap`; checked
// by shared/worldgen/biomes.mjs, the same rules gen.mjs applies; kept in the browser as an
// append-only record (unknown fields kept on rewrite).
import {
  validateBiome,
  validateBiomeMap,
  validateWorldKnobs,
  type BlockInfo,
} from '../../../shared/worldgen/biomes.mjs';
import defaultBiome from '../../../shared/worldgen/biomes/default.json';
import desert from '../../../shared/worldgen/biomes/fixtures/desert.json';
import archipelago from '../../../shared/worldgen/biomes/fixtures/archipelago.json';
import archipelagoWorld from '../../../shared/worldgen/worlds/archipelago.json';
import terrain from '../../../shared/worldgen/terrain.json';
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

/** What Chisel edits: the biomes (the default first), the biome map, and optional world knobs. */
export interface Edit {
  biomes: BiomeJson[];
  biomeMap: BiomeMapJson;
  world?: WorldJson;
}

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
};

/** The biomes a world can start from: the build's default and the fixtures. */
export const DEFAULT_BIOME: BiomeJson = defaultBiome;
export const DESERT_FIXTURE: BiomeJson = desert;
export const ARCHIPELAGO_FIXTURE: BiomeJson = archipelago;
export const TEMPLATES: readonly BiomeJson[] = [DEFAULT_BIOME, DESERT_FIXTURE, ARCHIPELAGO_FIXTURE];
/** The archipelago example's world knobs (shared/worldgen/worlds/archipelago.json). */
export const ARCHIPELAGO_WORLD: WorldJson = archipelagoWorld.overrides;

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
  for (const b of e.biomes) {
    try {
      validateBiome(b, BLOCKS, REFS);
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
  } catch (err) {
    errors.push((err as Error).message);
  }
  return errors;
}

/** The edit as one JSON file: `{ biomes, biomeMap, world? }` (each biome a biomes/<name>.json). */
export function toJson(e: Edit): string {
  const out: Edit = { biomes: e.biomes, biomeMap: e.biomeMap };
  if (e.world) out.world = e.world;
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
        }
      : {
          biomes: o.biomes as BiomeJson[],
          biomeMap: o.biomeMap ?? base.biomeMap,
          ...(o.world ? { world: o.world } : {}),
        };
  const errors = validateEdit(edit);
  return errors.length ? { errors } : { edit };
}

/** The browser record's key (append-only: new fields optional, unknown ones kept). */
export const STORE_KEY = 'dwell.chisel';

/** The stored edit, or the build's when none is stored or it no longer validates. */
export function loadEdit(store: Pick<Storage, 'getItem'> | undefined): Edit {
  try {
    const raw = store?.getItem(STORE_KEY);
    if (raw) {
      const rec = JSON.parse(raw) as { edit?: Edit };
      if (rec.edit && validateEdit(rec.edit).length === 0) return rec.edit;
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
