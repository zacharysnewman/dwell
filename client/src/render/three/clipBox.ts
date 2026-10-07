import { Plane, Vector3 } from 'three';
import type { Vec3 } from '../../protocol/messages';

/** Six planes keeping only what lies inside [lo, hi] (three.js clips the negative side). */
export function boxClipPlanes(lo: Vec3, hi: Vec3): Plane[] {
  return [
    new Plane(new Vector3(1, 0, 0), -lo[0]),
    new Plane(new Vector3(-1, 0, 0), hi[0]),
    new Plane(new Vector3(0, 1, 0), -lo[1]),
    new Plane(new Vector3(0, -1, 0), hi[1]),
    new Plane(new Vector3(0, 0, 1), -lo[2]),
    new Plane(new Vector3(0, 0, -1), hi[2]),
  ];
}
