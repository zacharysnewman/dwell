import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// iOS Safari turned Reader on by itself for the version page (owner, 2026-10-21): its heuristics
// pick pages with <main>/<article> landmarks and runs of <p> prose. The launcher is an app screen,
// so its markup must carry none of them: <div>s and the <ul> of versions only.
const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
const script = readFileSync(new URL('./main.ts', import.meta.url), 'utf8');

describe('the launcher is not Reader-eligible', () => {
  it('has no article landmarks or prose paragraphs in its page', () => {
    expect(html).not.toMatch(/<(main|article|p)[\s>]/);
    expect(html).not.toMatch(/itemprop="articleBody"|og:type/);
  });

  it('builds the version list without article or paragraph elements', () => {
    expect(script).not.toMatch(/createElement\('(main|article|section|p)'\)/);
  });
});
