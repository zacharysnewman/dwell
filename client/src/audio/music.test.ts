// The music player (docs/AUDIO.md §6a) with a mocked element, document and engine: the autoplay
// unlock (blocked, then the first gesture starts it, once), the tab hiding and returning, volume 0,
// the routing into the mixer, the fades and the position carried from the previous page.
import { describe, expect, it } from 'vitest';
import {
  chooseTrack,
  GESTURES,
  MUSIC_KEY,
  MUSIC_POSITION_KEY,
  musicEnabled,
  MusicPlayer,
  type MusicElement,
  type MusicEnv,
  type MusicHost,
} from './music';
import type { TrackDef } from './musicTypes';

const TRACK: TrackDef = {
  id: 'test:song',
  title: 'Song',
  author: 'Tester',
  source: 'song.mp3',
  file: 'song-00000000.mp3',
  volume: 0.5,
  loop: true,
  fadeInS: 4,
  fadeOutS: 2,
  when: 'always',
};

/** An element whose `play()` the browser allows or refuses (`allow`). */
class FakeElement implements MusicElement {
  src = '';
  loop = false;
  volume = 1;
  preload = '';
  currentTime = 0;
  duration = 200;
  paused = true;
  allow = false;
  playCalls = 0;
  private readonly listeners = new Map<string, (() => void)[]>();
  play(): Promise<void> {
    this.playCalls++;
    if (!this.allow) return Promise.reject(new DOMException('blocked', 'NotAllowedError'));
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
}

class FakeDoc {
  visibilityState: DocumentVisibilityState = 'visible';
  private readonly listeners = new Map<string, Set<() => void>>();
  addEventListener(type: string, fn: () => void): void {
    const set = this.listeners.get(type) ?? new Set();
    set.add(fn);
    this.listeners.set(type, set);
  }
  removeEventListener(type: string, fn: () => void): void {
    this.listeners.get(type)?.delete(fn);
  }
  count(type: string): number {
    return this.listeners.get(type)?.size ?? 0;
  }
  emit(type: string): void {
    for (const fn of [...(this.listeners.get(type) ?? [])]) fn();
  }
  hide(hidden: boolean): void {
    this.visibilityState = hidden ? 'hidden' : 'visible';
    this.emit('visibilitychange');
  }
}

class FakeHost implements MusicHost {
  running = false;
  unlocks = 0;
  connected = 0;
  levels: number[] = [];
  private readonly fns: (() => void)[] = [];
  unlock(): void {
    this.unlocks++;
  }
  onRunning(fn: () => void): void {
    this.fns.push(fn);
    if (this.running) fn();
  }
  start(): void {
    this.running = true;
    for (const fn of this.fns) fn();
  }
  connectMusic(): (level: number) => void {
    this.connected++;
    return (v) => this.levels.push(v);
  }
}

function memory(initial: Record<string, string> = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k: string) => data[k] ?? null,
    setItem: (k: string, v: string) => {
      data[k] = v;
    },
  };
}

