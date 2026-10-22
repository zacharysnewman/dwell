// Which world local mode generates (ARCHITECTURE.md §6.3): `?world=terrain|flat|sandbox` picks
// the generator (procedural terrain by default), `?seed=N` the world seed and `?gen=` its generation
// settings (WORLD_GENERATION.md §9).
import { canonicalWorldgen } from '../world/worldgenSettings';

/**
 * Generator versions by world type, as the Rust side numbers them (`dwell_world::GENERATOR_*`).
 * The sandbox (the flat world in its indestructible white floor block, for building structures) is
 * numbered away from the terrain's versions, which grow.
 */
export const GENERATORS = { flat: 0, terrain: 8, sandbox: 100 } as const;
/** The retired movement playground (now a test structure, Phase 15): old worlds only. */
export const RETIRED_PLAYGROUND = 1;

/**
 * File name (in `dwell/worlds/`) of the local world for a generator and seed, and for generation
 * settings other than the defaults (`-w` and their encoding's FNV-1a 32 hash).
 */
export function localWorldName(generatorVersion: number, worldSeed: number, worldgen = ''): string {
  const base = `local-g${String(generatorVersion)}-s${String(worldSeed)}`;
  return worldgen ? `${base}-w${fnv32(worldgen).toString(16).padStart(8, '0')}` : base;
}

function fnv32(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

export interface LocalWorld {
  worldSeed: number;
  generatorVersion: number;
  /**
   * A new world's generation settings, encoded (`world/worldgenSettings.ts`; a saved world keeps
   * its own). Links carry them as `?gen=`.
   */
  worldgen?: string;
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
}

export function parseLocalWorld(search: string): LocalWorld {
  const params = new URLSearchParams(search);
  const world = params.get('world') ?? 'terrain';
  const generatorVersion =
    world in GENERATORS ? GENERATORS[world as keyof typeof GENERATORS] : GENERATORS.terrain;
  // Seeds are u64 on the wire; local mode accepts integers exactly representable in a double.
  const seed = Number(params.get('seed') ?? '0');
  const worldSeed = Number.isSafeInteger(seed) && seed >= 0 ? seed : 0;
  // `?gen=`: generation settings (unreadable ones are dropped: the defaults).
  const worldgen = canonicalWorldgen(params.get('gen') ?? '') ?? '';
  const gen = worldgen ? { worldgen } : {};
  const name = localWorldName(generatorVersion, worldSeed, worldgen);
  const spawn = params.get('spawn');
  if (spawn === 'portal' || spawn === 'portal-b') {
    const file = `${name}-${spawn}`;
    const faceB = spawn === 'portal-b' ? { spawnFaceB: true } : {};
    return { worldSeed, generatorVersion, ...gen, file, ...faceB, spawnPortal: true };
  }
  if (spawn === 'b') {
    const file = `${name}-faceb`;
    return { worldSeed, generatorVersion, ...gen, file, spawnFaceB: true };
  }
  return worldgen
    ? { worldSeed, generatorVersion, worldgen, file: name }
    : { worldSeed, generatorVersion };
}
