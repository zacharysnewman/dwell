// Scripted scenarios for the dev bench (docs/DEV_TOOLS.md, client/bench/scenarios.json): a local
// world, a window and settings, then steps from a small fixed vocabulary that drive the game
// through `window.__dwell` and record what it reports. scripts/bench.ts runs them in a real browser;
// adding a benchmark or a repro is a new entry in the data file. Pure: no Node or browser imports.

import { AIMS, MAP_WATERS, type Aim, type FaceChoice, type MapWater } from '../src/debug/find.ts';
import { HOTBAR_SLOTS } from '../src/ui/inventory.gen.ts';
import {
  hasPlacement,
  parsePlacement,
  PLACEMENT_FIELDS,
  type Placement,
  type Spot,
} from './spots.ts';
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
  /** The camera placed 6 m from the creature nearest the player, looking at it (ADR 0073). */
  | { do: 'creatureCamera' }
  /** The water quality, and/or its cost-isolation switches (`?waterdbg=` names; "" for none). */
  | { do: 'water'; quality?: string; debug?: string }
  /** The Shadows setting, for this page only (docs/DAY_NIGHT.md §10). */
  | { do: 'shadows'; quality: 'off' | 'low' | 'high' }
  | {
      do: 'view';
      clouds?: 'off' | 'low' | 'medium' | 'high';
      speedEffects?: boolean;
      distanceM?: number;
      pixelError?: number;
      smoke?: boolean;
      flowParticles?: boolean;
      flowRipples?: boolean;
      flowFoam?: boolean;
      waterfalls?: boolean;
      shafts?: 'off' | 'low' | 'high';
      bloom?: 'off' | 'low' | 'high';
      torchShadows?: 'off' | 'low' | 'high';
      cards?: boolean;
      grass?: boolean;
      renderScale?: number;
    }
  /** Breaks or places at the crosshair, as a click does; selects a hotbar slot. */
  | { do: 'edit'; action: 'break' | 'place' }
  | { do: 'select'; slot: number }
  /**
   * Creative: puts the item named `item` (shared/items/items.json, e.g. `"dwell:torch"`) in the
   * selected hotbar slot (`__dwell.pick`), as a click in the inventory view does; fails the run if
   * it is not a creative item. Reach an item this way, not by a `select` of its old slot: the
   * hotbar holds `hotbarSlots` picks (shared/inventory/inventory.json), not the whole item list.
   */
  | { do: 'pick'; item: string }
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
  /** Gives the local player `count` of `item` (a local world that uses items; `__dwell.give`). */
  | { do: 'give'; item: string; count: number }
  /**
   * The local server's lava (`__dwell.lava`, VOLCANOES.md §6.3): `report` prints its fields,
   * `rate` [k] scales every vent's rate, `vent` [x, y, z, rateM3s, depthM] places a lake vent.
   */
  | { do: 'lava'; op: 'report' | 'rate' | 'vent' | 'ventNear'; args: number[] }
  /**
   * Stands the player on a raft `lift` m (default 1) above its feet and moves the raft at `speed`
   * m/s toward `yaw` (degrees, 0 = +Z, 90 = +X) for `s` seconds at a held height, then waits out
   * the ride (a local world only): "carried by a moving body" bugs (`__dwell.carry`).
   */
  /**
   * Points the streaming probe reports per frame (`probe.target`, `probe.control`: 0 drawn as
   * chunks, 1 loaded but not, 2 not loaded): offsets in metres from the player's feet.
   */
  | { do: 'probe'; at: Vec3[] }
  | { do: 'carry'; speed: number; yaw: number; s: number; lift?: number }
  /** Forgets the telemetry so far: the next `record` measures from here. */
  | { do: 'mark' }
  /**
   * Saves the telemetry, the controller ticks and the game's state under `name`. With a `budget`
   * (`{"<series>.<stat>": max}`, e.g. `{"frame.cpu.p95": 14}`), a value over its maximum fails the
   * scenario (`overBudget`; BUGS.md #72's phone budget while moving).
   */
  | { do: 'record'; name: string; budget?: Record<string, number> }
  /**
   * Saves a screenshot `<name>.png`. With a placement (`at` [x, y, z], the eye, or `above`
   * [x, z, height], the eye `height` m over the ground there along `face`'s up; `yaw`, `pitch`),
   * it first places the camera exactly there (the player flown to it, so its chunks stream) and
   * waits for the view to settle (the LOD idle, nothing waiting in view, `quietFrames` frames)
   * for at most `maxS`.
   */
  | { do: 'shot'; name: string; place?: Placement; quietFrames: number; maxS: number }
  /** Shoots the saved spot `spot` (client/bench/spots.json) as a placed `shot` named `name`. */
  | { do: 'spot'; spot: string; name: string }
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
  /**
   * One axe swing frame by frame (docs/DEV_TOOLS.md "Clip strip"): the level (or `vertical`)
   * chop held at `frames` clip times across it (`__dwell.holdSwing`), each shot from the `views`
   * (`fp`: your eyes; `tp-behind`, `tp-side`: a stand-in holding the same tool, seen from behind
   * and to its right, and from its right side), `<name>-<view>-00.png`…, and one contact sheet
   * `<name>-sheet.png` (a row per view). Hold the axe first (`give`, `pick`).
   */
  | {
      do: 'clip-strip';
      name: string;
      vertical: boolean;
      frames: number;
      views: ClipStripView[];
      /** The stand-in's feet from yours (m); it stands 4 m ahead of `at` + 10 m along x. */
      at: Vec3;
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
  /**
   * Extra page parameters (e.g. `{"worldgen": "test:starter", "seed": "0", "at": "x,y,z"}`, as a
   * spot's): a test world and a start point for a scenario of its own steps.
   */
  query?: Record<string, string>;
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
  probe: ['at'],
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
  creatureCamera: [],
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
    'shafts',
    'bloom',
    'cards',
    'grass',
    'renderScale',
    'torchShadows',
  ],
  edit: ['action'],
  select: ['slot'],
  pick: ['item'],
  brush: ['command'],
  region: ['op', 'from', 'to', 'up'],
  puppets: ['count', 'at', 'face', 'held'],
  arms: ['on'],
  settle: ['maxS'],
  until: ['what', 'maxS'],
  die: ['maxS'],
  give: ['item', 'count'],
  lava: ['op', 'args'],
  mark: [],
  record: ['name', 'budget'],
  shot: ['name', ...PLACEMENT_FIELDS, 'quietFrames', 'maxS'],
  spot: ['spot', 'name'],
  traceStart: [],
  traceStop: ['name'],
  time: ['t', 'moon'],
  waterDebug: ['names'],
  timelapse: ['name', 'frames', 'from', 'to', 'waitS'],
  burst: ['name', 'frames', 'clock', 'maxFlicker', 'drift', 'maxHueFlips'],
  'clip-strip': ['name', 'vertical', 'frames', 'views', 'at'],
  repeat: ['times', 'steps'],
};

