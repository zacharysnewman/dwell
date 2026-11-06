// The warning before a world from an older version line opens in this build (owner, 2026-11-06;
// RELEASES.md §6): the player sees what changes and chooses to play a copy (the original stays on
// its version), upgrade the world itself, or go back. Shown by the build the version page chose.
import { compatibilityLine } from '../version/semver';

export type UpgradeAnswer = 'copy' | 'upgrade' | 'cancel';

export interface UpgradeText {
  title: string;
  /** What happens, one point a line. */
  points: string[];
  copy: string;
  upgrade: string;
  cancel: string;
}

/** The prompt's words for a world `name` last saved by Dwell `from`, opening in `build`. */
export function upgradeText(name: string, from: string, build: string): UpgradeText {
  const line = (v: string) => `${compatibilityLine(v) ?? v}.x`;
  return {
    title: 'This world is from an older version',
    points: [
      `“${name}” was saved by Dwell ${from} (line ${line(from)}). You are about to open it in Dwell ${build} (line ${line(build)}), which it was not made for.`,
      'Land you haven’t explored generates with this version’s terrain, so there may be cliffs or seams where it meets the land you have.',
      'Blocks this version no longer has show as placeholders.',
      `Once this version saves it, Dwell ${from} can’t open it again. Play a copy to keep the original as it is.`,
    ],
    copy: 'Play a copy',
    upgrade: 'Upgrade this world',
    cancel: 'Cancel',
  };
}

export function showUpgradePrompt(
  doc: Document,
  name: string,
  from: string,
  build: string,
): Promise<UpgradeAnswer> {
  const t = upgradeText(name, from, build);
  const root = doc.createElement('div');
  root.className = 'game-menu registry-prompt upgrade-prompt';
  root.setAttribute('role', 'alertdialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-labelledby', 'upgrade-prompt-title');
  const sheet = doc.createElement('div');
  sheet.className = 'rp-sheet';
  const title = doc.createElement('h2');
  title.id = 'upgrade-prompt-title';
  title.className = 'gm-title';
  title.textContent = `⚠ ${t.title}`;
  const list = doc.createElement('ul');
  list.className = 'rp-list up-points';
  for (const point of t.points) {
    const li = doc.createElement('li');
    li.textContent = point;
    list.append(li);
  }
  const actions = doc.createElement('div');
  actions.className = 'rp-actions';
  const button = (label: string, cls: string, answer: UpgradeAnswer) => {
    const b = doc.createElement('button');
    b.type = 'button';
    b.className = cls;
    b.textContent = label;
    b.dataset.answer = answer;
    if (answer === 'cancel') b.dataset.sound = 'back';
    actions.append(b);
    return b;
  };
  const cancel = button(t.cancel, 'menu-button-small', 'cancel');
  button(t.upgrade, 'menu-button-small', 'upgrade');
  button(t.copy, 'menu-button-main', 'copy');
  sheet.append(title, list, actions);
  root.append(sheet);
  doc.body.append(root);
  // Nothing is chosen by a stray key: focus starts on Cancel.
  cancel.focus();
  return new Promise((resolve) => {
    actions.addEventListener('click', (e) => {
      const answer = (e.target as HTMLElement | null)?.dataset.answer as UpgradeAnswer | undefined;
      if (!answer) return;
      root.remove();
      resolve(answer);
    });
  });
}
