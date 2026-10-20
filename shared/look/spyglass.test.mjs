// node --test shared/look/spyglass.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spyglassRust, spyglassTs, validateSpyglass } from './spyglass.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'spyglass.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in tunables validate and generate the checked-in Rust and TypeScript', () => {
  const d = validateSpyglass(data());
  assert.equal(spyglassRust(d), checkedIn('crates/dwell-look/src/spyglass.gen.rs'));
  assert.equal(spyglassTs(d), checkedIn('client/src/ui/spyglass.gen.ts'));
});

test('a step added by data alone is in both outputs', () => {
  const d = data();
  d.steps.push({ magnification: 24 });
  d.vignette.opacity = 0.4;
  const rs = spyglassRust(validateSpyglass(d));
  assert.match(rs, /SpyglassStep \{ magnification: 24\.0 \},\n {4}\],/);
  assert.match(rs, /opacity: 0\.4,/);
  assert.match(spyglassTs(d), /magnifications: \[4, 8, 24\]/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.key = 'KeyF'), /key KeyF is already another control's/],
    [(d) => (d.key = 'F3'), /key F3 is already another control's/],
    [(d) => (d.key = 'Digit0'), /key Digit0 is already another control's/],
    [(d) => (d.key = 'a b'), /key must be a KeyboardEvent.code/],
    [(d) => (d.steps = []), /steps must be a list of 1 to 8 steps/],
    [(d) => (d.steps[1].magnification = 3), /steps\[1\]\.magnification must be greater than 4/],
    [(d) => (d.steps[0].magnification = 1), /steps\[0\]\.magnification must be greater than 1/],
    [(d) => (d.steps[0].fov = 10), /unknown entry steps\[0\]\.fov/],
    [(d) => (d.steps[0].magnification = 100), /steps\[0\]\.magnification must be a number in \[1, 64\]/],
    [(d) => delete d.detailBoost, /missing entry \(root\)\.detailBoost/],
    [(d) => (d.transitionS = -1), /transitionS must be a number in \[0, 5\]/],
    [(d) => (d.vignette.color = 'black'), /vignette\.color must be a colour/],
    [(d) => (d.vignette.outerRadius = 0.5), /vignette\.outerRadius must be greater than vignette\.innerRadius/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateSpyglass(d), message);
  }
});
