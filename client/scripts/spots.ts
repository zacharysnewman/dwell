// Saved camera spots for the bench (docs/DEV_TOOLS.md "Screenshots of a place"): a data file
// (client/bench/spots.json) of named places in test worlds, each an exact camera placement (`at`,
// a world point, or `above`, a height over the ground there) with its look. The bench's `shot`
// step places the camera this way and `spot` shoots a named one; `npm run bench -- --spot NAME`
// opens the spot's world and shoots it. Pure: no Node or browser imports.

import { ViewError } from './views.ts';

export type Vec3 = [number, number, number];

/**
 * Where an exact camera goes: `at`, a world point (the eye, metres), or `above`, `[x, z, height]`
 * (the eye `height` m over the top of the ground's column at x, z, along `face`'s up).
 */
export type Placement =
  | { at: Vec3; above?: undefined; yaw: number; pitch: number; face: 'a' | 'b' }
  | { above: Vec3; at?: undefined; yaw: number; pitch: number; face: 'a' | 'b' };

export interface Spot {
  name: string;
  notes: string;
  /** The local world (`?worldgen=` for a fixture, e.g. `test:starter`; `terrain` the seed's own). */
  world: string;
  seed: number;
  /** The time of day (0–1; 0.45 by default: late morning, readable light). */
  time: number;
  place: Placement;
  /**
   * For an `above` spot, the height (m, along the face's up) the page opens at, over the ground
   * there (the tour's `at=` y plus a margin); the `shot` descends from it to the ground.
   */
  openY: number | null;
}

