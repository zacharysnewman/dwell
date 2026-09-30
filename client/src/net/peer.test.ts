import { describe, expect, it } from 'vitest';
import { fingerprintSha256, parseFingerprint } from './peer';

const FP = Array.from({ length: 32 }, (_, i) => i.toString(16).padStart(2, '0').toUpperCase()).join(
  ':',
);

describe('certificate fingerprints', () => {
  it('are read from an answer’s SDP as the transport binding', () => {
    const sdp = `v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\na=fingerprint:sha-256 ${FP}\r\na=setup:active\r\n`;
    expect(fingerprintSha256(sdp)).toEqual(Uint8Array.from({ length: 32 }, (_, i) => i));
  });

  it('must be SHA-256 and whole', () => {
    expect(fingerprintSha256(`a=fingerprint:sha-1 ${FP.slice(0, 59)}\r\n`)).toBeNull();
    expect(fingerprintSha256('v=0\r\n')).toBeNull();
    expect(parseFingerprint(FP.slice(3))).toBeNull();
    expect(parseFingerprint(FP.replace('00', 'ZZ'))).toBeNull();
  });
});
