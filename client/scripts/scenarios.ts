// Scripted scenarios for the dev bench (docs/DEV_TOOLS.md, client/bench/scenarios.json): a local
// world, a window and settings, then steps from a small fixed vocabulary that drive the game
// through `window.__dwell` and record what it reports. scripts/bench.ts runs them in a real browser;
// adding a benchmark or a repro is a new entry in the data file. Pure: no Node or browser imports.

import { AIMS, MAP_WATERS, type Aim, type FaceChoice, type MapWater } from '../src/debug/find.ts';
import { ViewError, viewUrl, WATER_TIER_NAMES, type View } from './views.ts';

export type Vec3 = [number, number, number];

/** One step. Times are seconds. */
export type Step =
  /** Waits. */
  | { do: 'wait'; s: number }
  /** Samples the local server's figures (`__dwell.serverStats`) once a second for `s` seconds. */
  | { do: 'server'; name: string; s: number }
  /** Presses or releases a key (`KeyboardEvent.code`), as the keyboard does. */
  | { do: 'key'; code: string; down: boolean }
  /** Holds a key for `s`, then releases it. */
  | { do: 'hold'; code: string; s: number }
  /** Turns the player's look (degrees). */
  | { do: 'look'; yaw: number; pitch: number }
  /** Creative flight on or off; its speed level. */
  | { do: 'fly'; on: boolean }
  | { do: 'lod'; on: boolean }
  | { do: 'flySpeed'; level: number }
  /** The spyglass zoomed to `step` (0 the least zoom, shared/look/spyglass.json) or out. */
  | { do: 'spyglass'; on: boolean; step?: number }
  /**
   * Places the camera (the player stays), or returns it to the player. With `aroundPlayer`, the
   * position's x and z are offsets from the player's feet (y stays absolute): a view near wherever
   * a flight ended, where the chunks are streamed (e.g. the eye just under the sea's surface).
   */
  | {
      do: 'camera';
      position: Vec3;
      yaw: number;
      pitch: number;
      aroundPlayer?: boolean;
      /** With `aroundPlayer`, y too is an offset from the feet (a height over the spawn's ground). */
      aboveFeet?: boolean;
      /**
       * x and z are offsets from the last `goto`'s found place and y a height over its surface
       * (along its face's up): views around the water a `goto` found.
       */
      aroundFound?: boolean;
    }
  /**
   * Finds a place by what is there (`__dwell.goto`, docs/DEV_TOOLS.md "Finding places"): the
   * nearest loaded `find` (a block or fluid by name, `water`, `fluid` or `land`) within `radius`
   * m of the player, at least `minDepth` m deep, `minArea` cells wide and running `minCurrent`
   * m/s, on `face`; with `scope: "world"` the world map's nearest `water` (`sea`, `river`, `lake`)
   * first, the player moved there. Places the camera `height` m over it looking `aim`
   * (`downstream`, `upstream`, `across` the current), at `yaw`, `toward` a point or at the nearest
   * `lookAt` (e.g. `land`: the shore), turned `turn` more, at `pitch`; with `move` the player goes
   * there too (flying). Tries for `maxS`; finding nothing fails the run.
   */
  | {
      do: 'goto';
      find: string;
      radius?: number;
      surface?: boolean;
      minDepth?: number;
      minArea?: number;
      minCurrent?: number;
      face?: FaceChoice;
      height?: number;
      aim?: Aim;
      yaw?: number;
      toward?: Vec3;
      lookAt?: string;
      turn?: number;
      pitch?: number;
      move?: boolean;
      scope?: 'loaded' | 'world';
      water?: MapWater;
      maxS?: number;
    }
  | { do: 'releaseCamera' }
  /** The water quality, and/or its cost-isolation switches (`?waterdbg=` names; "" for none). */
  | { do: 'water'; quality?: string; debug?: string }
  /** The Shadows setting, for this page only (docs/DAY_NIGHT.md §10). */
  | { do: 'shadows'; quality: 'off' | 'low' | 'high' }
  | {
      do: 'view';
      clouds?: 'off' | 'low' | 'high';
      speedEffects?: boolean;
      distanceM?: number;
      pixelError?: number;
      smoke?: boolean;
      flowParticles?: boolean;
      flowRipples?: boolean;
      flowFoam?: boolean;
      waterfalls?: boolean;
    }
  /** Breaks or places at the crosshair, as a click does; selects a hotbar slot. */
  | { do: 'edit'; action: 'break' | 'place' }
  | { do: 'select'; slot: number }
  /**
   * Drives the brush tool (`__dwell.brush`, docs/BRUSHES.md) as its keys and clicks do:
   * `toggle`, `preset:<name>`, `size:<n>`, `apply`, … (in worlds that allow brushes: sandbox).
   */
  | { do: 'brush'; command: string }
  /**
   * A region edit (the region tools, `__dwell.tool`) of the box between `from` and `to`
   * (inclusive), offsets in metres from the player's feet, `up` the face's up (1 face A, −1 face
   * B): `fill` with the selected hotbar block, or `clear` to air. For carving or building a scene.
   */
  | { do: 'region'; op: 'fill' | 'clear'; from: Vec3; to: Vec3; up?: number }
  /**
   * Stand-in players (`__dwell.puppets`, docs/CHARACTERS.md): `count` full bodies ahead of the
   * player (offset `at` from its feet, metres) on `face` (1 or −1), each playing a clip of its
   * own, holding the hotbar's first block when `held`; 0 removes them.
   */
  | { do: 'puppets'; count: number; at?: Vec3; face?: number; held?: boolean }
  /** Your first-person arms shown or hidden (the Show your arms setting, for this page only). */
  | { do: 'arms'; on: boolean }
  /** Waits (at most `maxS`) until the LOD has nothing to generate, mesh or ask for, for 1 s. */
  | { do: 'settle'; maxS: number }
  /**
   * Waits (at most `maxS`) until the view waits for nothing `behind` the camera (out of the view's
   * frustum: `lod.waitingOffView` 0), `nearBehind` it within the full-detail distance
   * (`waitingNearOffView` 0: the full detail behind is there) or `ahead` of it (`waitingInView`
   * 0), and saves the seconds
   * since the page was opened in the report's `untils` (the join's timeline: ARCHITECTURE.md §6.3).
   */
  | { do: 'until'; what: UntilWhat; maxS: number }
  /**
   * Kills the local player where it is, waits for its respawn and then (at most `maxS`) until it
   * is playing again, and saves the respawn → playing seconds in the report's `respawns` (the
   * respawn's loading screen, ARCHITECTURE.md §6.3).
   */
  | { do: 'die'; maxS: number }
  /**
   * Stands the player on a raft `lift` m (default 1) above its feet and moves the raft at `speed`
   * m/s toward `yaw` (degrees, 0 = +Z, 90 = +X) for `s` seconds at a held height, then waits out
   * the ride (a local world only): "carried by a moving body" bugs (`__dwell.carry`).
   */
  | { do: 'carry'; speed: number; yaw: number; s: number; lift?: number }
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
   * Turns pieces of the water's shading off (`names`, the `?waterdbg=` switches, comma-separated;
   * "" turns them all back on), so a cost can be told apart within one page and one load.
   */
  | { do: 'waterDebug'; names: string }
  /**
   * A timelapse: `frames` screenshots `<name>-00.png`… from time of day `from` to `to` (wrapping
   * past midnight when `to` < `from`), waiting `waitS` after each change.
   */
  | { do: 'timelapse'; name: string; frames: number; from: number; to: number; waitS: number }
  /**
   * A still camera's burst: `frames` screenshots `<name>-00.png`… back to back, and their
   * flicker (`compareReport.ts` `flickerOf`: the share of pixels whose brightness changes from
   * one frame to the next), saved in the report's `bursts` and failing the run over `maxFlicker`
   * (a share, 0–1). With `clock`, the time of day runs on from `clock` (0–1) at the world clock's
   * rate while it shoots, as it does in play (the bench's time of day is otherwise held still).
   */
  | {
      do: 'burst';
      name: string;
      frames: number;
      clock?: number;
      maxFlicker?: number;
      /**
       * The camera drifting while it shoots (after a `camera` step): `position` metres, `yaw` and
       * `pitch` degrees added each frame. Surfaces that fight in depth swap only as the view moves
       * (the owner: z-fighting stops when the camera does), so a still burst can't see them; a
       * drifting one counts them as `hueFlips`, failing the run over `maxHueFlips` (a share, 0–1).
       */
      drift?: { position: Vec3; yaw: number; pitch: number };
      maxHueFlips?: number;
    }
  /** Runs `steps` `times` times (e.g. a burst of edits); no `mark`, `record`, `shot` or traces inside. */
  | { do: 'repeat'; times: number; steps: Step[] };

