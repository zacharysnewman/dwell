import { describe, expect, it } from 'vitest';
import { LOD_VOLUME, lodCell } from '../lod/grid';
import { World } from '../protocol/constants.gen';
import { lodColor, meshSection } from '../mesh/lodMesher';
import { meshChunk, PADDED_VOLUME, paddedIndex } from '../mesh/mesher';
import {
  DEFAULT_EXPOSURE,
  EXPOSURE_LIMITS,
  faceTint,
  HORIZON_COLOR,
  LIGHT,
  MOON_DIRECTION,
  MOON_DIRECTION_WORLD,
  NIGHT_SKY_STOPS,
  SKY_GLSL,
  SKY_STOPS,
  SUN_DIRECTION,
  sanitizeExposure,
  skyColor,
  type Rgb,
} from './look';
import { srgbToLinear } from './textures';
import { patchFogShader } from './three/heightFog';

const GRASS = 4;
/** Axis and sign of the face a vertex normal points along. */
const faceOf = (n: readonly number[]): [number, number] => {
  const axis = n.findIndex((c) => c !== 0);
  return [axis, Math.sign(n[axis] ?? 0)];
};

describe('face tints (WORLD_GENERATION.md §1.4)', () => {
  const faces: [number, number][] = [
    [0, 1],
    [0, -1],
    [1, 1],
    [1, -1],
    [2, 1],
    [2, -1],
  ];

  it('is warm on top and bluest underneath', () => {
    const top = faceTint(1, 1);
    expect(top[0]).toBeGreaterThan(top[2]); // red over blue: warm
    const blueness = (c: Rgb) => c[2] / c[0];
    for (const [axis, sign] of faces) {
      if (axis === 1 && sign < 0) continue;
      expect(blueness(faceTint(1, -1))).toBeGreaterThan(blueness(faceTint(axis, sign)));
    }
    // Shade is cooler than the lit top, not merely darker.
    for (const [axis, sign] of faces) {
      if (axis === 1 && sign > 0) continue;
      expect(blueness(faceTint(axis, sign))).toBeGreaterThan(blueness(top));
    }
  });

  it('is what both meshers write, so a chunk face and an LOD face of a material match', () => {
    const chunk = meshChunk(
      (() => {
        const v = new Uint16Array(PADDED_VOLUME);
        v[paddedIndex(3, 4, 5)] = GRASS;
        return v;
      })(),
    ).opaque;
    const cells = new Uint16Array(LOD_VOLUME);
    cells[lodCell(3, 4, 5)] = GRASS;
    const lod = meshSection(cells).opaque;
    for (const [axis, sign] of faces) {
      const tint = faceTint(axis, sign);
      // The LOD colours a face by its material's top, side or bottom tile.
      const base = lodColor(GRASS, axis === 1 ? (sign > 0 ? 0 : 2) : 1);
      const baseLinear = [16, 8, 0].map((shift) => srgbToLinear((base >> shift) & 0xff));
      const find = (n: Float32Array) => {
        for (let v = 0; v < n.length / 3; v++) {
          const f = faceOf([n[v * 3] ?? 0, n[v * 3 + 1] ?? 0, n[v * 3 + 2] ?? 0]);
          if (f[0] === axis && f[1] === sign) return v;
        }
        return -1;
      };
      const c = find(chunk.normals);
      const l = find(lod.normals);
      expect(c).toBeGreaterThanOrEqual(0);
      expect(l).toBeGreaterThanOrEqual(0);
      for (let ch = 0; ch < 3; ch++) {
        // Textured chunk faces are white × tint (the texture carries the colour)…
        expect(chunk.colors[c * 3 + ch]).toBeCloseTo(tint[ch] ?? 0, 5);
        // …and LOD faces the tile's average × the same tint.
        expect(lod.colors[l * 3 + ch]).toBeCloseTo((baseLinear[ch] ?? 0) * (tint[ch] ?? 0), 5);
      }
    }
  });
});

