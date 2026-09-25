import { Channel, MAX_RELIABLE_MESSAGE_BYTES, TransportKind } from '../protocol/constants.gen';
import type { WebRtcInvite } from './invite';
import type { Transport, TransportHandlers } from './Transport';

const CONNECT_TIMEOUT_MS = 8000;
/** Data-channel ids pre-negotiated with the server (ADR 0008). */
const DATAGRAM_CHANNEL_ID = 2;

export function isWebRtcSupported(): boolean {
  return typeof globalThis.RTCPeerConnection === 'function';
}

/**
 * Builds the ICE-lite server's answer locally from invite data, so no signaling server is needed
 * (ADR 0008). `mid` must match the offer's data m-line.
 */
export function buildServerAnswer(invite: WebRtcInvite, certHash: Uint8Array, mid: string): string {
  const fingerprint = Array.from(certHash, (b) => b.toString(16).padStart(2, '0'))
    .join(':')
    .toUpperCase();
  const ipVersion = invite.ip.includes(':') ? 'IP6' : 'IP4';
  return [
    'v=0',
    `o=- 1 1 IN ${ipVersion} ${invite.ip}`,
    's=-',
    't=0 0',
    `a=group:BUNDLE ${mid}`,
    'a=ice-lite',
    'm=application 9 UDP/DTLS/SCTP webrtc-datachannel',
    `c=IN ${ipVersion} ${invite.ip}`,
    `a=mid:${mid}`,
    `a=ice-ufrag:${invite.ufrag}`,
    `a=ice-pwd:${invite.pwd}`,
    `a=fingerprint:sha-256 ${fingerprint}`,
    'a=setup:passive',
    'a=sctp-port:5000',
    'a=max-message-size:262144',
    `a=candidate:1 1 udp 2130706431 ${invite.ip} ${String(invite.port)} typ host`,
    'a=end-of-candidates',
    '',
  ].join('\r\n');
}

/**
 * WebRTC connection to a dedicated server's ICE-lite endpoint (ARCHITECTURE.md §8.1, ADR 0008).
 * Channels: 0 = control and 1 = world (reliable, ordered), 2 = datagrams (unordered, no
 * retransmits). SCTP keeps message boundaries, so messages are sent unframed.
 */
export class WebRtcTransport implements Transport {
  readonly kind = TransportKind.WebRtc;
  private handlers: TransportHandlers | null = null;
  private closed = false;

  private constructor(
    private readonly pc: RTCPeerConnection,
    private readonly control: RTCDataChannel,
    private readonly datagrams: RTCDataChannel,
    readonly binding: Uint8Array,
  ) {}

  static async connect(invite: WebRtcInvite, certHash: Uint8Array): Promise<WebRtcTransport> {
    const pc = new RTCPeerConnection({ iceServers: [] });
    const control = pc.createDataChannel('control', { negotiated: true, id: Channel.control });
    const world = pc.createDataChannel('world', { negotiated: true, id: Channel.world });
    const datagrams = pc.createDataChannel('datagrams', {
      negotiated: true,
      id: DATAGRAM_CHANNEL_ID,
      ordered: false,
      maxRetransmits: 0,
    });
    const channels = [control, world, datagrams];
    for (const c of channels) c.binaryType = 'arraybuffer';

    try {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      const mid = /^a=mid:(\S+)/m.exec(offer.sdp ?? '')?.[1] ?? '0';
      await pc.setRemoteDescription({
        type: 'answer',
        sdp: buildServerAnswer(invite, certHash, mid),
      });
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => {
          reject(new Error('WebRTC connection timed out'));
        }, CONNECT_TIMEOUT_MS);
        const check = () => {
          if (channels.every((c) => c.readyState === 'open')) {
            clearTimeout(timer);
            resolve();
          }
        };
        for (const c of channels) c.onopen = check;
        pc.onconnectionstatechange = () => {
          if (pc.connectionState === 'failed') {
            clearTimeout(timer);
            reject(new Error('WebRTC connection failed'));
          }
        };
        check();
      });
    } catch (err) {
      pc.close();
      throw err;
    }

    const transport = new WebRtcTransport(pc, control, datagrams, certHash);
    const deliver = (channel: Channel) => (e: MessageEvent<ArrayBuffer>) => {
      if (e.data.byteLength <= MAX_RELIABLE_MESSAGE_BYTES) {
        transport.handlers?.onReliable(channel, new Uint8Array(e.data));
      }
    };
    control.onmessage = deliver(Channel.control);
    world.onmessage = deliver(Channel.world);
    datagrams.onmessage = (e: MessageEvent<ArrayBuffer>) => {
      transport.handlers?.onDatagram(new Uint8Array(e.data));
    };
    for (const c of channels) {
      c.onclose = () => {
        transport.fail('connection closed');
      };
    }
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === 'failed' || pc.connectionState === 'closed') {
        transport.fail('connection lost');
      }
    };
    return transport;
  }

  setHandlers(handlers: TransportHandlers): void {
    this.handlers = handlers;
  }

  sendReliable(channel: Channel, bytes: Uint8Array): void {
    if (this.closed) return;
    if (channel !== Channel.control) throw new Error('clients send only on the control channel');
    this.control.send(bytes.slice());
  }

  sendDatagram(bytes: Uint8Array): void {
    if (this.closed || this.datagrams.readyState !== 'open') return;
    this.datagrams.send(bytes.slice());
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.pc.close();
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