/** Steps that name an output or change the run's state, so can't run more than once in a row. */
const NOT_REPEATED: readonly Step['do'][] = [
  'mark',
  'server',
  'record',
  'shot',
  'spot',
  'traceStart',
  'traceStop',
  'timelapse',
  'burst',
  'clip-strip',
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
      if (c !== undefined && c !== 'off' && c !== 'low' && c !== 'medium' && c !== 'high') {
        throw new ViewError(`${at('clouds')}: expected "off", "low", "medium" or "high"`);
      }
      const tier = (k: string): 'off' | 'low' | 'high' | undefined => {
        const v = o[k];
        if (v === undefined) return undefined;
        if (v !== 'off' && v !== 'low' && v !== 'high') {
          throw new ViewError(`${at(k)}: expected "off", "low" or "high"`);
        }
        return v;
      };
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
        ...opt('shafts', tier('shafts')),
        ...opt('bloom', tier('bloom')),
        ...opt('torchShadows', tier('torchShadows')),
        ...opt('cards', o.cards === undefined ? undefined : bool(at('cards'), o.cards)),
        ...opt('grass', o.grass === undefined ? undefined : bool(at('grass'), o.grass)),
        ...opt(
          'renderScale',
          o.renderScale === undefined ? undefined : num(at('renderScale'), o.renderScale, 0.25, 1),
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
    case 'select': {
      // Past the hotbar a number key wraps onto another slot (0.52.23's six slots moved the
      // ladder, sand, torch and axe off theirs): fail loudly instead.
      const slot = num(at('slot'), o.slot, 0, HOTBAR_SLOTS - 1);
      if (!Number.isInteger(slot)) throw new ViewError(`${at('slot')}: expected a whole number`);
      return { do: d, slot };
    }
    case 'pick':
      if (typeof o.item !== 'string' || !/^[a-z0-9_]+:[a-z0-9_]+$/.test(o.item)) {
        throw new ViewError(`${at('item')}: expected an item name (e.g. "dwell:torch")`);
      }
      return { do: d, item: o.item };
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
    case 'probe': {
      if (!Array.isArray(o.at) || o.at.length > 2) {
        throw new ViewError(`${at('at')}: expected a list of at most 2 points`);
      }
      return { do: d, at: o.at.map((p, i) => vec3(at(`at[${String(i)}]`), p)) };
    }
    case 'carry':
      return {
        do: d,
        speed: num(at('speed'), o.speed, 0, 100),
        yaw: num(at('yaw'), o.yaw, -360, 360),
        s: num(at('s'), o.s, 0, 600),
        ...(o.lift === undefined ? {} : { lift: num(at('lift'), o.lift, 0, 64) }),
      };
    case 'lava': {
      if (o.op !== 'report' && o.op !== 'rate' && o.op !== 'vent' && o.op !== 'ventNear') {
        throw new ViewError(`${at('op')}: expected "report", "rate", "vent" or "ventNear"`);
      }
      const args = o.args === undefined ? [] : o.args;
      if (!Array.isArray(args) || !args.every((a) => typeof a === 'number' && Number.isFinite(a))) {
        throw new ViewError(`${at('args')}: expected a list of numbers`);
      }
      return { do: d, op: o.op, args: args as number[] };
    }
    case 'give': {
      if (typeof o.item !== 'string' || !/^[a-z][a-z0-9_]*:[a-z][a-z0-9_]*$/.test(o.item)) {
        throw new ViewError(`${at('item')}: expected an item name (namespace:name)`);
      }
      return { do: d, item: o.item, count: num(at('count'), o.count, 1, 999) };
    }
    case 'settle':
    case 'die':
      return { do: d, maxS: num(at('maxS'), o.maxS, 0, 3600) };
    case 'until': {
      if (!UNTIL_WHATS.includes(o.what as UntilWhat)) {
        throw new ViewError(`${at('what')}: expected one of ${UNTIL_WHATS.join(', ')}`);
      }
      return { do: d, what: o.what as UntilWhat, maxS: num(at('maxS'), o.maxS, 0, 3600) };
    }
    case 'shot':
      return {
        do: d,
        name: name(at('name'), o.name),
        ...(hasPlacement(o) ? { place: parsePlacement(where, o) } : {}),
        quietFrames: num(at('quietFrames'), o.quietFrames, 0, 600, 30),
        maxS: num(at('maxS'), o.maxS, 1, 600, 90),
      };
    case 'spot': {
      const spot = str(at('spot'), o.spot);
      return { do: d, spot, name: name(at('name'), o.name ?? spot.replace(':', '-')) };
    }
    case 'record': {
      const n = name(at('name'), o.name);
      if (o.budget === undefined) return { do: d, name: n };
      if (typeof o.budget !== 'object' || o.budget === null || Array.isArray(o.budget)) {
        throw new ViewError(`${at('budget')}: expected {"<series>.<stat>": max}`);
      }
      const budget: Record<string, number> = {};
      for (const [k, v] of Object.entries(o.budget as Record<string, unknown>)) {
        const stat = k.slice(k.lastIndexOf('.') + 1);
        if (!k.includes('.') || !BUDGET_STATS.includes(stat as BudgetStat)) {
          throw new ViewError(
            `${at('budget')}.${k}: expected "<series>.<stat>" with a stat of ${BUDGET_STATS.join(', ')}`,
          );
        }
        budget[k] = num(`${at('budget')}.${k}`, v, 0, 1e9);
      }
      return { do: d, name: n, budget };
    }
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
    case 'creatureCamera':
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
    case 'clip-strip': {
      const views = o.views ?? CLIP_STRIP_VIEWS;
      if (
        !Array.isArray(views) ||
        views.length === 0 ||
        !views.every((v) => CLIP_STRIP_VIEWS.includes(v as ClipStripView))
      ) {
        throw new ViewError(`${at('views')}: expected a list of ${CLIP_STRIP_VIEWS.join(', ')}`);
      }
      const frames = num(at('frames'), o.frames, 2, 24, 8);
      if (!Number.isInteger(frames))
        throw new ViewError(`${at('frames')}: expected a whole number`);
      return {
        do: d,
        name: name(at('name'), o.name),
        vertical: o.vertical === undefined ? false : bool(at('vertical'), o.vertical),
        frames,
        views: views as ClipStripView[],
        at: o.at === undefined ? [0, 0, 0] : vec3(at('at'), o.at),
      };
    }
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
    'query',
    'steps',
  ]);
  const query: Record<string, string> = {};
  if (o.query !== undefined) {
    if (typeof o.query !== 'object' || o.query === null || Array.isArray(o.query)) {
      throw new ViewError(`${where}.query: expected an object of strings`);
    }
    for (const [k, v] of Object.entries(o.query)) {
      if (typeof v !== 'string') throw new ViewError(`${where}.query.${k}: expected a string`);
      query[k] = v;
    }
  }
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
    ...(o.query === undefined ? {} : { query }),
    steps,
  };
}

