// The launcher (RELEASES.md §5): `https://dropkickarcade.com/dwell/`, the one page that is not
// versioned. It reads `versions.json`, picks the build the address needs (a world's, a host's or
// the latest) and replaces itself with it, keeping the query. Everything else is an error page
// that offers the latest version.
import { isBuildOf, parseManifest, type Manifest } from './manifest';
import {
  choose,
  forwardedSearch,
  isPlainVisit,
  latestBuild,
  listBuilds,
  parseWorldIndex,
  VERSIONS_PARAM,
  type Channel,
} from './select';

// The player's dev-builds choice, kept by the version page below.
const CHANNEL_KEY = 'dwell.channel';
const WORLDS_KEY = 'dwell.worlds';

const status = document.getElementById('launcher-status');
const actions = document.getElementById('launcher-actions');

/** Local storage, or null where it is blocked. */
function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The channel the player chose on the version page; stable unless they chose dev. */
function channel(): Channel {
  return storage()?.getItem(CHANNEL_KEY) === 'dev' ? 'dev' : 'stable';
}

/** The app version that last played a local world (the menu's index, `dwell.worlds`). */
function worldVersion(id: string): string | null {
  try {
    const raw: unknown = JSON.parse(storage()?.getItem(WORLDS_KEY) ?? '[]');
    if (!Array.isArray(raw)) return null;
    for (const entry of raw as unknown[]) {
      if (typeof entry !== 'object' || entry === null) continue;
      const record = entry as { id?: unknown; appVersion?: unknown };
      if (record.id === id) return typeof record.appVersion === 'string' ? record.appVersion : null;
    }
  } catch {
    // damaged index: no version known, the build explains
  }
  return null;
}

function show(message: string, buttons: { label: string; run: () => void }[] = []): void {
  if (status) status.textContent = message;
  if (!actions) return;
  actions.replaceChildren();
  for (const b of buttons) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = b.label;
    button.addEventListener('click', b.run);
    actions.append(button);
  }
}

function buildUrl(version: string): URL {
  return new URL(`v/${encodeURIComponent(version)}/`, new URL('./', location.href));
}

async function manifestOf(): Promise<Manifest | null> {
  try {
    const response = await fetch(new URL('versions.json', new URL('./', location.href)), {
      cache: 'no-cache',
    });
    return response.ok ? parseManifest(await response.json()) : null;
  } catch {
    return null;
  }
}

/**
 * Whether a version's directory is there: its `build.json` loads and names that version (a host
 * may answer a missing path with a page of its own, so a good status is not enough).
 */
async function published(version: string): Promise<boolean> {
  try {
    const response = await fetch(new URL('build.json', buildUrl(version)), { cache: 'no-cache' });
    if (!response.ok) return false;
    return isBuildOf(await response.json(), version);
  } catch {
    return false;
  }
}

function openLatest(manifest: Manifest): void {
  const latest = latestBuild(manifest, channel());
  if (latest) location.replace(buildUrl(latest.version));
}

/**
 * The version page (RELEASES.md §5, Phase 6b): every published build, to open one for this visit.
 * Nothing is remembered, so Back to the menu still lands on the latest.
 */
