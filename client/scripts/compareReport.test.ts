import { describe, expect, it } from 'vitest';
import {
  deltaE,
  diffImages,
  droppedFrames,
  flickerOf,
  formatTable,
  metricRows,
  metricsOf,
  percentile,
  sideBySide,
  type RecordData,
  type Rgba,
} from './compareReport.ts';

const stats = (mean: number, p95: number, sum = 0) => ({
  n: 1,
  mean,
  p50: mean,
  p95,
  p99: p95,
  max: p95,
  sum,
});

function image(w: number, h: number, rgb: [number, number, number]): Rgba {
  const data = new Uint8Array(w * h * 4);
  for (let i = 0; i < w * h; i++) data.set([...rgb, 255], i * 4);
  return { width: w, height: h, data };
}

describe('compare report math', () => {
  it('percentiles as the telemetry takes them', () => {
    const v = Array.from({ length: 100 }, (_, i) => 100 - i);
    expect([0.5, 0.95, 0.99].map((q) => percentile(v, q))).toEqual([51, 95, 99]);
    expect(percentile([], 0.5)).toBe(0);
  });

  it('counts dropped frames at 60 Hz', () => {
    expect(droppedFrames([16.7, 16.6, 33.3, 50, 8])).toBe(3);
    expect(droppedFrames([16.7, 20])).toBe(0);
  });

  it('reads a record: intervals from the frames, CPU and GPU series, uploads, streaming', () => {
    const r: RecordData = {
      telemetry: {
        frames: {
          interval: stats(16.7, 17),
          'frame.cpu': stats(2, 3),
          'gpu.terrain': stats(1, 1.5),
          'upload.bytes': stats(0, 0, 2 ** 21),
          'upload.chunks': stats(0, 0, 4),
        },
        samples: { 'gpu.composite': stats(0.5, 0.6), 'job.chunk.run': stats(3, 4) },
        frameCount: 4,
      },
      frames: { names: [], frames: [[0], [16.7], [50], [66.7]] },
      memory: {
        coreBytes: 10,
        serverBytes: 5,
        worldgenBytes: [1, 2],
        meshBytes: 3,
        screenBytes: 4,
        jsHeapBytes: null,
      },
      state: { lod: { generating: 0, meshing: 2, asking: 0 }, terrain: { generating: 0 } },
      seconds: 1,
    };
    const m = metricsOf(r);
    expect(m.interval.p50).toBeCloseTo(16.7);
    expect(m.dropped60).toBe(1);
    expect(Object.keys(m.cpu)).toEqual(['frame.cpu']);
    expect(Object.keys(m.gpu ?? {}).sort()).toEqual(['gpu.composite', 'gpu.terrain']);
    expect(m.memory?.totalBytes).toBe(25);
    expect(m.upload).toEqual({ bytes: 2 ** 21, bytesPerFrame: 2 ** 19, chunks: 4, sections: 0 });
    expect(m.streaming).toEqual({ complete: false, outstanding: { 'lod.meshing': 2 } });

    const rows = metricRows([m, { ...m, gpu: null }, null]);
    expect(rows.find((x) => x[0] === 'gpu.terrain p95 ms')).toEqual([
      'gpu.terrain p95 ms',
      '1.50',
      '—',
      '—',
    ]);
    expect(rows.find((x) => x[0] === 'upload MiB')?.[1]).toBe('2.0');
  });

  it('without gpu.* telemetry there are no GPU metrics', () => {
    const m = metricsOf({
      telemetry: { frames: {}, samples: {}, frameCount: 0 },
      frames: { names: [], frames: [] },
      memory: null,
      state: null,
      seconds: 0,
    });
    expect(m.gpu).toBeNull();
    expect(m.streaming.complete).toBe(true);
    expect(metricRows([m]).some((r) => r[0].startsWith('gpu.'))).toBe(false);
  });

  it('ΔE is zero for equal colours and about 100 from black to white', () => {
    expect(deltaE([10, 20, 30], [10, 20, 30])).toBe(0);
    expect(deltaE([0, 0, 0], [255, 255, 255])).toBeCloseTo(100, 1);
    expect(deltaE([128, 128, 128], [129, 128, 128])).toBeLessThan(2.3);
  });

  it('diff stats: the share of pixels over the threshold, mean and max', () => {
    const a = image(4, 1, [100, 100, 100]);
    const b = image(4, 1, [100, 100, 100]);
    b.data.set([0, 0, 0], 0); // one pixel very different
    b.data.set([101, 100, 100], 4); // one barely
    const { stats: s, heatmap } = diffImages(a, b, 2.3);
    expect(s.pixels).toBe(4);
    expect(s.identicalPct).toBe(50);
    expect(s.overPct).toBe(25);
    expect(s.maxDeltaE).toBeGreaterThan(40);
    expect(s.meanDeltaE).toBeCloseTo((s.maxDeltaE + deltaE([100, 100, 100], [101, 100, 100])) / 4);
    expect([...heatmap.data.subarray(0, 3)]).toEqual([255, 255, 255]); // far over: white
    expect(heatmap.data[6]).toBeGreaterThan(80); // under: blue
    expect(() => diffImages(a, image(2, 2, [0, 0, 0]), 2.3)).toThrow(/differ in size/);
  });

  it('flicker: the share of pixels that change from frame to frame, and their mean change', () => {
    const still = [image(2, 2, [50, 50, 50]), image(2, 2, [50, 50, 50]), image(2, 2, [50, 50, 50])];
    expect(flickerOf(still)).toEqual({ frames: 3, changed: 0, meanDelta: 0, threshold: 8 });
    // One pixel of four blinks between 50 and 150 every frame: a quarter of the view flickers.
    const blink = still.map((im, i) => {
      const c = { ...im, data: im.data.slice() };
      if (i % 2) c.data.set([150, 150, 150], 0);
      return c;
    });
    const f = flickerOf(blink);
    expect(f.changed).toBeCloseTo(0.25);
    expect(f.meanDelta).toBeCloseTo(25, 3);
    // A change under the threshold isn't flicker.
    const faint = [image(1, 1, [50, 50, 50]), image(1, 1, [55, 55, 55])];
    expect(flickerOf(faint).changed).toBe(0);
    expect(() => flickerOf([still[0]])).toThrow(/two frames/);
    expect(() => flickerOf([still[0], image(1, 1, [0, 0, 0])])).toThrow(/sizes differ/);
  });

  it('identical images agree everywhere', () => {
    const a = image(3, 3, [40, 90, 200]);
    const { stats: s } = diffImages(a, image(3, 3, [40, 90, 200]), 2.3);
    expect([s.identicalPct, s.overPct, s.maxDeltaE, s.meanDeltaE]).toEqual([100, 0, 0, 0]);
  });

  it('places images side by side with a gap', () => {
    const s = sideBySide([image(2, 1, [255, 0, 0]), image(1, 2, [0, 0, 255])], 1);
    expect([s.width, s.height]).toEqual([4, 2]);
    const px = (x: number, y: number): number[] => [
      ...s.data.subarray((y * 4 + x) * 4, (y * 4 + x) * 4 + 3),
    ];
    expect(px(1, 0)).toEqual([255, 0, 0]);
    expect(px(2, 0)).toEqual([128, 128, 128]);
    expect(px(3, 1)).toEqual([0, 0, 255]);
  });

  it('formats a table', () => {
    expect(formatTable(['x', 'a', 'bb'], [['row', '1', '22']])).toBe('x    a  bb\nrow  1  22');
  });
});