/** What an `until` step waits for. */
export type UntilWhat = 'behind' | 'nearBehind' | 'ahead';
export const UNTIL_WHATS: readonly UntilWhat[] = ['behind', 'nearBehind', 'ahead'];

/** Which part of the view a scenario measures, for a comparison report's near/far split. */
export type Split = 'near' | 'far';

export interface Scenario {
  name: string;
  notes: string;
  seed: number;
  /** The page's CSS size; `pixelRatio` the device's (1 by default; 2 a Retina screen, shots at its pixels). */
  window: { width: number; height: number; pixelRatio?: number };
  settings: View['settings'];
  debug: boolean;
  scale: number;
  /** Near or far view, for `bench --compare`'s split; null: neither. */
  split: Split | null;
  /** The face the player spawns on (`?spawn=b`, BIFACIAL_WORLD.md §11); default `a`. */
  face: 'a' | 'b';
  /**
   * The player spawns by a portal's opening on `face` (`?spawn=portal`, `portal-b`;
   * BIFACIAL_WORLD.md §6); default false.
   */
  portal: boolean;
  /** The local world's type (`?world=`, local/world.ts `GENERATORS`); default `terrain`. */
  world: World;
  steps: Step[];
}

/** The world types a scenario may open. */
const WORLDS = ['terrain', 'flat', 'sandbox'] as const;
type World = (typeof WORLDS)[number];

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

