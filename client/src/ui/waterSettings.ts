// The water quality setting (docs/WATER_RENDERING.md §4, Phase 26), in the settings menu and kept
// in this browser under `dwell.water` as a record. The store is append-only (docs/RELEASES.md):
// later builds add optional fields, and a build rewriting the record keeps the fields it doesn't
// know (`waterRecord`). The tiers are data (shared/look/watertiers.json, cheapest first, checked
// here as shared/look/watertiers.mjs checks them for Rust): Low sees through with one ripple layer
// and the bank band (phones), Medium is the whole look (the desktop default; High was folded into
// it in 0.50.29, and a stored "high" reads as Medium and stays stored as it was). A CPU adapter
// draws the data's opaque CPU tier whatever is chosen (render/mod.rs `water_for`). Measured in
// docs/DEV_TOOLS.md ("Water tiers (M1 Air)").

import data from '../../../shared/look/watertiers.json';

export interface WaterTierData {
  id: string;
  label: string;
  setting: boolean;
  cpu: boolean;
  readsScene: boolean;
  rippleLayers: number;
  flowStreaks: boolean;
  bankBand: boolean;
}

export interface WaterTiersFile {
  tiers: WaterTierData[];
  aliases: Record<string, string>;
}

const fail = (m: string): never => {
  throw new Error(`watertiers.json: ${m}`);
};

/** The tiers, checked: names unique, one CPU tier, aliases naming a Settings tier. */
export function checkWaterTiers(file: WaterTiersFile): WaterTiersFile {
  const ids = new Set<string>();
  for (const t of file.tiers) {
    if (typeof t.id !== 'string' || !/^[a-z][a-z0-9-]*$/.test(t.id))
      fail(`bad tier id ${JSON.stringify(t.id)}`);
    if (ids.has(t.id)) fail(`tier ${t.id} is listed twice`);
    ids.add(t.id);
  }
  if (file.tiers.filter((t) => t.cpu).length !== 1)
    fail('exactly one tier must be the CPU adapter’s');
  if (!file.tiers.some((t) => t.setting)) fail('no tier is offered in Settings');
  for (const [from, to] of Object.entries(file.aliases)) {
    if (from === '$comment') continue;
    if (ids.has(from)) fail(`aliases.${from}: a listed tier can’t be an alias`);
    if (!file.tiers.some((t) => t.id === to && t.setting))
      fail(`aliases.${from}: ${to} is not a Settings tier`);
  }
  return file;
}

const TIERS = checkWaterTiers(data);

/** The tiers offered in Settings, cheapest first (the presets name them). */
export const WATER_QUALITIES: readonly string[] = TIERS.tiers
  .filter((t) => t.setting)
  .map((t) => t.id);
export type WaterQuality = string;

/** A stored or named tier as the one it reads as (a removed tier's alias), or null if unknown. */
export function waterTier(name: unknown, file: WaterTiersFile = TIERS): string | null {
  if (typeof name !== 'string') return null;
  const alias = name === '$comment' ? undefined : file.aliases[name];
  const id = alias ?? name;
  return file.tiers.some((t) => t.id === id) ? id : null;
}

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

/** The Settings buttons' labels, from the data. */
export const WATER_QUALITY_LABELS: Record<WaterQuality, string> = Object.fromEntries(
  TIERS.tiers.map((t) => [t.id, t.label]),
);

export function defaultWater(mobile: boolean): WaterSettings {
  return {
    quality: mobile ? 'low' : 'medium',
    flowRipples: true,
    flowFoam: true,
    flowParticles: true,
    waterfalls: true,
  };
}

/** A stored quality as the Settings tier it reads as ("high" → "medium"), or null. */
function settingTier(v: unknown): WaterQuality | null {
  const id = waterTier(v);
  return id !== null && WATER_QUALITIES.includes(id) ? id : null;
}

/** The stored record as settings; anything missing or unknown takes the default. */
export function sanitizeWater(value: unknown, fallback: WaterSettings): WaterSettings {
  const stored = value as Partial<Record<keyof WaterSettings, unknown>> | null;
  const flag = (k: 'flowRipples' | 'flowFoam' | 'flowParticles' | 'waterfalls') =>
    typeof stored?.[k] === 'boolean' ? stored[k] : fallback[k];
  return {
    quality: settingTier(stored?.quality) ?? fallback.quality,
    flowRipples: flag('flowRipples'),
    flowFoam: flag('flowFoam'),
    flowParticles: flag('flowParticles'),
    waterfalls: flag('waterfalls'),
  };
}

/**
 * What the renderer takes (`setWaterQuality`): the tier's index in the data (the renderer's
 * `WaterQuality::from_level`), any tier by name (the bench may name the CPU's), an alias as its tier.
 */
export function waterQualityLevel(quality: WaterQuality): number {
  const id = waterTier(quality) ?? 'medium';
  return TIERS.tiers.findIndex((t) => t.id === id);
}

/**
 * The record to store: the stored one (if it is a record) with this build's fields written over
 * it, so fields a newer build added survive an older build's rewrite. A stored quality that reads
 * as the chosen one (a removed tier's alias: "high" for Medium) is kept as it was.
 */
export function waterRecord(stored: unknown, settings: WaterSettings): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  const quality =
    kept.quality !== settings.quality && settingTier(kept.quality) === settings.quality
      ? kept.quality
      : settings.quality;
  return {
    ...kept,
    quality,
    flowRipples: settings.flowRipples,
    flowFoam: settings.flowFoam,
    flowParticles: settings.flowParticles,
    waterfalls: settings.waterfalls,
  };
}
