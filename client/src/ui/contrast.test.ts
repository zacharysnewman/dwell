// The main menu's text against its glass (owner, 2026-11-09: "the contrast is off on some
// elements"): body text at least 4.5:1, read from the colours in style.css, over the darkest and
// brightest backdrops a panorama shows through the glass.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contrast, hex, over, type Rgb } from './contrast';

const css = readFileSync(new URL('../style.css', import.meta.url), 'utf8');

function token(name: string): string {
  const m = new RegExp(`--${name}:\\s*([^;]+);`).exec(css);
  if (!m?.[1]) throw new Error(`no --${name} in style.css`);
  return m[1].trim();
}

/** `rgb(r g b / a%)` as a colour and its alpha. */
function rgba(value: string): { rgb: Rgb; alpha: number } {
  const m = /^rgb\((\d+) (\d+) (\d+) \/ (\d+)%\)$/.exec(value);
  if (!m) throw new Error(`not rgb(r g b / a%): ${value}`);
  return { rgb: [Number(m[1]), Number(m[2]), Number(m[3])], alpha: Number(m[4]) / 100 };
}

/** What a daytime panorama puts behind the glass: deep forest shade and bright sky or snow. */
const BACKDROPS: Record<string, Rgb> = {
  forest: hex('#2a3a2a'),
  grass: hex('#4e8a2b'),
  sky: hex('#86b3ea'),
  snow: hex('#ffffff'),
};

describe('the main menu text contrast', () => {
  const glass = rgba(token('mm-glass'));
  const texts = { ink: hex(token('ink')), 'soft ink': hex(token('mm-ink-soft')) };

  for (const [backdrop, under] of Object.entries(BACKDROPS)) {
    const panel = over(glass.rgb, glass.alpha, under);
    for (const [name, color] of Object.entries(texts)) {
      it(`${name} on the glass over ${backdrop} is at least 4.5:1`, () => {
        expect(contrast(color, panel)).toBeGreaterThanOrEqual(4.5);
      });
    }
  }

  it("the big actions' white text is at least 4.5:1 on both ends of their green", () => {
    const white = hex('#ffffff');
    expect(contrast(white, hex(token('mm-action-top')))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(white, hex(token('mm-action')))).toBeGreaterThanOrEqual(4.5);
  });

  it('the logo screen prompt on its scrim is at least 4.5:1 over the brightest backdrop', () => {
    const scrim = rgba(token('mm-scrim'));
    const behind = over(scrim.rgb, scrim.alpha, BACKDROPS.snow ?? hex('#ffffff'));
    expect(contrast(hex('#ffffff'), behind)).toBeGreaterThanOrEqual(4.5);
  });

  it("the world cards' mode badges carry white text at 4.5:1", () => {
    const white = hex('#ffffff');
    expect(contrast(white, hex(token('mm-mode-creative')))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(white, hex(token('mm-mode-adventure')))).toBeGreaterThanOrEqual(4.5);
  });

  it("the small wordmark's dark edge stands out from the glass at 3:1 (large text) over snow", () => {
    const panel = over(glass.rgb, glass.alpha, BACKDROPS.snow ?? hex('#ffffff'));
    expect(contrast(hex(token('mm-wordmark-edge')), panel)).toBeGreaterThanOrEqual(3);
    // ...and from the wordmark's white top, so the letters keep their shape.
    expect(contrast(hex(token('mm-wordmark-edge')), hex('#ffffff'))).toBeGreaterThanOrEqual(4.5);
  });

  it('computes known ratios', () => {
    expect(contrast(hex('#000000'), hex('#ffffff'))).toBeCloseTo(21, 5);
    expect(contrast(hex('#777777'), hex('#ffffff'))).toBeCloseTo(4.48, 2);
  });
});
