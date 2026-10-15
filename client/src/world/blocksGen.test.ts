// The block generator (shared/blocks/gen.mjs) over fixture data: a copy runs in a temporary tree
// with its own block files, so entries are added by data alone and its validation is exercised.
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';

const blocksDir = fileURLToPath(new URL('../../../shared/blocks/', import.meta.url));
let root = '';

/** Runs the generator over `blocks` (the `dwell` namespace): its exit status, output and the Rust table. */
function generate(blocks: object[]): { status: number | null; output: string; rust: string } {
  root = mkdtempSync(join(tmpdir(), 'dwell-blocks-'));
  const here = join(root, 'shared/blocks');
  // The fixture run (`gen.mjs --fixture`) reads fixtures/; none here.
  mkdirSync(join(here, 'fixtures'), { recursive: true });
  mkdirSync(join(root, 'client/src/world'), { recursive: true });
  mkdirSync(join(root, 'crates/dwell-blocks/src'), { recursive: true });
  for (const f of ['gen.mjs', 'shapes.mjs', 'fluids.mjs'])
    copyFileSync(join(blocksDir, f), join(here, f));
  const air = { name: 'air', density: 0, shape: 'empty', color: '#000000', opacity: 0 };
  writeFileSync(
    join(here, 'dwell.json'),
    JSON.stringify({ namespace: 'dwell', shapeFamilies: [], blocks: [air, ...blocks] }),
  );
  const r = spawnSync(process.execPath, [join(here, 'gen.mjs')], { encoding: 'utf8' });
  let rust = '';
  try {
    rust = readFileSync(join(root, 'crates/dwell-blocks/src/blocks.gen.rs'), 'utf8');
  } catch {
    // Not generated: the run failed.
  }
  return { status: r.status, output: `${r.stdout}${r.stderr}`, rust };
}

const block = (name: string, extra: object) => ({
  name,
  density: 1000,
  shape: 'full',
  color: '#ffffff',
  ...extra,
});

afterEach(() => {
  if (root) rmSync(root, { recursive: true, force: true });
  root = '';
});

describe('block generator: the grid look option', () => {
  it('emits a fixture block’s grid into the Rust table', () => {
    const r = generate([block('tiles', { textures: 'grid', grid: { every: 4, strength: 0.5 } })]);
    expect(r.status, r.output).toBe(0);
    expect(r.rust).toContain('name: "dwell:tiles"');
    expect(r.rust).toContain('grid: Some(Grid { every: 4, strength: 0.5 })');
    expect(r.rust).toMatch(/name: "dwell:air".*grid: None/);
  });

  it('rejects two blocks that want different grids on one texture tile', () => {
    const r = generate([
      block('a', { textures: 'grid', grid: { every: 8, strength: 0.3 } }),
      block('b', { textures: 'grid' }),
    ]);
    expect(r.status).not.toBe(0);
    expect(r.output).toContain('dwell:b: texture tile grid is drawn with every 8, strength 0.3');
  });

  it('counts an untextured block as the plain tile', () => {
    const r = generate([block('a', { grid: { every: 8, strength: 0.3 } }), block('b', {})]);
    expect(r.status).not.toBe(0);
    expect(r.output).toContain('texture tile plain');
  });

  it('rejects bad grid values with the block named', () => {
    for (const [grid, message] of [
      [{ every: 3, strength: 0.3 }, 'grid.every must divide 32'],
      [{ every: 8, strength: 0 }, 'grid.strength must be in (0, 1]'],
      [{ every: 8, strength: 0.3, width: 1 }, 'grid: unknown width'],
    ] as const) {
      const r = generate([block('bad', { textures: 'grid', grid })]);
      expect(r.status).not.toBe(0);
      expect(r.output).toContain(`dwell:bad: ${message}`);
    }
  });
});
