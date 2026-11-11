// Types of paint.mjs, for Chisel's Textures tab (client/src/chisel/textures.ts) and its tests.
export interface Ramp {
  shadow: string;
  base: string;
  light: string;
}
export interface Palette {
  $comment?: string;
  ramps: Record<string, Ramp>;
  swatches?: Record<string, string>;
}
export type Layer = { kind: string; [k: string]: unknown };
export interface Material {
  use?: string;
  ramp?: string;
  like?: string;
  over?: string;
  layers?: Layer[];
  [k: string]: unknown;
}
export interface Materials {
  $comment?: string;
  tileSize?: number;
  tiles: Record<string, string>;
  materials: Record<string, Material>;
  [k: string]: unknown;
}
export const TILE: number;
export const STROKE_KINDS: readonly string[];
export const LAYER_KINDS: readonly string[];
export const USES: readonly string[];
export const RAMP_STOPS: readonly ('shadow' | 'base' | 'light')[];
export function validatePalette(palette: Palette): void;
export function resolveColor(palette: Palette, ref: string, ramp?: string, where?: string): [number, number, number];
export function resolveMaterial(materials: Materials, name: string): Material;
export function validateMaterials(materials: Materials, palette: Palette, atlasTiles?: string[] | null): void;
export function paintMaterial(name: string, materials: Materials, palette: Palette): Uint8Array;
export function imageRows(tile: Uint8Array): Uint8Array;
