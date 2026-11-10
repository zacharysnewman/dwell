import { describe, expect, it } from 'vitest';
import {
  REPORT_MAX_BYTES,
  formatStreamingReport,
  streamingReport,
  type StreamingReportInput,
} from './streamingReport';

const input = (game: Record<string, unknown> | null): StreamingReportInput => ({
  build: { version: '0.51.23', sha: 'abc1234', channel: 'stable' },
  game,
  device: {
    ua: 'test',
    hardwareConcurrency: 8,
    deviceMemory: null,
    gpu: 'apple m1',
    cpuAdapter: false,
    screen: { width: 1440, height: 900, dpr: 2 },
  },
  settings: {
    renderScale: 1,
    upscaler: 'bilinear',
    fullDetailM: 96,
    distantPixelError: 2,
    distantMemoryMb: 256,
    preset: 'high',
  },
  world: {
    local: true,
    seed: '18446744073709551615',
    generatorVersion: 3,
    type: null,
    mode: null,
    compatibilityLine: '0.51',
  },
  time: new Date(0),
});

describe('streaming report', () => {
  it('puts the build, device, settings and world before the game parts and round-trips', () => {
    const r = streamingReport(input({ player: { feet: [1, 2, 3] }, nearRing: { chunks: [] } }));
    expect(r.kind).toBe('dwell-streaming-report');
    expect(r.version).toBe('0.51.23');
    expect(r.time).toBe('1970-01-01T00:00:00.000Z');
    expect(r.player).toEqual({ feet: [1, 2, 3] });
    expect(JSON.parse(formatStreamingReport(r))).toEqual(r);
  });

  it('says there is no game before playing', () => {
    expect(streamingReport(input(null)).game).toBeNull();
  });

  it('stays under the size limit by dropping the chunks drawn in full', () => {
    const chunks = Array.from({ length: 4000 }, (_, i) => ({
      c: [i, 0, 0],
      d: i,
      flags: i % 2 ? ['loaded', 'drawn'] : ['loaded', 'dirty'],
      pad: 'x'.repeat(40),
    }));
    const text = formatStreamingReport(streamingReport(input({ nearRing: { chunks } })));
    const back = JSON.parse(text) as { nearRing: { chunks: unknown[]; drawnOmitted: number } };
    expect(back.nearRing.drawnOmitted).toBe(2000);
    expect(back.nearRing.chunks).toHaveLength(2000);
    expect(text.length).toBeLessThan(REPORT_MAX_BYTES + 100_000);
  });
});
