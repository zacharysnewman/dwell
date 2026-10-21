// node --test shared/look/motes.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { moteNames, motesRust, validateMotes } from './motes.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'motes.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(motesRust(validateMotes(data())), checkedIn('crates/dwell-look/src/motes.gen.rs'));
  assert.deepEqual(moteNames(data()), ['dwell:portal']);
});

test('a mote look added by data alone reaches the Rust table (a fixture of green fireflies)', () => {
  const d = data();
  d.motes['test:fireflies'] = {
    color: '#80ff60',
    core: '#f0ffe0',
    count: 12,
    cpuCount: 4,
    sizeM: 0.03,
    radiusM: 1,
    heightM: 2,
    swirl: 0,
    drift: 0.1,
    wobbleM: 0.5,
    glow: 1,
    pulseHz: 1.5,
    pulseDepth: 1,
  };
  d.drawM = 32;
  const rs = motesRust(validateMotes(d));
  assert.match(rs, /pub static MOTES: \[MoteLook; 2\]/);
  assert.match(rs, /name: "test:fireflies",\n {8}color: 0x80ff60,\n {8}core: 0xf0ffe0,\n {8}count: 12,\n {8}cpu_count: 4,/);
  assert.match(rs, /draw_m: 32\.0,/);
  assert.deepEqual(moteNames(d), ['dwell:portal', 'test:fireflies']);
});

test('bad entries fail with their path named', () => {
  const portal = (d) => d.motes['dwell:portal'];
  const cases = [
    [(d) => (portal(d).color = 'blue'), /motes\.dwell:portal\.color must be #rrggbb/],
    [(d) => (portal(d).count = 2.5), /motes\.dwell:portal\.count must be a whole number/],
    [(d) => (portal(d).cpuCount = 999), /motes\.dwell:portal\.cpuCount must be a number in \[0, 256\]/],
    [(d) => (portal(d).cpuCount = portal(d).count + 1), /cpuCount must be at most count/],
    [(d) => (portal(d).sizeM = 0), /motes\.dwell:portal\.sizeM must be a number in \(0, 0\.5\]/],
    [(d) => (portal(d).pulseDepth = 2), /motes\.dwell:portal\.pulseDepth must be a number in \[0, 1\]/],
    [(d) => delete portal(d).glow, /missing entry motes\.dwell:portal\.glow/],
    [(d) => (portal(d).sparkle = 1), /unknown entry motes\.dwell:portal\.sparkle/],
    [(d) => (d.motes.Portal = portal(d)), /motes\.Portal: the name must be/],
    [(d) => (d.maxMotes = 0), /maxMotes must be a number in \[1, 4096\]/],
    [(d) => (d.motes = {}), /at least one look/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateMotes(d), re);
  }
});
