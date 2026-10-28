import { describe, expect, it } from 'vitest';
import { GENERATORS, parseLocalWorld } from '../local/world';
import { launchOf, withRoute } from './launch';
import {
  advancedRoute,
  clockShare,
  OVERRIDE_PARAMS,
  spawnChoices,
  type OverrideField,
} from './playAdvanced';
import { OVERRIDE_FIELDS } from './playOverrides.gen';

const field = (param: string): OverrideField => {
  const f = OVERRIDE_FIELDS.find((x) => x.param === param);
  if (!f) throw new Error(param);
  return f;
};

describe('Play advanced', () => {
  it('opens a link world with only the fields the player set', () => {
    expect(advancedRoute({})).toEqual({ route: { local: '1' } });
    const r = advancedRoute({
      world: 'flat',
      seed: ' 42 ',
      spawn: 'portal-b',
      time: '18:00',
      moon: '',
    });
    expect(r).toEqual({
      route: { local: '1', world: 'flat', seed: '42', spawn: 'portal-b', time: '0.75' },
    });
    // The page it opens reads them as the game does.
    if (!('route' in r)) throw new Error('no route');
    const search = withRoute('', r.route);
    expect(launchOf(search)).toEqual({ kind: 'link' });
    expect(parseLocalWorld(search)).toMatchObject({
      worldSeed: 42,
      generatorVersion: GENERATORS.flat,
      spawnFaceB: true,
      spawnPortal: true,
    });
  });

  it('turns a 24-hour time into the share of the day ?time= takes', () => {
    expect(clockShare('00:00')).toBe(0);
    expect(clockShare('12:00')).toBe(0.5);
    expect(clockShare('6:30')).toBe(0.2708);
    expect(clockShare('24:00')).toBeNull();
    expect(clockShare('noon')).toBeNull();
  });

  it('names the field of a bad value', () => {
    expect(advancedRoute({ seed: '-1' })).toEqual({
      problem: 'Seed: a whole number from 0 to 9007199254740991.',
    });
    expect(advancedRoute({ moon: '2' })).toEqual({ problem: 'Moon phase: a number from 0 to 1.' });
    expect(advancedRoute({ spawn: 'c' })).toEqual({ problem: 'Spawn: not a choice.' });
    expect(advancedRoute({ time: '25:00' })).toEqual({
      problem: 'Time of day: a time such as 06:30.',
    });
  });

  it('every choice is one the game reads', () => {
    for (const o of (field('world') as Extract<OverrideField, { kind: 'choice' }>).options) {
      if (o.value) expect(Object.keys(GENERATORS)).toContain(o.value);
    }
    const spawn = (field('spawn') as Extract<OverrideField, { kind: 'choice' }>).options;
    for (const o of spawn.filter((x) => x.value)) {
      expect(parseLocalWorld(`?spawn=${o.value}`).file).toBeDefined();
    }
  });

  it('a field added by data alone is passed', () => {
    const fields: OverrideField[] = [
      ...OVERRIDE_FIELDS,
      { param: 'crossingroll', label: 'Crossing roll', kind: 'number', min: 0, max: 5, step: 0.1 },
    ];
    expect(advancedRoute({ crossingroll: '0.5' }, fields)).toEqual({
      route: { local: '1', crossingroll: '0.5' },
    });
  });

  it('is one-off: the overrides leave the address on the next route', () => {
    expect(OVERRIDE_PARAMS).toEqual(
      expect.arrayContaining(['time', 'moon', 'timespeed', 'outline']),
    );
    expect(
      withRoute('?local=1&seed=3&time=0.5&moon=0.2&outline=creases&debug=1', { play: 'wabc' }),
    ).toBe('?debug=1&play=wabc');
    expect(withRoute('?local=1&time=0.5&timespeed=60', {})).toBe('');
  });

  it('offers its spawns to the New world form too', () => {
    expect(spawnChoices().map((o) => o.value)).toEqual(['', 'b', 'portal', 'portal-b']);
  });
});
