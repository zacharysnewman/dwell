import { describe, expect, it } from 'vitest';
import { MAX_HORIZONTAL_FOV, VERTICAL_FOV, horizontalFov, verticalFov } from './fov';

describe('camera field of view', () => {
  it('keeps the horizontal field of view at or under the cap on wide screens', () => {
    // Desktop 16:9, ultrawide, and a landscape phone (iPhone 13: 844 × 390).
    for (const aspect of [16 / 9, 21 / 9, 844 / 390]) {
      const h = horizontalFov(verticalFov(aspect), aspect);
      expect(h).toBeLessThanOrEqual(MAX_HORIZONTAL_FOV + 1e-9);
      expect(h).toBeGreaterThan(MAX_HORIZONTAL_FOV - 0.5);
    }
  });

  it('uses the vertical field of view on narrower screens', () => {
    for (const aspect of [4 / 3, 1, 390 / 844]) expect(verticalFov(aspect)).toBe(VERTICAL_FOV);
  });
});