function showVersions(manifest: Manifest): void {
  const root = document.getElementById('launcher');
  if (!root) return;
  const worlds = parseWorldIndex(storage()?.getItem(WORLDS_KEY) ?? null);
  const showDev = channel() === 'dev';
  const rows = listBuilds(manifest, showDev, worlds);
  if (status) status.textContent = 'Choose a version to play. Worlds stay with their version line.';
  actions?.replaceChildren();

  const toggle = document.createElement('input');
  toggle.type = 'checkbox';
  toggle.id = 'use-dev-builds';
  toggle.checked = showDev;
  toggle.addEventListener('change', () => {
    try {
      storage()?.setItem(CHANNEL_KEY, toggle.checked ? 'dev' : 'stable');
    } catch {
      // storage blocked: the choice isn't kept
    }
    showVersions(manifest);
  });
  const label = document.createElement('label');
  label.id = 'dev-toggle';
  label.append(
    toggle,
    ' Show dev builds (unfinished; a world made in one stays in that exact build)',
  );

  const list = document.createElement('ul');
  list.id = 'version-list';
  for (const row of rows) {
    const item = document.createElement('li');
    item.className = 'version-row';
    item.dataset.version = row.version;
    const head = document.createElement('div');
    head.className = 'version-head';
    const name = document.createElement('strong');
    name.textContent = `Dwell ${row.version}`;
    const meta = document.createElement('span');
    meta.className = 'version-meta';
    meta.textContent = [
      row.recommended ? 'recommended' : row.channel,
      row.date.slice(0, 10),
      `line ${row.line}`,
    ].join(' · ');
    const open = document.createElement('button');
    open.type = 'button';
    open.className = 'version-open';
    open.textContent = 'Play';
    open.disabled = !row.openable;
    open.addEventListener('click', () => {
      void openBuild(manifest, row.version);
    });
    head.append(name, meta, open);
    const note = document.createElement('p');
    note.className = 'version-worlds';
    note.textContent = row.openable
      ? row.worlds.length
        ? `Can open: ${row.worlds.join(', ')}`
        : 'Opens none of your worlds yet.'
      : 'Needs a newer launcher: reload the page.';
    item.append(head, note);
    list.append(item);
  }
  root.querySelector('#dev-toggle')?.remove();
  root.querySelector('#version-list')?.remove();
  root.append(label, list);
}

/** Opens a build from the version page, checking first that it is there. */
async function openBuild(manifest: Manifest, version: string): Promise<void> {
  if (await published(version)) {
    location.assign(buildUrl(version));
    return;
  }
  show(`Dwell ${version} couldn't be loaded. It may have been removed.`, [
    {
      label: 'Back to the list',
      run: () => {
        showVersions(manifest);
      },
    },
  ]);
}

/**
 * The screen a plain visit lands on (Phase 6b): the launcher is unversioned, so this is where the
 * version choice lives whatever build was played last. Nothing happens until a button is pressed;
 * Play opens the latest build (the stable one unless the player opted into dev builds).
 */
function landing(version: string): Promise<'play' | 'pick'> {
  return new Promise((resolve) => {
    show('Welcome to Dwell.', [
      {
        label: `Play Dwell ${version}`,
        run: () => {
          resolve('play');
        },
      },
      {
        label: 'Choose version',
        run: () => {
          resolve('pick');
        },
      },
    ]);
    actions?.querySelector('button')?.focus();
  });
}

async function run(): Promise<void> {
  const manifest = await manifestOf();
  if (!manifest) {
    show("Dwell couldn't load its list of versions. Check your connection and try again.", [
      {
        label: 'Try again',
        run: () => {
          location.reload();
        },
      },
    ]);
    return;
  }
  if (new URLSearchParams(location.search).has(VERSIONS_PARAM)) {
    showVersions(manifest);
    return;
  }
  const choice = choose(manifest, { search: location.search, worldVersion, channel: channel() });
  const latest = latestBuild(manifest, channel());
  const offerLatest = latest
    ? [
        {
          label: `Open Dwell ${latest.version}`,
          run: () => {
            openLatest(manifest);
          },
        },
      ]
    : [];
  if (choice.kind === 'error') {
    show(choice.message, offerLatest);
    return;
  }
  if (choice.why === 'latest' && isPlainVisit(location.search)) {
    if ((await landing(choice.version)) === 'pick') {
      showVersions(manifest);
      return;
    }
  }
  if (!(await published(choice.version))) {
    show(`Dwell ${choice.version} couldn't be loaded. It may have been removed.`, [
      {
        label: 'Try again',
        run: () => {
          location.reload();
        },
      },
      ...offerLatest,
    ]);
    return;
  }
  const target = buildUrl(choice.version);
  location.replace(`${target.href}${forwardedSearch(location.search)}${location.hash}`);
}

void run();
