import { describe, expect, it } from 'vitest';
import { parseShotArgs, shotQuery } from './shotArgs';

const noFiles = (): string => {
  throw new Error('no file');
};

describe('shot arguments (scripts/shot.ts)', () => {
  it('reads one view with debug views and builds the page query', () => {
    const o = parseShotArgs(
      [
        '--at',
        '15557,30,-12546',
        '--look',
        '135,-12',
        '--out',
        'coast.png',
        '--standins',
        '--seed',
        '7',
      ],
      noFiles,
    );
    expect(o.views).toEqual([{ at: [15557, 30, -12546], look: [135, -12], out: 'coast.png' }]);
    expect(o.standins).toBe(true);
    const q = new URLSearchParams(shotQuery(o));
    expect(q.get('local')).toBe('1');
    expect(q.get('seed')).toBe('7');
    expect(q.get('at')).toBe('15557,30,-12546');
    expect(q.get('look')).toBe('135,-12');
    expect(q.get('standins')).toBe('1');
    expect(q.get('lodcolors')).toBeNull();
  });

  it('defaults to the spawn, looking ahead, into shot.png', () => {
    const o = parseShotArgs([], noFiles);
    expect(o.views).toEqual([{ at: null, look: [0, 0], out: 'shot.png' }]);
    expect(new URLSearchParams(shotQuery(o)).get('at')).toBeNull();
  });

  it('reads several views from a file', () => {
    const file = JSON.stringify([
      { at: [1, 2, 3], look: [90, -10], out: 'a.png' },
      { out: 'b.png' },
    ]);
    const o = parseShotArgs(['--views', 'v.json'], () => file);
    expect(o.views).toEqual([
      { at: [1, 2, 3], look: [90, -10], out: 'a.png' },
      { at: null, look: [0, 0], out: 'b.png' },
    ]);
  });

  it('rejects malformed input with a message', () => {
    expect(() => parseShotArgs(['--at', '1,2'], noFiles)).toThrow(/--at: x,y,z/);
    expect(() => parseShotArgs(['--look'], noFiles)).toThrow(/needs a value/);
    expect(() => parseShotArgs(['--world', 'moon'], noFiles)).toThrow(/terrain\|flat/);
    expect(() => parseShotArgs(['--bogus'], noFiles)).toThrow(/unknown option/);
    expect(() => parseShotArgs(['--views', 'v.json'], () => '[{"at":[1,2,3]}]')).toThrow(
      /"out" is required/,
    );
  });
});
