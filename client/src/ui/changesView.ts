// What changed in a build (shared/releases/changes.json, docs/RELEASES.md §3), for the launcher's
// version page and the main menu's Versions card: the build's own lines, then any earlier builds
// it brings (retired patches) folded under "Earlier". Plain divs and lists, no <p>: iOS Safari
// offers Reader on paragraphs.
import type { VersionChanges } from '../version/changes';

function lines(changes: string[]): HTMLUListElement {
  const list = document.createElement('ul');
  list.className = 'version-changes';
  for (const c of changes) {
    const item = document.createElement('li');
    item.textContent = c;
    list.append(item);
  }
  return list;
}

/** The changes, newest first; null with none to show. */
export function changesView(entries: VersionChanges[]): HTMLElement | null {
  const [own, ...earlier] = entries;
  if (!own) return null;
  const root = document.createElement('div');
  root.className = 'version-changelog';
  root.append(lines(own.changes));
  if (earlier.length > 0) {
    const details = document.createElement('details');
    details.className = 'version-earlier';
    const summary = document.createElement('summary');
    const last = earlier[earlier.length - 1]?.version ?? '';
    summary.textContent =
      earlier.length === 1
        ? `Earlier: ${last}`
        : `Earlier: ${last} – ${earlier[0]?.version ?? ''} (${String(earlier.length)} builds)`;
    details.append(summary);
    for (const e of earlier) {
      const name = document.createElement('div');
      name.className = 'version-earlier-name';
      name.textContent = e.version;
      details.append(name, lines(e.changes));
    }
    root.append(details);
  }
  return root;
}
