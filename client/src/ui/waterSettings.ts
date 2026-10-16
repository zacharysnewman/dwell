// The water quality setting (docs/WATER_RENDERING.md §4, Phase 26), in the settings menu and kept
// in this browser under `dwell.water` as a record. The store is append-only (docs/RELEASES.md):
// later builds add optional fields, and a build rewriting the record keeps the fields it doesn't
// know (`waterRecord`). W1b: Low draws one layer of ripples, Medium and High three (they part with
// refraction, W3, and reflections, W5). Phones default to Low (owner, tentatively), desktops High.

export const WATER_QUALITIES = ['low', 'medium', 'high'] as const;
export type WaterQuality = (typeof WATER_QUALITIES)[number];

export interface WaterSettings {
  quality: WaterQuality;
}

export const WATER_QUALITY_LABELS: Record<WaterQuality, string> = {
  low: 'Low',
  medium: 'Medium',
  high: 'High',
};

export function defaultWater(mobile: boolean): WaterSettings {
  return { quality: mobile ? 'low' : 'high' };
}

function isQuality(v: unknown): v is WaterQuality {
  return typeof v === 'string' && (WATER_QUALITIES as readonly string[]).includes(v);
}

/** The stored record as settings; anything missing or unknown takes the default. */
export function sanitizeWater(value: unknown, fallback: WaterSettings): WaterSettings {
  const stored = value as { quality?: unknown } | null;
  return { quality: isQuality(stored?.quality) ? stored.quality : fallback.quality };
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
  return { ...kept, quality: settings.quality };
}
