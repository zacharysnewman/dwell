// The main menu (ARCHITECTURE.md §2.1, Phase 5a), shown when the page opens without a world or
// server in its address: the local worlds (create with a seed, play, regenerate, delete) and Join
// (a join code, a server address or an invite link; servers and friend worlds on this network;
// recently joined servers), and the server browser (the lobby list, Phase 5e). Choosing one
// navigates to it (ui/launch.ts).
import { buildInfo, channelOf, recordedVersion } from '../buildInfo';
import { GENERATORS } from '../local/world';
import {
  cleanName,
  parseSeed,
  typeLabel,
  WORLD_TYPES,
  type WorldIndex,
  type WorldMeta,
  type WorldType,
} from '../local/worldIndex';
import type { ListedServer, Lobby, LobbyQuery, Nearby } from '../net/master';
import { PROTOCOL_VERSION } from '../protocol/constants.gen';
import { looksLikeAddress, pastedCode, pastedInvite } from './launch';
import type { RecentServer } from './recentServers';
import { ServerBrowser } from './serverBrowser';

export interface MainMenuDeps {
  index: WorldIndex;
  recent: RecentServer[];
  /** World files saved in the browser (names), to list worlds the index doesn't know yet. */
  listFiles(): Promise<string[]>;
  deleteFiles(id: string): Promise<boolean>;
  /** Opens a route: `{ play: id }`, or invite parameters. */
  go(route: Record<string, string>): void;
  now(): number;
  /** Shown on opening, e.g. why the menu came up instead of a world. */
  message?: string;
  /** The master server (Phase 5d), when this build has one: addresses and nearby games. */
  master?: {
    /** Invite parameters for a server address, or a rejection with a message for the player. */
    resolveAddress(address: string): Promise<Record<string, string>>;
    nearby(): Promise<Nearby>;
    /** The lobby list (Phase 5e), and a dedicated server's round trip in ms. */
    lobby(query: LobbyQuery): Promise<Lobby>;
    ping(server: ListedServer): Promise<number>;
  };
}

/** A nearby game's label: "Home server · 2/16 players" or "Bravo · friend world". */
export function nearbyLabel(
  entry: { name: string } & ({ players: number; maxPlayers: number } | { players?: undefined }),
): string {
  return entry.players === undefined
    ? `${entry.name} · friend world`
    : `${entry.name} · ${String(entry.players)}/${String(entry.maxPlayers)} players`;
}

/** "just now", "5 min ago", "3 h ago", "2 days ago", or "never". */
export function formatPlayed(at: number, now: number): string {
  if (at <= 0) return 'never played';
  const min = Math.floor((now - at) / 60_000);
  if (min < 1) return 'played just now';
  if (min < 60) return `played ${String(min)} min ago`;
  const h = Math.floor(min / 60);
  if (h < 24) return `played ${String(h)} h ago`;
  const days = Math.floor(h / 24);
  return `played ${String(days)} day${days === 1 ? '' : 's'} ago`;
}

/** A world's details line: "Terrain · seed 42 · played 3 h ago · v0.1.0". */
export function worldDetails(world: WorldMeta, now: number): string {
  const version = world.appVersion ? ` · v${world.appVersion}` : '';
  return `${typeLabel(world.type)} · seed ${String(world.seed)} · ${formatPlayed(world.lastPlayedAt, now)}${version}`;
}

function button(label: string, className: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.className = className;
  b.textContent = label;
  b.addEventListener('click', onClick);
  return b;
}

/**
 * A button that acts on a second click: the first asks to confirm (for delete and regenerate),
 * reverting after a few seconds.
 */
function confirmButton(label: string, confirm: string, onConfirm: () => void): HTMLButtonElement {
  let armed = false;
  let timer = 0;
  const b = button(label, 'menu-button-small', () => {
    if (armed) {
      window.clearTimeout(timer);
      onConfirm();
      return;
    }
    armed = true;
    b.textContent = confirm;
    b.classList.add('menu-confirm');
    timer = window.setTimeout(() => {
      armed = false;
      b.textContent = label;
      b.classList.remove('menu-confirm');
    }, 4000);
  });
  return b;
}

