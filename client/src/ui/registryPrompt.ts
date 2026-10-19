// The join prompt for a server whose blocks differ from this version's (ADR 0038): the server's
// definitions are used either way (it is authoritative, and the client predicts against it), so a
// difference never refuses the join; the player sees what differs and chooses Continue or Cancel.
// "Don't ask again for this server" is remembered per server and definitions hash, so a server
// that changes its blocks asks again. Identical registries never reach here.
import type { KeyValueStore } from '../local/worldIndex';

/** One state the server's registry treats differently: its name and what differs (or a note). */
export interface RegistryItem {
  name: string;
  fields: string[];
}

/** `sessionState().registry` while `phase === 'confirming'` (dwell-client's web/app.rs). */
export interface RegistryDifferences {
  /** The server's definitions hash, 16 hex digits. */
  hash: string;
  /** Known here, with different rules or shape: the server's are used. */
  different: RegistryItem[];
  /** Unknown to this version: drawn with a placeholder look, with the server's shape and rules. */
  added: RegistryItem[];
  /** This version's blocks the server does not have: never sent. */
  clientOnly: string[];
}

export const GROUP_TITLES = {
  different: 'Different on this server',
  added: 'New on this server (shown as placeholders)',
  clientOnly: 'Only in your version (not used here)',
} as const;

export interface PromptGroup {
  key: keyof typeof GROUP_TITLES;
  title: string;
  items: { name: string; detail: string }[];
}

/** The prompt's lists, in order, empty groups left out. */
export function promptGroups(r: RegistryDifferences): PromptGroup[] {
  const groups: PromptGroup[] = [
    {
      key: 'different',
      title: GROUP_TITLES.different,
      items: r.different.map((d) => ({ name: d.name, detail: d.fields.join(', ') })),
    },
    {
      key: 'added',
      title: GROUP_TITLES.added,
      items: r.added.map((d) => ({ name: d.name, detail: d.fields.join(', ') })),
    },
    {
      key: 'clientOnly',
      title: GROUP_TITLES.clientOnly,
      items: r.clientOnly.map((name) => ({ name, detail: '' })),
    },
  ];
  return groups.filter((g) => g.items.length > 0);
}

/**
 * The record of servers not to ask about again: `{ servers: { [server]: { hash, at } } }`. It is a
 * cross-version store (RELEASES.md §6), so it is append-only: other fields and other servers'
 * entries are kept on rewrite, and an unreadable record is left as it is.
 */
export const REGISTRY_TRUST_KEY = 'dwell.registryTrust';

function readTrust(store: KeyValueStore | null): Record<string, unknown> | null {
  const text = store?.getItem(REGISTRY_TRUST_KEY) ?? null;
  if (text === null) return {};
  try {
    const v: unknown = JSON.parse(text);
    return typeof v === 'object' && v !== null && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function serversOf(record: Record<string, unknown>): Record<string, unknown> {
  const s = record.servers;
  return typeof s === 'object' && s !== null && !Array.isArray(s)
    ? (s as Record<string, unknown>)
    : {};
}

/** The definitions hash the player trusts for `server`, or null. */
export function trustedHash(store: KeyValueStore | null, server: string): string | null {
  const record = readTrust(store);
  if (!record) return null;
  const entry = serversOf(record)[server];
  if (typeof entry !== 'object' || entry === null) return null;
  const hash = (entry as Record<string, unknown>).hash;
  return typeof hash === 'string' && /^[0-9a-f]{16}$/.test(hash) ? hash : null;
}

/** The record with `server` trusted at `hash`: everything else in it kept. */
export function trustRecord(
  stored: Record<string, unknown>,
  server: string,
  hash: string,
  now: number,
): Record<string, unknown> {
  const servers = serversOf(stored);
  const before = servers[server];
  const kept = typeof before === 'object' && before !== null ? before : {};
  return { ...stored, servers: { ...servers, [server]: { ...kept, hash, at: now } } };
}

/** Remembers "don't ask again" for `server` at `hash` (not over an unreadable record). */
export function rememberTrust(
  store: KeyValueStore | null,
  server: string,
  hash: string,
  now: number,
): void {
  const record = readTrust(store);
  if (!record) return;
  try {
    store?.setItem(REGISTRY_TRUST_KEY, JSON.stringify(trustRecord(record, server, hash, now)));
  } catch {
    // Not kept: it asks again next time.
  }
}

export interface PromptAnswer {
  proceed: boolean;
  dontAsk: boolean;
}

/** Shows the prompt over the page (the game menu's sheet style); resolves with the answer. */
export function showRegistryPrompt(
  doc: Document,
  target: string,
  r: RegistryDifferences,
): Promise<PromptAnswer> {
  const root = doc.createElement('div');
  root.className = 'game-menu registry-prompt';
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-labelledby', 'registry-prompt-title');
  const sheet = doc.createElement('div');
  sheet.className = 'rp-sheet';
  const title = doc.createElement('h2');
  title.id = 'registry-prompt-title';
  title.className = 'gm-title';
  title.textContent = 'This server’s blocks differ';
  const lead = doc.createElement('p');
  lead.className = 'menu-hint';
  lead.textContent = `${target} defines some blocks differently from this version of Dwell. Its definitions are used while you play here.`;
  sheet.append(title, lead);
  for (const g of promptGroups(r)) {
    const h = doc.createElement('h3');
    h.className = 'rp-group';
    h.textContent = `${g.title} (${String(g.items.length)})`;
    const list = doc.createElement('ul');
    list.className = 'rp-list';
    list.dataset.group = g.key;
    for (const item of g.items) {
      const li = doc.createElement('li');
      const name = doc.createElement('code');
      name.textContent = item.name;
      li.append(name);
      if (item.detail) {
        const detail = doc.createElement('span');
        detail.className = 'rp-detail';
        detail.textContent = ` — ${item.detail}`;
        li.append(detail);
      }
      list.append(li);
    }
    sheet.append(h, list);
  }
  const remember = doc.createElement('label');
  remember.className = 'rp-remember';
  const box = doc.createElement('input');
  box.type = 'checkbox';
  remember.append(box, doc.createTextNode(' Don’t ask again for this server'));
  const actions = doc.createElement('div');
  actions.className = 'rp-actions';
  const cancel = doc.createElement('button');
  cancel.type = 'button';
  cancel.className = 'menu-button-small';
  cancel.dataset.sound = 'back';
  cancel.textContent = 'Cancel';
  const go = doc.createElement('button');
  go.type = 'button';
  go.className = 'menu-button-main';
  go.textContent = 'Continue';
  actions.append(cancel, go);
  sheet.append(remember, actions);
  root.append(sheet);
  doc.body.append(root);
  go.focus();
  return new Promise((resolve) => {
    const done = (proceed: boolean) => {
      root.remove();
      resolve({ proceed, dontAsk: proceed && box.checked });
    };
    go.addEventListener('click', () => {
      done(true);
    });
    cancel.addEventListener('click', () => {
      done(false);
    });
  });
}
