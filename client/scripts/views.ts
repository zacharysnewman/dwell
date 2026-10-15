// Named views of the game (client/views/views.json): where the camera is, where it looks, the
// window and the settings, for screenshots that compare builds (scripts/shots.ts) and for e2e specs
// that need the game seen from somewhere (e2e/views.ts). The camera is placed, not flown: the view
// and the LOD come from there while the player stays at the spawn (`__dwell.placeCamera`), so
// full-detail chunks show only near the spawn; elsewhere the view is the LOD's. A view without a
// camera position is the player's own eyes at the spawn. Pure: no Node or browser imports.

export type Vec3 = [number, number, number];

export interface View {
  /** Lowercase letters, digits and dashes; unique. Also the screenshot's file name. */
  name: string;
  notes: string;
  /** The local world's seed (`?seed=`). */
  seed: number;
  camera: {
    /** World metres; null: the player's eyes at the spawn. */
    position: Vec3 | null;
    /** Degrees, as the player's look: yaw 0 = +Z, pitch −90 (down) to 90 (up). */
    yaw: number;
    pitch: number;
  };
  window: { width: number; height: number };
  /** Settings menu values stored before the page loads (the menu's storage keys); unset: defaults. */
  settings: {
    fog?: { distanceM: number; density: number; heightM: number };
    detail?: { distanceM: number; pixelError: number; memoryMb: number };
    exposure?: number;
    /** The water quality (ui/waterSettings.ts). */
    water?: { quality: 'low' | 'medium' | 'high' };
    /** Speed effects (ui/motionSettings.ts): on or off and the FOV boost's strength 0–1. */
    motion?: { speedEffects: boolean; fovBoost: number };
  };
  /** The F3 overlay (`?debug=1`) and per-level LOD tints (`?lodcolors=1`) in the shot. */
  debug: boolean;
  lodColors: boolean;
  /** Resolution scale (`?scale=`, 0.25–2). */
  scale: number;
  /** Longest wait (s) for the LOD to finish loading before shooting anyway. */
  settleS: number;
}

export class ViewError extends Error {}

const NAME = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const DEFAULTS = { seed: 0, width: 1280, height: 720, scale: 1, settleS: 60 };

type Obj = Record<string, unknown>;

