// The music playlist (docs/AUDIO.md §6a): the shuffled bag never plays a track twice in a row, the
// gap after each track is drawn in the data's range and respected, a cross-fade starts the next
// early, the previous page's track is picked up, and a fixture playlist works by data alone. Music at
// random times (owner, 2026-11-12): the game's first track waits its drawn delay, the menu's starts
// at once, and a draw takes only the tracks for the time of day.
import { describe, expect, it } from 'vitest';
import { MUSIC_POSITION_KEY, type MusicElement, type MusicEnv, type MusicHost } from './music';
import type { MusicContext, MusicData, MusicTime, PlaylistDef, TrackDef } from './musicTypes';
import {
  drawFirstDelay,
  drawGap,
  forTime,
  MusicPlaylist,
  nextStartAfterEnd,
  playable,
  resumedTrack,
  Shuffler,
  timeOfDay,
} from './playlist';

/** A seeded [0, 1) generator (mulberry32). */
function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const track = (
  name: string,
  duration = 10,
  times: MusicTime[] = ['day', 'night'],
): TrackDef & { duration: number } => ({
  id: `test:${name}`,
  title: name,
  author: 'Tester',
  source: `${name}.m4a`,
  file: `${name}-00000000.m4a`,
  volume: 1,
  loop: false,
  fadeInS: 0,
  fadeOutS: 0,
  when: 'always',
  times,
  duration,
});

/** An element whose time runs with the clock while it plays, ending at its duration. */
class Clip implements MusicElement {
  src = '';
  loop = false;
  volume = 1;
  preload = '';
  currentTime = 0;
  duration = NaN;
  paused = true;
  private readonly listeners = new Map<string, (() => void)[]>();
  play(): Promise<void> {
    this.paused = false;
    return Promise.resolve();
  }
  pause(): void {
    this.paused = true;
  }
  addEventListener(type: string, fn: () => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }
  emit(type: string): void {
    for (const fn of this.listeners.get(type) ?? []) fn();
  }
  step(dt: number): void {
    if (this.paused || !Number.isFinite(this.duration)) return;
    this.currentTime = Math.min(this.duration, this.currentTime + dt);
    if (this.currentTime >= this.duration) {
      this.paused = true;
      this.emit('ended');
    }
  }
}

const host: MusicHost = {
  unlock: () => undefined,
  running: false,
  onRunning: () => undefined,
  connectMusic: () => null,
};

function rig(
  data: MusicData,
  o: { seed?: number; session?: Record<string, string>; context?: MusicContext } = {},
) {
  const durations = new Map(
    data.tracks.map((t) => [t.file, (t as { duration?: number }).duration ?? 10]),
  );
  const clips: Clip[] = [];
  const clock = { now: 0, wall: 5_000_000 };
  const timers = new Map<number, () => void>();
  let next = 1;
  const session = { ...(o.session ?? {}) };
  const doc = {
    visibilityState: 'visible' as DocumentVisibilityState,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  };
  const env: MusicEnv = {
    doc: doc,
    makeElement: () => {
      const c = new Clip();
      clips.push(c);
      return c;
    },
    now: () => clock.now,
    wall: () => clock.wall,
    session: {
      getItem: (k) => session[k] ?? null,
      setItem: (k, v) => {
        session[k] = v;
      },
    },
    setInterval: (fn) => {
      timers.set(next, fn);
      return next++;
    },
    clearInterval: (id) => {
      timers.delete(id as number);
    },
  };
  const list = new MusicPlaylist(
    data,
    (t) => `/m/${t.file}`,
    host,
    env,
    seeded(o.seed ?? 1),
    ['always'],
    o.context ?? 'game',
  );
  /** When each track of `list.history` started (the env clock, s). */
  const starts: number[] = [];
  list.setVolumes({ master: 1, music: 1 });
  /** Starts each new clip's metadata (its duration from the file name) and runs time forward. */
  const advance = async (s: number, dt = 0.05) => {
    for (let i = 0; i < Math.round(s / dt); i++) {
      for (const c of clips) {
        if (!Number.isFinite(c.duration)) {
          c.duration = durations.get(c.src.slice(3)) ?? 10;
          c.emit('loadedmetadata');
        }
        c.step(dt);
      }
      clock.now += dt;
      for (const fn of [...timers.values()]) fn();
      while (starts.length < list.history.length) starts.push(clock.now);
      await Promise.resolve();
    }
  };
  return { list, clips, clock, advance, session, starts };
}

