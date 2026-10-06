// The slope pieces the terrain generator and the LOD mesher pick for four corner heights
// (docs/SLOPE_BLOCKS.md §5): corner heights are halves of a cell (0, 1, 2) in the order NW, NE, SE,
// SW, and the table PIECE_NEAREST (generated, shared with C++) maps any pattern to the piece used.
import { PIECE_NEAREST, SHAPES, type ShapeDef } from './blocks';

/** Pattern index of corner heights (each 0..2): nw + 3·ne + 9·se + 27·sw. */
export const patternIndex = (nw: number, ne: number, se: number, sw: number): number =>
  nw + 3 * ne + 9 * se + 27 * sw;

/** The corner heights of a pattern, NW NE SE SW. */
export function patternCorners(pattern: number): [number, number, number, number] {
  return [
    pattern % 3,
    Math.floor(pattern / 3) % 3,
    Math.floor(pattern / 9) % 3,
    Math.floor(pattern / 27) % 3,
  ];
}

/** The pattern of the piece used for corner heights: itself if it has a piece, else the nearest. */
export function pieceFor(nw: number, ne: number, se: number, sw: number): number {
  return PIECE_NEAREST[patternIndex(nw, ne, se, sw)] ?? 0;
}

const upright = (corners: readonly number[]): ShapeDef | null =>
  SHAPES.find((s) => !s.inverted && s.corners.every((h, i) => h === corners[i])) ?? null;

/** The upright shape of each pattern a piece exists for (null: air, or no piece). */
export const PATTERN_SHAPE: readonly (ShapeDef | null)[] = Array.from({ length: 81 }, (_, p) =>
  p === 0 ? null : upright(patternCorners(p)),
);
