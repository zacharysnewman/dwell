import { describe, expect, it } from 'vitest';
import { buildServerAnswer } from './webRtc';

describe('buildServerAnswer', () => {
  it('describes the ICE-lite server with its fingerprint and host candidate', () => {
    const sdp = buildServerAnswer(
      { ip: '127.0.0.1', port: 4434, ufrag: 'abcd', pwd: 'p'.repeat(22) },
      Uint8Array.from({ length: 32 }, (_, i) => i),
      '0',
    );
    expect(sdp).toContain('a=ice-lite');
    expect(sdp).toContain('a=setup:passive');
    expect(sdp).toContain('a=fingerprint:sha-256 00:01:02:03:');
    expect(sdp).toContain('a=candidate:1 1 udp 2130706431 127.0.0.1 4434 typ host');
    expect(sdp.endsWith('\r\n')).toBe(true);
  });

  it('uses IP6 for IPv6 servers', () => {
    const sdp = buildServerAnswer(
      { ip: '::1', port: 4434, ufrag: 'abcd', pwd: 'p'.repeat(22) },
      new Uint8Array(32),
      'data',
    );
    expect(sdp).toContain('c=IN IP6 ::1');
    expect(sdp).toContain('a=mid:data');
  });
});
