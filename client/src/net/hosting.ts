// Friend-world hosting from the browser (ARCHITECTURE.md §10.2, Phase 5c). The host page keeps
// playing through its LoopbackTransport; each guest connects over WebRTC (net/peer.ts), signaled
// through the master's room (net/roomSocket.ts), and the host page relays the guest's data channels
// to the local-mode worker as another session (TransportKind WebRTC, binding = the SHA-256 of the
// host's certificate). Peer connections live on the main thread because workers have none.
import type { ToWorker } from '../local/messages';
import { HostPolicy } from '../local/wasmCore';
import { Channel, HostState, TransportKind } from '../protocol/constants.gen';
import { LOCAL_SESSION, type GuestOutput } from './loopback';
import type { MasterClient } from './master';
import { certificateSha256, HostPeer, type HostPeerHandlers, type SignalData } from './peer';
import { RoomSocket, type RoomEvent, type RoomSocketHandlers } from './roomSocket';

export { HostPolicy };

export interface HostSettings {
  /** Guests at once (not counting the host). */
  maxGuests: number;
  /** Who may edit blocks, and who may fly (the host is always an op). */
  edits: HostPolicy;
  flight: HostPolicy;
  /** Code only, or also listed to players on the host's network (Phase 5d). */
  visibility: 'code' | 'network';
  /** The world's name, as listed to the network. */
  name: string;
}

/** Most guests a host may allow, by platform (host profiles, §10.2). */
export const MAX_GUESTS_DESKTOP = 8;
export const MAX_GUESTS_MOBILE = 4;

export function maxGuestsFor(mobile: boolean): number {
  return mobile ? MAX_GUESTS_MOBILE : MAX_GUESTS_DESKTOP;
}

/** How long a guest's channels stay open after the server closes its session (its Reject). */
export const CLOSE_GRACE_MS = 500;

/** The local server, as the relay sees it (LoopbackTransport). */
export interface WorkerLink {
  postToWorker(msg: ToWorker): void;
  onGuestOutput: ((msg: GuestOutput) => void) | null;
}

/** The room socket, as the relay sees it. */
export interface RoomLink {
  setHandlers(handlers: RoomSocketHandlers): void;
  signal(data: SignalData, to?: number): void;
  close(): void;
}

/** A guest's connection, as the relay sees it (HostPeer). */
export interface GuestLink {
  onSignal(data: SignalData): Promise<void>;
  send(channel: Channel, bytes: Uint8Array): void;
  sendDatagram(bytes: Uint8Array): void;
  close(): void;
}

export type PeerFactory = (
  signal: (data: SignalData) => void,
  handlers: HostPeerHandlers,
) => GuestLink;

interface Guest {
  link: GuestLink;
  /** The guest's session in the local server, once its channels are open. */
  session: number | null;
}

/** Session ids for guests: never the local player's, never reused while the page lives. */
let nextSession = LOCAL_SESSION + 1;

/** Relays guests between the room, their peer connections and the local server. */
export class HostRelay {
  private readonly guests = new Map<number, Guest>();
  private readonly bySession = new Map<number, number>();
  private stopped = false;
  /** Guests connected (their channels open). */
  onGuestsChanged: ((count: number) => void) | null = null;
  /** The room closed before hosting stopped: the code no longer works (guests stay connected). */
  onRoomLost: (() => void) | null = null;

  constructor(
    private readonly worker: WorkerLink,
    private readonly room: RoomLink,
    private readonly binding: Uint8Array,
    private readonly newPeer: PeerFactory,
    private readonly schedule: (fn: () => void, ms: number) => void = (fn, ms) => {
      window.setTimeout(fn, ms);
    },
  ) {
    worker.onGuestOutput = (msg) => {
      this.onGuestOutput(msg);
    };
    room.setHandlers({
      onEvent: (event) => {
        this.onRoomEvent(event);
      },
      onClose: () => {
        if (!this.stopped) this.onRoomLost?.();
      },
    });
  }

  get guestCount(): number {
    return this.bySession.size;
  }

  /** Pauses or resumes the world (the host page was hidden or shown); guests are told. */
  setPaused(paused: boolean): void {
    if (this.stopped) return;
    this.worker.postToWorker({
      t: 'hostStatus',
      state: paused ? HostState.Paused : HostState.Resumed,
    });
  }

  /**
   * Stops hosting: the server ends each guest's session with Reject(ServerClosing) before its
   * connection closes, then the room closes (the code stops working). The world resumes if paused.
   */
  stop(): void {
    if (this.stopped) return;
    this.worker.postToWorker({ t: 'hostStatus', state: HostState.Resumed });
    this.stopped = true;
    this.worker.postToWorker({ t: 'closeGuests' });
    // Guests whose channels never opened have no session to close.
    for (const [peer, guest] of this.guests) {
      if (guest.session === null) this.drop(peer);
    }
    // The room closing tells guests the host left; let the Rejects arrive first.
    this.schedule(() => {
      this.room.close();
    }, CLOSE_GRACE_MS);
  }

