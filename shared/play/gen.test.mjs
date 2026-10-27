// node --test shared/play/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { overridesTs, validateOverrides } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'overrides.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in overrides validate and generate the checked-in TypeScript', () => {
  assert.equal(overridesTs(validateOverrides(data())), checkedIn('client/src/ui/playOverrides.gen.ts'));
});

test('a field added by data alone is in the output', () => {
  const d = data();
  d.overrides.push({
    param: 'crossingroll',
    label: 'Crossing roll',
    kind: 'number',
    min: 0,
    max: 5,
    step: 0.1,
  });
  const fields = validateOverrides(d);
  assert.deepEqual(fields.at(-1), {
    param: 'crossingroll',
    label: 'Crossing roll',
    kind: 'number',
    min: 0,
    max: 5,
    step: 0.1,
  });
  assert.match(overridesTs(fields), /"param": "crossingroll"/);
});

test('a look data file supplies a choice its options, a new entry there included', () => {
  const d = data();
  const read = (p) =>
    p.endsWith('outline.json')
      ? JSON.stringify({ modes: [{ name: 'silhouette' }, { name: 'wobbly' }] })
      : readFileSync(join(here, '../..', p), 'utf8');
  const outline = validateOverrides(d, read).find((f) => f.param === 'outline');
  assert.deepEqual(outline.options, [
    { value: '', label: 'Off' },
    { value: 'silhouette', label: 'Silhouette' },
    { value: 'wobbly', label: 'Wobbly' },
  ]);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.overrides[0].kind = 'slider'), /overrides\[0\]\.kind must be one of/],
    [(d) => (d.overrides[1].param = 'play'), /param play chooses what the page opens/],
    [(d) => (d.overrides[1].param = 'Seed'), /must be a lowercase URL parameter name/],
    [(d) => (d.overrides[2].param = 'world'), /param world is used twice/],
    [(d) => (d.overrides[0].options[0].value = 'terrain'), /must start with the default, value ""/],
    [(d) => (d.overrides[0].optionsFrom = 'outline'), /needs options or optionsFrom \(one of them\)/],
    [(d) => (d.overrides[1].max = -1), /max must be greater than min/],
    [(d) => (d.overrides[4].step = 0), /step must be positive/],
    [(d) => (d.overrides[3].colour = 'red'), /unknown entry overrides\[3\]\.colour/],
    [(d) => delete d.overrides[3].label, /missing entry overrides\[3\]\.label/],
    [(d) => (d.overrides[6].optionsFrom = 'shadows'), /optionsFrom must be one of outline, pixelart/],
  ];
  for (const [change, message] of cases) {
    const d = data();
    change(d);
    assert.throws(() => validateOverrides(d), message);
  }
});
