// BUGS.md #110: own sounds sat in the ears (footsteps dry and centred). The listener frame on both
// faces, and the own sounds placed round the ear by their event's `self` (AUDIO.md §5a).
import { describe, expect, it } from 'vitest';
import { EventPlayer, type SoundEvent } from './events';
import { heardAt, selfAt, type ListenerPose, type Vec3 } from './listener';
import { SOUND_DATA } from './sounds.gen';

/**
 * The Rust client's `listener()` (game.rs; pinned there against the drawn camera on both faces):
 * forward the look (yaw 0 = +Z, pitch toward the face's up), up the camera's, on face B the world's
 * y mirrored and the player's right reversed.
 */
function camera(face: 'a' | 'b', eye: Vec3, yawDeg: number, pitchDeg: number): ListenerPose {
  const s = face === 'a' ? 1 : -1;
  const [y, p] = [(yawDeg * Math.PI) / 180, (pitchDeg * Math.PI) / 180];
  const forward: Vec3 = [Math.sin(y) * Math.cos(p), s * Math.sin(p), Math.cos(y) * Math.cos(p)];
  const right: Vec3 = [s * -Math.cos(y), 0, s * Math.sin(y)];
  const back: Vec3 = [-forward[0], -forward[1], -forward[2]];
  const up: Vec3 = [
    back[1] * right[2] - back[2] * right[1],
    back[2] * right[0] - back[0] * right[2],
    back[0] * right[1] - back[1] * right[0],
  ];
  return { position: eye, forward, up };
}

const kind = (name: string): number => SOUND_DATA.events.findIndex((e) => e.name === name);
const own = (name: string, gait: number | null = null): SoundEvent => ({
  kind: kind(name),
  block: 0,
  intensity: 1,
  at: null,
  gait,
  voice: null,
});

describe('the listener frame (both faces)', () => {
  for (const face of ['a', 'b'] as const) {
    const eye: Vec3 = face === 'a' ? [3, 41.6, -7] : [3, -41.6, -7];
    it(`hears a sound ahead ahead and one to the right right, face ${face}`, () => {
      for (const yaw of [0, 90, 213]) {
        for (const pitch of [0, 45, -45]) {
          const l = camera(face, eye, yaw, pitch);
          const along = (d: Vec3, m: number): Vec3 => [
            eye[0] + d[0] * m,
            eye[1] + d[1] * m,
            eye[2] + d[2] * m,
          ];
          const ahead = heardAt(l, along(l.forward, 5));
          expect(ahead[2]).toBeCloseTo(-5, 9);
          // The player's right: on face A at heading 0 world −X, on face B (mirrored) +X.
          const s = face === 'a' ? 1 : -1;
          const r = (yaw * Math.PI) / 180;
          const right = heardAt(l, along([s * -Math.cos(r), 0, s * Math.sin(r)], 5));
          expect(right[0]).toBeCloseTo(5, 9);
          expect(right[2]).toBeCloseTo(0, 9);
        }
      }
    });
  }
});

