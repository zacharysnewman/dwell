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
}
export const GEOLOGY: Record<string, Record<string, Range>>;
export const JOINTING: readonly string[];
export function validateRocks<T>(data: T, blocks: Map<string, BlockInfo>): T;
export const VEGETATION: Record<string, Record<string, Range>>;
export function validateVegetation<T>(data: T, rocks?: Set<string>, placements?: Set<string>): T;
export function validateBiome<T>(
  data: T,
  blocks: Map<string, BlockInfo>,
  refs: BiomeRefs,
): T;
export function validateBiomeMap<T>(bm: T): T;
export const WORLD_KNOBS: Record<string, readonly string[]>;
export const WATER_LOOK_FIELDS: readonly string[];
export function validateWorldKnobs<T>(w: T): T;
export const BIOME_TEXT: Record<'label' | 'description', number>;
export function validatePresets<T>(
  presets: T[],
  blocks: Map<string, BlockInfo>,
  refs: BiomeRefs,
  taken?: readonly string[],
): T[];
