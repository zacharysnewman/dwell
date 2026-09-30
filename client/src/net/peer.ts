// WebRTC between browsers for friend worlds (ARCHITECTURE.md §10.2, Phase 5c). A guest's
// PeerTransport and the host's HostPeer (one per guest) negotiate through the master's Room
// (net/roomSocket.ts): SDP offer and answer, then trickled ICE candidates. The data channels are
// the dedicated servers' (§8.1): 0 control and 1 world (reliable, ordered), 2 datagrams (unordered,
// no retransmits), 3 lod (reliable, ordered), pre-negotiated by id.
//
// The transport binding the guest signs in its ClientAuth (ADR 0004) is the SHA-256 of the host's
// DTLS certificate, which the guest reads from the answer's fingerprint and the host from its own
// certificate, so a signed join can't be replayed to another host.
import { Channel, MAX_RELIABLE_MESSAGE_BYTES, TransportKind } from '../protocol/constants.gen';
import type { Transport, TransportHandlers } from './Transport';

const DATAGRAM_CHANNEL_ID = 2;
const LOD_CHANNEL_ID = 3;
export const PEER_CONNECT_TIMEOUT_MS = 20_000;

/** What the two sides exchange through the Room. */
export type SignalData =
  | { type: 'offer' | 'answer'; sdp: string }
  | { type: 'candidate'; candidate: RTCIceCandidateInit | null };

export function isSignalData(v: unknown): v is SignalData {
  if (typeof v !== 'object' || v === null) return false;
  const d = v as Record<string, unknown>;
  if (d.type === 'offer' || d.type === 'answer') return typeof d.sdp === 'string';
  return d.type === 'candidate' && (d.candidate === null || typeof d.candidate === 'object');
}

/** The SHA-256 DTLS fingerprint in an SDP (32 bytes), or null. */
export function fingerprintSha256(sdp: string): Uint8Array | null {
  const m = /^a=fingerprint:sha-256 ([0-9A-Fa-f:]+)\s*$/m.exec(sdp);
  return m?.[1] ? parseFingerprint(m[1]) : null;
}

/** "AB:CD:…" (32 bytes) → bytes, or null. */
export function parseFingerprint(text: string): Uint8Array | null {
  const parts = text.split(':');
  if (parts.length !== 32 || !parts.every((p) => /^[0-9A-Fa-f]{2}$/.test(p))) return null;
  return Uint8Array.from(parts, (p) => parseInt(p, 16));
}

/** The SHA-256 of a certificate the host made for this hosting session, or null. */
export function certificateSha256(cert: RTCCertificate): Uint8Array | null {
  const fp = cert.getFingerprints().find((f) => f.algorithm?.toLowerCase() === 'sha-256');
  return fp?.value ? parseFingerprint(fp.value) : null;
}

interface Channels {
  control: RTCDataChannel;
  world: RTCDataChannel;
  datagrams: RTCDataChannel;
  lod: RTCDataChannel;
}

function createChannels(pc: RTCPeerConnection): Channels {
  const channels = {
    control: pc.createDataChannel('control', { negotiated: true, id: Channel.control }),
    world: pc.createDataChannel('world', { negotiated: true, id: Channel.world }),
    datagrams: pc.createDataChannel('datagrams', {
      negotiated: true,
      id: DATAGRAM_CHANNEL_ID,
      ordered: false,
      maxRetransmits: 0,
    }),
    lod: pc.createDataChannel('lod', { negotiated: true, id: LOD_CHANNEL_ID }),
  };
  for (const c of channelList(channels)) c.binaryType = 'arraybuffer';
  return channels;
}

function channelList(c: Channels): RTCDataChannel[] {
  return [c.control, c.world, c.datagrams, c.lod];
}

function allOpen(channels: Channels): Promise<void> {
  const list = channelList(channels);
  return new Promise((resolve) => {
    const check = () => {
      if (list.every((c) => c.readyState === 'open')) resolve();
    };
    for (const c of list) c.addEventListener('open', check);
    check();
  });
}

/**
 * Applies the other side's signals: the description first, candidates as they come (queued until
 * the description is set).
 */
class Negotiation {
  private readonly pending: RTCIceCandidateInit[] = [];
  private described = false;

  constructor(private readonly pc: RTCPeerConnection) {}

