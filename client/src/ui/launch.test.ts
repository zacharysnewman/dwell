import { describe, expect, it } from 'vitest';
import { launchOf, pastedInvite, withRoute } from './launch';

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
