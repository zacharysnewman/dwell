// Which builds may open a local world (RELEASES.md §6): a world records the app version that last
// played it and opens in a build on that version's compatibility line, never an older one. A build
// on a newer line may open it after the player confirms a warning (owner, 2026-11-06): `upgrade`.
import { canOpenWorld, canUpgradeWorld, compatibilityLine } from '../version/semver';
import { isVersioned, type WorldMeta } from './worldIndex';

export type WorldAccess =
  | { ok: true }
  | {
      ok: false;
      legacy: boolean;
      /** From an older line: it opens in this build once the player confirms (upgradePrompt.ts). */
      upgrade: { from: string } | null;
      message: string;
    };

export function worldAccess(world: WorldMeta, build: string): WorldAccess {
  if (!isVersioned(world)) {
    return {
      ok: false,
      legacy: true,
      upgrade: null,
      message: `"${world.name}" was saved before versioned releases and can't be opened. Delete it to free its space.`,
    };
  }
  if (canOpenWorld(build, world.appVersion)) return { ok: true };
  if (canUpgradeWorld(build, world.appVersion)) {
    return {
      ok: false,
      legacy: false,
      upgrade: { from: world.appVersion },
      message: `"${world.name}" was saved by Dwell ${world.appVersion}, an older version line than this build (${build}).`,
    };
  }
  const sameLine = compatibilityLine(build) === compatibilityLine(world.appVersion);
  return {
    ok: false,
    legacy: false,
    upgrade: null,
    message: sameLine
      ? `"${world.name}" was last saved by Dwell ${world.appVersion}, which is newer than this build (${build}).`
      : `"${world.name}" was saved by Dwell ${world.appVersion} and can't be opened in this build (${build}); a build on its line is needed.`,
  };
}
