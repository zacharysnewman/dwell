// The block generator (shared/blocks/gen.mjs) over fixture data: a copy runs in a temporary tree
// with its own block files, so entries are added by data alone and its validation is exercised.
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';

const blocksDir = fileURLToPath(new URL('../../../shared/blocks/', import.meta.url));
const soundsDir = fileURLToPath(new URL('../../../shared/sounds/', import.meta.url));
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
  // The sound data the generator checks blocks' `sound` against (shared/sounds).
  const sounds = join(root, 'shared/sounds');
  mkdirSync(sounds, { recursive: true });
  for (const f of ['load.mjs', 'validate.mjs', 'dwell.json'])
    copyFileSync(join(soundsDir, f), join(sounds, f));
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

// docs/TRANSPARENT_BLOCKS.md: render modes, the cross geometry and the placement rules.
const plant = (name: string, extra: object) =>
  block(name, { shape: 'empty', look: 'cross', render: 'cutout', textures: 'tallGrass', ...extra });

describe('block generator: render modes', () => {
  it('adds a fixture block of each mode by data alone', () => {
    const r = generate([
      block('rock', {}),
      block('leaf', { render: 'cutout', textures: 'ladder' }),
      block('glass', { render: 'translucent', opacity: 0.4, textures: 'water' }),
    ]);
    expect(r.status, r.output).toBe(0);
    expect(r.rust).toMatch(/name: "dwell:rock".*render: RenderMode::Opaque/);
    expect(r.rust).toMatch(/name: "dwell:leaf".*render: RenderMode::Cutout/);
    expect(r.rust).toMatch(/name: "dwell:glass".*render: RenderMode::Translucent/);
    // Air (opacity 0) needs no mode.
    expect(r.rust).toMatch(/name: "dwell:air".*render: RenderMode::Opaque/);
  });

  it('rejects an unknown mode, see-through opacity without translucent, and untextured cutout', () => {
    for (const [extra, message] of [
      [{ render: 'glow' }, 'render must be one of opaque, cutout, translucent'],
      [{ opacity: 0.5 }, 'opacity 0.5 needs "render": "translucent"'],
      [{ render: 'cutout' }, 'render cutout needs textures'],
    ] as const) {
      const r = generate([block('bad', extra)]);
      expect(r.status).not.toBe(0);
      expect(r.output).toContain(`dwell:bad: ${message}`);
    }
  });
});

describe('block generator: the cross geometry and wind', () => {
  it('emits a cross with its options, wind and fade', () => {
    const r = generate([
      plant('reeds', {
        cross: { height: 0.9, quads: 3, jitter: { offset: 0.2, rotation: 30, height: [0.7, 1] } },
        wind: { strength: 0.1, speed: 2 },
        fade: { start: 40, end: 80 },
        selectionBox: [0.25, 0, 0.25, 0.75, 0.5, 0.75],
      }),
      plant('tuft', {}),
    ]);
    expect(r.status, r.output).toBe(0);
    expect(r.rust).toContain(
      'cross: Some(Cross { height: 0.9, quads: 3, offset: 0.2, rotation_deg: 30.0, height_min: 0.7, height_max: 1.0 })',
    );
    expect(r.rust).toContain('wind: Some(Wind { strength: 0.1, speed: 2.0 })');
    expect(r.rust).toContain('fade: Some(Fade { start: 40.0, end: 80.0 })');
    expect(r.rust).toContain('selection: Some([0.25, 0.0, 0.25, 0.75, 0.5, 0.75])');
    // Defaults: two full-height quads, no jitter, no wind.
    expect(r.rust).toMatch(
      /name: "dwell:tuft".*cross: Some\(Cross \{ height: 1\.0, quads: 2, offset: 0\.0, rotation_deg: 0\.0, height_min: 1\.0, height_max: 1\.0 \}\), wind: None/,
    );
  });

  it('rejects bad cross, wind and fade values with the block named', () => {
    for (const [extra, message] of [
      [{ render: 'opaque' }, 'look cross has gaps'],
      [{ shape: 'full' }, 'look cross needs shape empty'],
      [{ cross: { quads: 5 } }, 'cross.quads must be 2, 3 or 4'],
      [{ cross: { height: 0 } }, 'cross.height must be in (0, 1]'],
      [{ cross: { sway: 1 } }, 'cross: unknown sway'],
      [{ cross: { jitter: { offset: 0.5 } } }, 'cross.jitter.offset must be in [0, 0.5)'],
      [{ cross: { jitter: { height: [1, 0.5] } } }, 'cross.jitter.height must be [min, max]'],
      [{ wind: { strength: 1, speed: 1 } }, 'wind.strength must be in [0, 0.5]'],
      [{ wind: { strength: 0.1, speed: 0 } }, 'wind.speed must be in (0, 10]'],
      [{ fade: { start: 50, end: 200 } }, 'fade must have 0 < start < end <= 96'],
      [{ selectionBox: [0, 0, 0, 1, 1] }, 'selectionBox must be'],
    ] as const) {
      const r = generate([plant('bad', extra)]);
      expect(r.status, JSON.stringify(extra)).not.toBe(0);
      expect(r.output).toContain(`dwell:bad: ${message}`);
    }
    const cube = generate([block('bad', { cross: { quads: 2 } })]);
    expect(cube.output).toContain('dwell:bad: cross options need look cross');
    const windy = generate([block('bad', { wind: { strength: 0.1, speed: 1 } })]);
    expect(windy.output).toContain('dwell:bad: wind needs look cross');
  });
});

describe('block generator: placement rules', () => {
  it('emits placeOn, needsSupport and replaceable', () => {
    const r = generate([
      block('soil', {}),
      plant('sprout', { placeOn: ['dwell:soil'], needsSupport: 'below', replaceable: true }),
    ]);
    expect(r.status, r.output).toBe(0);
    expect(r.rust).toMatch(
      /name: "dwell:sprout".*place_on: &\["dwell:soil"\], needs_support: Support::Below, replaceable: true/,
    );
    expect(r.rust).toMatch(
      /name: "dwell:soil".*place_on: &\[\], needs_support: Support::None, replaceable: false/,
    );
  });

  it('rejects unknown blocks, empty lists and unknown supports', () => {
    for (const [extra, message] of [
      [{ placeOn: ['dwell:nowhere'] }, 'placeOn names unknown block dwell:nowhere'],
      [{ placeOn: [] }, 'placeOn must be a non-empty list of block ids'],
      [{ needsSupport: 'above' }, 'needsSupport must be one of below'],
      [{ replaceable: 'yes' }, 'replaceable must be true or false'],
    ] as const) {
      const r = generate([plant('bad', extra)]);
      expect(r.status).not.toBe(0);
      expect(r.output).toContain(`dwell:bad: ${message}`);
    }
  });
});
