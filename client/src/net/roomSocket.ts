// A WebSocket to a friend world's room on the master (ARCHITECTURE.md §10.2–10.3): signaling
// between the host and its guests. The Room Durable Object (services/master/src/room.ts) relays
// JSON messages:
//   host → room  {t:'signal', to:<peer>, data}      room → guest {t:'signal', data}
//   guest → room {t:'signal', data}                  room → host  {t:'signal', from:<peer>, data}
//   room → host  {t:'guest', peer} / {t:'guest-left', peer}   room → guest {t:'host-left'}
import { isSignalData, type SignalData } from './peer';

export type RoomEvent =
  | { t: 'signal'; from: number | null; data: SignalData }
  | { t: 'guest'; peer: number }
  | { t: 'guest-left'; peer: number }
  | { t: 'host-left' };

/** One message from the room, or null if it is not one we understand. */
export function parseRoomEvent(text: string): RoomEvent | null {
  let m: unknown;
  try {
    m = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof m !== 'object' || m === null) return null;
  const r = m as Record<string, unknown>;
  switch (r.t) {
    case 'signal': {
      if (!isSignalData(r.data)) return null;
      const from = typeof r.from === 'number' ? r.from : null;
      return { t: 'signal', from, data: r.data };
    }
    case 'guest':
    case 'guest-left':
      return typeof r.peer === 'number' ? { t: r.t, peer: r.peer } : null;
    case 'host-left':
      return { t: 'host-left' };
    default:
      return null;
  }
}

export interface RoomSocketHandlers {
  onEvent(event: RoomEvent): void;
  /** The socket closed (the room closed, or the connection to the master was lost). */
  onClose(): void;
}

/** The room socket for a host or a guest (the token says which). */
export class RoomSocket {
  private handlers: RoomSocketHandlers | null = null;
  private closed = false;

  private constructor(private readonly ws: WebSocket) {
    ws.onmessage = (e: MessageEvent) => {
      if (typeof e.data !== 'string') return;
      const event = parseRoomEvent(e.data);
      if (event) this.handlers?.onEvent(event);
    };
    ws.onclose = () => {
      this.closed = true;
      const h = this.handlers;
      this.handlers = null;
      h?.onClose();
    };
  }

  static open(url: string, timeoutMs = 10_000): Promise<RoomSocket> {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(url);
      const timer = window.setTimeout(() => {
        ws.close();
        reject(new Error('Could not reach the master server.'));
      }, timeoutMs);
      ws.onopen = () => {
        window.clearTimeout(timer);
        resolve(new RoomSocket(ws));
      };
      ws.onerror = () => {
        window.clearTimeout(timer);
        reject(new Error('Could not reach the master server.'));
      };
    });
  }

  setHandlers(handlers: RoomSocketHandlers): void {
    this.handlers = handlers;
  }

  /** Sends a signal: to a guest (`to` is its peer number) from the host, or to the host. */
  signal(data: SignalData, to?: number): void {
    if (this.closed || this.ws.readyState !== WebSocket.OPEN) return;
    this.ws.send(
      JSON.stringify(to === undefined ? { t: 'signal', data } : { t: 'signal', to, data }),
    );
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.handlers = null;
    this.ws.close(1000, 'bye');
  }
}