const playlist = (p: Partial<PlaylistDef> = {}): PlaylistDef => ({
  order: 'shuffle',
  gapMinS: 0,
  gapMaxS: 0,
  crossfadeS: 0,
  dayAt: 0.1,
  contexts: {
    menu: { firstDelayMinS: 0, firstDelayMaxS: 0 },
    game: { firstDelayMinS: 0, firstDelayMaxS: 0 },
  },
  ...p,
});

/** A playlist whose game waits [min, max] before its first track; the menu none. */
const waiting = (min: number, max: number, p: Partial<PlaylistDef> = {}): PlaylistDef =>
  playlist({
    contexts: {
      menu: { firstDelayMinS: 0, firstDelayMaxS: 0 },
      game: { firstDelayMinS: min, firstDelayMaxS: max },
    },
    ...p,
  });

describe('shuffler', () => {
  it('never plays the same track twice in a row, over many rounds and seeds', () => {
    for (let seed = 1; seed <= 50; seed++) {
      for (const n of [2, 3, 5, 20]) {
        const ids = Array.from({ length: n }, (_, i) => `t${String(i)}`);
        const s = new Shuffler(ids, seeded(seed));
        let last: string | null = null;
        for (let i = 0; i < n * 10; i++) {
          const id = s.next();
          expect(id).not.toBe(last);
          last = id;
        }
      }
    }
  });

  it('plays every track once per round', () => {
    const ids = ['a', 'b', 'c', 'd', 'e'];
    const s = new Shuffler(ids, seeded(7));
    for (let round = 0; round < 5; round++) {
      const got = ids.map(() => s.next());
      expect([...got].sort()).toEqual(ids);
    }
  });

  it('does not open with the track resumed from the previous page', () => {
    for (let seed = 1; seed <= 30; seed++) {
      const s = new Shuffler(['a', 'b', 'c'], seeded(seed));
      s.played('b');
      expect(s.next()).not.toBe('b');
    }
  });

  it('plays the only track again (nothing else to pick) and nothing from an empty list', () => {
    const one = new Shuffler(['a'], seeded(1));
    expect([one.next(), one.next()]).toEqual(['a', 'a']);
    expect(new Shuffler([], seeded(1)).next()).toBeNull();
  });
});

describe('gaps and cross-fades', () => {
  it('draws gaps inside the range', () => {
    const p = playlist({ gapMinS: 20, gapMaxS: 60 });
    const r = seeded(3);
    for (let i = 0; i < 500; i++) {
      const g = drawGap(p, r);
      expect(g).toBeGreaterThanOrEqual(20);
      expect(g).toBeLessThanOrEqual(60);
    }
    expect(drawGap(playlist({ gapMinS: 5, gapMaxS: 5 }), r)).toBe(5);
  });

  it('starts the next track the gap after the end, less the cross-fade', () => {
    expect(nextStartAfterEnd(playlist({ crossfadeS: 0 }), 30)).toBe(30);
    expect(nextStartAfterEnd(playlist({ crossfadeS: 4 }), 0)).toBe(-4);
  });

  it('waits out the gap in silence before the next track, and never repeats', async () => {
    const data: MusicData = {
      playlist: playlist({ gapMinS: 3, gapMaxS: 3 }),
      tracks: [track('a', 5), track('b', 5), track('c', 5)],
    };
    const { list, advance } = rig(data);
    list.start();
    await advance(5.2);
    // Track 1 ended at 5 s: silence until 8 s.
    expect(list.history).toHaveLength(1);
    expect(list.state).toBe('between');
    await advance(2.6);
    expect(list.history).toHaveLength(1);
    await advance(0.4);
    expect(list.history).toHaveLength(2);
    await advance(80);
    expect(list.history.length).toBeGreaterThan(8);
    for (let i = 1; i < list.history.length; i++) {
      expect(list.history[i]).not.toBe(list.history[i - 1]);
    }
  });

  it('starts the next track before the end with a cross-fade, the two overlapping', async () => {
    const data: MusicData = {
      playlist: playlist({ crossfadeS: 2 }),
      tracks: [track('a', 6), track('b', 6)],
    };
    const { list, clips, advance } = rig(data);
    list.start();
    await advance(3.8);
    expect(list.history).toHaveLength(1);
    await advance(0.4);
    // 2 s before the first's end the second starts while the first still plays.
    expect(list.history).toHaveLength(2);
    expect(clips[0]?.paused).toBe(false);
    expect(clips[1]?.paused).toBe(false);
  });
});

