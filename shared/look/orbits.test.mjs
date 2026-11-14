// node --test shared/look/orbits.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { orbitsRust, orbitsTs, validateOrbits } from './orbits.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'orbits.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in orbits validate and generate the checked-in Rust and TypeScript', () => {
  const d = validateOrbits(data());
  assert.equal(orbitsRust(d), checkedIn('crates/dwell-look/src/orbits.gen.rs'));
  assert.equal(orbitsTs(d), checkedIn('client/src/ui/skyPresets.gen.ts'));
});

test('a preset added by data alone is in both outputs', () => {
  const d = data();
  d.presets.tropic = {
    label: 'Tropic',
    description: 'A low pole: warm worlds.',
    roll: { ...d.presets.default.roll, poleDeg: [10, 20] },
  };
  const rs = orbitsRust(validateOrbits(d));
  assert.match(rs, /name: "tropic",\n {12}label: "Tropic",/);
  assert.match(rs, /pole_deg: \[10\.0, 20\.0\],/);
  assert.match(orbitsTs(d), /\{ name: 'tropic', label: "Tropic"/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.presets.default.roll.poleDeg = [40, 30]), /presets\.default\.roll\.poleDeg: max 30 is under min 40/],
    [(d) => (d.presets.extreme.roll.poleDeg = [10, 70]), /presets\.extreme\.roll\.poleDeg: max 70 is over limits\.maxPoleDeg/],
    [(d) => (d.presets.default.roll.dayShare = [0.5, 0.85]), /presets\.default\.roll\.dayShare/],
    [(d) => (d.presets.default.roll.moon.phase = [0, 2]), /presets\.default\.roll\.moon\.phase\[1\]/],
    [(d) => (d.presets.default.roll.wobble = [0, 1]), /unknown entry presets\.default\.roll\.wobble/],
    [(d) => delete d.presets.default.roll.yearDays, /missing entry presets\.default\.roll\.yearDays/],
    [(d) => (d.presets.default.label = ''), /presets\.default\.label/],
    [
      (d) => {
        const p = d.presets;
        d.presets = { extreme: p.extreme, default: p.default };
      },
      /the first preset must be `default`/,
    ],
    [(d) => (d.presets['Bad-Name'] = d.presets.default), /presets\.Bad-Name: a preset's name/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateOrbits(d), message);
  }
});
