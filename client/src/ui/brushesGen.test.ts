// The brush generator (shared/brushes/gen.mjs, docs/BRUSHES.md) over fixture data: a copy runs in a
// temporary tree with its own brushes.json, so presets are added by data alone and its validation
// is exercised. The checked-in output is the real data's (CI regenerates it and fails on a diff).
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { BRUSH_PRESETS, BRUSH_SIZES } from './brushes.gen';

const brushesDir = fileURLToPath(new URL('../../../shared/brushes/', import.meta.url));
const real = JSON.parse(readFileSync(join(brushesDir, 'brushes.json'), 'utf8')) as {
  presets: object[];
};
let root = '';

/** Runs the generator over `data`: its exit status, output, and the Rust and TypeScript tables. */
function generate(data: object): {
  status: number | null;
  output: string;
  rust: string;
  ts: string;
} {
  root = mkdtempSync(join(tmpdir(), 'dwell-brushes-'));
  const here = join(root, 'shared/brushes');
  mkdirSync(here, { recursive: true });
  mkdirSync(join(root, 'client/src/ui'), { recursive: true });
  mkdirSync(join(root, 'crates/dwell-world/src'), { recursive: true });
  copyFileSync(join(brushesDir, 'gen.mjs'), join(here, 'gen.mjs'));
  writeFileSync(join(here, 'brushes.json'), JSON.stringify(data));
  const r = spawnSync(process.execPath, [join(here, 'gen.mjs')], { encoding: 'utf8' });
  const read = (p: string): string => {
    try {
      return readFileSync(join(root, p), 'utf8');
    } catch {
      return ''; // not generated: the run failed
    }
  };
  return {
    status: r.status,
    output: `${r.stdout}${r.stderr}`,
    rust: read('crates/dwell-world/src/brushes.gen.rs'),
    ts: read('client/src/ui/brushes.gen.ts'),
  };
}

const base = (presets: object[]): object => ({
  formatVersion: 1,
  sizes: { min: 1, max: 16 },
  worldTypes: { sandbox: { brushes: true, reach: 128 } },
  presets,
});

const ball = { name: 'ball', label: 'Ball', brush: 'ball', performer: 'set', size: 3 };

afterEach(() => {
  if (root) rmSync(root, { recursive: true, force: true });
  root = '';
});

describe('brush presets (shared/brushes/gen.mjs)', () => {
  it('the checked-in table is the data', () => {
    expect(BRUSH_PRESETS.map((p) => p.name)).toEqual(
      real.presets.map((p) => (p as { name: string }).name),
    );
    expect(BRUSH_SIZES).toEqual({ min: 1, max: 16 });
  });

  it('adds a preset by data alone', () => {
    const tall = {
      name: 'tall_ellipsoid',
      label: 'Tall ellipsoid',
      brush: 'ellipsoid',
      performer: 'erode',
      size: 5,
      key: 'Semicolon',
      params: { stretch: [1, 2, 1], fillIterations: 0 },
    };
    const out = generate(base([ball, tall]));
    expect(out.status, out.output).toBe(0);
    expect(out.rust).toContain('name: "tall_ellipsoid"');
    expect(out.rust).toContain('brush: BrushKind::Ellipsoid');
    expect(out.rust).toContain('performer: PerformerKind::Erode');
    expect(out.rust).toContain('stretch: [1.0, 2.0, 1.0]');
    expect(out.rust).toContain('fill_iterations: 0');
    expect(out.rust).toContain('key: Some("Semicolon")');
    expect(out.ts).toContain("name: 'tall_ellipsoid'");
  }, 20_000);

  it('fails a bad entry with a message naming it', () => {
    const cases: [object, RegExp][] = [
      [base([{ ...ball, brush: 'torus' }]), /presets\[0\] \(ball\): brush torus/],
      [base([{ ...ball, performer: 'melt' }]), /performer melt/],
      [base([{ ...ball, size: 17 }]), /size must be an integer 1–16/],
      [base([ball, { ...ball, label: 'Again' }]), /duplicate name/],
      [base([{ ...ball, key: 'KeyW' }]), /KeyW is not a free key/],
      [
        base([
          { ...ball, key: 'KeyU' },
          { ...ball, name: 'b2', key: 'KeyU' },
        ]),
        /taken by another/,
      ],
      [base([{ ...ball, params: { height: 3 } }]), /height is for cylinder/],
      [base([{ ...ball, brush: 'splatter', params: { density: 2 } }]), /density must be/],
      [base([{ ...ball, colour: 'red' }]), /unknown field colour/],
      [
        { ...base([ball]), worldTypes: { moon: { brushes: true, reach: 128 } } },
        /moon: not a world type/,
      ],
      [
        { ...base([ball]), worldTypes: { sandbox: { brushes: true, reach: 999 } } },
        /reach must be/,
      ],
    ];
    for (const [data, message] of cases) {
      const out = generate(data);
      expect(out.status, JSON.stringify(data)).not.toBe(0);
      expect(out.output).toMatch(message);
    }
  }, 60_000); // one generator run per case
});
