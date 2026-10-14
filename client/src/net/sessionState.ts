// The session's state as the Rust client reports it (`Client.sessionState()`, dwell-client's
// web/app.rs): the join handshake's progress, the world once joined, and the connection's stats.
import type { RejectReason } from '../protocol/constants.gen';

export interface SessionStats {
  /** Smoothed round trip over the reliable control stream, ms. */
  rttMs: number | null;
  /** Smoothed round trip over datagrams, ms. */
  datagramRttMs: number | null;
  serverTick: number;
  /** A friend world's host has paused it (their page is hidden, HostStatus, §10.2). */
  hostPaused: boolean;
}

export type SessionState =
  | { phase: 'connecting' }
  | { phase: 'handshaking' }
  | {
      phase: 'joined';
      playerId: number;
      worldSeed: bigint;
      generatorVersion: number;
      /** The server lets this player use creative flight (§8.3). */
      mayFly: boolean;
    }
  | { phase: 'rejected'; reason: RejectReason; message: string }
  | { phase: 'closed'; message: string };

/** `sessionState()`'s object: the state, its stats once a session exists, and the transport. */
export type SessionReport = SessionState & {
  stats?: SessionStats;
  /** 'webtransport', 'webrtc', 'loopback', or '' before connecting. */
  transport: string;
};
