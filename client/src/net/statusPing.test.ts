import { describe, expect, it } from 'vitest';
import { Channel, MessageType } from '../protocol/constants.gen';
import { decode, encode } from '../protocol/messages';
import { pingServer, queryStatus } from './statusPing';
import type { Transport, TransportHandlers } from './Transport';

/** A transport whose "server" answers a StatusRequest after `delay` ms of fake time. */
class FakeTransport implements Transport {
  readonly kind = 1 as Transport['kind'];
  readonly binding = new Uint8Array(32);
  handlers: TransportHandlers | null = null;
  closed = 0;
  constructor(
    private readonly clock: { t: number },
    private readonly answer: 'status' | 'nothing' | 'close',
  ) {}
  setHandlers(h: TransportHandlers): void {
    this.handlers = h;
  }
  sendReliable(channel: Channel, bytes: Uint8Array): void {
    expect(channel).toBe(Channel.control);
    expect(decode(bytes)).toEqual({ type: MessageType.StatusRequest });
    queueMicrotask(() => {
      if (this.answer === 'close') {
        this.handlers?.onClose({ message: 'gone' });
        return;
      }
      if (this.answer !== 'status') return;
      this.clock.t += 42;
      const status = encode({
        type: MessageType.StatusResponse,
        protocolVersion: 10,
        serverName: 'Home',
        motd: 'Hi',
        players: 2,
        maxPlayers: 16,
        flags: 0,
      });
      this.handlers?.onReliable(Channel.control, status);
    });
  }
  sendDatagram(): void {}
  close(): void {
    this.closed++;
  }
}

describe('status ping', () => {
  it('times the StatusResponse and closes the connection', async () => {
    const clock = { t: 1000 };
    const t = new FakeTransport(clock, 'status');
    const result = await queryStatus(t, () => clock.t);
    expect(result.rttMs).toBe(42);
    expect(result.status).toMatchObject({ serverName: 'Home', players: 2, protocolVersion: 10 });
    expect(t.closed).toBe(1);
  });

  it('fails when the server does not answer or the connection closes', async () => {
    const clock = { t: 0 };
    const silent = new FakeTransport(clock, 'nothing');
    await expect(queryStatus(silent, () => clock.t, 20)).rejects.toThrow(/did not answer/);
    expect(silent.closed).toBe(1);
    await expect(queryStatus(new FakeTransport(clock, 'close'), () => clock.t)).rejects.toThrow(
      /gone/,
    );
  });

  it('gives up on a connection that takes too long, and closes it when it opens', async () => {
    const clock = { t: 0 };
    const late = new FakeTransport(clock, 'status');
    let open: (t: Transport) => void = () => undefined;
    const ping = pingServer(
      () =>
        new Promise<Transport>((resolve) => {
          open = resolve;
        }),
      () => clock.t,
      20,
    );
    await expect(ping).rejects.toThrow(/did not answer/);
    open(late);
    await Promise.resolve();
    expect(late.closed).toBe(1);
  });
});
