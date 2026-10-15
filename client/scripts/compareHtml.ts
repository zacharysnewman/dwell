// `bench --compare`'s HTML report (OUT/compare.html; scripts/compare.ts, docs/DEV_TOOLS.md): one
// self-contained page, images referenced relative to it. At the top, each scenario's metrics table;
// then per scenario and shot, every configuration's screenshot at full size side by side, and per
// configuration against the baseline a blink view (click it, or focus it and press Space, Enter or
// B, to swap the two in place), the ΔE heatmap and its stats. Inline styles and script only. Pure:
// no Node or browser imports.

import { scenarioTable, type ScenarioResult } from './compareReport.ts';

export interface CompareHtmlInput {
  baseline: string;
  deltaE: number;
  /** The configurations run, the baseline first. */
  configs: readonly { name: string; notes: string }[];
  skipped: readonly { name: string; reason: string }[];
  scenarios: readonly ScenarioResult[];
  /** How the run was made (GPU or SwiftShader, vsync or unthrottled), shown under the title. */
  mode: string;
  /** When it was made (ISO 8601). */
  when: string;
}

/** Text safe in HTML element content and quoted attributes. */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** A relative image path, each segment URL-encoded (forward slashes, whatever the platform). */
export function imagePath(...segments: string[]): string {
  return segments
    .flatMap((s) => s.split(/[\\/]/))
    .map(encodeURIComponent)
    .join('/');
}

const STYLE = `
:root { color-scheme: light dark; --bg: #f6f6f4; --fg: #1d1d1b; --muted: #6b6b66; --line: #d9d9d4;
  --card: #fff; --accent: #2a62c9; }
@media (prefers-color-scheme: dark) { :root { --bg: #161616; --fg: #e8e8e4; --muted: #9a9a94;
  --line: #333; --card: #1f1f1f; --accent: #7ea6ff; } }
* { box-sizing: border-box; }
body { margin: 0; padding: 24px 16px 64px; background: var(--bg); color: var(--fg);
  font: 14px/1.45 system-ui, -apple-system, 'Segoe UI', sans-serif; }
h1 { font-size: 22px; margin: 0 0 4px; }
h2 { font-size: 18px; margin: 40px 0 4px; border-top: 1px solid var(--line); padding-top: 24px; }
h3 { font-size: 15px; margin: 24px 0 8px; }
p, .meta { color: var(--muted); margin: 4px 0; }
code { font: 12px ui-monospace, Menlo, monospace; }
.tables { display: flex; flex-wrap: wrap; gap: 16px; margin-top: 16px; }
table { border-collapse: collapse; background: var(--card); border: 1px solid var(--line);
  font: 12px ui-monospace, Menlo, monospace; }
th, td { padding: 3px 10px; text-align: right; white-space: nowrap; }
th:first-child, td:first-child { text-align: left; }
thead th { border-bottom: 1px solid var(--line); }
tr.group td { color: var(--muted); padding-top: 8px; }
.strip { display: flex; gap: 12px; overflow-x: auto; padding-bottom: 8px; }
figure { margin: 0; flex: none; }
figure img { display: block; max-width: none; border: 1px solid var(--line); }
figcaption { color: var(--muted); font-size: 12px; margin-top: 4px; }
.blink { cursor: pointer; }
.blink:focus { outline: 2px solid var(--accent); outline-offset: 2px; }
.blink figcaption b { color: var(--accent); }
.stats { font: 12px ui-monospace, Menlo, monospace; }
.error { color: #c33; }
ul { margin: 4px 0; padding-left: 20px; }
`;

// Swaps a blink figure's image between its two sources: on click, or Space/Enter/B when focused.
const SCRIPT = `
for (const el of document.querySelectorAll('.blink')) {
  const img = el.querySelector('img');
  const label = el.querySelector('figcaption b');
  const swap = () => {
    const b = el.dataset.shown !== 'b';
    el.dataset.shown = b ? 'b' : 'a';
    img.src = b ? el.dataset.b : el.dataset.a;
    label.textContent = b ? el.dataset.bName : el.dataset.aName;
  };
  el.addEventListener('click', swap);
  el.addEventListener('keydown', (e) => {
    if (e.key === ' ' || e.key === 'Enter' || e.key === 'b' || e.key === 'B') {
      e.preventDefault();
      swap();
    }
  });
}
`;

function table(r: ScenarioResult, input: CompareHtmlInput): string {
  const { header, rows } = scenarioTable(r, input.configs, input.deltaE);
  const head = `<thead><tr>${header.map((h) => `<th>${escapeHtml(h)}</th>`).join('')}</tr></thead>`;
  const body = rows
    .map((row) => {
      const group = row[0].startsWith('[') ? ' class="group"' : '';
      return `<tr${group}>${row.map((c) => `<td>${escapeHtml(c)}</td>`).join('')}</tr>`;
    })
    .join('\n');
  return `<table>${head}<tbody>\n${body}\n</tbody></table>`;
}

