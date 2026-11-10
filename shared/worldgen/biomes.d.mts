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
}
export function validateBiome<T>(
  data: T,
  blocks: Map<string, BlockInfo>,
  refs: BiomeRefs,
): T;
export function validateBiomeMap<T>(bm: T): T;
