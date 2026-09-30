// The flight speed slider (PLAYER_CONTROLLER.md §6.7): a level carried in every input frame
// (InputButtons.flySpeed). Level 0 flies at the height-based speed alone; level L flies at least
// fly.speed × 2^(L/2), up to the speed the height gives near the flight ceiling. The server and the
// client's WASM prediction apply it (C++ FlySpeedFactor); this file only mirrors it for display.
import { Players } from '../protocol/constants.gen';

export const MAX_FLY_SPEED_LEVEL = Players.flySpeedMaxLevel;
/** fly.speed in PlayerControllerConfig (m/s), for showing speeds. */
export const FLY_BASE_SPEED = 11;
/** fly.terrainSpeed: the cap below WORLD_MAX_Y, where terrain has to stream in (m/s). */
export const FLY_TERRAIN_SPEED = 400;

export function clampFlySpeedLevel(level: number): number {
  return Number.isFinite(level) ? Math.min(MAX_FLY_SPEED_LEVEL, Math.max(0, Math.round(level))) : 0;
}

/** 2^(level / 2), as C++ FlySpeedFactor. */
export function flySpeedFactor(level: number): number {
  const l = clampFlySpeedLevel(level);
  return (l % 2 === 1 ? Math.SQRT2 : 1) * 2 ** Math.floor(l / 2);
}

/** The slowest the chosen level flies (without Run), in m/s. */
export function flySpeedFloor(level: number): number {
  return FLY_BASE_SPEED * flySpeedFactor(level);
}

/** A speed for the HUD: "45 m/s", "1.2 km/s", "740 km/s", "8,160 km/s". */
export function formatSpeed(mps: number): string {
  if (mps < 1000) return `${String(Math.round(mps))} m/s`;
  const km = mps / 1000;
  if (km < 10) return `${km.toFixed(1)} km/s`;
  return `${Math.round(km).toLocaleString('en-US')} km/s`;
}

/** The slider's caption for a level. */
export function flySpeedLabel(level: number): string {
  const l = clampFlySpeedLevel(level);
  if (l === 0) return 'Normal (faster with height)';
  return `At least ${formatSpeed(flySpeedFloor(l))}`;
}

const KEY = 'dwell.flySpeed';

/** The level kept in this browser (0 if none). */
export function loadFlySpeedLevel(): number {
  try {
    return clampFlySpeedLevel(Number(localStorage.getItem(KEY) ?? '0'));
  } catch {
    return 0;
  }
}

export function saveFlySpeedLevel(level: number): void {
  try {
    localStorage.setItem(KEY, String(clampFlySpeedLevel(level)));
  } catch {
    // Not kept.
  }
}
