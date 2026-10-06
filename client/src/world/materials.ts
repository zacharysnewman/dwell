// How block states are drawn. The styles come from the block registry (world/blocks.ts, generated
// from shared/blocks/*.json), id for id with the C++ table (ARCHITECTURE.md §6.1); the WASM core
// reports faces by these runtime state ids.
import { SHAPES, STATE_DEFS, type MaterialLook, type MaterialTextures } from './blocks';

export type { MaterialLook, MaterialTextures };

export interface MaterialStyle {
  /** Canonical state string, e.g. `dwell:ladder[facing=north,flooded=false]`. */
  name: string;
  look: MaterialLook;
  color: number;
  opacity: number;
  textures?: MaterialTextures;
  /** Ladders: the face whose side the plate faces (0 +X, 1 −X, 4 +Z, 5 −Z). */
  ladderFace?: number;
  /** Index into SHAPES: the solid's geometry in its cell (0 = none). */
  shape: number;
  /** Shaped blocks: water fills the open part of the cell. */
  flooded: boolean;
  /** In the infinite creative palette (§6.5; C++ `Placeable`). */
  placeable?: boolean;
}

/** Index of the full cube in SHAPES (unknown ids draw as cubes). */
const CUBE_SHAPE = SHAPES.findIndex((sh) => !sh.inverted && sh.corners.every((h) => h === 2));

export const MATERIALS: readonly MaterialStyle[] = STATE_DEFS.map((s) => ({
  name: s.state,
  look: s.look,
  color: s.color,
  opacity: s.opacity,
  shape: s.shape,
  flooded: s.flooded,
  ...(s.textures ? { textures: s.textures } : {}),
  ...(s.ladderFace !== undefined ? { ladderFace: s.ladderFace } : {}),
  ...(s.placeable ? { placeable: true } : {}),
}));

export function materialStyle(id: number): MaterialStyle {
  return (
    MATERIALS[id] ?? {
      name: `unknown_${String(id)}`,
      look: 'cube',
      color: 0xff00ff,
      opacity: 1,
      shape: CUBE_SHAPE,
      flooded: false,
    }
  );
}

/** The infinite creative palette (§6.5): every placeable state, in id order. */
export const PLACEABLE: readonly number[] = MATERIALS.flatMap((m, id) => (m.placeable ? [id] : []));
