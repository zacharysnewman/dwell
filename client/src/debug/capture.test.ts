import { describe, expect, it } from 'vitest';
import { CaptureRegistry, formatCapture } from './capture';

const PNG = 'data:image/png;base64,iVBORw0KGgo=';
const AT = new Date('2026-10-13T12:00:00Z');

describe('capture (debug tooling)', () => {
  it('collects every registered section, in order, with the image', () => {
    const registry = new CaptureRegistry();
    registry.register('player', () => ({ feet: [1, 2, 3], yaw: 90 }));
    registry.register('lod', () => ({ drawn: [0, 4] }));
    const c = registry.capture(PNG, AT);
    expect(c.kind).toBe('dwell-capture');
    expect(c.format).toBe(1);
    expect(c.takenAt).toBe('2026-10-13T12:00:00.000Z');
    expect(Object.keys(c.sections)).toEqual(['player', 'lod']);
    expect(c.sections.player).toEqual({ feet: [1, 2, 3], yaw: 90 });
    expect(c.image).toBe(PNG);
  });

  it('takes a new section by registration alone', () => {
    const registry = new CaptureRegistry();
    registry.register('player', () => ({ feet: [0, 0, 0] }));
    // A fixture system nobody wrote the capture for.
    registry.register('weather2', () => ({ rain: 0.5 }));
    expect(registry.names()).toEqual(['player', 'weather2']);
    expect(registry.capture(null, AT).sections.weather2).toEqual({ rain: 0.5 });
  });

  it('rejects malformed and repeated section names with a message', () => {
    const registry = new CaptureRegistry();
    expect(() => {
      registry.register('Player', () => 1);
    }).toThrow(/camelCase/);
    expect(() => {
      registry.register('lod stats', () => 1);
    }).toThrow(/camelCase/);
    registry.register('lod', () => 1);
    expect(() => {
      registry.register('lod', () => 2);
    }).toThrow(/already registered/);
  });

  it('records a failing section as its error and keeps the rest', () => {
    const registry = new CaptureRegistry();
    registry.register('broken', () => {
      throw new Error('no world yet');
    });
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    registry.register('cyclic', () => cyclic);
    registry.register('fine', () => 'ok');
    const c = registry.capture(null, AT);
    expect(c.sections.broken).toEqual({ error: 'no world yet' });
    expect((c.sections.cyclic as { error: string }).error).toMatch(/circular/i);
    expect(c.sections.fine).toBe('ok');
  });

  it('writes bigints (seeds) as decimal strings, and the image last', () => {
    const registry = new CaptureRegistry();
    registry.register('world', () => ({ seed: 18446744073709551615n, generatorVersion: 12 }));
    const text = formatCapture(registry.capture(PNG, AT));
    const parsed = JSON.parse(text) as { sections: { world: { seed: string } } };
    expect(parsed.sections.world.seed).toBe('18446744073709551615');
    expect(text.trimEnd().endsWith(`"image": "${PNG}"\n}`)).toBe(true);
  });
});
