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
}

/** How the next track is picked (validate.mjs `ORDER`). */
export type MusicOrder = 'shuffle';

/** The playlist (shared/music/music.json `playlist`): the order and what lies between tracks. */
export interface PlaylistDef {
  order: MusicOrder;
  /** The silence after a track ends before the next starts: drawn in [gapMinS, gapMaxS] (s). */
  gapMinS: number;
  gapMaxS: number;
  /** The next track starts this much earlier (s): with a gap of 0 the two overlap and cross-fade. */
  crossfadeS: number;
}

export interface MusicData {
  playlist: PlaylistDef;
  /** As listed; the playlist plays those whose `when` holds. */
  tracks: TrackDef[];
}
