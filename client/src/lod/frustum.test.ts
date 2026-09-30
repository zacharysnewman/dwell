import { describe, expect, it } from 'vitest';
import { verticalFov } from '../render/fov';
import { Frustum, lodViewport } from './frustum';

describe('LOD viewport (§6.6)', () => {
  it('measures the pixel error in CSS pixels, whatever the screen density', () => {
    // Regression (playtest: frame rate and memory on 2× screens): the height was in device
    // pixels, so a 2× screen refined as if it were twice as tall — four times the sections.
    const v = lodViewport(1440, 900);
    expect(v).toEqual({ fovYDeg: verticalFov(1.6), aspect: 1.6, heightPx: 900 });
    const cell = new Frustum({ position: [0, 0, 0], yawDeg: 0, pitchDeg: 0, ...v });
    // A 1 m cell 1 km away covers under a CSS pixel there.
    expect(cell.pixelsPerRadian / 1000).toBeLessThan(1);
  });
});