describe('playlist', () => {
  it('takes a fixture playlist by data alone, playing only what its `when` allows', () => {
    const data: MusicData = {
      playlist: playlist({ gapMinS: 1, gapMaxS: 2 }),
      tracks: [track('x'), track('y')],
    };
    expect(playable(data).map((t) => t.id)).toEqual(['test:x', 'test:y']);
    expect(playable(data, [])).toEqual([]);
    const { list } = rig(data);
    list.start();
    expect(list.history).toHaveLength(1);
    expect(['test:x', 'test:y']).toContain(list.track?.id);
  });

  it('picks up the track the previous page was playing', () => {
    const data: MusicData = { playlist: playlist(), tracks: [track('a'), track('b'), track('c')] };
    const saved = {
      [MUSIC_POSITION_KEY]: JSON.stringify({ id: 'test:c', t: 3, at: 5_000_000 - 2000 }),
    };
    const { list } = rig(data, { session: saved });
    list.start();
    expect(list.track?.id).toBe('test:c');
  });

  it('ignores a stale or unknown saved track', () => {
    const env = (v: unknown) => ({
      wall: () => 5_000_000,
      session: { getItem: () => JSON.stringify(v), setItem: () => undefined },
    });
    expect(resumedTrack(env({ id: 'test:a', t: 1, at: 5_000_000 - 1000 }), ['test:a'])).toBe(
      'test:a',
    );
    expect(
      resumedTrack(env({ id: 'test:a', t: 1, at: 5_000_000 - 60_000 }), ['test:a']),
    ).toBeNull();
    expect(resumedTrack(env({ id: 'test:z', t: 1, at: 5_000_000 }), ['test:a'])).toBeNull();
    expect(resumedTrack(env(null), ['test:a'])).toBeNull();
  });
});

describe('random times', () => {
  it('draws the first wait inside its context range', () => {
    const p = waiting(30, 180);
    const r = seeded(9);
    for (let i = 0; i < 500; i++) {
      const d = drawFirstDelay(p, 'game', r);
      expect(d).toBeGreaterThanOrEqual(30);
      expect(d).toBeLessThanOrEqual(180);
    }
    expect(drawFirstDelay(p, 'menu', r)).toBe(0);
  });

  it('in game, waits a random [firstDelayMin, firstDelayMax] before the first track', async () => {
    const data: MusicData = { playlist: waiting(30, 180), tracks: [track('a'), track('b')] };
    const at: number[] = [];
    for (let seed = 1; seed <= 6; seed++) {
      const { list, advance, starts } = rig(data, { seed });
      list.start();
      expect(list.history).toHaveLength(0);
      expect(list.state).toBe('between');
      await advance(29.9, 0.1);
      expect(list.history).toHaveLength(0);
      await advance(151, 0.1);
      expect(list.history.length).toBeGreaterThanOrEqual(1);
      const first = starts[0] as number;
      expect(first).toBeGreaterThanOrEqual(30);
      expect(first).toBeLessThanOrEqual(180.2);
      at.push(first);
    }
    // Random, not a fixed wait.
    expect(new Set(at.map((t) => Math.round(t))).size).toBeGreaterThan(1);
  });

  it('on the menu, starts the first track at once', () => {
    const data: MusicData = { playlist: waiting(30, 180), tracks: [track('a'), track('b')] };
    const { list } = rig(data, { context: 'menu' });
    list.start();
    expect(list.history).toHaveLength(1);
  });

  it('in game, plays on the track carried over from the menu without a wait', () => {
    const data: MusicData = { playlist: waiting(30, 180), tracks: [track('a'), track('b')] };
    const saved = {
      [MUSIC_POSITION_KEY]: JSON.stringify({ id: 'test:b', t: 3, at: 5_000_000 - 2000 }),
    };
    const { list } = rig(data, { session: saved });
    list.start();
    expect(list.history).toEqual(['test:b']);
  });

  it('leaves a random [gapMin, gapMax] of silence between tracks (3–10 minutes)', async () => {
    const data: MusicData = {
      playlist: playlist({ gapMinS: 180, gapMaxS: 600 }),
      tracks: [track('a', 5), track('b', 5), track('c', 5)],
    };
    const { list, advance, starts } = rig(data, { seed: 4 });
    list.start();
    await advance(4000, 0.25);
    expect(list.history.length).toBeGreaterThanOrEqual(7);
    const gaps = starts.slice(1).map((t, i) => t - ((starts[i] as number) + 5));
    for (const g of gaps) {
      expect(g).toBeGreaterThanOrEqual(180 - 0.5);
      expect(g).toBeLessThanOrEqual(600 + 0.5);
    }
    expect(new Set(gaps.map((g) => Math.round(g))).size).toBeGreaterThan(1);
  });
});

