// The game's look (WORLD_GENERATION.md §1, Phase 7): warm light, cool shadow, one sky. Everything
// the meshers, the lights and the fog must agree on lives here, as plain data (no WebGL), so the
// chunk and LOD meshers share it (they run in workers) and tests can pin it.
//
// Colours are sRGB triples in 0–1 unless a name says linear.

import { World } from '../protocol/constants.gen';
import type { SkyFrame } from './skyFrame';

export type Rgb = readonly [number, number, number];

const hex = (c: number): Rgb => [
  ((c >> 16) & 0xff) / 255,
  ((c >> 8) & 0xff) / 255,
  (c & 0xff) / 255,
];

/**
 * Per-face colour multiplier (linear light): warm on top, cooler on the sides, blue underneath.
 * Shaded faces lean blue or violet rather than just darker — the rule that does most of the work
 * (§1.2). Chunk faces and LOD faces both read this table, so a material looks the same at any
 * distance (tests pin it).
 */
const TOP: Rgb = [1.0, 0.97, 0.9];
const SIDE_X: Rgb = [0.86, 0.84, 0.88];
const SIDE_Z: Rgb = [0.74, 0.77, 0.9];
const BOTTOM: Rgb = [0.5, 0.55, 0.74];

/** The tint of the face on `axis` (0 X, 1 Y, 2 Z) looking toward `sign` (+1 or −1). */
export function faceTint(axis: number, sign: number): Rgb {
  if (axis === 1) return sign > 0 ? TOP : BOTTOM;
  return axis === 0 ? SIDE_X : SIDE_Z;
}

/**
 * The tint of a face with unit normal (nx, ny, nz): the side tints blended by the horizontal part
 * of the normal, then blended toward the top (or bottom) tint by how vertical it is. Equals
 * `faceTint` on axis-aligned normals, so sloped faces shade continuously with the cubes beside
 * them, at any distance (the chunk and LOD meshers both use it).
 */
export function normalTint(nx: number, ny: number, nz: number): Rgb {
  const x2 = nx * nx;
  const z2 = nz * nz;
  const horizontal = x2 + z2;
  const vertical = Math.min(1, Math.abs(ny));
  const end = ny >= 0 ? TOP : BOTTOM;
  const wx = horizontal > 0 ? x2 / horizontal : 0;
  const wz = horizontal > 0 ? z2 / horizontal : 0;
  return [
    (SIDE_X[0] * wx + SIDE_Z[0] * wz) * (1 - vertical) + end[0] * vertical,
    (SIDE_X[1] * wx + SIDE_Z[1] * wz) * (1 - vertical) + end[1] * vertical,
    (SIDE_X[2] * wx + SIDE_Z[2] * wz) * (1 - vertical) + end[2] * vertical,
  ];
}

/** Where the sun is (unit vector, toward the sun): ~45° up, upper left in the default spawn view. */
export const SUN_DIRECTION: Rgb = (() => {
  const [x, y, z] = [0.62, 0.7, 0.35];
  const n = Math.hypot(x, y, z);
  return [x / n, y / n, z / n];
})();

/**
 * Where the moon is, in the world: counter-angled to the sun, pointing the opposite way
 * (BIFACIAL_WORLD.md §6): the moon's place in the sky frame at angle 0 (`render/skyFrame.ts`).
 */
export const MOON_DIRECTION_WORLD: Rgb = [-SUN_DIRECTION[0], -SUN_DIRECTION[1], -SUN_DIRECTION[2]];
export const LIGHT = {
  sun: 0xfff1d6,
  sunIntensity: 2.2,
  hemisphereSky: 0x9cc6f0,
  /** Green bounce off the grass, not brown. */
  hemisphereGround: 0x7a8a4a,
  hemisphereIntensity: 1.9,
  /** Face B's: a cooler, dimmer moonlight, and a night sky's ambient. */
  moon: 0xb8ccff,
  moonIntensity: 0.9,
  moonHemisphereSky: 0x3a4f8c,
  moonHemisphereGround: 0x232c42,
  moonHemisphereIntensity: 1.1,
} as const;

/** The midplane's height: fragments below it belong to face B (lit by the moon only). */
export const MIDPLANE_Y = World.midplaneY;

/** Tone mapping exposure: 1 leaves the lights as set. */
export const DEFAULT_EXPOSURE = 1;
export const EXPOSURE_LIMITS = { min: 0.4, max: 2.5 } as const;

