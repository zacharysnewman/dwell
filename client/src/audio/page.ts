// A page's sound (docs/AUDIO.md §6): the engine started on the first gesture with the kept
// volumes, and the menus' delegated UI sounds. Once per page, whoever asks first (the game page,
// or the main menu, which the launcher shows without WASM).
import { audio, type AudioEngine } from './engine';
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
  (window as unknown as { __dwellSounds?: () => string[] }).__dwellSounds = () => [...sfx.recent];
  return sfx;
}
