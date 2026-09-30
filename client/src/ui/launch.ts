// What the page opens (ARCHITECTURE.md §2.1, Phase 5a): the main menu, or — from a link or a menu
// choice — straight into a game. The menu starts a game by navigating, so a reload continues the
// same world or server and Back returns to the menu.
import { parseInvite } from '../net/invite';

export type Launch =
  /** No world or server in the address: the main menu. */
  | { kind: 'menu' }
  /** An invite link (`?join=…&cert=…`). */
  | { kind: 'join' }
  /** A world from the menu (`?play=<world id>`). */
  | { kind: 'play'; id: string }
  /** A local world by link (`?local=1`, `?world=`, `?seed=`): one per generator and seed. */
  | { kind: 'link' };

/** Parameters that choose what the page opens; every other one (debug flags) is kept. */
const ROUTE_PARAMS = ['join', 'cert', 'rtc', 'ice', 'play', 'local', 'world', 'seed'];
const INVITE_PARAMS = ['join', 'cert', 'rtc', 'ice'];

export function launchOf(search: string): Launch {
  const params = new URLSearchParams(search);
  if (params.get('local') === '1') return { kind: 'link' };
  if (params.has('join')) return { kind: 'join' };
  const play = params.get('play');
  if (play) return { kind: 'play', id: play };
  if (params.has('world') || params.has('seed')) return { kind: 'link' };
  return { kind: 'menu' };
}

/**
 * The query string that opens `route` (e.g. `{ play: id }`, or `{}` for the menu), keeping the
 * current page's other parameters such as `?debug=1`. Empty when nothing is left.
 */
export function withRoute(search: string, route: Record<string, string>): string {
  const params = new URLSearchParams(search);
  for (const key of ROUTE_PARAMS) params.delete(key);
  for (const [key, value] of Object.entries(route)) params.set(key, value);
  const query = params.toString();
  return query ? `?${query}` : '';
}

/**
 * The invite parameters in something a player pasted — a whole invite link, or just its query —
 * or null if it holds no valid invite.
 */
export function pastedInvite(text: string): Record<string, string> | null {
  let query = text.trim();
  const q = query.indexOf('?');
  if (q >= 0) query = query.slice(q + 1);
  query = query.replace(/#.*$/, '');
  if (!parseInvite(`?${query}`)) return null;
  const params = new URLSearchParams(query);
  const route: Record<string, string> = {};
  for (const key of INVITE_PARAMS) {
    const value = params.get(key);
    if (value !== null) route[key] = value;
  }
  return route;
}
