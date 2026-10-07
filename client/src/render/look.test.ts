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
  gradient,
  MOON_DIRECTION_WORLD,
  MOON_GLOW,
  NIGHT_SKY_STOPS,
  SKY_GLSL,
  SKY_SPHERE_STOPS,
  SKY_STOPS,
  SUN_GLOW,
  SUN_DIRECTION,
  sanitizeExposure,
  skyColor,
  type Rgb,
} from './look';
import { srgbToLinear } from './textures';
import { skyFrame } from './skyFrame';
import { lightWeight, patchFogShader } from './three/heightFog';

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
  const close = (a: Rgb, b: Rgb, tol: number): void => {
    for (let i = 0; i < 3; i++) expect(Math.abs((a[i] ?? 0) - (b[i] ?? 0))).toBeLessThan(tol);
  };

  it("is the gradient's horizon colour at and below the horizon, away from the sun", () => {
    // (Within the moon's faint halo, which lies opposite the sun.)
    close(skyColor(away(0)), HORIZON_COLOR, 2e-3);
    close(skyColor(away(-0.5)), HORIZON_COLOR, 2e-3);
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
    // The shader's stops are the gradient's, and its directions are the sky frame's uniforms.
    for (const { color } of SKY_SPHERE_STOPS) {
      expect(SKY_GLSL).toContain(color.map((c) => c.toFixed(5)).join(', '));
    }
    for (const u of ['dwellFace', 'dwellDayPole', 'dwellSun', 'dwellMoon']) {
      expect(SKY_GLSL).toContain(`uniform ${u === 'dwellFace' ? 'float' : 'vec3'} ${u};`);
    }
  });

  describe('the sphere, at rest: the static sky of each face', () => {
    // Elevations from the horizon band up, on both faces, against the old day and night gradients.
    for (const face of [1, -1] as const) {
      it(`matches face ${face > 0 ? 'A' : 'B'}'s gradient from 0.12 up, and within 0.09 below`, () => {
        const stops = face > 0 ? SKY_STOPS : NIGHT_SKY_STOPS;
        const [sx, , sz] = SUN_DIRECTION;
        const n = Math.hypot(sx, sz);
        const glowAt = (g: typeof SUN_GLOW, light: Rgb, dir: Rgb): number => {
          const dot = Math.max(0, dir[0] * light[0] + dir[1] * light[1] + dir[2] * light[2]);
          return Math.min(1, g.tight * dot ** g.tightPower + g.broad * dot ** g.broadPower);
        };
        for (let e = 0; e <= 1.0001; e += 0.01) {
          const h = Math.sqrt(Math.max(0, 1 - e * e));
          const dir: Rgb = [(-sx / n) * h, face * e, (-sz / n) * h];
          // The old sky: the face's gradient and its light's glow. The sphere also lets the other
          // light's glow show where it is in the sky (a faint halo opposite the sun by day).
          let old = gradient(stops, Math.min(1, e));
          old = mix3(old, SUN_GLOW.color, glowAt(SUN_GLOW, SUN_DIRECTION, dir));
          old = mix3(old, MOON_GLOW.color, glowAt(MOON_GLOW, MOON_DIRECTION_WORLD, dir));
          close(skyColor(dir, face), old, e >= 0.12 ? 1e-6 : 0.09);
        }
      });
    }
  });

  describe('the sky frame turns the sky as a unit', () => {
    const random = (() => {
      let a = 12345;
      return () => {
        a = (a + 0x6d2b79f5) | 0;
        let t = Math.imul(a ^ (a >>> 15), 1 | a);
        t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
      };
    })();
    const unit = (): Rgb => {
      const v: Rgb = [random() * 2 - 1, random() * 2 - 1, random() * 2 - 1];
      const n = Math.hypot(...v) || 1;
      return [v[0] / n, v[1] / n, v[2] / n];
    };

    it('is continuous in the angle', () => {
      for (let i = 0; i < 100; i++) {
        const dir = unit();
        const angle = random() * Math.PI * 2;
        for (const face of [1, -1] as const) {
          const a = skyColor(dir, face, skyFrame(angle));
          const b = skyColor(dir, face, skyFrame(angle + 0.001));
          close(a, b, 0.01);
        }
      }
    });

    it('turns day to night: at π a face-A viewer sees the night zenith, at π/2 the twilight', () => {
      close(
        skyColor([0, 1, 0], 1, skyFrame(Math.PI)),
        SKY_SPHERE_STOPS[0]?.color ?? HORIZON_COLOR,
        0.03,
      );
      close(
        skyColor([0, 1, 0], 1, skyFrame(Math.PI / 2)),
        SKY_SPHERE_STOPS[3]?.color ?? HORIZON_COLOR,
        0.05,
      );
    });

    it('at π the viewer on face B has the day sky overhead', () => {
      close(
        skyColor([0, -1, 0], -1, skyFrame(Math.PI)),
        SKY_STOPS[SKY_STOPS.length - 1]?.color ?? HORIZON_COLOR,
        0.03,
      );
    });
  });
});

