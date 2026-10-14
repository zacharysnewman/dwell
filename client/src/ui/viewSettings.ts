// The view's settings (the settings menu, kept per browser): the height fog's and the exposure's
// values, slider ranges, and validation of stored values. The renderer that uses them is the Rust
// client's (dwell-look's fog.rs and look.rs hold the same defaults and limits).
import { World } from '../protocol/constants.gen';

export interface FogSettings {
  /** At sea level, the distance at which the haze reaches half its maximum (m). */
  distanceM: number;
  /** Maximum haze, 0 (no fog) to 1 (far terrain fades fully into the sky). */
  density: number;
  /** Scale height: the air's haze halves every H·ln 2 of height (m). */
  heightM: number;
}

export const DEFAULT_FOG: FogSettings = { distanceM: 4000, density: 0.5, heightM: 1500 };

/** Slider ranges (the settings menu): distance up to the world's diameter. */
export const FOG_LIMITS = {
  distanceM: { min: 1000, max: 2 * World.worldRadius },
  density: { min: 0, max: 1 },
  heightM: { min: 50, max: 50_000 },
} as const;

/** Clamps settings into the menu's ranges; anything missing or not a number takes the default. */
export function sanitizeFog(value: unknown): FogSettings {
  const v = (value ?? {}) as Partial<Record<keyof FogSettings, unknown>>;
  const pick = (key: keyof FogSettings): number => {
    const x = v[key];
    const { min, max } = FOG_LIMITS[key];
    return typeof x === 'number' && Number.isFinite(x)
      ? Math.min(max, Math.max(min, x))
      : DEFAULT_FOG[key];
  };
  return { distanceM: pick('distanceM'), density: pick('density'), heightM: pick('heightM') };
}

/** Tone mapping exposure: 1 leaves the lights as set. */
export const DEFAULT_EXPOSURE = 1;
export const EXPOSURE_LIMITS = { min: 0.4, max: 2.5 } as const;

/** Clamps an exposure into range; anything missing or not a number takes the default. */
export function sanitizeExposure(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(EXPOSURE_LIMITS.max, Math.max(EXPOSURE_LIMITS.min, value))
    : DEFAULT_EXPOSURE;
}
