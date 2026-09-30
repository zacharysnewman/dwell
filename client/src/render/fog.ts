// Height fog (ARCHITECTURE.md §6.6): a light atmospheric haze whose density falls off
// exponentially above sea level, σ(y) = σ₀·e^(−(y − seaLevel)/H). A pixel's haze follows the air
// its view ray crosses, so the ground's horizon fades toward the sky while rays from high up cross
// the thick air only once, near the ground — the haze thins with altitude by itself, and from the
// flight ceiling the whole world is clear. Capped at `density`, so nothing is ever fully hidden.
//
// `hazeAmount` is the reference for the renderer's shader (three/heightFog.ts): keep them in step.
import { World } from '../protocol/constants.gen';

export interface FogSettings {
  /** At sea level, the distance at which the haze reaches half its maximum (m). */
  distanceM: number;
  /** Maximum haze, 0 (no fog) to 1 (far terrain fades fully into the sky). */
  density: number;
  /** Scale height: the air's haze halves every H·ln 2 of height (m). */
  heightM: number;
}

export const DEFAULT_FOG: FogSettings = { distanceM: 100_000, density: 0.6, heightM: 1500 };

/** Slider ranges (the settings menu): distance up to the world's diameter. */
export const FOG_LIMITS = {
  distanceM: { min: 1000, max: 2 * World.worldRadius },
  density: { min: 0, max: 1 },
  heightM: { min: 50, max: 50_000 },
} as const;

/** Extinction coefficient at sea level (per m): haze before the cap reaches ½ at `distanceM`. */
export function fogSigma(distanceM: number): number {
  return Math.LN2 / distanceM;
}

/** Largest exponent used: keeps e^x finite in float32 (the shader) deep below sea level. */
const MAX_EXPONENT = 80;

/**
 * Haze (0 to `density`) over a point `distance` metres from a camera at height `cameraY`, the point
 * at height `pointY`: the optical depth of the exponential atmosphere along the straight ray.
 */
export function hazeAmount(
  fog: FogSettings,
  cameraY: number,
  pointY: number,
  distance: number,
): number {
  const h = fog.heightM;
  // The ray's integral of e^(−y/H) is symmetric in its ends: written from the lower one, e^x can
  // only shrink, so it neither overflows nor multiplies 0 by infinity from orbit.
  const low = Math.min(cameraY, pointY) - World.seaLevel;
  const k = Math.abs(pointY - cameraY) / h;
  const base = Math.exp(Math.min(MAX_EXPONENT, -low / h));
  const along = k < 1e-4 ? 1 - k / 2 : -Math.expm1(-k) / k;
  const depth = fogSigma(fog.distanceM) * distance * base * along;
  return fog.density * -Math.expm1(-depth);
}

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
