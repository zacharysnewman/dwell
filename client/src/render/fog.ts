// Distance fog (ARCHITECTURE.md §6.6): linear, from `near` (terrain starts to fade) to `far`
// (terrain is sky-coloured). On the ground the view reaches FOG_VISIBILITY_M; high above the
// terrain both scale with height so the disc stays visible from orbit.
export const FOG_START_M = 32_000;
export const FOG_VISIBILITY_M = 512_000; // about the farthest anyone sees on Earth

export function fogRange(height: number): { near: number; far: number } {
  const h = Math.max(0, height);
  return { near: Math.max(FOG_START_M, h * 2), far: Math.max(FOG_VISIBILITY_M, h * 40) };
}
