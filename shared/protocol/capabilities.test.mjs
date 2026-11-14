// The capabilities table's validation and outputs (shared/protocol/capabilities.mjs).
import assert from 'node:assert/strict';
import test from 'node:test';
import { capabilitiesRust, capabilitiesTs, loadCapabilities, validateCapabilities } from './capabilities.mjs';

test("the build's table validates and names structures.place", () => {
  const t = validateCapabilities(loadCapabilities());
  assert.ok(t.capabilities.some((e) => e.name === 'structures.place'));
  assert.match(capabilitiesRust(t).join('\n'), /pub const STRUCTURES_PLACE: &str = "structures\.place";/);
  assert.match(capabilitiesTs(t).join('\n'), /names: \[.*'structures\.place'.*\]/);
});

test('a capability added by data alone is emitted', () => {
  const t = loadCapabilities();
  t.capabilities.push({ name: 'mymod:chop.kinds', description: 'A fixture.' });
  const rs = capabilitiesRust(validateCapabilities(t)).join('\n');
  assert.match(rs, /pub const MYMOD_CHOP_KINDS: &str = "mymod:chop\.kinds";/);
  assert.match(rs, /pub const ALL: &\[&str\] = &\[STRUCTURES_PLACE, [A-Z_, ]*MYMOD_CHOP_KINDS\];/);
});

test('a bad entry fails with a message', () => {
  const bad = (f, msg) => {
    const t = loadCapabilities();
    f(t);
    assert.throws(() => validateCapabilities(t), msg);
  };
  bad((t) => t.capabilities.push({ name: 'Upper.Case', description: 'x' }), /bad name "Upper\.Case"/);
  bad((t) => t.capabilities.push({ ...t.capabilities[0] }), /"structures\.place" twice/);
  bad((t) => t.capabilities.push({ name: 'structures_place', description: 'x' }), /same constant/);
  bad((t) => t.capabilities.push({ name: 'a.b' }), /"a\.b" needs a description/);
  bad((t) => t.capabilities.push({ name: 'a.b', description: 'x', bit: 3 }), /"a\.b" has no field "bit"/);
  bad((t) => t.capabilities.push({ name: 'x'.repeat(49), description: 'x' }), /longer than maxNameBytes/);
  bad(
    (t) => {
      t.maxNames = 255;
      for (let i = 0; i < 30; i++) t.capabilities.push({ name: `area${i}.${'y'.repeat(30)}`, description: 'x' });
    },
    /more than one WorldSettings field/,
  );
});
