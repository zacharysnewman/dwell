// The HUD's layout on phones (shared/ui/layout.json, ui/hudLayout.ts): on an iPhone either way up,
// flying (so the flight slider shows), no two visible parts of the on-screen interface overlap and
// each stays on screen. Before the layout solver the minimap covered the hotbar, the status and
// performance lines and the info button (owner's screenshots, 0.50.39).
import { devices, expect, test, type Page } from '@playwright/test';
import { untilTicking } from './ticking';

/** The parts the layout places (shared/ui/layout.json's selectors). */
const PARTS = [
  '#menu-button',
  '#touch-debug',
  '#minimap',
  '#hotbar',
  '#net-status',
  '#perf-status',
  '#flight-speed',
  '.touch-buttons',
  '#health',
  '#status-chip',
  '#crosshair',
];

interface Box {
  sel: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

const boxes = (page: Page) =>
  page.evaluate((sels) => {
    const out: { sel: string; x: number; y: number; w: number; h: number }[] = [];
    for (const sel of sels) {
      const el = document.querySelector(sel);
      if (!el) continue;
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) continue;
      // A part the layout found no room for is hidden: a failure here, as an overlap is.
      if (getComputedStyle(el).visibility === 'hidden')
        out.push({ sel: `${sel} (no room)`, x: -1, y: -1, w: 0, h: 0 });
      out.push({ sel, x: r.left, y: r.top, w: r.width, h: r.height });
    }
    return out;
  }, PARTS);

function overlaps(a: Box, b: Box): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

for (const name of ['iPhone 13', 'iPhone 13 landscape'] as const) {
  test.describe(name, () => {
    // Chromium is the suite's only browser (playwright.config.ts): a device's own browser type is left out.
    const device: Partial<(typeof devices)[string]> = { ...devices[name] };
    delete device.defaultBrowserType;
    test.use(device);

    test(`hud layout: no two parts overlap on an ${name}`, async ({ page }) => {
      test.setTimeout(60_000);
      await page.goto('./?world=flat');
      await expect
        .poll(
          () =>
            page.evaluate<boolean>(
              '(globalThis.__dwell && globalThis.__dwell.state() && globalThis.__dwell.state().active) || false',
            ),
          { timeout: 20_000 },
        )
        .toBe(true);
      await untilTicking(page);
      await page.evaluate('globalThis.__dwell.fly(true)');
      await expect(page.locator('#flight-speed')).toBeVisible();
      await expect(page.locator('#minimap')).toBeVisible();
      await expect(page.locator('#net-status')).not.toBeEmpty();
      await expect(page.locator('#perf-status')).not.toBeEmpty({ timeout: 10_000 });
      // The layout runs a few times a second: let it settle on the lines' text.
      await page.waitForTimeout(600);
      const all = await boxes(page);
      const vp = page.viewportSize() ?? { width: 0, height: 0 };
      const problems: string[] = [];
      for (const b of all) {
        if (b.x < -0.5 || b.y < -0.5 || b.x + b.w > vp.width + 0.5 || b.y + b.h > vp.height + 0.5)
          problems.push(`${b.sel} off screen ${JSON.stringify(b)}`);
      }
      for (let i = 0; i < all.length; i++)
        for (let j = i + 1; j < all.length; j++) {
          const a = all[i];
          const b = all[j];
          if (overlaps(a, b)) problems.push(`${a.sel} overlaps ${b.sel}`);
        }
      expect(problems, JSON.stringify(all)).toEqual([]);
    });
  });
}
