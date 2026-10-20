import type { SessionState, SessionStats } from '../net/sessionState';
import { RejectReason } from '../protocol/constants.gen';

const TRANSPORT_NAMES: Record<string, string> = {
  webtransport: 'WebTransport',
  webrtc: 'WebRTC',
  loopback: 'local',
};

function ms(v: number | null): string {
  return v === null ? '–' : `${String(Math.round(v))} ms`;
}

/**
 * One-line connection status for the overlay (Phase 1 exit criterion: show RTT), with the frame
 * rate once known.
 */
export function formatStatus(
  target: string,
  transport: string,
  state: SessionState,
  stats: SessionStats,
  fps: number | null = null,
): string {
  switch (state.phase) {
    case 'connecting':
    case 'handshaking':
      return `Joining ${target}…`;
    case 'confirming':
      return `Joining ${target}: its blocks differ from this version’s…`;
    case 'joined':
      return `${stats.hostPaused ? 'Host paused · ' : ''}${target} (${TRANSPORT_NAMES[transport] ?? transport}) · player ${String(state.playerId)} · RTT ${ms(stats.rttMs)} (datagram ${ms(stats.datagramRttMs)}) · tick ${String(stats.serverTick)}${fps === null ? '' : ` · ${String(Math.round(fps))} fps`}`;
    case 'rejected':
      return state.reason === RejectReason.Replaced || state.reason === RejectReason.ServerClosing
        ? `Disconnected from ${target}: ${state.message}`
        : `${target} refused the connection: ${state.message}`;
    case 'closed':
      return `Disconnected from ${target}: ${state.message}`;
  }
}