export class MainMenu {
  readonly root = document.createElement('div');
  private readonly list = document.createElement('ul');
  private readonly message = document.createElement('p');
  private readonly form = document.createElement('form');
  /** Worlds saved before versioned releases (RELEASES.md §6): listed only to be deleted. */
  private readonly legacy = document.createElement('section');

  constructor(
    parent: HTMLElement,
    private readonly deps: MainMenuDeps,
  ) {
    this.root.id = 'main-menu';
    const panel = document.createElement('div');
    panel.className = 'menu-panel';
    const title = document.createElement('h1');
    title.textContent = 'Dwell';
    this.message.id = 'menu-message';
    this.message.setAttribute('aria-live', 'polite');
    this.message.textContent = deps.message ?? '';

    const worldsTitle = document.createElement('h2');
    worldsTitle.textContent = 'Worlds';
    const create = button('New world', 'menu-button-main', () => {
      this.form.hidden = !this.form.hidden;
      if (!this.form.hidden) this.form.querySelector('input')?.focus();
    });
    create.id = 'new-world';
    this.list.id = 'world-list';
    this.buildForm();

    panel.append(
      title,
      this.message,
      worldsTitle,
      create,
      this.form,
      this.list,
      this.joinSection(),
    );
    this.legacy.id = 'legacy-worlds';
    const master = deps.master;
    if (master) {
      const browser = new ServerBrowser({
        lobby: (query) => master.lobby(query),
        ping: (server) => master.ping(server),
        go: (route) => {
          deps.go(route);
        },
        protocol: PROTOCOL_VERSION,
        version: recordedVersion(),
      });
      panel.append(browser.root);
    }
    panel.append(this.legacy, this.aboutSection());
    this.root.append(panel);
    parent.append(this.root);
    this.render();
    void this.adoptSavedWorlds();
  }

  /** The version, and the About details: licence and third-party notices (RELEASES.md §2). */
  private aboutSection(): HTMLElement {
    const section = document.createElement('section');
    section.id = 'about';
    const line = document.createElement('p');
    line.className = 'menu-hint';
    const version = recordedVersion();
    // The version page is the launcher's (RELEASES.md §5), always the current one; the launcher
    // is the site's root, `/dwell/` in production.
    const versions = document.createElement('a');
    versions.id = 'versions-link';
    versions.className = 'menu-link';
    versions.href = new URL(
      '../../?versions',
      new URL(import.meta.env.BASE_URL, location.href),
    ).href;
    versions.textContent = 'Versions';
    line.append(
      `Dwell ${version} (${channelOf(buildInfo.version)}) · `,
      button('About', 'menu-link', () => {
        details.hidden = !details.hidden;
      }),
      ' · ',
      versions,
    );
    line.id = 'app-version';
    const details = document.createElement('div');
    details.id = 'about-details';
    details.hidden = true;
    const text = document.createElement('p');
    text.className = 'menu-hint';
    text.textContent = `Build ${buildInfo.sha.slice(0, 7)}, ${buildInfo.time.slice(0, 10)}. Dwell is not open source: all rights reserved, and the source is public for reference only. `;
    const notices = document.createElement('a');
    notices.href = `${import.meta.env.BASE_URL}THIRD_PARTY_NOTICES.txt`;
    notices.target = '_blank';
    notices.rel = 'noopener';
    notices.textContent = 'Third-party notices';
    details.append(text, notices);
    section.append(line, details);
    return section;
  }

  private say(text: string): void {
    this.message.textContent = text;
  }

