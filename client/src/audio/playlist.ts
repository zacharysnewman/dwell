// The music playlist (docs/AUDIO.md §6a): the tracks whose `when` holds, played one after another
// in the data's order (`shuffle`: a shuffled bag, never the same track twice in a row), a gap of
// silence drawn in [gapMinS, gapMaxS] after each, the next starting `crossfadeS` early so the two
// overlap. Each track is its own MusicPlayer (its element, fades, unlock and routing); this only
// decides which plays next and when.
import type { Volumes } from './mixer';
import {
  MUSIC_POSITION_KEY,
  MusicPlayer,
  POSITION_MAX_AGE_S,
  type MusicEnv,
  type MusicHost,
  type MusicState,
} from './music';
import type { MusicData, MusicWhen, PlaylistDef, TrackDef } from './musicTypes';

/** How often the playlist checks the playing track's time left (ms). */
const POLL_MS = 250;

/**
 * A shuffled bag of track ids: each plays once per round, in a random order, and a round never
 * starts with the track the last one ended on (no repeat back to back). `random` is [0, 1).
 */
export class Shuffler {
  private bag: string[] = [];
  private last: string | null = null;

  constructor(
    private readonly ids: readonly string[],
    private readonly random: () => number,
  ) {}

  /** Makes `id` the last played (a track resumed from the previous page). */
  played(id: string): void {
    this.last = id;
    this.bag = this.bag.filter((x) => x !== id);
  }

  next(): string | null {
    if (this.ids.length === 0) return null;
    if (this.bag.length === 0) {
      const bag = [...this.ids];
      const swap = (a: number, b: number): void => {
        const t = bag[a] as string;
        bag[a] = bag[b] as string;
        bag[b] = t;
      };
      for (let i = bag.length - 1; i > 0; i--) swap(i, Math.floor(this.random() * (i + 1)));
      // The round's first (popped from the end) must differ from the last round's end.
      if (bag.length > 1 && bag[bag.length - 1] === this.last) {
        swap(bag.length - 1, Math.floor(this.random() * (bag.length - 1)));
      }
      this.bag = bag;
    }
    const id = this.bag.pop() ?? null;
    this.last = id;
    return id;
  }
}

/** The gap after a track (s), drawn in [gapMinS, gapMaxS]. */
export function drawGap(p: PlaylistDef, random: () => number): number {
  return p.gapMinS + (p.gapMaxS - p.gapMinS) * random();
}

/**
 * When the next track starts, in seconds after the playing one ends: its gap less the cross-fade
 * (negative: before the end, the two overlapping).
 */
export function nextStartAfterEnd(p: PlaylistDef, gap: number): number {
  return gap - p.crossfadeS;
}

/** The tracks the playlist plays: those whose `when` holds. */
export function playable(data: MusicData, holding: readonly MusicWhen[] = ['always']): TrackDef[] {
  return data.tracks.filter((t) => holding.includes(t.when));
}

/** The track the previous page was playing, if it saved one recently (the menu → the game). */
export function resumedTrack(
  env: Pick<MusicEnv, 'session' | 'wall'>,
  ids: readonly string[],
): string | null {
  let saved: unknown;
  try {
    saved = JSON.parse(env.session?.getItem(MUSIC_POSITION_KEY) ?? 'null');
  } catch {
    return null;
  }
  if (typeof saved !== 'object' || saved === null) return null;
  const s = saved as { id?: unknown; at?: unknown };
  if (typeof s.id !== 'string' || typeof s.at !== 'number' || !ids.includes(s.id)) return null;
  const age = (env.wall() - s.at) / 1000;
  return age >= 0 && age <= POSITION_MAX_AGE_S ? s.id : null;
}

export class MusicPlaylist {
  private readonly byId = new Map<string, TrackDef>();
  private readonly shuffler: Shuffler;
  private current: MusicPlayer | null = null;
  /** A track still fading out under the next (a cross-fade). */
  private previous: MusicPlayer | null = null;
  private volumes: Pick<Volumes, 'master' | 'music'> = { master: 1, music: 0 };
  /** The env clock's time the next track starts at, once the playing one's end is known. */
  private nextAt: number | null = null;
  private gap = 0;
  private timer: unknown = null;
  private started = false;
  /** Tracks started, in order (the debug hook and the tests). */
  readonly history: string[] = [];

  constructor(
    data: MusicData,
    private readonly url: (t: TrackDef) => string,
    private readonly host: MusicHost,
    private readonly env: MusicEnv,
    private readonly random: () => number = Math.random,
    holding: readonly MusicWhen[] = ['always'],
  ) {
    this.playlist = data.playlist;
    for (const t of playable(data, holding)) this.byId.set(t.id, t);
    this.shuffler = new Shuffler([...this.byId.keys()], random);
  }

  private readonly playlist: PlaylistDef;

  get track(): TrackDef | null {
    return this.current?.track ?? null;
  }

  get state(): MusicState | 'between' {
    if (!this.current) return 'idle';
    return this.current.state === 'stopped' && this.nextAt !== null
      ? 'between'
      : this.current.state;
  }

  get routed(): boolean {
    return this.current?.routed ?? false;
  }

  get time(): number {
    return this.current?.time ?? 0;
  }

  get output(): number {
    return this.current?.output ?? 0;
  }

  get plays(): number {
    return this.current?.plays ?? 0;
  }

  setVolumes(v: Pick<Volumes, 'master' | 'music'>): void {
    this.volumes = v;
    this.current?.setVolumes(v);
    this.previous?.setVolumes(v);
  }

  /** Starts the first track (the previous page's, if it saved one), then keeps the list going. */
  start(): void {
    if (this.started || this.byId.size === 0) return;
    this.started = true;
    const resumed = resumedTrack(this.env, [...this.byId.keys()]);
    if (resumed) this.shuffler.played(resumed);
    this.play(resumed ?? this.shuffler.next());
    this.timer = this.env.setInterval(() => {
      this.tick();
    }, POLL_MS);
  }

  private play(id: string | null): void {
    const t = id === null ? undefined : this.byId.get(id);
    if (!t) return;
    this.previous = this.current;
    const p = new MusicPlayer(t, this.url(t), this.host, this.env);
    p.setVolumes(this.volumes);
    p.start();
    this.current = p;
    this.nextAt = null;
    this.gap = drawGap(this.playlist, this.random);
    this.history.push(t.id);
  }

  /** Every POLL_MS: schedules the next track once the playing one's end is known, starts it on time. */
  private tick(): void {
    const p = this.current;
    if (!p || this.env.doc.visibilityState === 'hidden') return;
    if (p.state !== 'playing' && p.state !== 'stopped') return;
    if (this.nextAt === null) {
      const left = p.state === 'stopped' ? 0 : p.duration - p.time;
      if (!Number.isFinite(left)) return;
      const lead = nextStartAfterEnd(this.playlist, this.gap);
      // Known once the remaining time is within the lead (a cross-fade), or the track ended.
      if (p.state === 'stopped' || left <= -lead) this.nextAt = this.env.now() + left + lead;
    }
    if (this.nextAt !== null && this.env.now() >= this.nextAt) {
      this.play(this.shuffler.next());
    }
    if (this.previous && this.previous.state === 'stopped') this.previous = null;
  }

  /** Stops the music (fades out) and the list. */
  stop(): void {
    if (this.timer !== null) this.env.clearInterval(this.timer);
    this.timer = null;
    this.current?.stop();
    this.previous?.stop();
  }
}
