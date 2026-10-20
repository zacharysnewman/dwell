// The spyglass's vignette (docs/SPYGLASS.md): a dark ring around the zoomed view, faded in with the
// zoom. Its shape is shared/look/spyglass.json's; the Rust client sends it with the zoom's progress
// in `Client.hud().spyglass` (null while unzoomed).

/** `Client.hud().spyglass`. */
export interface SpyglassHud {
  /** How far the zoom is in, 0–1 (the vignette's share). */
  amount: number;
  magnification: number;
  /** Clear inside, dark from the outer radius on, in units of half the screen height. */
  innerRadius: number;
  outerRadius: number;
  opacity: number;
  /** sRGB 0–1. */
  color: [number, number, number];
}

/** The vignette's CSS: a radial gradient sized to the screen height, and its opacity. */
export function spyglassStyle(s: SpyglassHud): { background: string; opacity: string } {
  const [r, g, b] = s.color.map((c) => Math.round(Math.min(1, Math.max(0, c)) * 255));
  const rgb = `${String(r)}, ${String(g)}, ${String(b)}`;
  const at = (radius: number) => `calc(${String(radius)} * 50vh)`;
  return {
    background:
      `radial-gradient(circle at 50% 50%, rgba(${rgb}, 0) ${at(s.innerRadius)}, ` +
      `rgba(${rgb}, 1) ${at(s.outerRadius)})`,
    opacity: String(Math.min(1, Math.max(0, s.amount)) * s.opacity),
  };
}
