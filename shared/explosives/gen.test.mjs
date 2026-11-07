// node --test shared/explosives/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { explosivesRust, registryBlocks, validateExplosives } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'explosives.json'), 'utf8'));

test('the checked-in table validates and generates the checked-in Rust', () => {
  assert.equal(
    explosivesRust(validateExplosives(data())),
    readFileSync(join(here, '../../crates/dwell-world/src/explosives.gen.rs'), 'utf8'),
  );
});

test('an explosive added by data alone is in the output', () => {
  const d = data();
  const blocks = registryBlocks();
  blocks.set('test:charge', { placeable: true, properties: {} });
  blocks.set('test:lit_charge', { placeable: false, properties: {} });
  d.explosives.push({ ...structuredClone(d.explosives[0]), name: 'test:charge', block: 'test:charge', lit: 'test:lit_charge', radiusM: 2.5 });
  const rust = explosivesRust(validateExplosives(d, blocks));
  assert.match(rust, /name: "test:charge",/);
  assert.match(rust, /radius_m: 2\.5,/);
});

test('bad entries fail with their path named', () => {
  for (const [change, message] of [
    [(d) => delete d.explosives[0].fuseSeconds, /explosives\[0\]\.fuseSeconds must be a number/],
    [(d) => (d.explosives[0].radiusM = 100), /radiusM must be a number in \[0\.5, 16\]/],
    [(d) => (d.explosives[0].block = 'dwell:nope'), /block: "dwell:nope" is not a block/],
    [(d) => (d.explosives[0].lit = 'dwell:stone'), /must not be placeable/],
    [(d) => (d.explosives[0].ignitedBy = ['dwell:nope']), /ignitedBy\[0\]/],
    [(d) => (d.explosives[0].chainDelaySeconds = [1, 0.5]), /high below low/],
    [(d) => (d.explosives[0].effects.smoke.cpuCount = 999), /cpuCount must be/],
    [(d) => (d.explosives[0].effects.flash.color = 'red'), /flash\.color must be #rrggbb/],
    [(d) => (d.explosives[0].bang = 1), /unknown entry explosives\[0\]\.bang/],
    [(d) => (d.limits.blastsPerTick = 0), /limits\.blastsPerTick/],
    [(d) => d.explosives.push(structuredClone(d.explosives[0])), /listed twice/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateExplosives(d), message);
  }
});
