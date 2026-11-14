import { describe, expect, it } from 'vitest';
import { WORLD_CHOICES, choicesOf, worldChoice } from './worldChoices';
import { WorldIndex } from '../local/worldIndex';
import { ORBIT_WORLDS } from './sky';
import { newWorldRoute } from './mainMenu';

describe('the New world form’s World choices (world files with newWorld)', () => {
  it('offers the Realistic and starter worlds from their data, and only files that ask', () => {
    expect(WORLD_CHOICES.map((c) => [c.name, c.label])).toEqual([
      ['test:realistic', 'Realistic'],
      ['test:starter', 'Starter'],
    ]);
    // A file added by data alone is offered.
    const more = choicesOf([
      { name: 'test:a', newWorld: { label: 'Zed', description: 'z' } },
      { name: 'test:b' },
      { name: 'test:c', newWorld: { label: 'Alpha' } },
    ]);
    expect(more).toEqual([
      { name: 'test:c', label: 'Alpha', description: '' },
      { name: 'test:a', label: 'Zed', description: 'z' },
    ]);
  });

  it('takes only an offered name from a route (anything else is the build’s terrain)', () => {
    expect(worldChoice('test:starter')).toBe('test:starter');
    expect(worldChoice('test:realistic')).toBe('test:realistic');
    expect(worldChoice('test:tall_peak')).toBe('');
    expect(worldChoice('{"biomes":[]}')).toBe('');
    expect(worldChoice('')).toBe('');
  });

  it('records the choice in the world’s entry, so the world reopens on it', () => {
    const items = new Map<string, string>();
    const store = {
      getItem: (k: string) => items.get(k) ?? null,
      setItem: (k: string, v: string) => {
        items.set(k, v);
      },
      removeItem: (k: string) => {
        items.delete(k);
      },
    };
    const index = new WorldIndex(store);
    const w = index.create(
      { name: 'S', type: 'terrain', seed: 3, worldgen: worldChoice('test:starter') },
      1,
    );
    expect(new WorldIndex(store).get(w.id)?.worldgen).toBe('test:starter');
  });

  it('offers every world that rolls an orbit, so its Sky field shows (REALISTIC_WORLD.md R6)', () => {
    const offered = WORLD_CHOICES.map((c) => c.name);
    for (const w of ORBIT_WORLDS) expect(offered).toContain(w);
  });

  it('creates a Realistic world on realistic.json with its Sky preset, and reopens on both', () => {
    const items = new Map<string, string>();
    const store = {
      getItem: (k: string) => items.get(k) ?? null,
      setItem: (k: string, v: string) => {
        items.set(k, v);
      },
      removeItem: (k: string) => {
        items.delete(k);
      },
    };
    // The form's route: the World list's value is the world file's name.
    const route = newWorldRoute({
      name: 'Real',
      seed: '7',
      type: 'terrain',
      mode: '',
      spawn: '',
      worldgen: worldChoice('test:realistic'),
      sky: 'day_varies',
    });
    expect(route.worldgen).toBe('test:realistic');
    expect(route.sky).toBe('day_varies');
    const index = new WorldIndex(store);
    const w = index.create(
      {
        name: route.name ?? '',
        type: 'terrain',
        seed: 7,
        worldgen: String(route.worldgen),
        sky: String(route.sky),
      },
      1,
    );
    const again = new WorldIndex(store).get(w.id);
    expect(again?.worldgen).toBe('test:realistic');
    expect(again?.sky).toBe('day_varies');
    expect(again?.seed).toBe(7);
  });
});
