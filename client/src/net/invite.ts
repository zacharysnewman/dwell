// Invite links (ARCHITECTURE.md §10.1):
//   ?join=host:port&cert=<sha-256 hex>[&rtc=<port>&ice=<ufrag>:<pwd>]
// The optional rtc/ice part enables the WebRTC fallback (ADR 0008).

export interface WebRtcInvite {
  /** Server IP literal (WebRTC host candidates can't carry DNS names). */
  ip: string;
  port: number;
  ufrag: string;
  pwd: string;
}

export interface Invite {
  /** WebTransport URL of the server. */
  url: string;
  /** Server certificate SHA-256 (32 bytes) for `serverCertificateHashes`. */
  certHash: Uint8Array;
  host: string;
  port: number;
  /** Present when the server accepts WebRTC and the host is an IP literal. */
  webrtc: WebRtcInvite | null;
}

const ICE_CHARS = /^[A-Za-z0-9+/]+$/;

function isIpLiteral(host: string): boolean {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || /^\[[0-9a-f:]+\]$/i.test(host);
}

function parseWebRtc(params: URLSearchParams, host: string): WebRtcInvite | null {
  const rtc = params.get('rtc');
  const ice = params.get('ice');
  if (!rtc || !ice || !isIpLiteral(host)) return null;
  const port = Number(rtc);
  const [ufrag = '', pwd = ''] = ice.split(':');
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  if (ufrag.length < 4 || pwd.length < 22 || !ICE_CHARS.test(ufrag) || !ICE_CHARS.test(pwd)) {
    return null;
  }
  return { ip: host.replace(/^\[|\]$/g, ''), port, ufrag, pwd };
}

export function parseHex(hex: string): Uint8Array | null {
  if (!/^(?:[0-9a-f]{2})*$/i.test(hex)) return null;
  return Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16));
}

/** Parses the invite from a page's query string; null if absent or invalid. */
export function parseInvite(search: string): Invite | null {
  const params = new URLSearchParams(search);
  const join = params.get('join');
  const cert = params.get('cert');
  if (!join || !cert) return null;
  const m = /^(\[[0-9a-f:]+\]|[a-z0-9.-]+):(\d{1,5})$/i.exec(join);
  if (!m?.[1] || !m[2]) return null;
  const port = Number(m[2]);
  const certHash = parseHex(cert);
  if (port < 1 || port > 65535 || certHash?.length !== 32) return null;
  const host = m[1];
  return {
    url: `https://${host}:${String(port)}/dwell`,
    certHash,
    host,
    port,
    webrtc: parseWebRtc(params, host),
  };
}
