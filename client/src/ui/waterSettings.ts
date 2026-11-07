// The water quality setting (docs/WATER_RENDERING.md §4, Phase 26), in the settings menu and kept
// in this browser under `dwell.water` as a record. The store is append-only (docs/RELEASES.md):
// later builds add optional fields, and a build rewriting the record keeps the fields it doesn't
// know (`waterRecord`). Low: one layer of ripples, no scene reads (phones); Medium: the whole
// look, refraction, depth colour, foam and caustics (the desktop default); High: extras over Medium
// (finer caustics farther out; reflections to come). A CPU adapter draws Low whatever is chosen
// (render/mod.rs `water_for`). Measured in docs/WATER_RENDERING.md §8 ("Cost").

export const WATER_QUALITIES = ['low', 'medium', 'high'] as const;
export type WaterQuality = (typeof WATER_QUALITIES)[number];

export interface WaterSettings {
  quality: WaterQuality;
  /**
   * How moving water shows its flow (docs/WATER_SURFACE_GRID.md §2c, G3d; owner, 2026-11-05: each
   * one a setting): ripples carried by the current, foam streaks along it, particles drifting on it.
   * Optional fields of the record (0.49.5): a record without them takes the defaults.
   */
  flowRipples: boolean;
  flowFoam: boolean;
  flowParticles: boolean;
  /**
   * Waterfalls drawn (docs/WATER_SURFACE_GRID.md §2e, G6): falling sheets with foam and spray where
   * they land. An optional field of the record (0.49.31): a record without it takes the default.
   */
  waterfalls: boolean;
}

export const WATER_QUALITY_LABELS: Record<WaterQuality, string> = {
  low: 'Low',
  medium: 'Medium',
  high: 'High',
};

export function defaultWater(mobile: boolean): WaterSettings {
  return {
    quality: mobile ? 'low' : 'medium',
    flowRipples: true,
    flowFoam: true,
    flowParticles: true,
    waterfalls: true,
  };
}

function isQuality(v: unknown): v is WaterQuality {
  return typeof v === 'string' && (WATER_QUALITIES as readonly string[]).includes(v);
}

/** The stored record as settings; anything missing or unknown takes the default. */
export function sanitizeWater(value: unknown, fallback: WaterSettings): WaterSettings {
  const stored = value as Partial<Record<keyof WaterSettings, unknown>> | null;
  const flag = (k: 'flowRipples' | 'flowFoam' | 'flowParticles' | 'waterfalls') =>
    typeof stored?.[k] === 'boolean' ? stored[k] : fallback[k];
  return {
    quality: isQuality(stored?.quality) ? stored.quality : fallback.quality,
    flowRipples: flag('flowRipples'),
    flowFoam: flag('flowFoam'),
    flowParticles: flag('flowParticles'),
    waterfalls: flag('waterfalls'),
  };
}

/** What the renderer takes (`setWaterQuality`): 0 Low, 1 Medium, 2 High. */
export function waterQualityLevel(quality: WaterQuality): number {
  return WATER_QUALITIES.indexOf(quality);
}

/**
 * The record to store: the stored one (if it is a record) with this build's fields written over
 * it, so fields a newer build added survive an older build's rewrite.
 */
export function waterRecord(stored: unknown, settings: WaterSettings): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return {
    ...kept,
    quality: settings.quality,
    flowRipples: settings.flowRipples,
    flowFoam: settings.flowFoam,
    flowParticles: settings.flowParticles,
    waterfalls: settings.waterfalls,
  };
}
