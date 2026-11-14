// The Graphics settings for features that had none (WORK_QUEUE item 30, owner 2026-11-12: "Ensure
// all of our graphics have settings in our graphics menu."): Tall grass (the share of grass and small
// plants meshed), Far trees (the distant terrain's trees), Distant shadows (the distant terrain's sun
// shadows, at the Shadows tier when on; before this they followed Shadows alone), Hit shards (the
// share of a pickaxe hit's flying fragments), Particles (the share of chips, sparks, falling leaves
// and steam) and Far herds (the far ecology's herds past the near creatures). Each preset sets them
// (shared/settings/graphics-presets.json); every preset's value is what was drawn before they existed.
//
// Kept in this browser under `dwell.features` as one record, append-only (docs/RELEASES.md): a field
// missing or unreadable takes the default, fields a newer build adds are kept on rewrite, and a build
// before it reads nothing here and draws everything, as it did.

/** The record the settings are stored in. */
export const FEATURES_KEY = 'dwell.features';

export interface FeatureSettings {
  /** The share of tall grass and small plants drawn, 0–1. */
  tallGrass: number;
  /** The distant terrain's trees. */
  farTrees: boolean;
  /** The distant terrain's sun shadows (at the Shadows setting's tier). */
  distantShadows: boolean;
  /** The share of a hit's flying shards, 0–1. */
  hitShards: number;
  /** The share of chips, sparks, falling leaves and steam, 0–1. */
  particles: number;
  /** The far ecology's herds. */
  farHerds: boolean;
}

/** The share settings' names (0–1 sliders), and the switches' (on/off). */
export const FEATURE_SHARES = ['tallGrass', 'hitShards', 'particles'] as const;
export const FEATURE_SWITCHES = ['farTrees', 'distantShadows', 'farHerds'] as const;

/** Everything drawn: what every build before these settings drew. */
export function defaultFeatures(): FeatureSettings {
  return {
    tallGrass: 1,
    farTrees: true,
    distantShadows: true,
    hitShards: 1,
    particles: 1,
    farHerds: true,
  };
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

/** The stored record as settings: each field checked, anything missing or unknown the fallback's. */
export function sanitizeFeatures(value: unknown, fallback: FeatureSettings): FeatureSettings {
  const r = isRecord(value) ? value : {};
  const out = { ...fallback };
  for (const k of FEATURE_SHARES) {
    const v = r[k];
    if (typeof v === 'number' && Number.isFinite(v)) out[k] = Math.min(1, Math.max(0, v));
  }
  for (const k of FEATURE_SWITCHES) {
    const v = r[k];
    if (typeof v === 'boolean') out[k] = v;
  }
  return out;
}

/** The record to store: the stored one's other fields kept (newer builds' fields survive). */
export function featureRecord(stored: unknown, s: FeatureSettings): Record<string, unknown> {
  return { ...(isRecord(stored) ? stored : {}), ...s };
}
