// What the on-screen interface shows (owner, 2026-11-06: every part of it can be hidden from
// Settings, On screen, and the choice is kept), over the parts listed as data
// (shared/ui/hud.json → hud.gen.ts). Kept in this browser under `dwell.hud` as a record of each
// part's id and whether it shows; the store is append-only (docs/RELEASES.md): ids this build
// doesn't know (a newer build's parts) are kept on rewrite. A part hides by a class on the body
// (`hud-hide-<id>`) that the generated stylesheet (hud.gen.css, linked by index.html; the page's CSP
// takes no inline styles) turns into `display: none`. On a phone a part
// marked `phone: false` (the menu button, the only way to the menu there) always shows.

import { HUD_ELEMENTS, type HudElement } from './hud.gen';
import { MAP } from './map.gen';

export interface HudSettings {
  /** Each part's id: whether it shows. */
  shown: Record<string, boolean>;
  /** The minimap's height shading (ADR 0068; a toggle to compare, shared/ui/map.json's default). */
  mapShading: boolean;
}

export function defaultHud(): HudSettings {
  return {
    shown: Object.fromEntries(HUD_ELEMENTS.map((e) => [e.id, e.shown])),
    mapShading: MAP.heightShading.default,
  };
}

/** The parts Settings offers on this device (a phone can't hide what it can't do without). */
export function hideableParts(
  mobile: boolean,
  parts: readonly HudElement[] = HUD_ELEMENTS,
): HudElement[] {
  return parts.filter((e) => !mobile || e.phone);
}

/** The stored record as settings: each known part's flag, or its default. */
export function sanitizeHud(
  value: unknown,
  parts: readonly HudElement[] = HUD_ELEMENTS,
): HudSettings {
  const stored = (value as { shown?: unknown } | null)?.shown;
  const flags =
    typeof stored === 'object' && stored !== null ? (stored as Record<string, unknown>) : {};
  const shown: Record<string, boolean> = {};
  for (const e of parts) {
    const v = flags[e.id];
    shown[e.id] = typeof v === 'boolean' ? v : e.shown;
  }
  const shading = (value as { mapShading?: unknown } | null)?.mapShading;
  return {
    shown,
    mapShading: typeof shading === 'boolean' ? shading : MAP.heightShading.default,
  };
}

/** The record to store: the stored one with this build's flags written over it. */
export function hudRecord(stored: unknown, settings: HudSettings): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  const keptShown =
    typeof kept.shown === 'object' && kept.shown !== null && !Array.isArray(kept.shown)
      ? (kept.shown as Record<string, unknown>)
      : {};
  return {
    ...kept,
    shown: { ...keptShown, ...settings.shown },
    mapShading: settings.mapShading,
  };
}

/** Whether a part shows on this device. */
export function partShown(
  part: HudElement,
  settings: Pick<HudSettings, 'shown'>,
  mobile: boolean,
): boolean {
  if (mobile && !part.phone) return true;
  return settings.shown[part.id] ?? part.shown;
}

/** Shows and hides the parts. */
export function applyHud(
  doc: Document,
  settings: Pick<HudSettings, 'shown'>,
  mobile: boolean,
  parts: readonly HudElement[] = HUD_ELEMENTS,
): void {
  for (const e of parts) {
    doc.body.classList.toggle(`hud-hide-${e.id}`, !partShown(e, settings, mobile));
  }
}
