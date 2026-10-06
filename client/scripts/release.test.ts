import { describe, expect, it } from 'vitest';
import { releasePlan } from './release';

const SHA = 'ab12cd34ef56ab12cd34ef56ab12cd34ef56ab12';
const base = { run: 42, sha: SHA, packageVersion: '0.2.0' };

describe('what a release run builds', () => {
  it('builds a dev pre-release of package.json’s version for a push to main', () => {
    expect(releasePlan({ ...base, event: 'push', ref: 'refs/heads/main' })).toEqual({
      version: '0.2.0-dev.42',
      buildVersion: '0.2.0-dev.42+ab12cd3',
      tag: 'v0.2.0-dev.42',
      channel: 'dev',
      archive: 'dwell-0.2.0-dev.42.tar.gz',
    });
  });

  it('orders dev builds below the release they lead to, and by run number', () => {
    const at = (run: number) => releasePlan({ ...base, run, event: 'push', ref: '' }).version;
    expect(at(9)).toBe('0.2.0-dev.9');
    expect(at(10)).toBe('0.2.0-dev.10');
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

  it('refuses a tag that does not name package.json’s version', () => {
    expect(() => releasePlan({ ...base, event: 'tag', ref: 'refs/tags/v0.3.0' })).toThrow(
      'does not match',
    );
    expect(() => releasePlan({ ...base, event: 'tag', ref: 'refs/tags/v0.2.0-dev.1' })).toThrow();
    expect(() => releasePlan({ ...base, event: 'tag', ref: 'refs/tags/nightly' })).toThrow();
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
    expect(() => releasePlan({ ...base, run: 0, event: 'push', ref: '' })).toThrow('run number');
    expect(() => releasePlan({ ...base, sha: 'unknown', event: 'push', ref: '' })).toThrow(
      'commit',
    );
  });
});
