import { describe, expect, it } from 'vitest';
import type { ToWorker } from '../local/messages';
import { Channel, HostState, TransportKind } from '../protocol/constants.gen';
import {
  CLOSE_GRACE_MS,
  HostRelay,
  maxGuestsFor,
  type GuestLink,
  type RoomLink,
  type WorkerLink,
} from './hosting';
import { LOCAL_SESSION } from './loopback';
import type { HostPeerHandlers, SignalData } from './peer';
import type { RoomSocketHandlers } from './roomSocket';

class FakeGuest implements GuestLink {
  signals: SignalData[] = [];
  sent: [Channel | 'datagram', number][] = [];
  closed = false;
  constructor(
    readonly signal: (data: SignalData) => void,
    readonly handlers: HostPeerHandlers,
  ) {}
  onSignal(data: SignalData): Promise<void> {
    this.signals.push(data);
    return Promise.resolve();
  }
  send(channel: Channel, bytes: Uint8Array): void {
    this.sent.push([channel, bytes[0] ?? -1]);
  }
  sendDatagram(bytes: Uint8Array): void {
    this.sent.push(['datagram', bytes[0] ?? -1]);
  }
  close(): void {
    this.closed = true;
  }
}

class FakeRoom implements RoomLink {
  signals: [SignalData, number | undefined][] = [];
  closed = false;
  constructor(readonly setHandlers: (h: RoomSocketHandlers) => void) {}
  signal(data: SignalData, to?: number): void {
    this.signals.push([data, to]);
  }
  close(): void {
    this.closed = true;
  }
}

function setup() {
  const posted: ToWorker[] = [];
  const worker: WorkerLink = {
    postToWorker: (msg) => posted.push(msg),
    onGuestOutput: null,
  };
  let room: RoomSocketHandlers | null = null;
  const roomLink = new FakeRoom((h) => {
    room = h;
  });
  const guests: FakeGuest[] = [];
  const timers: (() => void)[] = [];
  const binding = new Uint8Array(32).fill(7);
  const relay = new HostRelay(
    worker,
    roomLink,
    binding,
    (signal, handlers) => {
      const g = new FakeGuest(signal, handlers);
      guests.push(g);
      return g;
    },
    (fn, ms) => {
      expect(ms).toBe(CLOSE_GRACE_MS);
      timers.push(fn);
    },
  );
  const roomEvent = (e: Parameters<RoomSocketHandlers['onEvent']>[0]) => room?.onEvent(e);
  const output = (msg: Parameters<NonNullable<WorkerLink['onGuestOutput']>>[0]) =>
    worker.onGuestOutput?.(msg);
  const runTimers = () => {
    for (const t of timers.splice(0)) t();
  };
  return {
    relay,
    posted,
    roomLink,
    guests,
    roomEvent,
    output,
    runTimers,
    binding,
    room: () => room,
  };
}

