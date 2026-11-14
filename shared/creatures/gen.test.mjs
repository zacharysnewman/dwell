// node --test shared/creatures/gen.test.mjs: the creatures' data (docs/adr/0073-creatures.md).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { loadBlocks } from '../inventory/gen.mjs';
import { generate, loadAppearance, loadModels, loadPatterns, validateAppearance, validateCreatures, validatePatterns } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'creatures.json'), 'utf8'));
const blocks = loadBlocks();
const models = loadModels();

test('the data validates and the generated tables are current', () => {
  validateCreatures(data(), blocks, models);
  for (const [path, text] of generate()) assert.equal(readFileSync(path, 'utf8'), text, `${path} is stale: run node shared/creatures/gen.mjs`);
});

test('a fixture creature is added by data alone', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/creatures.json'), 'utf8'));
  const v = validateCreatures(d, blocks, models);
  assert.equal(v.creatures[0].name, 'test:walker');
});

test('a fixture NPC is added by data alone', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/npcs.json'), 'utf8'));
  const v = validateCreatures(d, blocks, models, undefined, undefined, undefined, undefined, loadAppearance('fixtures/appearance.json'), loadPatterns('fixtures/patterns.json'));
  assert.equal(v.creatures[0].npc.appearance.set, 'test:pair');
  assert.equal(v.creatures[0].npc.pattern.set, 'test:duo');
  assert.equal(v.creatures[0].npc.held, 'dwell:stone_axe');
  assert.equal(v.rules[0].spacing, 20);
});

test('the villager is the Peasant, its colours palette ramps (WORK_QUEUE item 38)', () => {
  const v = validateCreatures(data(), blocks, models).creatures.find((c) => c.name === 'dwell:villager');
  assert.equal(v.model, 'peasant');
  assert.deepEqual(
    v.npc.colours.map((c) => c.part),
    ['shirt', 'trousers', 'hat'],
  );
});

test('bad entries fail with a message naming them', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateCreatures(d, blocks, models), re);
  };
  bad((d) => (d.creatures[0].model = 'unicorn'), /creatures\.dwell:deer: model "unicorn"/);
  bad((d) => (d.rules[0].on = ['dwell:nope']), /rules\[0\]\.on\[0\]: "dwell:nope" is not a block/);
  bad((d) => (d.rules[0].creature = 'dwell:nope'), /rules\[0\]: creature "dwell:nope"/);
  bad((d) => (d.rules[0].group = [4, 2]), /rules\[0\]: group/);
  bad((d) => (d.creatures[0].idleSeconds = [5, 1]), /idleSeconds/);
  bad((d) => (d.creatures[0].walkSpeed = 0), /walkSpeed/);
  bad((d) => (d.limits.max = 0), /limits\.max/);
  bad((d) => (d.creatures[0].wings = 2), /unknown field wings/);
  const npc = (d) => d.creatures.find((c) => c.npc).npc;
  bad((d) => (npc(d).idles[0].pose = 'dance'), /idles\[0\]: pose "dance" is not one of stand, look, sit/);
  bad((d) => (npc(d).colours[0].to = ['neon']), /colours\[0\]\.to\[0\]: "neon" is not a palette ramp/);
  bad((d) => (npc(d).colours[1].from = npc(d).colours[0].from), /is another part's/);
  bad((d) => (npc(d).held = 'dwell:laser'), /held "dwell:laser" is not an item/);
  bad((d) => (npc(d).disposition = 'grumpy'), /disposition "grumpy"/);
  bad((d) => (npc(d).combat = 'flee'), /combat "flee"/);
  bad((d) => (npc(d).lookAtM = -1), /lookAtM/);
  bad((d) => (npc(d).fight.stance = 'dodge'), /fight: stance "dodge" is not one of counter, random/);
  bad((d) => (npc(d).fight.giveUpS = 0), /fight: giveUpS must be in/);
  bad((d) => (npc(d).fight.parry = true), /fight: unknown field parry/);
  bad((d) => (d.rules[2].biomes = ['preset:moon']), /rules\[2\]\.biomes\[0\]: "preset:moon" is not a biome/);
  bad((d) => (d.rules[2].spacingM = -3), /rules\[2\]: spacingM/);
});