/** A burst's per-frame camera drift: small, or it is a flight, not a drift. */
function drift(where: string, v: unknown): { position: Vec3; yaw: number; pitch: number } {
  const o = obj(where, v, ['position', 'yaw', 'pitch']);
  const position =
    o.position === undefined ? ([0, 0, 0] as Vec3) : vec3(`${where}.position`, o.position);
  for (const [i, c] of position.entries()) {
    if (Math.abs(c) > 1) throw new ViewError(`${where}.position[${String(i)}]: over 1 m a frame`);
  }
  return {
    position,
    yaw: num(`${where}.yaw`, o.yaw, -1, 1, 0),
    pitch: num(`${where}.pitch`, o.pitch, -1, 1, 0),
  };
}

const STEP_FIELDS: Record<Step['do'], readonly string[]> = {
  wait: ['s'],
  carry: ['speed', 'yaw', 's', 'lift'],
  server: ['name', 's'],
  key: ['code', 'down'],
  hold: ['code', 's'],
  look: ['yaw', 'pitch'],
  fly: ['on'],
  lod: ['on'],
  flySpeed: ['level'],
  spyglass: ['on', 'step'],
  camera: ['position', 'yaw', 'pitch', 'aroundPlayer', 'aboveFeet', 'aroundFound'],
  goto: [
    'find',
    'radius',
    'surface',
    'minDepth',
    'minArea',
    'minCurrent',
    'face',
    'height',
    'aim',
    'yaw',
    'toward',
    'lookAt',
    'turn',
    'pitch',
    'move',
    'scope',
    'water',
    'maxS',
  ],
  releaseCamera: [],
  water: ['quality', 'debug'],
  shadows: ['quality'],
  view: [
    'clouds',
    'speedEffects',
    'distanceM',
    'pixelError',
    'smoke',
    'flowParticles',
    'flowRipples',
    'flowFoam',
    'waterfalls',
  ],
  edit: ['action'],
  select: ['slot'],
  brush: ['command'],
  region: ['op', 'from', 'to', 'up'],
  puppets: ['count', 'at', 'face', 'held'],
  arms: ['on'],
  settle: ['maxS'],
  until: ['what', 'maxS'],
  die: ['maxS'],
  mark: [],
  record: ['name'],
  shot: ['name'],
  traceStart: [],
  traceStop: ['name'],
  time: ['t', 'moon'],
  waterDebug: ['names'],
  timelapse: ['name', 'frames', 'from', 'to', 'waitS'],
  burst: ['name', 'frames', 'clock', 'maxFlicker', 'drift', 'maxHueFlips'],
  repeat: ['times', 'steps'],
};

