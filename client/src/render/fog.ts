// Distance fog (ARCHITECTURE.md §6.6). Off for now: the whole world is drawn without haze out to
// the rim. To bring it back, set FOG to a start and full-fog distance for the ground; high above
// the terrain both scale with height (2× and 40× it) so the disc stays visible from orbit.
export const FOG: { startM: number; visibilityM: number } | null = null;

/** Linear fog range at a camera height, or null for no fog. */
export function fogRange(height: number): { near: number; far: number } | null {
  if (!FOG) return null;
  const h = Math.max(0, height);
  return { near: Math.max(FOG.startM, h * 2), far: Math.max(FOG.visibilityM, h * 40) };
}