function shotSection(r: ScenarioResult, shot: string, input: CompareHtmlInput): string {
  const raw = (config: string): string => imagePath(r.name, config, `${shot}.png`);
  const e = escapeHtml;
  const strip = input.configs
    .map(
      (c) =>
        `<figure><img src="${e(raw(c.name))}" alt="${e(`${shot}, ${c.name}`)}" loading="lazy">` +
        `<figcaption>${e(c.name)}${c.name === input.baseline ? ' (baseline)' : ''}</figcaption></figure>`,
    )
    .join('\n');
  const pairs = input.configs
    .filter((c) => c.name !== input.baseline)
    .map((c) => {
      const d = r.shots.find((s) => s.shot === shot && s.config === c.name);
      const stats = d?.stats
        ? `${d.stats.overPct.toFixed(2)} % of pixels over ΔE ${String(input.deltaE)} · mean ΔE ` +
          `${d.stats.meanDeltaE.toFixed(2)} · p99 ${d.stats.p99DeltaE.toFixed(1)} · max ` +
          `${d.stats.maxDeltaE.toFixed(1)} · ${d.stats.identicalPct.toFixed(1)} % identical`
        : null;
      const heat = d?.diff
        ? `<figure><img src="${e(imagePath(d.diff))}" alt="${e(`${shot}: ΔE heatmap, ${c.name}`)}" loading="lazy">` +
          `<figcaption>ΔE heatmap — grey equal, blue under ${e(String(input.deltaE))}, red → white over</figcaption></figure>`
        : '';
      return (
        `<h3>${e(shot)}: ${e(input.baseline)} ⇄ ${e(c.name)}</h3>\n` +
        (stats
          ? `<p class="stats">${e(stats)}</p>\n`
          : `<p class="stats error">${e(d?.error ?? 'no difference image')}</p>\n`) +
        `<div class="strip">` +
        `<figure class="blink" tabindex="0" role="button" data-shown="a"` +
        ` data-a="${e(raw(input.baseline))}" data-b="${e(raw(c.name))}"` +
        ` data-a-name="${e(input.baseline)}" data-b-name="${e(c.name)}"` +
        ` aria-label="${e(`Blink ${input.baseline} and ${c.name}`)}">` +
        `<img src="${e(raw(input.baseline))}" alt="${e(`${shot}, blink`)}" loading="lazy">` +
        `<figcaption>Blink: showing <b>${e(input.baseline)}</b> — click, or Space / B</figcaption></figure>\n` +
        `${heat}</div>`
      );
    })
    .join('\n');
  return `<h3>${e(shot)}: every configuration</h3>\n<div class="strip">\n${strip}\n</div>\n${pairs}`;
}

/** The report page. */
export function compareHtml(input: CompareHtmlInput): string {
  const e = escapeHtml;
  const configs = input.configs
    .map((c) => `<li><code>${e(c.name)}</code>${c.notes ? ` — ${e(c.notes)}` : ''}</li>`)
    .join('');
  const skipped = input.skipped.length
    ? `<p>Skipped (not in this build):</p><ul>${input.skipped
        .map((s) => `<li><code>${e(s.name)}</code>: ${e(s.reason)}</li>`)
        .join('')}</ul>`
    : '';
  const tables = input.scenarios.map((r) => table(r, input)).join('\n');
  const sections = input.scenarios
    .map((r) => {
      const errors = Object.entries(r.errors).flatMap(([c, list]) =>
        list.map((m) => `<li><code>${e(c)}</code>: ${e(m)}</li>`),
      );
      const shots = r.shotNames.length
        ? r.shotNames.map((s) => shotSection(r, s, input)).join('\n')
        : '<p>No <code>shot</code> steps in this scenario.</p>';
      return (
        `<section id="${e(r.name)}"><h2>${e(r.name)}${r.split ? ` <small>(${e(r.split)})</small>` : ''}</h2>\n` +
        `<p>${e(r.notes)}</p>\n` +
        (errors.length ? `<ul class="error">${errors.join('')}</ul>\n` : '') +
        `${shots}</section>`
      );
    })
    .join('\n');
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Bench comparison</title>
<style>${STYLE}</style>
</head>
<body>
<h1>Bench comparison</h1>
<p class="meta">${e(input.when)} · ${e(input.mode)} · baseline <code>${e(input.baseline)}</code> · ΔE threshold ${e(String(input.deltaE))} (CIE76)</p>
<ul>${configs}</ul>
${skipped}
<div class="tables">
${tables}
</div>
${sections}
<script>${SCRIPT}</script>
</body>
</html>
`;
}
