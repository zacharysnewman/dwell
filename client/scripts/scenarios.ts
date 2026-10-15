// Scripted scenarios for the dev bench (docs/DEV_TOOLS.md, client/bench/scenarios.json): a local
// world, a window and settings, then steps from a small fixed vocabulary that drive the game
// through `window.__dwell` and record what it reports. scripts/bench.ts runs them in a real browser;
// adding a benchmark or a repro is a new entry in the data file. Pure: no Node or browser imports.

import { ViewError, viewUrl, type View } from './views.ts';

export type Vec3 = [number, number, number];

/** One step. Times are seconds. */
export type Step =
  /** Waits. */
  | { do: 'wait'; s: number }
  /** Presses or releases a key (`KeyboardEvent.code`), as the keyboard does. */
  | { do: 'key'; code: string; down: boolean }
  /** Holds a key for `s`, then releases it. */
  | { do: 'hold'; code: string; s: number }
  /** Turns the player's look (degrees). */
  | { do: 'look'; yaw: number; pitch: number }
  /** Creative flight on or off; its speed level. */
  | { do: 'fly'; on: boolean }
  | { do: 'flySpeed'; level: number }
  /** Places the camera (the player stays), or returns it to the player. */
  | { do: 'camera'; position: Vec3; yaw: number; pitch: number }
  | { do: 'releaseCamera' }
  /** Breaks or places at the crosshair, as a click does; selects a hotbar slot. */
  | { do: 'edit'; action: 'break' | 'place' }
  | { do: 'select'; slot: number }
  /** Waits (at most `maxS`) until the LOD has nothing to generate, mesh or ask for, for 1 s. */
  | { do: 'settle'; maxS: number }
  /** Forgets the telemetry so far: the next `record` measures from here. */
  | { do: 'mark' }
  /** Saves the telemetry, the controller ticks and the game's state under `name`. */
  | { do: 'record'; name: string }
  /** Saves a screenshot `<name>.png`. */
  | { do: 'shot'; name: string }
  /** Starts or stops a Chrome performance trace (`<name>.trace.json`, open in DevTools). */
  | { do: 'traceStart' }
  | { do: 'traceStop'; name: string }
  /**
   * Shows the time of day `t` (0–1, 0 midnight, 0.5 noon on face A) and, if given, the moon's
   * phase (0 new, 0.5 full), on this client only (docs/DAY_NIGHT.md).
   */
  | { do: 'time'; t: number; moon?: number }
  /**
   * A timelapse: `frames` screenshots `<name>-00.png`… from time of day `from` to `to` (wrapping
   * past midnight when `to` < `from`), waiting `waitS` after each change.
   */
  | { do: 'timelapse'; name: string; frames: number; from: number; to: number; waitS: number }
  /** Runs `steps` `times` times (e.g. a burst of edits); no `mark`, `record`, `shot` or traces inside. */
  | { do: 'repeat'; times: number; steps: Step[] };

/** Which part of the view a scenario measures, for a comparison report's near/far split. */
export type Split = 'near' | 'far';

export interface Scenario {
  name: string;
  notes: string;
  seed: number;
  window: { width: number; height: number };
  settings: View['settings'];
  debug: boolean;
  scale: number;
  /** `?renderer=` (a renderer the client knows by name); null: the client's default. */
  renderer: string | null;
  /** Near or far view, for `bench --compare`'s split; null: neither. */
  split: Split | null;
  steps: Step[];
}

const NAME = /^[a-z0-9]+(-[a-z0-9]+)*$/;
type Obj = Record<string, unknown>;

function obj(where: string, v: unknown, keys: readonly string[]): Obj {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) {
    throw new ViewError(`${where}: expected an object`);
  }
  for (const k of Object.keys(v)) {
    if (!keys.includes(k)) {
      throw new ViewError(`${where}: unknown field "${k}" (expected one of ${keys.join(', ')})`);
    }
  }
  return v as Obj;
}

function num(where: string, v: unknown, min: number, max: number, fallback?: number): number {
  if (v === undefined && fallback !== undefined) return fallback;
  if (typeof v !== 'number' || !Number.isFinite(v))
    throw new ViewError(`${where}: expected a number`);
  if (v < min || v > max) {
    throw new ViewError(`${where}: ${String(v)} is outside ${String(min)}–${String(max)}`);
  }
  return v;
}

function str(where: string, v: unknown): string {
  if (typeof v !== 'string' || v === '') throw new ViewError(`${where}: expected a string`);
  return v;
}

