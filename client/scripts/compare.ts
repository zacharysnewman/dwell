// `npm run bench -- --compare <scenario…>` (docs/DEV_TOOLS.md):
// each scenario runs once per configuration (client/bench/configs.json, scripts/configs.ts)
// in a fresh page on the same seed and inputs. Writes OUT/compare.json and prints a table side by
// side; for every `shot` step, OUT/<scenario>/<shot>.<config>.side.png (the baseline left, the
// configuration right) and <shot>.<config>.diff.png (a ΔE heatmap). Each run's own output (its
// report.json and screenshots) is in OUT/<scenario>/<config>/. The math is in
// scripts/compareReport.ts.

import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Browser, Page } from '@playwright/test';
import { compareHtml } from './compareHtml.ts';
import {
  diffImages,
  formatTable,
  metricsOf,
  scenarioTable,
  sideBySide,
  type Metrics,
  type RecordData,
  type Rgba,
  type ScenarioResult,
  type ShotDiff,
} from './compareReport.ts';
import { configParams, parseConfigs, selectConfigs, type Config } from './configs.ts';
import type { Scenario, Step } from './scenarios.ts';
import type { Flicker } from './compareReport.ts';

/** What bench.ts's run of one scenario saves (its `report.json`). */
export interface ScenarioReport {
  scenario: Scenario;
  gpu: unknown;
  errors: string[];
  records: Record<string, unknown>;
  /** Each `settle` step's seconds, in order. */
  settles: number[];
  /** Each `burst` step's flicker, by name. */
  bursts?: Record<string, Flicker>;
  /** The page address it ran at. */
  url: string;
}

export interface CompareOptions {
  browser: Browser;
  out: string;
  /** How the run is made (GPU or SwiftShader, vsync or unthrottled), for the HTML report. */
  mode: string;
  scenarios: Scenario[];
  configsFile: string;
  /** `--configs`: these (and the baseline) only; empty: all. */
  configNames: string[];
  /** Runs a scenario into `dir` with the configuration's page parameters. */
  run: (sc: Scenario, dir: string, params: Record<string, string>) => Promise<ScenarioReport>;
}

/** The `shot` names of `steps`, in order. */
function shots(steps: readonly Step[]): string[] {
  return steps.flatMap((s) =>
    s.do === 'shot' ? [s.name] : s.do === 'repeat' ? shots(s.steps) : [],
  );
}

// Decoding and encoding PNGs in a blank page (the browser's codecs; no dependency).
export const CODEC = `
  globalThis.__decodePng = async (b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }),
      { colorSpaceConversion: 'none', premultiplyAlpha: 'none' });
    const cv = new OffscreenCanvas(bmp.width, bmp.height);
    const cx = cv.getContext('2d');
    cx.drawImage(bmp, 0, 0);
    const d = cx.getImageData(0, 0, bmp.width, bmp.height).data;
    let s = '';
    for (let i = 0; i < d.length; i += 0x8000) s += String.fromCharCode(...d.subarray(i, i + 0x8000));
    return { width: bmp.width, height: bmp.height, b64: btoa(s) };
  };
  globalThis.__encodePng = async (w, h, b64) => {
    const bytes = Uint8ClampedArray.from(atob(b64), (c) => c.charCodeAt(0));
    const cv = new OffscreenCanvas(w, h);
    cv.getContext('2d').putImageData(new ImageData(bytes, w, h), 0, 0);
    const buf = new Uint8Array(await (await cv.convertToBlob({ type: 'image/png' })).arrayBuffer());
    let s = '';
    for (let i = 0; i < buf.length; i += 0x8000) s += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    return btoa(s);
  };
`;
interface Codec {
  __decodePng(b64: string): Promise<{ width: number; height: number; b64: string }>;
  __encodePng(w: number, h: number, b64: string): Promise<string>;
}

export async function decode(page: Page, path: string): Promise<Rgba> {
  const b64 = readFileSync(path).toString('base64');
  const r = await page.evaluate((x) => (globalThis as unknown as Codec).__decodePng(x), b64);
  return { width: r.width, height: r.height, data: new Uint8Array(Buffer.from(r.b64, 'base64')) };
}

async function encode(page: Page, im: Rgba, path: string): Promise<void> {
  const b64 = await page.evaluate(
    ([w, h, d]) => (globalThis as unknown as Codec).__encodePng(w, h, d),
    [im.width, im.height, Buffer.from(im.data).toString('base64')] as const,
  );
  writeFileSync(path, Buffer.from(b64, 'base64'));
}

