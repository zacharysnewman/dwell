// The Rust renderer (Phase 24e) draws its smoke scene in headless Chromium: sky above the horizon,
// lit terrain below it, and the red player in the middle.
import { expect, test } from '@playwright/test';

const SIZE = 128;

test('the wgpu renderer draws sky, terrain and a player', async ({ page }) => {
  await page.goto('./');
  const result = (await (
    await page.waitForFunction(() => (window as { __probe?: unknown }).__probe)
  ).jsonValue()) as { scene?: number[]; error?: string };
  expect(result.error).toBeUndefined();
  const px = result.scene ?? [];
  expect(px).toHaveLength(SIZE * SIZE * 4);
  const at = (x: number, y: number) => px.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 3);
  const sky = at(SIZE / 2, 4);
  const ground = at(SIZE / 2, SIZE - 4);
  // Sky: bluish (blue the largest channel). Ground: not the sky's colour, and not black.
  expect(sky[2]).toBeGreaterThan(sky[0]);
  expect(ground.reduce((a, b) => a + b)).toBeGreaterThan(30);
  expect(Math.abs(ground[2] - sky[2]) + Math.abs(ground[1] - sky[1])).toBeGreaterThan(30);
  // The player: some reddish pixel in the middle band.
  let red = 0;
  for (let y = SIZE / 4; y < SIZE; y++)
    for (let x = SIZE / 4; x < (3 * SIZE) / 4; x++) {
      const [r, g, b] = at(x, y);
      if (r > 2 * g && r > 2 * b && r > 60) red++;
    }
  expect(red).toBeGreaterThan(4);
});
