import { describe, expect, it } from 'vitest';
import { PlayerFlags, PlayerState } from '../protocol/constants.gen';
import type { RemotePlayerState } from '../protocol/messages';
import { isDead, RemotePlayers } from './remotes';

const remote = (x: number, flags = 0): RemotePlayerState => ({
  playerId: 4,
  position: [x, 0, 0],
  velocity: [1, 0, 0],
  yaw: 0,
  pitch: 0,
  state: PlayerState.Walking,
  flags,
});

describe('RemotePlayers', () => {
  it('interpolates INTERP_DELAY_MS in the past between snapshots', () => {
    const r = new RemotePlayers(100);
    // Snapshots every 3 ticks (50 ms), each arriving 30 ms after it was sent.
    for (let i = 0; i < 6; i++) r.push(i * 3, [remote(i)], i * 50 + 30);
    // At local 280 ms: server time 250 − 100 = 150 ms → between snapshot 3 (150 ms) and 4.
    expect(r.sample(280)[0]?.feet[0]).toBeCloseTo(3);
    expect(r.sample(305)[0]?.feet[0]).toBeCloseTo(3.5);
  });

  it('holds the newest sample when the buffer runs dry, and forgets silent players', () => {
    const r = new RemotePlayers(100);
    r.push(0, [remote(0)], 0);
    r.push(3, [remote(1, PlayerFlags.dead)], 50);
    const late = r.sample(1000)[0];
    expect(late?.feet[0]).toBe(1);
    expect(late && isDead(late)).toBe(true);
    r.push(100, [], 2000);
    expect(r.ids()).toEqual([]);
  });

  it('ignores reordered snapshots', () => {
    const r = new RemotePlayers(0);
    r.push(6, [remote(2)], 100);
    r.push(3, [remote(1)], 110);
    expect(r.latest()[0]?.feet[0]).toBe(2);
  });
});
