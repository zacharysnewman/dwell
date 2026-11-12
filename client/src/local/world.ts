// Which world local mode generates (ARCHITECTURE.md §6.3): `?world=terrain|flat|sandbox` picks
// the generator (procedural terrain by default) and `?seed=N` the world seed.

import { WorldSizes } from '../protocol/constants.gen';
import { isGameMode } from '../ui/modes';

/**
 * Generator versions by world type, as the Rust side numbers them (`dwell_world::GENERATOR_*`).
 * The sandbox (the flat world in its indestructible white floor block, for building structures) is
 * numbered away from the terrain's versions, which grow.
 */
export const GENERATORS = { flat: 0, terrain: 8, sandbox: 100 } as const;
/** The retired movement playground (now a test structure, Phase 15): old worlds only. */
export const RETIRED_PLAYGROUND = 1;

/** File name (in `dwell/worlds/`) of the local world for a generator and seed. */
export function localWorldName(generatorVersion: number, worldSeed: number): string {
  return `local-g${String(generatorVersion)}-s${String(worldSeed)}`;
}

export interface LocalWorld {
  worldSeed: number;
  generatorVersion: number;
  /**
   * The world file's name in `dwell/worlds/` (without `.dwellworld`); by default the one per
   * generator and seed that `?world=`/`?seed=` links open (`localWorldName`).
   */
  file?: string;
  /**
   * Dev link `?spawn=b`: a new world spawns its player on face B (BIFACIAL_WORLD.md §11), to play
   * and check face B; it is its own world file (`<name>-faceb`), so the face-A world is untouched.
   */
  spawnFaceB?: boolean;
  /**
   * Dev link `?spawn=portal` (`portal-b`: face B's side): a new world spawns its player by the
   * opening of the portal nearest the centre (BIFACIAL_WORLD.md §6), on the rim over its ladder; its
   * own world file (`<name>-portal`, `<name>-portal-b`).
   */
  spawnPortal?: boolean;
  /**
   * A spawn link (`?spawn=`) on a world file that is already there: its player starts at that
   * spawn too, not where it left the world (owner, 2026-10-27). A menu world's chosen spawn is its
   * first one only.
   */
  spawnOnOpen?: boolean;
  /**
   * Dev link `?at=x,y,z` (world metres, the feet): the world's spawn is that point and its player
   * starts there, however the world was left (docs/DEV_TOOLS.md).
   */
  spawnAt?: [number, number, number];
  /**
   * The player confirmed opening a world from an older version line (owner, 2026-11-06;
   * ui/upgradePrompt.ts): the world file opens though its line is older.
   */
  upgrade?: boolean;
  /** A new world's game mode by name (ADR 0065, the New world form's Mode); a saved world keeps its own. */
  mode?: string;
  /** A new world's Sky preset (REALISTIC_WORLD.md §8, `shared/look/orbits.json`); a saved world keeps its orbit. */
  sky?: string;
  /**
   * A bigger world's size (docs/proposals/BIG_MAP.md M2b: the New world form's World size, the dev
   * link `?size=2x`), as its radius scale; absent: today's (1×). A new world is created at it and
   * saves it in its settings; a saved world keeps its own. Its maps are built at it too.
   */
  radiusScale?: number;
}

/**
 * The named world sizes (shared/protocol/constants.json `worldSizes`, BIG_MAP.md): name and radius
 * scale, today's (1×) first.
 */
export const WORLD_SIZES: readonly { name: string; radiusScale: number }[] = WorldSizes.sizes;

/** The radius scale of the size named `name` (`2x`), or null for none of `worldSizes`. */
export function sizeScale(name: string | null | undefined): number | null {
  return WORLD_SIZES.find((s) => s.name === name)?.radiusScale ?? null;
}

/** `{ radiusScale }` for a bigger world's scale, nothing for today's (so 1× options are as before). */
export function sizeOptions(
  radiusScale: number | null | undefined,
): Pick<LocalWorld, 'radiusScale'> {
  return radiusScale !== null &&
    radiusScale !== undefined &&
    radiusScale !== 1 &&
    WORLD_SIZES.some((s) => s.radiusScale === radiusScale)
    ? { radiusScale }
    : {};
}