/** The scenarios of a scenarios file (`{ "scenarios": [...] }`), checked; throws ViewError. */
/** The statistics a `record` budget can bound. */
export const BUDGET_STATS = ['mean', 'p50', 'p95', 'p99', 'max'] as const;
type BudgetStat = (typeof BUDGET_STATS)[number];

/**
 * A record's budget broken: one line per series and statistic over its maximum (a series the
 * record lacks is reported too, so a renamed series can't pass silently).
 */
export function overBudget(
  frames: Partial<Record<string, Partial<Record<BudgetStat, number>>>>,
  budget: Record<string, number>,
): string[] {
  const out: string[] = [];
  for (const [k, max] of Object.entries(budget)) {
    const i = k.lastIndexOf('.');
    const series = k.slice(0, i);
    const stat = k.slice(i + 1) as BudgetStat;
    const v = frames[series]?.[stat];
    if (v === undefined) out.push(`${k}: not recorded`);
    else if (v > max) out.push(`${k} ${v.toFixed(2)} over ${String(max)}`);
  }
  return out;
}

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

/** A clip strip's views: your eyes, and a stand-in seen from behind-right and from its right. */
export type ClipStripView = 'fp' | 'tp-behind' | 'tp-side';
export const CLIP_STRIP_VIEWS: readonly ClipStripView[] = ['fp', 'tp-behind', 'tp-side'];