/** Steps that name an output or change the run's state, so can't run more than once in a row. */
const NOT_REPEATED: readonly Step['do'][] = [
  'mark',
  'server',
  'record',
  'shot',
  'traceStart',
  'traceStop',
  'timelapse',
  'burst',
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
    case 'server':
      return { do: d, name: name(at('name'), o.name), s: num(at('s'), o.s, 1, 3600) };
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
    case 'lod':
    case 'arms':
      return { do: d, on: bool(at('on'), o.on) };
    case 'flySpeed':
      return { do: d, level: num(at('level'), o.level, 0, 63) };
    case 'spyglass':
      return {
        do: d,
        on: bool(at('on'), o.on),
        ...(o.step === undefined ? {} : { step: num(at('step'), o.step, 0, 7) }),
      };
    case 'camera':
      if (o.aroundFound === true && (o.aroundPlayer !== undefined || o.aboveFeet !== undefined)) {
        throw new ViewError(`${at('aroundFound')}: not with aroundPlayer or aboveFeet`);
      }
      return {
        do: d,
        position: vec3(at('position'), o.position),
        yaw: num(at('yaw'), o.yaw, -360, 360),
        pitch: num(at('pitch'), o.pitch, -90, 90),
        ...(o.aroundPlayer === undefined
          ? {}
          : { aroundPlayer: bool(at('aroundPlayer'), o.aroundPlayer) }),
        ...(o.aboveFeet === undefined ? {} : { aboveFeet: bool(at('aboveFeet'), o.aboveFeet) }),
        ...(o.aroundFound === undefined
          ? {}
          : { aroundFound: bool(at('aroundFound'), o.aroundFound) }),
      };
    case 'goto':
      return parseGoto(where, o);
    case 'shadows': {
      const q = o.quality;
      if (q !== 'off' && q !== 'low' && q !== 'high') {
        throw new ViewError(`${at('quality')}: expected "off", "low" or "high"`);
      }
      return { do: d, quality: q };
    }
    case 'view': {
      const c = o.clouds;
      if (c !== undefined && c !== 'off' && c !== 'low' && c !== 'high') {
        throw new ViewError(`${at('clouds')}: expected "off", "low" or "high"`);
      }
      const opt = <T>(k: string, v: T | undefined): Record<string, T> =>
        v === undefined ? {} : { [k]: v };
      return {
        do: d,
        ...opt('clouds', c),
        ...opt(
          'speedEffects',
          o.speedEffects === undefined ? undefined : bool(at('speedEffects'), o.speedEffects),
        ),
        ...opt(
          'distanceM',
          o.distanceM === undefined ? undefined : num(at('distanceM'), o.distanceM, 16, 4096),
        ),
        ...opt(
          'pixelError',
          o.pixelError === undefined ? undefined : num(at('pixelError'), o.pixelError, 1, 16),
        ),
        ...opt('smoke', o.smoke === undefined ? undefined : bool(at('smoke'), o.smoke)),
        ...opt(
          'flowParticles',
          o.flowParticles === undefined ? undefined : bool(at('flowParticles'), o.flowParticles),
        ),
        ...opt(
          'flowRipples',
          o.flowRipples === undefined ? undefined : bool(at('flowRipples'), o.flowRipples),
        ),
        ...opt('flowFoam', o.flowFoam === undefined ? undefined : bool(at('flowFoam'), o.flowFoam)),
        ...opt(
          'waterfalls',
          o.waterfalls === undefined ? undefined : bool(at('waterfalls'), o.waterfalls),
        ),
      };
    }
    case 'water': {
      const q = o.quality;
      if (q !== undefined && (typeof q !== 'string' || !WATER_TIER_NAMES.includes(q))) {
        throw new ViewError(
          `${at('quality')}: expected ${WATER_TIER_NAMES.map((n) => `"${n}"`).join(', ')}`,
        );
      }
      // "" turns every switch off again, so text may be empty here.
      if (o.debug !== undefined && typeof o.debug !== 'string') {
        throw new ViewError(`${at('debug')}: expected text`);
      }
      return {
        do: d,
        ...(q === undefined ? {} : { quality: q }),
        ...(o.debug === undefined ? {} : { debug: o.debug }),
      };
    }
    case 'edit':
      if (o.action !== 'break' && o.action !== 'place') {
        throw new ViewError(`${at('action')}: expected "break" or "place"`);
      }
      return { do: d, action: o.action };
    case 'select':
      return { do: d, slot: num(at('slot'), o.slot, 0, 63) };
    case 'region': {
      if (o.op !== 'fill' && o.op !== 'clear')
        throw new ViewError(`${at('op')}: expected "fill" or "clear"`);
      if (o.up !== undefined && o.up !== 1 && o.up !== -1)
        throw new ViewError(`${at('up')}: expected 1 or -1`);
      return {
        do: d,
        op: o.op,
        from: vec3(at('from'), o.from),
        to: vec3(at('to'), o.to),
        ...(o.up === undefined ? {} : { up: o.up }),
      };
    }
    case 'brush':
      if (typeof o.command !== 'string' || !/^[A-Za-z]+(:[A-Za-z0-9_-]+)?$/.test(o.command)) {
        throw new ViewError(`${at('command')}: expected a brush command (e.g. "preset:cube")`);
      }
      return { do: d, command: o.command };
    case 'puppets': {
      const count = num(at('count'), o.count, 0, 256);
      if (!Number.isInteger(count)) throw new ViewError(`${at('count')}: expected a whole number`);
      if (o.face !== undefined && o.face !== 1 && o.face !== -1) {
        throw new ViewError(`${at('face')}: expected 1 or -1`);
      }
      return {
        do: d,
        count,
        ...(o.at === undefined ? {} : { at: vec3(at('at'), o.at) }),
        ...(o.face === undefined ? {} : { face: o.face }),
        ...(o.held === undefined ? {} : { held: bool(at('held'), o.held) }),
      };
    }
    case 'carry':
      return {
        do: d,
        speed: num(at('speed'), o.speed, 0, 100),
        yaw: num(at('yaw'), o.yaw, -360, 360),
        s: num(at('s'), o.s, 0, 600),
        ...(o.lift === undefined ? {} : { lift: num(at('lift'), o.lift, 0, 64) }),
      };
    case 'settle':
    case 'die':
      return { do: d, maxS: num(at('maxS'), o.maxS, 0, 3600) };
    case 'until': {
      if (!UNTIL_WHATS.includes(o.what as UntilWhat)) {
        throw new ViewError(`${at('what')}: expected one of ${UNTIL_WHATS.join(', ')}`);
      }
      return { do: d, what: o.what as UntilWhat, maxS: num(at('maxS'), o.maxS, 0, 3600) };
    }
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
    case 'waterDebug': {
      if (typeof o.names !== 'string' || !/^[a-z0-9,]*$/.test(o.names)) {
        throw new ViewError(`${at('names')}: expected switch names joined by commas`);
      }
      return { do: d, names: o.names };
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
    case 'burst':
      return {
        do: d,
        name: name(at('name'), o.name),
        frames: Math.round(num(at('frames'), o.frames, 2, 200)),
        ...(o.clock === undefined ? {} : { clock: num(at('clock'), o.clock, 0, 1) }),
        ...(o.maxFlicker === undefined
          ? {}
          : { maxFlicker: num(at('maxFlicker'), o.maxFlicker, 0, 1) }),
        ...(o.drift === undefined ? {} : { drift: drift(at('drift'), o.drift) }),
        ...(o.maxHueFlips === undefined
          ? {}
          : { maxHueFlips: num(at('maxHueFlips'), o.maxHueFlips, 0, 1) }),
      };
  }
}

