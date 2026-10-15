// The main menu as a page (RELEASES.md §5): the launcher shows it at `/dwell/`; a build served at
// the launcher's path (a local or preview build, the e2e suite) shows it itself. DOM only, no WASM:
// pinging a server is passed in (the build's own client, or the latest build's `tools.js`).
import { IndexedDbKeyStore, loadOrCreateDeviceKey } from '../identity/deviceKey';
import { WorldIndex, type WorldMeta } from '../local/worldIndex';
import { deleteWorldFiles, listWorldFiles } from '../local/worldFiles';
import { parseInvite } from '../net/invite';
import { configuredMasterUrl, MasterClient, serverInvite } from '../net/master';
import { MainMenu, type MainMenuDeps } from './mainMenu';
import { loadRecent } from './recentServers';

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
  const master = menuMaster(options.ping);
  return new MainMenu(document.body, {
    index: new WorldIndex(storage()),
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
}
