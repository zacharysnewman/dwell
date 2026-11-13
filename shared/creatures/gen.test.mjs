// node --test shared/creatures/gen.test.mjs: the creatures' data (docs/adr/0073-creatures.md).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { loadBlocks } from '../inventory/gen.mjs';
import { generate, loadModels, validateCreatures } from './gen.mjs';

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
  const v = validateCreatures(d, blocks, models);
  assert.equal(v.creatures[0].npc.held, 'dwell:stone_axe');
  assert.equal(v.rules[0].spacing, 20);
});

test('the villager is the Peasant, its colours palette ramps (WORK_QUEUE item 38)', () => {
  const v = validateCreatures(data(), blocks, models).creatures.find((c) => c.name === 'dwell:villager');
  assert.equal(v.model, 'peasant');
  assert.deepEqual(
    v.npc.colours.map((c) => c.part),
    ['shirt', 'trousers', 'skin', 'hair', 'hat'],
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
  bad((d) => (npc(d).idles[0].seat = { on: 'ground', levelM: 1 }), /idles\[0\]\.seat: only a sit idle takes a seat/);
  bad((d) => (npc(d).idles.find((i) => i.pose === 'sit').seat = { on: 'bench', levelM: 1 }), /seat: on "bench" is not one of ground/);
  bad((d) => (npc(d).idles.find((i) => i.pose === 'sit').seat = { on: 'ground', levelM: 9 }), /seat: levelM must be in \[0, 4\]/);
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
