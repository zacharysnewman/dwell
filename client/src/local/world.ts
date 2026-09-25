// Which world local mode generates (ARCHITECTURE.md §6.3): `?world=terrain|playground|flat` picks
// the generator (procedural terrain by default) and `?seed=N` the world seed.

export const GENERATORS = { flat: 0, playground: 1, terrain: 2 } as const;

export interface LocalWorld {
  worldSeed: number;
  generatorVersion: number;
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
