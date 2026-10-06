// Which builds may open a local world (RELEASES.md §6): a world records the app version that last
// played it and opens only in a build on that version's compatibility line, never an older one.
import { canOpenWorld, compatibilityLine } from '../version/semver';
import { isVersioned, type WorldMeta } from './worldIndex';

export type WorldAccess = { ok: true } | { ok: false; legacy: boolean; message: string };

export function worldAccess(world: WorldMeta, build: string): WorldAccess {
  if (!isVersioned(world)) {
    return {
      ok: false,
      legacy: true,
      message: `"${world.name}" was saved before versioned releases and can't be opened. Delete it to free its space.`,
    };
  }
  if (canOpenWorld(build, world.appVersion)) return { ok: true };
  const sameLine = compatibilityLine(build) === compatibilityLine(world.appVersion);
  return {
    ok: false,
    legacy: false,
    message: sameLine
      ? `"${world.name}" was last saved by Dwell ${world.appVersion}, which is newer than this build (${build}).`
      : `"${world.name}" was saved by Dwell ${world.appVersion} and can't be opened in this build (${build}); a build on its line is needed.`,
  };
}
