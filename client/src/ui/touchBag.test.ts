import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// Owner, 2026-11-10: "How to open inventory on mobile?" The touch Bag button (0.52.23) sat at the
// same safe corner as the menu button, both 44 px, so the menu button covered it.
const css = readFileSync(join(__dirname, '..', 'style.css'), 'utf8');

function rule(selector: string): string {
  const at = css.indexOf(`${selector} {`);
  expect(at, selector).toBeGreaterThanOrEqual(0);
  return css.slice(at, css.indexOf('}', at));
}

function prop(body: string, name: string): string {
  const m = new RegExp(`\\n\\s*${name}:\\s*([^;]+);`).exec(body);
  return m?.[1]?.trim() ?? '';
}

describe('the touch Bag button', () => {
  it('is not under the menu button', () => {
    const bag = rule('#touch-bag');
    const menu = rule('#menu-button');
    const same = prop(bag, 'top') === prop(menu, 'top') && prop(bag, 'left') === prop(menu, 'left');
    expect(same).toBe(false);
  });
});