describe('the hosting relay', () => {
  it('connects a guest to the local server as its own WebRTC session once its channels open', () => {
    const t = setup();
    t.roomEvent({ t: 'guest', peer: 1 });
    const guest = t.guests[0];
    if (!guest) throw new Error('no guest');

    // Signaling flows between the room and the guest's peer connection.
    t.roomEvent({ t: 'signal', from: 1, data: { type: 'offer', sdp: 'o' } });
    expect(guest.signals).toEqual([{ type: 'offer', sdp: 'o' }]);
    guest.signal({ type: 'answer', sdp: 'a' });
    expect(t.roomLink.signals).toEqual([[{ type: 'answer', sdp: 'a' }, 1]]);
    expect(t.posted).toEqual([]);

    guest.handlers.onOpen();
    const connect = t.posted[0];
    expect(connect).toMatchObject({ t: 'connect', kind: TransportKind.WebRtc, binding: t.binding });
    const session = connect?.t === 'connect' ? connect.session : -1;
    expect(session).not.toBe(LOCAL_SESSION);
    expect(t.relay.guestCount).toBe(1);

    // Guest → server.
    guest.handlers.onReliable(Channel.control, Uint8Array.of(5));
    guest.handlers.onDatagram(Uint8Array.of(6));
    expect(t.posted.slice(1)).toEqual([
      { t: 'reliable', session, channel: Channel.control, bytes: Uint8Array.of(5) },
      { t: 'datagram', session, bytes: Uint8Array.of(6) },
    ]);

    // Server → guest, on the server's channel.
    t.output({ t: 'reliable', session, channel: Channel.world, bytes: Uint8Array.of(8) });
    t.output({ t: 'datagram', session, bytes: Uint8Array.of(9) });
    t.output({
      t: 'reliable',
      session: session + 100,
      channel: Channel.world,
      bytes: Uint8Array.of(1),
    });
    expect(guest.sent).toEqual([
      [Channel.world, 8],
      ['datagram', 9],
    ]);

    // The connection drops: the server hears the session is gone.
    guest.handlers.onClose();
    expect(t.posted.at(-1)).toEqual({ t: 'disconnect', session });
    expect(t.relay.guestCount).toBe(0);
  });

  it('gives each guest its own session', () => {
    const t = setup();
    t.roomEvent({ t: 'guest', peer: 1 });
    t.roomEvent({ t: 'guest', peer: 2 });
    for (const g of t.guests) g.handlers.onOpen();
    const sessions = t.posted.flatMap((m) => (m.t === 'connect' ? [m.session] : []));
    expect(new Set(sessions).size).toBe(2);
  });

  it('closes a guest the server ends only after its Reject has had time to go out', () => {
    const t = setup();
    t.roomEvent({ t: 'guest', peer: 1 });
    const guest = t.guests[0];
    guest?.handlers.onOpen();
    const connect = t.posted[0];
    const session = connect?.t === 'connect' ? connect.session : -1;
    t.output({ t: 'reliable', session, channel: Channel.control, bytes: Uint8Array.of(1) });
    t.output({ t: 'close', session });
    expect(guest?.closed).toBe(false);
    expect(t.relay.guestCount).toBe(0);
    t.runTimers();
    expect(guest?.closed).toBe(true);
    // The server closed it; no disconnect is echoed back.
    expect(t.posted.some((m) => m.t === 'disconnect')).toBe(false);
  });

  it('pauses and resumes the world', () => {
    const t = setup();
    t.relay.setPaused(true);
    t.relay.setPaused(false);
    expect(t.posted).toEqual([
      { t: 'hostStatus', state: HostState.Paused },
      { t: 'hostStatus', state: HostState.Resumed },
    ]);
  });

  it('on stop, ends the guests’ sessions, then closes the room, and admits nobody new', () => {
    const t = setup();
    t.roomEvent({ t: 'guest', peer: 1 });
    t.roomEvent({ t: 'guest', peer: 2 });
    t.guests[0]?.handlers.onOpen();
    t.posted.length = 0;
    t.relay.stop();
    expect(t.posted).toEqual([{ t: 'hostStatus', state: HostState.Resumed }, { t: 'closeGuests' }]);
    // A guest still connecting has no session: its connection closes at once.
    expect(t.guests[1]?.closed).toBe(true);
    expect(t.roomLink.closed).toBe(false);
    t.runTimers();
    expect(t.roomLink.closed).toBe(true);
    t.roomEvent({ t: 'guest', peer: 3 });
    expect(t.guests).toHaveLength(2);
  });

  it('drops a guest that leaves the room before connecting, and reports a lost room', () => {
    const t = setup();
    let lost = false;
    t.relay.onRoomLost = () => {
      lost = true;
    };
    t.roomEvent({ t: 'guest', peer: 1 });
    t.roomEvent({ t: 'guest-left', peer: 1 });
    expect(t.guests[0]?.closed).toBe(true);
    t.room()?.onClose();
    expect(lost).toBe(true);
  });

  it('allows fewer guests on phones', () => {
    expect(maxGuestsFor(true)).toBeLessThan(maxGuestsFor(false));
  });
});
