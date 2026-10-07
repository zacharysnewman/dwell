// The game's look (WORLD_GENERATION.md §1, Phase 7): warm light, cool shadow, one sky. Everything
// the meshers, the lights and the fog must agree on lives here, as plain data (no WebGL), so the
// chunk and LOD meshers share it (they run in workers) and tests can pin it.
//
// Colours are sRGB triples in 0–1 unless a name says linear.

import { World } from '../protocol/constants.gen';

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
 * (BIFACIAL_WORLD.md §6: both static for now). It lights face B, whose sky is below the disc.
 */
export const MOON_DIRECTION_WORLD: Rgb = [-SUN_DIRECTION[0], -SUN_DIRECTION[1], -SUN_DIRECTION[2]];
/**
 * The same direction in face B's local frame, where the sky is evaluated (heights mirrored, so
 * "up" is toward the sky of face B): above its horizon at the sun's elevation, opposite in azimuth.
 */
export const MOON_DIRECTION: Rgb = [-SUN_DIRECTION[0], SUN_DIRECTION[1], -SUN_DIRECTION[2]];

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

function gradient(stops: readonly { at: number; color: Rgb }[], e: number): Rgb {
  let c = stops[0]?.color ?? HORIZON_COLOR;
  for (let i = 1; i < stops.length; i++) {
    const a = stops[i - 1];
    const b = stops[i];
    if (!a || !b) continue;
    if (e <= b.at) {
      c = mix(a.color, b.color, (e - a.at) / (b.at - a.at));
      break;
    }
    c = b.color;
  }
  return c;
}

/**
 * The sky's colour (sRGB, 0–1) toward the unit world direction `dir` for a viewer on `face` (+1 A,
 * −1 B): the gradient by elevation (the horizon's colour at and below it) plus the sun's glow. On
 * face B the sky is evaluated in the face-local frame (the world mirrored: its sky hangs below the
 * disc) with the night gradient and the moon's glow. The sky drawn behind the world and the haze
 * that distant terrain fades into are both this function (`SKY_GLSL` is its shader twin), so far
 * terrain dissolves into exactly the sky behind it.
 */
export function skyColor(dir: Rgb, face: 1 | -1 = 1): Rgb {
  const night = face < 0;
  const y = night ? -dir[1] : dir[1];
  const e = Math.min(1, Math.max(0, y));
  const c = gradient(night ? NIGHT_SKY_STOPS : SKY_STOPS, e);
  const light = night ? MOON_DIRECTION : SUN_DIRECTION;
  const glowShape = night ? MOON_GLOW : SUN_GLOW;
  const d = Math.max(0, dir[0] * light[0] + y * light[1] + dir[2] * light[2]);
  const glow =
    glowShape.tight * d ** glowShape.tightPower + glowShape.broad * d ** glowShape.broadPower;
  return mix(c, glowShape.color, Math.min(1, glow));
}

const f = (n: number): string => n.toFixed(5);
const vec3 = (c: Rgb): string => `vec3(${f(c[0])}, ${f(c[1])}, ${f(c[2])})`;

function skyBranch(
  stops: readonly { at: number; color: Rgb }[],
  light: Rgb,
  glow: typeof SUN_GLOW,
): string[] {
  return [
    `\tfloat e = clamp(dir.y, 0.0, 1.0);`,
    `\tvec3 c = ${vec3(stops[0]?.color ?? HORIZON_COLOR)};`,
    ...stops.slice(1).map((s, i) => {
      const a = stops[i];
      if (!a) return '';
      return `\tc = mix(c, ${vec3(s.color)}, clamp((e - ${f(a.at)}) / ${f(s.at - a.at)}, 0.0, 1.0));`;
    }),
    `\tvec3 lit = ${vec3(light)};`,
    '\tfloat d = max(0.0, dot(dir, lit));',
    `\tfloat glow = ${f(glow.tight)} * pow(d, ${f(glow.tightPower)}) + ${f(glow.broad)} * pow(d, ${f(glow.broadPower)});`,
    `\treturn mix(c, ${vec3(glow.color)}, min(1.0, glow));`,
  ];
}

/**
 * `skyColor` for shaders: `vec3 dwellSky(vec3 dir)` — keep in step with the function above. The
 * viewer's face is the uniform `dwellFace` (+1 or −1), shared by the sky and the haze.
 */
export const SKY_GLSL: string = [
  'uniform float dwellFace;',
  'vec3 dwellSky(vec3 dir) {',
  '\tif (dwellFace < 0.0) {',
  '\t\tdir.y = -dir.y;',
  ...skyBranch(NIGHT_SKY_STOPS, MOON_DIRECTION, MOON_GLOW).map((l) => `\t${l}`),
  '\t}',
  ...skyBranch(SKY_STOPS, SUN_DIRECTION, SUN_GLOW),
  '}',
].join('\n');
