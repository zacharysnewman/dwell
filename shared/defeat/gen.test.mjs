// Defeat's validation and output (shared/defeat/gen.mjs; docs/CHARACTERS.md §11.13, WORK_QUEUE item 55).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { RUST, defeatRust, loadDefeat, validateDefeat } from './gen.mjs';

test('the checked-in data validates and the Rust is up to date', () => {
  const d = validateDefeat(loadDefeat());
  assert.equal(readFileSync(RUST, 'utf8'), defeatRust(d), 'run `node shared/defeat/gen.mjs`');
  assert.ok(d.kneelS > 0 && d.fadeS >= 0 && d.standS > 0);
  assert.equal(d.characters['dwell:villager'].fadeS, d.fadeS, 'an override keeps the defaults it leaves out');
});

test('an override is data alone: a new one is emitted, its camera merged over the defaults', () => {
  const raw = loadDefeat();
  raw.characters['dwell:player'] = { fadeS: 2.5, camera: { backM: 5 } };
  const d = validateDefeat(raw);
  assert.equal(d.characters['dwell:player'].camera.backM, 5);
  assert.equal(d.characters['dwell:player'].camera.upM, d.camera.upM);
  const rs = defeatRust(d);
  assert.match(rs, /\("dwell:player", Defeat \{/);
  assert.match(rs, /fade_s: 2\.5,/);
  assert.match(rs, /back_m: 5\.0,/);
});

test('bad entries fail naming the field and the problem', () => {
  const cases = [
    [(d) => (d.kneelS = 0), /kneelS must be at least 0\.5/],
    [(d) => (d.roles.kneel = 'grovel'), /roles\.kneel: character "player" has no clip for role "grovel"/],
    [(d) => (d.roles.kneel = 'idle'), /roles\.kneel: clip "biped_idle" must not loop/],
    [(d) => (d.standS = 0.5), /standS \(0\.5 s\) is shorter than biped_stand/],
    [(d) => (d.characters['dwell:wolf'] = { kneelS: 3 }), /characters\.dwell:wolf: not a human character or NPC kind/],
    [(d) => (d.characters['dwell:villager'].sleepS = 3), /characters\.dwell:villager: unknown field sleepS/],
    [(d) => (d.camera.pitchDeg = 120), /camera\.pitchDeg must be at least -89 and at most 45/],
    [(d) => (d.colour = 1), /the file: unknown field colour/],
  ];
  for (const [mutate, want] of cases) {
    const d = loadDefeat();
    mutate(d);
    assert.throws(() => validateDefeat(d), want);
  }
});