describe('the sky (WORLD_GENERATION.md §1.4)', () => {
  const away = (y: number): Rgb => {
    // A direction with elevation sine y, pointing away from the sun's side.
    const h = Math.sqrt(1 - y * y);
    const [sx, , sz] = SUN_DIRECTION;
    const n = Math.hypot(sx, sz);
    return [(-sx / n) * h, y, (-sz / n) * h];
  };

  it("is the gradient's horizon colour at and below the horizon, away from the sun", () => {
    expect(skyColor(away(0))).toEqual(HORIZON_COLOR);
    expect(skyColor(away(-0.5))).toEqual(HORIZON_COLOR);
    // The gradient's first stop is that colour: the haze at the horizon is the sky's there.
    expect(SKY_STOPS[0]?.color).toEqual(HORIZON_COLOR);
  });

  it('grades to the zenith colour, bluer and darker than the horizon', () => {
    const zenith = skyColor([0, 1, 0]);
    // (The sun's broad halo reaches a little way overhead, so this is the stop plus a touch of glow.)
    SKY_STOPS[SKY_STOPS.length - 1]?.color.forEach((c, i) => {
      expect(Math.abs((zenith[i] ?? 0) - c)).toBeLessThan(0.03);
    });
    expect(zenith[2]).toBeGreaterThan(zenith[0]);
    expect(zenith[0]).toBeLessThan(HORIZON_COLOR[0]);
    // Monotone: each step up the sky is further from the horizon's haze.
    let last = 0;
    for (let y = 0; y <= 1; y += 0.1) {
      const d = HORIZON_COLOR[0] - skyColor(away(y))[0];
      expect(d).toBeGreaterThanOrEqual(last - 1e-9);
      last = d;
    }
  });

  it('glows warm toward the sun', () => {
    const sun = skyColor(SUN_DIRECTION);
    const same = skyColor(away(SUN_DIRECTION[1]));
    expect(sun[0]).toBeGreaterThan(same[0]);
    expect(sun[0] - sun[2]).toBeGreaterThan(same[0] - same[2]); // warmer: red over blue
  });

  it('is one function for the sky drawn and the haze: the fog shader carries the same GLSL', () => {
    const shader = { uniforms: {}, vertexShader: '', fragmentShader: '' };
    shader.fragmentShader = '#include <fog_pars_fragment>\n#include <fog_fragment>';
    patchFogShader(shader as never);
    expect(shader.fragmentShader).toContain(SKY_GLSL);
    expect(shader.fragmentShader).toContain('dwellSky(fogDir)');
    // The shader's stops are the gradient's.
    for (const { color } of SKY_STOPS) {
      expect(SKY_GLSL).toContain(color.map((c) => c.toFixed(5)).join(', '));
    }
    expect(SKY_GLSL).toContain(SUN_DIRECTION.map((c) => c.toFixed(5)).join(', '));
  });
});

