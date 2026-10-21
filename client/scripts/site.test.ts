import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  buildManifest,
  directorySize,
  hoistMedia,
  loadRetention,
  parseRetention,
  planSite,
  readBuilds,
  sizeReport,
  sizeVerdict,
  tagVersion,
  type BuildJson,
  type RetentionPolicy,
} from './site';

const tags = (...names: string[]) => names.map((tagName) => ({ tagName }));

/** A policy that keeps every stable release (newestOverall above any list here). */
const policy = (over: Partial<RetentionPolicy> = {}): RetentionPolicy => ({
  newestPerLine: false,
  newestOverall: 1000,
  pinned: [],
  devKeep: 10,
  sizeBudgetMB: 900,
  ...over,
});

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
      policy({ devKeep: 2 }),
    );
    expect(plan.keep).toEqual(['v0.2.0', 'v0.1.1', 'v0.1.0', 'v0.2.0-dev.11', 'v0.2.0-dev.10']);
    expect(plan.prune).toEqual(['v0.2.0-dev.9', 'v0.2.0-dev.8']);
    expect(plan.ignored).toEqual(['latest']);
  });

  it('orders dev builds by version precedence, not by when they were made', () => {
    const plan = planSite(
      tags('v0.2.0-dev.100', 'v0.2.0-dev.99', 'v0.3.0-dev.1'),
      policy({ devKeep: 1 }),
    );
    expect(plan.keep).toEqual(['v0.3.0-dev.1']);
    expect(plan.prune).toEqual(['v0.2.0-dev.100', 'v0.2.0-dev.99']);
  });

  it('never deletes a stable release, even one it retires from the site', () => {
    const many = tags(...Array.from({ length: 30 }, (_, i) => `v0.1.${String(i)}`));
    const plan = planSite(many, policy({ newestOverall: 3, devKeep: 0 }));
    expect(plan.prune).toEqual([]);
    expect(plan.keep).toEqual(['v0.1.29', 'v0.1.28', 'v0.1.27']);
    expect(plan.retired).toHaveLength(27);
  });
});