describe("the local player's own sounds (AUDIO.md §5a)", () => {
  const feet = SOUND_DATA.self.feet;
  it('are placed for every own movement event: the steps at the feet, the tool at the hand', () => {
    for (const e of ['step', 'land', 'jump', 'slide', 'brush'])
      expect(SOUND_DATA.events[kind(e)]?.self, e).toBe('feet');
    for (const e of ['swim', 'swim_under', 'climb', 'enter', 'exit'])
      expect(SOUND_DATA.events[kind(e)]?.self, e).toBe('body');
    for (const e of ['tool', 'axe_swish', 'break', 'hit'])
      expect(SOUND_DATA.events[kind(e)]?.self, e).toBe('hand');
    // Your own voice stays at the ear.
    for (const e of ['gasp', 'hurt', 'exhausted'])
      expect(SOUND_DATA.events[kind(e)]?.self, e).toBe(undefined);
    expect(SOUND_DATA.self.hand?.right).toBeGreaterThan(0);
  });

  for (const face of ['a', 'b'] as const) {
    it(`a footstep is heard below the ears and not behind them at pitch 0 and ±45°, face ${face}`, () => {
      expect(feet).toBeDefined();
      if (!feet) return;
      const eye: Vec3 = face === 'a' ? [0, 41.6, 0] : [0, -41.6, 0];
      for (const pitch of [0, 45, -45]) {
        for (const yaw of [0, 120]) {
          const l = camera(face, eye, yaw, pitch);
          for (const side of [-1, 1]) {
            const h = heardAt(l, selfAt(l, feet, side * (feet.stepM ?? 0)));
            expect(h[1], `pitch ${String(pitch)}`).toBeLessThan(-1);
            expect(h[2], `pitch ${String(pitch)}`).toBeLessThan(0);
            expect(Math.sign(h[0])).toBe(feet.stepM ? side : 0);
          }
          // At pitch 0 that is the feet, a little ahead of the body, on the face's ground side.
          if (pitch === 0) {
            const p = selfAt(l, feet);
            const s = face === 'a' ? 1 : -1;
            expect(s * (p[1] - eye[1])).toBeCloseTo(feet.up, 9);
          }
        }
      }
    });
  }

  it('take the self path: placed round the ear with the room send, steps alternating; others keep theirs', () => {
    const l = camera('b', [0, -41.6, 0], 30, 0);
    const plays: { at: Vec3 | null; pan?: number; reverb?: number }[] = [];
    const player = new EventPlayer(
      SOUND_DATA,
      (_, o) => plays.push(o),
      null,
      undefined,
      () => l,
    );
    const walk = SOUND_DATA.gaitOrder.indexOf('walk');
    player.playAll([own('step', walk), own('step', walk)]);
    expect(plays.map((p) => p.pan)).toEqual([undefined, undefined]);
    const [a, b] = plays.map((p) => heardAt(l, p.at ?? [0, 0, 0]));
    expect(a?.[1]).toBeCloseTo(feet?.up ?? 0, 9);
    expect(a?.[2]).toBeCloseTo(-(feet?.ahead ?? 0), 9);
    expect(Math.sign(a?.[0] ?? 0)).toBe(-Math.sign(b?.[0] ?? 0));
    expect(plays[0]?.reverb).toBe(feet?.reverb);
    // The held tool: at the right hand.
    plays.length = 0;
    player.playAll([own('tool')]);
    expect(heardAt(l, plays[0]?.at ?? [0, 0, 0])[0]).toBeGreaterThan(0);
    // A remote player's footstep stays at its feet; your own voice at the ear.
    plays.length = 0;
    player.playAll([{ ...own('step', walk), at: [4, -40, 2] }, own('gasp')]);
    expect(plays.map((p) => p.at)).toEqual([[4, -40, 2], null]);
    expect(plays[0]?.reverb).toBe(undefined);
  });

  it("an own slide's loop follows the ear; another's stays where the game puts it", () => {
    let l = camera('a', [0, 41.6, 0], 0, 0);
    const started: (Vec3 | null)[] = [];
    const updated: (Vec3 | null)[] = [];
    const player = new EventPlayer(
      SOUND_DATA,
      () => undefined,
      {
        start: (_v, _s, o) => started.push(o.at),
        update: (_v, _i, at) => {
          updated.push(at);
          return true;
        },
        stop: () => undefined,
      },
      undefined,
      () => l,
    );
    player.playAll([own('slide')]);
    l = camera('a', [10, 41.6, 0], 90, 0);
    player.playAll([own('slide_speed')]);
    expect(heardAt(camera('a', [0, 41.6, 0], 0, 0), started[0] ?? [0, 0, 0])[1]).toBeCloseTo(
      feet?.up ?? 0,
      9,
    );
    expect(heardAt(l, updated[0] ?? [0, 0, 0])[2]).toBeCloseTo(-(feet?.ahead ?? 0), 9);
    player.playAll([{ ...own('slide'), voice: 3, at: [1, 2, 3] }]);
    expect(started[1]).toEqual([1, 2, 3]);
  });
});
