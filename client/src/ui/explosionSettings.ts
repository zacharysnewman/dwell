// The Explosions settings (docs/EXPLOSIVES.md §4): a blast's flash, its smoke and dust, and the
// screen's shake near it, each on or off, kept in this browser under `dwell.explosions` as a
// record. The store is append-only (docs/RELEASES.md): later builds add optional fields, and a
// build rewriting the record keeps the fields it doesn't know (`explosionRecord`). The shake is off
// by default when the browser prefers reduced motion. A CPU adapter draws fewer puffs whatever is
// chosen (the data's `cpuCount`).

export interface ExplosionSettings {
  flash: boolean;
  smoke: boolean;
  shake: boolean;
}

export const EXPLOSIONS_KEY = 'dwell.explosions';

export function defaultExplosions(reducedMotion = false): ExplosionSettings {
  return { flash: true, smoke: true, shake: !reducedMotion };
}

/** The stored record as settings; anything missing or unknown takes the default. */
export function sanitizeExplosions(value: unknown, fallback: ExplosionSettings): ExplosionSettings {
  const stored = value as Partial<Record<keyof ExplosionSettings, unknown>> | null;
  const flag = (k: keyof ExplosionSettings) =>
    typeof stored?.[k] === 'boolean' ? stored[k] : fallback[k];
  return { flash: flag('flash'), smoke: flag('smoke'), shake: flag('shake') };
}

/**
 * The record to store: the stored one (if it is a record) with this build's fields written over
 * it, so fields a newer build added survive an older build's rewrite.
 */
export function explosionRecord(stored: unknown, s: ExplosionSettings): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...kept, flash: s.flash, smoke: s.smoke, shake: s.shake };
}
