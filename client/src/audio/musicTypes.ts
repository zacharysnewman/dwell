// The music data's shape (shared/music/*.json, resolved by shared/music/validate.mjs).

/** Where a track plays (validate.mjs `WHEN`). */
export type MusicWhen = 'always';

export interface TrackDef {
  /** `namespace:name`. */
  id: string;
  title: string;
  author: string;
  /** The file in shared/music/. */
  source: string;
  /** Its published name under the site's `media/` (`name-<content hash>.ext`). */
  file: string;
  /** 0–1, under the Music volume. */
  volume: number;
  loop: boolean;
  fadeInS: number;
  fadeOutS: number;
  when: MusicWhen;
  /** The times of day it may be drawn at (validate.mjs `TIMES`; the data's default both). */
  times: MusicTime[];
}

/** A time of day (validate.mjs `TIMES`). */
export type MusicTime = 'day' | 'night';

/** How the next track is picked (validate.mjs `ORDER`). */
export type MusicOrder = 'shuffle';

/** Where the playlist plays (validate.mjs `CONTEXTS`): the main menu's page, or the game's. */
export type MusicContext = 'menu' | 'game';

/** A context's wait before its first track, drawn in [firstDelayMinS, firstDelayMaxS] (s). */
export interface MusicContextDef {
  firstDelayMinS: number;
  firstDelayMaxS: number;
}

/** The playlist (shared/music/music.json `playlist`): the order and what lies between tracks. */
export interface PlaylistDef {
  order: MusicOrder;
  /** The silence after a track ends before the next starts: drawn in [gapMinS, gapMaxS] (s). */
  gapMinS: number;
  gapMaxS: number;
  /** The next track starts this much earlier (s): with a gap of 0 the two overlap and cross-fade. */
  crossfadeS: number;
  /** Daylight (0 night – 1 day, the ambience's `day`) at or above this is day. */
  dayAt: number;
  /** Each context's wait before the first track (none for a track carried over from the last page). */
  contexts: Record<MusicContext, MusicContextDef>;
}

export interface MusicData {
  playlist: PlaylistDef;
  /** As listed; the playlist plays those whose `when` holds. */
  tracks: TrackDef[];
}
