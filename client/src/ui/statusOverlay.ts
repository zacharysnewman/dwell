import type { SessionState, SessionStats } from '../net/session';
import { TransportKind } from '../protocol/constants.gen';

const TRANSPORT_NAMES: Record<TransportKind, string> = {
  [TransportKind.WebTransport]: 'WebTransport',
  [TransportKind.WebRtc]: 'WebRTC',
  [TransportKind.Loopback]: 'local',
};

function ms(v: number | null): string {
  return v === null ? '–' : `${String(Math.round(v))} ms`;
}

/** One-line connection status for the overlay (Phase 1 exit criterion: show RTT). */
export function formatStatus(
  target: string,
  transport: TransportKind,
  state: SessionState,
  stats: SessionStats,
): string {
  switch (state.phase) {
    case 'handshaking':
      return `Joining ${target}…`;
    case 'joined':
      return `${target} (${TRANSPORT_NAMES[transport]}) · player ${String(state.playerId)} · RTT ${ms(stats.rttMs)} (datagram ${ms(stats.datagramRttMs)}) · tick ${String(stats.serverTick)}`;
    case 'rejected':
      return `${target} refused the connection: ${state.message}`;
    case 'closed':
      return `Disconnected from ${target}: ${state.message}`;
  }
}
