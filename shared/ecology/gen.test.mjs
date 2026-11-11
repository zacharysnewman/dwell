// node --test shared/ecology/gen.test.mjs: the far ecology's data (docs/ECOLOGY.md).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { ecologyRust, farLooksRust, generate, validateEcology } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'ecology.json'), 'utf8'));

test('the data validates and the generated tables are current', () => {
  const v = validateEcology(data());
  assert.deepEqual(v.species.map((s) => s.name), ['dwell:deer', 'dwell:wolf']);
  for (const [path, text] of generate()) assert.equal(readFileSync(path, 'utf8'), text, `${path} is stale: run node shared/ecology/gen.mjs`);
});

test('species refer to each other by name, emitted as indices', () => {
  const v = validateEcology(data());
  assert.deepEqual(v.species[0].flee.from, [1]);
  assert.deepEqual(v.species[1].diet.prey.species, [0]);
  const rs = ecologyRust(v, 'x');
  assert.match(rs, /Diet::Prey \{ species: &\[0\]/);
  assert.match(rs, /flee: Some\(Flee \{ from: &\[1\]/);
  assert.match(rs, /creature: "dwell:doe"/);
  assert.match(rs, /max_slope_deg: 40\.0, search_m: 24\.0/);
  assert.match(rs, /block: Some\("dwell:tall_grass"\)/);
});

test('a species without `near` stays a record (the fixture\'s)', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/ecology.json'), 'utf8'));
  const rs = ecologyRust(validateEcology(d), 'x');
  assert.match(rs, /near: None/);
});

test('a fixture species is added by data alone', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/ecology.json'), 'utf8'));
  const v = validateEcology(d);
  assert.deepEqual(v.species.map((s) => s.name), ['test:hare', 'test:fox']);
  // A third species by data alone: a grazer with no predators.
  d.species.push({ ...d.species[0], name: 'test:vole', flee: undefined });
  delete d.species[2].flee;
  assert.equal(validateEcology(d).species[2].diet.graze.perHeadPerDay, 0.2);
});

test('bad entries fail with a message naming them', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateEcology(d), re);
  };
  bad((d) => (d.formatVersion = 2), /formatVersion must be 1/);
  bad((d) => (d.island.stepDays = 0), /island\.stepDays must be a number in \(0, 30\]/);
  bad((d) => (d.grazing.grounds[0].max = -1), /grazing\.grounds\.meadow\.max/);
  bad((d) => d.grazing.grounds.push({ ...d.grazing.grounds[0] }), /grazing\.grounds\.meadow: listed twice/);
  bad((d) => (d.species[0].group.dieBelow = 7), /species\.dwell:deer\.group: cap must be at least twice dieBelow/);
  bad((d) => (d.grazing.grounds[0].refuge = 2), /grazing\.grounds\.meadow\.refuge must be a number in \[0, 1\]/);
  bad((d) => delete d.species[1].diet.prey.halfSaturationHead, /species\.dwell:wolf\.diet\.prey\.halfSaturationHead must be a number/);
  bad((d) => (d.species[0].start.size = 40), /species\.dwell:deer\.start\.size/);
  bad((d) => (d.species[0].start.groups = 99), /species\.dwell:deer\.start\.groups/);
  bad((d) => (d.species[0].flee.from = ['dwell:bear']), /species\.dwell:deer\.flee\.from\[0\]: "dwell:bear" is not a species/);
  bad((d) => (d.species[1].diet.prey.species = ['dwell:wolf']), /a species cannot hunt itself/);
  bad((d) => (d.species[0].diet = { graze: { perHeadPerDay: 1 }, prey: {} }), /diet must name exactly one/);
  bad((d) => (d.species[0].disperse = { aboveHunger: 0.5, speedFactor: 2, leavePerDay: 0.01 }), /only a hunter disperses/);
  bad((d) => (d.species[0].wings = 2), /species\.dwell:deer: unknown field wings/);
  bad((d) => (d.species[1].name = 'Wolf'), /species\[1\]: name must be namespace:name/);
  bad((d) => (d.species[0].starvePerDay = 0.5), /species\.dwell:deer\.starvePerDay/);
  bad((d) => (d.near.foldM = d.near.spawnM), /near\.foldM must be past near\.spawnM/);
  bad((d) => (d.near.maxSlopeDeg = 95), /near\.maxSlopeDeg/);
  bad((d) => delete d.near.searchM, /near\.searchM/);
  bad((d) => (d.species[0].near.creature = 'Doe'), /species\.dwell:deer\.near\.creature must name a creature/);
  bad((d) => (d.species[0].near.hunt = d.species[1].near.hunt), /species\.dwell:deer\.near\.hunt: a grazer doesn't hunt/);
  bad((d) => (d.species[1].near.flee = { radiusM: 5, pace: 2 }), /species\.dwell:wolf\.near\.flee: the species flees nothing/);
  bad((d) => (d.species[0].near.graze.fullBelow = 0.9), /species\.dwell:deer\.near\.graze\.fullBelow/);
  bad((d) => (d.species[0].near.mate.chance = 2), /species\.dwell:deer\.near\.mate\.chance/);
  bad(
    (d) => {
      d.species.push({ ...d.species[1], name: 'dwell:bear', disperse: undefined });
      delete d.species[2].disperse;
      d.species[2].diet = { prey: { ...d.species[1].diet.prey, species: ['dwell:wolf'] } };
    },
    /"dwell:wolf" must graze/,
  );
});

test("a species' far look (how clients draw a far herd) is data, validated, and reaches the client", () => {
  const d = data();
  const rs = farLooksRust(validateEcology(d), 'x');
  assert.match(rs, /FarLook \{ species: "dwell:deer", color: \[154, 107, 69\], size_m: 1\.3, fade_start_m: 450\.0, fade_end_m: 700\.0 \}/);
  // A fixture species' look by data alone.
  d.species.push({ ...d.species[0], name: 'test:elk', far: { color: '#102030', sizeM: 2, fadeStartM: 10, fadeEndM: 20 } });
  delete d.species[2].flee;
  delete d.species[2].near;
  assert.match(farLooksRust(validateEcology(d), 'x'), /species: "test:elk", color: \[16, 32, 48\], size_m: 2\.0/);
  const bad = (far, re) => {
    const e = data();
    e.species[0].far = far;
    assert.throws(() => validateEcology(e), re);
  };
  bad({ color: 'tan', sizeM: 1, fadeStartM: 1, fadeEndM: 2 }, /far\.color must be a #rrggbb colour/);
  bad({ color: '#102030', sizeM: 1, fadeStartM: 5, fadeEndM: 2 }, /far\.fadeEndM must be past fadeStartM/);
  bad({ color: '#102030', sizeM: 1, fadeStartM: 5, fadeEndM: 9, glow: 1 }, /glow/);
});
