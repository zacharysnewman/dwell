// The music playlist (docs/AUDIO.md §6a): the shuffled bag never plays a track twice in a row, the
// gap after each track is drawn in the data's range and respected, a cross-fade starts the next
// early, the previous page's track is picked up, and a fixture playlist works by data alone.
import { describe, expect, it } from 'vitest';
import { MUSIC_POSITION_KEY, type MusicElement, type MusicEnv, type MusicHost } from './music';
import type { MusicData, PlaylistDef, TrackDef } from './musicTypes';
import {
  drawGap,
  MusicPlaylist,
  nextStartAfterEnd,
  playable,
  resumedTrack,
  Shuffler,
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

const track = (name: string, duration = 10): TrackDef & { duration: number } => ({
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

function rig(data: MusicData, o: { seed?: number; session?: Record<string, string> } = {}) {
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
  const list = new MusicPlaylist(data, (t) => `/m/${t.file}`, host, env, seeded(o.seed ?? 1));
  list.setVolumes({ master: 1, music: 1 });
  /** Starts each new clip's metadata (its duration from the file name) and runs time forward. */
  const advance = async (s: number) => {
    for (let i = 0; i < Math.round(s / 0.05); i++) {
      for (const c of clips) {
        if (!Number.isFinite(c.duration)) {
          c.duration = durations.get(c.src.slice(3)) ?? 10;
          c.emit('loadedmetadata');
        }
        c.step(0.05);
      }
      clock.now += 0.05;
      for (const fn of [...timers.values()]) fn();
      await Promise.resolve();
    }
  };
  return { list, clips, clock, advance, session };
}

const playlist = (p: Partial<PlaylistDef> = {}): PlaylistDef => ({
  order: 'shuffle',
  gapMinS: 0,
  gapMaxS: 0,
  crossfadeS: 0,
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