/** Clamps an exposure into range; anything missing or not a number takes the default. */
export function sanitizeExposure(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(EXPOSURE_LIMITS.max, Math.max(EXPOSURE_LIMITS.min, value))
    : DEFAULT_EXPOSURE;
}

// --- The sky ------------------------------------------------------------------------------------

/**
 * Sky gradient stops by the sine of the elevation (0 the horizon, 1 straight up): saturated
 * cerulean overhead grading to a near-white haze at the horizon (§1.3).
 */
export const SKY_STOPS: readonly { at: number; color: Rgb }[] = [
  { at: 0, color: hex(0xe5e5e1) },
  { at: 0.12, color: hex(0xb3cce6) },
  { at: 0.4, color: hex(0x91b7e6) },
  { at: 1, color: hex(0x649ada) },
];
export const HORIZON_COLOR: Rgb = hex(0xe5e5e1);

/** The warm glow around the sun: a tight core and a broad halo, in this colour. */
export const SUN_GLOW = {
  color: hex(0xf5d9a2),
  tight: 0.4,
  tightPower: 48,
  broad: 0.18,
  broadPower: 5,
};

/** Face B's night sky: a deep blue overhead grading to a dim blue-grey haze at the horizon. */
export const NIGHT_SKY_STOPS: readonly { at: number; color: Rgb }[] = [
  { at: 0, color: hex(0x3a4766) },
  { at: 0.12, color: hex(0x2b3856) },
  { at: 0.4, color: hex(0x18213f) },
  { at: 1, color: hex(0x0a1026) },
];
export const NIGHT_HORIZON_COLOR: Rgb = hex(0x3a4766);

/** The cool glow around the moon: a small bright core and a broad, faint halo. */
export const MOON_GLOW = {
  color: hex(0xcfdcff),
  tight: 0.7,
  tightPower: 220,
  broad: 0.12,
  broadPower: 8,
};

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

export function gradient(stops: readonly { at: number; color: Rgb }[], e: number): Rgb {
  let c = stops[0]?.color ?? HORIZON_COLOR;
  for (let i = 1; i < stops.length; i++) {
    const a = stops[i - 1];
    const b = stops[i];
    if (!a || !b) continue;
    if (e <= b.at) {
      c = mix(a.color, b.color, Math.max(0, (e - a.at) / (b.at - a.at)));
      break;
    }
    c = b.color;
  }
  return c;
}

/**
 * The sky sphere's gradient by the dot of a direction with the day sky's zenith (−1 the night
 * zenith, +1 the day's): the day stops above 0.12 and the night stops below −0.12 are
 * `SKY_STOPS`' and `NIGHT_SKY_STOPS`', with a twilight between.
 */
export const SKY_SPHERE_STOPS: readonly { at: number; color: Rgb }[] = [
  { at: -1, color: hex(0x0a1026) },
  { at: -0.4, color: hex(0x18213f) },
  { at: -0.12, color: hex(0x2b3856) },
  { at: 0, color: mix(hex(0x2b3856), hex(0xb3cce6), 0.5) }, // twilight
  { at: 0.12, color: hex(0xb3cce6) },
  { at: 0.4, color: hex(0x91b7e6) },
  { at: 1, color: hex(0x649ada) },
];
/** How far (in elevation sine) the viewer's horizon haze reaches up, and the twilight's width. */
export const HAZE_BAND = 0.12;
export const TWILIGHT = 0.1;

/** The static sky (`skyFrame(0)`, render/skyFrame.ts, which imports this file). */
const REST_FRAME: SkyFrame = {
  dayPole: [0, 1, 0],
  sun: SUN_DIRECTION,
  moon: MOON_DIRECTION_WORLD,
};

const smoothstep = (a: number, b: number, x: number): number => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};
const glow = (g: typeof SUN_GLOW, x: number): number =>
  g.tight * x ** g.tightPower + g.broad * x ** g.broadPower;

/**
 * The sky's colour (sRGB, 0–1) toward the unit world direction `dir` for a viewer on `face` (+1 A,
 * −1 B): one sphere graded from day to night by the dot with the frame's day zenith, with a haze
 * at the viewer's horizon (the day's or the night's by how far up the viewer's up points into the
 * day sky) and the sun's and moon's glows. Below the horizon it is the horizon's colour. The sky
 * drawn behind the world and the haze that distant terrain fades into are both this function
 * (`SKY_GLSL` is its shader twin), so far terrain dissolves into exactly the sky behind it.
 */
