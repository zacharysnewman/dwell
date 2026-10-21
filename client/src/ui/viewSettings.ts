// The view's settings (the settings menu, kept per browser): the exposure, the clouds, the field of
// view and the arms, with their ranges and validation of stored values. The renderer that uses them
// is the Rust client's. The height fog is not a setting: it is fixed look data (shared/look/fog.json).

/** Tone mapping exposure: 1 leaves the lights as set. */
export const DEFAULT_EXPOSURE = 1;
export const EXPOSURE_LIMITS = { min: 0.4, max: 2.5 } as const;

/** Clamps an exposure into range; anything missing or not a number takes the default. */
export function sanitizeExposure(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(EXPOSURE_LIMITS.max, Math.max(EXPOSURE_LIMITS.min, value))
    : DEFAULT_EXPOSURE;
}

/** The sky's clouds (dwell-look's clouds.rs): Low has fewer noise octaves and no self-shadow. */
export const CLOUD_QUALITIES = ['off', 'low', 'high'] as const;
export type CloudQuality = (typeof CLOUD_QUALITIES)[number];

/** High on desktops, Low on phones (the GPU time a phone can spare). */
export function defaultClouds(mobile: boolean): CloudQuality {
  return mobile ? 'low' : 'high';
}

/** A stored Clouds setting; anything else (missing, unknown) takes the device's default. */
export function sanitizeClouds(value: unknown, mobile: boolean): CloudQuality {
  return CLOUD_QUALITIES.find((q) => q === value) ?? defaultClouds(mobile);
}

/** The Field of view setting: vertical degrees (dwell-lod's `base_fov` narrows it on wide screens). */
export const DEFAULT_FOV = 75;
export const FOV_LIMITS = { min: 60, max: 100 } as const;
/** The record the view's settings are stored in (append-only: other fields kept on rewrite). */
export const VIEW_KEY = 'dwell.view';

/** A stored view record's FOV: clamped and rounded to whole degrees, else the default. */
export function sanitizeFovSetting(record: unknown): number {
  const v =
    typeof record === 'object' && record !== null
      ? (record as Record<string, unknown>).fovDeg
      : undefined;
  return typeof v === 'number' && Number.isFinite(v)
    ? Math.round(Math.min(FOV_LIMITS.max, Math.max(FOV_LIMITS.min, v)))
    : DEFAULT_FOV;
}

/** A stored view record's Show your arms setting (docs/CHARACTERS.md §5): on unless stored off. */
export function sanitizeShowArms(record: unknown): boolean {
  const v =
    typeof record === 'object' && record !== null
      ? (record as Record<string, unknown>).showArms
      : undefined;
  return v !== false;
}

/** The view record to store: the stored one's other fields kept (newer builds' fields survive). */
export function viewRecord(
  stored: unknown,
  fovDeg: number,
  showArms = true,
): Record<string, unknown> {
  const base =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...base, fovDeg, showArms };
}