// WORK_QUEUE item 35: a creature's body to a weapon (health, hit radius, drops) is data.
test('a creature’s health, hit radius and drops are checked and emitted', () => {
  const v = validateCreatures(data(), blocks, models);
  const doe = v.creatures.find((c) => c.name === 'dwell:doe');
  assert.ok(doe.health > 0 && doe.hitRadiusM > 0);
  assert.deepEqual(doe.drops, [{ item: 'dwell:raw_meat', min: 1, max: 2 }]);
  const bad = (edit, re) => {
    const d = data();
    edit(d.creatures.find((c) => c.name === 'dwell:doe'));
    assert.throws(() => validateCreatures(d, blocks, models), re);
  };
  bad((c) => (c.drops = [{ item: 'dwell:nothing' }]), /creatures\.dwell:doe\.drops\[0\]: no item "dwell:nothing"/);
  bad((c) => (c.health = -1), /health must be in \[0, 10000\]/);
  bad((c) => (c.hitRadiusM = 0), /hitRadiusM must be in \(0, 4\]/);
  bad((c) => delete c.health, /drops need health/);
});

// WORK_QUEUE item 35, round 2: an NPC's fight rules are data, defaults filled for one that names none.
test('an NPC’s fight rules are emitted, with defaults when it names none', async () => {
  const { creaturesRust } = await import('./gen.mjs');
  const d = validateCreatures(data(), blocks, models);
  assert.match(creaturesRust(d, 'x'), /fight: FightDef \{ stance: FightStance::Counter, raise_on_windup: true, strike_back: true, strike_within_m: 2\.4, cooldown_s: 1\.2, give_up_s: 8\.0 \}/);
  const e = data();
  delete e.creatures.find((c) => c.npc).npc.fight;
  assert.match(creaturesRust(validateCreatures(e, blocks, models), 'x'), /fight: FightDef \{ stance: FightStance::Counter, [^}]*give_up_s: 10\.0 \}/);
});

