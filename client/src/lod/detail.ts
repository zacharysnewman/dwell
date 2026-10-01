// Detail settings (ARCHITECTURE.md §6.6), in the settings menu and kept in the browser:
// - the full-detail distance: how far the terrain is drawn as chunks (every block) before the
//   LOD's level 1 takes over. At least the streamed view (always loaded, for collision); at most
//   the radius the server streams requested chunks in;
// - the LOD's pixel error (CSS pixels): the finest the distant view refines to;
// - the LOD's memory (LOD_CACHE_MB): the view coarsens while its sections need more than this.
import { CHUNK_SIZE, Lod, World } from '../protocol/constants.gen';

export interface DetailSettings {
  /** Chunks are drawn out to this distance from the camera (m). */
  distanceM: number;
  /** Distant sections refine while their cells project larger than this (CSS pixels). */
  pixelError: number;
  /** The LOD's cache budget (MB): the pixel error is raised while the view needs more. */
  memoryMb: number;
}

export const DETAIL_LIMITS = {
  distanceM: {
    min: World.viewRadiusChunks * CHUNK_SIZE,
    max: (World.renderRadiusChunks - 1) * CHUNK_SIZE,
  },
  pixelError: { min: 1, max: 16 },
  memoryMb: { min: 32, max: 1024 },
} as const;

/**
 * The ranges on this device: on phones the distant view's memory stops at MOBILE_MEMORY_MAX_MB,
 * as mobile Safari closes a tab that uses too much (more was a crash, not more detail).
 */
export const MOBILE_MEMORY_MAX_MB = 192;
export type DetailLimits = { [K in keyof DetailSettings]: { min: number; max: number } };
export function detailLimits(mobile: boolean): DetailLimits {
  return mobile
    ? { ...DETAIL_LIMITS, memoryMb: { min: DETAIL_LIMITS.memoryMb.min, max: MOBILE_MEMORY_MAX_MB } }
    : DETAIL_LIMITS;
}

/** Defaults: phones hold fewer chunks and sections (memory), desktops more. */
export function defaultDetail(mobile: boolean): DetailSettings {
  return mobile
    ? { distanceM: 128, pixelError: Lod.pixelErrorMobile, memoryMb: Lod.cacheMbMobile }
    : { distanceM: 256, pixelError: Lod.pixelErrorDesktop, memoryMb: Lod.cacheMbDesktop };
}

/** Clamps stored settings into range; anything missing or not a number takes the default. */
export function sanitizeDetail(
  value: unknown,
  fallback: DetailSettings,
  limits: DetailLimits = DETAIL_LIMITS,
): DetailSettings {
  const stored = value as Partial<Record<keyof DetailSettings, unknown>> | null;
  const field = (key: keyof DetailSettings): number => {
    const x = stored?.[key];
    const { min, max } = limits[key];
    return typeof x === 'number' && Number.isFinite(x)
      ? Math.min(max, Math.max(min, x))
      : fallback[key];
  };
  return {
    distanceM: field('distanceM'),
    pixelError: field('pixelError'),
    memoryMb: field('memoryMb'),
  };
}
