// The motion effects' settings (docs/MOTION_EFFECTS.md, Phase 29): speed effects on or off (the
// FOV widening with speed and the wind streaks) and the FOV boost's strength. Kept per browser under
// their own key; the stored record is append-only (unknown fields kept on rewrite), like every
// cross-version browser store (CLAUDE.md, RELEASES.md).

export interface MotionSettings {
  /** The FOV boost and the wind, on or off. */
  speedEffects: boolean;
  /** The FOV boost's strength, 0–1 (the menu shows 0–100 %). */
  fovBoost: number;
}

export const MOTION_KEY = 'dwell.motion';

/** On by default, off when the browser asks for reduced motion. */
export function defaultMotion(reducedMotion: boolean): MotionSettings {
  return { speedEffects: !reducedMotion, fovBoost: 1 };
}

/** Whether the browser asks for reduced motion (false where it can't say). */
export function prefersReducedMotion(): boolean {
  try {
    return globalThis.matchMedia('(prefers-reduced-motion: reduce)').matches;
  } catch {
    return false;
  }
}

/** Stored settings into range; anything missing or of the wrong type takes the default. */
export function sanitizeMotion(value: unknown, reducedMotion: boolean): MotionSettings {
  const d = defaultMotion(reducedMotion);
  if (typeof value !== 'object' || value === null) return d;
  const v = value as Record<string, unknown>;
  const boost = v.fovBoost;
  return {
    speedEffects: typeof v.speedEffects === 'boolean' ? v.speedEffects : d.speedEffects,
    fovBoost:
      typeof boost === 'number' && Number.isFinite(boost)
        ? Math.min(1, Math.max(0, boost))
        : d.fovBoost,
  };
}

/** The record to store: the stored one's other fields kept (newer builds' fields survive). */
export function motionRecord(stored: unknown, s: MotionSettings): Record<string, unknown> {
  const base =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...base, speedEffects: s.speedEffects, fovBoost: s.fovBoost };
}
