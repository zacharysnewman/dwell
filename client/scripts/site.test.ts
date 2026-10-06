import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  buildManifest,
  directorySize,
  planSite,
  readBuilds,
  sizeReport,
  tagVersion,
  type BuildJson,
} from './site';

const tags = (...names: string[]) => names.map((tagName) => ({ tagName }));

function build(version: string, channel?: string): BuildJson {
  return {
    version,
    channel: channel ?? (version.includes('-') ? 'dev' : 'stable'),
    date: '2026-10-06T00:00:00.000Z',
    commit: 'abc',
    protocolVersion: 10,
    minLauncher: 1,
  };
}

describe('release tags', () => {
  it('name a version without build metadata', () => {
    expect(tagVersion('v0.1.0')).toBe('0.1.0');
    expect(tagVersion('v0.2.0-dev.42')).toBe('0.2.0-dev.42');
    expect(tagVersion('0.1.0')).toBeNull();
    expect(tagVersion('v0.2.0-dev.42+ab12cd3')).toBeNull();
    expect(tagVersion('nightly')).toBeNull();
  });
});

describe('which releases the site holds', () => {
  it('keeps every stable release and the newest dev builds, pruning the rest', () => {
    const plan = planSite(
      tags(
        'v0.1.0',
        'v0.2.0-dev.9',
        'v0.2.0-dev.10',
        'v0.2.0-dev.11',
        'v0.1.1',
        'v0.2.0-dev.8',
        'v0.2.0',
        'latest',
      ),
      2,
    );
    expect(plan.keep).toEqual(['v0.2.0', 'v0.1.1', 'v0.1.0', 'v0.2.0-dev.11', 'v0.2.0-dev.10']);
    expect(plan.prune).toEqual(['v0.2.0-dev.9', 'v0.2.0-dev.8']);
    expect(plan.ignored).toEqual(['latest']);
  });

  it('orders dev builds by version precedence, not by when they were made', () => {
    const plan = planSite(tags('v0.2.0-dev.100', 'v0.2.0-dev.99', 'v0.3.0-dev.1'), 1);
    expect(plan.keep).toEqual(['v0.3.0-dev.1']);
    expect(plan.prune).toEqual(['v0.2.0-dev.100', 'v0.2.0-dev.99']);
  });

  it('never prunes a stable release', () => {
    const many = tags(...Array.from({ length: 30 }, (_, i) => `v0.1.${String(i)}`));
    expect(planSite(many, 0).prune).toEqual([]);
    expect(planSite(many, 0).keep).toHaveLength(30);
  });
});

describe('the manifest', () => {
  it('lists builds newest first, with the latest of each channel', () => {
    const m = buildManifest(
      new Map([
        ['0.1.0', build('0.1.0')],
        ['0.2.0-dev.7', build('0.2.0-dev.7')],
        ['0.2.0-dev.10', build('0.2.0-dev.10')],
        ['0.1.1', build('0.1.1')],
      ]),
      'now',
    );
    expect(m.versions.map((v) => v.version)).toEqual([
      '0.2.0-dev.10',
      '0.2.0-dev.7',
      '0.1.1',
      '0.1.0',
    ]);
    expect(m.latestStable).toBe('0.1.1');
    expect(m.latestDev).toBe('0.2.0-dev.10');
    expect(m.schema).toBe(1);
  });

  // The release workflow's first real run: a dev build's build.json carried `+<commit>` (the
  // version the build embeds), which the manifest refused, so the site was never deployed.
  it('takes a build whose build.json version carries build metadata, listing it without', () => {
    const m = buildManifest(
      new Map([
        ['0.1.0-dev.1', build('0.1.0-dev.1+d9a097d')],
        ['0.1.0', build('0.1.0')],
      ]),
      'now',
    );
    expect(m.versions.map((v) => v.version)).toEqual(['0.1.0', '0.1.0-dev.1']);
    expect(m.latestDev).toBe('0.1.0-dev.1');
  });

  it('has no latest for a channel with no builds', () => {
    const m = buildManifest(new Map([['0.1.0', build('0.1.0')]]), 'now');
    expect(m.latestDev).toBeNull();
    expect(buildManifest(new Map(), 'now').latestStable).toBeNull();
  });

  it('refuses a build that does not match its directory', () => {
    expect(() => buildManifest(new Map([['0.1.0', build('0.1.1')]]), 'now')).toThrow('0.1.0');
    expect(() => buildManifest(new Map([['0.1.0', build('0.1.0', 'dev')]]), 'now')).toThrow(
      'channel',
    );
    expect(() => buildManifest(new Map([['x', build('nope')]]), 'now')).toThrow('bad version');
    expect(() =>
      buildManifest(new Map([['0.1.0', { ...build('0.1.0'), minLauncher: 'x' as never }]]), 'now'),
    ).toThrow('minLauncher');
  });

  it('is read from the site directory', () => {
    const site = mkdtempSync(join(tmpdir(), 'dwell-site-'));
    for (const v of ['0.1.0', '0.2.0']) {
      mkdirSync(join(site, 'v', v), { recursive: true });
      writeFileSync(join(site, 'v', v, 'build.json'), JSON.stringify(build(v)));
    }
    expect([...readBuilds(site).keys()]).toEqual(['0.1.0', '0.2.0']);
    mkdirSync(join(site, 'v', '0.3.0'));
    expect(() => readBuilds(site)).toThrow('no build.json');
    expect(directorySize(site)).toBeGreaterThan(0);
  });
});

describe('the size report', () => {
  it('shows the size against the limit and warns near it', () => {
    expect(sizeReport(50e6, 5)).toBe(
      'Site: 50.0 MB of 1000.0 MB (5.0%), 5 builds, about 10.0 MB per build',
    );
    expect(sizeReport(900e6, 90)).toContain('⚠️');
    expect(sizeReport(0, 0)).toContain('0 builds');
  });
});
