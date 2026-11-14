// Stamina and oxygen's validation and outputs (shared/vitals/gen.mjs, ADR 0069).
import assert from 'node:assert/strict';
import test from 'node:test';
import { ACTIVITIES, loadVitals, validateVitals, vitalsRust, vitalsTs } from './gen.mjs';

test("the build's data validates: the owner's 25 % rule, every activity spends", () => {
  const d = validateVitals(loadVitals());
  assert.equal(d.stamina.recoverFraction, 0.25);
  for (const a of ACTIVITIES) assert.ok(d.stamina.drains[a] > 0, a);
  const rs = vitalsRust(d);
  assert.match(rs, /\(Activity::Sprint, 12\.0\)/);
  assert.match(rs, /recover_fraction: 0\.25,/);
  assert.match(vitalsTs(d), /maxStamina: 100,/);
});

test("an activity's rate is data alone: one left out spends nothing, one added is emitted", () => {
  const d = loadVitals();
  d.stamina.drains = d.stamina.drains.filter((x) => x.activity !== 'tread');
  assert.match(vitalsRust(validateVitals(d)), /\(Activity::Tread, 0\.0\)/);
  d.stamina.drains.push({ activity: 'tread', perSecond: 2.5 });
  assert.match(vitalsRust(validateVitals(d)), /\(Activity::Tread, 2\.5\)/);
  d.oxygen.capacitySeconds = 45;
  assert.match(vitalsRust(validateVitals(d)), /oxygen_seconds: 45\.0,/);
});

test('bad entries fail naming the field and the problem', () => {
  const cases = [
    [(d) => d.stamina.drains.push({ activity: 'climb', perSecond: 1 }), /stamina\.drains\.climb: unknown activity climb/],
    [(d) => d.stamina.drains.push({ activity: 'sprint', perSecond: 1 }), /stamina\.drains\.sprint: the activity is listed twice/],
    [(d) => (d.stamina.recoverFraction = 1.5), /stamina\.recoverFraction must be above 0 and at most 1/],
    [(d) => (d.stamina.max = 0), /stamina\.max must be above 0/],
    [(d) => (d.stamina.regen = 3), /stamina: unknown field regen/],
    [(d) => (d.oxygen.drownDamage = 2.5), /oxygen\.drownDamage must be a whole number/],
    [(d) => (d.oxygen.capacitySeconds = 'long'), /oxygen\.capacitySeconds must be a number/],
    [(d) => (d.formatVersion = 2), /formatVersion must be 1/],
  ];
  for (const [mutate, message] of cases) {
    const d = loadVitals();
    mutate(d);
    assert.throws(() => validateVitals(d), message);
  }
});
