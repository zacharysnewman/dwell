// Types of biomes.mjs, for Chisel (client/src/chisel) and its tests.
export type Range = [number | { above: number }, number, boolean?];
export const TERRAIN: Record<string, Record<string, Range>>;
export const WHERE_FIELDS: readonly string[];
export const BIOME_GROUPS: readonly string[];
export const WORLD_GROUPS: readonly string[];
export interface BlockInfo {
  shape?: string;
  fluid?: unknown;
}
export interface BiomeRefs {
  decorations: Set<string>;
  placements: Set<string>;
  stacks?: Set<string>;
  rocks?: Set<string>;
  /** Each stack's rocks (stack name → its layers' rocks), for `arches.rock`. */
  stackRocks?: Map<string, Set<string>>;
  ground?: Set<string>;
}
export const GEOLOGY: Record<string, Record<string, Range>>;
export const JOINTING: readonly string[];
/** The climates a rock's `capBy` may name (geology.rs `CapClimate`). */
export const CAP_CLIMATES: readonly string[];
export function validateRocks<T>(data: T, blocks: Map<string, BlockInfo>): T;
export const VEGETATION: Record<string, Record<string, Range>>;
export function validateVegetation<T>(
  data: T,
  rocks?: Set<string>,
  placements?: Set<string>,
  refs?: {
    decorations?: Set<string>;
    ground?: Set<string>;
    blocks?: Map<string, BlockInfo>;
  },
): T;
export const SNOW: Record<string, Range>;
export function validateBiome<T>(
  data: T,
  blocks: Map<string, BlockInfo>,
  refs: BiomeRefs,
): T;
export function validateBiomeMap<T>(bm: T): T;
export function fixtureWorldEdit<E>(
  f: unknown,
  world: { defaultBiome: string; biomeMap: unknown },
  pool: readonly unknown[],
  rocks: unknown,
  blocks: Map<string, BlockInfo>,
  refs: BiomeRefs,
  fail: (message: string) => never,
): E;
export const WORLD_KNOBS: Record<string, readonly string[]>;
export const WATER_LOOK_FIELDS: readonly string[];
/** The knob groups' list keys (terrain.json's `TERRAIN_LISTS`, e.g. `tectonics.lavaVents`). */
export const WORLD_LISTS: Record<string, Record<string, unknown>>;
export function validateWorldKnobs<T>(w: T, base?: Record<string, unknown>): T;
export const BIOME_TEXT: Record<"label" | "description", number>;
export function validatePresets<T>(
  presets: T[],
  blocks: Map<string, BlockInfo>,
  refs: BiomeRefs,
  taken?: readonly string[],
): T[];
export const TREE_DENSITY_MAX: number;
export const ARCHES: {
  density: readonly [number, number];
  span: readonly [number, number];
  rise: readonly [number, number];
  lintel: readonly [number, number];
  fin: readonly [number, number];
};
export function checkArches(
  a: unknown,
  fail: (m: string) => never,
  rocks?: Set<string>,
  stackRocks?: Set<string>,
): void;
export const GROUND_DENSITY_MAX: number;
export function checkGround(
  ground: unknown,
  kinds: ReadonlySet<string> | undefined,
  fail: (m: string) => never,
): void;
export function checkTrees(
  trees: unknown,
  placements: readonly string[],
  fail: (m: string) => never,
): void;
