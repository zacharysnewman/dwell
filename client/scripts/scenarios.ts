// Scripted scenarios for the dev bench (docs/DEV_TOOLS.md, client/bench/scenarios.json): a local
// world, a window and settings, then steps from a small fixed vocabulary that drive the game
// through `window.__dwell` and record what it reports. scripts/bench.ts runs them in a real browser;
// adding a benchmark or a repro is a new entry in the data file. Pure: no Node or browser imports.

import { ViewError, type View } from './views.ts';

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
  | { do: 'traceStop'; name: string };

export interface Scenario {
  name: string;
  notes: string;
  seed: number;
  window: { width: number; height: number };
  settings: View['settings'];
  debug: boolean;
  scale: number;
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
};

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
    case 'releaseCamera':
    case 'mark':
    case 'traceStart':
      return { do: d };
  }
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
