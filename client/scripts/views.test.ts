import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  parseViews,
  selectViews,
  ViewError,
  viewStorage,
  viewUrl,
  WATER_SETTING_NAMES,
  WATER_TIER_NAMES,
  type View,
} from './views.ts';

const minimal = { name: 'a', camera: { yaw: 0, pitch: 0 } };

function only(view: unknown): View {
  const [v] = parseViews({ views: [view] });
  return v;
}

describe('views', () => {
  it('name the water tiers as shared/look/watertiers.json does (its tiers, then the aliases)', () => {
    const data = JSON.parse(
      readFileSync(new URL('../../shared/look/watertiers.json', import.meta.url), 'utf8'),
    ) as { tiers: { id: string; setting: boolean }[]; aliases: Record<string, string> };
    const aliases = Object.keys(data.aliases).filter((k) => k !== '$comment');
    expect(WATER_TIER_NAMES).toEqual([...data.tiers.map((t) => t.id), ...aliases]);
    expect(WATER_SETTING_NAMES).toEqual([
      ...data.tiers.filter((t) => t.setting).map((t) => t.id),
      ...aliases,
    ]);
  });

  it('the checked-in views parse', () => {
    const json = JSON.parse(
      readFileSync(new URL('../views/views.json', import.meta.url), 'utf8'),
    ) as unknown;
    const views = parseViews(json);
    expect(views.map((v) => v.name)).toContain('spawn');
  });

  it('a new view is data alone: added to a file, it is selected, addressed and set up', () => {
    const views = parseViews({
      views: [
        minimal,
        {
          name: 'fixture-high',
          seed: 42,
          camera: { position: [100, 900, -50], yaw: 90, pitch: -40 },
          window: { width: 640, height: 480 },
          settings: {
            detail: { distanceM: 96, pixelError: 3, memoryMb: 512 },
            exposure: 1.2,
            timeOfDay: 0,
            clouds: 'low',
            water: { quality: 'low', flowParticles: false },
          },
          debug: true,
          lodColors: true,
          scale: 0.5,
        },
      ],
    });
    const [v] = selectViews(views, ['fixture-high']);
    expect(v.camera).toEqual({ position: [100, 900, -50], yaw: 90, pitch: -40 });
    expect(v.window).toEqual({ width: 640, height: 480 });
    expect(viewUrl('./', v)).toBe('./?local=1&seed=42&scale=0.5&debug=1&lodcolors=1&time=0');
    expect(viewStorage(v)).toEqual({
      'dwell.detail': '{"distanceM":96,"pixelError":3,"memoryMb":512}',
      'dwell.exposure': '1.2',
      'dwell.clouds': '"low"',
      'dwell.water': '{"quality":"low","flowParticles":false}',
    });
  });

  it('fills in defaults: the player camera, seed 0, a 1280 × 720 window', () => {
    const v = only(minimal);
    expect(v.camera.position).toBeNull();
    expect(v.seed).toBe(0);
    expect(v.window).toEqual({ width: 1280, height: 720 });
    expect(v.settleS).toBe(60);
    expect(viewUrl('./', v)).toBe('./?local=1&seed=0');
    expect(viewStorage(v)).toEqual({});
  });

  it('stores the speed effects setting under its own key', () => {
    const v = only({ ...minimal, settings: { motion: { speedEffects: false } } });
    expect(viewStorage(v)).toEqual({ 'dwell.motion': '{"speedEffects":false,"fovBoost":1}' });
    expect(() => only({ ...minimal, settings: { motion: { fovBoost: 2 } } })).toThrow(
      'settings.motion.fovBoost: 2 is outside 0–1',
    );
  });

  it.each([
    [{ ...minimal, name: 'Bad Name' }, 'views[0].name: expected lowercase'],
    [{ ...minimal, camera: { yaw: 0 } }, 'views["a"].camera.pitch: expected a number'],
    [{ ...minimal, camera: { yaw: 0, pitch: -120 } }, 'camera.pitch: -120 is outside -90–90'],
    [{ ...minimal, camera: { position: [1, 2], yaw: 0, pitch: 0 } }, 'expected [x, y, z]'],
    [{ ...minimal, window: { width: 10 } }, 'window.width: 10 is outside'],
    [{ ...minimal, settings: { detail: { distanceM: 96 } } }, 'settings.detail.pixelError'],
    [{ ...minimal, settings: { fov: 90 } }, 'settings: unknown field "fov"'],
    [{ ...minimal, settings: { clouds: 'ultra' } }, 'settings.clouds: expected "off"'],
    [{ ...minimal, settings: { timeOfDay: 1.5 } }, 'settings.timeOfDay: 1.5 is outside'],
    [{ ...minimal, settings: { water: { quality: 'ultra' } } }, 'settings.water.quality: expected'],
    [
      { ...minimal, settings: { water: { quality: 'low', flowFoam: 1 } } },
      'settings.water.flowFoam: expected true',
    ],
    [{ ...minimal, pitch: 0 }, 'views["a"]: unknown field "pitch"'],
    [{ ...minimal, seed: 1.5 }, 'seed: expected a whole number'],
    [{ ...minimal, debug: 'yes' }, 'debug: expected true or false'],
  ])('rejects %j with a message naming the field', (view, message) => {
    expect(() => parseViews({ views: [view] })).toThrow(ViewError);
    expect(() => parseViews({ views: [view] })).toThrow(message);
  });

  it('rejects a name used twice, a file without views and an unknown selection', () => {
    expect(() => parseViews({ views: [minimal, minimal] })).toThrow('used twice');
    expect(() => parseViews({ view: [] })).toThrow('unknown field "view"');
    expect(() => selectViews(parseViews({ views: [minimal] }), ['b'])).toThrow(
      'no view named b (views: a)',
    );
  });
});
