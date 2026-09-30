import { describe, expect, it } from 'vitest';
import type { SessionState } from '../net/session';
import { RejectReason, TransportKind } from '../protocol/constants.gen';
import { formatStatus } from './statusOverlay';

const joined: SessionState = {
  phase: 'joined',
  playerId: 2,
  worldSeed: 0n,
  generatorVersion: 1,
  verificationChunk: [0, 0, 0],
  mayFly: false,
};
const stats = { rttMs: 12.4, datagramRttMs: null, serverTick: 99, hostPaused: false };

describe('formatStatus', () => {
  it('shows RTTs and tick when joined', () => {
    expect(formatStatus('localhost:4433', TransportKind.WebRtc, joined, stats)).toBe(
      'localhost:4433 (WebRTC) · player 2 · RTT 12 ms (datagram –) · tick 99',
    );
  });

  it('says when a friend world’s host has paused it', () => {
    expect(
      formatStatus('KQ7-XM4', TransportKind.WebRtc, joined, { ...stats, hostPaused: true }),
    ).toMatch(/^Host paused · KQ7-XM4 \(WebRTC\)/);
  });

  it('reports a host that stopped hosting as a disconnection, not a refusal', () => {
    expect(
      formatStatus(
        'KQ7-XM4',
        TransportKind.WebRtc,
        {
          phase: 'rejected',
          reason: RejectReason.ServerClosing,
          message: 'The host stopped hosting.',
        },
        stats,
      ),
    ).toBe('Disconnected from KQ7-XM4: The host stopped hosting.');
  });
});

describe('the frame rate', () => {
  it('is shown once measured', () => {
    expect(formatStatus('Home', TransportKind.Loopback, joined, stats, 58.6)).toMatch(/ · 59 fps$/);
    expect(formatStatus('Home', TransportKind.Loopback, joined, stats)).not.toMatch(/fps/);
  });
});