export function skyColor(dir: Rgb, face: 1 | -1 = 1, frame: SkyFrame = REST_FRAME): Rgb {
  const up: Rgb = [0, face, 0];
  const above = dir[0] * up[0] + dir[1] * up[1] + dir[2] * up[2];
  const e = Math.min(1, Math.max(0, above));
  let d: Rgb = dir;
  if (above <= 0) {
    const level: Rgb = [dir[0] - above * up[0], dir[1] - above * up[1], dir[2] - above * up[2]];
    const len = Math.hypot(level[0], level[1], level[2]);
    if (len >= 1e-6) d = [level[0] / len, level[1] / len, level[2] / len];
  }
  const dot = (a: Rgb, b: Rgb): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const base = gradient(SKY_SPHERE_STOPS, dot(d, frame.dayPole));
  const dayness = smoothstep(-TWILIGHT, TWILIGHT, dot(up, frame.dayPole));
  const haze = mix(NIGHT_HORIZON_COLOR, HORIZON_COLOR, dayness);
  let c = mix(base, haze, 1 - smoothstep(0, HAZE_BAND, e));
  c = mix(c, SUN_GLOW.color, Math.min(1, glow(SUN_GLOW, Math.max(0, dot(d, frame.sun)))));
  c = mix(c, MOON_GLOW.color, Math.min(1, glow(MOON_GLOW, Math.max(0, dot(d, frame.moon)))));
  return c;
}

const f = (n: number): string => n.toFixed(5);
const vec3 = (c: Rgb): string => `vec3(${f(c[0])}, ${f(c[1])}, ${f(c[2])})`;

const gl = (g: typeof SUN_GLOW, x: string): string =>
  `${f(g.tight)} * pow(${x}, ${f(g.tightPower)}) + ${f(g.broad)} * pow(${x}, ${f(g.broadPower)})`;

/**
 * `skyColor` for shaders: `vec3 dwellSky(vec3 dir)` — keep in step with the function above. The
 * viewer's face is the uniform `dwellFace` (+1 or −1), shared by the sky and the haze; the sky
 * frame's directions are the uniforms `dwellDayPole`, `dwellSun` and `dwellMoon`.
 */
export const SKY_GLSL: string = [
  'uniform float dwellFace;',
  'uniform vec3 dwellDayPole;',
  'uniform vec3 dwellSun;',
  'uniform vec3 dwellMoon;',
  'vec3 dwellSky(vec3 dir) {',
  '\tvec3 up = vec3(0.0, dwellFace, 0.0);',
  '\tfloat above = dot(dir, up);',
  '\tfloat e = clamp(above, 0.0, 1.0);',
  '\tvec3 d = dir;',
  '\tif (above <= 0.0) {',
  '\t\tvec3 level = dir - above * up;',
  '\t\tfloat len = length(level);',
  '\t\tif (len >= 1e-6) d = level / len;',
  '\t}',
  '\tfloat u = dot(d, dwellDayPole);',
  `\tvec3 c = ${vec3(SKY_SPHERE_STOPS[0]?.color ?? HORIZON_COLOR)};`,
  ...SKY_SPHERE_STOPS.slice(1).map((s, i) => {
    const a = SKY_SPHERE_STOPS[i];
    if (!a) return '';
    return `\tc = mix(c, ${vec3(s.color)}, clamp((u - ${f(a.at)}) / ${f(s.at - a.at)}, 0.0, 1.0));`;
  }),
  `\tfloat dayness = smoothstep(${f(-TWILIGHT)}, ${f(TWILIGHT)}, dot(up, dwellDayPole));`,
  `\tvec3 haze = mix(${vec3(NIGHT_HORIZON_COLOR)}, ${vec3(HORIZON_COLOR)}, dayness);`,
  `\tc = mix(c, haze, 1.0 - smoothstep(0.0, ${f(HAZE_BAND)}, e));`,
  '\tfloat ds = max(0.0, dot(d, dwellSun));',
  `\tc = mix(c, ${vec3(SUN_GLOW.color)}, min(1.0, ${gl(SUN_GLOW, 'ds')}));`,
  '\tfloat dm = max(0.0, dot(d, dwellMoon));',
  `\tc = mix(c, ${vec3(MOON_GLOW.color)}, min(1.0, ${gl(MOON_GLOW, 'dm')}));`,
  '\treturn c;',
  '}',
].join('\n');
