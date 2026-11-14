// The audio listener's frame (docs/AUDIO.md §5a, BUGS.md #110): the camera's eye, forward and up in
// the world frame the game places sounds in (the Rust client's `listener()`, both faces), where
// WebAudio hears a point, and where the local player's own sounds are placed from it.
import type { SelfPlacement } from './types';

export type Vec3 = [number, number, number];

/** The listener: the eye, and its forward and up (unit vectors), in world space. */
export interface ListenerPose {
  position: Vec3;
  forward: Vec3;
  up: Vec3;
}

const dot = (a: Vec3, b: Vec3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

/** The listener's right: forward × up, as WebAudio's right-handed listener frame has it. */
export function rightOf(l: ListenerPose): Vec3 {
  const r = cross(l.forward, l.up);
  const n = Math.hypot(...r) || 1;
  return [r[0] / n, r[1] / n, r[2] / n];
}

/** Where the listener hears world point `p`: [right, up, back] (metres; ahead is −z). */
export function heardAt(l: ListenerPose, p: Vec3): Vec3 {
  const d: Vec3 = [p[0] - l.position[0], p[1] - l.position[1], p[2] - l.position[2]];
  return [dot(d, rightOf(l)), dot(d, l.up), -dot(d, l.forward)];
}

/** The world point of an own sound's placement (`side`: metres more to the right, a step's). */
export function selfAt(l: ListenerPose, p: SelfPlacement, side = 0): Vec3 {
  const r = rightOf(l);
  const x = p.right + side;
  return [0, 1, 2].map(
    (k) =>
      (l.position[k] ?? 0) +
      (r[k] ?? 0) * x +
      (l.up[k] ?? 0) * p.up +
      (l.forward[k] ?? 0) * p.ahead,
  ) as Vec3;
}
