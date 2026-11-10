// The main menu as a page (RELEASES.md §5): the launcher shows it at `/dwell/`; a build served at
// the launcher's path (a local or preview build, the e2e suite) shows it itself. DOM only, no WASM:
// pinging a server is passed in (the build's own client, or the latest build's `tools.js`).
import { WorldThumbs } from '../local/worldThumbs';
import { IndexedDbKeyStore, loadOrCreateDeviceKey } from '../identity/deviceKey';
import { WorldIndex, type WorldMeta } from '../local/worldIndex';
import { deleteWorldFiles, listWorldFiles } from '../local/worldFiles';
import { parseInvite } from '../net/invite';
import { configuredMasterUrl, MasterClient, serverInvite } from '../net/master';
import { MainMenu, type MainMenuDeps } from './mainMenu';
import { loadRecent } from './recentServers';
import { installPageSounds } from '../audio/page';
import { startMenuGamepad } from '../input/gamepad';
import { loadGamepadSettings, saveGamepadSettings } from '../input/gamepadSettings';
import { ControllerPanel } from './controllerPanel';
import { SettingsMenu } from './settingsMenu';
import { prefersTouch } from './touchControls';
import { audio } from '../audio/engine';

/** Most time a lobby ping may take, connecting included (§8.3). */
export const PING_TIMEOUT_MS = 5000;

/** A dedicated server's round trip, ms (the Rust client's `pingServer`). */
export type PingServer = (
  url: string,
  certHash: Uint8Array,
  rtc: (string | number)[] | undefined,
  timeoutMs: number,
) => Promise<number>;

export interface MenuPageOptions {
  /** Opens a route (`{ play: id }`, `{ new: '1', … }`, invite parameters; ui/launch.ts). */
  go: (route: Record<string, string>) => void;
  message?: string;
  version: string;
  channel: 'stable' | 'dev';
  protocol: number;
  detail: string;
  versionsHref: string;
  noticesHref: string;
  /** Pings dedicated servers in the server browser; without it they show no round trip. */
  ping?: () => Promise<PingServer>;
  canRegenerate?: (world: WorldMeta) => boolean;
}

/** Local storage, or null where it is blocked. */
function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * The master as the menu uses it: server addresses and games on this network (Phase 5d), and the
 * lobby list with its pings (Phase 5e).
 */
function menuMaster(ping: MenuPageOptions['ping']): MainMenuDeps['master'] {
  const base = configuredMasterUrl();
  if (!base) return undefined;
  const client = loadOrCreateDeviceKey(new IndexedDbKeyStore()).then(
    (key) => new MasterClient(base, key),
  );
  return {
    resolveAddress: async (address) => {
      const found = await (await client).resolve({ address });
      if (found.kind !== 'server') throw new Error('That is not a server address.');
      return serverInvite(found.server);
    },
    nearby: async () => (await client).nearby(),
    lobby: async (query) => (await client).lobby(query),
    ping: async (server) => {
      if (!ping) throw new Error('no ping here');
      const invite = parseInvite(`?${new URLSearchParams(serverInvite(server)).toString()}`);
      if (!invite) throw new Error('unusable server address');
      const w = invite.webrtc;
      const rtc = w ? [w.ip, w.port, w.ufrag, w.pwd] : undefined;
      return (await ping())(invite.url, invite.certHash, rtc, PING_TIMEOUT_MS);
    },
  };
}

/** Shows the main menu over the page. */
export function showMenu(options: MenuPageOptions): MainMenu {
  // Its buttons sound (docs/AUDIO.md §6), in plain TS: the launcher has no WASM.
  installPageSounds();
  // A controller moves through it too (docs/GAMEPAD.md); B is Esc.
  const pad = startMenuGamepad(document, () => {
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }),
    );
  });
  const master = menuMaster(options.ping);
  // Settings, every pane (owner, 2026-11-08: "All settings" from the main menu): the same menu as
  // in a world, saved the same way; the pages that need a world say so.
  let menu: MainMenu | null = null;
  const settings = new SettingsMenu(
    document.body,
    prefersTouch(),
    (s) => {
      audio().setVolumes(s.audio);
      menu?.setBackground(s.menuBackground);
    },
    { world: false },
  );
  const controller = new ControllerPanel({
    settings: loadGamepadSettings(),
    apply: saveGamepadSettings,
    capture: (done) => {
      pad.capture = done;
    },
    connected: () => pad.active,
  });
  settings.addControllerPanel(
    controller.element,
    () => {
      controller.refresh();
    },
    () => {
      controller.cancel();
    },
  );
  addWorldPages(settings);
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    // Esc (a controller's B) closes Settings, else goes back a page (ui/menuNav.ts).
    if (settings.isOpen) settings.setOpen(false);
    else menu?.back();
  });
  menu = new MainMenu(document.body, {
    background: settings.current.menuBackground,
    openSettings: () => {
      settings.setOpen(true);
    },
    index: new WorldIndex(storage()),
    thumbs: new WorldThumbs(storage()),
    recent: loadRecent(storage()),
    listFiles: listWorldFiles,
    deleteFiles: deleteWorldFiles,
    go: options.go,
    now: () => Date.now(),
    version: options.version,
    channel: options.channel,
    protocol: options.protocol,
    detail: options.detail,
    versionsHref: options.versionsHref,
    noticesHref: options.noticesHref,
    ...(options.message ? { message: options.message } : {}),
    ...(master ? { master } : {}),
    ...(options.canRegenerate ? { canRegenerate: options.canRegenerate } : {}),
  });
  return menu;
}

/** The pages that need a world, listed in Settings from the main menu with what they are for. */
export const WORLD_PAGES = [
  ['structures', 'Open a world to save structures from it and load them into it.'],
  ['debug', 'Open a world to use the debug tools: the overlay, the map and the block dump.'],
  ['host', 'Open one of your worlds to host it for friends.'],
] as const;

function addWorldPages(settings: SettingsMenu): void {
  for (const [page, note] of WORLD_PAGES) settings.addUnavailablePage(page, note);
}
