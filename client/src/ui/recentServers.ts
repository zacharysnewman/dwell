// Servers this browser joined by invite, newest first, for the main menu's Join section. The
// entries keep the whole invite, so a server that has since restarted (a new certificate) needs a
// fresh link; join codes (Phase 5c) and addresses (5d) will not.
import type { KeyValueStore } from '../local/worldIndex';

export interface RecentServer {
  /** The invite parameters (`join`, `cert`, and optionally `rtc`, `ice`). */
  invite: Record<string, string>;
  /** `host:port`. */
  label: string;
  joinedAt: number;
}

export const RECENT_KEY = 'dwell.recentServers';
export const MAX_RECENT = 8;

function isRecent(v: unknown): v is RecentServer {
  if (typeof v !== 'object' || v === null) return false;
  const r = v as Record<string, unknown>;
  const invite = r.invite as Record<string, unknown> | null;
  return (
    typeof r.label === 'string' &&
    typeof r.joinedAt === 'number' &&
    typeof invite === 'object' &&
    invite !== null &&
    typeof invite.join === 'string' &&
    typeof invite.cert === 'string' &&
    Object.values(invite).every((x) => typeof x === 'string')
  );
}

export function loadRecent(store: KeyValueStore | null): RecentServer[] {
  try {
    const raw: unknown = JSON.parse(store?.getItem(RECENT_KEY) ?? '[]');
    return Array.isArray(raw) ? raw.filter(isRecent).slice(0, MAX_RECENT) : [];
  } catch {
    return [];
  }
}

/** Records a joined server, replacing an earlier entry for the same address. */
export function rememberServer(
  store: KeyValueStore | null,
  invite: Record<string, string>,
  now: number,
): void {
  const label = invite.join ?? '';
  const list = [
    { invite, label, joinedAt: now },
    ...loadRecent(store).filter((r) => r.label !== label),
  ].slice(0, MAX_RECENT);
  try {
    store?.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // Not kept.
  }
}
