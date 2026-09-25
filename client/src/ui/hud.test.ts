import { describe, expect, it } from 'vitest';
import { GroundKind, PlayerState } from '../protocol/constants.gen';
import type { ClientState } from '../sim/clientCore';
import { formatDebug } from './hud';

describe('debug overlay', () => {
  it('shows state, layers, and prediction stats', () => {
    const core = {
      state: PlayerState.Running,
      groundKind: GroundKind.Terrain,
      groundGap: 0,
      touchingWall: true,
      ceilingBlocked: false,
      submerged: 0,
      position: [1, 0.9, 2],
      velocity: [0, 0, 8],
      horizontalCurrent: [0, 0, 8],
      horizontalExternal: [0, 0, 0],
      targetVelocity: [0, 0, 8],
      renderOffset: [0, 0, 0.03],
      nextSeq: 101,
      stats: {
        ticks: 100,
        snapshots: 30,
        replays: 2,
        snaps: 0,
        knockbackReplays: 1,
        resets: 1,
        lastCorrection: 0.02,
        lastError: 0.001,
      },
    } as unknown as ClientState;
    const text = formatDebug({
      core,
      health: 83,
      inputBuffer: 2,
      tickRate: 1,
      remotes: 1,
      rttMs: 151.4,
    });
    expect(text).toContain('state Running · ground Terrain gap 0.00 · wall');
    expect(text).toContain('replays 2');
    expect(text).toContain('smoothing 3.00 cm');
    expect(text).toContain('RTT 151 ms');
    expect(text).toContain('health 83/100');
  });
});
