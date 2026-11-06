import { describe, expect, it } from 'vitest';
import { worldAccess } from './worldAccess';
import type { WorldMeta } from './worldIndex';

function world(appVersion?: string): WorldMeta {
  return {
    id: 'w1',
    name: 'Hills',
    type: 'terrain',
    seed: 7,
    generatorVersion: 8,
    createdAt: 0,
    lastPlayedAt: 0,
    ...(appVersion ? { appVersion } : {}),
  };
}

describe('worldAccess', () => {
  it('opens a world of its own line', () => {
    expect(worldAccess(world('0.48.1'), '0.48.2')).toEqual({ ok: true });
  });

  it("offers an older line's world after a warning (owner, 2026-11-06)", () => {
    const a = worldAccess(world('0.47.3'), '0.48.2');
    expect(a.ok).toBe(false);
    if (a.ok) return;
    expect(a.upgrade).toEqual({ from: '0.47.3' });
  });

  it('refuses a newer world, and a legacy one, with no upgrade', () => {
    for (const [w, build] of [
      [world('0.49.0'), '0.48.2'],
      [world('0.48.3'), '0.48.2'],
      [world(), '0.48.2'],
      [world('0.47.0'), '0.48.2-dev.4'],
    ] as const) {
      const a = worldAccess(w, build);
      expect(a.ok).toBe(false);
      if (!a.ok) expect(a.upgrade).toBeNull();
    }
  });
});
