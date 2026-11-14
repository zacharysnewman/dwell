// The theater panel at a phone's 360 px (owner, 2026-11-13: "... responsive layout"): built in
// happy-dom, laid out in headless Chromium with style.css, nothing may be wider than the screen.
// Node's environment (Playwright inside happy-dom's breaks on its fake origin).

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { Window } from 'happy-dom';
import { describe, expect, it } from 'vitest';

import { rig } from './panelRig';

/** The browser this checkout can launch (CI's e2e Chromium, the sandbox's, or Playwright's own). */
function chromiumPath(): string | undefined {
  const env = process.env.DWELL_CHROMIUM;
  if (env) return env;
  const sandbox = '/opt/pw-browsers/chromium';
  return existsSync(sandbox) ? sandbox : undefined;
}

describe('the theater panel at 360 px', () => {
  it('has nothing wider than the viewport, in every tool and with Advanced open', async (ctx) => {
    const win = new Window({ url: 'http://localhost/' });
    const g = globalThis as Record<string, unknown>;
    for (const k of [
      'HTMLElement',
      'HTMLInputElement',
      'HTMLSelectElement',
      'HTMLButtonElement',
      'Event',
    ])
      g[k] ??= (win as unknown as Record<string, unknown>)[k];
    const { theater, bar } = rig([], win.document as unknown as Document);
    theater.enter();
    theater.relayout(360, 740);
    for (const d of bar.querySelectorAll('details')) d.open = true;
    const html = bar.outerHTML;
    const css = readFileSync(join(__dirname, '../style.css'), 'utf8');
    let browser;
    try {
      const { chromium } = await import('@playwright/test');
      const executablePath = chromiumPath();
      browser = await chromium.launch(executablePath ? { executablePath } : {});
    } catch (e) {
      console.warn(`no Chromium to lay the panel out in: ${String(e).slice(0, 120)}`);
      ctx.skip();
      return;
    }
    try {
      const page = await browser.newPage({ viewport: { width: 360, height: 740 } });
      await page.setContent(
        `<!doctype html><html><head><style>${css}</style></head><body>${html}</body></html>`,
      );
      for (const tool of ['presets', 'keys', 'record']) {
        const wide = await page.evaluate((t) => {
          const panels = [...document.querySelectorAll<HTMLElement>('[role="tabpanel"]')];
          const order = ['presets', 'keys', 'record'];
          panels.forEach((p, i) => {
            p.hidden = order[i] !== t;
          });
          const out: string[] = [];
          for (const e of document.querySelectorAll<HTMLElement>('#theater-bar *')) {
            const r = e.getBoundingClientRect();
            if (r.width > 0 && (r.right > 360.5 || r.left < -0.5))
              out.push(
                `${e.tagName}.${e.className} ${String(Math.round(r.left))}–${String(Math.round(r.right))}`,
              );
          }
          if (document.documentElement.scrollWidth > 360)
            out.push(`page ${String(document.documentElement.scrollWidth)} px`);
          return out;
        }, tool);
        expect(wide, `${tool}: wider than 360 px`).toEqual([]);
      }
    } finally {
      await browser.close();
    }
  }, 30_000);
});