describe('time of day', () => {
  const tracks = [
    track('day', 5, ['day']),
    track('night', 5, ['night']),
    track('both', 5, ['day', 'night']),
    track('night2', 5, ['night']),
  ];

  it('calls it day at or above the data threshold', () => {
    const p = playlist({ dayAt: 0.25 });
    expect(timeOfDay(p, 1)).toBe('day');
    expect(timeOfDay(p, 0.25)).toBe('day');
    expect(timeOfDay(p, 0.2)).toBe('night');
    expect(timeOfDay(p, 0)).toBe('night');
    expect(forTime(tracks, 'night')).toEqual(['test:night', 'test:both', 'test:night2']);
    expect(forTime(tracks, 'day')).toEqual(['test:day', 'test:both']);
  });

  it('draws only night or both at night, day or both by day, never twice in a row', () => {
    for (let seed = 1; seed <= 30; seed++) {
      const s = new Shuffler(
        tracks.map((t) => t.id),
        seeded(seed),
      );
      let last: string | null = null;
      for (let i = 0; i < 40; i++) {
        const time: MusicTime = Math.floor(i / 7) % 2 === 0 ? 'night' : 'day';
        const id = s.next(forTime(tracks, time));
        expect(id).not.toBeNull();
        expect(tracks.find((t) => t.id === id)?.times).toContain(time);
        // The only day-and-night track may follow itself when the time changes; nothing else does.
        if (id !== 'test:both') expect(id).not.toBe(last);
        last = id;
      }
    }
  });

  it('plays only night tracks at night by data alone', async () => {
    const data: MusicData = { playlist: playlist({ gapMinS: 1, gapMaxS: 1 }), tracks };
    const { list, advance } = rig(data, { seed: 3 });
    list.setDaylight(0);
    list.start();
    await advance(30);
    expect(list.history.length).toBeGreaterThan(3);
    for (const id of list.history) expect(['test:night', 'test:both', 'test:night2']).toContain(id);
  });

  it('lets a night track finish when day comes, then draws a day track', async () => {
    const data: MusicData = { playlist: playlist({ gapMinS: 1, gapMaxS: 1 }), tracks };
    const { list, clips, advance } = rig(data, { seed: 5 });
    list.setDaylight(0);
    list.start();
    await advance(1);
    const first = list.history[0];
    expect(['test:night', 'test:both', 'test:night2']).toContain(first);
    list.setDaylight(1);
    await advance(3);
    // Day came at 1 s: the 5 s track plays on, nothing new started.
    expect(list.history).toEqual([first]);
    expect(clips[0]?.paused).toBe(false);
    await advance(30);
    expect(list.history.length).toBeGreaterThan(3);
    for (const id of list.history.slice(1)) expect(['test:day', 'test:both']).toContain(id);
  });

  it('waits when no track fits the time, and plays once one does', async () => {
    const data: MusicData = {
      playlist: playlist({ gapMinS: 2, gapMaxS: 2 }),
      tracks: [track('day', 5, ['day'])],
    };
    const { list, advance } = rig(data);
    list.setDaylight(0);
    list.start();
    expect(list.history).toHaveLength(0);
    await advance(10);
    expect(list.history).toHaveLength(0);
    list.setDaylight(1);
    await advance(2.2);
    expect(list.history).toEqual(['test:day']);
  });

  it('the menu plays day tracks (it never sets the daylight)', () => {
    const data: MusicData = {
      playlist: playlist(),
      tracks: [track('night', 5, ['night']), track('day', 5, ['day'])],
    };
    const { list } = rig(data, { context: 'menu' });
    list.start();
    expect(list.history).toEqual(['test:day']);
  });
});
