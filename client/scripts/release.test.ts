import { describe, expect, it } from 'vitest';
import { latestStable, nextVersion, releasePlan } from './release';

const SHA = 'ab12cd34ef56ab12cd34ef56ab12cd34ef56ab12';
const base = { run: 42, sha: SHA, packageVersion: '0.2.0', latestStable: null };

describe('what a release run builds', () => {
  it('builds a dev pre-release of the next version for a push to main', () => {
    expect(
      releasePlan({ ...base, latestStable: '0.2.0', event: 'push', ref: 'refs/heads/main' }),
    ).toEqual({
      version: '0.2.1-dev.42',
      buildVersion: '0.2.1-dev.42+ab12cd3',
      tag: 'v0.2.1-dev.42',
      channel: 'dev',
      archive: 'dwell-0.2.1-dev.42.tar.gz',
    });
  });

  it('orders dev builds below the release they lead to, and by run number', () => {
    const at = (run: number) =>
      releasePlan({ ...base, latestStable: '0.2.0', run, event: 'push', ref: '' }).version;
    expect(at(9)).toBe('0.2.1-dev.9');
    expect(at(10)).toBe('0.2.1-dev.10');
  });

  it('releases package.json’s version for a tag that names it', () => {
    expect(releasePlan({ ...base, event: 'tag', ref: 'refs/tags/v0.2.0' })).toEqual({
      version: '0.2.0',
      buildVersion: '0.2.0',
      tag: 'v0.2.0',
      channel: 'stable',
      archive: 'dwell-0.2.0.tar.gz',
    });
  });

  it('refuses a tag that is not a release version', () => {
    expect(() => releasePlan({ ...base, event: 'tag', ref: 'refs/tags/v0.2.0-dev.1' })).toThrow();
    expect(() => releasePlan({ ...base, event: 'tag', ref: 'refs/tags/nightly' })).toThrow();
    expect(() => releasePlan({ ...base, event: 'tag', ref: 'refs/tags/v0.2.0+abc' })).toThrow();
  });

  it('releases package.json’s version when run by hand', () => {
    expect(releasePlan({ ...base, event: 'dispatch', ref: 'refs/heads/main' })).toMatchObject({
      version: '0.2.0',
      tag: 'v0.2.0',
      channel: 'stable',
    });
  });

  it('needs package.json to hold a release version', () => {
    for (const packageVersion of ['0.2.0-dev.1', '0.2', '0.2.0+abc', 'next']) {
      expect(() => releasePlan({ ...base, packageVersion, event: 'dispatch', ref: '' })).toThrow(
        'package.json',
      );
    }
  });

  it('checks the run number and commit of a dev build', () => {
    const released = { ...base, latestStable: '0.2.0' };
    expect(() => releasePlan({ ...released, run: 0, event: 'push', ref: '' })).toThrow(
      'run number',
    );
    expect(() => releasePlan({ ...released, sha: 'unknown', event: 'push', ref: '' })).toThrow(
      'commit',
    );
  });
});

