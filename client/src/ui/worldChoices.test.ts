import { describe, expect, it } from 'vitest';
import { WORLD_CHOICES, choicesOf, worldChoice } from './worldChoices';
import { WorldIndex } from '../local/worldIndex';

describe('the New world form’s World choices (world files with newWorld)', () => {
  it('offers the starter world from its data, and only files that ask', () => {
    expect(WORLD_CHOICES.map((c) => [c.name, c.label])).toEqual([['test:starter', 'Starter']]);
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
});
