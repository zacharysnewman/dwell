// Pings a dedicated server for the lobby list (ARCHITECTURE.md §8.3, §10.3, Phase 5e): opens a
// transport, sends `StatusRequest` on the control stream and times the `StatusResponse`, which
// the server answers without a join. Friend worlds are not pinged (their players are listed).
import { Channel, MessageType } from '../protocol/constants.gen';
import { decode, encode, type Message } from '../protocol/messages';
import type { Transport } from './Transport';

export type StatusResponse = Extract<Message, { type: typeof MessageType.StatusResponse }>;

export interface PingResult {
  /** Round trip of the status query, ms (the connection's setup not included). */
  rttMs: number;
  status: StatusResponse;
}

/** Most time a ping may take, connecting included. */
export const PING_TIMEOUT_MS = 5000;

/**
 * Asks an open transport for the server's status and closes it. Rejects on a timeout, a closed
 * connection or an unexpected answer.
 */
export function queryStatus(
  transport: Transport,
  now: () => number = () => performance.now(),
  timeoutMs = PING_TIMEOUT_MS,
): Promise<PingResult> {
  return new Promise<PingResult>((resolve, reject) => {
    let sentAt = 0;
    const timer = setTimeout(() => {
      finish(new Error('The server did not answer.'));
    }, timeoutMs);
    const finish = (result: PingResult | Error) => {
      clearTimeout(timer);
      transport.setHandlers({
        onReliable: () => undefined,
        onDatagram: () => undefined,
        onClose: () => undefined,
      });
      transport.close();
      if (result instanceof Error) reject(result);
      else resolve(result);
    };
    transport.setHandlers({
      onReliable: (channel, bytes) => {
        if (channel !== Channel.control) return;
        let m: Message;
        try {
          m = decode(bytes);
        } catch {
          finish(new Error('The server sent a malformed status.'));
          return;
        }
        if (m.type === MessageType.StatusResponse) finish({ rttMs: now() - sentAt, status: m });
      },
      onDatagram: () => undefined,
      onClose: (info) => {
        finish(new Error(info.message || 'The connection closed.'));
      },
    });
    sentAt = now();
    transport.sendReliable(Channel.control, encode({ type: MessageType.StatusRequest }));
  });
}

/** Opens a transport with `open` (bounded by the timeout) and queries the server's status. */
export async function pingServer(
  open: () => Promise<Transport>,
  now: () => number = () => performance.now(),
  timeoutMs = PING_TIMEOUT_MS,
): Promise<PingResult> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error('The server did not answer.'));
    }, timeoutMs);
  });
  const opening = open();
  let transport: Transport;
  try {
    transport = await Promise.race([opening, timeout]);
  } catch (err) {
    clearTimeout(timer);
    // A connection that opens after the timeout is closed at once.
    opening.then(
      (t) => {
        t.close();
      },
      () => undefined,
    );
    throw err;
  }
  try {
    return await queryStatus(transport, now, timeoutMs);
  } finally {
    clearTimeout(timer);
  }
}