// The next version follows the newest release, so nobody has to bump package.json after one
// (RELEASES.md §3): package.json is a floor, raised by hand only for a breaking change.
describe('the version after a release', () => {
  const released = { ...base, packageVersion: '0.1.0', latestStable: '0.1.0' };

  it('is the next patch once package.json’s version is released', () => {
    expect(nextVersion('0.1.0', null)).toBe('0.1.0'); // the baseline
    expect(nextVersion('0.1.0', '0.1.0')).toBe('0.1.1');
    expect(nextVersion('0.1.0', '0.1.7')).toBe('0.1.8');
    expect(nextVersion('1.4.2', '1.4.2')).toBe('1.4.3');
  });

  it('is package.json’s version when that is higher (a breaking change raised it)', () => {
    expect(nextVersion('0.2.0', '0.1.7')).toBe('0.2.0');
    expect(nextVersion('1.0.0', '0.9.9')).toBe('1.0.0');
  });

  it('follows a release even when package.json was left behind', () => {
    expect(nextVersion('0.1.0', '0.3.2')).toBe('0.3.3');
  });

  it('numbers dev builds as pre-releases of it', () => {
    expect(releasePlan({ ...released, event: 'push', ref: '' })).toMatchObject({
      version: '0.1.1-dev.42',
      buildVersion: '0.1.1-dev.42+ab12cd3',
      tag: 'v0.1.1-dev.42',
    });
    // package.json above the newest release is a release, not a dev build (below).
    expect(
      releasePlan({
        ...released,
        packageVersion: '0.1.1',
        latestStable: '0.1.5',
        event: 'push',
        ref: '',
      }).version,
    ).toBe('0.1.6-dev.42');
  });

  it('releases the next version when run by hand, or the one asked for', () => {
    expect(releasePlan({ ...released, event: 'dispatch', ref: '' })).toMatchObject({
      version: '0.1.1',
      channel: 'stable',
    });
    expect(
      releasePlan({ ...released, event: 'dispatch', ref: '', requested: '0.2.0' }).version,
    ).toBe('0.2.0');
    expect(releasePlan({ ...released, event: 'dispatch', ref: '', requested: '' }).version).toBe(
      '0.1.1',
    );
  });

  it('releases the version a tag names, if it is newer than the last release and not below the floor', () => {
    expect(releasePlan({ ...released, event: 'tag', ref: 'refs/tags/v0.1.1' }).version).toBe(
      '0.1.1',
    );
    expect(releasePlan({ ...released, event: 'tag', ref: 'refs/tags/v0.2.0' }).version).toBe(
      '0.2.0',
    );
    expect(() => releasePlan({ ...released, event: 'tag', ref: 'refs/tags/v0.1.0' })).toThrow(
      'already',
    );
    expect(() => releasePlan({ ...released, event: 'tag', ref: 'refs/tags/v0.0.9' })).toThrow(
      'already',
    );
    expect(() =>
      releasePlan({ ...released, packageVersion: '0.3.0', event: 'tag', ref: 'refs/tags/v0.2.0' }),
    ).toThrow('package.json');
  });

  it('refuses a requested version that is not newer, or not a release', () => {
    for (const requested of ['0.1.0', '0.0.5', '0.2.0-dev.1', 'next', '0.2.0+abc']) {
      expect(() => releasePlan({ ...released, event: 'dispatch', ref: '', requested })).toThrow();
    }
  });
});

describe('the newest release', () => {
  it('is the highest stable version among the release tags', () => {
    expect(latestStable(['v0.1.0', 'v0.1.10', 'v0.1.9', 'v0.2.0-dev.5', 'nightly'])).toBe('0.1.10');
    expect(latestStable(['v0.2.0-dev.5', 'x'])).toBeNull();
    expect(latestStable([])).toBeNull();
  });
});

// A push to main that raises package.json above the newest release releases that version: the
// bump is the deliberate act (RELEASES.md §3), so no one has to run the workflow for it.
describe('a push that raises package.json above the newest release', () => {
  const push = { ...base, event: 'push' as const, ref: 'refs/heads/main' };

  it('releases the baseline when nothing is released yet', () => {
    expect(releasePlan({ ...push, packageVersion: '0.1.0', latestStable: null })).toEqual({
      version: '0.1.0',
      buildVersion: '0.1.0',
      tag: 'v0.1.0',
      channel: 'stable',
      archive: 'dwell-0.1.0.tar.gz',
    });
  });

  it('releases a new line when package.json was raised to it', () => {
    expect(releasePlan({ ...push, packageVersion: '0.2.0', latestStable: '0.1.4' })).toMatchObject({
      version: '0.2.0',
      channel: 'stable',
      tag: 'v0.2.0',
    });
  });

  it('is only a dev build once that version is released', () => {
    for (const latest of ['0.2.0', '0.3.1']) {
      const plan = releasePlan({ ...push, packageVersion: '0.2.0', latestStable: latest });
      expect(plan.channel).toBe('dev');
    }
  });
});