const NAME = /^[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*$/;
const WORLD = /^(terrain|test:[a-z0-9_]+)$/;

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

function vec3(where: string, v: unknown): Vec3 {
  if (!Array.isArray(v) || v.length !== 3) throw new ViewError(`${where}: expected [x, y, z]`);
  return v.map((x, i) => num(`${where}[${String(i)}]`, x, -1e7, 1e7)) as Vec3;
}

/** The fields of a placement, for the step and spot parsers' field lists. */
export const PLACEMENT_FIELDS = ['at', 'above', 'yaw', 'pitch', 'face'] as const;

/** Whether `o` gives a placement at all (`at` or `above`). */
export function hasPlacement(o: Obj): boolean {
  return o.at !== undefined || o.above !== undefined;
}

/**
 * A placement from `o`'s `at` | `above`, `yaw`, `pitch`, `face`. The midplane is y = 0: face A is
 * above it, face B below, so an `at` whose y is on the other side of the midplane from `face` (a
 * face-B point given for face A: a sign slip) fails rather than shooting the wrong face.
 */
export function parsePlacement(where: string, o: Obj): Placement {
  if (o.at !== undefined && o.above !== undefined) {
    throw new ViewError(`${where}: give "at" or "above", not both`);
  }
  if (o.face !== undefined && o.face !== 'a' && o.face !== 'b') {
    throw new ViewError(`${where}.face: expected "a" or "b"`);
  }
  const face = o.face === 'b' ? 'b' : 'a';
  const yaw = num(`${where}.yaw`, o.yaw, -360, 360, 0);
  const pitch = num(`${where}.pitch`, o.pitch, -90, 90, 0);
  if (o.at !== undefined) {
    const at = vec3(`${where}.at`, o.at);
    if (face === 'a' ? at[1] <= 0 : at[1] >= 0) {
      throw new ViewError(
        `${where}.at: y ${String(at[1])} is on face ${face === 'a' ? 'B' : 'A'}'s side of the midplane (y 0) but face is "${face}"${o.face === undefined ? ' (the default)' : ''}`,
      );
    }
    return { at, yaw, pitch, face };
  }
  if (o.above !== undefined) {
    const above = vec3(`${where}.above`, o.above);
    num(`${where}.above[2]`, above[2], -64, 2000);
    return { above, yaw, pitch, face };
  }
  throw new ViewError(`${where}: expected "at" [x, y, z] or "above" [x, z, height]`);
}

/** The world point (feet) the spot's page opens at: `at=` (below the eye; for `above`, high up). */
export function openAt(p: Placement, openY: number | null = null): Vec3 {
  const up = p.face === 'b' ? -1 : 1;
  if (p.at) return [p.at[0], p.at[1] - 1.6 * up, p.at[2]];
  // The ground's height is not known before the chunks arrive: the player waits flying over it
  // (the `shot` step turns flight on, finds the ground and places the camera).
  return [p.above[0], (openY ?? OPEN_HEIGHT_M) * up, p.above[1]];
}

/** How high an `above` spot's page opens (m along the face's up): over the starter's peaks. */
export const OPEN_HEIGHT_M = 1200;

/** The spots of a spots file (`{ "spots": { "world:place": {...} } }`), checked. */
export function parseSpots(json: unknown): Spot[] {
  const file = obj('spots file', json, ['$comment', 'spots']);
  const spots = file.spots;
  if (typeof spots !== 'object' || spots === null || Array.isArray(spots)) {
    throw new ViewError('spots file: expected "spots": { "<world>:<place>": {...} }');
  }
  return Object.entries(spots as Obj).map(([n, v]) => {
    const where = `spots["${n}"]`;
    if (!NAME.test(n)) {
      throw new ViewError(
        `${where}: the name must be <world>:<place>, lowercase words with dashes`,
      );
    }
    const o = obj(where, v, ['notes', 'world', 'seed', 'time', 'openY', ...PLACEMENT_FIELDS]);
    if (o.openY !== undefined && o.above === undefined) {
      throw new ViewError(`${where}.openY: only an "above" spot opens over its ground`);
    }
    if (typeof o.world !== 'string' || !WORLD.test(o.world)) {
      throw new ViewError(`${where}.world: expected "terrain" or "test:<fixture>"`);
    }
    return {
      name: n,
      notes: typeof o.notes === 'string' ? o.notes : '',
      world: o.world,
      seed: num(`${where}.seed`, o.seed, 0, 2 ** 32 - 1, 0),
      time: num(`${where}.time`, o.time, 0, 1, 0.45),
      place: parsePlacement(where, o),
      openY: o.openY === undefined ? null : num(`${where}.openY`, o.openY, 1, 2000),
    };
  });
}

/** The spot named `name`, or a ViewError listing the names there are. */
export function findSpot(spots: Spot[], name: string): Spot {
  const s = spots.find((x) => x.name === name);
  if (!s) throw new ViewError(`no spot "${name}" (have ${spots.map((x) => x.name).join(', ')})`);
  return s;
}

/** The page parameters that open a spot's world at it (a `runScenario` configuration's params). */
export function spotParams(s: Spot): Record<string, string> {
  const at = openAt(s.place, s.openY);
  return {
    ...(s.world === 'terrain' ? {} : { worldgen: s.world }),
    seed: String(s.seed),
    fresh: '1',
    at: at.map((c) => String(Math.round(c * 100) / 100)).join(','),
    time: String(s.time),
  };
}

/**
 * The column scan the `shot` step's `above` runs in the page: the cell rows to read, from the
 * face's top down toward the midplane (y in rows of `cellH` m; face A's rows count up from 0, face
 * B's down from −1), and where the eye goes over the first one that is not air.
 */
export function groundRows(face: 'a' | 'b', topM: number, cellH: number): number[] {
  const n = Math.ceil(topM / cellH);
  return Array.from({ length: n }, (_, i) => (face === 'a' ? n - 1 - i : -n + i));
}

/** The eye's y over the ground cell `row` (its top along the face's up), `height` m up. */
export function eyeOver(face: 'a' | 'b', row: number, cellH: number, height: number): number {
  return face === 'a' ? (row + 1) * cellH + height : row * cellH - height;
}
