import { describe, expect, it } from 'vitest';

import { type TheaterClient, setHudShown, theaterOp } from './theater';

function fake(): TheaterClient & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    theaterPlay: (json) => calls.push(`play ${json}`),
    theaterStop: () => calls.push('stop'),
    theaterPause: (p) => calls.push(`pause ${String(p)}`),
    theaterState: () => ({ time: 1, duration: 4, finished: false }),
    theaterPresets: () => '{"formatVersion":1,"presets":[]}',
    theaterPreset: (name, params, subject) => JSON.stringify({ name, params, subject }),
  };
}

/** A doc with just what `setHudShown` touches. */
function fakeDoc(): Document & { styles: string[]; classes: Set<string> } {
  const styles: string[] = [];
  const classes = new Set<string>();
  const byId = new Map<string, unknown>();
  const doc = {
    styles,
    classes,
    getElementById: (id: string) => byId.get(id) ?? null,
    createElement: () => ({ id: '', textContent: '' }),
    head: {
      appendChild: (e: { id: string; textContent: string }) => {
        byId.set(e.id, e);
        styles.push(e.textContent);
      },
    },
    body: {
      classList: {
        toggle: (c: string, on: boolean) => (on ? classes.add(c) : classes.delete(c)),
      },
    },
  };
  return doc as unknown as Document & { styles: string[]; classes: Set<string> };
}

describe('theater dev API', () => {
  it('plays a path object as JSON, expands presets and stops', () => {
    const c = fake();
    const doc = fakeDoc();
    theaterOp(c, doc, 'play', { path: { formatVersion: 1 } });
    expect(c.calls).toEqual(['play {"formatVersion":1}']);
    expect(theaterOp(c, doc, 'preset', { name: 'orbit', params: { radiusM: 20 } })).toEqual({
      name: 'orbit',
      params: '{"radiusM":20}',
      subject: '"player"',
    });
    expect(theaterOp(c, doc, 'state')).toEqual({ time: 1, duration: 4, finished: false });
    theaterOp(c, doc, 'stop');
    expect(c.calls.at(-1)).toBe('stop');
    expect(() => theaterOp(c, doc, 'spin' as never)).toThrow(/unknown op spin/);
  });

  it('hides every element but the canvas with one rule, and shows them again', () => {
    const doc = fakeDoc();
    setHudShown(doc, false);
    setHudShown(doc, false);
    expect(doc.styles).toHaveLength(1);
    expect(doc.styles[0]).toContain('body.theater-clean > *:not(#view)');
    expect(doc.classes.has('theater-clean')).toBe(true);
    setHudShown(doc, true);
    expect(doc.classes.has('theater-clean')).toBe(false);
  });
});
