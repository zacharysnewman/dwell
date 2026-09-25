import { Channel, MAX_RELIABLE_MESSAGE_BYTES, TransportKind } from '../protocol/constants.gen';
import { DatagramSender } from './datagramSender';
import { frame, FrameReader } from './framing';
import type { Transport, TransportHandlers } from './Transport';

export function isWebTransportSupported(): boolean {
  return typeof globalThis.WebTransport === 'function';
}

/**
 * WebTransport connection to a dedicated server (ARCHITECTURE.md §8.1–8.2, ADR 0001).
 * `control` is a client-opened bidirectional stream; `world` is a server-opened unidirectional
 * stream; datagrams carry unreliable traffic.
 */
export class WebTransportTransport implements Transport {
  readonly kind = TransportKind.WebTransport;
  private handlers: TransportHandlers | null = null;
  private closed = false;
  private readonly datagrams: DatagramSender;

  private constructor(
    private readonly wt: WebTransport,
    private readonly controlWriter: WritableStreamDefaultWriter<Uint8Array>,
    private readonly controlReader: ReadableStream<Uint8Array>,
    readonly binding: Uint8Array,
  ) {
    const writer = (wt.datagrams.writable as WritableStream<Uint8Array>).getWriter();
    this.datagrams = new DatagramSender((bytes) => writer.write(bytes));
  }

  /** Connects using a pinned certificate hash (self-signed server certificate, §2.3). */
  static async connect(url: string, certHash: Uint8Array): Promise<WebTransportTransport> {
    const wt = new WebTransport(url, {
      serverCertificateHashes: [{ algorithm: 'sha-256', value: certHash.slice().buffer }],
      congestionControl: 'low-latency',
    });
    await wt.ready;
    const control = await wt.createBidirectionalStream();
    const writer = control.writable.getWriter() as WritableStreamDefaultWriter<Uint8Array>;
    await writer.write(new Uint8Array([Channel.control]));
    const transport = new WebTransportTransport(
      wt,
      writer,
      control.readable as ReadableStream<Uint8Array>,
      certHash,
    );
    transport.run();
    return transport;
  }

  setHandlers(handlers: TransportHandlers): void {
    this.handlers = handlers;
  }

  sendReliable(channel: Channel, bytes: Uint8Array): void {
    if (this.closed) return;
    if (channel !== Channel.control) throw new Error('clients send only on the control channel');
    this.controlWriter.write(frame(bytes)).catch(() => {
      this.fail('control stream write failed');
    });
  }

  sendDatagram(bytes: Uint8Array): void {
    if (this.closed) return;
    this.datagrams.send(bytes);
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.wt.close();
  }

  private run(): void {
    void this.readStream(this.controlReader, Channel.control);
    void this.acceptWorldStreams();
    void this.readDatagrams();
    this.wt.closed
      .then(
        (info) => {
          this.fail(info.reason || 'connection closed');
        },
        (err: unknown) => {
          this.fail(err instanceof Error ? err.message : 'connection lost');
        },
      )
      .catch(() => undefined);
  }

  private fail(message: string): void {
    const wasClosed = this.closed;
    this.closed = true;
    if (!wasClosed) this.wt.close();
    this.handlers?.onClose({ message });
    this.handlers = null;
  }

  private async readStream(stream: ReadableStream<Uint8Array>, channel: Channel): Promise<void> {
    const reader = stream.getReader();
    const frames = new FrameReader(MAX_RELIABLE_MESSAGE_BYTES);
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        for (const f of frames.push(value)) this.handlers?.onReliable(channel, f);
      }
    } catch {
      // Stream errors surface through `wt.closed`.
    }
  }

  private async acceptWorldStreams(): Promise<void> {
    const streams = (
      this.wt.incomingUnidirectionalStreams as ReadableStream<ReadableStream<Uint8Array>>
    ).getReader();
    try {
      for (;;) {
        const { value, done } = await streams.read();
        if (done) break;
        void this.readTaggedStream(value);
      }
    } catch {
      // Closed.
    }
  }

  /** Server-opened streams start with their channel id. */
  private async readTaggedStream(stream: ReadableStream<Uint8Array>): Promise<void> {
    const reader = stream.getReader();
    const first = await reader.read().catch(() => ({ value: undefined, done: true }));
    if (first.done || !first.value || first.value.length === 0) return;
    const channel = first.value[0];
    if (channel !== Channel.world) {
      this.fail('unexpected server stream');
      return;
    }
    const frames = new FrameReader(MAX_RELIABLE_MESSAGE_BYTES);
    try {
      for (const f of frames.push(first.value.subarray(1))) this.handlers?.onReliable(channel, f);
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        for (const f of frames.push(value)) this.handlers?.onReliable(channel, f);
      }
    } catch {
      // Closed.
    }
  }

  private async readDatagrams(): Promise<void> {
    const reader = (this.wt.datagrams.readable as ReadableStream<Uint8Array>).getReader();
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        this.handlers?.onDatagram(value);
      }
    } catch {
      // Closed.
    }
  }
}
