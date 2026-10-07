import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  canOpenWorld,
  compareVersionText,
  compareVersions,
  compatibilityLine,
  isPreviewVersion,
  isStable,
  parseVersion,
  sameLine,
  type Version,
} from './semver';

const vectors = readFileSync(
  new URL('../../../shared/version/vectors.txt', import.meta.url),
  'utf8',
)
  .split('\n')
  .filter((l) => l.trim() !== '' && !l.startsWith('#'))
  .map((l) => l.trim().split(/\s+/));

/** A version the vectors say is valid. */
function valid(text: string): Version {
  const v = parseVersion(text);
  if (!v) throw new Error(`not a valid version: ${text}`);
  return v;
}

function ofKind(kind: string): string[][] {
  return vectors.filter((v) => v[0] === kind).map((v) => v.slice(1));
}

describe('version vectors (shared with C++)', () => {
  it.each(ofKind('compare'))('compare %s %s = %s', (a, b, r) => {
    expect(compareVersions(valid(a), valid(b))).toBe(Number(r));
  });

  it.each(ofKind('line'))('line %s = %s', (v, line) => {
    expect(compatibilityLine(v)).toBe(line === '-' ? null : line);
  });

  it.each(ofKind('open'))('open %s %s = %s', (build, world, answer) => {
    expect(canOpenWorld(build, world)).toBe(answer === 'YES');
  });

  it('rejects invalid versions', () => {
    const invalid = vectors.filter((v) => v[0] === 'invalid').map((v) => v[1] ?? '');
    expect(invalid.length).toBeGreaterThan(0);
    for (const v of invalid) expect(parseVersion(v)).toBeNull();
  });
});

describe('versions', () => {
  it('tells releases from dev builds', () => {
    expect(isStable(valid('0.1.0'))).toBe(true);
    expect(isStable(valid('0.2.0-dev.42+ab12cd3'))).toBe(false);
  });

  it('sorts version strings, invalid ones first', () => {
    expect(
      ['0.2.0', '0.1.0', '0.2.0-dev.10', 'junk', '0.2.0-dev.9'].sort(compareVersionText),
    ).toEqual(['junk', '0.1.0', '0.2.0-dev.9', '0.2.0-dev.10', '0.2.0']);
  });

  it('knows which versions can play together', () => {
    expect(sameLine('0.1.0', '0.1.4')).toBe(true);
    expect(sameLine('0.1.0', '0.2.0')).toBe(false);
    expect(sameLine('0.2.0-dev.1', '0.2.0-dev.1+x')).toBe(true);
    expect(sameLine('0.2.0-dev.1', '0.2.0-dev.2')).toBe(false);
    expect(sameLine('nope', 'nope')).toBe(false);
  });
});

describe('preview versions', () => {
  it('are the pull requests’ pre-releases, not dev builds or releases', () => {
    expect(isPreviewVersion('0.2.1-pr.63.9')).toBe(true);
    expect(isPreviewVersion('0.2.1-pr.63.9+ab12cd3')).toBe(true);
    expect(isPreviewVersion('0.2.1-dev.9')).toBe(false);
    expect(isPreviewVersion('0.2.1')).toBe(false);
    expect(isPreviewVersion('pr')).toBe(false);
  });

  it('rank above dev builds of their version, so they must be skipped by name', () => {
    expect(compareVersionText('0.2.1-pr.63.9', '0.2.1-dev.99')).toBe(1);
    expect(compareVersionText('0.2.1-pr.63.9', '0.2.1')).toBe(-1);
  });
});
