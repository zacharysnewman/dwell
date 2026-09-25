import type { SessionState, SessionStats } from '../net/session';

function ms(v: number | null): string {
  return v === null ? '–' : `${String(Math.round(v))} ms`;
}

/** One-line connection status for the overlay (Phase 1 exit criterion: show RTT). */
export function formatStatus(target: string, state: SessionState, stats: SessionStats): string {
  switch (state.phase) {
    case 'handshaking':
      return `Joining ${target}…`;
    case 'joined':
      return `${target} · player ${String(state.playerId)} · RTT ${ms(stats.rttMs)} (datagram ${ms(stats.datagramRttMs)}) · tick ${String(stats.serverTick)}`;
    case 'rejected':
      return `${target} refused the connection: ${state.message}`;
    case 'closed':
      return `Disconnected from ${target}: ${state.message}`;
  }
}
