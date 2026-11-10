// WCAG contrast ratios, for checking the menu's text colours against its glass panels
// (contrast.test.ts): a panel's colour is composited over the backdrop behind it first.

export type Rgb = readonly [number, number, number];

/** `#rrggbb` as 0–255 channels. */
export function hex(color: string): Rgb {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color);
  if (!m) throw new Error(`not a #rrggbb colour: ${color}`);
  return [parseInt(m[1] ?? '0', 16), parseInt(m[2] ?? '0', 16), parseInt(m[3] ?? '0', 16)];
}

/** `top` at `alpha` over `under`. */
export function over(top: Rgb, alpha: number, under: Rgb): Rgb {
  const mix = (i: 0 | 1 | 2): number => top[i] * alpha + under[i] * (1 - alpha);
  return [mix(0), mix(1), mix(2)];
}

function channel(v: number): number {
  const s = v / 255;
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function luminance(c: Rgb): number {
  return 0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2]);
}

/** The contrast ratio of two colours, 1–21. */
export function contrast(a: Rgb, b: Rgb): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}
