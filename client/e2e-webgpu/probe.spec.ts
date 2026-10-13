// Headless WebGPU works on this machine (Phase 24a): the Rust client's probe draws a triangle with
// wgpu offscreen and into a canvas, and the offscreen pixels read back as drawn.
import { expect, test } from '@playwright/test';

interface Probe {
  pixels?: number[];
  adapter?: string;
  error?: string;
}

const SIZE = 64;
const CLEAR = [0, 0, 255, 255];
const TRIANGLE = [255, 128, 0, 255];

test('wgpu draws offscreen and to a canvas', async ({ page }) => {
  await page.goto('./');
  const probe = (
    await page.waitForFunction(() => (window as { __probe?: unknown }).__probe)
  ).jsonValue();
  const result = (await probe) as Probe;
  expect(result.error).toBeUndefined();
  const pixels = result.pixels ?? [];
  expect(pixels).toHaveLength(SIZE * SIZE * 4);
  const pixel = (x: number, y: number) => pixels.slice((y * SIZE + x) * 4, (y * SIZE + x) * 4 + 4);
  expect(pixel(SIZE / 2, SIZE / 2)).toEqual(TRIANGLE);
  expect(pixel(1, 1)).toEqual(CLEAR);
  expect(result.adapter).toBeTruthy();
});
