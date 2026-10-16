// The terrain decorations' validation and generation (node --test shared/worldgen/gen.test.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { decorationsRust, registryBlocks, validateDecorations } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const load = () => JSON.parse(readFileSync(join(here, 'decorations.json'), 'utf8'));
const blocks = registryBlocks();

test('the checked-in decorations are valid and generate the checked-in Rust', () => {
  const rs = decorationsRust(validateDecorations(load(), blocks));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/decorations.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('an entry added by data alone is emitted', () => {
  const d = load();
  d.decorations.push({
    name: 'dirt_tufts',
    block: 'dwell:tall_grass',
    on: ['dwell:dirt'],
    density: 0.1,
    maxSlope: 2,
    height: [0, 50],
  });
  const rs = decorationsRust(validateDecorations(d, blocks));
  assert.match(rs, /DECORATIONS: \[Decoration; 2\]/);
  assert.match(rs, /name: "dirt_tufts", block: "dwell:tall_grass", on: &\["dwell:dirt"\], density: 0\.1, patch_wavelength: 0, patch_coverage: 1\.0, max_slope: 2, min_height: 0, max_height: 50/);
});

test('a bad entry fails with its name and the problem', () => {
  const cases = [
    [(d) => (d.decorations[0].block = 'dwell:moss'), /meadow_grass: block dwell:moss is not in the registry/],
    [(d) => (d.decorations[0].block = 'dwell:stone'), /must have shape empty/],
    [(d) => (d.decorations[0].on = ['dwell:sand']), /may not stand on dwell:sand/],
    [(d) => (d.decorations[0].on = ['dwell:water']), /is not a full block/],
    [(d) => (d.decorations[0].density = 0), /density must be in \(0, 1\]/],
    [(d) => (d.decorations[0].patches.wavelength = 2), /patches\.wavelength/],
    [(d) => (d.decorations[0].maxSlope = 1.5), /maxSlope must be a whole number/],
    [(d) => (d.decorations[0].height = [10, 5]), /height must be \[min, max\]/],
    [(d) => (d.decorations[0].rarity = 1), /unknown rarity/],
    [(d) => d.decorations.push({ ...d.decorations[0] }), /duplicate name/],
  ];
  for (const [edit, message] of cases) {
    const d = load();
    edit(d);
    assert.throws(() => validateDecorations(d, blocks), message);
  }
});
