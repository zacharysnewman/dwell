// The game modes' validation and outputs (shared/modes/gen.mjs, ADR 0065).
import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModes, modesRust, modesTs, validateModes } from './gen.mjs';

test("the build's data validates: creative is the default, adventure is experimental", () => {
  const d = validateModes(loadModes());
  assert.equal(d.defaultMode, 'creative');
  const creative = d.modes.find((m) => m.name === 'creative');
  const adventure = d.modes.find((m) => m.name === 'adventure');
  assert.deepEqual(creative.abilities, ['flight', 'brushes', 'unlimitedBlocks']);
  assert.deepEqual(adventure.abilities, ['damage', 'unlimitedBlocks']);
  assert.equal(adventure.experimental, true);
  assert.match(modesRust(d), /pub const DEFAULT_MODE: &str = "creative";/);
  assert.match(modesTs(d), /export const GAME_MODES/);
});

test('a mode added by data alone is emitted with its abilities', () => {
  const d = loadModes();
  d.modes.push({ name: 'spectator', label: 'Spectator', abilities: ['flight'] });
  const v = validateModes(d);
  const rs = modesRust(v);
  assert.match(rs, /name: "spectator",\n {8}label: "Spectator",\n {8}experimental: false,\n {8}abilities: Ability::Flight\.bit\(\),/);
  assert.match(modesTs(v), /name: "spectator",[\s\S]*abilities: \["flight"\]/);
  // No abilities at all is a mode too.
  d.modes.push({ name: 'still', label: 'Still', abilities: [] });
  assert.match(modesRust(validateModes(d)), /abilities: 0,/);
});

test('bad entries fail with the mode and the problem', () => {
  const cases = [
    [(d) => (d.modes[0].abilities = ['fly']), /modes\.creative: unknown ability fly/],
    [(d) => (d.modes[0].abilities = ['flight', 'flight']), /modes\.creative: ability flight is listed twice/],
    [(d) => (d.modes[1].name = 'Adventure'), /name must be lower case/],
    [(d) => (d.modes[1].name = 'creative'), /modes\.creative: the name is listed twice/],
    [(d) => (d.modes[0].reachM = 5), /modes\.creative: unknown field reachM/],
    [(d) => (d.modes[1].experimental = 'yes'), /modes\.adventure: experimental must be true or false/],
    [(d) => (d.modes[0].label = ''), /modes\.creative: label must be/],
    [(d) => (d.defaultMode = 'survival'), /defaultMode must name one of the modes/],
    [(d) => (d.modes = []), /modes: at least one game mode/],
    [(d) => (d.extra = 1), /unknown field extra/],
  ];
  for (const [change, message] of cases) {
    const d = loadModes();
    change(d);
    assert.throws(() => validateModes(d), message);
  }
});
