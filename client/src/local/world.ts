// Which world local mode generates (ARCHITECTURE.md §6.3): `?world=terrain|flat|sandbox` picks
// the generator (procedural terrain by default) and `?seed=N` the world seed.

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

export function parseLocalWorld(search: string): LocalWorld {
  const params = new URLSearchParams(search);
  const world = params.get('world') ?? 'terrain';
  const generatorVersion =
    world in GENERATORS ? GENERATORS[world as keyof typeof GENERATORS] : GENERATORS.terrain;
  // Seeds are u64 on the wire; local mode accepts integers exactly representable in a double.
  const seed = Number(params.get('seed') ?? '0');
  const worldSeed = Number.isSafeInteger(seed) && seed >= 0 ? seed : 0;
  const spawn = params.get('spawn');
  if (spawn !== '' && isSpawn(spawn)) {
    // Each spawn is its own world file, so the others are untouched.
    const file = `${localWorldName(generatorVersion, worldSeed)}-${spawn === 'b' ? 'faceb' : spawn}`;
    return { worldSeed, generatorVersion, file, ...spawnOptions(spawn), spawnOnOpen: true };
  }
  return { worldSeed, generatorVersion };
}
