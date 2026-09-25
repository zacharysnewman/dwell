// Invite links (ARCHITECTURE.md §10.1): ?join=host:port&cert=<sha-256 hex>.

export interface Invite {
  /** WebTransport URL of the server. */
  url: string;
  /** Server certificate SHA-256 (32 bytes) for `serverCertificateHashes`. */
  certHash: Uint8Array;
  host: string;
  port: number;
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
  return { url: `https://${host}:${String(port)}/dwell`, certHash, host, port };
}