/** The spawns a world can start at (`?spawn=`, a new world's Spawn): "" is face A's usual one. */
export const SPAWNS = ['', 'b', 'portal', 'portal-b'] as const;
export type Spawn = (typeof SPAWNS)[number];

export function isSpawn(v: unknown): v is Spawn {
  return (SPAWNS as readonly unknown[]).includes(v);
}

/** The local world's options for a spawn. */
export function spawnOptions(spawn: Spawn): Pick<LocalWorld, 'spawnFaceB' | 'spawnPortal'> {
  return {
    ...(spawn === 'b' || spawn === 'portal-b' ? { spawnFaceB: true } : {}),
    ...(spawn === 'portal' || spawn === 'portal-b' ? { spawnPortal: true } : {}),
  };
}

/**
 * A point the dev tools' `teleport` takes (BUGS.md #84): `[x, y, z]` or `{ x, y, z }` of finite
 * numbers, or null. Anything else used to reach the worker as NaN, where the move was refused and
 * the page still answered true.
 */
export function feetOf(v: unknown): [number, number, number] | null {
  const xyz = Array.isArray(v)
    ? v.length === 3
      ? v
      : null
    : typeof v === 'object' && v !== null
      ? [(v as { x?: unknown }).x, (v as { y?: unknown }).y, (v as { z?: unknown }).z]
      : null;
  if (xyz === null || !xyz.every((n) => typeof n === 'number' && Number.isFinite(n))) return null;
  return xyz as [number, number, number];
}

/** `?at=x,y,z`: three finite numbers, or null. */
export function parseAt(value: string | null): [number, number, number] | null {
  if (value === null) return null;
  const [x, y, z, ...rest] = value.split(',').map((v) => Number(v.trim()));
  if (x === undefined || y === undefined || z === undefined || rest.length > 0) return null;
  return [x, y, z].every(Number.isFinite) ? [x, y, z] : null;
}

export function parseLocalWorld(search: string): LocalWorld {
  const params = new URLSearchParams(search);
  const world = params.get('world') ?? 'terrain';
  const generatorVersion =
    world in GENERATORS ? GENERATORS[world as keyof typeof GENERATORS] : GENERATORS.terrain;
  // Seeds are u64 on the wire; local mode accepts integers exactly representable in a double.
  const seed = Number(params.get('seed') ?? '0');
  const worldSeed = Number.isSafeInteger(seed) && seed >= 0 ? seed : 0;
  // `?mode=adventure`: the world's game mode (a link's test world; ADR 0065, 0066).
  const m = params.get('mode');
  const mode = m !== null && isGameMode(m) ? { mode: m } : {};
  // `?size=2x` (BIG_MAP.md M2b): a bigger terrain world, its own world file (`<name>-x2`) so the
  // seed's 1× world is untouched; the flat and sandbox worlds are 1× only.
  const sized =
    generatorVersion === GENERATORS.terrain ? sizeOptions(sizeScale(params.get('size'))) : {};
  const suffix = sized.radiusScale ? `-x${String(sized.radiusScale)}` : '';
  const sizedFile = suffix
    ? { file: `${localWorldName(generatorVersion, worldSeed)}${suffix}` }
    : {};
  const at = parseAt(params.get('at'));
  if (at) return { worldSeed, generatorVersion, ...sizedFile, spawnAt: at, ...mode, ...sized };
  const spawn = params.get('spawn');
  if (spawn !== '' && isSpawn(spawn)) {
    // Each spawn is its own world file, so the others are untouched.
    const file = `${localWorldName(generatorVersion, worldSeed)}-${spawn === 'b' ? 'faceb' : spawn}${suffix}`;
    return {
      worldSeed,
      generatorVersion,
      file,
      ...spawnOptions(spawn),
      spawnOnOpen: true,
      ...mode,
      ...sized,
    };
  }
  return { worldSeed, generatorVersion, ...sizedFile, ...mode, ...sized };
}
