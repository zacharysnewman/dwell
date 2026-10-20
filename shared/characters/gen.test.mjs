// The character data's validation, composition and animation (node --test shared/characters/).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { loadData, loadStyle, loadTextures, PAGE, renderPage } from './gen.mjs';
import { bounds, chainProblem, chainSegments, compose, crossfade, layer, mirrorName, pivots, sample, validate } from './rig.mjs';

const style = loadStyle();
const load = () => loadData();

test('the checked-in data is valid and the Character Lab page is up to date', () => {
  const data = validate(load(), style);
  const page = readFileSync(PAGE, 'utf8');
  assert.equal(renderPage(page, data, loadTextures()), page, 'run `node shared/characters/gen.mjs`');
});

test('a bad entry fails with its path named', () => {
  const cases = [
    [(d) => (d.characters.player.parts[0].kind = 'torus'), /characters\.player\.parts\[0\] \(head\)\.kind: "torus" is not one of box, round_box/],
    [(d) => (d.characters.player.parts[0].on = 'tail'), /parts\[0\] \(head\): on "tail" is not a bone or socket of character "player"/],
    [(d) => (d.characters.player.parts[0].material = 'velvet_purple'), /material "velvet_purple" is not a slot of character "player"/],
    [(d) => (d.characters.player.parts[0].size = [0.3, -1, 0.3]), /\(head\)\.size: must be three numbers/],
    [(d) => (d.characters.player.parts[0].tip = 0.5), /only a cone takes "tip"/],
    [(d) => (d.characters.player.parts[0].colour = 'red'), /parts\[0\]: unknown entry "colour"/],
    [(d) => (d.characters.player.parts[1].id = 'head'), /part id "head" is repeated/],
    [(d) => (d.characters.player.parts[3].id = 'brow'), /a mirrored part's id ends in "_l"/],
    [(d) => delete d.characters.wolf.bones.tail_2, /characters\.wolf\.bones: missing the rest offset of bone "tail_2"/],
    [(d) => (d.characters.player.height = 2.4), /characters\.player\.height: the rest pose without accessories is 1\.8\d\d m tall, not 2\.4 m/],
    [(d) => (d.characters.player.bones.root = [0, 0.2, 0]), /not on the ground/],
    [(d) => d.characters.player.variants.ranger.accessories.push('wide_brim_hat'), /variants\.ranger: "wide_brim_hat" and "hood" both take the head/],
    [(d) => d.characters.player.variants.ranger.accessories.push('antlers'), /accessory "antlers" doesn't fit rig "biped"/],
    [(d) => (d.characters.player.variants.ranger.slots.cloak = 'leather'), /variants\.ranger\.slots\.cloak: no slot "cloak"/],
    [(d) => (d.accessories.hood.hides = ['fringe']), /accessories\.hood\.hides: "fringe" is not a part of character "player"/],
    [(d) => (d.rigs.biped.bones[2].parent = 'head'), /parent "head" of "chest" is not an earlier bone/],
    [(d) => (d.rigs.biped.bones[5].joint = 'swivel'), /bones\[5\]\.joint: "swivel" is not one of ball, hinge, fixed/],
    [(d) => (d.clips.biped_walk.keys[1].bones.shin_r = [0, 20, 0]), /"shin_r" is a hinge about x only/],
    [(d) => (d.clips.biped_walk.keys[1].bones.shin_r = [-30, 0, 0]), /x -30° is outside "shin_r"'s limits 0–150°/],
    [(d) => (d.clips.biped_walk.keys[1].bones.tail_1 = [0, 0, 0]), /"tail_1" is not a bone of rig "biped"/],
    [(d) => (d.clips.biped_walk.keys[2].t = 0.1), /keys must be in increasing time/],
    [(d) => (d.clips.biped_idle.keys[1].t = 3), /a loop's keys come before its duration/],
    [(d) => (d.characters.player.clips.walk = 'wolf_trot'), /clip "wolf_trot" is for rig "quadruped", not "biped"/],
    [(d) => (d.characters.player.clips.walk = 'fp_walk'), /plays on the arms; list it under firstPerson\.clips/],
    [(d) => (d.clips.fp_idle.keys[0].bones.chest = [0, 0, 0]), /keys "chest", which is not in the arm chain/],
    [(d) => d.characters.player.firstPerson.bones.splice(1, 1), /"hand_r"'s parent is not in the chain/],
    [(d) => (d.characters.player.parts[0].with = 'hair_top'), /\(head\)\.with: "hair_top" is not an earlier part of character "player"/],
    [(d) => (d.accessories.hood.parts[0].with = 'hood_brow'), /"with" is for a character's own parts/],
    [(d) => (d.characters.wolf.chains = { tail: { from: 'haunch', parts: ['tail_base'] } }), /characters\.wolf\.chains\.tail\.parts: must list two or more part ids/],
    [(d) => (d.characters.wolf.chains = { tail: { from: 'haunch', parts: ['tail_base', 'tail_plume'] } }), /chains\.tail\.parts: "tail_plume" is not a part of character "wolf"/],
    [(d) => (d.characters.wolf.chains = { tail: { from: 'rump', parts: ['tail_base', 'tail_tip'] } }), /chains\.tail\.from: "rump" is not a part of character "wolf"/],
    [(d) => (d.characters.wolf.chains = { tail: { from: 'haunch', parts: ['tail_base', 'muzzle'] } }), /chains\.tail: in the rest pose "tail_base" ends \d\.\d+ m from where "muzzle" starts/],
    [(d) => (d.characters.wolf.chains = { tail: { from: 'muzzle', parts: ['tail_base', 'tail_tip'] } }), /chains\.tail: in the rest pose "tail_base" starts \d\.\d+ m off "muzzle", which it grows from/],
    [(d) => (d.clips.biped_idle.grounded = 'yes'), /clips\.biped_idle\.grounded: must be true or false/],
    [(d) => (d.clips.biped_break.grounded = true), /only a base clip stands on something/],
    [(d) => (d.clips.biped_walk.blend = 5), /clips\.biped_walk\.blend: must be seconds, 0–2/],
  ];
  for (const [edit, message] of cases) {
    const d = load();
    edit(d);
    assert.throws(() => validate(d, style), message);
  }
});

test('a part added by data alone is composed, drawn on its bone and mirrored', () => {
  const d = load();
  d.characters.player.parts.push({
    id: 'pauldron_l',
    on: 'arm_l',
    kind: 'round_box',
    size: [0.2, 0.08, 0.2],
    at: [0.02, 0.02, 0],
    material: 'metal_iron',
    mirror: true,
  });
  validate(d, style);
  const m = compose(d, style, 'player', { accessories: [] });
  const left = m.parts.find((p) => p.id === 'pauldron_l');
  const right = m.parts.find((p) => p.id === 'pauldron_r');
  assert.deepEqual([left.bone, left.at, left.material], ['arm_l', [0.02, 0.02, 0], 'metal_iron']);
  assert.deepEqual([right.bone, right.at], ['arm_r', [-0.02, 0.02, 0]]);
  // It raises nothing above the head, but now reaches past the old shoulders.
  const before = bounds(compose(load(), style, 'player', { accessories: [] }));
  const after = bounds(m);
  assert.ok(after.max[0] > before.max[0], 'the pauldron sticks out past the arm');
  assert.equal(after.max[1], before.max[1]);
});

test('an accessory and a variant added by data alone are worn', () => {
  const d = load();
  d.accessories.top_hat = {
    label: 'Top hat',
    place: 'head',
    fits: ['biped'],
    slots: { hat: 'cloth_wool_blue' },
    hides: ['hair_top'],
    parts: [{ id: 'top_hat', on: 'head_top', kind: 'cylinder', size: [0.3, 0.3, 0.3], at: [0, 0.15, 0], material: 'hat' }],
  };
  d.characters.player.variants.dandy = { label: 'Dandy', accessories: ['top_hat'], slots: { hat: 'metal_brass' } };
  validate(d, style);
  const m = compose(d, style, 'player', { variant: 'dandy' });
  assert.equal(m.parts.find((p) => p.id === 'top_hat').material, 'metal_brass');
  assert.ok(!m.parts.some((p) => p.id === 'hair_top'), 'the hat hides the hair it covers');
  assert.ok(bounds(m).max[1] > 2.1, 'the hat stands on the head');
});

test('a held item rides the grip socket; the first-person chain shares the body\'s arm parts', () => {
  const d = load();
  const m = compose(d, style, 'player', { item: 'block' });
  const block = m.parts.find((p) => p.id === 'held_block');
  assert.equal(block.bone, 'hand_r');
  assert.equal(block.material, '$block');
  const chain = m.firstPerson.bones;
  const arms = m.parts.filter((p) => chain.includes(p.bone) && p.view !== 'body');
  const body = m.parts.filter((p) => chain.includes(p.bone) && p.view !== 'arms');
  // Everything the body draws on the arm is drawn in first person too (arms-only parts add detail).
  for (const p of body) assert.ok(arms.includes(p), `${p.id} is in the arms view`);
  assert.ok(arms.some((p) => p.view === 'arms'), 'there is arms-only detail');
});

test('mirrored names and the rest pose', () => {
  assert.equal(mirrorName('arm_l'), 'arm_r');
  assert.equal(mirrorName('head'), undefined);
  const m = compose(load(), style, 'player', {});
  const piv = pivots(m);
  assert.deepEqual(piv.get('head').at.map((x) => +x.toFixed(3)), [0, 1.45, 0]);
  assert.deepEqual(piv.get('arm_r').at.map((x) => +x.toFixed(3)), [-0.25, 1.37, 0]);
});

test('clips sample between keys, loop, and layer by replacing the bones they key', () => {
  const d = load();
  const walk = d.clips.biped_walk;
  assert.deepEqual(sample(walk, 0).bones.thigh_l, [-28, 0, 0]);
  assert.deepEqual(sample(walk, 1).bones.thigh_l, [-28, 0, 0], 'a loop wraps to its first key');
  const mid = sample(walk, 0.125).bones.thigh_l[0];
  assert.ok(mid > -28 && mid < -2, `between the keys (${mid})`);
  const smooth = { ...d.clips.biped_place };
  assert.deepEqual(sample(smooth, 0.07).bones.arm_r, [-46, 0, -3], 'smooth is symmetric at the midpoint');
  const pose = layer([{ pose: sample(walk, 0) }, { pose: sample(d.clips.biped_hold, 0) }]);
  assert.deepEqual(pose.bones.arm_r, [-20, 0, -4], 'hold replaces the arm');
  assert.deepEqual(pose.bones.thigh_l, [-28, 0, 0], 'and leaves the legs walking');
  const half = layer([{ pose: sample(walk, 0) }, { pose: sample(d.clips.biped_hold, 0), weight: 0.5 }]);
  assert.deepEqual(half.bones.arm_r, [-21, 0, -5]);
});

test('every grounded clip keeps the feet of a standing character near the ground', () => {
  // A walk, trot, crouch or slide that drives a foot through the ground or lifts both off it is a bad
  // clip. Clips in the air, in water or on a ladder (jump, fall, climb, swim, fly) say "grounded": false.
  const d = load();
  for (const [id, ch] of Object.entries(d.characters)) {
    const m = compose(d, style, id, { accessories: [] });
    for (const c of Object.values(ch.clips ?? {})) {
      const clip = d.clips[c];
      if (clip.layer !== 'base' || clip.grounded === false) continue;
      for (let i = 0; i < 16; i++) {
        const b = bounds(m, 'body', sample(clip, (i / 16) * clip.duration));
        assert.ok(b.min[1] > -0.06 && b.min[1] < 0.06, `${id} ${c} at ${((i / 16) * clip.duration).toFixed(2)} s: lowest point ${b.min[1].toFixed(3)} m`);
      }
    }
  }
});

// ---- Hair under hats (owner's review, 2026-10-20) ------------------------------------------------------
/** Every look a character can wear: [name, compose options]. */
const looksOf = (d, id) => [
  ['default', {}],
  ...Object.keys(d.characters[id].variants ?? {}).map((v) => [v, { variant: v }]),
];
const partBounds = (m, id) => bounds({ ...m, parts: m.parts.filter((p) => p.id === id) });

test('no hair is left floating when the hair it hangs from is hidden', () => {
  const d = load();
  for (const id of Object.keys(d.characters)) {
    for (const [name, opts] of looksOf(d, id)) {
      const m = compose(d, style, id, opts);
      const drawn = new Set(m.parts.filter((p) => p.from === 'body').map((p) => p.id));
      for (const p of m.parts)
        if (p.from === 'body' && p.with) assert.ok(drawn.has(p.with), `${id} ${name}: "${p.id}" is drawn but "${p.with}", which it hangs from, is hidden`);
    }
  }
  // The fringe hangs from the top hair, so every hat that hides the top hair takes the fringe with it.
  const fringe = d.characters.player.parts.find((p) => p.id === 'hair_fringe');
  assert.equal(fringe.with, 'hair_top', 'the fringe hangs from the top hair');
  for (const [a, acc] of Object.entries(d.accessories)) {
    if (!(acc.hides ?? []).includes('hair_top')) continue;
    const m = compose(d, style, 'player', { accessories: [a] });
    assert.ok(!m.parts.some((p) => p.id === 'hair_fringe'), `${a} hides the top hair, so the fringe goes too`);
  }
});

test('the wide-brimmed hat tucks its own fringe under the brim; the hood shows none', () => {
  const d = load();
  const m = compose(d, style, 'player', { accessories: ['wide_brim_hat'] });
  const hair = d.characters.player.slots.hair;
  const tucked = m.parts.filter((p) => p.from === 'wide_brim_hat' && p.material === hair);
  assert.ok(tucked.length > 0, 'the hat brings hair of its own, painted with the hair slot');
  // Its top tucked into the brim (no higher than the brim's middle): hair pushed down by the
  // hat, not a bar floating on the face.
  const bb = partBounds(m, 'hat_brim');
  const brim = (bb.min[1] + bb.max[1]) / 2; // the brim's middle: its rim tilts, so its box is no guide
  for (const p of tucked) assert.ok(partBounds(m, p.id).max[1] <= brim, `${p.id} sits under the brim`);
  // It recolours with the look: the ranger's black hair would paint it black too.
  const tinted = compose(d, style, 'player', { accessories: ['wide_brim_hat'], slots: { hair: 'hair_black' } });
  assert.ok(tinted.parts.filter((p) => p.from === 'wide_brim_hat').some((p) => p.material === 'hair_black'));
  const r = compose(d, style, 'player', { variant: 'ranger' });
  assert.ok(!r.parts.some((p) => p.id.includes('fringe')), 'the hood covers the forehead: no fringe');
});

// ---- Chains: tails (owner's review, 2026-10-20) ---------------------------------------------------------
test('every quadruped declares its tail as a chain', () => {
  const d = load();
  for (const [id, ch] of Object.entries(d.characters)) {
    if (ch.rig !== 'quadruped') continue;
    assert.ok(ch.chains?.tail?.parts?.length >= 2, `${id} lists its tail's segments, root first, under chains.tail`);
  }
});

test("a chain's segments stay joined end to end and bend smoothly, at rest and through every clip", () => {
  const d = load();
  for (const [id, ch] of Object.entries(d.characters)) {
    for (const [name, opts] of looksOf(d, id)) {
      const m = compose(d, style, id, opts);
      for (const [cn, chain] of Object.entries(ch.chains ?? {})) {
        assert.equal(chainProblem(m, chain), undefined, `${id} ${name} ${cn} at rest`);
        for (const c of Object.values(ch.clips ?? {})) {
          const clip = d.clips[c];
          for (let i = 0; i <= 24; i++) {
            const t = (i / 24) * clip.duration;
            const bad = chainProblem(m, chain, sample(clip, t));
            assert.equal(bad, undefined, `${id} ${name} ${cn} in ${c} at ${t.toFixed(2)} s: ${bad}`);
          }
        }
      }
    }
  }
});

test('a segment turned across its chain, or moved off its root, is caught', () => {
  for (const id of ['wolf', 'deer']) {
    const d = load();
    const ch = d.characters[id];
    const m = compose(d, style, id, { accessories: [] });
    assert.equal(chainSegments(m, ch.chains.tail).length, ch.chains.tail.parts.length);
    const p = ch.parts.find((x) => x.id === ch.chains.tail.parts[1]);
    p.rot = [(p.rot?.[0] ?? 0) + 90, 0, 0];
    assert.throws(() => validate(d, style), /chains\.tail: in the rest pose/, `${id}: turned`);
    const e = load();
    e.characters[id].bones.tail_1 = [0, 0.1, -0.6];
    assert.throws(() => validate(e, style), /chains\.tail: in the rest pose .* off /, `${id}: moved off`);
  }
});

// ---- Player clips for the controller's states (owner's review, 2026-10-20) -------------------------------
// PLAYER_CONTROLLER.md §3: Idle, Walking, Running, Crouching, Sliding, Jumping, Falling, Climbing,
// Swimming, Flying, and the JUMPED and LANDED events. Each has a role; crouching has two (still, moving).
const BODY_ROLES = ['idle', 'walk', 'sprint', 'crouch', 'crouch_walk', 'slide', 'jump', 'fall', 'land', 'climb', 'swim', 'fly'];
const ARMS_ROLES = ['idle', 'walk', 'sprint', 'slide', 'jump', 'fall', 'land'];

test('the player has a body clip for every controller state, and the arms the ones that show', () => {
  const d = load();
  const p = d.characters.player;
  for (const r of BODY_ROLES) assert.ok(p.clips[r], `body role "${r}"`);
  for (const r of ARMS_ROLES) assert.ok(p.firstPerson.clips[r], `arms role "${r}"`);
  for (const r of ['jump', 'fall', 'climb', 'swim', 'fly']) assert.equal(d.clips[p.clips[r]].grounded, false, `${r} is not grounded`);
  for (const r of ['idle', 'walk', 'sprint', 'crouch', 'crouch_walk', 'slide', 'land'])
    assert.notEqual(d.clips[p.clips[r]].grounded, false, `${r} is grounded`);
  // Gaits play at speed / cycle: the sprint covers more ground a loop than the walk, the crouch-walk less.
  const cyc = (r) => d.clips[p.clips[r]].cycle;
  assert.ok(cyc('sprint') > cyc('walk') && cyc('crouch_walk') < cyc('walk'), 'cycles');
  // Crouched (the 0.9 m crouch capsule): the top stays low through crouch, crouch-walk and the slide.
  const m = compose(d, style, 'player', { accessories: [] });
  for (const r of ['crouch', 'crouch_walk', 'slide']) {
    const clip = d.clips[p.clips[r]];
    for (let i = 0; i < 8; i++) {
      const top = bounds(m, 'body', sample(clip, (i / 8) * clip.duration)).max[1];
      assert.ok(top < (r === 'slide' ? 1.4 : 1.3), `${r}: top ${top.toFixed(2)} m`);
    }
  }
  // The slide holds an arm out for balance: a hand well out to the side.
  const slide = pivots(m, sample(d.clips[p.clips.slide], 0.2));
  const reach = Math.max(Math.abs(slide.get('hand_l').at[0]), Math.abs(slide.get('hand_r').at[0]));
  assert.ok(reach > 0.45, `a hand out to the side (${reach.toFixed(2)} m)`);
  // The jump crouches to take off and ends airborne, the land starts tucked and ends standing.
  const land = d.clips[p.clips.land];
  assert.equal(land.loop, false);
  assert.equal(d.clips[p.clips.jump].loop, false);
});

test('switching base clips cross-fades every bone either keys', () => {
  const d = load();
  const walk = sample(d.clips.biped_walk, 0);
  const slide = sample(d.clips[d.characters.player.clips.slide], 0.2);
  const half = crossfade(walk, slide, 0.5);
  for (const n of new Set([...Object.keys(walk.bones), ...Object.keys(slide.bones)])) {
    const a = walk.bones[n] ?? [0, 0, 0];
    const b = slide.bones[n] ?? [0, 0, 0];
    assert.deepEqual(half.bones[n], [0, 1, 2].map((j) => a[j] + (b[j] - a[j]) / 2), n);
  }
  assert.deepEqual(crossfade(walk, slide, 0).bones.thigh_l, walk.bones.thigh_l);
  assert.deepEqual(crossfade(walk, slide, 1).root, slide.root ?? [0, 0, 0]);
});
