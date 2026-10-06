import { describe, expect, it } from 'vitest';
import type { DeviceKey } from '../identity/deviceKey';
import { connectToCode, HostVersionError } from './connect';
import { MasterClient, type ServerEntry } from './master';

const key: DeviceKey = {
  publicKey: new Uint8Array(32),
  sign: () => Promise.resolve(new Uint8Array(64)),
};

const server: ServerEntry = {
  code: 'ABCDEF',
  display: 'ABC-DEF',
  name: 'Castle',
  motd: '',
  players: 0,
  maxPlayers: 16,
  protocol: 10,
  host: '203.0.113.5',
  port: 4433,
  cert: 'ab'.repeat(32),
  rtcPort: null,
  ice: null,
};

/** A master that answers every resolve with `reply`, and counts the calls it got. */
function master(reply: unknown): { client: MasterClient; calls: string[] } {
  const calls: string[] = [];
  const client = new MasterClient(
    'https://m.test',
    key,
    () => 1,
    (url) => {
      calls.push(url as string);
      return Promise.resolve(new Response(JSON.stringify(reply)));
    },
  );
  return { client, calls };
}

const options = { displayName: 'x', clientVersion: '0.1.0' };

// RELEASES.md §7: any build on the host's compatibility line can join; the code of a host on
// another line leads to a build of that line, through the launcher.
describe('joining a code whose host runs another version line', () => {
  it('stops before connecting to a dedicated server, naming both versions', async () => {
    const { client, calls } = master({
      kind: 'server',
      server: { ...server, appVersion: '0.2.0' },
    });
    const err = await connectToCode(client, 'ABCDEF', options).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(HostVersionError);
    expect(err).toMatchObject({ hostVersion: '0.2.0', buildVersion: '0.1.0' });
    expect((err as Error).message).toContain('0.2.0');
    expect(calls).toEqual(['https://m.test/v1/resolve']); // no room joined, no receipt
  });

  it('stops before joining a friend world too', async () => {
    const { client, calls } = master({
      kind: 'room',
      code: 'ABCDEF',
      display: 'ABC-DEF',
      appVersion: '0.2.0-dev.3',
    });
    await expect(connectToCode(client, 'ABCDEF', options)).rejects.toBeInstanceOf(HostVersionError);
    expect(calls).toEqual(['https://m.test/v1/resolve']);
  });

  it('does not stop for a host of this line, or one that reports no version', async () => {
    // Neither reaches a real transport here: both go on to connect (and fail in this test).
    for (const appVersion of ['0.1.7', null, undefined]) {
      const { client } = master({
        kind: 'server',
        server: { ...server, ...(appVersion === undefined ? {} : { appVersion }) },
      });
      const err = await connectToCode(client, 'ABCDEF', options).catch((e: unknown) => e);
      expect(err).not.toBeInstanceOf(HostVersionError);
    }
  });
});
