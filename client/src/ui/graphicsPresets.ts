// Graphics presets (docs/DEV_TOOLS.md, Graphics presets; ARCHITECTURE.md §6): Low → Mobile →
// Medium → High → Ultra, each a set of graphics settings by name, as data
// (shared/settings/graphics-presets.json). Settings → Graphics's Preset slider applies one; a
// graphics setting changed by hand shows Custom. Nothing new is stored: the preset shown is the
// one the current settings match (`presetOf`), so the stores stay as they were (each setting
// under its own append-only key) and a player's own render scale is kept. A new player starts
// on the data's default preset for the device (Mobile on phones, High on desktops).
//
// The engine is the fixed vocabulary of fields below (`PRESET_FIELDS`): each names one setting,
// checks a value from the data against the setting's own choices (the water tiers, the upscale
// data's scales and upscalers, …), and reads and writes it. A preset sets every field.
import data from '../../../shared/settings/graphics-presets.json';
import type { Settings } from './settingsMenu';
import { DETAIL_LIMITS, detailLimits } from './detailSettings';
import { UPSCALE } from './renderScaleSettings';
import { CLOUD_QUALITIES, SHAFT_QUALITIES } from './viewSettings';
import { SHADOW_QUALITIES } from './shadowSettings';
import { WATER_QUALITIES } from './waterSettings';

export type PresetValue = string | number;

/** One setting a preset can set: how to check a value, read it and write it. */
interface PresetField {
  /** The value's error, or null if it is one this setting takes. */
  check: (v: unknown) => string | null;
  get: (s: Settings) => PresetValue;
  /** Writes the value; `mobile` clamps the detail to the device's ranges. */
  with: (s: Settings, v: PresetValue, mobile: boolean) => Settings;
}

function oneOf(names: readonly (string | number)[]): (v: unknown) => string | null {
  return (v) =>
    (names as readonly unknown[]).includes(v)
      ? null
      : `${JSON.stringify(v)} is not one of ${names.map((n) => JSON.stringify(n)).join(', ')}`;
}

function inRange(r: { min: number; max: number }): (v: unknown) => string | null {
  return (v) =>
    typeof v === 'number' && v >= r.min && v <= r.max
      ? null
      : `${JSON.stringify(v)} is not a number in [${String(r.min)}, ${String(r.max)}]`;
}

function clamp(v: number, r: { min: number; max: number }): number {
  return Math.min(r.max, Math.max(r.min, v));
}

function detailField(key: 'distanceM' | 'pixelError' | 'memoryMb'): PresetField {
  return {
    check: inRange(DETAIL_LIMITS[key]),
    get: (s) => s.detail[key],
    with: (s, v, mobile) => ({
      ...s,
      detail: { ...s.detail, [key]: clamp(Number(v), detailLimits(mobile)[key]) },
    }),
  };
}

/** The settings a preset sets, by the data's names (the fixed vocabulary). */
export const PRESET_FIELDS = {
  renderScale: {
    check: oneOf(UPSCALE.scales.map((c) => c.value)),
    get: (s) => s.renderScale.scale,
    with: (s, v) => ({ ...s, renderScale: { ...s.renderScale, scale: Number(v) } }),
  },
  upscaler: {
    check: oneOf(UPSCALE.upscalers.map((u) => u.id)),
    get: (s) => s.renderScale.upscaler,
    with: (s, v) => ({ ...s, renderScale: { ...s.renderScale, upscaler: String(v) } }),
  },
  water: {
    check: oneOf(WATER_QUALITIES),
    get: (s) => s.water.quality,
    with: (s, v) => ({ ...s, water: { ...s.water, quality: v as Settings['water']['quality'] } }),
  },
  shadows: {
    check: oneOf(SHADOW_QUALITIES),
    get: (s) => s.shadows.quality,
    with: (s, v) => ({
      ...s,
      shadows: { ...s.shadows, quality: v as Settings['shadows']['quality'] },
    }),
  },
  torchShadows: {
    check: oneOf(SHADOW_QUALITIES),
    get: (s) => s.shadows.torch,
    with: (s, v) => ({
      ...s,
      shadows: { ...s.shadows, torch: v as Settings['shadows']['torch'] },
    }),
  },
  shafts: {
    check: oneOf(SHAFT_QUALITIES),
    get: (s) => s.shafts,
    with: (s, v) => ({ ...s, shafts: v as Settings['shafts'] }),
  },
  clouds: {
    check: oneOf(CLOUD_QUALITIES),
    get: (s) => s.clouds,
    with: (s, v) => ({ ...s, clouds: v as Settings['clouds'] }),
  },
  distanceM: detailField('distanceM'),
  pixelError: detailField('pixelError'),
  memoryMb: detailField('memoryMb'),
} satisfies Record<string, PresetField>;

