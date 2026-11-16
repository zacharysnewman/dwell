// The Ambient occlusion settings (docs/SSAO.md; owner 2026-11-15: "Data + Settings toggle"): the
// screen-space ambient occlusion's tier (off, low: half resolution, high: full; dwell-look's
// ssao.rs) and its strength (× shared/look/ssao.json's, 0–2), kept in this browser under
// `dwell.ssao` as a record. The store is append-only (docs/RELEASES.md): later builds add optional
// fields, and a build rewriting the record keeps the fields it doesn't know (`ssaoRecord`). The
// graphics presets set the tier (off on Low, Mobile and Medium; Low on High; High on Ultra); a
// phone or a CPU adapter never draws it whatever this says (render/ssao.rs `ssao_for`).

export const SSAO_QUALITIES = ['off', 'low', 'high'] as const;
export type SsaoQuality = (typeof SSAO_QUALITIES)[number];

/** Its look (shared/look/ssao.json `levels`; owner, 2026-11-15: "make a weak, normal, and strong
 * setting for SSAO with weak as our current setting"). */
export const SSAO_LEVELS = ['weak', 'normal', 'strong'] as const;
export type SsaoLevel = (typeof SSAO_LEVELS)[number];

export interface SsaoSettings {
  quality: SsaoQuality;
  /** × the look's strength: 0 draws none, 1 the look's (no longer in the menu: the bench's). */
  strength: number;
  /** The look: weak (the first one's), normal (the default) or strong. */
  level: SsaoLevel;
}

export const SSAO_KEY = 'dwell.ssao';
export const SSAO_STRENGTH = { min: 0, max: 2 } as const;

export function defaultSsao(): SsaoSettings {
  return { quality: 'off', strength: 1, level: 'normal' };
}

/** The stored record as settings; anything missing or unknown takes the fallback's. */
export function sanitizeSsao(value: unknown, fallback: SsaoSettings): SsaoSettings {
  const stored = value as Partial<Record<keyof SsaoSettings, unknown>> | null;
  const quality = SSAO_QUALITIES.find((q) => q === stored?.quality) ?? fallback.quality;
  const s = stored?.strength;
  const strength =
    typeof s === 'number' && Number.isFinite(s)
      ? Math.min(SSAO_STRENGTH.max, Math.max(SSAO_STRENGTH.min, s))
      : fallback.strength;
  // A record from before the look (no `level`) takes the default.
  const level = SSAO_LEVELS.find((l) => l === stored?.level) ?? fallback.level;
  return { quality, strength, level };
}

/**
 * The record to store: the stored one (if it is a record) with this build's fields written over
 * it, so fields a newer build added survive an older build's rewrite.
 */
export function ssaoRecord(stored: unknown, s: SsaoSettings): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...kept, quality: s.quality, strength: s.strength, level: s.level };
}
