// Regenerating a world (the menu's Regenerate, `?regenerate=<id>`, ui/launch.ts): the build that
// opens the world recreates it from its seed with its own generator, discarding every change.
import { recordedVersion } from '../buildInfo';
import { GENERATORS } from './world';
import type { WorldIndex, WorldMeta } from './worldIndex';

/**
 * Deletes the world's files and records it as this build's, from its seed. False when its files
 * are open elsewhere (another tab), which keeps them.
 */
export async function regenerateWorld(
  index: WorldIndex,
  world: WorldMeta,
  deleteFiles: (id: string) => Promise<boolean>,
): Promise<boolean> {
  if (!(await deleteFiles(world.id))) return false;
  // The retired playground (Phase 15) starts again as a flat world.
  const type = world.type === 'playground' ? 'flat' : world.type;
  index.put({ ...world, type, generatorVersion: GENERATORS[type], appVersion: recordedVersion() });
  return true;
}
