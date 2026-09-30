// Client side of the join handshake and connection health (ARCHITECTURE.md §8.3, ADR 0004).
import type { DeviceKey } from '../identity/deviceKey';
import {
  Channel,
  MessageType,
  PROTOCOL_VERSION,
  type RejectReason,
  type TransportKind,
  WelcomeFlags,
  HostState,
} from '../protocol/constants.gen';
import {
  authTranscript,
  decode,
  encode,
  type ChunkCoord,
  type Message,
} from '../protocol/messages';
import type { Transport } from './Transport';

export type SessionState =
  | { phase: 'handshaking' }
  | {
      phase: 'joined';
      playerId: number;
      worldSeed: bigint;
      generatorVersion: number;
      verificationChunk: ChunkCoord;
      /** The server lets this player use creative flight (§8.3). */
      mayFly: boolean;
    }
  | { phase: 'rejected'; reason: RejectReason; message: string }
  | { phase: 'closed'; message: string };

export interface SessionStats {
  /** Smoothed round-trip time over the reliable control stream, ms. */
  rttMs: number | null;
  /** Smoothed round-trip time over datagrams, ms. */
  datagramRttMs: number | null;
  /** Latest server tick seen. */
  serverTick: number;
  /** A friend world's host has paused it (their page is hidden, HostStatus, §10.2). */
  hostPaused: boolean;
}

/** Level-of-detail messages (the `lod` channel, §6.6). */
export type LodMessage = Extract<
  Message,
  | { type: typeof MessageType.LodIndex }
  | { type: typeof MessageType.LodIndexUpdate }
  | { type: typeof MessageType.LodData }
>;

/**
 * Gameplay messages from the server (datagram snapshots, world- and lod-channel messages), with raw
 * bytes.
 */
export type GameMessage =
  | Extract<
      Message,
      | { type: typeof MessageType.PhysicsSnapshot }
      | { type: typeof MessageType.PlayerEvent }
      | { type: typeof MessageType.ChunkData }
      | { type: typeof MessageType.ChunkUnload }
      | { type: typeof MessageType.VoxelModification }
    >
  | LodMessage;
export type GameListener = (message: GameMessage, bytes: Uint8Array) => void;

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
  private readonly stats: SessionStats = {
    rttMs: null,
    datagramRttMs: null,
    serverTick: 0,
    hostPaused: false,
  };
  private readonly listeners = new Set<(s: SessionState, stats: SessionStats) => void>();
  private readonly gameListeners = new Set<GameListener>();
  /** World-channel messages that arrived before any game listener (the game is still loading). */
  private readonly heldWorld: [GameMessage, Uint8Array][] = [];
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
        else this.onWorld(bytes);
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

  get transportKind(): TransportKind {
    return this.transport.kind;
  }

  subscribe(listener: (s: SessionState, stats: SessionStats) => void): () => void {
    this.listeners.add(listener);
    listener(this.state, this.stats);
    return () => this.listeners.delete(listener);
  }

  /** Gameplay messages (snapshots, player events) while joined. */
  onGame(listener: GameListener): () => void {
    this.gameListeners.add(listener);
    for (const [m, bytes] of this.heldWorld.splice(0)) listener(m, bytes);
    return () => this.gameListeners.delete(listener);
  }

  /** Sends a reliable control message while joined (WorldgenCheck, edits, resyncs, LodRequest). */
  sendControl(m: Message): void {
    if (this.state.phase === 'joined') this.send(m);
  }

  /** Sends a gameplay datagram (player input); dropped unless joined. */
  sendGameDatagram(m: Message): void {
    if (this.state.phase === 'joined') this.transport.sendDatagram(encode(m));
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
        this.setState({
          phase: 'joined',
          playerId: m.playerId,
          worldSeed: m.worldSeed,
          generatorVersion: m.generatorVersion,
          verificationChunk: m.verificationChunk,
          mayFly: (m.flags & WelcomeFlags.flight) !== 0,
        });
        break;
      case MessageType.Reject:
        this.setState({ phase: 'rejected', reason: m.reason, message: m.message });
        this.stopPings();
        break;
      case MessageType.HostStatus:
        this.stats.hostPaused = m.state === HostState.Paused;
        this.emit();
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

  private onWorld(bytes: Uint8Array): void {
    let m: Message;
    try {
      m = decode(bytes);
    } catch {
      this.setState({ phase: 'closed', message: 'Server sent a malformed message.' });
      this.close();
      return;
    }
    if (
      m.type === MessageType.PlayerEvent ||
      m.type === MessageType.ChunkData ||
      m.type === MessageType.ChunkUnload ||
      m.type === MessageType.VoxelModification ||
      m.type === MessageType.LodIndex ||
      m.type === MessageType.LodIndexUpdate ||
      m.type === MessageType.LodData
    ) {
      // Reliable world messages must not be lost while the game loads: hold them until then.
      if (this.state.phase === 'joined' && this.gameListeners.size === 0) {
        this.heldWorld.push([m, bytes]);
        return;
      }
      this.emitGame(m, bytes);
    }
  }

  private emitGame(m: GameMessage, bytes: Uint8Array): void {
    if (this.state.phase !== 'joined') return;
    for (const l of this.gameListeners) l(m, bytes);
  }

  private onDatagram(bytes: Uint8Array): void {
    let m: Message;
    try {
      m = decode(bytes);
    } catch {
      return; // unreliable traffic: drop malformed datagrams
    }
    if (m.type === MessageType.PhysicsSnapshot) {
      this.stats.serverTick = Math.max(this.stats.serverTick, m.serverTick);
      this.emitGame(m, bytes);
      return;
    }
    if (m.type === MessageType.DatagramPong) {
      this.stats.datagramRttMs = smooth(this.stats.datagramRttMs, this.now() - m.clientTimeMs);
      this.stats.serverTick = Math.max(this.stats.serverTick, m.serverTick);
      this.emit();
    }
  }
}
