// Tests for the ground items' generator (docs/adr/0071-ground-items.md): run with `node --test shared/ground/`.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadBlocks, loadInventory } from '../inventory/gen.mjs';
import { groundRust, loadGround, validateGround } from './gen.mjs';

const fresh = () => [structuredClone(loadGround()), loadBlocks(), loadInventory()];

test('the shipped data validates and emits the kinds and rules', () => {
  const [g, b, i] = fresh();
  const rs = groundRust(validateGround(g, b, i));
  assert.match(rs, /GroundKind \{ name: "dwell:ground_stick", item: "dwell:stick", shape: GroundShape::Rod/);
  assert.match(rs, /under: Some\(Under \{ blocks: &\["dwell:leaves", "dwell:snowy_leaves"\], within_rows: 16 \}\)/);
  assert.match(rs, /near_wall: Some\(NearWall \{ rows: 6, reach: 2 \}\)/);
});

test('a kind and a rule added by data alone are emitted', () => {
  const [g, b, i] = fresh();
  g.kinds.push({ name: 'test:pebble', item: 'dwell:rock', shape: 'lump', size: [0.05, 0.03, 0.05], texture: 'dwell:sand', color: '#ffffff' });
  g.rules.push({ kind: 'test:pebble', on: ['dwell:sand'], density: 0.5 });
  const rs = groundRust(validateGround(g, b, i));
  assert.match(rs, /GroundKind \{ name: "test:pebble", item: "dwell:rock", shape: GroundShape::Lump, size: \[0.05, 0.03, 0.05\], texture: "dwell:sand", color: \[255, 255, 255\], scale: \[1.0, 1.0\] \}/);
  assert.match(rs, /GroundRule \{ kind: 2, on: &\["dwell:sand"\], under: None, near_wall: None, density: 0.5 \}/);
});

test('bad entries fail naming the entry and the problem', () => {
  const cases = [
    [(g) => (g.kinds[0].item = 'dwell:diamond'), /kinds\.dwell:ground_stick: item "dwell:diamond" is neither/],
    [(g) => (g.kinds[0].shape = 'cube'), /kinds\.dwell:ground_stick: shape must be one of rod, lump/],
    [(g) => (g.kinds[1].size = [0.1, 0, 0.1]), /kinds\.dwell:ground_rock: size must be/],
    [(g) => (g.kinds[1].texture = 'dwell:marble'), /kinds\.dwell:ground_rock\.texture: "dwell:marble" is not a block/],
    [(g) => (g.kinds[0].colour = '#000000'), /kinds\.dwell:ground_stick: unknown field colour/],
    [(g) => (g.rules[0].kind = 'dwell:twig'), /rules\[0\]: kind "dwell:twig" is not in kinds/],
    [(g) => (g.rules[0].on = ['dwell:moss']), /rules\[0\]\.on\[0\]: "dwell:moss" is not a block/],
    [(g) => (g.rules[0].under.withinRows = 17), /rules\[0\]\.under\.withinRows must be a whole number in \[1, 16\]/],
    [(g) => (g.rules[1].nearWall.reach = 7), /rules\[1\]\.nearWall\.reach must be a whole number in \[1, 6\]/],
    [(g) => (g.rules[2].density = 0), /rules\[2\]: density must be in \(0, 1\]/],
  ];
  for (const [edit, message] of cases) {
    const [g, b, i] = fresh();
    edit(g);
    assert.throws(() => validateGround(g, b, i), message);
  }
});
