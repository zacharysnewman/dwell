// Full-detail distance (ARCHITECTURE.md §6.6): how far the terrain is drawn as chunks (every block)
// before the LOD's level 1 takes over. At least the streamed view (always loaded, for collision);
// at most the radius the server streams requested chunks in. A setting (the settings menu).
import { CHUNK_SIZE, World } from '../protocol/constants.gen';

export interface DetailSettings {
  /** Chunks are drawn out to this distance from the camera (m). */
  distanceM: number;
}

export const DETAIL_LIMITS = {
  distanceM: {
    min: World.viewRadiusChunks * CHUNK_SIZE,
    max: (World.renderRadiusChunks - 1) * CHUNK_SIZE,
  },
} as const;

/** Defaults: phones hold fewer chunks (memory), desktops more. */
export function defaultDetail(mobile: boolean): DetailSettings {
  return { distanceM: mobile ? 128 : 256 };
}

/** Clamps stored settings into range; anything missing or not a number takes the default. */
export function sanitizeDetail(value: unknown, fallback: DetailSettings): DetailSettings {
  const x = (value as Partial<Record<keyof DetailSettings, unknown>> | null)?.distanceM;
  const { min, max } = DETAIL_LIMITS.distanceM;
  return {
    distanceM:
      typeof x === 'number' && Number.isFinite(x)
        ? Math.min(max, Math.max(min, x))
        : fallback.distanceM,
  };
}
