import { afterEach, describe, expect, it } from 'vitest';
import { frame } from './framing';
import { WebTransportTransport } from './webTransport';

/** A WebTransport session the test drives: what the server sent, and when its end closed. */
class FakeWebTransport {
  static last: FakeWebTransport | null = null;
  readonly ready = Promise.resolve();
  control!: ReadableStreamDefaultController<Uint8Array>;
  writesFail = false;
  private closedResolve!: (info: { reason: string }) => void;
  readonly closed = new Promise<{ reason: string }>((r) => (this.closedResolve = r));
  readonly datagrams = {
    writable: new WritableStream<Uint8Array>(),
    readable: new ReadableStream<Uint8Array>(),
  };
  readonly incomingUnidirectionalStreams = new ReadableStream<ReadableStream<Uint8Array>>();
  constructor() {
    FakeWebTransport.last = this;
  }
  createBidirectionalStream(): Promise<{
    readable: ReadableStream<Uint8Array>;
    writable: WritableStream<Uint8Array>;
  }> {
    const readable = new ReadableStream<Uint8Array>({ start: (c) => (this.control = c) });
    const writable = new WritableStream<Uint8Array>({
      write: () => (this.writesFail ? Promise.reject(new Error('reset')) : Promise.resolve()),
    });
    return Promise.resolve({ readable, writable });
  }
  /** The server has closed: writes fail from now on. */
  serverClosed(): void {
    this.writesFail = true;
  }
  /** What the server sent before closing, read by the client only now; then the session ends. */
  deliverAndEnd(bytes: Uint8Array): void {
    this.control.enqueue(bytes);
    this.control.close();
    this.closedResolve({ reason: '' });
  }
  close(): void {}
}

const original = globalThis.WebTransport;
afterEach(() => {
  globalThis.WebTransport = original;
});

describe('WebTransport transport', () => {
  it('delivers what the server sent before closing, even when a write fails first', async () => {
    // Regression (CI): a Reject (e.g. Replaced) followed by the server closing was lost when the
    // client's next write failed first — the session showed "control stream write failed".
    globalThis.WebTransport = FakeWebTransport as unknown as typeof WebTransport;
    const t = await WebTransportTransport.connect('https://x', new Uint8Array(32));
    const events: string[] = [];
    t.setHandlers({
      onReliable: (_c, bytes) => events.push(`message ${String(bytes[0])}`),
      onDatagram: () => undefined,
      onClose: ({ message }) => events.push(`close ${message}`),
    });
    const server = FakeWebTransport.last;
    if (!server) throw new Error('no session');
    server.serverClosed();
    t.sendReliable(0, new Uint8Array([0x47])); // a ping, racing the close: its write fails
    await new Promise((r) => setTimeout(r, 0));
    server.deliverAndEnd(frame(new Uint8Array([0x46, 7])));
    await new Promise((r) => setTimeout(r, 50));
    expect(events[0]).toBe('message 70');
    expect(events.filter((e) => e.startsWith('close'))).toHaveLength(1);
    expect(events.at(-1)?.startsWith('close')).toBe(true);
  });
});
