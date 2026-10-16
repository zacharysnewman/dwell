import { describe, expect, it } from 'vitest';
import { compareHtml, escapeHtml, imagePath, type CompareHtmlInput } from './compareHtml.ts';
import type { Metrics, ScenarioResult } from './compareReport.ts';

const metrics: Metrics = {
  frames: 300,
  seconds: 5,
  interval: { p50: 16.7, p95: 18, p99: 19, max: 20, mean: 16.7 },
  dropped60: 2,
  cpu: { 'frame.cpu': { mean: 2, p95: 3, p99: 4 } },
  gpu: null,
  memory: null,
  upload: { bytes: 0, bytesPerFrame: 0, chunks: 0, sections: 0 },
  streaming: { complete: true, outstanding: {} },
};

const scenario: ScenarioResult = {
  name: 'look-horizon',
  notes: 'Grazing <rays> & "quotes"',
  split: 'far',
  shotNames: ['horizon', 'later'],
  settles: { raster: [27.7], 'raster-half': [27.6] },
  errors: { raster: [], 'raster-half': ['<boom>'] },
  records: { steady: { raster: metrics, 'raster-half': metrics } },
  shots: [
    {
      shot: 'horizon',
      config: 'raster-half',
      stats: {
        pixels: 4,
        identicalPct: 50,
        overPct: 11.96,
        threshold: 2.3,
        meanDeltaE: 1.27,
        p99DeltaE: 20,
        maxDeltaE: 60.9,
      },
      side: 'look-horizon/horizon.raster-half.side.png',
      diff: 'look-horizon/horizon.raster-half.diff.png',
    },
    { shot: 'later', config: 'raster-half', stats: null, error: 'images differ in size' },
  ],
};

const input: CompareHtmlInput = {
  baseline: 'raster',
  deltaE: 2.3,
  configs: [
    { name: 'raster', notes: 'The mesher.' },
    { name: 'raster-half', notes: 'Half <res>.' },
  ],
  skipped: [{ name: 'march', reason: 'this build has no "march" renderer' }],
  scenarios: [scenario],
  mode: 'GPU, unthrottled',
  when: '2026-10-14T00:00:00.000Z',
};

const count = (s: string, needle: string): number => s.split(needle).length - 1;

describe('compare.html', () => {
  const html = compareHtml(input);

  it('is a self-contained page: no external scripts, styles or absolute image paths', () => {
    expect(html.startsWith('<!doctype html>')).toBe(true);
    expect(html).not.toMatch(/<script[^>]+src=/);
    expect(html).not.toMatch(/<link /);
    for (const [, src] of html.matchAll(/src="([^"]*)"/g)) {
      expect(src).not.toMatch(/^(\/|[a-z]+:)/);
    }
  });

  it('puts the metrics table first, then each shot with every configuration and a blink pair', () => {
    expect(html.indexOf('<table>')).toBeLessThan(html.indexOf('<section'));
    expect(count(html, '<table>')).toBe(1);
    expect(html).toContain('<td>interval p95 ms</td><td>18.00</td><td>18.00</td>');
    expect(html).toContain('src="look-horizon/raster/horizon.png"');
    expect(html).toContain('src="look-horizon/raster-half/horizon.png"');
    expect(html).toContain('src="look-horizon/horizon.raster-half.diff.png"');
    // One blink per shot and non-baseline configuration, swapping the two screenshots.
    expect(count(html, 'class="blink"')).toBe(2);
    expect(html).toContain(
      'data-a="look-horizon/raster/horizon.png" data-b="look-horizon/raster-half/horizon.png"',
    );
    expect(html).toContain('11.96 % of pixels over ΔE 2.3 · mean ΔE 1.27');
    expect(html).toContain('max 60.9');
    // A shot whose diff failed says why.
    expect(html).toContain('<p class="stats error">images differ in size</p>');
    expect(html).toContain('this build has no &quot;march&quot; renderer');
  });

  it('escapes text from scenarios, configurations and errors', () => {
    expect(html).toContain('Grazing &lt;rays&gt; &amp; &quot;quotes&quot;');
    expect(html).toContain('Half &lt;res&gt;.');
    expect(html).toContain('&lt;boom&gt;');
    expect(html).not.toContain('<rays>');
    expect(html).not.toContain('<boom>');
    expect(escapeHtml(`<a href="x">'&'</a>`)).toBe(
      '&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;',
    );
  });

  it('makes image paths relative and URL-safe', () => {
    expect(imagePath('a b', 'c', 'd#1.png')).toBe('a%20b/c/d%231.png');
    expect(imagePath('x\\y.png')).toBe('x/y.png');
  });

  it('says so when a scenario has no shots', () => {
    const page = compareHtml({ ...input, scenarios: [{ ...scenario, shotNames: [], shots: [] }] });
    expect(page).toContain('No <code>shot</code> steps');
    expect(count(page, 'class="blink"')).toBe(0);
  });
});
