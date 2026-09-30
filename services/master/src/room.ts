// The Room Durable Object (ARCHITECTURE.md §10.2–10.3, ADR 0013): one per hosted friend world,
// named by its join code. It relays WebRTC signaling (SDP offers/answers and ICE candidates)
// between the host and each guest over hibernating WebSockets, and closes when the host leaves.
//
// Flow: the Worker calls open() for a new code (the host gets a token) and join() for a guest (a
// one-use token); each then opens a WebSocket to /v1/rooms/<code>/ws?token=…, which the Worker
// forwards here. Messages are JSON:
//   host → room  {t:'signal', to:<peer>, data}      room → guest {t:'signal', data}
//   guest → room {t:'signal', data}                  room → host  {t:'signal', from:<peer>, data}
//   room → host  {t:'guest', peer} / {t:'guest-left', peer}   room → guest {t:'host-left'}
import { DurableObject } from 'cloudflare:workers';
import type { Env } from './env';
import { problem } from './http';

/** How long a new room waits for its host's WebSocket, and a guest token stays valid. */
export const CONNECT_WITHIN_MS = 60_000;
/** Largest signaling message relayed (an SDP offer with candidates is a few KB). */
export const MAX_SIGNAL_BYTES = 16 * 1024;
/** Most messages a socket may send per 10 s window (trickled ICE sends dozens at most). */
export const MAX_MESSAGES_PER_WINDOW = 200;
export const MAX_GUESTS = 16;

interface GuestTicket {
  peer: number;
  key: string;
  expires: number;
}

interface RoomState {
  hostKey: string;
  hostToken: string;
  maxGuests: number;
  nextPeer: number;
  tickets: Record<string, GuestTicket>;
  closed: boolean;
}

interface Attachment {
  role: 'host' | 'guest';
  peer: number;
  window: number;
  count: number;
}

export type JoinResult =
  { ok: true; token: string; peer: number } | { ok: false; code: 'not_found' | 'full' };

function token(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(24));
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}

export class Room extends DurableObject<Env> {
  private async state(): Promise<RoomState | undefined> {
    return this.ctx.storage.get<RoomState>('room');
  }

  private host(): WebSocket | undefined {
    return this.ctx.getWebSockets('host')[0];
  }

  private guest(peer: number): WebSocket | undefined {
    return this.ctx.getWebSockets(`guest:${String(peer)}`)[0];
  }

  /** Opens the room for a host; null if the code is in use. */
  async open(
    hostKey: string,
    maxGuests: number,
    now: number,
  ): Promise<{ hostToken: string } | null> {
    const existing = await this.state();
    if (existing && !existing.closed) return null;
    const room: RoomState = {
      hostKey,
      hostToken: token(),
      maxGuests: Math.max(1, Math.min(MAX_GUESTS, Math.floor(maxGuests))),
      nextPeer: 1,
      tickets: {},
      closed: false,
    };
    await this.ctx.storage.put('room', room);
    // The host must connect soon, or the code is freed.
    await this.ctx.storage.setAlarm(now + CONNECT_WITHIN_MS);
    return { hostToken: room.hostToken };
  }

  /** Admits a guest (a one-use token for the WebSocket), unless the room is closed or full. */
  async join(guestKey: string, now: number): Promise<JoinResult> {
    const room = await this.state();
    if (!room || room.closed || !this.host()) return { ok: false, code: 'not_found' };
    for (const [t, ticket] of Object.entries(room.tickets)) {
      if (ticket.expires <= now) Reflect.deleteProperty(room.tickets, t);
    }
    const connected = this.ctx.getWebSockets().length - 1; // all but the host
    if (connected + Object.keys(room.tickets).length >= room.maxGuests) {
      return { ok: false, code: 'full' };
    }
    const peer = room.nextPeer++;
    const t = token();
    room.tickets[t] = { peer, key: guestKey, expires: now + CONNECT_WITHIN_MS };
    await this.ctx.storage.put('room', room);
    return { ok: true, token: t, peer };
  }

