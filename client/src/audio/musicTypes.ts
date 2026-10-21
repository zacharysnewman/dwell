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

export interface MusicData {
  /** In priority order: the first whose `when` matches plays. */
  tracks: TrackDef[];
}
