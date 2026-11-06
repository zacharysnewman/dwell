// Finding places for the dev tools (docs/DEV_TOOLS.md "Finding places"): `__dwell.find` and
// `__dwell.goto` over the client's search (crates/dwell-client/src/find.rs, `Client.find` and
// `Client.viewFound`) and the local world's map search and teleport (local.rs `find_on_map`,
// `teleport_local_player`). Scenarios and console sessions find their scene by what is there
// (the nearest water deep and wide enough, running water looked down along) instead of by
// coordinates that go stale when the terrain changes; a goto that finds nothing throws, so a run
// never measures the wrong scene. The host is the page's (main.ts) or a test's fake.

export type Vec3 = [number, number, number];

/** What `find` found (`Client.find`). */
export interface FoundPlace {
  cell: Vec3;
  /** Its surface over the cell's centre (world m). */
  at: Vec3;
  surfaceY: number;
  /** 1 face A, −1 face B. */
  face: number;
  /** Its state's canonical name. */
  block: string;
  /** How deep the matching cells under its surface go (m). */
  depth: number;
  /** The matching surface cells joined to it at its row (capped at 4096). */
  area: number;
  distance: number;
  /** The current in its column and the mean of the moving ones within 3 columns (m/s x, z). */
  current: [number, number];
  flow: [number, number];
}

export type FaceChoice = 'a' | 'b' | 'any';

export interface FindOptions {
  /** Where to search from (world m; default the player's feet). */
  from?: Vec3;
  /** How far (m; default 256, about the full-detail chunks held). */
  radius?: number;
  /** Open above (default true): a fluid's top under air, a block's top in the open. */
  surface?: boolean;
  minDepth?: number;
  minArea?: number;
  minCurrent?: number;
  /** Default: the face the search starts on. */
  face?: FaceChoice;
}

export type Aim = 'downstream' | 'upstream' | 'across';
export const AIMS: readonly Aim[] = ['downstream', 'upstream', 'across'];

/** The world map's kinds of water (local.rs `find_on_map`). */
export type MapWater = 'water' | 'sea' | 'river' | 'lake';
export const MAP_WATERS: readonly MapWater[] = ['water', 'sea', 'river', 'lake'];

export interface GotoOptions extends FindOptions {
  /** The camera's height over the surface along its face's up (m; default 1.6). */
  height?: number;
  /** Look along the flow there (`downstream`), against it or across it. */
  aim?: Aim;
  /** Or a heading (degrees, yaw 0 = +Z, 90 = +X: the player's look and `placeCamera`'s). */
  yaw?: number;
  /** Or toward a world point (pitched toward it unless `pitch` is given). */
  toward?: Vec3;
  /**
   * Or toward the nearest place matching this query from the place found (e.g. `dwell:sand`: the
   * shore from the sea), within `radius` (default 256 m), pitched toward it unless `pitch` is given.
   */
  lookAt?: string;
  /** Degrees added to the heading however it was chosen (180: away from `lookAt`'s shore). */
  turn?: number;
  /** Degrees toward the face's up (default 0, or toward `toward`). */
  pitch?: number;
  /** Move the player there too, flying (a local world): the full-detail chunks stream round it. */
  move?: boolean;
  /**
   * `loaded` (default): the chunks the client holds. `world`: first the world map's nearest
   * `water` (its columns, loaded or not; a local world), the player moved there, then its cells.
   */
  scope?: 'loaded' | 'world';
  /** With `scope: 'world'`: the map's kind of water (default `water`, any). */
  water?: MapWater;
  /** Keep trying this long (s) for chunks to stream and currents to arrive (default 0; 60 world). */
  maxS?: number;
}

export interface Camera {
  position: Vec3;
  yaw: number;
  pitch: number;
}

/** A map search's answer (local.rs `find_on_map`). */
export interface MapHit {
  at: Vec3;
  kind: string;
  face: 'a' | 'b';
  ground: number;
  distance: number;
}

export interface GotoResult {
  found: FoundPlace;
  camera: Camera;
  /** With `scope: 'world'`: the map's water it went to first. */
  map?: MapHit;
}

/** What the search needs from the page. */
export interface FindHost {
  /** The player's feet (world m), or null before playing. */
  feet(): Vec3 | null;
  /**
   * `Client.find`: `face` 1 A, −1 B, 0 the start's, 2 either; `keep` keeps it for `viewFound`.
   * Throws for an unknown block.
   */
  find(
    query: string,
    from: Vec3,
    radius: number,
    surface: boolean,
    minDepth: number,
    minArea: number,
    minCurrent: number,
    face: number,
    keep: boolean,
  ): FoundPlace | null;
  /** `Client.viewFound`; throws when it can't aim (still water downstream). */
  viewFound(
    height: number,
    aim: string,
    yaw: number,
    toward: Vec3,
    pitch: number,
    turn: number,
  ): Camera;
  fly(on: boolean): void;
  /** Moves the local player's feet there; false outside a local world. */
  teleport(feet: Vec3): boolean;
  /** The world map's nearest water (local.rs `find_on_map` JSON), or null outside a local world. */
  findOnMap(
    kind: MapWater,
    from: Vec3,
    radius: number,
    face: number,
    minDepth: number,
  ): Promise<string | null>;
  wait(ms: number): Promise<void>;
}

const DEFAULT_RADIUS = 256;
const DEFAULT_HEIGHT = 1.6;
const MAP_RADIUS = 30_000;
const WORLD_MAX_S = 60;
const POLL_MS = 250;

function faceCode(face: FaceChoice | undefined): number {
  return face === 'a' ? 1 : face === 'b' ? -1 : face === 'any' ? 2 : 0;
}

