// The music data (shared/music/*.json, docs/AUDIO.md §2a): validation with messages that name the
// entry, the generated data current, and a track added by data alone.
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { loadMusic, MUSIC_DIR, readMusicFiles } from '../../../shared/music/load.mjs';
import { contentHash, MAX_BYTES, resolveMusic } from '../../../shared/music/validate.mjs';
import { chooseTrack } from './music';
import { MUSIC_DATA } from './music.gen';

type File = { namespace: string; tracks: Record<string, Record<string, unknown>> };

const files = (): File[] => structuredClone(readMusicFiles()) as File[];

const track = {
  title: 'A Fixture',
  author: 'A Tester',
  file: 'fixture.mp3',
  volume: 0.5,
  loop: false,
  fadeInS: 1,
  fadeOutS: 1,
  when: 'always',
};

/** A directory holding `json` and a small fake audio file per track it names. */
function fixtureDir(json: unknown[], audio: Record<string, Uint8Array> = {}): string {
  const dir = mkdtempSync(join(tmpdir(), 'dwell-music-'));
  json.forEach((f, i) => {
    writeFileSync(join(dir, `${String(i)}.json`), JSON.stringify(f));
  });
  for (const [name, bytes] of Object.entries(audio)) writeFileSync(join(dir, name), bytes);
  return dir;
}

describe('music data', () => {
  it('validates, and the generated data is current', () => {
    expect(loadMusic()).toEqual(MUSIC_DATA);
  });

  it('has Starry Sunshine, by the owner, playing everywhere', () => {
    const t = chooseTrack(MUSIC_DATA);
    expect(t?.title).toBe('Starry Sunshine');
    expect(t?.author).toBe('Zachary Newman');
    expect(t?.loop).toBe(true);
    expect(t?.file).toMatch(/^starry-sunshine-[0-9a-f]{8}\.mp3$/);
  });

  it('takes a new track by data alone, its published name from its bytes', () => {
    const bytes = new Uint8Array([1, 2, 3, 4]);
    const dir = fixtureDir([{ namespace: 'test', tracks: { fixture: track } }], {
      'fixture.mp3': bytes,
    });
    const data = resolveMusic(readMusicFiles(dir), dir);
    expect(data.tracks).toEqual([
      {
        id: 'test:fixture',
        title: 'A Fixture',
        author: 'A Tester',
        source: 'fixture.mp3',
        file: `fixture-${contentHash(bytes)}.mp3`,
        volume: 0.5,
        loop: false,
        fadeInS: 1,
        fadeOutS: 1,
        when: 'always',
      },
    ]);
    expect(chooseTrack(data)?.id).toBe('test:fixture');
    // Shipped next to the shipped data: the namespaces in order, the first matching plays.
    const both = resolveMusic(
      [
        ...files(),
        { namespace: 'zz', tracks: { fixture: { ...track, file: 'starry-sunshine.mp3' } } },
      ],
      MUSIC_DIR,
    );
    expect(both.tracks.map((t) => t.id)).toEqual(['dwell:starry_sunshine', 'zz:fixture']);
  });

  const rejects: [string, (f: File) => void, RegExp][] = [
    [
      'an unknown field',
      (f) => (f.tracks.x = { ...track, file: 'starry-sunshine.mp3', tempo: 1 }),
      /track dwell:x: unknown field "tempo"/,
    ],
    [
      'a missing field',
      (f) => {
        f.tracks.x = { ...track, file: 'starry-sunshine.mp3' };
        delete f.tracks.x.author;
      },
      /track dwell:x: missing "author"/,
    ],
    [
      'an empty title',
      (f) => (f.tracks.x = { ...track, file: 'starry-sunshine.mp3', title: ' ' }),
      /track dwell:x\.title: must be a non-empty string/,
    ],
    [
      'a missing file',
      (f) => (f.tracks.x = { ...track, file: 'nowhere.mp3' }),
      /track dwell:x\.file: nowhere\.mp3 is not in shared\/music/,
    ],
    [
      'a format not every browser plays',
      (f) => (f.tracks.x = { ...track, file: 'song.ogg' }),
      /track dwell:x\.file: "song\.ogg" must be a lower-case \.mp3 or \.m4a/,
    ],
    [
      'a volume past 1',
      (f) => (f.tracks.x = { ...track, file: 'starry-sunshine.mp3', volume: 1.5 }),
      /track dwell:x\.volume: 1\.5 is outside \[0, 1\]/,
    ],
    [
      'a loop that is not a flag',
      (f) => (f.tracks.x = { ...track, file: 'starry-sunshine.mp3', loop: 'yes' }),
      /track dwell:x\.loop: must be true or false/,
    ],
    [
      'a negative fade',
      (f) => (f.tracks.x = { ...track, file: 'starry-sunshine.mp3', fadeInS: -1 }),
      /track dwell:x\.fadeInS: -1 is outside/,
    ],
    [
      'an unknown condition',
      (f) => (f.tracks.x = { ...track, file: 'starry-sunshine.mp3', when: 'night' }),
      /track dwell:x\.when: "night" is not one of always/,
    ],
    [
      'a bad name',
      (f) => (f.tracks['Bad Name'] = { ...track, file: 'starry-sunshine.mp3' }),
      /track dwell:Bad Name: bad name/,
    ],
    ['no tracks', (f) => (f.tracks = {}), /shared\/music: tracks: none/],
  ];
  for (const [what, edit, message] of rejects) {
    it(`rejects ${what}`, () => {
      const fs = files();
      const f = fs[0];
      if (!f) throw new Error('no music file');
      edit(f);
      expect(() => resolveMusic(fs, MUSIC_DIR)).toThrow(message);
    });
  }

  it('rejects a file too large to stream', () => {
    const dir = fixtureDir([], { 'big.mp3': new Uint8Array(MAX_BYTES + 1) });
    expect(() =>
      resolveMusic([{ namespace: 'test', tracks: { big: { ...track, file: 'big.mp3' } } }], dir),
    ).toThrow(/track test:big\.file: big\.mp3 is \d+ bytes, over/);
  });
});