const mix3 = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

describe('face B: a moonlit night sky and light (BIFACIAL_WORLD.md §6)', () => {
  const close = (a: Rgb, b: Rgb, tol: number): void => {
    for (let i = 0; i < 3; i++) expect(Math.abs((a[i] ?? 0) - (b[i] ?? 0))).toBeLessThan(tol);
  };
  const luma = (c: Rgb): number => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];

  it('puts the moon opposite the sun in the world', () => {
    SUN_DIRECTION.forEach((c, i) => {
      expect(MOON_DIRECTION_WORLD[i]).toBeCloseTo(-c, 12);
    });
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
    const flat = skyColor([0.6, 0, 0.8], -1);
    const deeper = Math.hypot(0.6, 0.5, 0.8);
    close(skyColor([0.6 / deeper, 0.5 / deeper, 0.8 / deeper], -1), flat, 1e-9);
  });

  it('glows cool and bright toward the moon', () => {
    const moon = skyColor(MOON_DIRECTION_WORLD, -1);
    const away = skyColor(
      [-MOON_DIRECTION_WORLD[0], MOON_DIRECTION_WORLD[1], -MOON_DIRECTION_WORLD[2]],
      -1,
    );
    expect(luma(moon)).toBeGreaterThan(luma(away));
    expect(moon[2]).toBeGreaterThan(moon[0]); // blue-white, not warm
  });

  it('dims the moon and the night’s ambient below the sun’s and the day’s', () => {
    expect(LIGHT.moonIntensity).toBeLessThan(LIGHT.sunIntensity);
    expect(LIGHT.moonHemisphereIntensity).toBeLessThanOrEqual(LIGHT.hemisphereIntensity);
    // Cooler: more blue than red.
    expect(LIGHT.moon & 0xff).toBeGreaterThan((LIGHT.moon >> 16) & 0xff);
  });

  it('is in the shader too, in the viewer’s frame: the face uniform', () => {
    expect(SKY_GLSL).toContain('vec3 up = vec3(0.0, dwellFace, 0.0);');
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

  it('weights a light by its height over the fragment’s own horizon, smoothly', () => {
    expect(lightWeight(0.7, 1)).toBe(1); // the sun high over face A
    expect(lightWeight(-0.7, 1)).toBe(0); // the moon under it
    expect(lightWeight(-0.7, -1)).toBe(1);
    expect(lightWeight(0.7, -1)).toBe(0);
    expect(lightWeight(0, 1)).toBeCloseTo(0.5, 12);
    expect(lightWeight(0, -1)).toBeCloseTo(0.5, 12);
    let last = -1;
    for (let y = -0.3; y <= 0.3; y += 0.01) {
      const w = lightWeight(y, 1);
      expect(w).toBeGreaterThanOrEqual(last);
      last = w;
    }
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