function describe(query: string, o: FindOptions): string {
  const parts = [`"${query}"`];
  if (o.minDepth) parts.push(`≥ ${String(o.minDepth)} m deep`);
  if (o.minArea && o.minArea > 1) parts.push(`≥ ${String(o.minArea)} cells wide`);
  if (o.minCurrent) parts.push(`running ≥ ${String(o.minCurrent)} m/s`);
  if (o.surface === false) parts.push('(buried too)');
  return parts.join(' ');
}

const fmt = (p: Vec3): string => `[${p.map((v) => v.toFixed(1)).join(', ')}]`;

/**
 * The nearest loaded place matching `query`, or null (kept for `viewFound` unless `keep` is
 * false). Throws for a query naming no block.
 */
export function findPlace(
  host: FindHost,
  query: string,
  o: FindOptions = {},
  keep = true,
): FoundPlace | null {
  const from = o.from ?? host.feet();
  if (!from) return null;
  return host.find(
    query,
    from,
    o.radius ?? DEFAULT_RADIUS,
    o.surface ?? true,
    o.minDepth ?? 0,
    o.minArea ?? 1,
    o.minCurrent ?? 0,
    faceCode(o.face),
    keep,
  );
}

/** The point `height` m over `p` along face `face`'s up (1 A, −1 B). */
function over(p: Vec3, face: number, height: number): Vec3 {
  return [p[0], p[1] + (face < 0 ? -height : height), p[2]];
}

/**
 * Finds `query` (as `findPlace`), optionally the world map's water first (`scope: 'world'`),
 * moves the player there if asked and places the camera over it, aimed. Throws with what was
 * looked for and where when nothing is found within `maxS`, so a scenario fails loudly.
 */
export async function gotoPlace(
  host: FindHost,
  query: string,
  o: GotoOptions = {},
): Promise<GotoResult> {
  const world = o.scope === 'world';
  const maxMs = (o.maxS ?? (world ? WORLD_MAX_S : 0)) * 1000;
  const height = o.height ?? DEFAULT_HEIGHT;
  let from = o.from ?? host.feet();
  if (!from) throw new Error('goto: not playing yet');
  let map: MapHit | undefined;
  if (world) {
    const kind = o.water ?? 'water';
    const json = await host.findOnMap(
      kind,
      from,
      o.radius ?? MAP_RADIUS,
      faceCode(o.face),
      o.minDepth ?? 0,
    );
    if (json === null) throw new Error('goto: the world scope needs a local world');
    const hit = JSON.parse(json) as MapHit | { error: string } | null;
    if (hit === null) {
      throw new Error(
        `goto: no ${kind} on the world map within ${String(o.radius ?? MAP_RADIUS)} m of ${fmt(from)}`,
      );
    }
    if ('error' in hit) throw new Error(`goto: ${hit.error}`);
    map = hit;
    from = hit.at;
  }
  const findOptions: FindOptions = {
    ...o,
    from,
    // On the map's water, its cells are near: the default radius from there.
    radius: world ? DEFAULT_RADIUS : (o.radius ?? DEFAULT_RADIUS),
  };
  const deadline = Date.now() + maxMs;
  const hold = map ? over(map.at, map.face === 'b' ? -1 : 1, Math.max(height, 2)) : null;
  if (hold) host.fly(true);
  const search = (): FoundPlace | null => {
    // Held over the map's water (flight engages a tick late) while its chunks stream in.
    if (hold && !host.teleport(hold)) throw new Error('goto: could not move the player');
    return findPlace(host, query, findOptions);
  };
  let found = search();
  while (!found && Date.now() < deadline) {
    await host.wait(POLL_MS);
    found = search();
  }
  if (!found) {
    const where = map ? `the map's ${map.kind} at ${fmt(map.at)}` : fmt(from);
    throw new Error(
      `goto: no ${describe(query, o)} within ${String(findOptions.radius)} m of ${where}` +
        (maxMs > 0 ? ` after ${String(maxMs / 1000)} s` : ''),
    );
  }
  if (o.move || world) {
    host.fly(true);
    if (!host.teleport(over(found.at, found.face, height))) {
      throw new Error('goto: could not move the player (not a local world)');
    }
  }
  // What to look at: a point, or the nearest `lookAt` from the place (not kept: the place is).
  const target = (): Vec3 | undefined => {
    if (!o.lookAt || !found) return o.toward;
    const t = findPlace(
      host,
      o.lookAt,
      { from: found.at, radius: DEFAULT_RADIUS, face: 'any' },
      false,
    );
    if (!t) {
      throw new Error(
        `goto: no "${o.lookAt}" to look at within ${String(DEFAULT_RADIUS)} m of ${fmt(found.at)}`,
      );
    }
    return t.at;
  };
  const toward = target();
  const pitch = o.pitch ?? (toward ? NaN : 0);
  const aim = o.aim ?? (toward ? 'toward' : 'yaw');
  for (;;) {
    try {
      const camera = host.viewFound(
        height,
        aim,
        o.yaw ?? 0,
        toward ?? [0, 0, 0],
        pitch,
        o.turn ?? 0,
      );
      return map ? { found, camera, map } : { found, camera };
    } catch (e) {
      // Currents arrive a few seconds into a session: keep trying until the deadline.
      if (Date.now() >= deadline) {
        throw new Error(`goto: ${e instanceof Error ? e.message : String(e)}`, { cause: e });
      }
      await host.wait(POLL_MS);
      const again = findPlace(host, query, findOptions);
      if (again) found = again;
    }
  }
}
