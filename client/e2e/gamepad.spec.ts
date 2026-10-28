// Controller support (docs/GAMEPAD.md) with a scripted controller (navigator.getGamepads replaced
// before the page loads): the left stick walks the player, Menu opens the game menu, the D-pad
// moves its focus and B closes it.
import { expect, test, type Page } from '@playwright/test';

type Vec3 = [number, number, number];

function call<T>(page: Page, expr: string): Promise<T> {
  return page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);
}
const feet = (page: Page) => call<Vec3>(page, 'd.state().feet');

/** Sets the scripted pad: buttons held by standard index, and the four axes. */
async function pad(page: Page, held: number[], axes: [number, number, number, number]) {
  await page.evaluate(
    ([h, a]) => {
      (globalThis as unknown as { __pad: { held: number[]; axes: number[] } }).__pad = {
        held: h,
        axes: a,
      };
    },
    [held, axes] as const,
  );
}

test('controller: the stick walks, Menu opens the menu, the D-pad moves, B closes', async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.addInitScript(() => {
    const g = globalThis as unknown as { __pad: { held: number[]; axes: number[] } };
    g.__pad = { held: [], axes: [0, 0, 0, 0] };
    const button = (i: number) => {
      const on = g.__pad.held.includes(i);
      return { pressed: on, touched: on, value: on ? 1 : 0 };
    };
    Object.defineProperty(navigator, 'getGamepads', {
      value: () => [
        {
          id: 'scripted',
          index: 0,
          connected: true,
          mapping: 'standard',
          timestamp: performance.now(),
          axes: g.__pad.axes,
          buttons: Array.from({ length: 17 }, (_, i) => button(i)),
        },
      ],
    });
  });
  await page.goto('./?world=flat');
  await expect
    .poll(
      async () => (await call<{ active: boolean } | null>(page, 'd.state()'))?.active ?? false,
      {
        timeout: 30_000,
      },
    )
    .toBe(true);
  await page.waitForTimeout(1000);
  const start = await feet(page);
  // Left stick up: forward.
  await pad(page, [], [0, -1, 0, 0]);
  await page.waitForTimeout(1500);
  await pad(page, [], [0, 0, 0, 0]);
  await page.waitForTimeout(500);
  const end = await feet(page);
  expect(Math.hypot(end[0] - start[0], end[2] - start[2])).toBeGreaterThan(1);

  // Menu (index 9) opens the game menu.
  const menu = page.locator('#settings-menu');
  await expect(menu).toBeHidden();
  await pad(page, [9], [0, 0, 0, 0]);
  await expect(menu).toBeVisible();
  await pad(page, [], [0, 0, 0, 0]);
  // D-pad down (13) focuses a control in it. Pressed and released until it does: CI's software
  // renderer draws a few frames a second, the pad is read once a frame, and a press is an edge.
  await expect(async () => {
    await pad(page, [13], [0, 0, 0, 0]);
    await page.waitForTimeout(400);
    await pad(page, [], [0, 0, 0, 0]);
    await page.waitForTimeout(400);
    expect(
      await page.evaluate(() => document.activeElement?.closest('#settings-menu') !== null),
    ).toBe(true);
  }).toPass({ timeout: 20_000 });
  // B (1) closes it.
  await pad(page, [1], [0, 0, 0, 0]);
  await expect(menu).toBeHidden();
  await pad(page, [], [0, 0, 0, 0]);
});
