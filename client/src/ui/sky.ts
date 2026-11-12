// A Realistic world's Sky (docs/proposals/REALISTIC_WORLD.md §8; owner 2026-11-12: "a great setting
// to have default with some presets"): the presets are shared/look/orbits.json's (skyPresets.gen.ts).
// A world whose climate reads the sky (its world file's `overrides.tectonics.climate` 1: Realistic)
// rolls its orbit from its seed at creation, in the chosen preset's ranges (`default` when none);
// every other world keeps today's sky (owner: Realistic only for now, "a good reference point to
// compare to").
import { SKY_PRESETS } from './skyPresets.gen';

export { SKY_PRESETS };

interface WorldFile {
  name?: unknown;
  overrides?: { tectonics?: { climate?: unknown } };
}

/** The names of the world files among `files` that roll an orbit (their climate reads the sky). */
export function orbitWorldsOf(files: readonly WorldFile[]): string[] {
  return files
    .filter((f) => typeof f.name === 'string' && f.overrides?.tectonics?.climate === 1)
    .map((f) => f.name as string)
    .sort();
}

/** The build's world files that roll an orbit. */
export const ORBIT_WORLDS: readonly string[] = orbitWorldsOf(
  Object.values(
    import.meta.glob<WorldFile>('../../../shared/worldgen/worlds/*.json', {
      eager: true,
      import: 'default',
    }),
  ),
);

/** `name` when it is a Sky preset, else the default (the first). */
export function skyPreset(name: string | null | undefined): string {
  return SKY_PRESETS.find((p) => p.name === name)?.name ?? SKY_PRESETS[0].name;
}

/**
 * A new world's `{ sky }` (the preset its orbit rolls in) when its worldgen rolls an orbit, nothing
 * for every other world (today's sky).
 */
export function skyOptions(
  worldgen: string | null | undefined,
  sky: string | null | undefined,
  worlds: readonly string[] = ORBIT_WORLDS,
): { sky?: string } {
  return worldgen && worlds.includes(worldgen) ? { sky: skyPreset(sky) } : {};
}
