import { describe, expect, it } from 'vitest';
import { parseInvite } from './invite';

const cert = 'ab'.repeat(32);

describe('parseInvite', () => {
  it('parses host, port, and certificate hash', () => {
    const invite = parseInvite(`?join=127.0.0.1:4433&cert=${cert}`);
    expect(invite?.url).toBe('https://127.0.0.1:4433/dwell');
    expect(invite?.certHash).toHaveLength(32);
  });

  it('accepts bracketed IPv6 hosts and names', () => {
    expect(parseInvite(`?join=[::1]:4433&cert=${cert}`)?.url).toBe('https://[::1]:4433/dwell');
    expect(parseInvite(`?join=play.example.com:443&cert=${cert}`)?.port).toBe(443);
  });

  it('parses the optional WebRTC fallback for IP hosts', () => {
    const ice = 'abcd1234:ABCDEFGHIJKLMNOPQRSTUVWX';
    expect(parseInvite(`?join=127.0.0.1:4433&cert=${cert}&rtc=4434&ice=${ice}`)?.webrtc).toEqual({
      ip: '127.0.0.1',
      port: 4434,
      ufrag: 'abcd1234',
      pwd: 'ABCDEFGHIJKLMNOPQRSTUVWX',
    });
    expect(parseInvite(`?join=[::1]:4433&cert=${cert}&rtc=4434&ice=${ice}`)?.webrtc?.ip).toBe(
      '::1',
    );
    // DNS names can't be WebRTC host candidates; short passwords are invalid.
    expect(parseInvite(`?join=a.example:4433&cert=${cert}&rtc=4434&ice=${ice}`)?.webrtc).toBeNull();
    expect(
      parseInvite(`?join=127.0.0.1:4433&cert=${cert}&rtc=4434&ice=abcd:short`)?.webrtc,
    ).toBeNull();
    expect(parseInvite(`?join=127.0.0.1:4433&cert=${cert}`)?.webrtc).toBeNull();
  });

  it('rejects malformed invites', () => {
    expect(parseInvite('')).toBeNull();
    expect(parseInvite(`?join=127.0.0.1&cert=${cert}`)).toBeNull();
    expect(parseInvite(`?join=127.0.0.1:99999&cert=${cert}`)).toBeNull();
    expect(parseInvite('?join=127.0.0.1:4433&cert=abcd')).toBeNull();
    expect(parseInvite(`?join=127.0.0.1:4433&cert=${'zz'.repeat(32)}`)).toBeNull();
  });
});
