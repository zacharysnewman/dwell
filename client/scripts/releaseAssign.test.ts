import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { MAX_CHANGE_LENGTH, MAX_CHANGES, parseChanges } from '../src/version/changes';
import {
  NOTE_MAX_LENGTH,
  NOTE_MAX_LINES,
  assignVersions,
  parsePendingNote,
  withChangesEntry,
} from './releaseAssign';

const PACKAGE = '{\n  "name": "@dwell/client",\n  "version": "0.52.70",\n  "private": true\n}\n';
const CHANGES = `{
  "$comment": "x",
  "versions": [
    {
      "version": "0.52.70",
      "changes": ["Seventy"]
    },
    {
      "version": "0.52.69",
      "changes": ["Sixty-nine"]
    }
  ]
}
`;
const base = { packageText: PACKAGE, changesText: CHANGES, latestStable: '0.52.70', line: '0.52' };
const note = (name: string, ...changes: string[]) => ({ name, changes });

describe('pending release notes (RELEASES.md §3)', () => {
  it('gives a note the next patch, its changes.json entry and package.json', () => {
    const out = assignVersions({ ...base, notes: [note('lava.json', 'Lava glows')] });
    expect(out.assigned.map((a) => a.version)).toEqual(['0.52.71']);
    expect(JSON.parse(out.packageText)).toMatchObject({ version: '0.52.71', private: true });
    expect(out.packageText).toBe(PACKAGE.replace('0.52.70', '0.52.71'));
    const changes = parseChanges(JSON.parse(out.changesText));
    expect(changes[0]).toEqual({ version: '0.52.71', changes: ['Lava glows'] });
    expect(changes.slice(1).map((c) => c.version)).toEqual(['0.52.70', '0.52.69']);
    // The rest of the file is kept as written (one-line arrays included).
    expect(
      out.changesText.endsWith(CHANGES.slice(CHANGES.indexOf('    {\n      "version": "0.52.70"'))),
    ).toBe(true);
  });

  it('gives several notes a patch each, in file name order; the last is the release', () => {
    const out = assignVersions({
      ...base,
      notes: [note('b.json', 'Second'), note('a.json', 'First', 'Also first')],
    });
    expect(out.assigned.map((a) => [a.version, a.note])).toEqual([
      ['0.52.71', 'a.json'],
      ['0.52.72', 'b.json'],
    ]);
    expect((JSON.parse(out.packageText) as { version: string }).version).toBe('0.52.72');
    expect(parseChanges(JSON.parse(out.changesText)).slice(0, 2)).toEqual([
      { version: '0.52.72', changes: ['Second'] },
      { version: '0.52.71', changes: ['First', 'Also first'] },
    ]);
  });

  it('changes nothing with no notes (a PR that still raised package.json by hand)', () => {
    const out = assignVersions({ ...base, notes: [] });
    expect(out).toEqual({ assigned: [], packageText: PACKAGE, changesText: CHANGES });
  });

  it('goes after a version raised by hand in the same push, keeping its entry', () => {
    // package.json raised to 0.52.71 with its own entry, and a note merged with it.
    const raised = withChangesEntry(CHANGES, { version: '0.52.71', changes: ['By hand'] });
    const out = assignVersions({
      ...base,
      packageText: PACKAGE.replace('0.52.70', '0.52.71'),
      changesText: raised,
      notes: [note('n.json', 'Noted')],
    });
    expect(out.assigned[0]?.version).toBe('0.52.72');
    expect(
      parseChanges(JSON.parse(out.changesText))
        .map((c) => c.version)
        .slice(0, 3),
    ).toEqual(['0.52.72', '0.52.71', '0.52.70']);
  });

  it('starts from package.json when it is above the newest release (a new line), or nothing is out', () => {
    const newLine = assignVersions({
      ...base,
      packageText: PACKAGE.replace('0.52.70', '0.53.0'),
      line: '0.53',
      notes: [note('n.json', 'x')],
    });
    expect(newLine.assigned[0]?.version).toBe('0.53.0');
    const first = assignVersions({
      ...base,
      changesText: '{\n  "versions": [\n  ]\n}\n',
      latestStable: null,
      notes: [note('n.json', 'x')],
    });
    expect(first.assigned[0]?.version).toBe('0.52.70');
    expect(parseChanges(JSON.parse(first.changesText))).toEqual([
      { version: '0.52.70', changes: ['x'] },
    ]);
  });

  it("refuses a version off shared/version/line.json's line", () => {
    expect(() => assignVersions({ ...base, line: '0.53', notes: [note('n.json', 'x')] })).toThrow(
      /not on the line in shared\/version\/line.json \(0.53\)/,
    );
  });

  it('checks a note, and refuses a breaking one loudly', () => {
    expect(parsePendingNote('a.json', { changes: ['ok'] })).toEqual({
      name: 'a.json',
      changes: ['ok'],
    });
    expect(parsePendingNote('a.json', { changes: ['ok'], breaking: false }).changes).toEqual([
      'ok',
    ]);
    expect(() => parsePendingNote('a.json', { changes: ['ok'], breaking: true })).toThrow(
      /pending\/a.json: a breaking change starts a new compatibility line, which the owner raises by hand/,
    );
    expect(() => parsePendingNote('a.json', { changes: [] })).toThrow(/1–6 lines/);
    expect(() => parsePendingNote('a.json', { changes: ['x'.repeat(241)] })).toThrow(/240/);
    expect(() => parsePendingNote('a.json', { change: ['x'] })).toThrow(/unknown field "change"/);
    expect(() => parsePendingNote('Bad Name.json', { changes: ['x'] })).toThrow(/<slug>.json/);
    expect(() => parsePendingNote('a.json', ['x'])).toThrow(/must be an object/);
  });

  it("keeps the client's limits for changes.json", () => {
    expect([NOTE_MAX_LINES, NOTE_MAX_LENGTH]).toEqual([MAX_CHANGES, MAX_CHANGE_LENGTH]);
  });

  // CI: every note a PR carries is valid now, not only when the release workflow takes it.
  it('every pending note in shared/releases/pending is valid', () => {
    const dir = new URL('../../shared/releases/pending/', import.meta.url);
    expect(existsSync(dir)).toBe(true);
    for (const name of readdirSync(dir).filter((n) => n.endsWith('.json'))) {
      parsePendingNote(name, JSON.parse(readFileSync(new URL(name, dir), 'utf8')) as unknown);
    }
  });
});
