import { describe, expect, it } from 'vitest';
import { PATTERN_SHAPE, patternCorners, patternIndex, pieceFor } from './slopePieces';

describe('slope pieces', () => {
  it('keeps a pattern that has a piece, and maps the rest to the nearest', () => {
    let own = 0;
    for (let p = 0; p < 81; p++) {
      const [a, b, c, d] = patternCorners(p);
      expect(patternIndex(a, b, c, d)).toBe(p);
      const piece = pieceFor(a, b, c, d);
      // A fixed point: the piece for the piece's own corners is itself.
      const [e, f, g, h] = patternCorners(piece);
      expect(pieceFor(e, f, g, h)).toBe(piece);
      // Nothing allowed is nearer.
      const distance = (q: number): number => {
        const k = patternCorners(q);
        return Math.abs(a - k[0]) + Math.abs(b - k[1]) + Math.abs(c - k[2]) + Math.abs(d - k[3]);
      };
      for (let q = 0; q < 81; q++) {
        if (pieceFor(...patternCorners(q)) === q)
          expect(distance(piece)).toBeLessThanOrEqual(distance(q));
      }
      own += piece === p ? 1 : 0;
    }
    expect(own).toBe(39); // 9 shapes × 4 facings, the slab, the cube and air
  });

  it('has a shape for every piece but air', () => {
    for (let p = 1; p < 81; p++) {
      const piece = pieceFor(...patternCorners(p));
      expect(PATTERN_SHAPE[piece], `pattern ${String(p)} → ${String(piece)}`).not.toBeNull();
    }
    expect(PATTERN_SHAPE[0]).toBeNull();
    expect(pieceFor(0, 0, 0, 0)).toBe(0);
    expect(pieceFor(2, 2, 2, 2)).toBe(patternIndex(2, 2, 2, 2));
  });
});