export type PresetFieldName = keyof typeof PRESET_FIELDS;
const FIELD_NAMES = Object.keys(PRESET_FIELDS) as PresetFieldName[];

export interface GraphicsPreset {
  id: string;
  label: string;
  settings: Record<PresetFieldName, PresetValue>;
}

export interface PresetData {
  presets: GraphicsPreset[];
  default: { desktop: string; mobile: string };
}

/**
 * Checks the data: at least one preset, unique ids, a label each, every field set and no unknown
 * one, each value one its setting takes, and defaults that name presets. Fails with the entry.
 */
export function validatePresets(raw: unknown): PresetData {
  const fail = (m: string): never => {
    throw new Error(`graphics-presets.json: ${m}`);
  };
  const d = raw as Partial<PresetData> | null;
  if (!d || !Array.isArray(d.presets) || d.presets.length === 0)
    return fail('presets must be a non-empty list');
  const ids = new Set<string>();
  for (const [i, p] of d.presets.entries()) {
    const at = `presets[${String(i)}]`;
    if (typeof p.id !== 'string' || p.id === '') fail(`${at}.id must be text`);
    if (ids.has(p.id)) fail(`${at}.id ${JSON.stringify(p.id)} is listed twice`);
    ids.add(p.id);
    if (typeof p.label !== 'string' || p.label === '') fail(`${at}.label must be text`);
    const settings = p.settings as unknown as Record<string, unknown> | null | undefined;
    if (typeof settings !== 'object' || settings === null)
      return fail(`${at}.settings must be a record`);
    for (const k of Object.keys(settings))
      if (!(FIELD_NAMES as string[]).includes(k))
        fail(`${at} (${p.id}): unknown setting ${JSON.stringify(k)}`);
    for (const k of FIELD_NAMES) {
      if (!(k in settings)) fail(`${at} (${p.id}): ${k} is missing`);
      const err = PRESET_FIELDS[k].check(settings[k]);
      if (err) fail(`${at} (${p.id}): ${k}: ${err}`);
    }
  }
  for (const device of ['desktop', 'mobile'] as const) {
    const id = d.default?.[device];
    if (typeof id !== 'string' || !ids.has(id))
      fail(`default.${device} ${JSON.stringify(id)} is not a preset's id`);
  }
  return d as PresetData;
}

export const GRAPHICS_PRESETS: PresetData = validatePresets(data);

/** The preset by its id, or undefined. */
export function findPreset(
  id: string,
  d: PresetData = GRAPHICS_PRESETS,
): GraphicsPreset | undefined {
  return d.presets.find((p) => p.id === id);
}

/** The preset a new player on this device starts on. */
export function defaultPreset(mobile: boolean, d: PresetData = GRAPHICS_PRESETS): GraphicsPreset {
  const id = mobile ? d.default.mobile : d.default.desktop;
  const p = findPreset(id, d);
  if (!p) throw new Error(`graphics-presets.json: no preset ${id}`);
  return p;
}

/** The settings with the preset's settings written over them, and nothing else changed. */
export function applyPreset(s: Settings, p: GraphicsPreset, mobile: boolean): Settings {
  let out = s;
  for (const k of FIELD_NAMES) out = PRESET_FIELDS[k].with(out, p.settings[k], mobile);
  return out;
}

/**
 * `?preset=<id>` draws a visit at that preset's graphics whatever is stored (the bench's preset
 * runs, docs/DEV_TOOLS.md, Graphics presets); nothing is stored. An unknown id changes nothing.
 */
export function presetFromQuery(
  search: string,
  s: Settings,
  mobile: boolean,
  d: PresetData = GRAPHICS_PRESETS,
): Settings {
  const id = new URLSearchParams(search).get('preset');
  const p = id === null ? undefined : findPreset(id, d);
  return p ? applyPreset(s, p, mobile) : s;
}

/** The preset the settings match (as applied on this device), or null: Custom. */
export function presetOf(
  s: Settings,
  mobile: boolean,
  d: PresetData = GRAPHICS_PRESETS,
): GraphicsPreset | null {
  return (
    d.presets.find((p) => {
      const applied = applyPreset(s, p, mobile);
      return FIELD_NAMES.every((k) => PRESET_FIELDS[k].get(applied) === PRESET_FIELDS[k].get(s));
    }) ?? null
  );
}
