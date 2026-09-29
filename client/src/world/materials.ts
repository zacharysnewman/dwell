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
  /** Ladders: the face whose side the plate faces (0 +X, 1 −X, 4 +Z, 5 −Z). */
  ladderFace?: number;
  /** In the infinite creative palette (§6.5; C++ `Placeable`). */
  placeable?: boolean;
}

const GRASS: MaterialTextures = { top: 'grass', side: 'grassSide', bottom: 'dirt' };
const STONE: MaterialTextures = { top: 'stone', side: 'stone', bottom: 'stone' };
const all = (tile: TileName): MaterialTextures => ({ top: tile, side: tile, bottom: tile });
const LAUNCH_PAD: MaterialTextures = { top: 'launchPad', side: 'stone', bottom: 'stone' };
const LADDER = (name: string, ladderFace: number): MaterialStyle => ({
  name,
  look: 'ladder',
  color: 0xa0703a,
  opacity: 1,
  textures: all('ladder'),
  ladderFace,
  placeable: true,
});
const LOG: MaterialTextures = { top: 'logTop', side: 'logSide', bottom: 'logTop' };

export const MATERIALS: readonly MaterialStyle[] = [
  { name: 'air', look: 'cube', color: 0x000000, opacity: 0 },
  { name: 'bedrock', look: 'cube', color: 0x2e2e33, opacity: 1, textures: all('bedrock') },
  { name: 'stone', look: 'cube', color: 0x8a8d91, opacity: 1, textures: STONE, placeable: true },
  {
    name: 'dirt',
    look: 'cube',
    color: 0x7a5534,
    opacity: 1,
    textures: all('dirt'),
    placeable: true,
  },
  { name: 'grass', look: 'cube', color: 0x5e9c3a, opacity: 1, textures: GRASS, placeable: true },
  {
    name: 'stone_slab',
    look: 'slab',
    color: 0xa9adb2,
    opacity: 1,
    textures: STONE,
    placeable: true,
  },
  LADDER('ladder_n', 5),
  LADDER('ladder_e', 0),
  LADDER('ladder_s', 4),
  LADDER('ladder_w', 1),
  { name: 'water', look: 'water', color: 0x2f6fd0, opacity: 0.55, textures: all('water') },
  { name: 'launch_pad', look: 'cube', color: 0xe8792a, opacity: 1, textures: LAUNCH_PAD },
  // Terrain generator materials (Phase 3).
  {
    name: 'sand',
    look: 'cube',
    color: 0xdbcf9a,
    opacity: 1,
    textures: all('sand'),
    placeable: true,
  },
  {
    name: 'sandstone',
    look: 'cube',
    color: 0xc9b37a,
    opacity: 1,
    textures: all('sandstone'),
    placeable: true,
  },
  {
    name: 'gravel',
    look: 'cube',
    color: 0x8c8580,
    opacity: 1,
    textures: all('gravel'),
    placeable: true,
  },
  {
    name: 'snow',
    look: 'cube',
    color: 0xf2f5f8,
    opacity: 1,
    textures: all('snow'),
    placeable: true,
  },
  { name: 'log', look: 'cube', color: 0x6b4a2b, opacity: 1, textures: LOG, placeable: true },
  {
    name: 'leaves',
    look: 'cube',
    color: 0x3f7d2c,
    opacity: 1,
    textures: all('leaves'),
    placeable: true,
  },
  {
    name: 'coal_ore',
    look: 'cube',
    color: 0x8a8d91,
    opacity: 1,
    textures: all('coalOre'),
    placeable: true,
  },
  {
    name: 'iron_ore',
    look: 'cube',
    color: 0x8a8d91,
    opacity: 1,
    textures: all('ironOre'),
    placeable: true,
  },
  {
    name: 'gold_ore',
    look: 'cube',
    color: 0x8a8d91,
    opacity: 1,
    textures: all('goldOre'),
    placeable: true,
  },
];

export function materialStyle(id: number): MaterialStyle {
  return (
    MATERIALS[id] ?? { name: `unknown_${String(id)}`, look: 'cube', color: 0xff00ff, opacity: 1 }
  );
}

/** The infinite creative palette (§6.5): every placeable material, in table order. */
export const PLACEABLE: readonly number[] = MATERIALS.flatMap((m, id) => (m.placeable ? [id] : []));
