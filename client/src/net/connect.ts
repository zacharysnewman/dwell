// Chooses how to reach a server and runs the session (ARCHITECTURE.md §8.1, §10).
import { IndexedDbKeyStore, loadOrCreateDeviceKey } from '../identity/deviceKey';
import type { Invite } from './invite';
import { ClientSession } from './session';
import type { Transport } from './Transport';
import { isWebTransportSupported, WebTransportTransport } from './webTransport';

export interface ConnectOptions {
  displayName: string;
  clientVersion: string;
}

export class TransportUnavailableError extends Error {
  override name = 'TransportUnavailableError';
}

/** Opens the best available transport to the invited server. WebRTC fallback arrives later. */
export async function openTransport(invite: Invite): Promise<Transport> {
  if (isWebTransportSupported()) {
    return WebTransportTransport.connect(invite.url, invite.certHash);
  }
  throw new TransportUnavailableError('This browser does not support WebTransport yet.');
}

export async function connectToInvite(
  invite: Invite,
  options: ConnectOptions,
): Promise<ClientSession> {
  const key = await loadOrCreateDeviceKey(new IndexedDbKeyStore());
  const transport = await openTransport(invite);
  const session = new ClientSession(transport, key, options);
  session.start();
  return session;
}
