// A page's sound (docs/AUDIO.md §6): the engine started on the first gesture with the kept
// volumes, the menus' delegated UI sounds, and the music (§6a). Once per page, whoever asks first
// (the game page, or the main menu, which the launcher shows without WASM).
import { audio, type AudioEngine } from './engine';
import { browserMusicEnv, musicEnabled } from './music';
import { MUSIC_DATA } from './music.gen';
import type { MusicContext } from './musicTypes';
import { MusicPlaylist } from './playlist';
import { loadVolumes } from './settings';
import { installUiSounds } from './uiSounds';

let installed = false;
let playlist: MusicPlaylist | null = null;

/**
 * `context`: the main menu's page (music at once) or the game's (the first track after a random
 * wait, unless the menu's carries over): `shared/music/music.json` `playlist.contexts`.
 */
export function installPageSounds(context: MusicContext, doc: Document = document): AudioEngine {
  const sfx = audio();
  if (installed) return sfx;
  installed = true;
  sfx.install(doc);
  sfx.setVolumes(loadVolumes());
  installUiSounds(doc, (kind) => {
    sfx.ui(kind);
  });
  // For tests and the console, on any page with sound (the menu has no `window.__dwell`).
  const w = window as unknown as {
    __dwellSounds?: () => string[];
    __dwellMusic?: () => Record<string, unknown> | null;
  };
  w.__dwellSounds = () => [...sfx.recent];
  const music = installMusic(sfx, doc, context);
  playlist = music;
  w.__dwellMusic = () =>
    music
      ? {
          track: music.track?.title ?? null,
          history: [...music.history],
          state: music.state,
          routed: music.routed,
          time: music.time,
          output: music.output,
          plays: music.plays,
          context: sfx.running,
        }
      : null;
  return sfx;
}

/** The listener's daylight (0 night – 1 day) for the music's day and night tracks (§6a). */
export function setMusicDaylight(day: number): void {
  playlist?.setDaylight(day);
}

/** The music, from the first screen (the menu, the loading screen, the game): off for tests. */
function installMusic(
  sfx: AudioEngine,
  doc: Document,
  context: MusicContext,
): MusicPlaylist | null {
  let store: Storage | null;
  try {
    store = typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    store = null;
  }
  if (!musicEnabled(location.search, store)) return null;
  const player = new MusicPlaylist(
    MUSIC_DATA,
    (t) => `${__MEDIA_BASE__}${t.file}`,
    sfx,
    browserMusicEnv(doc),
    Math.random,
    ['always'],
    context,
  );
  sfx.onVolumes((v) => {
    player.setVolumes(v);
  });
  player.start();
  return player;
}
