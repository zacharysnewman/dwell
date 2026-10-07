// The sky frame (BIFACIAL_WORLD.md §6, ARCHITECTURE.md §6.3): the three world directions the sky is
// built from — the day sky's zenith, the sun and the moon — which turn together about one axis. At
// angle 0 they are the static sky; π turns day and night over. Pure, so tests pin it.
import { MOON_DIRECTION_WORLD, SUN_DIRECTION, type Rgb } from './look';

export interface SkyFrame {
  dayPole: Rgb;
  sun: Rgb;
  moon: Rgb;
}

const normalize = (v: Rgb): Rgb => {
  const n = Math.hypot(v[0], v[1], v[2]);
  return [v[0] / n, v[1] / n, v[2] / n];
};

/** Horizontal, perpendicular to the sun's azimuth: the sun's elevation changes with the angle. */
export const SKY_AXIS: Rgb = normalize([SUN_DIRECTION[2], 0, -SUN_DIRECTION[0]]);

/** `v` turned by `angle` about the unit axis `k` (Rodrigues' formula, right-handed). */
function rotate(v: Rgb, k: Rgb, angle: number): Rgb {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  const dot = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
  const cross: Rgb = [
    k[1] * v[2] - k[2] * v[1],
    k[2] * v[0] - k[0] * v[2],
    k[0] * v[1] - k[1] * v[0],
  ];
  return [
    v[0] * c + cross[0] * s + k[0] * dot * (1 - c),
    v[1] * c + cross[1] * s + k[1] * dot * (1 - c),
    v[2] * c + cross[2] * s + k[2] * dot * (1 - c),
  ];
}

/** The frame turned by `angle` radians about SKY_AXIS. */
export function skyFrame(angle: number): SkyFrame {
  return {
    dayPole: rotate([0, 1, 0], SKY_AXIS, angle),
    sun: rotate(SUN_DIRECTION, SKY_AXIS, angle),
    moon: rotate(MOON_DIRECTION_WORLD, SKY_AXIS, angle),
  };
}

/** The static sky: the frame at angle 0. */
export const SKY_REST: SkyFrame = skyFrame(0);

/** The debug switches `?skyrot=<degrees>` (a fixed angle) and `?skyspin=<degrees per second>`. */
export interface SkySwitches {
  rotateDeg: number;
  spinDegPerSecond: number;
}

/** Reads the switches from a query string; anything missing or not a number is 0. */
export function skySwitches(search: string): SkySwitches {
  const params = new URLSearchParams(search);
  const read = (key: string): number => {
    const v = Number(params.get(key));
    return Number.isFinite(v) ? v : 0;
  };
  return { rotateDeg: read('skyrot'), spinDegPerSecond: read('skyspin') };
}

/**
 * The sky's angle (radians, 0 up to 2π): the settings slider's degrees plus the `?skyrot` angle
 * plus the spin accumulated over `seconds`. Client-side only (not synchronised between players).
 */
export function skyAngle(settingDeg: number, switches: SkySwitches, seconds: number): number {
  const deg = settingDeg + switches.rotateDeg + switches.spinDegPerSecond * seconds;
  return ((((deg % 360) + 360) % 360) * Math.PI) / 180;
}
