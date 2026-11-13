// The music data (shared/music/*.json, docs/AUDIO.md §2a): validation with messages that name the
// entry, the generated data current, and a track added by data alone.
import { mkdtempSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { loadMusic, MUSIC_DIR, readMusicFiles } from '../../../shared/music/load.mjs';
import { contentHash, MAX_BYTES, resolveMusic } from '../../../shared/music/validate.mjs';
import { playable } from './playlist';
import { MUSIC_DATA } from './music.gen';

type File = {
  namespace: string;
  playlist?: Record<string, unknown>;
  tracks: Record<string, Record<string, unknown>>;
};

const PLAYLIST = {
  order: 'shuffle',
  gapMinS: 5,
  gapMaxS: 10,
  crossfadeS: 2,
  dayAt: 0.1,
  contexts: {
    menu: { firstDelayMinS: 0, firstDelayMaxS: 0 },
    game: { firstDelayMinS: 3, firstDelayMaxS: 9 },
  },
};

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

  it('plays the casual tracks (Andrew Sitkov) everywhere, shuffled, each small enough to ship', () => {
    const ts = playable(MUSIC_DATA);
    expect(ts.length).toBeGreaterThanOrEqual(10);
    expect(MUSIC_DATA.playlist.order).toBe('shuffle');
    for (const t of ts) {
      expect(t.author).toBe('Andrew Sitkov');
      // Played once each, then the next: the playlist moves on, they never loop.
      expect(t.loop).toBe(false);
      expect(t.file).toMatch(/^[a-z0-9-]+-[0-9a-f]{8}\.m4a$/);
    }
    // No AI-generated track left (owner, 2026-11-12: these replace it).
    expect(MUSIC_DATA.tracks.some((t) => t.title === 'Starry Sunshine')).toBe(false);
  });

  it('keeps every file under 2 MB and the set under 25 MB', () => {
    const sizes = MUSIC_DATA.tracks.map((t) => statSync(join(MUSIC_DIR, t.source)).size);
    for (const s of sizes) expect(s).toBeLessThan(2_000_000);
    expect(sizes.reduce((a, b) => a + b, 0)).toBeLessThan(25_000_000);
  });

  it('takes a new track by data alone, its published name from its bytes', () => {
    const bytes = new Uint8Array([1, 2, 3, 4]);
    const dir = fixtureDir(
      [{ namespace: 'test', playlist: PLAYLIST, tracks: { fixture: track } }],
      {
        'fixture.mp3': bytes,
      },
    );
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
        times: ['day', 'night'],
      },
    ]);
    expect(data.playlist).toEqual(PLAYLIST);
    expect(playable(data).map((t) => t.id)).toEqual(['test:fixture']);
    // Shipped next to the shipped data: the namespaces in order, the first matching plays.
    const both = resolveMusic(
      [
        ...files(),
        { namespace: 'zz', tracks: { fixture: { ...track, file: 'casual-theme-1.m4a' } } },
      ],
      MUSIC_DIR,
    );
    expect(both.tracks.map((t) => t.id).slice(-1)).toEqual(['zz:fixture']);
    expect(both.tracks[0]?.id).toBe(MUSIC_DATA.tracks[0]?.id);
  });

  it('plays at random times (owner, 2026-11-12): 3–10 minute gaps, the game waiting, the menu not', () => {
    const p = MUSIC_DATA.playlist;
    expect([p.gapMinS, p.gapMaxS]).toEqual([180, 600]);
    expect(p.contexts.menu).toEqual({ firstDelayMinS: 0, firstDelayMaxS: 0 });
    expect(p.contexts.game.firstDelayMinS).toBeGreaterThan(0);
    expect(p.contexts.game.firstDelayMaxS).toBeGreaterThanOrEqual(p.contexts.game.firstDelayMinS);
  });

  it('accepts gaps and first waits up to an hour, and a track for the night by data alone', () => {
    const dir = fixtureDir([], { 'fixture.mp3': new Uint8Array([1]) });
    const data = resolveMusic(
      [
        {
          namespace: 'test',
          playlist: {
            ...PLAYLIST,
            gapMinS: 3600,
            gapMaxS: 3600,
            contexts: {
              menu: { firstDelayMinS: 0, firstDelayMaxS: 0 },
              game: { firstDelayMinS: 3600, firstDelayMaxS: 3600 },
            },
          },
          tracks: {
            night: { ...track, times: ['night'] },
            both: { ...track, times: ['night', 'day'] },
            any: track,
          },
        },
      ],
      dir,
    );
    expect(data.playlist.gapMaxS).toBe(3600);
    expect(data.playlist.contexts.game.firstDelayMinS).toBe(3600);
    expect(data.tracks.map((t) => t.times)).toEqual([
      ['night'],
      ['day', 'night'],
      ['day', 'night'],
    ]);
  });

  const withContexts = (f: File, contexts: unknown): unknown =>
    (f.playlist = { ...PLAYLIST, contexts });
  const rejects: [string, (f: File) => void, RegExp][] = [
    [
      'a gap over an hour',
      (f) => (f.playlist = { ...PLAYLIST, gapMaxS: 3601 }),
      /playlist \(dwell\)\.gapMaxS: 3601 is outside \[0, 3600\]/,
    ],
    [
      'a daylight threshold past 1',
      (f) => (f.playlist = { ...PLAYLIST, dayAt: 2 }),
      /playlist \(dwell\)\.dayAt: 2 is outside \[0, 1\]/,
    ],
    [
      'no contexts',
      (f) => {
        f.playlist = { ...PLAYLIST };
        delete f.playlist.contexts;
      },
      /playlist \(dwell\): missing "contexts"/,
    ],
    [
      'a missing context',
      (f) => withContexts(f, { menu: PLAYLIST.contexts.menu }),
      /playlist \(dwell\)\.contexts: missing "game"/,
    ],
    [
      'an unknown context',
      (f) => withContexts(f, { ...PLAYLIST.contexts, credits: PLAYLIST.contexts.menu }),
      /playlist \(dwell\)\.contexts: "credits" is not one of menu, game/,
    ],
    [
      'a first wait whose max is below its min',
      (f) =>
        withContexts(f, { ...PLAYLIST.contexts, game: { firstDelayMinS: 60, firstDelayMaxS: 30 } }),
      /playlist \(dwell\)\.contexts\.game\.firstDelayMaxS: 30 is below firstDelayMinS 60/,
    ],
    [
      'a negative first wait',
      (f) =>
        withContexts(f, { ...PLAYLIST.contexts, game: { firstDelayMinS: -1, firstDelayMaxS: 30 } }),
      /playlist \(dwell\)\.contexts\.game\.firstDelayMinS: -1 is outside \[0, 3600\]/,
    ],
    [
      'a first wait missing a field',
      (f) => withContexts(f, { ...PLAYLIST.contexts, menu: { firstDelayMinS: 0 } }),
      /playlist \(dwell\)\.contexts\.menu: missing "firstDelayMaxS"/,
    ],
    [
      'an unknown time of day',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', times: ['dusk'] }),
      /track dwell:x\.times: "dusk" is not one of day, night/,
    ],
    [
      'no times of day',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', times: [] }),
      /track dwell:x\.times: must be a non-empty list of day, night/,
    ],
    [
      'a time of day twice',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', times: ['day', 'day'] }),
      /track dwell:x\.times: lists a time twice/,
    ],
    [
      'an unknown field',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', tempo: 1 }),
      /track dwell:x: unknown field "tempo"/,
    ],
    [
      'a missing field',
      (f) => {
        f.tracks.x = { ...track, file: 'casual-theme-1.m4a' };
        delete f.tracks.x.author;
      },
      /track dwell:x: missing "author"/,
    ],
    [
      'an empty title',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', title: ' ' }),
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
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', volume: 1.5 }),
      /track dwell:x\.volume: 1\.5 is outside \[0, 1\]/,
    ],
    [
      'a loop that is not a flag',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', loop: 'yes' }),
      /track dwell:x\.loop: must be true or false/,
    ],
    [
      'a negative fade',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', fadeInS: -1 }),
      /track dwell:x\.fadeInS: -1 is outside/,
    ],
    [
      'an unknown condition',
      (f) => (f.tracks.x = { ...track, file: 'casual-theme-1.m4a', when: 'night' }),
      /track dwell:x\.when: "night" is not one of always/,
    ],
    [
      'a bad name',
      (f) => (f.tracks['Bad Name'] = { ...track, file: 'casual-theme-1.m4a' }),
      /track dwell:Bad Name: bad name/,
    ],
    ['no tracks', (f) => (f.tracks = {}), /shared\/music: tracks: none/],
    ['no playlist', (f) => delete f.playlist, /shared\/music: playlist: missing/],
    [
      'an unknown order',
      (f) => (f.playlist = { ...PLAYLIST, order: 'alphabetical' }),
      /playlist \(dwell\)\.order: "alphabetical" is not one of shuffle/,
    ],
    [
      'a gap whose max is below its min',
      (f) => (f.playlist = { ...PLAYLIST, gapMinS: 30, gapMaxS: 10 }),
      /playlist \(dwell\)\.gapMaxS: 10 is below gapMinS 30/,
    ],
    [
      'a negative cross-fade',
      (f) => (f.playlist = { ...PLAYLIST, crossfadeS: -1 }),
      /playlist \(dwell\)\.crossfadeS: -1 is outside/,
    ],
    [
      'an unknown playlist field',
      (f) => (f.playlist = { ...PLAYLIST, repeat: true }),
      /playlist \(dwell\): unknown field "repeat"/,
    ],
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
