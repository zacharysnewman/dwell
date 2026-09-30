import { describe, expect, it } from 'vitest';
import type { ListedServer, ListedWorld } from '../net/master';
import { formatPing, incompatibility, lobbyDetails } from './serverBrowser';

const server: ListedServer = {
  code: 'ABCDEF',
  display: 'ABC-DEF',
  name: 'Castle',
  motd: 'Hi',
  players: 2,
  maxPlayers: 16,
  protocol: 10,
  host: '203.0.113.5',
  port: 4433,
  cert: 'ab'.repeat(32),
  rtcPort: null,
  ice: null,
  tags: ['pve', 'creative'],
  verified: true,
};
const world: ListedWorld = {
  code: 'GHJKMN',
  display: 'GHJ-KMN',
  name: 'Bravo',
  players: 1,
  maxPlayers: 5,
  protocol: 10,
};

describe('server browser text', () => {
  it('marks other protocol versions with the handshake reason', () => {
    expect(incompatibility(10, 10)).toBeNull();
    expect(incompatibility(null, 10)).toBeNull();
    expect(incompatibility(9, 10)).toBe('Server runs protocol 9, client runs 10.');
  });

  it('describes servers and friend worlds', () => {
    expect(lobbyDetails(server)).toBe('2/16 players · pve, creative');
    expect(lobbyDetails({ ...server, tags: [] })).toBe('2/16 players');
    expect(lobbyDetails(world)).toBe('friend world · 1/5 players');
  });

  it('formats pings', () => {
    expect(formatPing(null)).toBe('—');
    expect(formatPing(37.6)).toBe('38 ms');
    expect(formatPing(0.2)).toBe('1 ms');
  });
});
