// The main menu (ARCHITECTURE.md §2.1, Phase 5a; since the M1 redesign a logo screen, a home and
// pages over the panorama, ui/menuNav.ts): the local worlds (create with a seed, play,
// regenerate, delete) and Join (a join code, a server address or an invite link; servers and friend
// worlds on this network; recently joined servers), and the server browser (the lobby list, Phase
// 5e). The launcher shows it (RELEASES.md §5), so it is the same whichever build plays: every
// choice is a route the launcher sends to the build that carries it out (ui/launch.ts) — creating
// and regenerating a world too, so the world is the build's own. A build served at the launcher's
// path (a local or preview build) shows it itself (ui/menuPage.ts).
import { DEFAULT_MODE, GAME_MODES } from './modes.gen';
import { modeLabel } from './modes';
import { changesFor } from '../version/changes';
import { changesView } from './changesView';
import {
  cleanName,
  typeLabel,
  WORLD_TYPES,
  type WorldIndex,
  type WorldMeta,
} from '../local/worldIndex';
import type { ListedServer, Lobby, LobbyQuery, Nearby } from '../net/master';
import { looksLikeAddress, pastedCode, pastedInvite } from './launch';
import type { RecentServer } from './recentServers';
import { ServerBrowser } from './serverBrowser';
import { playAdvancedPanel, spawnChoices } from './playAdvanced';
import { audio } from '../audio/engine';
import { DEFAULT_MENU_BACKGROUND, MenuPanorama } from './menuBackground';
import {
  afterIntro,
  back,
  firstView,
  introSeen,
  isIntro,
  markIntroSeen,
  type MenuView,
} from './menuNav';

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
  /** The version the menu plays by default (the launcher's latest), its channel and protocol. */
  version: string;
  channel: 'stable' | 'dev';
  protocol: number;
  /** The build or launcher behind the menu, for About ("Build 1a2b3c4, 2026-10-14."). */
  detail: string;
  /** The version page and the licence notices. */
  versionsHref: string;
  noticesHref: string;
  /** The background (shared/ui/menu-backgrounds.json, Settings → On screen); default the data's. */
  background?: string;
  /**
   * Shows or hides Settings (every pane; ui/menuPage.ts): the menu's `settings` page (menu M3),
   * opened by the Settings link and closed by Back, Esc or B. Its own Back calls `settingsClosed`.
   */
  openSettings?: (open: boolean) => void;
  /** The worlds' thumbnails (local/worldThumbs.ts); without it every card shows its placeholder. */
  thumbs?: { get(id: string): string | null; remove(id: string): void };
  /** Whether a world can be regenerated (the build that opens it carries out `?regenerate=`). */
  canRegenerate?(world: WorldMeta): boolean;
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

/**
 * A world's game mode as the world list shows it (owner, 2026-11-09: "The world should show in the
 * menu as creative or adventure so you can tell"): the mode it was created in (`WorldMeta.mode`,
 * ADR 0065), the default mode when it has none; a mode this build doesn't know shows its name.
 */
export function worldModeLabel(world: WorldMeta): { name: string; label: string } {
  const name = world.mode ?? DEFAULT_MODE;
  const mode = GAME_MODES.find((m) => m.name === name);
  return { name, label: mode ? mode.label : name };
}

/** A world's details line: "Terrain · seed 42 · played 3 h ago · v0.1.0". */
export function worldDetails(world: WorldMeta, now: number): string {
  const version = world.appVersion ? ` · v${world.appVersion}` : '';
  return `${typeLabel(world.type)} · seed ${String(world.seed)} · ${formatPlayed(world.lastPlayedAt, now)}${version}`;
}

