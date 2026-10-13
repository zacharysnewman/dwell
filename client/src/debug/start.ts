// Where a debug session starts (debug tooling: scripts/shot.ts, and by hand): `?at=x,y,z` puts the
// player's feet there (local worlds only: the local server moves the player), `?look=yaw,pitch`
// turns the view (degrees), and `?fly=1` turns creative flight on. Applied once the player is in
// the world.

export interface DebugStart {
  /** Feet position (m), or null to stay where the world puts the player. */
  at: [number, number, number] | null;
  /** View direction (degrees), or null. */
  look: { yaw: number; pitch: number } | null;
  fly: boolean;
}

/** Comma-separated finite numbers, exactly `count` of them, or null. */
function numbers(value: string | null, count: number): number[] | null {
  if (value === null) return null;
  const parts = value.split(',').map((s) => (s.trim() === '' ? NaN : Number(s)));
  if (parts.length !== count || parts.some((n) => !Number.isFinite(n))) return null;
  return parts;
}

/** The debug start in a page's query string; malformed values are ignored. */
export function parseDebugStart(search: string): DebugStart {
  const params = new URLSearchParams(search);
  const at = numbers(params.get('at'), 3);
  const look = numbers(params.get('look'), 2);
  return {
    at: at ? [at[0] ?? 0, at[1] ?? 0, at[2] ?? 0] : null,
    look: look ? { yaw: look[0] ?? 0, pitch: look[1] ?? 0 } : null,
    fly: params.get('fly') === '1',
  };
}

/** Whether the start asks for anything at all. */
export function hasDebugStart(s: DebugStart): boolean {
  return s.at !== null || s.look !== null || s.fly;
}