function setup(o: { allow?: boolean; track?: TrackDef; session?: ReturnType<typeof memory> } = {}) {
  const el = new FakeElement();
  el.allow = o.allow ?? false;
  const doc = new FakeDoc();
  const host = new FakeHost();
  const clock = { now: 0, wall: 1_000_000 };
  const timers = new Map<number, () => void>();
  let next = 1;
  const env: MusicEnv = {
    doc: doc as unknown as MusicEnv['doc'],
    makeElement: () => el,
    now: () => clock.now,
    wall: () => clock.wall,
    session: o.session ?? memory(),
    setInterval: (fn) => {
      timers.set(next, fn);
      return next++;
    },
    clearInterval: (id) => {
      timers.delete(id as number);
    },
  };
  const player = new MusicPlayer(o.track ?? TRACK, '/dwell/media/song-00000000.mp3', host, env);
  player.setVolumes({ master: 1, music: 1 });
  /** Advances the clock by `s` seconds, running the fade's timers at each 50 ms step. */
  const advance = (s: number) => {
    for (let i = 0; i < Math.round(s / 0.05); i++) {
      clock.now += 0.05;
      for (const fn of [...timers.values()]) fn();
    }
  };
  return { el, doc, host, player, clock, advance, timers };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe('music: autoplay', () => {
  it('starts at once when the browser allows it, fading in', async () => {
    const { el, player, advance } = setup({ allow: true });
    player.start();
    await settle();
    expect(el.src).toBe('/dwell/media/song-00000000.mp3');
    expect(el.loop).toBe(true);
    expect(player.state).toBe('playing');
    // Straight out of the element (no context yet), from silence to the track's volume.
    expect(el.volume).toBe(0);
    advance(2);
    expect(el.volume).toBeCloseTo(0.25, 2);
    advance(2);
    expect(el.volume).toBeCloseTo(0.5, 2);
  });

  it('when blocked, waits for the first gesture and starts inside it, once', async () => {
    const { el, doc, host, player } = setup();
    player.start();
    await settle();
    expect(player.state).toBe('blocked');
    expect(el.playCalls).toBe(1);
    for (const g of GESTURES) expect(doc.count(g), g).toBe(1);
    // The gesture: the engine unlocks, the element is routed and plays.
    el.allow = true;
    doc.emit('pointerdown');
    expect(host.unlocks).toBe(1);
    expect(host.connected).toBe(1);
    expect(el.playCalls).toBe(2);
    await settle();
    expect(player.state).toBe('playing');
    expect(player.plays).toBe(1);
    // Once: the listeners are gone, later gestures start nothing.
    for (const g of GESTURES) expect(doc.count(g), g).toBe(0);
    doc.emit('keydown');
    doc.emit('pointerdown');
    await settle();
    expect(el.playCalls).toBe(2);
    expect(player.plays).toBe(1);
  });

  it('keeps waiting when a gesture does not count (a play still refused)', async () => {
    const { el, doc, player } = setup();
    player.start();
    await settle();
    doc.emit('keydown'); // e.g. Escape: no user activation
    await settle();
    expect(player.state).toBe('blocked');
    expect(doc.count('keydown')).toBe(1);
    el.allow = true;
    doc.emit('touchend');
    await settle();
    expect(player.state).toBe('playing');
    expect(doc.count('touchend')).toBe(0);
  });

  it('moves into the mixer when the context starts running', async () => {
    const { el, host, player, advance } = setup({ allow: true });
    player.start();
    await settle();
    advance(4);
    expect(player.routed).toBe(false);
    host.start();
    expect(player.routed).toBe(true);
    expect(el.volume).toBe(1); // the gain does the level now
    expect(host.levels.at(-1)).toBeCloseTo(0.5, 2);
  });

  it('is off for ?music=off or the dev store', () => {
    expect(musicEnabled('', null)).toBe(true);
    expect(musicEnabled('?music=off', null)).toBe(false);
    expect(musicEnabled('?world=flat&music=off', null)).toBe(false);
    expect(musicEnabled('?music=on', null)).toBe(true);
    expect(musicEnabled('', memory({ [MUSIC_KEY]: '"off"' }))).toBe(false);
    expect(musicEnabled('', memory({ [MUSIC_KEY]: 'off' }))).toBe(false);
    expect(musicEnabled('', memory({ [MUSIC_KEY]: '"on"' }))).toBe(true);
    const throwing = {
      getItem: () => {
        throw new Error('blocked');
      },
    };
    expect(musicEnabled('', throwing)).toBe(true);
  });

  it('picks the first track that plays everywhere', () => {
    const other = { ...TRACK, id: 'test:other' };
    expect(chooseTrack({ tracks: [TRACK, other] })?.id).toBe('test:song');
    expect(chooseTrack({ tracks: [] })).toBeNull();
  });
});

describe('music: hidden tab, volume, fades', () => {
  it('pauses while the tab is hidden and fades back in on return', async () => {
    const { el, doc, player, advance } = setup({ allow: true });
    player.start();
    await settle();
    advance(4);
    doc.hide(true);
    expect(el.paused).toBe(true);
    expect(player.state).toBe('paused');
    doc.hide(false);
    await settle();
    expect(el.paused).toBe(false);
    expect(player.state).toBe('playing');
    // A quick fade back in (RESUME_FADE_S), not the track's whole fade-in.
    expect(el.volume).toBe(0);
    advance(1);
    expect(el.volume).toBeCloseTo(0.5, 2);
  });

  it('does not start while hidden, and a blocked start retries on return', async () => {
    const { el, doc, player } = setup();
    player.start();
    await settle();
    doc.hide(true);
    el.allow = true;
    doc.hide(false);
    await settle();
    expect(player.state).toBe('playing');
  });

  it('is paused at volume 0 and plays again when raised', async () => {
    const { el, player, advance } = setup({ allow: true });
    player.start();
    await settle();
    advance(4);
    player.setVolumes({ master: 1, music: 0 });
    expect(el.paused).toBe(true);
    expect(player.state).toBe('muted');
    player.setVolumes({ master: 1, music: 0.5 });
    await settle();
    expect(player.state).toBe('playing');
    advance(1);
    expect(el.volume).toBeCloseTo(0.25, 2);
    // The master volume scales the element while it plays straight out.
    player.setVolumes({ master: 0.5, music: 0.5 });
    expect(el.volume).toBeCloseTo(0.125, 3);
  });

  it('starting at volume 0 loads nothing into play until raised', async () => {
    const { el, player } = setup({ allow: true });
    player.setVolumes({ master: 1, music: 0 });
    player.start();
    await settle();
    expect(el.playCalls).toBe(0);
    expect(player.state).toBe('muted');
    player.setVolumes({ master: 1, music: 1 });
    await settle();
    expect(player.state).toBe('playing');
  });

  it('fades out on stop, then pauses', async () => {
    const { el, player, advance } = setup({ allow: true });
    player.start();
    await settle();
    advance(4);
    player.stop();
    advance(1);
    expect(el.volume).toBeCloseTo(0.25, 2);
    advance(1.1);
    expect(el.volume).toBe(0);
    expect(el.paused).toBe(true);
  });

  it('fades out over the end of a track that does not loop', async () => {
    const { el, player, advance } = setup({ allow: true, track: { ...TRACK, loop: false } });
    player.start();
    await settle();
    advance(4);
    el.currentTime = 199; // 1 s left of 2 s fade-out
    el.emit('timeupdate');
    expect(el.volume).toBeCloseTo(0.25, 2);
    el.emit('ended');
    expect(player.state).toBe('stopped');
  });
});

describe('music: across pages', () => {
  it('picks up where the previous page left it', async () => {
    const session = memory();
    const a = setup({ allow: true, session });
    a.player.start();
    await settle();
    a.el.currentTime = 42;
    a.doc.hide(true); // navigating away hides the page first
    const saved = JSON.parse(session.data[MUSIC_POSITION_KEY] ?? '{}') as { t: number };
    expect(saved.t).toBe(42);
    const b = setup({ allow: true, session });
    b.clock.wall = a.clock.wall + 3000;
    b.player.start();
    b.el.emit('loadedmetadata');
    expect(b.el.currentTime).toBe(45);
  });

  it('starts over after a while, or for another track, and wraps a loop', async () => {
    const session = memory({
      [MUSIC_POSITION_KEY]: JSON.stringify({ id: 'test:song', t: 199, at: 1_000_000 - 60_000 }),
    });
    const late = setup({ session });
    late.player.start();
    late.el.emit('loadedmetadata');
    expect(late.el.currentTime).toBe(0);
    session.data[MUSIC_POSITION_KEY] = JSON.stringify({ id: 'test:x', t: 10, at: 1_000_000 });
    const other = setup({ session });
    other.player.start();
    other.el.emit('loadedmetadata');
    expect(other.el.currentTime).toBe(0);
    session.data[MUSIC_POSITION_KEY] = JSON.stringify({ id: 'test:song', t: 199, at: 999_000 });
    const wrap = setup({ session });
    wrap.player.start();
    wrap.el.emit('loadedmetadata');
    expect(wrap.el.currentTime).toBe(0); // 199 + 1 s around a 200 s loop
    await settle();
  });
});
