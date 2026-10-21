// The Shadows setting (docs/DAY_NIGHT.md §10): the sun's and moon's cascaded shadow maps, in the
// settings menu and kept in this browser under `dwell.shadows` as a record. The store is
// append-only (docs/RELEASES.md): later builds add optional fields, and a build rewriting the
// record keeps the fields it doesn't know (`shadowRecord`). Off: no maps (direct light shaded by
// the sky light alone); Low: fewer, smaller cascades nearer the eye (phones); High: the desktop
// default. What each tier draws is data (shared/look/shadows.json). A CPU adapter draws Low at
// most whatever is chosen (render/shadows.rs `shadows_for`).
//
// The record's `torch` field is the Torch shadows setting (docs/LIGHTS.md §4, Phase 47 T2): the
// nearest torches' shadow maps, Off / Low / High (how many lights, at what resolution:
// shared/look/lights.json `shadows`), a setting of its own beside the sun's and moon's. Off on
// phones, Low on desktops; none on a CPU adapter (render/point_shadows.rs `torch_shadows_for`).
// Builds before it ignore the field and keep it on rewrite (append-only).

export const SHADOW_QUALITIES = ['off', 'low', 'high'] as const;
export type ShadowQuality = (typeof SHADOW_QUALITIES)[number];

export interface ShadowSettings {
  /** Sun and moon shadows. */
  quality: ShadowQuality;
  /** Torch shadows. */
  torch: ShadowQuality;
}

export const SHADOW_QUALITY_LABELS: Record<ShadowQuality, string> = {
  off: 'Off',
  low: 'Low',
  high: 'High',
};

/** Sun and moon: Low on phones, High on desktops; torches: Off on phones, Low on desktops. */
export function defaultShadows(mobile: boolean): ShadowSettings {
  return { quality: mobile ? 'low' : 'high', torch: mobile ? 'off' : 'low' };
}

function isQuality(v: unknown): v is ShadowQuality {
  return typeof v === 'string' && (SHADOW_QUALITIES as readonly string[]).includes(v);
}

/** The stored record as settings; anything missing or unknown takes the default. */
export function sanitizeShadows(value: unknown, fallback: ShadowSettings): ShadowSettings {
  const stored = value as { quality?: unknown; torch?: unknown } | null;
  return {
    quality: isQuality(stored?.quality) ? stored.quality : fallback.quality,
    torch: isQuality(stored?.torch) ? stored.torch : fallback.torch,
  };
}

/** What the renderer takes (`setShadows`): 0 Off, 1 Low, 2 High. */
export function shadowLevel(quality: ShadowQuality): number {
  return SHADOW_QUALITIES.indexOf(quality);
}

/**
 * The record to store: the stored one (if it is a record) with this build's fields written over
 * it, so fields a newer build added survive an older build's rewrite.
 */
export function shadowRecord(stored: unknown, settings: ShadowSettings): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...kept, quality: settings.quality, torch: settings.torch };
}

/**
 * The setting a page's query names (`?shadows=off|low|high`, a dev switch for the bench's
 * configurations), over the stored one; null without it or for an unknown name.
 */
export function shadowsFromQuery(search: string): ShadowQuality | null {
  const v = new URLSearchParams(search).get('shadows');
  return isQuality(v) ? v : null;
}

/**
 * The Torch shadows setting a page's query names (`?torchshadows=off|low|high`, the bench's
 * configurations), over the stored one; null without it or for an unknown name.
 */
export function torchShadowsFromQuery(search: string): ShadowQuality | null {
  const v = new URLSearchParams(search).get('torchshadows');
  return isQuality(v) ? v : null;
}
