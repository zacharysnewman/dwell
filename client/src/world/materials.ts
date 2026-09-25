// Material ids and how they are drawn. Ids mirror the table in server/core/include/dwell/core/voxel.h
// (ARCHITECTURE.md §6.1); the WASM core reports faces by these ids.
import type { TileName } from '../render/textures';

export type MaterialLook = 'cube' | 'slab' | 'ladder' | 'water';

/** Texture tiles (render/textures.ts) per face group; untextured materials use `color`. */
export interface MaterialTextures {
  top: TileName;
  side: TileName;
  bottom: TileName;
}

export interface MaterialStyle {
  name: string;
  look: MaterialLook;
  color: number;
  opacity: number;
  textures?: MaterialTextures;
}

const GRASS: MaterialTextures = { top: 'grass', side: 'grassSide', bottom: 'dirt' };
const STONE: MaterialTextures = { top: 'stone', side: 'stone', bottom: 'stone' };
const all = (tile: TileName): MaterialTextures => ({ top: tile, side: tile, bottom: tile });
const LOG: MaterialTextures = { top: 'logTop', side: 'logSide', bottom: 'logTop' };

export const MATERIALS: readonly MaterialStyle[] = [
  { name: 'air', look: 'cube', color: 0x000000, opacity: 0 },
  { name: 'bedrock', look: 'cube', color: 0x2e2e33, opacity: 1 },
  { name: 'stone', look: 'cube', color: 0x8a8d91, opacity: 1, textures: STONE },
  { name: 'dirt', look: 'cube', color: 0x7a5534, opacity: 1 },
  { name: 'grass', look: 'cube', color: 0x5e9c3a, opacity: 1, textures: GRASS },
  { name: 'stone_slab', look: 'slab', color: 0xa9adb2, opacity: 1, textures: STONE },
  { name: 'ladder_n', look: 'ladder', color: 0xa0703a, opacity: 1 },
  { name: 'ladder_e', look: 'ladder', color: 0xa0703a, opacity: 1 },
  { name: 'ladder_s', look: 'ladder', color: 0xa0703a, opacity: 1 },
  { name: 'ladder_w', look: 'ladder', color: 0xa0703a, opacity: 1 },
  { name: 'water', look: 'water', color: 0x2f6fd0, opacity: 0.55 },
  { name: 'launch_pad', look: 'cube', color: 0xe8792a, opacity: 1 },
  // Terrain generator materials (Phase 3).
  { name: 'sand', look: 'cube', color: 0xdbcf9a, opacity: 1, textures: all('sand') },
  { name: 'sandstone', look: 'cube', color: 0xc9b37a, opacity: 1, textures: all('sandstone') },
  { name: 'gravel', look: 'cube', color: 0x8c8580, opacity: 1, textures: all('gravel') },
  { name: 'snow', look: 'cube', color: 0xf2f5f8, opacity: 1, textures: all('snow') },
  { name: 'log', look: 'cube', color: 0x6b4a2b, opacity: 1, textures: LOG },
  { name: 'leaves', look: 'cube', color: 0x3f7d2c, opacity: 1, textures: all('leaves') },
  { name: 'coal_ore', look: 'cube', color: 0x8a8d91, opacity: 1, textures: all('coalOre') },
  { name: 'iron_ore', look: 'cube', color: 0x8a8d91, opacity: 1, textures: all('ironOre') },
  { name: 'gold_ore', look: 'cube', color: 0x8a8d91, opacity: 1, textures: all('goldOre') },
];

export function materialStyle(id: number): MaterialStyle {
  return (
    MATERIALS[id] ?? { name: `unknown_${String(id)}`, look: 'cube', color: 0xff00ff, opacity: 1 }
  );
}
