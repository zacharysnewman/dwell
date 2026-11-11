// node --test shared/look/leaves.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { leavesRust, validateLeaves } from './leaves.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'leaves.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in look validates and generates the checked-in Rust', () => {
  assert.equal(leavesRust(validateLeaves(data())), checkedIn('crates/dwell-look/src/leaves.gen.rs'));
});

test('a block added by data alone reaches the output', () => {
  const d = data();
  d.blocks.push('mymod:autumn_leaves');
  assert.match(leavesRust(validateLeaves(d)), /"mymod:autumn_leaves"\]/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.blocks = []), /blocks must list at least one block/],
    [(d) => (d.blocks[0] = 'leaves'), /blocks\[0\] must be a block name/],
    [(d) => d.blocks.push(d.blocks[0]), /repeats dwell:leaves/],
    [(d) => (d.canopyBlend = 1.5), /canopyBlend must be a number in \[0, 1\]/],
    [(d) => (d.chamfer = 0.6), /chamfer must be a number in \[0, 0.45\]/],
    [(d) => (d.extra = 1), /unknown entry \(root\)\.extra/],
    [(d) => delete d.chamfer, /missing entry \(root\)\.chamfer/],
    [(d) => (d.cards.fadeEndM = d.cards.fadeStartM), /cards\.fadeEndM must be a number/],
    [(d) => (d.cards.density = -1), /cards\.density must be a number/],
    [(d) => delete d.cards.sizeM, /missing entry cards\.sizeM/],
    [(d) => delete d.cards.tiers.phone, /missing entry cards\.tiers\.phone/],
    [(d) => (d.cards.tiers.cpu = 'no'), /cards\.tiers\.cpu must be true or false/],
    [(d) => (d.cards.tiers.tablet = true), /unknown entry cards\.tiers\.tablet/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLeaves(d), message);
  }
});

test('the phone and CPU tiers mesh no cards; a tier turned on by data alone reaches the output', () => {
  const d = data();
  assert.equal(d.cards.tiers.phone, false);
  assert.equal(d.cards.tiers.cpu, false);
  d.cards.tiers.phone = true;
  assert.match(leavesRust(validateLeaves(d)), /phone: true,/);
});