const FACES: readonly FaceChoice[] = ['a', 'b', 'any'];

/** A place query: a block or state name (`dwell:sand`, `water`) or `fluid`. */
function query(where: string, v: unknown): string {
  const q = str(where, v);
  if (!/^[a-z0-9_:-]+(\[[a-z0-9_=,]*\])?$/.test(q)) {
    throw new ViewError(`${where}: expected a block name ("dwell:sand"), "water" or "fluid"`);
  }
  return q;
}
const SCOPES = ['loaded', 'world'] as const;

/** A `goto` step's fields (checked: one way to aim, known kinds, ranges). */
function parseGoto(where: string, o: Obj): Extract<Step, { do: 'goto' }> {
  const at = (k: string): string => `${where}.${k}`;
  const find = query(at('find'), o.find);
  if (o.face !== undefined && o.face !== 'a' && o.face !== 'b' && o.face !== 'any') {
    throw new ViewError(`${at('face')}: expected "a", "b" or "any"`);
  }
  if (o.aim !== undefined && !AIMS.includes(o.aim as Aim)) {
    throw new ViewError(`${at('aim')}: expected one of ${AIMS.join(', ')}`);
  }
  const ways = ['aim', 'yaw', 'toward', 'lookAt'].filter((k) => o[k] !== undefined);
  if (ways.length > 1) throw new ViewError(`${where}: aim one way (${ways.join(' or ')})`);
  if (o.scope !== undefined && o.scope !== 'loaded' && o.scope !== 'world') {
    throw new ViewError(`${at('scope')}: expected "loaded" or "world"`);
  }
  if (o.water !== undefined) {
    if (o.scope !== 'world') throw new ViewError(`${at('water')}: only with scope "world"`);
    if (!MAP_WATERS.includes(o.water as MapWater)) {
      throw new ViewError(`${at('water')}: expected one of ${MAP_WATERS.join(', ')}`);
    }
  }
  const opt = <K extends string, V>(k: K, v: V | undefined): Partial<Record<K, V>> =>
    v === undefined ? {} : ({ [k]: v } as Record<K, V>);
  const n = (k: string, min: number, max: number): number | undefined =>
    o[k] === undefined ? undefined : num(at(k), o[k], min, max);
  const minArea = n('minArea', 1, 4096);
  if (minArea !== undefined && !Number.isInteger(minArea)) {
    throw new ViewError(`${at('minArea')}: expected a whole number`);
  }
  return {
    do: 'goto',
    find,
    ...opt('radius', n('radius', 1, 100_000)),
    ...opt('surface', o.surface === undefined ? undefined : bool(at('surface'), o.surface)),
    ...opt('minDepth', n('minDepth', 0, 256)),
    ...opt('minArea', minArea),
    ...opt('minCurrent', n('minCurrent', 0, 100)),
    ...opt(
      'face',
      FACES.find((x) => x === o.face),
    ),
    ...opt('height', n('height', -64, 4096)),
    ...opt('aim', o.aim as Aim | undefined),
    ...opt('yaw', n('yaw', -360, 360)),
    ...opt('toward', o.toward === undefined ? undefined : vec3(at('toward'), o.toward)),
    ...opt('lookAt', o.lookAt === undefined ? undefined : query(at('lookAt'), o.lookAt)),
    ...opt('turn', n('turn', -360, 360)),
    ...opt('pitch', n('pitch', -90, 90)),
    ...opt('move', o.move === undefined ? undefined : bool(at('move'), o.move)),
    ...opt(
      'scope',
      SCOPES.find((x) => x === o.scope),
    ),
    ...opt('water', o.water as MapWater | undefined),
    ...opt('maxS', n('maxS', 0, 3600)),
  };
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
    'split',
    'face',
    'portal',
    'world',
    'steps',
  ]);
  if (o.world !== undefined && !(WORLDS as readonly unknown[]).includes(o.world)) {
    throw new ViewError(`${where}.world: expected one of ${WORLDS.join(', ')}`);
  }
  if (o.face !== undefined && o.face !== 'a' && o.face !== 'b') {
    throw new ViewError(`${where}.face: expected "a" or "b"`);
  }
  const n = name(`${where}.name`, o.name);
  const w =
    o.window === undefined
      ? {}
      : obj(`${where}.window`, o.window, ['width', 'height', 'pixelRatio']);
  const settings = (
    o.settings === undefined
      ? {}
      : obj(`${where}.settings`, o.settings, [
          'detail',
          'exposure',
          'clouds',
          'motion',
          'water',
          'shadows',
          'timeOfDay',
        ])
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
      ...(w.pixelRatio === undefined
        ? {}
        : { pixelRatio: num(`${where}.window.pixelRatio`, w.pixelRatio, 1, 4, 1) }),
    },
    settings,
    debug: bool(`${where}.debug`, o.debug, false),
    scale: num(`${where}.scale`, o.scale, 0.25, 2, 1),
    split: split(`${where}.split`, o.split),
    face: o.face === 'b' ? 'b' : 'a',
    portal: bool(`${where}.portal`, o.portal, false),
    world: (o.world as World | undefined) ?? 'terrain',
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
 * The scenario's page address (views.ts's `viewUrl`) with `params` (a `bench --compare`
 * configuration's: `scale` and its own) over it.
 */
export function scenarioUrl(
  base: string,
  s: Scenario,
  params: Record<string, string> = {},
): string {
  const url = new URL(viewUrl(base, scenarioView(s)), 'http://x');
  if (s.portal) url.searchParams.set('spawn', s.face === 'b' ? 'portal-b' : 'portal');
  else if (s.face === 'b') url.searchParams.set('spawn', 'b');
  if (s.world !== 'terrain') url.searchParams.set('world', s.world);
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
