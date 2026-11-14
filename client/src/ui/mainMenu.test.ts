import { describe, expect, it } from 'vitest';
import type { WorldMeta } from '../local/worldIndex';
import {
  formatPlayed,
  moreOptionsRoute,
  nearbyLabel,
  newWorldRoute,
  worldCardData,
  worldDetails,
  worldModeLabel,
  worldSizeChoices,
} from './mainMenu';
import { moreOptionsFields } from './playAdvanced';

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
      generatorVersion: 5,
      createdAt: 0,
      lastPlayedAt: 0,
    };
    expect(worldDetails(world, 1)).toBe('Terrain · seed 42 · never played');
    // The version that last played it is the badge (RELEASES.md §6).
    expect(worldDetails({ ...world, appVersion: '0.1.0' }, 1)).toBe(
      'Terrain · seed 42 · never played · v0.1.0',
    );
    expect(worldDetails({ ...world, type: 'sandbox', generatorVersion: 100 }, 1)).toBe(
      'Sandbox · seed 42 · never played',
    );
  });
});

describe('games on your network', () => {
  it('are labelled with players for servers, and as friend worlds', () => {
    expect(nearbyLabel({ name: 'Home server', players: 2, maxPlayers: 16 })).toBe(
      'Home server · 2/16 players',
    );
    expect(nearbyLabel({ name: 'Bravo' })).toBe('Bravo · friend world');
  });
});

describe("the world list shows each world's game mode", () => {
  const base = {
    id: 'w',
    name: 'Home',
    type: 'terrain' as const,
    seed: 42,
    generatorVersion: 1,
    createdAt: 0,
    lastPlayedAt: 0,
  };
  it('names adventure and creative worlds', () => {
    expect(worldModeLabel({ ...base, mode: 'adventure' })).toEqual({
      name: 'adventure',
      label: 'Adventure',
    });
    expect(worldModeLabel({ ...base, mode: 'creative' }).label).toBe('Creative');
  });
  it('shows a world made before modes, or in the default mode, as the default', () => {
    expect(worldModeLabel(base)).toEqual({ name: 'creative', label: 'Creative' });
  });
  it('shows an unknown mode by its name', () => {
    expect(worldModeLabel({ ...base, mode: 'mymod:hardcore' }).label).toBe('mymod:hardcore');
  });
});

describe("a world's card", () => {
  const world: WorldMeta = {
    id: 'wabc',
    name: 'Home',
    type: 'flat',
    seed: 42,
    generatorVersion: 5,
    createdAt: 0,
    lastPlayedAt: 0,
  };
  it('shows the mode badge, the type and when it was played; the seed and version under ⋯', () => {
    const now = 10 * 60 * MIN;
    const played = {
      ...world,
      appVersion: '0.51.6',
      mode: 'adventure',
      lastPlayedAt: now - 3 * 60 * MIN,
    };
    expect(worldCardData(played, now)).toEqual({
      mode: 'adventure',
      modeLabel: 'Adventure',
      details: 'Flat · played 3 h ago',
      note: 'Seed 42 · last played in v0.51.6',
    });
    const card = worldCardData(world, now);
    expect(card.mode).toBe('creative');
    expect(card.details).toBe('Flat · never played');
    expect(card.note).toBe('Seed 42');
  });
});

describe("New world → More options' Sky (REALISTIC_WORLD.md §8)", () => {
  it('carries the Sky preset for a world that rolls an orbit, and nothing for the others', () => {
    const base = { name: 'Real', seed: '7', type: 'terrain', mode: '', spawn: '' };
    expect(newWorldRoute({ ...base, worldgen: 'test:realistic', sky: 'extreme' })).toEqual({
      new: '1',
      name: 'Real',
      seed: '7',
      type: 'terrain',
      worldgen: 'test:realistic',
      sky: 'extreme',
    });
    // The starter and the default terrain keep today's sky: no sky in their route.
    expect(newWorldRoute({ ...base, worldgen: 'test:starter', sky: 'extreme' })).toEqual({
      new: '1',
      name: 'Real',
      seed: '7',
      type: 'terrain',
      worldgen: 'test:starter',
    });
    expect(newWorldRoute({ ...base, sky: 'extreme' })).toEqual({
      new: '1',
      name: 'Real',
      seed: '7',
      type: 'terrain',
    });
  });
});

describe("New world → More options' World size (BIG_MAP.md M2b)", () => {
  it("offers every named size on a desktop page, today's first, and none on a touch page", () => {
    const desktop = worldSizeChoices(false);
    expect(desktop.map((c) => c.value)).toEqual(['1x', '2x', '4x', '8x', '10x']);
    expect(desktop[0]?.label).toBe('Normal (100 km across)');
    expect(desktop[4]?.label).toBe('10x (1,000 km across)');
    // Desktop first (owner, 2026-11-11): a phone page has no World size.
    expect(worldSizeChoices(true)).toEqual([]);
  });

  it("opens a bigger terrain world by its size, and today's route as before", () => {
    const base = { name: 'Big', seed: '7', type: 'terrain', mode: '', spawn: '' };
    expect(newWorldRoute({ ...base, size: '2x' })).toEqual({
      new: '1',
      name: 'Big',
      seed: '7',
      type: 'terrain',
      size: '2x',
    });
    // Today's size, none (a touch page), an unknown one and a flat world add nothing.
    const plain = { new: '1', name: 'Big', seed: '7', type: 'terrain' };
    expect(newWorldRoute({ ...base, size: '1x' })).toEqual(plain);
    expect(newWorldRoute(base)).toEqual(plain);
    expect(newWorldRoute({ ...base, size: '3x' })).toEqual(plain);
    expect(newWorldRoute({ ...base, type: 'flat', size: '2x' })).toEqual({
      ...plain,
      type: 'flat',
    });
  });
});

// BUGS.md #118: New world → More options repeated World type, Seed and Spawn (already in the form
// above), and its copy had no Realistic choice.
describe('New world → More options', () => {
  it("doesn't repeat the form's fields, and takes them from the form", () => {
    const labels = moreOptionsFields().map((f) => f.label);
    for (const l of ['World type', 'Seed', 'Spawn']) expect(labels).not.toContain(l);
    expect(labels).toContain('Time of day');
    const form = {
      name: 'x',
      seed: '42',
      type: 'terrain',
      mode: '',
      spawn: 'b',
      worldgen: 'realistic',
      sky: '',
    };
    expect(moreOptionsRoute(form)).toMatchObject({ seed: '42', spawn: 'b', worldgen: 'realistic' });
    expect(moreOptionsRoute(form).world).toBeUndefined();
    expect(moreOptionsRoute({ ...form, type: 'flat', worldgen: '', spawn: '' })).toEqual({
      seed: '42',
      world: 'flat',
    });
    // A text seed is the same number the form's world would get.
    expect(moreOptionsRoute({ ...form, seed: 'hello' }).seed).toMatch(/^\d+$/);
  });
});
