// A page's sound (docs/AUDIO.md §6): the engine started on the first gesture with the kept
// volumes, the menus' delegated UI sounds, and the music (§6a). Once per page, whoever asks first
// (the game page, or the main menu, which the launcher shows without WASM).
import { audio, type AudioEngine } from './engine';
import { browserMusicEnv, musicEnabled } from './music';
import { MUSIC_DATA } from './music.gen';
import { MusicPlaylist } from './playlist';
import { loadVolumes } from './settings';
import { installUiSounds } from './uiSounds';

let installed = false;

export function installPageSounds(doc: Document = document): AudioEngine {
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
  const music = installMusic(sfx, doc);
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

/** The music, from the first screen (the menu, the loading screen, the game): off for tests. */
function installMusic(sfx: AudioEngine, doc: Document): MusicPlaylist | null {
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
  );
  sfx.onVolumes((v) => {
    player.setVolumes(v);
  });
  player.start();
  return player;
}