  private onRoomEvent(event: RoomEvent): void {
    if (this.stopped) return;
    switch (event.t) {
      case 'guest':
        this.admit(event.peer);
        break;
      case 'guest-left':
        // Signaling is over; an open connection carries on (the guest's page left the room).
        if (this.guests.get(event.peer)?.session === null) this.drop(event.peer);
        break;
      case 'signal':
        if (event.from !== null) void this.guests.get(event.from)?.link.onSignal(event.data);
        break;
      case 'host-left':
        break;
    }
  }

  private admit(peer: number): void {
    if (this.guests.has(peer)) return;
    const guest: Guest = { link: null as unknown as GuestLink, session: null };
    guest.link = this.newPeer(
      (data) => {
        this.room.signal(data, peer);
      },
      {
        onOpen: () => {
          if (this.stopped) {
            guest.link.close();
            return;
          }
          const session = nextSession++;
          guest.session = session;
          this.bySession.set(session, peer);
          this.worker.postToWorker({
            t: 'connect',
            session,
            kind: TransportKind.WebRtc,
            binding: this.binding,
          });
          this.onGuestsChanged?.(this.guestCount);
        },
        onReliable: (channel, bytes) => {
          if (guest.session !== null) {
            this.worker.postToWorker({ t: 'reliable', session: guest.session, channel, bytes });
          }
        },
        onDatagram: (bytes) => {
          if (guest.session !== null) {
            this.worker.postToWorker({ t: 'datagram', session: guest.session, bytes });
          }
        },
        onClose: () => {
          this.forget(peer, true);
        },
      },
    );
    this.guests.set(peer, guest);
  }

  private onGuestOutput(msg: GuestOutput): void {
    const peer = this.bySession.get(msg.session);
    const guest = peer === undefined ? undefined : this.guests.get(peer);
    if (peer === undefined || !guest) return;
    switch (msg.t) {
      case 'reliable':
        guest.link.send(msg.channel, msg.bytes);
        break;
      case 'datagram':
        guest.link.sendDatagram(msg.bytes);
        break;
      case 'close':
        // The server ended the session (its Reject is on its way): close once it has gone.
        this.forget(peer, false);
        this.schedule(() => {
          guest.link.close();
        }, CLOSE_GRACE_MS);
        break;
    }
  }

  /** Closes a guest's connection now. */
  private drop(peer: number): void {
    const guest = this.guests.get(peer);
    if (!guest) return;
    this.forget(peer, true);
    guest.link.close();
  }

  /** Forgets a guest; `disconnect`: tell the server its session is gone. */
  private forget(peer: number, disconnect: boolean): void {
    const guest = this.guests.get(peer);
    if (!guest) return;
    this.guests.delete(peer);
    if (guest.session !== null) {
      this.bySession.delete(guest.session);
      if (disconnect) this.worker.postToWorker({ t: 'disconnect', session: guest.session });
      this.onGuestsChanged?.(this.guestCount);
    }
  }
}

/** A running friend world: its join code, and the relay. */
export interface Hosting {
  /** The code as typed (e.g. "KQ7XM4") and as shown ("KQ7-XM4"). */
  code: string;
  display: string;
  relay: HostRelay;
}

/**
 * Starts hosting the local world: opens a room on the master, sets the server's limits and
 * policies (the host, `hostKey`, is an op) and relays guests as they come.
 */
export async function startHosting(
  worker: WorkerLink,
  master: MasterClient,
  hostKey: Uint8Array,
  settings: HostSettings,
): Promise<Hosting> {
  const keygen: EcKeyGenParams = { name: 'ECDSA', namedCurve: 'P-256' };
  const certificate = await RTCPeerConnection.generateCertificate(keygen);
  const binding = certificateSha256(certificate);
  if (!binding) throw new Error('This browser gave no certificate fingerprint.');
  const [iceServers, room] = await Promise.all([
    master.turn(),
    master.createRoom(settings.maxGuests, settings.visibility, settings.name),
  ]);
  const socket = await RoomSocket.open(master.roomSocketUrl(room.code, room.hostToken));
  worker.postToWorker({
    t: 'host',
    maxPlayers: settings.maxGuests + 1,
    edits: settings.edits,
    flight: settings.flight,
    hostKey,
  });
  const relay = new HostRelay(
    worker,
    socket,
    binding,
    (signal, handlers) => new HostPeer(signal, iceServers, certificate, handlers),
  );
  return { code: room.code, display: room.display, relay };
}