function isObj(v: unknown): v is Obj {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** Checks `value` against `keys`: unknown keys are errors (a typo would silently do nothing). */
function fields(where: string, value: unknown, keys: readonly string[]): Obj {
  if (!isObj(value)) throw new ViewError(`${where}: expected an object`);
  for (const k of Object.keys(value)) {
    if (!keys.includes(k)) {
      throw new ViewError(`${where}: unknown field "${k}" (expected one of ${keys.join(', ')})`);
    }
  }
  return value;
}

function num(where: string, v: unknown, min: number, max: number, fallback?: number): number {
  if (v === undefined && fallback !== undefined) return fallback;
  if (typeof v !== 'number' || !Number.isFinite(v)) {
    throw new ViewError(`${where}: expected a number`);
  }
  if (v < min || v > max) {
    throw new ViewError(`${where}: ${String(v)} is outside ${String(min)}–${String(max)}`);
  }
  return v;
}

function bool(where: string, v: unknown): boolean {
  if (v === undefined) return false;
  if (typeof v !== 'boolean') throw new ViewError(`${where}: expected true or false`);
  return v;
}

function numbers<K extends string>(
  where: string,
  v: unknown,
  keys: readonly K[],
): Record<K, number> {
  const o = fields(where, v, keys);
  const out = {} as Record<K, number>;
  for (const k of keys) out[k] = num(`${where}.${k}`, o[k], -Infinity, Infinity);
  return out;
}

function parseView(index: string, raw: unknown): View {
  // Named by its name where it has a usable one, else by its place in the list.
  const named = isObj(raw) && typeof raw.name === 'string' && NAME.test(raw.name);
  const where = named ? `views["${String(raw.name)}"]` : index;
  const o = fields(where, raw, [
    'name',
    'notes',
    'seed',
    'camera',
    'window',
    'settings',
    'debug',
    'lodColors',
    'scale',
    'settleS',
  ]);
  if (typeof o.name !== 'string' || !NAME.test(o.name)) {
    throw new ViewError(`${where}.name: expected lowercase letters, digits and dashes`);
  }
  const at = `views["${o.name}"]`;
  if (o.notes !== undefined && typeof o.notes !== 'string') {
    throw new ViewError(`${at}.notes: expected text`);
  }
  const seed = num(`${at}.seed`, o.seed, 0, Number.MAX_SAFE_INTEGER, DEFAULTS.seed);
  if (!Number.isInteger(seed)) throw new ViewError(`${at}.seed: expected a whole number`);

  const c = fields(`${at}.camera`, o.camera, ['position', 'yaw', 'pitch']);
  let position: Vec3 | null = null;
  if (c.position !== undefined && c.position !== null) {
    const p = c.position;
    if (!Array.isArray(p) || p.length !== 3) {
      throw new ViewError(`${at}.camera.position: expected [x, y, z]`);
    }
    position = [0, 1, 2].map((i) =>
      num(`${at}.camera.position[${String(i)}]`, p[i], -1e9, 1e9),
    ) as Vec3;
  }
  const camera = {
    position,
    yaw: num(`${at}.camera.yaw`, c.yaw, -360, 360),
    pitch: num(`${at}.camera.pitch`, c.pitch, -90, 90),
  };

  const w = o.window === undefined ? {} : fields(`${at}.window`, o.window, ['width', 'height']);
  const window = {
    width: num(`${at}.window.width`, w.width, 64, 4096, DEFAULTS.width),
    height: num(`${at}.window.height`, w.height, 64, 4096, DEFAULTS.height),
  };

  const s =
    o.settings === undefined
      ? {}
      : fields(`${at}.settings`, o.settings, ['fog', 'detail', 'exposure', 'motion', 'water']);
  const settings: View['settings'] = {};
  if (s.fog !== undefined) {
    settings.fog = numbers(`${at}.settings.fog`, s.fog, ['distanceM', 'density', 'heightM']);
  }
  if (s.detail !== undefined) {
    settings.detail = numbers(`${at}.settings.detail`, s.detail, [
      'distanceM',
      'pixelError',
      'memoryMb',
    ]);
  }
  if (s.exposure !== undefined)
    settings.exposure = num(`${at}.settings.exposure`, s.exposure, 0, 10);
  if (s.water !== undefined) {
    const w = fields(`${at}.settings.water`, s.water, ['quality']);
    if (w.quality !== 'low' && w.quality !== 'medium' && w.quality !== 'high') {
      throw new ViewError(`${at}.settings.water.quality: expected "low", "medium" or "high"`);
    }
    settings.water = { quality: w.quality };
  }
  if (s.motion !== undefined) {
    const m = fields(`${at}.settings.motion`, s.motion, ['speedEffects', 'fovBoost']);
    settings.motion = {
      speedEffects:
        m.speedEffects === undefined
          ? true
          : bool(`${at}.settings.motion.speedEffects`, m.speedEffects),
      fovBoost: num(`${at}.settings.motion.fovBoost`, m.fovBoost, 0, 1, 1),
    };
  }

  return {
    name: o.name,
    notes: o.notes ?? '',
    seed,
    camera,
    window,
    settings,
    debug: bool(`${at}.debug`, o.debug),
    lodColors: bool(`${at}.lodColors`, o.lodColors),
    scale: num(`${at}.scale`, o.scale, 0.25, 2, DEFAULTS.scale),
    settleS: num(`${at}.settleS`, o.settleS, 1, 600, DEFAULTS.settleS),
  };
}

/** The views of a views file's contents (`{ "views": [...] }`), checked; throws ViewError. */
export function parseViews(json: unknown): View[] {
  const file = fields('views file', json, ['$comment', 'views']);
  if (!Array.isArray(file.views)) throw new ViewError('views file: expected "views": [...]');
  const views = file.views.map((v, i) => parseView(`views[${String(i)}]`, v));
  const seen = new Set<string>();
  for (const v of views) {
    if (seen.has(v.name)) throw new ViewError(`views["${v.name}"]: the name is used twice`);
    seen.add(v.name);
  }
  return views;
}

/** The views named (all of them when `names` is empty); throws on an unknown name. */
export function selectViews(views: readonly View[], names: readonly string[]): View[] {
  const unknown = names.filter((n) => !views.some((v) => v.name === n));
  if (unknown.length > 0) {
    throw new ViewError(
      `no view named ${unknown.join(', ')} (views: ${views.map((v) => v.name).join(', ')})`,
    );
  }
  return names.length === 0 ? [...views] : views.filter((v) => names.includes(v.name));
}

/** The page address for a view: a local world by seed, with its debug switches. */
export function viewUrl(base: string, view: View): string {
  const q = new URLSearchParams({ local: '1', seed: String(view.seed) });
  if (view.scale !== 1) q.set('scale', String(view.scale));
  if (view.debug) q.set('debug', '1');
  if (view.lodColors) q.set('lodcolors', '1');
  return `${base}?${q.toString()}`;
}

/** localStorage entries for a view's settings (the settings menu's keys, ui/settingsMenu.ts). */
export function viewStorage(view: View): Record<string, string> {
  const out: Record<string, string> = {};
  if (view.settings.fog) out['dwell.fog'] = JSON.stringify(view.settings.fog);
  if (view.settings.detail) out['dwell.detail'] = JSON.stringify(view.settings.detail);
  if (view.settings.exposure !== undefined) {
    out['dwell.exposure'] = JSON.stringify(view.settings.exposure);
  }
  if (view.settings.water) out['dwell.water'] = JSON.stringify(view.settings.water);
  if (view.settings.motion) out['dwell.motion'] = JSON.stringify(view.settings.motion);
  return out;
}