/** What a world's card shows: the mode badge, "Terrain · played 3 h ago", and its seed and version. */
export function worldCardData(
  world: WorldMeta,
  now: number,
): { mode: string; modeLabel: string; details: string; note: string } {
  const mode = worldModeLabel(world);
  const version = world.appVersion ? ` · last played in v${world.appVersion}` : '';
  return {
    mode: mode.name,
    modeLabel: mode.label,
    details: `${typeLabel(world.type)} · ${formatPlayed(world.lastPlayedAt, now)}`,
    note: `Seed ${String(world.seed)}${version}`,
  };
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

/** The New world form's values, as the player left them (empty = the default). */
export interface NewWorldValues {
  name: string;
  seed: string;
  type: string;
  mode: string;
  spawn: string;
}

/** The route the New world form opens: the build that opens it creates the world. */
export function newWorldRoute(v: NewWorldValues): Record<string, string> {
  return {
    new: '1',
    name: cleanName(v.name, 'New world'),
    seed: v.seed.trim(),
    type: v.type,
    ...(v.spawn ? { spawn: v.spawn } : {}),
    ...(v.mode && v.mode !== DEFAULT_MODE ? { mode: v.mode } : {}),
  };
}

/** A page's title row: Back, the title, and actions at the end. */
function pageHead(title: string, onBack: () => void, ...actions: HTMLElement[]): HTMLElement {
  const head = document.createElement('div');
  head.className = 'mm-page-head';
  const backButton = button('Back', 'mm-back', onBack);
  const h = document.createElement('h2');
  h.textContent = title;
  h.tabIndex = -1;
  head.append(backButton, h, ...actions);
  return head;
}

/** `sessionStorage`, or null where it is blocked. */
function session(): Storage | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

export class MainMenu {
  readonly root = document.createElement('div');
  private readonly list = document.createElement('ul');
  private readonly message = document.createElement('p');
  private readonly form = document.createElement('form');
  /** Worlds saved before versioned releases (RELEASES.md §6): listed only to be deleted. */
  private readonly legacy = document.createElement('section');
  /** The panorama behind the menu (ui/menuBackground.ts). */
  private readonly panorama: MenuPanorama;
  /** The view shown (ui/menuNav.ts), and each view's element. */
  private view: MenuView;
  private readonly views = new Map<MenuView, HTMLElement>();
  private readonly shell = document.createElement('div');
  /** The Worlds page's own New world, hidden while the empty state offers its big one. */
  private newWorldHead!: HTMLButtonElement;

  constructor(
    parent: HTMLElement,
    private readonly deps: MainMenuDeps,
  ) {
    this.root.id = 'main-menu';
    // The sky and the terraced hills behind the menu (CSS), under its content.
    const backdrop = document.createElement('div');
    backdrop.className = 'mm-backdrop';
    backdrop.setAttribute('aria-hidden', 'true');
    // Over it, once loaded: the panorama of a real spot, turning slowly.
    this.panorama = new MenuPanorama(deps.background ?? DEFAULT_MENU_BACKGROUND);
    backdrop.append(this.panorama.root);
    this.message.id = 'menu-message';
    this.message.setAttribute('aria-live', 'polite');
    this.message.textContent = deps.message ?? '';

    // The home column, and the pages it opens over the same panorama, each with Back.
    this.list.id = 'world-list';
    this.buildForm();
    this.legacy.id = 'legacy-worlds';
    this.shell.className = 'mm-shell';
    const home = this.homeView();
    this.views.set('home', home);
    const pages = document.createElement('div');
    pages.className = 'mm-pages';
    const add = (view: MenuView, el: HTMLElement) => {
      this.views.set(view, el);
      pages.append(el);
    };
    add('worlds', this.worldsPage());
    add('new-world', this.newWorldPage());
    add('online', this.onlinePage());
    add('versions', this.versionsPage());
    this.shell.append(home, pages, this.message);
    const logo = this.logoView();
    this.views.set('logo', logo);
    this.root.append(backdrop, logo, this.shell);
    parent.append(this.root);
    this.view = firstView(introSeen(session()), Boolean(deps.message));
    this.show(this.view, false);
    this.render();
    void this.adoptSavedWorlds();
  }

  /** The view shown: `logo`, `home` or a page (ui/menuNav.ts). */
  get current(): MenuView {
    return this.view;
  }

  /** Back to the page's parent (Esc, a controller's B); on an intro screen, on to the next. */
  back(): void {
    if (isIntro(this.view)) this.leaveIntro();
    else this.show(back(this.view));
  }

  private leaveIntro(): void {
    markIntroSeen(session());
    this.show(afterIntro(this.view));
  }

  /** Shows a view: the home column stays beside a page on wide screens; the logo stands alone. */
  /** Settings' own Back closed it: the menu leaves its `settings` page for the home. */
  settingsClosed(): void {
    if (this.view === 'settings') this.show(back('settings'));
  }

  private show(view: MenuView, focus = true): void {
    const from = this.view;
    this.view = view;
    this.root.dataset.view = view;
    // Settings is ui/settingsMenu.ts's page, drawn over the menu's panorama (menu M3).
    if (view === 'settings') this.deps.openSettings?.(true);
    else if (from === 'settings') this.deps.openSettings?.(false);
    for (const [v, el] of this.views) {
      if (v !== 'home') el.hidden = v !== view;
    }
    this.shell.hidden = isIntro(view);
    this.root.classList.toggle('mm-on-page', !isIntro(view) && view !== 'home');
    // The home's action for the page shown (or its parent's, for New world) reads as pressed.
    const section = view === 'new-world' ? 'worlds' : view;
    for (const opener of this.root.querySelectorAll<HTMLElement>('.mm-home [data-opens]')) {
      if (opener.dataset.opens === section) opener.setAttribute('aria-current', 'page');
      else opener.removeAttribute('aria-current');
    }
    if (!focus) {
      if (view === 'logo') this.views.get('logo')?.focus();
      return;
    }
    if (view === 'home') {
      // Back on the home: focus the action that opened the page left.
      const opener = this.root.querySelector<HTMLElement>(`[data-opens="${from}"]`);
      (opener ?? this.root.querySelector<HTMLElement>('.mm-action'))?.focus();
    } else this.views.get(view)?.querySelector<HTMLElement>('h2')?.focus();
  }

  /** The logo screen: the wordmark over the turning panorama, waiting for a key or a tap. */
  private logoView(): HTMLElement {
    const screen = document.createElement('div');
    screen.id = 'menu-logo';
    screen.className = 'mm-logo';
    screen.tabIndex = 0;
    screen.setAttribute('role', 'button');
    const touch = typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches;
    const prompt = touch ? 'Tap to start' : 'Press any key';
    screen.setAttribute('aria-label', `Dwell. ${prompt}`);
    const title = document.createElement('h1');
    title.className = 'wordmark';
    title.textContent = 'Dwell';
    const press = document.createElement('p');
    press.className = 'mm-press';
    press.textContent = prompt;
    screen.append(title, press);
    // A click, not a pointerdown: the press then can't land on the home's buttons beneath. The
    // same gesture unlocks audio (audio/engine.ts listens for it on the document).
    screen.addEventListener('click', () => {
      if (isIntro(this.view)) this.leaveIntro();
    });
    document.addEventListener('keydown', (e) => {
      if (!isIntro(this.view) || e.key === 'Escape' || e.repeat) return;
      if (e.key === 'Tab' || e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      this.leaveIntro();
    });
    return screen;
  }

  /** The home: a column with the small logo, the two big actions, and Settings and Versions. */
  private homeView(): HTMLElement {
    const nav = document.createElement('nav');
    nav.id = 'menu-home';
    nav.className = 'mm-home mm-glass';
    nav.setAttribute('aria-label', 'Main menu');
    const title = document.createElement('h1');
    title.className = 'wordmark mm-wordmark-small';
    title.textContent = 'Dwell';
    const tagline = document.createElement('p');
    tagline.className = 'mm-tagline';
    tagline.textContent = 'Build a world, alone or with friends.';
    const action = (id: string, label: string, note: string, opens: MenuView) => {
      const b = button('', 'mm-action', () => {
        this.show(opens);
      });
      b.id = id;
      b.dataset.opens = opens;
      const name = document.createElement('span');
      name.className = 'mm-action-name';
      name.textContent = label;
      const small = document.createElement('span');
      small.className = 'mm-action-note';
      small.textContent = note;
      b.append(name, small);
      return b;
    };
    const actions = document.createElement('div');
    actions.className = 'mm-actions';
    actions.append(
      action('home-worlds', 'Worlds', 'Play yours or make a new one', 'worlds'),
      action('home-online', 'Play with others', 'Join a friend or a server', 'online'),
    );
    const links = document.createElement('div');
    links.className = 'mm-links';
    if (this.deps.openSettings) {
      const settings = button('Settings', 'mm-link', () => {
        this.show('settings');
      });
      settings.id = 'menu-settings';
      settings.dataset.opens = 'settings';
      settings.setAttribute('aria-controls', 'settings-menu');
      links.append(settings);
    }
    const versions = button('Versions', 'mm-link', () => {
      this.show('versions');
    });
    versions.id = 'home-versions';
    versions.dataset.opens = 'versions';
    links.append(versions);
    nav.append(title, tagline, actions, links, this.aboutSection());
    return nav;
  }

  private page(id: string, ...children: HTMLElement[]): HTMLElement {
    const page = document.createElement('section');
    page.id = id;
    page.className = 'mm-page mm-glass';
    page.hidden = true;
    page.append(...children);
    return page;
  }

  /** Worlds: the list, New world (its own page), and the legacy worlds. */
  private worldsPage(): HTMLElement {
    const create = button('New world', 'block-button', () => {
      this.show('new-world');
    });
    create.id = 'new-world';
    create.dataset.opens = 'new-world';
    this.newWorldHead = create;
    return this.page(
      'menu-page-worlds',
      pageHead(
        'Worlds',
        () => {
          this.show(back('worlds'));
        },
        create,
      ),
      this.list,
      this.legacy,
    );
  }

  /** New world: the basics up top, the rest under More options (Play advanced's overrides). */
  private newWorldPage(): HTMLElement {
    const more = document.createElement('details');
    more.id = 'more-options';
    more.className = 'mm-more';
    const summary = document.createElement('summary');
    summary.id = 'play-advanced-toggle';
    summary.textContent = 'More options';
    // Play advanced: a one-off world by link with overrides (ui/playAdvanced.ts).
    const advanced = playAdvancedPanel((route) => {
      this.deps.go(route);
    });
    advanced.hidden = false;
    more.append(summary, advanced);
    return this.page(
      'menu-page-new-world',
      pageHead('New world', () => {
        this.show(back('new-world'));
      }),
      this.form,
      more,
    );
  }

  /** Play with others: join by code, address or link; games nearby; recent servers; the lobby. */
  private onlinePage(): HTMLElement {
    const page = this.page(
      'menu-page-online',
      pageHead('Play with others', () => {
        this.show(back('online'));
      }),
      this.joinSection(),
    );
    const master = this.deps.master;
    if (master) {
      const browser = new ServerBrowser({
        lobby: (query) => master.lobby(query),
        ping: (server) => master.ping(server),
        go: (route) => {
          this.deps.go(route);
        },
        protocol: this.deps.protocol,
        version: this.deps.version,
      });
      page.append(browser.root);
    }
    return page;
  }

  /** Shows another background (Settings changed it). */
  setBackground(name: string): void {
    this.panorama.show(name);
  }

  /**
   * Versions (owner, 2026-10-28: a first-class part of the menu, not a footer link): this build,
   * what changed in it (shared/releases/changes.json) and the launcher's page of every build
   * (RELEASES.md §5).
   */
  private versionsPage(): HTMLElement {
    const all = document.createElement('a');
    all.id = 'versions-link';
    all.className = 'quiet-button';
    all.href = this.deps.versionsHref;
    all.textContent = 'All versions';
    const card = this.page(
      'versions-card',
      pageHead(
        'Versions',
        () => {
          this.show(back('versions'));
        },
        all,
      ),
    );
    card.classList.add('mm-versions');
    const current = document.createElement('div');
    current.className = 'mm-version-current';
    const { version, channel } = this.deps;
    current.textContent = `Dwell ${version} · ${channel === 'stable' ? 'this build' : `${channel} build`}`;
    card.append(current);
    const changes = changesFor(version);
    const view = changes && changesView([{ version, changes }]);
    if (view) {
      const label = document.createElement('div');
      label.className = 'mm-version-label';
      label.textContent = "What's new";
      card.append(label, view);
    }
    return card;
  }

  /** The version, and the About details: licence and third-party notices (RELEASES.md §2). */
  private aboutSection(): HTMLElement {
    const section = document.createElement('section');
    section.id = 'about';
    const line = document.createElement('p');
    line.className = 'menu-hint';
    const { version, channel } = this.deps;
    line.append(
      `Dwell ${version} (${channel}) · `,
      button('About', 'menu-link', () => {
        details.hidden = !details.hidden;
      }),
    );
    line.id = 'app-version';
    const details = document.createElement('div');
    details.id = 'about-details';
    details.hidden = true;
    const text = document.createElement('p');
    text.className = 'menu-hint';
    text.textContent = `${this.deps.detail} Dwell is not open source: all rights reserved, and the source is public for reference only. `;
    const notices = document.createElement('a');
    notices.href = this.deps.noticesHref;
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
    // Where the world's first spawn is (Play advanced's Spawn choices, shared/play/overrides.json).
    const spawn = document.createElement('select');
    spawn.id = 'world-spawn';
    for (const s of spawnChoices()) {
      const option = document.createElement('option');
      option.value = s.value;
      option.textContent = s.label;
      spawn.append(option);
    }
    // The world's game mode (ADR 0065, shared/modes/modes.json); experimental ones say so.
    const mode = document.createElement('select');
    mode.id = 'world-mode';
    for (const m of GAME_MODES) {
      const option = document.createElement('option');
      option.value = m.name;
      option.textContent = modeLabel(m);
      if (m.description) option.title = m.description;
      if (m.name === DEFAULT_MODE) option.selected = true;
      mode.append(option);
    }
    const submit = document.createElement('button');
    submit.type = 'submit';
    submit.className = 'block-button';
    submit.textContent = 'Create and play';
    const hint = document.createElement('p');
    hint.className = 'menu-hint';
    hint.textContent = 'Seed: a number, or any text. The same seed makes the same world.';
    f.append(
      field('Name', name),
      field('Seed', seed),
      field('Type', type),
      field('Mode', mode),
      field('Spawn', spawn),
      hint,
      submit,
    );
    f.addEventListener('submit', (e) => {
      e.preventDefault();
      // The build that opens it creates it (its generator, its version).
      this.deps.go(
        newWorldRoute({
          name: name.value,
          seed: seed.value,
          type: type.value,
          mode: mode.value,
          spawn: spawn.value,
        }),
      );
    });
  }

  private joinSection(): HTMLElement {
    const section = document.createElement('section');
    section.className = 'menu-join';
    const form = document.createElement('form');
    form.className = 'menu-join-form';
    const input = document.createElement('input');
    input.id = 'join-input';
    input.placeholder = 'Join code, address or invite link';
    input.setAttribute('aria-label', 'Join code, server address or invite link');
    input.autocapitalize = 'characters';
    input.autocomplete = 'off';
    const submit = document.createElement('button');
    submit.type = 'submit';
    submit.className = 'block-button';
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
            audio().ui('error');
          },
        );
      } else {
        this.say(
          'That is not a join code, a server address or an invite link. A host shows its code (e.g. KQ7-XM4); a server prints its code and a link when it starts.',
        );
        audio().ui('error');
      }
    });
    section.append(form);
    if (!this.deps.master && this.deps.recent.length === 0) {
      // Nothing else to list here: say what to join with, so the page reads as finished.
      const help = document.createElement('p');
      help.className = 'menu-join-help';
      help.textContent =
        'A host shows a join code like KQ7-XM4. A server prints its code and an invite link when it starts. Paste either above.';
      section.append(help);
    }
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
    this.list.classList.toggle('world-cards', worlds.length > 0);
    this.newWorldHead.hidden = worlds.length === 0;
    if (worlds.length === 0) {
      this.list.append(this.emptyState());
      return;
    }
    for (const world of worlds) this.list.append(this.worldCard(world, now));
  }

  /** No worlds yet: a line and a big New world, not a strip. */
  private emptyState(): HTMLElement {
    const empty = document.createElement('li');
    empty.className = 'menu-empty';
    const art = document.createElement('div');
    art.className = 'menu-empty-art';
    art.setAttribute('aria-hidden', 'true');
    const line = document.createElement('p');
    line.className = 'menu-empty-line';
    line.textContent = 'No worlds yet.';
    const sub = document.createElement('p');
    sub.className = 'menu-empty-sub';
    sub.textContent = 'Make one: pick a name and a type, or keep the defaults.';
    const create = button('New world', 'block-button menu-empty-create', () => {
      this.show('new-world');
    });
    create.dataset.opens = 'new-world';
    empty.append(art, line, sub, create);
    return empty;
  }

  /** A world's card: its last view (or a placeholder by type), name, mode, type and Play. */
  private worldCard(world: WorldMeta, now: number): HTMLElement {
    const card = worldCardData(world, now);
    const item = document.createElement('li');
    item.className = 'world-item world-card';
    item.dataset.id = world.id;
    const thumb = document.createElement('div');
    thumb.className = 'world-thumb';
    thumb.dataset.type = world.type;
    const image = this.deps.thumbs?.get(world.id) ?? null;
    if (image) {
      const img = document.createElement('img');
      img.src = image;
      img.alt = '';
      img.decoding = 'async';
      thumb.append(img);
    } else thumb.classList.add('world-thumb-empty');
    const badge = document.createElement('span');
    badge.className = 'world-mode';
    badge.dataset.mode = card.mode;
    badge.textContent = card.modeLabel;
    thumb.append(badge);
    const info = document.createElement('div');
    info.className = 'world-info';
    const name = document.createElement('span');
    name.className = 'world-name';
    name.textContent = world.name;
    const details = document.createElement('span');
    details.className = 'world-details';
    details.textContent = card.details;
    info.append(name, details);

    // Play is the card's action; the rest sit under the "⋯" button.
    const more = document.createElement('div');
    more.className = 'world-menu';
    more.hidden = true;
    more.id = `world-menu-${world.id}`;
    const about = document.createElement('p');
    about.className = 'world-menu-note';
    about.textContent = card.note;
    more.append(
      about,
      ...((this.deps.canRegenerate?.(world) ?? true)
        ? [
            confirmButton('Regenerate', 'Lose all changes?', () => {
              this.deps.go({ regenerate: world.id });
            }),
          ]
        : []),
      confirmButton('Delete', 'Delete forever?', () => void this.remove(world)),
    );
    const toggle = button('⋯', 'world-more', () => {
      more.hidden = !more.hidden;
      toggle.setAttribute('aria-expanded', String(!more.hidden));
    });
    toggle.setAttribute('aria-label', `More for ${world.name}`);
    toggle.setAttribute('aria-expanded', 'false');
    toggle.setAttribute('aria-controls', more.id);
    const actions = document.createElement('div');
    actions.className = 'world-actions';
    actions.append(
      button('Play', 'block-button world-play', () => {
        this.deps.go({ play: world.id });
      }),
      toggle,
    );
    item.append(thumb, info, actions, more);
    return item;
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

  private async remove(world: WorldMeta): Promise<void> {
    if (!(await this.deps.deleteFiles(world.id))) {
      this.say(`"${world.name}" is open in another tab. Close it there first.`);
      return;
    }
    this.deps.index.remove(world.id);
    this.deps.thumbs?.remove(world.id);
    this.say(`Deleted "${world.name}".`);
    this.render();
  }
}