/** The clip times (s) a strip shows: `frames` of them from 0 to `duration`, both ends included. */
export function clipStripTimes(duration: number, frames: number): number[] {
  return Array.from({ length: frames }, (_, i) => (duration * i) / (frames - 1));
}

/**
 * The first-person clip a swing plays and its length (s), read from the characters data
 * (shared/characters/characters.json: the player's `firstPerson.clips` role → clip).
 */
export function swingClip(
  characters: unknown,
  vertical: boolean,
): { clip: string; duration: number } {
  const c = characters as {
    characters: Partial<
      Record<string, { firstPerson?: { clips?: Partial<Record<string, string>> } }>
    >;
    clips: Partial<Record<string, { duration: number }>>;
  };
  const role = vertical ? 'chop_vertical' : 'chop_level';
  const clip = c.characters.player?.firstPerson?.clips?.[role];
  const duration = clip === undefined ? undefined : c.clips[clip]?.duration;
  if (clip === undefined || duration === undefined) {
    throw new Error(`clip-strip: the player has no first-person ${role} clip`);
  }
  return { clip, duration };
}

/**
 * Where a third-person view of a stand-in (feet `feet`, facing +Z, standing still while a swing
 * is held) is shot from: behind it and to its right (its right is −X at a heading of 0), or from
 * its right side, looking at its chest; `up` is the face's (1 face A, −1 face B: mirrored).
 */
