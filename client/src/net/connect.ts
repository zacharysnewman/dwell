// Chooses how to reach a server and runs the session (ARCHITECTURE.md §8.1, §10).
import { IndexedDbKeyStore, loadOrCreateDeviceKey } from '../identity/deviceKey';
import type { Invite } from './invite';
import { LoopbackTransport } from './loopback';
import { SimulatedTransport, type NetConditions } from './netsim';
import { ClientSession } from './session';
import type { Transport } from './Transport';
import { isWebRtcSupported, WebRtcTransport } from './webRtc';
import { isWebTransportSupported, WebTransportTransport } from './webTransport';

export interface ConnectOptions {
  displayName: string;
  clientVersion: string;
  transport?: TransportPreference;
  /** Simulated latency / jitter / loss (`?netsim=`), for testing prediction. */
  netsim?: NetConditions | null;
}

function simulate(transport: Transport, options: ConnectOptions): Transport {
  return options.netsim ? new SimulatedTransport(transport, options.netsim) : transport;
}

export class TransportUnavailableError extends Error {
  override name = 'TransportUnavailableError';
}

export type TransportPreference = 'auto' | 'webtransport' | 'webrtc';

/**
 * Opens the best available transport to the invited server (ARCHITECTURE.md §8.1): WebTransport
 * when supported, otherwise — or if it fails — WebRTC when the invite allows it (ADR 0008).
 */
export async function openTransport(
  invite: Invite,
  preference: TransportPreference = 'auto',
): Promise<Transport> {
  const canWebRtc = invite.webrtc !== null && isWebRtcSupported();
  if (preference !== 'webrtc' && isWebTransportSupported()) {
    try {
      return await WebTransportTransport.connect(invite.url, invite.certHash);
    } catch (err) {
      if (preference === 'webtransport' || !canWebRtc) throw err;
    }
  }
  if (canWebRtc && invite.webrtc && preference !== 'webtransport') {
    return WebRtcTransport.connect(invite.webrtc, invite.certHash);
  }
  throw new TransportUnavailableError(
    invite.webrtc
      ? 'This browser supports neither WebTransport nor WebRTC.'
      : 'This browser does not support WebTransport, and the invite has no WebRTC details.',
  );
}

export async function connectToInvite(
  invite: Invite,
  options: ConnectOptions,
): Promise<ClientSession> {
  const key = await loadOrCreateDeviceKey(new IndexedDbKeyStore());
  const transport = simulate(await openTransport(invite, options.transport), options);
  const session = new ClientSession(transport, key, options);
  session.start();
  return session;
}

/** Starts the integrated server in a worker and joins it (local mode, ARCHITECTURE.md §2.1). */
export async function connectLocal(options: ConnectOptions): Promise<ClientSession> {
  const key = await loadOrCreateDeviceKey(new IndexedDbKeyStore());
  const worker = new Worker(new URL('../local/worker.ts', import.meta.url), { type: 'module' });
  const transport = simulate(await LoopbackTransport.start(worker), options);
  const session = new ClientSession(transport, key, options);
  session.start();
  return session;
}
