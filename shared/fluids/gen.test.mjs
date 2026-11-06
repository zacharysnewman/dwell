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
    readFileSync(join(here, '../../crates/dwell-world/src/flow.gen.rs'), 'utf8'),
  );
});

test('a liquid added by data alone is in the output', () => {
  const d = data();
  d.liquids.push({ block: 'dwell:running_water', manning: 0.05, weir: 1.2, running: 'dwell:water' });
  assert.match(
    flowRust(validateFlow(d)),
    /LiquidFlow \{ block: "dwell:running_water", manning: 0\.05, weir: 1\.2, running: "dwell:water" \}/,
  );
});

test('bad entries fail with their path named', () => {
  for (const [change, message] of [
    [(d) => (d.tickEvery = 0.5), /tickEvery must be a whole number/],
    [(d) => delete d.speed, /speed must be a number/],
    [(d) => (d.fly = 1), /unknown entry \(root\)\.fly/],
    [(d) => (d.liquids[0].block = 'dwell:stone'), /liquids\[0\]\.block: dwell:stone is not a liquid/],
    [(d) => (d.liquids[0].block = 'dwell:nope'), /is not a block of the registry/],
    [(d) => (d.liquids[0].weir = 0), /liquids\[0\]\.weir must be a number in \(0, 10\]/],
    [(d) => (d.liquids[0].running = 'dwell:stone'), /liquids\[0\]\.running: dwell:stone is not a liquid/],
    [(d) => (d.liquids[0].running = 'dwell:water'), /must not be the liquid's own block/],
    [(d) => (d.liquids[0].manning = 0), /liquids\[0\]\.manning must be a number in \[0\.005, 0\.5\]/],
    [(d) => (d.theta = 1.5), /theta must be a number in \[0\.5, 1\]/],
    [(d) => (d.editReach = 1.5), /editReach must be a whole number/],
    [(d) => (d.wakeBandM = 0.001), /wakeBandM must be at least sleepBandM/],
    [(d) => (d.leaks = {}), /unknown entry \(root\)\.leaks/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateFlow(d), message);
  }
});
