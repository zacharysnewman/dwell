// How block states are drawn. The styles come from the block registry (world/blocks.ts, generated
// from shared/blocks/*.json), id for id with the C++ table (ARCHITECTURE.md §6.1); the WASM core
// reports faces by these runtime state ids.
import { STATE_DEFS, type MaterialLook, type MaterialTextures } from './blocks';

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
  /** In the infinite creative palette (§6.5; C++ `Placeable`). */
  placeable?: boolean;
}

export const MATERIALS: readonly MaterialStyle[] = STATE_DEFS.map((s) => ({
  name: s.state,
  look: s.look,
  color: s.color,
  opacity: s.opacity,
  ...(s.textures ? { textures: s.textures } : {}),
  ...(s.ladderFace !== undefined ? { ladderFace: s.ladderFace } : {}),
  ...(s.placeable ? { placeable: true } : {}),
}));

export function materialStyle(id: number): MaterialStyle {
  return (
    MATERIALS[id] ?? { name: `unknown_${String(id)}`, look: 'cube', color: 0xff00ff, opacity: 1 }
  );
}

/** The infinite creative palette (§6.5): every placeable state, in id order. */
export const PLACEABLE: readonly number[] = MATERIALS.flatMap((m, id) => (m.placeable ? [id] : []));
