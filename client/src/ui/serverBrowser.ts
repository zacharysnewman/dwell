// The server browser (ARCHITECTURE.md §10.3, Phase 5e): the master's lobby list in the main menu —
// public dedicated servers (verified by players' join receipts; unverified ones under "New
// servers") and public friend worlds, with a search box. Dedicated servers are pinged with a
// `StatusRequest`; friend worlds show their players only. Servers running another protocol version
// are marked with the handshake's reason and can't be joined.
import type { Lobby, LobbyQuery, ListedServer, ListedWorld } from '../net/master';

export interface ServerBrowserDeps {
  lobby(query: LobbyQuery): Promise<Lobby>;
  /** Round trip to a dedicated server, ms. */
  ping(server: ListedServer): Promise<number>;
  /** Opens a route: `{ code }`. */
  go(route: Record<string, string>): void;
  /** This client's protocol version. */
  protocol: number;
}

/** Pings at once, so a long list doesn't open dozens of connections together. */
const PING_CONCURRENCY = 4;
/** Typing in the search box waits this long before asking the master. */
const SEARCH_DELAY_MS = 300;

/**
 * Why this client can't join a server running `serverProtocol`: the handshake's own reason
 * (`Reject(ProtocolVersion)`), or null when compatible or unknown.
 */
export function incompatibility(
  serverProtocol: number | null,
  clientProtocol: number,
): string | null {
  if (serverProtocol === null || serverProtocol === clientProtocol) return null;
  return `Server runs protocol ${String(serverProtocol)}, client runs ${String(clientProtocol)}.`;
}

/** An entry's details line: "2/16 players · pve, creative" or "friend world · 1/5 players". */
export function lobbyDetails(entry: ListedServer | ListedWorld): string {
  const players = `${String(entry.players)}/${String(entry.maxPlayers)} players`;
  if (!('cert' in entry)) return `friend world · ${players}`;
  const tags = entry.tags ?? [];
  return tags.length > 0 ? `${players} · ${tags.join(', ')}` : players;
}

/** A ping for display: "38 ms", or "—" when the server didn't answer. */
export function formatPing(ms: number | null): string {
  return ms === null ? '—' : `${String(Math.max(1, Math.round(ms)))} ms`;
}

/** Runs `task` over `items`, at most `limit` at a time. */
async function eachLimited<T>(items: T[], limit: number, task: (item: T) => Promise<void>) {
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const item = items[next++];
      if (item !== undefined) await task(item);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
}

export class ServerBrowser {
  readonly root = document.createElement('section');
  private readonly search = document.createElement('input');
  private readonly fresh = document.createElement('input');
  private readonly status = document.createElement('p');
  private readonly list = document.createElement('ul');
  /** Bumped on each query, so a slow older answer doesn't replace a newer one. */
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly deps: ServerBrowserDeps) {
    this.root.id = 'server-browser';
    const title = document.createElement('h2');
    title.textContent = 'Servers';
    const form = document.createElement('form');
    form.className = 'lobby-search-form';
    this.search.id = 'lobby-search';
    this.search.type = 'search';
    this.search.placeholder = 'Search servers';
    this.search.autocomplete = 'off';
    const refresh = document.createElement('button');
    refresh.type = 'submit';
    refresh.className = 'menu-button-small';
    refresh.textContent = 'Refresh';
    form.append(this.search, refresh);
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      void this.refresh();
    });
    this.search.addEventListener('input', () => {
      clearTimeout(this.timer);
      this.timer = setTimeout(() => void this.refresh(), SEARCH_DELAY_MS);
    });
    const freshLabel = document.createElement('label');
    freshLabel.className = 'menu-hint lobby-filter';
    this.fresh.id = 'lobby-new';
    this.fresh.type = 'checkbox';
    this.fresh.addEventListener('change', () => void this.refresh());
    freshLabel.append(this.fresh, ' New servers (not yet confirmed reachable by players)');
    this.status.className = 'menu-hint';
    this.status.id = 'lobby-status';
    this.list.id = 'lobby-list';
    this.root.append(title, form, freshLabel, this.status, this.list);
    void this.refresh();
  }

  /** Asks the master for the list again with the current search, and pings its servers. */
  async refresh(): Promise<void> {
    clearTimeout(this.timer);
    const generation = ++this.generation;
    this.status.textContent = 'Looking for servers…';
    let lobby: Lobby;
    try {
      lobby = await this.deps.lobby({ q: this.search.value, fresh: this.fresh.checked });
    } catch (err) {
      if (generation !== this.generation) return;
      this.status.textContent = `Could not load the server list: ${err instanceof Error ? err.message : String(err)}`;
      this.list.replaceChildren();
      return;
    }
    if (generation !== this.generation) return;
    const count = lobby.servers.length + lobby.worlds.length;
    this.status.textContent =
      count > 0
        ? ''
        : this.fresh.checked
          ? 'No new servers.'
          : 'No public games right now. Host one, or look under New servers.';
    this.list.replaceChildren(
      ...lobby.servers.map((s) => this.item(s)),
      ...lobby.worlds.map((w) => this.item(w)),
    );
    await eachLimited(lobby.servers, PING_CONCURRENCY, async (server) => {
      if (generation !== this.generation) return;
      const cell = this.list.querySelector<HTMLElement>(`[data-code="${server.code}"] .lobby-ping`);
      if (!cell || incompatibility(server.protocol, this.deps.protocol)) return;
      let ms: number | null = null;
      try {
        ms = await this.deps.ping(server);
      } catch {
        // unreachable from here: shown as "—"
      }
      cell.textContent = formatPing(ms);
    });
  }

  private item(entry: ListedServer | ListedWorld): HTMLLIElement {
    const li = document.createElement('li');
    li.className = 'world-item lobby-entry';
    li.dataset.code = entry.code;
    const info = document.createElement('div');
    info.className = 'world-info';
    const name = document.createElement('span');
    name.className = 'world-name';
    name.textContent = entry.name;
    const details = document.createElement('span');
    details.className = 'world-details';
    details.textContent = lobbyDetails(entry);
    info.append(name, details);
    if ('motd' in entry && entry.motd !== '') {
      const motd = document.createElement('span');
      motd.className = 'world-details lobby-motd';
      motd.textContent = entry.motd;
      info.append(motd);
    }
    const reason = incompatibility(entry.protocol, this.deps.protocol);
    if (reason) {
      const why = document.createElement('span');
      why.className = 'world-details lobby-reason';
      why.textContent = reason;
      info.append(why);
    }
    const actions = document.createElement('div');
    actions.className = 'world-actions';
    if ('cert' in entry) {
      const ping = document.createElement('span');
      ping.className = 'world-details lobby-ping';
      ping.textContent = reason ? '' : '…';
      actions.append(ping);
    }
    const join = document.createElement('button');
    join.type = 'button';
    join.className = 'menu-button-main lobby-join';
    join.textContent = 'Join';
    join.disabled = reason !== null;
    join.addEventListener('click', () => {
      this.deps.go({ code: entry.code });
    });
    actions.append(join);
    li.append(info, actions);
    return li;
  }
}