export function clipStripCamera(
  view: Exclude<ClipStripView, 'fp'>,
  feet: Vec3,
  up: number,
): { position: Vec3; yaw: number; pitch: number } {
  // Face-local offsets (right is −x), the look's yaw from them (0 = +Z, 90 = +X).
  const [dx, dy, dz] = view === 'tp-behind' ? [-1.6, 1.9, -2.6] : [-3.2, 1.4, 0.3];
  const chest = 1.2;
  const yaw = (Math.atan2(-dx, -dz) * 180) / Math.PI;
  const pitch = (Math.atan2(chest - dy, Math.hypot(dx, dz)) * 180) / Math.PI;
  return {
    position: [feet[0] + dx * up, feet[1] + dy * up, feet[2] + dz],
    yaw: up > 0 ? yaw : -yaw,
    pitch,
  };
}

/** The contact sheet's page: a row per view, a column per clip time, each frame labelled. */
export function clipStripSheet(
  title: string,
  rows: { view: string; frames: { t: number; src: string }[] }[],
): string {
  const esc = (v: string): string =>
    v.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');
  const cells = rows
    .map(
      (r) =>
        `<tr><th>${esc(r.view)}</th>${r.frames
          .map((f) => `<td><img src="${esc(f.src)}"><div>${f.t.toFixed(3)} s</div></td>`)
          .join('')}</tr>`,
    )
    .join('');
  return `<!doctype html><meta charset="utf-8"><style>body{margin:8px;font:12px sans-serif;background:#fff}td,th{padding:2px;vertical-align:top}img{width:240px;display:block}</style><h3>${esc(title)}</h3><table>${cells}</table>`;
}

/** The times of day a timelapse shows, `frames` of them from `from` toward `to` (not reaching it). */
export function timelapseTimes(s: { from: number; to: number; frames: number }): number[] {
  const span = s.to > s.from ? s.to - s.from : s.to + 1 - s.from;
  return Array.from({ length: s.frames }, (_, i) => (s.from + (span * i) / s.frames) % 1);
}

/**
 * The game page for a served `base`: a server's root (`http://localhost:4173`) → its `/dwell/`; a
 * base already naming a build's path (`…/dwell/`, a released build's `…/dwell/v/0.51.24/`) → that
 * path. The launcher's builds 404'd as `…/dwell/v/0.51.24/dwell/` (`--base`).
 */
export function buildPage(base: string): string {
  const b = base.replace(/\/+$/, '');
  return /\/dwell(\/|$)/.test(new URL(b).pathname) ? `${b}/` : `${b}/dwell/`;
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
  for (const [k, v] of Object.entries(s.query ?? {})) url.searchParams.set(k, v);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  return `${base}${url.search}`;
}

/** A scenario that opens a saved spot's world at it and shoots it (`bench --spot NAME`). */
export function spotScenario(spot: Spot): Scenario {
  const name = spot.name.replace(':', '-');
  return {
    name,
    notes: spot.notes || `the spot ${spot.name}`,
    seed: spot.seed,
    window: { width: 1280, height: 720 },
    settings: {},
    debug: false,
    scale: 1,
    split: null,
    face: spot.place.face,
    portal: false,
    world: 'terrain',
    steps: [{ do: 'spot', spot: spot.name, name }],
  };
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
