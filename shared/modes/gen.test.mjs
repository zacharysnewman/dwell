// The game modes' validation and outputs (shared/modes/gen.mjs, ADR 0065).
import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModes, modesRust, modesTs, validateModes } from './gen.mjs';

test("the build's data validates: creative is the default, adventure is experimental", () => {
  const d = validateModes(loadModes());
  assert.equal(d.defaultMode, 'creative');
  const creative = d.modes.find((m) => m.name === 'creative');
  const adventure = d.modes.find((m) => m.name === 'adventure');
  assert.deepEqual(creative.abilities, ['flight', 'brushes', 'unlimitedBlocks', 'map', 'mapRevealed', 'instantBreaks', 'structures', 'theaterViewpoint']);
  // The big map shows everything in creative; adventure explores it (owner, 2026-11-08).
  assert.ok(!adventure.abilities.includes('mapRevealed'));
  assert.ok(!adventure.abilities.includes('instantBreaks'));
  // Saved structures are placed in creative only, for now (owner, 2026-11-11).
  assert.ok(!adventure.abilities.includes('structures'));
  // Theater mode's camera is a simulation viewpoint in creative only (owner, 2026-11-12: "can't be
  // used for cheating in adventure"; docs/THEATER_MODE.md §7).
  assert.ok(!adventure.abilities.includes('theaterViewpoint'));
  assert.match(modesRust(d), /Ability::Map\.bit\(\) \| Ability::MapRevealed\.bit\(\)/);
  assert.deepEqual(adventure.abilities, ['damage', 'map', 'stamina', 'oxygen']);
  assert.equal(adventure.experimental, true);
  assert.match(modesRust(d), /pub const DEFAULT_MODE: &str = "creative";/);
  assert.match(modesTs(d), /export const GAME_MODES/);
});

test("creative's description says what its breaks do: tools keep their hits (BUGS.md #127)", () => {
  // BLOCK_DAMAGE.md §7: without a tool any block breaks in one hit; a held tool keeps its hits. The
  // New world form shows this text, and "any block ... in one hit" read as tools too.
  const creative = validateModes(loadModes()).modes.find((m) => m.name === 'creative');
  assert.match(creative.description, /without a tool any block breaks in one hit/);
  assert.match(creative.description, /held tools? keeps? (its|their) hits/);
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
