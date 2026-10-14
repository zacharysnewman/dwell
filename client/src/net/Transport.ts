import type { Channel, TransportKind } from '../protocol/constants.gen';

/** Why a transport closed; `message` is for display and logs. */
export interface CloseInfo {
  message: string;
}

export interface TransportHandlers {
  onReliable(channel: Channel, bytes: Uint8Array): void;
  onDatagram(bytes: Uint8Array): void;
  onClose(info: CloseInfo): void;
}

/**
 * A connection to a server that the shell makes and hands to the Rust client (ARCHITECTURE.md
 * §8.1): WebRTC to a friend world's host (peer.ts) and Loopback to local mode's worker. The Rust
 * client makes its own WebTransport and WebRTC links to dedicated servers.
 */
export interface Transport {
  readonly kind: TransportKind;
  /**
   * Transport binding signed during the join handshake (ADR 0004): the server certificate's
   * SHA-256 for WebTransport, the DTLS fingerprint for WebRTC, zeros for Loopback.
   */
  readonly binding: Uint8Array;
  setHandlers(handlers: TransportHandlers): void;
  sendReliable(channel: Channel, bytes: Uint8Array): void;
  /** Best-effort; may be dropped. */
  sendDatagram(bytes: Uint8Array): void;
  close(): void;
}
