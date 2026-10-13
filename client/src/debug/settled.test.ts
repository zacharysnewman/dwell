import { describe, expect, it } from 'vitest';
import { isSettled } from './settled';

const idle = { generating: 0, dirty: 0, meshing: 0 };
const lodIdle = { generating: 0, meshing: 0, asking: 0 };

describe('settled (debug tooling)', () => {
  it('is settled only when terrain and LOD are both idle', () => {
    expect(isSettled(idle, lodIdle)).toBe(true);
    expect(isSettled({ ...idle, dirty: 3 }, lodIdle)).toBe(false);
    expect(isSettled({ ...idle, generating: 1 }, lodIdle)).toBe(false);
    expect(isSettled({ ...idle, meshing: 1 }, lodIdle)).toBe(false);
    expect(isSettled(idle, { ...lodIdle, meshing: 4 })).toBe(false);
    expect(isSettled(idle, { ...lodIdle, generating: 8 })).toBe(false);
    expect(isSettled(idle, { ...lodIdle, asking: 1 })).toBe(false);
  });

  it('lets the terrain decide without an LOD', () => {
    expect(isSettled(idle, null)).toBe(true);
    expect(isSettled({ ...idle, dirty: 1 }, null)).toBe(false);
  });
});