  async describe(description: RTCSessionDescriptionInit): Promise<void> {
    await this.pc.setRemoteDescription(description);
    this.described = true;
    for (const c of this.pending.splice(0)) await this.pc.addIceCandidate(c).catch(() => undefined);
  }

  async candidate(candidate: RTCIceCandidateInit | null): Promise<void> {
    if (!candidate) return;
    if (this.described) await this.pc.addIceCandidate(candidate).catch(() => undefined);
    else this.pending.push(candidate);
  }
}

/** Sends one side's signals, and receives the other's. */
export interface Signaler {
  send(data: SignalData): void;
  onSignal: ((data: SignalData) => void) | null;
}

/**
 * A guest's connection to a browser-hosted friend world. Clients send only on control and
 * datagrams, like WebRtcTransport (§8.1).
 */
export class PeerTransport implements Transport {
  readonly kind = TransportKind.WebRtc;
  private handlers: TransportHandlers | null = null;
  private closed = false;

  private constructor(
    private readonly pc: RTCPeerConnection,
    private readonly channels: Channels,
    readonly binding: Uint8Array,
  ) {}

  static async connect(
    signaler: Signaler,
    iceServers: RTCIceServer[],
    timeoutMs = PEER_CONNECT_TIMEOUT_MS,
  ): Promise<PeerTransport> {
    const pc = new RTCPeerConnection({ iceServers });
    const channels = createChannels(pc);
    const negotiation = new Negotiation(pc);
    const answered: { binding: Uint8Array | null } = { binding: null };
    let failed: ((err: Error) => void) | null = null;
    const failure = new Promise<never>((_, reject) => {
      failed = reject;
    });
    pc.onicecandidate = (e) => {
      signaler.send({ type: 'candidate', candidate: e.candidate?.toJSON() ?? null });
    };
    signaler.onSignal = (data) => {
      if (data.type === 'answer') {
        answered.binding = fingerprintSha256(data.sdp);
        if (!answered.binding) failed?.(new Error('The host sent no certificate fingerprint.'));
        void negotiation.describe({ type: 'answer', sdp: data.sdp }).catch((err: unknown) => {
          failed?.(err instanceof Error ? err : new Error(String(err)));
        });
      } else if (data.type === 'candidate') {
        void negotiation.candidate(data.candidate);
      }
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === 'failed') failed?.(new Error('Could not reach the host.'));
    };
    let timer = 0;
    try {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      signaler.send({ type: 'offer', sdp: offer.sdp ?? '' });
      const timeout = new Promise<never>((_, reject) => {
        timer = window.setTimeout(() => {
          reject(new Error('Connecting to the host timed out.'));
        }, timeoutMs);
      });
      await Promise.race([allOpen(channels), failure, timeout]);
    } catch (err) {
      pc.close();
      throw err;
    } finally {
      window.clearTimeout(timer);
    }
    if (!answered.binding) {
      pc.close();
      throw new Error('The host sent no certificate fingerprint.');
    }
    const transport = new PeerTransport(pc, channels, answered.binding);
    transport.listen();
    return transport;
  }

  private listen(): void {
    const deliver = (channel: Channel) => (e: MessageEvent<ArrayBuffer>) => {
      if (e.data.byteLength <= MAX_RELIABLE_MESSAGE_BYTES) {
        this.handlers?.onReliable(channel, new Uint8Array(e.data));
      }
    };
    this.channels.control.onmessage = deliver(Channel.control);
    this.channels.world.onmessage = deliver(Channel.world);
    this.channels.lod.onmessage = deliver(Channel.lod);
    this.channels.datagrams.onmessage = (e: MessageEvent<ArrayBuffer>) => {
      this.handlers?.onDatagram(new Uint8Array(e.data));
    };
    for (const c of channelList(this.channels)) {
      c.onclose = () => {
        this.fail('the host closed the connection');
      };
    }
    this.pc.onconnectionstatechange = () => {
      const state = this.pc.connectionState;
      if (state === 'failed' || state === 'closed') this.fail('connection to the host lost');
    };
  }

  setHandlers(handlers: TransportHandlers): void {
    this.handlers = handlers;
  }

  sendReliable(channel: Channel, bytes: Uint8Array): void {
    if (this.closed) return;
    if (channel !== Channel.control) throw new Error('clients send only on the control channel');
    this.channels.control.send(bytes.slice());
  }

  sendDatagram(bytes: Uint8Array): void {
    if (this.closed || this.channels.datagrams.readyState !== 'open') return;
    this.channels.datagrams.send(bytes.slice());
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.pc.close();
  }

  /** Ends the connection as if it were lost (e.g. the room said the host left). */
  end(message: string): void {
    this.fail(message);
  }

  private fail(message: string): void {
    if (!this.closed) {
      this.closed = true;
      this.pc.close();
    }
    this.handlers?.onClose({ message });
    this.handlers = null;
  }
}

