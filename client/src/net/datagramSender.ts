// Unreliable datagrams through a WritableStream (WebTransport). A stream hands its sink one chunk
// at a time, and each hand-off needs a turn of the main thread, so when frames run long (slow
// devices, several ticks per frame) queued writes drain at about one per frame: the queue, and
// with it the latency of every input, grows without bound. Datagrams are unreliable and newer
// ones supersede older ones of the same kind (each PlayerInput repeats the latest inputs), so
// only one write is in flight and, per message type, only the newest datagram waits behind it.

export class DatagramSender {
  private inFlight = false;
  /** Newest waiting datagram per message type (first byte), in first-queued order. */
  private readonly waiting = new Map<number, Uint8Array>();

  constructor(private readonly write: (bytes: Uint8Array) => Promise<void>) {}

  send(bytes: Uint8Array): void {
    if (this.inFlight) {
      this.waiting.set(bytes[0] ?? 0, bytes);
      return;
    }
    this.start(bytes);
  }

  private start(bytes: Uint8Array): void {
    this.inFlight = true;
    const next = () => {
      this.inFlight = false;
      const [first] = this.waiting;
      if (!first) return;
      this.waiting.delete(first[0]);
      this.start(first[1]);
    };
    this.write(bytes).then(next, next);
  }
}
