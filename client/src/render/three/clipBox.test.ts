import { Vector3 } from 'three';
import { describe, expect, it } from 'vitest';
import { boxClipPlanes } from './clipBox';

describe('boxClipPlanes', () => {
  const lo: [number, number, number] = [-4, 10, 0];
  const hi: [number, number, number] = [4, 20, 8];
  const planes = boxClipPlanes(lo, hi);

  it('keeps a point inside on the positive side of all six planes', () => {
    const p = new Vector3(0, 15, 4);
    for (const plane of planes) expect(plane.distanceToPoint(p)).toBeGreaterThanOrEqual(0);
  });

  it('puts a point just outside a face on the negative side of exactly that plane', () => {
    const outside: [Vector3, number][] = [
      [new Vector3(-4.1, 15, 4), 0],
      [new Vector3(4.1, 15, 4), 1],
      [new Vector3(0, 9.9, 4), 2],
      [new Vector3(0, 20.1, 4), 3],
      [new Vector3(0, 15, -0.1), 4],
      [new Vector3(0, 15, 8.1), 5],
    ];
    for (const [p, face] of outside) {
      planes.forEach((plane, i) => {
        expect(plane.distanceToPoint(p) < 0).toBe(i === face);
      });
    }
  });
});
