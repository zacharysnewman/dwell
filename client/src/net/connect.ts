// Chooses how to reach a server and hands the connection to the Rust client (ARCHITECTURE.md
// §8.1, §10): it connects to dedicated servers itself (WebTransport, or WebRTC to their ICE-lite
// endpoint) and runs the session; friend worlds' peer connections and local mode's loopback are
// made here and handed to it as transports.
import { IndexedDbKeyStore, loadOrCreateDeviceKey, type DeviceKey } from '../identity/deviceKey';
import type { Client } from '../engine/wasm';
import type { Invite } from './invite';
import { MasterClient, MasterError, serverInvite } from './master';
import { parseInvite } from './invite';
import { PeerTransport, type SignalData, type Signaler } from './peer';
import { RoomSocket } from './roomSocket';
import { GENERATORS, type LocalWorld } from '../local/world';
import { sameLine } from '../version/semver';
import { LoopbackTransport } from './loopback';
import type { Transport } from './Transport';

export type TransportPreference = 'auto' | 'webtransport' | 'webrtc';

export interface ConnectOptions {
  clientVersion: string;
  transport?: TransportPreference;
  /** Local mode: the world to open (from the menu, or `?world=`, `?seed=`). */
  localWorld?: LocalWorld;
  /**
   * A terrain world's maps (ADR 0049; `world/worldMaps.ts`): the local world's server installs
   * them rather than computing its own on its thread.
   */
  worldMaps?: (seed: bigint, generatorVersion: number) => Promise<Uint8Array[]> | null;
}

export class TransportUnavailableError extends Error {
  override name = 'TransportUnavailableError';
}

export function isWebTransportSupported(): boolean {
  return typeof globalThis.WebTransport === 'function';
}

export function isWebRtcSupported(): boolean {
  return typeof globalThis.RTCPeerConnection === 'function';
}

/** The device key, loaded once, and given to the client to sign its joins with (ADR 0004). */
export async function identify(client: Client): Promise<DeviceKey> {
  const key = await loadOrCreateDeviceKey(new IndexedDbKeyStore());
  client.setIdentity(key.publicKey, (transcript: Uint8Array<ArrayBuffer>) => key.sign(transcript));
  return key;
}

/**
 * Connects to the invited server (ARCHITECTURE.md §8.1): WebTransport when supported, otherwise —
 * or if it fails — WebRTC when the invite allows it (ADR 0008).
 */
export async function connectToInvite(
  client: Client,
  invite: Invite,
  preference: TransportPreference = 'auto',
): Promise<void> {
  const canWebRtc = invite.webrtc !== null && isWebRtcSupported();
  if (preference !== 'webrtc' && isWebTransportSupported()) {
    try {
      await client.connectWebTransport(invite.url, invite.certHash);
      return;
    } catch (err) {
      if (preference === 'webtransport' || !canWebRtc) throw err;
    }
  }
  if (canWebRtc && invite.webrtc && preference !== 'webtransport') {
    const w = invite.webrtc;
    await client.connectWebRtc(w.ip, w.port, w.ufrag, w.pwd, invite.certHash);
    return;
  }
  throw new TransportUnavailableError(
    invite.webrtc
      ? 'This browser supports neither WebTransport nor WebRTC.'
      : 'This browser does not support WebTransport, and the invite has no WebRTC details.',
  );
}

/**
 * A joined local world: a way to save the world now (e.g. before quitting), and what hosting it
 * needs (the loopback to relay guests through, and the player's key).
 */
export interface LocalSession {
  save: () => Promise<boolean>;
  loopback: LoopbackTransport;
  key: DeviceKey;
}

/** Starts the integrated server in a worker and joins it (local mode, ARCHITECTURE.md §2.1). */
export async function connectLocal(
  client: Client,
  key: DeviceKey,
  options: ConnectOptions,
): Promise<LocalSession> {
  const worker = new Worker(new URL('../local/worker.ts', import.meta.url), { type: 'module' });
  const world = options.localWorld ?? { worldSeed: 0, generatorVersion: GENERATORS.terrain };
  let maps: Uint8Array[] | undefined;
  try {
    maps =
      (await options.worldMaps?.(BigInt(world.worldSeed), world.generatorVersion)) ?? undefined;
  } catch (err) {
    // The server computes its own (slower to start, the same terrain).
    console.warn('Dwell: the world maps are unavailable', err);
  }
  const loopback = await LoopbackTransport.start(worker, maps ? { ...world, maps } : world);
  // The world saves every few seconds, and at once when the page is hidden or closed (§6.4).
  const save = () => {
    void loopback.save();
  };
  window.addEventListener('pagehide', save);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') save();
  });
  client.connectTransport(loopback);
  return { save: () => loopback.save(), loopback, key };
}

