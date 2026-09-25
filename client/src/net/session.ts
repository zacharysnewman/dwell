// Client side of the join handshake and connection health (ARCHITECTURE.md §8.3, ADR 0004).
import type { DeviceKey } from '../identity/deviceKey';
import { Channel, MessageType, PROTOCOL_VERSION, RejectReason } from '../protocol/constants.gen';
import { authTranscript, decode, encode, type Message } from '../protocol/messages';
import type { Transport } from './Transport';

export type SessionState =
  | { phase: 'handshaking' }
  | { phase: 'joined'; playerId: number; worldSeed: bigint }
  | { phase: 'rejected'; reason: RejectReason; message: string }
  | { phase: 'closed'; message: string };

export interface SessionStats {
  /** Smoothed round-trip time over the reliable control stream, ms. */
  rttMs: number | null;
  /** Smoothed round-trip time over datagrams, ms. */
  datagramRttMs: number | null;
  /** Latest server tick seen. */
  serverTick: number;
}

export interface SessionOptions {
  displayName: string;
  clientVersion: string;
  pingIntervalMs?: number;
  now?: () => number;
}

const RTT_SMOOTHING = 0.2;

function smooth(prev: number | null, sample: number): number {
  return prev === null ? sample : prev + (sample - prev) * RTT_SMOOTHING;
}

export class ClientSession {
  private state: SessionState = { phase: 'handshaking' };
  private readonly stats: SessionStats = { rttMs: null, datagramRttMs: null, serverTick: 0 };
  private readonly listeners = new Set<(s: SessionState, stats: SessionStats) => void>();
  private pingTimer: ReturnType<typeof setInterval> | undefined;
  private pingSeq = 0;
  private readonly now: () => number;

  constructor(
    private readonly transport: Transport,
    private readonly key: DeviceKey,
    private readonly options: SessionOptions,
  ) {
    this.now = options.now ?? (() => performance.now());
    transport.setHandlers({
      onReliable: (channel, bytes) => {
        if (channel === Channel.control) void this.onControl(bytes);
      },
      onDatagram: (bytes) => {
        this.onDatagram(bytes);
      },
      onClose: ({ message }) => {
        if (this.state.phase !== 'rejected') this.setState({ phase: 'closed', message });
        this.stopPings();
      },
    });
  }

  /** Sends ClientHello and starts pinging. */
  start(): void {
    this.send({
      type: MessageType.ClientHello,
      protocolVersion: PROTOCOL_VERSION,
      clientVersion: this.options.clientVersion,
      publicKey: this.key.publicKey,
      displayName: this.options.displayName,
    });
    this.pingTimer = setInterval(() => {
      this.ping();
    }, this.options.pingIntervalMs ?? 1000);
    this.ping();
  }

  subscribe(listener: (s: SessionState, stats: SessionStats) => void): () => void {
    this.listeners.add(listener);
    listener(this.state, this.stats);
    return () => this.listeners.delete(listener);
  }

  close(): void {
    this.stopPings();
    this.transport.close();
  }

  private send(m: Message): void {
    this.transport.sendReliable(Channel.control, encode(m));
  }

  private setState(state: SessionState): void {
    this.state = state;
    this.emit();
  }

  private emit(): void {
    for (const l of this.listeners) l(this.state, this.stats);
  }

  private stopPings(): void {
    clearInterval(this.pingTimer);
    this.pingTimer = undefined;
  }

  private ping(): void {
    const seq = ++this.pingSeq;
    const clientTimeMs = this.now();
    this.send({ type: MessageType.Ping, seq, clientTimeMs });
    if (this.state.phase === 'joined') {
      this.transport.sendDatagram(encode({ type: MessageType.DatagramPing, seq, clientTimeMs }));
    }
  }

  private async onControl(bytes: Uint8Array): Promise<void> {
    let m: Message;
    try {
      m = decode(bytes);
    } catch {
      this.setState({ phase: 'closed', message: 'Server sent a malformed message.' });
      this.close();
      return;
    }
    switch (m.type) {
      case MessageType.Challenge: {
        const transcript = authTranscript(m.nonce, this.transport.binding, this.key.publicKey);
        this.send({ type: MessageType.ClientAuth, signature: await this.key.sign(transcript) });
        break;
      }
      case MessageType.Welcome:
        this.stats.serverTick = m.serverTick;
        this.setState({ phase: 'joined', playerId: m.playerId, worldSeed: m.worldSeed });
        break;
      case MessageType.Reject:
        this.setState({ phase: 'rejected', reason: m.reason, message: m.message });
        this.stopPings();
        break;
      case MessageType.Pong:
        this.stats.rttMs = smooth(this.stats.rttMs, this.now() - m.clientTimeMs);
        this.stats.serverTick = Math.max(this.stats.serverTick, m.serverTick);
        this.emit();
        break;
      default:
        break;
    }
  }

  private onDatagram(bytes: Uint8Array): void {
    let m: Message;
    try {
      m = decode(bytes);
    } catch {
      return; // unreliable traffic: drop malformed datagrams
    }
    if (m.type === MessageType.DatagramPong) {
      this.stats.datagramRttMs = smooth(this.stats.datagramRttMs, this.now() - m.clientTimeMs);
      this.stats.serverTick = Math.max(this.stats.serverTick, m.serverTick);
      this.emit();
    }
  }
}
