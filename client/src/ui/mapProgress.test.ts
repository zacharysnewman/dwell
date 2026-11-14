import { describe, expect, it } from 'vitest';
import { MAP_STEPS } from './loading.gen';
import { faceFraction, mapProgress } from './mapProgress';
import { StageTracker } from './loadingStages';

const face = (face: number, step: number | null, fraction = 0, done = false) => ({
  face,
  step,
  fraction,
  done,
});

describe('the world map stage inside each face (BUGS.md #91)', () => {
  it("has the builder's six steps", () => {
    expect(MAP_STEPS.map((s) => s.id)).toEqual([
      'base',
      'uplift',
      'erosion',
      'lakes',
      'rivers',
      'shores',
    ]);
  });

  it('counts a face by its steps and the running one', () => {
    expect(faceFraction(face(0, null))).toBe(0);
    expect(faceFraction(face(0, 0, 0))).toBe(0);
    expect(faceFraction(face(0, 2, 0.5))).toBeCloseTo(2.5 / 6);
    expect(faceFraction(face(0, 5, 1))).toBe(1);
    expect(faceFraction(face(0, 3, 0.2, true))).toBe(1);
  });

  it('names the step of the face furthest behind', () => {
    const p = mapProgress([face(0, 4, 0.5), face(1, 2, 0.1)]);
    expect(p.detail).toBe('erosion (3/6), face 2 of 2');
    expect(p.done).toBeCloseTo(4.5 / 6 + 2.1 / 6);
    expect(mapProgress([face(0, 0, 0, true), face(1, 4, 0)]).detail).toBe(
      'rivers (5/6), face 2 of 2',
    );
    expect(mapProgress([face(0, null), face(1, null)]).detail).toBeNull();
    expect(mapProgress([face(0, 0, 1, true), face(1, 5, 1, true)])).toEqual({
      done: 2,
      detail: null,
    });
  });

  it("shows on the stage's line, with whole faces counted", () => {
    const t = new StageTracker();
    const p = mapProgress([face(0, 4, 0.5), face(1, 5, 0.9)]);
    t.progress('worldMap', p.done, 2, 10, p.detail);
    const line = t.lines(20).find((l) => l.id === 'worldMap');
    expect(line?.text).toBe('Shaping the world map — rivers (5/6), face 1 of 2…');
    expect(line?.progress).toBe('1 / 2');
    t.end('worldMap', 30);
    expect(t.lines(40).find((l) => l.id === 'worldMap')?.text).toBe('Shaping the world map');
  });
});
