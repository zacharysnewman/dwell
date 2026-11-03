// node --test shared/fluids/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { flowRust, validateFlow } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'flow.json'), 'utf8'));

test('the checked-in flow data validates and generates the checked-in Rust', () => {
  assert.equal(
    flowRust(validateFlow(data())),
    readFileSync(join(here, '../../crates/dwell-sim/src/flow.gen.rs'), 'utf8'),
  );
});

test('a block leak added by data alone is in the output', () => {
  const d = data();
  d.leaks['dwell:log'] = 0.002;
  assert.match(flowRust(validateFlow(d)), /\("dwell:log", 0\.002\),/);
});

test('bad entries fail with their path named', () => {
  for (const [change, message] of [
    [(d) => (d.tickEvery = 0.5), /tickEvery must be a whole number/],
    [(d) => delete d.speed, /speed must be a number/],
    [(d) => (d.fly = 1), /unknown entry \(root\)\.fly/],
    [(d) => (d.liquids[0].block = 'dwell:stone'), /liquids\[0\]\.block: dwell:stone is not a liquid/],
    [(d) => (d.liquids[0].block = 'dwell:nope'), /is not a block of the registry/],
    [(d) => (d.liquids[0].weir = 0), /liquids\[0\]\.weir must be a number in \(0, 10\]/],
    [(d) => (d.leaks['dwell:water'] = 1), /leaks: dwell:water is a liquid/],
    [(d) => (d.leaks['dwell:sand'] = -1), /leaks\.dwell:sand must be a number in \[0, 10\]/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateFlow(d), message);
  }
});
