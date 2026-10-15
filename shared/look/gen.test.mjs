// The motion tunables' validation and generation (node --test shared/look/).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { motionRust, playerStates, validateMotion } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const load = () => JSON.parse(readFileSync(join(here, 'motion.json'), 'utf8'));
const states = playerStates();

test('the checked-in tunables are valid and generate the checked-in Rust', () => {
  const rs = motionRust(validateMotion(load(), states), states);
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/motion.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/look/gen.mjs`');
});

test('a bad entry fails with its path named', () => {
  const cases = [
    [(d) => (d.fov.sprintBoostDeg = -1), /fov\.sprintBoostDeg must be a number/],
    [(d) => (d.wind.color = 'white'), /wind\.color must be a colour/],
    [(d) => (d.wind.streaks = 4.5), /wind\.streaks must be a whole number/],
    [(d) => (d.wind.gust = 1), /unknown entry wind\.gust/],
    [(d) => delete d.stateGain.flying, /missing entry stateGain\.flying/],
    [(d) => (d.stateGain.hovering = 1), /unknown entry stateGain\.hovering/],
    [(d) => (d.wind.outerRadius = d.wind.innerRadius), /outerRadius must be greater/],
  ];
  for (const [edit, message] of cases) {
    const d = load();
    edit(d);
    assert.throws(() => validateMotion(d, states), message);
  }
});

test('a changed tunable reaches the generated Rust by data alone', () => {
  const d = load();
  d.fov.sprintBoostDeg = 7.25;
  d.wind.color = '#ff0000';
  d.stateGain.climbing = 0.75;
  const rs = motionRust(validateMotion(d, states), states);
  assert.match(rs, /sprint_boost_deg: 7\.25,/);
  assert.match(rs, /color: \[1\.0, 0\.0, 0\.0\],/);
  // Climbing is state 7: the gain lands at its index.
  const gains = /state_gain: \[([^\]]*)\]/.exec(rs)[1].split(', ');
  assert.equal(gains[states.indexOf('Climbing')], '0.75');
});
