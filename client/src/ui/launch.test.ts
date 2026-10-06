import { describe, expect, it } from 'vitest';
import {
  codeLink,
  launcherHref,
  launchOf,
  looksLikeAddress,
  pastedCode,
  pastedInvite,
  withRoute,
} from './launch';

const CERT = 'ab'.repeat(32);
const ICE = `ufrag:${'p'.repeat(22)}`;

describe('what the page opens', () => {
  it('opens the main menu with no world or server in the address', () => {
    expect(launchOf('')).toEqual({ kind: 'menu' });
    expect(launchOf('?debug=1&lodcolors=1')).toEqual({ kind: 'menu' });
  });

  it('opens invites, menu worlds and world links', () => {
    expect(launchOf(`?join=1.2.3.4:4433&cert=${CERT}`)).toEqual({ kind: 'join' });
    expect(launchOf('?play=wabc')).toEqual({ kind: 'play', id: 'wabc' });
    expect(launchOf('?world=flat')).toEqual({ kind: 'link' });
    expect(launchOf('?seed=7')).toEqual({ kind: 'link' });
    // ?local=1 wins over an invite, as before the menu existed.
    expect(launchOf(`?local=1&join=1.2.3.4:4433&cert=${CERT}`)).toEqual({ kind: 'link' });
  });

  it('keeps debug parameters when changing route', () => {
    expect(withRoute('?debug=1&world=flat&seed=3', { play: 'wabc' })).toBe('?debug=1&play=wabc');
    expect(withRoute('?play=wabc', {})).toBe('');
    expect(withRoute(`?join=h:1&cert=${CERT}&netsim=150,20,5`, {})).toBe('?netsim=150%2C20%2C5');
  });
});

describe('pasted invites', () => {
  it('reads a whole link or just its query', () => {
    const expected = { join: '1.2.3.4:4433', cert: CERT };
    expect(
      pastedInvite(`https://dropkickarcade.com/dwell/?join=1.2.3.4:4433&cert=${CERT}`),
    ).toEqual(expected);
    expect(pastedInvite(`  ?join=1.2.3.4:4433&cert=${CERT}#x `)).toEqual(expected);
    expect(pastedInvite(`join=1.2.3.4:4433&cert=${CERT}`)).toEqual(expected);
  });

  it('keeps the WebRTC part and drops other parameters', () => {
    expect(pastedInvite(`?join=1.2.3.4:4433&cert=${CERT}&rtc=4434&ice=${ICE}&debug=1`)).toEqual({
      join: '1.2.3.4:4433',
      cert: CERT,
      rtc: '4434',
      ice: ICE,
    });
  });

  it('rejects anything that is not a valid invite', () => {
    expect(pastedInvite('')).toBeNull();
    expect(pastedInvite('KQ7-XM4')).toBeNull();
    expect(pastedInvite('192.168.1.50')).toBeNull();
    expect(pastedInvite(`?join=1.2.3.4:4433&cert=${CERT.slice(2)}`)).toBeNull();
  });
});

describe('join codes', () => {
  it('open a friend world from ?code=, and the menu for a malformed one', () => {
    expect(launchOf('?code=kq7-xm4')).toEqual({ kind: 'code', code: 'KQ7XM4' });
    expect(launchOf('?code=nope')).toEqual({ kind: 'menu' });
  });

  it('are read from a typed code or a pasted link', () => {
    expect(pastedCode(' kq7 xm4 ')).toBe('KQ7XM4');
    expect(pastedCode('https://dropkickarcade.com/dwell/?code=KQ7-XM4')).toBe('KQ7XM4');
    expect(pastedCode('https://dropkickarcade.com/dwell/?join=a')).toBeNull();
    expect(pastedCode('hello')).toBeNull();
  });

  it('make an invite link on this page, keeping its master and debug parameters', () => {
    expect(
      codeLink('http://localhost:5173/dwell/?play=wabc&master=http://localhost:8787#x', 'KQ7XM4'),
    ).toBe('http://localhost:5173/dwell/?master=http%3A%2F%2Flocalhost%3A8787&code=KQ7XM4');
  });
});

describe('server addresses in the Join box (Phase 5d)', () => {
  it('are told apart from codes and other text', () => {
    for (const a of [
      '192.168.1.50',
      '192.168.1.50:4433',
      'play.example.com',
      'localhost:4433',
      '[::1]:4433',
    ]) {
      expect(looksLikeAddress(a), a).toBe(true);
    }
    for (const a of ['KQ7XM4', 'hello world', 'localhostx', 'a b.c', 'host:99999999']) {
      expect(looksLikeAddress(a), a).toBe(false);
    }
  });
});

describe('the host version in an invite', () => {
  it('is kept from a pasted invite, for the launcher to open a build on the host line', () => {
    expect(pastedInvite(`?join=1.2.3.4:4433&cert=${CERT}&v=0.1.0&debug=1`)).toEqual({
      join: '1.2.3.4:4433',
      cert: CERT,
      v: '0.1.0',
    });
  });

  it('is dropped when the route changes', () => {
    expect(withRoute(`?join=h:1&cert=${CERT}&v=0.1.0&debug=1`, { play: 'wabc' })).toBe(
      '?debug=1&play=wabc',
    );
  });
});

describe('where routes open', () => {
  // From a versioned page (/dwell/v/0.1.0/) too: the launcher picks the build (RELEASES.md §5), and
  // Back to the menu is the launcher with no route.
  it('goes through the launcher at /dwell/, keeping debug parameters', () => {
    expect(launcherHref('?play=wabc&debug=1', {})).toBe('/dwell/?debug=1');
    expect(launcherHref('', {})).toBe('/dwell/');
    expect(launcherHref('?debug=1', { play: 'wabc' })).toBe('/dwell/?debug=1&play=wabc');
    expect(launcherHref(`?join=h:1&cert=x&v=0.1.0`, { code: 'KQ7XM4' })).toBe(
      '/dwell/?code=KQ7XM4',
    );
  });
});
