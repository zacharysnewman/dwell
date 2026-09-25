import { describe, expect, it } from 'vitest';
import { TransportKind } from '../protocol/constants.gen';
import { formatStatus } from './statusOverlay';

describe('formatStatus', () => {
  it('shows RTTs and tick when joined', () => {
    expect(
      formatStatus(
        'localhost:4433',
        TransportKind.WebRtc,
        { phase: 'joined', playerId: 2, worldSeed: 0n, generatorVersion: 1 },
        { rttMs: 12.4, datagramRttMs: null, serverTick: 99 },
      ),
    ).toBe('localhost:4433 (WebRTC) · player 2 · RTT 12 ms (datagram –) · tick 99');
  });
});