  private buildForm(): void {
    const f = this.form;
    f.id = 'create-world';
    f.hidden = true;
    const field = (label: string, control: HTMLElement): HTMLLabelElement => {
      const l = document.createElement('label');
      l.className = 'menu-field';
      const span = document.createElement('span');
      span.textContent = label;
      l.append(span, control);
      return l;
    };
    const name = document.createElement('input');
    name.id = 'world-name';
    name.maxLength = 40;
    name.placeholder = 'New world';
    name.autocomplete = 'off';
    const seed = document.createElement('input');
    seed.id = 'world-seed';
    seed.placeholder = 'Random';
    seed.autocomplete = 'off';
    const type = document.createElement('select');
    type.id = 'world-type';
    for (const t of WORLD_TYPES) {
      const option = document.createElement('option');
      option.value = t;
      option.textContent = typeLabel(t);
      type.append(option);
    }
    const submit = document.createElement('button');
    submit.type = 'submit';
    submit.className = 'menu-button-main';
    submit.textContent = 'Create and play';
    const hint = document.createElement('p');
    hint.className = 'menu-hint';
    hint.textContent = 'Seed: a number, or any text. The same seed makes the same world.';
    f.append(field('Name', name), field('Seed', seed), field('Type', type), hint, submit);
    f.addEventListener('submit', (e) => {
      e.preventDefault();
      const world = this.deps.index.create(
        {
          name: cleanName(name.value, 'New world'),
          type: type.value as WorldType,
          seed: parseSeed(seed.value),
        },
        this.deps.now(),
      );
      this.deps.go({ play: world.id });
    });
  }

