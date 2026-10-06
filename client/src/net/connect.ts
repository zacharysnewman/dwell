// Chooses how to reach a server and runs the session (ARCHITECTURE.md §8.1, §10).
import { IndexedDbKeyStore, loadOrCreateDeviceKey, type DeviceKey } from '../identity/deviceKey';
import type { Invite } from './invite';
import { MasterClient, MasterError, serverInvite } from './master';
import { parseInvite } from './invite';
import { PeerTransport, type SignalData, type Signaler } from './peer';
import { RoomSocket } from './roomSocket';
import { GENERATORS, type LocalWorld } from '../local/world';
import { sameLine } from '../version/semver';
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
  /** Local mode: the world to open (from the menu, or `?world=`, `?seed=`). */
  localWorld?: LocalWorld;
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

/**
 * A joined local world: its session, a way to save the world now (e.g. before quitting), and what
 * hosting it needs (the loopback to relay guests through, and the player's key).
 */
export interface LocalSession {
  session: ClientSession;
  save: () => Promise<boolean>;
  loopback: LoopbackTransport;
  key: DeviceKey;
}

/** Starts the integrated server in a worker and joins it (local mode, ARCHITECTURE.md §2.1). */
export async function connectLocal(options: ConnectOptions): Promise<LocalSession> {
  const key = await loadOrCreateDeviceKey(new IndexedDbKeyStore());
  const worker = new Worker(new URL('../local/worker.ts', import.meta.url), { type: 'module' });
  const world = options.localWorld ?? { worldSeed: 0, generatorVersion: GENERATORS.terrain };
  const loopback = await LoopbackTransport.start(worker, world);
  // The world saves every few seconds, and at once when the page is hidden or closed (§6.4).
  const save = () => {
    void loopback.save();
  };
  window.addEventListener('pagehide', save);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') save();
  });
  const transport = simulate(loopback, options);
  const session = new ClientSession(transport, key, options);
  session.start();
  return { session, save: () => loopback.save(), loopback, key };
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
 * friend world (connectToRoom).
 */
export async function connectToCode(
  master: MasterClient,
  code: string,
  options: ConnectOptions,
): Promise<ClientSession> {
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
  if (found.kind === 'room') return connectToRoom(master, code, options);
  const invite = parseInvite(`?${new URLSearchParams(serverInvite(found.server)).toString()}`);
  if (!invite) throw new Error('The master gave an unusable address for that server.');
  const session = await connectToInvite(invite, options);
  // Once joined, confirm it to the master: players' receipts verify a public server (5e).
  const serverCode = found.server.code;
  let confirmed = false;
  const unsubscribe = session.subscribe((state) => {
    if (state.phase !== 'joined' || confirmed) return;
    confirmed = true;
    queueMicrotask(() => {
      unsubscribe();
    });
    master.receipt(serverCode).catch(() => undefined);
  });
  return session;
}

/**
 * Joins a browser-hosted friend world by its code (ARCHITECTURE.md §10.2): the master's room
 * introduces this page to the host's, and they connect over WebRTC. The room socket stays open
 * while playing, so the room counts this guest and can say when the host leaves.
 */
export async function connectToRoom(
  master: MasterClient,
  code: string,
  options: ConnectOptions,
): Promise<ClientSession> {
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
  const session = new ClientSession(
    simulate(
      {
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
      },
      options,
    ),
    await loadOrCreateDeviceKey(new IndexedDbKeyStore()),
    options,
  );
  session.start();
  return session;
}