function bool(where: string, v: unknown, fallback?: boolean): boolean {
  if (v === undefined && fallback !== undefined) return fallback;
  if (typeof v !== 'boolean') throw new ViewError(`${where}: expected true or false`);
  return v;
}

function name(where: string, v: unknown): string {
  const n = str(where, v);
  if (!NAME.test(n))
    throw new ViewError(`${where}: "${n}" must be lowercase words joined by dashes`);
  return n;
}

function vec3(where: string, v: unknown): Vec3 {
  if (!Array.isArray(v) || v.length !== 3) throw new ViewError(`${where}: expected [x, y, z]`);
  return v.map((x, i) => num(`${where}[${String(i)}]`, x, -1e7, 1e7)) as Vec3;
}

const STEP_FIELDS: Record<Step['do'], readonly string[]> = {
  wait: ['s'],
  key: ['code', 'down'],
  hold: ['code', 's'],
  look: ['yaw', 'pitch'],
  fly: ['on'],
  flySpeed: ['level'],
  camera: ['position', 'yaw', 'pitch'],
  releaseCamera: [],
  edit: ['action'],
  select: ['slot'],
  settle: ['maxS'],
  mark: [],
  record: ['name'],
  shot: ['name'],
  traceStart: [],
  traceStop: ['name'],
  time: ['t', 'moon'],
  timelapse: ['name', 'frames', 'from', 'to', 'waitS'],
  repeat: ['times', 'steps'],
};

/** Steps that name an output or change the run's state, so can't run more than once in a row. */
const NOT_REPEATED: readonly Step['do'][] = [
  'mark',
  'record',
  'shot',
  'traceStart',
  'traceStop',
  'timelapse',
];

export function parseStep(where: string, v: unknown): Step {
  const kind = (v as Obj | null)?.do;
  if (typeof kind !== 'string' || !(kind in STEP_FIELDS)) {
    throw new ViewError(
      `${where}: "do" must be one of ${Object.keys(STEP_FIELDS).join(', ')} (got ${JSON.stringify(kind)})`,
    );
  }
  const d = kind as Step['do'];
  const o = obj(where, v, ['do', ...STEP_FIELDS[d]]);
  const at = (k: string): string => `${where}.${k}`;
  switch (d) {
    case 'wait':
      return { do: d, s: num(at('s'), o.s, 0, 3600) };
    case 'key':
      return { do: d, code: str(at('code'), o.code), down: bool(at('down'), o.down) };
    case 'hold':
      return { do: d, code: str(at('code'), o.code), s: num(at('s'), o.s, 0, 3600) };
    case 'look':
      return {
        do: d,
        yaw: num(at('yaw'), o.yaw, -360, 360),
        pitch: num(at('pitch'), o.pitch, -90, 90),
      };
    case 'fly':
      return { do: d, on: bool(at('on'), o.on) };
    case 'flySpeed':
      return { do: d, level: num(at('level'), o.level, 0, 63) };
    case 'camera':
      return {
        do: d,
        position: vec3(at('position'), o.position),
        yaw: num(at('yaw'), o.yaw, -360, 360),
        pitch: num(at('pitch'), o.pitch, -90, 90),
      };
    case 'edit':
      if (o.action !== 'break' && o.action !== 'place') {
        throw new ViewError(`${at('action')}: expected "break" or "place"`);
      }
      return { do: d, action: o.action };
    case 'select':
      return { do: d, slot: num(at('slot'), o.slot, 0, 63) };
    case 'settle':
      return { do: d, maxS: num(at('maxS'), o.maxS, 0, 3600) };
    case 'record':
    case 'shot':
    case 'traceStop':
      return { do: d, name: name(at('name'), o.name) };
    case 'repeat': {
      if (!Array.isArray(o.steps) || o.steps.length === 0) {
        throw new ViewError(`${at('steps')}: expected a non-empty list`);
      }
      const steps = o.steps.map((s, i) => parseStep(`${at('steps')}[${String(i)}]`, s));
      for (const [i, s] of steps.entries()) {
        if (NOT_REPEATED.includes(s.do)) {
          throw new ViewError(
            `${at('steps')}[${String(i)}]: "${s.do}" can't be repeated (it names an output or marks the run)`,
          );
        }
      }
      const times = num(at('times'), o.times, 1, 10000);
      if (!Number.isInteger(times)) throw new ViewError(`${at('times')}: expected a whole number`);
      return { do: d, times, steps };
    }
    case 'releaseCamera':
    case 'mark':
    case 'traceStart':
      return { do: d };
    case 'time': {
      const t = num(at('t'), o.t, 0, 1);
      return o.moon === undefined
        ? { do: d, t }
        : { do: d, t, moon: num(at('moon'), o.moon, 0, 1) };
    }
    case 'timelapse':
      return {
        do: d,
        name: name(at('name'), o.name),
        frames: Math.round(num(at('frames'), o.frames, 2, 1000)),
        from: num(at('from'), o.from, 0, 1, 0),
        to: num(at('to'), o.to, 0, 1, 1),
        waitS: num(at('waitS'), o.waitS, 0, 60, 0.3),
      };
  }
}