  private joinSection(): HTMLElement {
    const section = document.createElement('section');
    section.className = 'menu-join';
    const title = document.createElement('h2');
    title.textContent = 'Join';
    const form = document.createElement('form');
    form.className = 'menu-join-form';
    const input = document.createElement('input');
    input.id = 'join-input';
    input.placeholder = 'Join code, address or invite link';
    input.autocapitalize = 'characters';
    input.autocomplete = 'off';
    const submit = document.createElement('button');
    submit.type = 'submit';
    submit.className = 'menu-button-main';
    submit.textContent = 'Join';
    form.append(input, submit);
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      const invite = pastedInvite(input.value);
      const code = invite ? null : pastedCode(input.value);
      const master = this.deps.master;
      if (invite) this.deps.go(invite);
      else if (code) this.deps.go({ code });
      else if (looksLikeAddress(input.value) && master) {
        const address = input.value.trim();
        this.say(`Looking up ${address}…`);
        master.resolveAddress(address).then(
          (route) => {
            this.deps.go(route);
          },
          (err: unknown) => {
            this.say(err instanceof Error ? err.message : String(err));
          },
        );
      } else {
        this.say(
          'That is not a join code, a server address or an invite link. A host shows its code (e.g. KQ7-XM4); a server prints its code and a link when it starts.',
        );
      }
    });
    section.append(title, form);
    if (this.deps.master) section.append(this.nearbySection(this.deps.master));
    if (this.deps.recent.length > 0) {
      const recent = document.createElement('ul');
      recent.id = 'recent-servers';
      for (const server of this.deps.recent) {
        const item = document.createElement('li');
        item.append(
          button(server.label, 'menu-button-small', () => {
            this.deps.go(server.invite);
          }),
        );
        recent.append(item);
      }
      const label = document.createElement('p');
      label.className = 'menu-hint';
      label.textContent = 'Recent servers (a restarted server needs a new link):';
      section.append(label, recent);
    }
    return section;
  }

  /** Servers and friend worlds on this network (Phase 5d), filled in when the master answers. */
  private nearbySection(master: NonNullable<MainMenuDeps['master']>): HTMLElement {
    const box = document.createElement('div');
    box.id = 'nearby';
    const label = document.createElement('p');
    label.className = 'menu-hint';
    label.textContent = 'Looking for games on your network…';
    const list = document.createElement('ul');
    list.id = 'nearby-list';
    box.append(label, list);
    master.nearby().then(
      (found) => {
        const items = [
          ...found.servers.map((s) => ({
            code: s.code,
            text: nearbyLabel(s),
            version: s.appVersion,
          })),
          ...found.worlds.map((w) => ({
            code: w.code,
            text: nearbyLabel(w),
            version: w.appVersion,
          })),
        ];
        label.textContent =
          items.length > 0 ? 'On your network:' : 'Nothing is being hosted on your network.';
        for (const item of items) {
          const li = document.createElement('li');
          li.append(
            button(item.text, 'menu-button-small', () => {
              this.deps.go(
                item.version ? { code: item.code, v: item.version } : { code: item.code },
              );
            }),
          );
          list.append(li);
        }
      },
      () => {
        box.hidden = true; // no master reachable: nothing to show
      },
    );
    return box;
  }

  /** Lists worlds saved before the index existed (or opened by link in another way). */
  private async adoptSavedWorlds(): Promise<void> {
    const before = this.deps.index.list().length + this.deps.index.legacy().length;
    // A file the index doesn't know was saved before versioned releases: it is listed to delete.
    for (const name of await this.deps.listFiles()) this.deps.index.adopt(name, this.deps.now());
    if (this.deps.index.list().length + this.deps.index.legacy().length !== before) this.render();
  }

  private render(): void {
    const worlds = this.deps.index.list();
    const now = this.deps.now();
    this.renderLegacy();
    this.list.replaceChildren();
    if (worlds.length === 0) {
      const empty = document.createElement('li');
      empty.className = 'menu-empty';
      empty.textContent = 'No worlds yet. Create one to start playing.';
      this.list.append(empty);
      this.form.hidden = false;
      return;
    }
    for (const world of worlds) {
      const item = document.createElement('li');
      item.className = 'world-item';
      item.dataset.id = world.id;
      const info = document.createElement('div');
      info.className = 'world-info';
      const name = document.createElement('span');
      name.className = 'world-name';
      name.textContent = world.name;
      const details = document.createElement('span');
      details.className = 'world-details';
      details.textContent = worldDetails(world, now);
      info.append(name, details);
      const actions = document.createElement('div');
      actions.className = 'world-actions';
      actions.append(
        button('Play', 'menu-button-main world-play', () => {
          this.deps.go({ play: world.id });
        }),
        confirmButton('Regenerate', 'Lose all changes?', () => void this.regenerate(world)),
        confirmButton('Delete', 'Delete forever?', () => void this.remove(world)),
      );
      item.append(info, actions);
      this.list.append(item);
    }
  }

  private renderLegacy(): void {
    const old = this.deps.index.legacy();
    this.legacy.replaceChildren();
    this.legacy.hidden = old.length === 0;
    if (old.length === 0) return;
    const title = document.createElement('h2');
    title.textContent = 'Saved before versioned releases';
    const hint = document.createElement('p');
    hint.className = 'menu-hint';
    hint.textContent =
      "These worlds can't be opened by any version of Dwell. Delete them to free their space.";
    const list = document.createElement('ul');
    for (const world of old) {
      const item = document.createElement('li');
      item.className = 'world-item';
      item.dataset.id = world.id;
      const name = document.createElement('span');
      name.className = 'world-name';
      name.textContent = world.name;
      item.append(
        name,
        confirmButton('Delete', 'Delete forever?', () => void this.remove(world)),
      );
      list.append(item);
    }
    this.legacy.append(title, hint, list);
  }

  /** Recreates a world from its seed with its type's current generator, discarding changes. */
  private async regenerate(world: WorldMeta): Promise<void> {
    if (!(await this.deps.deleteFiles(world.id))) {
      this.say(`"${world.name}" is open in another tab. Close it there first.`);
      return;
    }
    // A new file: this build's world, whichever version the old one was.
    this.deps.index.put({
      ...world,
      generatorVersion: GENERATORS[world.type],
      appVersion: recordedVersion(),
    });
    this.deps.go({ play: world.id });
  }

  private async remove(world: WorldMeta): Promise<void> {
    if (!(await this.deps.deleteFiles(world.id))) {
      this.say(`"${world.name}" is open in another tab. Close it there first.`);
      return;
    }
    this.deps.index.remove(world.id);
    this.say(`Deleted "${world.name}".`);
    this.render();
  }
}