describe('the retention policy', () => {
  const releases = tags(
    'v0.1.0',
    'v0.10.0',
    'v0.10.1',
    'v0.10.2',
    'v0.11.0',
    'v0.11.1',
    'v0.12.0',
    'v0.12.1',
    'v0.12.2',
    'v0.13.0',
    'v0.13.1',
    'v0.13.2-dev.5',
    'v0.13.2-dev.6',
    'v1.0.0',
    'v1.1.0',
  );

  it('keeps the newest release of every compatibility line', () => {
    const plan = planSite(releases, policy({ newestPerLine: true, newestOverall: 0, devKeep: 0 }));
    expect(plan.keep).toEqual(['v1.1.0', 'v0.13.1', 'v0.12.2', 'v0.11.1', 'v0.10.2', 'v0.1.0']);
  });

  it('keeps the newest stable releases overall, across lines', () => {
    const plan = planSite(releases, policy({ newestOverall: 4, devKeep: 0 }));
    expect(plan.keep).toEqual(['v1.1.0', 'v1.0.0', 'v0.13.1', 'v0.13.0']);
  });

  it('keeps every release of a pinned line, and a pinned exact release', () => {
    const plan = planSite(
      releases,
      policy({ newestOverall: 0, pinned: ['0.10', '0.12.0', '1', '0.99'], devKeep: 0 }),
    );
    expect(plan.keep).toEqual(['v1.1.0', 'v1.0.0', 'v0.12.0', 'v0.10.2', 'v0.10.1', 'v0.10.0']);
    expect(plan.unmatchedPins).toEqual(['0.99']);
  });

  it('keeps the union of its rules and retires the rest, leaving the dev builds to devKeep', () => {
    const plan = planSite(
      releases,
      policy({ newestPerLine: true, newestOverall: 2, pinned: ['0.10'], devKeep: 1 }),
    );
    expect(plan.keep).toEqual([
      'v1.1.0',
      'v1.0.0',
      'v0.13.1',
      'v0.12.2',
      'v0.11.1',
      'v0.10.2',
      'v0.10.1',
      'v0.10.0',
      'v0.1.0',
      'v0.13.2-dev.6',
    ]);
    expect(plan.retired).toEqual(['v0.13.0', 'v0.12.1', 'v0.12.0', 'v0.11.0']);
    expect(plan.prune).toEqual(['v0.13.2-dev.5']);
    // Dev builds are the same whatever the stable rules say.
    const strict = planSite(releases, policy({ newestOverall: 0, devKeep: 1 }));
    expect(strict.keep).toEqual(['v0.13.2-dev.6']);
    expect(strict.prune).toEqual(plan.prune);
  });

  it("is read from shared/version/site-retention.json: the owner's policy (2026-10-21)", () => {
    expect(loadRetention()).toEqual({
      newestPerLine: true,
      newestOverall: 10,
      pinned: ['0.10'],
      devKeep: 10,
      sizeBudgetMB: 900,
    });
  });

  it('refuses a policy it does not understand, naming the field', () => {
    const good = { ...policy(), comment: 'notes are allowed' };
    expect(parseRetention(good).newestOverall).toBe(1000);
    const bad: [unknown, string][] = [
      [[], 'not an object'],
      [{ ...good, newestOveral: 3 }, 'unknown field "newestOveral"'],
      [{ ...good, newestPerLine: 'yes' }, '"newestPerLine"'],
      [{ ...good, newestOverall: -1 }, '"newestOverall"'],
      [{ ...good, devKeep: 1.5 }, '"devKeep"'],
      [{ ...good, pinned: '0.10' }, '"pinned" must be a list'],
      [{ ...good, pinned: ['0.10.x'] }, 'pin "0.10.x"'],
      [{ ...good, pinned: ['v0.10.1'] }, 'pin "v0.10.1"'],
      [{ ...good, pinned: ['0.13.2-dev.5'] }, 'pin "0.13.2-dev.5"'],
      [{ ...good, sizeBudgetMB: 1200 }, '"sizeBudgetMB"'],
    ];
    for (const [json, message] of bad) {
      expect(() => parseRetention(json, 'policy.json'), message).toThrow(`policy.json: ${message}`);
    }
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
  it('shows the size against the limit and warns over the budget', () => {
    expect(sizeReport(50e6, 5, 900)).toBe(
      'Site: 50.0 MB of 1000.0 MB (5.0%; budget 900 MB), 5 builds, about 10.0 MB per build',
    );
    expect(sizeReport(899e6, 90, 900)).not.toContain('⚠️');
    expect(sizeReport(901e6, 90, 900)).toContain('⚠️ over the 900 MB budget');
    expect(sizeReport(1168e6, 140, 900)).toContain('⛔');
    expect(sizeReport(0, 0, 900)).toContain('0 builds');
  });

  it('warns over the budget and fails the deploy over the Pages limit', () => {
    expect(sizeVerdict(899e6, 900)).toBe('ok');
    expect(sizeVerdict(901e6, 900)).toBe('warn');
    expect(sizeVerdict(1168e6, 900)).toBe('fail');
  });
});

describe('the music on the site', () => {
  it('is moved out of the builds into one media/ at the root, each file once', () => {
    const site = mkdtempSync(join(tmpdir(), 'dwell-site-'));
    for (const [v, files] of [
      ['0.1.0', []],
      ['0.2.0', ['song-aaaaaaaa.mp3']],
      ['0.2.1', ['song-aaaaaaaa.mp3', 'other-bbbbbbbb.mp3']],
    ] as const) {
      mkdirSync(join(site, 'v', v), { recursive: true });
      writeFileSync(join(site, 'v', v, 'build.json'), '{}');
      if (files.length) mkdirSync(join(site, 'v', v, 'media'));
      for (const f of files) writeFileSync(join(site, 'v', v, 'media', f), `bytes of ${f}`);
    }
    expect(hoistMedia(site)).toEqual({ moved: 2, shared: 1 });
    expect(readdirSync(join(site, 'media')).sort()).toEqual([
      'other-bbbbbbbb.mp3',
      'song-aaaaaaaa.mp3',
    ]);
    for (const v of ['0.2.0', '0.2.1']) expect(existsSync(join(site, 'v', v, 'media'))).toBe(false);
    // The launcher's own copy (already at the root) is kept, and the builds' dropped.
    mkdirSync(join(site, 'v', '0.3.0', 'media'), { recursive: true });
    writeFileSync(
      join(site, 'v', '0.3.0', 'media', 'song-aaaaaaaa.mp3'),
      'bytes of song-aaaaaaaa.mp3',
    );
    expect(hoistMedia(site)).toEqual({ moved: 0, shared: 1 });
  });

  it('refuses a file whose name matches but whose bytes do not', () => {
    const site = mkdtempSync(join(tmpdir(), 'dwell-site-'));
    mkdirSync(join(site, 'media'));
    writeFileSync(join(site, 'media', 'song-aaaaaaaa.mp3'), 'one');
    mkdirSync(join(site, 'v', '0.2.0', 'media'), { recursive: true });
    writeFileSync(join(site, 'v', '0.2.0', 'media', 'song-aaaaaaaa.mp3'), 'longer');
    expect(() => hoistMedia(site)).toThrow(/0\.2\.0: media\/song-aaaaaaaa\.mp3 differs/);
  });
});