export interface HostPeerHandlers {
  /** All channels are open: the guest's session starts. */
  onOpen(): void;
  onReliable(channel: Channel, bytes: Uint8Array): void;
  onDatagram(bytes: Uint8Array): void;
  /** The connection ended (after onOpen, or instead of it). */
  onClose(): void;
}

/** The host's side of one guest's connection. */
export class HostPeer {
  private readonly pc: RTCPeerConnection;
  private readonly channels: Channels;
  private readonly negotiation: Negotiation;
  private open = false;
  private ended = false;
  /** Control messages that arrived before every channel was open. */
  private readonly early: Uint8Array[] = [];

  constructor(
    private readonly signal: (data: SignalData) => void,
    iceServers: RTCIceServer[],
    certificate: RTCCertificate,
    private readonly handlers: HostPeerHandlers,
  ) {
    this.pc = new RTCPeerConnection({ iceServers, certificates: [certificate] });
    this.channels = createChannels(this.pc);
    this.negotiation = new Negotiation(this.pc);
    this.pc.onicecandidate = (e) => {
      this.signal({ type: 'candidate', candidate: e.candidate?.toJSON() ?? null });
    };
    this.pc.onconnectionstatechange = () => {
      const state = this.pc.connectionState;
      if (state === 'failed' || state === 'closed') this.end();
    };
    // Guests send only on control and datagrams; anything on world or lod is ignored. The guest
    // may send its ClientHello before this side has seen every channel open: it waits for onOpen.
    this.channels.control.onmessage = (e: MessageEvent<ArrayBuffer>) => {
      if (e.data.byteLength > MAX_RELIABLE_MESSAGE_BYTES) return;
      const bytes = new Uint8Array(e.data);
      if (this.open) this.handlers.onReliable(Channel.control, bytes);
      else this.early.push(bytes);
    };
    this.channels.datagrams.onmessage = (e: MessageEvent<ArrayBuffer>) => {
      if (this.open) this.handlers.onDatagram(new Uint8Array(e.data));
    };
    for (const c of channelList(this.channels)) {
      c.onclose = () => {
        this.end();
      };
    }
    void allOpen(this.channels).then(() => {
      if (this.ended) return;
      this.open = true;
      this.handlers.onOpen();
      for (const bytes of this.early.splice(0)) {
        if (this.connected) this.handlers.onReliable(Channel.control, bytes);
      }
    });
  }

  /** The guest's offer or candidates. */
  async onSignal(data: SignalData): Promise<void> {
    if (data.type === 'offer') {
      await this.negotiation.describe({ type: 'offer', sdp: data.sdp });
      const answer = await this.pc.createAnswer();
      await this.pc.setLocalDescription(answer);
      this.signal({ type: 'answer', sdp: answer.sdp ?? '' });
    } else if (data.type === 'candidate') {
      await this.negotiation.candidate(data.candidate);
    }
  }

  get connected(): boolean {
    return this.open && !this.ended;
  }

  send(channel: Channel, bytes: Uint8Array): void {
    if (!this.connected) return;
    const c =
      channel === Channel.world
        ? this.channels.world
        : channel === Channel.lod
          ? this.channels.lod
          : this.channels.control;
    if (c.readyState === 'open') c.send(bytes.slice());
  }

  sendDatagram(bytes: Uint8Array): void {
    if (this.connected && this.channels.datagrams.readyState === 'open') {
      this.channels.datagrams.send(bytes.slice());
    }
  }

  close(): void {
    this.end();
  }

  private end(): void {
    if (this.ended) return;
    this.ended = true;
    this.pc.close();
    this.handlers.onClose();
  }
}
