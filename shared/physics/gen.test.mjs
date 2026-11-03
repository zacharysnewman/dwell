// node --test shared/physics/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bodiesRust, fellingRust, validateBodies, validateFelling } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'felling.json'), 'utf8'));

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  assert.equal(
    fellingRust(validateFelling(data())),
    readFileSync(join(here, '../../crates/dwell-sim/src/felling.gen.rs'), 'utf8'),
  );
});

test('a value changed by data alone is in the output', () => {
  const d = data();
  d.toppleSpin = 1.25;
  assert.match(fellingRust(validateFelling(d)), /topple_spin: 1\.25,/);
});

test('bad entries fail with their path named', () => {
  for (const [change, message] of [
    [(d) => (d.riderReach = 1.5), /riderReach must be a whole number/],
    [(d) => (d.maxPieceCells = 5000), /maxPieceCells must be a whole number in \[1, 4096\]/],
    [(d) => delete d.hitSpeed, /missing entry \(root\)\.hitSpeed/],
    [(d) => (d.settleBounce = 1.5), /settleBounce must be a number in \[0, 1\]/],
    [(d) => (d.spin = 1), /unknown entry \(root\)\.spin/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateFelling(d), message);
  }
});

const bodies = () => JSON.parse(readFileSync(join(here, 'bodies.json'), 'utf8'));

test('the checked-in body kinds validate and generate the checked-in Rust', () => {
  assert.equal(
    bodiesRust(validateBodies(bodies())),
    readFileSync(join(here, '../../crates/dwell-sim/src/bodies.gen.rs'), 'utf8'),
  );
});

test('a body kind added by data alone is generated', () => {
  const d = bodies();
  d.kinds.push({ name: 'cart', place: false, break: true, split: false, settles: false, saved: true });
  assert.match(
    bodiesRust(validateBodies(d)),
    /BodyKind \{ name: "cart", place: false, breaks: true, split: false, settles: false, saved: true, built_on: &\[\] \}/,
  );
});

test('bad body kinds fail with their path named', () => {
  for (const [change, message] of [
    [(d) => (d.kinds[1].place = 'yes'), /kinds\[1\]\.place must be true or false/],
    [(d) => delete d.kinds[0].settles, /kinds\[0\]\.settles must be true or false/],
    [(d) => (d.kinds[1].name = 'natural'), /kinds\[1\]\.name natural is listed twice/],
    [(d) => (d.kinds[1].fly = true), /kinds\[1\]: unknown fly/],
    [(d) => d.kinds.reverse(), /kinds\[0\] must be natural/],
    [(d) => (d.water.drag = -1), /water\.drag must be a number in \[0, 50\]/],
    [(d) => delete d.water, /water must be an object/],
    [(d) => (d.kinds[1].builtOn = ['dwell:nope']), /kinds\[1\]\.builtOn: "dwell:nope" is not a block of the registry/],
    [(d) => (d.kinds[0].builtOn = ['dwell:build_platform']), /builtOn: dwell:build_platform already builds natural/],
  ]) {
    const d = bodies();
    change(d);
    assert.throws(() => validateBodies(d), message);
  }
});