function split(where: string, v: unknown): Split | null {
  if (v === undefined) return null;
  if (v !== 'near' && v !== 'far') throw new ViewError(`${where}: expected "near" or "far"`);
  return v;
}

function parseScenario(where: string, v: unknown): Scenario {
  const o = obj(where, v, [
    'name',
    'notes',
    'seed',
    'window',
    'settings',
    'debug',
    'scale',
    'renderer',
    'split',
    'steps',
  ]);
  const n = name(`${where}.name`, o.name);
  const w = o.window === undefined ? {} : obj(`${where}.window`, o.window, ['width', 'height']);
  const settings = (
    o.settings === undefined
      ? {}
      : obj(`${where}.settings`, o.settings, ['fog', 'detail', 'exposure'])
  ) as View['settings'];
  if (!Array.isArray(o.steps) || o.steps.length === 0) {
    throw new ViewError(`${where}.steps: expected a non-empty list`);
  }
  const steps = o.steps.map((s, i) => parseStep(`${where}.steps[${String(i)}]`, s));
  let tracing = false;
  for (const [i, s] of steps.entries()) {
    if (s.do === 'traceStart' && tracing)
      throw new ViewError(`${where}.steps[${String(i)}]: already tracing`);
    if (s.do === 'traceStop' && !tracing)
      throw new ViewError(`${where}.steps[${String(i)}]: not tracing`);
    if (s.do === 'traceStart') tracing = true;
    if (s.do === 'traceStop') tracing = false;
  }
  return {
    name: n,
    notes: typeof o.notes === 'string' ? o.notes : '',
    seed: num(`${where}.seed`, o.seed, 0, 2 ** 32 - 1, 7),
    window: {
      width: num(`${where}.window.width`, w.width, 320, 4096, 1280),
      height: num(`${where}.window.height`, w.height, 240, 4096, 720),
    },
    settings,
    debug: bool(`${where}.debug`, o.debug, false),
    scale: num(`${where}.scale`, o.scale, 0.25, 2, 1),
    renderer: o.renderer === undefined ? null : name(`${where}.renderer`, o.renderer),
    split: split(`${where}.split`, o.split),
    steps,
  };
}

/** The scenarios of a scenarios file (`{ "scenarios": [...] }`), checked; throws ViewError. */
export function parseScenarios(json: unknown): Scenario[] {
  const file = obj('scenarios file', json, ['$comment', 'scenarios']);
  if (!Array.isArray(file.scenarios))
    throw new ViewError('scenarios file: expected "scenarios": [...]');
  const list = file.scenarios.map((s, i) => parseScenario(`scenarios[${String(i)}]`, s));
  const seen = new Set<string>();
  for (const s of list) {
    if (seen.has(s.name)) throw new ViewError(`scenarios["${s.name}"]: the name is used twice`);
    seen.add(s.name);
  }
  return list;
}

/** The times of day a timelapse shows, `frames` of them from `from` toward `to` (not reaching it). */
export function timelapseTimes(s: { from: number; to: number; frames: number }): number[] {
  const span = s.to > s.from ? s.to - s.from : s.to + 1 - s.from;
  return Array.from({ length: s.frames }, (_, i) => (s.from + (span * i) / s.frames) % 1);
}

/**
 * The scenario's page address (views.ts's `viewUrl`) with its renderer, or `params`' (a
 * `bench --compare` configuration's: `renderer`, `scale`, `upscaler`) in its place.
 */
export function scenarioUrl(
  base: string,
  s: Scenario,
  params: Record<string, string> = {},
): string {
  const url = new URL(viewUrl(base, scenarioView(s)), 'http://x');
  if (s.renderer !== null) url.searchParams.set('renderer', s.renderer);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  return `${base}${url.search}`;
}

/** The scenario as a view (its page address and stored settings come from views.ts). */
export function scenarioView(s: Scenario): View {
  return {
    name: s.name,
    notes: s.notes,
    seed: s.seed,
    camera: { position: null, yaw: 0, pitch: 0 },
    window: s.window,
    settings: s.settings,
    debug: s.debug,
    lodColors: false,
    scale: s.scale,
    settleS: 60,
  };
}
