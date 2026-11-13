// WORK_QUEUE item 22 (owner, 2026-11-11: "The buttons should be aligned to match the design of the
// others instead of looking like web links"): every action on the main menu is one of the menu's own
// buttons, a `<button>` or a link to another page alike, and each of those classes has its hover,
// pressed and focus states. Read from the source (the tests run without a DOM).
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { MENU_BUTTON_CLASSES } from './mainMenu';

const source = readFileSync(new URL('./mainMenu.ts', import.meta.url), 'utf8');
const css = readFileSync(new URL('../style.css', import.meta.url), 'utf8');

const isMenuButton = (classes: string) =>
  classes.split(/\s+/).some((c) => (MENU_BUTTON_CLASSES as readonly string[]).includes(c));

/** A string literal, as the first argument of a call. */
const LABEL = String.raw`(?:'(?:[^'\\]|\\.)*'|\x60[^\x60]*\x60|[\w.]+)`;

/** Every selector list in style.css with its rule's body. */
function rules(): { selectors: string[]; body: string }[] {
  const out: { selectors: string[]; body: string }[] = [];
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const m of bare.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    out.push({ selectors: (m[1] ?? '').split(',').map((s) => s.trim()), body: m[2] ?? '' });
  }
  return out;
}

describe("the main menu's actions are its own buttons, not web links", () => {
  it('creates links only through the menu link button', () => {
    // One `<a>` factory (linkButton), so no bare link can be added beside it.
    expect(source.match(/createElement\('a'\)/g)).toHaveLength(1);
    expect(source).toMatch(
      /function linkButton\([^)]*\): HTMLAnchorElement \{\n {2}const a = document\.createElement\('a'\);/,
    );
  });

  it('gives every button and link button a menu button class', () => {
    // button(label, class, …) and linkButton(label, href, class, …), not their definitions.
    const calls = [
      ...source.matchAll(new RegExp(String.raw`\bbutton\(\s*${LABEL},\s*([^,)]+)`, 'g')),
      ...source.matchAll(
        new RegExp(String.raw`\blinkButton\(\s*${LABEL},\s*[^,]+,\s*([^,)]+)`, 'g'),
      ),
    ].filter((m) => !/^(button|linkButton)\(label/.test(m[0]));
    // Back, the home's actions, Settings, Versions, About, All versions, the notices, the world cards'.
    expect(calls.length).toBeGreaterThanOrEqual(10);
    for (const m of calls) {
      const classes = m.at(-1)?.trim() ?? '';
      expect(classes, m[0]).toMatch(/^'[^']*'$/);
      expect(isMenuButton(classes.slice(1, -1)), `${m[0]}: not a menu button class`).toBe(true);
    }
    // Buttons made by hand take a class too.
    for (const m of source.matchAll(/const (\w+) = document\.createElement\('button'\);/g)) {
      const name = m[1] ?? '';
      if (name === 'b') continue; // button() itself
      const set = new RegExp(String.raw`\b${name}\.className = '([^']*)'`).exec(source);
      expect(set, `${name} has no class`).not.toBeNull();
      expect(isMenuButton(set?.[1] ?? ''), `${name}: ${set?.[1] ?? ''}`).toBe(true);
    }
  });

  it('styles no menu control as an underlined link', () => {
    for (const r of rules()) {
      if (!/text-decoration:\s*underline/.test(r.body)) continue;
      for (const s of r.selectors) {
        expect(s, 'an underlined menu control').not.toMatch(/mm-|menu-|#about|#versions/);
      }
    }
  });

  it('has hover, pressed and focus states for every menu button class', () => {
    const selectors = rules().flatMap((r) => r.selectors);
    const has = (s: string) => selectors.some((x) => x === s || x.endsWith(` ${s}`));
    // The page-wide focus ring covers every button and link.
    expect(has('button:focus-visible') && has('a:focus-visible')).toBe(true);
    for (const c of MENU_BUTTON_CLASSES) {
      expect(has(`.${c}:hover`), `.${c}:hover`).toBe(true);
      expect(has(`.${c}:active`), `.${c}:active`).toBe(true);
    }
    // A link styled as a button loses the underline and takes the button's box.
    const link = rules().find((r) => r.selectors.includes('a.menu-button-small'));
    expect(link?.body).toMatch(/text-decoration:\s*none/);
  });
});
