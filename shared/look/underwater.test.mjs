// node --test shared/look/underwater.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { underwaterRust, validateUnderwater } from './underwater.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'underwater.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in looks validate and generate the checked-in Rust', () => {
  assert.equal(underwaterRust(validateUnderwater(data())), checkedIn('crates/dwell-look/src/underwater.gen.rs'));
});

test('a murky fluid added by data alone reaches the output', () => {
  const d = data();
  d.fluids['test:mud'] = {
    lensM: 0.1,
    line: { amplitudeM: 0, wavelengthM: 1, speed: 0 },
    meniscus: { color: '#806040', strength: 0, widthPx: 1 },
    fog: { clearM: 0, density: 4 },
  };
  const rs = underwaterRust(validateUnderwater(d));
  assert.match(rs, /fluid: "test:mud",[\s\S]*fog_density: 4\.0,/);
});

test('bad entries fail with their path named', () => {
  const w = 'dwell:water';
  const cases = [
    [(d) => (d.fluids[w].fog.density = 0), /fluids\.dwell:water\.fog\.density must be a number in \(0, 16\]/],
    [(d) => (d.fluids[w].fog.clearM = -1), /fluids\.dwell:water\.fog\.clearM must be a number/],
    [(d) => (d.fluids[w].meniscus.strength = 2), /meniscus\.strength must be a number in \[0, 1\]/],
    [(d) => (d.fluids[w].meniscus.color = 'white'), /meniscus\.color must be #rrggbb/],
    [(d) => ((d.fluids[w].lensM = 0.05), (d.fluids[w].line.amplitudeM = 0.1)), /line\.amplitudeM must be under lensM/],
    [(d) => (d.fluids[w].lensM = 0), /lensM must be a number in \[0\.01, 2\]/],
    [(d) => (d.fluids.Water = d.fluids[w]), /fluids\.Water: the name must be/],
    [(d) => (d.default.fog.extra = 1), /unknown entry default\.fog\.extra/],
    [(d) => delete d.fluids[w].meniscus, /missing entry fluids\.dwell:water\.meniscus/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateUnderwater(d), message);
  }
});

test("water's waterline band is 4 px wide (owner, 2026-11-07: 'a little thicker')", () => {
  assert.equal(data().fluids['dwell:water'].meniscus.widthPx, 4);
});
