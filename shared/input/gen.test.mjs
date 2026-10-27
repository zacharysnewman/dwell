// The controller data's validation and outputs (shared/input/gen.mjs).
import assert from 'node:assert/strict';
import test from 'node:test';
import { gamepadRust, gamepadTs, loadGamepad, validateGamepad, PLAY_ACTIONS } from './gen.mjs';

test("the build's data validates and binds every action", () => {
  const d = validateGamepad(loadGamepad());
  const rs = gamepadRust(d);
  for (const a of PLAY_ACTIONS) assert.match(rs, new RegExp(`\\("${a}", "`));
  assert.match(gamepadTs(d), /export const GAMEPAD = /);
});

test('a binding changed by data alone is emitted', () => {
  const d = loadGamepad();
  d.play.jump = 'x';
  d.play.spyglass = 'a';
  const rs = gamepadRust(validateGamepad(d));
  assert.match(rs, /\("jump", "x"\),/);
  assert.match(rs, /\("spyglass", "a"\),/);
});

test('bad entries fail with the key and the problem', () => {
  const cases = [
    [(d) => (d.play.jump = 'z'), /play\.jump: "z" is not in buttons/],
    [(d) => (d.play.dance = 'a'), /play: unknown action dance/],
    [(d) => delete d.play.fly, /play\.fly must name a button/],
    [(d) => (d.play.fly = 'a'), /play\.fly: a is already play\.jump/],
    [(d) => (d.menu.confirm = 'q'), /menu\.confirm: "q" is not in buttons/],
    [(d) => (d.buttons[1].index = 0), /buttons\[1\]\.index 0 is listed twice/],
    [(d) => (d.sticks.lookX = 7), /sticks\.lookX must be an axis in \[0, 3\]/],
    [(d) => (d.tunables.deadzone = 1), /tunables\.deadzone must be in \[0, 0\.9\]/],
    [(d) => (d.extra = 1), /unknown extra/],
  ];
  for (const [change, message] of cases) {
    const d = loadGamepad();
    change(d);
    assert.throws(() => validateGamepad(d), message);
  }
});