  /** A WebSocket upgrade forwarded by the Worker (`?token=` names the host or a guest). */
  override async fetch(request: Request): Promise<Response> {
    if (request.headers.get('upgrade')?.toLowerCase() !== 'websocket') {
      return problem(426, 'upgrade', 'Expected a WebSocket.');
    }
    const room = await this.state();
    const given = new URL(request.url).searchParams.get('token') ?? '';
    if (!room || room.closed || given === '') return problem(404, 'not_found', 'No such room.');
    const pair = new WebSocketPair();
    const [client, server] = [pair[0], pair[1]];
    const now = Date.now();
    if (given === room.hostToken) {
      // A reconnecting host replaces its old socket.
      this.host()?.close(4000, 'replaced');
      this.ctx.acceptWebSocket(server, ['host']);
      server.serializeAttachment({
        role: 'host',
        peer: 0,
        window: now,
        count: 0,
      } satisfies Attachment);
    } else {
      const ticket = room.tickets[given];
      if (!ticket || ticket.expires <= now || !this.host()) {
        return problem(404, 'not_found', 'This invitation has expired.');
      }
      Reflect.deleteProperty(room.tickets, given);
      await this.ctx.storage.put('room', room);
      this.ctx.acceptWebSocket(server, [`guest:${String(ticket.peer)}`]);
      server.serializeAttachment({
        role: 'guest',
        peer: ticket.peer,
        window: now,
        count: 0,
      } satisfies Attachment);
      this.host()?.send(JSON.stringify({ t: 'guest', peer: ticket.peer }));
    }
    return new Response(null, { status: 101, webSocket: client });
  }

  override webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void {
    const a = ws.deserializeAttachment() as Attachment;
    const now = Date.now();
    if (now - a.window > 10_000) {
      a.window = now;
      a.count = 0;
    }
    a.count++;
    ws.serializeAttachment(a);
    if (a.count > MAX_MESSAGES_PER_WINDOW) {
      ws.close(4008, 'too many messages');
      return;
    }
    if (typeof message !== 'string' || message.length > MAX_SIGNAL_BYTES) return;
    let m: unknown;
    try {
      m = JSON.parse(message);
    } catch {
      return;
    }
    if (typeof m !== 'object' || m === null || (m as { t?: unknown }).t !== 'signal') return;
    const data = (m as { data?: unknown }).data;
    if (a.role === 'host') {
      const to = (m as { to?: unknown }).to;
      if (typeof to !== 'number') return;
      this.guest(to)?.send(JSON.stringify({ t: 'signal', data }));
    } else {
      this.host()?.send(JSON.stringify({ t: 'signal', from: a.peer, data }));
    }
  }

  override async webSocketClose(ws: WebSocket): Promise<void> {
    const a = ws.deserializeAttachment() as Attachment | null;
    if (!a) return;
    if (a.role === 'guest') {
      this.host()?.send(JSON.stringify({ t: 'guest-left', peer: a.peer }));
      return;
    }
    // A replaced host socket closing leaves the room open.
    if (this.ctx.getWebSockets('host').some((s) => s !== ws)) return;
    await this.shut();
  }

  override async webSocketError(ws: WebSocket): Promise<void> {
    await this.webSocketClose(ws);
  }

  /** The host never connected (or left): close the room and free the code. */
  override async alarm(): Promise<void> {
    if (!this.host()) await this.shut();
  }

  private async shut(): Promise<void> {
    for (const ws of this.ctx.getWebSockets()) {
      const a = ws.deserializeAttachment() as Attachment | null;
      if (a?.role === 'guest') {
        try {
          ws.send(JSON.stringify({ t: 'host-left' }));
          ws.close(1000, 'host left');
        } catch {
          // already closed
        }
      }
    }
    await this.ctx.storage.deleteAll();
  }

  /** Tests: whether a room is open under this name. */
  async isOpen(): Promise<boolean> {
    const room = await this.state();
    return room !== undefined && !room.closed;
  }
}
