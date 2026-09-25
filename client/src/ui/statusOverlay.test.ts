import { describe, expect, it } from 'vitest';
import { formatStatus } from './statusOverlay';

describe('formatStatus', () => {
  it('shows RTTs and tick when joined', () => {
    expect(
      formatStatus(
        'localhost:4433',
        { phase: 'joined', playerId: 2, worldSeed: 0n },
        { rttMs: 12.4, datagramRttMs: null, serverTick: 99 },
      ),
    ).toBe('localhost:4433 · player 2 · RTT 12 ms (datagram –) · tick 99');
  });
});
