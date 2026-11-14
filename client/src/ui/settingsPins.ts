// Settings that follow the build's defaults (owner, 2026-11-07: "Can we setup settings in a way that
// it's marked as default so if we change defaults they shift too?"; ARCHITECTURE.md §6, the settings
// row). Each setting is either *default* — not stored, so it takes whatever the running build's
// default is for this device (or the chosen graphics preset's current value) — or *pinned* by the
// player, who changed it: then its value is stored and kept whatever the defaults become.
//
// What is pinned is what is stored: a field present in its store is pinned, a missing one follows the
// default. Nothing new marks a pin, so the stores stay as they were (each setting under its own
// append-only key) and an older build reads a pinned-only record as before: the fields it finds, and
// its own defaults for the rest. Builds before 0.50.31 wrote every field, so a player's settings from
// them are all pinned and kept as they are (owner: "Keep their saved value"; no migration).
//
// The graphics preset chosen is stored by id (`dwell.graphicsPreset`, `{ "id": "high" }`, a new
// record, append-only): the graphics settings a preset sets then follow that preset's current
// definition in shared/settings/graphics-presets.json, so retuning a preset reaches its players.
//
// The vocabulary below is every stored setting: its path in `Settings`, its store key and its path in
// that record (none for a key holding the bare value). A test checks it covers every setting.
import type { PresetFieldName } from './graphicsPresets';
import { HUD_ELEMENTS } from './hud.gen';
import { FEATURES_KEY } from './featureSettings';

export const DETAIL_KEY = 'dwell.detail';
export const EXPOSURE_KEY = 'dwell.exposure';
/** Its own key (the stores are append-only: an older build ignores it, a newer one reads it). */
export const CLOUDS_KEY = 'dwell.clouds';
export const SHAFTS_KEY = 'dwell.shafts';
export const BLOOM_KEY = 'dwell.bloom';
/** A record, append-only: unknown fields are kept on rewrite (ui/waterSettings.ts). */
export const WATER_KEY = 'dwell.water';
/** A record, append-only: unknown fields are kept on rewrite (ui/shadowSettings.ts). */
export const SHADOWS_KEY = 'dwell.shadows';
/** A record, append-only: parts and fields a newer build stored are kept (ui/hudSettings.ts). */
export const HUD_KEY = 'dwell.hud';
/** The graphics preset chosen: `{ "id": … }`, append-only (0.50.31; older builds don't read it). */
export const PRESET_KEY = 'dwell.graphicsPreset';
/** The main menu's own settings: `{ "background": … }` (ui/menuBackground.ts), append-only (0.50.43). */
export const MENU_KEY = 'dwell.menu';
// The other keys live with their settings: MOTION_KEY, EXPLOSIONS_KEY, VIEW_KEY, RENDER_SCALE_KEY,
// AUDIO_KEY (passed in below, so this module doesn't import them in a cycle).

/** One stored setting. */
export interface SettingField {
  /** The pin's name: the path in `Settings`, dotted ("water.quality", "hud.shown.crosshair"). */
  id: string;
  /** The path in `Settings`. */
  at: readonly string[];
  /** The store key. */
  key: string;
  /** The path in the key's record; empty for a key holding the bare value. */
  path: readonly string[];
  /** The graphics preset field that sets it (ui/graphicsPresets.ts `PRESET_FIELDS`), if any. */
  preset?: PresetFieldName;
}

export interface StoreKeys {
  motion: string;
  explosions: string;
  view: string;
  renderScale: string;
  audio: string;
  modelDetail: string;
  antialias: string;
}

/** Every stored setting, by key. */
export function settingFields(keys: StoreKeys): SettingField[] {
  const one = (at: string[], key: string, path: string[], preset?: PresetFieldName) => ({
    id: at.join('.'),
    at,
    key,
    path,
    ...(preset ? { preset } : {}),
  });
  const rec = (prefix: string, key: string, names: string[], presets: PresetFieldName[] = []) =>
    names.map((n, i) => one([prefix, n], key, [n], presets[i]));
  return [
    ...rec(
      'detail',
      DETAIL_KEY,
      ['distanceM', 'pixelError', 'memoryMb'],
      ['distanceM', 'pixelError', 'memoryMb'],
    ),
    one(['exposure'], EXPOSURE_KEY, []),
    ...rec('audio', keys.audio, ['master', 'effects', 'ui', 'music', 'ambience']),
    one(['clouds'], CLOUDS_KEY, [], 'clouds'),
    one(['shafts'], SHAFTS_KEY, [], 'shafts'),
    one(['bloom'], BLOOM_KEY, [], 'bloom'),
    ...rec(
      'water',
      WATER_KEY,
      ['quality', 'flowRipples', 'flowFoam', 'flowParticles', 'waterfalls'],
      ['water'],
    ),
    ...rec('shadows', SHADOWS_KEY, ['quality', 'torch'], ['shadows', 'torchShadows']),
    ...rec('motion', keys.motion, ['speedEffects', 'fovBoost', 'plantSway', 'cameraSway']),
    ...rec('explosions', keys.explosions, ['flash', 'smoke', 'shake']),
    one(['fovDeg'], keys.view, ['fovDeg']),
    one(['showArms'], keys.view, ['showArms']),
    one(['leafCards'], keys.view, ['leafCards'], 'leafCards'),
    ...rec(
      'features',
      FEATURES_KEY,
      ['tallGrass', 'farTrees', 'distantShadows', 'hitShards', 'particles', 'farHerds'],
      ['tallGrass', 'farTrees', 'distantShadows', 'hitShards', 'particles', 'farHerds'],
    ),
    ...HUD_ELEMENTS.map((e) => one(['hud', 'shown', e.id], HUD_KEY, ['shown', e.id])),
    one(['hud', 'mapShading'], HUD_KEY, ['mapShading']),
    ...rec('renderScale', keys.renderScale, ['scale', 'upscaler'], ['renderScale', 'upscaler']),
    one(['menuBackground'], MENU_KEY, ['background']),
    one(['modelDetail'], keys.modelDetail, []),
    ...rec('antialias', keys.antialias, ['mode', 'anisotropy']),
  ];
}

