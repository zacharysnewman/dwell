import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { CHANGES, changesBetween, changesFor, parseChanges } from './changes';

const packageVersion = (
  JSON.parse(readFileSync(new URL('../../package.json', import.meta.url), 'utf8')) as {
    version: string;
  }
).version;

describe('changes', () => {
  // docs/RELEASES.md §3: every release says what changed. A PR raises package.json's version and
  // adds its entry to shared/releases/changes.json in the same change.
  it("has an entry for this build's version (package.json)", () => {
    expect(
      changesFor(packageVersion),
      `add ${packageVersion} to shared/releases/changes.json`,
    ).not.toBeNull();
    expect(CHANGES[0]?.version).toBe(packageVersion);
  });

  it('lists every version once, newest first, each a few short lines', () => {
    expect(CHANGES.length).toBeGreaterThan(100);
    expect(changesFor('0.38.4')?.join(' ')).toMatch(/Drag building/);
  });

  it('a dev build shows its base version', () => {
    expect(changesFor('0.38.4-dev.3')).toEqual(changesFor('0.38.4'));
    expect(changesFor('9.9.9')).toBeNull();
  });

  it('what a build brings over an older one, retired patches included', () => {
    const all = parseChanges({
      versions: [
        { version: '0.2.1', changes: ['c'] },
        { version: '0.2.0', changes: ['b'] },
        { version: '0.1.0', changes: ['a'] },
      ],
    });
    expect(changesBetween('0.1.0', '0.2.1', all).map((e) => e.version)).toEqual(['0.2.1', '0.2.0']);
    expect(changesBetween(null, '0.1.0', all).map((e) => e.version)).toEqual(['0.1.0']);
  });

  it('a bad entry fails with its version named', () => {
    const bad = (versions: unknown) => () => parseChanges({ versions });
    expect(bad([])).toThrow(/non-empty/);
    expect(bad([{ version: '0.1.0-dev.1', changes: ['x'] }])).toThrow(
      /0.1.0-dev.1: version must be/,
    );
    expect(bad([{ version: '0.1.0', changes: [] }])).toThrow(/0.1.0: changes must be/);
    expect(bad([{ version: '0.1.0', changes: ['x'.repeat(241)] }])).toThrow(/0.1.0: each change/);
    expect(
      bad([
        { version: '0.1.0', changes: ['a'] },
        { version: '0.2.0', changes: ['b'] },
      ]),
    ).toThrow(/0.2.0: listed after 0.1.0/);
  });
});
