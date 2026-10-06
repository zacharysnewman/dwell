import { describe, expect, it } from 'vitest';
import { isBuildOf, LAUNCHER_VERSION, parseManifest, type Manifest } from './manifest';
import { choose, forwardedSearch, latestBuild, type Channel } from './select';

function manifest(versions: string[], minLauncher: Record<string, number> = {}): Manifest {
  const entries = versions.map((version) => ({
    version,
    channel: version.includes('-') ? ('dev' as const) : ('stable' as const),
    date: '2026-10-06T00:00:00.000Z',
    commit: 'abc1234',
    protocolVersion: 10,
    minLauncher: minLauncher[version] ?? 1,
  }));
  return {
    schema: 1,
    generated: '2026-10-06T00:00:00.000Z',
    versions: entries,
    latestStable: entries.find((e) => e.channel === 'stable')?.version ?? null,
    latestDev: entries.find((e) => e.channel === 'dev')?.version ?? null,
  };
}

function request(
  search: string,
  worlds: Record<string, string> = {},
  channel: Channel = 'stable',
): Parameters<typeof choose>[1] {
  return { search, worldVersion: (id) => worlds[id] ?? null, channel };
}

// Newest first, as the manifest lists them.
const SITE = manifest(['0.2.0', '0.1.1', '0.1.0', '0.2.0-dev.7']);

describe('the launcher', () => {
  it('opens the latest stable build for the main menu', () => {
    expect(choose(SITE, request(''))).toEqual({ kind: 'open', version: '0.2.0', why: 'latest' });
    expect(choose(SITE, request('?debug=1'))).toMatchObject({ version: '0.2.0' });
  });

  it('opens the latest dev build when the player chose the dev channel', () => {
    expect(choose(SITE, request('', {}, 'dev'))).toMatchObject({ version: '0.2.0-dev.7' });
    // With no dev build published, the stable one.
    expect(choose(manifest(['0.1.0']), request('', {}, 'dev'))).toMatchObject({ version: '0.1.0' });
    expect(latestBuild(manifest([]), 'stable')).toBeNull();
  });

  it('opens a world in the newest build of its compatibility line, never an older one', () => {
    const worlds = { a: '0.1.0', b: '0.1.1', c: '0.2.0' };
    expect(choose(SITE, request('?play=a', worlds))).toEqual({
      kind: 'open',
      version: '0.1.1',
      why: 'world',
    });
    expect(choose(SITE, request('?play=b', worlds))).toMatchObject({ version: '0.1.1' });
    expect(choose(SITE, request('?play=c', worlds))).toMatchObject({ version: '0.2.0' });
    // A world saved by 0.1.2, which is not published yet, has no build to open it.
    expect(choose(SITE, request('?play=d', { d: '0.1.2' }))).toMatchObject({ kind: 'error' });
  });

  it("opens a dev build's world only in that build", () => {
    expect(choose(SITE, request('?play=a', { a: '0.2.0-dev.7' }))).toMatchObject({
      version: '0.2.0-dev.7',
    });
    const gone = choose(SITE, request('?play=a', { a: '0.2.0-dev.3' }));
    expect(gone).toMatchObject({ kind: 'error' });
    expect(gone.kind === 'error' && gone.message).toContain('0.2.0-dev.3');
  });

  it("opens a host's invite in the newest build on the host's line", () => {
    expect(choose(SITE, request('?join=h:1&cert=x&v=0.1.0'))).toEqual({
      kind: 'open',
      version: '0.1.1',
      why: 'host',
    });
    expect(choose(SITE, request('?code=KQ7XM4&v=0.2.0'))).toMatchObject({ version: '0.2.0' });
    const none = choose(SITE, request('?code=KQ7XM4&v=0.3.0'));
    expect(none).toMatchObject({ kind: 'error' });
    expect(none.kind === 'error' && none.message).toContain('0.3.0');
  });

  it('opens the latest for an invite or code that names no version', () => {
    expect(choose(SITE, request('?join=h:1&cert=x'))).toMatchObject({ version: '0.2.0' });
    expect(choose(SITE, request('?code=KQ7XM4&v=junk'))).toMatchObject({ version: '0.2.0' });
  });

  it('opens the latest for a world it has no version for (the build explains)', () => {
    expect(choose(SITE, request('?play=unknown'))).toMatchObject({ version: '0.2.0' });
    expect(choose(SITE, request('?play=old', { old: 'not-a-version' }))).toMatchObject({
      version: '0.2.0',
    });
  });

  it('lets a developer pin a version', () => {
    expect(choose(SITE, request('?version=0.1.0'))).toEqual({
      kind: 'open',
      version: '0.1.0',
      why: 'pinned',
    });
    expect(choose(SITE, request('?version=9.9.9'))).toMatchObject({ kind: 'error' });
  });

  it('skips builds that need a newer launcher', () => {
    const m = manifest(['0.1.1', '0.1.0'], { '0.1.1': LAUNCHER_VERSION + 1 });
    expect(choose(m, request('?play=a', { a: '0.1.0' }))).toMatchObject({ version: '0.1.0' });
  });

  it('says so when nothing is published', () => {
    expect(choose(manifest([]), request(''))).toMatchObject({ kind: 'error' });
  });

  it('forwards the query, minus its own parameters', () => {
    expect(forwardedSearch('?play=wabc&debug=1')).toBe('?play=wabc&debug=1');
    expect(forwardedSearch('?version=0.1.0&debug=1')).toBe('?debug=1');
    expect(forwardedSearch('')).toBe('');
  });
});

describe('the manifest', () => {
  const entry = {
    version: '0.1.0',
    channel: 'stable',
    date: 'd',
    commit: 'c',
    protocolVersion: 10,
    minLauncher: 1,
  };

  it('parses, dropping entries that do not', () => {
    const m = parseManifest({
      schema: 1,
      generated: 'now',
      versions: [entry, { ...entry, version: '0.2.0', channel: 'dev' }, { nope: true }, 7],
      latestStable: '0.1.0',
      latestDev: '9.9.9',
    });
    expect(m?.versions.map((v) => v.version)).toEqual(['0.1.0']);
    expect(m?.latestStable).toBe('0.1.0');
    expect(m?.latestDev).toBeNull();
  });

  it('rejects anything else', () => {
    expect(parseManifest(null)).toBeNull();
    expect(parseManifest({ schema: 2, versions: [] })).toBeNull();
    expect(parseManifest({ schema: 1 })).toBeNull();
  });
});

describe('a version directory', () => {
  // build.json names the version, with or without the commit as build metadata.
  it('is the version its build.json names, ignoring build metadata', () => {
    expect(isBuildOf({ version: '0.1.0' }, '0.1.0')).toBe(true);
    expect(isBuildOf({ version: '0.1.0-dev.1+d9a097d' }, '0.1.0-dev.1')).toBe(true);
    expect(isBuildOf({ version: '0.1.1' }, '0.1.0')).toBe(false);
    expect(isBuildOf({ version: '0.1.0-dev.2+abc' }, '0.1.0-dev.1')).toBe(false);
    for (const odd of [null, 7, 'x', {}, { version: 3 }])
      expect(isBuildOf(odd, '0.1.0')).toBe(false);
  });
});