/** How long a guest waits after the room says the host left, for the host's Reject to arrive. */
const HOST_LEFT_GRACE_MS = 1000;

/** Why a join code didn't work, for the player. */
function roomError(err: unknown): Error {
  if (err instanceof MasterError) {
    if (err.code === 'not_found') return new Error('No friend world is open with that code.');
    if (err.code === 'full') return new Error('That world is full.');
    if (err.status === 429) return new Error('Too many tries; wait a minute and try again.');
  }
  return err instanceof Error ? err : new Error(String(err));
}

/**
 * A join code that leads to a host on another compatibility line than this build (RELEASES.md §7):
 * the game opens a build of the host's line through the launcher instead.
 */
export class HostVersionError extends Error {
  override name = 'HostVersionError';
  constructor(
    readonly hostVersion: string,
    readonly buildVersion: string,
  ) {
    super(
      `This game runs Dwell ${hostVersion}, which this build (${buildVersion}) can't join; a build on its version line is needed.`,
    );
  }
}

/**
 * Joins whatever a join code names (§10.3): a dedicated server (connected to directly, like an
 * invite link: the master supplies its current address and certificate) or a browser-hosted
 * friend world (connectToRoom). Returns what to do once joined (a server's receipt).
 */
export async function connectToCode(
  client: Client,
  master: MasterClient,
  code: string,
  options: ConnectOptions,
): Promise<{ onJoined?: () => void }> {
  let found;
  try {
    found = await master.resolve({ code });
  } catch (err) {
    if (err instanceof MasterError && err.code === 'not_found') {
      throw new Error('Nothing is being hosted with that code.', { cause: err });
    }
    throw roomError(err);
  }
  // The master reports the host's app version: any build on its compatibility line can join, others
  // can't (a host from before versioned releases reports none: try, the handshake decides).
  const hostVersion = found.kind === 'room' ? found.appVersion : found.server.appVersion;
  if (hostVersion && !sameLine(hostVersion, options.clientVersion)) {
    throw new HostVersionError(hostVersion, options.clientVersion);
  }
  if (found.kind === 'room') {
    await connectToRoom(client, master, code);
    return {};
  }
  const invite = parseInvite(`?${new URLSearchParams(serverInvite(found.server)).toString()}`);
  if (!invite) throw new Error('The master gave an unusable address for that server.');
  await connectToInvite(client, invite, options.transport);
  // Once joined, confirm it to the master: players' receipts verify a public server (5e).
  const serverCode = found.server.code;
  return {
    onJoined: () => {
      master.receipt(serverCode).catch(() => undefined);
    },
  };
}

/**
 * Joins a browser-hosted friend world by its code (ARCHITECTURE.md §10.2): the master's room
 * introduces this page to the host's, and they connect over WebRTC. The room socket stays open
 * while playing, so the room counts this guest and can say when the host leaves.
 */
export async function connectToRoom(
  client: Client,
  master: MasterClient,
  code: string,
): Promise<void> {
  if (!isWebRtcSupported()) throw new TransportUnavailableError('This browser has no WebRTC.');
  let socket: RoomSocket;
  let iceServers: RTCIceServer[];
  try {
    const [{ token }, servers] = await Promise.all([master.joinRoom(code), master.turn()]);
    iceServers = servers;
    socket = await RoomSocket.open(master.roomSocketUrl(code, token));
  } catch (err) {
    throw roomError(err);
  }
  let transport: PeerTransport | null = null;
  const room = { hostLeft: false };
  const signaler: Signaler = {
    send: (data: SignalData) => {
      socket.signal(data);
    },
    onSignal: null,
  };
  socket.setHandlers({
    onEvent: (event) => {
      if (event.t === 'signal') {
        signaler.onSignal?.(event.data);
      } else if (event.t === 'host-left') {
        room.hostLeft = true;
        const t = transport;
        window.setTimeout(() => t?.end('the host left'), HOST_LEFT_GRACE_MS);
      }
    },
    onClose: () => undefined,
  });
  try {
    transport = await PeerTransport.connect(signaler, iceServers);
  } catch (err) {
    socket.close();
    throw room.hostLeft ? new Error('The host left.') : roomError(err);
  }
  const peer = transport;
  const wrapped: Transport = {
    kind: peer.kind,
    binding: peer.binding,
    setHandlers: (handlers) => {
      peer.setHandlers({
        ...handlers,
        onClose: (info) => {
          socket.close();
          handlers.onClose(info);
        },
      });
    },
    sendReliable: (channel, bytes) => {
      peer.sendReliable(channel, bytes);
    },
    sendDatagram: (bytes) => {
      peer.sendDatagram(bytes);
    },
    close: () => {
      socket.close();
      peer.close();
    },
  };
  client.connectTransport(wrapped);
}