// WORK_QUEUE item 35, round 2: wolves bite (`attack`) and a hit shows (`hurtLook`), both data.
test('a creature’s attack and hurt look are checked, defaulted and emitted', async () => {
  const { creaturesRust } = await import('./gen.mjs');
  const v = validateCreatures(data(), blocks, models);
  const wolf = v.creatures.find((c) => c.name === 'dwell:wolf');
  assert.ok(wolf.attack.reachM > 0 && wolf.attack.damage > 0 && wolf.attack.angryS > 0);
  assert.equal(v.creatures.find((c) => c.name === 'dwell:doe').attack, null, 'deer don’t bite');
  // The file's hurt look, the wolf's own field over it.
  assert.equal(wolf.hurtLook.flinchM, 0.18);
  assert.equal(wolf.hurtLook.tint, data().hurtLook.tint);
  const rs = creaturesRust(v, 'test');
  assert.match(rs, /name: "dwell:wolf".*attack: Some\(AttackDef \{ reach_m: 1\.3,/);
  assert.match(rs, /name: "dwell:doe".*attack: None, hurt_look: HurtLook \{ seconds: 0\.4, tint: \[0\.878, 0\.251, 0\.165\]/);
  // The fixture's walker pecks, by data alone, with the defaults filled.
  const fx = validateCreatures(JSON.parse(readFileSync(join(here, 'fixtures/creatures.json'), 'utf8')), blocks, models);
  assert.deepEqual(fx.creatures[0].attack, { reachM: 1, cooldownS: 0.5, damage: 2, knockbackMps: 0, liftMps: 0, angryS: 4, frontDeg: 120 });
  const bad = (edit, re) => {
    const d = data();
    edit(d, d.creatures.find((c) => c.name === 'dwell:wolf'));
    assert.throws(() => validateCreatures(d, blocks, models), re);
  };
  bad((_, c) => (c.attack.reachM = 0), /creatures\.dwell:wolf\.attack\.reachM must be in \(0, 4\]/);
  bad((_, c) => (c.attack.bite = 1), /bite/);
  bad((_, c) => delete c.attack.angryS, /angryS must be in/);
  bad((_, c) => ((c.health = 0), (c.drops = [])), /attack needs health/);
  bad((_, c) => (c.hurtLook = { tint: 'red' }), /hurtLook\.tint must be #rrggbb/);
  bad((d) => (d.hurtLook.strength = 2), /hurtLook\.strength must be in \[0, 1\]/);
  bad((d) => (d.hurtLook.glow = 1), /glow/);
});

// BUGS.md #152: the skin tones and the hair colours that go with each, as data.
const appearance = () => JSON.parse(readFileSync(join(here, 'appearance.json'), 'utf8'));

test('the appearance sets validate, and the villagers pick from one', () => {
  const sets = loadAppearance();
  const v = sets.get('villagers');
  assert.ok(v.tones.length >= 8, 'a natural range of skin tones');
  for (const t of v.tones) assert.ok(t.hair.length > 0 && t.hair.every((h) => /^#[0-9a-f]{6}$/.test(h.colour)), t.name);
  const villager = validateCreatures(data(), blocks, models).creatures.find((c) => c.name === 'dwell:villager');
  assert.equal(villager.npc.appearance.set, 'villagers');
  assert.ok(!villager.npc.colours.some((c) => c.part === 'skin' || c.part === 'hair'), 'skin and hair come from the set only');
  // The fixture set, by data alone.
  assert.deepEqual([...loadAppearance('fixtures/appearance.json').get('test:pair').tones.map((t) => t.name)], ['pale', 'dark']);
});

test('bad appearance entries fail naming the set and tone', () => {
  const bad = (edit, re) => {
    const d = appearance();
    edit(d);
    assert.throws(() => validateAppearance(d), re);
  };
  bad((d) => (d.sets.villagers.tones[0].base = 'pink'), /set villagers: tone porcelain: base must be #rrggbb/);
  bad((d) => (d.sets.villagers.tones[1].hair = []), /set villagers: tone fair: hair must list at least one colour/);
  bad((d) => (d.sets.villagers.tones[2].hair[0].colour = 'green'), /set villagers: tone rose: hair\[0\]: "green" is not a hair colour/);
  bad((d) => (d.hair.black = '#12'), /hair black must be #rrggbb/);
  bad((d) => (d.sets.villagers.tones = []), /set villagers: tones must list 1-32/);
  bad((d) => (d.sets.villagers.shade = 2), /set villagers: shade must be in \[0, 1\]/);
  bad((d) => d.sets.villagers.tones.push({ ...d.sets.villagers.tones[0] }), /tone porcelain: listed twice/);
  const npc = (edit, re) => {
    const d = data();
    edit(d.creatures.find((c) => c.name === 'dwell:villager').npc);
    assert.throws(() => validateCreatures(d, blocks, models), re);
  };
  npc((n) => (n.appearance.set = 'elves'), /appearance: set "elves" is not in appearance.json/);
  npc((n) => n.colours.push({ part: 'skin', from: 'skin_tan', to: ['skin_tan'] }), /appearance\.skin: part skin is also in colours/);
  npc((n) => (n.appearance.hair.from = 'nope'), /appearance\.hair: from "nope" is not a palette ramp/);
});

// WORK_QUEUE item 43 (owner, 2026-11-13: "a colorful day of the dead Mexico kind of vibes"): the
// pattern sets are data, checked with a message naming the bad entry.
test('the pattern sets are checked: sequences divide the places, no colour next to itself', () => {
  const sets = loadPatterns();
  const fiesta = sets.get('fiesta');
  assert.equal(fiesta.places, 4);
  for (const p of fiesta.patterns) {
    assert.ok(p.colours.length >= 2 && 4 % p.colours.length === 0, p.name);
    p.colours.forEach((c, i) => assert.notEqual(c, p.colours[(i + 1) % p.colours.length], p.name));
  }
  const ok = { formatVersion: 1, sets: { t: { places: 4, patterns: [{ name: 'a', colours: ['#111111', '#222222'] }] } } };
  assert.equal(validatePatterns(ok).get('t').patterns[0].weight, 1);
  const bad = (patch, re) => {
    const d = structuredClone(ok);
    Object.assign(d.sets.t.patterns[0], patch);
    assert.throws(() => validatePatterns(d), re);
  };
  bad({ colours: ['#111111', '#222222', '#333333'] }, /length divides the ring's 4 places/);
  bad({ colours: ['#111111', '#111111'] }, /next to itself/);
  bad({ colours: ['#111111', 'red'] }, /must be #rrggbb/);
  bad({ pairs: { shirt: ['nope'] } }, /neither a palette ramp nor #rrggbb/);
  // An NPC's pattern names the set's places, and a pattern pairs only its clothes parts.
  const d = JSON.parse(readFileSync(join(here, 'fixtures/npcs.json'), 'utf8'));
  d.creatures[0].npc.pattern.places.pop();
  assert.throws(
    () => validateCreatures(d, blocks, models, undefined, undefined, undefined, undefined, loadAppearance('fixtures/appearance.json'), loadPatterns('fixtures/patterns.json')),
    /places must list the set's 4 places/,
  );
});
