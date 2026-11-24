import { describe, expect, it } from 'vitest';
import { formatPerf, TickRate } from './perfLine';

describe('performance line', () => {
  it('names each figure, so the one over the frame shows', () => {
    expect(
      formatPerf({ fps: 29.6, cpuMs: 4.12, gpuMs: 31.24, serverTicks: 60, serverTickMs: 2.84 }),
    ).toBe('30 fps · CPU 4.1 ms · GPU 31.2 ms · server 60/s, 2.8 ms');
  });

  it('shows a dash for what is not known, and no tick time for a remote server', () => {
    expect(
      formatPerf({ fps: null, cpuMs: 3, gpuMs: Number.NaN, serverTicks: 38, serverTickMs: null }),
    ).toBe('– fps · CPU 3.0 ms · GPU – · server 38/s');
  });

  it('shows drawn of the animation frames and the skips by reason', () => {
    expect(
      formatPerf({
        fps: 38,
        animationFps: 60,
        pacedShare: 22 / 60,
        noSurfaceShare: 0,
        cpuMs: 4,
        gpuMs: 6,
        serverTicks: 60,
        serverTickMs: null,
      }),
    ).toBe('38/60 fps (22 GPU-wait) · CPU 4.0 ms · GPU 6.0 ms · server 60/s');
    expect(
      formatPerf({
        fps: 60,
        animationFps: 60,
        cpuMs: 4,
        gpuMs: 6,
        serverTicks: 60,
        serverTickMs: null,
      }),
    ).toBe('60/60 fps · CPU 4.0 ms · GPU 6.0 ms · server 60/s');
  });

  it("measures a server's ticks a second from the tick it sends", () => {
    const r = new TickRate();
    expect(r.rate).toBeNull();
    // A server falling behind: 45 ticks a second over two seconds, sampled four times a second.
    for (let t = 0; t <= 2000; t += 250) r.sample(t, Math.round((t / 1000) * 45));
    expect(r.rate).toBeCloseTo(45, 0);
    // A new session's ticks start lower: measured again from there.
    r.sample(2250, 3);
    r.sample(3250, 63);
    expect(r.rate).toBeCloseTo(60, 0);
  });
});

describe('the memory on the performance line', () => {
  // Owner, 2026-11-24: "I'd like it to show memory usage" (a climb while playing shows a leak).
  it('ends the line, with the JavaScript heap where the browser tells it', () => {
    const base = { fps: 60, cpuMs: 4, gpuMs: 8, serverTicks: 60, serverTickMs: null };
    expect(formatPerf({ ...base, memoryBytes: 3158 * 1048576, jsHeapBytes: 140 * 1048576 })).toBe(
      '60 fps · CPU 4.0 ms · GPU 8.0 ms · server 60/s · mem 3,158 MB (JS 140 MB)',
    );
    expect(formatPerf({ ...base, memoryBytes: 512 * 1048576, jsHeapBytes: null })).toBe(
      '60 fps · CPU 4.0 ms · GPU 8.0 ms · server 60/s · mem 512 MB',
    );
    expect(formatPerf(base)).toBe('60 fps · CPU 4.0 ms · GPU 8.0 ms · server 60/s');
  });
});
