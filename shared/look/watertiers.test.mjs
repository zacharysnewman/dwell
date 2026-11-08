// node --test shared/look/watertiers.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateWaterTiers, waterTiersRust } from './watertiers.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'watertiers.json'), 'utf8'));

test('the checked-in tiers validate and generate the checked-in Rust', () => {
  const rs = waterTiersRust(validateWaterTiers(data()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/watertiers.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn);
});

test("the owner's tiers (2026-11-07): an opaque CPU tier, a see-through Low, Medium; High an alias", () => {
  const d = validateWaterTiers(data());
  assert.deepEqual(
    d.tiers.map((t) => t.id),
    ['opaque', 'low', 'medium'],
  );
  const [opaque, low, medium] = d.tiers;
  assert.ok(opaque.cpu && !opaque.readsScene && !opaque.setting);
  assert.ok(low.readsScene && low.bankBand && !low.flowStreaks && low.rippleLayers === 1);
  assert.ok(medium.readsScene && medium.flowStreaks && medium.rippleLayers === 3);
  assert.equal(d.aliases.high, 'medium');
});

test('a tier added by data alone reaches the Rust', () => {
  const d = data();
  d.tiers.push({ ...d.tiers[2], id: 'glass', label: 'Glass', rippleLayers: 2 });
  d.aliases.crystal = 'glass';
  const rs = waterTiersRust(validateWaterTiers(d));
  assert.match(rs, /id: "glass",/);
  assert.match(rs, /\("crystal", "glass"\)/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.tiers[1].rippleLayers = 4), /tiers\[1\]\.rippleLayers must be a number in \[1, 3\]/],
    [(d) => (d.tiers[1].id = 'Low'), /tiers\[1\]\.id must be a lower-case name/],
    [(d) => d.tiers.push({ ...d.tiers[1] }), /"low" is listed twice/],
    [(d) => (d.tiers[0].cpu = false), /exactly one tier must be the CPU adapter's/],
    [(d) => (d.tiers[1].cpu = true), /the CPU adapter's tier may not read the scene/],
    [(d) => (d.tiers[0].bankBand = true), /bankBand needs readsScene/],
    [(d) => (d.tiers[2].speed = 1), /unknown entry tiers\[2\]\.speed/],
    [(d) => delete d.tiers[2].flowStreaks, /missing entry tiers\[2\]\.flowStreaks/],
    [(d) => (d.aliases.high = 'opaque'), /aliases\.high: opaque is not offered in Settings/],
    [(d) => (d.aliases.low = 'medium'), /aliases\.low: a listed tier can't be an alias/],
    [(d) => (d.aliases.ultra = 'glass'), /aliases\.ultra: unknown tier "glass"/],
    [(d) => (d.band.darken = 2), /band\.darken must be a number in \[0, 1\]/],
    [(d) => (d.causticEveryFrames = 0), /causticEveryFrames must be a number in \[1, 8\]/],
    [(d) => (d.tiers = []), /tiers must be a non-empty list/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateWaterTiers(d), message);
  }
});
