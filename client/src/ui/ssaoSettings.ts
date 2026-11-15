// The Ambient occlusion settings (docs/SSAO.md; owner 2026-11-15: "Data + Settings toggle"): the
// screen-space ambient occlusion's tier (off, low: half resolution, high: full; dwell-look's
// ssao.rs) and its strength (× shared/look/ssao.json's, 0–2), kept in this browser under
// `dwell.ssao` as a record. The store is append-only (docs/RELEASES.md): later builds add optional
// fields, and a build rewriting the record keeps the fields it doesn't know (`ssaoRecord`). The
// graphics presets set the tier (off on Low, Mobile and Medium; Low on High; High on Ultra); a
// phone or a CPU adapter never draws it whatever this says (render/ssao.rs `ssao_for`).

export const SSAO_QUALITIES = ['off', 'low', 'high'] as const;
export type SsaoQuality = (typeof SSAO_QUALITIES)[number];

export interface SsaoSettings {
  quality: SsaoQuality;
  /** × the data's strength: 0 draws none, 1 the data's, 2 twice it. */
  strength: number;
}

export const SSAO_KEY = 'dwell.ssao';
export const SSAO_STRENGTH = { min: 0, max: 2 } as const;

export function defaultSsao(): SsaoSettings {
  return { quality: 'off', strength: 1 };
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
  return { quality, strength };
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
  return { ...kept, quality: s.quality, strength: s.strength };
}
