// Placing shaped blocks (docs/SLOPE_BLOCKS.md §6): the pieces a player can pick for a material, and
// how the facing and the half follow the placement context. Pure, so tests pin it.
import type { Vec3 } from '../protocol/messages';
import { STATE_DEFS, stateId } from '../world/blocks';

/** The pieces, in the order the shape key cycles them (the cube first). */
export const PIECES = [
  'cube',
  'slab',
  'wedge',
  'outer',
  'inner',
  'gentle_low',
  'gentle_high',
  'gentle_outer_low',
  'gentle_outer_high',
  'gentle_inner_low',
  'gentle_inner_high',
] as const;
export type Piece = (typeof PIECES)[number];

export type Facing = 'north' | 'east' | 'south' | 'west';
export type Half = 'bottom' | 'top';

/** Name shown for a piece. */
export function pieceLabel(piece: Piece): string {
  return piece.replace(/_/g, ' ');
}

/**
 * The facing of a slope placed by a player looking along `yawDeg` (0 = +Z, 90 = +X): it rises away
 * from the player, like stairs — so it descends toward them.
 */
export function facingToward(yawDeg: number): Facing {
  const yaw = (yawDeg * Math.PI) / 180;
  const lx = Math.sin(yaw); // where the player looks
  const lz = Math.cos(yaw);
  // Descends toward the player: against the look direction's dominant axis.
  if (Math.abs(lx) > Math.abs(lz)) return lx > 0 ? 'west' : 'east';
  return lz > 0 ? 'north' : 'south';
}

/**
 * Whether a piece placed against `face` of a block (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z) hangs from
 * the ceiling: against a block's underside, or on the upper half of a side face at height
 * `hitY` (the fraction of the block's height where the view ray meets the face).
 */
export function placementHalf(face: number, hitY: number): Half {
  if (face === 3) return 'top';
  if (face === 2) return 'bottom';
  return hitY > 0.5 ? 'top' : 'bottom';
}

/** Where the view ray from `eye` along `dir` meets the plane of `face` of `cell`, as a fraction of the block's height. */
export function hitFractionY(eye: Vec3, dir: Vec3, cell: Vec3, face: number): number {
  const axis = face >> 1;
  if (axis === 1) return face === 2 ? 1 : 0;
  const plane = (cell[axis] ?? 0) + (face % 2 === 0 ? 1 : 0);
  const d = dir[axis] ?? 0;
  if (d === 0) return 0.5;
  const t = (plane - (eye[axis] ?? 0)) / d;
  return Math.min(1, Math.max(0, eye[1] + dir[1] * t - cell[1]));
}

/** Material blocks that have shaped families: `dwell:stone` → true. */
const FAMILIES = new Set(
  STATE_DEFS.flatMap((s) => (s.look === 'shaped' ? [s.state.replace(/\[.*$/, '')] : [])),
);

/** Does the material (a block id such as `dwell:stone`) come in slopes and slabs? */
export function hasShapes(blockId: string): boolean {
  return FAMILIES.has(`${blockId}_slope`);
}

/**
 * The state placing `piece` of the material block `blockId` with the given orientation: a slab or a
 * slope state, or null when the material has no shapes (or the piece is the cube: use the slot's own
 * state). `flooded` is the server's to set from the cell, so it is always false here.
 */
export function pieceState(
  blockId: string,
  piece: Piece,
  facing: Facing,
  half: Half,
): number | null {
  if (piece === 'cube' || !hasShapes(blockId)) return null;
  if (piece === 'slab') return stateId(`${blockId}_slab[flooded=false,half=${half}]`);
  return stateId(`${blockId}_slope[facing=${facing},flooded=false,half=${half},shape=${piece}]`);
}
