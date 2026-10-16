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

export interface LocalWorld {
  worldSeed: number;
  generatorVersion: number;
  /**
   * The world file's name in `dwell/worlds/` (without `.dwellworld`); by default the one per
   * generator and seed that `?world=`/`?seed=` links open (`localWorldName`).
   */
  file?: string;
}

export function parseLocalWorld(search: string): LocalWorld {
  const params = new URLSearchParams(search);
  const world = params.get('world') ?? 'terrain';
  const generatorVersion =
    world in GENERATORS ? GENERATORS[world as keyof typeof GENERATORS] : GENERATORS.terrain;
  // Seeds are u64 on the wire; local mode accepts integers exactly representable in a double.
  const seed = Number(params.get('seed') ?? '0');
  const worldSeed = Number.isSafeInteger(seed) && seed >= 0 ? seed : 0;
  return { worldSeed, generatorVersion };
}
