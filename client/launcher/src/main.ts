// The launcher (RELEASES.md §5): `https://dropkickarcade.com/dwell/`, the one page that is not
// versioned. It reads `versions.json`, picks the build the address needs (a world's, a host's or
// the latest) and replaces itself with it, keeping the query. Everything else is an error page
// that offers the latest version.
import { isBuildOf, parseManifest, type Manifest } from './manifest';
import { choose, forwardedSearch, latestBuild, type Channel } from './select';

// Written by the game's menu (src/ui/channel.ts).
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

/** The channel the player chose in the menu's About section; stable unless they chose dev. */
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
