// Camera field of view. A wide horizontal field of view exaggerates forward motion (the edges of
// the view stream past) against strafing, so forward and backward walking look faster than
// sideways at the same speed.

/** Vertical field of view (degrees) on screens narrow enough not to hit the horizontal cap. */
export const VERTICAL_FOV = 75;
/** Widest horizontal field of view (degrees); wider screens get a narrower vertical one. */
export const MAX_HORIZONTAL_FOV = 100;

/** Vertical field of view (degrees) for the given aspect ratio (width / height). */
export function verticalFov(aspect: number): number {
  const h = (MAX_HORIZONTAL_FOV * Math.PI) / 180;
  const capped = (2 * Math.atan(Math.tan(h / 2) / aspect) * 180) / Math.PI;
  return Math.min(VERTICAL_FOV, capped);
}

/** Horizontal field of view (degrees) that a vertical one gives at an aspect ratio. */
export function horizontalFov(verticalDeg: number, aspect: number): number {
  const v = (verticalDeg * Math.PI) / 180;
  return (2 * Math.atan(Math.tan(v / 2) * aspect) * 180) / Math.PI;
}