describe('face B: a moonlit night sky and light (BIFACIAL_WORLD.md §6)', () => {
  const luma = (c: Rgb): number => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];

  it('puts the moon opposite the sun in the world, and above face B’s horizon in its own frame', () => {
    SUN_DIRECTION.forEach((c, i) => {
      expect(MOON_DIRECTION_WORLD[i]).toBeCloseTo(-c, 12);
    });
    // Face B's frame is the world mirrored: the moon is as high there as the sun is on face A.
    expect(MOON_DIRECTION[1]).toBeCloseTo(SUN_DIRECTION[1], 12);
    expect(MOON_DIRECTION[0]).toBeCloseTo(-SUN_DIRECTION[0], 12);
    expect(MOON_DIRECTION[2]).toBeCloseTo(-SUN_DIRECTION[2], 12);
  });

  it('is evaluated in the viewer’s own frame: a face-B sky is the night gradient, upside down', () => {
    // Overhead for a face-B player is −y; its horizon is the night haze's colour.
    const overhead = skyColor([0, -1, 0], -1);
    const horizon = skyColor([0.6, 0, -0.8], -1);
    expect(horizon[2]).toBeCloseTo(NIGHT_SKY_STOPS[0]?.color[2] ?? 0, 2);
    expect(luma(overhead)).toBeLessThan(luma(horizon)); // darker overhead, brighter near the horizon
    // Dim: the night sky is darker than the day's at the same place.
    expect(luma(overhead)).toBeLessThan(luma(skyColor([0, 1, 0], 1)) * 0.3);
    // The same direction seen from face A is the day sky (the world's up is face A's).
    expect(skyColor([0, 1, 0], 1)).not.toEqual(skyColor([0, -1, 0], -1));
    // And face B's sky ignores what is above the disc: looking +y it is below the horizon.
    expect(skyColor([0, 1, 0], -1)).toEqual(skyColor([0.6, 0.0, 0.8], -1));
  });

  it('glows cool and bright toward the moon', () => {
    // Toward the moon in the world (its y negated is face B's frame).
    const toMoon: Rgb = [MOON_DIRECTION[0], -MOON_DIRECTION[1], MOON_DIRECTION[2]];
    const moon = skyColor(toMoon, -1);
    const away = skyColor([-toMoon[0], -toMoon[1], -toMoon[2]], -1);
    expect(luma(moon)).toBeGreaterThan(luma(away));
    expect(moon[2]).toBeGreaterThan(moon[0]); // blue-white, not warm
  });

  it('dims the moon and the night’s ambient below the sun’s and the day’s', () => {
    expect(LIGHT.moonIntensity).toBeLessThan(LIGHT.sunIntensity);
    expect(LIGHT.moonHemisphereIntensity).toBeLessThanOrEqual(LIGHT.hemisphereIntensity);
    // Cooler: more blue than red.
    expect(LIGHT.moon & 0xff).toBeGreaterThan((LIGHT.moon >> 16) & 0xff);
  });

  it('is in the shader too, in the viewer’s frame: its night branch, and the face uniform', () => {
    expect(SKY_GLSL).toContain('uniform float dwellFace;');
    expect(SKY_GLSL).toContain('if (dwellFace < 0.0)');
    for (const { color } of NIGHT_SKY_STOPS) {
      expect(SKY_GLSL).toContain(color.map((c) => c.toFixed(5)).join(', '));
    }
    expect(SKY_GLSL).toContain(MOON_DIRECTION.map((c) => c.toFixed(5)).join(', '));
  });

  it('keeps each fragment to its own face’s light: the lights chunk is patched by side', () => {
    const shader = { uniforms: {}, vertexShader: '', fragmentShader: '' };
    shader.fragmentShader = '#include <fog_pars_fragment>\n#include <lights_fragment_begin>';
    patchFogShader(shader as never);
    const src = shader.fragmentShader;
    expect(src).not.toContain('#include <lights_fragment_begin>'); // replaced by the chunk's text
    expect(src).toContain('dwellFragA'); // the fragment's side of the midplane
    expect(src).toContain('directLight.color *= dwellOwn(');
    expect(src).toContain('hemisphereLights[ i ].direction');
    expect(src).toContain(`dwellFragY >= ${World.midplaneY.toFixed(1)}`);
  });
});

describe('exposure', () => {
  it('clamps into range and defaults when missing', () => {
    expect(sanitizeExposure(undefined)).toBe(DEFAULT_EXPOSURE);
    expect(sanitizeExposure(Number.NaN)).toBe(DEFAULT_EXPOSURE);
    expect(sanitizeExposure(100)).toBe(EXPOSURE_LIMITS.max);
    expect(sanitizeExposure(0)).toBe(EXPOSURE_LIMITS.min);
    expect(sanitizeExposure(1.3)).toBe(1.3);
  });
});
