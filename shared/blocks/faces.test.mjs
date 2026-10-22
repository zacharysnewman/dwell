// Tests for the face-tile rules of block data (faces.mjs): `sideUnder`.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { blockTiles, sideUnderRules } from './faces.mjs';
import { registryTiles } from '../textures/gen.mjs';

const registry = JSON.parse(readFileSync(new URL('./dwell.json', import.meta.url), 'utf8'));
const ids = registry.blocks.map((b) => `${registry.namespace}:${b.name}`);
const dirt = registry.blocks.find((b) => b.name === 'dirt');

test("dirt's side under grass takes the skirt tile, and the tile is a registry tile", () => {
  assert.deepEqual(sideUnderRules('dwell:dirt', dirt, ids), [
    { above: 'dwell:grass', side: 'dirtSkirt' },
    { above: 'dwell:snowy_grass', side: 'dirtSnowSkirt' },
  ]);
  assert.ok(blockTiles(dirt).includes('dirtSkirt'));
  // The texture bake paints it (every registry tile must be painted).
  assert.ok(registryTiles(registry).includes('dirtSkirt'));
});

test('a rule is added by data alone', () => {
  const loam = { name: 'loam', textures: 'sand', sideUnder: { 'dwell:stone': 'grassSide', 'dwell:grass': 'dirtSkirt' } };
  assert.deepEqual(sideUnderRules('test:loam', loam, [...ids, 'test:loam']), [
    { above: 'dwell:stone', side: 'grassSide' },
    { above: 'dwell:grass', side: 'dirtSkirt' },
  ]);
  assert.deepEqual(blockTiles(loam), ['sand', 'sand', 'sand', 'grassSide', 'dirtSkirt']);
  assert.deepEqual(sideUnderRules('test:plain', { textures: 'sand' }, ids), []);
});

test('bad rules fail with a message naming the block', () => {
  const bad = (sideUnder, message, extra = {}) =>
    assert.throws(() => sideUnderRules('dwell:dirt', { textures: 'dirt', sideUnder, ...extra }, ids), { message });
  bad({ 'dwell:gras': 'dirtSkirt' }, 'dwell:dirt: sideUnder names unknown block dwell:gras');
  bad({ grass: 'dirtSkirt' }, 'dwell:dirt: sideUnder names unknown block grass');
  bad({ 'dwell:grass': 3 }, 'dwell:dirt: sideUnder.dwell:grass must be a texture tile name (got 3)');
  bad({ 'dwell:grass': '' }, 'dwell:dirt: sideUnder.dwell:grass must be a texture tile name (got "")');
  bad(['dwell:grass'], 'dwell:dirt: sideUnder must be an object of block id → side tile');
  bad({}, 'dwell:dirt: sideUnder is empty (leave it out)');
  assert.throws(() => sideUnderRules('dwell:dirt', { sideUnder: { 'dwell:grass': 'dirtSkirt' } }, ids), {
    message: 'dwell:dirt: sideUnder needs textures (it replaces the side tile)',
  });
});