// Backports (RELEASES.md §3): a patch of an older compatibility line, released from its
// maintenance branch while a newer line is out.
describe('a patch of an older line', () => {
  const branch = { ...base, packageVersion: '0.3.0', latestStable: '0.4.0', lineLatest: '0.3.0' };

  it('releases the patch a tag names on the line’s branch', () => {
    expect(releasePlan({ ...branch, event: 'tag', ref: 'refs/tags/v0.3.1' })).toEqual({
      version: '0.3.1',
      buildVersion: '0.3.1',
      tag: 'v0.3.1',
      channel: 'stable',
      archive: 'dwell-0.3.1.tar.gz',
    });
    expect(
      releasePlan({
        ...branch,
        event: 'dispatch',
        ref: 'refs/heads/release/0.3',
        requested: '0.3.1',
      }).version,
    ).toBe('0.3.1');
  });

  it('refuses a version the line already has', () => {
    expect(() => releasePlan({ ...branch, event: 'tag', ref: 'refs/tags/v0.3.0' })).toThrow(
      'already released',
    );
    expect(() =>
      releasePlan({ ...branch, lineLatest: '0.3.2', event: 'tag', ref: 'refs/tags/v0.3.2' }),
    ).toThrow('newest release on its line is 0.3.2');
  });

  it('refuses an older line’s version from a branch on another line', () => {
    // main (package.json 0.4.0) cannot be released as a 0.3 patch.
    const main = { ...base, packageVersion: '0.4.0', latestStable: '0.4.0', lineLatest: '0.4.0' };
    expect(() => releasePlan({ ...main, event: 'tag', ref: 'refs/tags/v0.3.1' })).toThrow();
    // Nor a 0.2 patch from the 0.3 branch.
    expect(() => releasePlan({ ...branch, event: 'tag', ref: 'refs/tags/v0.2.5' })).toThrow();
  });

  it('releases the line’s next patch when run by hand on its branch', () => {
    const run = { ...branch, event: 'dispatch' as const, ref: 'refs/heads/release/0.3' };
    expect(releasePlan(run).version).toBe('0.3.1');
    expect(releasePlan({ ...run, lineLatest: '0.3.4' }).version).toBe('0.3.5');
    expect(() => releasePlan({ ...run, requested: '0.4.1' })).toThrow('not on this branch');
  });

  it('finds the newest release of a line', () => {
    const tags = ['v0.3.0', 'v0.3.1', 'v0.4.0', 'v0.3.2-dev.4', 'v0.2.0'];
    expect(latestStable(tags, '0.3.0')).toBe('0.3.1');
    expect(latestStable(tags, '0.4.0')).toBe('0.4.0');
    expect(latestStable(tags, '0.5.0')).toBeNull();
    expect(latestStable(tags)).toBe('0.4.0');
  });
});

describe('a preview of a pull request', () => {
  const pr = {
    ...base,
    latestStable: '0.2.0',
    event: 'preview' as const,
    ref: 'refs/pull/63/merge',
  };

  it('is a pre-release of the next version, named by the PR and the run', () => {
    expect(releasePlan(pr)).toEqual({
      version: '0.2.1-pr.63.42',
      buildVersion: '0.2.1-pr.63.42+ab12cd3',
      tag: 'v0.2.1-pr.63.42',
      channel: 'dev',
      archive: 'dwell-0.2.1-pr.63.42.tar.gz',
    });
  });

  it('is never a release, even when the PR raises package.json', () => {
    expect(releasePlan({ ...pr, packageVersion: '0.3.0' })).toMatchObject({
      version: '0.3.0-pr.63.42',
      channel: 'dev',
    });
  });

  it('needs a pull request ref, a run and a commit', () => {
    expect(() => releasePlan({ ...pr, ref: 'refs/heads/main' })).toThrow('pull request');
    expect(() => releasePlan({ ...pr, ref: 'refs/pull/0/merge' })).toThrow('pull request');
    expect(() => releasePlan({ ...pr, run: 0 })).toThrow('run number');
    expect(() => releasePlan({ ...pr, sha: 'main' })).toThrow('commit');
  });
});
