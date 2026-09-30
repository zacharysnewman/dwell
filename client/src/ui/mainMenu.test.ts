import { describe, expect, it } from 'vitest';
import type { WorldMeta } from '../local/worldIndex';
import { formatPlayed, worldDetails } from './mainMenu';

const MIN = 60_000;

describe('main menu text', () => {
  it('says when a world was last played', () => {
    const now = 100 * 24 * 60 * MIN;
    expect(formatPlayed(0, now)).toBe('never played');
    expect(formatPlayed(now - 20_000, now)).toBe('played just now');
    expect(formatPlayed(now - 5 * MIN, now)).toBe('played 5 min ago');
    expect(formatPlayed(now - 3 * 60 * MIN, now)).toBe('played 3 h ago');
    expect(formatPlayed(now - 25 * 60 * MIN, now)).toBe('played 1 day ago');
    expect(formatPlayed(now - 72 * 60 * MIN, now)).toBe('played 3 days ago');
  });

  it('describes a world by type, seed and when it was played', () => {
    const world: WorldMeta = {
      id: 'wabc',
      name: 'Home',
      type: 'terrain',
      seed: 42,
      generatorVersion: 4,
      createdAt: 0,
      lastPlayedAt: 0,
    };
    expect(worldDetails(world, 1)).toBe('Terrain · seed 42 · never played');
  });
});