type Obj = Record<string, unknown>;
const isRecord = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v);

/** The value at a path, or undefined. */
export function getAt(v: unknown, path: readonly string[]): unknown {
  let cur = v;
  for (const p of path) {
    if (!isRecord(cur)) return undefined;
    cur = cur[p];
  }
  return cur;
}

/** A copy with the value at a path replaced (records along it copied, nothing else changed). */
export function withAt<T>(v: T, path: readonly string[], value: unknown): T {
  if (path.length === 0) return value as T;
  const [head, ...rest] = path as [string, ...string[]];
  const base: Obj = isRecord(v) ? v : {};
  return { ...base, [head]: withAt(base[head], rest, value) } as T;
}

/** Whether a stored value holds the field: a key holding a bare value, or a record with it. */
export function storedHas(stored: unknown, path: readonly string[]): boolean {
  if (path.length === 0) return stored !== null && stored !== undefined;
  const parent = getAt(stored, path.slice(0, -1));
  return isRecord(parent) && (path.at(-1) as string) in parent;
}

/** The fields pinned: those their stores hold. */
export function pinsIn(
  fields: readonly SettingField[],
  load: (key: string) => unknown,
): Set<string> {
  return new Set(fields.filter((f) => storedHas(load(f.key), f.path)).map((f) => f.id));
}

/** The defaults, with each pinned field's value from `values` written over them. */
export function resolveSettings<T>(
  fields: readonly SettingField[],
  defaults: T,
  values: T,
  pins: ReadonlySet<string>,
): T {
  let out = defaults;
  for (const f of fields) if (pins.has(f.id)) out = withAt(out, f.at, getAt(values, f.at));
  return out;
}

/**
 * The value to store under a key, from the whole record this build writes (`full`: the stored one's
 * unknown fields kept, every known field written): the known fields not pinned taken out, records
 * left empty by it dropped. Undefined: nothing to store (remove the key).
 */
export function pinnedRecord(
  fields: readonly SettingField[],
  key: string,
  full: unknown,
  pins: ReadonlySet<string>,
): unknown {
  const mine = fields.filter((f) => f.key === key);
  if (mine.some((f) => f.path.length === 0))
    return mine.some((f) => pins.has(f.id)) ? full : undefined;
  let out: unknown = full;
  for (const f of mine) if (!pins.has(f.id)) out = withoutAt(out, f.path);
  return isRecord(out) && Object.keys(out).length === 0 ? undefined : out;
}

/** A copy without the field at a path; a record it leaves empty is dropped too. */
function withoutAt(v: unknown, path: readonly string[]): unknown {
  if (!isRecord(v) || path.length === 0) return v;
  const [head, ...rest] = path as [string, ...string[]];
  if (!(head in v)) return v;
  const others = Object.fromEntries(Object.entries(v).filter(([k]) => k !== head));
  if (rest.length === 0) return others;
  const inner = withoutAt(v[head], rest);
  return isRecord(inner) && Object.keys(inner).length === 0 ? others : { ...others, [head]: inner };
}

/** The stored preset id (`dwell.graphicsPreset`'s `id`), or null when none is (or it isn't text). */
export function storedPresetId(stored: unknown): string | null {
  const id = getAt(stored, ['id']);
  return typeof id === 'string' ? id : null;
}

/**
 * The preset record to store: the stored one's other fields kept (append-only), the id written or,
 * with none chosen, taken out. Undefined: nothing to store.
 */
export function presetRecord(stored: unknown, id: string | null): Obj | undefined {
  const kept: Obj = isRecord(stored) ? { ...stored } : {};
  if (id === null) delete kept.id;
  else kept.id = id;
  return Object.keys(kept).length === 0 ? undefined : kept;
}
