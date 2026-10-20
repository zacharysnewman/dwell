// The `?scale=` drawing-buffer scale (comparing performance on a device; `bench --compare`'s
// `raster-half`): the canvas renders at this share of its CSS size × device pixel ratio and the
// browser stretches it.

/** `?scale=`, clamped to 0.25–2; 1 without it or for anything not a number. */
export function displayScale(search: string): number {
  const scale = Number(new URLSearchParams(search).get('scale') ?? 1);
  return Number.isFinite(scale) ? Math.min(2, Math.max(0.25, scale)) : 1;
}