export async function compare(o: CompareOptions): Promise<void> {
  const file = parseConfigs(JSON.parse(readFileSync(o.configsFile, 'utf8')) as unknown);
  const configs = selectConfigs(file, o.configNames);
  const baseline = configs[0];
  const ran: Config[] = configs;
  const results: ScenarioResult[] = [];
  const codec = await o.browser.newPage();
  await codec.evaluate(CODEC);
  try {
    for (const sc of o.scenarios) {
      console.log(`${sc.name}: ${sc.notes}`);
      const reports: Record<string, ScenarioReport> = {};
      for (const c of ran) {
        console.log(`  — ${c.name}`);
        reports[c.name] = await o.run(sc, join(o.out, sc.name, c.name), configParams(c));
      }
      const result: ScenarioResult = {
        name: sc.name,
        notes: sc.notes,
        split: sc.split,
        shotNames: shots(sc.steps),
        settles: {},
        errors: {},
        records: {},
        shots: [],
      };
      for (const c of ran) {
        const r = reports[c.name];
        result.settles[c.name] = r.settles;
        result.errors[c.name] = r.errors;
        for (const [k, rec] of Object.entries(r.records)) {
          (result.records[k] ??= {})[c.name] = metricsOf(rec as RecordData);
        }
      }
      for (const shot of shots(sc.steps)) {
        const base = await decode(codec, join(o.out, sc.name, baseline.name, `${shot}.png`));
        for (const c of ran.slice(1)) {
          const entry: ShotDiff = { shot, config: c.name, stats: null };
          try {
            const other = await decode(codec, join(o.out, sc.name, c.name, `${shot}.png`));
            const { stats, heatmap } = diffImages(base, other, file.deltaE);
            entry.stats = stats;
            entry.side = join(sc.name, `${shot}.${c.name}.side.png`);
            entry.diff = join(sc.name, `${shot}.${c.name}.diff.png`);
            await encode(codec, sideBySide([base, other]), join(o.out, entry.side));
            await encode(codec, heatmap, join(o.out, entry.diff));
          } catch (e) {
            entry.error = e instanceof Error ? e.message : String(e);
          }
          result.shots.push(entry);
        }
      }
      results.push(result);
    }
  } finally {
    await codec.close();
  }

  const report = {
    baseline: baseline.name,
    deltaE: file.deltaE,
    configs: ran,
    scenarios: results,
    splits: splits(results, ran),
  };
  writeFileSync(join(o.out, 'compare.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`\n${table(results, ran, file.deltaE)}`);
  if (
    !results.some((r) => Object.values(r.records).some((m) => Object.values(m).some((x) => x.gpu)))
  ) {
    console.log('no gpu.* telemetry in this build (GPU timestamp queries): GPU rows omitted');
  }
  writeFileSync(
    join(o.out, 'compare.html'),
    compareHtml({
      baseline: baseline.name,
      deltaE: file.deltaE,
      configs: ran,
      scenarios: results,
      mode: o.mode,
      when: new Date().toISOString(),
    }),
  );
  console.log(`\n${join(o.out, 'compare.json')}`);
  console.log(`report: ${join(o.out, 'compare.html')}`);
}

/** Per near/far split and configuration: the worst frame intervals and the dropped frames. */
function splits(
  results: readonly ScenarioResult[],
  ran: readonly Config[],
): Record<
  string,
  Record<string, { records: number; p95Max: number; p99Max: number; dropped60: number }>
> {
  const out: Record<
    string,
    Record<string, { records: number; p95Max: number; p99Max: number; dropped60: number }>
  > = {};
  for (const r of results) {
    if (!r.split) continue;
    const per = (out[r.split] ??= {});
    for (const c of ran) {
      const acc = (per[c.name] ??= { records: 0, p95Max: 0, p99Max: 0, dropped60: 0 });
      for (const m of Object.values(r.records)) {
        const x = m[c.name] as Metrics | undefined;
        if (!x) continue;
        acc.records += 1;
        acc.p95Max = Math.max(acc.p95Max, x.interval.p95);
        acc.p99Max = Math.max(acc.p99Max, x.interval.p99);
        acc.dropped60 += x.dropped60;
      }
    }
  }
  return out;
}

function table(results: readonly ScenarioResult[], ran: readonly Config[], deltaE: number): string {
  return results
    .map((r) => {
      const { header, rows } = scenarioTable(r, ran, deltaE);
      return formatTable(header, rows);
    })
    .join('\n\n');
}
