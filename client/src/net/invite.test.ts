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

  it('rejects malformed invites', () => {
    expect(parseInvite('')).toBeNull();
    expect(parseInvite(`?join=127.0.0.1&cert=${cert}`)).toBeNull();
    expect(parseInvite(`?join=127.0.0.1:99999&cert=${cert}`)).toBeNull();
    expect(parseInvite('?join=127.0.0.1:4433&cert=abcd')).toBeNull();
    expect(parseInvite(`?join=127.0.0.1:4433&cert=${'zz'.repeat(32)}`)).toBeNull();
  });
});
